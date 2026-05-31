//! Session commands (Path B Stage 1: v3-native).
//!
//! `session start` writes one v3 Session claim with session-scoped
//! asserter. `session close` writes a superseding Session claim.
//! Reads (`list`, `status`) walk the SPARQL view.

use anyhow::{Result, anyhow, bail};
use nomograph_claim::ClaimType;
use serde_json::{Value, json};

use crate::cli::SessionCmd;
use crate::store::{SynthStore, json_out};

pub fn run(cmd: &SessionCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        SessionCmd::Start {
            id,
            tree,
            spec,
            summary,
        } => cmd_session_start(id, tree.as_deref(), spec.as_deref(), summary.as_deref()),
        SessionCmd::List => cmd_session_list(),
        SessionCmd::Status { id } => cmd_session_status(id),
        SessionCmd::Merge { .. } => bail!(
            "session merge removed in v2; merges are automatic (git pull; CRDT merge)."
        ),
        SessionCmd::Discard { .. } => bail!(
            "session discard removed in v2; use `synthesist session close <id>` instead."
        ),
        SessionCmd::Close { id, start_id } => {
            cmd_session_close(id, start_id.as_deref(), session)
        }
    }
}

fn asserter_base() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".into());
    format!("user:local:{user}")
}

fn cmd_session_start(
    id: &str,
    tree: Option<&str>,
    spec: Option<&str>,
    summary: Option<&str>,
) -> Result<()> {
    if id.is_empty() {
        bail!("session id must be non-empty");
    }
    let base = asserter_base();
    let session_asserter = format!("{}:{}", base, id);
    let mut props = serde_json::Map::new();
    props.insert("id".to_string(), Value::String(id.to_string()));
    if let Some(t) = tree {
        props.insert("tree".to_string(), Value::String(t.to_string()));
    }
    if let Some(s) = spec {
        props.insert("spec".to_string(), Value::String(s.to_string()));
    }
    if let Some(s) = summary {
        props.insert("summary".to_string(), Value::String(s.to_string()));
    }

    let mut store = SynthStore::discover()?.with_asserter(session_asserter.clone());
    store
        .append(ClaimType::Session, Value::Object(props), None)
        .map_err(|e| anyhow!("session start failed: {e}"))?;

    json_out(&json!({
        "id": id,
        "asserter": session_asserter,
        "started_at": Value::Null,
    }))
}

fn cmd_session_list() -> Result<()> {
    let store = SynthStore::discover()?;
    let q = r#"
        SELECT ?c ?id ?tree ?spec ?summary WHERE {
          GRAPH ?g {
            ?c rdf:type synthesist:Session ;
               synthesist:id ?id .
            OPTIONAL { ?c synthesist:tree ?tree }
            OPTIONAL { ?c synthesist:spec ?spec }
            OPTIONAL { ?c synthesist:summary ?summary }
            FILTER NOT EXISTS { ?c synthesist:supersedes ?prev }
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?c }
            }
          }
        }
        ORDER BY ?id
    "#;
    let r = store.sparql(q)?;
    let mut out: Vec<Value> = Vec::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let str_at = |i: usize| -> Option<String> {
            match row.get(i) {
                Some(Term::Literal { value, .. }) if !value.is_empty() => Some(value.clone()),
                _ => None,
            }
        };
        let iri = match row.first() {
            Some(Term::Iri(s)) => s.clone(),
            _ => continue,
        };
        let id = match str_at(1) {
            Some(s) => s,
            None => continue,
        };
        out.push(json!({
            "id": id,
            "tree": str_at(2),
            "spec": str_at(3),
            "summary": str_at(4),
            "asserter_base": format!("{}:{}", asserter_base(), id),
            "start_id": short_claim_id(&iri),
        }));
    }
    json_out(&json!({ "sessions": out }))
}

fn cmd_session_status(id: &str) -> Result<()> {
    let store = SynthStore::discover()?;
    // Pull the opener's props + start time in one SELECT. The
    // `FILTER NOT EXISTS { ?c synthesist:supersedes ?prev }` clause
    // pins ?c to the opener (vs the closer, which carries the same
    // synthesist:id but DOES supersede a prior).
    let q_opener = format!(
        r#"
        SELECT ?c ?tree ?spec ?summary ?started_at WHERE {{
          GRAPH ?g {{
            ?c rdf:type synthesist:Session ;
               synthesist:id "{id}" ;
               prov:generatedAtTime ?started_at .
            OPTIONAL {{ ?c synthesist:tree    ?tree }}
            OPTIONAL {{ ?c synthesist:spec    ?spec }}
            OPTIONAL {{ ?c synthesist:summary ?summary }}
            FILTER NOT EXISTS {{ ?c synthesist:supersedes ?prev }}
          }}
        }}
        LIMIT 1
        "#
    );
    let r = store.sparql(&q_opener)?;
    let row = r.rows.into_iter().next().ok_or_else(|| {
        anyhow!(
            "session '{id}' not found. \
             Run `synthesist session list` to see known sessions, \
             or `synthesist session start <id>` to open a new one."
        )
    })?;
    use nomograph_claim::graph_view::Term;
    let str_at = |i: usize| -> Option<String> {
        match row.get(i) {
            Some(Term::Literal { value, .. }) if !value.is_empty() => Some(value.clone()),
            _ => None,
        }
    };
    let tree = str_at(1);
    let spec = str_at(2);
    let summary = str_at(3);
    let started_at = str_at(4);

    // Live = opener with no later claim superseding it.
    let q_live = format!(
        r#"
        ASK {{
          GRAPH ?g {{
            ?c rdf:type synthesist:Session ;
               synthesist:id "{id}" .
            FILTER NOT EXISTS {{ ?c synthesist:supersedes ?prev }}
            FILTER NOT EXISTS {{
              GRAPH ?g2 {{ ?later synthesist:supersedes ?c }}
            }}
          }}
        }}
        "#
    );
    let live = store.ask(&q_live)?;
    let status = if live { "active" } else { "closed" };

    let mut props = serde_json::Map::new();
    props.insert("id".into(), Value::String(id.to_string()));
    if let Some(t) = tree {
        props.insert("tree".into(), Value::String(t));
    }
    if let Some(s) = spec {
        props.insert("spec".into(), Value::String(s));
    }
    if let Some(s) = summary {
        props.insert("summary".into(), Value::String(s));
    }

    json_out(&json!({
        "id": id,
        "status": status,
        "started_at": started_at.map(Value::String).unwrap_or(Value::Null),
        "props": Value::Object(props),
    }))
}

fn cmd_session_close(id: &str, start_id: Option<&str>, session: &Option<String>) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;

    // Collect all live openers for this display id. The v2 contract
    // tolerates name collisions across sessions; `--start-id` picks the
    // intended target. With one live opener we proceed; with more we
    // require disambiguation (or, when no prefix is supplied, fall back
    // to the most recently asserted opener per `prov:generatedAtTime`).
    //
    // ORDER BY DESC pushes the freshest opener to the top so the
    // implicit "single live session" path keeps the v2 behaviour.
    let q = format!(
        r#"
        SELECT ?c ?tree ?spec ?summary ?ts WHERE {{
          GRAPH ?g {{
            ?c rdf:type synthesist:Session ;
               synthesist:id "{id}" ;
               prov:generatedAtTime ?ts .
            OPTIONAL {{ ?c synthesist:tree    ?tree }}
            OPTIONAL {{ ?c synthesist:spec    ?spec }}
            OPTIONAL {{ ?c synthesist:summary ?summary }}
            FILTER NOT EXISTS {{ ?c synthesist:supersedes ?prev }}
            FILTER NOT EXISTS {{
              GRAPH ?g2 {{ ?later synthesist:supersedes ?c }}
            }}
          }}
        }}
        ORDER BY DESC(?ts)
        "#
    );
    let r = store.sparql(&q)?;
    use nomograph_claim::graph_view::Term;

    struct Candidate {
        iri: String,
        tree: Option<String>,
        spec: Option<String>,
        summary: Option<String>,
    }

    let mut candidates: Vec<Candidate> = Vec::new();
    for row in &r.rows {
        let iri = match row.first() {
            Some(Term::Iri(s)) => s.clone(),
            _ => continue,
        };
        let str_at = |i: usize| -> Option<String> {
            match row.get(i) {
                Some(Term::Literal { value, .. }) if !value.is_empty() => Some(value.clone()),
                _ => None,
            }
        };
        candidates.push(Candidate {
            iri,
            tree: str_at(1),
            spec: str_at(2),
            summary: str_at(3),
        });
    }

    if candidates.is_empty() {
        bail!(
            "session '{id}' not found or already closed. \
             Run `synthesist session list` to see live sessions."
        );
    }

    let chosen = match start_id {
        Some(prefix) if !prefix.is_empty() => {
            let matched: Vec<&Candidate> = candidates
                .iter()
                .filter(|c| short_claim_id(&c.iri).starts_with(prefix))
                .collect();
            match matched.len() {
                0 => {
                    let ids: Vec<String> =
                        candidates.iter().map(|c| short_claim_id(&c.iri)).collect();
                    bail!(
                        "no live session '{id}' matches --start-id '{prefix}' \
                         (candidates: {})",
                        ids.join(", ")
                    );
                }
                1 => matched.into_iter().next().unwrap(),
                _ => {
                    let ids: Vec<String> =
                        matched.iter().map(|c| short_claim_id(&c.iri)).collect();
                    bail!(
                        "--start-id '{prefix}' is ambiguous among {} live sessions named '{id}' \
                         (candidates: {}); supply a longer prefix",
                        ids.len(),
                        ids.join(", ")
                    );
                }
            }
        }
        _ => {
            // No prefix supplied. With multiple live openers the v2
            // contract takes the most recently asserted one (already at
            // the head of the ORDER BY DESC list); that keeps the
            // single-session happy path stable while still terminating
            // cleanly on name collisions without forcing the caller to
            // pick.
            candidates.first().unwrap()
        }
    };

    let prior_id = short_claim_id(&chosen.iri);
    let mut props = serde_json::Map::new();
    props.insert("id".into(), Value::String(id.to_string()));
    if let Some(t) = chosen.tree.clone() {
        props.insert("tree".into(), Value::String(t));
    }
    if let Some(s) = chosen.spec.clone() {
        props.insert("spec".into(), Value::String(s));
    }
    if let Some(s) = chosen.summary.clone() {
        props.insert("summary".into(), Value::String(s));
    }

    store.append(
        ClaimType::Session,
        Value::Object(props),
        Some(prior_id.clone()),
    )?;
    json_out(&json!({ "closed": true, "id": id, "start_id": prior_id }))
}

fn short_claim_id(iri: &str) -> String {
    iri.strip_prefix("https://nomograph.org/synthesist/claim/")
        .or_else(|| iri.strip_prefix("synthesist:claim/"))
        .unwrap_or(iri)
        .to_string()
}
