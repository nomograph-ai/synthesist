//! Import command — replays a `cmd_export` dump into the claim substrate.
//!
//! Accepts the v2 export shape written by [`cmd_export::cmd_export`]:
//!
//!   - If `claims_raw` is present, each raw row is reconstructed into a
//!     [`Claim`] with its original id, timestamps, supersedes chain,
//!     and asserter, then appended via the underlying
//!     [`ClaimStore::append`]. This preserves supersession relationships
//!     and provides a perfect round-trip.
//!
//!   - If only the typed buckets (`trees`, `specs`, `tasks`, ...) are
//!     present (a human-edited import, perhaps), each props payload is
//!     wrapped in a fresh [`Claim`] via [`SynthStore::append`] with
//!     today's asserter. Supersession is lost; the caller gets a
//!     flattened log.
//!
//! Any row whose shape doesn't match (missing required columns, bad
//! timestamp, unknown claim_type) is skipped and counted, never aborting
//! the batch. We print `{imported: N, skipped: M}` at the end.

use std::io::Read;

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use nomograph_claim::{Claim, ClaimType};
use serde_json::{json, Value};

use crate::store::{json_out, SynthStore};

pub fn cmd_import(file: &Option<String>) -> Result<()> {
    let json_str = match file {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("read import file {path}"))?,
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("read stdin")?;
            buf
        }
    };

    let data: Value = serde_json::from_str(&json_str).context("parse import JSON")?;
    let obj = data
        .as_object()
        .context("expected top-level JSON object")?;

    let mut store = SynthStore::discover()?;
    let mut imported: usize = 0;
    let mut skipped: usize = 0;

    if let Some(raw) = obj.get("claims_raw").and_then(Value::as_array) {
        // Canonical path: replay full rows, preserving id + timestamps.
        for row in raw {
            match build_claim_from_raw(row) {
                Some(claim) => {
                    // Bypass SynthStore::append (which mints a new id) and
                    // push the original claim bytes into the log directly.
                    if store.inner().append(&claim).is_ok() {
                        imported += 1;
                    } else {
                        skipped += 1;
                    }
                }
                None => skipped += 1,
            }
        }
        // One view rebuild at the end beats a sync per claim on big imports.
        store.sync_view().context("sync view after raw import")?;
    } else {
        // Typed path: walk each bucket, wrap props in a fresh Claim.
        for (bucket, claim_type) in TYPED_BUCKETS {
            let Some(rows) = obj.get(*bucket).and_then(Value::as_array) else {
                continue;
            };
            for row in rows {
                if store
                    .append(claim_type.clone(), row.clone(), None)
                    .is_ok()
                {
                    imported += 1;
                } else {
                    skipped += 1;
                }
            }
        }
    }

    json_out(&json!({ "imported": imported, "skipped": skipped }))
}

/// Reconstruct a [`Claim`] from an exported raw row. Returns `None` on
/// any validation failure — caller counts it as skipped.
fn build_claim_from_raw(row: &Value) -> Option<Claim> {
    let obj = row.as_object()?;
    let id = obj.get("id")?.as_str()?.to_string();
    let claim_type_str = obj.get("claim_type")?.as_str()?;
    let claim_type: ClaimType =
        serde_json::from_value(Value::String(claim_type_str.to_string())).ok()?;
    // `props` may arrive as a real JSON object (from cmd_export v2) or as
    // a string (legacy / hand-edited). Accept both.
    let props = match obj.get("props")? {
        Value::String(s) => serde_json::from_str::<Value>(s).ok()?,
        other => other.clone(),
    };
    let asserted_by = obj.get("asserted_by")?.as_str()?.to_string();
    let valid_from = obj.get("valid_from").and_then(parse_time)?;
    let asserted_at = obj.get("asserted_at").and_then(parse_time)?;
    let valid_until = obj.get("valid_until").and_then(parse_time);
    let supersedes = obj
        .get("supersedes")
        .and_then(Value::as_str)
        .map(str::to_string);
    let parent_asserter = obj
        .get("parent_asserter")
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(Claim {
        id,
        claim_type,
        props,
        valid_from,
        valid_until,
        supersedes,
        parent_asserter,
        asserted_by,
        asserted_at,
    })
}

/// Accept both RFC3339 strings and millisecond timestamps (the two
/// forms the view emits depending on SQLite column affinity).
fn parse_time(v: &Value) -> Option<DateTime<Utc>> {
    match v {
        Value::Number(n) => {
            let millis = n.as_i64()?;
            Utc.timestamp_millis_opt(millis).single()
        }
        Value::String(s) => {
            let parsed = DateTime::parse_from_rfc3339(s).ok()?;
            Some(parsed.with_timezone(&Utc))
        }
        _ => None,
    }
}

/// Bucket name → ClaimType mapping for the typed import path. Kept
/// explicit so an unknown bucket becomes "unrecognized data" (skipped),
/// not an accidental claim_type miscoercion.
const TYPED_BUCKETS: &[(&str, ClaimType)] = &[
    ("trees", ClaimType::Tree),
    ("specs", ClaimType::Spec),
    ("tasks", ClaimType::Task),
    ("discoveries", ClaimType::Discovery),
    ("campaigns", ClaimType::Campaign),
    ("sessions", ClaimType::Session),
    ("phases", ClaimType::Phase),
    ("intents", ClaimType::Intent),
    ("heartbeats", ClaimType::Heartbeat),
    ("outcomes", ClaimType::Outcome),
    ("directives", ClaimType::Directive),
    ("stakeholders", ClaimType::Stakeholder),
    ("topics", ClaimType::Topic),
    ("signals", ClaimType::Signal),
    ("dispositions", ClaimType::Disposition),
];
