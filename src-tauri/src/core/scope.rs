//! Which repositories are watched.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScopeMode {
    #[default]
    Everything,
    Allowlist,
}

/// The resolved scope used during a poll.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scope {
    pub mode: ScopeMode,
    /// Lower-cased `owner/name` (team repos + extra repos).
    pub allowed: BTreeSet<String>,
    /// Lower-cased `owner/name`; always wins.
    pub muted: BTreeSet<String>,
}

impl Scope {
    /// `team_repos` are the repos resolved from the picked teams. When a team lookup failed the
    /// caller simply passes what it has: we never fall back to "everything".
    pub fn build<I, J, K>(mode: ScopeMode, team_repos: I, extra: J, muted: K) -> Self
    where
        I: IntoIterator<Item = String>,
        J: IntoIterator<Item = String>,
        K: IntoIterator<Item = String>,
    {
        let norm = |s: String| normalize_repo(&s);
        Scope {
            mode,
            allowed: team_repos
                .into_iter()
                .chain(extra)
                .filter_map(norm)
                .collect(),
            muted: muted.into_iter().filter_map(norm).collect(),
        }
    }

    pub fn watches(&self, repo: &str) -> bool {
        let r = repo.trim().to_lowercase();
        if self.muted.contains(&r) {
            return false;
        }
        match self.mode {
            ScopeMode::Everything => true,
            ScopeMode::Allowlist => self.allowed.contains(&r),
        }
    }
}

/// Accept `owner/name`, `https://github.com/owner/name(.git)` and surrounding whitespace.
/// Returns lower-case `owner/name`, or `None` if it does not look like a repo.
pub fn normalize_repo(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/').trim_end_matches(".git");
    let s = s
        .strip_prefix("https://github.com/")
        .or_else(|| s.strip_prefix("github.com/"))
        .unwrap_or(s);
    let mut parts = s.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    let ok = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (parts.next().is_none() && ok(owner) && ok(name))
        .then(|| format!("{owner}/{name}").to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn everything_watches_all_but_muted() {
        let sc = Scope::build(
            ScopeMode::Everything,
            vec![],
            vec![],
            s(&["octo-org/Noisy"]),
        );
        assert!(sc.watches("octo-org/web-app"));
        assert!(!sc.watches("OCTO-ORG/noisy"));
    }

    #[test]
    fn allowlist_is_teams_plus_extras_case_insensitive() {
        let sc = Scope::build(
            ScopeMode::Allowlist,
            s(&["octo-org/web-app"]),
            s(&["Octo-Org/API-Server"]),
            vec![],
        );
        assert!(sc.watches("octo-org/WEB-APP"));
        assert!(sc.watches("octo-org/api-server"));
        assert!(!sc.watches("octo-org/other"));
    }

    #[test]
    fn muted_beats_allowlist() {
        let sc = Scope::build(
            ScopeMode::Allowlist,
            s(&["octo-org/web-app"]),
            vec![],
            s(&["octo-org/web-app"]),
        );
        assert!(!sc.watches("octo-org/web-app"));
    }

    #[test]
    fn failed_team_lookup_keeps_only_explicit_repos() {
        // Team lookup failed => caller passes no team repos; must not watch everything.
        let sc = Scope::build(
            ScopeMode::Allowlist,
            vec![],
            s(&["octo-org/web-app"]),
            vec![],
        );
        assert!(sc.watches("octo-org/web-app"));
        assert!(!sc.watches("octo-org/api-server"));
        let empty = Scope::build(ScopeMode::Allowlist, vec![], vec![], vec![]);
        assert!(!empty.watches("octo-org/web-app"));
    }

    #[test]
    fn normalizes_repo_input() {
        assert_eq!(
            normalize_repo(" Octo-Org/Web-App "),
            Some("octo-org/web-app".into())
        );
        assert_eq!(
            normalize_repo("https://github.com/octo-org/web-app.git"),
            Some("octo-org/web-app".into())
        );
        assert_eq!(normalize_repo("octo-org"), None);
        assert_eq!(normalize_repo("a/b/c"), None);
        assert_eq!(normalize_repo("a b/c"), None);
    }
}
