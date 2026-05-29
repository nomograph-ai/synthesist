//! End-to-end overlay acceptance test (T8.3).
//!
//! Mirrors a realistic workflow:
//!   1. Create a spec with synthesist:agreeSnapshot pointing at 3 pre-AGREE claims.
//!   2. Note the spec's prov:generatedAtTime as the AGREE moment.
//!   3. Append a "later" claim with synthesist:supersedes pointing at one of the
//!      snapshot claims, with prov:generatedAtTime AFTER the spec's.
//!   4. Open a GraphView (in-memory), rebuild from the log.
//!   5. Fetch the plan-at-risk overlay from the registry by name and run it.
//!   6. Assert exactly 1 hit, naming the spec as subject and the new
//!      superseding claim as object.

use nomograph_claim::graph_view::{rebuild, GraphView};
use nomograph_claim::log::LogWriter;
use nomograph_synthesist::overlay;
use serde_json::json;
use std::time::Instant;
use tempfile::TempDir;

/// Inline @context shared by all fixture documents in this test.
///
/// Reuses the canonical context from the synthesist crate so the e2e
/// exercises the shape the production dual-write actually emits.
fn ctx() -> serde_json::Value {
    nomograph_synthesist::wire_format::jsonld_context()
}

// ---------------------------------------------------------------------------
// Main acceptance test
// ---------------------------------------------------------------------------

/// Full workflow: spec -> AGREE snapshot -> post-AGREE supersession -> warning.
///
/// Steps:
///   a. Three pre-AGREE task claims (claim-alpha, claim-beta, claim-gamma).
///   b. A Spec that references all three in synthesist:agreeSnapshot, timestamped
///      at 2026-05-10T12:00:00Z (the AGREE moment).
///   c. A superseding claim that replaces claim-beta, timestamped at
///      2026-05-15T08:00:00Z (after AGREE).
///
/// Expected: exactly 1 hit, subject = spec-e2e, object = claim-beta-v2.
#[test]
fn plan_at_risk_e2e_one_hit_after_agree() {
    let tmp = TempDir::new().unwrap();
    let writer = LogWriter::new(tmp.path()).unwrap();

    // Step a: three pre-AGREE snapshot claims.
    for id in ["claim-alpha", "claim-beta", "claim-gamma"] {
        let doc = json!({
            "@context": ctx(),
            "@id": format!("synthesist:{}", id),
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-01T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": format!("Pre-agree task {}", id),
        });
        writer.append("user:local:agd", &doc).unwrap();
    }

    // Step b: the Spec at AGREE time.
    let spec_doc = json!({
        "@context": ctx(),
        "@id": "synthesist:spec-e2e",
        "@type": "synthesist:Spec",
        "prov:generatedAtTime": "2026-05-10T12:00:00.000Z",
        "prov:wasAttributedTo": "asserter:user:local:agd",
        "synthesist:summary": "E2E acceptance spec",
        "synthesist:agreeSnapshot": [
            "synthesist:claim-alpha",
            "synthesist:claim-beta",
            "synthesist:claim-gamma"
        ],
    });
    writer.append("user:local:agd", &spec_doc).unwrap();

    // Step c: post-AGREE supersession of claim-beta.
    let superseder = json!({
        "@context": ctx(),
        "@id": "synthesist:claim-beta-v2",
        "@type": "synthesist:Task",
        "prov:generatedAtTime": "2026-05-15T08:00:00.000Z",
        "prov:wasAttributedTo": "asserter:user:local:bob",
        "synthesist:supersedes": "synthesist:claim-beta",
        "synthesist:summary": "Revised beta task (post-agree)",
    });
    writer.append("user:local:bob", &superseder).unwrap();

    // Step d: open an in-memory GraphView and rebuild from the log.
    let view = GraphView::open_in_memory().unwrap();
    rebuild(&view, tmp.path()).unwrap();

    // Step e: fetch the overlay from the registry by name and time the run.
    let start = Instant::now();
    let plan_at_risk = overlay::find("plan-at-risk")
        .expect("plan-at-risk overlay must be registered");
    let hits = plan_at_risk.run(&view).unwrap();
    let elapsed_ms = start.elapsed().as_millis();

    // Acceptance criterion: runs in under 5 seconds (5000 ms).
    assert!(
        elapsed_ms < 5000,
        "overlay took {}ms, must be under 5000ms",
        elapsed_ms
    );

    // Step f: exactly 1 hit.
    assert_eq!(
        hits.len(),
        1,
        "expected exactly 1 plan-at-risk hit, got {}: {:?}",
        hits.len(),
        hits
    );

    let hit = &hits[0];

    // Subject is the spec.
    assert!(
        hit.subject.ends_with("spec-e2e"),
        "subject should be spec-e2e, got: {}",
        hit.subject
    );

    // Predicate is the canonical plan-at-risk predicate.
    assert_eq!(
        hit.predicate, "synthesist:planAtRisk",
        "predicate must be synthesist:planAtRisk, got: {}",
        hit.predicate
    );

    // Object is the superseding claim.
    assert!(
        hit.object.ends_with("claim-beta-v2"),
        "object should be claim-beta-v2, got: {}",
        hit.object
    );

    // Detail must reference the original superseded claim.
    let old = hit
        .detail
        .get("old_claim")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        old.ends_with("claim-beta"),
        "detail.old_claim should end with claim-beta, got: {}",
        old
    );
}

// ---------------------------------------------------------------------------
// Negative case: no supersession after AGREE -> zero hits.
// ---------------------------------------------------------------------------

/// A fully agreed spec with no post-AGREE supersession must return no hits.
#[test]
fn plan_at_risk_e2e_no_hit_when_plan_is_intact() {
    let tmp = TempDir::new().unwrap();
    let writer = LogWriter::new(tmp.path()).unwrap();

    for id in ["task-1", "task-2", "task-3"] {
        let doc = json!({
            "@context": ctx(),
            "@id": format!("synthesist:{}", id),
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-01T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": format!("Intact task {}", id),
        });
        writer.append("user:local:agd", &doc).unwrap();
    }

    let spec_doc = json!({
        "@context": ctx(),
        "@id": "synthesist:spec-intact",
        "@type": "synthesist:Spec",
        "prov:generatedAtTime": "2026-05-10T12:00:00.000Z",
        "prov:wasAttributedTo": "asserter:user:local:agd",
        "synthesist:summary": "Spec with intact plan",
        "synthesist:agreeSnapshot": [
            "synthesist:task-1",
            "synthesist:task-2",
            "synthesist:task-3"
        ],
    });
    writer.append("user:local:agd", &spec_doc).unwrap();

    // No post-AGREE supersession: plan remains intact.

    let view = GraphView::open_in_memory().unwrap();
    rebuild(&view, tmp.path()).unwrap();

    let plan_at_risk = overlay::find("plan-at-risk")
        .expect("plan-at-risk overlay must be registered");
    let hits = plan_at_risk.run(&view).unwrap();

    assert!(
        hits.is_empty(),
        "expected no hits for intact plan, got {:?}",
        hits
    );
}
