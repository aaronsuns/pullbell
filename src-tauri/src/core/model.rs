//! Shared data types: event kinds, inbox items and pull-request snapshots.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What kind of thing happened. Every kind defaults to "needs you" except [`Kind::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    ReviewRequested,
    Comment,
    Reply,
    Mention,
    Approved,
    ChangesRequested,
    ReadyToMerge,
    Conflict,
    DeployFailure,
    Digest,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 11] = [
        Kind::ReviewRequested,
        Kind::Comment,
        Kind::Reply,
        Kind::Mention,
        Kind::Approved,
        Kind::ChangesRequested,
        Kind::ReadyToMerge,
        Kind::Conflict,
        Kind::DeployFailure,
        Kind::Digest,
        Kind::Other,
    ];

    pub fn emoji(self) -> &'static str {
        match self {
            Kind::ReviewRequested => "👀",
            Kind::Comment | Kind::Mention => "💬",
            Kind::Reply => "↩️",
            Kind::Approved => "✅",
            Kind::ChangesRequested => "❌",
            Kind::ReadyToMerge => "🚀",
            Kind::Conflict => "⚠️",
            Kind::DeployFailure => "🔥",
            Kind::Digest => "📋",
            Kind::Other => "🤖",
        }
    }

    /// Whether this kind is "needs you" on a fresh install.
    pub fn default_needs_you(self) -> bool {
        self != Kind::Other
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::ReviewRequested => "Review requested",
            Kind::Comment => "Comment on my PR",
            Kind::Reply => "Reply in my review thread",
            Kind::Mention => "Mention",
            Kind::Approved => "Approved",
            Kind::ChangesRequested => "Changes requested",
            Kind::ReadyToMerge => "Ready to merge",
            Kind::Conflict => "Conflict",
            Kind::DeployFailure => "Default-branch deploy failure",
            Kind::Digest => "Daily review digest",
            Kind::Other => "Bot / other",
        }
    }
}

/// A new thing that happened, produced by classification or transition detection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Stable identity of the thread this event belongs to. A newer event with the same key
    /// replaces the older item instead of adding a second one.
    pub key: String,
    pub kind: Kind,
    pub repo: String,
    /// PR or issue number, when known (used to group banners per PR).
    pub number: Option<u64>,
    pub title: String,
    pub actor: String,
    /// Short verb phrase, e.g. "commented" or "requested your review".
    pub action: String,
    /// At most [`crate::core::text::BANNER_TEXT_LEN`] characters.
    pub text: String,
    pub url: String,
    pub updated_at: DateTime<Utc>,
    /// GitHub notification thread id, if any, so "done" can mark it read.
    pub thread_id: Option<String>,
}

/// An entry in the inbox. Same fields as an [`Event`]; kept as an alias so the JSON on disk is
/// self-describing.
pub type Item = Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiState {
    Green,
    Failing,
    Running,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mergeable {
    Mergeable,
    Conflicting,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
    None,
}

/// One open pull request as seen by the GraphQL search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub is_draft: bool,
    pub review: ReviewDecision,
    pub mergeable: Mergeable,
    pub ci: CiState,
    pub updated_at: DateTime<Utc>,
}

impl PullRequest {
    pub fn key(&self) -> String {
        format!("{}#{}", self.repo.to_lowercase(), self.number)
    }

    pub fn ci_glyph(&self) -> &'static str {
        match self.ci {
            CiState::Green => "🟢",
            CiState::Failing => "🔴",
            CiState::Running => "🟡",
            CiState::None => "⚪",
        }
    }

    pub fn review_glyph(&self) -> &'static str {
        if self.is_draft {
            "📝"
        } else if self.mergeable == Mergeable::Conflicting {
            "⚠️"
        } else {
            match self.review {
                ReviewDecision::ChangesRequested => "❌",
                ReviewDecision::Approved => "✅",
                _ => "👀",
            }
        }
    }

    /// `🟢✅ octo-org/web-app#7 Speed up the build cache`
    pub fn row(&self) -> String {
        format!(
            "{}{} {}#{} {}",
            self.ci_glyph(),
            self.review_glyph(),
            self.repo,
            self.number,
            self.title
        )
    }
}
