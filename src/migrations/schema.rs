//! Helpers to read and write `claims/_schema.json`.
//!
//! Schema file shape:
//! ```json
//! { "schema_version": "3.0.0-pre.1", "migrated_at": "2026-05-29T14:00:00.000Z" }
//! ```
//!
//! Missing file is valid: it means either a fresh v3 store (no migration
//! ever run) or a v2 store that has not yet been migrated. Callers
//! distinguish those two cases by checking for `claims/changes/`.

use std::path::Path;

use anyhow::Context as _;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::MigrationError;

pub const SCHEMA_FILE: &str = "_schema.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRecord {
    pub schema_version: String,
    pub migrated_at: String,
}

/// Read `claims/_schema.json`. Returns `None` when the file is absent.
pub fn read(claims_dir: &Path) -> Result<Option<SchemaRecord>, MigrationError> {
    let path = claims_dir.join(SCHEMA_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))
        .map_err(|e| MigrationError::Io(std::io::Error::other(e.to_string())))?;
    let record: SchemaRecord = serde_json::from_str(&raw)
        .with_context(|| format!("parse {}", path.display()))
        .map_err(|e| MigrationError::Io(std::io::Error::other(e.to_string())))?;
    Ok(Some(record))
}

/// Write `claims/_schema.json` with the given version and current UTC timestamp.
pub fn write(claims_dir: &Path, version: &str, at: DateTime<Utc>) -> Result<(), MigrationError> {
    std::fs::create_dir_all(claims_dir)
        .map_err(MigrationError::Io)?;
    let record = SchemaRecord {
        schema_version: version.to_string(),
        migrated_at: at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
    };
    let path = claims_dir.join(SCHEMA_FILE);
    let json = serde_json::to_string_pretty(&record)
        .map_err(|e| MigrationError::Io(std::io::Error::other(e.to_string())))?;
    std::fs::write(&path, json)
        .map_err(MigrationError::Io)?;
    Ok(())
}
