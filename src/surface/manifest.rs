//! Surface manifest -- schema and parser.
//!
//! A surface manifest is a TOML file that declares which CLI commands are
//! exposed in the generated skill file, which are hidden, and which additional
//! commands beyond the v2.5 baseline are made available.
//!
//! # Format
//!
//! ```toml
//! [manifest]
//! name        = "baseline-v25"
//! description = "v2.5-identical surface"
//!
//! [commands]
//! include = ["status", "task add", "task ready", ...]
//! exclude = []
//! add     = []
//! ```
//!
//! All three command lists are optional and default to empty. The `name` and
//! `description` fields in `[manifest]` are required.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Structured errors surfaced by manifest loading and validation.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// The file could not be read from disk.
    #[error("could not read manifest file '{path}': {cause}")]
    Io {
        path: String,
        #[source]
        cause: std::io::Error,
    },

    /// The file is not valid TOML or does not conform to the manifest schema.
    #[error("manifest parse error in '{path}': {cause}")]
    Parse { path: String, cause: String },

    /// A required field is absent.
    #[error("manifest '{path}' is missing required field '{field}'")]
    MissingField { path: String, field: &'static str },
}

// ---------------------------------------------------------------------------
// On-disk TOML shape (private, for deserialization only)
// ---------------------------------------------------------------------------

/// The raw TOML envelope -- matches the file structure exactly so that
/// `toml::from_str` can deserialize it directly.
#[derive(Deserialize)]
struct RawManifest {
    manifest: RawManifestHeader,
    #[serde(default)]
    commands: RawCommands,
}

#[derive(Deserialize)]
struct RawManifestHeader {
    name: Option<String>,
    description: Option<String>,
}

#[derive(Default, Deserialize)]
struct RawCommands {
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    add: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A parsed and validated surface manifest.
///
/// The fields map directly to the TOML schema:
///
/// - `name` / `description` -- human-readable identity from `[manifest]`.
/// - `include` -- command names that are explicitly listed in the skill
///   output. An empty list means "include all baseline commands" (the
///   default when the field is omitted from the file).
/// - `exclude` -- command names to suppress from the skill output.
/// - `add` -- command names beyond the v2.5 baseline to enable.
///
/// `include`, `exclude`, and `add` contain command surface keys such as
/// `"status"`, `"task add"`, `"overlay run"`. T5.2 interprets these
/// against the registry; at this layer they are opaque strings.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub description: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub add: Vec<String>,
}

/// Load and validate a surface manifest from `path`.
///
/// Returns a structured [`ManifestError`] (wrapped in `anyhow::Error`) on
/// any failure: I/O error, malformed TOML, or a missing required field.
pub fn load(path: &Path) -> Result<Manifest> {
    let path_str = path.display().to_string();

    let text = std::fs::read_to_string(path).map_err(|e| ManifestError::Io {
        path: path_str.clone(),
        cause: e,
    })?;

    parse_str(&text, &path_str)
}

/// Parse a manifest from an in-memory string.
///
/// `source_label` is used only for error messages (e.g. the file path, or
/// `"<inline>"` in tests).
pub fn parse_str(text: &str, source_label: &str) -> Result<Manifest> {
    let raw: RawManifest = toml::from_str(text)
        .map_err(|e| ManifestError::Parse {
            path: source_label.to_owned(),
            cause: e.to_string(),
        })
        .context("loading surface manifest")?;

    let name = raw
        .manifest
        .name
        .ok_or_else(|| ManifestError::MissingField {
            path: source_label.to_owned(),
            field: "manifest.name",
        })?;

    let description = raw
        .manifest
        .description
        .ok_or_else(|| ManifestError::MissingField {
            path: source_label.to_owned(),
            field: "manifest.description",
        })?;

    Ok(Manifest {
        name,
        description,
        include: raw.commands.include,
        exclude: raw.commands.exclude,
        add: raw.commands.add,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // The baseline-v25 manifest mirrors the example in proposal 002,
    // section "Surface manifests and the jig".
    const BASELINE_V25: &str = r#"
[manifest]
name        = "baseline-v25"
description = "v2.5-identical surface"

[commands]
include = [
    "status", "task add", "task ready", "task done",
    "spec add", "spec show", "discovery add", "phase set",
    "session start", "session close", "tree add",
    "campaign add",
]
exclude = []
add     = []
"#;

    // The overlay-exposed manifest adds graph-query commands on top of
    // the v2.5 baseline.
    const SPARQL_EXPOSED: &str = r#"
[manifest]
name        = "overlay-exposed"
description = "v2.5 baseline plus graph query surface"

[commands]
include = [
    "status", "task add", "task ready", "task done",
    "spec add", "spec show", "discovery add", "phase set",
    "session start", "session close", "tree add",
    "campaign add",
]
exclude = []
add     = ["overlay list", "overlay run", "spec hierarchy"]
"#;

    #[test]
    fn parse_baseline_v25() {
        let m = parse_str(BASELINE_V25, "<inline:baseline-v25>").unwrap();
        assert_eq!(m.name, "baseline-v25");
        assert_eq!(m.description, "v2.5-identical surface");
        assert!(m.include.contains(&"status".to_string()));
        assert!(m.include.contains(&"task add".to_string()));
        assert!(m.include.contains(&"campaign add".to_string()));
        assert!(m.exclude.is_empty());
        assert!(m.add.is_empty());
    }

    #[test]
    fn parse_sparql_exposed() {
        let m = parse_str(SPARQL_EXPOSED, "<inline:overlay-exposed>").unwrap();
        assert_eq!(m.name, "overlay-exposed");
        assert_eq!(m.description, "v2.5 baseline plus graph query surface");
        assert!(m.include.contains(&"status".to_string()));
        assert!(m.add.contains(&"overlay list".to_string()));
        assert!(m.add.contains(&"overlay run".to_string()));
        assert!(m.add.contains(&"spec hierarchy".to_string()));
    }

    #[test]
    fn malformed_toml_produces_structured_error() {
        let bad = r#"
[manifest]
name = "oops
description = "missing closing quote on previous line"
"#;
        let err = parse_str(bad, "<inline:bad>").unwrap_err();
        // The error chain should mention "manifest" and give a useful message.
        let msg = format!("{err:#}");
        assert!(
            msg.contains("manifest"),
            "error should mention manifest, got: {msg}"
        );
    }

    #[test]
    fn missing_name_field_is_a_structured_error() {
        let no_name = r#"
[manifest]
description = "no name here"
"#;
        let err = parse_str(no_name, "<inline:no-name>").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("manifest.name"),
            "error should name the missing field, got: {msg}"
        );
    }

    #[test]
    fn missing_description_field_is_a_structured_error() {
        let no_desc = r#"
[manifest]
name = "no-description"
"#;
        let err = parse_str(no_desc, "<inline:no-desc>").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("manifest.description"),
            "error should name the missing field, got: {msg}"
        );
    }

    #[test]
    fn commands_section_is_optional() {
        // A manifest with only [manifest] and no [commands] section is valid.
        // All three lists default to empty.
        let minimal = r#"
[manifest]
name        = "minimal"
description = "commands block absent"
"#;
        let m = parse_str(minimal, "<inline:minimal>").unwrap();
        assert!(m.include.is_empty());
        assert!(m.exclude.is_empty());
        assert!(m.add.is_empty());
    }

    #[test]
    fn load_from_file() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{}", BASELINE_V25).unwrap();
        let m = load(f.path()).unwrap();
        assert_eq!(m.name, "baseline-v25");
    }

    #[test]
    fn load_missing_file_is_io_error() {
        let path = std::path::Path::new("/tmp/this-file-does-not-exist-T5.1.toml");
        let err = load(path).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("could not read"),
            "expected I/O error message, got: {msg}"
        );
    }
}
