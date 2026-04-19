//! Campaign commands (v2: claim-backed).
//!
//! Writes and reads `Campaign` claims via the synthesist claim store.
//! Campaign is a cross-spec coordination primitive: a campaign can be
//! either `active` (currently being worked) or `backlog` (planned). The
//! v1 `campaign_active` / `campaign_backlog` / `campaign_blocked_by`
//! tables collapse into a single `Campaign` claim per spec.
//!
//! CLI surface is unchanged from v1.2.x.

use anyhow::Result;
use serde_json::{json, Value};

use crate::cli::CampaignCmd;
use crate::store::{json_out, SynthStore};

/// Dispatch a `synthesist campaign <...>` subcommand.
pub fn run(cmd: &CampaignCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        CampaignCmd::Add {
            tree,
            spec_id,
            summary,
            phase: _phase,
            backlog,
            title,
            blocked_by,
        } => {
            let kind = if *backlog { "backlog" } else { "active" };
            cmd_add(
                tree,
                spec_id,
                kind,
                title.as_deref(),
                summary,
                blocked_by,
                session,
            )
        }
        CampaignCmd::List { tree } => cmd_list(tree, session),
    }
}

/// Append a new `Campaign` claim (either `active` or `backlog`).
///
/// The `Campaign` props shape (per `nomograph_claim::schema::validate_campaign`):
/// ```json
/// { "tree": "...", "spec": "...", "kind": "active"|"backlog",
///   "summary": "...", "title": "...", "blocked_by": ["..."] }
/// ```
fn cmd_add(
    tree: &str,
    spec_id: &str,
    kind: &str,
    title: Option<&str>,
    summary: &str,
    blocked_by: &[String],
    session: &Option<String>,
) -> Result<()> {
    if tree.is_empty() {
        anyhow::bail!("Campaign requires non-empty 'tree' field");
    }
    if spec_id.is_empty() {
        anyhow::bail!("Campaign requires non-empty 'spec' field");
    }

    let mut props = serde_json::Map::new();
    props.insert("tree".into(), Value::from(tree));
    props.insert("spec".into(), Value::from(spec_id));
    props.insert("kind".into(), Value::from(kind));
    if !summary.is_empty() {
        props.insert("summary".into(), Value::from(summary));
    }
    if let Some(t) = title {
        if !t.is_empty() {
            props.insert("title".into(), Value::from(t));
        }
    }
    let deps: Vec<Value> = blocked_by
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| Value::from(s.as_str()))
        .collect();
    if !deps.is_empty() {
        props.insert("blocked_by".into(), Value::Array(deps));
    }

    let mut store = SynthStore::discover_for(session)?;
    store.append(
        nomograph_claim::ClaimType::Campaign,
        Value::Object(props),
        None,
    )?;

    json_out(&json!({"tree": tree, "spec_id": spec_id, "list": kind}))
}

/// List active and backlog `Campaign` claims for a tree.
///
/// When the same `(tree, spec)` pair has multiple Campaign claims (from
/// re-adds over time), the most recent `asserted_at` wins. This preserves
/// the v1 INSERT-OR-REPLACE semantic without requiring explicit
/// supersession claims.
fn cmd_list(tree: &str, session: &Option<String>) -> Result<()> {
    let store = SynthStore::discover_for(session)?;

    // Latest-per-spec via GROUP BY + MAX(asserted_at). SQLite's "bare
    // columns from aggregate" rule returns the row matching the MAX per
    // group.
    let active = store.query(
        "SELECT \
             json_extract(props, '$.spec')       AS spec_id, \
             json_extract(props, '$.summary')    AS summary, \
             json_extract(props, '$.blocked_by') AS blocked_by, \
             MAX(asserted_at)                     AS _max \
         FROM claims \
         WHERE claim_type = 'campaign' \
           AND json_extract(props, '$.tree') = ?1 \
           AND json_extract(props, '$.kind') = 'active' \
         GROUP BY json_extract(props, '$.spec') \
         ORDER BY json_extract(props, '$.spec')",
        &[&tree],
    )?;

    let backlog = store.query(
        "SELECT \
             json_extract(props, '$.spec')       AS spec_id, \
             json_extract(props, '$.title')      AS title, \
             json_extract(props, '$.summary')    AS summary, \
             json_extract(props, '$.blocked_by') AS blocked_by, \
             MAX(asserted_at)                     AS _max \
         FROM claims \
         WHERE claim_type = 'campaign' \
           AND json_extract(props, '$.tree') = ?1 \
           AND json_extract(props, '$.kind') = 'backlog' \
         GROUP BY json_extract(props, '$.spec') \
         ORDER BY json_extract(props, '$.spec')",
        &[&tree],
    )?;

    // Strip the `_max` helper column from each row before returning.
    let active = strip_max(active);
    let backlog = strip_max(backlog);

    json_out(&json!({"tree": tree, "active": active, "backlog": backlog}))
}

/// Remove the `_max` helper column used for GROUP BY latest-row selection.
fn strip_max(rows: Vec<Value>) -> Vec<Value> {
    rows.into_iter()
        .map(|v| match v {
            Value::Object(mut map) => {
                map.remove("_max");
                Value::Object(map)
            }
            other => other,
        })
        .collect()
}
