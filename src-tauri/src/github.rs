//! Thin GitHub REST + GraphQL client. Maps HTTP failures to [`ApiError`] so the poller can
//! react (401 streaks, SSO, rate limits) and never logs or leaks the token.

use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use reqwest::header::{
    HeaderMap, ACCEPT, AUTHORIZATION, IF_MODIFIED_SINCE, LAST_MODIFIED, USER_AGENT,
};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::core::classify::Notification;
use crate::core::health::parse_sso_header;
use crate::core::model::PullRequest;
use crate::core::prs::parse_search;
use crate::core::runs::{Run, Runs};

pub const API_BASE: &str = "https://api.github.com";

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized (401)")]
    Unauthorized,
    #[error("SAML SSO authorization required for {org}")]
    Sso { org: String, url: String },
    #[error("rate limited until {until}")]
    RateLimited { until: DateTime<Utc> },
    #[error("not found")]
    NotFound,
    #[error("HTTP {0}: {1}")]
    Http(u16, String),
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected response: {0}")]
    Decode(String),
}

pub type Result<T> = std::result::Result<T, ApiError>;

/// Result of polling `/notifications`.
#[derive(Debug, Clone, PartialEq)]
pub enum NotificationsPage {
    NotModified {
        poll_interval: Option<u64>,
    },
    Page {
        items: Vec<Value>,
        last_modified: Option<String>,
        poll_interval: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Team {
    pub org: String,
    pub slug: String,
    pub name: String,
}

#[derive(Clone)]
pub struct GitHub {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl std::fmt::Debug for GitHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHub")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("Pullbell/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client builds")
}

impl GitHub {
    pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
        GitHub {
            http: http_client(),
            base: base.into().trim_end_matches('/').to_string(),
            token: token.into(),
        }
    }

    fn url(&self, path_or_url: &str) -> Result<String> {
        if path_or_url.starts_with('/') {
            return Ok(format!("{}{path_or_url}", self.base));
        }
        // Absolute URLs (latest_comment_url etc.) must point at the same API host, so the token
        // is never sent anywhere else.
        if path_or_url.starts_with(&format!("{}/", self.base)) {
            return Ok(path_or_url.to_string());
        }
        Err(ApiError::Decode(format!(
            "refusing foreign URL {path_or_url}"
        )))
    }

    fn req(&self, method: Method, url: &str) -> RequestBuilder {
        self.http
            .request(method, url)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header(USER_AGENT, concat!("Pullbell/", env!("CARGO_PKG_VERSION")))
    }

    async fn send(&self, rb: RequestBuilder) -> Result<Response> {
        let resp = rb
            .send()
            .await
            .map_err(|e| ApiError::Network(e.without_url().to_string()))?;
        check(resp, Utc::now()).await
    }

    pub async fn get_json(&self, path_or_url: &str) -> Result<Value> {
        let url = self.url(path_or_url)?;
        let resp = self.send(self.req(Method::GET, &url)).await?;
        resp.json()
            .await
            .map_err(|e| ApiError::Decode(e.without_url().to_string()))
    }

    /// The signed-in user's login.
    pub async fn viewer(&self) -> Result<String> {
        let v = self.get_json("/user").await?;
        v.get("login")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| ApiError::Decode("no login".into()))
    }

    /// Unread notifications, participating and not, honouring `If-Modified-Since`.
    pub async fn notifications(
        &self,
        if_modified_since: Option<&str>,
    ) -> Result<NotificationsPage> {
        let url = self.url("/notifications?all=false&participating=false&per_page=50")?;
        let mut rb = self.req(Method::GET, &url);
        if let Some(lm) = if_modified_since {
            rb = rb.header(IF_MODIFIED_SINCE, lm);
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| ApiError::Network(e.without_url().to_string()))?;
        let poll_interval = header_u64(resp.headers(), "x-poll-interval");
        if resp.status() == StatusCode::NOT_MODIFIED {
            return Ok(NotificationsPage::NotModified { poll_interval });
        }
        let resp = check(resp, Utc::now()).await?;
        let last_modified = resp
            .headers()
            .get(LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let items: Vec<Value> = resp
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.without_url().to_string()))?;
        Ok(NotificationsPage::Page {
            items,
            last_modified,
            poll_interval,
        })
    }

    pub fn parse_notification(v: &Value) -> Option<Notification> {
        serde_json::from_value(v.clone()).ok()
    }

    /// The JSON describing the latest activity on a notification: the comment or review that
    /// `latest_comment_url` points at, or — when it points at the PR itself — its newest review.
    pub async fn latest_activity(&self, n: &Notification) -> Result<Option<Value>> {
        let Some(url) = n.subject.latest_comment_url.as_deref() else {
            return Ok(None);
        };
        let v = self.get_json(url).await?;
        let is_pr_itself = v.get("head").is_some() && v.get("merged").is_some();
        if !is_pr_itself {
            return Ok(Some(v));
        }
        let reviews = self
            .get_json(&format!("{url}/reviews?per_page=100"))
            .await?;
        let last = reviews.as_array().and_then(|a| a.last()).cloned();
        // Only use the review when it is about as new as the notification itself.
        Ok(last.filter(|r| {
            r.get("submitted_at")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<DateTime<Utc>>().ok())
                .is_some_and(|t| (n.updated_at - t).num_minutes().abs() <= 10)
        }))
    }

    pub async fn mark_thread_read(&self, thread_id: &str) -> Result<()> {
        let url = self.url(&format!("/notifications/threads/{thread_id}"))?;
        self.send(self.req(Method::PATCH, &url)).await.map(|_| ())
    }

    pub async fn graphql(&self, query: &str, variables: Value) -> Result<Value> {
        let url = self.url("/graphql")?;
        let rb = self
            .req(Method::POST, &url)
            .json(&json!({ "query": query, "variables": variables }));
        let v: Value = self
            .send(rb)
            .await?
            .json()
            .await
            .map_err(|e| ApiError::Decode(e.without_url().to_string()))?;
        if v.get("data").is_some_and(|d| !d.is_null()) {
            // Partial results (e.g. an SSO-protected org omitted) are still useful.
            return Ok(v);
        }
        let msg = v
            .pointer("/errors/0/message")
            .and_then(Value::as_str)
            .unwrap_or("GraphQL error");
        Err(ApiError::Http(200, msg.to_string()))
    }

    /// Open PRs matching a search, e.g. `is:pr is:open author:@me archived:false`.
    pub async fn search_prs(&self, q: &str) -> Result<Vec<PullRequest>> {
        let v = self.graphql(PR_SEARCH, json!({ "q": q })).await?;
        Ok(parse_search(&v))
    }

    pub async fn user_teams(&self) -> Result<Vec<Team>> {
        let mut out = Vec::new();
        for page in 1..=10 {
            let v = self
                .get_json(&format!("/user/teams?per_page=100&page={page}"))
                .await?;
            let arr = v.as_array().cloned().unwrap_or_default();
            for t in &arr {
                let s = |p: &str| {
                    t.pointer(p)
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                };
                out.push(Team {
                    org: s("/organization/login"),
                    slug: s("/slug"),
                    name: s("/name"),
                });
            }
            if arr.len() < 100 {
                break;
            }
        }
        Ok(out)
    }

    pub async fn team_repos(&self, org: &str, slug: &str) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for page in 1..=10 {
            let v = self
                .get_json(&format!(
                    "/orgs/{org}/teams/{slug}/repos?per_page=100&page={page}"
                ))
                .await?;
            let arr = v.as_array().cloned().unwrap_or_default();
            out.extend(
                arr.iter()
                    .filter_map(|r| r.get("full_name").and_then(Value::as_str))
                    .map(str::to_string),
            );
            if arr.len() < 100 {
                break;
            }
        }
        Ok(out)
    }

    pub async fn default_branch(&self, repo: &str) -> Result<String> {
        let v = self.get_json(&format!("/repos/{repo}")).await?;
        v.get("default_branch")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| ApiError::Decode("no default_branch".into()))
    }

    pub async fn workflow_runs(&self, repo: &str, branch: &str) -> Result<Vec<Run>> {
        let v = self
            .get_json(&format!(
                "/repos/{repo}/actions/runs?branch={}&per_page=20&exclude_pull_requests=true",
                urlencode(branch)
            ))
            .await?;
        serde_json::from_value::<Runs>(v)
            .map(|r| r.workflow_runs)
            .map_err(|e| ApiError::Decode(e.to_string()))
    }
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

const PR_SEARCH: &str = r#"
query($q: String!) {
  search(query: $q, type: ISSUE, first: 50) {
    nodes {
      ... on PullRequest {
        number title url isDraft updatedAt
        repository { nameWithOwner }
        author { login }
        reviewDecision mergeable
        commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
      }
    }
  }
}"#;

fn header_u64(h: &HeaderMap, name: &str) -> Option<u64> {
    h.get(name)?.to_str().ok()?.trim().parse().ok()
}

/// Map a non-success response to an [`ApiError`]; success passes through.
pub async fn check(resp: Response, now: DateTime<Utc>) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let headers = resp.headers().clone();
    let body = resp.text().await.unwrap_or_default();
    Err(classify_error(status.as_u16(), &headers, &body, now))
}

pub fn classify_error(status: u16, h: &HeaderMap, body: &str, now: DateTime<Utc>) -> ApiError {
    if status == 401 {
        return ApiError::Unauthorized;
    }
    if status == 403 {
        if let Some((org, url)) = h
            .get("x-github-sso")
            .and_then(|v| v.to_str().ok())
            .and_then(parse_sso_header)
        {
            return ApiError::Sso { org, url };
        }
    }
    if status == 403 || status == 429 {
        let retry_after = header_u64(h, "retry-after");
        let remaining = header_u64(h, "x-ratelimit-remaining");
        let reset = header_u64(h, "x-ratelimit-reset");
        let secondary = body.to_ascii_lowercase().contains("rate limit");
        if let Some(secs) = retry_after {
            return ApiError::RateLimited {
                until: now + chrono::Duration::seconds(secs as i64),
            };
        }
        if remaining == Some(0) || status == 429 || secondary {
            let until = reset
                .and_then(|r| Utc.timestamp_opt(r as i64, 0).single())
                .unwrap_or(now + chrono::Duration::seconds(60));
            return ApiError::RateLimited { until };
        }
    }
    if status == 404 {
        return ApiError::NotFound;
    }
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    ApiError::Http(status, msg)
}
