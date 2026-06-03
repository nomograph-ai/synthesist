# nomograph-claim

Bi-temporal claim substrate. Rust library (`[lib]`-only) shared across
the nomograph estate: `claim`, `synthesist`, `lattice`.

## Source of Truth

All design decisions are locked in the keaton repo:

- `keaton/research/graph-primitive/BUILDING.md` -- decisions D1 through D20,
  claim schema (16 types), file naming, asserter format.
- `keaton/research/graph-primitive/BUILDING-lever-principles.md` -- LLM
  correctness checklist that every Rust module must satisfy.
- `keaton/research/graph-primitive/BUILDING-pipeline-catalog.md` -- CI
  pipeline include block (pasted verbatim; do not modify).

Read these before making design decisions. If a rule here contradicts
BUILDING.md, BUILDING.md wins. If a code-style rule here contradicts
BUILDING-lever-principles.md, lever wins.

## Build

```
cargo build                 # dev build
cargo build --release       # release build
cargo test                  # unit + integration tests
cargo clippy -- -D warnings # lint; zero warnings tolerated
cargo doc --no-deps         # local docs; no broken intradoc links
```

`nomograph-claim` is a `[lib]`-only crate; it ships no binary. The
consuming modules (synthesist, lattice) own the CLI surface.

## Storage Contract

The substrate owns a visible `claims/` directory at project root (D3).
Each writer appends JSON-LD documents to its own log; the union of those
logs is the source of truth. The gamma typed-query index is a disposable
local cache rebuilt from the union.

| File | Tracked | Purpose |
|------|---------|---------|
| `claims/<asserter>/log.jsonl` | yes | Per-asserter append-only JSON-LD log |
| `claims/config.toml` | yes | Schema version |
| `claims/_view.gamma` | no | redb gamma index (disposable cache) |

Never read or write these files outside the `log` and `gamma` modules.
Consumers route through the public API.

## Module Responsibilities

- `claim` -- claim struct, id hash, supersession helpers.
- `log` -- `LogWriter` / `LogReader` over `claims/<asserter>/log.jsonl`.
  Appends one JSON-LD doc per line via the temp-file-plus-rename atomic
  write strategy. The v3 write/read surface.
- `gamma` -- `Gamma`, the redb-backed POS/PSO typed-query index. A
  derived projection of the log union; rebuilt when `heads` shows the
  logs changed. Replaces the v2 SQLite view and the v3-alpha Oxigraph
  engine.
- `heads` -- staleness signal: hashes the asserter directory names and
  per-file line counts so `Gamma::sync` skips a rebuild when the logs
  are unchanged.
- `jsonld` -- the compact JSON-LD on-disk form: base @context body,
  envelope helpers, asserter-IRI handling.
- `asserter` -- asserter id parsing and the per-asserter directory name
  derivation.
- `prov` -- PROV envelope predicates (generatedAtTime, wasAttributedTo,
  wasRevisionOf, parentAsserter).
- `ontology` -- substrate vocabulary constants (prefixes, predicate
  IRIs).
- `store` -- v2-READ shim only. The legacy Automerge `Store` survives so
  `synthesist migrate v2-to-v3` can drain old `claims/changes/*.amc`
  trees; `open` + `load_claims` is the primary surface, with
  `init`/`append` retained to build migration fixtures. Not part of the
  v3 runtime path.
- `error` -- `thiserror`-derived `Error` + `Result`.

## Conventions

- **Errors**: `thiserror::Error` in the library; `anyhow::Error` only
  inside the `bin/` entrypoint. Error messages are single-line and name
  the next action. No trailing periods.
- **Docs**: every public item carries a `///` summary. Non-trivial
  items carry a doctest. Every module top opens with `//!`.
- **Tests**: unit tests in-file under `#[cfg(test)]`. Integration tests
  in `tests/` use only the public API.
- **Property tests** are required for: content hash stability (reorder
  fields -> same hash), merge commutativity (`merge(a,b) == merge(b,a)`),
  E2EE round-trip (`decrypt(encrypt(x, k), k) == x`), supersession-chain
  well-formedness, view-rebuild determinism.
- **No `unwrap` / `expect`** outside tests unless a comment justifies it.
- **No TODO** without a date + owner + tracking issue.
- **File size**: keep each file focused, one concern per file.

## CLI Shape

`nomograph-claim` exposes no binary; it is a library. The substrate
verbs are public API on the `log`, `gamma`, and `claim` modules:

- append a typed claim -- `log::LogWriter`
- read current state / walk a supersession chain -- `log::LogReader`
  plus the `gamma` index
- rebuild the typed-query projection -- `gamma::Gamma::sync` (driven by
  the `heads` staleness signal)

Consuming modules (synthesist) map their own subcommands onto this API.
Do not add public surface without updating BUILDING.md first.

## Release Checklist

Before tagging:

1. `cargo build --release && cargo test && cargo clippy -- -D warnings`
2. Push to main -- CI pipeline green
3. README.md, CHANGELOG.md, CLAUDE.md reflect the release
4. `git tag -a vX.Y.Z -m "release notes"` -- annotated tag only
5. `git push --tags` -- wait for tag pipeline
6. `glab release create vX.Y.Z --notes "release notes"`

Never tag before CI passes. Never tag with stale documentation.
