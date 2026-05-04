//! `outcome` CLI surface — first-class Outcome claim operations.
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

use anyhow::{Context, Result};
use nomograph_claim::ClaimType;
use serde_json::{Value, json};

use crate::cli::OutcomeCmd;
use crate::output::{Output, emit};
use crate::store::{SynthStore, json_out, parse_tree_spec};

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
            cmd_list(&tree, &spec, session)
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

fn cmd_list(tree: &str, spec: &str, session: &Option<String>) -> Result<()> {
    let store = SynthStore::discover_for(session)?;
    let rows = store.query(
        "SELECT id, asserted_by, asserted_at, props FROM claims \
         WHERE claim_type = 'outcome' \
           AND json_extract(props, '$.tree') = ?1 \
           AND json_extract(props, '$.spec') = ?2 \
         ORDER BY asserted_at DESC",
        &[&tree, &spec],
    )?;
    let outcomes: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            let props_str = r
                .get("props")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let props: Value = serde_json::from_str(props_str).unwrap_or(json!({}));
            json!({
                "claim_id": r.get("id").cloned().unwrap_or(Value::Null),
                "asserted_by": r.get("asserted_by").cloned().unwrap_or(Value::Null),
                "asserted_at": r.get("asserted_at").cloned().unwrap_or(Value::Null),
                "props": props,
            })
        })
        .collect();
    json_out(&json!({ "outcomes": outcomes }))
}

fn today_iso() -> String {
    use time::OffsetDateTime;
    use time::macros::format_description;
    OffsetDateTime::now_utc()
        .format(format_description!("[year]-[month]-[day]"))
        .unwrap_or_default()
}
