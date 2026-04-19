//! Table-driven happy-path matrix for every ClaimType.
//!
//! Exercises only the public API: constructs a `Claim` for each type with
//! a minimal-but-valid `props` payload and asserts `validate_claim` accepts
//! it. Blank-node rejection lives in the in-file unit tests.

use nomograph_claim::schema::validate_claim;
use nomograph_claim::{Claim, ClaimType};
use serde_json::{Value, json};

fn fixture(ct: &ClaimType) -> Value {
    match ct {
        ClaimType::Tree => json!({"name": "keaton"}),
        ClaimType::Spec => json!({
            "tree": "keaton",
            "id": "sch-1",
            "goal": "validate claims at boundary",
            "status": "active",
            "topics": ["schema"],
        }),
        ClaimType::Task => json!({
            "tree": "keaton",
            "spec": "sch-1",
            "id": "t1",
            "summary": "wire dispatch",
            "status": "pending",
        }),
        ClaimType::Discovery => json!({
            "tree": "keaton",
            "spec": "sch-1",
            "id": "d1",
            "date": "2026-04-18",
            "finding": "dispatch is cheap",
        }),
        ClaimType::Campaign => json!({
            "tree": "keaton",
            "spec": "sch-1",
            "kind": "active",
        }),
        ClaimType::Session => json!({"id": "sess-1"}),
        ClaimType::Phase => json!({"session_id": "sess-1", "name": "execute"}),
        ClaimType::Intent => json!({
            "target": "src/schema.rs",
            "expires_at": 1_700_000_000_000u64,
        }),
        ClaimType::Heartbeat => json!({"intent_id": "i1", "status": "working"}),
        ClaimType::Outcome => json!({"intent_id": "i1", "outcome": "completed"}),
        ClaimType::Directive => json!({"target": "i1", "action": "pause"}),
        ClaimType::Stakeholder => json!({"id": "sh-1"}),
        ClaimType::Topic => json!({"name": "claim-substrate"}),
        ClaimType::Signal => json!({
            "stakeholder_id": "sh-1",
            "source": "https://gitlab.com/example/-/issues/1#note_1",
            "source_type": "issue_comment",
            "content": "looks good",
            "event_date": "2026-04-18",
        }),
        ClaimType::Disposition => json!({
            "stakeholder_id": "sh-1",
            "topic": "claim-substrate",
            "stance": "supportive",
            "confidence": "verified",
        }),
    }
}

const ALL_TYPES: &[ClaimType] = &[
    ClaimType::Tree,
    ClaimType::Spec,
    ClaimType::Task,
    ClaimType::Discovery,
    ClaimType::Campaign,
    ClaimType::Session,
    ClaimType::Phase,
    ClaimType::Intent,
    ClaimType::Heartbeat,
    ClaimType::Outcome,
    ClaimType::Directive,
    ClaimType::Stakeholder,
    ClaimType::Topic,
    ClaimType::Signal,
    ClaimType::Disposition,
];

#[test]
fn every_claim_type_has_a_passing_fixture() {
    for ct in ALL_TYPES {
        let claim = Claim::new(ct.clone(), fixture(ct), "user:test");
        validate_claim(&claim)
            .unwrap_or_else(|e| panic!("fixture for {:?} should validate: {e}", ct));
    }
}
