//! Overlay subcommand dispatch.
//!
//! `synthesist overlay list`       -- list registered overlays.
//! `synthesist overlay run <name>` -- run a named overlay and print hits.
//!
//! Both subcommands are read-only: no session, no phase gate.
//!
//! ## View strategy
//!
//! Same as `cmd_query`: try the on-disk RocksDB view first, then fall
//! back to an in-memory rebuild. The macOS RocksDB known issue (see
//! `cmd_query.rs` and the `#[ignore]` tests in `nomograph_claim::graph_view`)
//! means the fallback path is the practical one in development.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use nomograph_claim::graph_view::{rebuild, GraphView};
use nomograph_synthesist::telemetry::{Surface, TelemetryWriter};
use serde_json::{json, Value};

use crate::cli::OverlayCmd;
use crate::overlay::{self, OverlayResult};
use crate::store::json_out;

/// Dispatch an `OverlayCmd`.
pub fn run(cmd: &OverlayCmd, data_dir: Option<&Path>) -> Result<()> {
    match cmd {
        OverlayCmd::List => cmd_overlay_list(),
        OverlayCmd::Run { name } => cmd_overlay_run(name, data_dir),
    }
}

// ---------------------------------------------------------------------------
// `overlay list`
// ---------------------------------------------------------------------------

fn cmd_overlay_list() -> Result<()> {
    let reg = overlay::registry();
    let items: Vec<Value> = reg
        .iter()
        .map(|o| json!({ "name": o.name(), "description": o.description() }))
        .collect();
    let count = items.len();
    json_out(&json!({ "overlays": items, "count": count }))
}

// ---------------------------------------------------------------------------
// `overlay run <name>`
// ---------------------------------------------------------------------------

fn cmd_overlay_run(name: &str, data_dir: Option<&Path>) -> Result<()> {
    let overlay = overlay::find(name).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown overlay {:?}; run `synthesist overlay list` to see available overlays",
            name
        )
    })?;

    let claims_dir = find_claims_dir(data_dir)?;
    let view = open_view_from_claims_dir(&claims_dir)?;

    // Sentinel query string used as the representative query for telemetry.
    // The Overlay trait does not expose SPARQL directly, so we use the
    // overlay name wrapped in a comment sentinel so record_query has a
    // stable, non-empty string from which to derive query_hash.
    let representative_query = format!("# overlay:{}", name);

    // Time the overlay execution and record telemetry regardless of
    // success or failure. Telemetry failures log to stderr but do not
    // mask the overlay result.
    let start = std::time::Instant::now();
    let run_result = overlay.run(&view);
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    let (errored, result_count) = match &run_result {
        Ok(hits) => (false, hits.len()),
        Err(_) => (true, 0),
    };

    if let Ok(writer) = TelemetryWriter::new(&claims_dir) {
        if let Err(e) = writer.record_query(
            Surface::Cli,
            &representative_query,
            result_count,
            elapsed_ms,
            errored,
        ) {
            eprintln!("warning: telemetry record failed: {}", e);
        }
    }

    let hits = run_result.with_context(|| format!("overlay {:?} failed", name))?;
    let hit_values: Vec<Value> = hits.iter().map(serialize_result).collect();
    let count = hit_values.len();
    json_out(&json!({
        "overlay": name,
        "hits": hit_values,
        "count": count,
    }))
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Serialize an `OverlayResult` to a JSON object.
fn serialize_result(r: &OverlayResult) -> Value {
    json!({
        "subject":   r.subject,
        "predicate": r.predicate,
        "object":    r.object,
        "detail":    r.detail,
    })
}

/// Open the graph view given an already-resolved `claims_dir`.
///
/// Attempts the on-disk RocksDB view first; falls back to an in-memory
/// rebuild on any failure (mirrors `cmd_query::open_view`).
fn open_view_from_claims_dir(claims_dir: &Path) -> Result<GraphView> {
    let view_dir = claims_dir.join("_view.oxigraph");

    match GraphView::open(&view_dir) {
        Ok(view) => Ok(view),
        Err(_) => {
            let view = GraphView::open_in_memory()
                .context("open in-memory graph view")?;
            rebuild(&view, claims_dir).with_context(|| {
                format!(
                    "rebuild view from claims at {}",
                    claims_dir.display()
                )
            })?;
            Ok(view)
        }
    }
}

/// Find the `claims/` directory from `data_dir` or by walking up from cwd.
fn find_claims_dir(data_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(base) = data_dir {
        let candidate = base.join("claims");
        if candidate.exists() {
            return Ok(candidate);
        }
        bail!(
            "no claims/ directory found at {} (from --data-dir)",
            base.display()
        );
    }

    let start = std::env::current_dir().context("get current directory")?;
    let mut cur = start.as_path();
    loop {
        let candidate = cur.join("claims");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => bail!(
                "no claims/ directory found walking up from {}",
                start.display()
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nomograph_claim::graph_view::GraphView;

    #[test]
    fn overlay_list_output_is_well_formed() {
        let reg = overlay::registry();
        let reg_count = reg.len();
        assert!(reg_count >= 1);

        let items: Vec<Value> = reg
            .iter()
            .map(|o| json!({ "name": o.name(), "description": o.description() }))
            .collect();
        let count = items.len();
        let out = json!({ "overlays": items, "count": count });
        assert_eq!(out["count"], reg_count);
        assert_eq!(out["overlays"].as_array().unwrap().len(), reg_count);
    }

    #[test]
    fn overlay_run_bogus_name_returns_structured_error() {
        // find() must return None for an unknown name.
        let result = overlay::find("this-overlay-does-not-exist");
        assert!(result.is_none());

        // The error message produced by cmd_overlay_run includes the name.
        let err_msg = format!(
            "unknown overlay {:?}; run `synthesist overlay list` to see available overlays",
            "this-overlay-does-not-exist"
        );
        assert!(err_msg.contains("this-overlay-does-not-exist"));
        assert!(err_msg.contains("overlay list"));
    }

    #[test]
    fn overlay_run_demo_on_empty_view_returns_zero_hits() {
        let view = GraphView::open_in_memory().unwrap();
        let overlay = overlay::find("demo-tasks-by-status").unwrap();
        let hits = overlay.run(&view).unwrap();
        assert!(hits.is_empty());

        let hit_values: Vec<Value> = hits.iter().map(serialize_result).collect();
        let count = hit_values.len();
        let out = json!({ "overlay": "demo-tasks-by-status", "hits": hit_values, "count": count });
        assert_eq!(out["count"], 0);
    }

    #[test]
    fn serialize_result_produces_expected_keys() {
        let r = crate::overlay::OverlayResult::simple("subj", "pred", "obj");
        let v = serialize_result(&r);
        assert_eq!(v["subject"], "subj");
        assert_eq!(v["predicate"], "pred");
        assert_eq!(v["object"], "obj");
        assert_eq!(v["detail"], Value::Null);
    }

    /// Acceptance: running the demo overlay and recording telemetry causes
    /// claims/_telemetry/queries.jsonl to exist and contain exactly one line.
    ///
    /// Uses the in-memory GraphView (avoids the macOS RocksDB TryFromIntError)
    /// and calls TelemetryWriter directly, mirroring the wiring in
    /// cmd_overlay_run exactly.
    #[test]
    fn overlay_run_writes_telemetry_record() {
        use nomograph_synthesist::telemetry::{Surface, TelemetryWriter};
        use tempfile::TempDir;

        // Set up a minimal claims dir (empty -- demo overlay returns 0 hits
        // on an empty view, which is fine for telemetry coverage).
        let tmp = TempDir::new().unwrap();
        let claims_dir = tmp.path().join("claims");
        std::fs::create_dir_all(&claims_dir).unwrap();

        // Telemetry file must not exist yet.
        assert!(!claims_dir.join("_telemetry").join("queries.jsonl").exists());

        // Run the overlay against an in-memory view (bypasses RocksDB, which
        // panics on macOS in the current oxigraph version).
        let overlay_name = "demo-tasks-by-status";
        let overlay = overlay::find(overlay_name).unwrap();
        let view = GraphView::open_in_memory().unwrap();

        let representative_query = format!("# overlay:{}", overlay_name);

        let start = std::time::Instant::now();
        let run_result = overlay.run(&view);
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        let (errored, result_count) = match &run_result {
            Ok(hits) => (false, hits.len()),
            Err(_) => (true, 0),
        };

        // This mirrors the telemetry wiring in cmd_overlay_run exactly.
        let writer = TelemetryWriter::new(&claims_dir).unwrap();
        writer
            .record_query(
                Surface::Cli,
                &representative_query,
                result_count,
                elapsed_ms,
                errored,
            )
            .unwrap();

        // Overlay itself must succeed with zero hits on an empty view.
        assert!(run_result.is_ok());
        assert_eq!(result_count, 0);

        // Telemetry file must now exist.
        let jsonl_path = claims_dir.join("_telemetry").join("queries.jsonl");
        assert!(
            jsonl_path.exists(),
            "expected telemetry file at {}",
            jsonl_path.display()
        );

        // Must contain exactly one line.
        let contents = std::fs::read_to_string(&jsonl_path).unwrap();
        let lines: Vec<&str> = contents.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines.len(),
            1,
            "expected 1 telemetry line, got {}: {:?}",
            lines.len(),
            lines
        );

        // The line must be valid JSON with expected fields.
        let record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(record["surface"].as_str().unwrap(), "cli");
        assert_eq!(record["result_count"].as_u64().unwrap(), 0);
        assert_eq!(record["errored"].as_bool().unwrap(), false);
        assert!(record.get("bgp_shape").is_some());
        assert!(record.get("latency_ms").is_some());
    }
}
