//! SPARQL query command (read-only) against the local graph view.
//!
//! Provides `synthesist query --sparql <query>` and
//! `synthesist query --file <path>`. Both modes execute a SPARQL SELECT
//! against the local Oxigraph view and print results as JSON.
//!
//! ## View selection strategy
//!
//! The command tries to open the on-disk RocksDB view at
//! `claims/_view.oxigraph/` first. If that path does not exist, or if
//! opening it fails (see the macOS RocksDB known issue below), the
//! command falls back to an in-memory view rebuilt from the log union.
//! The help text surfaces this behavior explicitly.
//!
//! Known macOS issue: `GraphView::open` may fail with a TryFromIntError
//! inside oxigraph's rocksdb_wrapper on some macOS environments. See
//! the ignored tests in `nomograph_claim::graph_view`. Until that is
//! resolved, the in-memory fallback is the practical path for macOS
//! development and tests.
//!
//! ## T5.2 follow-up
//!
//! This subcommand is wired as a regular clap subcommand. T5.2 (CLI
//! command registry refactor) will introduce manifest-driven
//! visibility. At that point, this command should be registered under
//! the `sparql-exposed` manifest key and hidden from `baseline-v25`.
//! The migration point is the `Query` variant in `cli::Command` and the
//! dispatch arm in `main::run`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use nomograph_claim::graph_view::{GraphView, SelectResults, Term, rebuild, select};

use nomograph_synthesist::telemetry::{Surface, TelemetryWriter};
use serde_json::{Value, json};

use crate::store::json_out;

/// Execute a SPARQL SELECT query and print results as JSON.
///
/// The caller supplies either an inline query string (`--sparql`) or a
/// path to a file containing the query (`--file`). Exactly one of the
/// two must be provided; the clap layer enforces this.
pub fn cmd_query(sparql: Option<&str>, file: Option<&Path>, data_dir: Option<&Path>) -> Result<()> {
    let query = resolve_query(sparql, file)?;
    let claims_dir = locate_claims_dir(data_dir)?;

    let view = open_view(Some(&claims_dir))?;

    // Time the query and record telemetry, regardless of success or
    // failure. Telemetry record failures must not mask the query
    // result, so the writer is best-effort: log to stderr but proceed.
    let start = std::time::Instant::now();
    let query_result = select(&view, &query);
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    let (errored, result_count) = match &query_result {
        Ok(r) => (false, r.rows.len()),
        Err(_) => (true, 0),
    };

    if let Ok(writer) = TelemetryWriter::new(&claims_dir) {
        if let Err(e) = writer.record_query(
            Surface::Cli,
            &query,
            result_count,
            elapsed_ms,
            errored,
        ) {
            eprintln!("warning: telemetry record failed: {}", e);
        }
    }

    let results = query_result.context("SPARQL query failed")?;
    json_out(&serialize_results(&results))
}

/// Resolve the claims dir from the optional --data-dir flag,
/// falling back to nomograph_workflow's discover path. Shared by
/// open_view and the telemetry writer so both observe the same dir.
fn locate_claims_dir(data_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = data_dir {
        return Ok(p.join("claims"));
    }
    // Use the same discovery synthesist uses for its store.
    let store = crate::store::Store::discover().context("discover claims dir")?;
    Ok(store.root().to_path_buf().join("claims"))
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Return the SPARQL query string, reading from file if needed.
fn resolve_query(sparql: Option<&str>, file: Option<&Path>) -> Result<String> {
    match (sparql, file) {
        (Some(q), None) => Ok(q.to_string()),
        (None, Some(path)) => {
            std::fs::read_to_string(path)
                .with_context(|| format!("read SPARQL file {}", path.display()))
        }
        (Some(_), Some(_)) => bail!("--sparql and --file are mutually exclusive; pass one, not both"),
        (None, None) => bail!("one of --sparql or --file is required"),
    }
}

/// Locate the claims directory and open the graph view.
///
/// Strategy:
/// 1. Use `data_dir` (from `--data-dir`) joined with `claims/` if
///    provided.
/// 2. Otherwise, walk up from the current directory looking for a
///    `claims/` directory (mirrors how `Store::discover` works for
///    the SQL view).
/// 3. Try to open the on-disk RocksDB view at `claims/_view.oxigraph/`.
/// 4. On any failure, open an in-memory view and rebuild from the log
///    union.
fn open_view(data_dir: Option<&Path>) -> Result<GraphView> {
    let claims_dir = find_claims_dir(data_dir)?;
    let view_dir = claims_dir.join("_view.oxigraph");

    // Attempt on-disk view first; fall back to in-memory rebuild on any
    // error (including the macOS RocksDB TryFromIntError).
    match GraphView::open(&view_dir) {
        Ok(view) => Ok(view),
        Err(_) => {
            // On-disk open failed (RocksDB issue or missing dir).
            // Fall back: in-memory view rebuilt from the log union.
            let view = GraphView::open_in_memory()
                .context("open in-memory graph view")?;
            rebuild(&view, &claims_dir)
                .with_context(|| format!("rebuild view from claims at {}", claims_dir.display()))?;
            Ok(view)
        }
    }
}

/// Find the `claims/` directory starting from `data_dir` or by walking
/// up the filesystem from the current working directory.
fn find_claims_dir(data_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(base) = data_dir {
        let candidate = base.join("claims");
        if candidate.exists() {
            return Ok(candidate);
        }
        bail!(
            "no claims/ directory found at {} (from --data-dir)",
            base.display()
        );
    }

    // Walk up from cwd.
    let start = std::env::current_dir().context("get current directory")?;
    let mut cur = start.as_path();
    loop {
        let candidate = cur.join("claims");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => bail!(
                "no claims/ directory found walking up from {}",
                start.display()
            ),
        }
    }
}

/// Serialize a [`SelectResults`] to the synthesist JSON output shape.
///
/// Output:
/// ```json
/// {
///   "columns": ["type", "n"],
///   "rows": [
///     [{"iri": "https://..."}, {"literal": {"value": "10", "datatype": "..."}}],
///     ...
///   ],
///   "count": 1
/// }
/// ```
///
/// Each term in a row is one of:
/// - `{"iri": "<iri>"}` for an IRI.
/// - `{"blank_node": "<id>"}` for a blank node.
/// - `{"literal": {"value": "...", "datatype": "...", "language": "..."}}` for
///   a literal (datatype and language are omitted when null).
fn serialize_results(results: &SelectResults) -> Value {
    let rows: Vec<Value> = results
        .rows
        .iter()
        .map(|row| {
            let cells: Vec<Value> = row.iter().map(serialize_term).collect();
            Value::Array(cells)
        })
        .collect();

    json!({
        "columns": results.columns,
        "rows": rows,
        "count": results.rows.len(),
    })
}

fn serialize_term(term: &Term) -> Value {
    match term {
        Term::Iri(s) => json!({"iri": s}),
        Term::BlankNode(s) => json!({"blank_node": s}),
        Term::Literal { value, datatype, language } => {
            let mut obj = serde_json::Map::new();
            obj.insert("value".into(), json!(value));
            if let Some(dt) = datatype {
                obj.insert("datatype".into(), json!(dt));
            }
            if let Some(lang) = language {
                obj.insert("language".into(), json!(lang));
            }
            json!({"literal": Value::Object(obj)})
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nomograph_claim::graph_view::rebuild;
    use nomograph_claim::log::LogWriter;
    use serde_json::json;
    use tempfile::TempDir;

    // Helper: build a minimal synthesist:Task JSON-LD doc with an inline
    // context so Oxigraph can parse it without a network fetch.
    fn make_task_claim(id_suffix: &str, status: &str) -> serde_json::Value {
        json!({
            "@context": {
                "nomograph": "https://nomograph.org/v3/",
                "prov":      "http://www.w3.org/ns/prov#",
                "xsd":       "http://www.w3.org/2001/XMLSchema#",
                "synthesist": "https://nomograph.org/synthesist/",
                "prov:generatedAtTime": {"@type": "xsd:dateTime"},
                "prov:wasAttributedTo": {"@type": "@id"}
            },
            "@id": format!("synthesist:claim/{}", id_suffix),
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd",
            "synthesist:summary": format!("Task {}", id_suffix),
            "synthesist:status":  status,
        })
    }

    /// Build a populated test view: temp dir with LogWriter appends,
    /// then in-memory GraphView rebuilt from the log union.
    fn build_test_view(claim_count: usize) -> (TempDir, GraphView) {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        let statuses = ["pending", "in_progress", "done"];
        for i in 0..claim_count {
            let status = statuses[i % statuses.len()];
            let doc = make_task_claim(&format!("{:04}", i), status);
            writer.append("user:local:agd", &doc).unwrap();
        }
        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();
        (tmp, view)
    }

    // ------------------------------------------------------------------
    // Acceptance criterion 1: status-shape query on a populated view.
    // ------------------------------------------------------------------

    #[test]
    fn status_shape_query_returns_correct_type_counts() {
        // Build a view with 9 claims: 3 types x 3 statuses.
        let (_tmp, view) = build_test_view(9);

        // Status-shape query from the spike: count claims by type.
        let q = r#"
            PREFIX rdf:   <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
            SELECT ?type (COUNT(?c) AS ?n)
            WHERE { GRAPH ?g { ?c rdf:type ?type } }
            GROUP BY ?type
        "#;

        let results = select(&view, q).unwrap();
        assert_eq!(results.columns, vec!["type".to_string(), "n".to_string()]);
        assert_eq!(results.rows.len(), 1, "expected one type group (synthesist:Task)");

        // The count should equal the number of claims.
        let count_term = &results.rows[0][1];
        match count_term {
            Term::Literal { value, .. } => {
                assert_eq!(value, "9", "expected 9 claims, got {}", value);
            }
            other => panic!("expected Literal count, got {:?}", other),
        }
    }

    // ------------------------------------------------------------------
    // Acceptance criterion 1 (extended): serialize_results shape.
    // ------------------------------------------------------------------

    #[test]
    fn query_results_serialize_to_expected_shape() {
        let (_tmp, view) = build_test_view(3);

        let q = r#"
            PREFIX synthesist: <https://nomograph.org/synthesist/>
            SELECT ?c ?summary
            WHERE { GRAPH ?g { ?c synthesist:summary ?summary } }
            ORDER BY ?c
        "#;

        let results = select(&view, q).unwrap();
        let json_val = serialize_results(&results);

        // Top-level keys.
        assert!(json_val.get("columns").is_some());
        assert!(json_val.get("rows").is_some());
        assert_eq!(json_val["count"], 3);

        // Each row cell is either {iri:...} or {literal:{...}}.
        let rows = json_val["rows"].as_array().unwrap();
        for row in rows {
            let cells = row.as_array().unwrap();
            assert_eq!(cells.len(), 2);
            // First cell: IRI.
            assert!(cells[0].get("iri").is_some(), "expected iri cell: {:?}", cells[0]);
            // Second cell: literal (synthesist:summary).
            assert!(cells[1].get("literal").is_some(), "expected literal cell: {:?}", cells[1]);
        }
    }

    // ------------------------------------------------------------------
    // Acceptance criterion 2: invalid SPARQL produces a structured error.
    // ------------------------------------------------------------------

    #[test]
    fn invalid_sparql_produces_error() {
        let view = GraphView::open_in_memory().unwrap();
        let bad_query = "this is not valid SPARQL at all";
        let result = select(&view, bad_query);
        assert!(result.is_err(), "expected Err for invalid SPARQL");
        let msg = result.unwrap_err().to_string();
        // The error chain should mention evaluation or SPARQL context.
        assert!(
            msg.contains("SPARQL") || msg.contains("evaluate") || msg.contains("parse"),
            "error should reference SPARQL failure, got: {}",
            msg
        );
    }

    // ------------------------------------------------------------------
    // resolve_query: both modes.
    // ------------------------------------------------------------------

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

    // ------------------------------------------------------------------
    // serialize_term: each variant.
    // ------------------------------------------------------------------

    #[test]
    fn serialize_term_iri() {
        let t = Term::Iri("https://example.org/foo".into());
        let v = serialize_term(&t);
        assert_eq!(v, json!({"iri": "https://example.org/foo"}));
    }

    #[test]
    fn serialize_term_blank_node() {
        let t = Term::BlankNode("b0".into());
        let v = serialize_term(&t);
        assert_eq!(v, json!({"blank_node": "b0"}));
    }

    #[test]
    fn serialize_term_literal_with_datatype() {
        let t = Term::Literal {
            value: "42".into(),
            datatype: Some("http://www.w3.org/2001/XMLSchema#integer".into()),
            language: None,
        };
        let v = serialize_term(&t);
        assert_eq!(
            v,
            json!({"literal": {"value": "42", "datatype": "http://www.w3.org/2001/XMLSchema#integer"}})
        );
    }

    #[test]
    fn serialize_term_literal_with_language() {
        let t = Term::Literal {
            value: "hello".into(),
            datatype: None,
            language: Some("en".into()),
        };
        let v = serialize_term(&t);
        assert_eq!(v, json!({"literal": {"value": "hello", "language": "en"}}));
    }

    // ------------------------------------------------------------------
    // Empty view: query returns no rows.
    // ------------------------------------------------------------------

    #[test]
    fn query_on_empty_view_returns_zero_rows() {
        let view = GraphView::open_in_memory().unwrap();
        let q = "SELECT ?s WHERE { ?s ?p ?o }";
        let results = select(&view, q).unwrap();
        assert!(results.is_empty());
        let json_val = serialize_results(&results);
        assert_eq!(json_val["count"], 0);
        assert_eq!(json_val["rows"].as_array().unwrap().len(), 0);
    }
}
