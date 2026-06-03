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
}
