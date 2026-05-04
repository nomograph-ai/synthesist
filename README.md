![hero](hero.svg)

# nomograph-claim

[![pipeline](https://gitlab.com/nomograph/claim/badges/main/pipeline.svg)](https://gitlab.com/nomograph/claim/-/pipelines)
[![license](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![built with GitLab](https://img.shields.io/badge/built_with-GitLab-FC6D26?logo=gitlab)](https://gitlab.com/nomograph/claim)

Bi-temporal CRDT claim substrate with asserter attribution and E2EE.

`nomograph-claim` is the storage primitive shared by every nomograph tool
that writes state: synthesist (workflow), lattice (observation), seer
(propagation). It gives each tool a single append-only log of typed
assertions that merge losslessly across peers, preserve who asserted
what when, and stay encrypted at rest and in transit.

The substrate is deliberately small. A claim is a typed, signed, dated
record of what someone asserts to be true. Storage is Automerge over
content-addressed `.amc` files under a visible `claims/` directory.
Projection to SQLite is a local, rebuildable cache.

## Install

### Source

```bash
git clone https://gitlab.com/nomograph/claim.git
cd claim && cargo build --release
```

Requires Rust 1.88+. No system dependencies beyond a C compiler.

### Library (Cargo)

```toml
[dependencies]
nomograph-claim = "0.2"
```

## Quickstart

```bash
# Initialize a claims/ directory alongside your project
claim init

# Append a typed claim
claim append --type spec --props '{"goal":"v1"}' --as user:gitlab:andunn

# List current state
claim list

# Walk the supersession chain for a claim
claim history <claim-id>

# Surface unresolved conflicts
claim conflicts

# Rebuild the local SQLite projection
claim view sync
```

Library use:

```rust
use nomograph_claim::{Claim, ClaimType};

let claim = Claim::new(
    ClaimType::Spec,
    serde_json::json!({ "goal": "v1" }),
    "user:gitlab:andunn",
);
```

## Storage Layout

At project root, under `claims/`:

| File | Tracked | Purpose |
|------|---------|---------|
| `genesis.amc` | yes | Bootstrap document |
| `changes/<hash>.amc` | yes | Content-addressed append-only changes |
| `config.toml` | yes | Schema version, project metadata |
| `snapshot.amc` | no | Local compaction cache |
| `view.sqlite` | no | Local SQL projection |
| `view.heads` | no | Heads-stale check |

Encryption keys live out-of-tree at `~/.config/nomograph/keys/<project>.key`.

## Architecture

The locked design lives in the keaton repo under
`research/graph-primitive/`:

- [`BUILDING.md`](https://gitlab.com/nomograph/keaton/-/blob/main/research/graph-primitive/BUILDING.md)
  -- locked decisions (D1-D20), claim schema, file naming.
- [`BUILDING-lever-principles.md`](https://gitlab.com/nomograph/keaton/-/blob/main/research/graph-primitive/BUILDING-lever-principles.md)
  -- LLM-correctness checklist for Rust implementors.
- [`BUILDING-pipeline-catalog.md`](https://gitlab.com/nomograph/keaton/-/blob/main/research/graph-primitive/BUILDING-pipeline-catalog.md)
  -- CI and container catalog.

## Module Map

| Module | Responsibility |
|--------|----------------|
| `claim` | Claim type, id computation, supersession |
| `store` | `claims/` directory I/O, Automerge append + merge |
| `view` | SQLite projection, heads-stale rebuild |
| `crypto` | Argon2id KDF, ChaCha20-Poly1305 AEAD |
| `session` | Session claim semantics (D14) |
| `schema` | Per-claim-type JSON validation |
| `error` | `thiserror`-derived error surface |

## Building

```bash
cargo build --release
cargo test
cargo clippy -- -D warnings
cargo doc --no-deps --open
```

## Relationship to `nomograph-workflow`

`nomograph-claim` is the storage substrate. `nomograph-workflow` is the
thin shared logic layer that sits on top (Store adapter, phase state
machine, helpers shared by `synthesist` and `lattice`). They are
released independently because their consumers and cadences differ:
substrate changes ripple through every tool that writes state, while
workflow changes affect only the binaries that share workflow logic.
Keep them as two crates.

## License

MIT. See [LICENSE](LICENSE).
