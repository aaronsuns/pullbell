//! Small pure policies: update retry, Chrome profiles, browser open commands.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::settings::Browser;

pub const UPDATE_CHECK_HOURS: i64 = 6;

/// Remembers the last failed update attempt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateAttempt {
    pub version: String,
    pub at: DateTime<Utc>,
}

/// May we try to install `version` now? A failed version waits 6 h; a different one goes at once.
pub fn may_attempt_update(
    version: &str,
    last_failed: Option<&UpdateAttempt>,
    now: DateTime<Utc>,
) -> bool {
    match last_failed {
        Some(f) if f.version == version => now - f.at >= Duration::hours(UPDATE_CHECK_HOURS),
        _ => true,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChromeProfile {
    pub dir: String,
    pub name: String,
    pub email: String,
}

/// Read profiles from the JSON of Chrome's `Local State` file (`profile.info_cache`).
pub fn chrome_profiles(local_state: &Value) -> Vec<ChromeProfile> {
    let mut out: Vec<ChromeProfile> = local_state
        .pointer("/profile/info_cache")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(dir, info)| {
                    let s = |k: &str| {
                        info.get(k)
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string()
                    };
                    ChromeProfile {
                        dir: dir.clone(),
                        name: s("name"),
                        email: s("user_name"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

/// The `open` command line to show `url` in the chosen browser.
pub fn open_command(browser: &Browser, url: &str) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match browser {
        Browser::System => v(&["open", url]),
        Browser::App { name } => v(&["open", "-a", name, url]),
        Browser::ChromeProfile { dir, .. } => {
            let profile = format!("--profile-directory={dir}");
            v(&["open", "-na", "Google Chrome", "--args", &profile, url])
        }
    }
}

/// Only http(s) links are ever opened.
pub fn is_openable(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn update_retry_policy() {
        let now: DateTime<Utc> = "2026-03-02T12:00:00Z".parse().unwrap();
        let failed = UpdateAttempt {
            version: "0.2.0".into(),
            at: now - Duration::hours(1),
        };
        assert!(!may_attempt_update("0.2.0", Some(&failed), now));
        assert!(may_attempt_update(
            "0.2.0",
            Some(&failed),
            now + Duration::hours(5)
        ));
        assert!(
            may_attempt_update("0.2.1", Some(&failed), now),
            "newer version at once"
        );
        assert!(may_attempt_update("0.2.0", None, now));
    }

    #[test]
    fn reads_chrome_profiles() {
        let ls = json!({"profile": {"info_cache": {
            "Profile 1": {"name": "Work", "user_name": "alex@example.invalid"},
            "Default": {"name": "Personal"}
        }}});
        let p = chrome_profiles(&ls);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].dir, "Default");
        assert_eq!(p[0].email, "");
        assert_eq!(p[1].name, "Work");
        assert!(chrome_profiles(&json!({})).is_empty());
    }

    #[test]
    fn open_commands() {
        let url = "https://github.com/octo-org/web-app/pull/7";
        assert_eq!(open_command(&Browser::System, url), vec!["open", url]);
        assert_eq!(
            open_command(
                &Browser::App {
                    name: "Firefox".into()
                },
                url
            ),
            vec!["open", "-a", "Firefox", url]
        );
        let c = open_command(
            &Browser::ChromeProfile {
                dir: "Profile 1".into(),
                label: "Work".into(),
            },
            url,
        );
        assert_eq!(c[4], "--profile-directory=Profile 1");
        assert!(is_openable(url));
        assert!(!is_openable("file:///etc/passwd"));
    }
}
