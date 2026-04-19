//! Export command — dumps the claim substrate to JSON.
//!
//! v2 (claim-based): every row in the `claims` view is exported. The
//! output has two kinds of keys:
//!
//!   - `claims_raw`: the full row shape (id, claim_type, props,
//!     valid_from, valid_until, supersedes, asserted_by, asserted_at)
//!     for every claim, in asserted_at order. This is the canonical
//!     form — `cmd_import` replays it for perfect round-trip.
//!
//!   - Typed keys (`trees`, `specs`, `tasks`, `discoveries`, ...):
//!     claims grouped by `claim_type`, carrying just the `props`
//!     payload. Human-readable; downstream tools can read without
//!     understanding supersession.
//!
//! The envelope also has `version` and `exported` (today's date).

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};

// Note: `result` below uses `serde_json::Map<String, Value>` for the
// outer envelope (fixed schema). Typed groupings are held in a plain
// `BTreeMap<String, Vec<Value>>` so we can push into the Vecs.

use crate::store::{json_out, SynthStore};

pub fn cmd_export() -> Result<()> {
    let store = SynthStore::discover()?;
    let rows = store.query(
        "SELECT id, claim_type, props, valid_from, valid_until, \
                supersedes, asserted_by, asserted_at \
         FROM claims \
         ORDER BY asserted_at",
        &[],
    )?;

    let mut result: Map<String, Value> = Map::new();
    result.insert("version".into(), json!("2"));
    result.insert("exported".into(), json!(SynthStore::today()));

    // Typed groupings (props only, for human readability). Plain
    // HashMap rather than serde_json::Map so we can push into Vecs —
    // serde_json::Map only maps String to Value.
    let mut grouped: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();
    // Canonical raw rows (for perfect round-trip).
    let mut claims_raw: Vec<Value> = Vec::with_capacity(rows.len());

    for row in rows {
        let obj = row
            .as_object()
            .context("expected object row from claims query")?;

        let claim_type = obj
            .get("claim_type")
            .and_then(Value::as_str)
            .context("row missing claim_type")?
            .to_string();

        // Parse props (stored as TEXT / JSON string) into a real Value
        // so the export is a pretty-printable object, not a quoted blob.
        let props_parsed = match obj.get("props") {
            Some(Value::String(s)) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
            Some(other) => other.clone(),
            None => Value::Null,
        };

        // Raw row: re-emit the SQL columns, with props as parsed JSON.
        let mut raw = obj.clone();
        raw.insert("props".into(), props_parsed.clone());
        claims_raw.push(Value::Object(raw));

        // Typed row: just props, grouped by claim_type.
        let bucket = plural_for(&claim_type);
        grouped.entry(bucket).or_default().push(props_parsed);
    }

    for (bucket, values) in grouped {
        result.insert(bucket, Value::Array(values));
    }
    result.insert("claims_raw".into(), Value::Array(claims_raw));

    json_out(&Value::Object(result))
}

/// Return the plural bucket name for a claim type (e.g. "task" → "tasks").
fn plural_for(claim_type: &str) -> String {
    match claim_type {
        // Claim types whose bucket is just `{type}s`. Kept explicit so a
        // new claim type doesn't silently collide with a SQL column name.
        "tree" => "trees".into(),
        "spec" => "specs".into(),
        "task" => "tasks".into(),
        "discovery" => "discoveries".into(),
        "campaign" => "campaigns".into(),
        "session" => "sessions".into(),
        "phase" => "phases".into(),
        "intent" => "intents".into(),
        "heartbeat" => "heartbeats".into(),
        "outcome" => "outcomes".into(),
        "directive" => "directives".into(),
        "stakeholder" => "stakeholders".into(),
        "topic" => "topics".into(),
        "signal" => "signals".into(),
        "disposition" => "dispositions".into(),
        other => format!("{other}s"),
    }
}
