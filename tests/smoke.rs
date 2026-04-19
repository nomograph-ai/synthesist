//! Smoke test: verify the crate is wired up end-to-end.
//!
//! Wave 2 expands this with real Store/View round-trips.

use nomograph_claim::{Claim, ClaimType};

#[test]
fn claim_constructs() {
    let claim = Claim::new(
        ClaimType::Spec,
        serde_json::json!({ "goal": "wave 1 scaffold" }),
        "user:gitlab:andunn",
    );

    assert_eq!(claim.claim_type, ClaimType::Spec);
    assert_eq!(claim.asserted_by, "user:gitlab:andunn");
    assert!(!claim.id.is_empty(), "content hash populated");
    assert!(claim.supersedes.is_none());
    assert!(claim.parent_asserter.is_none());
}
