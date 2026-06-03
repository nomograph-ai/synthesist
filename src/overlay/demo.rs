//! Demo overlay: count synthesist claims by (type, status).
//!
//! Plumbing demo (not a diagnostic). It proves the registry, trait
//! dispatch, and CLI wiring end to end. Ported to the gamma surface
//! (C-2): it walks the live heads of each synthesist type (H2) and
//! groups them by status, emitting one `OverlayResult` per (type,
//! status) pair found.
//!
//! Output shape per hit:
//!   subject   = the type IRI (e.g. "synthesist:Task")
//!   predicate = "synthesist:status"
//!   object    = the status literal (e.g. "pending")
//!   detail    = {"count": <n>}

use std::collections::BTreeMap;

use anyhow::Result;
use nomograph_claim::gamma::Gamma;
use serde_json::json;

use super::{Overlay, OverlayResult};

/// The synthesist types the demo scans. Workflow types only; the demo's
/// fixtures and CLI usage are workflow-scoped.
const TYPES: &[&str] = &[
    "tree",
    "spec",
    "task",
    "discovery",
    "campaign",
    "session",
    "phase",
    "outcome",
];

/// Counts synthesist claims grouped by (type, status).
pub struct DemoTasksByStatus;

impl Overlay for DemoTasksByStatus {
    fn name(&self) -> &str {
        "demo-tasks-by-status"
    }

    fn description(&self) -> &str {
        "Count synthesist claims by status. Demo overlay; proves the registry and query plumbing."
    }

    fn run(&self, gamma: &Gamma) -> Result<Vec<OverlayResult>> {
        let status_pred = crate::wire_format::predicate_iri("status");
        let mut hits = Vec::new();

        for ty in TYPES {
            let type_iri = crate::wire_format::type_iri(ty);
            let live = gamma.live_heads(&type_iri, crate::wire_format::SUPERSEDES_PRED)?;

            // Group the live heads by their status value.
            let mut by_status: BTreeMap<String, u64> = BTreeMap::new();
            for id in live {
                if let Some(status) = gamma.scalar(&id, &status_pred)?
                    && !status.is_empty()
                {
                    *by_status.entry(status).or_insert(0) += 1;
                }
            }

            for (status, count) in by_status {
                hits.push(OverlayResult::with_detail(
                    type_iri.clone(),
                    "synthesist:status",
                    status,
                    json!({ "count": count }),
                ));
            }
        }

        Ok(hits)
    }
}
