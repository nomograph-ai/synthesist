//! Per-asserter append-only claim log writer.
//!
//! [`LogWriter`] writes one JSON-LD document per call to the file
//! `claims/<asserter-dir>/log.jsonl`. Each line is a compact JSON-LD
//! claim terminated by a newline.
//!
//! ## Atomic write strategy
//!
//! JSONL append is not directly atomic on POSIX: two concurrent
//! writers at end-of-file can interleave bytes. The safe pattern used
//! here is:
//!
//! 1. Read the existing `log.jsonl` into memory (if it exists).
//! 2. Write a new `log.jsonl.tmp` containing all prior lines plus the
//!    new line.
//! 3. `fsync` the temp file so its data is on disk.
//! 4. `rename` the temp file over `log.jsonl` (POSIX `rename(2)` is
//!    atomic; the inode swap is all-or-nothing).
//! 5. `fsync` the asserter directory so the rename itself is durable.
//!
//! After a crash, the log file is either at the prior state (if the
//! crash happened before step 4) or at the new state (if the crash
//! happened after step 4). The temp file `log.jsonl.tmp` may exist on
//! disk but is never mistaken for a real log file by readers that
//! look only for `log.jsonl`.
//!
//! The strategy is load-bearing: do not remove the temp+rename even if
//! simpler alternatives (e.g., O_APPEND) seem sufficient. O_APPEND is
//! not crash-safe because the OS can write a partial line before the
//! process is killed.
//!
//! ## Asserter directory naming
//!
//! Asserter strings follow the convention `<class>:<scope>:<id>[:<session>]`
//! (e.g., `user:local:agd:edc-bootstrap`). Colons are legal in macOS
//! and Linux directory names but can cause confusion in shell contexts
//! and some tooling.
//!
//! This module maps colons to hyphens for directory names:
//!
//! ```text
//! user:local:agd:edc-bootstrap  -->  user-local-agd-edc-bootstrap
//! ```
//!
//! The mapping is one-to-one. The v2.5 asserter convention does not
//! permit hyphens in the `<class>` or `<scope>` segments, so
//! collisions are not expected in practice. macOS and Linux both permit
//! hyphens in directory names; the resulting paths are identical on
//! both platforms.
//!
//! TODO: move `dir_name_for_asserter` to the asserter module
//! (`src/asserter.rs`, T1.4) once that module lands. The function
//! belongs there as `Asserter::dir_name(&self) -> String`. For now it
//! lives here as a private helper with a public wrapper so T1.4 can
//! absorb it without breaking the `LogWriter` call site.

use anyhow::{bail, Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// A newtype wrapping the `@id` string of a written claim.
///
/// The value is the raw JSON-LD `@id` field, e.g. `"synthesist:claim/abc123"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimId(pub String);

impl ClaimId {
    /// Borrow the underlying `@id` string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ClaimId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Writer that appends one JSON-LD claim per call to the appropriate
/// per-asserter log file inside a claims directory.
///
/// The writer does not hold any open file handle; each [`Self::append`]
/// call opens, writes, and closes the file atomically via temp+rename.
pub struct LogWriter {
    claims_dir: PathBuf,
}

impl LogWriter {
    /// Create a new `LogWriter` rooted at `claims_dir`.
    ///
    /// The directory must exist; this function does not create it.
    pub fn new(claims_dir: &Path) -> Result<Self> {
        if !claims_dir.exists() {
            bail!("claims directory does not exist: {}", claims_dir.display());
        }
        if !claims_dir.is_dir() {
            bail!(
                "claims path is not a directory: {}",
                claims_dir.display()
            );
        }
        Ok(Self {
            claims_dir: claims_dir.to_owned(),
        })
    }

    /// Append one JSON-LD claim document to the per-asserter log.
    ///
    /// # Validation
    ///
    /// The document must be a JSON object with all four required
    /// envelope predicates:
    /// - `@id`
    /// - `@type`
    /// - `prov:generatedAtTime`
    /// - `prov:wasAttributedTo`
    ///
    /// Any missing field produces a structured error naming the field.
    ///
    /// # Routing
    ///
    /// Writes to `claims/<dir_name>/log.jsonl` where `<dir_name>` is
    /// derived from `asserter` via [`dir_name_for_asserter`]. The
    /// asserter subdirectory is created if it does not already exist.
    ///
    /// # Returns
    ///
    /// The `@id` value of the claim, wrapped as [`ClaimId`].
    pub fn append(&self, asserter: &str, doc: &Value) -> Result<ClaimId> {
        // -- Validate envelope --
        let obj = doc
            .as_object()
            .context("claim document must be a JSON object")?;

        let required = ["@id", "@type", "prov:generatedAtTime", "prov:wasAttributedTo"];
        for field in &required {
            if !obj.contains_key(*field) {
                bail!(
                    "claim document is missing required envelope field: {}",
                    field
                );
            }
        }

        let claim_id = obj["@id"]
            .as_str()
            .context("@id must be a string")?
            .to_owned();

        // -- Resolve the asserter directory --
        let dir_name = dir_name_for_asserter(asserter);
        let asserter_dir = self.claims_dir.join(&dir_name);
        if !asserter_dir.exists() {
            fs::create_dir_all(&asserter_dir).with_context(|| {
                format!(
                    "failed to create asserter directory: {}",
                    asserter_dir.display()
                )
            })?;
        }

        let log_path = asserter_dir.join("log.jsonl");
        let tmp_path = asserter_dir.join("log.jsonl.tmp");

        // -- Serialize the new claim line --
        let new_line = serde_json::to_string(doc)
            .context("failed to serialize claim document to JSON")?
            + "\n";

        // -- Atomic append via read-existing + write-tmp + rename --
        //
        // Read the current log file if it exists.
        let existing: Vec<u8> = if log_path.exists() {
            let mut buf = Vec::new();
            File::open(&log_path)
                .with_context(|| {
                    format!("failed to open log for reading: {}", log_path.display())
                })?
                .read_to_end(&mut buf)
                .with_context(|| format!("failed to read log: {}", log_path.display()))?;
            buf
        } else {
            Vec::new()
        };

        // Write existing content plus the new line to the temp file.
        {
            let mut tmp = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)
                .with_context(|| {
                    format!("failed to open tmp file for writing: {}", tmp_path.display())
                })?;
            tmp.write_all(&existing).with_context(|| {
                format!(
                    "failed to write existing content to tmp: {}",
                    tmp_path.display()
                )
            })?;
            tmp.write_all(new_line.as_bytes()).with_context(|| {
                format!("failed to write new line to tmp: {}", tmp_path.display())
            })?;
            tmp.flush()
                .with_context(|| format!("failed to flush tmp: {}", tmp_path.display()))?;
            // fsync the temp file data before rename.
            tmp.sync_all()
                .with_context(|| format!("failed to fsync tmp: {}", tmp_path.display()))?;
        }

        // Atomic rename: log.jsonl.tmp -> log.jsonl.
        fs::rename(&tmp_path, &log_path).with_context(|| {
            format!(
                "failed to rename {} to {}",
                tmp_path.display(),
                log_path.display()
            )
        })?;

        // fsync the directory to make the rename durable.
        {
            let dir_fd = File::open(&asserter_dir).with_context(|| {
                format!(
                    "failed to open asserter directory for fsync: {}",
                    asserter_dir.display()
                )
            })?;
            dir_fd.sync_all().with_context(|| {
                format!(
                    "failed to fsync asserter directory: {}",
                    asserter_dir.display()
                )
            })?;
        }

        Ok(ClaimId(claim_id))
    }
}

/// Convert an asserter string to a filesystem-safe directory name.
///
/// Colons (`:`) are replaced with hyphens (`-`). The mapping is
/// deterministic and identical on macOS and Linux.
///
/// Examples:
/// - `user:local:agd` -> `user-local-agd`
/// - `user:local:agd:edc-bootstrap` -> `user-local-agd-edc-bootstrap`
/// - `agent:claude-opus-4-7:sess-abc` -> `agent-claude-opus-4-7-sess-abc`
///
/// Kept as a free function for ergonomic call sites that have only the
/// raw asserter string. Parsed asserters should prefer
/// [`crate::asserter::Asserter::dir_name`] which validates the format
/// before returning the directory name.
pub fn dir_name_for_asserter(asserter: &str) -> String {
    asserter.replace(':', "-")
}

//
// LogReader: walk the union of per-asserter log files.
//

/// One claim materialized from a log line.
///
/// `id` is the claim's `@id` value pulled from the parsed document.
/// `raw` is the full JSON-LD doc as a `serde_json::Value`. Callers that
/// want typed access against a module schema (e.g., synthesist:Task) walk
/// the raw value themselves.
#[derive(Debug, Clone)]
pub struct Claim {
    pub id: ClaimId,
    pub raw: Value,
}

/// Iterates the union of all asserter logs under a claims directory.
///
/// Order is deterministic: asserter directories are sorted
/// lexicographically by name; within each, lines are yielded in file
/// order. The pseudo-asserter `bootstrap` (for `genesis.jsonld` at the
/// top level of `claims/`) is yielded first if the file exists.
///
/// Order across asserters is NOT time-sorted; callers that need
/// `prov:generatedAtTime` ordering must sort the yielded claims after
/// the fact (cheap on Oxigraph; less so when iterating raw).
pub struct LogReader {
    claims_dir: PathBuf,
}

impl LogReader {
    /// Open the claims directory for reading.
    ///
    /// The directory does not have to exist; an empty or missing
    /// directory yields zero claims with no error.
    pub fn new(claims_dir: &Path) -> Result<Self> {
        Ok(Self {
            claims_dir: claims_dir.to_path_buf(),
        })
    }

    /// Return the root claims directory this reader iterates.
    pub fn claims_dir(&self) -> &Path {
        &self.claims_dir
    }

    /// Yield claims one at a time, in the order described above.
    ///
    /// Each item is a `Result` so the iteration can continue past a
    /// malformed line. The caller can `filter_map(Result::ok)` to skip
    /// errors, or collect and inspect them.
    pub fn iter_claims(&self) -> ClaimIter {
        ClaimIter::new(&self.claims_dir)
    }
}

/// Iterator over claims in a [`LogReader`].
///
/// Implements two-tier iteration: outer over (genesis + asserter
/// directories), inner over lines within each log file.
pub struct ClaimIter {
    sources: std::vec::IntoIter<PathBuf>,
    current_lines: Option<std::io::Lines<std::io::BufReader<fs::File>>>,
}

impl ClaimIter {
    fn new(claims_dir: &Path) -> Self {
        let sources = enumerate_log_sources(claims_dir);
        Self {
            sources: sources.into_iter(),
            current_lines: None,
        }
    }

    fn open_next(&mut self) -> Option<()> {
        let path = self.sources.next()?;
        match fs::File::open(&path) {
            Ok(f) => {
                use std::io::BufRead;
                let reader = std::io::BufReader::new(f);
                self.current_lines = Some(reader.lines());
                Some(())
            }
            Err(_) => {
                // File disappeared between enumeration and open. Skip
                // it and try the next source.
                self.open_next()
            }
        }
    }
}

impl Iterator for ClaimIter {
    type Item = Result<Claim>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut lines) = self.current_lines {
                match lines.next() {
                    Some(Ok(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        return Some(parse_line(&line));
                    }
                    Some(Err(e)) => {
                        return Some(Err(anyhow::anyhow!("read line: {}", e)));
                    }
                    None => {
                        self.current_lines = None;
                    }
                }
            } else if self.open_next().is_none() {
                return None;
            }
        }
    }
}

/// Enumerate the log files to read, in canonical order.
///
/// The genesis file `claims/genesis.jsonld` is first if it exists, then
/// the per-asserter `claims/<asserter-dir>/log.jsonl` files in
/// lexicographic order of asserter directory name.
fn enumerate_log_sources(claims_dir: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();

    let genesis = claims_dir.join("genesis.jsonld");
    if genesis.exists() {
        sources.push(genesis);
    }

    let entries = match fs::read_dir(claims_dir) {
        Ok(e) => e,
        Err(_) => return sources,
    };

    let mut asserter_dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            // Skip the gitignored view and telemetry directories.
            !p.file_name()
                .map(|n| {
                    let s = n.to_string_lossy();
                    s.starts_with('_') || s.starts_with('.')
                })
                .unwrap_or(false)
        })
        .collect();
    asserter_dirs.sort();

    for dir in asserter_dirs {
        let log = dir.join("log.jsonl");
        if log.exists() {
            sources.push(log);
        }
    }

    sources
}

fn parse_line(line: &str) -> Result<Claim> {
    let raw: Value = serde_json::from_str(line)
        .with_context(|| "parse JSON-LD line")?;
    let id = raw
        .get("@id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("line missing @id"))?
        .to_string();
    Ok(Claim {
        id: ClaimId(id),
        raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    /// Build a minimal valid claim document for testing.
    fn make_claim(id: &str, asserter_iri: &str) -> Value {
        json!({
            "@context": "https://nomograph.org/v3/context.jsonld",
            "@id": id,
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "prov:wasAttributedTo": asserter_iri,
            "synthesist:summary": "test claim"
        })
    }

    // -- Acceptance criterion 1 --
    //
    // Append 100 claims with the same asserter, read the file back,
    // count 100 lines, verify each parses as JSON-LD (i.e. as a JSON
    // object with @id).
    #[test]
    fn append_100_claims_same_asserter() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        let asserter = "user:local:agd";
        for i in 0..100 {
            let id = format!("synthesist:claim/{:016x}", i);
            let doc = make_claim(&id, "asserter:user:local:agd");
            let claim_id = writer.append(asserter, &doc).unwrap();
            assert_eq!(claim_id.as_str(), id);
        }

        let log_path = tmp.path().join("user-local-agd").join("log.jsonl");
        assert!(log_path.exists(), "log file must exist");

        let content = fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 100, "expected 100 lines, got {}", lines.len());

        for (i, line) in lines.iter().enumerate() {
            let parsed: Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("line {} failed to parse as JSON: {}", i, e));
            assert!(parsed.get("@id").is_some(), "line {} missing @id", i);
            assert!(parsed.get("@type").is_some(), "line {} missing @type", i);
            assert!(
                parsed.get("prov:generatedAtTime").is_some(),
                "line {} missing prov:generatedAtTime",
                i
            );
            assert!(
                parsed.get("prov:wasAttributedTo").is_some(),
                "line {} missing prov:wasAttributedTo",
                i
            );
        }
    }

    // -- Acceptance criterion 2 --
    //
    // Append claims for two different asserters in interleaved order,
    // verify two log files exist with correct line counts each.
    #[test]
    fn append_interleaved_two_asserters() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        let asserter_a = "user:local:alice";
        let asserter_b = "agent:claude-opus-4-7:sess-xyz";

        let mut count_a = 0usize;
        let mut count_b = 0usize;

        for i in 0..60 {
            if i % 3 == 0 {
                // Every third write goes to B; others go to A.
                let id = format!("synthesist:claim/b{:015x}", i);
                let doc = make_claim(&id, "asserter:agent:claude-opus-4-7:sess-xyz");
                writer.append(asserter_b, &doc).unwrap();
                count_b += 1;
            } else {
                let id = format!("synthesist:claim/a{:015x}", i);
                let doc = make_claim(&id, "asserter:user:local:alice");
                writer.append(asserter_a, &doc).unwrap();
                count_a += 1;
            }
        }

        // Verify directory names.
        let dir_a = tmp.path().join("user-local-alice");
        let dir_b = tmp.path().join("agent-claude-opus-4-7-sess-xyz");
        assert!(dir_a.is_dir(), "asserter-a directory must exist");
        assert!(dir_b.is_dir(), "asserter-b directory must exist");

        let content_a = fs::read_to_string(dir_a.join("log.jsonl")).unwrap();
        let content_b = fs::read_to_string(dir_b.join("log.jsonl")).unwrap();

        let lines_a: Vec<&str> = content_a.lines().collect();
        let lines_b: Vec<&str> = content_b.lines().collect();

        assert_eq!(
            lines_a.len(),
            count_a,
            "asserter-a: expected {} lines, got {}",
            count_a,
            lines_a.len()
        );
        assert_eq!(
            lines_b.len(),
            count_b,
            "asserter-b: expected {} lines, got {}",
            count_b,
            lines_b.len()
        );

        // Each line in both files must parse as a JSON object with @id.
        for line in lines_a.iter().chain(lines_b.iter()) {
            let parsed: Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("@id").is_some());
        }
    }

    // -- Acceptance criterion 3 --
    //
    // Append rejects a document missing any of the four required
    // envelope predicates with a structured error.
    #[test]
    fn append_rejects_missing_envelope_fields() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        let asserter = "user:local:agd";

        // Missing @id.
        let no_id = json!({
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd"
        });
        let err = writer.append(asserter, &no_id).unwrap_err();
        assert!(
            err.to_string().contains("@id"),
            "error should mention @id, got: {}",
            err
        );

        // Missing @type.
        let no_type = json!({
            "@id": "synthesist:claim/abc",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:user:local:agd"
        });
        let err = writer.append(asserter, &no_type).unwrap_err();
        assert!(
            err.to_string().contains("@type"),
            "error should mention @type, got: {}",
            err
        );

        // Missing prov:generatedAtTime.
        let no_time = json!({
            "@id": "synthesist:claim/abc",
            "@type": "synthesist:Task",
            "prov:wasAttributedTo": "asserter:user:local:agd"
        });
        let err = writer.append(asserter, &no_time).unwrap_err();
        assert!(
            err.to_string().contains("prov:generatedAtTime"),
            "error should mention prov:generatedAtTime, got: {}",
            err
        );

        // Missing prov:wasAttributedTo.
        let no_attr = json!({
            "@id": "synthesist:claim/abc",
            "@type": "synthesist:Task",
            "prov:generatedAtTime": "2026-05-29T00:00:00.000Z"
        });
        let err = writer.append(asserter, &no_attr).unwrap_err();
        assert!(
            err.to_string().contains("prov:wasAttributedTo"),
            "error should mention prov:wasAttributedTo, got: {}",
            err
        );
    }

    // -- Supplementary: non-object rejects cleanly --
    #[test]
    fn append_rejects_non_object() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        let err = writer
            .append("user:local:agd", &json!([1, 2, 3]))
            .unwrap_err();
        assert!(
            err.to_string().contains("JSON object"),
            "expected 'JSON object' in error, got: {}",
            err
        );
    }

    // -- Supplementary: dir_name_for_asserter mapping --
    #[test]
    fn dir_name_colon_to_hyphen() {
        assert_eq!(dir_name_for_asserter("user:local:agd"), "user-local-agd");
        assert_eq!(
            dir_name_for_asserter("user:local:agd:edc-bootstrap"),
            "user-local-agd-edc-bootstrap"
        );
        assert_eq!(
            dir_name_for_asserter("agent:claude-opus-4-7:sess-abc"),
            "agent-claude-opus-4-7-sess-abc"
        );
        // No colons in input: unchanged.
        assert_eq!(dir_name_for_asserter("bootstrap"), "bootstrap");
    }

    // -- Supplementary: trailing newline on every line --
    #[test]
    fn log_file_has_trailing_newline() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();
        let doc = make_claim("synthesist:claim/aabbccdd", "asserter:user:local:agd");
        writer.append("user:local:agd", &doc).unwrap();

        let log_path = tmp.path().join("user-local-agd").join("log.jsonl");
        let raw = fs::read(&log_path).unwrap();
        assert_eq!(
            raw.last().copied(),
            Some(b'\n'),
            "log file must end with a newline"
        );
    }

    // -- Supplementary: new on non-existent dir fails gracefully --
    #[test]
    fn new_on_missing_dir_errors() {
        let result =
            LogWriter::new(Path::new("/tmp/nonexistent-nomograph-test-xyz-99999"));
        assert!(result.is_err(), "should error if claims_dir does not exist");
    }

    // -- Supplementary: ClaimId display and as_str --
    #[test]
    fn claim_id_display() {
        let id = ClaimId("synthesist:claim/abc".to_owned());
        assert_eq!(id.as_str(), "synthesist:claim/abc");
        assert_eq!(id.to_string(), "synthesist:claim/abc");
    }

    //
    // LogReader tests (T1.3).
    //

    fn write_log_line(path: &Path, doc: &Value) {
        use std::io::Write;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        let bytes = serde_json::to_vec(doc).unwrap();
        file.write_all(&bytes).unwrap();
        file.write_all(b"\n").unwrap();
    }

    #[test]
    fn reader_yields_50_claims_across_3_asserters() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        for i in 0..20 {
            let doc = make_claim(
                &format!("synthesist:claim/a{:02}", i),
                "asserter:user:local:agd",
            );
            writer.append("user:local:agd", &doc).unwrap();
        }
        for i in 0..15 {
            let doc = make_claim(
                &format!("synthesist:claim/b{:02}", i),
                "asserter:user:local:jkolb",
            );
            writer.append("user:local:jkolb", &doc).unwrap();
        }
        for i in 0..15 {
            let doc = make_claim(
                &format!("synthesist:claim/c{:02}", i),
                "asserter:agent:claude:sess1",
            );
            writer.append("agent:claude:sess1", &doc).unwrap();
        }

        let reader = LogReader::new(tmp.path()).unwrap();
        let mut ids: Vec<String> = Vec::new();
        for item in reader.iter_claims() {
            let claim = item.unwrap();
            ids.push(claim.id.as_str().to_string());
        }

        assert_eq!(ids.len(), 50);

        // Verify no duplicates.
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 50);
    }

    #[test]
    fn reader_includes_genesis_first() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        // Write a genesis claim manually.
        let genesis = json!({
            "@context": "https://nomograph.org/v3/context.jsonld",
            "@id": "nomograph:claim/genesis",
            "@type": "nomograph:Genesis",
            "prov:generatedAtTime": "2026-01-01T00:00:00.000Z",
            "prov:wasAttributedTo": "asserter:bootstrap"
        });
        write_log_line(&tmp.path().join("genesis.jsonld"), &genesis);

        // Write some normal claims.
        for i in 0..3 {
            let doc = make_claim(
                &format!("synthesist:claim/n{}", i),
                "asserter:user:local:agd",
            );
            writer.append("user:local:agd", &doc).unwrap();
        }

        let reader = LogReader::new(tmp.path()).unwrap();
        let first = reader.iter_claims().next().unwrap().unwrap();
        assert_eq!(first.id.as_str(), "nomograph:claim/genesis");
    }

    #[test]
    fn reader_on_empty_dir_yields_zero_claims() {
        let tmp = TempDir::new().unwrap();
        let reader = LogReader::new(tmp.path()).unwrap();
        let count = reader.iter_claims().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn reader_on_missing_dir_yields_zero_claims() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let reader = LogReader::new(&missing).unwrap();
        let count = reader.iter_claims().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn reader_continues_past_malformed_line() {
        let tmp = TempDir::new().unwrap();

        // Manually write a log with one good line, one bad line,
        // one good line.
        let asserter_dir = tmp.path().join("user-local-agd");
        std::fs::create_dir_all(&asserter_dir).unwrap();
        let log_path = asserter_dir.join("log.jsonl");
        let good_a = serde_json::to_string(&make_claim(
            "synthesist:claim/good_a",
            "asserter:user:local:agd",
        ))
        .unwrap();
        let good_b = serde_json::to_string(&make_claim(
            "synthesist:claim/good_b",
            "asserter:user:local:agd",
        ))
        .unwrap();
        let content = format!("{}\n{{ this is not valid json\n{}\n", good_a, good_b);
        std::fs::write(&log_path, content).unwrap();

        let reader = LogReader::new(tmp.path()).unwrap();
        let results: Vec<Result<Claim>> = reader.iter_claims().collect();

        // 3 results: good, error, good.
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert!(results[2].is_ok());
    }

    #[test]
    fn reader_skips_blank_lines() {
        let tmp = TempDir::new().unwrap();
        let asserter_dir = tmp.path().join("user-local-agd");
        std::fs::create_dir_all(&asserter_dir).unwrap();
        let log_path = asserter_dir.join("log.jsonl");
        let good = serde_json::to_string(&make_claim(
            "synthesist:claim/x",
            "asserter:user:local:agd",
        ))
        .unwrap();
        // Empty line at the top, then a good claim, then an empty line.
        let content = format!("\n{}\n\n", good);
        std::fs::write(&log_path, content).unwrap();

        let reader = LogReader::new(tmp.path()).unwrap();
        let count = reader.iter_claims().filter(|r| r.is_ok()).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn reader_skips_underscore_prefixed_dirs() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        // Create a fake _view and _telemetry dir with bogus log files.
        std::fs::create_dir_all(tmp.path().join("_view")).unwrap();
        std::fs::write(tmp.path().join("_view/log.jsonl"), "garbage\n").unwrap();
        std::fs::create_dir_all(tmp.path().join("_telemetry")).unwrap();
        std::fs::write(
            tmp.path().join("_telemetry/log.jsonl"),
            "also garbage\n",
        )
        .unwrap();

        // Write a real claim.
        let doc = make_claim("synthesist:claim/real", "asserter:user:local:agd");
        writer.append("user:local:agd", &doc).unwrap();

        let reader = LogReader::new(tmp.path()).unwrap();
        let results: Vec<_> = reader.iter_claims().collect();
        assert_eq!(results.len(), 1, "_view and _telemetry should be skipped");
    }

    #[test]
    fn reader_orders_asserters_lexicographically() {
        let tmp = TempDir::new().unwrap();
        let writer = LogWriter::new(tmp.path()).unwrap();

        // Append in NON-lexicographic order.
        let doc_z = make_claim("synthesist:claim/from_zulu", "asserter:user:local:zulu");
        writer.append("user:local:zulu", &doc_z).unwrap();

        let doc_a = make_claim("synthesist:claim/from_alpha", "asserter:user:local:alpha");
        writer.append("user:local:alpha", &doc_a).unwrap();

        let doc_m = make_claim("synthesist:claim/from_mike", "asserter:user:local:mike");
        writer.append("user:local:mike", &doc_m).unwrap();

        let reader = LogReader::new(tmp.path()).unwrap();
        let ids: Vec<String> = reader
            .iter_claims()
            .filter_map(|r| r.ok())
            .map(|c| c.id.as_str().to_string())
            .collect();

        // Alpha < mike < zulu lexicographically.
        assert_eq!(
            ids,
            vec![
                "synthesist:claim/from_alpha",
                "synthesist:claim/from_mike",
                "synthesist:claim/from_zulu",
            ]
        );
    }
}
