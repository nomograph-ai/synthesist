# Changelog

All notable changes to `nomograph-claim` follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [3.0.0-rc.1] - 2026-06-02

v3 substrate. The crate keeps the JSON-LD per-asserter log as the
source of truth and replaces the query engine: a redb-backed "gamma"
typed-query index (`claim/src/gamma.rs`) stands in for the former
Oxigraph SPARQL view. Oxigraph and its companion RDF crates are
removed, the v2 Automerge substrate is deleted, and the dependency
tree shrinks accordingly. A minimal v2-read shim is retained so an
existing `.amc` estate can still be drained by migration. The
standalone migrate binary that shipped with v2.5 is removed;
migration is now a subcommand under `synthesist migrate` (see
`synthesist` 3.0.0-rc.1 changelog).

### Added

- **JSON-LD log writer and reader.** Each asserter gets a private
  append-only log at `claims/<asserter>/log.jsonl`. Every claim line
  is a self-describing JSON-LD document with an inline `@context`
  declaring the `synthesist`, `nomograph`, `prov`, and `xsd` prefix
  bindings, plus `@type` and `@id` for supersedes/agreeSnapshot
  predicates. Predicate names are lowerCamelCase throughout. The
  per-asserter logs are the multi-user source of truth.
- **Gamma typed-query index.** A redb-backed index
  (`claim/src/gamma.rs`) replaces the Oxigraph SPARQL view. The
  index is a disposable, gitignored, on-disk redb FILE
  (`claims/_view.gamma`) rebuilt from the per-asserter JSON-LD log
  union, heads-keyed: when the heads signal has not moved the open is
  a no-op, otherwise the index rebuilds from the logs. It exposes
  typed query helpers (live heads, live tasks, session openers,
  dangling supersedes, diamond conflicts, plan-at-risk, counts)
  instead of a SPARQL endpoint. It carries no source-of-truth state
  and can be deleted at any time.
- **Embedded base ontology.** The crate bundles the nomograph base
  ontology as a compiled-in Turtle string. Consumers can emit it with
  the `emit-shacl` binary (shipped from `synthesist`) or read it via
  the `claim::ontology` module.
- **SHACL artifact.** Per-type SHACL Turtle shapes are generated from
  the embedded ontology and can be used as documentation or for
  external validator tooling. The shapes are schema-stable as of
  pre.1; the format is not yet guaranteed across pre releases.
- **Minimal v2-read shim.** A read-only path over the old Automerge
  `.amc` change files remains so `synthesist migrate v2-to-v3` can
  read an existing v2 estate and replay it into v3 logs. It is the
  only v2 surface that survives.

### Removed

- **Oxigraph and the RDF query stack.** `oxigraph`, `oxjsonld`,
  `oxrdf`, and `oxttl` are removed. The crate no longer reparses a
  multi-megabyte N-Quads snapshot on every process open; the gamma
  index reads typed edges directly out of redb. The dependency tree
  shrinks substantially as a result.
- **v2 Automerge substrate.** The v2 Store/View, session, crypto, and
  beacon modules are deleted from `nomograph-claim`. Only the minimal
  v2-read shim (above) remains, scoped to migration.
- **Standalone migrate binary.** The `claim-migrate` binary that
  shipped with claim 2.5.x is removed. Migration is now the
  `synthesist migrate v2-to-v3` subcommand, which lives in the
  engineered migrations module in synthesist. Operators who have
  the standalone binary on `$PATH` should uninstall it before
  installing synthesist 3.0.0-rc.1 to avoid confusion.

### Notes

- **Vocabulary-agnostic substrate.** `nomograph-claim` stores and
  indexes any well-formed claim regardless of `@type`; the synthesist
  vocabulary (ClaimType, per-type validation) lives in the consumer,
  not here.
- **Platforms.** macOS ARM and (now) Linux ARM64 are supported. The
  former Oxigraph/RocksDB `Store::open` panic on macOS ARM no longer
  applies: there is no RocksDB dependency in the gamma path.

## [0.2.0] (2026-04-28)

### Breaking

- The substrate is now type-agnostic for validation. Per-type
  validators (`validate_spec`, `validate_task`, `validate_tree`,
  `validate_discovery`, `validate_campaign`, `validate_session`,
  `validate_phase`, `validate_intent`, `validate_heartbeat`,
  `validate_outcome`, `validate_directive`, `validate_stakeholder`,
  `validate_topic`, `validate_signal`, `validate_disposition`) and
  the `validate_claim` dispatcher have moved out of this crate.
  The `schema` module is removed. Domain validation is now the
  responsibility of the consumer (typically a CLI or library
  layered above `nomograph-workflow`); the substrate stores any
  well-formed claim regardless of `claim_type`. See
  `synthesist::schema` for the consumer-side pattern.
- `Error::Schema(String)` renamed to `Error::Invalid(String)`. The
  variant covered substrate-level argument errors (empty session
  id, unknown claim_type string at parse time) and is no longer
  used for domain validation. Domain validation uses the new
  `validation::SchemaError` type.

### Added

- `validation` module exposes the building blocks consumers need
  to construct their own per-type validators: `obj`, `req_str`,
  `opt_str`, `req_str_array`, `opt_str_array`, `req_int`, and
  `check_enum`. Each helper returns a structured `SchemaError`
  with claim_type, field, actual value, and expected enum set
  populated for callers to format or pattern-match.
- `validation::SchemaError` is a structured error type with
  variants `NotAnObject`, `MissingField`, `EmptyString`,
  `EmptyArray`, `WrongType`, `InvalidEnum`, and `Other`. Replaces
  the prior opaque `Error::Schema(String)` for domain validation.
  Each variant carries enough context for callers to diagnose
  without re-reading the schema or running `strings` on a binary.
- `SchemaError` and `SchemaResult` re-exported at the crate root
  for ergonomic consumer use.

### Changed

- `claim` binary's `append` subcommand no longer validates claims
  against a per-type schema. The binary remains a substrate-only
  debug tool; use a typed consumer CLI such as `synthesist` for
  validated appends.

### Security

- `rustls-webpki` bumped from `0.103.12` to `0.103.13` to address
  RUSTSEC-2026-0104 (reachable panic in certificate revocation list
  parsing). Transitive via the optional `beacon` feature's
  `tokio-tungstenite` dependency. Caught by the v0.2.0 ship
  pipeline's `cargo audit` gate.

## [Unreleased]

### Added

- Initial repository scaffold: crate layout, CLI binary stub, module
  placeholders (`store`, `view`, `crypto`, `session`, `schema`), dual-license,
  CI pipeline include, and cargo-deny configuration. Wave-2 implementor
  agents fill in module bodies; the substrate contract is specified in
  `keaton/research/graph-primitive/BUILDING.md`.
