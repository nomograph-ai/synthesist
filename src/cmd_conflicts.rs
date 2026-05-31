//! `synthesist conflicts` -- surface diamond conflicts in the claim log.
//!
//! A diamond conflict is a prior claim that has been superseded by more
//! than one distinct live successor. Happens when two peers, working
//! offline, supersede the same prior claim in different ways. CRDT
//! merge delivers both successor edges cleanly; resolution means
//! appending a new claim that supersedes the contested pair.
//!
//! Ported to v3 SPARQL (Path B Stage 2). The v2 implementation loaded
//! every claim into memory and built a `HashMap<prior, Vec<super>>` to
//! find priors with >1 distinct super. The SPARQL version aggregates
//! the same shape with a single `GROUP BY ?prior HAVING (COUNT > 1)`,
//! restricted to live successors via the standard
//! `FILTER NOT EXISTS { ?later synthesist:supersedes ?super }` pattern
//! that Stage 1 ports use across `cmd_tree`, `cmd_spec`, `cmd_task`.

use anyhow::Result;
use serde_json::{Value, json};

use crate::store::{SynthStore, json_out};

pub fn cmd_conflicts() -> Result<()> {
    let store = SynthStore::discover()?;

    // Aggregate over (prior, super) edges, keeping only edges whose
    // ?super is itself a live head (not superseded by anything later).
    // GROUP_CONCAT with a unit-separator joiner so successor IRIs that
    // contain `:` (they all do) survive the round-trip cleanly.
    let q = r#"
        SELECT ?prior
               (GROUP_CONCAT(DISTINCT ?super; SEPARATOR="\u001F") AS ?supers)
               (COUNT(DISTINCT ?super) AS ?n)
        WHERE {
          GRAPH ?g {
            ?super synthesist:supersedes ?prior .
            FILTER NOT EXISTS {
              GRAPH ?g2 { ?later synthesist:supersedes ?super }
            }
          }
        }
        GROUP BY ?prior
        HAVING (COUNT(DISTINCT ?super) > 1)
        ORDER BY ?prior
    "#;

    let r = store.sparql(q)?;
    let mut conflicts: Vec<Value> = Vec::new();
    for row in &r.rows {
        use nomograph_claim::graph_view::Term;
        let prior_iri = match row.first() {
            Some(Term::Iri(s)) => s.clone(),
            _ => continue,
        };
        let supers_concat = match row.get(1) {
            Some(Term::Literal { value, .. }) => value.clone(),
            _ => continue,
        };
        let mut superseders: Vec<String> = supers_concat
            .split('\u{001F}')
            .filter(|s| !s.is_empty())
            .map(short_claim_id)
            .collect();
        superseders.sort();
        superseders.dedup();
        if superseders.len() > 1 {
            conflicts.push(json!({
                "prior": short_claim_id(&prior_iri),
                "superseders": superseders,
            }));
        }
    }

    json_out(&json!({ "conflicts": conflicts }))
}

/// Strip the IRI prefix to recover a bare claim hash for display.
fn short_claim_id(iri: &str) -> String {
    iri.strip_prefix("https://nomograph.org/synthesist/claim/")
        .or_else(|| iri.strip_prefix("synthesist:claim/"))
        .unwrap_or(iri)
        .to_string()
}
