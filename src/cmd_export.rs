//! `synthesist export` -- emit the claim log as JSON.
//!
//! Path B Stage 2: v3-native export. Output is a single JSON object
//! with two views of the corpus:
//!
//! - `claims_raw`: the asserter-walked union of v3 JSON-LD documents,
//!   one per write (every supersession step is a separate entry).
//!   Streamed verbatim from `SynthStore::iter_claims()`.
//! - per-type buckets (`trees`, `specs`, `tasks`, `discoveries`,
//!   `campaigns`, `sessions`, `phases`, `outcomes`): SPARQL projections
//!   of the *live heads* for each type (the `FILTER NOT EXISTS` chain
//!   walk that every read command already uses). One props-shaped JSON
//!   object per head.
//!
//! ## Round-trip semantics
//!
//! `cmd_import` consumes `claims_raw` and replays each entry via
//! `SynthStore::append_replay`. Because v3 claim ids are content hashes
//! over (claim_type, props, asserter, generated_at), and `append_replay`
//! samples a fresh wall clock for `generated_at`, **the import re-mints
//! every id**. Logical content (props, supersession chain) is preserved;
//! the @id strings change. The export's `claims_raw` retains the
//! original @ids for reference but the import path drops the envelope.
//!
//! Stable-id round-trip is a 3.0.0-final concern and would require a
//! SynthStore raw-write helper that writes a JSON-LD doc verbatim
//! (preserving the exporter's @id and prov:* envelope). That helper is
//! intentionally NOT added in this commit; see the report.

use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::store::{SynthStore, json_out};

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
// SPARQL projections of live heads
//
// Each projector returns one props-shaped JSON object per live head.
// Shape mirrors the per-claim-type list commands (cmd_tree::list,
// cmd_spec::list, etc.) so operators familiar with the existing list
// surface recognize the export bucket immediately. The projector does
// NOT include the @id; live-heads-only export prioritizes the values
// view, and `claims_raw` carries the @ids if a caller needs them.
// ---------------------------------------------------------------------------

fn str_at(row: &[nomograph_claim::graph_view::Term], i: usize) -> Option<String> {
    use nomograph_claim::graph_view::Term;
    match row.get(i) {
        Some(Term::Literal { value, .. }) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn project_trees(store: &SynthStore) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?name ?description ?status WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Tree ;
               synthesist:name ?name .
            OPTIONAL { ?c synthesist:description ?description }
            OPTIONAL { ?c synthesist:status      ?status }
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
        let name = match str_at(row, 0) {
            Some(s) => s,
            None => continue,
        };
        let mut props = Map::new();
        props.insert("name".into(), Value::String(name));
        if let Some(v) = str_at(row, 1) {
            props.insert("description".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 2) {
            props.insert("status".into(), Value::String(v));
        }
        out.push(Value::Object(props));
    }
    Ok(out)
}

fn project_specs(store: &SynthStore) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?tree ?id ?goal ?constraints ?decisions ?status ?outcome WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Spec ;
               synthesist:tree ?tree ;
               synthesist:id   ?id .
            OPTIONAL { ?c synthesist:goal        ?goal }
            OPTIONAL { ?c synthesist:constraints ?constraints }
            OPTIONAL { ?c synthesist:decisions   ?decisions }
            OPTIONAL { ?c synthesist:status      ?status }
            OPTIONAL { ?c synthesist:outcome     ?outcome }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        ORDER BY ?tree ?id
    "#;
    let r = store.sparql(q)?;
    let mut out = Vec::new();
    for row in &r.rows {
        let tree = match str_at(row, 0) {
            Some(s) => s,
            None => continue,
        };
        let id = match str_at(row, 1) {
            Some(s) => s,
            None => continue,
        };
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree));
        props.insert("id".into(), Value::String(id));
        if let Some(v) = str_at(row, 2) {
            props.insert("goal".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 3) {
            props.insert("constraints".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 4) {
            props.insert("decisions".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 5) {
            props.insert("status".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 6) {
            props.insert("outcome".into(), Value::String(v));
        }
        out.push(Value::Object(props));
    }
    Ok(out)
}

fn project_tasks(store: &SynthStore) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?tree ?spec ?id ?status ?summary ?description ?gate
               (GROUP_CONCAT(?dep; SEPARATOR="\u001F") AS ?deps)
               (GROUP_CONCAT(?file; SEPARATOR="\u001F") AS ?files)
        WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Task ;
               synthesist:tree   ?tree ;
               synthesist:spec   ?spec ;
               synthesist:id     ?id ;
               synthesist:status ?status .
            OPTIONAL { ?c synthesist:summary     ?summary }
            OPTIONAL { ?c synthesist:description ?description }
            OPTIONAL { ?c synthesist:gate        ?gate }
            OPTIONAL { ?c synthesist:dependsOn   ?dep }
            OPTIONAL { ?c synthesist:files       ?file }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        GROUP BY ?tree ?spec ?id ?status ?summary ?description ?gate
        ORDER BY ?tree ?spec ?id
    "#;
    let r = store.sparql(q)?;
    let mut out = Vec::new();
    for row in &r.rows {
        let tree = match str_at(row, 0) {
            Some(s) => s,
            None => continue,
        };
        let spec = match str_at(row, 1) {
            Some(s) => s,
            None => continue,
        };
        let id = match str_at(row, 2) {
            Some(s) => s,
            None => continue,
        };
        let status = str_at(row, 3).unwrap_or_default();
        let summary = str_at(row, 4);
        let description = str_at(row, 5);
        let gate = str_at(row, 6);
        let deps_concat = str_at(row, 7).unwrap_or_default();
        let files_concat = str_at(row, 8).unwrap_or_default();

        let deps: Vec<Value> = if deps_concat.is_empty() {
            Vec::new()
        } else {
            deps_concat
                .split('\u{001F}')
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect()
        };
        let files: Vec<Value> = if files_concat.is_empty() {
            Vec::new()
        } else {
            files_concat
                .split('\u{001F}')
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect()
        };

        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree));
        props.insert("spec".into(), Value::String(spec));
        props.insert("id".into(), Value::String(id));
        props.insert("status".into(), Value::String(status));
        if let Some(s) = summary {
            props.insert("summary".into(), Value::String(s));
        }
        if let Some(s) = description {
            props.insert("description".into(), Value::String(s));
        }
        if let Some(s) = gate {
            props.insert("gate".into(), Value::String(s));
        }
        props.insert("depends_on".into(), Value::Array(deps));
        props.insert("files".into(), Value::Array(files));
        out.push(Value::Object(props));
    }
    Ok(out)
}

fn project_discoveries(store: &SynthStore) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?tree ?spec ?id ?date ?author ?finding ?impact ?action WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Discovery ;
               synthesist:tree ?tree ;
               synthesist:spec ?spec ;
               synthesist:id   ?id .
            OPTIONAL { ?c synthesist:date    ?date }
            OPTIONAL { ?c synthesist:author  ?author }
            OPTIONAL { ?c synthesist:finding ?finding }
            OPTIONAL { ?c synthesist:impact  ?impact }
            OPTIONAL { ?c synthesist:action  ?action }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        ORDER BY ?tree ?spec ?id
    "#;
    let r = store.sparql(q)?;
    let mut out = Vec::new();
    for row in &r.rows {
        let tree = match str_at(row, 0) {
            Some(s) => s,
            None => continue,
        };
        let spec = match str_at(row, 1) {
            Some(s) => s,
            None => continue,
        };
        let id = match str_at(row, 2) {
            Some(s) => s,
            None => continue,
        };
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree));
        props.insert("spec".into(), Value::String(spec));
        props.insert("id".into(), Value::String(id));
        if let Some(v) = str_at(row, 3) {
            props.insert("date".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 4) {
            props.insert("author".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 5) {
            props.insert("finding".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 6) {
            props.insert("impact".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 7) {
            props.insert("action".into(), Value::String(v));
        }
        out.push(Value::Object(props));
    }
    Ok(out)
}

fn project_campaigns(store: &SynthStore) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?tree ?spec ?kind ?summary ?title
               (GROUP_CONCAT(?blk; SEPARATOR="\u001F") AS ?blocked_by)
        WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Campaign ;
               synthesist:tree ?tree ;
               synthesist:spec ?spec .
            OPTIONAL { ?c synthesist:kind      ?kind }
            OPTIONAL { ?c synthesist:summary   ?summary }
            OPTIONAL { ?c synthesist:title     ?title }
            OPTIONAL { ?c synthesist:blockedBy ?blk }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        GROUP BY ?tree ?spec ?kind ?summary ?title
        ORDER BY ?tree ?spec
    "#;
    let r = store.sparql(q)?;
    let mut out = Vec::new();
    for row in &r.rows {
        let tree = match str_at(row, 0) {
            Some(s) => s,
            None => continue,
        };
        let spec = match str_at(row, 1) {
            Some(s) => s,
            None => continue,
        };
        let blocked_by_concat = str_at(row, 5).unwrap_or_default();
        let blocked_by: Vec<Value> = if blocked_by_concat.is_empty() {
            Vec::new()
        } else {
            blocked_by_concat
                .split('\u{001F}')
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect()
        };
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree));
        props.insert("spec".into(), Value::String(spec));
        if let Some(v) = str_at(row, 2) {
            props.insert("kind".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 3) {
            props.insert("summary".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 4) {
            props.insert("title".into(), Value::String(v));
        }
        props.insert("blocked_by".into(), Value::Array(blocked_by));
        out.push(Value::Object(props));
    }
    Ok(out)
}

fn project_sessions(store: &SynthStore) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?id ?tree ?spec ?summary WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Session ;
               synthesist:id ?id .
            OPTIONAL { ?c synthesist:tree    ?tree }
            OPTIONAL { ?c synthesist:spec    ?spec }
            OPTIONAL { ?c synthesist:summary ?summary }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        ORDER BY ?id
    "#;
    let r = store.sparql(q)?;
    let mut out = Vec::new();
    for row in &r.rows {
        let id = match str_at(row, 0) {
            Some(s) => s,
            None => continue,
        };
        let mut props = Map::new();
        props.insert("id".into(), Value::String(id));
        if let Some(v) = str_at(row, 1) {
            props.insert("tree".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 2) {
            props.insert("spec".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 3) {
            props.insert("summary".into(), Value::String(v));
        }
        out.push(Value::Object(props));
    }
    Ok(out)
}

fn project_phases(store: &SynthStore) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?sessionId ?name WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Phase ;
               synthesist:sessionId ?sessionId ;
               synthesist:name      ?name .
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        ORDER BY ?sessionId
    "#;
    let r = store.sparql(q)?;
    let mut out = Vec::new();
    for row in &r.rows {
        let session_id = match str_at(row, 0) {
            Some(s) => s,
            None => continue,
        };
        let name = match str_at(row, 1) {
            Some(s) => s,
            None => continue,
        };
        let mut props = Map::new();
        props.insert("session_id".into(), Value::String(session_id));
        props.insert("name".into(), Value::String(name));
        out.push(Value::Object(props));
    }
    Ok(out)
}

fn project_outcomes(store: &SynthStore) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?tree ?spec ?status ?note ?linkedSpec ?date WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Outcome ;
               synthesist:tree ?tree ;
               synthesist:spec ?spec .
            OPTIONAL { ?c synthesist:status     ?status }
            OPTIONAL { ?c synthesist:note       ?note }
            OPTIONAL { ?c synthesist:linkedSpec ?linkedSpec }
            OPTIONAL { ?c synthesist:date       ?date }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        ORDER BY ?tree ?spec
    "#;
    let r = store.sparql(q)?;
    let mut out = Vec::new();
    for row in &r.rows {
        let tree = match str_at(row, 0) {
            Some(s) => s,
            None => continue,
        };
        let spec = match str_at(row, 1) {
            Some(s) => s,
            None => continue,
        };
        let mut props = Map::new();
        props.insert("tree".into(), Value::String(tree));
        props.insert("spec".into(), Value::String(spec));
        if let Some(v) = str_at(row, 2) {
            props.insert("status".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 3) {
            props.insert("note".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 4) {
            props.insert("linked_spec".into(), Value::String(v));
        }
        if let Some(v) = str_at(row, 5) {
            props.insert("date".into(), Value::String(v));
        }
        out.push(Value::Object(props));
    }
    Ok(out)
}
