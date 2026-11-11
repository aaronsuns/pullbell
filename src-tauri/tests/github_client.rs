//! GitHub client against a mock server.

use pullbell_lib::github::{ApiError, GitHub, NotificationsPage};
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, header, header_exists, method, path, query_param};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

/// Matches requests that do NOT carry the given header.
struct NoHeader(&'static str);
impl Match for NoHeader {
    fn matches(&self, req: &Request) -> bool {
        !req.headers.contains_key(self.0)
    }
}

fn fixture(name: &str) -> Value {
    let p = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

async fn setup() -> (MockServer, GitHub) {
    let server = MockServer::start().await;
    let gh = GitHub::new(server.uri(), "test-token");
    (server, gh)
}

/// Rewrite fixture URLs so they point at the mock server.
fn localize(mut v: Value, base: &str) -> Value {
    let s = v.to_string().replace("https://api.github.com", base);
    v = serde_json::from_str(&s).unwrap();
    v
}

#[tokio::test]
async fn sends_auth_and_api_headers() {
    let (server, gh) = setup().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("authorization", "Bearer test-token"))
        .and(header("accept", "application/vnd.github+json"))
        .and(header_exists("x-github-api-version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"login": "octocat"})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(gh.viewer().await.unwrap(), "octocat");
}

#[tokio::test]
async fn notifications_page_then_304() {
    let (server, gh) = setup().await;
    let lm = "Mon, 02 Mar 2026 10:00:00 GMT";
    Mock::given(method("GET"))
        .and(path("/notifications"))
        .and(header_exists("if-modified-since"))
        .respond_with(ResponseTemplate::new(304).insert_header("x-poll-interval", "120"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/notifications"))
        .and(query_param("participating", "false"))
        .and(NoHeader("if-modified-since"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("last-modified", lm)
                .insert_header("x-poll-interval", "60")
                .set_body_json(json!([fixture("notification_review_requested")])),
        )
        .mount(&server)
        .await;

    let first = gh.notifications(None).await.unwrap();
    let NotificationsPage::Page {
        items,
        last_modified,
        poll_interval,
    } = first
    else {
        panic!("expected a page")
    };
    assert_eq!(items.len(), 1);
    assert_eq!(last_modified.as_deref(), Some(lm));
    assert_eq!(poll_interval, Some(60));
    assert!(GitHub::parse_notification(&items[0]).is_some());

    let second = gh.notifications(Some(lm)).await.unwrap();
    assert_eq!(
        second,
        NotificationsPage::NotModified {
            poll_interval: Some(120)
        }
    );
}

#[tokio::test]
async fn a_401_streak_stays_unauthorized() {
    let (server, gh) = setup().await;
    Mock::given(method("GET"))
        .and(path("/notifications"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"message": "Bad credentials"})),
        )
        .expect(3)
        .mount(&server)
        .await;
    for _ in 0..3 {
        assert_eq!(
            gh.notifications(None).await.unwrap_err(),
            ApiError::Unauthorized
        );
    }
}

#[tokio::test]
async fn sso_403_names_the_org() {
    let (server, gh) = setup().await;
    let url = "https://github.com/orgs/octo-org/sso?authorization_request=xyz";
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/issues/comments/555"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-github-sso", format!("required; url={url}").as_str())
                .set_body_json(
                    json!({"message": "Resource protected by organization SAML enforcement."}),
                ),
        )
        .mount(&server)
        .await;
    let err = gh
        .get_json("/repos/octo-org/web-app/issues/comments/555")
        .await
        .unwrap_err();
    assert_eq!(
        err,
        ApiError::Sso {
            org: "octo-org".into(),
            url: url.into()
        }
    );
}

#[tokio::test]
async fn rate_limits_from_retry_after_and_reset() {
    let (server, gh) = setup().await;
    Mock::given(method("GET"))
        .and(path("/a"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "30"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/b"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", "1900000000")
                .set_body_json(json!({"message": "API rate limit exceeded"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/c"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(json!({"message": "Must have admin rights"})),
        )
        .mount(&server)
        .await;

    let before = chrono::Utc::now();
    match gh.get_json("/a").await.unwrap_err() {
        ApiError::RateLimited { until } => {
            let secs = (until - before).num_seconds();
            assert!((29..=31).contains(&secs), "{secs}");
        }
        e => panic!("{e:?}"),
    }
    match gh.get_json("/b").await.unwrap_err() {
        ApiError::RateLimited { until } => assert_eq!(until.timestamp(), 1_900_000_000),
        e => panic!("{e:?}"),
    }
    assert_eq!(
        gh.get_json("/c").await.unwrap_err(),
        ApiError::Http(403, "Must have admin rights".into())
    );
}

#[tokio::test]
async fn refuses_to_send_token_to_foreign_hosts() {
    let (_server, gh) = setup().await;
    let err = gh
        .get_json("https://example.invalid/steal")
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Decode(_)));
}

#[tokio::test]
async fn latest_activity_follows_comment_url() {
    let (server, gh) = setup().await;
    let base = server.uri();
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/issues/comments/555"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("issue_comment_sam")))
        .mount(&server)
        .await;
    let n = GitHub::parse_notification(&localize(fixture("notification_author_comment"), &base))
        .unwrap();
    let detail = gh.latest_activity(&n).await.unwrap().unwrap();
    assert_eq!(detail["user"]["login"], "sam");
}

#[tokio::test]
async fn latest_activity_on_the_pr_itself_uses_newest_review() {
    let (server, gh) = setup().await;
    let base = server.uri();
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/pulls/7"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"number": 7, "head": {}, "merged": false})),
        )
        .mount(&server)
        .await;
    let mut review = fixture("review_approved_alex");
    review["submitted_at"] = "2026-03-02T10:14:00Z".into();
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/pulls/7/reviews"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([fixture("review_changes_sam"), review])),
        )
        .mount(&server)
        .await;
    let mut nv = localize(fixture("notification_author_comment"), &base);
    nv["subject"]["latest_comment_url"] = format!("{base}/repos/octo-org/web-app/pulls/7").into();
    let n = GitHub::parse_notification(&nv).unwrap();
    let detail = gh.latest_activity(&n).await.unwrap().unwrap();
    assert_eq!(detail["state"], "APPROVED");
}

#[tokio::test]
async fn graphql_pr_search_and_errors() {
    let (server, gh) = setup().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(
            json!({"variables": {"q": "is:pr is:open author:@me archived:false"}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("graphql_my_prs")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(json!({"variables": {"q": "broken"}})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"errors": [{"message": "Parse error"}]})),
        )
        .mount(&server)
        .await;
    let prs = gh
        .search_prs("is:pr is:open author:@me archived:false")
        .await
        .unwrap();
    assert_eq!(prs.len(), 3);
    assert_eq!(
        gh.search_prs("broken").await.unwrap_err(),
        ApiError::Http(200, "Parse error".into())
    );
}

#[tokio::test]
async fn marks_thread_read() {
    let (server, gh) = setup().await;
    Mock::given(method("PATCH"))
        .and(path("/notifications/threads/1001"))
        .respond_with(ResponseTemplate::new(205))
        .expect(1)
        .mount(&server)
        .await;
    gh.mark_thread_read("1001").await.unwrap();
}

#[tokio::test]
async fn teams_repos_branches_and_runs() {
    let (server, gh) = setup().await;
    Mock::given(method("GET"))
        .and(path("/user/teams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"slug": "frontend", "name": "Frontend", "organization": {"login": "octo-org"}}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/orgs/octo-org/teams/frontend/repos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"full_name": "octo-org/web-app"}, {"full_name": "octo-org/design-system"}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"default_branch": "main"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/octo-org/web-app/actions/runs"))
        .and(query_param("branch", "main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("workflow_runs")))
        .mount(&server)
        .await;
    let teams = gh.user_teams().await.unwrap();
    assert_eq!(teams[0].org, "octo-org");
    assert_eq!(
        gh.team_repos("octo-org", "frontend").await.unwrap().len(),
        2
    );
    assert_eq!(gh.default_branch("octo-org/web-app").await.unwrap(), "main");
    assert_eq!(
        gh.workflow_runs("octo-org/web-app", "main")
            .await
            .unwrap()
            .len(),
        3
    );
}
