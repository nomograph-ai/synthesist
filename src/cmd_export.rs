//! `synthesist export` -- emit the claim log as JSON.
//!
//! v3-native export ported to the gamma typed query surface (C-2).
//! Output is a single JSON object with two views of the corpus:
//!
//! - `claims_raw`: the asserter-walked union of v3 JSON-LD documents,
//!   one per write (every supersession step is a separate entry).
//!   Streamed verbatim from `SynthStore::iter_claims()`.
//! - per-type buckets (`trees`, `specs`, `tasks`, `discoveries`,
//!   `campaigns`, `sessions`, `phases`, `outcomes`): projections of the
//!   *live heads* for each type (gamma H2 live-head anti-join). One
//!   props-shaped JSON object per head.
//!
//! ## Round-trip semantics
//!
//! `cmd_import` consumes `claims_raw` and replays each entry via
//! `SynthStore::append_replay`. Because v3 claim ids are content hashes
//! over (claim_type, props, asserter, generated_at), and `append_replay`
//! samples a fresh wall clock for `generated_at`, **the import re-mints
//! every id**. Logical content (props, supersession chain) is preserved;
//! the @id strings change.

use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::store::{SynthStore, bare_props, json_out};
use crate::wire_format as wf;

pub fn cmd_export() -> Result<()> {
    let store = SynthStore::discover()?;

    let claims_raw: Vec<Value> = store.iter_claims()?.collect();

    let trees = project_trees(&store)?;
    let specs = project_specs(&store)?;
    let tasks = project_tasks(&store)?;
    let discoveries = project_discoveries(&store)?;
    let campaigns = project_campaigns(&store)?;
    let sessions = project_sessions(&store)?;
    let phases = project_phases(&store)?;
    let outcomes = project_outcomes(&store)?;

    json_out(&json!({
        "claims_raw": claims_raw,
        "trees": trees,
        "specs": specs,
        "tasks": tasks,
        "discoveries": discoveries,
        "campaigns": campaigns,
        "sessions": sessions,
        "phases": phases,
        "outcomes": outcomes,
    }))
}

// ---------------------------------------------------------------------------
// Live-head projections
//
// Each projector returns one props-shaped JSON object per live head.
// Shape mirrors the per-claim-type list commands. The projector does
// NOT include the @id; `claims_raw` carries the @ids if a caller needs
// them.
// ---------------------------------------------------------------------------

/// Copy a scalar string prop from `bare` into `props` when present and
/// non-empty.
fn copy_str(bare: &Map<String, Value>, props: &mut Map<String, Value>, key: &str) {
    if let Some(v) = bare.get(key).and_then(|v| v.as_str())
        && !v.is_empty()
    {
        props.insert(key.into(), Value::String(v.to_string()));
    }
}

/// Read a member-array prop (e.g. `depends_on`, `files`, `blocked_by`)
/// from `bare` as a vector of string Values.
fn member_array(bare: &Map<String, Value>, key: &str) -> Vec<Value> {
    match bare.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| Value::String(s.to_string()))
            .collect(),
        Some(Value::String(s)) if !s.is_empty() => vec![Value::String(s.clone())],
        _ => Vec::new(),
    }
}

fn project_trees(store: &SynthStore) -> Result<Vec<Value>> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("tree"))? {
        let bare = bare_props(&doc);
        let name = match bare.get("name").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let mut props = Map::new();
        props.insert("name".into(), Value::String(name.clone()));
        copy_str(&bare, &mut props, "description");
        copy_str(&bare, &mut props, "status");
        out.push((name, Value::Object(props)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out.into_iter().map(|(_, v)| v).collect())
}

fn project_specs(store: &SynthStore) -> Result<Vec<Value>> {
    let mut out: Vec<((String, String), Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("spec"))? {
        let bare = bare_props(&doc);
        let tree = match bare.get("tree").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let id = match bare.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree.clone()));
        props.insert("id".into(), Value::String(id.clone()));
        for k in ["goal", "constraints", "decisions", "status", "outcome"] {
            copy_str(&bare, &mut props, k);
        }
        out.push(((tree, id), Value::Object(props)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out.into_iter().map(|(_, v)| v).collect())
}

fn project_tasks(store: &SynthStore) -> Result<Vec<Value>> {
    let mut out: Vec<((String, String, String), Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("task"))? {
        let bare = bare_props(&doc);
        let tree = match bare.get("tree").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let spec = match bare.get("spec").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let id = match bare.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let status = bare
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree.clone()));
        props.insert("spec".into(), Value::String(spec.clone()));
        props.insert("id".into(), Value::String(id.clone()));
        props.insert("status".into(), Value::String(status));
        copy_str(&bare, &mut props, "summary");
        copy_str(&bare, &mut props, "description");
        copy_str(&bare, &mut props, "gate");
        props.insert("depends_on".into(), Value::Array(member_array(&bare, "depends_on")));
        props.insert("files".into(), Value::Array(member_array(&bare, "files")));
        out.push(((tree, spec, id), Value::Object(props)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out.into_iter().map(|(_, v)| v).collect())
}

fn project_discoveries(store: &SynthStore) -> Result<Vec<Value>> {
    let mut out: Vec<((String, String, String), Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("discovery"))? {
        let bare = bare_props(&doc);
        let tree = match bare.get("tree").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let spec = match bare.get("spec").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let id = match bare.get("id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree.clone()));
        props.insert("spec".into(), Value::String(spec.clone()));
        props.insert("id".into(), Value::String(id.clone()));
        for k in ["date", "author", "finding", "impact", "action"] {
            copy_str(&bare, &mut props, k);
        }
        out.push(((tree, spec, id), Value::Object(props)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out.into_iter().map(|(_, v)| v).collect())
}

fn project_campaigns(store: &SynthStore) -> Result<Vec<Value>> {
    let mut out: Vec<((String, String), Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("campaign"))? {
        let bare = bare_props(&doc);
        let tree = match bare.get("tree").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let spec = match bare.get("spec").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree.clone()));
        props.insert("spec".into(), Value::String(spec.clone()));
        copy_str(&bare, &mut props, "kind");
        copy_str(&bare, &mut props, "summary");
        copy_str(&bare, &mut props, "title");
        props.insert("blocked_by".into(), Value::Array(member_array(&bare, "blocked_by")));
        out.push(((tree, spec), Value::Object(props)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out.into_iter().map(|(_, v)| v).collect())
}

fn project_sessions(store: &SynthStore) -> Result<Vec<Value>> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for opener in store.live_session_openers()? {
        let Some(doc) = store.doc(&opener)? else {
            continue;
        };
        let bare = bare_props(&doc);
        let id = match bare.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let mut props = Map::new();
        props.insert("id".into(), Value::String(id.clone()));
        copy_str(&bare, &mut props, "tree");
        copy_str(&bare, &mut props, "spec");
        copy_str(&bare, &mut props, "summary");
        out.push((id, Value::Object(props)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out.into_iter().map(|(_, v)| v).collect())
}

fn project_phases(store: &SynthStore) -> Result<Vec<Value>> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("phase"))? {
        let bare = bare_props(&doc);
        let session_id = match bare.get("session_id").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let name = match bare.get("name").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let mut props = Map::new();
        props.insert("session_id".into(), Value::String(session_id.clone()));
        props.insert("name".into(), Value::String(name));
        out.push((session_id, Value::Object(props)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out.into_iter().map(|(_, v)| v).collect())
}

fn project_outcomes(store: &SynthStore) -> Result<Vec<Value>> {
    let mut out: Vec<((String, String), Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("outcome"))? {
        let bare = bare_props(&doc);
        let tree = match bare.get("tree").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let spec = match bare.get("spec").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree.clone()));
        props.insert("spec".into(), Value::String(spec.clone()));
        copy_str(&bare, &mut props, "status");
        copy_str(&bare, &mut props, "note");
        // The v2 projection surfaced linked_spec under that bare key.
        copy_str(&bare, &mut props, "linked_spec");
        copy_str(&bare, &mut props, "date");
        out.push(((tree, spec), Value::Object(props)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out.into_iter().map(|(_, v)| v).collect())
}
