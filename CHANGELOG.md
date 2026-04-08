# Changelog

## v1.0.0 (unreleased)

Full rewrite from Go+Dolt to Rust+SQLite. Schema stabilized for v1.0.0.

### Changed

- **Storage**: embedded Dolt replaced by SQLite via rusqlite (bundled).
  No CGo, no ICU, no system dependencies beyond a C compiler.
- **Data directory**: `.synth/` renamed to `synthesist/` (visible, full name).
- **Schema**: 30 tables reduced to 16. Literature-informed cutline defers
  14 tables pending empirical validation (see docs/architecture-v1.md).
- **CLI**: ~55 command paths reduced to ~40. Consistent `add` verb for
  creation. `tree/spec` format everywhere.
- **Sessions**: Dolt branches replaced by per-file SQLite copies with
  ATTACH-based three-way merge. PK-aware EXCEPT diffing with conflict
  detection.
- **Phase enforcement**: transitions validated (orient->plan->agree->execute).
  Cannot jump from PLAN to EXECUTE without passing through AGREE.
- **CI**: custom Go pipeline replaced by nomograph/pipeline rust-cli component.

### Added

- `task reset` command for crash recovery (orphaned in_progress tasks).
- `task show` and `task update` commands.
- `spec list` and `spec update` commands (with status and outcome fields).
- `synthesist sql` command for ad-hoc read-only queries.
- Version check against GitLab releases API.
- Session ID validation against path traversal.
- Transaction wrapping for multi-table writes (task add, disposition
  supersede, import, claim).
- Foreign key constraints and performance indexes in schema.
- 22 integration tests covering task DAG, sessions, dispositions,
  phase enforcement, security.

### Removed (deferred to future releases)

- Directions (may be redundant with dispositions)
- Influences (needs empirical evidence)
- Patterns (belongs in context files per literature)
- Retros/transforms (representation unclear)
- Propagation chains (staleness mechanism needs design)
- Archives (replaced by spec status field)
- Threads (merged into session_meta)
- Quality/validations (unimplemented in v5)

Each deferred feature has a documented re-entry criterion in
docs/architecture-v1.md.

## v5.4.0 (2026-04-07)

- `task reset`: release orphaned in_progress tasks to pending.
- Version check: JSON output with update availability from GitLab API.

## v5.3.4 and earlier

See git history for the Go+Dolt era.
