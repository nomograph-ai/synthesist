//! CLI for the v1 → v2 synthesist migration.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use synthesist_migrate_v1_to_v2::{migrate, MigrateError};

/// One-shot synthesist v1 (SQLite) → v2 (nomograph-claim) migration.
///
/// Reads a v1 `.synth/main.db` (or similar) and writes a fresh `claims/`
/// directory with one claim per row, preserving `created_at` as
/// `asserted_at` / `valid_from` on each claim.
#[derive(Debug, Parser)]
#[command(name = "synthesist-migrate-v1-to-v2", version)]
struct Cli {
    /// Path to the v1 SQLite database (e.g. `.synth/main.db`).
    #[arg(long)]
    from: PathBuf,

    /// Path to the v2 `claims/` directory to create.
    #[arg(long)]
    to: PathBuf,

    /// Read only; do not write any claims to disk.
    #[arg(long)]
    dry_run: bool,

    /// Delete any existing `claims/` at `--to` before migrating.
    #[arg(long)]
    overwrite: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match migrate(&cli.from, &cli.to, cli.dry_run, cli.overwrite) {
        Ok(summary) => {
            println!("migration complete ({} claims appended)", summary.total());
            println!("  trees:         {}", summary.trees);
            println!("  specs:         {}", summary.specs);
            println!("  tasks:         {}", summary.tasks);
            println!("  discoveries:   {}", summary.discoveries);
            println!("  campaigns:     {}", summary.campaigns);
            println!("  sessions:      {}", summary.sessions);
            println!("  stakeholders:  {}", summary.stakeholders);
            println!("  dispositions:  {}", summary.dispositions);
            println!("  signals:       {}", summary.signals);
            println!("  phase:         {}", summary.phase);
            if !summary.skipped.is_empty() {
                eprintln!("\n{} rows skipped:", summary.skipped.len());
                for reason in &summary.skipped {
                    eprintln!("  - {reason}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(MigrateError::AlreadyMigrated) => {
            eprintln!("error: destination already migrated; pass --overwrite to re-migrate");
            ExitCode::from(2)
        }
        Err(MigrateError::SourceMissing(p)) => {
            eprintln!("error: source db not found at {p}");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
