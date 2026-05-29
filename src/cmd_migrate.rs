//! Dispatch for `synthesist migrate <...>` subcommands.
//!
//! Delegates to `crate::migrations` for all migration logic. This file
//! only wires CLI arguments to the migration module and formats output.

use crate::cli::MigrateCmd;
use crate::migrations::{MigrationError, MigrationOpts, registry, runner};
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
            cmd_run(Some("3.0.0-pre.1"), *dry_run, false)
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
                "name": format!("v{}-to-v{}", m.from_version(), m.to_version()),
                "from_version": m.from_version(),
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

    // Read schema record if present.
    let schema_record = crate::migrations::schema::read(&claims).unwrap_or(None);

    let current_version = schema_record
        .as_ref()
        .map(|r| r.schema_version.as_str())
        .unwrap_or_else(|| {
            if claims.join("changes").exists() {
                "2.x"
            } else {
                "fresh"
            }
        });

    let migrated_at = schema_record.as_ref().map(|r| r.migrated_at.as_str());

    // Check pending chain.
    let pending = match runner::plan(&root, None) {
        Ok(chain) => chain
            .iter()
            .map(|m| {
                serde_json::json!({
                    "from_version": m.from_version(),
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
            anyhow::bail!("no applicable migration: {msg}");
        }
        Err(e) => return Err(e.into()),
    };

    if dry_run {
        let plan: Vec<_> = chain
            .iter()
            .map(|m| {
                serde_json::json!({
                    "from_version": m.from_version(),
                    "to_version": m.to_version(),
                    "description": m.description(),
                })
            })
            .collect();
        json_out(&serde_json::json!({
            "ok": true,
            "dry_run": true,
            "plan": plan,
        }))?;
        return Ok(());
    }

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

    json_out(&serde_json::json!({
        "ok": true,
        "dry_run": false,
        "steps": report_json,
        "next_actions": [
            "run `synthesist migrate status` to confirm schema version",
            "run `synthesist status` to verify claims loaded correctly",
        ],
    }))?;

    Ok(())
}

