//! OAuth device flow and refresh against a mock server.

use pullbell_lib::auth::{
    device_poll, device_start, device_wait, refresh, AuthError, DeviceCode, PollOutcome,
};
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn start_returns_user_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/device/code"))
        .and(body_string_contains("client_id=test-client"))
        .and(body_string_contains("scope=notifications+repo+read%3Aorg"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "dc", "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device", "expires_in": 900, "interval": 5
        })))
        .mount(&server)
        .await;
    let code = device_start(&server.uri(), "test-client").await.unwrap();
    assert_eq!(code.user_code, "ABCD-1234");
    assert_eq!(code.interval, 5);
}

#[tokio::test]
async fn poll_outcomes() {
    let server = MockServer::start().await;
    for (dc, body) in [
        ("pending", json!({"error": "authorization_pending"})),
        ("slow", json!({"error": "slow_down", "interval": 10})),
        ("expired", json!({"error": "expired_token"})),
        ("denied", json!({"error": "access_denied"})),
        ("weird", json!({"error": "unsupported_grant_type"})),
        (
            "ok",
            json!({"access_token": "gho_x", "token_type": "bearer", "scope": "repo",
                       "refresh_token": "ghr_y", "expires_in": 28800}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path("/login/oauth/access_token"))
            .and(body_string_contains(format!("device_code={dc}&")))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
    }
    let p = |dc: &'static str| {
        let uri = server.uri();
        async move { device_poll(&uri, "c", dc, 5).await }
    };
    assert_eq!(p("pending").await.unwrap(), PollOutcome::Pending);
    assert_eq!(p("slow").await.unwrap(), PollOutcome::SlowDown(10));
    assert_eq!(p("expired").await.unwrap(), PollOutcome::Expired);
    assert_eq!(p("denied").await.unwrap(), PollOutcome::Denied);
    assert!(matches!(p("weird").await.unwrap_err(), AuthError::OAuth(_)));
    let PollOutcome::Done(t) = p("ok").await.unwrap() else {
        panic!()
    };
    assert_eq!(t.access_token, "gho_x");
    assert_eq!(t.refresh_token.as_deref(), Some("ghr_y"));
    assert!(t.expires_at.is_some());
}

#[tokio::test]
async fn wait_loops_until_done() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"error": "authorization_pending"})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "gho_z"})))
        .mount(&server)
        .await;
    let code = DeviceCode {
        device_code: "dc".into(),
        user_code: "U".into(),
        verification_uri: "v".into(),
        expires_in: 60,
        interval: 1,
    };
    let out = device_wait(&server.uri(), "c", &code, || false)
        .await
        .unwrap();
    assert!(matches!(out, PollOutcome::Done(t) if t.access_token == "gho_z"));
}

#[tokio::test]
async fn refresh_exchanges_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login/oauth/access_token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=ghr_old"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "gho_new", "refresh_token": "ghr_new", "expires_in": 28800
        })))
        .mount(&server)
        .await;
    let t = refresh(&server.uri(), "c", "ghr_old").await.unwrap();
    assert_eq!(t.access_token, "gho_new");
    assert_eq!(t.refresh_token.as_deref(), Some("ghr_new"));

    let bad = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"error": "bad_refresh_token"})),
        )
        .mount(&bad)
        .await;
    assert_eq!(
        refresh(&bad.uri(), "c", "x").await.unwrap_err(),
        AuthError::OAuth("bad_refresh_token".into())
    );
}
