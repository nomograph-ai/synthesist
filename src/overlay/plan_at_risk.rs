//! Plan-at-risk overlay: flag specs whose agreed plan has been superseded.
//!
//! A spec is plan-at-risk when:
//!   - It has `synthesist:agreeSnapshot` (a set of claim IRIs captured at
//!     AGREE time), AND
//!   - At least one claim in that snapshot has been superseded by a newer
//!     claim whose `prov:generatedAtTime` is after the spec's own
//!     `prov:generatedAtTime` (the AGREE timestamp).
//!
//! One `OverlayResult` is emitted per (spec, superseding-claim) pair:
//!
//! ```text
//! subject   = spec IRI  (the at-risk spec)
//! predicate = "synthesist:planAtRisk"
//! object    = new_claim IRI  (the claim that superseded a snapshot member)
//! detail    = {"old_claim": ..., "stakeholder": ..., "new_at": ..., "spec_id": ...}
//! ```
//!
//! Ported to the gamma typed surface (C-2): the multi-hop
//! agreeSnapshot -> supersedes -> dateTime range query is gamma's H10
//! `plan_at_risk`. The canonical `now_iso` timestamp format makes the
//! `new_at > spec_agreed_at` comparison a correct lexical string compare.
//!
//! ## spec_id in detail
//!
//! `cmd_task ready` matches overlay hits against task records by raw
//! spec id. H10 returns the spec's CLAIM id; we read the spec doc's
//! `synthesist:id` prop and surface it as `detail.spec_id` (empty string
//! when the spec carries no id).

use anyhow::Result;
use nomograph_claim::gamma::Gamma;
use serde_json::json;

use super::{Overlay, OverlayResult};

/// Detects specs whose agreed plan snapshot contains a superseded claim.
pub struct PlanAtRiskOverlay;

impl Overlay for PlanAtRiskOverlay {
    fn name(&self) -> &str {
        "plan-at-risk"
    }

    fn description(&self) -> &str {
        "Flag specs whose agreed-plan snapshot contains a claim that has since been superseded."
    }

    fn run(&self, gamma: &Gamma) -> Result<Vec<OverlayResult>> {
        let spec_type = crate::wire_format::type_iri("spec");
        let agree_pred = crate::wire_format::predicate_iri("agree_snapshot");
        let id_pred = crate::wire_format::predicate_iri("id");

        let hits =
            gamma.plan_at_risk(&spec_type, &agree_pred, crate::wire_format::SUPERSEDES_PRED)?;

        let mut out = Vec::with_capacity(hits.len());
        for hit in hits {
            // Recover the spec's raw `synthesist:id` from its doc so the
            // ready-task matcher can key on it. Empty string when absent.
            let spec_id = gamma
                .doc(&hit.spec)?
                .as_ref()
                .and_then(|d| d.get(id_pred.as_str()))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            out.push(OverlayResult::with_detail(
                hit.spec,
                "synthesist:planAtRisk",
                hit.new_claim,
                json!({
                    "old_claim": hit.old_claim,
                    "stakeholder": hit.stakeholder,
                    "new_at": hit.new_at,
                    "spec_id": spec_id,
                }),
            ));
        }

        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nomograph_claim::log::LogWriter;
    use serde_json::json;
    use std::time::Instant;
    use tempfile::TempDir;

    /// Inline @context shared by all test fixture documents.
    fn ctx() -> serde_json::Value {
        crate::wire_format::jsonld_context()
    }

    /// Build a gamma index over the logs under `dir`.
    fn gamma_for(dir: &std::path::Path) -> Gamma {
        let mut g = Gamma::open_in_memory().unwrap();
        g.sync(dir).unwrap();
        g
    }

    // ------------------------------------------------------------------
    // Spec with agreeSnapshot referencing 3 claims; later supersession
    // of one of them. Overlay reports exactly 1 hit naming the spec and
    // the superseding claim.
    // ------------------------------------------------------------------
    #[test]
    fn one_hit_when_snapshot_claim_is_superseded() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        for id in ["claim-a", "claim-b", "claim-c"] {
            let doc = json!({
                "@context": ctx(),
                "@id": format!("synthesist:{}", id),
                "@type": "synthesist:Task",
                "prov:generatedAtTime": "2026-05-01T00:00:00.000Z",
                "prov:wasAttributedTo": "asserter:user:local:agd",
                "synthesist:summary": format!("Original {}", id),
            });
            writer.append("user:local:agd", &doc).unwrap();
        }

        let spec_doc = json!({
            "@context": ctx(),
            "@id": "synthesist:spec-alpha",
            "@type": "synthesist:Spec",
            "prov:generatedAtTime": "2026-05-05T12:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Alpha spec",
            "synthesist:agreeSnapshot": ["synthesist:claim-a", "synthesist:claim-b", "synthesist:claim-c"],
        });
        writer.append("user:local:agd", &spec_doc).unwrap();

        let superseder = json!({
            "@context": ctx(),
            "@id": "synthesist:claim-b-v2",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-10T09:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:bob",
            "synthesist:summary": "Revised claim-b",
            "synthesist:supersedes": "synthesist:claim-b",
        });
        writer.append("user:local:agd", &superseder).unwrap();

        let gamma = gamma_for(tmp.path());
        let overlay = PlanAtRiskOverlay;
        let hits = overlay.run(&gamma).unwrap();

        assert_eq!(
            hits.len(),
            1,
            "expected 1 hit (claim-b superseded after AGREE), got {}: {:?}",
            hits.len(),
            hits
        );
        assert!(
            hits[0].subject.ends_with("spec-alpha"),
            "subject should be spec-alpha, got: {}",
            hits[0].subject
        );
        assert!(
            hits[0].object.ends_with("claim-b-v2"),
            "object should be claim-b-v2 (the superseder), got: {}",
            hits[0].object
        );
        assert_eq!(hits[0].predicate, "synthesist:planAtRisk");
        let old = hits[0]
            .detail
            .get("old_claim")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            old.ends_with("claim-b"),
            "detail.old_claim should end with claim-b, got: {}",
            old
        );
    }

    // ------------------------------------------------------------------
    // spec_id surfaces in detail when synthesist:id is present.
    // ------------------------------------------------------------------
    #[test]
    fn spec_id_surfaces_in_detail_when_present() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        let claim_doc = json!({
            "@context": ctx(),
            "@id": "synthesist:claim-for-id-test",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-01T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Claim for id test",
        });
        writer.append("user:local:agd", &claim_doc).unwrap();

        let spec_doc = json!({
            "@context": ctx(),
            "@id": "synthesist:spec-with-id",
            "@type": "synthesist:Spec",
            "synthesist:id": "my-spec",
            "prov:generatedAtTime": "2026-05-05T12:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Spec with id",
            "synthesist:agreeSnapshot": ["synthesist:claim-for-id-test"],
        });
        writer.append("user:local:agd", &spec_doc).unwrap();

        let superseder = json!({
            "@context": ctx(),
            "@id": "synthesist:claim-for-id-test-v2",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-10T09:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Revised",
            "synthesist:supersedes": "synthesist:claim-for-id-test",
        });
        writer.append("user:local:agd", &superseder).unwrap();

        let gamma = gamma_for(tmp.path());
        let overlay = PlanAtRiskOverlay;
        let hits = overlay.run(&gamma).unwrap();

        assert_eq!(hits.len(), 1, "expected 1 hit, got {:?}", hits);
        let spec_id = hits[0]
            .detail
            .get("spec_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(
            spec_id, "my-spec",
            "detail.spec_id should be 'my-spec', got: {}",
            spec_id
        );
    }

    // ------------------------------------------------------------------
    // Spec with no agreeSnapshot returns zero hits.
    // ------------------------------------------------------------------
    #[test]
    fn no_hits_when_spec_has_no_agree_snapshot() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        let spec_doc = json!({
            "@context": ctx(),
            "@id": "synthesist:spec-no-snapshot",
            "@type": "synthesist:Spec",
            "prov:generatedAtTime": "2026-05-05T12:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Spec without a snapshot",
        });
        writer.append("user:local:agd", &spec_doc).unwrap();

        let superseder = json!({
            "@context": ctx(),
            "@id": "synthesist:claim-x-v2",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-10T09:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:supersedes": "synthesist:claim-x",
        });
        writer.append("user:local:agd", &superseder).unwrap();

        let gamma = gamma_for(tmp.path());
        let overlay = PlanAtRiskOverlay;
        let hits = overlay.run(&gamma).unwrap();

        assert!(
            hits.is_empty(),
            "expected no hits for spec without agreeSnapshot, got {:?}",
            hits
        );
    }

    // ------------------------------------------------------------------
    // Latency under 50 ms against a populated index.
    // ------------------------------------------------------------------
    #[test]
    fn latency_under_50ms_on_300_claim_view() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        for i in 0..300usize {
            let doc = json!({
                "@context": ctx(),
                "@id": format!("synthesist:claim-bulk-{:04}", i),
                "@type": "synthesist:Task",
                "prov:generatedAtTime": "2026-05-01T00:00:00.000Z",
                "prov:wasAttributedTo": "asserter:user:local:agd",
                "synthesist:summary": format!("Bulk task {}", i),
            });
            writer.append("user:local:agd", &doc).unwrap();
        }

        for id in ["snap-1", "snap-2", "snap-3"] {
            let doc = json!({
                "@context": ctx(),
                "@id": format!("synthesist:claim-{}", id),
                "@type": "synthesist:Task",
                "prov:generatedAtTime": "2026-05-01T00:00:00.000Z",
                "prov:wasAttributedTo": "asserter:user:local:agd",
                "synthesist:summary": format!("Snap claim {}", id),
            });
            writer.append("user:local:agd", &doc).unwrap();
        }
        let spec_doc = json!({
            "@context": ctx(),
            "@id": "synthesist:spec-perf",
            "@type": "synthesist:Spec",
            "prov:generatedAtTime": "2026-05-05T12:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Perf spec",
            "synthesist:agreeSnapshot": ["synthesist:claim-snap-1", "synthesist:claim-snap-2", "synthesist:claim-snap-3"],
        });
        writer.append("user:local:agd", &spec_doc).unwrap();

        let gamma = gamma_for(tmp.path());
        let overlay = PlanAtRiskOverlay;
        let start = Instant::now();
        let hits = overlay.run(&gamma).unwrap();
        let elapsed_ms = start.elapsed().as_millis();

        assert!(
            hits.is_empty(),
            "expected no hits in perf fixture, got {:?}",
            hits
        );
        assert!(
            elapsed_ms < 50,
            "overlay latency {}ms exceeded 50ms budget (304-claim index)",
            elapsed_ms
        );
    }

    // ------------------------------------------------------------------
    // Supersession that predates the AGREE timestamp must NOT trigger.
    // ------------------------------------------------------------------
    #[test]
    fn no_hit_when_supersession_predates_agree() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        let orig = json!({
            "@context": ctx(),
            "@id": "synthesist:claim-old",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-04-01T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Original old claim",
        });
        writer.append("user:local:agd", &orig).unwrap();

        let superseder = json!({
            "@context": ctx(),
            "@id": "synthesist:claim-old-v2",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-04-15T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:supersedes": "synthesist:claim-old",
        });
        writer.append("user:local:agd", &superseder).unwrap();

        let spec_doc = json!({
            "@context": ctx(),
            "@id": "synthesist:spec-early-super",
            "@type": "synthesist:Spec",
            "prov:generatedAtTime": "2026-05-01T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Spec with early supersession",
            "synthesist:agreeSnapshot": ["synthesist:claim-old"],
        });
        writer.append("user:local:agd", &spec_doc).unwrap();

        let gamma = gamma_for(tmp.path());
        let overlay = PlanAtRiskOverlay;
        let hits = overlay.run(&gamma).unwrap();

        assert!(
            hits.is_empty(),
            "supersession before AGREE should not be at-risk, got {:?}",
            hits
        );
    }
}
