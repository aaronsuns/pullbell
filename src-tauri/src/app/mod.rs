//! Everything that touches Tauri: shared state, the poll loop, tray, banners, shortcuts,
//! windows, commands and the updater.

pub mod banner;
pub mod commands;
pub mod poller;
pub mod shortcuts;
pub mod tray;
pub mod updater;
pub mod windows;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::auth::{SecretStore, TokenSet};
use crate::core::health::{Health, Status};
use crate::core::inbox::State;
use crate::core::misc::{is_openable, open_command, UpdateAttempt};
use crate::core::model::PullRequest;
use crate::core::settings::Settings;
use crate::core::store;

pub struct Session {
    pub tokens: TokenSet,
    pub login: Option<String>,
    /// "device" or "gh"
    pub source: &'static str,
}

#[derive(Default, Clone, Serialize)]
pub struct PrLists {
    pub mine: Vec<PullRequest>,
    pub waiting: Vec<PullRequest>,
}

#[derive(Default)]
pub struct TeamCache {
    pub fetched_at: Option<DateTime<Utc>>,
    pub key: Vec<String>,
    pub repos: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Default, Clone, Serialize)]
pub struct UpdateInfo {
    pub available: Option<String>,
    #[serde(skip)]
    pub last_failed: Option<UpdateAttempt>,
    pub installing: bool,
}

pub struct Shared {
    pub settings: Mutex<Settings>,
    pub state: Mutex<State>,
    pub health: Mutex<Health>,
    pub session: Mutex<Option<Session>>,
    pub prs: Mutex<PrLists>,
    pub teams: Mutex<TeamCache>,
    pub update: Mutex<UpdateInfo>,
    pub store: Box<dyn SecretStore>,
    pub wake: Notify,
    pub sign_in_cancel: AtomicBool,
    /// False after an explicit sign-out, so the `gh` fallback does not sign straight back in.
    pub gh_allowed: AtomicBool,
    pub settings_path: PathBuf,
    pub state_path: PathBuf,
}

impl Shared {
    pub fn load(dir: PathBuf, store: Box<dyn SecretStore>) -> Shared {
        let settings_path = dir.join("settings.json");
        let state_path = dir.join("state.json");
        let settings = match std::fs::read_to_string(&settings_path) {
            Err(_) => Settings::default(),
            Ok(raw) => {
                let (s, corrupt) = Settings::from_json(&raw);
                if corrupt {
                    let moved = store::quarantine(&settings_path);
                    log::warn!("settings file was unreadable; moved to {moved:?}, using defaults");
                }
                s
            }
        };
        let (state, how) = store::load_json::<State>(&state_path);
        if let store::Loaded::Quarantined(moved) = how {
            log::warn!("state file was unreadable; moved to {moved:?}, starting fresh");
        }
        Shared {
            settings: Mutex::new(settings),
            state: Mutex::new(state),
            health: Mutex::new(Health::default()),
            session: Mutex::new(None),
            prs: Mutex::new(PrLists::default()),
            teams: Mutex::new(TeamCache::default()),
            update: Mutex::new(UpdateInfo::default()),
            store,
            wake: Notify::new(),
            sign_in_cancel: AtomicBool::new(false),
            gh_allowed: AtomicBool::new(true),
            settings_path,
            state_path,
        }
    }

    pub fn save_state(&self) {
        let st = lock(&self.state).clone();
        if let Err(e) = store::save_json(&self.state_path, &st) {
            log::error!("could not save state: {e}");
        }
    }

    pub fn save_settings(&self) {
        let s = lock(&self.settings).clone();
        if let Err(e) = store::save_json(&self.settings_path, &s) {
            log::error!("could not save settings: {e}");
        }
    }

    pub fn signed_in(&self) -> bool {
        lock(&self.session).is_some()
    }

    pub fn status(&self) -> Option<Status> {
        let update = lock(&self.update).available.clone();
        lock(&self.health).status(self.signed_in(), update.as_deref(), Utc::now())
    }
}

/// Lock a mutex, recovering from poisoning (a panicked poll must not wedge the menu).
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn shared(app: &AppHandle) -> tauri::State<'_, Shared> {
    app.state::<Shared>()
}

/// Open a URL in the browser chosen in Settings.
pub fn open_url(app: &AppHandle, url: &str) {
    if !is_openable(url) {
        log::warn!("refusing to open non-http URL");
        return;
    }
    let browser = lock(&shared(app).settings).browser.clone();
    let cmd = open_command(&browser, url);
    if let Err(e) = std::process::Command::new(&cmd[0]).args(&cmd[1..]).spawn() {
        log::error!("could not open browser: {e}");
    }
}

/// Mark items done, optionally opening the first one, and mark their GitHub threads read.
pub fn mark_done(app: &AppHandle, keys: &[String], open: bool) {
    let sh = shared(app);
    let mut done = Vec::new();
    {
        let mut st = lock(&sh.state);
        for k in keys {
            if let Some(item) = st.mark_done(k) {
                done.push(item);
            }
        }
    }
    if open {
        if let Some(first) = done.first() {
            open_url(app, &first.url);
        }
    }
    sh.save_state();
    let threads: Vec<String> = done.iter().filter_map(|i| i.thread_id.clone()).collect();
    if !threads.is_empty() {
        let token = lock(&sh.session)
            .as_ref()
            .map(|s| s.tokens.access_token.clone());
        if let Some(token) = token {
            tauri::async_runtime::spawn(async move {
                let gh = crate::github::GitHub::new(crate::github::API_BASE, token);
                for t in threads {
                    if let Err(e) = gh.mark_thread_read(&t).await {
                        log::warn!("could not mark thread read: {e}");
                    }
                }
            });
        }
    }
    refresh_ui(app);
}

/// Rebuild the tray and tell open windows to reload their data.
pub fn refresh_ui(app: &AppHandle) {
    tray::update(app);
    let _ = app.emit("pullbell://changed", ());
}
