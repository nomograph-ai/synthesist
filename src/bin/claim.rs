//! `claim` CLI entrypoint.
//!
//! Maps every subcommand to the `nomograph_claim` public API. Stays thin
//! on purpose — no file IO or business logic lives here; this binary is
//! the JSON-over-stdout boundary documented in `CLAUDE.md` §"CLI Shape".
//!
//! Output contract:
//! - success prints a single JSON value on stdout
//! - `--pretty` pretty-prints the same value
//! - errors go to stderr with a prescriptive one-line message
//!
//! Exit codes:
//! - `0` success
//! - `1` user error (bad args, unknown claim type, invalid JSON)
//! - `2` system error (io, storage, sqlite)
//! - `3` composition degraded (reserved; not emitted today)

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use nomograph_claim::claim::ClaimId;
use nomograph_claim::schema::validate_claim;
use nomograph_claim::session::SessionClaim;
use nomograph_claim::{Claim, ClaimType, Error as ClaimError, Session, Store, View};
use serde_json::{json, Value};

#[derive(Debug, Parser)]
#[command(name = "claim", version, about = "Bi-temporal CRDT claim substrate")]
struct Cli {
    /// Pretty-print JSON output (two-space indent).
    #[arg(long, global = true)]
    pretty: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scaffold a fresh `claims/` directory in the current project root.
    Init,

    /// Append a typed claim to the log.
    Append {
        /// Claim type, e.g. `spec`, `task`, `disposition`.
        #[arg(long)]
        r#type: String,
        /// JSON props matching the per-type schema.
        #[arg(long)]
        props: String,
        /// Asserter id, e.g. `user:gitlab:andunn`.
        #[arg(long = "as")]
        asserted_by: String,
    },

    /// List every claim at the current heads (deduped by id).
    List,

    /// Walk the supersession chain for a claim id (chronological).
    History {
        /// Claim id to trace.
        id: String,
    },

    /// Surface diamond conflicts (same prior superseded by >1 live claim).
    Conflicts,

    /// Summarise the store: root path, total claims, type counts, head count.
    Status,

    /// View-projection subcommands.
    View {
        #[command(subcommand)]
        action: ViewAction,
    },

    /// Session lifecycle subcommands.
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
}

#[derive(Debug, Subcommand)]
enum ViewAction {
    /// Rebuild `claims/view.sqlite` if the heads cache is stale.
    Sync,
}

#[derive(Debug, Subcommand)]
enum SessionAction {
    /// Open a new session; subsequent writes can be tagged to it.
    Start {
        /// Logical session id (non-empty).
        #[arg(long)]
        id: String,
        /// Asserter base, e.g. `user:gitlab:andunn`.
        #[arg(long)]
        asserter_base: String,
        /// Optional tree tag.
        #[arg(long)]
        tree: Option<String>,
        /// Optional spec tag.
        #[arg(long)]
        spec: Option<String>,
        /// Optional summary.
        #[arg(long)]
        summary: Option<String>,
    },

    /// List currently-live sessions (openers with no superseding close).
    List,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let pretty = cli.pretty;
    let outcome = match cli.command {
        Command::Init => cmd_init(),
        Command::Append {
            r#type,
            props,
            asserted_by,
        } => cmd_append(&r#type, &props, &asserted_by),
        Command::List => cmd_list(),
        Command::History { id } => cmd_history(&id),
        Command::Conflicts => cmd_conflicts(),
        Command::Status => cmd_status(),
        Command::View { action } => match action {
            ViewAction::Sync => cmd_view_sync(),
        },
        Command::Session { action } => match action {
            SessionAction::Start {
                id,
                asserter_base,
                tree,
                spec,
                summary,
            } => cmd_session_start(
                &id,
                &asserter_base,
                tree.as_deref(),
                spec.as_deref(),
                summary.as_deref(),
            ),
            SessionAction::List => cmd_session_list(),
        },
    };

    match outcome {
        Ok(value) => {
            emit(&value, pretty);
            ExitCode::SUCCESS
        }
        Err(cmd_err) => {
            eprintln!("{}", cmd_err.message);
            ExitCode::from(cmd_err.exit_code)
        }
    }
}

fn emit(value: &Value, pretty: bool) {
    let s = if pretty {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    };
    println!("{s}");
}

// --- exit-coded errors -----------------------------------------------------

/// Wrap an anyhow error with a CLI exit code.
struct CmdError {
    message: String,
    exit_code: u8,
}

type CmdResult = std::result::Result<Value, CmdError>;

fn user_err(err: impl Into<anyhow::Error>) -> CmdError {
    CmdError {
        message: format!("error: {}", err.into()),
        exit_code: 1,
    }
}

fn sys_err(err: impl Into<anyhow::Error>) -> CmdError {
    CmdError {
        message: format!("error: {}", err.into()),
        exit_code: 2,
    }
}

/// Route a library `Error` to the appropriate exit code.
fn route_claim_err(err: ClaimError) -> CmdError {
    match &err {
        ClaimError::Schema(_) => user_err(anyhow!(err)),
        ClaimError::Io(_)
        | ClaimError::Sqlite(_)
        | ClaimError::Automerge(_)
        | ClaimError::AutomergeLoad(_)
        | ClaimError::MissingGenesis(_)
        | ClaimError::KeyFileNotFound(_)
        | ClaimError::Corrupt(_) => sys_err(anyhow!(err)),
        _ => sys_err(anyhow!(err)),
    }
}

// --- commands --------------------------------------------------------------

fn claims_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("resolve current working directory")?;
    Ok(cwd.join("claims"))
}

fn open_store() -> std::result::Result<Store, CmdError> {
    let root = claims_root().map_err(sys_err)?;
    Store::open(&root).map_err(route_claim_err)
}

fn cmd_init() -> CmdResult {
    let root = claims_root().map_err(sys_err)?;
    let store = Store::init(&root).map_err(route_claim_err)?;
    Ok(json!({
        "ok": true,
        "path": store.root().display().to_string(),
    }))
}

fn cmd_append(claim_type: &str, props_json: &str, asserted_by: &str) -> CmdResult {
    if asserted_by.is_empty() {
        return Err(user_err(anyhow!(
            "--as must be non-empty; pass e.g. `user:gitlab:andunn`"
        )));
    }
    let claim_type = parse_claim_type(claim_type).map_err(user_err)?;
    let props: Value = serde_json::from_str(props_json)
        .with_context(|| "parse --props as JSON; pass a JSON object matching the per-type schema")
        .map_err(user_err)?;

    let claim = Claim::new(claim_type, props, asserted_by.to_string());
    validate_claim(&claim).map_err(route_claim_err)?;

    let mut store = open_store()?;
    store.append(&claim).map_err(route_claim_err)?;
    Ok(json!({ "id": claim.id }))
}

fn cmd_list() -> CmdResult {
    let mut store = open_store()?;
    let claims = store.load_claims().map_err(route_claim_err)?;
    let arr: Vec<Value> = claims.iter().map(claim_to_json).collect();
    Ok(Value::Array(arr))
}

fn cmd_history(id: &str) -> CmdResult {
    if id.is_empty() {
        return Err(user_err(anyhow!(
            "claim id must be non-empty; pass a blake3 hex id"
        )));
    }
    let mut store = open_store()?;
    let claims = store.load_claims().map_err(route_claim_err)?;
    let by_id: HashMap<&ClaimId, &Claim> = claims.iter().map(|c| (&c.id, c)).collect();

    if !by_id.contains_key(&id.to_string()) {
        return Err(user_err(anyhow!(
            "claim id `{id}` not found; run `claim list` to see available ids"
        )));
    }

    // Walk backward along `supersedes` from `id` to collect ancestors.
    let mut ancestors: Vec<&Claim> = Vec::new();
    let mut cursor: Option<String> = Some(id.to_string());
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    while let Some(cur) = cursor {
        if !seen.insert(cur.clone()) {
            break; // cycle guard
        }
        match by_id.get(&cur) {
            Some(c) => {
                ancestors.push(*c);
                cursor = c.supersedes.clone();
            }
            None => break,
        }
    }
    ancestors.reverse(); // oldest first

    // Walk forward: any claim whose `supersedes` equals a prior id in
    // the current chain is a descendant. Follow chains greedily.
    let mut descendants: Vec<&Claim> = Vec::new();
    let mut frontier: String = id.to_string();
    let mut dseen: std::collections::HashSet<String> = std::collections::HashSet::new();
    dseen.insert(frontier.clone());
    loop {
        let next = claims
            .iter()
            .find(|c| c.supersedes.as_deref() == Some(frontier.as_str()));
        match next {
            Some(n) => {
                if !dseen.insert(n.id.clone()) {
                    break;
                }
                descendants.push(n);
                frontier = n.id.clone();
            }
            None => break,
        }
    }

    // Chronological order: ancestors (root -> id) then descendants (id -> leaves).
    // `ancestors` already contains `id` at its tail; don't double-include.
    let mut out: Vec<Value> = Vec::with_capacity(ancestors.len() + descendants.len());
    for c in ancestors {
        out.push(claim_to_json(c));
    }
    for c in descendants {
        out.push(claim_to_json(c));
    }
    Ok(Value::Array(out))
}

fn cmd_conflicts() -> CmdResult {
    let mut store = open_store()?;
    let claims = store.load_claims().map_err(route_claim_err)?;

    // Build {prior_id -> [superseding_claim_ids]} and collect prior ids
    // that are named by more than one distinct live superseder.
    let mut supers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in &claims {
        if let Some(prior) = &c.supersedes {
            supers
                .entry(prior.clone())
                .or_default()
                .push(c.id.clone());
        }
    }

    let mut conflicts: Vec<Value> = Vec::new();
    for (prior, mut superseders) in supers {
        superseders.sort();
        superseders.dedup();
        if superseders.len() > 1 {
            conflicts.push(json!({
                "prior": prior,
                "superseders": superseders,
            }));
        }
    }
    Ok(Value::Array(conflicts))
}

fn cmd_status() -> CmdResult {
    let mut store = open_store()?;
    let claims = store.load_claims().map_err(route_claim_err)?;
    let mut type_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for c in &claims {
        *type_counts.entry(c.claim_type.as_str()).or_insert(0) += 1;
    }
    let heads = store.heads();
    let type_counts_json: serde_json::Map<String, Value> = type_counts
        .into_iter()
        .map(|(k, v)| (k.to_string(), Value::from(v)))
        .collect();
    Ok(json!({
        "root": store.root().display().to_string(),
        "total": claims.len(),
        "type_counts": Value::Object(type_counts_json),
        "heads": heads.len(),
    }))
}

fn cmd_view_sync() -> CmdResult {
    let root = claims_root().map_err(sys_err)?;
    let mut store = Store::open(&root).map_err(route_claim_err)?;
    let mut view = View::open(&root).map_err(route_claim_err)?;
    let rebuilt = view.sync(&mut store).map_err(route_claim_err)?;
    Ok(json!({ "rebuilt": rebuilt }))
}

fn cmd_session_start(
    id: &str,
    asserter_base: &str,
    tree: Option<&str>,
    spec: Option<&str>,
    summary: Option<&str>,
) -> CmdResult {
    let mut store = open_store()?;
    let handle = Session::start(&mut store, id, asserter_base, tree, spec, summary)
        .map_err(route_claim_err)?;
    Ok(json!({
        "id": handle.id(),
        "asserter": handle.asserter(),
    }))
}

fn cmd_session_list() -> CmdResult {
    let mut store = open_store()?;
    let live = Session::list_live(&mut store).map_err(route_claim_err)?;
    let arr: Vec<Value> = live.iter().map(session_to_json).collect();
    Ok(Value::Array(arr))
}

// --- helpers ---------------------------------------------------------------

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
        other => Err(anyhow!(
            "unknown claim type `{other}`; pass one of \
             tree|spec|task|discovery|campaign|session|phase|\
             intent|heartbeat|outcome|directive|\
             stakeholder|topic|signal|disposition"
        )),
    }
}

fn claim_to_json(c: &Claim) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), Value::String(c.id.clone()));
    obj.insert(
        "claim_type".into(),
        Value::String(c.claim_type.as_str().into()),
    );
    obj.insert("props".into(), c.props.clone());
    obj.insert(
        "valid_from".into(),
        Value::String(c.valid_from.to_rfc3339()),
    );
    if let Some(vu) = c.valid_until {
        obj.insert("valid_until".into(), Value::String(vu.to_rfc3339()));
    }
    if let Some(sup) = &c.supersedes {
        obj.insert("supersedes".into(), Value::String(sup.clone()));
    }
    if let Some(pa) = &c.parent_asserter {
        obj.insert("parent_asserter".into(), Value::String(pa.clone()));
    }
    obj.insert("asserted_by".into(), Value::String(c.asserted_by.clone()));
    obj.insert(
        "asserted_at".into(),
        Value::String(c.asserted_at.to_rfc3339()),
    );
    Value::Object(obj)
}

fn session_to_json(s: &SessionClaim) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), Value::String(s.id.clone()));
    obj.insert(
        "asserter_base".into(),
        Value::String(s.asserter_base.clone()),
    );
    if let Some(t) = &s.tree {
        obj.insert("tree".into(), Value::String(t.clone()));
    }
    if let Some(sp) = &s.spec {
        obj.insert("spec".into(), Value::String(sp.clone()));
    }
    if let Some(sm) = &s.summary {
        obj.insert("summary".into(), Value::String(sm.clone()));
    }
    obj.insert("start_id".into(), Value::String(s.start_id.clone()));
    Value::Object(obj)
}
