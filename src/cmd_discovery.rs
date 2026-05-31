//! Discovery commands -- ported to the v3 SPARQL substrate.
//!
//! Writes and reads `Discovery` claims via the synthesist claim store.
//! Every `discovery add` appends one [`nomograph_claim::ClaimType::Discovery`]
//! claim through `SynthStore::append`; `discovery list` projects the live
//! Discovery heads scoped to `(tree, spec)` via SPARQL.
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
use crate::store::{SynthStore, json_out, parse_tree_spec, today};

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
        nomograph_claim::ClaimType::Discovery,
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
    let q = format!(
        r#"
        SELECT ?id ?date ?author ?finding ?impact ?action ?asserted_at ?asserted_by WHERE {{
          GRAPH ?g {{
            ?c rdf:type synthesist:Discovery ;
               synthesist:tree "{tree}" ;
               synthesist:spec "{spec}" ;
               synthesist:id ?id .
            OPTIONAL {{ ?c synthesist:date           ?date }}
            OPTIONAL {{ ?c synthesist:author         ?author }}
            OPTIONAL {{ ?c synthesist:finding        ?finding }}
            OPTIONAL {{ ?c synthesist:impact         ?impact }}
            OPTIONAL {{ ?c synthesist:action         ?action }}
            OPTIONAL {{ ?c prov:generatedAtTime      ?asserted_at }}
            OPTIONAL {{ ?c prov:wasAttributedTo      ?asserted_by }}
            FILTER NOT EXISTS {{
              GRAPH ?g2 {{ ?later synthesist:supersedes ?c }}
            }}
          }}
        }}
        ORDER BY DESC(?date)
        "#
    );
    let r = store.sparql(&q)?;
    let mut out: Vec<Value> = Vec::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let s = |i: usize| match row.get(i) {
            Some(Term::Literal { value, .. }) if !value.is_empty() => Value::String(value.clone()),
            // asserted_by is an IRI (asserter:user:local:...), surfaced
            // as a string so the JSON contract stays scalar.
            Some(Term::Iri(value)) if !value.is_empty() => Value::String(value.clone()),
            _ => Value::Null,
        };
        out.push(json!({
            "id":           s(0),
            "date":         s(1),
            "author":       s(2),
            "finding":      s(3),
            "impact":       s(4),
            "action":       s(5),
            "asserted_at":  s(6),
            "asserted_by":  s(7),
        }));
    }
    json_out(&json!({"tree": tree, "spec": spec, "discoveries": out}))
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
