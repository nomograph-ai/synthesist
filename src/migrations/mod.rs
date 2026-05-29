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
    fn from_version(&self) -> &'static str;
    /// Target schema version this migration produces (e.g. `"3.0.0-pre.1"`).
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
    #[error("unsupported claim type: {ty} -- v2-to-v3 migration handles synthesist-owned claim types only; if your store contains lattice-typed claims, surface this to the synthesist authors")]
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
pub fn registry() -> Vec<Box<dyn Migration>> {
    vec![Box::new(v2_to_v3::V2ToV3)]
}
