//! Synthesist's v3-native Store facade.
//!
//! This is the v3-native cut: no v2 Automerge substrate, no Oxigraph
//! N-Quads view, no `.amc` dual-write. Every write lands in the v3
//! JSON-LD log; every read is a typed query against the disposable
//! redb gamma index.
//!
//! The v2 substrate retired:
//!   - the Automerge Store/View that synthesist used to delegate to
//!   - the Oxigraph N-Quads snapshot reparsed on every process open
//!   - Automerge `.amc` change files under `claims/changes/`
//!
//! The migration path (`migrations::v2_to_v3`) still reaches through
//! the minimal v2-read shim that `nomograph_claim` keeps so an existing
//! `.amc` estate can be drained into v3 logs. That code path is
//! untouched.
//!
//! ## Write contract
//!
//! `SynthStore::append` validates `props` against the synthesist
//! schema (strict-on-write), computes a deterministic claim id, and
//! appends one v3 JSON-LD document via
//! [`nomograph_claim::log::LogWriter`]. The append needs an asserter
//! to be set (`with_asserter` or `discover_for`); without it the
//! write fails fast because attribution is required for every claim.
//!
//! `SynthStore::append_replay` bypasses the per-type validator (for
//! import / migration) but still writes a v3 doc.
//!
//! ## Read contract
//!
//! All reads go through the typed query helpers on the gamma index.
//! `SynthStore` opens the disposable redb gamma index
//! (`nomograph_claim::gamma::Gamma::open`) on the first read and
//! reuses it for the rest of the process. The index rebuilds from the
//! per-asserter logs only when the cache is absent or stale, so a cold
//! open against a 1.5K-claim corpus finishes in milliseconds when
//! heads have not changed.
//!
//! Commands that need to walk every claim raw (`cmd_check`,
//! `cmd_export`) get an iterator via `iter_claims`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::claim_type::ClaimType;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

// Leaf helpers folded in from the dropped workflow crate. Every
// downstream site imports these via `crate::store::{...}`, so defining
// them here as local `pub` items keeps those call sites compiling
// unchanged. Their unit tests live in this module's `tests`.

/// Directory inside a project that holds the claim substrate.
///
/// Visible name, plural, no nicknames.
pub const CLAIMS_DIR: &str = "claims";

/// Split a `tree/spec` identifier into its two parts. Prescriptive
/// error on malformed input so CLI users see the expected shape.
pub fn parse_tree_spec(input: &str) -> Result<(String, String)> {
    let (tree, spec) = input
        .split_once('/')
        .context("identifier must be <tree>/<spec>, e.g. keaton/graphs")?;
    if tree.is_empty() || spec.is_empty() {
        anyhow::bail!("identifier must be <tree>/<spec>, e.g. keaton/graphs");
    }
    Ok((tree.to_string(), spec.to_string()))
}

/// Today's date as `YYYY-MM-DD` in local time. Used by commands that
/// default a `date` / `event_date` prop to "today". This is the
/// date-only helper; do not conflate it with `cmd_outcome`'s
/// `today_iso` (a full timestamp).
pub fn today() -> String {
    use time::macros::format_description;
    let fmt = format_description!("[year]-[month]-[day]");
    time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(&fmt)
        .unwrap_or_else(|_| "1970-01-01".into())
}

/// Render a JSON value as a single line on stdout. Every synthesist
/// command emits JSON by convention; routing through this helper keeps
/// the output shape consistent.
pub fn json_out(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string(v).context("serialize output")?);
    Ok(())
}

/// Walk upward from `start` looking for a v1 `.synth/main.db`.
/// Returns the path to the first one found, or `None`.
///
/// Used inside the discover path so a bad directory produces a
/// prescriptive error instead of a silent empty-estate creation.
pub fn find_legacy_v1_db(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        let legacy = cur.join(".synth").join("main.db");
        if legacy.is_file() {
            return Some(legacy);
        }
        if !cur.pop() {
            break;
        }
    }
    None
}

/// Build the standardized migration error. Centralized so every caller
/// emits an identical prescriptive message naming the exact
/// `synthesist migrate v2-to-v3` subcommand to run and pointing at
/// `MIGRATION-v2-to-v3.md` for rollback + verification steps.
pub fn legacy_migration_error(legacy_db: &Path) -> anyhow::Error {
    // legacy_db = <repo>/.synth/main.db. Compute <repo>/claims as the
    // suggested migration target.
    let target = legacy_db
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("claims"))
        .unwrap_or_else(|| PathBuf::from("claims"));
    anyhow::anyhow!(
        "found legacy v1 database at `{legacy}` but no v3 `claims/` directory in scope.\n\
         \n\
         synthesist v3 stores data in `claims/` (per-asserter JSON-LD logs + redb gamma index).\n\
         The in-place `migrate v1-to-v2` path was removed; bring data forward through the\n\
         v2-to-v3 migration surface instead. Inspect what is available, then run it:\n\
         \n\
         \u{20}\u{20}synthesist migrate list\n\
         \u{20}\u{20}synthesist migrate status\n\
         \u{20}\u{20}synthesist migrate v2-to-v3 --from {legacy} --to {target}\n\
         \u{20}\u{20}synthesist migrate run\n\
         \n\
         See MIGRATION-v2-to-v3.md in the synthesist repo for rollback + verification.\n\
         If the legacy db is obsolete and you want a fresh v3 estate here,\n\
         remove `.synth/` first.",
        legacy = legacy_db.display(),
        target = target.display(),
    )
}

/// File name (under `claims/`) of the gamma index cache.
///
/// The gamma index is a DISPOSABLE local redb FILE (`claims/_view.gamma`)
/// rebuilt from the per-asserter log union whenever the heads signal
/// moves; it is gitignored. This is the single source for the cache
/// path -- every site that needs it ([`gamma_view_path`], cmd_task's
/// overlay open, cmd_overlay, and the cli help text) references this
/// constant rather than hardcoding the name.
pub const GAMMA_VIEW_DIR: &str = "_view.gamma";

/// The gamma index path for a given `claims/` directory.
pub fn gamma_view_path(claims_dir: &Path) -> PathBuf {
    claims_dir.join(GAMMA_VIEW_DIR)
}

/// v3 claim id type. Identical to the substrate's `ClaimId` (an
/// opaque string carrying the blake3 content hash, hex encoded).
pub type ClaimId = String;

/// Synthesist's v3-native Store facade.
///
/// Wraps a per-process [`nomograph_claim::log::LogWriter`] for writes
/// and a lazily-opened [`nomograph_claim::gamma::Gamma`] index for
/// reads. The index is cached on the instance: every command opens its
/// store once, runs as many typed queries as it needs, drops the
/// store. The redb gamma cache amortizes the rebuild cost across CLI
/// invocations.
pub struct SynthStore {
    claims_root: PathBuf,
    log_writer: Option<nomograph_claim::log::LogWriter>,
    gamma: RefCell<Option<nomograph_claim::gamma::Gamma>>,
    asserter: Option<String>,
}

impl SynthStore {
    fn from_claims_dir(claims_root: PathBuf) -> Self {
        let log_writer = nomograph_claim::log::LogWriter::new(&claims_root).ok();
        Self {
            claims_root,
            log_writer,
            gamma: RefCell::new(None),
            asserter: None,
        }
    }
}

#[allow(dead_code)]
impl SynthStore {
    /// Discover from `SYNTHESIST_DIR` env var or cwd walk-up.
    pub fn discover() -> Result<Self> {
        if let Ok(raw) = std::env::var("SYNTHESIST_DIR")
            && !raw.is_empty()
        {
            return Self::open_explicit(Path::new(&raw));
        }
        let cwd = std::env::current_dir().context("cwd")?;
        Self::discover_from(&cwd)
    }

    /// Open the store at an explicit path (`SYNTHESIST_DIR` / `--data-dir`).
    ///
    /// The path names the directory CONTAINING `claims/`. Fails loudly
    /// if `claims/` is missing -- silent fallback to `init_at` would
    /// mask a misconfigured path.
    fn open_explicit(dir: &Path) -> Result<Self> {
        if !dir.exists() {
            bail!(
                "SYNTHESIST_DIR / --data-dir points at `{}` which does not exist",
                dir.display()
            );
        }
        if !dir.is_dir() {
            bail!(
                "SYNTHESIST_DIR / --data-dir points at `{}` which is not a directory",
                dir.display()
            );
        }
        let claims = dir.join(CLAIMS_DIR);
        if !claims.is_dir() {
            return Err(anyhow!(
                "SYNTHESIST_DIR / --data-dir points at `{}` but no `{}/` directory is present there. \
                 Run `synthesist init` in that directory first, or unset the override.",
                dir.display(),
                CLAIMS_DIR
            ));
        }
        Self::open_at(&claims)
    }

    /// Walk up from `start` looking for a `claims/` directory, opening
    /// the first hit. Falls back to `init_at(start/claims)` if none
    /// found (and there's no v1 legacy db to bail on).
    pub fn discover_from(start: &Path) -> Result<Self> {
        let mut cur = start.to_path_buf();
        loop {
            let candidate = cur.join(CLAIMS_DIR);
            // Accept either v3 (a directory with any per-asserter logs)
            // or v2 (legacy genesis.amc) since the migration tool may
            // still need to read a v2-shaped estate to convert it. The
            // runtime read path goes through the gamma index either way
            // -- v2 genesis.amc files don't populate the index, so a
            // pure-v2 estate just renders as empty until migrated.
            if candidate.is_dir() {
                return Self::open_at(&candidate);
            }
            if !cur.pop() {
                break;
            }
        }
        if let Some(legacy) = find_legacy_v1_db(start) {
            return Err(legacy_migration_error(&legacy));
        }
        Self::init_at(&start.join(CLAIMS_DIR))
    }

    /// Discover and scope the asserter with an optional session id.
    pub fn discover_for(session: &Option<String>) -> Result<Self> {
        let mut s = Self::discover()?;
        let base = local_asserter_base();
        let asserter = match session {
            Some(id) if !id.is_empty() => format!("{base}:{id}"),
            _ => base,
        };
        s.asserter = Some(asserter);
        Ok(s)
    }

    /// Open at an explicit `claims/` directory.
    pub fn open_at(claims_dir: &Path) -> Result<Self> {
        if !claims_dir.is_dir() {
            bail!("claims path is not a directory: {}", claims_dir.display());
        }
        Ok(Self::from_claims_dir(claims_dir.to_path_buf()))
    }

    /// Initialize a fresh store at `claims_dir`. Idempotent.
    pub fn init_at(claims_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(claims_dir)
            .with_context(|| format!("create claims dir {}", claims_dir.display()))?;
        Ok(Self::from_claims_dir(claims_dir.to_path_buf()))
    }

    /// Override the asserter. Required before any `append` call.
    pub fn with_asserter(mut self, asserted_by: impl Into<String>) -> Self {
        self.asserter = Some(asserted_by.into());
        // Asserter change does not invalidate the cached view.
        self
    }

    /// The `claims/` directory backing this store.
    pub fn root(&self) -> &Path {
        &self.claims_root
    }

    /// The asserter string this store will use for subsequent appends.
    pub fn asserted_by(&self) -> Option<&str> {
        self.asserter.as_deref()
    }

    /// Append a typed claim (validated).
    ///
    /// Validates `props` against the synthesist schema for `claim_type`
    /// before writing. Builds a v3 JSON-LD document via
    /// [`crate::wire_format`], writes it via
    /// [`nomograph_claim::log::LogWriter`], and returns the computed
    /// claim hash.
    ///
    /// Requires an asserter (set via `with_asserter` or `discover_for`).
    pub fn append(
        &mut self,
        claim_type: ClaimType,
        props: Value,
        supersedes: Option<ClaimId>,
    ) -> Result<ClaimId> {
        crate::schema::validate_props(&claim_type, &props)
            .map_err(anyhow::Error::from)
            .context("validate claim before append")?;
        self.append_inner(claim_type, props, supersedes)
    }

    /// Replay an existing claim into the store without per-type
    /// validation. Used by migration and import paths.
    pub fn append_replay(
        &mut self,
        claim_type: ClaimType,
        props: Value,
        supersedes: Option<ClaimId>,
    ) -> Result<ClaimId> {
        self.append_inner(claim_type, props, supersedes)
    }

    fn append_inner(
        &mut self,
        claim_type: ClaimType,
        props: Value,
        supersedes: Option<ClaimId>,
    ) -> Result<ClaimId> {
        let asserter = self.asserter.as_deref().ok_or_else(|| {
            anyhow!(
                "SynthStore::append requires an asserter; call with_asserter or discover_for first"
            )
        })?;
        let writer = self
            .log_writer
            .as_ref()
            .ok_or_else(|| anyhow!(
                "SynthStore log writer is not initialized; claims root may be missing or unreadable: {}",
                self.claims_root.display()
            ))?;

        // Deterministic claim id: blake3 over a canonical encoding of
        // (claim_type, props, asserter, generated_at). The substrate's
        // `Claim::compute_id` uses the same blake3-over-canonical-form
        // approach with different inputs. Here we sample the wall
        // clock once and use it for BOTH the @id hash AND the
        // generatedAtTime field so the two stay consistent within the
        // emitted document.
        let now = chrono::Utc::now();
        let generated_at = now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let claim_id = compute_claim_id(&claim_type, &props, asserter, &generated_at);

        let doc = build_jsonld_doc(
            &claim_id,
            &claim_type,
            &props,
            asserter,
            &generated_at,
            supersedes.as_deref(),
        );

        writer
            .append(asserter, &doc)
            .context("v3 log writer append")?;

        // Any cached gamma index is now stale. Drop it; the next read
        // re-opens and re-syncs against the new log line via the heads
        // check.
        *self.gamma.borrow_mut() = None;

        Ok(claim_id)
    }

    /// Iterate raw claims from the log union, in deterministic
    /// (genesis-first, then asserter-dir-lexicographic) order. Used by
    /// `cmd_check` and `cmd_export` which need the raw documents.
    pub fn iter_claims(&self) -> Result<impl Iterator<Item = Value> + '_> {
        let reader = nomograph_claim::log::LogReader::new(&self.claims_root)?;
        Ok(reader.iter_claims().filter_map(|r| r.ok().map(|c| c.raw)))
    }

    // ==================================================================
    // Gamma-backed typed query surface.
    //
    // These replace the retired SPARQL `sparql`/`ask` gateways. The
    // gamma index is vocabulary-agnostic, so each helper passes the
    // synthesist `@type` and predicate IRIs (from `wire_format`) as
    // arguments. The index is opened lazily on first read and reused for
    // the rest of the process (a write drops it so the next read
    // re-syncs against the new log line).
    // ==================================================================

    fn ensure_gamma(&self) -> Result<()> {
        if self.gamma.borrow().is_some() {
            return Ok(());
        }
        let view_dir = gamma_view_path(&self.claims_root);
        let g = nomograph_claim::gamma::Gamma::open(&view_dir, &self.claims_root)
            .context("open gamma index")?;
        *self.gamma.borrow_mut() = Some(g);
        Ok(())
    }

    /// Run `f` against the lazily-opened gamma index.
    fn with_gamma<T>(
        &self,
        f: impl FnOnce(&nomograph_claim::gamma::Gamma) -> Result<T>,
    ) -> Result<T> {
        self.ensure_gamma()?;
        let g = self.gamma.borrow();
        f(g.as_ref().expect("ensure_gamma"))
    }

    /// Live head ids of `type_value` (e.g. `synthesist:Task`), sorted.
    /// The dominant live-head anti-join (gamma H2).
    pub fn live_heads(&self, type_value: &str) -> Result<Vec<String>> {
        self.with_gamma(|g| g.live_heads(type_value, crate::wire_format::SUPERSEDES_PRED))
    }

    /// Live heads of `type_value` paired with their full JSON-LD docs.
    pub fn live_docs(&self, type_value: &str) -> Result<Vec<(String, Value)>> {
        self.with_gamma(|g| {
            let ids = g.live_heads(type_value, crate::wire_format::SUPERSEDES_PRED)?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(doc) = g.doc(&id)? {
                    out.push((id, doc));
                }
            }
            Ok(out)
        })
    }

    /// Fetch the canonical JSON-LD doc for a claim id (compact form,
    /// `synthesist:claim/<short>`).
    pub fn doc(&self, claim_id: &str) -> Result<Option<Value>> {
        self.with_gamma(|g| g.doc(claim_id))
    }

    /// H3: live Task heads with their native dep/file vectors.
    pub fn live_tasks(&self) -> Result<Vec<nomograph_claim::gamma::LiveTask>> {
        self.with_gamma(|g| {
            g.live_tasks(
                &crate::wire_format::type_iri("task"),
                crate::wire_format::SUPERSEDES_PRED,
                &crate::wire_format::predicate_iri("status"),
                &crate::wire_format::predicate_iri("depends_on"),
                &crate::wire_format::predicate_iri("files"),
            )
        })
    }

    /// H4: the live Session opener carrying `id`, if any.
    pub fn session_opener_by_id(&self, id: &str) -> Result<Option<String>> {
        self.with_gamma(|g| {
            g.session_opener_by_id(
                &crate::wire_format::type_iri("session"),
                crate::wire_format::SUPERSEDES_PRED,
                &crate::wire_format::predicate_iri("id"),
                id,
            )
        })
    }

    /// H4: live Session openers (no `id` filter).
    pub fn live_session_openers(&self) -> Result<Vec<String>> {
        self.with_gamma(|g| {
            g.live_session_openers(
                &crate::wire_format::type_iri("session"),
                crate::wire_format::SUPERSEDES_PRED,
            )
        })
    }

    /// H5: is the session carrying `id` live? Replaces `ask`.
    pub fn session_is_live(&self, id: &str) -> Result<bool> {
        self.with_gamma(|g| {
            g.session_is_live(
                &crate::wire_format::type_iri("session"),
                crate::wire_format::SUPERSEDES_PRED,
                &crate::wire_format::predicate_iri("id"),
                id,
            )
        })
    }

    /// H6: the head-of-chain Phase claim id for `session_id`, if any.
    pub fn current_phase(&self, session_id: &str) -> Result<Option<String>> {
        self.with_gamma(|g| {
            g.current_phase(
                &crate::wire_format::type_iri("phase"),
                crate::wire_format::SUPERSEDES_PRED,
                &crate::wire_format::predicate_iri("session_id"),
                session_id,
            )
        })
    }

    /// H7: supersedes edges whose target is absent from the log.
    pub fn dangling_supersedes(&self) -> Result<Vec<nomograph_claim::gamma::DanglingEdge>> {
        self.with_gamma(|g| g.dangling_supersedes(crate::wire_format::SUPERSEDES_PRED))
    }

    /// H8: a task's acceptance criteria (nested array, read from the doc).
    pub fn task_acceptance(
        &self,
        claim_id: &str,
    ) -> Result<Vec<nomograph_claim::gamma::AcceptanceCriterion>> {
        self.with_gamma(|g| {
            g.task_acceptance(claim_id, &crate::wire_format::predicate_iri("acceptance"))
        })
    }

    /// H9: diamond conflicts (prior superseded by >1 live superseder).
    pub fn diamond_conflicts(&self) -> Result<Vec<nomograph_claim::gamma::DiamondConflict>> {
        self.with_gamma(|g| g.diamond_conflicts(crate::wire_format::SUPERSEDES_PRED))
    }

    /// H10: plan-at-risk hits over the Spec agreeSnapshot edges.
    pub fn plan_at_risk(&self) -> Result<Vec<nomograph_claim::gamma::PlanAtRiskHit>> {
        self.with_gamma(|g| {
            g.plan_at_risk(
                &crate::wire_format::type_iri("spec"),
                &crate::wire_format::predicate_iri("agree_snapshot"),
                crate::wire_format::SUPERSEDES_PRED,
            )
        })
    }

    /// H1: total claim count in the index.
    pub fn count_total(&self) -> Result<usize> {
        self.with_gamma(|g| g.count_total())
    }

    /// H1: count claims with `@type == type_value`.
    pub fn count_by_type(&self, type_value: &str) -> Result<usize> {
        self.with_gamma(|g| g.count_by_type(type_value))
    }

    /// H1 variant: count all (non-live-filtered) claims of `type_value`
    /// that also carry `(pred, value)`.
    pub fn count_by_type_and_value(
        &self,
        type_value: &str,
        pred: &str,
        value: &str,
    ) -> Result<usize> {
        self.with_gamma(|g| g.count_by_type_and_value(type_value, pred, value))
    }
}

/// Strip the compact-claim-IRI prefix to recover the bare claim hash for
/// display / `supersedes` arguments. Accepts both the compact
/// `synthesist:claim/<hash>` form gamma returns and the expanded
/// `https://nomograph.org/synthesist/claim/<hash>` form.
pub fn short_claim_id(iri: &str) -> String {
    iri.strip_prefix("https://nomograph.org/synthesist/claim/")
        .or_else(|| iri.strip_prefix("synthesist:claim/"))
        .unwrap_or(iri)
        .to_string()
}

/// Project a v3 JSON-LD doc into a flat bare-key `props` object, the v2
/// shape the command surface produces. Drops the JSON-LD envelope
/// (`@context`, `@id`, `@type`, `prov:*`, `synthesist:supersedes`,
/// `nomograph:parentAsserter`) and rewrites `synthesist:<lowerCamel>`
/// keys to bare `snake_case`. This is the read-side inverse of the
/// write-path mapping (mirrors `crate::integrity::v3_to_v2_props`).
pub fn bare_props(doc: &Value) -> serde_json::Map<String, Value> {
    crate::integrity::v3_to_v2_props(doc)
        .as_object()
        .cloned()
        .unwrap_or_default()
}

/// Local asserter base derived from `$USER` (mirrors the v2 convention
/// so v3 logs route to the same per-asserter directories that v2
/// sessions did).
fn local_asserter_base() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".into());
    format!("user:local:{user}")
}

/// Deterministic claim id over a canonical (type, props, asserter,
/// generated_at) tuple. Returned as a hex string.
fn compute_claim_id(
    claim_type: &ClaimType,
    props: &Value,
    asserter: &str,
    generated_at: &str,
) -> String {
    let canon = serde_json::json!({
        "claim_type": claim_type.as_str(),
        "props": props,
        "asserter": asserter,
        "generated_at": generated_at,
    });
    let mut bytes = Vec::with_capacity(256);
    write_canonical(&canon, &mut bytes);
    blake3::hash(&bytes).to_hex().to_string()
}

/// Serialize a JSON value with recursively sorted object keys.
/// Cross-machine deterministic so two writers producing the same
/// logical claim land on the same id.
fn write_canonical(v: &Value, buf: &mut Vec<u8>) {
    match v {
        Value::Null => buf.extend_from_slice(b"null"),
        Value::Bool(true) => buf.extend_from_slice(b"true"),
        Value::Bool(false) => buf.extend_from_slice(b"false"),
        Value::Number(n) => buf.extend_from_slice(n.to_string().as_bytes()),
        Value::String(s) => {
            let escaped = serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
            buf.extend_from_slice(escaped.as_bytes());
        }
        Value::Array(items) => {
            buf.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                write_canonical(item, buf);
            }
            buf.push(b']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            buf.push(b'{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    buf.push(b',');
                }
                let escaped = serde_json::to_string(k).unwrap_or_else(|_| "\"\"".into());
                buf.extend_from_slice(escaped.as_bytes());
                buf.push(b':');
                write_canonical(&map[*k], buf);
            }
            buf.push(b'}');
        }
    }
}

/// Build the v3 JSON-LD document for a claim. Mirrors the
/// wire_format contract so the result round-trips through the gamma
/// index rebuild to produce the typed edges the query helpers expect.
fn build_jsonld_doc(
    claim_id: &str,
    claim_type: &ClaimType,
    props: &Value,
    asserter: &str,
    generated_at: &str,
    supersedes: Option<&str>,
) -> Value {
    use crate::wire_format as wf;
    use serde_json::Map;

    let mut doc: Map<String, Value> = Map::new();
    doc.insert("@context".into(), wf::jsonld_context());
    doc.insert("@id".into(), Value::String(wf::claim_iri(claim_id)));
    doc.insert(
        "@type".into(),
        Value::String(wf::type_iri(claim_type.as_str())),
    );
    doc.insert(
        wf::GENERATED_AT_PRED.into(),
        Value::String(generated_at.to_string()),
    );
    doc.insert(
        wf::ATTRIBUTED_TO_PRED.into(),
        Value::String(wf::asserter_iri(asserter)),
    );
    if let Some(sup) = supersedes {
        doc.insert(
            wf::SUPERSEDES_PRED.into(),
            Value::String(wf::claim_iri(sup)),
        );
    }
    if let Some(props_map) = props.as_object() {
        for (k, v) in props_map {
            doc.insert(wf::predicate_iri(k), v.clone());
        }
    }
    Value::Object(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    /// `append` validates and writes one v3 log line per successful call.
    #[test]
    fn append_validates_and_writes_one_line() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");

        let props = json!({
            "tree": "proj",
            "spec": "s1",
            "id": "t1",
            "summary": "hello",
            "status": "pending",
        });
        let _id = store.append(ClaimType::Task, props, None).unwrap();

        let log_path = claims.join("user-local-test-t1").join("log.jsonl");
        assert!(log_path.exists(), "v3 log must be created");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 1);

        let doc: Value = serde_json::from_str(lines[0]).unwrap();
        assert!(
            doc["@id"]
                .as_str()
                .unwrap()
                .starts_with("synthesist:claim/")
        );
        assert_eq!(doc["@type"].as_str().unwrap(), "synthesist:Task");
    }

    /// `append` rejects bad input via the per-type validator.
    #[test]
    fn append_rejects_invalid_props() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        // Spec missing required `goal`.
        let bad = json!({
            "tree": "k",
            "id": "x",
            "status": "active",
            "topics": ["x"],
        });
        let err = store.append(ClaimType::Spec, bad, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("validate claim before append") || msg.contains("goal"),
            "expected validator error, got: {msg}"
        );
    }

    /// `append_replay` bypasses the per-type validator (for migration).
    #[test]
    fn append_replay_skips_validator() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        let id = store
            .append_replay(ClaimType::Stakeholder, json!({"id": "alice"}), None)
            .expect("replay accepts unowned types");
        assert!(!id.is_empty());
    }

    /// `append` without an asserter fails fast.
    #[test]
    fn append_without_asserter_errors() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims).unwrap();
        let props = json!({
            "tree": "proj",
            "spec": "s1",
            "id": "t1",
            "summary": "hello",
            "status": "pending",
        });
        let err = store.append(ClaimType::Task, props, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("requires an asserter"),
            "expected asserter error, got: {msg}"
        );
    }

    /// The gamma index opens lazily and reflects an appended claim.
    #[test]
    fn gamma_live_heads_after_append() {
        let dir = tempdir().unwrap();
        let claims = dir.path().join("claims");
        let mut store = SynthStore::init_at(&claims)
            .unwrap()
            .with_asserter("user:local:test:t1");
        store
            .append(
                ClaimType::Task,
                json!({
                    "tree": "proj",
                    "spec": "s1",
                    "id": "t1",
                    "summary": "hello",
                    "status": "pending",
                }),
                None,
            )
            .unwrap();
        let heads = store
            .live_heads(&crate::wire_format::type_iri("task"))
            .unwrap();
        assert_eq!(heads.len(), 1, "one live Task head after append");
        let doc = store.doc(&heads[0]).unwrap().expect("doc for live head");
        assert_eq!(doc["@type"].as_str().unwrap(), "synthesist:Task");
    }

    /// Deterministic claim id: same inputs hash to the same id.
    #[test]
    fn compute_claim_id_is_deterministic() {
        let a = compute_claim_id(
            &ClaimType::Task,
            &json!({"id": "t1"}),
            "user:local:agd",
            "2026-05-29T00:00:00.000Z",
        );
        let b = compute_claim_id(
            &ClaimType::Task,
            &json!({"id": "t1"}),
            "user:local:agd",
            "2026-05-29T00:00:00.000Z",
        );
        assert_eq!(a, b);
        assert_eq!(a.len(), 64, "blake3 hex is 64 chars");
    }

    // -----------------------------------------------------------------
    // Folded workflow leaf-helper tests.
    // -----------------------------------------------------------------

    #[test]
    fn parse_tree_spec_ok() {
        let (t, s) = parse_tree_spec("keaton/graphs").unwrap();
        assert_eq!(t, "keaton");
        assert_eq!(s, "graphs");
    }

    #[test]
    fn parse_tree_spec_errors_on_missing_slash() {
        assert!(parse_tree_spec("keaton").is_err());
    }

    #[test]
    fn parse_tree_spec_errors_on_empty_halves() {
        assert!(parse_tree_spec("/graphs").is_err());
        assert!(parse_tree_spec("keaton/").is_err());
    }

    #[test]
    fn today_shape_is_yyyy_mm_dd() {
        let s = today();
        assert_eq!(s.len(), 10);
        assert_eq!(s.chars().nth(4), Some('-'));
        assert_eq!(s.chars().nth(7), Some('-'));
    }

    #[test]
    fn find_legacy_returns_none_for_clean_dir() {
        let tmp = tempdir().unwrap();
        assert!(find_legacy_v1_db(tmp.path()).is_none());
    }

    #[test]
    fn find_legacy_walks_upward() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".synth")).unwrap();
        std::fs::write(tmp.path().join(".synth/main.db"), b"fake").unwrap();
        let nested = tmp.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        let found = find_legacy_v1_db(&nested).expect("walks up");
        assert!(found.ends_with(".synth/main.db"));
    }

    #[test]
    fn legacy_error_names_target_dir() {
        let tmp = tempdir().unwrap();
        let db = tmp.path().join(".synth/main.db");
        let err = legacy_migration_error(&db).to_string();
        assert!(err.contains("v1 database"));
        assert!(err.contains("synthesist migrate v2-to-v3"));
        assert!(err.contains("MIGRATION-v2-to-v3.md"));
        // Target path = tmp/claims.
        assert!(err.contains(&tmp.path().join("claims").display().to_string()));
    }
}
