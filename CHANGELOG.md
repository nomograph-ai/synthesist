# Changelog

All notable changes to `nomograph-claim` follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
