//! Dispatch for `synthesist migrate <...>` subcommands.
//!
//! Delegates to `crate::migrations` for all migration logic. This file
//! only wires CLI arguments to the migration module and formats output.

use crate::cli::MigrateCmd;
use crate::migrations::{MigrationError, MigrationOpts, V3_SCHEMA_VERSION, registry, runner};
use crate::store::json_out;

/// Top-level dispatch for `synthesist migrate`.
pub fn run(cmd: &MigrateCmd) -> anyhow::Result<()> {
    match cmd {
        MigrateCmd::List => cmd_list(),
        MigrateCmd::Status => cmd_status(),
        MigrateCmd::Run {
            target,
            dry_run,
            no_backup,
        } => cmd_run(target.as_deref(), *dry_run, *no_backup),
        MigrateCmd::V2ToV3 { dry_run } => {
            // Target the v3 schema id explicitly (safest once more
            // migrations exist; equivalent to `migrate run --target 3.0.0`).
            cmd_run(Some(V3_SCHEMA_VERSION), *dry_run, false)
        }
    }
}

/// `synthesist migrate list` -- print registry entries in chain order.
fn cmd_list() -> anyhow::Result<()> {
    let reg = registry();
    let entries: Vec<_> = reg
        .iter()
        .map(|m| {
            serde_json::json!({
                "name": format!("v{}-to-v{}", m.source_version(), m.to_version()),
                "from_version": m.source_version(),
                "to_version": m.to_version(),
                "description": m.description(),
            })
        })
        .collect();
    json_out(&serde_json::json!({ "migrations": entries }))
}

/// `synthesist migrate status` -- show current schema version and pending chain.
fn cmd_status() -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    let claims = root.join("claims");

    // Read schema record if present (for migrated_at only).
    let schema_record = crate::migrations::schema::read(&claims).unwrap_or(None);

    // SINGLE source of truth for the displayed version: the runner's
    // `current_version`. It applies Rule 1 (_schema.json), Rule 2 (registry
    // detect walk -- now correct on compacted estates per issue #11), then
    // Rule 3 ("fresh"). No parallel `claims/changes/` heuristic here.
    let current_version = runner::current_version(&root).unwrap_or_else(|_| "fresh".to_string());

    let migrated_at = schema_record.as_ref().map(|r| r.migrated_at.as_str());

    // Check pending chain.
    let pending = match runner::plan(&root, None) {
        Ok(chain) => chain
            .iter()
            .map(|m| {
                serde_json::json!({
                    "from_version": m.source_version(),
                    "to_version": m.to_version(),
                    "description": m.description(),
                })
            })
            .collect::<Vec<_>>(),
        Err(MigrationError::AlreadyAtVersion(_)) => vec![],
        Err(MigrationError::NoApplicableMigration(_)) => vec![],
        Err(_) => vec![],
    };

    let status = if current_version == "fresh" && !claims.exists() {
        "fresh store -- no claims directory found"
    } else if pending.is_empty() {
        "store is at latest"
    } else {
        "migrations pending"
    };

    json_out(&serde_json::json!({
        "current_version": current_version,
        "migrated_at": migrated_at,
        "status": status,
        "pending": pending,
    }))
}

/// Core for `migrate run` and `migrate v2-to-v3`.
fn cmd_run(target: Option<&str>, dry_run: bool, no_backup: bool) -> anyhow::Result<()> {
    let root = std::env::current_dir()?;

    let chain = match runner::plan(&root, target) {
        Ok(c) => c,
        Err(MigrationError::AlreadyAtVersion(v)) => {
            json_out(&serde_json::json!({
                "ok": true,
                "message": format!("store is already at version {v}; nothing to do"),
                "current_version": v,
            }))?;
            return Ok(());
        }
        Err(MigrationError::NoApplicableMigration(msg)) => {
            anyhow::bail!(
                "no applicable migration: {msg}. \
                 Run `synthesist migrate list` to see available migrations, \
                 or `synthesist migrate status` to check the current schema version."
            );
        }
        Err(e) => return Err(e.into()),
    };

    // A dry run takes the SAME path as a real one -- it opens the v2 store,
    // loads every claim, and runs the full translation loop -- but writes
    // nothing: each migration's `run()` skips the tarball backup and the
    // LogWriter when `opts.dry_run`, and `apply_chain` skips `_schema.json`.
    // So `--dry-run` is a genuine NON-DESTRUCTIVE validation that exercises the
    // real-`.amc`-only code paths (Store::open, load_claims, asserter
    // normalization) and reports `artifacts_touched` + `notes` (skips) -- not a
    // plan-only stub. This is what lets a migration be proven against a real
    // production estate before any write.
    let opts = MigrationOpts {
        dry_run,
        backup: !no_backup,
    };

    let reports = runner::apply_chain(&root, &chain, &opts)?;

    let report_json: Vec<_> = reports
        .iter()
        .map(|r| {
            serde_json::json!({
                "from": r.from,
                "to": r.to,
                "artifacts_touched": r.artifacts_touched,
                "backup_path": r.backup_path.as_ref().map(|p| p.display().to_string()),
                "notes": r.notes,
            })
        })
        .collect();

    let next_actions: Vec<&str> = if dry_run {
        vec![
            "DRY RUN -- nothing was written (no backup, no logs, no _schema.json)",
            "check each step's `artifacts_touched` (claims that WOULD migrate) and \
             `notes` (any that WOULD be skipped)",
            "re-run without --dry-run to apply; a tarball backup is written first \
             and the source v2 files are left intact",
        ]
    } else {
        vec![
            "run `synthesist migrate status` to confirm schema version",
            "run `synthesist status` to verify claims loaded correctly",
        ]
    };

    json_out(&serde_json::json!({
        "ok": true,
        "dry_run": dry_run,
        "steps": report_json,
        "next_actions": next_actions,
    }))?;

    Ok(())
}
