//! Engineered migrations module for nomograph-synthesist.
//!
//! Provides a forward-only migration chain from v2 onward. Adding a new
//! migration (e.g. v3 to v4) is a file drop plus one `registry()` entry.
//!
//! ## Concepts
//!
//! - `Migration` trait: implemented once per schema transition.
//! - `registry()`: ordered list of all registered migrations (oldest first).
//! - `runner`: reads `claims/_schema.json`, builds a chain, applies it.
//! - `claims/_schema.json`: single source of truth for the current store version.

pub mod runner;
pub mod schema;
pub mod v2_to_v3;

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Canonical v3 schema id written to `claims/_schema.json`.
///
/// This tracks the on-disk FORMAT, not the binary release tag. The binary
/// may ship as `3.0.0-rc.1` / `3.0.0` / etc., but the format produced by
/// the v2-to-v3 migration is stable and identified as `"3.0.0"`.
pub const V3_SCHEMA_VERSION: &str = "3.0.0";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Options controlling migration behavior.
pub struct MigrationOpts {
    /// If true, walk and validate but do not write anything.
    pub dry_run: bool,
    /// If true (the default), write a tarball backup before mutating.
    pub backup: bool,
}

impl Default for MigrationOpts {
    fn default() -> Self {
        Self {
            dry_run: false,
            backup: true,
        }
    }
}

/// Summary returned by a successful `Migration::run`.
pub struct MigrationReport {
    pub from: String,
    pub to: String,
    /// Number of claim artifacts touched (translated, written, etc.).
    pub artifacts_touched: usize,
    /// Path to the tarball backup written before mutation, if any.
    pub backup_path: Option<PathBuf>,
    /// Human-readable notes about skipped items or other observations.
    pub notes: Vec<String>,
}

/// A single schema transition.
///
/// Implementations must be `Send + Sync` so they can live in a global
/// registry and be shared across threads.
pub trait Migration: Send + Sync {
    /// Source schema version this migration reads from (e.g. `"2.x"`).
    fn source_version(&self) -> &'static str;
    /// Target schema version this migration produces (e.g. `"3.0.0"`).
    fn to_version(&self) -> &'static str;
    /// One-line human description shown by `migrate list`.
    fn description(&self) -> &'static str;
    /// Returns true when this migration is applicable to the store at `root`.
    ///
    /// `root` is the directory that contains `claims/`.
    fn detect(&self, root: &Path) -> Result<bool, MigrationError>;
    /// Execute the migration. Called only when `detect` returned true and
    /// the caller has confirmed the chain is correct.
    fn run(&self, root: &Path, opts: &MigrationOpts) -> Result<MigrationReport, MigrationError>;
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "unsupported claim type: {ty} -- v2-to-v3 migration handles synthesist-owned claim types only; if your store contains lattice-typed claims, surface this to the synthesist authors"
    )]
    UnsupportedClaimType { ty: String },
    #[error("no migration applicable: {0}")]
    NoApplicableMigration(String),
    #[error("store already at version {0}; no migration needed")]
    AlreadyAtVersion(String),
    #[error("target version {0} not found in registry")]
    TargetNotFound(String),
    #[error("migration failed: {0}")]
    Failed(String),
    #[error("anyhow: {0}")]
    Anyhow(#[from] anyhow::Error),
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Ordered list of all registered migrations (oldest source first).
///
/// To add a new migration: implement `Migration`, create a new module,
/// and append `Box::new(YourMigration)` here.
///
/// The chain walk in `runner::plan_with` advances a cursor through this vec
/// in order, so each entry (after the first) must have a `source_version`
/// reachable from a prior entry's `to_version`. An out-of-order insertion
/// would silently mis-plan; the debug_assert below fails CI on that mistake.
pub fn registry() -> Vec<Box<dyn Migration>> {
    let reg: Vec<Box<dyn Migration>> = vec![Box::new(v2_to_v3::V2ToV3)];
    debug_assert!(
        registry_chains_head_to_tail(&reg),
        "registry is not in chain order: each entry's source_version must equal a prior entry's to_version (oldest first)"
    );
    reg
}

/// True when, scanning the registry in order, every entry's `source_version`
/// is the FIRST entry's source or equals some earlier entry's `to_version`.
/// (A linear forward-only chain; the same property `runner::plan_with` relies
/// on.) Used only by a debug_assert in `registry()`.
fn registry_chains_head_to_tail(reg: &[Box<dyn Migration>]) -> bool {
    let mut reached: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (i, m) in reg.iter().enumerate() {
        if i == 0 {
            reached.insert(m.to_version());
            continue;
        }
        if !reached.contains(m.source_version()) {
            return false;
        }
        reached.insert(m.to_version());
    }
    true
}
