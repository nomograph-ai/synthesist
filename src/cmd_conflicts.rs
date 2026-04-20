//! `synthesist conflicts` — surface diamond conflicts in the claim log.
//!
//! A diamond conflict is a prior claim that has been superseded by more
//! than one distinct live successor. Happens when two peers, working
//! offline, supersede the same prior claim in different ways. CRDT
//! merge delivers both successor edges cleanly; resolution means
//! appending a new claim that supersedes the contested pair.
//!
//! Mirrors `claim conflicts` but is reachable from the synthesist CLI
//! so users running the multi-user workflow never need to install the
//! substrate binary separately.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{Value, json};

use crate::store::{SynthStore, json_out};

pub fn cmd_conflicts() -> Result<()> {
    let mut store = SynthStore::discover()?;
    let claims = store.inner().load_claims()?;

    let mut supers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in &claims {
        if let Some(prior) = &c.supersedes {
            supers
                .entry(prior.clone())
                .or_default()
                .push(c.id.clone());
        }
    }

    let mut conflicts: Vec<Value> = Vec::new();
    for (prior, mut superseders) in supers {
        superseders.sort();
        superseders.dedup();
        if superseders.len() > 1 {
            conflicts.push(json!({
                "prior": prior,
                "superseders": superseders,
            }));
        }
    }

    json_out(&json!({ "conflicts": conflicts }))
}
