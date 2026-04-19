//! Init, status, and check commands — ported to the claim substrate.
//!
//! - `cmd_init`: idempotent `SynthStore::init_at(<cwd>/claims)`.
//! - `cmd_status`: estate overview aggregated across all trees.
//! - `cmd_check`: claim-level integrity (validate every claim, dangling
//!   supersedes, dangling task depends_on).
//!
//! v1 had SQL referential checks against concrete tables. v2 has no FK
//! layer; the checks below walk the loaded claim log once and use the
//! view projection for the per-type scans needed for reporting.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use nomograph_claim::{schema::validate_claim, Session};
use serde_json::{json, Value};

use crate::store::{json_out, SynthStore, CLAIMS_DIR};

/// `synthesist init`: create `<cwd>/claims` if absent, else no-op.
pub fn cmd_init() -> Result<()> {
    let cwd = std::env::current_dir().context("cwd")?;
    let claims_dir = cwd.join(CLAIMS_DIR);
    let genesis = claims_dir.join("genesis.amc");
    if genesis.is_file() {
        // Touch the store to confirm it opens cleanly, but report idempotent.
        let _ = SynthStore::open_at(&claims_dir)?;
        return json_out(&json!({
            "ok": true,
            "already_initialized": true,
            "root": claims_dir.display().to_string(),
        }));
    }
    let store = SynthStore::init_at(&claims_dir)?;
    json_out(&json!({
        "ok": true,
        "root": store.root().display().to_string(),
    }))
}

/// `synthesist status`: estate overview.
///
/// Output shape (parallel to v1 where possible):
/// - `total_claims`, `claim_counts` (by type)
/// - `trees` (name + status)
/// - `ready_tasks` (pending + all deps done, aggregated across all trees)
/// - `sessions` (live Session claims)
/// - `phase` (latest Phase claim's name, or null)
pub fn cmd_status() -> Result<()> {
    let mut store = SynthStore::discover()?;

    // Totals + per-type counts.
    let total_rows = store.query("SELECT COUNT(*) as n FROM claims", &[])?;
    let total: i64 = total_rows
        .first()
        .and_then(|r| r.get("n"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let count_rows = store.query(
        "SELECT claim_type, COUNT(*) as n FROM claims GROUP BY claim_type",
        &[],
    )?;
    let mut claim_counts = serde_json::Map::new();
    for row in count_rows {
        let ct = row
            .get("claim_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let n = row.get("n").and_then(|v| v.as_i64()).unwrap_or(0);
        if !ct.is_empty() {
            claim_counts.insert(ct, json!(n));
        }
    }

    // Trees: dedupe heads by name (most recent non-superseded wins).
    let trees = query_tree_heads(&store)?;

    // Ready tasks: aggregate across every (tree, spec).
    let ready = ready_tasks_all(&store)?;

    // Active sessions via the Session API.
    let sessions: Vec<Value> = Session::list_live(store.inner())
        .context("list live sessions")?
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "tree": s.tree,
                "spec": s.spec,
                "summary": s.summary,
            })
        })
        .collect();

    // Current phase: latest Phase claim, if any.
    let phase_rows = store.query(
        "SELECT props FROM claims \
         WHERE claim_type = 'phase' \
         ORDER BY asserted_at DESC LIMIT 1",
        &[],
    )?;
    let phase = phase_rows
        .into_iter()
        .next()
        .and_then(|row| {
            row.get("props")
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
        })
        .and_then(|props| {
            props
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    json_out(&json!({
        "total_claims": total,
        "claim_counts": Value::Object(claim_counts),
        "trees": trees,
        "ready_tasks": ready,
        "sessions": sessions,
        "phase": phase,
    }))
}

/// `synthesist check`: claim-level integrity.
///
/// 1. Validate every claim against its schema.
/// 2. Dangling `supersedes`: claim X points at Y not present in the log.
/// 3. Dangling task `depends_on`: task references a sibling id that has
///    no live Task claim in the same (tree, spec).
///
/// Exits 0 when clean, 1 when any issue is found.
pub fn cmd_check() -> Result<()> {
    let mut store = SynthStore::discover()?;
    let mut issues: Vec<Value> = Vec::new();

    // 1 + 2: walk the full claim log once.
    let claims = store
        .inner()
        .load_claims()
        .context("load_claims for check")?;

    let known_ids: HashSet<String> = claims.iter().map(|c| c.id.clone()).collect();

    for c in &claims {
        if let Err(e) = validate_claim(c) {
            issues.push(json!({
                "level": "error",
                "kind": "schema",
                "claim_id": c.id,
                "claim_type": c.claim_type.as_str(),
                "message": format!("{e}"),
            }));
        }
        if let Some(prior) = &c.supersedes
            && !known_ids.contains(prior)
        {
            issues.push(json!({
                "level": "error",
                "kind": "dangling_supersedes",
                "claim_id": c.id,
                "claim_type": c.claim_type.as_str(),
                "message": format!("supersedes {prior} which is not in the log"),
            }));
        }
    }

    // 3: dangling task depends_on.
    let tasks = store.query(
        "SELECT id, props, supersedes FROM claims \
         WHERE claim_type = 'task' \
         ORDER BY asserted_at DESC",
        &[],
    )?;
    let superseded: HashSet<String> = tasks
        .iter()
        .filter_map(|r| r.get("supersedes").and_then(|v| v.as_str()).map(String::from))
        .collect();

    // Per (tree, spec): live task ids = most recent non-superseded per task id.
    let mut live: HashMap<(String, String), HashMap<String, Value>> = HashMap::new();
    for row in &tasks {
        let claim_id = row
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if superseded.contains(&claim_id) {
            continue;
        }
        let props_str = match row.get("props").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let props: Value = match serde_json::from_str(props_str) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let tree = props.get("tree").and_then(|v| v.as_str()).unwrap_or("");
        let spec = props.get("spec").and_then(|v| v.as_str()).unwrap_or("");
        let tid = props.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if tree.is_empty() || spec.is_empty() || tid.is_empty() {
            continue;
        }
        live.entry((tree.to_string(), spec.to_string()))
            .or_default()
            .entry(tid.to_string())
            .or_insert(props);
    }

    for ((tree, spec), by_id) in &live {
        for (tid, props) in by_id {
            let deps = props
                .get("depends_on")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for d in deps.iter().filter_map(|v| v.as_str()) {
                if !by_id.contains_key(d) {
                    issues.push(json!({
                        "level": "error",
                        "kind": "dangling_depends_on",
                        "message": format!(
                            "task {tree}/{spec}/{tid} depends on {d} which does not exist"
                        ),
                    }));
                }
            }
        }
    }

    let errors = issues.iter().filter(|i| i["level"] == "error").count();
    let warnings = issues.iter().filter(|i| i["level"] == "warn").count();
    let passed = errors == 0;

    json_out(&json!({
        "errors": errors,
        "warnings": warnings,
        "issues": issues,
        "passed": passed,
    }))?;

    if !passed {
        std::process::exit(1);
    }
    Ok(())
}

/// Dedupe Tree claims by `props.name` keeping the most recent non-superseded.
fn query_tree_heads(store: &SynthStore) -> Result<Vec<Value>> {
    let rows = store.query(
        "SELECT id, props, supersedes FROM claims \
         WHERE claim_type = 'tree' \
         ORDER BY asserted_at DESC",
        &[],
    )?;
    let superseded: HashSet<String> = rows
        .iter()
        .filter_map(|r| r.get("supersedes").and_then(|v| v.as_str()).map(String::from))
        .collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Value> = Vec::new();
    for row in rows {
        let claim_id = row
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if superseded.contains(&claim_id) {
            continue;
        }
        let props_str = match row.get("props").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let props: Value = match serde_json::from_str(props_str) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let name = props
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() || !seen.insert(name.clone()) {
            continue;
        }
        out.push(json!({
            "name": name,
            "status": "active",
            "description": props.get("description").cloned().unwrap_or(Value::Null),
        }));
    }
    // Stable order for downstream consumers.
    out.sort_by(|a, b| {
        let a = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let b = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        a.cmp(b)
    });
    Ok(out)
}

/// Aggregate ready tasks across every (tree, spec) in the estate.
///
/// A task is ready when its live claim has status=pending and every
/// id in its depends_on array refers to a live task with status=done.
fn ready_tasks_all(store: &SynthStore) -> Result<Vec<Value>> {
    let rows = store.query(
        "SELECT id, props, supersedes FROM claims \
         WHERE claim_type = 'task' \
         ORDER BY asserted_at DESC",
        &[],
    )?;
    let superseded: HashSet<String> = rows
        .iter()
        .filter_map(|r| r.get("supersedes").and_then(|v| v.as_str()).map(String::from))
        .collect();

    // Live task props per (tree, spec, task_id), first seen (most recent) wins.
    let mut by_key: HashMap<(String, String), HashMap<String, Value>> = HashMap::new();
    for row in rows {
        let claim_id = row
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if superseded.contains(&claim_id) {
            continue;
        }
        let props_str = match row.get("props").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let props: Value = match serde_json::from_str(props_str) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let tree = props.get("tree").and_then(|v| v.as_str()).unwrap_or("");
        let spec = props.get("spec").and_then(|v| v.as_str()).unwrap_or("");
        let tid = props.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if tree.is_empty() || spec.is_empty() || tid.is_empty() {
            continue;
        }
        by_key
            .entry((tree.to_string(), spec.to_string()))
            .or_default()
            .entry(tid.to_string())
            .or_insert(props);
    }

    let mut out: Vec<Value> = Vec::new();
    for ((tree, spec), by_id) in &by_key {
        let status_by_id: HashMap<&str, &str> = by_id
            .iter()
            .filter_map(|(tid, p)| {
                let s = p.get("status").and_then(|v| v.as_str())?;
                Some((tid.as_str(), s))
            })
            .collect();
        for (tid, props) in by_id {
            let status = props.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if status != "pending" {
                continue;
            }
            let deps = props
                .get("depends_on")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let deps_ok = deps
                .iter()
                .filter_map(|d| d.as_str())
                .all(|d| status_by_id.get(d).copied() == Some("done"));
            if !deps_ok {
                continue;
            }
            let mut entry = serde_json::Map::new();
            entry.insert("tree".into(), json!(tree));
            entry.insert("spec".into(), json!(spec));
            entry.insert("id".into(), json!(tid));
            entry.insert(
                "summary".into(),
                props.get("summary").cloned().unwrap_or(Value::Null),
            );
            if let Some(gate) = props.get("gate").cloned() {
                if !gate.is_null() {
                    entry.insert("gate".into(), gate);
                }
            }
            out.push(Value::Object(entry));
        }
    }
    out.sort_by(|a, b| {
        let ka = (
            a.get("tree").and_then(|v| v.as_str()).unwrap_or(""),
            a.get("spec").and_then(|v| v.as_str()).unwrap_or(""),
            a.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        );
        let kb = (
            b.get("tree").and_then(|v| v.as_str()).unwrap_or(""),
            b.get("spec").and_then(|v| v.as_str()).unwrap_or(""),
            b.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        );
        ka.cmp(&kb)
    });
    Ok(out)
}
