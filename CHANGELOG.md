# Changelog

All notable changes to `nomograph-claim` follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [3.0.0-pre.1] - 2026-05-29

v3 substrate. The crate grows a JSON-LD log layer, a SPARQL graph
view backed by Oxigraph, named-graph routing, an embedded base
ontology, a SHACL Turtle artifact, and v2 surface deprecations that
schedule removal for 3.0.0 final. The standalone migrate binary that
shipped with v2.5 is removed; migration is now a subcommand under
`synthesist migrate` (see `synthesist` 3.0.0-pre.1 changelog).

### Added

- **JSON-LD log writer and reader.** Each asserter gets a private
  append-only log at `claims/<asserter>/log.jsonl`. Every claim line
  is a self-describing JSON-LD document with an inline `@context`
  declaring the `synthesist`, `nomograph`, `prov`, and `xsd` prefix
  bindings, plus `@type` and `@id` for supersedes/agreeSnapshot
  predicates. Predicate names are lowerCamelCase throughout.
  The writer and reader are byte-compatible: the v2-to-v3 migration
  and the dual-write path produce identical documents.
- **`GraphView` backed by Oxigraph.** Named-graph routing assigns
  each module prefix its own named graph inside the Oxigraph store
  (`Store::open` for persistent, in-memory rebuild on fallback).
  SPARQL queries run directly against the graph without round-tripping
  through JSON; the graph view is exposed to callers via the
  `claim::graph` module.
- **Named-graph routing.** Module prefixes in the claim type field
  drive graph assignment; claims land in the graph that matches their
  prefix, allowing cross-type SPARQL joins by named-graph scope.
- **Embedded base ontology.** The crate bundles the nomograph base
  ontology as a compiled-in Turtle string. Consumers can emit it with
  the `emit-shacl` binary (shipped from `synthesist`) or read it via
  the `claim::ontology` module.
- **SHACL artifact.** Per-type SHACL Turtle shapes are generated from
  the embedded ontology and can be used as documentation or for
  external validator tooling. The shapes are schema-stable as of
  pre.1; the format is not yet guaranteed across pre releases.
- **v2 surface deprecations.** The `.amc`-based store modules are
  annotated `#[deprecated]` with guidance pointing at the v3
  equivalents. Consumers see 200-300 warnings on a clean build;
  this is intentional. The deprecated APIs are scheduled for removal
  in 3.0.0 final.

### Removed

- **Standalone migrate binary.** The `claim-migrate` binary that
  shipped with claim 2.5.x is removed. Migration is now the
  `synthesist migrate v2-to-v3` subcommand, which lives in the
  engineered migrations module in synthesist. Operators who have
  the standalone binary on `$PATH` should uninstall it before
  installing synthesist 3.0.0-pre.1 to avoid confusion.

### Known issues

- **macOS ARM: Oxigraph `Store::open` panics (RocksDB
  `TryFromIntError`).** The persistent Oxigraph store triggers a
  panic on macOS ARM via oxigraph 0.4.11's RocksDB backend. The
  `cmd_overlay` caller catches the panic and falls back to an
  in-memory graph rebuild for the duration of the command. No data
  loss occurs; the fallback is transparent to the operator. Root
  cause investigation is tracked under Phase C.1 of the v3
  integration plan. A fix or workaround is expected before 3.0.0
  final.
- **200-300 deprecation warnings on consumers.** The v2 `.amc`
  surface carries `#[deprecated]` attributes. Consumers of
  `nomograph-claim` 3.0.0-pre.1 that have not yet migrated will
  see a large warning spray on `cargo build`. The warnings are
  intentional; the deprecated surface is still functional for the
  duration of the pre.1 cycle, which operates in dual-write mode.
- **Runtime manifest dispatch is parser-side filtering only.**
  Named-graph routing filters claims at parse time; the runtime
  rejection layer that refuses writes for misrouted claims is
  deferred. Phase D (rejection layer) targets 3.0.0-pre.2 if not
  landed before the pre.1 tag.

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
