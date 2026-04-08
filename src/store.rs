//! Database access layer for synthesist.
//!
//! Owns the SQLite connection, schema initialization, and session management.
//! All database access goes through Store. No raw SQL elsewhere.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use crate::schema;

/// Data directory name. Visible, full name, no dot prefix.
pub const DATA_DIR: &str = "synthesist";
/// Database file within the data directory.
const DB_FILE: &str = "main.db";
/// Sessions subdirectory (gitignored).
const SESSIONS_DIR: &str = "sessions";

/// Store wraps a SQLite connection with synthesist-specific operations.
pub struct Store {
    pub conn: Connection,
    /// Root of the project (parent of the data directory).
    pub root: PathBuf,
    /// Path to the data directory.
    pub data_dir: PathBuf,
}

impl Store {
    /// Open the database at the given path. Sets PRAGMAs and creates schema.
    fn open(db_path: &Path, root: PathBuf, data_dir: PathBuf) -> Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("opening database at {}", db_path.display()))?;

        // PRAGMAs: journal_mode=DELETE (not WAL), foreign keys on, busy timeout 5s.
        conn.execute_batch(
            "PRAGMA journal_mode = DELETE;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;

        conn.execute_batch(schema::CREATE_SCHEMA)?;

        Ok(Store {
            conn,
            root,
            data_dir,
        })
    }

    /// Initialize a new synthesist data directory in the current directory.
    pub fn init(root: &Path) -> Result<Self> {
        let data_dir = root.join(DATA_DIR);
        if data_dir.join(DB_FILE).exists() {
            bail!(
                "already initialized: {} exists",
                data_dir.join(DB_FILE).display()
            );
        }
        fs::create_dir_all(&data_dir)?;

        // Create .gitignore for sessions directory.
        let sessions_dir = data_dir.join(SESSIONS_DIR);
        fs::create_dir_all(&sessions_dir)?;
        fs::write(
            sessions_dir.join(".gitignore"),
            "# Session databases are ephemeral; only main.db is tracked.\n*.db\n",
        )?;

        let db_path = data_dir.join(DB_FILE);
        Self::open(&db_path, root.to_path_buf(), data_dir)
    }

    /// Discover an existing synthesist database by walking parent directories.
    /// Opens main.db.
    pub fn discover() -> Result<Self> {
        let (root, data_dir) = Self::find_data_dir()?;
        let db_path = data_dir.join(DB_FILE);
        Self::open(&db_path, root, data_dir)
    }

    /// Discover and open the appropriate database for the given session.
    /// If a session .db file exists, opens it (isolated writes).
    /// If no session file exists, falls back to main.db (the session name
    /// is still used for ownership tracking via the owner field on tasks).
    pub fn discover_for(session: &Option<String>) -> Result<Self> {
        match session {
            Some(id) => {
                // Validate session ID against path traversal.
                if id.contains('/') || id.contains('\\') || id.contains("..") || id.is_empty() {
                    bail!("invalid session ID '{id}': must not contain path separators or '..'");
                }
                let (root, data_dir) = Self::find_data_dir()?;
                let session_path = data_dir.join(SESSIONS_DIR).join(format!("{id}.db"));
                if session_path.exists() {
                    Self::open(&session_path, root, data_dir)
                } else {
                    // No session file -- write to main.db. The session name
                    // is still used for logical ownership (task claim).
                    let db_path = data_dir.join(DB_FILE);
                    Self::open(&db_path, root, data_dir)
                }
            }
            None => Self::discover(),
        }
    }

    /// Walk parent directories to find the synthesist data directory.
    fn find_data_dir() -> Result<(PathBuf, PathBuf)> {
        let mut dir = std::env::current_dir()?;
        loop {
            let data_dir = dir.join(DATA_DIR);
            let db_path = data_dir.join(DB_FILE);
            if db_path.exists() {
                return Ok((dir, data_dir));
            }
            if !dir.pop() {
                bail!(
                    "no synthesist database found in any parent directory -- run 'synthesist init'"
                );
            }
        }
    }

    /// Path to the sessions directory.
    pub fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join(SESSIONS_DIR)
    }

    /// Path to the main database.
    pub fn main_db_path(&self) -> PathBuf {
        self.data_dir.join(DB_FILE)
    }

    /// Path to a session database.
    pub fn session_db_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(format!("{session_id}.db"))
    }

    /// Today's date as YYYY-MM-DD.
    pub fn today() -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }

    /// Generate the next sequential ID with a given prefix.
    /// Scans existing IDs matching the prefix and increments.
    pub fn next_id(&self, table: &str, tree: &str, spec: &str, prefix: &str) -> Result<String> {
        let query = format!(
            "SELECT id FROM [{}] WHERE tree = ?1 AND spec = ?2 AND id LIKE ?3 ORDER BY id",
            table
        );
        let pattern = format!("{prefix}%");
        let mut stmt = self.conn.prepare(&query)?;
        let ids: Vec<String> = stmt
            .query_map(rusqlite::params![tree, spec, pattern], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

        let max_num: u32 = ids
            .iter()
            .filter_map(|id| id.strip_prefix(prefix)?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);

        Ok(format!("{prefix}{}", max_num + 1))
    }
}

/// Output JSON to stdout with 2-space indentation.
pub fn json_out(value: &impl serde::Serialize) -> Result<()> {
    let stdout = std::io::stdout();
    let writer = stdout.lock();
    serde_json::to_writer_pretty(writer, value)?;
    println!();
    Ok(())
}

/// Parse "tree/spec" format into (tree, spec).
pub fn parse_tree_spec(s: &str) -> Result<(&str, &str)> {
    let (tree, spec) = s
        .split_once('/')
        .with_context(|| format!("expected tree/spec format, got '{s}'"))?;
    if tree.is_empty() || spec.is_empty() {
        bail!("expected tree/spec format with non-empty components, got '{s}'");
    }
    Ok((tree, spec))
}
