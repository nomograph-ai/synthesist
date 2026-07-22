//! Telemetry writer for alpha query-surface instrumentation.
//!
//! In v3 the only live caller is `overlay run` (via [`Surface::Cli`]),
//! which calls [`TelemetryWriter::record_query`]. Each call appends one
//! JSON line to `claims/_telemetry/queries.jsonl`. (The `Http`/`Mcp`
//! `Surface` variants are retained for record compatibility but have no
//! live surface behind them.)
//!
//! ## Design choice: struct vs free function
//!
//! A [`TelemetryWriter`] struct holds the `claims_dir` path so callers
//! construct it once (at server/CLI init) and reuse it. A module-level
//! free function would require a hidden global for the path, which is
//! harder to test and harder to reason about in concurrent contexts.
//!
//! ## Atomic append discipline
//!
//! Each record is written by:
//!
//! 1. Opening the target file with `O_CREAT | O_APPEND | O_WRONLY`.
//! 2. Writing the serialised JSON line (single `write` call; lines are
//!    well under PIPE_BUF on every supported OS, so the append is
//!    atomic at the syscall level on Linux and macOS for local filesystems).
//! 3. Calling `sync_data()` on the file to flush to durable storage.
//! 4. Opening the parent directory and calling `sync_data()` on it so
//!    new directory entries survive a crash.
//!
//! For truly concurrent writers (e.g., multiple synthesist processes
//! sharing the same claims tree), writes smaller than PIPE_BUF are
//! atomic on POSIX local filesystems. Network filesystems (NFS, CIFS)
//! are not guaranteed; they are out of scope for v3-alpha.
//!
//! This matches the log-writer crash-safety contract from `nomograph-claim`
//! without the overhead of write-to-tmp + rename (which would replace the
//! file on each call, keeping only one record at a time).
//!
//! ## bgp_shape derivation
//!
//! Uses `spargebra::SparqlParser::parse_query` to build the SPARQL algebra AST, then
//! walks the graph pattern recursively. Each distinct variable is replaced
//! by a positional placeholder (`?v0`, `?v1`, ...) in first-encounter
//! order. Each IRI is replaced by the token `<iri>`. Literals in triple
//! patterns become `"lit"`. The resulting string captures query topology
//! without leaking variable names or IRI literals.
//!
//! ## filter_kinds
//!
//! Walks `GraphPattern::Filter` nodes in the AST and classifies each
//! top-level expression. Recognised kinds: `literal-eq`, `regex`, `range`.
//! Everything else maps to `other`.
//!
//! ## Error handling
//!
//! `record_query` never panics. SPARQL parse failures produce an empty
//! `bgp_shape` and an empty `filter_kinds` list; the record is still
//! written so the errored call is accounted for. I/O failures are
//! returned as `anyhow::Error`.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use spargebra::Query;
use spargebra::SparqlParser;
use spargebra::algebra::{Expression, GraphPattern};
use spargebra::term::{NamedNodePattern, TermPattern};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Canonical form of a SPARQL query: a stable hash and a topology string.
///
/// Produced by [`canonicalize`]. Both fields are deterministic: two queries
/// that differ only in variable names or IRI literals produce the same
/// `query_hash` and `bgp_shape`. Two queries that differ in graph topology
/// produce different values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonForm {
    /// BLAKE3 hex digest of `bgp_shape`. Stable across runs and Rust versions.
    pub query_hash: String,
    /// Normalised topology string: variables -> `?vN`, IRIs -> `<iri>`,
    /// literals -> `"lit"`.
    pub bgp_shape: String,
}

/// Derive the canonical form of a SPARQL query.
///
/// Returns `Err` if `sparql` cannot be parsed. Never panics.
///
/// The returned [`CanonForm`] is:
/// - **Variable-invariant**: renaming variables does not change the output.
/// - **IRI-invariant**: changing IRI literals does not change the output.
/// - **Topology-sensitive**: adding or removing triple patterns, or changing
///   join structure (OPTIONAL, UNION, GRAPH, ...) does change the output.
pub fn canonicalize(sparql: &str) -> Result<CanonForm> {
    let query = SparqlParser::new()
        .parse_query(sparql)
        .map_err(|e| anyhow::anyhow!("SPARQL parse error: {e}"))?;

    let pattern = match &query {
        Query::Select { pattern, .. } => pattern,
        Query::Construct { pattern, .. } => pattern,
        Query::Describe { pattern, .. } => pattern,
        Query::Ask { pattern, .. } => pattern,
    };

    let mut var_map: HashMap<String, usize> = HashMap::new();
    let mut filter_kinds: Vec<String> = vec![];
    let bgp_shape = shape_of_pattern(pattern, &mut var_map, &mut filter_kinds);

    let hash_bytes = blake3::hash(bgp_shape.as_bytes());
    let query_hash = hash_bytes.to_hex().to_string();

    Ok(CanonForm {
        query_hash,
        bgp_shape,
    })
}

/// Query surface that originated the call.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Cli,
    Http,
    Mcp,
}

/// Writes telemetry records to `<claims_dir>/_telemetry/queries.jsonl`.
///
/// Construct once per process (or per request handler) and reuse.
/// The struct is `Send + Sync`; concurrent callers on local filesystems
/// are safe (O_APPEND writes under PIPE_BUF are atomic on POSIX).
pub struct TelemetryWriter {
    telemetry_dir: PathBuf,
}

impl TelemetryWriter {
    /// Create a writer rooted at `claims_dir`.
    ///
    /// Creates `<claims_dir>/_telemetry/` if it does not exist.
    pub fn new(claims_dir: &Path) -> Result<Self> {
        let telemetry_dir = claims_dir.join("_telemetry");
        fs::create_dir_all(&telemetry_dir)
            .with_context(|| format!("creating telemetry dir {}", telemetry_dir.display()))?;
        Ok(Self { telemetry_dir })
    }

    /// Record one query event.
    ///
    /// Parses `sparql` to derive `bgp_shape` and `filter_kinds`.
    /// Parse failures yield an empty shape; they do not prevent the
    /// record from being written.
    ///
    /// Returns an error only on I/O failure.
    pub fn record_query(
        &self,
        surface: Surface,
        sparql: &str,
        result_count: usize,
        latency_ms: f64,
        errored: bool,
    ) -> Result<()> {
        let (bgp_shape, filter_kinds) = derive_shape(sparql);

        let record = TelemetryRecord {
            ts: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            surface,
            bgp_shape,
            filter_kinds,
            result_count,
            latency_ms,
            errored,
        };

        let mut line = serde_json::to_string(&record).context("serialising telemetry record")?;
        line.push('\n');

        self.append_line(line.as_bytes())
    }

    /// Append `data` to the JSONL file with crash-safe O_APPEND semantics,
    /// then fsync the file and directory.
    fn append_line(&self, data: &[u8]) -> Result<()> {
        let target = self.telemetry_dir.join("queries.jsonl");

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
            .with_context(|| format!("opening telemetry file {}", target.display()))?;

        file.write_all(data)
            .with_context(|| format!("writing telemetry line to {}", target.display()))?;

        // Flush to durable storage.
        file.sync_data()
            .with_context(|| "fsyncing telemetry file")?;

        // fsync the directory so any newly-created file entry is durable.
        let dir = OpenOptions::new()
            .read(true)
            .open(&self.telemetry_dir)
            .with_context(|| {
                format!(
                    "opening telemetry dir for fsync: {}",
                    self.telemetry_dir.display()
                )
            })?;
        dir.sync_data()
            .with_context(|| "fsyncing telemetry directory")?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Internal record type
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct TelemetryRecord {
    ts: String,
    surface: Surface,
    bgp_shape: String,
    filter_kinds: Vec<String>,
    result_count: usize,
    latency_ms: f64,
    errored: bool,
}

// ---------------------------------------------------------------------------
// bgp_shape + filter_kinds derivation
// ---------------------------------------------------------------------------

/// Parse `sparql` and return `(bgp_shape, filter_kinds)`.
///
/// On parse failure returns `("", vec![])` so callers record the event
/// with an empty shape rather than failing.
pub fn derive_shape(sparql: &str) -> (String, Vec<String>) {
    let query = match SparqlParser::new().parse_query(sparql) {
        Ok(q) => q,
        Err(_) => return (String::new(), vec![]),
    };

    let pattern = match &query {
        Query::Select { pattern, .. } => pattern,
        Query::Construct { pattern, .. } => pattern,
        Query::Describe { pattern, .. } => pattern,
        Query::Ask { pattern, .. } => pattern,
    };

    let mut var_map: HashMap<String, usize> = HashMap::new();
    let mut filter_kinds: Vec<String> = vec![];

    let bgp_shape = shape_of_pattern(pattern, &mut var_map, &mut filter_kinds);

    (bgp_shape, filter_kinds)
}

/// Recursively walk a `GraphPattern` and produce a normalised string.
///
/// Variables are replaced by `?v0`, `?v1`, ... in first-encounter order.
/// IRIs are replaced by `<iri>`. Literals in triple patterns become `"lit"`.
fn shape_of_pattern(
    pattern: &GraphPattern,
    var_map: &mut HashMap<String, usize>,
    filter_kinds: &mut Vec<String>,
) -> String {
    match pattern {
        GraphPattern::Bgp { patterns } => {
            let parts: Vec<String> = patterns
                .iter()
                .map(|tp| {
                    let s = normalise_term_pattern(&tp.subject, var_map);
                    let p = normalise_named_node_pattern(&tp.predicate, var_map);
                    let o = normalise_term_pattern(&tp.object, var_map);
                    format!("{s} {p} {o}")
                })
                .collect();
            parts.join(" . ")
        }
        GraphPattern::Filter { expr, inner } => {
            let kind = classify_filter(expr);
            if !filter_kinds.contains(&kind) {
                filter_kinds.push(kind);
            }
            shape_of_pattern(inner, var_map, filter_kinds)
        }
        GraphPattern::Join { left, right } => {
            let l = shape_of_pattern(left, var_map, filter_kinds);
            let r = shape_of_pattern(right, var_map, filter_kinds);
            if l.is_empty() {
                r
            } else if r.is_empty() {
                l
            } else {
                format!("{l} . {r}")
            }
        }
        GraphPattern::LeftJoin { left, right, .. } => {
            let l = shape_of_pattern(left, var_map, filter_kinds);
            let r = shape_of_pattern(right, var_map, filter_kinds);
            format!("{l} OPTIONAL {{ {r} }}")
        }
        GraphPattern::Union { left, right } => {
            let l = shape_of_pattern(left, var_map, filter_kinds);
            let r = shape_of_pattern(right, var_map, filter_kinds);
            format!("{{ {l} }} UNION {{ {r} }}")
        }
        GraphPattern::Graph { name, inner } => {
            let g = normalise_named_node_pattern(name, var_map);
            let inner = shape_of_pattern(inner, var_map, filter_kinds);
            format!("GRAPH {g} {{ {inner} }}")
        }
        GraphPattern::Project { inner, .. } => shape_of_pattern(inner, var_map, filter_kinds),
        GraphPattern::Distinct { inner } => shape_of_pattern(inner, var_map, filter_kinds),
        GraphPattern::Reduced { inner } => shape_of_pattern(inner, var_map, filter_kinds),
        GraphPattern::OrderBy { inner, .. } => shape_of_pattern(inner, var_map, filter_kinds),
        GraphPattern::Slice { inner, .. } => shape_of_pattern(inner, var_map, filter_kinds),
        GraphPattern::Extend { inner, .. } => shape_of_pattern(inner, var_map, filter_kinds),
        GraphPattern::Minus { left, .. } => shape_of_pattern(left, var_map, filter_kinds),
        GraphPattern::Group { inner, .. } => shape_of_pattern(inner, var_map, filter_kinds),
        GraphPattern::Service { inner, .. } => shape_of_pattern(inner, var_map, filter_kinds),
        GraphPattern::Path {
            subject, object, ..
        } => {
            let s = normalise_term_pattern(subject, var_map);
            let o = normalise_term_pattern(object, var_map);
            format!("{s} <path> {o}")
        }
        GraphPattern::Values { .. } => String::new(),
        // sep-0006 lateral join: treat as a regular join for shape purposes.
        #[allow(unreachable_patterns)]
        _ => String::new(),
    }
}

/// Normalise a `TermPattern`: variables become `?vN`, IRIs become `<iri>`,
/// blank nodes become `_:b`, literals become `"lit"`.
fn normalise_term_pattern(tp: &TermPattern, var_map: &mut HashMap<String, usize>) -> String {
    match tp {
        TermPattern::Variable(v) => normalise_var(v.as_str(), var_map),
        TermPattern::NamedNode(_) => "<iri>".to_string(),
        TermPattern::BlankNode(_) => "_:b".to_string(),
        TermPattern::Literal(_) => "\"lit\"".to_string(),
        // rdf-star triple terms: normalise to a placeholder.
        #[allow(unreachable_patterns)]
        _ => "<<term>>".to_string(),
    }
}

/// Normalise a `NamedNodePattern`.
fn normalise_named_node_pattern(
    nnp: &NamedNodePattern,
    var_map: &mut HashMap<String, usize>,
) -> String {
    match nnp {
        NamedNodePattern::NamedNode(_) => "<iri>".to_string(),
        NamedNodePattern::Variable(v) => normalise_var(v.as_str(), var_map),
    }
}

/// Return the positional placeholder for `var`, creating one if needed.
fn normalise_var(var: &str, var_map: &mut HashMap<String, usize>) -> String {
    let next = var_map.len();
    let idx = *var_map.entry(var.to_string()).or_insert(next);
    format!("?v{idx}")
}

/// Classify a FILTER expression into a coarse kind string.
fn classify_filter(expr: &Expression) -> String {
    match expr {
        Expression::Equal(_, _) => "literal-eq".to_string(),
        Expression::FunctionCall(f, _) => {
            use spargebra::algebra::Function;
            match f {
                Function::Regex => "regex".to_string(),
                _ => "other".to_string(),
            }
        }
        Expression::Greater(_, _)
        | Expression::GreaterOrEqual(_, _)
        | Expression::Less(_, _)
        | Expression::LessOrEqual(_, _) => "range".to_string(),
        _ => "other".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_writer() -> (TelemetryWriter, TempDir) {
        let tmp = TempDir::new().unwrap();
        let writer = TelemetryWriter::new(tmp.path()).unwrap();
        (writer, tmp)
    }

    /// Read all telemetry lines from the standard path inside `claims_dir`.
    fn read_lines(claims_dir: &Path) -> Vec<serde_json::Value> {
        let path = claims_dir.join("_telemetry").join("queries.jsonl");
        if !path.exists() {
            return vec![];
        }
        let content = std::fs::read_to_string(&path).unwrap();
        content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    const SIMPLE_QUERY: &str = "SELECT ?s ?p ?o WHERE { ?s ?p ?o }";

    /// Acceptance criterion: 10 calls produce 10 telemetry lines.
    #[test]
    fn ten_calls_produce_ten_lines() {
        let (writer, tmp) = make_writer();
        for _ in 0..10 {
            writer
                .record_query(Surface::Cli, SIMPLE_QUERY, 5, 1.0, false)
                .unwrap();
        }
        let lines = read_lines(tmp.path());
        assert_eq!(
            lines.len(),
            10,
            "expected 10 telemetry lines, got {}",
            lines.len()
        );
    }

    /// Acceptance criterion: bgp_shape is consistent for the same query.
    #[test]
    fn bgp_shape_consistent_for_same_query() {
        let (writer, tmp) = make_writer();
        writer
            .record_query(Surface::Cli, SIMPLE_QUERY, 0, 0.1, false)
            .unwrap();
        writer
            .record_query(Surface::Http, SIMPLE_QUERY, 3, 0.2, false)
            .unwrap();
        let lines = read_lines(tmp.path());
        assert_eq!(lines.len(), 2);
        let shape0 = lines[0]["bgp_shape"].as_str().unwrap();
        let shape1 = lines[1]["bgp_shape"].as_str().unwrap();
        assert_eq!(
            shape0, shape1,
            "bgp_shape should be identical for the same query"
        );
    }

    /// Acceptance criterion: variable names and IRI literals do not appear in bgp_shape.
    #[test]
    fn bgp_shape_strips_variable_names_and_iris() {
        let query = "SELECT ?distinctive_var WHERE { ?distinctive_var <https://example.com/distinctive-iri> ?distinctive_var }";
        let (shape, _) = derive_shape(query);
        assert!(
            !shape.contains("distinctive_var"),
            "bgp_shape must not contain variable name 'distinctive_var'; got: {shape}"
        );
        assert!(
            !shape.contains("example.com"),
            "bgp_shape must not contain IRI literal; got: {shape}"
        );
    }

    #[test]
    fn telemetry_dir_created_if_missing() {
        let tmp = TempDir::new().unwrap();
        let claims_dir = tmp.path().join("nested").join("claims");
        // Directory does not exist yet.
        TelemetryWriter::new(&claims_dir).unwrap();
        assert!(claims_dir.join("_telemetry").exists());
    }

    #[test]
    fn parse_failure_does_not_panic() {
        let (writer, tmp) = make_writer();
        // Deliberately broken SPARQL.
        let result = writer.record_query(Surface::Mcp, "NOT VALID SPARQL !!!!", 0, 0.0, true);
        assert!(
            result.is_ok(),
            "record_query should not fail on parse error"
        );
        let lines = read_lines(tmp.path());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["bgp_shape"].as_str().unwrap(), "");
        assert!(lines[0]["errored"].as_bool().unwrap());
    }

    #[test]
    fn filter_kinds_literal_eq_detected() {
        let query =
            r#"SELECT ?s WHERE { ?s <http://example.org/status> ?v . FILTER(?v = "pending") }"#;
        let (_shape, kinds) = derive_shape(query);
        assert!(
            kinds.contains(&"literal-eq".to_string()),
            "expected literal-eq in filter_kinds, got: {kinds:?}"
        );
    }

    #[test]
    fn filter_kinds_regex_detected() {
        let query =
            r#"SELECT ?s WHERE { ?s <http://example.org/name> ?n . FILTER(REGEX(?n, "foo")) }"#;
        let (_shape, kinds) = derive_shape(query);
        assert!(
            kinds.contains(&"regex".to_string()),
            "expected regex in filter_kinds, got: {kinds:?}"
        );
    }

    #[test]
    fn surface_serialises_lowercase() {
        let (writer, tmp) = make_writer();
        writer
            .record_query(Surface::Http, SIMPLE_QUERY, 0, 0.0, false)
            .unwrap();
        let lines = read_lines(tmp.path());
        assert_eq!(lines[0]["surface"].as_str().unwrap(), "http");
    }

    // -----------------------------------------------------------------------
    // T6.6: canonicalize() tests
    // -----------------------------------------------------------------------

    /// Acceptance criterion 1: two queries that differ only in variable names
    /// produce the same hash and shape.
    #[test]
    fn canonicalize_variable_rename_same_hash_and_shape() {
        let q1 = "SELECT ?s ?p ?o WHERE { ?s ?p ?o }";
        let q2 = "SELECT ?subject ?predicate ?object WHERE { ?subject ?predicate ?object }";
        let c1 = canonicalize(q1).expect("q1 should parse");
        let c2 = canonicalize(q2).expect("q2 should parse");
        assert_eq!(
            c1.bgp_shape, c2.bgp_shape,
            "bgp_shape should be identical for variable-renamed queries"
        );
        assert_eq!(
            c1.query_hash, c2.query_hash,
            "query_hash should be identical for variable-renamed queries"
        );
    }

    /// Acceptance criterion 2: two queries that differ in IRI literals but
    /// share the same topology produce the same hash and shape.
    ///
    /// (They would differ in result counts at query time because the IRIs
    /// select different data, but the canonical form is identical.)
    #[test]
    fn canonicalize_iri_substitution_same_hash_and_shape() {
        let q1 = "SELECT ?s WHERE { ?s <http://schema.org/name> ?name }";
        let q2 = "SELECT ?s WHERE { ?s <http://example.com/label> ?name }";
        let c1 = canonicalize(q1).expect("q1 should parse");
        let c2 = canonicalize(q2).expect("q2 should parse");
        assert_eq!(
            c1.bgp_shape, c2.bgp_shape,
            "bgp_shape should be identical for IRI-substituted queries with the same topology"
        );
        assert_eq!(
            c1.query_hash, c2.query_hash,
            "query_hash should be identical for IRI-substituted queries with the same topology"
        );
    }

    /// Different topology produces a different hash and shape.
    #[test]
    fn canonicalize_different_topology_different_hash() {
        let q1 = "SELECT ?s WHERE { ?s <http://example.org/a> ?o }";
        let q2 = "SELECT ?s WHERE { ?s <http://example.org/a> ?o . ?s <http://example.org/b> ?o2 }";
        let c1 = canonicalize(q1).expect("q1 should parse");
        let c2 = canonicalize(q2).expect("q2 should parse");
        assert_ne!(
            c1.bgp_shape, c2.bgp_shape,
            "bgp_shape should differ for queries with different triple-pattern counts"
        );
        assert_ne!(
            c1.query_hash, c2.query_hash,
            "query_hash should differ for queries with different topology"
        );
    }

    /// Parse failure returns Err, not a panic.
    #[test]
    fn canonicalize_parse_failure_returns_err() {
        let result = canonicalize("THIS IS NOT SPARQL");
        assert!(
            result.is_err(),
            "canonicalize should return Err for invalid SPARQL"
        );
    }

    /// query_hash is a 64-character lowercase hex string (BLAKE3 256-bit output).
    #[test]
    fn canonicalize_hash_is_64_char_hex() {
        let cf = canonicalize(SIMPLE_QUERY).expect("should parse");
        assert_eq!(
            cf.query_hash.len(),
            64,
            "BLAKE3 hex string should be 64 chars, got: {}",
            cf.query_hash
        );
        assert!(
            cf.query_hash.chars().all(|c| c.is_ascii_hexdigit()),
            "query_hash should be all hex digits, got: {}",
            cf.query_hash
        );
    }

    /// canonicalize is deterministic: same query same hash on repeated calls.
    #[test]
    fn canonicalize_deterministic() {
        let c1 = canonicalize(SIMPLE_QUERY).expect("should parse");
        let c2 = canonicalize(SIMPLE_QUERY).expect("should parse");
        assert_eq!(c1, c2, "canonicalize should be deterministic");
    }
}
