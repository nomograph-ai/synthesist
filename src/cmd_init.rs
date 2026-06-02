//! Init, status, and check commands -- ported to the v3 SPARQL substrate.
//!
//! - `cmd_init`: idempotent `SynthStore::init_at(<cwd>/claims)`.
//! - `cmd_status`: estate overview aggregated via SPARQL across all trees.
//! - `cmd_check`: claim-level integrity (schema, dangling supersedes,
//!   dangling task `depends_on`). See [`cmd_check`] for the three checks
//!   and [`crate::integrity`] for the v3-to-v2 props mapping helper.

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use crate::integrity::{claim_type_from_iri, doc_id, doc_type_str, v3_to_v2_props};
use crate::schema::{ValidationOutcome, validate_props};
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
/// Three checks run over the v3 substrate:
///
/// 1. **Schema**: every claim's props normalize via
///    [`crate::integrity::v3_to_v2_props`] and run through the per-type
///    validator. Failures surface as `error/schema`; claims for types
///    synthesist does not own (lattice, coordination protocol) surface
///    as `warn/no_validator`.
///
/// 2. **Dangling `synthesist:supersedes`**: one SPARQL SELECT finds every
///    `?sup synthesist:supersedes ?prior` where no triple in any graph
///    has `?prior` as its subject. Each row becomes
///    `error/dangling_supersedes`.
///
/// 3. **Dangling task `depends_on`**: pulls live Task heads with their
///    `synthesist:dependsOn` values (the Stage 1 `live_task_props`
///    pattern). Per task, every declared dep id must resolve to a live
///    Task in the same (tree, spec). Missing ids surface as
///    `error/dangling_depends_on`.
///
/// Output preserves the v2 contract:
/// `{ errors, warnings, issues: [...], passed }`. Exits 0 when clean,
/// 1 when any error fires (warnings alone do not fail).
pub fn cmd_check() -> Result<()> {
    let store = SynthStore::discover()?;
    let mut issues: Vec<Value> = Vec::new();

    check_schema_walk(&store, &mut issues).context("schema integrity walk")?;
    check_dangling_supersedes(&store, &mut issues)
        .context("dangling supersedes check")?;
    check_dangling_depends_on(&store, &mut issues)
        .context("dangling task depends_on check")?;

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

// ---------------------------------------------------------------------------
// Integrity checks (see `cmd_check`).
// ---------------------------------------------------------------------------

/// Check 1: walk every claim doc via `iter_claims`, validate v2-shape
/// props against the per-type schema.
///
/// `iter_claims` opens a fresh `LogReader` rather than borrowing the
/// store's cached SPARQL view, so the iterator composes cleanly with
/// the SPARQL calls made by checks 2/3. We collect into a `Vec` up
/// front to keep the borrow lifetimes straightforward.
fn check_schema_walk(store: &SynthStore, issues: &mut Vec<Value>) -> Result<()> {
    let docs: Vec<Value> = store.iter_claims()?.collect();
    for doc in &docs {
        let cid = doc_id(doc);
        let ctype_str = doc_type_str(doc);
        let type_iri = doc.get("@type").and_then(|v| v.as_str()).unwrap_or("");
        let Some(ct) = claim_type_from_iri(type_iri) else {
            // Unknown @type IRI -- surface as no_validator (warn).
            issues.push(json!({
                "level": "warn",
                "kind": "no_validator",
                "claim_id": cid,
                "claim_type": ctype_str,
                "message": "claim type not validated by synthesist; @type IRI unrecognized",
            }));
            continue;
        };
        let v2_props = v3_to_v2_props(doc);
        match synth_validate_outcome(&ct, &v2_props) {
            ValidationOutcome::Ok => {}
            ValidationOutcome::SchemaFail(e) => issues.push(json!({
                "level": "error",
                "kind": "schema",
                "claim_id": cid,
                "claim_type": ctype_str,
                "message": format!("{e}"),
            })),
            ValidationOutcome::NotOwnedBySynthesist => issues.push(json!({
                "level": "warn",
                "kind": "no_validator",
                "claim_id": cid,
                "claim_type": ctype_str,
                "message": "claim type not validated by synthesist; may be written by another consumer (lattice, coordination protocol)",
            })),
        }
    }
    Ok(())
}

/// Classify a (claim_type, v2_props) pair into a `ValidationOutcome`
/// for cmd_check. `schema::validate_props` rejects types synthesist
/// does not own as schema errors; we map those to
/// `NotOwnedBySynthesist` so the CLI surfaces them as warnings.
fn synth_validate_outcome(
    ct: &crate::claim_type::ClaimType,
    v2_props: &Value,
) -> ValidationOutcome {
    use crate::claim_type::ClaimType;
    let owned = matches!(
        ct,
        ClaimType::Tree
            | ClaimType::Spec
            | ClaimType::Task
            | ClaimType::Discovery
            | ClaimType::Campaign
            | ClaimType::Session
            | ClaimType::Phase
            | ClaimType::Outcome
    );
    if !owned {
        return ValidationOutcome::NotOwnedBySynthesist;
    }
    match validate_props(ct, v2_props) {
        Ok(()) => ValidationOutcome::Ok,
        Err(e) => ValidationOutcome::SchemaFail(e),
    }
}

/// Check 2: one SPARQL pass finds every `?sup synthesist:supersedes
/// ?prior` whose `?prior` IRI is not the subject of any triple. The
/// filter is `NOT EXISTS { GRAPH ?g2 { ?prior ?p ?o } }` so a prior
/// claim recorded in any graph is treated as present.
fn check_dangling_supersedes(store: &SynthStore, issues: &mut Vec<Value>) -> Result<()> {
    // H7: supersedes edges whose target is absent from the index.
    for edge in store.dangling_supersedes()? {
        // Strip the compact prefix so the issue surfaces the bare hash
        // (matches the v2 wire shape).
        let sup_id = crate::store::short_claim_id(&edge.superseder);
        let prior_id = crate::store::short_claim_id(&edge.target);
        // Read the superseder's @type for the claim_type field.
        let bare_type = store
            .doc(&edge.superseder)?
            .as_ref()
            .and_then(|d| d.get("@type"))
            .and_then(|v| v.as_str())
            .map(lowercase_first_of_type)
            .unwrap_or_default();
        issues.push(json!({
            "level": "error",
            "kind": "dangling_supersedes",
            "claim_id": sup_id,
            "claim_type": bare_type,
            "message": format!("supersedes {prior_id} which is not in the log"),
        }));
    }
    Ok(())
}

/// Check 3: dangling task `depends_on`.
///
/// Walk live Task heads via `live_task_props` (the Stage 1 SPARQL
/// query that GROUP_CONCATs the dep list per claim), then verify
/// every declared dep id resolves to a live Task in the same
/// (tree, spec). The compare is client-side; a SPARQL self-join over
/// the GROUP_CONCAT'd list is awkward and the live-task working set
/// is small enough that the cost is negligible.
fn check_dangling_depends_on(store: &SynthStore, issues: &mut Vec<Value>) -> Result<()> {
    let tasks = live_task_props(store)?;

    use std::collections::{HashMap, HashSet};
    let mut live: HashMap<(String, String), HashSet<String>> = HashMap::new();
    for props in &tasks {
        let tree = props.get("tree").and_then(|v| v.as_str()).unwrap_or("");
        let spec = props.get("spec").and_then(|v| v.as_str()).unwrap_or("");
        let id = props.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if tree.is_empty() || spec.is_empty() || id.is_empty() {
            continue;
        }
        live.entry((tree.to_string(), spec.to_string()))
            .or_default()
            .insert(id.to_string());
    }

    for props in &tasks {
        let tree = props.get("tree").and_then(|v| v.as_str()).unwrap_or("");
        let spec = props.get("spec").and_then(|v| v.as_str()).unwrap_or("");
        let tid = props.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if tree.is_empty() || spec.is_empty() || tid.is_empty() {
            continue;
        }
        let Some(siblings) = live.get(&(tree.to_string(), spec.to_string())) else {
            continue;
        };
        let deps = props
            .get("depends_on")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for d in deps.iter().filter_map(|v| v.as_str()) {
            if !siblings.contains(d) {
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
    Ok(())
}

/// Strip the `@type` IRI prefix and lowercase the leading character so
/// `synthesist:Task` reads `task` (matches the v2 wire shape).
fn lowercase_first_of_type(iri: &str) -> String {
    let bare = iri
        .strip_prefix("https://nomograph.org/synthesist/")
        .or_else(|| iri.strip_prefix("synthesist:"))
        .unwrap_or(iri);
    lowercase_first(bare)
}

/// Lowercase the first character of `s`. Used for issue `claim_type`
/// payloads so `Task` -> `task` (matches v2).
fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// SPARQL helpers
// ---------------------------------------------------------------------------

fn count_total_claims(store: &SynthStore) -> Result<i64> {
    Ok(store.count_total()? as i64)
}

fn count_by_type(store: &SynthStore) -> Result<Map<String, Value>> {
    // Aggregate every claim by its @type. Not live-filtered (matches the
    // prior non-live count). Walks the raw union since the breakdown
    // spans every type, not a single one.
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    for doc in store.iter_claims()? {
        if let Some(t) = doc.get("@type").and_then(|v| v.as_str()) {
            *counts.entry(strip_type_prefix(t)).or_insert(0) += 1;
        }
    }
    let mut out = Map::new();
    for (k, n) in counts {
        out.insert(k, json!(n));
    }
    Ok(out)
}

/// Live Tree heads: Tree claims that have not been superseded.
fn live_tree_heads(store: &SynthStore) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for (_, doc) in store.live_docs(&crate::wire_format::type_iri("tree"))? {
        let props = crate::store::bare_props(&doc);
        let name = match props.get("name").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let desc = props.get("description").cloned().unwrap_or(Value::Null);
        out.push(json!({
            "name": name,
            "status": "active",
            "description": desc,
        }));
    }
    out.sort_by(|a, b| {
        a.get("name").and_then(|v| v.as_str()).unwrap_or("")
            .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
    });
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
    let mut out = Vec::new();
    for (_, doc) in store.live_docs(&crate::wire_format::type_iri("task"))? {
        let bare = crate::store::bare_props(&doc);
        let getstr = |k: &str| -> String {
            bare.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
        };
        let tree = getstr("tree");
        let spec = getstr("spec");
        let id = getstr("id");
        let status = getstr("status");
        let deps: Vec<Value> = match bare.get("depends_on") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect(),
            Some(Value::String(s)) if !s.is_empty() => vec![Value::String(s.clone())],
            _ => Vec::new(),
        };
        let mut props = serde_json::Map::new();
        props.insert("tree".into(), json!(tree));
        props.insert("spec".into(), json!(spec));
        props.insert("id".into(), json!(id));
        props.insert("status".into(), json!(status));
        if let Some(s) = bare.get("summary").and_then(|v| v.as_str())
            && !s.is_empty()
        {
            props.insert("summary".into(), json!(s));
        }
        if let Some(g) = bare.get("gate").and_then(|v| v.as_str())
            && !g.is_empty()
        {
            props.insert("gate".into(), json!(g));
        }
        props.insert("depends_on".into(), Value::Array(deps));
        out.push(Value::Object(props));
    }
    Ok(out)
}

/// Live sessions with their current phase (defaults to `orient`).
fn live_sessions_with_phase(store: &SynthStore) -> Result<Vec<Value>> {
    // A live session opener = a Session claim that neither supersedes a
    // prior nor is superseded by a later (gamma H4 dual anti-join).
    let mut out: Vec<Value> = Vec::new();
    for opener in store.live_session_openers()? {
        let Some(doc) = store.doc(&opener)? else {
            continue;
        };
        let props = crate::store::bare_props(&doc);
        let id = match props.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let opt = |k: &str| -> Option<String> {
            props
                .get(k)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        };
        let phase = crate::cmd_phase::current_phase_name(store, &id)?
            .unwrap_or_else(|| "orient".to_string());
        out.push(json!({
            "id": id,
            "tree": opt("tree"),
            "spec": opt("spec"),
            "summary": opt("summary"),
            "phase": phase,
        }));
    }
    out.sort_by(|a, b| {
        a.get("id").and_then(|v| v.as_str()).unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });
    Ok(out)
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
