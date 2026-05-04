//! `claims` CLI surface — substrate maintenance commands.
//!
//! Distinct from the typed-append commands (`tree`, `spec`, `task`,
//! `discovery`, `outcome`), `claims` operates on the on-disk store
//! directly: compaction, future verify/gc/snapshot operations.
//!
//! Compaction reference implementation (issue #7, MR !8) by Josh
//! Meekhof. See CHANGELOG entry for v2.4.0 for attribution.

use std::io::{self, IsTerminal, Write};

use anyhow::{Result, bail};
use serde_json::json;

use crate::cli::ClaimsCmd;
use crate::compaction::ClaimCompaction;
use crate::output::{Output, emit};
use crate::store::SynthStore;

pub fn run(cmd: &ClaimsCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        ClaimsCmd::Compact { dry_run, yes } => cmd_compact(*dry_run, *yes, session),
    }
}

fn cmd_compact(dry_run: bool, yes: bool, session: &Option<String>) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let claims_root = store.root().display().to_string();

    let stats = pre_compaction_stats(&store)?;

    if dry_run {
        return emit(Output::new(json!({
            "dry_run": true,
            "claims_root": claims_root,
            "would_compact": stats,
            "note": "no changes made; rerun without --dry-run to compact",
        })));
    }

    if !yes {
        // Non-interactive callers (agents, CI, scripts) must pass
        // --yes explicitly. We refuse to prompt on a pipe because
        // hanging waiting for stdin in an automated context is the
        // worst failure mode. TTY callers get the prompt for safety.
        if !io::stdin().is_terminal() {
            bail!(
                "non-interactive invocation; pass --yes to compact (or --dry-run to preview)"
            );
        }
        if !confirm_interactively(&claims_root, &stats)? {
            bail!("aborted at confirmation; rerun with --yes to skip the prompt");
        }
    }

    store.compact_claim_log()?;
    emit(Output::new(json!({
        "ok": true,
        "claims_root": claims_root,
        "compacted": stats,
    })))
}

#[derive(Debug)]
struct CompactionStats {
    change_file_count: usize,
}

impl serde::Serialize for CompactionStats {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("CompactionStats", 1)?;
        st.serialize_field("change_file_count", &self.change_file_count)?;
        st.end()
    }
}

fn pre_compaction_stats(store: &SynthStore) -> Result<CompactionStats> {
    let changes_dir = store.root().join("changes");
    let count = if changes_dir.is_dir() {
        std::fs::read_dir(&changes_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "amc")
            })
            .count()
    } else {
        0
    };
    Ok(CompactionStats {
        change_file_count: count,
    })
}

fn confirm_interactively(claims_root: &str, stats: &CompactionStats) -> Result<bool> {
    eprint!(
        "Compact {} change files in {} into snapshot.amc? [y/N] ",
        stats.change_file_count, claims_root
    );
    io::stderr().flush().ok();
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}
