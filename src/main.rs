mod cli;
mod cmd_campaign;
mod cmd_conflicts;
mod cmd_discovery;
mod cmd_export;
mod cmd_import;
mod cmd_init;
mod cmd_migrate;
mod cmd_phase;
mod cmd_serve;
mod cmd_session;
mod cmd_spec;
mod cmd_sql;
mod cmd_task;
mod cmd_tree;
mod schema;
mod skill;
mod store;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: cli::Cli) -> anyhow::Result<()> {
    // Propagate --data-dir to SYNTHESIST_DIR so every Store::discover call picks
    // it up. Store::find_data_dir reads SYNTHESIST_DIR; the flag is syntactic
    // sugar so callers don't have to export the env var manually.
    // SAFETY: set_var is unsafe in edition 2024 because env vars are
    // process-global and not thread-safe. Synthesist is single-threaded at
    // this point -- we're before any Store::open or command dispatch.
    if let Some(path) = cli.data_dir.as_ref() {
        // Canonicalize best-effort for a clearer error if the path is wrong.
        let value = path.to_string_lossy().into_owned();
        unsafe {
            std::env::set_var("SYNTHESIST_DIR", value);
        }
    }

    // Commands that don't need a database
    match &cli.command {
        cli::Command::Init => return cmd_init::cmd_init(),
        cli::Command::Skill => return skill::cmd_skill(),
        cli::Command::Version { offline } => return cmd_version(*offline),
        // Landscape family (stakeholder/disposition/signal/stance) moved to
        // the `lattice` binary in v2. We short-circuit before session and
        // phase enforcement so users get the migration message even when
        // they forget `--session` or are in the wrong phase. Args are
        // swallowed by clap via `trailing_var_arg` so any invocation
        // reaches this arm rather than failing clap validation.
        cli::Command::Stakeholder { .. } => moved_to_lattice("stakeholder"),
        cli::Command::Disposition { .. } => moved_to_lattice("disposition"),
        cli::Command::Signal { .. } => moved_to_lattice("signal"),
        cli::Command::Stance { .. } => moved_to_lattice("stance"),
        // `session merge` and `session discard` are v1-only concepts
        // (file-copy session model). Intercept here so muscle-memory
        // calls get the "removed in v2" message instead of bouncing off
        // the generic "session required for write operations" error —
        // which used to happen because Session is a write-family command
        // and hit session enforcement first.
        cli::Command::Session {
            cmd: cli::SessionCmd::Merge { .. },
        } => session_removed_in_v2("merge"),
        cli::Command::Session {
            cmd: cli::SessionCmd::Discard { .. },
        } => session_removed_in_v2("discard"),
        _ => {}
    }

    // Read-only commands (no session required, no phase check)
    let read_only = matches!(
        &cli.command,
        cli::Command::Status
            | cli::Command::Check
            | cli::Command::Conflicts
            | cli::Command::Migrate { .. }
            | cli::Command::Export
            | cli::Command::Sql { .. }
            | cli::Command::Serve { .. }
            | cli::Command::Phase {
                cmd: cli::PhaseCmd::Show
            }
            | cli::Command::Session {
                cmd: cli::SessionCmd::List
            }
    );

    // The landscape family (stakeholder/disposition/signal/stance) moved to
    // `lattice` and short-circuits above, so it is not considered here.
    let is_list_or_show = matches!(
        &cli.command,
        cli::Command::Tree {
            cmd: cli::TreeCmd::List { .. },
        } | cli::Command::Spec {
            cmd: cli::SpecCmd::Show { .. } | cli::SpecCmd::List { .. },
        } | cli::Command::Task {
            cmd: cli::TaskCmd::List { .. } | cli::TaskCmd::Show { .. } | cli::TaskCmd::Ready { .. },
        } | cli::Command::Discovery {
            cmd: cli::DiscoveryCmd::List { .. },
        } | cli::Command::Campaign {
            cmd: cli::CampaignCmd::List { .. },
        } | cli::Command::Session {
            cmd: cli::SessionCmd::Status { .. },
        }
    );

    // Session enforcement for write operations
    if !read_only && !is_list_or_show {
        let is_session_cmd = matches!(&cli.command, cli::Command::Session { .. });
        let is_phase_cmd = matches!(&cli.command, cli::Command::Phase { .. });
        let is_import = matches!(&cli.command, cli::Command::Import { .. });

        if !is_session_cmd && !is_phase_cmd && !is_import && cli.session.is_none() {
            eprintln!("error: session required for write operations");
            eprintln!("  start new: synthesist session start <name>");
            eprintln!("  then:      synthesist --session=<name> ...");
            std::process::exit(1);
        }
    }

    // Phase enforcement for write operations
    if !read_only && !is_list_or_show && !cli.force {
        let (top, sub) = command_path(&cli.command);
        if !matches!(top, "session" | "phase" | "import") {
            let store = store::Store::discover()?;
            cmd_phase::check_phase(&store, cli.session.as_deref(), top, sub, cli.force)?;
        }
    }

    // Dispatch
    match &cli.command {
        cli::Command::Status => cmd_init::cmd_status(),
        cli::Command::Check => cmd_init::cmd_check(),
        cli::Command::Conflicts => cmd_conflicts::cmd_conflicts(),
        cli::Command::Migrate { cmd } => cmd_migrate::run(cmd),
        cli::Command::Tree { cmd } => cmd_tree::run(cmd, &cli.session),
        cli::Command::Spec { cmd } => cmd_spec::run(cmd, &cli.session),
        cli::Command::Task { cmd } => cmd_task::run(cmd, &cli.session),
        cli::Command::Discovery { cmd } => cmd_discovery::run(cmd, &cli.session),
        cli::Command::Campaign { cmd } => cmd_campaign::run(cmd, &cli.session),
        cli::Command::Session { cmd } => cmd_session::run(cmd),
        cli::Command::Phase { cmd } => cmd_phase::run(cmd, &cli.session, cli.force),
        cli::Command::Export => cmd_export::cmd_export(),
        cli::Command::Import { file } => cmd_import::cmd_import(file),
        cli::Command::Sql { query } => cmd_sql::cmd_sql(query),
        cli::Command::Serve { port, bind_all } => cmd_serve::run(*port, *bind_all),
        // Init, Skill, Version, and the landscape family (stakeholder,
        // disposition, signal, stance) are handled in the short-circuit
        // match above.
        cli::Command::Init
        | cli::Command::Skill
        | cli::Command::Version { .. }
        | cli::Command::Stakeholder { .. }
        | cli::Command::Disposition { .. }
        | cli::Command::Signal { .. }
        | cli::Command::Stance { .. } => unreachable!(),
    }
}

/// Tell the user the landscape-family command moved to the `lattice` binary
/// and exit non-zero. `lattice` reads/writes the same `claims/` directory
/// synthesist does, so data is shared — only the binary changes.
fn moved_to_lattice(subcommand_name: &str) -> ! {
    eprintln!(
        "synthesist: the `{name}` command moved to `lattice` in v2.\n\
         \n\
         lattice is currently a private GitLab repo (pending origination\n\
         review); it is not published to crates.io. If you need lattice\n\
         access, contact the maintainer. When it lands on crates.io the\n\
         install path will be `cargo install nomograph-lattice`.\n\
         \n\
         lattice and synthesist share the same `claims/` directory, so\n\
         lattice writes back into this same project once installed. The\n\
         CLI shape mirrors this one:\n\
         \n\
           lattice --session=<id> {name} <args>\n\
         \n\
         If you only need synthesist (specs, tasks, discoveries, phase),\n\
         you can safely skip lattice for now.",
        name = subcommand_name,
    );
    std::process::exit(3);
}

/// `session merge` and `session discard` were v1-only. v2 uses CRDT
/// semantics: merges happen automatically on `git pull`, discards are
/// supersession (see `session close`). Intercepting here — before
/// session-required enforcement — gives users a specific message
/// instead of a generic "session required" bounce.
fn session_removed_in_v2(subcommand_name: &str) -> ! {
    eprintln!(
        "synthesist: `session {name}` was removed in v2.\n\
         \n\
         v1 copied main.db into a per-session file and required an explicit\n\
         merge or discard. v2 appends claims directly to the shared log;\n\
         CRDT merges are automatic on `git pull`, and sessions are closed\n\
         non-destructively via supersession.\n\
         \n\
         Migrations:\n\
           session merge    ->  (no-op; just `git pull` and commit your claims/)\n\
           session discard  ->  `synthesist session close <id>`  (supersedes)\n\
         \n\
         If supersession chains diverged on a peer's branch, list them with\n\
         `synthesist conflicts` and resolve by appending a claim that\n\
         supersedes both rivals.",
        name = subcommand_name,
    );
    std::process::exit(3);
}

fn cmd_version(offline: bool) -> anyhow::Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let mut result = serde_json::Map::new();
    result.insert("version".into(), serde_json::json!(format!("v{version}")));

    if !offline
        && std::env::var("SYNTHESIST_OFFLINE").as_deref() != Ok("1")
        && let Some((tag, url)) = check_latest_version()
    {
        let current = version.split('-').next().unwrap_or(version);
        let latest = tag.strip_prefix('v').unwrap_or(&tag);
        let latest = latest.split('-').next().unwrap_or(latest);
        result.insert("latest".into(), serde_json::json!(tag));
        result.insert(
            "update_available".into(),
            serde_json::json!(latest > current),
        );
        result.insert("update_url".into(), serde_json::json!(url));
    }

    store::json_out(&serde_json::Value::Object(result))
}

/// Query GitLab releases API via curl. No TLS dependency in the binary.
fn check_latest_version() -> Option<(String, String)> {
    let output = std::process::Command::new("curl")
        .args([
            "-sf",
            "--max-time",
            "3",
            "https://gitlab.com/api/v4/projects/nomograph%2Fsynthesist/releases?per_page=1",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let body: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let release = body.as_array()?.first()?;
    let tag = release.get("tag_name")?.as_str()?.to_string();
    let url = release
        .pointer("/_links/self")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("https://gitlab.com/nomograph/synthesist/-/releases/{tag}"));
    Some((tag, url))
}

/// Extract (top_command, sub_command) for phase enforcement.
fn command_path(cmd: &cli::Command) -> (&str, &str) {
    match cmd {
        cli::Command::Tree { cmd } => (
            "tree",
            match cmd {
                cli::TreeCmd::Add { .. } => "add",
                cli::TreeCmd::List { .. } => "list",
                cli::TreeCmd::Show { .. } => "show",
                cli::TreeCmd::Close { .. } => "close",
            },
        ),
        cli::Command::Spec { cmd } => (
            "spec",
            match cmd {
                cli::SpecCmd::Add { .. } => "add",
                cli::SpecCmd::Show { .. } => "show",
                cli::SpecCmd::Update { .. } => "update",
                cli::SpecCmd::List { .. } => "list",
            },
        ),
        cli::Command::Task { cmd } => (
            "task",
            match cmd {
                cli::TaskCmd::Add { .. } => "add",
                cli::TaskCmd::List { .. } => "list",
                cli::TaskCmd::Show { .. } => "show",
                cli::TaskCmd::Update { .. } => "update",
                cli::TaskCmd::Claim { .. } => "claim",
                cli::TaskCmd::Done { .. } => "done",
                cli::TaskCmd::Reset { .. } => "reset",
                cli::TaskCmd::Block { .. } => "block",
                cli::TaskCmd::Wait { .. } => "wait",
                cli::TaskCmd::Cancel { .. } => "cancel",
                cli::TaskCmd::Ready { .. } => "ready",
                cli::TaskCmd::Acceptance { .. } => "acceptance",
            },
        ),
        cli::Command::Discovery { cmd } => (
            "discovery",
            match cmd {
                cli::DiscoveryCmd::Add { .. } => "add",
                cli::DiscoveryCmd::List { .. } => "list",
            },
        ),
        // Stakeholder, Disposition, Signal moved to `lattice` in v2 and
        // short-circuit before this function is called; they do not
        // participate in phase enforcement.
        cli::Command::Campaign { cmd } => (
            "campaign",
            match cmd {
                cli::CampaignCmd::Add { .. } => "add",
                cli::CampaignCmd::List { .. } => "list",
            },
        ),
        cli::Command::Session { .. } => ("session", ""),
        cli::Command::Phase { .. } => ("phase", ""),
        _ => ("", ""),
    }
}
