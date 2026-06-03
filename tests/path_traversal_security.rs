//! Security regression: path-traversal / arbitrary-directory-write via the
//! asserter directory name.
//!
//! The asserter string is mapped to a directory name (colons -> hyphens)
//! and joined onto `claims/` before a write. Three sources feed that
//! string from untrusted input:
//!
//!   1. `--session` / `SYNTHESIST_SESSION` (raw, appended to the local
//!      asserter base; phase writes need no `--force`).
//!   2. `$USER` (via the local asserter base).
//!   3. `prov:wasAttributedTo` from an `import` payload.
//!
//! Before the fix, a value containing `..` or `/` could redirect the
//! write outside the claims tree. These tests drive the real binary and
//! assert nothing lands outside `claims/`.
//!
//! Two safe outcomes uphold that invariant, depending on the source:
//!   - `--session` / `$USER` (live write path): the value is parse-guarded
//!     and the write is REJECTED.
//!   - `import` `prov:wasAttributedTo` (migration/import path): the value is
//!     first run through the lossless legacy-asserter normalizer, which
//!     collapses path-unsafe characters to `-`, so a traversal attribution is
//!     NEUTRALIZED into a safe in-tree directory rather than dropped. Either
//!     way, nothing escapes `claims/`.

use std::fs;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

/// Base command with a deterministic `$USER` and a clean environment.
fn synth(dir: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("synthesist").unwrap();
    cmd.current_dir(dir.path());
    cmd.env("SYNTHESIST_OFFLINE", "1");
    cmd.env_remove("SYNTHESIST_DIR");
    cmd.env_remove("SYNTHESIST_SESSION");
    cmd.env("USER", "sectest");
    cmd
}

/// Recursively assert no file or directory below `root` resolves outside
/// of `claims_dir`. Catches any traversal that planted a file in the
/// tempdir root (a sibling of `claims/`).
fn assert_only_under_claims(root: &std::path::Path, claims_dir: &std::path::Path) {
    for entry in fs::read_dir(root).unwrap().filter_map(|e| e.ok()) {
        let p = entry.path();
        // The claims dir itself and the gamma view live under claims/.
        if p == claims_dir {
            continue;
        }
        // Anything that looks like a synthesist marker outside claims/ is
        // a traversal escape.
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            !name.contains("sectest") && !name.contains("log.jsonl"),
            "found a claim artifact outside claims/: {}",
            p.display()
        );
    }
}

// -- Vector 1: --session with traversal is rejected, writes nothing
//    outside claims/. `phase set` is phase-exempt (no --force). --
#[test]
fn session_with_traversal_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();

    synth(&tmp).args(["init"]).assert().success();

    // A session value that, under the old colon-only mapping, would make
    // the asserter dir `user-local-sectest-..-..-escape` and escape the
    // claims tree once `..` segments collapse on the filesystem.
    let malicious = "../../escape";
    synth(&tmp)
        .args(["--session", malicious, "phase", "set", "plan"])
        .assert()
        .failure();

    let claims = tmp.path().join("claims");
    // No escape artifact anywhere in the tempdir root.
    assert_only_under_claims(tmp.path(), &claims);
    // And specifically no file two levels up.
    assert!(!tmp.path().join("escape").exists());
}

#[test]
fn session_with_slash_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();

    synth(&tmp)
        .args(["--session", "a/b/c", "phase", "set", "plan"])
        .assert()
        .failure();

    let claims = tmp.path().join("claims");
    assert!(!claims.join("user-local-sectest-a").exists());
    // No nested 'a/b/c' under claims either.
    assert!(!claims.join("a").exists());
}

#[test]
fn session_via_env_with_traversal_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();

    synth(&tmp)
        .env("SYNTHESIST_SESSION", "../../pwned")
        .args(["phase", "set", "plan"])
        .assert()
        .failure();

    assert!(!tmp.path().join("pwned").exists());
    assert_only_under_claims(tmp.path(), &tmp.path().join("claims"));
}

// -- Vector 2: $USER carrying traversal is handled (rejected, no escape). --
#[test]
fn user_with_traversal_is_handled() {
    let tmp = tempfile::tempdir().unwrap();

    // init does not need an asserter, so it can succeed; the first write
    // (phase set) routes through the poisoned $USER.
    let mut init = synth(&tmp);
    init.env("USER", "../../evil");
    init.args(["init"]).assert().success();

    let mut cmd = synth(&tmp);
    cmd.env("USER", "../../evil");
    cmd.args(["phase", "set", "plan"]).assert().failure();

    assert!(!tmp.path().join("evil").exists());
    assert_only_under_claims(tmp.path(), &tmp.path().join("claims"));
}

// -- Vector 3: import with a malicious prov:wasAttributedTo is NEUTRALIZED
//    (sanitized into a safe in-tree dir), writes nothing outside claims/.
//
//    The lossless legacy-asserter normalizer (migration/import path) replaces
//    every path-unsafe character in an asserter segment with `-` BEFORE the
//    strict parse. A traversal attribution like `user:local:../../../pwned`
//    therefore no longer reaches `parse` verbatim: its `/`-laden id segment is
//    collapsed to a single path-safe segment, so the claim lands UNDER
//    `claims/` (under a hyphenated dir name) instead of being rejected. The
//    security guarantee is unchanged -- nothing escapes the claims tree -- but
//    the claim is preserved rather than dropped, matching the lossless policy.
#[test]
fn import_with_malicious_attribution_is_neutralized_in_tree() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    // Import writes claims, so the session needs a write-permitting phase.
    synth(&tmp)
        .args(["--session", "imp", "phase", "set", "plan"])
        .assert()
        .success();

    // An export-shaped payload whose single claim is attributed to a
    // traversal asserter IRI.
    let payload = serde_json::json!({
        "claims_raw": [
            {
                "@context": {"synthesist": "https://nomograph.org/synthesist/"},
                "@id": "synthesist:claim/aaaabbbbccccdddd",
                "@type": "synthesist:Stakeholder",
                "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
                "prov:wasAttributedTo": "asserter:user:local:../../../pwned",
                "synthesist:id": "evil"
            }
        ]
    });
    let import_file = tmp.path().join("evil-import.json");
    fs::write(&import_file, serde_json::to_string(&payload).unwrap()).unwrap();

    let out = synth(&tmp)
        .args(["--session", "imp", "import", import_file.to_str().unwrap()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    // The claim is preserved (normalized), not dropped. The security property
    // -- nothing escapes claims/ -- is asserted below.
    assert_eq!(
        report["imported"].as_u64(),
        Some(1),
        "traversal attribution is sanitized and imported in-tree, not dropped"
    );
    assert_eq!(
        report["skipped"].as_u64(),
        Some(0),
        "the sanitized attribution parses, so nothing is skipped"
    );

    // THE security guarantee: nothing escaped the claims tree. The traversal
    // segment was collapsed to a single hyphenated dir name; no file or dir
    // named `pwned` exists outside claims/, and every claim artifact stays
    // under claims/.
    assert!(!tmp.path().join("pwned").exists());
    assert_only_under_claims(tmp.path(), &tmp.path().join("claims"));
}

// -- Sanity: a well-formed import still lands. --
#[test]
fn import_with_valid_attribution_still_imported() {
    let tmp = tempfile::tempdir().unwrap();
    synth(&tmp).args(["init"]).assert().success();
    synth(&tmp)
        .args(["--session", "imp", "phase", "set", "plan"])
        .assert()
        .success();

    let payload = serde_json::json!({
        "claims_raw": [
            {
                "@context": {"synthesist": "https://nomograph.org/synthesist/"},
                "@id": "synthesist:claim/eeeeffff00001111",
                "@type": "synthesist:Stakeholder",
                "prov:generatedAtTime": "2026-05-29T00:00:00.000Z",
                "prov:wasAttributedTo": "asserter:user:local:agd",
                "synthesist:id": "alice"
            }
        ]
    });
    let import_file = tmp.path().join("good-import.json");
    fs::write(&import_file, serde_json::to_string(&payload).unwrap()).unwrap();

    let out = synth(&tmp)
        .args(["--session", "imp", "import", import_file.to_str().unwrap()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(report["imported"].as_u64(), Some(1));
    assert_eq!(report["skipped"].as_u64(), Some(0));

    let log = tmp
        .path()
        .join("claims")
        .join("user-local-agd")
        .join("log.jsonl");
    assert!(log.exists(), "valid import must write its asserter log");
}
