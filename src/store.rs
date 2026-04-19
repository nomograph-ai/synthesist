//! Synthesist store — thin adapter over `nomograph-claim`.
//!
//! v2: every workflow write becomes a typed claim via
//! [`SynthStore::append`]; every read runs SQL over the SQLite view
//! materialized by [`nomograph_claim::View`]. The old SQLite schema
//! and file-copy session machinery are gone.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nomograph_claim::{Claim, ClaimId, ClaimType, Store as ClaimStore, View};
use serde_json::Value;

/// Directory inside the repo that owns the claim substrate. Visible,
/// full name, no nicknames (per D3).
pub const CLAIMS_DIR: &str = "claims";

fn local_asserter() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".into());
    format!("user:local:{user}")
}

/// Synthesist's handle over a project's `claims/` directory.
///
/// Writes go through [`SynthStore::append`]; reads go through
/// [`SynthStore::query`]. Every append syncs the view before returning.
pub struct SynthStore {
    inner: ClaimStore,
    view: View,
    asserted_by: String,
    root: PathBuf,
}

impl SynthStore {
    /// Open (or initialize) a SynthStore at `<repo_root>/claims/`.
    /// Walks upward from cwd; initializes in cwd if none found.
    pub fn discover() -> Result<Self> {
        let cwd = std::env::current_dir().context("cwd")?;
        Self::discover_from(&cwd)
    }

    /// Like [`discover`] but starts from a given path.
    pub fn discover_from(start: &Path) -> Result<Self> {
        let mut cur = start.to_path_buf();
        loop {
            let candidate = cur.join(CLAIMS_DIR);
            if candidate.join("genesis.amc").is_file() {
                return Self::open_at(&candidate);
            }
            if !cur.pop() {
                break;
            }
        }
        Self::init_at(&start.join(CLAIMS_DIR))
    }

    /// Open at an explicit `claims/` directory.
    pub fn open_at(claims_dir: &Path) -> Result<Self> {
        let mut inner = ClaimStore::open(claims_dir)
            .with_context(|| format!("open claim store at {}", claims_dir.display()))?;
        let mut view = View::open(claims_dir)
            .with_context(|| format!("open view at {}", claims_dir.display()))?;
        view.sync(&mut inner).context("sync view on open")?;
        Ok(Self {
            inner,
            view,
            asserted_by: local_asserter(),
            root: claims_dir.to_path_buf(),
        })
    }

    /// Initialize a fresh SynthStore at `claims_dir`.
    pub fn init_at(claims_dir: &Path) -> Result<Self> {
        let inner = ClaimStore::init(claims_dir)
            .with_context(|| format!("init claim store at {}", claims_dir.display()))?;
        let view = View::open(claims_dir)
            .with_context(|| format!("open view at {}", claims_dir.display()))?;
        Ok(Self {
            inner,
            view,
            asserted_by: local_asserter(),
            root: claims_dir.to_path_buf(),
        })
    }

    /// v1 compatibility shim: discover, honoring an optional session id.
    pub fn discover_for(session: &Option<String>) -> Result<Self> {
        let mut s = Self::discover()?;
        if let Some(id) = session {
            s.asserted_by = format!("{}:{}", s.asserted_by, id);
        }
        Ok(s)
    }

    /// Override the asserter used for subsequent writes. Reserved for
    /// tests and for future callers that set asserter from a parsed
    /// session claim (e.g. beacon sync) rather than `USER`.
    #[allow(dead_code)]
    pub fn with_asserter(mut self, asserted_by: impl Into<String>) -> Self {
        self.asserted_by = asserted_by.into();
        self
    }

    /// Append a typed claim; syncs view before returning.
    ///
    /// Validates props against the per-type schema
    /// ([`nomograph_claim::schema::validate_claim`]) BEFORE the write,
    /// so a bad call site gets a prescriptive error without polluting
    /// the claim log. The substrate itself intentionally does not
    /// validate (per claim/src/schema.rs module docs: "validation at the
    /// boundary"); synthesist IS that boundary for its own callers.
    pub fn append(
        &mut self,
        claim_type: ClaimType,
        props: Value,
        supersedes: Option<ClaimId>,
    ) -> Result<ClaimId> {
        let mut claim = Claim::new(claim_type, props, self.asserted_by.clone());
        if let Some(prior) = supersedes {
            claim = claim.with_supersedes(prior);
        }
        nomograph_claim::schema::validate_claim(&claim).context("validate claim before append")?;
        self.inner.append(&claim).context("append claim")?;
        self.view
            .sync(&mut self.inner)
            .context("sync view after append")?;
        Ok(claim.id)
    }

    /// Run a read-only SQL query over the view. Rows are JSON objects.
    pub fn query(&self, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<Value>> {
        self.view.query(sql, params).context("view query")
    }

    /// The `claims/` directory backing this store.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Force a view rebuild.
    pub fn sync_view(&mut self) -> Result<()> {
        self.view.sync(&mut self.inner).context("sync view")?;
        Ok(())
    }

    /// Access the underlying claim store (session handles, compact, merge).
    pub fn inner(&mut self) -> &mut ClaimStore {
        &mut self.inner
    }
}

/// Retain the old `Store` alias so any still-unported M2/M3 call sites
/// compile against the new backing type without rewriting every import.
/// Remove after M2 + M3 land and all call sites use `SynthStore` directly.
pub type Store = SynthStore;

/// Split a `tree/spec` identifier into its two parts. Returns a
/// prescriptive error if the input is missing the `/`.
pub fn parse_tree_spec(input: &str) -> Result<(String, String)> {
    let (tree, spec) = input
        .split_once('/')
        .context("identifier must be <tree>/<spec>, e.g. keaton/graphs")?;
    if tree.is_empty() || spec.is_empty() {
        anyhow::bail!("identifier must be <tree>/<spec>, e.g. keaton/graphs");
    }
    Ok((tree.to_string(), spec.to_string()))
}

impl SynthStore {
    /// Today's date as `YYYY-MM-DD` in local time. Used by commands
    /// that default a `date` prop to "today".
    pub fn today() -> String {
        use time::macros::format_description;
        let fmt = format_description!("[year]-[month]-[day]");
        time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .format(&fmt)
            .unwrap_or_else(|_| "1970-01-01".into())
    }
}

/// Render a JSON value as a single line on stdout. Kept for v1 parity.
pub fn json_out(v: &Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(v).context("serialize output")?
    );
    Ok(())
}
