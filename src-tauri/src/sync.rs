//! One poll: fetch everything from GitHub, turn it into events and update the inbox state.
//! No UI here, so it is tested end-to-end against a mock server.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::core::classify::classify;
use crate::core::digest;
use crate::core::inbox::{banners, Banner, State};
use crate::core::model::{Event, PullRequest};
use crate::core::prs::transitions;
use crate::core::runs::{failures, pattern};
use crate::core::scope::{Scope, ScopeMode};
use crate::core::settings::Settings;
use crate::github::{ApiError, GitHub, NotificationsPage};

pub const MY_PRS_QUERY: &str = "is:pr is:open author:@me archived:false";
pub const WAITING_QUERY: &str = "is:pr is:open review-requested:@me archived:false";
const MAX_RUN_REPOS: usize = 30;

#[derive(Debug, Default)]
pub struct SyncResult {
    pub banners: Vec<Banner>,
    pub fresh: Vec<Event>,
    pub mine: Vec<PullRequest>,
    pub waiting: Vec<PullRequest>,
    pub poll_interval: Option<u64>,
    /// org → SSO authorization URL for orgs that refused us this poll.
    pub sso: BTreeMap<String, String>,
    /// Non-fatal problems worth a log line.
    pub warnings: Vec<String>,
}

/// Errors that abort the whole poll (the token is refused or we are rate limited).
fn fatal(e: &ApiError) -> bool {
    matches!(e, ApiError::Unauthorized | ApiError::RateLimited { .. })
}

/// Run one poll. `local_now` is wall-clock time for the digest hour.
pub async fn sync(
    gh: &GitHub,
    state: &mut State,
    settings: &Settings,
    scope: &Scope,
    login: &str,
    now: DateTime<Utc>,
    local_now: NaiveDateTime,
) -> Result<SyncResult, ApiError> {
    let mut out = SyncResult::default();
    let mut events = Vec::new();

    // 1. Notifications.
    match gh
        .notifications(state.notifications_last_modified.as_deref())
        .await?
    {
        NotificationsPage::NotModified { poll_interval } => out.poll_interval = poll_interval,
        NotificationsPage::Page {
            items,
            last_modified,
            poll_interval,
        } => {
            out.poll_interval = poll_interval;
            for raw in &items {
                let Some(n) = GitHub::parse_notification(raw) else {
                    continue;
                };
                if !scope.watches(&n.repository.full_name) {
                    continue;
                }
                let key = format!("n:{}", n.id);
                let known = state
                    .items
                    .iter()
                    .find(|i| i.key == key)
                    .map(|i| i.updated_at)
                    .into_iter()
                    .chain(state.done.get(&key).copied())
                    .max();
                if known.is_some_and(|t| t >= n.updated_at) {
                    continue; // nothing new on this thread; skip the detail request
                }
                let detail = match gh.latest_activity(&n).await {
                    Ok(d) => d,
                    Err(e) if fatal(&e) => return Err(e),
                    Err(ApiError::Sso { org, url }) => {
                        out.sso.insert(org, url);
                        None
                    }
                    Err(e) => {
                        out.warnings
                            .push(format!("detail for thread {}: {e}", n.id));
                        None
                    }
                };
                events.extend(classify(&n, detail.as_ref(), login));
            }
            state.notifications_last_modified = last_modified;
        }
    }

    // 2. My open PRs (transitions) and PRs waiting for my review.
    let in_scope = |prs: Vec<PullRequest>| -> Vec<PullRequest> {
        prs.into_iter().filter(|p| scope.watches(&p.repo)).collect()
    };
    out.mine = in_scope(gh.search_prs(MY_PRS_QUERY).await?);
    out.waiting = in_scope(gh.search_prs(WAITING_QUERY).await?);
    let (pr_events, phases) = transitions(&state.pr_phases, &out.mine, now);
    state.pr_phases = phases;
    events.extend(pr_events);

    // 3. Default-branch deploy failures.
    let pat = pattern(&settings.alerts.failure_pattern);
    for repo in run_repos(scope, &out) {
        let branch = match state.default_branches.get(&repo) {
            Some(b) => b.clone(),
            None => match gh.default_branch(&repo).await {
                Ok(b) => {
                    state.default_branches.insert(repo.clone(), b.clone());
                    b
                }
                Err(e) if fatal(&e) => return Err(e),
                Err(e) => {
                    note(&mut out, &repo, e);
                    continue;
                }
            },
        };
        match gh.workflow_runs(&repo, &branch).await {
            Ok(runs) => {
                let cursor = state.run_cursors.get(&repo).copied();
                let (evs, next) = failures(&repo, &branch, &runs, cursor, &pat);
                if let Some(c) = next {
                    state.run_cursors.insert(repo.clone(), c);
                }
                events.extend(evs);
            }
            Err(e) if fatal(&e) => return Err(e),
            Err(e) => note(&mut out, &repo, e),
        }
    }

    // 4. Daily digest.
    if digest::due(local_now, settings.alerts.digest_hour, state.last_digest) {
        state.last_digest = Some(local_now.date());
        events.extend(digest::build(&out.waiting, local_now.date(), now));
    }

    // 5. Apply. The first sync after sign-in is silent.
    let silent = !state.synced_once;
    out.fresh = state.apply(events);
    if !silent {
        out.banners = banners(&out.fresh, settings);
    }
    state.synced_once = true;
    state.prune(now);
    Ok(out)
}

fn note(out: &mut SyncResult, repo: &str, e: ApiError) {
    match e {
        ApiError::Sso { org, url } => {
            out.sso.insert(org, url);
        }
        // Actions disabled, no access, or repo gone: not worth a warning every poll.
        ApiError::NotFound | ApiError::Http(403, _) => {}
        e => out.warnings.push(format!("runs for {repo}: {e}")),
    }
}

/// Repos whose default-branch runs we check: the allowlist, or (watching everything) the repos
/// I have open PRs in or am asked to review in.
fn run_repos(scope: &Scope, out: &SyncResult) -> Vec<String> {
    let set: BTreeSet<String> = match scope.mode {
        ScopeMode::Allowlist => scope.allowed.iter().cloned().collect(),
        ScopeMode::Everything => out
            .mine
            .iter()
            .chain(&out.waiting)
            .map(|p| p.repo.to_lowercase())
            .collect(),
    };
    set.into_iter()
        .filter(|r| scope.watches(r))
        .take(MAX_RUN_REPOS)
        .collect()
}
