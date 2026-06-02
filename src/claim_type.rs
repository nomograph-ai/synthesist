//! Synthesist-owned claim vocabulary.
//!
//! claim is vocabulary-agnostic (string/IRI-typed): the gamma index and
//! the JSON-LD log store any `@type` without an enum. Each module owns
//! its own vocabulary. This is synthesist's: the 8 workflow types it
//! writes and validates, plus the 4 RESERVED coordination types that
//! exist for multi-user agent coordination (see
//! `keaton/research/graph-primitive/COORDINATION-TYPES-DECISION.md`).
//!
//! [`ClaimType`] lives adjacent to [`crate::wire_format`] (which is
//! already `&str`-typed) so the write path can map a typed variant to
//! its compact `@type` IRI via `wire_format::type_iri(ct.as_str())`.
//!
//! Lattice's 4 observation types (Stakeholder, Topic, Signal,
//! Disposition) are NOT owned here. They are carried as variants only so
//! synthesist's `check` walk and `append_replay` (import / migration)
//! can name a foreign claim's type; synthesist has no validator for
//! them and rejects them on the strict write path.

use serde::{Deserialize, Serialize};

/// A synthesist claim type.
///
/// The 8 workflow variants (Tree..Outcome) are synthesist-owned and
/// validated. The 4 coordination variants (Intent, Heartbeat, Outcome
/// is workflow; the coordination outcome is the same `Outcome` variant
/// reserved by COORDINATION-TYPES-DECISION.md, plus Directive) are
/// reserved for the multi-user coordination protocol and have no writer
/// yet. The 4 observation variants (Stakeholder..Disposition) belong to
/// lattice and are only named here so the read/import paths can refer to
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimType {
    // Workflow (synthesist-owned, validated).
    Tree,
    Spec,
    Task,
    Discovery,
    Campaign,
    Session,
    Phase,
    Outcome,
    // Coordination family. RESERVED per COORDINATION-TYPES-DECISION.md:
    // no writer yet; first use is multi-agent coordination when a second
    // agent joins a shared substrate. Do not delete; do not edit the
    // variants without updating the decision doc.
    Intent,
    Heartbeat,
    Directive,
    // Observation (lattice-owned). Named here only so synthesist's check
    // walk and append_replay can refer to a foreign claim's type.
    Stakeholder,
    Topic,
    Signal,
    Disposition,
}

impl ClaimType {
    /// The bare lowercase type name, the inverse of the `snake_case`
    /// serde rename. Used by the write path to build the compact `@type`
    /// IRI (`wire_format::type_iri(ct.as_str())`) and by the claim-id
    /// hash.
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimType::Tree => "tree",
            ClaimType::Spec => "spec",
            ClaimType::Task => "task",
            ClaimType::Discovery => "discovery",
            ClaimType::Campaign => "campaign",
            ClaimType::Session => "session",
            ClaimType::Phase => "phase",
            ClaimType::Outcome => "outcome",
            ClaimType::Intent => "intent",
            ClaimType::Heartbeat => "heartbeat",
            ClaimType::Directive => "directive",
            ClaimType::Stakeholder => "stakeholder",
            ClaimType::Topic => "topic",
            ClaimType::Signal => "signal",
            ClaimType::Disposition => "disposition",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_round_trips_the_workflow_variants() {
        assert_eq!(ClaimType::Tree.as_str(), "tree");
        assert_eq!(ClaimType::Spec.as_str(), "spec");
        assert_eq!(ClaimType::Task.as_str(), "task");
        assert_eq!(ClaimType::Outcome.as_str(), "outcome");
    }

    #[test]
    fn serde_uses_snake_case() {
        let j = serde_json::to_string(&ClaimType::Discovery).unwrap();
        assert_eq!(j, "\"discovery\"");
        let ct: ClaimType = serde_json::from_str("\"phase\"").unwrap();
        assert_eq!(ct, ClaimType::Phase);
    }
}
