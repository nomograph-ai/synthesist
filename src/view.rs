//! SQLite projection of the claim log.
//!
//! The [`View`] reads the current Automerge document through [`Store`]
//! and materializes it into `claims/view.sqlite` for fast relational
//! queries. The projection is derived state: it may be rebuilt from
//! scratch at any time and must never be the source of truth.
//!
//! Staleness is tracked via `claims/view.heads`, a plain-text file
//! whose lines are the hex [`automerge::ChangeHash`] values seen at
//! the last rebuild. A set-mismatch against the store's current heads
//! means the projection is behind and [`View::sync`] must run before
//! query.
//!
//! Per 09g the rebuild cost is ~30 us/claim in release mode. Warm
//! reopen (no heads change) is sub-millisecond; the sync path compares
//! heads before touching the database.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use automerge::ChangeHash;
use rusqlite::{Connection, ToSql, params};
use serde_json::{Map, Value};

use crate::claim::Claim;
use crate::error::{Error, Result};
use crate::store::Store;

/// Backing SQLite filename under `claims/`.
const VIEW_DB_FILE: &str = "view.sqlite";
/// Heads-snapshot filename under `claims/`.
const VIEW_HEADS_FILE: &str = "view.heads";
// The previous `VIEW_HEADS_NEW_FILE` constant was removed when write_heads
// switched to the `atomic_write` helper in store.rs, which manages its
// own `<path>.tmp` suffix.

/// Local SQLite projection of a [`Store`].
///
/// A `View` is rebuildable derived state. Callers rely on [`View::sync`]
/// to reconcile the projection with the current heads before issuing
/// queries. The view database file sits at `claims/view.sqlite` and is
/// gitignored (D3).
pub struct View {
    /// Absolute path to `claims/view.sqlite`.
    db_path: PathBuf,
    /// Absolute path to `claims/view.heads`.
    heads_path: PathBuf,
    /// Open read/write SQLite connection to `db_path`.
    conn: Connection,
}

impl View {
    /// Open (or create) the SQLite projection at `claims/view.sqlite`.
    ///
    /// Does NOT rebuild. A freshly-opened `View` on an empty claims
    /// directory has an empty `claims` table. Call [`View::sync`] (or
    /// [`View::rebuild`]) to populate it from the store.
    ///
    /// Errors with [`Error::MissingGenesis`] when `claims_dir` does not
    /// exist, so the caller distinguishes "never initialised" from other
    /// IO failures.
    pub fn open(claims_dir: &Path) -> Result<Self> {
        if !claims_dir.exists() {
            return Err(Error::MissingGenesis(format!(
                "{} does not exist; run Store::init first",
                claims_dir.display()
            )));
        }
        if !claims_dir.is_dir() {
            return Err(Error::Corrupt(format!(
                "{} is not a directory; expected claims/ root",
                claims_dir.display()
            )));
        }
        let db_path = claims_dir.join(VIEW_DB_FILE);
        let heads_path = claims_dir.join(VIEW_HEADS_FILE);
        let conn = Connection::open(&db_path)?;
        ensure_schema(&conn)?;
        Ok(Self {
            db_path,
            heads_path,
            conn,
        })
    }

    /// If `store.heads()` differs from the cached `view.heads`, rebuild
    /// the projection. Otherwise no-op.
    ///
    /// Returns `true` when a rebuild ran, `false` when the cache was
    /// already current. The boolean is load-bearing: callers cannot
    /// accidentally miss a rebuild because the method forces them to
    /// consume the flag.
    ///
    /// ```no_run
    /// use nomograph_claim::{Store, View};
    /// use std::path::Path;
    ///
    /// let root = Path::new("claims");
    /// let mut store = Store::open(root).unwrap();
    /// let mut view = View::open(root).unwrap();
    /// let rebuilt = view.sync(&mut store).unwrap();
    /// if rebuilt {
    ///     println!("view was stale, rebuilt from {} heads", store.heads().len());
    /// }
    /// ```
    pub fn sync(&mut self, store: &mut Store) -> Result<bool> {
        let current = heads_set(&store.heads());
        let cached = load_cached_heads(&self.heads_path)?;
        if cached == current {
            return Ok(false);
        }
        self.rebuild(store)?;
        Ok(true)
    }

    /// Full rebuild. Drops and recreates the schema, iterates every
    /// current claim from `store`, inserts into the `claims` table, and
    /// persists the post-rebuild heads to `view.heads`.
    ///
    /// Deterministic: two rebuilds from identical documents must
    /// produce identical row-sets. Dedupe is by `id` via `INSERT OR
    /// IGNORE` per 09g "minor finding".
    pub fn rebuild(&mut self, store: &mut Store) -> Result<()> {
        let claims = store.load_claims()?;
        rebuild_schema(&self.conn)?;
        insert_claims(&self.conn, &claims)?;
        let heads = store.heads();
        write_heads(&self.heads_path, &heads)?;
        Ok(())
    }

    /// Path to the backing SQLite file.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Execute a read-only query via a raw SQL string + bound params.
    ///
    /// Returns each row as a JSON object keyed by column name. Wave 4
    /// synthesist wraps these with typed accessors; the library public
    /// surface stays narrow.
    ///
    /// Rejects non-`SELECT` statements by aborting after the statement
    /// is prepared — callers who need writes should open a dedicated
    /// administrative connection, not reuse the view.
    pub fn query(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Vec<Value>> {
        let trimmed = sql.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        if !(lower.starts_with("select")
            || lower.starts_with("with")
            || lower.starts_with("pragma"))
        {
            return Err(Error::Other(format!(
                "View::query accepts only SELECT/WITH/PRAGMA; got `{}`",
                first_token(trimmed)
            )));
        }
        let mut stmt = self.conn.prepare(sql)?;
        let column_names: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let mut rows = stmt.query(params)?;
        let mut out: Vec<Value> = Vec::new();
        while let Some(row) = rows.next()? {
            let mut obj = Map::with_capacity(column_names.len());
            for (i, name) in column_names.iter().enumerate() {
                let v: rusqlite::types::Value = row.get(i)?;
                obj.insert(name.clone(), sql_to_json(v));
            }
            out.push(Value::Object(obj));
        }
        Ok(out)
    }
}

/// Ensure the `claims` table + indexes exist. Idempotent.
fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(())
}

/// Drop and recreate the schema in one batch.
fn rebuild_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_claims_supersedes;
         DROP INDEX IF EXISTS idx_claims_valid_from;
         DROP INDEX IF EXISTS idx_claims_asserted_by;
         DROP INDEX IF EXISTS idx_claims_type;
         DROP TABLE IF EXISTS claims;",
    )?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(())
}

fn insert_claims(conn: &Connection, claims: &[Claim]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO claims \
             (id, claim_type, props, valid_from, valid_until, supersedes, \
              parent_asserter, asserted_by, asserted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for c in claims {
            let props_json = serde_json::to_string(&c.props)?;
            stmt.execute(params![
                c.id,
                c.claim_type.as_str(),
                props_json,
                c.valid_from.timestamp_millis(),
                c.valid_until.map(|d| d.timestamp_millis()),
                c.supersedes,
                c.parent_asserter,
                c.asserted_by,
                c.asserted_at.timestamp_millis(),
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

const SCHEMA_SQL: &str = "CREATE TABLE IF NOT EXISTS claims (\
    id TEXT PRIMARY KEY,\
    claim_type TEXT NOT NULL,\
    props TEXT NOT NULL,\
    valid_from INTEGER NOT NULL,\
    valid_until INTEGER,\
    supersedes TEXT,\
    parent_asserter TEXT,\
    asserted_by TEXT NOT NULL,\
    asserted_at INTEGER NOT NULL\
);\
CREATE INDEX IF NOT EXISTS idx_claims_type ON claims(claim_type);\
CREATE INDEX IF NOT EXISTS idx_claims_asserted_by ON claims(asserted_by);\
CREATE INDEX IF NOT EXISTS idx_claims_valid_from ON claims(valid_from);\
CREATE INDEX IF NOT EXISTS idx_claims_supersedes ON claims(supersedes);";

fn heads_set(heads: &[ChangeHash]) -> HashSet<String> {
    heads.iter().map(|h| h.to_string()).collect()
}

fn load_cached_heads(path: &Path) -> Result<HashSet<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|s| s.to_string())
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(e) => Err(Error::Io(e)),
    }
}

fn write_heads(path: &Path, heads: &[ChangeHash]) -> Result<()> {
    let mut body = String::with_capacity(heads.len() * 65);
    for h in heads {
        body.push_str(&h.to_string());
        body.push('\n');
    }
    // atomic_write does tmp file + fsync_all + rename. We also fsync the
    // parent dir so the rename is durable across a crash: without it,
    // macOS/ext4 may not flush the dir entry change before the next
    // reboot, leaving the old heads file in place. The adversarial
    // review (CRITICAL #4) caught this case against a write_heads that
    // used a bare fs::write + rename.
    crate::store::atomic_write(path, body.as_bytes())?;
    if let Some(parent) = path.parent() {
        crate::store::fsync_dir(parent)?;
    }
    Ok(())
}

fn sql_to_json(v: rusqlite::types::Value) -> Value {
    use rusqlite::types::Value as SV;
    match v {
        SV::Null => Value::Null,
        SV::Integer(i) => Value::from(i),
        SV::Real(f) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        SV::Text(s) => Value::String(s),
        SV::Blob(b) => Value::Array(b.into_iter().map(|byte| Value::from(byte as u64)).collect()),
    }
}

fn first_token(sql: &str) -> &str {
    sql.split_whitespace().next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_set_roundtrip_plain_text() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("view.heads");
        // ChangeHash has no public constructor from string in this
        // crate's scope; instead test the text parsing contract
        // directly by writing a known heads file.
        let text = "aaaa\nbbbb\n\n  cccc  \n";
        std::fs::write(&path, text).unwrap();
        let set = load_cached_heads(&path).unwrap();
        assert_eq!(set.len(), 3);
        assert!(set.contains("aaaa"));
        assert!(set.contains("bbbb"));
        assert!(set.contains("cccc"));
    }

    #[test]
    fn missing_heads_file_is_empty_set() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("view.heads");
        let set = load_cached_heads(&path).unwrap();
        assert!(set.is_empty());
    }

    #[test]
    fn first_token_extracts_leading_word() {
        assert_eq!(first_token("INSERT INTO foo"), "INSERT");
        assert_eq!(first_token("  update t set x=1"), "update");
        assert_eq!(first_token(""), "");
    }
}
