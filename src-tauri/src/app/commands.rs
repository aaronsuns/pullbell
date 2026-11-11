//! Commands the windows call (`invoke("…")`).

use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_autostart::ManagerExt;

use super::{banner, lock, poller, refresh_ui, shared, shortcuts, updater};
use crate::auth::{self, client_id, AuthError, PollOutcome, OAUTH_BASE};
use crate::core::health::Status;
use crate::core::inbox::{row, Banner};
use crate::core::misc::{chrome_profiles as parse_profiles, ChromeProfile};
use crate::core::model::{Item, Kind, PullRequest};
use crate::core::settings::Settings;
use crate::core::shortcut;
use crate::github::{GitHub, Team, API_BASE};

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
pub struct ItemView {
    #[serde(flatten)]
    item: Item,
    needs_you: bool,
    emoji: &'static str,
    row: String,
}

#[derive(Serialize)]
pub struct PrView {
    #[serde(flatten)]
    pr: PullRequest,
    ci_glyph: &'static str,
    review_glyph: &'static str,
}

impl From<PullRequest> for PrView {
    fn from(pr: PullRequest) -> Self {
        PrView {
            ci_glyph: pr.ci_glyph(),
            review_glyph: pr.review_glyph(),
            pr,
        }
    }
}

#[derive(Serialize)]
pub struct Snapshot {
    items: Vec<ItemView>,
    mine: Vec<PrView>,
    waiting: Vec<PrView>,
    status: Option<Status>,
    status_label: Option<String>,
    signed_in: bool,
    login: Option<String>,
    needs_you: usize,
    fyi: usize,
    version: String,
}

#[tauri::command]
pub fn get_snapshot(app: AppHandle) -> Snapshot {
    let sh = shared(&app);
    let settings = lock(&sh.settings).clone();
    let (items, counts) = {
        let st = lock(&sh.state);
        (st.sorted(&settings), st.counts(&settings))
    };
    let prs = lock(&sh.prs).clone();
    let status = sh.status();
    let login = lock(&sh.session).as_ref().and_then(|s| s.login.clone());
    Snapshot {
        items: items
            .into_iter()
            .map(|i| ItemView {
                needs_you: settings.needs_you(i.kind),
                emoji: i.kind.emoji(),
                row: row(&i, 200),
                item: i,
            })
            .collect(),
        mine: prs.mine.into_iter().map(Into::into).collect(),
        waiting: prs.waiting.into_iter().map(Into::into).collect(),
        status_label: status.as_ref().map(Status::label),
        status,
        signed_in: sh.signed_in(),
        login,
        needs_you: counts.0,
        fyi: counts.1,
        version: app.package_info().version.to_string(),
    }
}

#[tauri::command]
pub fn mark_done(app: AppHandle, key: String, open: bool) {
    super::mark_done(&app, &[key], open);
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) {
    super::open_url(&app, &url);
}

#[tauri::command]
pub fn refresh_now(app: AppHandle) {
    shared(&app).wake.notify_one();
}

#[derive(Serialize)]
pub struct KindInfo {
    kind: Kind,
    label: &'static str,
    emoji: &'static str,
}

#[derive(Serialize)]
pub struct SettingsView {
    settings: Settings,
    kinds: Vec<KindInfo>,
    team_errors: Vec<String>,
    autostart: bool,
    version: String,
    updater_configured: bool,
    signed_in: bool,
    login: Option<String>,
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> SettingsView {
    let sh = shared(&app);
    let settings = lock(&sh.settings).clone();
    let team_errors = lock(&sh.teams).errors.clone();
    let login = lock(&sh.session).as_ref().and_then(|s| s.login.clone());
    SettingsView {
        settings,
        kinds: Kind::ALL
            .iter()
            .map(|k| KindInfo {
                kind: *k,
                label: k.label(),
                emoji: k.emoji(),
            })
            .collect(),
        team_errors,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        version: app.package_info().version.to_string(),
        updater_configured: updater::configured(&app),
        signed_in: sh.signed_in(),
        login,
    }
}

/// Save settings; returns them normalized. Takes effect on the next poll.
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Settings {
    let sh = shared(&app);
    let normalized = settings.normalized();
    let autostart_changed = lock(&sh.settings).start_at_login != normalized.start_at_login;
    *lock(&sh.settings) = normalized.clone();
    sh.save_settings();
    if autostart_changed {
        apply_autostart(&app);
    }
    shortcuts::register_all(&app);
    refresh_ui(&app);
    normalized
}

#[tauri::command]
pub fn validate_shortcut(candidate: String, others: Vec<String>) -> CmdResult<String> {
    let others: Vec<&str> = others.iter().map(String::as_str).collect();
    shortcut::validate(&candidate, &others)
}

#[tauri::command]
pub fn display_shortcut(acc: Option<String>) -> String {
    shortcut::display(acc.as_deref())
}

#[tauri::command]
pub fn pause_shortcuts(app: AppHandle) {
    shortcuts::pause(&app);
}

#[tauri::command]
pub fn resume_shortcuts(app: AppHandle) {
    shortcuts::register_all(&app);
}

#[tauri::command]
pub async fn list_teams(app: AppHandle) -> CmdResult<Vec<Team>> {
    let token = lock(&shared(&app).session)
        .as_ref()
        .map(|s| s.tokens.access_token.clone())
        .ok_or("Sign in first")?;
    GitHub::new(API_BASE, token)
        .user_teams()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn chrome_profiles() -> Vec<ChromeProfile> {
    let path = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join("Library/Application Support/Google/Chrome/Local State");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .map(|v| parse_profiles(&v))
        .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SignInStart {
    Device {
        user_code: String,
        verification_uri: String,
    },
    /// Development build without a client id: signed in with the GitHub CLI token.
    Gh,
}

#[tauri::command]
pub async fn sign_in_start(app: AppHandle) -> CmdResult<SignInStart> {
    let sh = shared(&app);
    sh.gh_allowed.store(true, Ordering::SeqCst);
    let Some(cid) = client_id() else {
        let tokens = auth::gh_cli_token().map_err(|e| {
            format!("{e}. This build has no OAuth client id; install the GitHub CLI and run `gh auth login`.")
        })?;
        poller::set_session(&app, tokens, "gh");
        signed_in(&app);
        return Ok(SignInStart::Gh);
    };
    let code = auth::device_start(OAUTH_BASE, cid)
        .await
        .map_err(|e| e.to_string())?;
    sh.sign_in_cancel.store(false, Ordering::SeqCst);
    let out = SignInStart::Device {
        user_code: code.user_code.clone(),
        verification_uri: code.verification_uri.clone(),
    };
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let sh = shared(&app2);
        let res = auth::device_wait(OAUTH_BASE, cid, &code, || {
            sh.sign_in_cancel.load(Ordering::SeqCst)
        })
        .await;
        let msg = match res {
            Ok(PollOutcome::Done(tokens)) => {
                match auth::save_tokens(sh.store.as_ref(), &tokens) {
                    Err(AuthError::KeychainDenied) => lock(&sh.health).keychain_denied = true,
                    Err(e) => log::error!("auth: could not store token: {e}"),
                    Ok(()) => {}
                }
                poller::set_session(&app2, tokens, "device");
                log::info!("auth: signed in with device flow");
                signed_in(&app2);
                return;
            }
            Ok(PollOutcome::Expired) => "The code expired. Try again.".to_string(),
            Ok(PollOutcome::Denied) => "Sign-in was cancelled.".to_string(),
            Ok(_) => "Sign-in did not finish.".to_string(),
            Err(e) => e.to_string(),
        };
        log::warn!("auth: device flow ended: {msg}");
        let _ = app2.emit("pullbell://auth", serde_json::json!({ "error": msg }));
    });
    Ok(out)
}

fn signed_in(app: &AppHandle) {
    let sh = shared(app);
    {
        let mut st = lock(&sh.state);
        st.synced_once = false; // the first sync after sign-in is silent
    }
    let _ = app.emit("pullbell://auth", serde_json::json!({ "signed_in": true }));
    sh.wake.notify_one();
    refresh_ui(app);
}

#[tauri::command]
pub fn sign_in_cancel(app: AppHandle) {
    shared(&app).sign_in_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn sign_out(app: AppHandle) {
    poller::sign_out(&app);
    let _ = app.emit("pullbell://auth", serde_json::json!({ "signed_in": false }));
    super::windows::open(&app, "welcome");
}

#[tauri::command]
pub fn test_banner(app: AppHandle) {
    banner::show(
        &app,
        Banner {
            title: "🔔 Pullbell banners are on".into(),
            body: "Review requests and comments on your PRs will show up like this.".into(),
            url: String::new(),
            keys: vec![],
        },
    );
}

#[tauri::command]
pub fn finish_welcome(app: AppHandle) {
    let sh = shared(&app);
    lock(&sh.settings).welcome_done = true;
    sh.save_settings();
}

#[tauri::command]
pub async fn check_updates(app: AppHandle) {
    updater::check(&app).await;
}

/// Make the login item match the setting. Only from an installed .app, never from `tauri dev`.
pub fn apply_autostart(app: &AppHandle) {
    let bundled = std::env::current_exe()
        .map(|p| p.to_string_lossy().contains(".app/Contents/MacOS"))
        .unwrap_or(false);
    if !bundled {
        return;
    }
    let want = lock(&shared(app).settings).start_at_login;
    let al = app.autolaunch();
    let have = al.is_enabled().unwrap_or(false);
    let res = match (want, have) {
        (true, false) => al.enable(),
        (false, true) => al.disable(),
        _ => Ok(()),
    };
    if let Err(e) = res {
        log::warn!("start at login: {e}");
    }
}
