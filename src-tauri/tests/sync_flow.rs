//! A full poll against a mock GitHub.

use chrono::{DateTime, NaiveDateTime, Utc};
use pullbell_lib::core::inbox::State;
use pullbell_lib::core::model::Kind;
use pullbell_lib::core::scope::{Scope, ScopeMode};
use pullbell_lib::core::settings::Settings;
use pullbell_lib::github::{ApiError, GitHub};
use pullbell_lib::sync::{sync, MY_PRS_QUERY, WAITING_QUERY};
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str, base: &str) -> Value {
    let p = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(p)
        .unwrap()
        .replace("https://api.github.com", base);
    serde_json::from_str(&raw).unwrap()
}

fn now() -> DateTime<Utc> {
    "2026-03-02T15:00:00Z".parse().unwrap()
}
fn local(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
}

async fn mount_github(server: &MockServer, notifications: Value, runs: Value) {
    let base = server.uri();
    Mock::given(method("GET"))
        .and(path("/notifications"))
        .respond_with(ResponseTemplate::new(200).set_body_json(notifications))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/issues/comments/555"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("issue_comment_sam", &base)))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/pulls/42"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"number": 42, "head": {}, "merged": false})),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/pulls/42/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(json!({"variables": {"q": MY_PRS_QUERY}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("graphql_my_prs", &base)))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(json!({"variables": {"q": WAITING_QUERY}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"search": {"nodes": [{
            "number": 42, "title": "Add dark mode toggle", "url": "https://github.com/octo-org/web-app/pull/42",
            "isDraft": false, "updatedAt": "2026-03-02T09:00:00Z", "repository": {"nameWithOwner": "octo-org/web-app"},
            "author": {"login": "alex"}, "reviewDecision": "REVIEW_REQUIRED", "mergeable": "MERGEABLE",
            "commits": {"nodes": []}
        }]}}})))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"default_branch": "main"})))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/api-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"default_branch": "main"})))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/api-server/actions/runs"))
        .respond_with(ResponseTemplate::new(404))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/actions/runs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(runs))
        .mount(server)
        .await;
}

#[tokio::test]
async fn first_sync_is_silent_then_new_events_banner() {
    let server = MockServer::start().await;
    let base = server.uri();
    let notifs = json!([
        fixture("notification_review_requested", &base),
        fixture("notification_author_comment", &base)
    ]);
    let all_runs = fixture("workflow_runs", &base);
    let mut older_runs = all_runs.clone();
    older_runs["workflow_runs"]
        .as_array_mut()
        .unwrap()
        .retain(|r| r["id"] == 2990);
    mount_github(&server, notifs.clone(), older_runs).await;

    let gh = GitHub::new(&base, "t");
    let mut state = State::default();
    let mut settings = Settings::default();
    settings.alerts.digest_hour = Some(9);
    let scope = Scope::default();

    let r1 = sync(
        &gh,
        &mut state,
        &settings,
        &scope,
        "octocat",
        now(),
        local("2026-03-02 08:00"),
    )
    .await
    .unwrap();
    assert!(r1.banners.is_empty(), "first sync is silent");
    assert!(state.synced_once);
    let kinds: Vec<Kind> = state.items.iter().map(|i| i.kind).collect();
    assert!(kinds.contains(&Kind::ReviewRequested));
    assert!(kinds.contains(&Kind::Comment));
    assert!(kinds.contains(&Kind::ReadyToMerge));
    assert!(kinds.contains(&Kind::Conflict));
    assert_eq!(r1.mine.len(), 3);
    assert_eq!(r1.waiting.len(), 1);
    assert_eq!(state.run_cursors.get("octo-org/web-app"), Some(&2990));
    assert!(r1.warnings.is_empty(), "{:?}", r1.warnings);

    // Second poll: a new deploy failure lands and the digest hour has passed.
    server.reset().await;
    mount_github(&server, notifs, all_runs).await;
    let r2 = sync(
        &gh,
        &mut state,
        &settings,
        &scope,
        "octocat",
        now(),
        local("2026-03-02 09:30"),
    )
    .await
    .unwrap();
    let titles: Vec<&str> = r2.banners.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(
        titles,
        vec!["🔥 octo-org/web-app", "📋 Daily digest"],
        "{titles:?}"
    );
    assert_eq!(state.last_digest, Some(local("2026-03-02 09:30").date()));
    // Same notifications again: no duplicates.
    assert_eq!(state.items.iter().filter(|i| i.key == "n:1002").count(), 1);
}

#[tokio::test]
async fn muted_repos_produce_nothing() {
    let server = MockServer::start().await;
    let base = server.uri();
    mount_github(
        &server,
        json!([fixture("notification_author_comment", &base)]),
        fixture("workflow_runs", &base),
    )
    .await;
    let gh = GitHub::new(&base, "t");
    let mut state = State::default();
    let scope = Scope::build(
        ScopeMode::Everything,
        vec![],
        vec![],
        vec!["octo-org/web-app".into()],
    );
    let r = sync(
        &gh,
        &mut state,
        &Settings::default(),
        &scope,
        "octocat",
        now(),
        local("2026-03-02 08:00"),
    )
    .await
    .unwrap();
    assert!(state
        .items
        .iter()
        .all(|i| !i.repo.eq_ignore_ascii_case("octo-org/web-app")));
    assert_eq!(r.mine.len(), 1, "only the api-server PR is in scope");
}

#[tokio::test]
async fn unauthorized_aborts_the_poll_without_touching_state() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/notifications"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let gh = GitHub::new(server.uri(), "t");
    let mut state = State::default();
    let err = sync(
        &gh,
        &mut state,
        &Settings::default(),
        &Scope::default(),
        "octocat",
        now(),
        local("2026-03-02 08:00"),
    )
    .await
    .unwrap_err();
    assert_eq!(err, ApiError::Unauthorized);
    assert!(!state.synced_once);
}

#[tokio::test]
async fn sso_on_a_comment_still_lists_the_item() {
    let server = MockServer::start().await;
    let base = server.uri();
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/issues/comments/555"))
        .respond_with(ResponseTemplate::new(403).insert_header(
            "x-github-sso",
            "required; url=https://github.com/orgs/octo-org/sso?x=1",
        ))
        .mount(&server)
        .await;
    mount_github(
        &server,
        json!([fixture("notification_author_comment", &base)]),
        json!({"workflow_runs": []}),
    )
    .await;
    let gh = GitHub::new(&base, "t");
    let mut state = State::default();
    let r = sync(
        &gh,
        &mut state,
        &Settings::default(),
        &Scope::default(),
        "octocat",
        now(),
        local("2026-03-02 08:00"),
    )
    .await
    .unwrap();
    assert_eq!(
        r.sso.get("octo-org").map(String::as_str),
        Some("https://github.com/orgs/octo-org/sso?x=1")
    );
    assert!(state.items.iter().any(|i| i.key == "n:1002"));
}
