# Synthesist microbenchmarks

Rust benches target the v2 stack (`SynthStore` / `nomograph_workflow::Store`): Automerge claim log + SQLite view materialization + representative read queries.

## Run

From the repo root (release mode; default for `cargo bench`):

```bash
cargo bench -p nomograph-synthesist --bench store
```

## Claims directory

Benchmarks copy the template `claims/` tree into a temporary directory per iteration (or batch setup). Your repo `claims/` is the default small fixture.

| Variable | Meaning |
|----------|---------|
| `SYNTHESIST_BENCH_CLAIMS` | Path to a `claims/` directory (`genesis.amc` required). Highest priority. |
| `SYNTHESIST_DIR` | Project root containing `claims/` (same convention as the CLI). Used if the first var is unset and `<SYNTHESIST_DIR>/claims/genesis.amc` exists. |

Point either variable at a **copy** of a large estate elsewhere on disk; do not benchmark against a live directory another process might write.

## Caveats

- **Setup vs. measurement**: `iter_batched` separates directory copy + tempdir creation (setup) from the timed routine. Very large trees spend noticeable time only in setup; watch Criterion’s reported times vs. wall-clock.
- **Append benchmark**: appends unique `Session` claims so the log grows across iterations within a batch; for a huge fixture this slightly shifts append cost.
