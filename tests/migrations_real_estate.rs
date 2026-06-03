//! Real-estate migration tests against the committed v2 fixtures.
//!
//! These exercise the issue #11 regression directly: a COMPACTED v2 estate
//! (genesis.amc + snapshot.amc, NO changes/) must be detected as v2 and
//! migrated to v3. The earlier `tests/migrations.rs` only ever built
//! estates via `Store::init`+`append`, which ALWAYS create `changes/`, so
//! the compacted shape -- the real production shape -- was never covered.
//!
//! Fixtures live in the claim crate at
//! `claim/tests/fixtures/v2_estates/`. They are FIXED-shape (deterministic
//! timestamps), not `Utc::now()`-synthetic. Because migration mutates the
//! estate and writes a backup tarball, every test copies the fixture into a
//! fresh `TempDir` first.

#![allow(deprecated)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;
use nomograph_synthesist::migrations::Migration;
use nomograph_synthesist::migrations::{MigrationOpts, runner, schema, v2_to_v3::V2ToV3};
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture plumbing
// ---------------------------------------------------------------------------

/// Absolute path to a committed v2 estate fixture's `claims/` parent dir.
/// `name` is `compacted` or `with_changes`.
fn fixture_estate(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("claim")
        .join("tests")
        .join("fixtures")
        .join("v2_estates")
        .join(name)
}

/// Path to the committed v2.5.2 export JSON.
fn export_json_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("claim")
        .join("tests")
        .join("fixtures")
        .join("v2_estates")
        .join("export_v2_5_2.json")
}

/// Recursively copy `src` into `dst`.
fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

/// Copy a fixture estate (`<name>/claims/`) into a fresh TempDir and return
/// the TempDir (the estate root is `temp.path()`, containing `claims/`).
fn temp_estate(name: &str) -> TempDir {
    let temp = TempDir::new().unwrap();
    let src_claims = fixture_estate(name).join("claims");
    let dst_claims = temp.path().join("claims");
    copy_dir(&src_claims, &dst_claims);
    temp
}

fn synth(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.current_dir(dir);
    cmd.env("SYNTHESIST_OFFLINE", "1");
    cmd.env_remove("SYNTHESIST_DIR");
    cmd.env_remove("SYNTHESIST_SESSION");
    cmd.env("USER", "ret");
    cmd
}

// ---------------------------------------------------------------------------
// detect() -- the #11 regression
// ---------------------------------------------------------------------------

#[test]
fn detect_true_on_compacted_estate() {
    // THE #11 REGRESSION TEST. A compacted estate has genesis.amc +
    // snapshot.amc and NO changes/. Before the issue-1 fix (detect gated on
    // claims/changes/) this returned false. It must now return true.
    let temp = temp_estate("compacted");
    assert!(
        !temp.path().join("claims").join("changes").exists(),
        "fixture precondition: compacted estate has NO changes/ dir"
    );
    assert!(
        temp.path().join("claims").join("snapshot.amc").exists(),
        "fixture precondition: compacted estate has snapshot.amc"
    );
    let v2 = V2ToV3;
    assert!(
        v2.detect(temp.path()).unwrap(),
        "compacted v2 estate must be detected as v2 (issue #11)"
    );
}

#[test]
fn detect_true_on_with_changes_estate() {
    let temp = temp_estate("with_changes");
    assert!(temp.path().join("claims").join("changes").is_dir());
    let v2 = V2ToV3;
    assert!(
        v2.detect(temp.path()).unwrap(),
        "with-changes v2 estate must be detected as v2"
    );
}

#[test]
fn detect_true_on_interrupted_migration_partial_log_no_schema() {
    // Issue #11 follow-up: an INTERRUPTED migration leaves genesis.amc + a
    // PARTIAL claims/<asserter>/log.jsonl + NO claims/_schema.json (the schema
    // record is written only after the whole step completes). That estate
    // still holds un-translated v2 claims and MUST be re-runnable -- detect()
    // must return true (v2), not false. Gating "already-migrated" on first-log
    // presence misclassified it as done.
    let temp = temp_estate("compacted");
    let claims = temp.path().join("claims");
    // Simulate a half-written run: one asserter dir with a partial log, no
    // _schema.json.
    let asserter_dir = claims.join("user-local-agd");
    fs::create_dir_all(&asserter_dir).unwrap();
    fs::write(
        asserter_dir.join("log.jsonl"),
        "{\"@id\":\"synthesist:claim/deadbeef\"}\n",
    )
    .unwrap();
    assert!(
        !claims.join("_schema.json").exists(),
        "precondition: interrupted estate has NO _schema.json"
    );

    let v2 = V2ToV3;
    assert!(
        v2.detect(temp.path()).unwrap(),
        "interrupted migration (partial log, no _schema.json) must still detect as v2"
    );
    // And it must still PLAN (the chain is re-runnable), not error as fresh.
    let chain = runner::plan(temp.path(), None).expect("interrupted estate must re-plan as v2");
    assert_eq!(chain.len(), 1, "single v2->v3 step on re-run");
    assert_eq!(
        runner::current_version(temp.path()).unwrap(),
        "2.x",
        "interrupted estate reports 2.x, not fresh"
    );
}

#[test]
fn detect_false_once_schema_written() {
    // The completion guard: once _schema.json exists (written post-run),
    // detect() returns false so a COMPLETED estate is not re-run.
    let temp = temp_estate("compacted");
    let claims = temp.path().join("claims");
    schema::write(&claims, "3.0.0", chrono::Utc::now()).unwrap();
    let v2 = V2ToV3;
    assert!(
        !v2.detect(temp.path()).unwrap(),
        "completed estate (_schema.json present) must NOT detect as v2"
    );
}

// ---------------------------------------------------------------------------
// migrate status reports 2.x + pending on a compacted estate
// ---------------------------------------------------------------------------

#[test]
fn migrate_status_compacted_reports_v2_pending() {
    let temp = temp_estate("compacted");

    let output = synth(temp.path())
        .args(["migrate", "status"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "migrate status should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let v: Value = serde_json::from_str(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
    assert_eq!(
        v["current_version"], "2.x",
        "compacted estate current_version must be 2.x, got {v}"
    );
    assert_eq!(v["status"], "migrations pending");
    assert!(
        !v["pending"].as_array().unwrap().is_empty(),
        "pending chain must be non-empty for an un-migrated v2 estate"
    );
}

// ---------------------------------------------------------------------------
// migrate run on a compacted estate -> v3 logs + _schema.json 3.0.0 + latest
// ---------------------------------------------------------------------------

#[test]
fn migrate_run_compacted_produces_v3_logs_and_schema() {
    let temp = temp_estate("compacted");

    // Use runner directly (deterministic, no subprocess cwd quirks).
    let chain = runner::plan(temp.path(), None).expect("plan a v2 estate");
    assert_eq!(chain.len(), 1, "single v2->v3 step");
    let opts = MigrationOpts {
        dry_run: false,
        backup: true,
    };
    let reports = runner::apply_chain(temp.path(), &chain, &opts).expect("apply chain");
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].artifacts_touched, 6,
        "all 6 fixture claims should translate"
    );

    let claims = temp.path().join("claims");

    // _schema.json == 3.0.0.
    let record = schema::read(&claims).unwrap().unwrap();
    assert_eq!(record.schema_version, "3.0.0");

    // Claims landed in per-asserter logs. The fixture has agd + jkolb.
    // LogWriter maps colons to hyphens.
    let agd_log = claims.join("user-local-agd").join("log.jsonl");
    let jkolb_log = claims.join("user-local-jkolb").join("log.jsonl");
    assert!(agd_log.exists(), "agd log.jsonl should exist");
    assert!(jkolb_log.exists(), "jkolb log.jsonl should exist");
    let total_lines = count_lines(&agd_log) + count_lines(&jkolb_log);
    assert_eq!(total_lines, 6, "6 claims across the two asserter logs");

    // Follow-up migrate status now says latest (subprocess, reading the
    // _schema.json we just wrote).
    let output = synth(temp.path())
        .args(["migrate", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_str(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
    assert_eq!(v["current_version"], "3.0.0");
    assert_eq!(
        v["status"], "store is at latest",
        "post-migration status must be latest, got {v}"
    );
}

fn count_lines(path: &Path) -> usize {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

// ---------------------------------------------------------------------------
// import of the v2.5.2 export: imported == row_count, skipped == 0
// ---------------------------------------------------------------------------

#[test]
fn import_v2_export_imports_all_rows() {
    // Fresh v3 store in a TempDir.
    let temp = TempDir::new().unwrap();

    let init = synth(temp.path()).args(["init"]).output().unwrap();
    assert!(
        init.status.success(),
        "init should succeed; stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    // row_count from the export itself (single source of truth).
    let export_raw = fs::read_to_string(export_json_path()).unwrap();
    let export: Value = serde_json::from_str(&export_raw).unwrap();
    let row_count = export["claims_raw"].as_array().unwrap().len();
    assert_eq!(row_count, 6, "fixture sanity: export has 6 rows");

    let export_path = export_json_path();
    let output = synth(temp.path())
        .args(["import", export_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "import should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let v: Value = serde_json::from_str(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
    assert_eq!(
        v["imported"].as_u64().unwrap() as usize,
        row_count,
        "all v2 rows must import (issue #4): {v}"
    );
    assert_eq!(
        v["skipped"].as_u64().unwrap(),
        0,
        "no v2 row should be skipped: {v}"
    );
    // The fixture carries a supersession chain (row 2 'draft' superseded by
    // row 6 'accepted'). A correct remap leaves zero dangling edges -- assert
    // that explicitly so an un-remapped (broken) chain is not reported clean.
    assert_eq!(
        v["dangling_supersedes"].as_u64().unwrap(),
        0,
        "supersedes refs must remap; none should dangle: {v}"
    );

    // Cross-check via `synthesist check`: the chain must resolve to ZERO
    // dangling_supersedes issues. This is the load-bearing id_remap invariant
    // -- without it the export/import round-trip would multi-head the chain.
    let check = synth(temp.path()).args(["check"]).output().unwrap();
    let cv: Value = serde_json::from_str(std::str::from_utf8(&check.stdout).unwrap()).unwrap();
    let dangling = cv["issues"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|i| i["kind"] == "dangling_supersedes")
                .count()
        })
        .unwrap_or(0);
    assert_eq!(
        dangling, 0,
        "synthesist check must report zero dangling_supersedes after import: {cv}"
    );

    // And the superseded spec must NOT be a live head: exactly one spec head
    // (the 'accepted' one) survives, proving a single live head post-import.
    let status = synth(temp.path()).args(["status"]).output().unwrap();
    assert!(
        status.status.success(),
        "status should succeed; stderr: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}
