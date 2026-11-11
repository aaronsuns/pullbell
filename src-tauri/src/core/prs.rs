//! Parse PR search results and detect "ready to merge" / "conflict" transitions.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::model::{CiState, Event, Kind, Mergeable, PullRequest, ReviewDecision};

/// Last-seen state of one of my open PRs, persisted so a restart does not re-fire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrPhase {
    pub ready: bool,
    pub conflict: bool,
}

/// Parse the `search.nodes` of a GraphQL PR search. Nodes that are not PRs are skipped.
pub fn parse_search(data: &Value) -> Vec<PullRequest> {
    let nodes = data
        .pointer("/data/search/nodes")
        .or_else(|| data.pointer("/search/nodes"))
        .and_then(Value::as_array);
    nodes.into_iter().flatten().filter_map(parse_node).collect()
}

fn parse_node(n: &Value) -> Option<PullRequest> {
    let s = |p: &str| n.pointer(p).and_then(Value::as_str);
    let review = match s("/reviewDecision") {
        Some("APPROVED") => ReviewDecision::Approved,
        Some("CHANGES_REQUESTED") => ReviewDecision::ChangesRequested,
        Some("REVIEW_REQUIRED") => ReviewDecision::ReviewRequired,
        _ => ReviewDecision::None,
    };
    let mergeable = match s("/mergeable") {
        Some("MERGEABLE") => Mergeable::Mergeable,
        Some("CONFLICTING") => Mergeable::Conflicting,
        _ => Mergeable::Unknown,
    };
    let ci = match s("/commits/nodes/0/commit/statusCheckRollup/state") {
        Some("SUCCESS") => CiState::Green,
        Some("FAILURE" | "ERROR") => CiState::Failing,
        Some("PENDING" | "EXPECTED") => CiState::Running,
        _ => CiState::None,
    };
    Some(PullRequest {
        repo: s("/repository/nameWithOwner")?.to_string(),
        number: n.get("number")?.as_u64()?,
        title: s("/title").unwrap_or_default().to_string(),
        url: s("/url")?.to_string(),
        author: s("/author/login").unwrap_or_default().to_string(),
        is_draft: n.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        review,
        mergeable,
        ci,
        updated_at: s("/updatedAt")
            .and_then(|t| t.parse().ok())
            .unwrap_or_default(),
    })
}

/// Compute the phase of a PR, using `prev` while GitHub is still computing mergeability.
pub fn phase_of(pr: &PullRequest, prev: Option<PrPhase>) -> PrPhase {
    let prev = prev.unwrap_or_default();
    let conflict = match pr.mergeable {
        Mergeable::Conflicting => true,
        Mergeable::Mergeable => false,
        Mergeable::Unknown => prev.conflict,
    };
    let others_ok =
        !pr.is_draft && pr.review == ReviewDecision::Approved && pr.ci == CiState::Green;
    let ready = others_ok
        && match pr.mergeable {
            Mergeable::Mergeable => true,
            Mergeable::Conflicting => false,
            Mergeable::Unknown => prev.ready,
        };
    PrPhase { ready, conflict }
}

/// Compare my open PRs against their last-seen phases. Returns the events for PRs that just
/// became ready or just got a conflict, plus the new phase map (closed PRs drop out).
pub fn transitions(
    prev: &BTreeMap<String, PrPhase>,
    prs: &[PullRequest],
    now: DateTime<Utc>,
) -> (Vec<Event>, BTreeMap<String, PrPhase>) {
    let mut events = Vec::new();
    let mut next = BTreeMap::new();
    for pr in prs {
        let key = pr.key();
        let before = prev.get(&key).copied();
        let after = phase_of(pr, before);
        let before = before.unwrap_or_default();
        if after.ready && !before.ready {
            events.push(pr_event(pr, Kind::ReadyToMerge, "is ready to merge", now));
        }
        if after.conflict && !before.conflict {
            events.push(pr_event(pr, Kind::Conflict, "has a merge conflict", now));
        }
        next.insert(key, after);
    }
    (events, next)
}

fn pr_event(pr: &PullRequest, kind: Kind, action: &str, now: DateTime<Utc>) -> Event {
    let suffix = if kind == Kind::ReadyToMerge {
        "ready"
    } else {
        "conflict"
    };
    Event {
        key: format!("pr:{}:{suffix}", pr.key()),
        kind,
        repo: pr.repo.clone(),
        number: Some(pr.number),
        title: pr.title.clone(),
        actor: format!("#{}", pr.number),
        action: action.into(),
        text: pr.title.clone(),
        url: pr.url.clone(),
        updated_at: now,
        thread_id: None,
        reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<PullRequest> {
        let path = format!(
            "{}/tests/fixtures/graphql_my_prs.json",
            env!("CARGO_MANIFEST_DIR")
        );
        parse_search(&serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap())
    }
    fn now() -> DateTime<Utc> {
        "2026-03-02T15:00:00Z".parse().unwrap()
    }

    #[test]
    fn parses_search_nodes() {
        let prs = fixture();
        assert_eq!(prs.len(), 3);
        assert_eq!(prs[0].key(), "octo-org/web-app#7");
        assert_eq!(prs[0].ci, CiState::Green);
        assert_eq!(prs[1].mergeable, Mergeable::Conflicting);
        assert_eq!(prs[1].ci, CiState::Failing);
        assert!(prs[2].is_draft);
        assert_eq!(prs[2].ci, CiState::None);
        assert_eq!(
            prs[0].row(),
            "🟢✅ octo-org/web-app#7 Speed up the build cache"
        );
        assert_eq!(prs[1].review_glyph(), "⚠️");
        assert_eq!(prs[2].review_glyph(), "📝");
    }

    #[test]
    fn first_sight_fires_ready_and_conflict() {
        let (events, next) = transitions(&BTreeMap::new(), &fixture(), now());
        let kinds: Vec<_> = events.iter().map(|e| (e.key.as_str(), e.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                ("pr:octo-org/web-app#7:ready", Kind::ReadyToMerge),
                ("pr:octo-org/web-app#12:conflict", Kind::Conflict)
            ]
        );
        assert_eq!(next.len(), 3);
    }

    #[test]
    fn same_state_again_does_not_refire() {
        let (_, state) = transitions(&BTreeMap::new(), &fixture(), now());
        let (events, again) = transitions(&state, &fixture(), now());
        assert!(events.is_empty());
        assert_eq!(state, again);
    }

    #[test]
    fn unknown_mergeability_keeps_previous_phase() {
        let mut prs = fixture();
        let (_, state) = transitions(&BTreeMap::new(), &prs, now());
        prs[0].mergeable = Mergeable::Unknown;
        prs[1].mergeable = Mergeable::Unknown;
        let (events, next) = transitions(&state, &prs, now());
        assert!(events.is_empty());
        assert!(next["octo-org/web-app#7"].ready);
        assert!(next["octo-org/web-app#12"].conflict);
    }

    #[test]
    fn leaving_and_reentering_ready_fires_again() {
        let mut prs = fixture();
        let (_, s1) = transitions(&BTreeMap::new(), &prs, now());
        prs[0].ci = CiState::Running;
        let (e2, s2) = transitions(&s1, &prs, now());
        assert!(e2.is_empty());
        assert!(!s2["octo-org/web-app#7"].ready);
        prs[0].ci = CiState::Green;
        let (e3, _) = transitions(&s2, &prs, now());
        assert_eq!(e3.len(), 1);
        assert_eq!(e3[0].kind, Kind::ReadyToMerge);
    }

    #[test]
    fn draft_or_unapproved_is_never_ready() {
        let mut pr = fixture()[0].clone();
        pr.is_draft = true;
        assert!(!phase_of(&pr, None).ready);
        pr.is_draft = false;
        pr.review = ReviewDecision::ReviewRequired;
        assert!(!phase_of(&pr, None).ready);
    }

    #[test]
    fn closed_prs_drop_out_of_state() {
        let prs = fixture();
        let (_, s1) = transitions(&BTreeMap::new(), &prs, now());
        let (_, s2) = transitions(&s1, &prs[..1], now());
        assert_eq!(s2.len(), 1);
    }
}
