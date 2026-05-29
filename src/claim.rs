//! Claim schema (v0) per Decision D8.
//!
//! Deprecated: v3 substrate uses JSON-LD documents; see `log`, `jsonld`, and `asserter` modules.

#![allow(deprecated)]
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

#[deprecated(
    since = "3.0.0-pre.1",
    note = "v3 uses URN-form identifiers; see `log::ClaimRef` or equivalent."
)]
pub type ClaimId = String;

#[deprecated(
    since = "3.0.0-pre.1",
    note = "v3 asserter handling lives in the `asserter` module."
)]
pub type AsserterId = String;

#[deprecated(
    since = "3.0.0-pre.1",
    note = "v3 uses JSON-LD @type IRIs. See `jsonld` module constants."
)]
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
    // Coordination family. Reserved per BUILDING.md D8 and
    // overnight-2026-04-18/09-decision-document.md Decision 6.
    // No writer yet; first use is multi-agent coordination when a
    // second agent joins a shared substrate (the Wednesday Josh-sync
    // scenario). Schema is stable; do not edit the variants or
    // validators without updating the decision doc at
    // keaton/research/graph-primitive/COORDINATION-TYPES-DECISION.md.
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

#[deprecated(
    since = "3.0.0-pre.1",
    note = "v3 substrate uses JSON-LD documents written through `log::LogWriter`; read via `log::LogReader`. See nomograph-claim README v3 section."
)]
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
    ///
    /// The canonical form sorts every object's keys lexicographically at
    /// every nesting level, so two machines that build the same logical
    /// claim always get the same bytes and therefore the same id. This
    /// is the cross-machine dedup contract on merges: whatever the
    /// `preserve_order` feature flag or ad-hoc construction order,
    /// identical (claim_type, props, valid_from, asserted_by, asserted_at)
    /// hash to the same id.
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
        let mut bytes = Vec::with_capacity(256);
        write_canonical(&canon, &mut bytes);
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

/// Serialize a JSON value with recursively sorted object keys.
///
/// Properties this provides and why each matters for claim identity:
///  - Objects: keys sorted lexicographically (ASCII byte order). This
///    makes `{"b":1,"a":2}` and `{"a":2,"b":1}` produce identical bytes,
///    so the same logical claim built on Mac vs Linux hashes the same.
///  - Arrays: order preserved. Arrays are ordered data; changing their
///    order IS a different claim.
///  - Strings: re-serialized through serde_json to reuse its RFC 8259
///    escaping rules (backslashes, \uXXXX for controls). Keeps escapes
///    consistent regardless of whether the input value came from parse
///    or from `json!()`.
///  - Numbers: serialized via `Number::Display`, which matches serde_json's
///    default. Our props only use integers and strings (timestamps,
///    ids, names), so number canonicalization quirks around floats
///    don't bite us here.
fn write_canonical(v: &serde_json::Value, buf: &mut Vec<u8>) {
    match v {
        serde_json::Value::Null => buf.extend_from_slice(b"null"),
        serde_json::Value::Bool(true) => buf.extend_from_slice(b"true"),
        serde_json::Value::Bool(false) => buf.extend_from_slice(b"false"),
        serde_json::Value::Number(n) => {
            buf.extend_from_slice(n.to_string().as_bytes());
        }
        serde_json::Value::String(s) => {
            let escaped = serde_json::to_string(s).expect("serialize string via serde_json");
            buf.extend_from_slice(escaped.as_bytes());
        }
        serde_json::Value::Array(arr) => {
            buf.push(b'[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                write_canonical(item, buf);
            }
            buf.push(b']');
        }
        serde_json::Value::Object(m) => {
            buf.push(b'{');
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                let escaped = serde_json::to_string(k).expect("serialize key via serde_json");
                buf.extend_from_slice(escaped.as_bytes());
                buf.push(b':');
                write_canonical(&m[*k], buf);
            }
            buf.push(b'}');
        }
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

    /// Cross-machine dedup: same logical claim built with props in
    /// different key orders must hash to the same id, otherwise the
    /// CRDT merge produces duplicates. Regression for ADVERSARIAL-REVIEW
    /// CRITICAL #1.
    #[test]
    fn content_hash_is_key_order_independent() {
        let t = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        let p1: serde_json::Value =
            serde_json::from_str(r#"{"a":1,"b":2,"c":{"d":4,"e":5}}"#).unwrap();
        let p2: serde_json::Value =
            serde_json::from_str(r#"{"c":{"e":5,"d":4},"b":2,"a":1}"#).unwrap();
        let p3: serde_json::Value =
            serde_json::from_str(r#"{"b":2,"a":1,"c":{"d":4,"e":5}}"#).unwrap();
        let h1 = Claim::compute_id(&ClaimType::Spec, &p1, t, "x", t);
        let h2 = Claim::compute_id(&ClaimType::Spec, &p2, t, "x", t);
        let h3 = Claim::compute_id(&ClaimType::Spec, &p3, t, "x", t);
        assert_eq!(h1, h2);
        assert_eq!(h1, h3);
    }

    #[test]
    fn content_hash_preserves_array_order() {
        let t = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        let p1 = serde_json::json!({"items": [1, 2, 3]});
        let p2 = serde_json::json!({"items": [3, 2, 1]});
        let h1 = Claim::compute_id(&ClaimType::Spec, &p1, t, "x", t);
        let h2 = Claim::compute_id(&ClaimType::Spec, &p2, t, "x", t);
        assert_ne!(h1, h2, "array order is semantically meaningful");
    }

    #[test]
    fn content_hash_handles_nested_scalars() {
        let t = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        let props = serde_json::json!({
            "n": 42,
            // Arbitrary float value; not a mathematical constant.
            "f": 17.5,
            "b": true,
            "s": "hello \"world\" \n",
            "nil": null,
            "nested": {"z": 1, "a": {"deep": "val"}}
        });
        // Must not panic, must be deterministic
        let h1 = Claim::compute_id(&ClaimType::Spec, &props, t, "x", t);
        let h2 = Claim::compute_id(&ClaimType::Spec, &props, t, "x", t);
        assert_eq!(h1, h2);
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
