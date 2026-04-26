//! Tree commands — ported to the claim substrate.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow, bail};
use nomograph_claim::ClaimType;
use serde_json::{Value, json};

use crate::cli::TreeCmd;
use crate::store::{SynthStore, json_out};

pub fn run(cmd: &TreeCmd, session: &Option<String>) -> Result<()> {
    match cmd {
        TreeCmd::Add {
            name,
            description,
            status,
        } => cmd_tree_add(name, description, status, session),
        TreeCmd::List { include_closed } => cmd_tree_list(*include_closed, session),
        TreeCmd::Show { name } => cmd_tree_show(name, session),
        TreeCmd::Close { name, start_id } => cmd_tree_close(name, start_id.as_deref(), session),
    }
}

fn cmd_tree_add(
    name: &str,
    description: &str,
    status: &str,
    session: &Option<String>,
) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let props = json!({ "name": name, "description": description });
    store.append(ClaimType::Tree, props, None)?;
    json_out(&json!({
        "name": name,
        "status": status,
        "description": description,
    }))
}

/// Decoded view of a Tree claim row.
struct TreeRow {
    /// Claim id (hash) of this row's claim.
    claim_id: String,
    /// `props.name`.
    name: String,
    /// `props.description` if present.
    description: String,
    /// `supersedes` column (claim id of prior). `None` for openers.
    supersedes: Option<String>,
    /// `props.status` if present (e.g. "closed"). Treated as "active"
    /// when missing.
    status: Option<String>,
}

/// Load every `Tree` claim, decoded. Ordered oldest-first by
/// `asserted_at` so the head of each supersession chain (the opener)
/// stays stable across rebuilds.
fn load_tree_rows(store: &SynthStore) -> Result<Vec<TreeRow>> {
    let raw = store.query(
        "SELECT id, props, supersedes FROM claims \
         WHERE claim_type = 'tree' \
         ORDER BY asserted_at",
        &[],
    )?;
    let mut out = Vec::with_capacity(raw.len());
    for row in raw {
        let claim_id = row
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("tree claim missing id"))?
            .to_string();
        let props_str = row
            .get("props")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("tree claim missing props"))?;
        let props: Value = serde_json::from_str(props_str)?;
        let name = props
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let description = props
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let status = props
            .get("status")
            .and_then(Value::as_str)
            .map(String::from);
        let supersedes = row
            .get("supersedes")
            .and_then(|v| v.as_str())
            .map(String::from);
        out.push(TreeRow {
            claim_id,
            name,
            description,
            supersedes,
            status,
        });
    }
    Ok(out)
}

/// Resolved view of a tree as a single (start_id, name, description,
/// status) tuple. Walks supersession chains so the latest claim's
/// status wins, but the chain root's claim id is the canonical
/// `start_id` for `--start-id` disambiguation.
struct ResolvedTree {
    start_id: String,
    name: String,
    description: String,
    status: String,
}

/// Walk supersession chains. Each opener (no `supersedes`) starts a
/// chain; for every chain return the latest description/status while
/// keeping the opener's claim id as the stable `start_id`.
fn resolve_trees(rows: &[TreeRow]) -> Vec<ResolvedTree> {
    // Map from claim_id -> row index for chain walks.
    let by_id: HashMap<&str, &TreeRow> =
        rows.iter().map(|r| (r.claim_id.as_str(), r)).collect();

    // Map from `supersedes` target -> claim id of the latest claim
    // pointing at it. Picks the last one we see (asserted_at order
    // guarantees newest wins) so a chain head stays well-defined even
    // when a row was superseded twice.
    let mut next_in_chain: HashMap<&str, &str> = HashMap::new();
    for r in rows {
        if let Some(prior) = r.supersedes.as_deref() {
            next_in_chain.insert(prior, r.claim_id.as_str());
        }
    }

    let mut out = Vec::new();
    for opener in rows.iter().filter(|r| r.supersedes.is_none()) {
        // Walk forward to the head of the chain.
        let mut current = opener;
        let mut visited: HashSet<&str> = HashSet::new();
        while let Some(next_id) = next_in_chain.get(current.claim_id.as_str()) {
            if !visited.insert(current.claim_id.as_str()) {
                // Cycle guard. Shouldn't happen with a healthy log.
                break;
            }
            match by_id.get(next_id) {
                Some(next_row) => current = next_row,
                None => break,
            }
        }
        out.push(ResolvedTree {
            start_id: opener.claim_id.clone(),
            name: current.name.clone(),
            description: current.description.clone(),
            status: current.status.clone().unwrap_or_else(|| "active".into()),
        });
    }
    out
}

fn cmd_tree_list(include_closed: bool, session: &Option<String>) -> Result<()> {
    let store = SynthStore::discover_for(session)?;
    let rows = load_tree_rows(&store)?;
    let resolved = resolve_trees(&rows);
    let trees: Vec<Value> = resolved
        .into_iter()
        .filter(|t| include_closed || t.status != "closed")
        .map(|t| {
            json!({
                "name": t.name,
                "status": t.status,
                "description": t.description,
                "start_id": t.start_id,
            })
        })
        .collect();
    json_out(&json!({ "trees": trees }))
}

fn cmd_tree_show(name: &str, session: &Option<String>) -> Result<()> {
    let store = SynthStore::discover_for(session)?;

    let tree_rows = store.query(
        "SELECT props FROM claims \
         WHERE claim_type = 'tree' AND json_extract(props, '$.name') = ?1 \
         ORDER BY asserted_at DESC LIMIT 1",
        &[&name],
    )?;
    let tree_row = tree_rows
        .first()
        .ok_or_else(|| anyhow!("tree not found: {name}. Try `synthesist tree list`."))?;
    let props_str = tree_row
        .get("props")
        .and_then(|v| v.as_str())
        .unwrap_or("{}");
    let props: serde_json::Value = serde_json::from_str(props_str).unwrap_or(json!({}));

    let spec_count_rows = store.query(
        "SELECT COUNT(*) AS n FROM claims \
         WHERE claim_type = 'spec' AND json_extract(props, '$.tree') = ?1",
        &[&name],
    )?;
    let spec_count = spec_count_rows
        .first()
        .and_then(|r| r.get("n"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let session_count_rows = store.query(
        "SELECT COUNT(*) AS n FROM claims \
         WHERE claim_type = 'session' AND json_extract(props, '$.tree') = ?1",
        &[&name],
    )?;
    let session_count = session_count_rows
        .first()
        .and_then(|r| r.get("n"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    json_out(&json!({
        "name": props.get("name").cloned().unwrap_or_else(|| json!(name)),
        "description": props.get("description").cloned().unwrap_or_default(),
        "spec_count": spec_count,
        "session_count": session_count,
    }))
}

/// `tree close <name>` — append a superseding `Tree` claim with
/// `status = "closed"`. When multiple trees share `<name>`, the
/// caller must disambiguate via `--start-id <hash-or-prefix>`.
///
/// Non-destructive: the tree's specs and sessions stay in the log.
/// Hidden from `tree list` by default; surface with `--include-closed`.
fn cmd_tree_close(name: &str, start_id: Option<&str>, session: &Option<String>) -> Result<()> {
    let mut store = SynthStore::discover_for(session)?;
    let rows = load_tree_rows(&store)?;
    let resolved = resolve_trees(&rows);

    // Filter chains to those matching `name` and not already closed.
    let candidates: Vec<&ResolvedTree> = resolved
        .iter()
        .filter(|t| t.name == name)
        .filter(|t| t.status != "closed")
        .collect();

    if candidates.is_empty() {
        bail!(
            "tree '{name}' not found (no live opener with that name). Try \
             `synthesist tree list --include-closed`."
        );
    }

    // Pick the chain to close. With --start-id, resolve via prefix
    // match against each candidate's `start_id`. Without it, single
    // candidate is fine; multiple is ambiguous and bails with the
    // candidate list.
    let target = match start_id {
        Some(needle) => {
            let needle = needle.trim();
            if needle.is_empty() {
                bail!("--start-id must be a non-empty hex prefix or full hash");
            }
            let matches: Vec<&&ResolvedTree> = candidates
                .iter()
                .filter(|t| t.start_id.starts_with(needle))
                .collect();
            match matches.len() {
                0 => bail!(
                    "no tree named '{name}' has start_id starting with '{needle}'. \
                     Live candidates: [{}]",
                    candidates
                        .iter()
                        .map(|t| t.start_id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                1 => *matches[0],
                _ => bail!(
                    "--start-id '{needle}' is ambiguous; matches: [{}]",
                    matches
                        .iter()
                        .map(|t| t.start_id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
        None => match candidates.len() {
            1 => candidates[0],
            _ => bail!(
                "tree '{name}' is ambiguous ({} live candidates). \
                 Re-run with --start-id <hash-or-prefix>; candidates: [{}]",
                candidates.len(),
                candidates
                    .iter()
                    .map(|t| t.start_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    };

    // Locate the head of the chain (the prior claim we'll supersede)
    // by start_id. Re-walk from the opener so we know the prior id
    // even when there have been multiple supersessions.
    let by_id: HashMap<&str, &TreeRow> = rows.iter().map(|r| (r.claim_id.as_str(), r)).collect();
    let mut next_in_chain: HashMap<&str, &str> = HashMap::new();
    for r in &rows {
        if let Some(prior) = r.supersedes.as_deref() {
            next_in_chain.insert(prior, r.claim_id.as_str());
        }
    }
    let opener = by_id
        .get(target.start_id.as_str())
        .ok_or_else(|| anyhow!("internal: opener {} vanished from rows", target.start_id))?;
    let mut head = *opener;
    let mut visited: HashSet<&str> = HashSet::new();
    while let Some(next_id) = next_in_chain.get(head.claim_id.as_str()) {
        if !visited.insert(head.claim_id.as_str()) {
            break;
        }
        match by_id.get(next_id) {
            Some(next_row) => head = next_row,
            None => break,
        }
    }

    let new_props = json!({
        "name": target.name,
        "description": target.description,
        "status": "closed",
    });
    store.append(ClaimType::Tree, new_props, Some(head.claim_id.clone()))?;

    json_out(&json!({
        "closed": true,
        "name": target.name,
        "start_id": target.start_id,
    }))
}
