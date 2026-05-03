# Claim store hot paths (t2)

This document traces where time goes when synthesist runs against a `claims/` directory. Crate versions pinned in this repo: **`nomograph-workflow` 0.1.1**, **`nomograph-claim` 0.1.0** (see `Cargo.lock`).

## Call chain for a typical write

1. CLI command → `nomograph_workflow::Store::append` (`discover_for` / `open_at`).
2. **`Store::append`** (workflow `store.rs`): `schema::validate_claim` → **`ClaimStore::append`** → **`View::sync`**.
3. Every successful append ends with **`View::sync(&mut inner)`** so the next read sees the write.

So wall-clock for one command is **at least**: open-path cost + one append + **view sync**.

## Layer A — `nomograph_workflow::Store`

Source: `nomograph-workflow` crate, `src/store.rs`.

| Step | What happens |
|------|----------------|
| `discover` / `open_explicit` | Resolve `SYNTHESIST_DIR` or walk parents to `claims/genesis.amc`. |
| `open_at` | **`ClaimStore::open`** + **`View::open`** + **`view.sync(&mut inner)`** immediately (cold path always tries to reconcile SQLite with Automerge heads). |
| `append` | Validate JSON schema → **`inner.append`** → **`view.sync`** — sync runs **after every append**. |

Implication: there is **no batching** of view refreshes at the workflow layer today; each mutating CLI invocation pays append + sync.

## Layer B — Automerge log (`nomograph_claim::Store`)

Source: `nomograph-claim` crate, `src/store.rs`.

| Operation | Cost drivers |
|-----------|----------------|
| **`Store::open`** | Load `genesis.amc`; optionally apply `snapshot.amc`; **linear replay** of every `changes/<hash>.amc` via `load_incremental` (sorted file order). Large estates = many files = large cumulative replay. |
| **`Store::append`** | Exclusive **`DirLock`** on `claims/.lock`; insert into Automerge list; **`commit`**; **`save_incremental`** (hash-named change file + `fsync_dir`). |
| **`Store::compact`** | Serialize full doc to snapshot; sweep `changes/` — reduces **future** open cost at expense of a heavy compaction pass. |

Automerge CPU time scales with **document size and history**; disk time scales with **number/size of incremental files** and fsync policy.

## Layer C — SQLite view (`nomograph_claim::View`)

Source: `nomograph-claim` crate, `src/view.rs`.

| Operation | Cost drivers |
|-----------|----------------|
| **`View::sync`** | Compare **current Automerge heads** to **`view.heads`**. If equal → **no-op** (cheap). If different → **`rebuild`**. |
| **`View::rebuild`** | **`store.load_claims()`** (walks entire claims list in the doc), **`DROP`/`CREATE` schema**, **`INSERT OR IGNORE`** every claim in a transaction, rewrite **`view.heads`**. |

Upstream comment in `view.rs`: rebuilding is on the order of **~30 µs per claim** in isolation; **dominant cost at scale is volume of claims × SQLite work**, plus whatever it took to get a stale view (heads mismatch).

## Separation summary

| Bucket | Mechanism | Typical symptom |
|--------|-----------|-----------------|
| **Automerge merge/replay** | `Store::open`, `load_incremental`, append transactions | Slow startup; slow append on huge docs |
| **SQLite materialization** | `View::rebuild`, schema drop, bulk inserts | Slow **`sync`** after heads change |
| **Filesystem / durability** | Atomic writes, `fsync_dir`, change-file creation | IO noise; spikes on slow disks |

Profilers should attribute samples across **automerge**, **rusqlite**, and **`fsync`** (e.g. `perf`, `cargo flamegraph` on a bench binary).

## Relationship to `benches/store.rs`

The bench harness measures synthesist’s **`SynthStore`** end-to-end on copied fixtures: cold open (stale heads → rebuild), warm open, append + implicit sync, explicit `sync_view`, and SQL query latency. Use those groups to match **this** decomposition when interpreting regressions.
