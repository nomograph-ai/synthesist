//! v2-read shim over a legacy `claims/` directory.
//!
//! Deprecated: v3 uses `log::LogWriter` and `log::LogReader`. This module
//! survives ONLY as a read-only shim so the v2-to-v3 migration can drain
//! an existing v2 Automerge log into the v3 per-asserter JSON-LD logs.
//! The write path (init/append/merge/compact) and its directory lock were
//! removed when the v2 substrate was retired.
//!
//! The on-disk v2 layout it reads:
//!
//! ```text
//! claims/
//!   genesis.amc          bootstrap
//!   changes/<hash>.amc   append-only changes, content-addressed
//!   snapshot.amc         local compaction cache (optional)
//! ```
//!
//! Depends only on automerge + `crate::{claim, error}` + chrono.

#![allow(deprecated)]

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ReadDoc, ROOT};

use crate::claim::{AsserterId, Claim, ClaimId, ClaimType};
use crate::error::{Error, Result};

/// Genesis bootstrap file name.
const GENESIS_FILE: &str = "genesis.amc";
/// Snapshot cache file name (gitignored).
const SNAPSHOT_FILE: &str = "snapshot.amc";
/// Directory under the claim root holding append-only change files.
const CHANGES_DIR: &str = "changes";
/// File extension for content-addressed change files.
const CHANGE_EXT: &str = "amc";
/// Key under which the claim list is stored in the Automerge root map.
const CLAIMS_KEY: &str = "claims";

/// Read-only handle over a legacy v2 `claims/` directory.
///
/// Open with [`Store::open`] and drain with [`Store::load_claims`]. The
/// handle owns an in-memory [`AutoCommit`] document reconstructed from
/// genesis + optional snapshot + every `changes/*.amc`.
#[deprecated(
    since = "3.0.0-pre.1",
    note = "v3 uses `log::LogWriter` and `log::LogReader`. This is a read-only shim for the v2-to-v3 migration."
)]
pub struct Store {
    /// Absolute path to the `claims/` directory.
    root: PathBuf,
    /// In-memory Automerge document.
    doc: AutoCommit,
}

impl Store {
    /// Initialize a fresh `claims/` directory at `root`.
    ///
    /// Retained ONLY so the v2-to-v3 migration test suite can construct a
    /// v2 fixture to migrate. Production v3 never writes through this path
    /// (it uses `log::LogWriter`). Writes `genesis.amc` + creates
    /// `changes/`. Errors if `genesis.amc` already exists.
    pub fn init(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        let genesis_path = root.join(GENESIS_FILE);
        if genesis_path.exists() {
            return Err(Error::Other(format!(
                "genesis already present at {}; call Store::open",
                genesis_path.display()
            )));
        }

        let mut doc = AutoCommit::new();
        doc.put_object(ROOT, CLAIMS_KEY, ObjType::List)?;
        doc.commit();
        let seed = doc.save();

        fs::create_dir_all(root.join(CHANGES_DIR))?;
        atomic_write(&genesis_path, &seed)?;

        // Rehydrate from the bytes we just wrote so the in-memory doc's
        // ObjId matches every peer that will open this genesis.
        let doc = AutoCommit::load(&seed)?;
        Ok(Self { root, doc })
    }

    /// Append a claim to the log.
    ///
    /// Retained ONLY for v2-to-v3 migration test fixtures (see `init`).
    /// Inserts the claim into the Automerge `claims` list and writes the
    /// incremental change to `changes/<hash>.amc`.
    pub fn append(&mut self, claim: &Claim) -> Result<()> {
        let list = self.claims_list()?;
        let len = self.doc.length(&list);
        let entry = self.doc.insert_object(&list, len, ObjType::Map)?;
        self.doc.put(&entry, "id", claim.id.as_str())?;
        self.doc
            .put(&entry, "claim_type", claim.claim_type.as_str())?;
        self.doc.put(
            &entry,
            "props",
            serde_json::to_string(&claim.props)?.as_str(),
        )?;
        self.doc
            .put(&entry, "valid_from", claim.valid_from.timestamp_millis())?;
        if let Some(vu) = claim.valid_until {
            self.doc.put(&entry, "valid_until", vu.timestamp_millis())?;
        }
        if let Some(sup) = &claim.supersedes {
            self.doc.put(&entry, "supersedes", sup.as_str())?;
        }
        if let Some(parent) = &claim.parent_asserter {
            self.doc.put(&entry, "parent_asserter", parent.as_str())?;
        }
        self.doc
            .put(&entry, "asserted_by", claim.asserted_by.as_str())?;
        self.doc
            .put(&entry, "asserted_at", claim.asserted_at.timestamp_millis())?;
        self.doc.commit();

        let bytes = self.doc.save_incremental();
        if !bytes.is_empty() {
            let changes_dir = self.root.join(CHANGES_DIR);
            fs::create_dir_all(&changes_dir)?;
            let name = format!("{}.{}", blake3::hash(&bytes).to_hex(), CHANGE_EXT);
            atomic_write(&changes_dir.join(&name), &bytes)?;
        }
        Ok(())
    }

    /// Open an existing `claims/` directory at `root`.
    ///
    /// Loads `genesis.amc` first (required; errors with
    /// [`Error::MissingGenesis`] if absent). Then applies `snapshot.amc`
    /// when present and non-corrupt (falling back to genesis-only on a
    /// snapshot load error). Finally applies every `changes/*.amc` file
    /// via `load_incremental`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let genesis_path = root.join(GENESIS_FILE);
        if !genesis_path.exists() {
            return Err(Error::MissingGenesis(genesis_path.display().to_string()));
        }
        let genesis_bytes = fs::read(&genesis_path)?;
        let mut doc = AutoCommit::load(&genesis_bytes)?;

        // Snapshot is optional and local-only. Corruption here must not
        // crash the load; fall back to genesis + changes replay.
        let snapshot_path = root.join(SNAPSHOT_FILE);
        if snapshot_path.exists() {
            match fs::read(&snapshot_path).and_then(|b| {
                doc.load_incremental(&b)
                    .map(|_| ())
                    .map_err(std::io::Error::other)
            }) {
                Ok(()) => {}
                Err(_) => {
                    // Rebuild doc from genesis only.
                    doc = AutoCommit::load(&genesis_bytes)?;
                }
            }
        }

        for path in list_change_files(&root)? {
            // A partially-written or corrupt `.amc` in `changes/` must
            // not crash the load. Policy: read-skip. If the file is
            // unreadable or load_incremental rejects it, log and continue.
            let bytes = match fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!(
                        "nomograph-claim: skipping unreadable change {} ({e})",
                        path.display()
                    );
                    continue;
                }
            };
            if let Err(e) = doc.load_incremental(&bytes) {
                eprintln!(
                    "nomograph-claim: skipping corrupt change {} ({e})",
                    path.display()
                );
                continue;
            }
        }

        Ok(Self { root, doc })
    }

    /// Load every claim currently present in the doc.
    ///
    /// Deduplicates by [`Claim::id`]: the Automerge list may contain the
    /// same id more than once when peers independently asserted it.
    pub fn load_claims(&mut self) -> Result<Vec<Claim>> {
        let list = self.claims_list()?;
        let n = self.doc.length(&list);
        let mut out: Vec<Claim> = Vec::with_capacity(n);
        let mut seen: HashSet<ClaimId> = HashSet::with_capacity(n);
        for i in 0..n {
            let entry = match self.doc.get(&list, i)? {
                Some((_, id)) => id,
                None => continue,
            };
            let claim = read_claim(&self.doc, &entry)?;
            if seen.insert(claim.id.clone()) {
                out.push(claim);
            }
        }
        Ok(out)
    }

    /// Path to this store's `claims/` directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    // ---- internals -------------------------------------------------

    fn claims_list(&self) -> Result<ObjId> {
        match self.doc.get(ROOT, CLAIMS_KEY)? {
            Some((_, id)) => Ok(id),
            None => Err(Error::Corrupt(format!(
                "claims list missing at root at {}",
                self.root.display()
            ))),
        }
    }
}

/// Read one claim map from the doc into a typed [`Claim`].
fn read_claim(doc: &AutoCommit, entry: &ObjId) -> Result<Claim> {
    let id = get_str(doc, entry, "id")?;
    let claim_type_str = get_str(doc, entry, "claim_type")?;
    let claim_type = parse_claim_type(&claim_type_str)?;
    let props_str = get_str(doc, entry, "props")?;
    let props: serde_json::Value = serde_json::from_str(&props_str)?;
    let valid_from_ms = get_int(doc, entry, "valid_from")?;
    let valid_from = chrono::DateTime::from_timestamp_millis(valid_from_ms).ok_or_else(|| {
        Error::Corrupt(format!(
            "valid_from {} not a valid timestamp_ms",
            valid_from_ms
        ))
    })?;
    let valid_until = match get_int_opt(doc, entry, "valid_until")? {
        Some(ms) => Some(chrono::DateTime::from_timestamp_millis(ms).ok_or_else(|| {
            Error::Corrupt(format!("valid_until {} not a valid timestamp_ms", ms))
        })?),
        None => None,
    };
    let supersedes = get_str_opt(doc, entry, "supersedes")?;
    let parent_asserter: Option<AsserterId> = get_str_opt(doc, entry, "parent_asserter")?;
    let asserted_by = get_str(doc, entry, "asserted_by")?;
    let asserted_at_ms = get_int(doc, entry, "asserted_at")?;
    let asserted_at = chrono::DateTime::from_timestamp_millis(asserted_at_ms).ok_or_else(|| {
        Error::Corrupt(format!(
            "asserted_at {} not a valid timestamp_ms",
            asserted_at_ms
        ))
    })?;
    Ok(Claim {
        id,
        claim_type,
        props,
        valid_from,
        valid_until,
        supersedes,
        parent_asserter,
        asserted_by,
        asserted_at,
    })
}

fn parse_claim_type(s: &str) -> Result<ClaimType> {
    match s {
        "tree" => Ok(ClaimType::Tree),
        "spec" => Ok(ClaimType::Spec),
        "task" => Ok(ClaimType::Task),
        "discovery" => Ok(ClaimType::Discovery),
        "campaign" => Ok(ClaimType::Campaign),
        "session" => Ok(ClaimType::Session),
        "phase" => Ok(ClaimType::Phase),
        "intent" => Ok(ClaimType::Intent),
        "heartbeat" => Ok(ClaimType::Heartbeat),
        "outcome" => Ok(ClaimType::Outcome),
        "directive" => Ok(ClaimType::Directive),
        "stakeholder" => Ok(ClaimType::Stakeholder),
        "topic" => Ok(ClaimType::Topic),
        "signal" => Ok(ClaimType::Signal),
        "disposition" => Ok(ClaimType::Disposition),
        other => Err(Error::Invalid(format!(
            "unknown claim_type `{}`; use one of tree/spec/task/discovery/campaign/session/phase/intent/heartbeat/outcome/directive/stakeholder/topic/signal/disposition",
            other
        ))),
    }
}

fn get_str(doc: &AutoCommit, obj: &ObjId, key: &str) -> Result<String> {
    match doc.get(obj, key)? {
        Some((val, _)) => match val.to_str() {
            Some(s) => Ok(s.to_string()),
            None => Err(Error::Corrupt(format!(
                "field `{}` is not a string",
                key
            ))),
        },
        None => Err(Error::Corrupt(format!(
            "field `{}` missing from claim entry",
            key
        ))),
    }
}

fn get_str_opt(doc: &AutoCommit, obj: &ObjId, key: &str) -> Result<Option<String>> {
    match doc.get(obj, key)? {
        Some((val, _)) => match val.to_str() {
            Some(s) => Ok(Some(s.to_string())),
            None => Err(Error::Corrupt(format!(
                "field `{}` is not a string",
                key
            ))),
        },
        None => Ok(None),
    }
}

fn get_int(doc: &AutoCommit, obj: &ObjId, key: &str) -> Result<i64> {
    match doc.get(obj, key)? {
        Some((val, _)) => match val.to_i64() {
            Some(i) => Ok(i),
            None => Err(Error::Corrupt(format!(
                "field `{}` is not an integer",
                key
            ))),
        },
        None => Err(Error::Corrupt(format!(
            "field `{}` missing from claim entry",
            key
        ))),
    }
}

fn get_int_opt(doc: &AutoCommit, obj: &ObjId, key: &str) -> Result<Option<i64>> {
    match doc.get(obj, key)? {
        Some((val, _)) => match val.to_i64() {
            Some(i) => Ok(Some(i)),
            None => Err(Error::Corrupt(format!(
                "field `{}` is not an integer",
                key
            ))),
        },
        None => Ok(None),
    }
}

/// Atomic write via `<path>.tmp` + rename. Used only by the retained
/// `init`/`append` fixture path.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp_path = PathBuf::from(tmp);
    {
        use std::io::Write;
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// List `changes/*.amc` files under `root`, sorted by path.
fn list_change_files(root: &Path) -> Result<Vec<PathBuf>> {
    let dir = root.join(CHANGES_DIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some(CHANGE_EXT) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("claims");
        (tmp, root)
    }

    #[test]
    fn open_without_genesis_is_missing() {
        let (_tmp, root) = fresh();
        fs::create_dir_all(&root).unwrap();
        let err = match Store::open(&root) {
            Ok(_) => panic!("expected open to fail without genesis"),
            Err(e) => e,
        };
        assert!(matches!(err, Error::MissingGenesis(_)));
    }

    #[test]
    fn parse_claim_type_rejects_unknown() {
        let err = parse_claim_type("bogus").unwrap_err();
        assert!(matches!(err, Error::Invalid(_)));
    }
}
