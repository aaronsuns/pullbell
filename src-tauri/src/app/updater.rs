//! Update checks via the Tauri updater plugin (signed `latest.json` on GitHub releases).
//!
//! Configuration lives in `tauri.conf.json` → `plugins.updater` (`pubkey`, `endpoints`). Until a
//! real public key replaces the placeholder, update checks are skipped.

use std::time::Duration;

use chrono::Utc;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

use super::{banner, lock, refresh_ui, shared, windows};
use crate::core::inbox::Banner;
use crate::core::misc::{may_attempt_update, UpdateAttempt, UPDATE_CHECK_HOURS};

pub const PUBKEY_PLACEHOLDER: &str = "REPLACE_WITH_UPDATER_PUBLIC_KEY";

pub fn configured(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.is_empty() && k != PUBKEY_PLACEHOLDER)
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if !configured(&app) {
            log::info!("update: updater public key not configured; skipping update checks");
            return;
        }
        loop {
            check(&app).await;
            tokio::time::sleep(Duration::from_secs(UPDATE_CHECK_HOURS as u64 * 3600)).await;
        }
    });
}

pub async fn check(app: &AppHandle) {
    let sh = shared(app);
    let (enabled, auto) = {
        let s = lock(&sh.settings);
        (s.updates.check, s.updates.auto_install)
    };
    if !enabled {
        return;
    }
    let update = match app.updater().map(|u| async move { u.check().await }) {
        Ok(fut) => match fut.await {
            Ok(u) => u,
            Err(e) => {
                log::warn!("update: check failed: {e}");
                return;
            }
        },
        Err(e) => {
            log::warn!("update: updater unavailable: {e}");
            return;
        }
    };
    let Some(update) = update else {
        lock(&sh.update).available = None;
        return;
    };
    log::info!("update: {} available", update.version);
    lock(&sh.update).available = Some(update.version.clone());
    refresh_ui(app);
    if auto {
        install(app.clone(), update).await;
    }
}

/// "Update available" clicked in the menu.
pub fn install_now(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(updater) = app.updater() else { return };
        match updater.check().await {
            Ok(Some(u)) => install(app.clone(), u).await,
            Ok(None) => log::info!("update: nothing to install"),
            Err(e) => log::warn!("update: check failed: {e}"),
        }
    });
}

async fn install(app: AppHandle, update: tauri_plugin_updater::Update) {
    let sh = shared(&app);
    let version = update.version.clone();
    {
        let mut u = lock(&sh.update);
        if u.installing || !may_attempt_update(&version, u.last_failed.as_ref(), Utc::now()) {
            return;
        }
        u.installing = true;
    }
    log::info!("update: downloading {version}");
    let res = update.download_and_install(|_, _| {}, || {}).await;
    lock(&sh.update).installing = false;
    match res {
        Ok(()) => {
            log::info!("update: installed {version}; restarting when no window is open");
            {
                let mut st = lock(&sh.state);
                st.installed_update = Some(version.clone());
            }
            sh.save_state();
            while windows::any_visible(&app) {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
            app.restart();
        }
        Err(e) => {
            log::error!("update: install of {version} failed: {e}");
            lock(&sh.update).last_failed = Some(UpdateAttempt {
                version,
                at: Utc::now(),
            });
        }
    }
}

/// After a restart into a new version, say so once.
pub fn announce_if_updated(app: &AppHandle) {
    let sh = shared(app);
    let current = app.package_info().version.to_string();
    let installed = lock(&sh.state).installed_update.take();
    if let Some(v) = installed {
        sh.save_state();
        if v == current {
            log::info!("update: now running {v}");
            banner::show(
                app,
                Banner {
                    title: format!("Pullbell updated to {v}"),
                    body: "Enjoy the new version.".into(),
                    url: String::new(),
                    keys: vec![],
                },
            );
        }
    }
}
