//! Sign-in: GitHub OAuth device flow, token refresh, Keychain storage and the `gh` CLI
//! development fallback. Tokens never touch disk or the log.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::github::{http_client, ApiError};

pub const OAUTH_BASE: &str = "https://github.com";
pub const SCOPES: &str = "notifications repo read:org";
pub const KEYCHAIN_SERVICE: &str = "Pullbell";
pub const KEYCHAIN_ACCOUNT: &str = "github";
/// Refresh this long before expiry.
const REFRESH_MARGIN_MINUTES: i64 = 10;

/// The OAuth App client id baked in at build time. `None` → use the `gh` CLI token instead.
pub fn client_id() -> Option<&'static str> {
    option_env!("PULLBELL_CLIENT_ID").filter(|s| !s.trim().is_empty())
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub refresh_expires_at: Option<DateTime<Utc>>,
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &"<redacted>")
            .field("has_refresh", &self.refresh_token.is_some())
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl TokenSet {
    pub fn needs_refresh(&self, now: DateTime<Utc>) -> bool {
        self.refresh_token.is_some()
            && self
                .expires_at
                .is_some_and(|t| t - now <= chrono::Duration::minutes(REFRESH_MARGIN_MINUTES))
    }

    fn from_response(v: &Value, now: DateTime<Utc>) -> Option<TokenSet> {
        let secs = |k: &str| {
            v.get(k)
                .and_then(Value::as_i64)
                .map(|s| now + chrono::Duration::seconds(s))
        };
        Some(TokenSet {
            access_token: v.get("access_token")?.as_str()?.to_string(),
            refresh_token: v
                .get("refresh_token")
                .and_then(Value::as_str)
                .map(str::to_string),
            expires_at: secs("expires_in"),
            refresh_expires_at: secs("refresh_token_expires_in"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, PartialEq)]
pub enum PollOutcome {
    Pending,
    /// GitHub asked us to slow down; use this new interval (seconds).
    SlowDown(u64),
    Done(TokenSet),
    Expired,
    Denied,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AuthError {
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("GitHub said: {0}")]
    OAuth(String),
    #[error("Keychain access was denied")]
    KeychainDenied,
    #[error("Keychain error: {0}")]
    Keychain(String),
    #[error("no GitHub CLI token: {0}")]
    NoGhToken(String),
}

async fn post_form(base: &str, path: &str, form: &[(&str, &str)]) -> Result<Value, AuthError> {
    let resp = http_client()
        .post(format!("{}{path}", base.trim_end_matches('/')))
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await
        .map_err(|e| ApiError::Network(e.without_url().to_string()))?;
    let resp = crate::github::check(resp, Utc::now()).await?;
    resp.json()
        .await
        .map_err(|e| AuthError::Api(ApiError::Decode(e.without_url().to_string())))
}

pub async fn device_start(base: &str, client_id: &str) -> Result<DeviceCode, AuthError> {
    let v = post_form(
        base,
        "/login/device/code",
        &[("client_id", client_id), ("scope", SCOPES)],
    )
    .await?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        return Err(AuthError::OAuth(err.to_string()));
    }
    serde_json::from_value(v).map_err(|e| AuthError::OAuth(e.to_string()))
}

pub async fn device_poll(
    base: &str,
    client_id: &str,
    device_code: &str,
    interval: u64,
) -> Result<PollOutcome, AuthError> {
    let v = post_form(
        base,
        "/login/oauth/access_token",
        &[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ],
    )
    .await?;
    Ok(match v.get("error").and_then(Value::as_str) {
        None => PollOutcome::Done(
            TokenSet::from_response(&v, Utc::now())
                .ok_or_else(|| AuthError::OAuth("no access_token".into()))?,
        ),
        Some("authorization_pending") => PollOutcome::Pending,
        Some("slow_down") => PollOutcome::SlowDown(
            v.get("interval")
                .and_then(Value::as_u64)
                .unwrap_or(interval + 5),
        ),
        Some("expired_token") => PollOutcome::Expired,
        Some("access_denied") => PollOutcome::Denied,
        Some(other) => return Err(AuthError::OAuth(other.to_string())),
    })
}

/// Poll until the user finished (or abandoned) the browser step.
pub async fn device_wait(
    base: &str,
    client_id: &str,
    code: &DeviceCode,
    cancelled: impl Fn() -> bool,
) -> Result<PollOutcome, AuthError> {
    let mut interval = code.interval.max(1);
    let deadline = std::time::Instant::now() + Duration::from_secs(code.expires_in);
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if cancelled() {
            return Ok(PollOutcome::Denied);
        }
        if std::time::Instant::now() > deadline {
            return Ok(PollOutcome::Expired);
        }
        match device_poll(base, client_id, &code.device_code, interval).await? {
            PollOutcome::Pending => {}
            PollOutcome::SlowDown(i) => interval = i,
            done => return Ok(done),
        }
    }
}

/// Exchange a refresh token for a new token set.
pub async fn refresh(
    base: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenSet, AuthError> {
    let v = post_form(
        base,
        "/login/oauth/access_token",
        &[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ],
    )
    .await?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        return Err(AuthError::OAuth(err.to_string()));
    }
    TokenSet::from_response(&v, Utc::now())
        .ok_or_else(|| AuthError::OAuth("no access_token".into()))
}

/// Where the token lives. The real one is the macOS Keychain; tests use [`MemoryStore`].
pub trait SecretStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, AuthError>;
    fn set(&self, value: &str) -> Result<(), AuthError>;
    fn delete(&self) -> Result<(), AuthError>;
}

pub fn load_tokens(store: &dyn SecretStore) -> Result<Option<TokenSet>, AuthError> {
    Ok(store
        .get()?
        .and_then(|raw| serde_json::from_str::<TokenSet>(&raw).ok()))
}

pub fn save_tokens(store: &dyn SecretStore, t: &TokenSet) -> Result<(), AuthError> {
    let raw = serde_json::to_string(t).map_err(|e| AuthError::Keychain(e.to_string()))?;
    store.set(&raw)
}

#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<Option<String>>);

impl SecretStore for MemoryStore {
    fn get(&self) -> Result<Option<String>, AuthError> {
        Ok(self.0.lock().expect("lock").clone())
    }
    fn set(&self, value: &str) -> Result<(), AuthError> {
        *self.0.lock().expect("lock") = Some(value.to_string());
        Ok(())
    }
    fn delete(&self) -> Result<(), AuthError> {
        *self.0.lock().expect("lock") = None;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub struct Keychain;

#[cfg(target_os = "macos")]
impl Keychain {
    fn map(e: security_framework::base::Error) -> AuthError {
        // errSecUserCanceled, errSecAuthFailed, errSecInteractionNotAllowed
        match e.code() {
            -128 | -25293 | -25308 => AuthError::KeychainDenied,
            _ => AuthError::Keychain(format!("OSStatus {}", e.code())),
        }
    }
}

#[cfg(target_os = "macos")]
impl SecretStore for Keychain {
    fn get(&self) -> Result<Option<String>, AuthError> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) if e.code() == -25300 => Ok(None), // errSecItemNotFound
            Err(e) => Err(Self::map(e)),
        }
    }
    fn set(&self, value: &str) -> Result<(), AuthError> {
        security_framework::passwords::set_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
            value.as_bytes(),
        )
        .map_err(Self::map)
    }
    fn delete(&self) -> Result<(), AuthError> {
        match security_framework::passwords::delete_generic_password(
            KEYCHAIN_SERVICE,
            KEYCHAIN_ACCOUNT,
        ) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(e) => Err(Self::map(e)),
        }
    }
}

/// The platform secret store (Keychain on macOS, in-memory elsewhere — only used by CI builds).
pub fn platform_store() -> Box<dyn SecretStore> {
    #[cfg(target_os = "macos")]
    {
        Box::new(Keychain)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(MemoryStore::default())
    }
}

/// Development fallback: the GitHub CLI's token. GUI apps get a minimal PATH, so the usual
/// install locations are added.
pub fn gh_cli_token() -> Result<TokenSet, AuthError> {
    let path = format!(
        "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:{}",
        std::env::var("PATH").unwrap_or_default()
    );
    let out = std::process::Command::new("gh")
        .args(["auth", "token"])
        .env("PATH", path)
        .output()
        .map_err(|e| AuthError::NoGhToken(e.to_string()))?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || token.is_empty() {
        return Err(AuthError::NoGhToken(
            "`gh auth token` returned nothing".into(),
        ));
    }
    Ok(TokenSet {
        access_token: token,
        refresh_token: None,
        expires_at: None,
        refresh_expires_at: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_window() {
        let now: DateTime<Utc> = "2026-03-02T10:00:00Z".parse().unwrap();
        let mut t = TokenSet {
            access_token: "a".into(),
            refresh_token: Some("r".into()),
            expires_at: Some(now + chrono::Duration::minutes(30)),
            refresh_expires_at: None,
        };
        assert!(!t.needs_refresh(now));
        assert!(t.needs_refresh(now + chrono::Duration::minutes(21)));
        t.refresh_token = None;
        assert!(
            !t.needs_refresh(now + chrono::Duration::hours(2)),
            "nothing to refresh with"
        );
    }

    #[test]
    fn debug_never_prints_the_token() {
        let t = TokenSet {
            access_token: "gho_secret".into(),
            refresh_token: Some("ghr_secret".into()),
            expires_at: None,
            refresh_expires_at: None,
        };
        let s = format!("{t:?}");
        assert!(!s.contains("secret"));
    }

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::default();
        assert_eq!(load_tokens(&s).unwrap(), None);
        let t = TokenSet {
            access_token: "a".into(),
            refresh_token: None,
            expires_at: None,
            refresh_expires_at: None,
        };
        save_tokens(&s, &t).unwrap();
        assert_eq!(load_tokens(&s).unwrap(), Some(t));
        s.delete().unwrap();
        assert_eq!(load_tokens(&s).unwrap(), None);
    }
}
