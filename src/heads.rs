//! View staleness check via a heads file.
//!
//! The graph view (`claims/_view.oxigraph/`) is a derived projection of
//! the per-asserter logs. To detect when the projection has fallen
//! behind the source-of-truth logs, the substrate records a hash of
//! the log union into `claims/_view.heads` (or alongside the view
//! directory, depending on caller's preference).
//!
//! The hash covers the sorted list of asserter directory names and
//! their per-file line counts (cheap to compute, deterministic). It
//! does not hash the content of every claim; for substrate-internal
//! "is the view fresh?" use that level of strictness is unnecessary.
//!
//! ## Usage
//!
//! ```ignore
//! let view_dir = claims_dir.join("_view.oxigraph");
//! let view = GraphView::open(&view_dir)?;
//! if !heads::heads_match(&view_dir, &claims_dir)? {
//!     rebuild(&view, &claims_dir)?;
//!     heads::write_heads(&view_dir, &claims_dir)?;
//! }
//! ```

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

/// File name for the heads marker, written next to the view directory.
pub const HEADS_FILE_NAME: &str = "view.heads";

/// Compute the current heads hash of the claim log union.
///
/// Walks `claims_dir` and produces a blake3 hash over:
///
/// - The sorted list of asserter directory names.
/// - For each, the count of lines in `log.jsonl` (the size of the
///   asserter's log in claims).
///
/// Deterministic across platforms with no locale, encoding, or
/// timestamp dependence. Cheap: O(number of asserters) plus one
/// linear read per log file.
pub fn current_heads(claims_dir: &Path) -> Result<String> {
    let mut hasher = blake3::Hasher::new();

    if !claims_dir.exists() {
        return Ok(hasher.finalize().to_hex().to_string());
    }

    let entries = fs::read_dir(claims_dir)
        .with_context(|| format!("read {}", claims_dir.display()))?;

    let mut asserter_dirs: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            !p.file_name()
                .map(|n| {
                    let s = n.to_string_lossy();
                    s.starts_with('_') || s.starts_with('.')
                })
                .unwrap_or(false)
        })
        .collect();
    asserter_dirs.sort();

    // Include the genesis file (top-level) in the hash if present.
    let genesis_path = claims_dir.join("genesis.jsonld");
    if genesis_path.exists() {
        hasher.update(b"genesis:");
        let count = count_lines(&genesis_path)?;
        hasher.update(count.to_le_bytes().as_slice());
    }

    for dir in asserter_dirs {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        hasher.update(name.as_bytes());
        hasher.update(b":");

        let log = dir.join("log.jsonl");
        if log.exists() {
            let count = count_lines(&log)?;
            hasher.update(count.to_le_bytes().as_slice());
        } else {
            hasher.update(&0u64.to_le_bytes());
        }
        hasher.update(b"\n");
    }

    Ok(hasher.finalize().to_hex().to_string())
}

fn count_lines(path: &Path) -> Result<u64> {
    let file = fs::File::open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let count = BufReader::new(file).lines().count() as u64;
    Ok(count)
}

/// Write the current heads hash to the heads file.
///
/// `view_dir` is the directory adjacent to the view (typically
/// `claims/`); the heads file is named [`HEADS_FILE_NAME`] inside
/// that directory.
pub fn write_heads(view_dir: &Path, claims_dir: &Path) -> Result<()> {
    fs::create_dir_all(view_dir)
        .with_context(|| format!("create view dir {}", view_dir.display()))?;
    let hash = current_heads(claims_dir)?;
    let path = view_dir.join(HEADS_FILE_NAME);
    fs::write(&path, &hash)
        .with_context(|| format!("write heads file {}", path.display()))?;
    Ok(())
}

/// Read the previously written heads hash.
///
/// Returns `None` if the heads file does not exist.
pub fn read_heads(view_dir: &Path) -> Result<Option<String>> {
    let path = view_dir.join(HEADS_FILE_NAME);
    match fs::read_to_string(&path) {
        Ok(s) => Ok(Some(s.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::anyhow!("read heads file: {}", e)),
    }
}

/// Compare the stored heads against the current state of `claims_dir`.
///
/// Returns true if the stored hash matches the current heads. Returns
/// false if the file does not exist (treat as stale) or if the hashes
/// differ.
pub fn heads_match(view_dir: &Path, claims_dir: &Path) -> Result<bool> {
    let stored = read_heads(view_dir)?;
    let current = current_heads(claims_dir)?;
    Ok(stored.as_deref() == Some(current.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::LogWriter;
    use serde_json::json;
    use tempfile::TempDir;

    fn make_claim(id: &str) -> serde_json::Value {
        json!({
            "@context": "https://nomograph.org/v3/context.jsonld",
            "@id": format!("synthesist:claim/{}", id),
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd"
        })
    }

    #[test]
    fn current_heads_on_empty_dir_is_stable() {
        let tmp = TempDir::new().unwrap();
        let h1 = current_heads(tmp.path()).unwrap();
        let h2 = current_heads(tmp.path()).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn current_heads_on_missing_dir_is_stable() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("not-here");
        let h1 = current_heads(&missing).unwrap();
        let h2 = current_heads(&missing).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn heads_match_after_write() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        for i in 0..3 {
            writer
                .append("user:local:agd", &make_claim(&format!("h{}", i)))
                .unwrap();
        }

        let view_dir = tmp.path().join("_view");
        write_heads(&view_dir, tmp.path()).unwrap();

        assert_eq!(heads_match(&view_dir, tmp.path()).unwrap(), true);
    }

    #[test]
    fn heads_stale_after_append() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        for i in 0..3 {
            writer
                .append("user:local:agd", &make_claim(&format!("s{}", i)))
                .unwrap();
        }

        let view_dir = tmp.path().join("_view");
        write_heads(&view_dir, tmp.path()).unwrap();

        // Append another claim.
        writer
            .append("user:local:agd", &make_claim("new"))
            .unwrap();

        assert_eq!(heads_match(&view_dir, tmp.path()).unwrap(), false);
    }

    #[test]
    fn heads_match_after_rewrite() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        for i in 0..3 {
            writer
                .append("user:local:agd", &make_claim(&format!("r{}", i)))
                .unwrap();
        }

        let view_dir = tmp.path().join("_view");
        write_heads(&view_dir, tmp.path()).unwrap();

        writer
            .append("user:local:agd", &make_claim("more"))
            .unwrap();

        assert_eq!(heads_match(&view_dir, tmp.path()).unwrap(), false);

        write_heads(&view_dir, tmp.path()).unwrap();
        assert_eq!(heads_match(&view_dir, tmp.path()).unwrap(), true);
    }

    #[test]
    fn heads_match_returns_false_when_file_absent() {
        let tmp = TempDir::new().unwrap();
        let view_dir = tmp.path().join("_view");
        fs::create_dir_all(&view_dir).unwrap();
        assert_eq!(heads_match(&view_dir, tmp.path()).unwrap(), false);
    }

    #[test]
    fn read_heads_returns_none_when_file_absent() {
        let tmp = TempDir::new().unwrap();
        let view_dir = tmp.path().join("_view");
        fs::create_dir_all(&view_dir).unwrap();
        assert!(read_heads(&view_dir).unwrap().is_none());
    }

    #[test]
    fn current_heads_changes_when_new_asserter_appears() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        writer
            .append("user:local:agd", &make_claim("a"))
            .unwrap();
        let h1 = current_heads(tmp.path()).unwrap();

        writer
            .append("user:local:jkolb", &make_claim("b"))
            .unwrap();
        let h2 = current_heads(tmp.path()).unwrap();

        assert_ne!(h1, h2);
    }

    #[test]
    fn current_heads_is_deterministic_across_calls() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        writer
            .append("user:local:agd", &make_claim("d1"))
            .unwrap();
        writer
            .append("user:local:jkolb", &make_claim("d2"))
            .unwrap();
        writer
            .append("user:local:agd", &make_claim("d3"))
            .unwrap();

        let h1 = current_heads(tmp.path()).unwrap();
        let h2 = current_heads(tmp.path()).unwrap();
        let h3 = current_heads(tmp.path()).unwrap();

        assert_eq!(h1, h2);
        assert_eq!(h2, h3);
    }
}
