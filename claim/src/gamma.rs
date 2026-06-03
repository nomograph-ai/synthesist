//! Gamma index: a redb-backed typed query index over the claim log.
//!
//! The gamma index is a derived projection of the per-asserter JSON-LD
//! logs (`claims/<asserter>/log.jsonl`) into a redb database. It is the
//! v3 replacement for the alpha Oxigraph/SPARQL graph view. The on-disk
//! index is a DISPOSABLE local cache: it is gitignored and can always be
//! rebuilt from the log union, which remains the only source of truth.
//!
//! ## Tables
//!
//! - **`docs`**: `claim-id -> full JSON-LD doc` (the canonical record).
//!   Nested and multi-valued props (`acceptance = [{criterion, verifyCmd}]`,
//!   `dependsOn`, `files`, `agreeSnapshot`) are read straight from the
//!   doc -- the index never shreds nested objects to triples.
//! - **`pos`**: `"{p}\x1f{o}\x1f{s}"` -> `()`. Range by `(p, o)` yields
//!   the subjects that carry a given predicate/object. Drives the
//!   live-head anti-join, supersession lookups, and aggregation scans.
//! - **`pso`**: `"{p}\x1f{s}\x1f{o}"` -> `()`. Range by `(p, s)` yields
//!   a subject's values for a predicate (scalar property fetch).
//! - **`meta`**: small key/value table; holds the heads signal the index
//!   was last built against, so a rebuild is skipped when the logs are
//!   unchanged.
//!
//! Both POS and PSO are populated over the SCALAR and MEMBER predicates
//! of every claim: `@type`, every string-valued module predicate
//! (`status`, `sessionId`, `summary`, `supersedes`, ...), `prov:*`
//! envelope predicates, and each MEMBER of a set-valued predicate
//! (`agreeSnapshot`, `dependsOn`, `files`). Nested arrays of OBJECTS
//! (`acceptance`) are NOT triple-shredded; they live in the doc.
//!
//! ## dateTime ordering
//!
//! `prov:generatedAtTime` is stored as its canonical string. The
//! substrate writer (`crate::prov::now_iso`) emits a fixed-width,
//! zero-padded `%Y-%m-%dT%H:%M:%S.NNNZ` form, for which lexical order
//! equals chronological order. The index ASSERTS this format at ingest
//! time (see [`is_canonical_datetime`]); plan-at-risk and any other
//! timestamp comparison is therefore a correct lexical string compare.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde_json::Value;

use crate::heads;
use crate::log::LogReader;

/// POS index: `"{p}\x1f{o}\x1f{s}"` -> `()`.
const POS: TableDefinition<&str, ()> = TableDefinition::new("pos");
/// PSO index: `"{p}\x1f{s}\x1f{o}"` -> `()`.
const PSO: TableDefinition<&str, ()> = TableDefinition::new("pso");
/// docs: `claim-id` -> canonical JSON-LD doc (serialized).
const DOCS: TableDefinition<&str, &str> = TableDefinition::new("docs");
/// meta: small key/value scratch (`heads` -> signal hash).
const META: TableDefinition<&str, &str> = TableDefinition::new("meta");

/// Field separator inside POS/PSO composite keys (ASCII unit separator).
const SEP: char = '\u{1f}';

/// meta key under which the heads signal of the last build is stored.
const META_HEADS_KEY: &str = "heads";

/// The `@type` predicate key, in the compact JSON-LD form claims use.
const TYPE_PRED: &str = "@type";

/// The `prov:generatedAtTime` predicate key (compact form).
const GENERATED_AT_PRED: &str = "prov:generatedAtTime";

/// The `prov:wasAttributedTo` predicate key (compact form).
const ATTRIBUTED_TO_PRED: &str = "prov:wasAttributedTo";

/// A redb-backed gamma index over the claim log union.
///
/// Open with [`Gamma::open`] for an on-disk index (rebuilt from the log
/// union when the heads signal has moved) or [`Gamma::open_in_memory`]
/// for an ephemeral in-process index used by tests. Both expose the same
/// typed H1-H10 query surface.
pub struct Gamma {
    db: Database,
    index_path: Option<PathBuf>,
}

impl Gamma {
    /// Open (or create) an on-disk gamma index at `index_path` and bring
    /// it into sync with the log union under `claims_dir`.
    ///
    /// The index is a disposable cache. On open, the current heads signal
    /// (per-asserter log line counts) is compared with the signal the
    /// index was last built against; the full rebuild runs only when they
    /// differ. Every CLI command is a fresh process, so this heads check
    /// is what keeps the cache correct across process boundaries.
    pub fn open(index_path: &Path, claims_dir: &Path) -> Result<Self> {
        if let Some(parent) = index_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create index parent {}", parent.display()))?;
        }
        let db = Database::create(index_path)
            .with_context(|| format!("open gamma index at {}", index_path.display()))?;
        let mut gamma = Self {
            db,
            index_path: Some(index_path.to_path_buf()),
        };
        gamma.sync(claims_dir)?;
        Ok(gamma)
    }

    /// Open an ephemeral in-memory gamma index. Nothing is written to
    /// disk; the index is dropped with the `Gamma`. Used by tests and by
    /// callers that want a throwaway index for a single process.
    pub fn open_in_memory() -> Result<Self> {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .context("create in-memory gamma index")?;
        Ok(Self {
            db,
            index_path: None,
        })
    }

    /// Path to the on-disk index file, if this index is on-disk.
    pub fn index_path(&self) -> Option<&Path> {
        self.index_path.as_deref()
    }

    /// Return true if this index is in-memory.
    pub fn is_in_memory(&self) -> bool {
        self.index_path.is_none()
    }

    /// Bring the index into sync with the log union under `claims_dir`.
    ///
    /// Runs a full rebuild only when the current heads signal differs
    /// from the one recorded in `meta`. Returns the rebuild stats when a
    /// rebuild ran, or `None` when the index was already current.
    pub fn sync(&mut self, claims_dir: &Path) -> Result<Option<RebuildStats>> {
        let current =
            heads::current_heads(claims_dir).context("compute current heads for gamma sync")?;
        if self.stored_heads()?.as_deref() == Some(current.as_str()) {
            return Ok(None);
        }
        let stats = self.rebuild(claims_dir, &current)?;
        Ok(Some(stats))
    }

    /// Force a full rebuild from the log union, regardless of heads.
    ///
    /// Clears every table and re-ingests the union, then records the
    /// supplied heads signal in `meta`.
    fn rebuild(&mut self, claims_dir: &Path, heads_signal: &str) -> Result<RebuildStats> {
        let reader = LogReader::new(claims_dir)?;

        let mut claims_loaded = 0usize;
        let mut parse_failures = 0usize;
        let mut datetime_violations = 0usize;
        let mut triples = 0u64;

        let w = self.db.begin_write().context("begin gamma write txn")?;
        {
            let mut pos = w.open_table(POS).context("open pos table")?;
            let mut pso = w.open_table(PSO).context("open pso table")?;
            let mut docs = w.open_table(DOCS).context("open docs table")?;
            let mut meta = w.open_table(META).context("open meta table")?;

            // Clear tables so the index matches the log union exactly.
            // redb has no truncate; retain+drain by collecting keys.
            clear_table(&mut pos)?;
            clear_pso(&mut pso)?;
            clear_docs(&mut docs)?;
            clear_meta(&mut meta)?;

            for item in reader.iter_claims() {
                let claim = match item {
                    Ok(c) => c,
                    Err(_) => {
                        parse_failures += 1;
                        continue;
                    }
                };
                let id = claim.id.as_str().to_string();
                let obj = match claim.raw.as_object() {
                    Some(o) => o,
                    None => {
                        parse_failures += 1;
                        continue;
                    }
                };

                // Store the canonical doc verbatim.
                let doc_json = serde_json::to_string(&claim.raw)
                    .context("re-serialize claim doc for docs table")?;
                docs.insert(id.as_str(), doc_json.as_str())
                    .context("insert into docs table")?;

                // Index scalar + member predicates into POS/PSO.
                for (pred, value) in obj {
                    if pred == "@id" || pred == "@context" {
                        continue;
                    }
                    if pred == GENERATED_AT_PRED
                        && let Some(ts) = value.as_str()
                        && !is_canonical_datetime(ts)
                    {
                        datetime_violations += 1;
                    }
                    for obj_str in scalar_members(value) {
                        insert_triple(&mut pos, &mut pso, pred, &id, &obj_str)?;
                        triples += 1;
                    }
                }

                claims_loaded += 1;
            }

            meta.insert(META_HEADS_KEY, heads_signal)
                .context("record heads signal in meta")?;
        }
        w.commit().context("commit gamma rebuild")?;

        Ok(RebuildStats {
            claims_loaded,
            triples_count: triples,
            parse_failures,
            datetime_violations,
        })
    }

    /// Read the heads signal recorded by the last rebuild.
    fn stored_heads(&self) -> Result<Option<String>> {
        let r = self.db.begin_read().context("begin gamma read txn")?;
        let meta = match r.open_table(META) {
            Ok(t) => t,
            // Table absent => never built.
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(e).context("open meta table"),
        };
        let got = meta
            .get(META_HEADS_KEY)
            .context("read heads from meta")?
            .map(|v| v.value().to_string());
        Ok(got)
    }

    // -- raw index access used by the typed helpers --------------------

    /// Subjects carrying `(pred, obj)`. Range scan of POS by the
    /// `"{p}\x1f{o}\x1f"` prefix; returns the trailing subject field.
    fn subjects_with(&self, pred: &str, obj: &str) -> Result<Vec<String>> {
        let prefix = format!("{pred}{SEP}{obj}{SEP}");
        self.pos_prefix_tail(&prefix)
    }

    /// All (subject, object) pairs for `pred`. Scans PSO by the
    /// `"{p}\x1f"` prefix; the first split field after the prefix is
    /// the subject and the second is the object.
    fn objects_of(&self, pred: &str) -> Result<Vec<(String, String)>> {
        // Returns (subject, object) pairs for the predicate.
        let prefix = format!("{pred}{SEP}");
        let r = self.db.begin_read()?;
        let pso = match r.open_table(PSO) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e).context("open pso table"),
        };
        let mut out = Vec::new();
        for item in pso.range::<&str>(prefix.as_str()..)? {
            let (k, _) = item?;
            let key = k.value();
            if !key.starts_with(&prefix) {
                break;
            }
            let rest = &key[prefix.len()..];
            let mut parts = rest.split(SEP);
            let s = parts.next().unwrap_or("").to_string();
            let o = parts.next().unwrap_or("").to_string();
            out.push((s, o));
        }
        Ok(out)
    }

    /// Collect the trailing (subject) field of every POS key under
    /// `prefix` (which must already include both trailing separators).
    fn pos_prefix_tail(&self, prefix: &str) -> Result<Vec<String>> {
        let r = self.db.begin_read()?;
        let pos = match r.open_table(POS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e).context("open pos table"),
        };
        let mut out = Vec::new();
        for item in pos.range::<&str>(prefix..)? {
            let (k, _) = item?;
            let key = k.value();
            if !key.starts_with(prefix) {
                break;
            }
            out.push(key[prefix.len()..].to_string());
        }
        Ok(out)
    }

    /// Values of `(subject, pred)` via PSO. Used for scalar property
    /// fetch when reading the doc would be heavier than needed.
    fn values_of(&self, subject: &str, pred: &str) -> Result<Vec<String>> {
        let prefix = format!("{pred}{SEP}{subject}{SEP}");
        let r = self.db.begin_read()?;
        let pso = match r.open_table(PSO) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e).context("open pso table"),
        };
        let mut out = Vec::new();
        for item in pso.range::<&str>(prefix.as_str()..)? {
            let (k, _) = item?;
            let key = k.value();
            if !key.starts_with(&prefix) {
                break;
            }
            out.push(key[prefix.len()..].to_string());
        }
        Ok(out)
    }

    /// Fetch the canonical doc for a claim id.
    pub fn doc(&self, claim_id: &str) -> Result<Option<Value>> {
        let r = self.db.begin_read()?;
        let docs = match r.open_table(DOCS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(e).context("open docs table"),
        };
        match docs.get(claim_id)? {
            Some(v) => {
                let parsed: Value = serde_json::from_str(v.value()).context("parse stored doc")?;
                Ok(Some(parsed))
            }
            None => Ok(None),
        }
    }

    /// The set of subjects superseded by some other claim, for a given
    /// supersedes predicate. A subject `x` is superseded when any claim
    /// carries `(supersedes_pred, x)`.
    fn superseded_set(&self, supersedes_pred: &str) -> Result<HashSet<String>> {
        let pairs = self.objects_of(supersedes_pred)?;
        Ok(pairs.into_iter().map(|(_, o)| o).collect())
    }

    /// The set of subjects that themselves supersede something (i.e.
    /// carry the supersedes predicate at all).
    fn supersedes_something(&self, supersedes_pred: &str) -> Result<HashSet<String>> {
        let pairs = self.objects_of(supersedes_pred)?;
        Ok(pairs.into_iter().map(|(s, _)| s).collect())
    }

    // ==================================================================
    // H1-H10 typed query surface.
    //
    // The helpers are vocabulary-agnostic: callers pass the type and
    // predicate strings of their own module (synthesist passes
    // "synthesist:Task", "synthesist:supersedes", ...). claim itself
    // owns no vocabulary.
    // ==================================================================

    /// H1: count claims by `@type`. NOT live-filtered: every claim with
    /// that type, superseded or not. Index-native: a POS range scan.
    pub fn count_by_type(&self, type_value: &str) -> Result<usize> {
        Ok(self.subjects_with(TYPE_PRED, type_value)?.len())
    }

    /// H1 variant: count claims of `@type` that also carry
    /// `(pred, value)`. NOT live-filtered -- every version of every
    /// matching claim, superseded or not. Mirrors a SPARQL
    /// `SELECT (COUNT(DISTINCT ?c)) WHERE { ?c rdf:type {type}; pred value }`
    /// with no `FILTER NOT EXISTS`. Index-native: two POS range scans
    /// intersected on the subject column.
    pub fn count_by_type_and_value(
        &self,
        type_value: &str,
        pred: &str,
        value: &str,
    ) -> Result<usize> {
        let typed: HashSet<String> = self
            .subjects_with(TYPE_PRED, type_value)?
            .into_iter()
            .collect();
        let matching = self.subjects_with(pred, value)?;
        Ok(matching.into_iter().filter(|s| typed.contains(s)).count())
    }

    /// H1: total claim count (every doc in the index).
    pub fn count_total(&self) -> Result<usize> {
        let r = self.db.begin_read()?;
        let docs = match r.open_table(DOCS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
            Err(e) => return Err(e).context("open docs table"),
        };
        Ok(docs.len()? as usize)
    }

    /// H2: live heads of `type_value` -- claims of that type not
    /// superseded by any later claim. The dominant query shape. Returns
    /// the live claim ids; callers fetch docs or columns as needed.
    ///
    /// `supersedes_pred` is the module's supersession predicate (e.g.
    /// `synthesist:supersedes`).
    pub fn live_heads(&self, type_value: &str, supersedes_pred: &str) -> Result<Vec<String>> {
        let superseded = self.superseded_set(supersedes_pred)?;
        let mut live: Vec<String> = self
            .subjects_with(TYPE_PRED, type_value)?
            .into_iter()
            .filter(|s| !superseded.contains(s))
            .collect();
        live.sort();
        Ok(live)
    }

    /// H2 helper: the value of a scalar predicate on a claim, read from
    /// the doc (handles single-valued props). Returns the first member
    /// for set-valued props.
    pub fn scalar(&self, claim_id: &str, pred: &str) -> Result<Option<String>> {
        Ok(self.values_of(claim_id, pred)?.into_iter().next())
    }

    /// H3: live Task heads with their native `dependsOn` / `files`
    /// vectors read straight from the doc (no GROUP_CONCAT round-trip).
    ///
    /// `task_type` is the module's task `@type`; `supersedes_pred` the
    /// supersession predicate; `status_pred` the module's status
    /// predicate (e.g. `synthesist:status`); `depends_pred` /
    /// `files_pred` the multi-valued reference predicates.
    pub fn live_tasks(
        &self,
        task_type: &str,
        supersedes_pred: &str,
        status_pred: &str,
        depends_pred: &str,
        files_pred: &str,
    ) -> Result<Vec<LiveTask>> {
        let live = self.live_heads(task_type, supersedes_pred)?;
        let mut out = Vec::with_capacity(live.len());
        for id in live {
            let doc = self
                .doc(&id)?
                .ok_or_else(|| anyhow::anyhow!("live task {id} missing from docs table"))?;
            let status = doc_scalar(&doc, status_pred);
            out.push(LiveTask {
                id: id.clone(),
                status,
                depends_on: doc_string_array(&doc, depends_pred),
                files: doc_string_array(&doc, files_pred),
            });
        }
        Ok(out)
    }

    /// H4: live session openers. A session opener is a claim of
    /// `session_type` that is BOTH (a) not superseded by any later claim
    /// AND (b) does not itself supersede an earlier claim -- the dual
    /// anti-join that separates openers from closers sharing a session
    /// id.
    pub fn live_session_openers(
        &self,
        session_type: &str,
        supersedes_pred: &str,
    ) -> Result<Vec<String>> {
        let superseded = self.superseded_set(supersedes_pred)?;
        let supersedes = self.supersedes_something(supersedes_pred)?;
        let mut out: Vec<String> = self
            .subjects_with(TYPE_PRED, session_type)?
            .into_iter()
            .filter(|s| !superseded.contains(s) && !supersedes.contains(s))
            .collect();
        out.sort();
        Ok(out)
    }

    /// H4: the live opener carrying a given `sessionId`, if any.
    ///
    /// `session_id_pred` is the module's session-id predicate; `id` the
    /// raw session id value.
    pub fn session_opener_by_id(
        &self,
        session_type: &str,
        supersedes_pred: &str,
        session_id_pred: &str,
        id: &str,
    ) -> Result<Option<String>> {
        let openers = self.live_session_openers(session_type, supersedes_pred)?;
        let with_id: HashSet<String> = self
            .subjects_with(session_id_pred, id)?
            .into_iter()
            .collect();
        Ok(openers.into_iter().find(|o| with_id.contains(o)))
    }

    /// H5: is a session live? True when a live opener carries `id`. The
    /// only boolean ASK in the surface; same dual anti-join as H4.
    pub fn session_is_live(
        &self,
        session_type: &str,
        supersedes_pred: &str,
        session_id_pred: &str,
        id: &str,
    ) -> Result<bool> {
        Ok(self
            .session_opener_by_id(session_type, supersedes_pred, session_id_pred, id)?
            .is_some())
    }

    /// H6: the head-of-chain phase claim for a session. Walks the
    /// supersession chain of `phase_type` claims carrying `sessionId ==
    /// session_id` and returns the one not superseded by a later phase
    /// claim of the same session.
    ///
    /// Only supersedes edges where both the superseder and the target are
    /// `phase_type` claims for this session are consulted, so a cross-type
    /// or cross-session supersedes edge cannot drop a phase head.
    ///
    /// Well-formed logs produce exactly one live head per session; if more
    /// than one survives (a data anomaly), the lexicographically first is
    /// returned for determinism.
    ///
    /// Returns `None` when the session has no phase claim.
    pub fn current_phase(
        &self,
        phase_type: &str,
        supersedes_pred: &str,
        session_id_pred: &str,
        session_id: &str,
    ) -> Result<Option<String>> {
        // Collect all Phase claims for this session.
        let typed: HashSet<String> = self
            .subjects_with(TYPE_PRED, phase_type)?
            .into_iter()
            .collect();
        let session_phases: HashSet<String> = self
            .subjects_with(session_id_pred, session_id)?
            .into_iter()
            .filter(|s| typed.contains(s))
            .collect();

        // Build a superseded set restricted to Phase claims of this session:
        // only count a supersedes edge if the superseder itself is in
        // session_phases, preventing cross-type or cross-session edges from
        // dropping a phase.
        let all_edges = self.objects_of(supersedes_pred)?;
        let superseded: HashSet<String> = all_edges
            .into_iter()
            .filter(|(superseder, _)| session_phases.contains(superseder))
            .map(|(_, target)| target)
            .collect();

        let mut heads: Vec<String> = session_phases
            .into_iter()
            .filter(|s| !superseded.contains(s))
            .collect();
        heads.sort();
        Ok(heads.into_iter().next())
    }

    /// H7: dangling supersedes -- supersede targets that do not exist as
    /// a claim in the index (orphan references). An integrity check.
    pub fn dangling_supersedes(&self, supersedes_pred: &str) -> Result<Vec<DanglingEdge>> {
        let pairs = self.objects_of(supersedes_pred)?;
        let r = self.db.begin_read()?;
        let docs = match r.open_table(DOCS) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(e).context("open docs table"),
        };
        let mut out = Vec::new();
        for (superseder, target) in pairs {
            if docs.get(target.as_str())?.is_none() {
                out.push(DanglingEdge { superseder, target });
            }
        }
        out.sort_by(|a, b| {
            a.superseder
                .cmp(&b.superseder)
                .then(a.target.cmp(&b.target))
        });
        Ok(out)
    }

    /// H8: a task's acceptance criteria, read straight from the doc's
    /// nested `acceptance = [{criterion, verifyCmd}]` array. No
    /// triple-shredding; the nested array lives in the doc.
    ///
    /// `acceptance_pred` is the predicate key under which the array is
    /// stored (e.g. `synthesist:acceptance` or `acceptance`).
    pub fn task_acceptance(
        &self,
        claim_id: &str,
        acceptance_pred: &str,
    ) -> Result<Vec<AcceptanceCriterion>> {
        let doc = match self.doc(claim_id)? {
            Some(d) => d,
            None => return Ok(Vec::new()),
        };
        let arr = match doc.get(acceptance_pred).and_then(|v| v.as_array()) {
            Some(a) => a,
            None => return Ok(Vec::new()),
        };
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let criterion = item
                .get("criterion")
                .or_else(|| item.get("synthesist:criterion"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let verify_cmd = item
                .get("verifyCmd")
                .or_else(|| item.get("synthesist:verifyCmd"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            out.push(AcceptanceCriterion {
                criterion,
                verify_cmd,
            });
        }
        Ok(out)
    }

    /// H9: diamond conflicts -- prior claims superseded by MORE THAN ONE
    /// distinct LIVE superseder. A group-by over the supersedes edge
    /// index, keeping only superseders that are themselves live heads.
    pub fn diamond_conflicts(&self, supersedes_pred: &str) -> Result<Vec<DiamondConflict>> {
        let superseded = self.superseded_set(supersedes_pred)?;
        let edges = self.objects_of(supersedes_pred)?; // (superseder, prior)
        let mut by_prior: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (superseder, prior) in edges {
            // Keep only superseders that are themselves live (not
            // superseded by anything later).
            if superseded.contains(&superseder) {
                continue;
            }
            by_prior.entry(prior).or_default().insert(superseder);
        }
        let mut out = Vec::new();
        for (prior, supers) in by_prior {
            if supers.len() > 1 {
                out.push(DiamondConflict {
                    prior,
                    superseders: supers.into_iter().collect(),
                });
            }
        }
        Ok(out)
    }

    /// H10: plan-at-risk -- specs whose agreed plan snapshot contains a
    /// claim that has since been superseded by a NEWER claim.
    ///
    /// For each `spec_type` claim with an `agree_pred` set (e.g.
    /// `agreeSnapshot`), for each snapshot member superseded by some
    /// `new_claim`, emit a hit when `new_claim`'s `prov:generatedAtTime`
    /// is lexically greater than the spec's own `prov:generatedAtTime`
    /// (the AGREE timestamp). Lexical compare is chronologically correct
    /// for the canonical `now_iso` format (asserted at ingest time).
    pub fn plan_at_risk(
        &self,
        spec_type: &str,
        agree_pred: &str,
        supersedes_pred: &str,
    ) -> Result<Vec<PlanAtRiskHit>> {
        // superseders keyed by the prior claim they supersede.
        let edges = self.objects_of(supersedes_pred)?; // (superseder, prior)
        let mut superseders_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (superseder, prior) in edges {
            superseders_of.entry(prior).or_default().push(superseder);
        }

        let mut hits = Vec::new();
        for spec in self.subjects_with(TYPE_PRED, spec_type)? {
            let spec_doc = match self.doc(&spec)? {
                Some(d) => d,
                None => continue,
            };
            let spec_agreed_at = match doc_scalar(&spec_doc, GENERATED_AT_PRED) {
                Some(t) => t,
                None => continue,
            };
            let snapshot = doc_string_array(&spec_doc, agree_pred);
            for member in snapshot {
                let Some(supers) = superseders_of.get(&member) else {
                    continue;
                };
                for new_claim in supers {
                    let Some(new_doc) = self.doc(new_claim)? else {
                        continue;
                    };
                    let Some(new_at) = doc_scalar(&new_doc, GENERATED_AT_PRED) else {
                        continue;
                    };
                    // Lexical compare == chronological for canonical form.
                    if new_at > spec_agreed_at {
                        let stakeholder =
                            doc_scalar(&new_doc, ATTRIBUTED_TO_PRED).unwrap_or_default();
                        hits.push(PlanAtRiskHit {
                            spec: spec.clone(),
                            old_claim: member.clone(),
                            new_claim: new_claim.clone(),
                            stakeholder,
                            new_at,
                        });
                    }
                }
            }
        }
        hits.sort_by(|a, b| {
            a.spec
                .cmp(&b.spec)
                .then(a.new_claim.cmp(&b.new_claim))
                .then(a.old_claim.cmp(&b.old_claim))
        });
        Ok(hits)
    }
}

// ----------------------------------------------------------------------
// Typed result structs returned by the H-helpers.
// ----------------------------------------------------------------------

/// A live Task head with its native multi-valued reference vectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTask {
    pub id: String,
    pub status: Option<String>,
    pub depends_on: Vec<String>,
    pub files: Vec<String>,
}

/// One acceptance criterion of a Task (H8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceCriterion {
    pub criterion: String,
    pub verify_cmd: String,
}

/// A supersedes edge whose target is absent from the index (H7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanglingEdge {
    pub superseder: String,
    pub target: String,
}

/// A prior claim superseded by more than one live superseder (H9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiamondConflict {
    pub prior: String,
    pub superseders: Vec<String>,
}

/// One (spec, superseding-claim) plan-at-risk pair (H10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanAtRiskHit {
    pub spec: String,
    pub old_claim: String,
    pub new_claim: String,
    pub stakeholder: String,
    pub new_at: String,
}

/// Stats from a gamma rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebuildStats {
    /// Canonical docs loaded into the index.
    pub claims_loaded: usize,
    /// POS/PSO triples written.
    pub triples_count: u64,
    /// Log lines that failed to parse as a JSON object.
    pub parse_failures: usize,
    /// `prov:generatedAtTime` values that did not match the canonical
    /// `now_iso` format. Non-fatal: counted so callers can flag a
    /// corrupt or foreign timestamp, but the value is still indexed.
    pub datetime_violations: usize,
}

// ----------------------------------------------------------------------
// Free helpers.
// ----------------------------------------------------------------------

/// Assert the canonical `now_iso` dateTime format:
/// `%Y-%m-%dT%H:%M:%S.NNNZ` (fixed width, zero-padded, millis, always Z).
///
/// Lexical order over strings in this exact shape equals chronological
/// order, which is what the H10 comparison relies on. This is a shape
/// check, not a calendar check: it does not validate that month <= 12.
pub fn is_canonical_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    // YYYY-MM-DDTHH:MM:SS.NNNZ -> 24 chars.
    if b.len() != 24 {
        return false;
    }
    let digit = |i: usize| b[i].is_ascii_digit();
    (0..4).all(digit)
        && b[4] == b'-'
        && digit(5)
        && digit(6)
        && b[7] == b'-'
        && digit(8)
        && digit(9)
        && b[10] == b'T'
        && digit(11)
        && digit(12)
        && b[13] == b':'
        && digit(14)
        && digit(15)
        && b[16] == b':'
        && digit(17)
        && digit(18)
        && b[19] == b'.'
        && digit(20)
        && digit(21)
        && digit(22)
        && b[23] == b'Z'
}

/// Index a single triple into both POS and PSO.
fn insert_triple(
    pos: &mut redb::Table<&str, ()>,
    pso: &mut redb::Table<&str, ()>,
    pred: &str,
    subject: &str,
    object: &str,
) -> Result<()> {
    let pos_key = format!("{pred}{SEP}{object}{SEP}{subject}");
    let pso_key = format!("{pred}{SEP}{subject}{SEP}{object}");
    pos.insert(pos_key.as_str(), ()).context("insert pos")?;
    pso.insert(pso_key.as_str(), ()).context("insert pso")?;
    Ok(())
}

/// Extract the indexable scalar members of a JSON value.
///
/// - String -> one member (the string).
/// - Array  -> each STRING element is a member; nested objects are NOT
///   shredded (they remain in the doc, read by H8).
/// - Bool / Number -> stringified (rare on the query surface).
/// - Object / Null -> no members (read from the doc when needed).
fn scalar_members(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => vec![s.clone()],
        Value::Bool(b) => vec![b.to_string()],
        Value::Number(n) => vec![n.to_string()],
        Value::Array(items) => items
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Read a scalar (string) property from a doc, handling the single-value
/// case. For array properties returns the first string member.
fn doc_scalar(doc: &Value, pred: &str) -> Option<String> {
    match doc.get(pred)? {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => items.iter().find_map(|v| v.as_str().map(|s| s.to_string())),
        _ => None,
    }
}

/// Read a string-array property from a doc. A single string yields a
/// one-element vector; missing yields empty.
fn doc_string_array(doc: &Value, pred: &str) -> Vec<String> {
    match doc.get(pred) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

// redb 4 has no table truncate; drain by collecting then removing keys.
fn clear_table(t: &mut redb::Table<&str, ()>) -> Result<()> {
    let keys: Vec<String> = t
        .iter()?
        .map(|item| item.map(|(k, _)| k.value().to_string()))
        .collect::<std::result::Result<_, _>>()?;
    for k in keys {
        t.remove(k.as_str())?;
    }
    Ok(())
}

fn clear_pso(t: &mut redb::Table<&str, ()>) -> Result<()> {
    clear_table(t)
}

fn clear_docs(t: &mut redb::Table<&str, &str>) -> Result<()> {
    let keys: Vec<String> = t
        .iter()?
        .map(|item| item.map(|(k, _)| k.value().to_string()))
        .collect::<std::result::Result<_, _>>()?;
    for k in keys {
        t.remove(k.as_str())?;
    }
    Ok(())
}

fn clear_meta(t: &mut redb::Table<&str, &str>) -> Result<()> {
    let keys: Vec<String> = t
        .iter()?
        .map(|item| item.map(|(k, _)| k.value().to_string()))
        .collect::<std::result::Result<_, _>>()?;
    for k in keys {
        t.remove(k.as_str())?;
    }
    Ok(())
}
