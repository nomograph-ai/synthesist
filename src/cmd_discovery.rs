//! Discovery commands -- ported to the v3 redb-gamma substrate.
//!
//! Writes and reads `Discovery` claims via the synthesist claim store.
//! Every `discovery add` appends one [`crate::claim_type::ClaimType::Discovery`]
//! claim through `SynthStore::append`; `discovery list` projects the live
//! Discovery heads scoped to `(tree, spec)` via the gamma index.
//!
//! The CLI surface is unchanged from v1.2.x: same subcommands, same flags,
//! same JSON output shape. Path B Stage 2 finishes the port to v3:
//! `discovery list` now projects asserted_at / asserted_by from the
//! substrate-level `prov:generatedAtTime` / `prov:wasAttributedTo`
//! predicates and applies the standard live-head FILTER NOT EXISTS
//! supersession filter, matching the pattern Stage 1 reference ports
//! use across `cmd_tree`, `cmd_spec`, `cmd_task`.

use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::DiscoveryCmd;
use crate::store::{SynthStore, bare_props, json_out, parse_tree_spec, today};
use crate::wire_format as wf;

/// Dispatch a `synthesist discovery <...>` subcommand.
pub fn run(cmd: &DiscoveryCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        DiscoveryCmd::Add {
            tree_spec,
            finding,
            impact,
            action,
            author,
            date,
        } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_add(
                &tree,
                &spec,
                finding,
                impact.as_deref(),
                action.as_deref(),
                author.as_deref(),
                date.as_deref(),
                session,
            )
        }
        DiscoveryCmd::List { tree_spec } => {
            let (tree, spec) = parse_tree_spec(tree_spec)?;
            cmd_list(&tree, &spec)
        }
    }
}

/// Append a new `Discovery` claim.
///
/// The spec-scoped `id` is a short blake3 hash of `finding + date` prefixed
/// with `d-`. This keeps per-spec ids stable and content-addressed without
/// a counter, which matters when multiple sessions append concurrently.
#[allow(clippy::too_many_arguments)]
fn cmd_add(
    tree: &str,
    spec: &str,
    finding: &str,
    impact: Option<&str>,
    action: Option<&str>,
    author: Option<&str>,
    date: Option<&str>,
    session: &Option<String>,
) -> Result<()> {
    if finding.is_empty() {
        anyhow::bail!(
            "discovery add requires a non-empty --finding value; \
             pass it as the positional argument: synthesist discovery add <tree/spec> <finding>"
        );
    }

    let today = today();
    let date = date.unwrap_or(&today).to_string();
    let id = discovery_id(finding, &date);

    let mut props = serde_json::Map::new();
    props.insert("tree".into(), Value::from(tree));
    props.insert("spec".into(), Value::from(spec));
    props.insert("id".into(), Value::from(id.clone()));
    props.insert("date".into(), Value::from(date.clone()));
    props.insert("finding".into(), Value::from(finding));
    if let Some(v) = author {
        props.insert("author".into(), Value::from(v));
    }
    if let Some(v) = impact {
        props.insert("impact".into(), Value::from(v));
    }
    if let Some(v) = action {
        props.insert("action".into(), Value::from(v));
    }

    let mut store = SynthStore::discover_for(session)?;
    store.append(
        crate::claim_type::ClaimType::Discovery,
        Value::Object(props),
        None,
    )?;

    json_out(&json!({"id": id, "finding": finding, "date": date}))
}

/// List every live `Discovery` head scoped to `tree/spec`.
///
/// Projects the v2 contract columns: id, date, author, finding, impact,
/// action, asserted_at, asserted_by. The first six come from
/// `synthesist:*` predicates on the Discovery claim itself; the last
/// two come from the substrate-level `prov:generatedAtTime` and
/// `prov:wasAttributedTo` predicates the v3 wire format emits on every
/// claim. Filters out claims that have been superseded (live-head
/// pattern shared with the rest of the Stage 1/2 ports).
fn cmd_list(tree: &str, spec: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let mut out: Vec<(String, Value)> = Vec::new();
    for (_, doc) in store.live_docs(&wf::type_iri("discovery"))? {
        let props = bare_props(&doc);
        if props.get("tree").and_then(|v| v.as_str()) != Some(tree)
            || props.get("spec").and_then(|v| v.as_str()) != Some(spec)
        {
            continue;
        }
        let s = |k: &str| -> Value {
            props
                .get(k)
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .map(|v| Value::String(v.to_string()))
                .unwrap_or(Value::Null)
        };
        // asserted_at / asserted_by come from the substrate envelope
        // (prov:generatedAtTime / prov:wasAttributedTo).
        let asserted_at = doc
            .get(wf::GENERATED_AT_PRED)
            .and_then(|v| v.as_str())
            .map(|v| Value::String(v.to_string()))
            .unwrap_or(Value::Null);
        let asserted_by = doc
            .get(wf::ATTRIBUTED_TO_PRED)
            .and_then(|v| v.as_str())
            .map(|v| Value::String(v.to_string()))
            .unwrap_or(Value::Null);
        let date = props
            .get("date")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push((
            date,
            json!({
                "id":           s("id"),
                "date":         s("date"),
                "author":       s("author"),
                "finding":      s("finding"),
                "impact":       s("impact"),
                "action":       s("action"),
                "asserted_at":  asserted_at,
                "asserted_by":  asserted_by,
            }),
        ));
    }
    // ORDER BY DESC(?date) in the v2 contract.
    out.sort_by(|a, b| b.0.cmp(&a.0));
    let discoveries: Vec<Value> = out.into_iter().map(|(_, v)| v).collect();
    json_out(&json!({"tree": tree, "spec": spec, "discoveries": discoveries}))
}

/// Compute a short stable id for a discovery from `finding + date`.
///
/// Format: `d-<12-hex-chars>`. Short enough to type, stable for the same
/// (finding, date) pair, and unlikely to collide within a single spec.
/// Uses `std::hash::DefaultHasher` (not cryptographic) because the id is a
/// convenience key, not a security boundary — the Claim `id` already
/// provides content-addressed integrity at the substrate layer.
fn discovery_id(finding: &str, date: &str) -> String {
    use std::hash::{Hash, Hasher};
    // Mix date and finding with two rotations of the default hasher to
    // produce 128 bits of fingerprint, then format as 16 hex chars (we
    // keep 12 for terseness).
    let mut h1 = std::collections::hash_map::DefaultHasher::new();
    "discovery".hash(&mut h1);
    finding.hash(&mut h1);
    date.hash(&mut h1);
    let a = h1.finish();

    let mut h2 = std::collections::hash_map::DefaultHasher::new();
    "v2".hash(&mut h2);
    date.hash(&mut h2);
    finding.hash(&mut h2);
    let b = h2.finish();

    let hex = format!("{:016x}{:016x}", a, b);
    format!("d-{}", &hex[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_id_is_stable() {
        let a = discovery_id("ship early", "2026-04-18");
        let b = discovery_id("ship early", "2026-04-18");
        assert_eq!(a, b);
    }

    #[test]
    fn discovery_id_differs_on_finding() {
        let a = discovery_id("ship early", "2026-04-18");
        let b = discovery_id("ship late", "2026-04-18");
        assert_ne!(a, b);
    }

    #[test]
    fn discovery_id_has_prefix() {
        assert!(discovery_id("x", "2026-04-18").starts_with("d-"));
    }
}
