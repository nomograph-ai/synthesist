# Claim store performance baseline (`SynthStore`)

This document records **quantitative Criterion results** for the v2 claim stack (Automerge log + SQLite view) using this repository’s **minimal** `claims/` tree as the template. Benchmarks copy that tree into a temp directory per iteration; setup (copy) is outside the timed routine where `iter_batched` is used.

## Commands

From the repository root (release profile is default for `cargo bench`):

```bash
cargo bench -p nomograph-synthesist --bench store
```

Optional quick run (fewer samples / shorter — useful when iterating):

```bash
cargo bench -p nomograph-synthesist --bench store -- --quick
```

### Claims directory resolution

Same as `benches/store.rs` (first match wins):

| Priority | Variable | Meaning |
|----------|----------|---------|
| 1 | `SYNTHESIST_BENCH_CLAIMS` | Absolute or relative path to a `claims/` directory that contains `genesis.amc`. |
| 2 | `SYNTHESIST_DIR` | Project root; uses `<SYNTHESIST_DIR>/claims` when `genesis.amc` exists there. |
| 3 | (default) | `<repo>/claims` via `CARGO_MANIFEST_DIR`. |

Example pointing at this repo explicitly:

```bash
export SYNTHESIST_DIR=/var/home/jam/projects/gitlab.com/nomograph/synthesist
cargo bench -p nomograph-synthesist --bench store
```

## Hardware note (reference machine)

Recorded on the system used for the table below:

- **CPU:** AMD Ryzen 9 9950X3D 16-Core Processor (`model name` from `/proc/cpuinfo`)
- **Kernel / arch:** Linux x86_64 (Fedora 44; `uname -a` at capture time)
- **RAM:** ~60 GiB total (~45 GiB available at capture; `free -h`)

Results vary by CPU, disk, and load; treat these as **relative baselines**, not guarantees.

## Results (repo `claims/` — default fixture)

**Run:** `cargo bench -p nomograph-synthesist --bench store` (full run, no `--quick`).  
**Criterion** reports `time: [lower estimate upper]`; the table uses the **estimate** (median) column.

| Criterion group | Benchmark name | Estimate |
|-----------------|----------------|----------|
| `cold_open_materialize_sqlite` | `open_at_after_stale_heads` | 2.8031 ms |
| `warm_open` | `open_at_heads_current` | 1.7109 ms |
| `append_session_claim` | `append_includes_view_sync` | 1.3083 ms (764.38 elem/s) |
| `sync_view` | `rebuild_after_heads_file_removed` | 1.1891 ms |
| `sync_view_noop` | `heads_already_match` | 71.712 µs |
| `query_view_sqlite` | `select_count_star_claims` | 75.144 µs (~13.3 Kelem/s) |

Same benchmark IDs are listed in `benches/README.md` under “Run” / claims env vars.

## External large `claims/` (optional)

To benchmark a **copy** of a large estate (for example a directory tree copied from another project such as **zeel-dev/zd** — operator-supplied; **do not** commit large fixtures into this repo):

1. Copy the entire `claims/` tree to a stable path on disk (not a live directory another process writes).
2. Point benchmarks at that copy:

```bash
export SYNTHESIST_BENCH_CLAIMS=/path/to/your/copy/of/claims
cargo bench -p nomograph-synthesist --bench store
```

Or:

```bash
export SYNTHESIST_DIR=/path/to/project/root   # must contain ./claims/genesis.amc
cargo bench -p nomograph-synthesist --bench store
```

Very large trees spend more wall time in **setup** (directory copy); Criterion’s timed portion still reflects open/sync/query on that tree once copied. Compare runs on the same machine for apples-to-apples conclusions.

## Convenience script

See `scripts/bench-baseline.sh` for a small wrapper that runs the store bench and tees output to `target/bench-store-<timestamp>.log`.
