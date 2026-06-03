//! `surface` command -- inspect and switch the active surface manifest.
//!
//! The active surface governs which commands the runtime permits (see the
//! rejection layer in `main.rs::run`). This command lets an operator switch
//! surfaces (`use`), enumerate the builtins (`list`), and inspect the active
//! surface's enabled commands (`show`).
//!
//! `surface` is always permitted regardless of the active manifest, so an
//! operator can always recover from a restrictive surface.

use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::SurfaceCmd;
use crate::store::{self, SynthStore};
use crate::surface::resolve;

/// Dispatch a `surface` subcommand.
///
/// `cli_manifest` is the global `--manifest` one-shot override (if any), so
/// `surface show`/`list` reflect the same precedence the rejection layer
/// uses.
pub fn run(cmd: &SurfaceCmd, cli_manifest: Option<&str>) -> Result<()> {
    match cmd {
        SurfaceCmd::Use { name } => cmd_use(name),
        SurfaceCmd::List => cmd_list(cli_manifest),
        SurfaceCmd::Show => cmd_show(cli_manifest),
    }
}

/// Locate the estate's `claims/` directory for the sticky setting, if one is
/// present. Returns `None` when no estate has been initialized yet.
fn claims_dir() -> Option<std::path::PathBuf> {
    SynthStore::discover().ok().map(|s| s.root().to_path_buf())
}

/// `surface use <name>` -- persist the active manifest for this estate.
fn cmd_use(name: &str) -> Result<()> {
    // Validate the reference resolves before persisting, so a typo fails
    // loudly instead of being silently written and biting later.
    let manifest = resolve::resolve_reference(name)
        .with_context(|| format!("cannot use surface '{name}'"))?;

    let dir = claims_dir().context(
        "no synthesist estate found here; run `synthesist init` before `surface use`",
    )?;
    resolve::write_sticky(&dir, name)?;

    store::json_out(&json!({
        "ok": true,
        "active": manifest.name,
        "reference": name,
    }))
}

/// `surface list` -- builtin manifest names plus which surface is active.
fn cmd_list(cli_manifest: Option<&str>) -> Result<()> {
    let dir = claims_dir();
    let active_ref = resolve::active_reference(cli_manifest, dir.as_deref())?;
    // Resolve the active reference to its manifest name for display.
    let active_name = resolve::resolve_reference(&active_ref)
        .map(|m| m.name)
        .unwrap_or_else(|_| active_ref.clone());

    let builtins: Vec<_> = resolve::builtin_names()
        .into_iter()
        .map(|name| {
            json!({
                "name": name,
                "active": name == active_name,
            })
        })
        .collect();

    store::json_out(&json!({
        "active": active_name,
        "active_reference": active_ref,
        "builtins": builtins,
    }))
}

/// `surface show` -- active manifest name and its enabled command keys.
fn cmd_show(cli_manifest: Option<&str>) -> Result<()> {
    let dir = claims_dir();
    let (reference, manifest) = resolve::active_manifest(cli_manifest, dir.as_deref())?;
    let keys = crate::cli::permitted_keys(&manifest);

    store::json_out(&json!({
        "active": manifest.name,
        "reference": reference,
        "description": manifest.description,
        "commands": keys,
    }))
}
