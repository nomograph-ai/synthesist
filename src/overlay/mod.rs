//! Overlay framework: named SPARQL-backed analysis passes over the graph view.
//!
//! An overlay is a named analysis that runs a SPARQL query (or a sequence of
//! queries) against the current graph view and returns structured hits.
//! Overlays are composable, independently testable, and registered centrally
//! so CLI commands (`overlay list`, `overlay run`) can discover them without
//! hard-coding names.
//!
//! ## Adding a new overlay
//!
//! 1. Create `src/overlay/<name>.rs` implementing `Overlay`.
//! 2. Add `mod <name>;` below.
//! 3. Push an instance onto the vec in `registry()`.
//!
//! The trait is intentionally minimal. Future overlays (plan-at-risk from
//! T8.1) slot in by implementing the same two methods.

use anyhow::Result;
use nomograph_claim::gamma::Gamma;
use serde_json::Value;

mod demo;
mod plan_at_risk;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A single hit returned by an overlay.
///
/// Fields map to the subject, predicate, and object of the RDF triple (or
/// triple-pattern) that triggered the hit. `detail` carries overlay-specific
/// extra context (e.g. a count, a timestamp, a severity label). Use
/// `serde_json::Value::Null` when there is no extra detail.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayResult {
    /// IRI or compact identifier for the resource flagged by this hit.
    pub subject: String,
    /// IRI or compact identifier for the property that caused the hit.
    pub predicate: String,
    /// IRI, literal value, or compact identifier for the object of the hit.
    pub object: String,
    /// Overlay-specific supplemental data. Null when not applicable.
    pub detail: Value,
}

impl OverlayResult {
    /// Construct a result with supplemental detail.
    pub fn with_detail(
        subject: impl Into<String>,
        predicate: impl Into<String>,
        object: impl Into<String>,
        detail: Value,
    ) -> Self {
        Self {
            subject: subject.into(),
            predicate: predicate.into(),
            object: object.into(),
            detail,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// An analysis pass that runs against the graph view.
///
/// Implementors are registered in `registry()` and dispatched by the
/// `overlay run` CLI subcommand. The trait is object-safe so overlays can
/// be collected as `Vec<Box<dyn Overlay>>`.
pub trait Overlay: Send + Sync {
    /// Short, kebab-case name used to dispatch the overlay from the CLI.
    ///
    /// Must be unique across all registered overlays. Collisions are
    /// caught at startup in `registry()` via a debug assertion.
    fn name(&self) -> &str;

    /// Human-readable, one-sentence description for `overlay list`.
    fn description(&self) -> &str;

    /// Execute the overlay against the gamma index and return any hits.
    ///
    /// An empty vec is a valid result: it means the overlay found no
    /// issues. Errors (index failures, unexpected state) are returned as
    /// `Err`.
    fn run(&self, gamma: &Gamma) -> Result<Vec<OverlayResult>>;
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Return all registered overlays in definition order.
///
/// Callers that need to dispatch by name call this and iterate; the list
/// is short enough that linear scan is fine for the alpha.
pub fn registry() -> Vec<Box<dyn Overlay>> {
    let overlays: Vec<Box<dyn Overlay>> = vec![
        Box::new(demo::DemoTasksByStatus),
        Box::new(plan_at_risk::PlanAtRiskOverlay),
    ];

    // Catch duplicate names early (debug builds only).
    #[cfg(debug_assertions)]
    {
        let mut seen = std::collections::HashSet::new();
        for o in &overlays {
            let inserted = seen.insert(o.name().to_string());
            debug_assert!(inserted, "duplicate overlay name: {}", o.name());
        }
    }

    overlays
}

/// Find a registered overlay by name, or return `None`.
pub fn find(name: &str) -> Option<Box<dyn Overlay>> {
    registry().into_iter().find(|o| o.name() == name)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_non_empty() {
        let reg = registry();
        assert!(!reg.is_empty(), "registry must have at least one overlay");
    }

    #[test]
    fn registry_names_are_unique() {
        let reg = registry();
        let mut names = std::collections::HashSet::new();
        for o in &reg {
            assert!(
                names.insert(o.name()),
                "duplicate overlay name: {}",
                o.name()
            );
        }
    }

    #[test]
    fn registry_names_are_kebab_case() {
        let reg = registry();
        for o in &reg {
            let name = o.name();
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "overlay name must be kebab-case (lowercase letters and hyphens only): {name}"
            );
        }
    }

    #[test]
    fn find_known_overlay_returns_some() {
        assert!(find("demo-tasks-by-status").is_some());
    }

    #[test]
    fn find_unknown_overlay_returns_none() {
        assert!(find("does-not-exist").is_none());
    }

    // The `simple` constructor was removed (compile-warning dead code).
    // Callers that want no supplemental detail pass `Value::Null` to
    // `with_detail`; the test below covers that path implicitly via
    // the demo overlay.

    #[test]
    fn overlay_result_with_detail_carries_value() {
        let r = OverlayResult::with_detail("s", "p", "o", serde_json::json!({"count": 3}));
        assert_eq!(r.detail, serde_json::json!({"count": 3}));
    }

    #[test]
    fn demo_overlay_runs_against_empty_view() {
        let gamma = Gamma::open_in_memory().unwrap();
        let overlay = find("demo-tasks-by-status").unwrap();
        let hits = overlay.run(&gamma).unwrap();
        // Empty index: zero task types, so zero hits.
        assert!(
            hits.is_empty(),
            "expected no hits on an empty index, got {:?}",
            hits
        );
    }

    #[test]
    fn demo_overlay_returns_hits_on_populated_view() {
        use nomograph_claim::log::LogWriter;
        use serde_json::json;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        for i in 0..5 {
            let doc = json!({
                "@context": crate::wire_format::jsonld_context(),
                "@id": format!("synthesist:claim/task{}", i),
                "@type": "synthesist:Task",
                "prov:generatedAtTime": "2026-05-28T00:00:00.000Z",
                "prov:wasAttributedTo": "asserter:user:local:agd",
                "synthesist:summary": format!("Task {}", i),
                "synthesist:status": "pending",
            });
            writer.append("user:local:agd", &doc).unwrap();
        }

        let mut gamma = Gamma::open_in_memory().unwrap();
        gamma.sync(tmp.path()).unwrap();

        let overlay = find("demo-tasks-by-status").unwrap();
        let hits = overlay.run(&gamma).unwrap();

        // One hit per distinct (type, status) combination. The 5 claims
        // are all synthesist:Task with status "pending", so we expect one hit.
        assert_eq!(
            hits.len(),
            1,
            "expected 1 hit (synthesist:Task/pending), got {:?}",
            hits
        );
        assert_eq!(hits[0].predicate, "synthesist:status");
        assert_eq!(hits[0].object, "pending");
    }
}
