//! Small text and URL helpers shared by the classifiers.

pub const BANNER_TEXT_LEN: usize = 120;

/// Collapse whitespace and cut to `max` characters (not bytes), adding `…` when cut.
pub fn snippet(body: &str, max: usize) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        return collapsed;
    }
    let mut out: String = collapsed.chars().take(max.saturating_sub(1)).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

/// Turn a REST API URL into the matching web URL.
/// `https://api.github.com/repos/o/r/pulls/7` → `https://github.com/o/r/pull/7`.
pub fn api_to_html(api_url: &str) -> String {
    let Some(rest) = api_url
        .strip_prefix("https://api.github.com/repos/")
        .or_else(|| api_url.split_once("/api/v3/repos/").map(|(_, r)| r))
    else {
        return api_url.to_string();
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let base = "https://github.com";
    match parts.as_slice() {
        [o, r, "pulls", n, ..] => format!("{base}/{o}/{r}/pull/{n}"),
        [o, r, "issues", n, ..] if *n != "comments" => format!("{base}/{o}/{r}/issues/{n}"),
        [o, r, "commits", sha, ..] => format!("{base}/{o}/{r}/commit/{sha}"),
        [o, r, "releases", ..] => format!("{base}/{o}/{r}/releases"),
        [o, r, ..] => format!("{base}/{o}/{r}"),
        _ => api_url.to_string(),
    }
}

/// The trailing number of an issue/PR URL (`…/pulls/7` → 7).
pub fn trailing_number(url: &str) -> Option<u64> {
    url.trim_end_matches('/').rsplit('/').next()?.parse().ok()
}

pub fn is_bot(login: &str, user_type: Option<&str>) -> bool {
    login.ends_with("[bot]") || user_type.is_some_and(|t| t.eq_ignore_ascii_case("bot"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_collapses_and_cuts() {
        assert_eq!(snippet("a\n\n  b", 10), "a b");
        let s = snippet(&"x".repeat(200), 120);
        assert_eq!(s.chars().count(), 120);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn snippet_counts_chars_not_bytes() {
        let s = snippet(&"å".repeat(130), 120);
        assert_eq!(s.chars().count(), 120);
    }

    #[test]
    fn api_urls_become_web_urls() {
        assert_eq!(
            api_to_html("https://api.github.com/repos/octo-org/web-app/pulls/7"),
            "https://github.com/octo-org/web-app/pull/7"
        );
        assert_eq!(
            api_to_html("https://api.github.com/repos/octo-org/web-app/issues/88"),
            "https://github.com/octo-org/web-app/issues/88"
        );
        assert_eq!(
            api_to_html("https://api.github.com/repos/octo-org/web-app"),
            "https://github.com/octo-org/web-app"
        );
        assert_eq!(
            api_to_html("https://example.invalid/x"),
            "https://example.invalid/x"
        );
    }

    #[test]
    fn numbers_and_bots() {
        assert_eq!(trailing_number("https://x/pulls/42"), Some(42));
        assert_eq!(trailing_number("https://x/pulls/"), None);
        assert!(is_bot("ci[bot]", None));
        assert!(is_bot("deployer", Some("Bot")));
        assert!(!is_bot("alex", Some("User")));
    }
}
