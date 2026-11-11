//! The inbox: persisted state, upsert of new events, done-marking, banners and menu text.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use super::model::{Event, Item};
use super::prs::PrPhase;
use super::settings::Settings;
use super::text::snippet;

/// How long a done-marker is remembered (so a stale notification does not come back).
const DONE_TTL_DAYS: i64 = 30;
const MAX_ITEMS: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Newest first.
    pub items: Vec<Item>,
    /// key → updated_at of the item when it was marked done.
    pub done: BTreeMap<String, DateTime<Utc>>,
    pub pr_phases: BTreeMap<String, PrPhase>,
    pub notifications_last_modified: Option<String>,
    /// repo → highest workflow run id seen.
    pub run_cursors: BTreeMap<String, u64>,
    pub default_branches: BTreeMap<String, String>,
    pub last_digest: Option<NaiveDate>,
    /// Login the state belongs to; a different user starts from scratch.
    pub login: Option<String>,
    /// False until the first sync after sign-in has completed (that sync is silent).
    pub synced_once: bool,
}

impl State {
    /// Insert or update items. Returns the events that are new or newer than what we had
    /// (candidates for a banner).
    pub fn apply(&mut self, events: Vec<Event>) -> Vec<Event> {
        let mut fresh = Vec::new();
        for e in events {
            if self.done.get(&e.key).is_some_and(|d| *d >= e.updated_at) {
                continue;
            }
            if let Some(pos) = self.items.iter().position(|i| i.key == e.key) {
                if self.items[pos].updated_at >= e.updated_at {
                    continue;
                }
                self.items.remove(pos);
            }
            self.items.push(e.clone());
            fresh.push(e);
        }
        self.items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        self.items.truncate(MAX_ITEMS);
        fresh
    }

    pub fn mark_done(&mut self, key: &str) -> Option<Item> {
        let pos = self.items.iter().position(|i| i.key == key)?;
        let item = self.items.remove(pos);
        self.done.insert(item.key.clone(), item.updated_at);
        Some(item)
    }

    pub fn clear_all(&mut self) -> Vec<Item> {
        let items = std::mem::take(&mut self.items);
        for i in &items {
            self.done.insert(i.key.clone(), i.updated_at);
        }
        items
    }

    pub fn prune(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::days(DONE_TTL_DAYS);
        self.done.retain(|_, t| *t >= cutoff);
    }

    /// (needs-you, FYI) counts.
    pub fn counts(&self, settings: &Settings) -> (usize, usize) {
        let n = self
            .items
            .iter()
            .filter(|i| settings.needs_you(i.kind))
            .count();
        (n, self.items.len() - n)
    }

    /// Needs-you first, then FYI; newest first within each.
    pub fn sorted(&self, settings: &Settings) -> Vec<Item> {
        let mut v = self.items.clone();
        v.sort_by(|a, b| {
            settings
                .needs_you(b.kind)
                .cmp(&settings.needs_you(a.kind))
                .then(b.updated_at.cmp(&a.updated_at))
        });
        v
    }
}

/// Menu bar title: `n+m`, or `0` when empty.
pub fn title(n: usize, m: usize) -> String {
    match (n, m) {
        (0, 0) => "0".into(),
        (n, 0) => n.to_string(),
        (n, m) => format!("{n}+{m}"),
    }
}

/// `emoji repo · actor action: text`, cut for a menu row.
pub fn row(item: &Item, max: usize) -> String {
    let who = if item.actor.is_empty() {
        item.action.clone()
    } else {
        format!("{} {}", item.actor, item.action)
    };
    let full = if item.text.is_empty() {
        format!("{} {} · {who}", item.kind.emoji(), item.repo)
    } else {
        format!("{} {} · {who}: {}", item.kind.emoji(), item.repo, item.text)
    };
    snippet(&full, max)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Banner {
    pub title: String,
    pub body: String,
    pub url: String,
    /// Items to mark done when the banner is clicked.
    pub keys: Vec<String>,
}

/// One banner per PR/thread for the fresh needs-you events of this poll.
pub fn banners(fresh: &[Event], settings: &Settings) -> Vec<Banner> {
    let mut groups: BTreeMap<String, Vec<&Event>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for e in fresh.iter().filter(|e| settings.needs_you(e.kind)) {
        let g = match e.number {
            Some(n) => format!("{}#{n}", e.repo.to_lowercase()),
            None => e.key.clone(),
        };
        if !groups.contains_key(&g) {
            order.push(g.clone());
        }
        groups.entry(g).or_default().push(e);
    }
    order
        .into_iter()
        .map(|g| {
            let mut evs = groups.remove(&g).unwrap_or_default();
            evs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            let newest = evs[0];
            let place = match newest.number {
                Some(n) => format!("{}#{n}", newest.repo),
                None => newest.repo.clone(),
            };
            let who = if newest.actor.is_empty() {
                newest.action.clone()
            } else {
                format!("{} {}", newest.actor, newest.action)
            };
            let mut body = if newest.text.is_empty() {
                who
            } else {
                format!("{who}: {}", newest.text)
            };
            let title = if evs.len() > 1 {
                body.push_str(&format!(" (+{} more)", evs.len() - 1));
                format!("{} {place} · {} updates", newest.kind.emoji(), evs.len())
            } else {
                format!("{} {place}", newest.kind.emoji())
            };
            Banner {
                title,
                body,
                url: newest.url.clone(),
                keys: evs.iter().map(|e| e.key.clone()).collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::Kind;

    fn ev(key: &str, kind: Kind, num: Option<u64>, t: &str) -> Event {
        Event {
            key: key.into(),
            kind,
            repo: "octo-org/web-app".into(),
            number: num,
            title: "Speed up the build cache".into(),
            actor: "sam".into(),
            action: "commented".into(),
            text: format!("text {key} {t}"),
            url: format!(
                "https://github.com/octo-org/web-app/pull/{}",
                num.unwrap_or(0)
            ),
            updated_at: format!("2026-03-02T{t}:00Z").parse().unwrap(),
            thread_id: None,
        }
    }

    #[test]
    fn newer_event_for_same_thread_updates_in_place() {
        let mut s = State::default();
        s.apply(vec![
            ev("n:1", Kind::Comment, Some(7), "10:00"),
            ev("n:2", Kind::Other, None, "11:00"),
        ]);
        let fresh = s.apply(vec![ev("n:1", Kind::Comment, Some(7), "12:00")]);
        assert_eq!(fresh.len(), 1);
        assert_eq!(s.items.len(), 2);
        assert_eq!(s.items[0].key, "n:1", "moved to the top");
        assert!(s.items[0].text.contains("12:00"));
    }

    #[test]
    fn same_or_older_event_is_not_fresh() {
        let mut s = State::default();
        s.apply(vec![ev("n:1", Kind::Comment, Some(7), "10:00")]);
        assert!(s
            .apply(vec![ev("n:1", Kind::Comment, Some(7), "10:00")])
            .is_empty());
        assert!(s
            .apply(vec![ev("n:1", Kind::Comment, Some(7), "09:00")])
            .is_empty());
    }

    #[test]
    fn done_items_stay_done_until_something_newer() {
        let mut s = State::default();
        s.apply(vec![ev("n:1", Kind::Comment, Some(7), "10:00")]);
        assert!(s.mark_done("n:1").is_some());
        assert!(s.items.is_empty());
        assert!(s
            .apply(vec![ev("n:1", Kind::Comment, Some(7), "10:00")])
            .is_empty());
        assert_eq!(
            s.apply(vec![ev("n:1", Kind::Comment, Some(7), "10:05")])
                .len(),
            1
        );
        assert!(s.mark_done("missing").is_none());
    }

    #[test]
    fn clear_all_and_prune() {
        let mut s = State::default();
        s.apply(vec![
            ev("n:1", Kind::Comment, Some(7), "10:00"),
            ev("n:2", Kind::Approved, Some(8), "10:01"),
        ]);
        assert_eq!(s.clear_all().len(), 2);
        assert_eq!(s.done.len(), 2);
        s.prune("2026-05-01T00:00:00Z".parse().unwrap());
        assert!(s.done.is_empty());
    }

    #[test]
    fn counts_follow_banner_settings() {
        let mut s = State::default();
        s.apply(vec![
            ev("n:1", Kind::Comment, Some(7), "10:00"),
            ev("n:2", Kind::Other, None, "10:01"),
        ]);
        let mut set = Settings::default();
        assert_eq!(s.counts(&set), (1, 1));
        set.alerts.banners.insert(Kind::Comment, false);
        assert_eq!(
            s.counts(&set),
            (0, 2),
            "turned-off kind becomes FYI, not dropped"
        );
    }

    #[test]
    fn sorted_puts_needs_you_first() {
        let mut s = State::default();
        s.apply(vec![
            ev("n:1", Kind::Comment, Some(7), "10:00"),
            ev("n:2", Kind::Other, None, "11:00"),
        ]);
        let keys: Vec<_> = s
            .sorted(&Settings::default())
            .into_iter()
            .map(|i| i.key)
            .collect();
        assert_eq!(keys, vec!["n:1", "n:2"]);
    }

    #[test]
    fn title_and_row() {
        assert_eq!(title(0, 0), "0");
        assert_eq!(title(2, 0), "2");
        assert_eq!(title(0, 3), "0+3");
        assert_eq!(title(2, 5), "2+5");
        let e = ev("n:1", Kind::Comment, Some(7), "10:00");
        assert_eq!(
            row(&e, 200),
            "💬 octo-org/web-app · sam commented: text n:1 10:00"
        );
        assert!(row(&e, 20).chars().count() <= 20);
    }

    #[test]
    fn banners_group_per_pr_and_skip_fyi() {
        let fresh = vec![
            ev("n:1", Kind::Comment, Some(7), "10:00"),
            ev("n:3", Kind::Approved, Some(7), "10:02"),
            ev("n:2", Kind::Other, Some(9), "10:01"),
            ev("run:x:1", Kind::DeployFailure, None, "10:03"),
        ];
        let b = banners(&fresh, &Settings::default());
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].title, "✅ octo-org/web-app#7 · 2 updates");
        assert!(b[0].body.ends_with("(+1 more)"));
        assert_eq!(b[0].keys, vec!["n:3", "n:1"]);
        assert_eq!(b[1].title, "🔥 octo-org/web-app");
    }

    #[test]
    fn banners_respect_per_kind_setting() {
        let mut set = Settings::default();
        set.alerts.banners.insert(Kind::Comment, false);
        assert!(banners(&[ev("n:1", Kind::Comment, Some(7), "10:00")], &set).is_empty());
    }
}
