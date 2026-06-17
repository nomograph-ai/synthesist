//! Migration runner: plan a chain from the current schema version, apply it.
//!
//! `plan()` reads `claims/_schema.json` (or infers the version from store
//! layout when the file is absent) and returns the ordered subset of
//! `registry()` that must run to reach `target`.
//!
//! `apply_chain()` executes the chain in order. After each successful step it
//! writes `claims/_schema.json` so that an interrupted migration can be resumed
//! or diagnosed correctly.

use std::path::Path;

use chrono::Utc;

use super::{Migration, MigrationError, MigrationOpts, MigrationReport, registry, schema};

/// Infer the store's current schema version.
///
/// Rules (in priority order):
/// 1. `claims/_schema.json` present: use its `schema_version`.
/// 2. Walk the registry, ask each `Migration::detect`. The first whose
///    `detect` returns true tells us we are at its `from_version`. This
///    is more precise than a layout heuristic: `V2ToV3::detect`, for
///    instance, also rejects stores that have already produced v3 logs.
/// 3. No migration applies: fresh or unknown state, version `"fresh"`.
///
/// This is the SINGLE source of truth for the displayed current version
/// (`cmd_status` calls it too -- it must not maintain a parallel heuristic).
pub fn current_version(root: &Path) -> Result<String, MigrationError> {
    current_version_with(root, &registry())
}

/// `current_version` against an explicit migration set.
///
/// Extracted so multi-step chaining can be unit-tested with a synthetic
/// registry (see the chain-extensibility test). The public `current_version`
/// delegates here using `registry()`.
fn current_version_with(
    root: &Path,
    migrations: &[Box<dyn Migration>],
) -> Result<String, MigrationError> {
    let claims = root.join("claims");

    // Rule 1: authoritative file.
    if let Some(record) = schema::read(&claims)? {
        return Ok(record.schema_version);
    }

    // Rule 2: registry-driven detection. Closes the gap left by the
    // previous hard-coded `claims/changes/ -> "2.x"` heuristic, which
    // misclassified partially migrated stores.
    for migration in migrations {
        if migration.detect(root)? {
            return Ok(migration.source_version().to_string());
        }
    }

    // Rule 3: fresh or unknown.
    Ok("fresh".to_string())
}

/// Build the chain of migrations needed to advance from the current version
/// to `target` (or to the latest registered version when `target` is `None`).
///
/// Returns a slice of `Box<dyn Migration>` from the registry in application
/// order. Because the registry owns the boxed values, the returned vec holds
/// references into a locally-constructed registry -- so we return owned boxes
/// and the caller holds them for the duration of `apply_chain`.
pub fn plan(root: &Path, target: Option<&str>) -> Result<Vec<Box<dyn Migration>>, MigrationError> {
    plan_with(root, target, registry())
}

/// `plan` against an explicit migration set, which the function CONSUMES
/// (the returned chain holds owned boxes pulled out of `migrations`).
///
/// Extracted so a synthetic multi-migration registry can be planned in a
/// unit test (proving v3->beyond chaining works). The public `plan`
/// delegates here using `registry()`.
fn plan_with(
    root: &Path,
    target: Option<&str>,
    migrations: Vec<Box<dyn Migration>>,
) -> Result<Vec<Box<dyn Migration>>, MigrationError> {
    let current = current_version_with(root, &migrations)?;

    // Resolve target: default to last migration's to_version.
    if migrations.is_empty() {
        return Err(MigrationError::NoApplicableMigration(
            "registry is empty".to_string(),
        ));
    }

    let resolved_target = match target {
        Some(t) => {
            // Validate target exists.
            if !migrations.iter().any(|m| m.to_version() == t) {
                return Err(MigrationError::TargetNotFound(t.to_string()));
            }
            t.to_string()
        }
        None => migrations.last().unwrap().to_version().to_string(),
    };

    if current == resolved_target {
        return Err(MigrationError::AlreadyAtVersion(current));
    }

    // Collect migrations: find where we are in the chain and walk forward.
    // A migration is included when:
    //   - its from_version == current (or transitively, the to_version of the
    //     previous step), AND
    //   - we have not yet reached resolved_target.
    let mut chain: Vec<Box<dyn Migration>> = Vec::new();
    let mut cursor = current.clone();

    for migration in migrations {
        if cursor == resolved_target {
            break;
        }
        if migration.source_version() == cursor {
            cursor = migration.to_version().to_string();
            chain.push(migration);
        }
    }

    if chain.is_empty() {
        // Current version is not a known from_version for any migration --
        // either already ahead, or a gap.
        if current == "fresh" {
            return Err(MigrationError::NoApplicableMigration(
                "store is fresh -- no migration needed".to_string(),
            ));
        }
        return Err(MigrationError::NoApplicableMigration(format!(
            "no migration path from {current} to {resolved_target}"
        )));
    }

    if cursor != resolved_target {
        return Err(MigrationError::NoApplicableMigration(format!(
            "migration chain from {current} reaches {cursor} but target is {resolved_target}"
        )));
    }

    Ok(chain)
}

/// Apply a pre-planned chain of migrations in order.
///
/// After each successful step, writes `claims/_schema.json` to record the
/// new version. Aborts and returns an error on the first failure; the
/// tarball backup written by the migration step is the rollback path.
///
/// DRY RUN: when `opts.dry_run`, each step's `run()` does its full read +
/// validation but writes nothing (see the `Migration::run` contract), and the
/// intermediate `_schema.json` write below is skipped. NOTE the resulting
/// limitation for MULTI-STEP chains: because the intermediate version record is
/// not written, a step-2 `run()` that reads `_schema.json` (or calls
/// `current_version`) will NOT observe step 1's result the way it would in a
/// real run, so a dry run does not fully reproduce a real multi-step run. This
/// is harmless for the single-migration registry shipping today; a future
/// multi-step chain that needs faithful dry-run semantics should thread an
/// in-memory version cursor rather than rely on on-disk `_schema.json`.
pub fn apply_chain(
    root: &Path,
    chain: &[Box<dyn Migration>],
    opts: &MigrationOpts,
) -> Result<Vec<MigrationReport>, MigrationError> {
    let claims = root.join("claims");
    let mut reports = Vec::new();

    for migration in chain {
        let report = migration.run(root, opts)?;

        // Write schema.json after each successful step (skip in dry-run).
        if !opts.dry_run {
            schema::write(&claims, migration.to_version(), Utc::now())?;
        }

        reports.push(report);
    }

    Ok(reports)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use crate::migrations::V3_SCHEMA_VERSION;
    use crate::migrations::v2_to_v3::V2ToV3;
    use chrono::Utc;
    use nomograph_claim::claim::{Claim, ClaimType};
    use nomograph_claim::store::Store as V2Store;
    use serde_json::json;
    use tempfile::TempDir;

    /// A synthetic no-op migration: 3.0.0 -> 3.1.0. Proves the runner can
    /// chain a SECOND step past the real v2->v3 (the v3->beyond requirement).
    struct FakeV3ToV31;

    impl Migration for FakeV3ToV31 {
        fn source_version(&self) -> &'static str {
            V3_SCHEMA_VERSION // "3.0.0"
        }
        fn to_version(&self) -> &'static str {
            "3.1.0"
        }
        fn description(&self) -> &'static str {
            "synthetic no-op migration for chain testing"
        }
        fn detect(&self, _root: &Path) -> Result<bool, MigrationError> {
            // Detection is irrelevant once _schema.json exists (Rule 1
            // short-circuits current_version); return false defensively.
            Ok(false)
        }
        fn run(
            &self,
            _root: &Path,
            _opts: &MigrationOpts,
        ) -> Result<MigrationReport, MigrationError> {
            Ok(MigrationReport {
                from: self.source_version().to_string(),
                to: self.to_version().to_string(),
                artifacts_touched: 0,
                backup_path: None,
                notes: vec!["no-op".to_string()],
            })
        }
    }

    fn synthetic_registry() -> Vec<Box<dyn Migration>> {
        vec![Box::new(V2ToV3), Box::new(FakeV3ToV31)]
    }

    /// A step-2 migration that asserts, INSIDE run(), that `_schema.json`
    /// already reads the step-1 version "3.0.0". This proves the runner
    /// writes the intermediate schema record BEFORE invoking step 2 -- the
    /// resumability property the doc comments promise. If apply_chain wrote
    /// _schema.json only once at the end, run() here would observe `None` (or
    /// the wrong version) and panic.
    struct AssertsIntermediateSchema;

    impl Migration for AssertsIntermediateSchema {
        fn source_version(&self) -> &'static str {
            V3_SCHEMA_VERSION // "3.0.0"
        }
        fn to_version(&self) -> &'static str {
            "3.1.0"
        }
        fn description(&self) -> &'static str {
            "synthetic step-2 that verifies the intermediate _schema.json write"
        }
        fn detect(&self, _root: &Path) -> Result<bool, MigrationError> {
            Ok(false)
        }
        fn run(
            &self,
            root: &Path,
            _opts: &MigrationOpts,
        ) -> Result<MigrationReport, MigrationError> {
            let claims = root.join("claims");
            let record = schema::read(&claims)?
                .expect("step 2 must see the intermediate _schema.json written by step 1");
            assert_eq!(
                record.schema_version, V3_SCHEMA_VERSION,
                "intermediate _schema.json must read 3.0.0 when step 2 runs"
            );
            Ok(MigrationReport {
                from: self.source_version().to_string(),
                to: self.to_version().to_string(),
                artifacts_touched: 0,
                backup_path: None,
                notes: vec!["verified intermediate schema".to_string()],
            })
        }
    }

    /// A step-2 migration that always fails, to exercise the mid-chain abort
    /// path: the error must propagate, and `_schema.json` must remain at the
    /// step-1 version so a re-plan resumes with exactly the remaining step.
    struct FailingStep2;

    impl Migration for FailingStep2 {
        fn source_version(&self) -> &'static str {
            V3_SCHEMA_VERSION
        }
        fn to_version(&self) -> &'static str {
            "3.1.0"
        }
        fn description(&self) -> &'static str {
            "synthetic step-2 that always fails"
        }
        fn detect(&self, _root: &Path) -> Result<bool, MigrationError> {
            Ok(false)
        }
        fn run(
            &self,
            _root: &Path,
            _opts: &MigrationOpts,
        ) -> Result<MigrationReport, MigrationError> {
            Err(MigrationError::Failed(
                "synthetic step-2 failure".to_string(),
            ))
        }
    }

    /// A disconnected migration 3.5.0 -> 3.6.0 with no path from 3.0.0, used
    /// to exercise the gap-rejection branch of `plan_with`.
    struct DisconnectedV35ToV36;

    impl Migration for DisconnectedV35ToV36 {
        fn source_version(&self) -> &'static str {
            "3.5.0"
        }
        fn to_version(&self) -> &'static str {
            "3.6.0"
        }
        fn description(&self) -> &'static str {
            "synthetic disconnected migration (gap)"
        }
        fn detect(&self, _root: &Path) -> Result<bool, MigrationError> {
            Ok(false)
        }
        fn run(
            &self,
            _root: &Path,
            _opts: &MigrationOpts,
        ) -> Result<MigrationReport, MigrationError> {
            unreachable!("disconnected migration must never run")
        }
    }

    /// Sorted (relative-path, contents) of every file under `dir`, for
    /// asserting a tree is byte-for-byte unchanged. Returns empty if absent.
    fn snapshot_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, base, out);
                } else if let Ok(bytes) = std::fs::read(&path) {
                    let rel = path.strip_prefix(base).unwrap_or(&path);
                    out.push((rel.to_string_lossy().into_owned(), bytes));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn make_v2_store(root: &Path) {
        let claims = root.join("claims");
        let mut store = V2Store::init(&claims).expect("init v2 store");
        let now = Utc::now();
        let claim = Claim {
            id: Claim::compute_id(
                &ClaimType::Task,
                &json!({"summary": "t", "status": "pending"}),
                now,
                "user:local:test",
                now,
            ),
            claim_type: ClaimType::Task,
            props: json!({"summary": "t", "status": "pending"}),
            valid_from: now,
            valid_until: None,
            supersedes: None,
            parent_asserter: None,
            asserted_by: "user:local:test".to_string(),
            asserted_at: now,
        };
        store.append(&claim).expect("append");
    }

    #[test]
    fn synthetic_chain_plans_both_steps_in_order() {
        let dir = TempDir::new().unwrap();
        make_v2_store(dir.path());

        let chain = plan_with(dir.path(), None, synthetic_registry()).unwrap();
        assert_eq!(chain.len(), 2, "v2 estate should plan BOTH steps");
        assert_eq!(chain[0].source_version(), "2.x");
        assert_eq!(chain[0].to_version(), V3_SCHEMA_VERSION);
        assert_eq!(chain[1].source_version(), V3_SCHEMA_VERSION);
        assert_eq!(chain[1].to_version(), "3.1.0");
    }

    #[test]
    fn synthetic_chain_apply_advances_schema_through_both() {
        let dir = TempDir::new().unwrap();
        make_v2_store(dir.path());

        let chain = plan_with(dir.path(), None, synthetic_registry()).unwrap();
        let opts = MigrationOpts {
            dry_run: false,
            backup: false,
        };
        let reports = apply_chain(dir.path(), &chain, &opts).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].to, V3_SCHEMA_VERSION);
        assert_eq!(reports[1].to, "3.1.0");

        // _schema.json now reads the FINAL version; current_version sees it.
        let claims = dir.path().join("claims");
        let record = schema::read(&claims).unwrap().unwrap();
        assert_eq!(record.schema_version, "3.1.0");

        let cur = current_version_with(dir.path(), &synthetic_registry()).unwrap();
        assert_eq!(cur, "3.1.0", "chain end-state read back as 3.1.0");
    }

    #[test]
    fn apply_chain_writes_intermediate_schema_before_step_two() {
        // Proves the INTERMEDIATE _schema.json write lands before step 2 runs
        // (the resumability property), not merely that endpoints match. The
        // step-2 migration asserts schema::read == "3.0.0" inside its run().
        let dir = TempDir::new().unwrap();
        make_v2_store(dir.path());

        let registry: Vec<Box<dyn Migration>> =
            vec![Box::new(V2ToV3), Box::new(AssertsIntermediateSchema)];
        let chain = plan_with(dir.path(), None, registry).unwrap();
        assert_eq!(chain.len(), 2);

        let opts = MigrationOpts {
            dry_run: false,
            backup: false,
        };
        // If the intermediate write were missing, step 2's run() panics.
        let reports = apply_chain(dir.path(), &chain, &opts).unwrap();
        assert_eq!(reports.len(), 2);

        let claims = dir.path().join("claims");
        assert_eq!(
            schema::read(&claims).unwrap().unwrap().schema_version,
            "3.1.0"
        );
    }

    #[test]
    fn mid_chain_failure_leaves_step_one_schema_and_resumes() {
        // Step 1 (v2->3.0.0) succeeds; step 2 fails. apply_chain must
        // propagate the error AND leave _schema.json at "3.0.0", so a re-plan
        // resumes with exactly the one remaining step.
        let dir = TempDir::new().unwrap();
        make_v2_store(dir.path());

        let registry: Vec<Box<dyn Migration>> = vec![Box::new(V2ToV3), Box::new(FailingStep2)];
        let chain = plan_with(dir.path(), None, registry).unwrap();
        assert_eq!(chain.len(), 2);

        let opts = MigrationOpts {
            dry_run: false,
            backup: false,
        };
        // `MigrationReport` is not Debug, so match rather than `.unwrap_err()`.
        match apply_chain(dir.path(), &chain, &opts) {
            Err(MigrationError::Failed(_)) => {}
            Err(other) => panic!("expected MigrationError::Failed, got {other:?}"),
            Ok(_) => panic!("mid-chain failure must propagate as an error"),
        }

        let claims = dir.path().join("claims");
        assert_eq!(
            schema::read(&claims).unwrap().unwrap().schema_version,
            V3_SCHEMA_VERSION,
            "step 1's schema must persist after step 2 fails (resume point)"
        );

        // Re-plan: current is now 3.0.0, so only the remaining step is planned.
        let registry2: Vec<Box<dyn Migration>> = vec![Box::new(V2ToV3), Box::new(FailingStep2)];
        let resume = plan_with(dir.path(), None, registry2).unwrap();
        assert_eq!(resume.len(), 1, "resume plans exactly the 1 remaining step");
        assert_eq!(resume[0].source_version(), V3_SCHEMA_VERSION);
        assert_eq!(resume[0].to_version(), "3.1.0");
    }

    #[test]
    fn apply_chain_dry_run_translates_but_writes_nothing() {
        // A dry run must EXERCISE the full translate (so a real estate can be
        // validated before any write) yet leave the store untouched: it reports
        // the real `artifacts_touched`, writes no tarball backup, no
        // `_schema.json`, and the estate still reads as v2 afterward. (The
        // per-asserter LogWriter is `None` under dry_run in V2ToV3::run, so no
        // log.jsonl is written either.) This is the property Josh's #11 re-test
        // relies on: `migrate v2-to-v3 --dry-run` proves the migration against a
        // real `.amc` estate non-destructively.
        let dir = TempDir::new().unwrap();
        make_v2_store(dir.path());

        // Snapshot the entire estate tree BEFORE: the dry run must leave it
        // byte-for-byte identical -- this catches ANY stray write (a per-asserter
        // log.jsonl, a backup, _schema.json), not just the ones detect() keys on.
        let claims = dir.path().join("claims");
        let before = snapshot_tree(&claims);

        let registry: Vec<Box<dyn Migration>> = vec![Box::new(V2ToV3)];
        let chain = plan_with(dir.path(), None, registry).unwrap();

        let opts = MigrationOpts {
            dry_run: true,
            backup: true, // even with backup requested, dry_run must skip it
        };
        let reports = apply_chain(dir.path(), &chain, &opts).unwrap();

        assert_eq!(reports.len(), 1);
        assert_eq!(
            reports[0].artifacts_touched, 1,
            "dry run must still translate + count the seeded claim"
        );
        assert!(
            reports[0].backup_path.is_none(),
            "dry run must not write a tarball backup"
        );

        // The estate tree is unchanged -- no logs, no _schema.json, nothing.
        let after = snapshot_tree(&claims);
        assert_eq!(
            before, after,
            "dry run must leave the claims/ tree byte-for-byte identical"
        );
        assert!(
            schema::read(&claims).unwrap().is_none(),
            "dry run must not write _schema.json"
        );
        let cur =
            current_version_with(dir.path(), &[Box::new(V2ToV3) as Box<dyn Migration>]).unwrap();
        assert_eq!(cur, "2.x", "estate must still read as v2 after a dry run");
    }

    #[test]
    fn plan_rejects_unreachable_target_gap() {
        // A registry with v2->3.0.0 and a DISCONNECTED 3.5.0->3.6.0. Planning
        // toward 3.6.0 from a v2 estate must reject: the chain reaches 3.0.0
        // but cannot bridge the gap to 3.5.0. Exercises the
        // `cursor != resolved_target` branch.
        let dir = TempDir::new().unwrap();
        make_v2_store(dir.path());

        let registry: Vec<Box<dyn Migration>> =
            vec![Box::new(V2ToV3), Box::new(DisconnectedV35ToV36)];
        // `Box<dyn Migration>` is not Debug, so match the Result rather than
        // calling `.unwrap_err()` (which would require the Ok side to be Debug).
        match plan_with(dir.path(), Some("3.6.0"), registry) {
            Err(MigrationError::NoApplicableMigration(_)) => {}
            Err(other) => panic!("expected NoApplicableMigration, got {other:?}"),
            Ok(_) => panic!("unreachable target across a version gap must be rejected"),
        }
    }

    #[test]
    fn plan_rejects_fresh_store_with_no_migration() {
        // The chain-empty + fresh branch: an empty TempDir (no genesis, no
        // _schema.json) is "fresh", and plan must reject with
        // NoApplicableMigration rather than panicking or planning a bogus step.
        let dir = TempDir::new().unwrap();
        match plan_with(dir.path(), None, synthetic_registry()) {
            Err(MigrationError::NoApplicableMigration(msg)) => {
                assert!(
                    msg.contains("fresh"),
                    "expected a fresh-store message, got {msg:?}"
                );
            }
            Err(other) => {
                panic!("expected NoApplicableMigration for a fresh store, got {other:?}")
            }
            Ok(_) => panic!("a fresh store must not plan any migration"),
        }
    }
}
