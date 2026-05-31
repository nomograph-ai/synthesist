//! Campaign commands -- ported to the v3 SPARQL substrate.
//!
//! Campaign is a cross-spec coordination primitive: each Campaign claim
//! per (tree, spec) is either `active`, `backlog`, or (after the close
//! port lands at the CLI level) `closed`. Reads project the live head
//! per (tree, spec) via SPARQL; writes route through `SynthStore::append`.
//!
//! NOTE: the v2 `CampaignCmd` enum exposes only `Add` and `List`. The
//! Stage 2 brief mentions a `campaign close <tree>/<spec>` subcommand,
//! which would require an additive change in `cli.rs`. That CLI surface
//! is intentionally out of scope for this commit (the constraint pins
//! us to `cmd_campaign.rs` / `cmd_outcome.rs`); see the report.

use anyhow::Result;
use serde_json::{Map, Value, json};

use crate::cli::CampaignCmd;
use crate::store::{SynthStore, json_out};

/// Dispatch a `synthesist campaign <...>` subcommand.
pub fn run(cmd: &CampaignCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        CampaignCmd::Add {
            tree,
            spec_id,
            summary,
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
        CampaignCmd::List { tree } => cmd_list(Some(tree.as_str())),
    }
}

/// Append a new `Campaign` claim (either `active` or `backlog`).
///
/// The `Campaign` props shape (per `crate::schema::campaign::validate`):
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
        anyhow::bail!(
            "campaign add requires a non-empty tree name; \
             pass it as the first positional argument: synthesist campaign add <tree> <spec_id>"
        );
    }
    if spec_id.is_empty() {
        anyhow::bail!(
            "campaign add requires a non-empty spec id; \
             pass it as the second positional argument: synthesist campaign add <tree> <spec_id>"
        );
    }

    let mut props = Map::new();
    props.insert("tree".into(), Value::from(tree));
    props.insert("spec".into(), Value::from(spec_id));
    props.insert("kind".into(), Value::from(kind));
    if !summary.is_empty() {
        props.insert("summary".into(), Value::from(summary));
    }
    if let Some(t) = title
        && !t.is_empty()
    {
        props.insert("title".into(), Value::from(t));
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

    json_out(&json!({"ok": true, "tree": tree, "spec_id": spec_id, "kind": kind}))
}

/// List the live Campaign head per (tree, spec) pair. Filter on `tree`
/// when supplied (the v2 CLI takes a required positional `tree`; the
/// `Option<&str>` signature anticipates the future `--tree`-as-flag
/// surface mentioned in the Stage 2 brief).
///
/// Output:
/// ```json
/// { "campaigns": [
///     { "tree": "...", "spec": "...", "kind": "...",
///       "summary": "...", "title": "...", "blocked_by": ["..."] },
///     ...
/// ] }
/// ```
fn cmd_list(tree_filter: Option<&str>) -> Result<()> {
    let store = SynthStore::discover()?;
    let tree_constraint = match tree_filter {
        Some(t) => format!("FILTER(?tree = \"{t}\")"),
        None => String::new(),
    };
    let q = format!(
        r#"
        SELECT ?c ?tree ?spec ?kind ?summary ?title
               (GROUP_CONCAT(?blk; SEPARATOR="\u001F") AS ?blocked_by)
        WHERE {{
          GRAPH ?g {{
            ?c rdf:type synthesist:Campaign ;
               synthesist:tree ?tree ;
               synthesist:spec ?spec .
            OPTIONAL {{ ?c synthesist:kind      ?kind }}
            OPTIONAL {{ ?c synthesist:summary   ?summary }}
            OPTIONAL {{ ?c synthesist:title     ?title }}
            OPTIONAL {{ ?c synthesist:blockedBy ?blk }}
            FILTER NOT EXISTS {{
              GRAPH ?g2 {{ ?later synthesist:supersedes ?c }}
            }}
            {tree_constraint}
          }}
        }}
        GROUP BY ?c ?tree ?spec ?kind ?summary ?title
        ORDER BY ?tree ?spec
        "#
    );
    let r = store.sparql(&q)?;
    let mut campaigns: Vec<Value> = Vec::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let str_at = |i: usize| -> Option<String> {
            match row.get(i) {
                Some(Term::Literal { value, .. }) if !value.is_empty() => Some(value.clone()),
                _ => None,
            }
        };
        let tree = match str_at(1) {
            Some(s) => s,
            None => continue,
        };
        let spec = match str_at(2) {
            Some(s) => s,
            None => continue,
        };
        let kind = str_at(3);
        let summary = str_at(4);
        let title = str_at(5);
        let blocked_by_concat = str_at(6).unwrap_or_default();
        let blocked_by: Vec<Value> = if blocked_by_concat.is_empty() {
            Vec::new()
        } else {
            blocked_by_concat
                .split('\u{001F}')
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect()
        };

        let mut obj = Map::new();
        obj.insert("tree".into(), Value::String(tree));
        obj.insert("spec".into(), Value::String(spec));
        if let Some(v) = kind {
            obj.insert("kind".into(), Value::String(v));
        }
        if let Some(v) = summary {
            obj.insert("summary".into(), Value::String(v));
        }
        if let Some(v) = title {
            obj.insert("title".into(), Value::String(v));
        }
        obj.insert("blocked_by".into(), Value::Array(blocked_by));
        campaigns.push(Value::Object(obj));
    }
    json_out(&json!({ "campaigns": campaigns }))
}
