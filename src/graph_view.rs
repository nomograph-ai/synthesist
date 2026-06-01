//! Oxigraph-backed graph view of the claim log.
//!
//! The graph view is a derived projection of the per-asserter JSON-LD
//! logs into an RDF triple store. It is gitignored; it can always be
//! rebuilt from the log union. Callers query the view via SPARQL.
//!
//! ## Lifecycle
//!
//! - Open with [`GraphView::open`] for a RocksDB-backed view at
//!   `claims/_view.oxigraph/`. The store persists across runs;
//!   re-opening is fast.
//! - Open with [`GraphView::open_in_memory`] for ephemeral test use.
//! - Open with [`open_or_in_memory`] for the recommended production
//!   path: try on-disk first, fall back to an in-memory rebuild.
//!
//! ## macOS ARM workaround
//!
//! `oxigraph 0.4.11`'s `Store::open` panics with `TryFromIntError`
//! inside `rocksdb_wrapper.rs` on macOS ARM during RocksDB
//! initialization. [`open_or_in_memory`] wraps the call in
//! `std::panic::catch_unwind` so the in-memory fallback engages
//! silently. Callers on macOS ARM should install a custom panic hook
//! (see `synthesist/src/main.rs::install_panic_hook`) to suppress
//! the panic message that the default hook prints to stderr BEFORE
//! `catch_unwind` intercepts.
//!
//! ## Rebuild
//!
//! Population is the job of T2.2 (view rebuild). This module
//! provides only the open/close lifecycle. Once T2.2 lands, callers
//! will do:
//!
//! ```ignore
//! let view = GraphView::open(claims_dir.join("_view.oxigraph"))?;
//! rebuild(&view, &claims_dir)?;
//! ```
//!
//! ## Backing store
//!
//! Oxigraph's RocksDB backend creates many small files under the
//! view directory. None of them should be committed; the substrate
//! convention is to gitignore `_view.oxigraph/` at the project root.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use oxigraph::io::{JsonLdProfileSet, RdfFormat};
use oxigraph::store::Store;
use serde_json::Value;

use crate::jsonld;
use crate::log::LogReader;

/// A graph view backed by an Oxigraph store.
///
/// On-disk variants hold a RocksDB-backed store; in-memory variants
/// hold an in-process store that does not touch disk. Both share the
/// same SPARQL surface.
pub struct GraphView {
    store: Store,
    view_dir: Option<PathBuf>,
}

impl GraphView {
    /// Open or create an on-disk graph view at `view_dir`.
    ///
    /// The directory is created if absent. Re-opening an existing view
    /// is fast: Oxigraph reuses the existing RocksDB.
    pub fn open(view_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(view_dir)
            .with_context(|| format!("create view dir {}", view_dir.display()))?;
        let store = Store::open(view_dir)
            .with_context(|| format!("open oxigraph store at {}", view_dir.display()))?;
        Ok(Self {
            store,
            view_dir: Some(view_dir.to_path_buf()),
        })
    }

    /// Open an in-memory graph view.
    ///
    /// Nothing is written to disk; the view is dropped when the
    /// `GraphView` is dropped.
    pub fn open_in_memory() -> Result<Self> {
        let store = Store::new().context("create in-memory oxigraph store")?;
        Ok(Self {
            store,
            view_dir: None,
        })
    }

    /// Open a populated, queryable graph view of the claim log.
    ///
    /// This is the recommended production path. It always returns a view
    /// rebuilt from the `claims_dir` log union (the source of truth),
    /// using a **view-cache snapshot** to amortize the rebuild cost
    /// across CLI invocations. On the fast path: if
    /// `claims_dir/_view.snapshot.nq` and `claims_dir/_view.heads.json`
    /// exist and the recorded heads match the current log union, the
    /// snapshot is loaded directly (no rebuild). On the slow path: the
    /// full rebuild runs and the resulting store is serialized as
    /// N-Quads alongside a heads record so the next invocation hits the
    /// fast path. The heads signal (per-asserter log line counts, read
    /// fresh each call) makes the cache correct across process
    /// boundaries -- every CLI command is a fresh process.
    ///
    /// ## Why not the on-disk RocksDB store
    ///
    /// [`GraphView::open`] only opens an empty RocksDB store; it never
    /// ingests the logs (population is the job of [`rebuild`], which the
    /// in-memory cache path runs). On machines where `Store::open`
    /// succeeds it would therefore return a silently EMPTY view, and on
    /// macOS ARM it panics with `TryFromIntError`. Both failure modes
    /// are avoided by routing through the snapshot-cached in-memory
    /// rebuild unconditionally. The on-disk Oxigraph backend is slated
    /// for removal in the 3.0.0 gamma engine; until then it is bypassed.
    ///
    /// `view_dir` is retained for signature compatibility with callers
    /// (e.g. `claims/_view.oxigraph`) but is unused. `claims_dir` is the
    /// per-asserter log root the view is rebuilt from.
    pub fn open_or_in_memory(view_dir: &Path, claims_dir: &Path) -> anyhow::Result<GraphView> {
        let _ = view_dir;
        open_in_memory_with_cache(claims_dir)
    }

    /// Borrow the underlying Oxigraph store for direct API access.
    ///
    /// Most callers should use the higher-level query and load helpers
    /// (provided by T2.2 and T2.3). This accessor is the escape hatch.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Path to the view directory, if this view is on-disk.
    pub fn view_dir(&self) -> Option<&Path> {
        self.view_dir.as_deref()
    }

    /// Return true if this view is in-memory.
    pub fn is_in_memory(&self) -> bool {
        self.view_dir.is_none()
    }

    /// Clear all triples from the view.
    ///
    /// Used by [`rebuild`] before re-ingesting the log union. Direct
    /// callers should usually prefer `rebuild` rather than calling
    /// this themselves.
    pub fn clear(&self) -> Result<()> {
        self.store.clear().context("clear oxigraph store")?;
        Ok(())
    }

    /// Return the named graphs currently present in the store.
    ///
    /// Result is a sorted, deduplicated list of named graph IRIs.
    /// The default graph (if it has triples) is not included; callers
    /// query it implicitly. Used to detect which modules have
    /// contributed claims and to drive cross-graph overlay queries.
    pub fn modules_in_view(&self) -> Result<Vec<String>> {
        use oxigraph::sparql::QueryResults;
        let q = "SELECT DISTINCT ?g WHERE { GRAPH ?g { ?s ?p ?o } } ORDER BY ?g";
        let results = self
            .store
            .query(q)
            .context("query named graphs")?;
        let mut graphs = Vec::new();
        if let QueryResults::Solutions(sols) = results {
            for sol in sols {
                let sol = sol?;
                if let Some(term) = sol.get("g") {
                    let s = term.to_string();
                    // Strip leading < and trailing >.
                    let cleaned = s
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                        .to_string();
                    graphs.push(cleaned);
                }
            }
        }
        Ok(graphs)
    }

    /// Quick triple count via SPARQL across all graphs (default plus
    /// named). Used by tests; production code will go through a richer
    /// query API.
    pub fn triple_count(&self) -> Result<usize> {
        use oxigraph::sparql::QueryResults;
        let q = r#"
            SELECT (COUNT(*) AS ?n) WHERE {
              { ?s ?p ?o } UNION { GRAPH ?g { ?s ?p ?o } }
            }
        "#;
        let results = self.store.query(q).context("count query")?;
        if let QueryResults::Solutions(mut sols) = results {
            if let Some(sol) = sols.next() {
                let sol = sol?;
                if let Some(term) = sol.get("n") {
                    let s = term.to_string();
                    // The term's display form is `"NNN"^^xsd:integer`.
                    if let Some(start) = s.find('"') {
                        if let Some(end) = s[start + 1..].find('"') {
                            return Ok(s[start + 1..start + 1 + end]
                                .parse()
                                .unwrap_or(0));
                        }
                    }
                }
            }
        }
        Ok(0)
    }
}

//
// Query: SPARQL SELECT and ASK against the view.
//

/// A single term in a SPARQL result row.
///
/// IRIs and blank nodes carry their string form; literals carry the
/// lexical value, an optional datatype IRI, and an optional language
/// tag. The shape is deliberately substrate-agnostic so callers can
/// match against it without coupling to Oxigraph types directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Iri(String),
    BlankNode(String),
    Literal {
        value: String,
        datatype: Option<String>,
        language: Option<String>,
    },
}

impl Term {
    /// Return the underlying string for IRI or BlankNode variants, or
    /// the literal value for Literal. Returns an empty string for
    /// terms that have no useful string projection.
    pub fn as_str(&self) -> &str {
        match self {
            Term::Iri(s) => s,
            Term::BlankNode(s) => s,
            Term::Literal { value, .. } => value,
        }
    }
}

/// Result of a SPARQL SELECT query.
///
/// `columns` lists the variable names in the order the query
/// projected them. `rows` is a flat list of bindings; each binding is
/// a vector of [`Term`] aligned with `columns`. A column with no
/// binding in a particular row appears as a `Term::Literal` with an
/// empty value (since SPARQL allows unbound vars but our flat shape
/// is easier to handle this way).
#[derive(Debug, Clone)]
pub struct SelectResults {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Term>>,
}

impl SelectResults {
    /// Return the number of rows in the result.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Return true if the result has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Run a SPARQL SELECT query against the view.
///
/// Returns a [`SelectResults`] with one row per solution.
pub fn select(view: &GraphView, query: &str) -> Result<SelectResults> {
    use oxigraph::sparql::QueryResults;
    let results = view
        .store
        .query(query)
        .context("evaluate SPARQL query")?;
    match results {
        QueryResults::Solutions(sols) => {
            let columns: Vec<String> = sols
                .variables()
                .iter()
                .map(|v| v.as_str().to_string())
                .collect();
            let mut rows = Vec::new();
            for sol in sols {
                let sol = sol.context("read solution")?;
                let mut row = Vec::with_capacity(columns.len());
                for col in &columns {
                    let term = sol
                        .iter()
                        .find(|(v, _)| v.as_str() == col)
                        .map(|(_, t)| convert_term(t))
                        .unwrap_or_else(|| Term::Literal {
                            value: String::new(),
                            datatype: None,
                            language: None,
                        });
                    row.push(term);
                }
                rows.push(row);
            }
            Ok(SelectResults { columns, rows })
        }
        QueryResults::Boolean(_) => Err(anyhow::anyhow!(
            "expected SELECT result, got ASK boolean; use ask() for ASK queries"
        )),
        QueryResults::Graph(_) => Err(anyhow::anyhow!(
            "expected SELECT result, got CONSTRUCT/DESCRIBE graph"
        )),
    }
}

/// Run a SPARQL ASK query against the view.
///
/// Returns true if the query has at least one solution.
pub fn ask(view: &GraphView, query: &str) -> Result<bool> {
    use oxigraph::sparql::QueryResults;
    let results = view
        .store
        .query(query)
        .context("evaluate SPARQL query")?;
    match results {
        QueryResults::Boolean(b) => Ok(b),
        QueryResults::Solutions(_) => Err(anyhow::anyhow!(
            "expected ASK result, got SELECT solutions; use select() for SELECT queries"
        )),
        QueryResults::Graph(_) => Err(anyhow::anyhow!(
            "expected ASK result, got CONSTRUCT/DESCRIBE graph"
        )),
    }
}

fn convert_term(t: &oxigraph::model::Term) -> Term {
    use oxigraph::model::Term as OxTerm;
    match t {
        OxTerm::NamedNode(n) => Term::Iri(n.as_str().to_string()),
        OxTerm::BlankNode(b) => Term::BlankNode(b.as_str().to_string()),
        OxTerm::Literal(l) => Term::Literal {
            value: l.value().to_string(),
            datatype: Some(l.datatype().as_str().to_string()),
            language: l.language().map(|s| s.to_string()),
        },
        OxTerm::Triple(_) => Term::Literal {
            value: format!("{:?}", t),
            datatype: None,
            language: None,
        },
    }
}

//
// Rebuild: ingest the claim log union into the graph view.
//

/// Result of a [`rebuild`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebuildStats {
    /// Number of claim documents loaded into the store.
    pub claims_loaded: usize,
    /// Number of triples in the store after the rebuild.
    pub triples_count: u64,
    /// Wall-clock duration of the rebuild in milliseconds.
    pub duration_ms: u64,
    /// Number of claim lines that failed to parse as JSON-LD.
    /// Such lines are skipped; the rebuild proceeds with the rest.
    pub parse_failures: usize,
}

/// Base IRI for per-module named graphs.
///
/// A claim with `@type: synthesist:Task` lands in the named graph
/// `<https://nomograph.org/graphs/synthesist>`. The prefix segment is
/// taken from the `@type` value's first compact-prefix component.
const NAMED_GRAPH_BASE: &str = "https://nomograph.org/graphs/";

/// Construct the named graph IRI for a module prefix.
///
/// Example: `module_graph_iri("synthesist")` returns
/// `https://nomograph.org/graphs/synthesist`.
pub fn module_graph_iri(module_prefix: &str) -> String {
    format!("{}{}", NAMED_GRAPH_BASE, module_prefix)
}

/// Extract the module prefix from a claim's `@type` value.
///
/// For compact form like `synthesist:Task`, returns `Some("synthesist")`. For
/// full IRI form, attempts to identify the module by URI pattern
/// (matches `https://nomograph.org/<module>/`); returns the module
/// segment if so, `None` otherwise.
fn extract_module_prefix(type_value: &str) -> Option<String> {
    if let Some(colon_idx) = type_value.find(':') {
        let prefix = &type_value[..colon_idx];
        // Filter out http(s) URIs by checking that the prefix does
        // not look like a scheme.
        if !type_value[colon_idx + 1..].starts_with("//") {
            return Some(prefix.to_string());
        }
        // Full URI form: match against the nomograph namespace.
        if let Some(rest) = type_value.strip_prefix("https://nomograph.org/") {
            if let Some(slash) = rest.find('/') {
                return Some(rest[..slash].to_string());
            }
        }
    }
    None
}

/// Rebuild the graph view from the claim log union under `claims_dir`.
///
/// Walks every per-asserter log via [`LogReader::iter_claims`], parses
/// each JSON-LD document, detects its module prefix from `@type`, and
/// inserts the resulting triples into the corresponding named graph
/// (`https://nomograph.org/graphs/<prefix>`). Claims with no
/// detectable module prefix land in the default graph.
///
/// The view is cleared first so the resulting state matches the
/// log union exactly.
///
/// Parse failures (malformed lines, JSON-LD decode errors) are
/// tolerated: the offending claim is counted in `parse_failures` and
/// iteration continues. This matches the substrate's append-only
/// posture; a broken line on disk does not break the whole rebuild.
///
/// The @context URI in each claim is replaced with the inline
/// [`jsonld::base_context_inner`] before parsing. This avoids any
/// network fetch and works offline.
pub fn rebuild(view: &GraphView, claims_dir: &Path) -> Result<RebuildStats> {
    let start = Instant::now();
    view.clear()?;

    let reader = LogReader::new(claims_dir)?;
    let inline_context = jsonld::base_context_inner();

    let mut claims_loaded = 0usize;
    let mut parse_failures = 0usize;

    for item in reader.iter_claims() {
        let claim = match item {
            Ok(c) => c,
            Err(_) => {
                parse_failures += 1;
                continue;
            }
        };

        // Detect module prefix from @type for named graph routing.
        let module_prefix = claim
            .raw
            .get("@type")
            .and_then(|v| v.as_str())
            .and_then(extract_module_prefix);

        // Inject the inline @context so Oxigraph does not need to
        // resolve the URI form over the network.
        let doc_with_context = inject_inline_context(&claim.raw, &inline_context);
        let bytes = serde_json::to_vec(&doc_with_context)
            .context("re-serialize claim doc")?;

        let load_result = if let Some(prefix) = module_prefix {
            let graph_iri = module_graph_iri(&prefix);
            // Parse into a temporary graph by loading with a
            // target-graph hint. Oxigraph's load_from_reader accepts
            // a target graph via the higher-level API; we use
            // bulk_loader for explicit named-graph placement.
            load_into_named_graph(&view.store, &graph_iri, &bytes)
        } else {
            view.store
                .load_from_reader(
                    RdfFormat::JsonLd {
                        profile: JsonLdProfileSet::empty(),
                    },
                    bytes.as_slice(),
                )
                .map_err(anyhow::Error::from)
        };

        match load_result {
            Ok(_) => claims_loaded += 1,
            Err(_) => parse_failures += 1,
        }
    }

    let triples_count = view.triple_count()? as u64;
    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(RebuildStats {
        claims_loaded,
        triples_count,
        duration_ms,
        parse_failures,
    })
}

/// Load a JSON-LD doc's triples into a specific named graph.
///
/// Oxigraph's `load_from_reader` lands triples in the default graph.
/// To route into a named graph, we first parse the doc into an
/// in-memory transient store, then copy the triples into the target
/// store under the named graph IRI.
fn load_into_named_graph(
    store: &oxigraph::store::Store,
    graph_iri: &str,
    bytes: &[u8],
) -> Result<()> {
    use oxigraph::model::{GraphName, NamedNode, Quad};

    let scratch = oxigraph::store::Store::new()
        .context("create scratch store for named-graph routing")?;
    scratch
        .load_from_reader(
            RdfFormat::JsonLd {
                profile: JsonLdProfileSet::empty(),
            },
            bytes,
        )
        .context("parse JSON-LD into scratch store")?;

    let target_graph: GraphName = NamedNode::new(graph_iri)
        .context("build named-graph IRI")?
        .into();

    for quad in scratch.iter() {
        let q = quad.context("read scratch quad")?;
        let routed = Quad::new(q.subject, q.predicate, q.object, target_graph.clone());
        store
            .insert(&routed)
            .context("insert into named graph")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// View-cache snapshot helpers (C.2)
// ---------------------------------------------------------------------------

/// Snapshot file name for the serialized in-memory store (N-Quads).
const SNAPSHOT_FILE: &str = "_view.snapshot.nq";

/// Heads record file name (plain text, one line containing the blake3 hash).
const SNAPSHOT_HEADS_FILE: &str = "_view.heads.json";

/// Open an in-memory store, using the snapshot cache if the heads match.
///
/// Fast path (heads match): loads `claims_dir/_view.snapshot.nq` directly.
/// Slow path (heads stale or missing): runs the full rebuild, then writes
/// the snapshot and heads record so the next call is fast.
///
/// Snapshot corruption is tolerated: on any parse error the function
/// falls through to the full rebuild rather than propagating the error.
///
/// Snapshot writes are atomic-ish: the data is written to a `.tmp`
/// sibling first, then renamed over the target so a crash mid-write
/// does not leave a half-written snapshot.
fn open_in_memory_with_cache(claims_dir: &Path) -> anyhow::Result<GraphView> {
    use crate::heads;
    use std::fs;

    let snapshot_path = claims_dir.join(SNAPSHOT_FILE);
    let heads_path = claims_dir.join(SNAPSHOT_HEADS_FILE);

    // Compute current heads once -- cheap (line counts only).
    let current = heads::current_heads(claims_dir)
        .context("compute current heads for cache check")?;

    // Try fast path: both files present AND heads match.
    if snapshot_path.exists() && heads_path.exists() {
        let stored = fs::read_to_string(&heads_path)
            .ok()
            .map(|s| s.trim().to_string());
        if stored.as_deref() == Some(current.as_str()) {
            // Attempt to load from snapshot. On any failure, fall through.
            match load_snapshot(&snapshot_path) {
                Ok(view) => return Ok(view),
                Err(_) => {
                    // Corrupted or unreadable snapshot; proceed to rebuild.
                }
            }
        }
    }

    // Slow path: full rebuild.
    let view = GraphView::open_in_memory().context("open in-memory graph view")?;
    rebuild(&view, claims_dir)
        .with_context(|| format!("rebuild view from claims at {}", claims_dir.display()))?;

    // Write snapshot + heads atomically. Failures are non-fatal; the
    // caller receives a valid (just-rebuilt) view regardless.
    let _ = write_snapshot(&view, claims_dir, &current);

    Ok(view)
}

/// Load an N-Quads snapshot file into a fresh in-memory store.
fn load_snapshot(snapshot_path: &std::path::Path) -> anyhow::Result<GraphView> {
    let file = std::fs::File::open(snapshot_path)
        .with_context(|| format!("open snapshot {}", snapshot_path.display()))?;
    let store = Store::new().context("create in-memory store for snapshot load")?;
    store
        .load_from_reader(RdfFormat::NQuads, std::io::BufReader::new(file))
        .with_context(|| format!("parse snapshot {}", snapshot_path.display()))?;
    Ok(GraphView {
        store,
        view_dir: None,
    })
}

/// Serialize the store to N-Quads and write the snapshot + heads files.
///
/// Uses a .tmp sibling + rename for atomicity.
fn write_snapshot(
    view: &GraphView,
    claims_dir: &std::path::Path,
    heads_hash: &str,
) -> anyhow::Result<()> {
    let snapshot_path = claims_dir.join(SNAPSHOT_FILE);
    let heads_path = claims_dir.join(SNAPSHOT_HEADS_FILE);

    let snapshot_tmp = claims_dir.join(format!("{}.tmp", SNAPSHOT_FILE));
    let heads_tmp = claims_dir.join(format!("{}.tmp", SNAPSHOT_HEADS_FILE));

    // Write snapshot to tmp.
    let writer = std::fs::File::create(&snapshot_tmp)
        .with_context(|| format!("create snapshot tmp {}", snapshot_tmp.display()))?;
    view.store
        .dump_to_writer(RdfFormat::NQuads, writer)
        .context("dump store to N-Quads")?;

    // Write heads to tmp.
    std::fs::write(&heads_tmp, heads_hash)
        .with_context(|| format!("write heads tmp {}", heads_tmp.display()))?;

    // Atomic rename both files.
    std::fs::rename(&snapshot_tmp, &snapshot_path)
        .with_context(|| format!("rename snapshot tmp to {}", snapshot_path.display()))?;
    std::fs::rename(&heads_tmp, &heads_path)
        .with_context(|| format!("rename heads tmp to {}", heads_path.display()))?;

    Ok(())
}

/// Replace a URI-form @context with the inline base context.
///
/// Doc-shape rules:
/// - No @context: insert the inline base.
/// - @context is a string (URI form): replace with the inline base.
/// - @context is an object or array (already inline): leave alone.
///   The doc author has declared its own prefixes; do not override.
fn inject_inline_context(doc: &Value, inline_context: &Value) -> Value {
    let mut clone = doc.clone();
    if let Value::Object(ref mut map) = clone {
        match map.get("@context") {
            None => {
                map.insert("@context".into(), inline_context.clone());
            }
            Some(Value::String(_)) => {
                map.insert("@context".into(), inline_context.clone());
            }
            Some(_) => {
                // Inline form already present; respect the doc author.
            }
        }
    }
    clone
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::LogWriter;
    use serde_json::json;
    use tempfile::TempDir;

    // The on-disk RocksDB-backed Store::open path hits a
    // TryFromIntError inside oxigraph 0.4.11's rocksdb_wrapper.rs:359
    // on macOS in our test environment. The architectural shape is
    // correct (open accepts a path, creates the dir, returns a Store);
    // the failure appears to be inside oxrocksdb-sys at runtime.
    //
    // Marked ignored until we can investigate. Candidates to check:
    //   - oxigraph version pin (0.4.11 vs latest)
    //   - macOS-specific RocksDB build flags via OXIROCKSDB_*
    //   - test environment vs production binary (release profile may behave
    //     differently)
    //   - file path content (TempDir paths under /var/folders may be the
    //     trigger; try /tmp directly)
    //
    // For v3-alpha thesis validation the in-memory path is sufficient.
    // T2.2 (view rebuild) will land with in-memory-store tests; T2.5
    // (heads file) will revisit on-disk persistence once we have the
    // investigation results.
    #[test]
    #[ignore]
    fn open_on_disk_view_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let view_dir = tmp.path().join("_view.oxigraph");
        assert!(!view_dir.exists());

        let view = GraphView::open(&view_dir).unwrap();
        assert!(view_dir.exists());
        assert_eq!(view.view_dir(), Some(view_dir.as_path()));
        assert!(!view.is_in_memory());
    }

    #[test]
    #[ignore]
    fn open_on_disk_view_then_close_then_reopen_persists() {
        let tmp = TempDir::new().unwrap();
        let view_dir = tmp.path().join("_view.oxigraph");

        {
            let _view = GraphView::open(&view_dir).unwrap();
            // Drop closes the view.
        }

        assert!(view_dir.exists());

        // Reopen successfully.
        let view = GraphView::open(&view_dir).unwrap();
        assert_eq!(view.triple_count().unwrap(), 0);
    }

    #[test]
    fn open_in_memory_does_not_touch_disk() {
        let view = GraphView::open_in_memory().unwrap();
        assert_eq!(view.view_dir(), None);
        assert!(view.is_in_memory());
        assert_eq!(view.triple_count().unwrap(), 0);
    }

    #[test]
    fn in_memory_view_supports_basic_query() {
        let view = GraphView::open_in_memory().unwrap();
        assert_eq!(view.triple_count().unwrap(), 0);
    }

    //
    // Rebuild tests (T2.2).
    //

    fn make_claim(_module: &str, id_suffix: &str, asserter_iri: &str) -> Value {
        // Use inline context with both the base prefixes and the synthesist
        // module prefix so the test doc expands correctly.
        json!({
            "@context": {
                "nomograph":  "https://nomograph.org/v3/",
                "prov":       "http://www.w3.org/ns/prov#",
                "xsd":        "http://www.w3.org/2001/XMLSchema#",
                "synthesist": "https://nomograph.org/synthesist/",
                "prov:generatedAtTime": {"@type": "xsd:dateTime"},
                "prov:wasAttributedTo": {"@type": "@id"}
            },
            "@id": format!("synthesist:claim/{}", id_suffix),
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "prov:wasAttributedTo": asserter_iri,
            "synthesist:summary": format!("Test claim {}", id_suffix),
        })
    }

    #[test]
    fn rebuild_against_empty_dir_yields_empty_view() {
        let tmp = TempDir::new().unwrap();
        let view = GraphView::open_in_memory().unwrap();
        let stats = rebuild(&view, tmp.path()).unwrap();

        assert_eq!(stats.claims_loaded, 0);
        assert_eq!(stats.triples_count, 0);
        assert_eq!(stats.parse_failures, 0);
    }

    #[test]
    fn rebuild_loads_100_claims_with_expected_triple_count() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        for i in 0..100 {
            let doc = make_claim(
                "synthesist",
                &format!("{:03}", i),
                "asserter:user:local:agd",
            );
            writer.append("user:local:agd", &doc).unwrap();
        }

        let view = GraphView::open_in_memory().unwrap();
        let stats = rebuild(&view, tmp.path()).unwrap();

        assert_eq!(stats.claims_loaded, 100);

        // Each claim emits 5 triples: rdf:type, prov:generatedAtTime,
        // prov:wasAttributedTo, synthesist:summary, and the implicit
        // mapping. Actual count is whatever Oxigraph produces; we
        // assert a lower bound that proves multiple triples land per
        // claim.
        assert!(
            stats.triples_count >= 100 * 4,
            "expected at least 400 triples for 100 claims, got {}",
            stats.triples_count
        );
        assert_eq!(stats.parse_failures, 0);
    }

    #[test]
    fn rebuild_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        for i in 0..10 {
            let doc = make_claim(
                "synthesist",
                &format!("i{}", i),
                "asserter:user:local:agd",
            );
            writer.append("user:local:agd", &doc).unwrap();
        }

        let view = GraphView::open_in_memory().unwrap();
        let stats1 = rebuild(&view, tmp.path()).unwrap();
        let stats2 = rebuild(&view, tmp.path()).unwrap();

        assert_eq!(stats1.claims_loaded, stats2.claims_loaded);
        assert_eq!(stats1.triples_count, stats2.triples_count);
        assert_eq!(stats1.parse_failures, stats2.parse_failures);
    }

    #[test]
    fn rebuild_clears_view_before_loading() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        // Round 1: 5 claims.
        for i in 0..5 {
            let doc = make_claim("synthesist", &format!("a{}", i), "asserter:user:local:agd");
            writer.append("user:local:agd", &doc).unwrap();
        }
        let view = GraphView::open_in_memory().unwrap();
        let stats_first = rebuild(&view, tmp.path()).unwrap();
        assert_eq!(stats_first.claims_loaded, 5);

        // Round 2: 5 more claims appended, total 10.
        for i in 0..5 {
            let doc = make_claim("synthesist", &format!("b{}", i), "asserter:user:local:agd");
            writer.append("user:local:agd", &doc).unwrap();
        }
        let stats_second = rebuild(&view, tmp.path()).unwrap();
        assert_eq!(stats_second.claims_loaded, 10);
        // The triples count is exactly twice what it was: the rebuild
        // cleared and re-loaded from scratch.
        assert_eq!(stats_second.triples_count, 2 * stats_first.triples_count);
    }

    //
    // Query tests (T2.3).
    //

    #[test]
    fn select_count_by_type_matches_status_shape() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        for i in 0..10 {
            let doc = make_claim("synthesist", &format!("q{}", i), "asserter:user:local:agd");
            writer.append("user:local:agd", &doc).unwrap();
        }
        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let q = r#"
            PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
            SELECT ?type (COUNT(?c) AS ?n)
            WHERE { GRAPH ?g { ?c rdf:type ?type } }
            GROUP BY ?type
        "#;
        let results = select(&view, q).unwrap();
        assert_eq!(results.columns, vec!["type".to_string(), "n".to_string()]);
        assert_eq!(results.rows.len(), 1);

        let type_term = &results.rows[0][0];
        match type_term {
            Term::Iri(s) => assert!(s.ends_with("synthesist/Task")),
            other => panic!("expected IRI for type, got {:?}", other),
        }
    }

    #[test]
    fn select_with_no_matches_returns_empty_rows() {
        let view = GraphView::open_in_memory().unwrap();
        let q = "SELECT ?s WHERE { ?s <http://nonexistent.example/> ?o }";
        let results = select(&view, q).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn select_on_malformed_sparql_errors() {
        let view = GraphView::open_in_memory().unwrap();
        let q = "this is not SPARQL";
        let err = select(&view, q).unwrap_err();
        let s = err.to_string();
        assert!(
            s.contains("SPARQL") || s.contains("evaluate") || s.contains("parse"),
            "error should describe SPARQL failure, got: {}",
            s
        );
    }

    #[test]
    fn ask_returns_true_when_claim_exists() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        let doc = make_claim("synthesist", "ask1", "asserter:user:local:agd");
        writer.append("user:local:agd", &doc).unwrap();
        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let q = r#"
            PREFIX rdf:         <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
            PREFIX synthesist:  <https://nomograph.org/synthesist/>
            ASK { GRAPH ?g { ?c rdf:type synthesist:Task } }
        "#;
        assert_eq!(ask(&view, q).unwrap(), true);
    }

    #[test]
    fn ask_returns_false_on_empty_view() {
        let view = GraphView::open_in_memory().unwrap();
        let q = "ASK { ?s ?p ?o }";
        assert_eq!(ask(&view, q).unwrap(), false);
    }

    #[test]
    fn select_term_distinguishes_iri_and_literal() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        let doc = make_claim("synthesist", "term1", "asserter:user:local:agd");
        writer.append("user:local:agd", &doc).unwrap();
        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let q = r#"
            PREFIX synthesist: <https://nomograph.org/synthesist/>
            SELECT ?c ?s WHERE {
              GRAPH ?g { ?c synthesist:summary ?s }
            }
        "#;
        let results = select(&view, q).unwrap();
        assert_eq!(results.rows.len(), 1);
        // First column: claim IRI; second column: literal summary.
        match &results.rows[0][0] {
            Term::Iri(_) => {}
            other => panic!("expected IRI for ?c, got {:?}", other),
        }
        match &results.rows[0][1] {
            Term::Literal { value, .. } => assert!(value.starts_with("Test claim")),
            other => panic!("expected Literal for ?s, got {:?}", other),
        }
    }

    //
    // Named graph routing tests (T2.4).
    //

    #[test]
    fn extract_module_prefix_handles_compact_form() {
        assert_eq!(extract_module_prefix("synthesist:Task"), Some("synthesist".into()));
        assert_eq!(extract_module_prefix("nomograph:Genesis"), Some("nomograph".into()));
    }

    #[test]
    fn extract_module_prefix_handles_full_iri() {
        assert_eq!(
            extract_module_prefix("https://nomograph.org/synthesist/Task"),
            Some("synthesist".into())
        );
    }

    #[test]
    fn extract_module_prefix_returns_none_for_unknown_uri() {
        assert_eq!(
            extract_module_prefix("https://example.com/some/Thing"),
            None
        );
    }

    #[test]
    fn module_graph_iri_composes_correctly() {
        assert_eq!(
            module_graph_iri("synthesist"),
            "https://nomograph.org/graphs/synthesist"
        );
    }

    #[test]
    fn rebuild_routes_synthesist_claims_single() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        for i in 0..5 {
            let doc = make_claim("synthesist", &format!("g{}", i), "asserter:user:local:agd");
            writer.append("user:local:agd", &doc).unwrap();
        }

        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let graphs = view.modules_in_view().unwrap();
        assert_eq!(graphs, vec!["https://nomograph.org/graphs/synthesist".to_string()]);
    }

    #[test]
    fn rebuild_routes_synthesist_claims_to_synthesist_named_graph() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        for i in 0..3 {
            let doc = make_claim("synthesist", &format!("s{}", i), "asserter:user:local:agd");
            writer.append("user:local:agd", &doc).unwrap();
        }

        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let graphs = view.modules_in_view().unwrap();
        assert_eq!(
            graphs,
            vec!["https://nomograph.org/graphs/synthesist".to_string()]
        );
    }

    #[test]
    fn graph_named_synthesist_filter_returns_only_synthesist_claims() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        for i in 0..4 {
            let doc = make_claim("synthesist", &format!("f{}", i), "asserter:user:local:agd");
            writer.append("user:local:agd", &doc).unwrap();
        }

        let view = GraphView::open_in_memory().unwrap();
        rebuild(&view, tmp.path()).unwrap();

        let q = r#"
            PREFIX rdf:         <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
            PREFIX synthesist:  <https://nomograph.org/synthesist/>
            SELECT (COUNT(?c) AS ?n)
            WHERE { GRAPH <https://nomograph.org/graphs/synthesist> { ?c rdf:type synthesist:Task } }
        "#;
        let results = select(&view, q).unwrap();
        assert_eq!(results.rows.len(), 1);
        if let Term::Literal { value, .. } = &results.rows[0][0] {
            assert_eq!(value, "4", "expected 4 synthesist:Task in the synthesist graph");
        } else {
            panic!("expected literal for count, got {:?}", results.rows[0][0]);
        }
    }

    #[test]
    fn open_or_in_memory_returns_usable_view_with_claims() {
        // Exercise open_or_in_memory against a real (non-empty) claims tree.
        // On macOS ARM the on-disk path panics; catch_unwind intercepts and
        // the in-memory rebuild must produce a populated GraphView.
        let tmp = TempDir::new().unwrap();
        let claims_dir = tmp.path().join("claims");
        std::fs::create_dir_all(&claims_dir).unwrap();
        let writer = LogWriter::new(&claims_dir).unwrap();
        for i in 0..5 {
            let doc = make_claim("synthesist", &format!("oom{}", i), "asserter:user:local:agd");
            writer.append("user:local:agd", &doc).unwrap();
        }

        let view_dir = claims_dir.join("_view.oxigraph");
        // open_or_in_memory must succeed regardless of whether the on-disk
        // Store::open succeeds or panics (both paths produce a valid view).
        let view = GraphView::open_or_in_memory(&view_dir, &claims_dir).unwrap();

        // The view is populated: at least the 5 claims loaded.
        let count = view.triple_count().unwrap();
        assert!(
            count >= 5,
            "expected at least 5 triples in the rebuilt view, got {}",
            count
        );
    }

    // -----------------------------------------------------------------------
    // View-cache snapshot tests (C.2)
    // -----------------------------------------------------------------------

    /// Helper: write N claims to claims_dir and return the triple count.
    fn setup_claims(claims_dir: &std::path::Path, n: usize, prefix: &str) {
        let writer = LogWriter::new(claims_dir).unwrap();
        for i in 0..n {
            let doc = make_claim(
                "synthesist",
                &format!("{}{}", prefix, i),
                "asserter:user:local:agd",
            );
            writer.append("user:local:agd", &doc).unwrap();
        }
    }

    #[test]
    fn view_cache_loads_from_snapshot_when_heads_match() {
        let tmp = TempDir::new().unwrap();
        let claims_dir = tmp.path().join("claims");
        std::fs::create_dir_all(&claims_dir).unwrap();
        setup_claims(&claims_dir, 5, "snap");

        let view_dir = claims_dir.join("_view.oxigraph");

        // First call: rebuild runs, snapshot written.
        let view1 = GraphView::open_or_in_memory(&view_dir, &claims_dir).unwrap();
        let count1 = view1.triple_count().unwrap();
        assert!(count1 >= 5, "expected at least 5 triples, got {}", count1);

        let snapshot_path = claims_dir.join(SNAPSHOT_FILE);
        let heads_path = claims_dir.join(SNAPSHOT_HEADS_FILE);
        assert!(snapshot_path.exists(), "snapshot should be written after rebuild");
        assert!(heads_path.exists(), "heads file should be written after rebuild");

        // Record mtime before second call.
        let mtime_before = std::fs::metadata(&snapshot_path)
            .unwrap()
            .modified()
            .unwrap();

        // Sleep briefly so any write would produce a different mtime.
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Second call: fast path -- snapshot unchanged.
        let view2 = GraphView::open_or_in_memory(&view_dir, &claims_dir).unwrap();
        let count2 = view2.triple_count().unwrap();
        assert_eq!(count1, count2, "triple counts should match on cache hit");

        let mtime_after = std::fs::metadata(&snapshot_path)
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "snapshot mtime should not change on cache hit (rebuild was skipped)"
        );
    }

    #[test]
    fn view_cache_rebuilds_when_heads_change() {
        let tmp = TempDir::new().unwrap();
        let claims_dir = tmp.path().join("claims");
        std::fs::create_dir_all(&claims_dir).unwrap();
        setup_claims(&claims_dir, 3, "chg");

        let view_dir = claims_dir.join("_view.oxigraph");

        // First open: builds snapshot.
        let _ = GraphView::open_or_in_memory(&view_dir, &claims_dir).unwrap();

        let snapshot_path = claims_dir.join(SNAPSHOT_FILE);
        let mtime_before = std::fs::metadata(&snapshot_path)
            .unwrap()
            .modified()
            .unwrap();

        // Add new claim to invalidate heads.
        std::thread::sleep(std::time::Duration::from_millis(20));
        setup_claims(&claims_dir, 1, "chg_new");

        // Second open: heads changed, so rebuild runs and new snapshot written.
        let view2 = GraphView::open_or_in_memory(&view_dir, &claims_dir).unwrap();
        let count2 = view2.triple_count().unwrap();
        assert!(count2 > 0);

        let mtime_after = std::fs::metadata(&snapshot_path)
            .unwrap()
            .modified()
            .unwrap();
        assert_ne!(
            mtime_before, mtime_after,
            "snapshot mtime should change when heads are stale (rebuild ran)"
        );
    }

    #[test]
    fn view_cache_handles_corrupted_snapshot() {
        let tmp = TempDir::new().unwrap();
        let claims_dir = tmp.path().join("claims");
        std::fs::create_dir_all(&claims_dir).unwrap();
        setup_claims(&claims_dir, 2, "corrupt");

        let view_dir = claims_dir.join("_view.oxigraph");

        // First open: creates a valid snapshot.
        let _ = GraphView::open_or_in_memory(&view_dir, &claims_dir).unwrap();

        // Corrupt the snapshot.
        let snapshot_path = claims_dir.join(SNAPSHOT_FILE);
        std::fs::write(&snapshot_path, b"this is not valid N-Quads @@@").unwrap();

        // Second open: corrupt snapshot is detected, fallback to rebuild.
        // Must not panic or return an error.
        let view = GraphView::open_or_in_memory(&view_dir, &claims_dir).unwrap();
        let count = view.triple_count().unwrap();
        assert!(
            count >= 2,
            "expected at least 2 triples after rebuild from corrupted snapshot, got {}",
            count
        );
    }

    #[test]
    fn view_cache_handles_missing_snapshot() {
        let tmp = TempDir::new().unwrap();
        let claims_dir = tmp.path().join("claims");
        std::fs::create_dir_all(&claims_dir).unwrap();
        setup_claims(&claims_dir, 4, "miss");

        let view_dir = claims_dir.join("_view.oxigraph");

        // No snapshot or heads file exists: rebuild must run and write them.
        let snapshot_path = claims_dir.join(SNAPSHOT_FILE);
        let heads_path = claims_dir.join(SNAPSHOT_HEADS_FILE);
        assert!(!snapshot_path.exists());
        assert!(!heads_path.exists());

        let view = GraphView::open_or_in_memory(&view_dir, &claims_dir).unwrap();
        let count = view.triple_count().unwrap();
        assert!(
            count >= 4,
            "expected at least 4 triples after rebuild, got {}",
            count
        );
        assert!(snapshot_path.exists(), "snapshot should be written after fresh rebuild");
        assert!(heads_path.exists(), "heads file should be written after fresh rebuild");
    }

    #[test]
    fn rebuild_tolerates_malformed_lines() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        // Write 2 good claims through the writer.
        for i in 0..2 {
            let doc = make_claim("synthesist", &format!("ok{}", i), "asserter:user:local:agd");
            writer.append("user:local:agd", &doc).unwrap();
        }

        // Then append a malformed line directly to the log file.
        let log_path = tmp
            .path()
            .join("user-local-agd")
            .join("log.jsonl");
        let mut existing = std::fs::read_to_string(&log_path).unwrap();
        existing.push_str("{ this is not valid JSON\n");
        std::fs::write(&log_path, existing).unwrap();

        let view = GraphView::open_in_memory().unwrap();
        let stats = rebuild(&view, tmp.path()).unwrap();

        // 2 good claims loaded, 1 parse failure.
        assert_eq!(stats.claims_loaded, 2);
        assert_eq!(stats.parse_failures, 1);
    }
}
