# Contributing

Thanks for your interest. This crate ships under the nomograph estate and
shares a common Rust contribution flow with `claim`, `synthesist`, and
`lattice`.

## Local checks

```sh
cargo test                                      # run the test suite
cargo fmt                                       # format the tree
cargo clippy --all-targets -- -D warnings       # lint (warnings are errors)
```

CI runs the same four stages (check, fmt, clippy, test) on every push.

## Monorepo workspace

This repository is the workspace. The `synthesist` crate lives at the
repo root; the `claim` substrate is a workspace member under `claim/`
(vendored with full git history via subtree merge). There is no
separate `workflow` crate in v3 -- its leaf helpers and the `Phase`
enum folded into synthesist, and the estate now publishes two crates
(`claim`, `synthesist`) instead of three.

The layout:

```
synthesist/            # repo root + workspace manifest
  Cargo.toml           # [workspace] members = ["claim"] + [package] synthesist
  src/                 # synthesist crate
  claim/               # nomograph-claim, the v3 substrate ([lib]-only)
    Cargo.toml         # member manifest
```

`synthesist` depends on `nomograph-claim` via a path-dep
(`path = "claim"`) pinned to the same version. `cargo build
--workspace` from the repo root resolves both crates against one
shared `target/` and a single canonical `Cargo.lock`. No sibling
checkouts, no out-of-tree manifest, no symlinks.

When 3.0.0 final lands and the crates publish to crates.io, the
path-dep unwinds into a version-pinned registry dep.

## Licensing

All contributions are accepted under the [MIT License](LICENSE). By submitting
a change you agree to license it under those terms.

## Architecture notes

Synthesist is the spec-graph manager. It sits directly on the
`nomograph-claim` substrate (workspace member under `claim/`); the
phase machine and store adapter live in synthesist, not a separate
workflow crate. Before touching the phase machine or store layer,
read the architecture docs in the claim crate:

- `claim/SYNC.md` — per-asserter log union, heads, and gamma rebuild
- `claim/IDENTITY.md` — asserter attribution

## Backwards-compatibility policy

Three surfaces have different compat needs and follow distinct rules:

- **Claim format on disk** — claims-forward only. New binaries must
  read every claim shape ever shipped, because estates in the wild
  carry unbounded history. The reverse (old binaries reading claims
  written by new versions) is not a contract we keep — agents and
  humans on a stale binary may see new fields they don't understand,
  and that's acceptable. Concretely: schema changes that drop or
  rename a required field require a migration tool that rewrites old
  claims; schema changes that add an optional field need no
  migration but also need no compat shim on the new code path.
- **CLI surface** — additive only within a major. Existing flags, commands,
  and JSON output shapes do not change in incompatible ways inside a major
  version. Agents pattern-match on this surface; breaking it mid-version
  invalidates working agent prompts. New flags and commands are fine.
- **Library API** (`nomograph_claim`) — semver. Public
  types and functions follow standard Rust semver: 0.x bumps the minor for
  breaking changes; 1.0+ bumps the major. Re-exports and internal
  refactors that don't change the public surface are patch-level.

In doubt, prefer the strictest applicable rule. A change that touches
two surfaces takes the strictest one's policy.

## Schema evolution

Domain claim schemas (Spec, Task, Tree, Discovery, Outcome, etc.) live in
`src/schema/<type>.rs`, each with a `pub const` for any enum value set
the validator references. When adding or extending a schema:

1. Update the const if the change is to an enum.
2. Update the validator in the same file.
3. Update CLI integrations that consume the const (clap
   `PossibleValuesParser` references the same constant — there is nothing
   to keep in sync because there is only one definition).
4. Add a parity test asserting CLI accepts iff schema accepts.
5. CHANGELOG entry; if the change is non-additive, also add migration
   tooling under `src/cmd_migrate.rs`.

## Schema-CLI parity is structural

Per the Lever principle "specify in one place, generate everywhere," the
same `pub const` slice drives both the schema validator and the CLI
parser. Drift is impossible because there is only one source. New
contributors should reach for the schema module first when adding a new
claim type or extending an existing one; everything else (CLI, error
formatting, eventually skill text) consumes from there.
