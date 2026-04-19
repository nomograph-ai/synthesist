//! Claim schema (v0) per Decision D8.
//!
//! 16 claim types across three families:
//!   - workflow: Tree, Spec, Task, Discovery, Campaign, Session, Phase
//!   - coordination: Intent, Heartbeat, Outcome, Directive
//!   - observation: Stakeholder, Topic, Signal, Disposition
//!
//! Universal fields on every claim:
//!   - id (blake3 content hash)
//!   - claim_type
//!   - props (typed per claim_type; JSON)
//!   - valid_from, valid_until
//!   - supersedes (prior claim id this replaces)
//!   - parent_asserter (for agent hierarchy audit — D8)
//!   - asserted_by, asserted_at

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type ClaimId = String;
pub type AsserterId = String;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimType {
    // Workflow (synthesist)
    Tree,
    Spec,
    Task,
    Discovery,
    Campaign,
    Session,
    Phase,
    // Coordination
    Intent,
    Heartbeat,
    Outcome,
    Directive,
    // Observation (lattice)
    Stakeholder,
    Topic,
    Signal,
    Disposition,
}

impl ClaimType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimType::Tree => "tree",
            ClaimType::Spec => "spec",
            ClaimType::Task => "task",
            ClaimType::Discovery => "discovery",
            ClaimType::Campaign => "campaign",
            ClaimType::Session => "session",
            ClaimType::Phase => "phase",
            ClaimType::Intent => "intent",
            ClaimType::Heartbeat => "heartbeat",
            ClaimType::Outcome => "outcome",
            ClaimType::Directive => "directive",
            ClaimType::Stakeholder => "stakeholder",
            ClaimType::Topic => "topic",
            ClaimType::Signal => "signal",
            ClaimType::Disposition => "disposition",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub claim_type: ClaimType,
    pub props: serde_json::Value,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub supersedes: Option<ClaimId>,
    /// Agent hierarchy audit — who authorized the asserter (D8).
    pub parent_asserter: Option<AsserterId>,
    pub asserted_by: AsserterId,
    pub asserted_at: DateTime<Utc>,
}

impl Claim {
    /// Content hash over the canonical form of the required fields.
    /// NOT included in the hash: id itself, supersedes, parent_asserter.
    /// Reason: supersession chains and delegation shouldn't change identity.
    pub fn compute_id(
        claim_type: &ClaimType,
        props: &serde_json::Value,
        valid_from: DateTime<Utc>,
        asserted_by: &str,
        asserted_at: DateTime<Utc>,
    ) -> ClaimId {
        let canon = serde_json::json!({
            "claim_type": claim_type.as_str(),
            "props": props,
            "valid_from": valid_from.timestamp_millis(),
            "asserted_by": asserted_by,
            "asserted_at": asserted_at.timestamp_millis(),
        });
        let bytes = serde_json::to_vec(&canon).expect("serialize canonical form");
        blake3::hash(&bytes).to_hex().to_string()
    }

    pub fn new(
        claim_type: ClaimType,
        props: serde_json::Value,
        asserted_by: impl Into<AsserterId>,
    ) -> Self {
        let now = Utc::now();
        let asserted_by = asserted_by.into();
        let id = Self::compute_id(&claim_type, &props, now, &asserted_by, now);
        Self {
            id,
            claim_type,
            props,
            valid_from: now,
            valid_until: None,
            supersedes: None,
            parent_asserter: None,
            asserted_by,
            asserted_at: now,
        }
    }

    pub fn with_supersedes(mut self, prior: ClaimId) -> Self {
        self.supersedes = Some(prior);
        self
    }

    pub fn with_parent_asserter(mut self, parent: AsserterId) -> Self {
        self.parent_asserter = Some(parent);
        self
    }

    pub fn with_valid_until(mut self, until: DateTime<Utc>) -> Self {
        self.valid_until = Some(until);
        self
    }
}

/// Asserter class discrimination per D8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsserterClass<'a> {
    User(&'a str),
    Agent(&'a str),
    Ingest(&'a str),
    Unknown,
}

pub fn asserter_class(asserter: &str) -> AsserterClass<'_> {
    if let Some(rest) = asserter.strip_prefix("user:") {
        AsserterClass::User(rest)
    } else if let Some(rest) = asserter.strip_prefix("agent:") {
        AsserterClass::Agent(rest)
    } else if let Some(rest) = asserter.strip_prefix("ingest:") {
        AsserterClass::Ingest(rest)
    } else {
        AsserterClass::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_stable() {
        let props = serde_json::json!({"foo": "bar"});
        let t = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        let a = Claim::compute_id(&ClaimType::Spec, &props, t, "user:gitlab:andunn", t);
        let b = Claim::compute_id(&ClaimType::Spec, &props, t, "user:gitlab:andunn", t);
        assert_eq!(a, b);
    }

    #[test]
    fn asserter_class_parses() {
        assert!(matches!(
            asserter_class("user:gitlab:andunn"),
            AsserterClass::User("gitlab:andunn")
        ));
        assert!(matches!(
            asserter_class("agent:claude-opus-4-7:sess@host"),
            AsserterClass::Agent("claude-opus-4-7:sess@host")
        ));
        assert!(matches!(
            asserter_class("ingest:lattice:gitlab:nomograph/keaton"),
            AsserterClass::Ingest("lattice:gitlab:nomograph/keaton")
        ));
    }
}
