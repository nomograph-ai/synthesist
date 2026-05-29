//! One-shot migration: v2 Automerge claim log to v3 JSON-LD per-asserter
//! logs.
//!
//! Reads every claim from a v2 `claims/changes/*.amc` tree (or
//! genesis-only repo) and writes equivalent JSON-LD documents to
//! `claims_v3/<asserter-dir>/log.jsonl`. Original timestamps are
//! preserved as `prov:generatedAtTime`. Supersession edges become
//! `synth:supersedes` IRIs pointing at the prior claim's v3 IRI.
//!
//! ## Usage
//!
//! ```bash
//! migrate-v2-to-v3 --from <v2-claims-dir> --to <v3-claims-dir> [--dry-run]
//! ```
//!
//! ## Mapping
//!
//! - `claim.id` -> `@id: "<module>:claim/<id>"` where `<module>` is
//!   `synth` for synth-owned types and `lat` for lattice-owned. The
//!   id is the v2 hash truncated to 16 hex chars.
//! - `claim.claim_type` -> `@type: "<module>:<TitleCased>"`.
//! - `claim.asserted_at` -> `prov:generatedAtTime` (RFC 3339, ms precision).
//! - `claim.asserted_by` -> `prov:wasAttributedTo: "asserter:<class>:<scope>:<id>[:<session>]"`.
//! - `claim.supersedes` -> `<module>:supersedes: "<module>:claim/<id>"`.
//! - `claim.parent_asserter` -> `nomograph:parentAsserter`.
//! - `claim.props` -> expanded as `<module>:<key>` predicates.

use std::path::PathBuf;
use std::process;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use nomograph_claim::asserter;
use nomograph_claim::claim::{Claim, ClaimType};
use nomograph_claim::jsonld;
use nomograph_claim::log::LogWriter;
use nomograph_claim::store::Store;
use serde_json::{Map, Value};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut from: Option<PathBuf> = None;
    let mut to: Option<PathBuf> = None;
    let mut dry_run = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--from" => {
                i += 1;
                from = Some(PathBuf::from(&args[i]));
            }
            "--to" => {
                i += 1;
                to = Some(PathBuf::from(&args[i]));
            }
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                print_help();
                return;
            }
            other => {
                eprintln!("unknown argument: {}", other);
                print_help();
                process::exit(2);
            }
        }
        i += 1;
    }

    let from = match from {
        Some(p) => p,
        None => {
            eprintln!("--from <v2-claims-dir> is required");
            process::exit(2);
        }
    };
    let to = match to {
        Some(p) => p,
        None => {
            eprintln!("--to <v3-claims-dir> is required");
            process::exit(2);
        }
    };

    match run(&from, &to, dry_run) {
        Ok(stats) => {
            println!("{}", serde_json::to_string_pretty(&stats).unwrap());
        }
        Err(e) => {
            eprintln!("migration failed: {:#}", e);
            process::exit(1);
        }
    }
}

fn print_help() {
    eprintln!(
        "migrate-v2-to-v3 -- one-shot conversion of v2 .amc claims to v3 JSON-LD logs

Usage:
  migrate-v2-to-v3 --from <v2-claims-dir> --to <v3-claims-dir> [--dry-run]

Options:
  --from <dir>   Source directory containing v2 claims/ (with genesis.amc and changes/).
  --to <dir>     Target directory for v3 per-asserter logs.
  --dry-run      Read and validate without writing.
  -h, --help     Print this help.
"
    );
}

fn run(from: &std::path::Path, to: &std::path::Path, dry_run: bool) -> Result<MigrationStats> {
    let mut store = Store::open(from)
        .with_context(|| format!("open v2 store at {}", from.display()))?;
    let claims = store
        .load_claims()
        .with_context(|| format!("load claims from {}", from.display()))?;

    let mut stats = MigrationStats {
        source: from.display().to_string(),
        target: to.display().to_string(),
        dry_run,
        total_claims: claims.len(),
        translated: 0,
        skipped: 0,
        per_type: Map::new(),
        per_asserter: Map::new(),
    };

    let writer = if dry_run {
        None
    } else {
        std::fs::create_dir_all(to)
            .with_context(|| format!("create target dir {}", to.display()))?;
        Some(LogWriter::new(to).context("create v3 log writer")?)
    };

    for claim in &claims {
        let doc = match v2_to_v3(claim) {
            Ok(d) => d,
            Err(_) => {
                stats.skipped += 1;
                continue;
            }
        };

        let asserter = claim.asserted_by.as_str();
        if asserter::parse(asserter).is_err() {
            stats.skipped += 1;
            continue;
        }

        if let Some(w) = &writer {
            w.append(asserter, &doc)
                .with_context(|| format!("append v3 claim {}", claim.id))?;
        }
        stats.translated += 1;

        let ty_name = claim.claim_type.as_str();
        bump_count(&mut stats.per_type, ty_name);
        bump_count(&mut stats.per_asserter, asserter);
    }

    Ok(stats)
}

/// Translate one v2 claim into a v3 JSON-LD document.
///
/// Module routing: synth-owned types go into `synth:`; lattice-owned
/// types (stakeholder, topic, signal, disposition, intent, heartbeat,
/// directive) go into `lat:`. synth:Outcome stays in synth (it's a
/// synthesist-specific claim type per the v2.4 spec).
pub fn v2_to_v3(claim: &Claim) -> Result<Value> {
    let module = module_for_type(&claim.claim_type);
    let type_camel = camel_case(claim.claim_type.as_str());

    let id_short = &claim.id[..claim.id.len().min(16)];

    let mut doc = Map::new();
    doc.insert(
        "@context".into(),
        Value::String(jsonld::BASE_CONTEXT_URI.to_string()),
    );
    doc.insert("@id".into(), Value::String(format!("{}:claim/{}", module, id_short)));
    doc.insert("@type".into(), Value::String(format!("{}:{}", module, type_camel)));
    doc.insert(
        "prov:generatedAtTime".into(),
        Value::String(format_iso(claim.asserted_at)),
    );
    doc.insert(
        "prov:wasAttributedTo".into(),
        Value::String(format!("asserter:{}", claim.asserted_by)),
    );

    if let Some(sup) = &claim.supersedes {
        let sup_short = &sup[..sup.len().min(16)];
        doc.insert(
            format!("{}:supersedes", module),
            Value::String(format!("{}:claim/{}", module, sup_short)),
        );
    }
    if let Some(parent) = &claim.parent_asserter {
        doc.insert(
            "nomograph:parentAsserter".into(),
            Value::String(format!("asserter:{}", parent)),
        );
    }

    // Expand props as <module>:<key> predicates.
    if let Some(props_map) = claim.props.as_object() {
        for (k, v) in props_map {
            doc.insert(format!("{}:{}", module, k), v.clone());
        }
    }

    Ok(Value::Object(doc))
}

fn module_for_type(t: &ClaimType) -> &'static str {
    match t {
        ClaimType::Stakeholder
        | ClaimType::Topic
        | ClaimType::Signal
        | ClaimType::Disposition
        | ClaimType::Intent
        | ClaimType::Heartbeat
        | ClaimType::Directive => "lat",
        // Outcome and the rest stay with synth in this migration. If a
        // lattice-side migration is run later for lat: claims that
        // somehow ended up in a synth store, the operator runs it
        // explicitly with a flag.
        _ => "synth",
    }
}

fn camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn format_iso(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

fn bump_count(map: &mut Map<String, Value>, key: &str) {
    let entry = map
        .entry(key.to_string())
        .or_insert_with(|| Value::from(0u64));
    let cur = entry.as_u64().unwrap_or(0);
    *entry = Value::from(cur + 1);
}

#[derive(serde::Serialize, Debug)]
struct MigrationStats {
    source: String,
    target: String,
    dry_run: bool,
    total_claims: usize,
    translated: usize,
    skipped: usize,
    per_type: Map<String, Value>,
    per_asserter: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn fake_claim(ty: ClaimType, id: &str, props: Value) -> Claim {
        Claim {
            id: id.to_string(),
            claim_type: ty,
            props,
            valid_from: Utc.with_ymd_and_hms(2026, 5, 5, 17, 43, 11).unwrap(),
            valid_until: None,
            supersedes: None,
            parent_asserter: None,
            asserted_by: "user:local:agd".to_string(),
            asserted_at: Utc.with_ymd_and_hms(2026, 5, 5, 17, 43, 11).unwrap(),
        }
    }

    #[test]
    fn synth_task_translates_to_synth_namespace() {
        let claim = fake_claim(
            ClaimType::Task,
            "abc123def456fed789",
            json!({"summary": "Test", "status": "pending"}),
        );
        let v3 = v2_to_v3(&claim).unwrap();
        assert_eq!(v3["@id"], Value::String("synth:claim/abc123def456fed7".into()));
        assert_eq!(v3["@type"], Value::String("synth:Task".into()));
        assert_eq!(
            v3["prov:wasAttributedTo"],
            Value::String("asserter:user:local:agd".into())
        );
        assert_eq!(v3["synth:summary"], Value::String("Test".into()));
        assert_eq!(v3["synth:status"], Value::String("pending".into()));
    }

    #[test]
    fn lattice_disposition_translates_to_lat_namespace() {
        let claim = fake_claim(
            ClaimType::Disposition,
            "deadbeef00000000",
            json!({"topic": "X", "stance": "opposed"}),
        );
        let v3 = v2_to_v3(&claim).unwrap();
        assert_eq!(v3["@id"], Value::String("lat:claim/deadbeef00000000".into()));
        assert_eq!(v3["@type"], Value::String("lat:Disposition".into()));
        assert_eq!(v3["lat:topic"], Value::String("X".into()));
    }

    #[test]
    fn supersedes_edge_lands_in_module_namespace() {
        let mut claim = fake_claim(
            ClaimType::Task,
            "aaaa111111111111",
            json!({"status": "done"}),
        );
        claim.supersedes = Some("bbbb222222222222".to_string());
        let v3 = v2_to_v3(&claim).unwrap();
        assert_eq!(
            v3["synth:supersedes"],
            Value::String("synth:claim/bbbb222222222222".into())
        );
    }

    #[test]
    fn parent_asserter_lands_in_nomograph_namespace() {
        let mut claim = fake_claim(
            ClaimType::Task,
            "cccc333333333333",
            json!({"status": "pending"}),
        );
        claim.parent_asserter = Some("user:local:agd".to_string());
        let v3 = v2_to_v3(&claim).unwrap();
        assert_eq!(
            v3["nomograph:parentAsserter"],
            Value::String("asserter:user:local:agd".into())
        );
    }

    #[test]
    fn camel_case_handles_simple_lowercase() {
        assert_eq!(camel_case("task"), "Task");
        assert_eq!(camel_case("spec"), "Spec");
        assert_eq!(camel_case("disposition"), "Disposition");
    }

    #[test]
    fn camel_case_handles_underscore_separation() {
        assert_eq!(camel_case("agent_intent"), "AgentIntent");
        assert_eq!(camel_case("agree_snapshot"), "AgreeSnapshot");
    }

    #[test]
    fn module_for_type_routes_correctly() {
        assert_eq!(module_for_type(&ClaimType::Task), "synth");
        assert_eq!(module_for_type(&ClaimType::Spec), "synth");
        assert_eq!(module_for_type(&ClaimType::Outcome), "synth");
        assert_eq!(module_for_type(&ClaimType::Disposition), "lat");
        assert_eq!(module_for_type(&ClaimType::Stakeholder), "lat");
    }

    #[test]
    fn format_iso_produces_ms_precision_with_z() {
        let t = Utc.with_ymd_and_hms(2026, 5, 29, 4, 30, 0).unwrap();
        let s = format_iso(t);
        assert!(s.starts_with("2026-05-29T04:30:00.000"));
        assert!(s.ends_with('Z'));
    }
}
