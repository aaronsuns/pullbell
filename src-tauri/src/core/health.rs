//! Connection health: 401 streaks, SSO requirements, rate-limit back-off and poll timing.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

/// After this long of continuous 401s the user is asked to sign in again.
pub const SIGN_IN_AFTER_MINUTES: i64 = 30;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Health {
    pub first_401: Option<DateTime<Utc>>,
    pub consecutive_401: u32,
    /// org → authorization URL
    pub sso: BTreeMap<String, String>,
    pub rate_limited_until: Option<DateTime<Utc>>,
    pub keychain_denied: bool,
    pub last_error: Option<String>,
}

/// The single status line shown at the top of the menu, most urgent first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Status {
    NotSignedIn,
    Refusing,
    KeychainDenied,
    Sso { org: String, url: String },
    UpdateAvailable { version: String },
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Status::NotSignedIn => "Not signed in — Sign in…".into(),
            Status::Refusing => "GitHub is refusing the sign-in — retrying".into(),
            Status::KeychainDenied => "Keychain access needed — Try again".into(),
            Status::Sso { org, .. } => format!("Authorize for {org}…"),
            Status::UpdateAvailable { version } => format!("Update available ({version})"),
        }
    }
}

impl Health {
    pub fn on_success(&mut self) {
        self.first_401 = None;
        self.consecutive_401 = 0;
        self.last_error = None;
    }

    pub fn on_401(&mut self, now: DateTime<Utc>) {
        self.first_401.get_or_insert(now);
        self.consecutive_401 += 1;
    }

    /// True once 401s have lasted long enough that the token must be considered dead.
    pub fn must_sign_in(&self, now: DateTime<Utc>) -> bool {
        self.first_401
            .is_some_and(|t| now - t >= Duration::minutes(SIGN_IN_AFTER_MINUTES))
    }

    pub fn status(
        &self,
        signed_in: bool,
        update: Option<&str>,
        now: DateTime<Utc>,
    ) -> Option<Status> {
        if self.keychain_denied {
            return Some(Status::KeychainDenied);
        }
        if !signed_in || self.must_sign_in(now) {
            return Some(Status::NotSignedIn);
        }
        if self.first_401.is_some() {
            return Some(Status::Refusing);
        }
        if let Some((org, url)) = self.sso.iter().next() {
            return Some(Status::Sso {
                org: org.clone(),
                url: url.clone(),
            });
        }
        update.map(|v| Status::UpdateAvailable {
            version: v.to_string(),
        })
    }
}

/// Delay until the next poll, in seconds: never faster than the user's interval or GitHub's
/// `X-Poll-Interval`; longer during a 401 streak (1, 2, 4, 8 … minutes, capped at 15) and until
/// a rate limit resets.
pub fn next_delay(
    user_minutes: u32,
    server_poll_secs: Option<u64>,
    consecutive_401: u32,
    rate_limited_until: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> u64 {
    let mut secs = u64::from(user_minutes.max(1)) * 60;
    if let Some(p) = server_poll_secs {
        secs = secs.max(p);
    }
    if consecutive_401 > 0 {
        let backoff = 60u64 << (consecutive_401 - 1).min(4);
        secs = secs.max(backoff.min(15 * 60));
    }
    if let Some(until) = rate_limited_until {
        let wait = (until - now).num_seconds();
        if wait > 0 {
            secs = secs.max(wait as u64 + 1);
        }
    }
    secs
}

/// Parse `X-GitHub-SSO: required; url=https://github.com/orgs/octo-org/sso?authorization_request=…`
/// into (org, url).
pub fn parse_sso_header(value: &str) -> Option<(String, String)> {
    let mut parts = value.split(';').map(str::trim);
    if parts.next()? != "required" {
        return None;
    }
    let url = parts.find_map(|p| p.strip_prefix("url="))?.to_string();
    let org = url
        .split("/orgs/")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .filter(|o| !o.is_empty())
        .unwrap_or("your organization")
        .to_string();
    Some((org, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(m: i64) -> DateTime<Utc> {
        "2026-03-02T10:00:00Z".parse::<DateTime<Utc>>().unwrap() + Duration::minutes(m)
    }

    #[test]
    fn a_401_is_not_a_sign_out_until_thirty_minutes() {
        let mut h = Health::default();
        h.on_401(t(0));
        h.on_401(t(10));
        assert_eq!(h.status(true, None, t(10)), Some(Status::Refusing));
        assert!(!h.must_sign_in(t(29)));
        assert!(h.must_sign_in(t(30)));
        assert_eq!(h.status(true, None, t(30)), Some(Status::NotSignedIn));
        h.on_success();
        assert_eq!(h.status(true, None, t(31)), None);
    }

    #[test]
    fn status_priorities() {
        let mut h = Health::default();
        assert_eq!(h.status(false, None, t(0)), Some(Status::NotSignedIn));
        assert_eq!(
            h.status(true, Some("0.2.0"), t(0)).unwrap().label(),
            "Update available (0.2.0)"
        );
        h.sso.insert(
            "octo-org".into(),
            "https://github.com/orgs/octo-org/sso".into(),
        );
        assert_eq!(
            h.status(true, Some("0.2.0"), t(0)).unwrap().label(),
            "Authorize for octo-org…"
        );
        h.keychain_denied = true;
        assert_eq!(h.status(true, None, t(0)), Some(Status::KeychainDenied));
    }

    #[test]
    fn delays_never_beat_the_intervals() {
        assert_eq!(next_delay(3, None, 0, None, t(0)), 180);
        assert_eq!(next_delay(1, Some(90), 0, None, t(0)), 90);
        assert_eq!(next_delay(1, Some(30), 0, None, t(0)), 60);
        assert_eq!(next_delay(1, None, 1, None, t(0)), 60);
        assert_eq!(next_delay(1, None, 3, None, t(0)), 240);
        assert_eq!(next_delay(1, None, 20, None, t(0)), 900);
        assert_eq!(next_delay(1, None, 0, Some(t(10)), t(0)), 601);
        assert_eq!(next_delay(3, None, 0, Some(t(-5)), t(0)), 180);
    }

    #[test]
    fn parses_sso_header() {
        let (org, url) = parse_sso_header(
            "required; url=https://github.com/orgs/octo-org/sso?authorization_request=abc",
        )
        .unwrap();
        assert_eq!(org, "octo-org");
        assert!(url.ends_with("authorization_request=abc"));
        assert!(parse_sso_header("partial-results; organizations=1,2").is_none());
    }
}
