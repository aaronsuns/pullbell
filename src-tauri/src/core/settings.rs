//! User settings: defaults, lenient loading and migration.
//!
//! Loading never fails. Unknown fields are dropped, missing or wrongly-typed fields fall back
//! to their defaults one by one (a single bad value does not reset everything), and values are
//! clamped to what the UI offers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::digest::DIGEST_HOURS;
use super::model::Kind;
use super::runs::{pattern_is_valid, DEFAULT_PATTERN};
use super::scope::{normalize_repo, ScopeMode};
use super::shortcut;

pub const CURRENT_VERSION: u32 = 1;
pub const POLL_CHOICES: [u32; 4] = [1, 3, 5, 10];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub repos: RepoSettings,
    pub alerts: AlertSettings,
    pub shortcuts: Shortcuts,
    pub poll_minutes: u32,
    pub start_at_login: bool,
    pub updates: UpdateSettings,
    pub browser: Browser,
    pub welcome_done: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoSettings {
    pub mode: ScopeMode,
    /// `org/team-slug`
    pub teams: Vec<String>,
    pub extra: Vec<String>,
    pub muted: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertSettings {
    /// Banner on/off per kind. Missing kinds use [`Kind::default_needs_you`].
    pub banners: BTreeMap<Kind, bool>,
    pub digest_hour: Option<u8>,
    pub failure_pattern: String,
    /// What to do with `ci_activity` notifications (one per workflow run you triggered).
    /// Settings files written before this field existed load as [`CiRuns::Ignore`].
    pub ci_runs: CiRuns,
}

/// Settings → Alerts → CI run notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiRuns {
    /// Show them as FYI items, like any other notification.
    Fyi,
    /// Leave them unread on GitHub and keep them out of the list. Unknown values load as this.
    #[default]
    #[serde(other)]
    Ignore,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    pub queue: Option<String>,
    pub my_prs: Option<String>,
    pub refresh: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    pub check: bool,
    pub auto_install: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Browser {
    #[default]
    System,
    /// A browser app by name, e.g. "Firefox" or "Safari".
    App { name: String },
    /// A Chrome profile directory, e.g. "Profile 1".
    ChromeProfile { dir: String, label: String },
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: CURRENT_VERSION,
            repos: RepoSettings::default(),
            alerts: AlertSettings::default(),
            shortcuts: Shortcuts::default(),
            poll_minutes: 3,
            start_at_login: true,
            updates: UpdateSettings::default(),
            browser: Browser::System,
            welcome_done: false,
        }
    }
}

impl Default for AlertSettings {
    fn default() -> Self {
        AlertSettings {
            banners: Kind::ALL
                .iter()
                .map(|k| (*k, k.default_needs_you()))
                .collect(),
            digest_hour: None,
            failure_pattern: DEFAULT_PATTERN.into(),
            ci_runs: CiRuns::Ignore,
        }
    }
}

impl Default for Shortcuts {
    fn default() -> Self {
        Shortcuts {
            queue: Some("Ctrl+Cmd+G".into()),
            my_prs: Some("Ctrl+Cmd+P".into()),
            refresh: Some("Ctrl+Shift+Cmd+G".into()),
        }
    }
}

impl Default for UpdateSettings {
    fn default() -> Self {
        UpdateSettings {
            check: true,
            auto_install: true,
        }
    }
}

impl Settings {
    /// Is this kind "needs you" (banner on)?
    pub fn needs_you(&self, kind: Kind) -> bool {
        self.alerts
            .banners
            .get(&kind)
            .copied()
            .unwrap_or_else(|| kind.default_needs_you())
    }

    /// Load from raw file contents. Returns the settings and whether the input was unusable
    /// (so the caller can move the file aside).
    pub fn from_json(raw: &str) -> (Settings, bool) {
        match serde_json::from_str::<Value>(raw) {
            Ok(v @ Value::Object(_)) => (migrate(v), false),
            _ => (Settings::default(), true),
        }
    }

    /// Clamp every field to a value the app can use.
    pub fn normalized(mut self) -> Settings {
        self.version = CURRENT_VERSION;
        if !POLL_CHOICES.contains(&self.poll_minutes) {
            // Snap to the nearest offered interval.
            self.poll_minutes = *POLL_CHOICES
                .iter()
                .min_by_key(|c| (i64::from(**c) - i64::from(self.poll_minutes)).abs())
                .expect("non-empty");
        }
        if let Some(h) = self.alerts.digest_hour {
            if !DIGEST_HOURS.contains(&h) {
                self.alerts.digest_hour = None;
            }
        }
        if !pattern_is_valid(&self.alerts.failure_pattern)
            || self.alerts.failure_pattern.trim().is_empty()
        {
            self.alerts.failure_pattern = DEFAULT_PATTERN.into();
        }
        for k in Kind::ALL {
            self.alerts
                .banners
                .entry(k)
                .or_insert_with(|| k.default_needs_you());
        }
        let clean = |v: &mut Vec<String>| {
            let mut out: Vec<String> = v.iter().filter_map(|r| normalize_repo(r)).collect();
            out.sort();
            out.dedup();
            *v = out;
        };
        clean(&mut self.repos.extra);
        clean(&mut self.repos.muted);
        self.repos.teams.retain(|t| t.contains('/'));
        self.repos.teams.dedup();
        // Drop unparsable or duplicate shortcuts rather than registering garbage.
        let mut seen: Vec<String> = Vec::new();
        for slot in [
            &mut self.shortcuts.queue,
            &mut self.shortcuts.my_prs,
            &mut self.shortcuts.refresh,
        ] {
            let others: Vec<&str> = seen.iter().map(String::as_str).collect();
            *slot = slot
                .as_deref()
                .and_then(|s| shortcut::validate(s, &others).ok());
            if let Some(s) = slot {
                seen.push(s.clone());
            }
        }
        self
    }
}

/// Bring an older or partially broken settings object up to date.
pub fn migrate(mut v: Value) -> Settings {
    let obj = v.as_object_mut().expect("caller checked object");
    let version = obj.get("version").and_then(Value::as_u64).unwrap_or(0);
    if version == 0 {
        // Pre-1 files stored the interval in seconds and a boolean for the scope.
        if let Some(secs) = obj.remove("poll_seconds").and_then(|s| s.as_u64()) {
            obj.insert("poll_minutes".into(), (secs / 60).max(1).into());
        }
        if let Some(all) = obj.remove("watch_all").and_then(|b| b.as_bool()) {
            let repos = obj
                .entry("repos")
                .or_insert_with(|| Value::Object(Default::default()));
            if let Some(r) = repos.as_object_mut() {
                r.insert(
                    "mode".into(),
                    (if all { "everything" } else { "allowlist" }).into(),
                );
            }
        }
    }
    let defaults = serde_json::to_value(Settings::default()).expect("defaults serialize");
    let merged = merge_typed(defaults, v);
    serde_json::from_value::<Settings>(merged)
        .unwrap_or_default()
        .normalized()
}

/// Overlay `user` on `base`, keeping the base wherever the user's value has a different JSON
/// type. Maps the defaults leave open (e.g. per-kind banners) accept any user keys. Values that
/// still fail to deserialize are caught by a per-field retry in [`fix_fields`].
fn merge_typed(base: Value, user: Value) -> Value {
    let merged = merge_value(base.clone(), user);
    fix_fields(base, merged)
}

fn merge_value(base: Value, user: Value) -> Value {
    match (base, user) {
        (Value::Object(mut b), Value::Object(u)) => {
            for (k, uv) in u {
                let nv = match b.remove(&k) {
                    Some(bv) => merge_value(bv, uv),
                    None => uv,
                };
                b.insert(k, nv);
            }
            Value::Object(b)
        }
        (Value::Null, u) => u,
        (b, Value::Null) if !b.is_null() => {
            // `null` is meaningful for optional fields (digest off, shortcut cleared).
            Value::Null
        }
        (b, u) if same_type(&b, &u) => u,
        (b, _) => b,
    }
}

fn same_type(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// Retry each top-level field on its own; a field that will not deserialize gets its default.
fn fix_fields(base: Value, merged: Value) -> Value {
    if serde_json::from_value::<Settings>(merged.clone()).is_ok() {
        return merged;
    }
    let (Value::Object(b), Value::Object(m)) = (base.clone(), merged) else {
        return base;
    };
    let mut out = b.clone();
    for (k, v) in m {
        let mut trial = b.clone();
        trial.insert(k.clone(), v.clone());
        if serde_json::from_value::<Settings>(Value::Object(trial)).is_ok() {
            out.insert(k, v);
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_the_spec() {
        let s = Settings::default();
        assert_eq!(s.poll_minutes, 3);
        assert!(s.start_at_login);
        assert_eq!(s.repos.mode, ScopeMode::Everything);
        assert!(s.updates.auto_install);
        assert_eq!(s.alerts.failure_pattern, "deploy|release|publish");
        assert_eq!(s.alerts.digest_hour, None);
        assert_eq!(s.alerts.ci_runs, CiRuns::Ignore);
        assert!(s.needs_you(Kind::ReviewRequested));
        assert!(!s.needs_you(Kind::Other));
        assert_eq!(s.shortcuts.queue.as_deref(), Some("Ctrl+Cmd+G"));
        assert_eq!(s.shortcuts.my_prs.as_deref(), Some("Ctrl+Cmd+P"));
        assert_eq!(s.shortcuts.refresh.as_deref(), Some("Ctrl+Shift+Cmd+G"));
    }

    #[test]
    fn round_trips() {
        let mut s = Settings::default();
        s.alerts.digest_hour = Some(9);
        s.browser = Browser::ChromeProfile {
            dir: "Profile 1".into(),
            label: "Work".into(),
        };
        let raw = serde_json::to_string(&s).unwrap();
        let (back, corrupt) = Settings::from_json(&raw);
        assert!(!corrupt);
        assert_eq!(back, s);
    }

    #[test]
    fn corrupt_input_gives_defaults_and_flags_it() {
        for raw in ["{not json", "[]", "42", ""] {
            let (s, corrupt) = Settings::from_json(raw);
            assert!(corrupt, "{raw}");
            assert_eq!(s, Settings::default());
        }
    }

    #[test]
    fn one_bad_field_does_not_reset_the_rest() {
        let raw = json!({
            "version": 1,
            "poll_minutes": "fast",
            "start_at_login": false,
            "alerts": { "digest_hour": 9, "banners": { "comment": false } },
            "browser": { "type": "warp_drive" },
            "unknown_field": 1
        })
        .to_string();
        let (s, corrupt) = Settings::from_json(&raw);
        assert!(!corrupt);
        assert_eq!(s.poll_minutes, 3);
        assert!(!s.start_at_login);
        assert_eq!(s.alerts.digest_hour, Some(9));
        assert!(!s.needs_you(Kind::Comment));
        assert!(s.needs_you(Kind::Approved), "missing kinds get defaults");
        assert_eq!(s.browser, Browser::System);
    }

    #[test]
    fn clamps_out_of_range_values() {
        let raw = json!({
            "poll_minutes": 4,
            "alerts": { "digest_hour": 23, "failure_pattern": "(bad" },
            "repos": { "extra": ["Octo-Org/Web-App", "octo-org/web-app", "nonsense"] },
            "shortcuts": { "queue": "G", "my_prs": "Ctrl+Cmd+P", "refresh": "Control+Command+P" }
        })
        .to_string();
        let (s, _) = Settings::from_json(&raw);
        assert_eq!(s.poll_minutes, 3);
        assert_eq!(s.alerts.digest_hour, None);
        assert_eq!(s.alerts.failure_pattern, DEFAULT_PATTERN);
        assert_eq!(s.repos.extra, vec!["octo-org/web-app".to_string()]);
        assert_eq!(s.shortcuts.queue, None);
        assert_eq!(s.shortcuts.my_prs.as_deref(), Some("Ctrl+Cmd+P"));
        assert_eq!(s.shortcuts.refresh, None, "duplicate dropped");
    }

    #[test]
    fn null_clears_optional_values() {
        let raw = json!({ "shortcuts": { "queue": null } }).to_string();
        let (s, _) = Settings::from_json(&raw);
        assert_eq!(s.shortcuts.queue, None);
        assert_eq!(s.shortcuts.my_prs.as_deref(), Some("Ctrl+Cmd+P"));
    }

    #[test]
    fn ci_runs_defaults_to_ignore_for_existing_files() {
        // A file saved before the setting existed.
        let raw = json!({
            "version": 1,
            "alerts": { "digest_hour": 9, "failure_pattern": "deploy", "banners": { "other": true } }
        })
        .to_string();
        let (s, corrupt) = Settings::from_json(&raw);
        assert!(!corrupt);
        assert_eq!(s.alerts.ci_runs, CiRuns::Ignore);
        assert_eq!(s.alerts.digest_hour, Some(9), "rest of alerts kept");
        assert!(s.needs_you(Kind::Other));
    }

    #[test]
    fn ci_runs_round_trips_and_bad_values_fall_back() {
        let raw = json!({ "alerts": { "ci_runs": "fyi" } }).to_string();
        assert_eq!(Settings::from_json(&raw).0.alerts.ci_runs, CiRuns::Fyi);
        let raw = json!({ "alerts": { "ci_runs": "loud", "digest_hour": 8 } }).to_string();
        let s = Settings::from_json(&raw).0;
        assert_eq!(s.alerts.ci_runs, CiRuns::Ignore);
        assert_eq!(
            s.alerts.digest_hour,
            Some(8),
            "one bad field keeps the rest of alerts"
        );
    }

    #[test]
    fn migrates_version_zero() {
        let raw = json!({ "poll_seconds": 600, "watch_all": false }).to_string();
        let (s, _) = Settings::from_json(&raw);
        assert_eq!(s.version, CURRENT_VERSION);
        assert_eq!(s.poll_minutes, 10);
        assert_eq!(s.repos.mode, ScopeMode::Allowlist);
    }
}
