//! Storage over the `claims/` directory.
//!
//! Wraps an Automerge document whose on-disk representation is:
//!
//! ```text
//! claims/
//!   genesis.amc          bootstrap (tracked)
//!   changes/<hash>.amc   append-only changes, content-addressed (tracked)
//!   snapshot.amc         local compaction cache (gitignored)
//!   config.toml          schema version (tracked)
//! ```
//!
//! The `Store` owns reads and writes. Callers never touch files in
//! `claims/` directly. See BUILDING.md §"Files inside `claims/`" for the
//! locked layout (D3).
//!
//! # Example
//!
//! ```no_run
//! use nomograph_claim::{Claim, ClaimType, Store};
//! use std::path::Path;
//!
//! let tmp = tempfile::tempdir().unwrap();
//! let root = tmp.path().join("claims");
//! let mut store = Store::init(&root).unwrap();
//! let claim = Claim::new(
//!     ClaimType::Spec,
//!     serde_json::json!({"goal": "demo"}),
//!     "user:gitlab:andunn",
//! );
//! store.append(&claim).unwrap();
//! let loaded = store.load_claims().unwrap();
//! assert_eq!(loaded.len(), 1);
//! ```

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use automerge::transaction::Transactable;
use automerge::{AutoCommit, ChangeHash, ObjId, ObjType, ROOT, ReadDoc};

use fs4::fs_std::FileExt;

use crate::claim::{AsserterId, Claim, ClaimId, ClaimType};
use crate::error::{Error, Result};

/// Schema version written to `config.toml` on [`Store::init`].
pub const SCHEMA_VERSION: &str = "0.1";

/// Directory under the claim root holding append-only change files.
const CHANGES_DIR: &str = "changes";
/// Genesis bootstrap file name.
const GENESIS_FILE: &str = "genesis.amc";
/// Snapshot cache file name (gitignored).
const SNAPSHOT_FILE: &str = "snapshot.amc";
/// Config file name.
const CONFIG_FILE: &str = "config.toml";
/// In-flight snapshot target during rename dance.
const SNAPSHOT_NEW_FILE: &str = "snapshot.amc.new";
/// File extension for content-addressed change files.
const CHANGE_EXT: &str = "amc";
/// Key under which the claim list is stored in the Automerge root map.
const CLAIMS_KEY: &str = "claims";
/// Advisory lockfile name, used to serialize compact() against
/// concurrent append() within the same claims/ directory. File content
/// is irrelevant; only fs-level flock state matters. Gitignored.
const LOCK_FILE: &str = ".lock";

/// On-disk claim store rooted at a project's `claims/` directory.
///
/// A `Store` is the single handle through which all claim reads and
/// writes flow. The handle owns an in-memory [`AutoCommit`] document;
/// every call to [`Store::append`] writes an incremental change file
/// atomically to `changes/<hash>.amc`.
pub struct Store {
    /// Absolute path to the `claims/` directory.
    root: PathBuf,
    /// In-memory Automerge document.
    doc: AutoCommit,
}

impl Store {
    /// Initialize a fresh `claims/` directory at `root`.
    ///
    /// Creates `root`, writes `genesis.amc`, creates `changes/`, and
    /// writes `config.toml`. Errors if `genesis.amc` already exists at
    /// `root`; call [`Store::open`] instead.
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

        // Build an empty doc with the claims list at a stable ObjId.
        // save() compacts to the canonical seed shape so every peer
        // that loads genesis observes the same ObjId (per 01 Amendment A).
        let mut doc = AutoCommit::new();
        doc.put_object(ROOT, CLAIMS_KEY, ObjType::List)?;
        doc.commit();
        let seed = doc.save();

        fs::create_dir_all(root.join(CHANGES_DIR))?;
        atomic_write(&genesis_path, &seed)?;

        let cfg = format!("schema_version = \"{}\"\n", SCHEMA_VERSION);
        atomic_write(&root.join(CONFIG_FILE), cfg.as_bytes())?;

        fsync_dir(&root)?;

        // Rehydrate from the bytes we just wrote so the in-memory doc's
        // ObjId matches every peer that will open this genesis.
        let doc = AutoCommit::load(&seed)?;
        Ok(Self { root, doc })
    }

    /// Open an existing `claims/` directory at `root`.
    ///
    /// Loads `genesis.amc` first (required; errors with
    /// [`Error::MissingGenesis`] if absent). Then applies `snapshot.amc`
    /// when present and non-corrupt (falling back to genesis-only on a
    /// snapshot load error, per 05 rule #3). Finally applies every
    /// `changes/*.amc` file via `load_incremental`.
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
                    // Rebuild doc from genesis only; subsequent compaction
                    // will rewrite a clean snapshot.
                    doc = AutoCommit::load(&genesis_bytes)?;
                }
            }
        }

        for path in list_change_files(&root)? {
            // A partially-written or corrupt `.amc` in `changes/` must
            // not crash the load. Partial writes can happen if a process
            // crashes between `File::create` and `sync_all` on append;
            // the adversarial review (HIGH #5) caught that the snapshot
            // path has recovery above but changes did not.
            //
            // Policy: read-skip. If the file is unreadable or
            // load_incremental rejects it, log and continue. The change
            // is effectively lost — it was not yet on beacon, so we
            // accept that loss rather than making the entire store
            // unopenable.
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

    /// Append a claim to the log.
    ///
    /// Inserts the claim into the Automerge `claims` list, saves the
    /// incremental change to `changes/<hash>.amc` (atomic write + dir
    /// fsync), and returns. The write is NOT silent on disk failure: any
    /// io error propagates and leaves the in-memory doc in a committed
    /// state whose change is not yet on disk (caller retries).
    ///
    /// Concurrent-writer safety: this method acquires an exclusive flock
    /// on `claims/.lock` for the duration of the write. Multiple
    /// processes appending to the same `claims/` directory serialize on
    /// this lock and cannot race with [`Store::compact`]
    /// (ADVERSARIAL-REVIEW CRITICAL #2). The lock is released when the
    /// guard drops, including on panic or process crash (kernel flock
    /// semantics).
    pub fn append(&mut self, claim: &Claim) -> Result<()> {
        let _lock = DirLock::exclusive(&self.root)?;
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

        let written = self.save_incremental()?;
        if written.is_none() {
            // Automerge dedupe inside the same actor elided the change; we
            // still consider this a non-error because the log is unchanged.
        }
        Ok(())
    }

    /// Load every claim currently present in the doc.
    ///
    /// Deduplicates by [`Claim::id`]: per Amendment in overnight-01 the
    /// Automerge list may contain the same id more than once when peers
    /// independently asserted it. Callers who want raw list order should
    /// access the doc directly (not part of the public API).
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

    /// CRDT-merge another store's doc into this one.
    ///
    /// Persists a new incremental change file capturing the merged delta
    /// so peers observe the merged state after a subsequent [`Store::open`].
    pub fn merge(&mut self, other: &mut Store) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        self.save_incremental()?;
        Ok(())
    }

    /// Compact the change log into `snapshot.amc`.
    ///
    /// Performs the rename dance (05 rule #2): write `snapshot.amc.new`,
    /// verify by round-trip load, rename over `snapshot.amc`, then delete
    /// the superseded `changes/*.amc`. Rolls back the `.new` file on any
    /// verification or rename error so the previous good snapshot is
    /// preserved.
    pub fn compact(&mut self) -> Result<()> {
        // Hold the directory lock across snapshot write + changes sweep
        // so no other process can slip a fresh change file into
        // `changes/` between our rename and our sweep. Without this the
        // compact races against concurrent append() calls on the same
        // claims/ (ADVERSARIAL-REVIEW CRITICAL #2).
        let _lock = DirLock::exclusive(&self.root)?;
        let snapshot_path = self.root.join(SNAPSHOT_FILE);
        let staging = self.root.join(SNAPSHOT_NEW_FILE);

        // 1. Serialize the full doc.
        let bytes = self.doc.save();

        // 2. Write to .new via atomic tmp+rename; then verify.
        atomic_write(&staging, &bytes)?;
        let verify_bytes = fs::read(&staging)?;
        if AutoCommit::load(&verify_bytes).is_err() {
            let _ = fs::remove_file(&staging);
            return Err(Error::Corrupt(format!(
                "snapshot staging failed verification at {}; retry compact",
                staging.display()
            )));
        }

        // 3. Rename .new over snapshot.amc (atomic on POSIX).
        if let Err(e) = fs::rename(&staging, &snapshot_path) {
            let _ = fs::remove_file(&staging);
            return Err(Error::Io(e));
        }
        fsync_dir(&self.root)?;

        // 4. Sweep superseded changes/*.amc. A failure here leaves a
        //    benign partial state (self-heals next compaction) per 05 S2.
        let changes = self.root.join(CHANGES_DIR);
        if changes.exists() {
            for path in list_change_files(&self.root)? {
                fs::remove_file(&path)?;
            }
            fsync_dir(&changes)?;
        }
        Ok(())
    }

    /// Persist pending Automerge changes as `changes/<hash>.amc`.
    ///
    /// Returns `Ok(None)` when there is nothing new to flush. Returns
    /// `Ok(Some((path, bytes)))` when a change file was written.
    /// Atomic: writes to `<hash>.amc.tmp` then renames (05 rule #1).
    pub fn save_incremental(&mut self) -> Result<Option<(PathBuf, usize)>> {
        let bytes = self.doc.save_incremental();
        if bytes.is_empty() {
            return Ok(None);
        }
        let changes_dir = self.root.join(CHANGES_DIR);
        fs::create_dir_all(&changes_dir)?;
        let name = format!("{}.{}", blake3::hash(&bytes).to_hex(), CHANGE_EXT);
        let path = changes_dir.join(&name);
        let size = bytes.len();
        atomic_write(&path, &bytes)?;
        fsync_dir(&changes_dir)?;
        Ok(Some((path, size)))
    }

    /// Current Automerge heads for this store.
    ///
    /// Used by [`crate::view::View`] to detect staleness. Takes `&mut`
    /// because [`automerge::AutoCommit::get_heads`] closes any in-flight
    /// transaction before returning.
    pub fn heads(&mut self) -> Vec<ChangeHash> {
        self.doc.get_heads()
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
                "claims list missing at root; re-run Store::init at {}",
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
                "field `{}` is not a string; re-run Store::init to reset",
                key
            ))),
        },
        None => Err(Error::Corrupt(format!(
            "field `{}` missing from claim entry; check appender wrote it",
            key
        ))),
    }
}

fn get_str_opt(doc: &AutoCommit, obj: &ObjId, key: &str) -> Result<Option<String>> {
    match doc.get(obj, key)? {
        Some((val, _)) => match val.to_str() {
            Some(s) => Ok(Some(s.to_string())),
            None => Err(Error::Corrupt(format!(
                "field `{}` is not a string; re-run Store::init to reset",
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
                "field `{}` is not an integer; re-run Store::init to reset",
                key
            ))),
        },
        None => Err(Error::Corrupt(format!(
            "field `{}` missing from claim entry; check appender wrote it",
            key
        ))),
    }
}

fn get_int_opt(doc: &AutoCommit, obj: &ObjId, key: &str) -> Result<Option<i64>> {
    match doc.get(obj, key)? {
        Some((val, _)) => match val.to_i64() {
            Some(i) => Ok(Some(i)),
            None => Err(Error::Corrupt(format!(
                "field `{}` is not an integer; re-run Store::init to reset",
                key
            ))),
        },
        None => Ok(None),
    }
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

/// Atomic write via `<path>.tmp` + rename (05 rule #1).
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
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

/// RAII guard around the `claims/.lock` file. Serializes mutating
/// operations against each other within a single `claims/` directory
/// across processes on the same host.
///
/// Acquire via [`DirLock::exclusive`]. The flock is released when the
/// guard drops (the file closes). On crash the kernel releases it too,
/// so a dead holder never wedges the directory.
///
/// Scope: advisory POSIX flock on unix, LockFileEx on Windows, via fs4.
/// Network filesystems (NFS without lockd, SMB) do NOT participate; the
/// lock is best-effort there. Synthesist assumes local filesystems.
pub(crate) struct DirLock {
    _file: fs::File,
}

impl DirLock {
    /// Acquire an exclusive lock on `<claims_dir>/.lock`, creating the
    /// file if missing. Blocks until granted; see fs4 for behavior.
    ///
    /// Used by both [`Store::append`] and [`Store::compact`] for 0.1.
    /// Future work: differentiate shared-append vs exclusive-compact
    /// once append throughput becomes load-bearing.
    pub fn exclusive(claims_dir: &Path) -> Result<Self> {
        fs::create_dir_all(claims_dir)?;
        let path = claims_dir.join(LOCK_FILE);
        let file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        file.lock_exclusive()?;
        Ok(Self { _file: file })
    }
}

/// Fsync a directory so rename/unlink entries are durable (05 rule #4).
///
/// No-op on platforms where opening a directory for fsync is not
/// supported by `std::fs`.
pub(crate) fn fsync_dir(dir: &Path) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        match fs::File::open(dir) {
            Ok(f) => {
                f.sync_all()?;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Io(e)),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = dir;
        Ok(())
    }
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
    fn atomic_write_leaves_no_tmp() {
        let (_tmp, root) = fresh();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("data.bin");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        let mut entries: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        entries.sort();
        assert_eq!(entries, vec!["data.bin".to_string()]);
    }

    #[test]
    fn init_refuses_to_clobber() {
        let (_tmp, root) = fresh();
        let _store = Store::init(&root).unwrap();
        let err = match Store::init(&root) {
            Ok(_) => panic!("expected init to refuse existing genesis"),
            Err(e) => e,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("genesis already present"), "msg was: {msg}");
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
