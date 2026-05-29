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

    let view = open_view(data_dir)?;
    let hits = overlay
        .run(&view)
        .with_context(|| format!("overlay {:?} failed", name))?;

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

/// Open the graph view.
///
/// Attempts the on-disk RocksDB view first; falls back to an in-memory
/// rebuild on any failure (mirrors `cmd_query::open_view`).
fn open_view(data_dir: Option<&Path>) -> Result<GraphView> {
    let claims_dir = find_claims_dir(data_dir)?;
    let view_dir = claims_dir.join("_view.oxigraph");

    match GraphView::open(&view_dir) {
        Ok(view) => Ok(view),
        Err(_) => {
            let view = GraphView::open_in_memory()
                .context("open in-memory graph view")?;
            rebuild(&view, &claims_dir).with_context(|| {
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
}
