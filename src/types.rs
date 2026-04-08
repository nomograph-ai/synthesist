//! Core data types for synthesist v1.
//!
//! These types ARE the schema. The serde tags are the wire format.
//! LLM agents read and write these via the synthesist CLI.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// --- Enums ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Done,
    Blocked,
    Waiting,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Waiting => "waiting",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "waiting" => Some(Self::Waiting),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Supportive,
    Cautious,
    Opposed,
    Neutral,
    Unknown,
}

impl Stance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Supportive => "supportive",
            Self::Cautious => "cautious",
            Self::Opposed => "opposed",
            Self::Neutral => "neutral",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "supportive" => Some(Self::Supportive),
            "cautious" => Some(Self::Cautious),
            "opposed" => Some(Self::Opposed),
            "neutral" => Some(Self::Neutral),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

impl std::fmt::Display for Stance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Documented,
    Verified,
    Inferred,
    Speculative,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Documented => "documented",
            Self::Verified => "verified",
            Self::Inferred => "inferred",
            Self::Speculative => "speculative",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "documented" => Some(Self::Documented),
            "verified" => Some(Self::Verified),
            "inferred" => Some(Self::Inferred),
            "speculative" => Some(Self::Speculative),
            _ => None,
        }
    }
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalType {
    PrComment,
    IssueComment,
    Review,
    CommitMessage,
    Chat,
    Meeting,
    Email,
    Other,
}

impl SignalType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PrComment => "pr_comment",
            Self::IssueComment => "issue_comment",
            Self::Review => "review",
            Self::CommitMessage => "commit_message",
            Self::Chat => "chat",
            Self::Meeting => "meeting",
            Self::Email => "email",
            Self::Other => "other",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pr_comment" => Some(Self::PrComment),
            "issue_comment" => Some(Self::IssueComment),
            "review" => Some(Self::Review),
            "commit_message" => Some(Self::CommitMessage),
            "chat" => Some(Self::Chat),
            "meeting" => Some(Self::Meeting),
            "email" => Some(Self::Email),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

impl std::fmt::Display for SignalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Orient,
    Plan,
    Agree,
    Execute,
    Reflect,
    Replan,
    Report,
}

impl Phase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Orient => "orient",
            Self::Plan => "plan",
            Self::Agree => "agree",
            Self::Execute => "execute",
            Self::Reflect => "reflect",
            Self::Replan => "replan",
            Self::Report => "report",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "orient" => Some(Self::Orient),
            "plan" => Some(Self::Plan),
            "agree" => Some(Self::Agree),
            "execute" => Some(Self::Execute),
            "reflect" => Some(Self::Reflect),
            "replan" => Some(Self::Replan),
            "report" => Some(Self::Report),
            _ => None,
        }
    }

    /// Return the list of phases reachable from this phase.
    pub fn valid_transitions(&self) -> &'static [Phase] {
        match self {
            Self::Orient => &[Self::Plan],
            Self::Plan => &[Self::Agree],
            Self::Agree => &[Self::Execute],
            Self::Execute => &[Self::Reflect, Self::Report],
            Self::Reflect => &[Self::Execute, Self::Replan, Self::Report],
            Self::Replan => &[Self::Agree],
            Self::Report => &[],
        }
    }

    /// Check whether transitioning from this phase to `target` is allowed.
    pub fn can_transition_to(&self, target: Phase) -> bool {
        self.valid_transitions().contains(&target)
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecStatus {
    Active,
    Completed,
    Abandoned,
    Superseded,
    Deferred,
}

impl SpecStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Abandoned => "abandoned",
            Self::Superseded => "superseded",
            Self::Deferred => "deferred",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "completed" => Some(Self::Completed),
            "abandoned" => Some(Self::Abandoned),
            "superseded" => Some(Self::Superseded),
            "deferred" => Some(Self::Deferred),
            _ => None,
        }
    }
}

impl std::fmt::Display for SpecStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Merged,
    Discarded,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Merged => "merged",
            Self::Discarded => "discarded",
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// --- Data structures ---

#[derive(Debug, Serialize, Deserialize)]
pub struct Tree {
    pub name: String,
    pub status: String,
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Spec {
    pub tree: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraints: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decisions: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<String>,
    pub depends_on: Vec<String>,
    pub files: Vec<String>,
    pub acceptance: Vec<Criterion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Criterion {
    pub criterion: String,
    pub verify: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Discovery {
    pub id: String,
    pub tree: String,
    pub spec: String,
    pub date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub finding: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Stakeholder {
    pub id: String,
    pub tree: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub context: String,
    #[serde(default)]
    pub orgs: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Disposition {
    pub id: String,
    pub tree: String,
    pub spec: String,
    pub stakeholder_id: String,
    pub topic: String,
    pub stance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_approach: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub confidence: String,
    pub valid_from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Signal {
    pub id: String,
    pub tree: String,
    pub spec: String,
    pub stakeholder_id: String,
    pub date: String,
    pub recorded_date: String,
    pub source: String,
    pub source_type: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub our_action: Option<String>,
}
