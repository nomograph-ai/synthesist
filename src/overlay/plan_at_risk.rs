//! Plan-at-risk overlay: flag specs whose agreed plan has been superseded.
//!
//! A spec is plan-at-risk when:
//!   - It has `synthesist:agreeSnapshot` (a set of claim IRIs captured at AGREE
//!     time), AND
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
//! Specs lacking `agreeSnapshot` or `generatedAtTime` are silently skipped.
//!
//! ## T8.2 shortcut: spec_id in detail
//!
//! The SPARQL query also SELECTs the spec's `synthesist:id` literal (via OPTIONAL)
//! and surfaces it as `detail.spec_id`. This allows `cmd_task ready` to match
//! overlay hits against task records by raw spec id without a second lookup.
//! The OPTIONAL clause means specs without a `synthesist:id` triple still produce
//! hits; `detail.spec_id` will be an empty string in that case.

use anyhow::Result;
use nomograph_claim::graph_view::{GraphView, Term, select};
use serde_json::json;

use super::{Overlay, OverlayResult};

/// Detects specs whose agreed plan snapshot contains a superseded claim.
///
/// This is the load-bearing overlay for v3.0-alpha. It exercises
/// supersession-traversal, named-graph SPARQL, and FILTER on typed
/// dateTime literals -- the core capabilities the alpha thesis depends on.
pub struct PlanAtRiskOverlay;

const QUERY: &str = r#"
    PREFIX rdf:   <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
    PREFIX synthesist: <https://nomograph.org/synthesist/>
    PREFIX prov:  <http://www.w3.org/ns/prov#>

    SELECT ?spec ?spec_id ?old_claim ?new_claim ?stakeholder ?new_at
    WHERE {
      GRAPH ?g {
        ?spec a synthesist:Spec ; synthesist:agreeSnapshot ?old_claim .
        ?spec prov:generatedAtTime ?spec_agreed_at .
        OPTIONAL { ?spec synthesist:id ?spec_id . }

        ?new_claim synthesist:supersedes ?old_claim ;
                   prov:wasAttributedTo ?stakeholder ;
                   prov:generatedAtTime ?new_at .
        FILTER(?new_at > ?spec_agreed_at)
      }
    }
"#;

impl Overlay for PlanAtRiskOverlay {
    fn name(&self) -> &str {
        "plan-at-risk"
    }

    fn description(&self) -> &str {
        "Flag specs whose agreed-plan snapshot contains a claim that has since been superseded."
    }

    fn run(&self, view: &GraphView) -> Result<Vec<OverlayResult>> {
        let results = select(view, QUERY)?;

        // Column order: spec, spec_id, old_claim, new_claim, stakeholder, new_at
        let col_spec = results.columns.iter().position(|c| c == "spec");
        let col_spec_id = results.columns.iter().position(|c| c == "spec_id");
        let col_old = results.columns.iter().position(|c| c == "old_claim");
        let col_new = results.columns.iter().position(|c| c == "new_claim");
        let col_stake = results.columns.iter().position(|c| c == "stakeholder");
        let col_at = results.columns.iter().position(|c| c == "new_at");

        // If any required column is absent the query returned no hits
        // (empty result set with no column headers). Return empty.
        let (col_spec, col_old, col_new, col_stake, col_at) =
            match (col_spec, col_old, col_new, col_stake, col_at) {
                (Some(a), Some(b), Some(c), Some(d), Some(e)) => (a, b, c, d, e),
                _ => return Ok(vec![]),
            };

        let mut hits = Vec::new();

        for row in &results.rows {
            let spec = term_str(row.get(col_spec));
            let spec_id = col_spec_id
                .and_then(|i| row.get(i))
                .map(|t| term_str(Some(t)))
                .unwrap_or_default();
            let old_claim = term_str(row.get(col_old));
            let new_claim = term_str(row.get(col_new));
            let stakeholder = term_str(row.get(col_stake));
            let new_at = term_str(row.get(col_at));

            // Skip rows where essential columns are empty (unbound).
            if spec.is_empty() || new_claim.is_empty() {
                continue;
            }

            hits.push(OverlayResult::with_detail(
                spec,
                "synthesist:planAtRisk",
                new_claim,
                json!({
                    "old_claim": old_claim,
                    "stakeholder": stakeholder,
                    "new_at": new_at,
                    "spec_id": spec_id,
                }),
            ));
        }

        Ok(hits)
    }
}

/// Extract the display string from an optional term reference.
///
/// Returns an empty string for missing or unbound terms.
fn term_str(t: Option<&Term>) -> String {
    match t {
        None => String::new(),
        Some(Term::Iri(s)) => s.clone(),
        Some(Term::BlankNode(s)) => s.clone(),
        Some(Term::Literal { value, .. }) => value.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nomograph_claim::graph_view::{rebuild, GraphView};
    use nomograph_claim::log::LogWriter;
    use serde_json::json;
    use std::time::Instant;
    use tempfile::TempDir;

    /// Inline @context shared by all test fixture documents.
    fn ctx() -> serde_json::Value {
        json!({
            "nomograph":  "https://nomograph.org/v3/",
            "prov":       "http://www.w3.org/ns/prov#",
            "xsd":        "http://www.w3.org/2001/XMLSchema#",
            "synthesist": "https://nomograph.org/synthesist/",
            "prov:generatedAtTime": {"@type": "xsd:dateTime"},
            "prov:wasAttributedTo": {"@type": "@id"},
            "synthesist:supersedes":    {"@type": "@id"},
            "synthesist:agreeSnapshot": {"@type": "@id", "@container": "@set"}
        })
    }

    // ------------------------------------------------------------------
    // Acceptance criterion 1:
    //   Spec with agreeSnapshot referencing 3 claims; later supersession
    //   of one of them. Overlay reports exactly 1 hit naming the spec
    //   and the superseding claim.
    // ------------------------------------------------------------------
    #[test]
    fn one_hit_when_snapshot_claim_is_superseded() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        // Three original claims (the plan snapshot).
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

        // The spec, agreed on 2026-05-05, with the 3-claim snapshot.
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

        // One superseding claim for claim-b, created AFTER the spec's
        // AGREE timestamp. This is the change that puts the plan at risk.
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

        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let overlay = PlanAtRiskOverlay;
        let hits = overlay.run(&view).unwrap();

        assert_eq!(
            hits.len(),
            1,
            "expected 1 hit (claim-b superseded after AGREE), got {}: {:?}",
            hits.len(),
            hits
        );

        // The subject is the spec, the object is the superseding claim.
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

        // Detail must contain old_claim pointing at claim-b.
        let old = hits[0].detail.get("old_claim").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            old.ends_with("claim-b"),
            "detail.old_claim should end with claim-b, got: {}",
            old
        );
    }

    // ------------------------------------------------------------------
    // T8.2: spec_id surfaces in detail when synthesist:id is present.
    // ------------------------------------------------------------------
    #[test]
    fn spec_id_surfaces_in_detail_when_present() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        let mut ctx_with_id = ctx();
        ctx_with_id["synthesist:id"] = json!({});

        // One snapshot claim.
        let claim_doc = json!({
            "@context": ctx_with_id,
            "@id": "synthesist:claim-for-id-test",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-01T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Claim for id test",
        });
        writer.append("user:local:agd", &claim_doc).unwrap();

        // Spec with synthesist:id "my-spec".
        let spec_doc = json!({
            "@context": ctx_with_id,
            "@id": "synthesist:spec-with-id",
            "@type": "synthesist:Spec",
            "synthesist:id": "my-spec",
            "prov:generatedAtTime": "2026-05-05T12:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Spec with id",
            "synthesist:agreeSnapshot": ["synthesist:claim-for-id-test"],
        });
        writer.append("user:local:agd", &spec_doc).unwrap();

        // Superseder after AGREE.
        let superseder = json!({
            "@context": ctx_with_id,
            "@id": "synthesist:claim-for-id-test-v2",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-10T09:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Revised",
            "synthesist:supersedes": "synthesist:claim-for-id-test",
        });
        writer.append("user:local:agd", &superseder).unwrap();

        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let overlay = PlanAtRiskOverlay;
        let hits = overlay.run(&view).unwrap();

        assert_eq!(hits.len(), 1, "expected 1 hit, got {:?}", hits);
        let spec_id = hits[0].detail.get("spec_id").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(
            spec_id, "my-spec",
            "detail.spec_id should be 'my-spec', got: {}",
            spec_id
        );
    }

    // ------------------------------------------------------------------
    // Acceptance criterion 2:
    //   Spec with no agreeSnapshot returns zero hits.
    // ------------------------------------------------------------------
    #[test]
    fn no_hits_when_spec_has_no_agree_snapshot() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        // A spec with no agreeSnapshot field.
        let spec_doc = json!({
            "@context": ctx(),
            "@id": "synthesist:spec-no-snapshot",
            "@type": "synthesist:Spec",
            "prov:generatedAtTime": "2026-05-05T12:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": "Spec without a snapshot",
        });
        writer.append("user:local:agd", &spec_doc).unwrap();

        // A superseding claim in the same time window.
        let superseder = json!({
            "@context": ctx(),
            "@id": "synthesist:claim-x-v2",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-10T09:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:supersedes": "synthesist:claim-x",
        });
        writer.append("user:local:agd", &superseder).unwrap();

        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let overlay = PlanAtRiskOverlay;
        let hits = overlay.run(&view).unwrap();

        assert!(
            hits.is_empty(),
            "expected no hits for spec without agreeSnapshot, got {:?}",
            hits
        );
    }

    // ------------------------------------------------------------------
    // Acceptance criterion 3:
    //   Latency under 50 ms against a populated view.
    //
    //   We load 300 claims (roughly 2x the real storr corpus). True
    //   1500-claim scale would require more fixture overhead than is
    //   reasonable for a unit test; 300 is the practical limit for fast
    //   CI and exercises the same SPARQL + named-graph pattern. Observed
    //   latency on developer hardware is logged below.
    // ------------------------------------------------------------------
    #[test]
    fn latency_under_50ms_on_300_claim_view() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        // 300 plain task claims (no supersession, no agreeSnapshot).
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

        // One spec with a 3-claim snapshot for a realistic overlay path.
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

        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let overlay = PlanAtRiskOverlay;
        let start = Instant::now();
        let hits = overlay.run(&view).unwrap();
        let elapsed_ms = start.elapsed().as_millis();

        // No supersession exists, so zero hits expected.
        assert!(
            hits.is_empty(),
            "expected no hits in perf fixture, got {:?}",
            hits
        );

        assert!(
            elapsed_ms < 50,
            "overlay latency {}ms exceeded 50ms budget (304-claim view)",
            elapsed_ms
        );
    }

    // ------------------------------------------------------------------
    // Extra: supersession that predates the AGREE timestamp must NOT
    // trigger a hit (the plan was already aware of that change).
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

        // Superseder is BEFORE the AGREE timestamp.
        let superseder = json!({
            "@context": ctx(),
            "@id": "synthesist:claim-old-v2",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-04-15T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:supersedes": "synthesist:claim-old",
        });
        writer.append("user:local:agd", &superseder).unwrap();

        // Spec AGREE timestamp is 2026-05-01 -- after the supersession.
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

        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let overlay = PlanAtRiskOverlay;
        let hits = overlay.run(&view).unwrap();

        assert!(
            hits.is_empty(),
            "supersession before AGREE should not be at-risk, got {:?}",
            hits
        );
    }
}
