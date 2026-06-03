//! `outcome` CLI surface -- first-class Outcome claim operations.
//!
//! The Outcome claim type expresses *what happened to a spec* (a spec
//! was completed, abandoned, deferred, or absorbed by another spec),
//! distinct from Spec status which expresses *what state the spec is
//! in* (`draft`, `active`, `done`, `superseded`).
//!
//! Issue #6 closed with the workaround of using
//! `spec update --status superseded --outcome "..."`. v2.4.0 surfaces
//! Outcome as a first-class CLI so the discoverable path matches the
//! mental model.
//!
//! Path B Stage 2: the read side now projects live Outcome heads via
//! typed passes over the redb gamma index. Writes were already routed
//! through `SynthStore::append` in Stage 1.

use crate::claim_type::ClaimType;
use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use crate::cli::OutcomeCmd;
use crate::output::{Output, emit};
use crate::store::{SynthStore, bare_props, json_out, parse_tree_spec};
use crate::wire_format as wf;

pub fn run(cmd: &OutcomeCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        OutcomeCmd::Add {
            tree_spec,
            status,
            note,
            linked_spec,
            date,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_add(
                &tree,
                &spec,
                status,
                note.as_deref(),
                linked_spec.as_deref(),
                date.as_deref(),
                session,
            )
        }
        OutcomeCmd::List { tree_spec } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_list(&tree, &spec)
        }
    }
}

fn cmd_add(
    tree: &str,
    spec: &str,
    status: &str,
    note: Option<&str>,
    linked_spec: Option<&str>,
    date: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    // The `superseded_by` -> `linked_spec` coupling lives at the
    // schema level (`crate::schema::outcome::validate`), so the
    // append below will reject a missing linked_spec with a
    // structured error. The CLI surface mirrors the rule in --help
    // so LLMs see it without round-tripping through an error.
    let mut store = SynthStore::discover_for(session)?;
    let mut props = json!({
        "tree": tree,
        "spec": spec,
        "status": status,
    });
    if let Some(n) = note {
        props["note"] = json!(n);
    }
    if let Some(l) = linked_spec {
        props["linked_spec"] = json!(l);
    }
    if let Some(d) = date {
        props["date"] = json!(d);
    } else {
        props["date"] = json!(today_iso());
    }
    let id = store
        .append(ClaimType::Outcome, props.clone(), None)
        .context("append outcome claim")?;
    emit(Output::new(json!({
        "ok": true,
        "claim_id": id,
        "outcome": props,
    })))
}

/// List live Outcome heads for `(tree, spec)`, newest first by
/// `prov:generatedAtTime`.
///
/// Output shape:
/// ```json
/// { "outcomes": [
///     { "tree": "...", "spec": "...", "status": "...",
///       "summary": "...", "asserted_at": "...", "asserted_by": "..." },
///     ...
/// ] }
/// ```
///
/// `summary` mirrors the v2 projection column name. The schema field
/// is `note` (the synthesist:summary predicate is not set on Outcomes),
/// so we OPTIONAL-bind both and project whichever the head carried.
fn cmd_list(tree: &str, spec: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let mut rows: Vec<(String, Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("outcome"))? {
        let props = bare_props(&doc);
        if props.get("tree").and_then(|v| v.as_str()) != Some(tree)
            || props.get("spec").and_then(|v| v.as_str()) != Some(spec)
        {
            continue;
        }
        let status = props
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // Prefer synthesist:summary if present, else synthesist:note --
        // the schema names the field `note`, the v2 projection called
        // it `summary`. Surface whichever the head carried under the
        // legacy column name to match the v2 output contract.
        let summary = props
            .get("summary")
            .or_else(|| props.get("note"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let at = doc
            .get(wf::GENERATED_AT_PRED)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let by = doc
            .get(wf::ATTRIBUTED_TO_PRED)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut obj = Map::new();
        obj.insert("tree".into(), Value::String(tree.to_string()));
        obj.insert("spec".into(), Value::String(spec.to_string()));
        obj.insert("status".into(), Value::String(status));
        obj.insert(
            "summary".into(),
            summary.map(Value::String).unwrap_or(Value::Null),
        );
        obj.insert("asserted_at".into(), Value::String(at.clone()));
        obj.insert("asserted_by".into(), Value::String(by));
        rows.push((at, Value::Object(obj)));
    }
    // ORDER BY DESC(?at): lexical == chronological for the canonical form.
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let outcomes: Vec<Value> = rows.into_iter().map(|(_, v)| v).collect();
    json_out(&json!({ "outcomes": outcomes }))
}

fn today_iso() -> String {
    use time::OffsetDateTime;
    use time::macros::format_description;
    OffsetDateTime::now_utc()
        .format(format_description!("[year]-[month]-[day]"))
        .unwrap_or_default()
}
