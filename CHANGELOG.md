# Changelog

## [Unreleased]

### Added

- `synthesist serve` -- local HTTP dashboard for browsing the claim
  graph in a browser. Server-rendered HTML built on axum + tokio, with
  push-based refresh via Server-Sent Events: a filesystem watcher on
  `claims/changes/` ticks the SSE stream and the page swaps in fresh
  state without reload, no timed polling. Dashboard exposes a network
  view (combined network/tree, d3-force layout) alongside the trees
  view, with a recent activity section that shows relative timestamps
  and a session claims drill-down. Persistent `<details>` open state
  survives refreshes. Design pass v1 lands the nomograph palette,
  DM Mono / DM Sans typography, the callout pattern, and the full
  nomograph mark in the header. Default bind is `127.0.0.1:5179`;
  `--bind-all` opens it for cross-machine review. Routes: `/`,
  `/api/state`, `/api/graph`, `/events`.
- `tree close <name>` closes a tree by appending a superseding claim
  with `closed: true`. Closed trees disappear from default
  `tree list` output.
- `tree list --include-closed` reveals closed trees alongside open
  ones, with their `closed: true` status.
- `tree close --start-id <hash>` and `session close --start-id <hash>`
  for disambiguation when multiple trees or sessions share the same
  name. The `start-id` is the opener claim's content-addressed hash;
  `tree list` and `session list` print it.
- Treatment v0 surface improvements driven by the jig agent-shape
  battery. `tree show <name>` prints a single tree's status,
  description, and counts. `phase get` is an alias of `phase show`.
  `spec list --tree <name>` accepts the flag form alongside the
  positional form; both shapes appeared frequently in agent
  invocations.
- `agent-shape.toml` declares `[commands].top_level` so the jig
  cross-validates the documented command surface against the binary's
  actual top-level subcommand list.

### Changed

- `hero.svg` refreshed to v2 surface: the OV-1 now describes the
  claim substrate without `lattice` references that no longer apply
  to the synthesist scope.
- `avatar.svg` lightly updated to render in IBM Plex Mono.
- Test helpers strip `SYNTHESIST_DIR` and `SYNTHESIST_SESSION` from
  the inherited environment before invoking the binary, so running
  the test suite from inside an active session no longer pollutes
  the caller's estate or causes spurious test failures.

## [2.1.1] (2026-04-20)

### Fixed

- `--data-dir` flag and `SYNTHESIST_DIR` environment variable are
  now honored. A regression from the v2 substrate rewrite left the
  flag wired but unread: the CLI forwarded `--data-dir` into the
  env var, but the workflow crate's `Store::discover` only consulted
  cwd. Any invocation from a directory with no claim ancestry fell
  through to a silent auto-init at `cwd/claims/`, making the flag
  appear to "succeed" while never opening the intended store.
- Explicit overrides now fail loudly when the path does not exist,
  is not a directory, or has no `claims/genesis.amc`. The silent
  fresh-init path only runs for the no-override case.
- Worktrees and other detached checkouts (sibling of main, not
  descendant) can now reach a main checkout's claim store via
  `synthesist --data-dir /path/to/main status`.

### Dependencies

- `nomograph-workflow` bumped to `0.1.1` for the `Store::discover`
  fix. No API-breaking changes in either crate.

## [2.1.0] (2026-04-20)

### Added

- `synthesist conflicts` lists diamond conflicts in the claim log:
  prior claims superseded by more than one live successor. Useful
  after CRDT merge when concurrent writers have disagreed on the
  replacement for a prior claim. Resolution is to append a new claim
  that supersedes the contested pair.

### Changed

- `stakeholder`, `disposition`, `signal`, and `stance` commands now
  print a single "moved to lattice" pointer regardless of args.
  Previously, clap would reject missing required arguments before
  reaching the interceptor, yielding a confusing error rather than
  the helpful pointer.
- `moved_to_lattice` message updated to reflect that `lattice` is a
  private repository pending origination review and is not yet on
  crates.io; the text names the future install path.

### Fixed

- Skill file, README, and MIGRATION.md now show `migrate v1-to-v2`
  with its required `--from` and `--to` flags. Copying the skill
  verbatim previously produced a clap rejection.
- Removed references to a standalone `migrate-v1-to-v2` executable.
  v2.1 folded the migrator into the subcommand; no separate binary
  ships.
- Skill's `conflicts` command reference rewritten to describe actual
  output (diamond conflicts) rather than "unresolved supersessions".

## [2.0.0] (2026-04-19)

### Breaking

- Storage moved from `.synth/main.db` (SQLite) to `claims/` (claim-core
  + Automerge). The v1 database is no longer read at runtime; use the
  migration tool to port existing data.
- `session merge` / `session discard` removed. CRDT merges are
  automatic via claim-core; there is no per-session database copy to
  merge or discard.
- Stakeholder / disposition / signal / topic commands removed. These
  observation-layer concerns are now owned by
  [`lattice`](https://gitlab.com/nomograph/lattice).
- Phase is now per-session, not a global singleton. Each session
  tracks its own phase via a Phase claim.

### Added

- Multi-user workflow via the
  [`nomograph-claim`](https://gitlab.com/nomograph/claim) substrate,
  with optional end-to-end encrypted relay through
  [`beacon`](https://gitlab.com/nomograph/beacon).
- Full supersession history per field. Previous values are retained
  in the claim log; in v1 they were overwritten in place.
- `synthesist migrate v1-to-v2 --from <v1.db> --to <claims-dir>`
  ports an existing `.synth/main.db` to the new `claims/` layout,
  preserving original `created_at` timestamps as `asserted_at`. The
  earlier standalone `migrate-v1-to-v2` binary was folded into this
  subcommand in v2.1; there is no separate executable.
- `synthesist conflicts` lists diamond conflicts (same prior claim
  superseded by more than one live successor) so concurrent writers
  can resolve divergent supersession chains.

### Changed

- CLI surface preserved where possible. Minor output formatting
  differences may appear where underlying fields moved to claims
  (e.g. timestamps now render as `asserted_at`).

### Removed

- Commands: `synthesist stakeholder`, `synthesist disposition`,
  `synthesist signal`, `synthesist topic`, `synthesist stance`,
  `synthesist landscape` (replaced by equivalents in `lattice`).
- Commands: `session merge`, `session discard` (automatic in the
  CRDT model).
- The `.synth/` directory (now `claims/`).

## v1.3.0 (2026-04-18)

### Added

- `--data-dir` global CLI flag and `SYNTHESIST_DIR` environment variable
  override the parent-directory walk for locating the synthesist data
  directory. Resolution order: `--data-dir` flag > `SYNTHESIST_DIR` env
  > parent-directory walk (unchanged fallback).

  Intended use: git worktrees and other detached checkouts that have no
  `synthesist/main.db` in their ancestry but need to reach a main
  checkout's data directory. Previously required a filesystem symlink;
  the env var is the architecturally correct solution.

  Clear errors when the explicit path exists but contains no `main.db`,
  and when no data directory is found via any method.

## v1.2.1 (2026-04-10)

### Fixed

- `--version` / `-V` now works. Previously returned exit code 2 with an unhelpful
  "unexpected argument" error, breaking automated verification scripts.

## v1.0.0 (2026-04-07)

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
