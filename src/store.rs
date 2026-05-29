//! Synthesist's Store surface.
//!
//! Wraps [`nomograph_workflow::Store`] with synthesist-side schema
//! validation at the API boundary. Every typed `append` runs the
//! claim through [`crate::schema::validate_claim`] before delegating
//! to the substrate, so consumers get a structured `SchemaError` (via
//! `anyhow`'s formatting at the binary edge) without garbage entering
//! the claim log.
//!
//! The substrate (`nomograph-claim` 0.2+) is type-agnostic for
//! validation; the workflow layer delegates the responsibility up,
//! and this is where it lands.
//!
//! Existing call sites that did `store.append(...)` continue to work
//! unchanged -- `SynthStore` provides the same signature. Read-only
//! methods (`query`, `root`, `inner`, `sync_view`, `with_asserter`)
//! transparently delegate to the wrapped workflow store via `Deref`.
//!
//! ## v3 dual write
//!
//! When `with_asserter` is called, `SynthStore` also initialises a
//! `nomograph_claim::log::LogWriter` rooted at the same claims
//! directory. After every successful v2 append, the store tries to
//! write a matching JSON-LD document to the per-asserter v3 log.
//! The v3 write is BEST EFFORT: failure produces a warning on stderr
//! and the call still returns `Ok` with the v2 `ClaimId`. The v2
//! substrate remains the source of truth for the alpha window.
//!
//! `append_replay` (migration path) does NOT dual-write.

use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nomograph_claim::{ClaimId, ClaimType};
use serde_json::Value;

pub use nomograph_workflow::{
    CLAIMS_DIR, find_legacy_v1_db, json_out, legacy_migration_error, parse_tree_spec, today,
};

/// Synthesist-flavored Store: workflow's CRDT-backed Store with the
/// synthesist schema validator applied at every `append` and an
/// optional v3 JSON-LD dual-write for the alpha thesis.
pub struct SynthStore {
    inner: nomograph_workflow::Store,
    /// Asserter string captured from the last `with_asserter` call.
    /// `None` until the caller sets it explicitly.
    asserted_by: Option<String>,
    /// v3 log writer rooted at the claims directory. Constructed when
    /// the store is opened; `None` if construction failed (e.g. the
    /// directory does not yet exist at open time).
    log_writer: Option<nomograph_claim::log::LogWriter>,
    /// The claims directory root, kept so `with_asserter` can
    /// (re-)construct the `LogWriter` after the `inner` store is
    /// known.
    claims_root: Option<PathBuf>,
}

impl SynthStore {
    /// Build the v3 `LogWriter` from a claims directory, returning
    /// `None` on any error rather than propagating. The dual write
    /// is best-effort; a missing writer just means no v3 output.
    fn make_log_writer(claims_dir: &Path) -> Option<nomograph_claim::log::LogWriter> {
        nomograph_claim::log::LogWriter::new(claims_dir).ok()
    }

    fn from_inner(inner: nomograph_workflow::Store) -> Self {
        let root = inner.root().to_path_buf();
        let log_writer = Self::make_log_writer(&root);
        Self {
            inner,
            asserted_by: None,
            log_writer,
            claims_root: Some(root),
        }
    }
}

#[allow(dead_code)]
impl SynthStore {
    pub fn discover() -> Result<Self> {
        Ok(Self::from_inner(nomograph_workflow::Store::discover()?))
    }

    pub fn discover_from(start: &Path) -> Result<Self> {
        Ok(Self::from_inner(
            nomograph_workflow::Store::discover_from(start)?,
        ))
    }

    pub fn discover_for(session: &Option<String>) -> Result<Self> {
        let inner = nomograph_workflow::Store::discover_for(session)?;
        // Mirror the asserter string from the workflow store so the v3
        // dual-write path sees the same value the inner store uses for
        // appends. Without this, asserted_by stays None and no v3 log
        // lines are produced for CLI commands that call discover_for.
        let asserter = inner.asserted_by().to_string();
        let mut s = Self::from_inner(inner);
        if !asserter.is_empty() {
            s.asserted_by = Some(asserter);
        }
        Ok(s)
    }

    pub fn open_at(claims_dir: &Path) -> Result<Self> {
        Ok(Self::from_inner(
            nomograph_workflow::Store::open_at(claims_dir)?,
        ))
    }

    pub fn init_at(claims_dir: &Path) -> Result<Self> {
        Ok(Self::from_inner(
            nomograph_workflow::Store::init_at(claims_dir)?,
        ))
    }

    pub fn with_asserter(mut self, asserted_by: impl Into<String>) -> Self {
        let s: String = asserted_by.into();
        self.asserted_by = Some(s.clone());
        self.inner = self.inner.with_asserter(s);
        // Re-init the log writer now that we know the claims root.
        if let Some(ref root) = self.claims_root {
            self.log_writer = Self::make_log_writer(root);
        }
        self
    }

    /// Append a typed claim. Validates `props` against the synthesist
    /// schema for `claim_type` before persisting. Returns the new
    /// claim id on success or a structured schema error on rejection.
    ///
    /// Validation runs at this synthesist boundary because the
    /// workflow layer (and the substrate beneath it) is type-agnostic
    /// since v0.2.0. The same `crate::schema::<type>::*` constants
    /// drive both this validator and the CLI's clap parsers, so
    /// CLI-accepts-iff-schema-accepts is structural.
    ///
    /// After a successful v2 write, attempts a v3 JSON-LD dual write
    /// to the per-asserter log. Dual-write failure is non-fatal.
    pub fn append(
        &mut self,
        claim_type: ClaimType,
        props: Value,
        supersedes: Option<ClaimId>,
    ) -> Result<ClaimId> {
        crate::schema::validate_props(&claim_type, &props)
            .map_err(anyhow::Error::from)
            .context("validate claim before append")?;
        let claim_id = self
            .inner
            .append(claim_type.clone(), props.clone(), supersedes.clone())?;

        // v3 dual write -- best effort.
        if let (Some(asserter), Some(writer)) = (&self.asserted_by, &self.log_writer) {
            match v3_dual_write(writer, asserter, &claim_id, &claim_type, &props, &supersedes) {
                Ok(_) => {}
                Err(e) => eprintln!("warning: v3 dual-write failed: {e}"),
            }
        }

        Ok(claim_id)
    }

    /// Replay an existing claim into the store without running
    /// synthesist's per-type validator.
    ///
    /// **Use this only for migration and import paths** -- moving
    /// existing claims (from a v1 SQLite estate via `cmd_migrate`,
    /// from a JSON export via `cmd_import`) into the new store. New
    /// consumer-driven writes must go through `Self::append`
    /// instead, which is the strict-on-write boundary that defends
    /// against agents hallucinating fake claim types.
    ///
    /// The name carries the warning: this is replay, not creation.
    /// The substrate's structural checks (content hash, append
    /// lock, IO durability) still apply, so this is "skip domain
    /// validation," not "skip all validation."
    ///
    /// Visibility is `pub(crate)` to keep the bypass within
    /// synthesist's own modules -- no external consumer should ever
    /// hold a `SynthStore` and reach for this.
    ///
    /// Per the claims-forward compat policy: new binaries must be
    /// able to read existing claim logs (including lattice and
    /// coordination types written by other consumers or migrated
    /// from v1). This is that read path's write side.
    ///
    /// `append_replay` does NOT dual-write to the v3 log. The
    /// migration tool owns that translation.
    pub(crate) fn append_replay(
        &mut self,
        claim_type: ClaimType,
        props: Value,
        supersedes: Option<ClaimId>,
    ) -> Result<ClaimId> {
        self.inner.append(claim_type, props, supersedes)
    }
}

impl Deref for SynthStore {
    type Target = nomograph_workflow::Store;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for SynthStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Back-compat alias retained from the v2 rewrite. Prefer `SynthStore`
/// at call sites.
pub type Store = SynthStore;

// ---------------------------------------------------------------------------
// v3 translation helpers
// ---------------------------------------------------------------------------

/// Convert a `snake_case` or `kebab-case` string to `TitleCase`.
///
/// Examples: `task` -> `Task`, `agree_snapshot` -> `AgreeSnapshot`,
/// `discovery` -> `Discovery`.
fn camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Inline @context for synthesist v3 JSON-LD docs.
///
/// Declares the `synthesist`, `nomograph`, `prov`, `xsd` prefixes plus
/// IRI-reference typing for substrate-level reference predicates
/// (`supersedes`, `agreeSnapshot`). Without these declarations a
/// SPARQL query that does `PREFIX synthesist: <https://nomograph.org/synthesist/>`
/// would not match the produced IRIs, because oxjsonld would treat
/// `synthesist:Spec` as the URI `<synthesist:Spec>` rather than as the
/// expanded `<https://nomograph.org/synthesist/Spec>`.
fn synthesist_jsonld_context() -> serde_json::Value {
    use serde_json::json;
    json!({
        "nomograph":  "https://nomograph.org/v3/",
        "synthesist": "https://nomograph.org/synthesist/",
        "prov":       "http://www.w3.org/ns/prov#",
        "xsd":        "http://www.w3.org/2001/XMLSchema#",
        "prov:generatedAtTime": {"@type": "xsd:dateTime"},
        "prov:wasAttributedTo": {"@type": "@id"},
        "prov:wasRevisionOf":   {"@type": "@id"},
        "nomograph:parentAsserter": {"@type": "@id"},
        "synthesist:supersedes":    {"@type": "@id"},
        "synthesist:agreeSnapshot": {"@type": "@id", "@container": "@set"}
    })
}

/// Convert a `snake_case` or `kebab-case` string to `lowerCamelCase`.
///
/// Used to align v3 JSON-LD predicate names with the SHACL ontology
/// (e.g. `agree_snapshot` -> `agreeSnapshot`). Single-word inputs are
/// unchanged.
///
/// Examples: `id` -> `id`, `agree_snapshot` -> `agreeSnapshot`,
/// `depends_on` -> `dependsOn`.
fn lower_camel_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Format the current wall-clock time as RFC 3339 with millisecond
/// precision and a `Z` suffix.
fn format_now() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Build and write one v3 JSON-LD document for a freshly appended v2 claim.
fn v3_dual_write(
    writer: &nomograph_claim::log::LogWriter,
    asserter: &str,
    claim_id: &str,
    claim_type: &ClaimType,
    props: &Value,
    supersedes: &Option<ClaimId>,
) -> Result<()> {
    use serde_json::{Map, Value as V};

    let id_short = &claim_id[..claim_id.len().min(16)];
    let type_camel = camel_case(claim_type.as_str());

    let mut doc: Map<String, V> = Map::new();
    // Inline @context: declare the synthesist prefix so JSON-LD parsers
    // expand `synthesist:Spec` to `<https://nomograph.org/synthesist/Spec>`
    // and SPARQL queries that `PREFIX synthesist: <...synthesist/>` match
    // the produced IRIs. `supersedes` and `agreeSnapshot` are typed as
    // IRI references so superseding-claim and snapshot triples bind to
    // node IRIs (matching `@id` of the target claim) rather than literals.
    // graph_view::rebuild's inject_inline_context respects an inline
    // @context object and overrides only the bare-URI form, so this
    // survives the rebuild round-trip.
    doc.insert("@context".into(), synthesist_jsonld_context());
    doc.insert(
        "@id".into(),
        V::String(format!("synthesist:claim/{}", id_short)),
    );
    doc.insert(
        "@type".into(),
        V::String(format!("synthesist:{}", type_camel)),
    );
    doc.insert("prov:generatedAtTime".into(), V::String(format_now()));
    doc.insert(
        "prov:wasAttributedTo".into(),
        V::String(format!("asserter:{}", asserter)),
    );

    if let Some(sup_id) = supersedes {
        let sup_short = &sup_id[..sup_id.len().min(16)];
        doc.insert(
            "synthesist:supersedes".into(),
            V::String(format!("synthesist:claim/{}", sup_short)),
        );
    }

    // Expand props as synthesist:<lowerCamelCase(key)> predicates.
    // Snake_case is synthesist's internal convention (v2 era); the v3
    // ontology uses lowerCamelCase to align with SHACL shapes and the
    // overlay SPARQL prefixes (e.g. synthesist:agreeSnapshot,
    // synthesist:dependsOn). Single-word keys pass through unchanged.
    if let Some(props_map) = props.as_object() {
        for (k, v) in props_map {
            doc.insert(format!("synthesist:{}", lower_camel_case(k)), v.clone());
        }
    }

    let v = V::Object(doc);
    writer
        .append(asserter, &v)
        .context("v3 log writer append")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// Method-resolution proof: `synth_store.append(...)` resolves to the
    /// inherent validating method on `SynthStore`, not the unvalidating
    /// one reachable through `Deref` to `nomograph_workflow::Store`.
    /// Rust's method resolution prefers inherent methods, but it's
    /// worth proving because the silent-fall-through to the workflow
    /// layer would be exactly the regression that the SynthStore
    /// wrapper exists to prevent.
    #[test]
    fn append_inherent_method_runs_validation() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        // Bad spec: missing required `goal`. If validation runs, this
        // returns Err with a structured SchemaError. If Deref shadowed
        // the inherent method, the unvalidating workflow::Store::append
        // would let it through and we'd get Ok.
        let bad = json!({
            "tree": "k",
            "id": "x",
            "status": "active",
            "topics": ["x"],
        });
        let err = store.append(ClaimType::Spec, bad, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("validate claim before append")
                || msg.contains("goal"),
            "expected validator error, got: {msg}"
        );
    }

    /// Strict-on-write: synthesist rejects appends for claim types it
    /// does not own (lattice or coordination types). This is the
    /// hallucination-defense from the adversarial review: agents that
    /// invent fake claim types get a clear rejection at the synthesist
    /// boundary instead of writing nonsense into the substrate.
    #[test]
    fn append_rejects_unowned_claim_types() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        for unowned in [
            ClaimType::Stakeholder,
            ClaimType::Topic,
            ClaimType::Signal,
            ClaimType::Disposition,
            ClaimType::Intent,
            ClaimType::Heartbeat,
            ClaimType::Directive,
        ] {
            let result = store.append(unowned.clone(), json!({}), None);
            assert!(
                result.is_err(),
                "synthesist must reject claim_type {unowned:?} at write boundary"
            );
        }
    }

    /// `append_replay` deliberately bypasses the synthesist
    /// validator for migration / import paths. The structural checks
    /// in the substrate (content hash, append lock) still run, but
    /// per-type schema validation is skipped. Verifying that the
    /// bypass actually bypasses, so we can move existing claims of
    /// any type without the strict-on-write gate.
    #[test]
    fn append_replay_skips_synthesist_validator() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        // A Stakeholder claim with empty props would be rejected by
        // both the synthesist write validator (unowned type) and any
        // future lattice validator (missing required fields). The
        // unvalidated path just stores it, which is what import wants.
        let id = store
            .append_replay(ClaimType::Stakeholder, json!({"id": "alice"}), None)
            .expect("unvalidated append accepts unowned types");
        assert!(!id.is_empty());
    }

    // -----------------------------------------------------------------------
    // T3.5b: v3 dual-write tests
    // -----------------------------------------------------------------------

    /// Build a minimal valid Task props value.
    fn task_props() -> Value {
        json!({
            "tree": "proj",
            "spec": "s1",
            "id": "t1",
            "summary": "hello world",
            "status": "pending",
        })
    }

    /// After one append with an asserter set, the v3 log file exists
    /// and contains exactly one JSON-LD line with the expected fields.
    #[test]
    fn dual_write_produces_v3_log_after_append() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let asserter = "user:local:test:sess1";
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter(asserter);

        let props = task_props();
        let _id = store.append(ClaimType::Task, props, None).unwrap();

        // claims/<asserter-dir>/log.jsonl must exist.
        let log_path = claims.join("user-local-test-sess1").join("log.jsonl");
        assert!(log_path.exists(), "v3 log file must be created after append");

        let content = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 1, "expected exactly one v3 log line");

        let doc: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        // @id must be synthesist:claim/<16-hex-chars>
        let at_id = doc["@id"].as_str().unwrap();
        assert!(at_id.starts_with("synthesist:claim/"), "@id must start with synthesist:claim/");
        assert_eq!(
            at_id.len(),
            "synthesist:claim/".len() + 16,
            "@id suffix must be 16 chars"
        );
        // @type
        assert_eq!(doc["@type"].as_str().unwrap(), "synthesist:Task");
        // prov:generatedAtTime
        let gen_time = doc["prov:generatedAtTime"].as_str().unwrap();
        assert!(gen_time.ends_with('Z'), "generatedAtTime must have Z suffix");
        assert!(gen_time.contains('T'), "generatedAtTime must be ISO-8601");
        // prov:wasAttributedTo
        let attr = doc["prov:wasAttributedTo"].as_str().unwrap();
        assert_eq!(attr, format!("asserter:{}", asserter));
        // synthesist:status from props
        assert_eq!(doc["synthesist:status"].as_str().unwrap(), "pending");
    }

    /// Two appends with the same asserter produce two lines in the log.
    #[test]
    fn dual_write_two_appends_two_lines() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let asserter = "user:local:test:sess2";
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter(asserter);

        store.append(ClaimType::Task, task_props(), None).unwrap();
        store.append(ClaimType::Task, task_props(), None).unwrap();

        let log_path = claims.join("user-local-test-sess2").join("log.jsonl");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2, "two appends must produce two v3 log lines");
        for line in &lines {
            let doc: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(doc.get("@id").is_some());
        }
    }

    /// Append with no asserter set still succeeds and produces no v3 log.
    #[test]
    fn dual_write_no_asserter_no_v3_log() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        // No with_asserter call.
        let mut store = SynthStore::init_at(&claims).unwrap();
        let id = store.append(ClaimType::Task, task_props(), None).unwrap();
        assert!(!id.is_empty());
        // No asserter subdir with log.jsonl should have been created.
        let entries: Vec<_> = std::fs::read_dir(&claims)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        for entry in &entries {
            let log = entry.path().join("log.jsonl");
            assert!(
                !log.exists(),
                "no v3 log should be created without an asserter; found: {}",
                log.display()
            );
        }
    }

    /// append_replay does NOT produce a v3 log line.
    #[test]
    fn dual_write_replay_does_not_write_v3() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let asserter = "user:local:test:sess3";
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter(asserter);

        let _id = store
            .append_replay(ClaimType::Stakeholder, json!({"id": "alice"}), None)
            .unwrap();

        let log_path = claims.join("user-local-test-sess3").join("log.jsonl");
        assert!(
            !log_path.exists(),
            "append_replay must NOT write to v3 log"
        );
    }

    /// `discovery` claim type produces `@type: synthesist:Discovery`.
    #[test]
    fn dual_write_camel_case_discovery() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let asserter = "user:local:test:sess4";
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter(asserter);

        // Discovery requires: tree, spec, id, date, finding.
        let props = json!({
            "tree": "proj",
            "spec": "s1",
            "id": "d1",
            "date": "2026-05-28",
            "finding": "found something",
        });
        store.append(ClaimType::Discovery, props, None).unwrap();

        let log_path = claims.join("user-local-test-sess4").join("log.jsonl");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let doc: serde_json::Value =
            serde_json::from_str(content.lines().next().unwrap()).unwrap();
        assert_eq!(
            doc["@type"].as_str().unwrap(),
            "synthesist:Discovery",
            "discovery claim must produce @type synthesist:Discovery"
        );
    }

    /// Internal: camel_case helper correctness.
    #[test]
    fn camel_case_helper() {
        assert_eq!(camel_case("task"), "Task");
        assert_eq!(camel_case("agree_snapshot"), "AgreeSnapshot");
        assert_eq!(camel_case("discovery"), "Discovery");
        assert_eq!(camel_case("session"), "Session");
        assert_eq!(camel_case("campaign"), "Campaign");
        assert_eq!(camel_case("tree"), "Tree");
        assert_eq!(camel_case("spec"), "Spec");
        assert_eq!(camel_case("outcome"), "Outcome");
        assert_eq!(camel_case("phase"), "Phase");
    }
}
