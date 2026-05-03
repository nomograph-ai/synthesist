# Performance recommendation (t4)

Ranked actions to reduce synthesist latency on large estates while preserving CRDT + append-only semantics. Grounded in **`docs/perf-hot-paths.md`**, **`docs/perf-options-matrix.md`**, and **`docs/perf-baseline.md`**.

## Ranked actions

### 1 — Instrument upstream hotspots (nomograph-claim / workflow)

Add scoped tracing or counters inside:

- `nomograph_claim::Store::open` — time spent in snapshot replay vs per-file `load_incremental`.
- `nomograph_claim::View::rebuild` — time in `load_claims` vs SQLite insert batch.

**Rollout:** dev builds / feature flag only first; keep release overhead minimal.

**Risk:** Low.

### 2 — Operational compaction policy

Expose or document **`Store::compact`** (already in claim crate) for estates with huge `changes/` directories. Compaction reduces **open** cost; scheduling must avoid fighting concurrent CLI usage (flock already serializes).

**Risk:** Medium — compaction is expensive; needs ops guidance (when idle, backup first).

### 3 — API: batched writes at workflow layer

Extend workflow **`Store`** (future) with **`append_batch`** or internal queue flushing **one** `View::sync` per batch for scripted migrations/importers. CLI would remain per-command unless UX accepts deferred consistency.

**Risk:** Medium — correctness and error surfaces must mirror single-append semantics.

### 4 — Incremental SQLite projection

Replace full `rebuild()` with incremental updates when heads delta is small.

**Risk:** High — correctness proof + tests for supersession and duplicate ids.

## Validation plan

| Gate | Check |
|------|--------|
| **Correctness** | `make test`; migration/import roundtrips; property tests if touching View. |
| **Conflicts** | `synthesist conflicts` on fixtures with diamond supersession scenarios after substrate changes. |
| **Perf** | `cargo bench -p nomograph-synthesist --bench store` + optional large `claims/` copy (external); compare to **`docs/perf-baseline.md`**. |
| **Durability** | No removal of `atomic_write` / `fsync_dir` without a reviewed durability story. |

## Owners / dependencies

- **`nomograph-claim`**: View + Store internals.
- **`nomograph-workflow`**: Adapter policy (when to sync).
- **`synthesist`**: CLI and benches; dependency bumps after upstream releases.

Issue tracker: **https://gitlab.com/nomograph/synthesist/-/work_items/7**
