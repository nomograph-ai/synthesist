---
name: synthesist-perf-experiments
description: >-
  Iterates on synthesist / nomograph-claim storage performance with reproducible
  benchmarks and CRDT-safe experiments. Covers isolated git worktrees or branches
  per hypothesis, measuring append + sync_view + read paths, and validating
  invariants with make test && make lint. Use when speeding up the Automerge claim
  log or SQLite view materialization, running perf agents, or benchmarking claims/
  workloads (including large external estates as reference only).
disable-model-invocation: true
---

# Synthesist performance experiments

## Authority

- **Spec:** `nomograph/crdt-storage-performance` in this repo’s `claims/` (Synthesist CLI only — no hand-editing stores).
- **Tracking issue:** https://gitlab.com/nomograph/synthesist/-/work_items/7
- **Substrate:** Hot paths live in `nomograph_workflow::Store` / `nomograph-claim`; synthesist wraps them. Profiling may require changes in those crates — coordinate versions in `Cargo.toml`.

## What we know is slow

- **Large `claims/` logs:** Each CLI write tends to trigger **view materialization** (`sync_view`) after append — tens of seconds on big estates is consistent with full replay / merge + SQLite rebuild, not with parsing a tiny new change file alone.
- **Separate concerns when benchmarking:** (1) Automerge load/merge, (2) append to log, (3) **view sync** to SQLite, (4) read/query. Any optimization must identify which segment moved.

## Non-negotiables (do not “optimize” these away)

1. **CRDT merge semantics** — multi-writer correctness; no “single primary lock” as the source of truth.
2. **Append-only, content-addressed claim history** — no rewriting history in place.
3. **Caches are disposable** — `snapshot.amc`, `view.sqlite`, `view.heads` must remain safe to delete and rebuild from the log.

If a shortcut violates any of the above, stop and document the rejection in a discovery or issue comment.

## Agent iteration loop (one lane)

1. **Hypothesis** — e.g. “batch sync_view after N appends” or “incremental SQL delta from last heads.”
2. **Isolate** — `git worktree add` or a dedicated branch off `main`; one experiment per lane so agents do not stomp each other.
3. **Implement** — smallest diff in synthesist / workflow / claim as needed.
4. **Measure** — run the in-repo benchmark harness once task **t5** exists (`cargo bench`, `hyperfine`, or project script). Record: hardware note, commit SHA, claims dir size (file count / bytes optional).
5. **Verify** — `make test && make lint` (or project equivalent). For behavioral guarantees, exercise conflict/diamond scenarios if touching merge or supersession (`synthesist conflicts` on representative fixtures).
6. **Record** — `synthesist discovery add nomograph/crdt-storage-performance --finding "..." --impact high` **or** a short GitLab comment with numbers and commit link.

## Parallel agents

- Assign **non-overlapping** hypotheses or **non-overlapping crates** (e.g. one agent workflow Store, another CLI batching).
- Merge order: land **harness first** (t5) when possible so all lanes share the same ruler.

## Commands (adapt as harness lands)

```bash
# From repo root; pick session from spec work
export SYNTHESIST_SESSION=crdt-storage-synth-2026-05   # if writing discoveries

make build && make test && make lint
# After t5:
# cargo bench -p nomograph-synthesist --bench store   # example; wire real target when added
```

## Upstream scope

If the win requires **nomograph-claim** or **nomograph-workflow** API changes, open or link a GitLab issue/MR there and bump the dependency in synthesist after merge.
