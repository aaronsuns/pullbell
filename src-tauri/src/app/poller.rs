//! The background poll loop.

use std::time::Duration;

use chrono::{Local, Utc};
use tauri::AppHandle;

use super::{banner, lock, refresh_ui, shared, PrLists, Session};
use crate::auth::{self, client_id, AuthError, OAUTH_BASE};
use crate::core::health::next_delay;
use crate::core::scope::{Scope, ScopeMode};
use crate::github::{ApiError, GitHub, API_BASE};
use crate::sync::sync;

const TEAM_CACHE_MINUTES: i64 = 30;

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let delay = poll_once(&app).await;
            let sh = shared(&app);
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
                _ = sh.wake.notified() => {}
            }
        }
    });
}

/// Make sure we have a token: Keychain first, then (without a built-in client id) `gh`.
pub async fn ensure_session(app: &AppHandle) -> bool {
    let sh = shared(app);
    let existing = lock(&sh.session)
        .as_ref()
        .map(|s| (s.tokens.clone(), s.source));
    if let Some((tokens, source)) = existing {
        if source == "device" && tokens.needs_refresh(Utc::now()) {
            if let (Some(cid), Some(rt)) = (client_id(), tokens.refresh_token.as_deref()) {
                match auth::refresh(OAUTH_BASE, cid, rt).await {
                    Ok(new) => {
                        if let Err(e) = auth::save_tokens(sh.store.as_ref(), &new) {
                            log::error!("could not store refreshed token: {e}");
                        }
                        if let Some(s) = lock(&sh.session).as_mut() {
                            s.tokens = new;
                        }
                        log::info!("auth: token refreshed");
                    }
                    Err(e) => log::warn!("auth: refresh failed: {e}"),
                }
            }
        }
        return true;
    }
    match auth::load_tokens(sh.store.as_ref()) {
        Ok(Some(tokens)) => {
            lock(&sh.health).keychain_denied = false;
            set_session(app, tokens, "device");
            log::info!("auth: signed in from Keychain");
            return true;
        }
        Ok(None) => lock(&sh.health).keychain_denied = false,
        Err(AuthError::KeychainDenied) => {
            lock(&sh.health).keychain_denied = true;
            log::warn!("auth: Keychain access denied");
            return false;
        }
        Err(e) => log::error!("auth: Keychain error: {e}"),
    }
    if client_id().is_none() && sh.gh_allowed.load(std::sync::atomic::Ordering::SeqCst) {
        match auth::gh_cli_token() {
            Ok(tokens) => {
                set_session(app, tokens, "gh");
                log::info!("auth: using GitHub CLI token (development build)");
                return true;
            }
            Err(e) => log::info!("auth: {e}"),
        }
    }
    false
}

pub fn set_session(app: &AppHandle, tokens: auth::TokenSet, source: &'static str) {
    let sh = shared(app);
    *lock(&sh.session) = Some(Session {
        tokens,
        login: None,
        source,
    });
    lock(&sh.health).on_success();
}

/// Resolve the watched-repo scope, using cached team lookups.
async fn resolve_scope(app: &AppHandle, gh: &GitHub) -> Scope {
    let sh = shared(app);
    let repos = lock(&sh.settings).repos.clone();
    if repos.mode == ScopeMode::Everything || repos.teams.is_empty() {
        lock(&sh.teams).errors.clear();
        return Scope::build(repos.mode, vec![], repos.extra, repos.muted);
    }
    let fresh = {
        let tc = lock(&sh.teams);
        tc.key == repos.teams
            && tc
                .fetched_at
                .is_some_and(|t| Utc::now() - t < chrono::Duration::minutes(TEAM_CACHE_MINUTES))
    };
    if !fresh {
        let mut all = Vec::new();
        let mut errors = Vec::new();
        for t in &repos.teams {
            let Some((org, slug)) = t.split_once('/') else {
                continue;
            };
            match gh.team_repos(org, slug).await {
                Ok(r) => all.extend(r),
                Err(e) => errors.push(format!("{t}: {e}")),
            }
        }
        for e in &errors {
            log::warn!("team lookup failed: {e}");
        }
        let mut tc = lock(&sh.teams);
        tc.key = repos.teams.clone();
        tc.repos = all;
        tc.errors = errors;
        tc.fetched_at = Some(Utc::now());
    }
    let team_repos = lock(&sh.teams).repos.clone();
    Scope::build(ScopeMode::Allowlist, team_repos, repos.extra, repos.muted)
}

/// One poll. Returns the number of seconds to wait before the next one.
pub async fn poll_once(app: &AppHandle) -> u64 {
    let sh = shared(app);
    let minutes = lock(&sh.settings).poll_minutes;
    if !ensure_session(app).await {
        refresh_ui(app);
        return u64::from(minutes) * 60;
    }
    let (token, login) = {
        let s = lock(&sh.session);
        let s = s.as_ref().expect("session ensured");
        (s.tokens.access_token.clone(), s.login.clone())
    };
    let gh = GitHub::new(API_BASE, token);

    let login = match login {
        Some(l) => l,
        None => match gh.viewer().await {
            Ok(l) => {
                if let Some(s) = lock(&sh.session).as_mut() {
                    s.login = Some(l.clone());
                }
                let mut st = lock(&sh.state);
                if st.login.as_deref() != Some(l.as_str()) {
                    *st = Default::default();
                    st.login = Some(l.clone());
                }
                l
            }
            Err(e) => return after_error(app, e, minutes, None),
        },
    };

    let scope = resolve_scope(app, &gh).await;
    let settings = lock(&sh.settings).clone();
    let mut state = lock(&sh.state).clone();
    let started = std::time::Instant::now();
    let result = sync(
        &gh,
        &mut state,
        &settings,
        &scope,
        &login,
        Utc::now(),
        Local::now().naive_local(),
    )
    .await;
    match result {
        Ok(r) => {
            *lock(&sh.state) = state;
            sh.save_state();
            *lock(&sh.prs) = PrLists {
                mine: r.mine.clone(),
                waiting: r.waiting.clone(),
            };
            {
                let mut h = lock(&sh.health);
                h.on_success();
                h.rate_limited_until = None;
                h.sso = r.sso.clone();
            }
            for w in &r.warnings {
                log::warn!("{w}");
            }
            let (n, m) = lock(&sh.state).counts(&settings);
            log::info!(
                "poll: {} new, {} banners, needs-you {n}, fyi {m}, my PRs {}, waiting {}, {} ms",
                r.fresh.len(),
                r.banners.len(),
                r.mine.len(),
                r.waiting.len(),
                started.elapsed().as_millis()
            );
            for b in r.banners {
                banner::show(app, b);
            }
            refresh_ui(app);
            next_delay(minutes, r.poll_interval, 0, None, Utc::now())
        }
        Err(e) => after_error(app, e, minutes, None),
    }
}

fn after_error(app: &AppHandle, e: ApiError, minutes: u32, server: Option<u64>) -> u64 {
    let sh = shared(app);
    let now = Utc::now();
    let delay = {
        let mut h = lock(&sh.health);
        match &e {
            ApiError::Unauthorized => {
                h.on_401(now);
                log::warn!(
                    "poll: GitHub refused the token ({} in a row)",
                    h.consecutive_401
                );
            }
            ApiError::RateLimited { until } => {
                h.rate_limited_until = Some(*until);
                log::warn!("poll: rate limited until {until}");
            }
            other => {
                h.last_error = Some(other.to_string());
                log::error!("poll failed: {other}");
            }
        }
        next_delay(
            minutes,
            server,
            h.consecutive_401,
            h.rate_limited_until,
            now,
        )
    };
    refresh_ui(app);
    delay
}

/// Forget the session and the stored token (Sign out).
pub fn sign_out(app: &AppHandle) {
    let sh = shared(app);
    *lock(&sh.session) = None;
    sh.gh_allowed
        .store(false, std::sync::atomic::Ordering::SeqCst);
    if let Err(e) = sh.store.delete() {
        log::error!("could not remove token from Keychain: {e}");
    }
    *lock(&sh.state) = Default::default();
    *lock(&sh.prs) = PrLists::default();
    *lock(&sh.health) = Default::default();
    sh.save_state();
    log::info!("auth: signed out");
    refresh_ui(app);
}
