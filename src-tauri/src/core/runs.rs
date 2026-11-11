//! Default-branch workflow failures that look like a deploy or release.

use chrono::{DateTime, Utc};
use regex::{Regex, RegexBuilder};
use serde::Deserialize;

use super::model::{Event, Kind};
use super::text::is_bot;

pub const DEFAULT_PATTERN: &str = "deploy|release|publish";

#[derive(Debug, Clone, Deserialize)]
pub struct Runs {
    pub workflow_runs: Vec<Run>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Run {
    pub id: u64,
    pub name: Option<String>,
    pub head_branch: Option<String>,
    pub conclusion: Option<String>,
    pub html_url: String,
    pub updated_at: DateTime<Utc>,
    pub actor: Option<RunActor>,
    pub display_title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RunActor {
    pub login: String,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// Compile the user's pattern case-insensitively; an invalid pattern falls back to the default.
pub fn pattern(user: &str) -> Regex {
    let build = |p: &str| RegexBuilder::new(p).case_insensitive(true).build();
    let trimmed = user.trim();
    if trimmed.is_empty() {
        return build(DEFAULT_PATTERN).expect("default pattern compiles");
    }
    build(trimmed).unwrap_or_else(|_| build(DEFAULT_PATTERN).expect("default pattern compiles"))
}

pub fn pattern_is_valid(user: &str) -> bool {
    RegexBuilder::new(user.trim()).build().is_ok()
}

/// New failed deploy-like runs on `default_branch`, newer than `cursor` (the highest run id seen).
/// With `cursor == None` (first look at this repo) nothing fires; the cursor is just set.
/// Returns the events and the new cursor.
pub fn failures(
    repo: &str,
    default_branch: &str,
    runs: &[Run],
    cursor: Option<u64>,
    pat: &Regex,
) -> (Vec<Event>, Option<u64>) {
    let max = runs.iter().map(|r| r.id).max();
    let next = match (cursor, max) {
        (Some(c), Some(m)) => Some(c.max(m)),
        (c, m) => c.or(m),
    };
    let Some(cursor) = cursor else {
        return (Vec::new(), next);
    };
    let events = runs
        .iter()
        .filter(|r| r.id > cursor)
        .filter(|r| r.head_branch.as_deref() == Some(default_branch))
        .filter(|r| matches!(r.conclusion.as_deref(), Some("failure" | "timed_out")))
        .filter(|r| pat.is_match(r.name.as_deref().unwrap_or_default()))
        .map(|r| {
            let name = r.name.clone().unwrap_or_else(|| "workflow".into());
            let actor = r
                .actor
                .as_ref()
                .filter(|a| !is_bot(&a.login, a.kind.as_deref()))
                .map(|a| a.login.clone())
                .unwrap_or_default();
            Event {
                key: format!("run:{}:{}", repo.to_lowercase(), r.id),
                kind: Kind::DeployFailure,
                repo: repo.to_string(),
                number: None,
                title: name.clone(),
                actor,
                action: format!("{name} failed on {default_branch}"),
                text: r.display_title.clone().unwrap_or_default(),
                url: r.html_url.clone(),
                updated_at: r.updated_at,
                thread_id: None,
                reason: None,
            }
        })
        .collect();
    (events, next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs() -> Vec<Run> {
        let path = format!(
            "{}/tests/fixtures/workflow_runs.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str::<Runs>(&std::fs::read_to_string(path).unwrap())
            .unwrap()
            .workflow_runs
    }

    #[test]
    fn first_look_is_silent_and_sets_cursor() {
        let (e, c) = failures("octo-org/web-app", "main", &runs(), None, &pattern(""));
        assert!(e.is_empty());
        assert_eq!(c, Some(3003));
    }

    #[test]
    fn only_new_deploy_like_failures_fire() {
        let (e, c) = failures(
            "octo-org/web-app",
            "main",
            &runs(),
            Some(2995),
            &pattern(""),
        );
        assert_eq!(e.len(), 1, "Lint is not deploy-like, 2990 is older");
        assert_eq!(e[0].key, "run:octo-org/web-app:3003");
        assert_eq!(e[0].actor, "jordan");
        assert_eq!(e[0].action, "Deploy to production failed on main");
        assert_eq!(c, Some(3003));
    }

    #[test]
    fn other_branches_are_ignored() {
        let (e, _) = failures("octo-org/web-app", "trunk", &runs(), Some(1), &pattern(""));
        assert!(e.is_empty());
    }

    #[test]
    fn custom_and_invalid_patterns() {
        let (e, _) = failures(
            "octo-org/web-app",
            "main",
            &runs(),
            Some(1),
            &pattern("lint"),
        );
        assert_eq!(e.len(), 1);
        assert!(!pattern_is_valid("(unclosed"));
        // Invalid falls back to the default rather than matching everything.
        let (e, _) = failures(
            "octo-org/web-app",
            "main",
            &runs(),
            Some(1),
            &pattern("(unclosed"),
        );
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn cursor_never_goes_backwards() {
        let (_, c) = failures(
            "octo-org/web-app",
            "main",
            &runs(),
            Some(5000),
            &pattern(""),
        );
        assert_eq!(c, Some(5000));
        let (_, c) = failures("octo-org/web-app", "main", &[], None, &pattern(""));
        assert_eq!(c, None);
    }
}
