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
fn current_version(root: &Path) -> Result<String, MigrationError> {
    let claims = root.join("claims");

    // Rule 1: authoritative file.
    if let Some(record) = schema::read(&claims)? {
        return Ok(record.schema_version);
    }

    // Rule 2: registry-driven detection. Closes the gap left by the
    // previous hard-coded `claims/changes/ -> "2.x"` heuristic, which
    // misclassified partially migrated stores.
    for migration in registry() {
        if migration.detect(root)? {
            return Ok(migration.from_version().to_string());
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
pub fn plan(
    root: &Path,
    target: Option<&str>,
) -> Result<Vec<Box<dyn Migration>>, MigrationError> {
    let current = current_version(root)?;

    // Resolve target: default to last registered migration's to_version.
    let reg = registry();
    if reg.is_empty() {
        return Err(MigrationError::NoApplicableMigration(
            "registry is empty".to_string(),
        ));
    }

    let resolved_target = match target {
        Some(t) => {
            // Validate target exists.
            if !reg.iter().any(|m| m.to_version() == t) {
                return Err(MigrationError::TargetNotFound(t.to_string()));
            }
            t.to_string()
        }
        None => reg.last().unwrap().to_version().to_string(),
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

    // We need to re-construct the registry to get owned boxes (registry()
    // returns fresh boxes each call).
    let reg2 = registry();
    for migration in reg2 {
        if cursor == resolved_target {
            break;
        }
        if migration.from_version() == cursor {
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
