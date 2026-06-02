//! `synthesist conflicts` -- surface diamond conflicts in the claim log.
//!
//! A diamond conflict is a prior claim that has been superseded by more
//! than one distinct live successor. Happens when two peers, working
//! offline, supersede the same prior claim in different ways. CRDT
//! merge delivers both successor edges cleanly; resolution means
//! appending a new claim that supersedes the contested pair.
//!
//! Ported to the gamma typed query surface (C-2). The aggregation that
//! the v2 implementation did in memory and the v3-alpha did in SPARQL
//! (`GROUP BY ?prior HAVING COUNT > 1`, live successors only) is now
//! gamma's H9 `diamond_conflicts`: a Rust group-by over the supersedes
//! edge index keeping only superseders that are themselves live heads.

use anyhow::Result;
use serde_json::{Value, json};

use crate::store::{SynthStore, json_out, short_claim_id};

pub fn cmd_conflicts() -> Result<()> {
    let store = SynthStore::discover()?;

    // H9: priors superseded by more than one distinct LIVE superseder.
    let mut conflicts: Vec<Value> = Vec::new();
    for c in store.diamond_conflicts()? {
        let mut superseders: Vec<String> =
            c.superseders.iter().map(|s| short_claim_id(s)).collect();
        superseders.sort();
        superseders.dedup();
        if superseders.len() > 1 {
            conflicts.push(json!({
                "prior": short_claim_id(&c.prior),
                "superseders": superseders,
            }));
        }
    }

    json_out(&json!({ "conflicts": conflicts }))
}
