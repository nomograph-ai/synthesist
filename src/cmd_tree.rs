//! Tree commands -- ported to the v3 SPARQL substrate.
//!
//! Reference port (Stage 1). The supersession chain walk is now a
//! SPARQL `FILTER NOT EXISTS` instead of a SQL `supersedes IS NULL`
//! + client-side dedup.

use anyhow::{Result, anyhow, bail};
use nomograph_claim::ClaimType;
use serde_json::{Value, json};

use crate::cli::TreeCmd;
use crate::store::{SynthStore, json_out};

pub fn run(cmd: &TreeCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        TreeCmd::Add {
            name,
            description,
            status,
        } => cmd_tree_add(name, description, status, session),
        TreeCmd::List { include_closed } => cmd_tree_list(*include_closed),
        TreeCmd::Show { name } => cmd_tree_show(name),
        TreeCmd::Close { name, start_id } => cmd_tree_close(name, start_id.as_deref(), session),
    }
}

fn cmd_tree_add(
    name: &str,
    description: &str,
    status: &str,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let props = json!({ "name": name, "description": description });
    store.append(ClaimType::Tree, props, None)?;
    json_out(&json!({
        "ok": true,
        "name": name,
        "status": status,
        "description": description,
    }))
}

fn cmd_tree_list(include_closed: bool) -> Result<()> {
    let store = SynthStore::discover()?;
    let trees = live_tree_heads(&store, include_closed)?;
    json_out(&json!({ "trees": trees }))
}

fn cmd_tree_show(name: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    let trees = live_tree_heads(&store, true)?;
    let row = trees
        .iter()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some(name))
        .ok_or_else(|| anyhow!("tree not found: {name}. Try `synthesist tree list`."))?;

    // Spec count and session count via SPARQL.
    let spec_count = count_for_tree(&store, "synthesist:Spec", name)?;
    let session_count = count_for_tree(&store, "synthesist:Session", name)?;

    json_out(&json!({
        "name": row.get("name").cloned().unwrap_or(json!(name)),
        "description": row.get("description").cloned().unwrap_or(Value::Null),
        "spec_count": spec_count,
        "session_count": session_count,
    }))
}

/// `tree close <name>` -- find the live `Tree` head with the given
/// name and append a superseding `Tree` claim with `status = "closed"`.
///
/// When `--start-id` is supplied it disambiguates between trees that
/// share a name (any unambiguous hex prefix of the original opener's
/// claim hash). When omitted and multiple live trees match, bail with
/// a prescriptive error listing the candidates.
fn cmd_tree_close(name: &str, start_id: Option<&str>, session: &Option<String>) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;

    // Find the live Tree head(s) matching `name`. The list path filters
    // by `name` and surfaces the same `start_id` shape we accept here.
    let q = format!(
        r#"
        SELECT ?c ?desc ?status WHERE {{
          GRAPH ?g {{
            ?c rdf:type synthesist:Tree ;
               synthesist:name "{name}" .
            OPTIONAL {{ ?c synthesist:description ?desc }}
            OPTIONAL {{ ?c synthesist:status      ?status }}
            FILTER NOT EXISTS {{
              GRAPH ?g2 {{ ?later synthesist:supersedes ?c }}
            }}
          }}
        }}
        "#
    );
    let r = store.sparql(&q)?;
    use nomograph_claim::graph_view::Term;

    let mut candidates: Vec<(String, String, String)> = Vec::new();
    for row in &r.rows {
        let iri = match row.first() {
            Some(Term::Iri(s)) => s.clone(),
            _ => continue,
        };
        let desc = match row.get(1) {
            Some(Term::Literal { value, .. }) => value.clone(),
            _ => String::new(),
        };
        let status = match row.get(2) {
            Some(Term::Literal { value, .. }) if !value.is_empty() => value.clone(),
            _ => "active".to_string(),
        };
        candidates.push((iri, desc, status));
    }

    if candidates.is_empty() {
        bail!(
            "tree not found: {name}. \
             Run `synthesist tree list --include-closed` to see all trees, or \
             `synthesist tree add {name}` to create it."
        );
    }

    // Filter to those still active (skip already-closed trees) before
    // disambiguating, so `close` on an already-closed name complains
    // about no candidates rather than silently re-closing.
    let active: Vec<(String, String, String)> = candidates
        .iter()
        .filter(|(_, _, s)| s != "closed")
        .cloned()
        .collect();
    if active.is_empty() {
        bail!(
            "tree '{name}' is already closed; \
             list with `synthesist tree list --include-closed` to confirm"
        );
    }

    let (iri, desc, _status) = match start_id {
        Some(prefix) if !prefix.is_empty() => {
            let matched: Vec<(String, String, String)> = active
                .iter()
                .filter(|(iri, _, _)| short_claim_id(iri).starts_with(prefix))
                .cloned()
                .collect();
            match matched.len() {
                0 => bail!(
                    "no active tree '{name}' matches --start-id '{prefix}'; \
                     run `synthesist tree list` to see candidates"
                ),
                1 => matched.into_iter().next().unwrap(),
                _ => bail!(
                    "--start-id '{prefix}' is ambiguous among {} active trees named '{name}'; \
                     supply a longer prefix",
                    matched.len()
                ),
            }
        }
        _ => {
            if active.len() > 1 {
                let ids: Vec<String> =
                    active.iter().map(|(iri, _, _)| short_claim_id(iri)).collect();
                bail!(
                    "multiple active trees named '{name}'; \
                     disambiguate with `synthesist tree close {name} --start-id <prefix>` \
                     (candidates: {})",
                    ids.join(", ")
                );
            }
            active.into_iter().next().unwrap()
        }
    };

    let prior_id = short_claim_id(&iri);
    let mut props = serde_json::Map::new();
    props.insert("name".into(), Value::String(name.to_string()));
    if !desc.is_empty() {
        props.insert("description".into(), Value::String(desc));
    }
    props.insert("status".into(), Value::String("closed".to_string()));

    store.append(
        ClaimType::Tree,
        Value::Object(props),
        Some(prior_id.clone()),
    )?;
    json_out(&json!({
        "closed": true,
        "name": name,
        "start_id": prior_id,
    }))
}

// ---------------------------------------------------------------------------
// SPARQL helpers
// ---------------------------------------------------------------------------

/// Live Tree heads (no later claim supersedes them). When
/// `include_closed` is false, filter out any whose status is "closed".
fn live_tree_heads(store: &SynthStore, include_closed: bool) -> Result<Vec<Value>> {
    let q = r#"
        SELECT ?c ?name ?desc ?status WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Tree ;
               synthesist:name ?name .
            OPTIONAL { ?c synthesist:description ?desc }
            OPTIONAL { ?c synthesist:status      ?status }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        ORDER BY ?name
    "#;
    let r = store.sparql(q)?;
    let mut out: Vec<Value> = Vec::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let claim_iri = match row.first() {
            Some(Term::Iri(s)) => s.clone(),
            _ => continue,
        };
        let name = match row.get(1) {
            Some(Term::Literal { value, .. }) if !value.is_empty() => value.clone(),
            _ => continue,
        };
        let desc = match row.get(2) {
            Some(Term::Literal { value, .. }) => Value::String(value.clone()),
            _ => Value::Null,
        };
        let status = match row.get(3) {
            Some(Term::Literal { value, .. }) if !value.is_empty() => value.clone(),
            _ => "active".to_string(),
        };
        if !include_closed && status == "closed" {
            continue;
        }
        out.push(json!({
            "name": name,
            "status": status,
            "description": desc,
            "start_id": short_claim_id(&claim_iri),
        }));
    }
    Ok(out)
}

fn count_for_tree(store: &SynthStore, type_iri: &str, tree: &str) -> Result<i64> {
    let q = format!(
        r#"
        SELECT (COUNT(DISTINCT ?c) AS ?n) WHERE {{
          GRAPH ?g {{
            ?c rdf:type {type_iri} ;
               synthesist:tree "{tree}" .
          }}
        }}
        "#
    );
    let r = store.sparql(&q)?;
    use nomograph_claim::graph_view::Term;
    Ok(match r.rows.first().and_then(|row| row.first()) {
        Some(Term::Literal { value, .. }) => value.parse().unwrap_or(0),
        _ => 0,
    })
}

/// Strip the IRI prefix to recover a bare claim hash for display.
fn short_claim_id(iri: &str) -> String {
    iri.strip_prefix("https://nomograph.org/synthesist/claim/")
        .or_else(|| iri.strip_prefix("synthesist:claim/"))
        .unwrap_or(iri)
        .to_string()
}
