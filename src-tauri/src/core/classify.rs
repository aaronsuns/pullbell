//! Turn a GitHub notification (plus the comment/review it points at) into an [`Event`].
//! Pure: no I/O. The GitHub client resolves `latest_comment_url` and passes the JSON in.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::model::{Event, Kind};
use super::settings::CiRuns;
use super::text::{api_to_html, is_bot, snippet, trailing_number, BANNER_TEXT_LEN};

#[derive(Debug, Clone, Deserialize)]
pub struct Notification {
    pub id: String,
    pub reason: String,
    pub updated_at: DateTime<Utc>,
    pub subject: Subject,
    pub repository: Repository,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Subject {
    pub title: String,
    pub url: Option<String>,
    pub latest_comment_url: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repository {
    pub full_name: String,
}

/// What the notification's latest activity turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Activity {
    IssueComment,
    /// A comment in a review thread; `reply` is true when it answers another comment.
    ReviewComment {
        reply: bool,
    },
    Review {
        state: String,
    },
    Unknown,
}

pub fn activity_of(detail: &Value) -> Activity {
    if let Some(state) = detail.get("state").and_then(Value::as_str) {
        if detail.get("submitted_at").is_some() {
            return Activity::Review {
                state: state.to_ascii_uppercase(),
            };
        }
    }
    if detail.get("diff_hunk").is_some() || detail.get("pull_request_review_id").is_some() {
        let reply = detail.get("in_reply_to_id").is_some_and(|v| !v.is_null());
        return Activity::ReviewComment { reply };
    }
    if detail.get("issue_url").is_some() && detail.get("body").is_some() {
        return Activity::IssueComment;
    }
    Activity::Unknown
}

fn actor_of(detail: Option<&Value>) -> Option<(String, Option<String>)> {
    let user = detail?.get("user")?;
    let login = user.get("login")?.as_str()?.to_string();
    let ty = user.get("type").and_then(Value::as_str).map(str::to_string);
    Some((login, ty))
}

/// The notification reason GitHub uses for workflow runs you triggered.
pub const CI_ACTIVITY: &str = "ci_activity";

/// Is this notification reason a CI run notification?
pub fn is_ci_activity(reason: &str) -> bool {
    reason.eq_ignore_ascii_case(CI_ACTIVITY)
}

/// Should a notification with this reason be left out of the list entirely? (It is not marked
/// read on GitHub either; it is simply not ours to handle.)
pub fn ignored(reason: &str, ci_runs: CiRuns) -> bool {
    ci_runs == CiRuns::Ignore && is_ci_activity(reason)
}

/// Classify one notification. `me` is the signed-in login; `detail` is the JSON behind
/// `latest_comment_url` (or the latest review), when it could be fetched.
/// Returns `None` for things that must not create an event (your own actions).
pub fn classify(n: &Notification, detail: Option<&Value>, me: &str) -> Option<Event> {
    let actor = actor_of(detail);
    if let Some((login, _)) = &actor {
        if login.eq_ignore_ascii_case(me) {
            return None;
        }
    }
    let activity = detail.map(activity_of).unwrap_or(Activity::Unknown);
    let bot = actor.as_ref().is_some_and(|(l, t)| is_bot(l, t.as_deref()));

    let (kind, action) = if bot {
        (Kind::Other, fyi_action(&activity))
    } else {
        match (n.reason.as_str(), &activity) {
            ("review_requested", _) => (Kind::ReviewRequested, "requested your review".into()),
            ("mention" | "team_mention", _) => (Kind::Mention, "mentioned you".into()),
            (_, Activity::ReviewComment { reply: true }) if n.reason == "comment" => {
                (Kind::Reply, "replied".into())
            }
            ("author", Activity::Review { state }) => match state.as_str() {
                "APPROVED" => (Kind::Approved, "approved".into()),
                "CHANGES_REQUESTED" => (Kind::ChangesRequested, "requested changes".into()),
                _ => (Kind::Comment, "reviewed".into()),
            },
            ("author", Activity::ReviewComment { reply: true }) => (Kind::Reply, "replied".into()),
            ("author", Activity::ReviewComment { .. } | Activity::IssueComment) => {
                (Kind::Comment, "commented".into())
            }
            _ => (Kind::Other, fyi_action(&activity)),
        }
    };

    let text = detail
        .and_then(|d| d.get("body"))
        .and_then(Value::as_str)
        .map(|b| snippet(b, BANNER_TEXT_LEN))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| snippet(&n.subject.title, BANNER_TEXT_LEN));

    let url = detail
        .and_then(|d| d.get("html_url"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| n.subject.url.as_deref().map(api_to_html))
        .unwrap_or_else(|| format!("https://github.com/{}", n.repository.full_name));

    Some(Event {
        key: format!("n:{}", n.id),
        kind,
        repo: n.repository.full_name.clone(),
        number: n.subject.url.as_deref().and_then(trailing_number),
        title: n.subject.title.clone(),
        actor: actor.map(|(l, _)| l).unwrap_or_default(),
        action,
        text,
        url,
        updated_at: n.updated_at,
        thread_id: Some(n.id.clone()),
        reason: Some(n.reason.clone()),
    })
}

fn fyi_action(activity: &Activity) -> String {
    match activity {
        Activity::IssueComment | Activity::ReviewComment { .. } => "commented",
        Activity::Review { .. } => "reviewed",
        Activity::Unknown => "updated",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }
    fn notif(name: &str) -> Notification {
        serde_json::from_value(fx(name)).unwrap()
    }
    fn with_reason(name: &str, reason: &str) -> Notification {
        let mut n = notif(name);
        n.reason = reason.into();
        n
    }

    #[test]
    fn review_request_without_detail() {
        let e = classify(&notif("notification_review_requested"), None, "octocat").unwrap();
        assert_eq!(e.kind, Kind::ReviewRequested);
        assert_eq!(e.key, "n:1001");
        assert_eq!(e.number, Some(42));
        assert_eq!(e.url, "https://github.com/octo-org/web-app/pull/42");
        assert_eq!(e.text, "Add dark mode toggle");
        assert_eq!(e.thread_id.as_deref(), Some("1001"));
        assert_eq!(e.reason.as_deref(), Some("review_requested"));
    }

    #[test]
    fn human_comment_on_my_pr() {
        let d = fx("issue_comment_sam");
        let e = classify(&notif("notification_author_comment"), Some(&d), "octocat").unwrap();
        assert_eq!(e.kind, Kind::Comment);
        assert_eq!(e.actor, "sam");
        assert_eq!(e.action, "commented");
        assert!(e.text.starts_with("Looks good overall, but could we keep"));
        assert!(e.text.chars().count() <= 120);
        assert!(e.url.ends_with("#issuecomment-555"));
    }

    #[test]
    fn my_own_comment_is_ignored_case_insensitively() {
        let mut d = fx("issue_comment_sam");
        d["user"]["login"] = "OctoCat".into();
        assert!(classify(&notif("notification_author_comment"), Some(&d), "octocat").is_none());
    }

    #[test]
    fn bot_comment_is_fyi() {
        let d = fx("issue_comment_bot");
        let e = classify(&notif("notification_author_comment"), Some(&d), "octocat").unwrap();
        assert_eq!(e.kind, Kind::Other);
        assert_eq!(e.actor, "preview-deployer[bot]");
    }

    #[test]
    fn bot_by_type_even_without_suffix() {
        let mut d = fx("issue_comment_sam");
        d["user"]["type"] = "Bot".into();
        let e = classify(&notif("notification_author_comment"), Some(&d), "octocat").unwrap();
        assert_eq!(e.kind, Kind::Other);
    }

    #[test]
    fn reviews_on_my_pr() {
        let n = notif("notification_author_comment");
        let a = classify(&n, Some(&fx("review_approved_alex")), "octocat").unwrap();
        assert_eq!((a.kind, a.action.as_str()), (Kind::Approved, "approved"));
        // Empty review body falls back to the PR title.
        assert_eq!(a.text, "Speed up the build cache");
        let c = classify(&n, Some(&fx("review_changes_sam")), "octocat").unwrap();
        assert_eq!(c.kind, Kind::ChangesRequested);
        assert_eq!(c.text, "Please add a test for the empty cache.");
    }

    #[test]
    fn reply_in_a_thread_i_took_part_in() {
        let n = with_reason("notification_author_comment", "comment");
        let e = classify(&n, Some(&fx("review_comment_reply_jordan")), "octocat").unwrap();
        assert_eq!(e.kind, Kind::Reply);
        assert_eq!(e.actor, "jordan");
    }

    #[test]
    fn plain_comment_on_someone_elses_thread_is_fyi() {
        let n = with_reason("notification_author_comment", "comment");
        let e = classify(&n, Some(&fx("issue_comment_sam")), "octocat").unwrap();
        assert_eq!(e.kind, Kind::Other);
    }

    #[test]
    fn mention_and_team_mention() {
        let n = notif("notification_mention");
        let mut d = fx("issue_comment_sam");
        d["body"] = "@octocat can you look?".into();
        assert_eq!(
            classify(&n, Some(&d), "octocat").unwrap().kind,
            Kind::Mention
        );
        let t = with_reason("notification_mention", "team_mention");
        assert_eq!(
            classify(&t, Some(&d), "octocat").unwrap().kind,
            Kind::Mention
        );
    }

    #[test]
    fn other_reasons_are_fyi() {
        for reason in [
            "subscribed",
            "state_change",
            "ci_activity",
            "assign",
            "manual",
        ] {
            let n = with_reason("notification_author_comment", reason);
            let e = classify(&n, Some(&fx("issue_comment_sam")), "octocat").unwrap();
            assert_eq!(e.kind, Kind::Other, "{reason}");
        }
    }

    #[test]
    fn ci_activity_is_ignored_by_default_and_fyi_on_request() {
        assert!(is_ci_activity("ci_activity"));
        assert!(is_ci_activity("CI_ACTIVITY"));
        assert!(!is_ci_activity("author"));
        assert!(!is_ci_activity(""));
        assert!(ignored("ci_activity", CiRuns::default()));
        assert!(!ignored("ci_activity", CiRuns::Fyi));
        for reason in ["author", "review_requested", "mention", "subscribed"] {
            assert!(!ignored(reason, CiRuns::Ignore), "{reason}");
        }
        // With FYI chosen, a CI run notification is an ordinary FYI item carrying its reason.
        let mut n = with_reason("notification_author_comment", "ci_activity");
        n.subject.kind = "CheckSuite".into();
        n.subject.url = None;
        n.subject.latest_comment_url = None;
        let e = classify(&n, None, "octocat").unwrap();
        assert_eq!(e.kind, Kind::Other);
        assert_eq!(e.reason.as_deref(), Some("ci_activity"));
        assert_eq!(e.url, "https://github.com/octo-org/web-app");
    }

    #[test]
    fn activity_detection() {
        assert_eq!(
            activity_of(&fx("issue_comment_sam")),
            Activity::IssueComment
        );
        assert_eq!(
            activity_of(&fx("review_comment_reply_jordan")),
            Activity::ReviewComment { reply: true }
        );
        assert_eq!(
            activity_of(&fx("review_approved_alex")),
            Activity::Review {
                state: "APPROVED".into()
            }
        );
        assert_eq!(
            activity_of(&serde_json::json!({"head": {}})),
            Activity::Unknown
        );
    }
}
