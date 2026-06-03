//! Active-surface resolution and the builtin manifest registry.
//!
//! Phase D wires surface manifests into the *runtime* dispatch (not just
//! skill emission). This module owns three things:
//!
//! 1. The **builtin manifest registry** -- the `surface/*.toml` files are
//!    `include_str!`-embedded so `surface use <name>` works in the shipped
//!    binary, with no source tree present.
//! 2. **Active-manifest resolution** with a documented precedence chain.
//! 3. The **sticky setting** read/write (set by `surface use`).
//!
//! # Active-manifest precedence
//!
//! The active manifest is resolved in this order (first match wins):
//!
//! 1. `--manifest <name-or-path>` one-shot override (global CLI flag).
//! 2. `SYNTHESIST_MANIFEST` env var (name or path).
//! 3. The persisted sticky setting written by `synthesist surface use`.
//! 4. The default builtin, [`DEFAULT_MANIFEST`] (`baseline-v25`).
//!
//! At each layer the value is interpreted as a builtin manifest NAME first;
//! if no builtin matches, it is treated as a filesystem PATH.
//!
//! # Sticky storage location
//!
//! The sticky setting is a plain-text file holding the chosen manifest name
//! (or path), stored per-estate at:
//!
//! ```text
//! <claims-dir>/_config/active-surface
//! ```
//!
//! It lives inside the estate's `claims/` tree alongside the other
//! underscore-prefixed runtime artifacts (`_view.gamma`, `_jig/`), so it
//! travels with the estate and is naturally scoped to it. The file is
//! created on first `surface use`; its absence means "no sticky setting".

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::manifest::{self, Manifest};

/// The default builtin manifest: the full v2.5 surface.
pub const DEFAULT_MANIFEST: &str = "baseline-v25";

/// Subdirectory (under `claims/`) holding per-estate runtime config.
const CONFIG_DIR: &str = "_config";

/// Filename (under `_config/`) holding the sticky active-surface setting.
const STICKY_FILE: &str = "active-surface";

/// The builtin manifests, embedded at build time from `surface/*.toml`.
///
/// Each entry is `(name, toml-source)`. The `name` matches the `[manifest]
/// name` field inside the corresponding file. Embedding means
/// `surface use <name>` and the default resolution both work from a shipped
/// binary with no `surface/` directory present on disk.
const BUILTINS: &[(&str, &str)] = &[
    ("baseline-v25", include_str!("../../surface/baseline-v25.toml")),
    (
        "composite-commands",
        include_str!("../../surface/composite-commands.toml"),
    ),
    (
        "overlay-first-class",
        include_str!("../../surface/overlay-first-class.toml"),
    ),
    ("pruned", include_str!("../../surface/pruned.toml")),
    (
        "sparql-exposed",
        include_str!("../../surface/sparql-exposed.toml"),
    ),
];

/// The names of all builtin manifests, in registry order.
pub fn builtin_names() -> Vec<&'static str> {
    BUILTINS.iter().map(|(name, _)| *name).collect()
}

/// Return the embedded TOML source for a builtin manifest name, if any.
pub fn builtin_toml(name: &str) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, toml)| *toml)
}

/// Resolve a manifest reference (a builtin NAME or a filesystem PATH) into a
/// parsed [`Manifest`].
///
/// A builtin name takes precedence: if `reference` names a builtin, the
/// embedded TOML is parsed. Otherwise `reference` is treated as a path on
/// disk and loaded via [`manifest::load`].
pub fn resolve_reference(reference: &str) -> Result<Manifest> {
    if let Some(toml) = builtin_toml(reference) {
        return manifest::parse_str(toml, &format!("<builtin:{reference}>"));
    }
    let path = Path::new(reference);
    manifest::load(path)
        .with_context(|| format!("'{reference}' is not a builtin manifest name and could not be loaded as a path"))
}

/// Path to the sticky setting file for the estate rooted at `claims_dir`.
///
/// `claims_dir` is the `claims/` directory (i.e. `SynthStore::root()`).
pub fn sticky_path(claims_dir: &Path) -> PathBuf {
    claims_dir.join(CONFIG_DIR).join(STICKY_FILE)
}

/// Read the sticky active-surface setting for the estate, if one is set.
///
/// Returns `Ok(None)` when no sticky file exists. A present-but-empty file is
/// treated as unset.
pub fn read_sticky(claims_dir: &Path) -> Result<Option<String>> {
    let path = sticky_path(claims_dir);
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read sticky surface at {}", path.display())),
    }
}

/// Persist the sticky active-surface setting for the estate.
///
/// Writes `value` (a builtin name or a path) to the per-estate sticky file,
/// creating the `_config/` directory if needed.
pub fn write_sticky(claims_dir: &Path, value: &str) -> Result<()> {
    let dir = claims_dir.join(CONFIG_DIR);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create config dir {}", dir.display()))?;
    let path = sticky_path(claims_dir);
    std::fs::write(&path, format!("{value}\n"))
        .with_context(|| format!("write sticky surface at {}", path.display()))?;
    Ok(())
}

/// Resolve the *reference string* for the active manifest, applying the
/// documented precedence chain. Does not parse the manifest.
///
/// `claims_dir` is the estate's `claims/` directory used for the sticky
/// lookup; pass `None` when no estate is available (e.g. before `init`), in
/// which case the sticky layer is skipped.
///
/// Precedence: `--manifest` (`cli_override`) > `SYNTHESIST_MANIFEST` env >
/// sticky setting > [`DEFAULT_MANIFEST`].
pub fn active_reference(cli_override: Option<&str>, claims_dir: Option<&Path>) -> Result<String> {
    if let Some(r) = cli_override {
        return Ok(r.to_string());
    }
    if let Ok(env) = std::env::var("SYNTHESIST_MANIFEST")
        && !env.trim().is_empty()
    {
        return Ok(env.trim().to_string());
    }
    if let Some(dir) = claims_dir
        && let Some(sticky) = read_sticky(dir)?
    {
        return Ok(sticky);
    }
    Ok(DEFAULT_MANIFEST.to_string())
}

/// Resolve and parse the active manifest, applying the precedence chain.
///
/// Returns the resolved reference string alongside the parsed manifest so
/// callers can report which surface is active (e.g. in error messages).
pub fn active_manifest(
    cli_override: Option<&str>,
    claims_dir: Option<&Path>,
) -> Result<(String, Manifest)> {
    let reference = active_reference(cli_override, claims_dir)?;
    let manifest = resolve_reference(&reference)?;
    Ok((reference, manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_parse_and_name_matches() {
        for (name, _toml) in BUILTINS {
            let m = resolve_reference(name)
                .unwrap_or_else(|e| panic!("builtin {name} failed to parse: {e:#}"));
            assert_eq!(
                &m.name, name,
                "builtin file name {name} must match its [manifest] name"
            );
        }
    }

    #[test]
    fn default_is_baseline_v25() {
        assert_eq!(DEFAULT_MANIFEST, "baseline-v25");
        assert!(builtin_toml(DEFAULT_MANIFEST).is_some());
    }

    #[test]
    fn unknown_name_falls_through_to_path_error() {
        let err = resolve_reference("definitely-not-a-builtin-xyz").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("not a builtin manifest name"),
            "expected fallthrough-to-path message, got: {msg}"
        );
    }

    #[test]
    fn sticky_roundtrip_and_absent_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let claims = tmp.path().join("claims");
        std::fs::create_dir_all(&claims).unwrap();

        // Absent -> None.
        assert!(read_sticky(&claims).unwrap().is_none());

        // Write -> read back.
        write_sticky(&claims, "pruned").unwrap();
        assert_eq!(read_sticky(&claims).unwrap().as_deref(), Some("pruned"));
    }

    #[test]
    fn precedence_override_beats_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let claims = tmp.path().join("claims");
        std::fs::create_dir_all(&claims).unwrap();
        write_sticky(&claims, "pruned").unwrap();

        // CLI override wins over sticky.
        let r = active_reference(Some("composite-commands"), Some(&claims)).unwrap();
        assert_eq!(r, "composite-commands");
    }

    #[test]
    fn precedence_sticky_beats_default() {
        let tmp = tempfile::tempdir().unwrap();
        let claims = tmp.path().join("claims");
        std::fs::create_dir_all(&claims).unwrap();
        write_sticky(&claims, "pruned").unwrap();

        // No override, env unset in this process for the test's scope: rely
        // on the sticky file. (Env is process-global; we avoid setting it
        // here to keep the test hermetic.)
        if std::env::var("SYNTHESIST_MANIFEST").is_err() {
            let r = active_reference(None, Some(&claims)).unwrap();
            assert_eq!(r, "pruned");
        }
    }

    #[test]
    fn precedence_default_when_nothing_set() {
        let tmp = tempfile::tempdir().unwrap();
        let claims = tmp.path().join("claims");
        std::fs::create_dir_all(&claims).unwrap();
        if std::env::var("SYNTHESIST_MANIFEST").is_err() {
            let r = active_reference(None, Some(&claims)).unwrap();
            assert_eq!(r, DEFAULT_MANIFEST);
        }
    }
}
