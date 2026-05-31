//! Init, status, and check commands -- ported to the v3 SPARQL substrate.
//!
//! - `cmd_init`: idempotent `SynthStore::init_at(<cwd>/claims)`.
//! - `cmd_status`: estate overview aggregated via SPARQL across all trees.
//! - `cmd_check`: claim-level integrity (TODO PATH-B; placeholder).

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use crate::store::{CLAIMS_DIR, SynthStore, find_legacy_v1_db, json_out, legacy_migration_error};

/// `synthesist init`: create `<cwd>/claims` if absent, else no-op.
pub fn cmd_init() -> Result<()> {
    let cwd = std::env::current_dir().context("cwd")?;
    let claims_dir = cwd.join(CLAIMS_DIR);
    if claims_dir.is_dir() {
        let _ = SynthStore::open_at(&claims_dir)?;
        return json_out(&json!({
            "ok": true,
            "already_initialized": true,
            "root": claims_dir.display().to_string(),
        }));
    }
    if let Some(legacy) = find_legacy_v1_db(&cwd) {
        return Err(legacy_migration_error(&legacy));
    }
    let store = SynthStore::init_at(&claims_dir)?;
    json_out(&json!({
        "ok": true,
        "root": store.root().display().to_string(),
    }))
}

/// `synthesist status`: estate overview via SPARQL.
///
/// Aggregates:
///   - `total_claims` + `claim_counts` (by type)
///   - `trees`        (live Tree heads with name + status)
///   - `ready_tasks`  (pending tasks whose deps are all done, across trees)
///   - `sessions`     (live Session openers carrying their current phase)
///
/// Reference port: this is one of the four SynthStore-based commands
/// the rest of Stage 2 will model after.
pub fn cmd_status() -> Result<()> {
    let store = SynthStore::discover()?;

    let total = count_total_claims(&store)?;
    let claim_counts = count_by_type(&store)?;
    let trees = live_tree_heads(&store)?;
    let ready = ready_tasks_all(&store)?;
    let sessions = live_sessions_with_phase(&store)?;

    json_out(&json!({
        "total_claims": total,
        "claim_counts": Value::Object(claim_counts),
        "trees": trees,
        "ready_tasks": ready,
        "sessions": sessions,
    }))
}

/// `synthesist check`: claim-level integrity.
///
/// TODO PATH-B: port to v3 (walk `iter_claims`, validate, check
/// dangling supersedes via SPARQL ASK, check task depends_on against
/// the SPARQL-derived live task ids).
pub fn cmd_check() -> Result<()> {
    json_out(&json!({
        "errors": 0,
        "warnings": 0,
        "issues": [],
        "passed": true,
        "todo_path_b": "cmd_check not yet ported to v3 SPARQL substrate",
    }))
}

// ---------------------------------------------------------------------------
// SPARQL helpers
// ---------------------------------------------------------------------------

fn count_total_claims(store: &SynthStore) -> Result<i64> {
    let q = r#"
        SELECT (COUNT(DISTINCT ?c) AS ?n) WHERE {
          GRAPH ?g { ?c rdf:type ?t }
        }
    "#;
    let r = store.sparql(q)?;
    Ok(literal_i64(&r, 0, 0))
}

fn count_by_type(store: &SynthStore) -> Result<Map<String, Value>> {
    let q = r#"
        SELECT ?t (COUNT(DISTINCT ?c) AS ?n) WHERE {
          GRAPH ?g { ?c rdf:type ?t }
        }
        GROUP BY ?t
    "#;
    let r = store.sparql(q)?;
    let mut out = Map::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let type_str = match row.first() {
            Some(Term::Iri(s)) => s.clone(),
            _ => continue,
        };
        let bare = strip_type_prefix(&type_str);
        let n = match row.get(1) {
            Some(Term::Literal { value, .. }) => value.parse::<i64>().unwrap_or(0),
            _ => 0,
        };
        out.insert(bare, json!(n));
    }
    Ok(out)
}

/// Live Tree heads: Tree claims that have not been superseded.
fn live_tree_heads(store: &SynthStore) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?c ?name ?desc WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Tree ;
               synthesist:name ?name .
            OPTIONAL { ?c synthesist:description ?desc }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        ORDER BY ?name
    "#;
    let r = store.sparql(q)?;
    let mut out = Vec::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let name = match row.get(1) {
            Some(Term::Literal { value, .. }) => value.clone(),
            _ => continue,
        };
        let desc = match row.get(2) {
            Some(Term::Literal { value, .. }) => Value::String(value.clone()),
            _ => Value::Null,
        };
        out.push(json!({
            "name": name,
            "status": "active",
            "description": desc,
        }));
    }
    Ok(out)
}

/// Ready tasks across every (tree, spec): a task is ready when its
/// status is "pending" and every depends_on id resolves to a live
/// task with status="done" in the same (tree, spec).
fn ready_tasks_all(store: &SynthStore) -> Result<Vec<Value>> {
    let tasks = live_task_props(store)?;

    // Build status map keyed by (tree, spec, id).
    use std::collections::HashMap;
    let mut status_by_key: HashMap<(String, String, String), String> = HashMap::new();
    for props in &tasks {
        let tree = props.get("tree").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let spec = props.get("spec").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = props.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = props.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if tree.is_empty() || spec.is_empty() || id.is_empty() {
            continue;
        }
        status_by_key.insert((tree, spec, id), status);
    }

    let mut out: Vec<Value> = Vec::new();
    for props in &tasks {
        let status = props.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status != "pending" {
            continue;
        }
        let tree = props.get("tree").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let spec = props.get("spec").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let id = props.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if tree.is_empty() || spec.is_empty() || id.is_empty() {
            continue;
        }
        let deps = props
            .get("depends_on")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let deps_ok = deps.iter().filter_map(|d| d.as_str()).all(|dep| {
            status_by_key
                .get(&(tree.clone(), spec.clone(), dep.to_string()))
                .map(|s| s.as_str())
                == Some("done")
        });
        if !deps_ok {
            continue;
        }
        let mut entry = serde_json::Map::new();
        entry.insert("tree".into(), json!(tree));
        entry.insert("spec".into(), json!(spec));
        entry.insert("id".into(), json!(id));
        entry.insert(
            "summary".into(),
            props.get("summary").cloned().unwrap_or(Value::Null),
        );
        if let Some(gate) = props.get("gate").cloned()
            && !gate.is_null()
        {
            entry.insert("gate".into(), gate);
        }
        out.push(Value::Object(entry));
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

/// Pull every live Task claim's relevant props via one SPARQL
/// SELECT. Shared by cmd_status and cmd_task_ready.
pub(crate) fn live_task_props(store: &SynthStore) -> Result<Vec<Value>> {
    // Aggregate depends_on into a list via GROUP_CONCAT so we can
    // recover the array from a single result row per task claim.
    let q = r#"
        SELECT ?c ?tree ?spec ?id ?status ?summary ?gate
               (GROUP_CONCAT(?dep; SEPARATOR="\u001F") AS ?deps)
        WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Task ;
               synthesist:tree ?tree ;
               synthesist:spec ?spec ;
               synthesist:id   ?id ;
               synthesist:status ?status .
            OPTIONAL { ?c synthesist:summary ?summary }
            OPTIONAL { ?c synthesist:gate ?gate }
            OPTIONAL { ?c synthesist:dependsOn ?dep }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        GROUP BY ?c ?tree ?spec ?id ?status ?summary ?gate
    "#;
    let r = store.sparql(q)?;
    let mut out = Vec::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let str_at = |i: usize| -> Option<String> {
            match row.get(i) {
                Some(Term::Literal { value, .. }) if !value.is_empty() => Some(value.clone()),
                _ => None,
            }
        };
        let tree = str_at(1).unwrap_or_default();
        let spec = str_at(2).unwrap_or_default();
        let id = str_at(3).unwrap_or_default();
        let status = str_at(4).unwrap_or_default();
        let summary = str_at(5);
        let gate = str_at(6);
        let deps_concat = str_at(7).unwrap_or_default();
        let deps: Vec<Value> = if deps_concat.is_empty() {
            Vec::new()
        } else {
            deps_concat
                .split('\u{001F}')
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect()
        };
        let mut props = serde_json::Map::new();
        props.insert("tree".into(), json!(tree));
        props.insert("spec".into(), json!(spec));
        props.insert("id".into(), json!(id));
        props.insert("status".into(), json!(status));
        if let Some(s) = summary {
            props.insert("summary".into(), json!(s));
        }
        if let Some(g) = gate {
            props.insert("gate".into(), json!(g));
        }
        props.insert("depends_on".into(), Value::Array(deps));
        out.push(Value::Object(props));
    }
    Ok(out)
}

/// Live sessions with their current phase (defaults to `orient`).
fn live_sessions_with_phase(store: &SynthStore) -> Result<Vec<Value>> {
    // A live session = a Session claim with no `supersedes` and not
    // itself the target of a `supersedes` from a later Session claim.
    let q = r#"
        SELECT ?c ?id ?tree ?spec ?summary WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Session ;
               synthesist:id ?id .
            OPTIONAL { ?c synthesist:tree ?tree }
            OPTIONAL { ?c synthesist:spec ?spec }
            OPTIONAL { ?c synthesist:summary ?summary }
            FILTER NOT EXISTS { ?c synthesist:supersedes ?prev }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        ORDER BY ?id
    "#;
    let r = store.sparql(q)?;
    let mut out: Vec<Value> = Vec::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let str_at = |i: usize| -> Option<String> {
            match row.get(i) {
                Some(Term::Literal { value, .. }) if !value.is_empty() => Some(value.clone()),
                _ => None,
            }
        };
        let id = match str_at(1) {
            Some(s) => s,
            None => continue,
        };
        let tree = str_at(2);
        let spec = str_at(3);
        let summary = str_at(4);
        let phase = crate::cmd_phase::current_phase_name(store, &id)?
            .unwrap_or_else(|| "orient".to_string());
        out.push(json!({
            "id": id,
            "tree": tree,
            "spec": spec,
            "summary": summary,
            "phase": phase,
        }));
    }
    Ok(out)
}

fn literal_i64(r: &nomograph_claim::graph_view::SelectResults, row: usize, col: usize) -> i64 {
    use nomograph_claim::graph_view::Term;
    match r.rows.get(row).and_then(|r| r.get(col)) {
        Some(Term::Literal { value, .. }) => value.parse().unwrap_or(0),
        _ => 0,
    }
}

/// Strip the prefix off a `@type` IRI to get the bare claim_type
/// string ("task", "spec", etc.) for status output. Accepts both
/// expanded form (`https://nomograph.org/synthesist/Task`) and
/// compact form (`synthesist:Task`).
fn strip_type_prefix(iri: &str) -> String {
    let stripped = iri
        .strip_prefix("https://nomograph.org/synthesist/")
        .or_else(|| iri.strip_prefix("synthesist:"))
        .unwrap_or(iri);
    // Lowercase the first character for v2 compat (`Task` -> `task`).
    let mut chars = stripped.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}
