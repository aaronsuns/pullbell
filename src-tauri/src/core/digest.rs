//! Daily digest of review requests still waiting on you.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Timelike, Utc};

use super::model::{Event, Kind, PullRequest};
use super::text::{snippet, BANNER_TEXT_LEN};

pub const DIGEST_HOURS: std::ops::RangeInclusive<u8> = 6..=12;

/// Is a digest due? `local_now` is wall-clock time in the user's zone, `hour` the chosen hour
/// (None = off) and `last` the local date of the last digest (sent or skipped).
pub fn due(local_now: NaiveDateTime, hour: Option<u8>, last: Option<NaiveDate>) -> bool {
    let Some(hour) = hour else { return false };
    local_now.hour() >= u32::from(hour) && last != Some(local_now.date())
}

/// Build the digest event. `None` when nothing is waiting (no banner on quiet days).
pub fn build(waiting: &[PullRequest], date: NaiveDate, now: DateTime<Utc>) -> Option<Event> {
    if waiting.is_empty() {
        return None;
    }
    let n = waiting.len();
    let list = waiting
        .iter()
        .map(|p| format!("{}#{}", p.repo, p.number))
        .collect::<Vec<_>>()
        .join(", ");
    Some(Event {
        key: format!("digest:{date}"),
        kind: Kind::Digest,
        repo: "Daily digest".into(),
        number: None,
        title: format!(
            "{n} review request{} waiting",
            if n == 1 { "" } else { "s" }
        ),
        actor: format!("{n}"),
        action: if n == 1 {
            "review waiting on you".into()
        } else {
            "reviews waiting on you".into()
        },
        text: snippet(&list, BANNER_TEXT_LEN),
        url: "https://github.com/pulls/review-requested".into(),
        updated_at: now,
        thread_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{CiState, Mergeable, ReviewDecision};

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }
    fn pr(n: u64) -> PullRequest {
        PullRequest {
            repo: "octo-org/web-app".into(),
            number: n,
            title: "t".into(),
            url: "u".into(),
            author: "alex".into(),
            is_draft: false,
            review: ReviewDecision::ReviewRequired,
            mergeable: Mergeable::Mergeable,
            ci: CiState::Green,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn due_once_per_day_after_the_hour() {
        let d = at("2026-03-02 09:00").date();
        assert!(!due(at("2026-03-02 08:59"), Some(9), None));
        assert!(due(at("2026-03-02 09:00"), Some(9), None));
        assert!(due(
            at("2026-03-02 17:00"),
            Some(9),
            Some(d.pred_opt().unwrap())
        ));
        assert!(!due(at("2026-03-02 10:00"), Some(9), Some(d)));
        assert!(!due(at("2026-03-02 10:00"), None, None));
    }

    #[test]
    fn builds_one_item_or_nothing() {
        let date = at("2026-03-02 09:00").date();
        assert!(build(&[], date, Utc::now()).is_none());
        let e = build(&[pr(1), pr(2)], date, Utc::now()).unwrap();
        assert_eq!(e.key, "digest:2026-03-02");
        assert_eq!(e.kind, Kind::Digest);
        assert_eq!(e.text, "octo-org/web-app#1, octo-org/web-app#2");
        assert_eq!(e.title, "2 review requests waiting");
    }
}
