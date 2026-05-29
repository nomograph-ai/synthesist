//! End-to-end test for the v3 storage layer.
//!
//! Exercises the write path (LogWriter) and the read path (LogReader)
//! against a fresh claims directory, verifying:
//!
//! - 200 claims write cleanly across 5 asserters.
//! - LogReader yields all 200 with no duplicates and no extras.
//! - File layout matches the spec: one log.jsonl per asserter
//!   directory, asserter dir names match the colon-to-hyphen
//!   convention.
//! - Re-opening the reader returns the same claims (storage is
//!   durable across reader instances).
//! - The genesis file is yielded first when present.
//! - The substrate-internal `_view` / `_telemetry` directories do
//!   not appear in the claim stream.

use std::fs;
use std::io::Write;
use std::path::Path;

use nomograph_claim::{
    log::{LogReader, LogWriter},
    ontology::serialize_ontology,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn make_claim(module: &str, id_suffix: &str, asserter_iri: &str) -> Value {
    json!({
        "@context": "https://nomograph.org/v3/context.jsonld",
        "@id": format!("{}:claim/{}", module, id_suffix),
        "@type": format!("{}:Task", module),
        "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
        "prov:wasAttributedTo": asserter_iri,
        "synthesist:summary": format!("Claim {}", id_suffix),
    })
}

fn write_genesis(claims_dir: &Path) {
    let doc = json!({
        "@context": "https://nomograph.org/v3/context.jsonld",
        "@id": "nomograph:claim/genesis",
        "@type": "nomograph:Genesis",
        "prov:generatedAtTime": "2026-01-01T00:00:00.000Z",
        "prov:wasAttributedTo": "asserter:bootstrap"
    });
    let mut file = fs::File::create(claims_dir.join("genesis.jsonld")).unwrap();
    file.write_all(serde_json::to_string(&doc).unwrap().as_bytes())
        .unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_all().unwrap();
}

#[test]
fn write_200_claims_across_5_asserters_then_read_back() {
    let tmp = TempDir::new().unwrap();
    let claims_dir = tmp.path();
    let writer = LogWriter::new(claims_dir).unwrap();

    let asserters = [
        "user:local:agd",
        "user:local:jkolb",
        "agent:claude-opus-4-7:sess1",
        "ingest:gitlab:nomograph-keaton",
        "user:local:agd:edc-bootstrap",
    ];

    // Write 40 claims per asserter, 200 total.
    let mut expected_ids: Vec<String> = Vec::new();
    for (a_idx, asserter) in asserters.iter().enumerate() {
        let asserter_iri = format!("asserter:{}", asserter);
        for i in 0..40 {
            let suffix = format!("{}_{:02}", a_idx, i);
            let doc = make_claim("synth", &suffix, &asserter_iri);
            let id = writer.append(asserter, &doc).unwrap();
            expected_ids.push(id.as_str().to_string());
        }
    }
    assert_eq!(expected_ids.len(), 200);

    // Read back.
    let reader = LogReader::new(claims_dir).unwrap();
    let mut observed_ids: Vec<String> = Vec::new();
    for item in reader.iter_claims() {
        let claim = item.unwrap();
        observed_ids.push(claim.id.as_str().to_string());
    }

    assert_eq!(observed_ids.len(), 200);

    // Sorted equality: every expected id appears exactly once.
    let mut e = expected_ids.clone();
    let mut o = observed_ids.clone();
    e.sort();
    o.sort();
    assert_eq!(e, o);

    // Verify file layout: one log.jsonl per asserter dir, asserter dir
    // names are the colon-to-hyphen form.
    for asserter in &asserters {
        let dir_name = asserter.replace(':', "-");
        let log_path = claims_dir.join(&dir_name).join("log.jsonl");
        assert!(
            log_path.exists(),
            "log.jsonl missing for {}",
            asserter
        );
        let line_count = fs::read_to_string(&log_path)
            .unwrap()
            .lines()
            .count();
        assert_eq!(line_count, 40, "wrong line count for {}", asserter);
    }
}

#[test]
fn genesis_is_yielded_first() {
    let tmp = TempDir::new().unwrap();
    let claims_dir = tmp.path();
    write_genesis(claims_dir);

    let writer = LogWriter::new(claims_dir).unwrap();
    for i in 0..5 {
        let doc = make_claim("synth", &format!("z{}", i), "asserter:user:local:zulu");
        writer.append("user:local:zulu", &doc).unwrap();
    }

    let reader = LogReader::new(claims_dir).unwrap();
    let first = reader.iter_claims().next().unwrap().unwrap();
    assert_eq!(first.id.as_str(), "nomograph:claim/genesis");
}

#[test]
fn substrate_internal_dirs_are_skipped() {
    let tmp = TempDir::new().unwrap();
    let claims_dir = tmp.path();
    let writer = LogWriter::new(claims_dir).unwrap();

    // Plant substrate-internal directories with bogus content.
    fs::create_dir_all(claims_dir.join("_view.oxigraph")).unwrap();
    fs::write(claims_dir.join("_view.oxigraph/log.jsonl"), "garbage\n").unwrap();

    fs::create_dir_all(claims_dir.join("_telemetry")).unwrap();
    fs::write(claims_dir.join("_telemetry/log.jsonl"), "also garbage\n").unwrap();

    // Real claim.
    let doc = make_claim("synth", "x", "asserter:user:local:agd");
    writer.append("user:local:agd", &doc).unwrap();

    let reader = LogReader::new(claims_dir).unwrap();
    let count = reader.iter_claims().filter(|r| r.is_ok()).count();
    assert_eq!(count, 1, "_view.oxigraph and _telemetry should not appear");
}

#[test]
fn read_after_reopen_returns_same_claims() {
    let tmp = TempDir::new().unwrap();
    let claims_dir = tmp.path();

    {
        let writer = LogWriter::new(claims_dir).unwrap();
        for i in 0..10 {
            let doc = make_claim("synth", &format!("p{}", i), "asserter:user:local:agd");
            writer.append("user:local:agd", &doc).unwrap();
        }
    }

    let r1: Vec<String> = LogReader::new(claims_dir)
        .unwrap()
        .iter_claims()
        .filter_map(|r| r.ok())
        .map(|c| c.id.as_str().to_string())
        .collect();
    let r2: Vec<String> = LogReader::new(claims_dir)
        .unwrap()
        .iter_claims()
        .filter_map(|r| r.ok())
        .map(|c| c.id.as_str().to_string())
        .collect();

    assert_eq!(r1, r2);
    assert_eq!(r1.len(), 10);
}

#[test]
fn ontology_serialization_lands_in_view_friendly_location() {
    let tmp = TempDir::new().unwrap();
    let claims_dir = tmp.path();
    let schema_dir = claims_dir.join("_schema");
    serialize_ontology(&schema_dir).unwrap();

    assert!(schema_dir.join("base.ttl").exists());
    assert!(schema_dir.join("base.shacl.ttl").exists());

    // The _schema dir is gitignored convention (starts with _), so the
    // LogReader should skip it like the other substrate-internal dirs.
    let writer = LogWriter::new(claims_dir).unwrap();
    let doc = make_claim("synth", "x", "asserter:user:local:agd");
    writer.append("user:local:agd", &doc).unwrap();

    let reader = LogReader::new(claims_dir).unwrap();
    let count = reader.iter_claims().filter(|r| r.is_ok()).count();
    assert_eq!(count, 1);
}
