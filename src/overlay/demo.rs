//! Demo overlay: count synthesist:Task claims by status.
//!
//! This overlay demonstrates the plumbing: it runs a SPARQL SELECT
//! counting tasks by status and returns one `OverlayResult` per
//! (type, status) pair found in the view. It is not a diagnostic
//! overlay (plan-at-risk lands in T8.1); it exists to prove the
//! registry, trait dispatch, and CLI wiring all work end to end.
//!
//! Output shape per hit:
//!   subject   = the task type IRI (e.g. "https://nomograph.org/synthesist/Task")
//!   predicate = "synthesist:status"
//!   object    = the status literal (e.g. "pending")
//!   detail    = {"count": <n>}

use anyhow::Result;
use nomograph_claim::graph_view::{GraphView, Term, select};
use serde_json::json;

use super::{Overlay, OverlayResult};

/// Counts synthesist:Task claims grouped by status.
///
/// Returns one result per distinct (type, status) pair. The result is
/// diagnostic in nature: an agent or human can see at a glance how many
/// tasks are in each state across the entire graph view.
pub struct DemoTasksByStatus;

impl Overlay for DemoTasksByStatus {
    fn name(&self) -> &str {
        "demo-tasks-by-status"
    }

    fn description(&self) -> &str {
        "Count synthesist:Task claims by status. Demo overlay; proves the registry and query plumbing."
    }

    fn run(&self, view: &GraphView) -> Result<Vec<OverlayResult>> {
        // Count tasks by type and status. Uses GRAPH ?g to sweep named
        // graphs; the default graph is not queried because claim loading
        // routes synthesist: types into the synth named graph.
        let query = r#"
            PREFIX rdf:   <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
            PREFIX synthesist: <https://nomograph.org/synthesist/>
            SELECT ?type ?status (COUNT(?c) AS ?n)
            WHERE {
                GRAPH ?g {
                    ?c rdf:type ?type .
                    OPTIONAL { ?c synthesist:status ?status }
                }
            }
            GROUP BY ?type ?status
            ORDER BY ?type ?status
        "#;

        let results = select(view, query)?;

        let mut hits = Vec::new();
        for row in &results.rows {
            if row.len() < 3 {
                continue;
            }
            let type_str = match &row[0] {
                Term::Iri(s) => s.clone(),
                other => other.as_str().to_string(),
            };
            let status_str = match &row[1] {
                Term::Literal { value, .. } => value.clone(),
                Term::Iri(s) => s.clone(),
                // Unbound OPTIONAL: no status predicate on this cluster.
                Term::BlankNode(_) => "(no status)".to_string(),
            };
            // Skip rows where status is empty (OPTIONAL was unbound and
            // came through as an empty literal from the SelectResults
            // default-fill logic).
            if status_str.is_empty() {
                continue;
            }
            let count_str = match &row[2] {
                Term::Literal { value, .. } => value.clone(),
                other => other.as_str().to_string(),
            };
            let count: u64 = count_str.parse().unwrap_or(0);

            hits.push(OverlayResult::with_detail(
                type_str,
                "synthesist:status",
                status_str,
                json!({ "count": count }),
            ));
        }

        Ok(hits)
    }
}
