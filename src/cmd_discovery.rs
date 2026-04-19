//! Discovery commands (v2: claim-backed).
//!
//! Writes and reads `Discovery` claims via the synthesist claim store.
//! Every `discovery add` appends one [`nomograph_claim::ClaimType::Discovery`]
//! claim; `discovery list` queries the SQLite view projection.
//!
//! The CLI surface is unchanged from v1.2.x: same subcommands, same flags,
//! same JSON output shape. Implementation moved from the v1 `discoveries`
//! SQL table to the claim substrate per D9/BUILDING-wave4-synthesist.md §M3.

use anyhow::Result;
use serde_json::{json, Value};

use crate::cli::DiscoveryCmd;
use crate::store::{json_out, parse_tree_spec, today, SynthStore};

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
            cmd_list(&tree, &spec, session)
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
        anyhow::bail!("Discovery requires non-empty 'finding' field");
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
        nomograph_claim::ClaimType::Discovery,
        Value::Object(props),
        None,
    )?;

    json_out(&json!({"id": id, "finding": finding, "date": date}))
}

/// List every `Discovery` claim scoped to `tree/spec`.
///
/// Results are ordered by `asserted_at` descending so the most recently
/// appended discovery appears first. JSON output shape matches v1.
fn cmd_list(tree: &str, spec: &str, session: &Option<String>) -> Result<()> {
    let store = SynthStore::discover_for(session)?;
    let rows = store.query(
        "SELECT \
             json_extract(props, '$.id')       AS id, \
             json_extract(props, '$.date')     AS date, \
             json_extract(props, '$.author')   AS author, \
             json_extract(props, '$.finding')  AS finding, \
             json_extract(props, '$.impact')   AS impact, \
             json_extract(props, '$.action')   AS action \
         FROM claims \
         WHERE claim_type = 'discovery' \
           AND json_extract(props, '$.tree') = ?1 \
           AND json_extract(props, '$.spec') = ?2 \
         ORDER BY asserted_at DESC",
        &[&tree, &spec],
    )?;

    json_out(&json!({"tree": tree, "spec": spec, "discoveries": rows}))
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
