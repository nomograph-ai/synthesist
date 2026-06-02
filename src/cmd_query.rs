//! `synthesist query` -- retired raw-SPARQL surface (C-2 stub).
//!
//! The raw `query --sparql` / `query --file` command executed arbitrary
//! SPARQL SELECTs against the Oxigraph graph view. The v3 gamma engine
//! (redb typed index) has no SPARQL evaluator: the typed query surface
//! is the ~10 H-helpers on the gamma index, surfaced through the
//! per-type commands and the `overlay` subcommand.
//!
//! The command is kept wired (so `cli.rs` / fixtures still parse) but
//! the body now returns a structured error pointing the caller at the
//! typed surface. The command + its `Query` cli variant are slated for
//! removal in the Stage 3 de-cruft (P3); this stub is the C-2 bridge so
//! the crate compiles with the engine swapped.

use std::path::Path;

use anyhow::{Result, bail};

/// Raw SPARQL is retired with the Oxigraph engine. Returns a structured
/// error directing the caller to the typed surface. `_sparql`, `_file`,
/// and `_data_dir` are accepted so the clap wiring is unchanged.
pub fn cmd_query(
    sparql: Option<&str>,
    file: Option<&Path>,
    _data_dir: Option<&Path>,
) -> Result<()> {
    // Preserve the mutually-exclusive / required-arg diagnostics so the
    // CLI contract for the flags is unchanged before the variant is
    // dropped in P3.
    let _ = resolve_query(sparql, file)?;
    bail!(
        "raw SPARQL queries were retired with the Oxigraph engine in v3. \
         The query surface is now the typed per-type commands \
         (`synthesist task ready`, `synthesist spec list`, ...) and \
         `synthesist overlay run <name>` for cross-graph analyses. \
         See `synthesist overlay list`."
    )
}

/// Validate the `--sparql` / `--file` argument shape (exactly one).
fn resolve_query(sparql: Option<&str>, file: Option<&Path>) -> Result<String> {
    match (sparql, file) {
        (Some(q), None) => Ok(q.to_string()),
        (None, Some(path)) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read SPARQL file {}: {e}", path.display())),
        (Some(_), Some(_)) => {
            bail!("--sparql and --file are mutually exclusive; pass one, not both")
        }
        (None, None) => bail!("one of --sparql or --file is required"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn resolve_query_from_string() {
        let q = "SELECT ?s WHERE { ?s ?p ?o }";
        let result = resolve_query(Some(q), None).unwrap();
        assert_eq!(result, q);
    }

    #[test]
    fn resolve_query_from_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("query.sparql");
        let q = "SELECT ?s WHERE { ?s ?p ?o }";
        std::fs::write(&path, q).unwrap();
        let result = resolve_query(None, Some(&path)).unwrap();
        assert_eq!(result, q);
    }

    #[test]
    fn resolve_query_both_flags_is_error() {
        let result = resolve_query(Some("SELECT ?s WHERE {}"), Some(Path::new("/tmp/x.sparql")));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("mutually exclusive"));
    }

    #[test]
    fn resolve_query_neither_flag_is_error() {
        let result = resolve_query(None, None);
        assert!(result.is_err());
    }

    /// The command itself returns the retired-surface error for a valid
    /// (single-flag) invocation.
    #[test]
    fn cmd_query_reports_retired_surface() {
        let err = cmd_query(Some("SELECT ?s WHERE { ?s ?p ?o }"), None, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("retired"), "got: {msg}");
        assert!(msg.contains("overlay"), "got: {msg}");
    }
}
