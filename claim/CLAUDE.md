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

The `claim` binary is produced at `target/release/claim`.

## Storage Contract

The substrate owns a visible `claims/` directory at project root (D3).

| File | Tracked | Purpose |
|------|---------|---------|
| `claims/genesis.amc` | yes | Bootstrap Automerge doc |
| `claims/changes/<hash>.amc` | yes | Content-addressed append-only |
| `claims/config.toml` | yes | Schema version |
| `claims/snapshot.amc` | no | Local compaction cache |
| `claims/view.sqlite` | no | Local SQL projection |
| `claims/view.heads` | no | Heads-stale check |

Never read or write these files outside the `store` and `view` modules.
CLI subcommands route through the public API.

## Module Responsibilities

- `claim` -- claim struct, id hash, supersession helpers. Already scaffolded.
- `store` -- `Store` type owning the `claims/` directory. Append, load,
  save-incremental. Automerge-backed. Wave 2.
- `view` -- `View` type projecting the Automerge doc into SQLite. Rebuild
  on heads mismatch. Wave 2.
- `crypto` -- Argon2id passphrase KDF + ChaCha20-Poly1305 AEAD envelope
  for changes going over the wire. Wave 2.
- `session` -- Session claim writer; subsequent writes inherit the
  session tag (D14, no file-copy). Wave 2.
- `schema` -- per-claim-type JSON schema validation at the `Store::append`
  boundary. Wave 2.
- `error` -- `thiserror`-derived `Error` + `Result`. Already scaffolded.
- `bin/claim.rs` -- clap-derive CLI; maps subcommands to public API
  calls. Wave 2 fills in subcommand bodies.

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

The binary is `claim`. Subcommands currently stubbed:

- `claim init` -- scaffold `claims/` in the cwd project root
- `claim append` -- append a typed claim
- `claim list` -- list current state
- `claim history <id>` -- walk supersession chain
- `claim conflicts` -- surface unresolved conflicts
- `claim view sync` -- rebuild SQLite projection

Wave 2 agents fill in the bodies. Do not add new subcommands without
updating BUILDING.md first.

## Release Checklist

Before tagging:

1. `cargo build --release && cargo test && cargo clippy -- -D warnings`
2. Push to main -- CI pipeline green
3. README.md, CHANGELOG.md, CLAUDE.md reflect the release
4. `git tag -a vX.Y.Z -m "release notes"` -- annotated tag only
5. `git push --tags` -- wait for tag pipeline
6. `glab release create vX.Y.Z --notes "release notes"`

Never tag before CI passes. Never tag with stale documentation.
