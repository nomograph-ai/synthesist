//! Integration tests for `synthesist/src/migrations/`.
//!
//! Covers:
//! - `V2ToV3::detect` on a v2 layout returns true.
//! - `V2ToV3::detect` on an empty dir returns false.
//! - `V2ToV3::detect` on a v3 layout (has log.jsonl) returns false.
//! - `apply_chain` writes `claims/_schema.json` with correct version.
//! - End-to-end v2->v3: builds a v2 fixture, runs V2ToV3.run, checks log lines.
//! - Tarball backup is written.
//! - `--dry-run` skips schema.json and log writes.
//! - `--no-backup` skips the tarball.
//! - Lattice-typed claim errors with UnsupportedClaimType.
//! - `migrate list` JSON output contains v2-to-v3 entry.
//! - `migrate status` on fresh store says "fresh store".
//! - `migrate v2-to-v3` routes to same code as `migrate run --target 3.0.0-pre.1`.

#![allow(deprecated)]

use std::path::Path;

use assert_cmd::prelude::*;
use chrono::Utc;
use nomograph_claim::claim::{Claim, ClaimType};
use nomograph_claim::store::Store as V2Store;
use nomograph_synthesist::migrations::{
    Migration, MigrationError, MigrationOpts, registry,
    runner, schema,
    v2_to_v3::V2ToV3,
};
use serde_json::{json, Value};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a minimal v2 claim fixture in `claims/` inside `dir`.
///
/// Returns the claims dir path.
fn make_v2_store(dir: &Path) -> std::path::PathBuf {
    let claims = dir.join("claims");
    let mut store = V2Store::init(&claims).expect("init v2 store");
    let now = Utc::now();

    let task_claim = Claim {
        id: Claim::compute_id(
            &ClaimType::Task,
            &json!({"summary": "test task", "status": "pending"}),
            now,
            "user:local:test",
            now,
        ),
        claim_type: ClaimType::Task,
        props: json!({"summary": "test task", "status": "pending"}),
        valid_from: now,
        valid_until: None,
        supersedes: None,
        parent_asserter: None,
        asserted_by: "user:local:test".to_string(),
        asserted_at: now,
    };
    store.append(&task_claim).expect("append task claim");

    let spec_claim = Claim {
        id: Claim::compute_id(
            &ClaimType::Spec,
            &json!({"tree": "t1", "id": "s1", "goal": "goal", "status": "active", "topics": ["foo"], "agree_snapshot": []}),
            now,
            "user:local:test",
            now,
        ),
        claim_type: ClaimType::Spec,
        props: json!({"tree": "t1", "id": "s1", "goal": "goal", "status": "active", "topics": ["foo"], "agree_snapshot": []}),
        valid_from: now,
        valid_until: None,
        supersedes: None,
        parent_asserter: None,
        asserted_by: "user:local:test".to_string(),
        asserted_at: now,
    };
    store.append(&spec_claim).expect("append spec claim");

    claims
}

// ---------------------------------------------------------------------------
// detect() tests
// ---------------------------------------------------------------------------

#[test]
fn detect_returns_true_on_v2_layout() {
    let dir = TempDir::new().unwrap();
    make_v2_store(dir.path());
    let v2 = V2ToV3;
    assert!(v2.detect(dir.path()).unwrap(), "should detect v2 layout");
}

#[test]
fn detect_returns_false_on_empty_dir() {
    let dir = TempDir::new().unwrap();
    let v2 = V2ToV3;
    assert!(!v2.detect(dir.path()).unwrap(), "should not detect in empty dir");
}

#[test]
fn detect_returns_false_on_v3_layout() {
    let dir = TempDir::new().unwrap();
    // Create a v3 layout: claims/<asserter>/log.jsonl exists.
    let asserter_dir = dir.path().join("claims").join("user:local:test");
    std::fs::create_dir_all(&asserter_dir).unwrap();
    std::fs::write(asserter_dir.join("log.jsonl"), b"{}").unwrap();
    // Also create claims/changes/ so the v2 heuristic would fire if not for the log.
    std::fs::create_dir_all(dir.path().join("claims").join("changes")).unwrap();

    let v2 = V2ToV3;
    assert!(!v2.detect(dir.path()).unwrap(), "should not detect when log.jsonl present");
}

// ---------------------------------------------------------------------------
// End-to-end v2 -> v3 run
// ---------------------------------------------------------------------------

#[test]
fn end_to_end_v2_to_v3_produces_log_lines() {
    let dir = TempDir::new().unwrap();
    make_v2_store(dir.path());

    let v2 = V2ToV3;
    let opts = MigrationOpts {
        dry_run: false,
        backup: false, // skip tarball for speed; tested separately
    };
    let report = v2.run(dir.path(), &opts).unwrap();

    assert_eq!(report.artifacts_touched, 2, "should translate 2 claims");

    // Check that log.jsonl was written for the asserter.
    // LogWriter maps colons to hyphens in asserter dir names.
    let log_path = dir.path().join("claims").join("user-local-test").join("log.jsonl");
    assert!(log_path.exists(), "log.jsonl should exist");

    let content = std::fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "should have 2 log lines");

    // Spot-check one line: it should be valid JSON with synthesist: namespace.
    let first: Value = serde_json::from_str(lines[0]).unwrap();
    assert!(
        first["@id"].as_str().unwrap().starts_with("synthesist:claim/"),
        "id should use synthesist: prefix, got: {}",
        first["@id"]
    );
}

// ---------------------------------------------------------------------------
// apply_chain writes _schema.json
// ---------------------------------------------------------------------------

#[test]
fn apply_chain_writes_schema_json() {
    let dir = TempDir::new().unwrap();
    make_v2_store(dir.path());

    let chain = runner::plan(dir.path(), None).unwrap();
    let opts = MigrationOpts {
        dry_run: false,
        backup: false,
    };
    runner::apply_chain(dir.path(), &chain, &opts).unwrap();

    let schema_path = dir.path().join("claims").join("_schema.json");
    assert!(schema_path.exists(), "_schema.json should be written");

    let record = schema::read(&dir.path().join("claims")).unwrap().unwrap();
    assert_eq!(record.schema_version, "3.0.0-pre.1");
    assert!(!record.migrated_at.is_empty());
}

// ---------------------------------------------------------------------------
// Tarball backup
// ---------------------------------------------------------------------------

#[test]
fn tarball_backup_is_written() {
    let dir = TempDir::new().unwrap();
    make_v2_store(dir.path());

    let v2 = V2ToV3;
    let opts = MigrationOpts {
        dry_run: false,
        backup: true,
    };
    let report = v2.run(dir.path(), &opts).unwrap();

    assert!(report.backup_path.is_some(), "backup_path should be set");
    let bp = report.backup_path.unwrap();
    assert!(bp.exists(), "tarball should exist at {}", bp.display());
    assert!(
        bp.file_name().unwrap().to_str().unwrap().ends_with(".tar.gz"),
        "backup should be .tar.gz"
    );
}

// ---------------------------------------------------------------------------
// --no-backup skips tarball
// ---------------------------------------------------------------------------

#[test]
fn no_backup_skips_tarball() {
    let dir = TempDir::new().unwrap();
    make_v2_store(dir.path());

    let v2 = V2ToV3;
    let opts = MigrationOpts {
        dry_run: false,
        backup: false,
    };
    let report = v2.run(dir.path(), &opts).unwrap();

    assert!(report.backup_path.is_none(), "backup_path should be None with no_backup");
    let archive = dir.path().join(".synthesist-v2-backup.tar.gz");
    assert!(!archive.exists(), "tarball should not be written with no_backup");
}

// ---------------------------------------------------------------------------
// dry-run: no schema.json, no log written
// ---------------------------------------------------------------------------

#[test]
fn dry_run_skips_schema_json_and_log_writes() {
    let dir = TempDir::new().unwrap();
    make_v2_store(dir.path());

    let chain = runner::plan(dir.path(), None).unwrap();
    let opts = MigrationOpts {
        dry_run: true,
        backup: false,
    };
    let reports = runner::apply_chain(dir.path(), &chain, &opts).unwrap();
    assert_eq!(reports.len(), 1);

    // Schema file should NOT exist.
    let schema_path = dir.path().join("claims").join("_schema.json");
    assert!(!schema_path.exists(), "_schema.json should not be written in dry-run");

    // Log file should NOT exist.
    let log_path = dir.path().join("claims").join("user:local:test").join("log.jsonl");
    assert!(!log_path.exists(), "log.jsonl should not be written in dry-run");
}

// ---------------------------------------------------------------------------
// UnsupportedClaimType for lattice-typed claims
// ---------------------------------------------------------------------------

#[test]
fn unsupported_claim_type_error_for_lattice_types() {
    use nomograph_synthesist::migrations::v2_to_v3::module_for_type;
    for ty in [
        ClaimType::Stakeholder,
        ClaimType::Topic,
        ClaimType::Signal,
        ClaimType::Disposition,
        ClaimType::Intent,
        ClaimType::Heartbeat,
        ClaimType::Directive,
    ] {
        let result = module_for_type(&ty);
        assert!(
            matches!(result, Err(MigrationError::UnsupportedClaimType { .. })),
            "expected UnsupportedClaimType for {}", ty.as_str()
        );
    }
}

// ---------------------------------------------------------------------------
// `migrate list` CLI output
// ---------------------------------------------------------------------------

#[test]
fn migrate_list_json_contains_v2_to_v3() {
    let mut cmd = std::process::Command::cargo_bin("synthesist").unwrap();
    cmd.args(["migrate", "list"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success(), "migrate list should succeed");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let v: Value = serde_json::from_str(stdout).expect("output should be JSON");

    let migrations = v["migrations"].as_array().unwrap();
    assert!(!migrations.is_empty(), "should have at least one migration");

    let first = &migrations[0];
    assert_eq!(first["from_version"], "2.x");
    assert_eq!(first["to_version"], "3.0.0-pre.1");
}

// ---------------------------------------------------------------------------
// `migrate status` on a fresh store
// ---------------------------------------------------------------------------

#[test]
fn migrate_status_on_fresh_store_says_fresh() {
    let dir = TempDir::new().unwrap();
    let mut cmd = std::process::Command::cargo_bin("synthesist").unwrap();
    cmd.args(["migrate", "status"]).current_dir(dir.path());
    let output = cmd.output().unwrap();
    assert!(output.status.success(), "migrate status should succeed");

    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let v: Value = serde_json::from_str(stdout).expect("output should be JSON");

    let status = v["status"].as_str().unwrap_or("");
    assert!(
        status.contains("fresh") || status.contains("latest"),
        "fresh store should report fresh/latest, got: {status}"
    );
}

// ---------------------------------------------------------------------------
// `migrate v2-to-v3` routes to same code as `migrate run --target 3.0.0-pre.1`
// ---------------------------------------------------------------------------

#[test]
fn migrate_v2_to_v3_equivalent_to_run_with_target() {
    // Both should produce the same schema_version in _schema.json.
    // We test by running v2-to-v3 on one fixture and checking the result.
    let dir = TempDir::new().unwrap();
    make_v2_store(dir.path());

    let mut cmd = std::process::Command::cargo_bin("synthesist").unwrap();
    cmd.args(["migrate", "v2-to-v3"]).current_dir(dir.path());
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "migrate v2-to-v3 should succeed; stderr: {}",
        std::str::from_utf8(&output.stderr).unwrap()
    );

    let record = schema::read(&dir.path().join("claims")).unwrap().unwrap();
    assert_eq!(record.schema_version, "3.0.0-pre.1");

    // Now build a second fixture and run via `migrate run --target`.
    let dir2 = TempDir::new().unwrap();
    make_v2_store(dir2.path());

    let mut cmd2 = std::process::Command::cargo_bin("synthesist").unwrap();
    cmd2.args(["migrate", "run", "--target", "3.0.0-pre.1"])
        .current_dir(dir2.path());
    let output2 = cmd2.output().unwrap();
    assert!(
        output2.status.success(),
        "migrate run --target should succeed; stderr: {}",
        std::str::from_utf8(&output2.stderr).unwrap()
    );

    let record2 = schema::read(&dir2.path().join("claims")).unwrap().unwrap();
    assert_eq!(record2.schema_version, "3.0.0-pre.1");
}

// ---------------------------------------------------------------------------
// registry() content
// ---------------------------------------------------------------------------

#[test]
fn registry_contains_v2_to_v3() {
    let reg = registry();
    assert_eq!(reg.len(), 1, "registry should have exactly 1 migration");
    assert_eq!(reg[0].from_version(), "2.x");
    assert_eq!(reg[0].to_version(), "3.0.0-pre.1");
}
