# Changelog

## [2.4.0] (2026-04-28)

This release is a cohesive architectural pass driven by community
feedback (issues #5, #6, #7 from
[Josh Meekhof](https://gitlab.com/jmeekhof)). Rather than patch each
issue at the symptom, v2.4.0 addresses the underlying classes:
substrate type-agnosticism, single-source-of-truth schemas with
structured validation errors, a real DAG primitive and unified task
mutation surface, and operator-grade compaction tooling drawn from
Josh's reference implementation in MR !8.

### Breaking dependencies

- Bumps `nomograph-claim` to `0.2` and `nomograph-workflow` to `0.2`.
  Both crates dropped per-type validators from their public surface;
  validation responsibility now lives at the synthesist API boundary.
  The on-disk claim format is unchanged. Existing estates upgrade
  in place without migration.

### Added

- **`synthesist outcome add` and `synthesist outcome list`** — first-
  class CLI surface for the Outcome claim type. Records "what
  happened to a spec" (`completed`, `abandoned`, `deferred`,
  `superseded_by`) as a separate timestamped, asserter-attributed
  claim, distinct from Spec status (which expresses "what state the
  spec is in"). Closes the discoverability gap behind issue #6: the
  workflow that previously required disassembling the binary
  (`strings synthesist`) to find the right enum is now `synthesist
  outcome add k/spec --status completed --note "..."`.
- **`synthesist claims compact`** — physical compaction of the on-
  disk claim log. Re-encodes incremental `changes/*.amc` files into
  a single `snapshot.amc` under the substrate lock. Logical claim
  history is unchanged; the size benefit is the encoding difference
  (large estates have observed ~1300x working-tree shrink). Ships
  with `--dry-run` and `--yes` safety belts. Reference implementation
  and trial methodology by Josh Meekhof
  ([MR !8](https://gitlab.com/nomograph/synthesist/-/merge_requests/8),
  [issue #7](https://gitlab.com/nomograph/synthesist/-/issues/7));
  re-implemented here against the cohesive architectural pass.
- **`task update --depends-on`** — replace a task's dependency list
  with comma-separated IDs. Validates: no self-dependency, no
  cycles in the resulting DAG, every referenced ID exists in the
  same spec. New deps that are themselves in `cancelled` status
  surface as a JSON `warnings` field rather than blocking the
  update — the entire purpose of editing deps is to repair away from
  cancelled predecessors. Closes [issue #5](https://gitlab.com/nomograph/synthesist/-/issues/5).
- **`synthesist::schema` module** — single source of truth for every
  claim type synthesist owns (Tree, Spec, Task, Discovery, Campaign,
  Session, Phase, Outcome). Each type's enum constants are `pub
  const` slices referenced by both the validator and clap's
  `PossibleValuesParser`, so CLI-accepts-iff-schema-accepts is
  structural — drift between CLI and validator is no longer
  possible because there is only one definition.
- **`task_dag` module** — pure-function DAG operations (cycle
  detection, ready set, dependents-of, dep validation). Replaces the
  inline DFS walks that were scattered across command files. The
  same primitive serves `task ready`, `task update --depends-on`,
  and any future cross-task command (rename, split, reparent in v2.5).
- **`task_mutate` module** — unified supersession helper for task
  state transitions. Future task commands compose mutation closures
  over this helper rather than re-implementing the load-mutate-
  append pattern.
- **JSON `warnings: []` output convention** — soft warnings (e.g.
  depending on a cancelled task) surface as a structured field in
  the JSON output rather than going to stderr. Aligns with the
  documented all-output-is-JSON contract.
- **`scripts/check-symlinks.sh` + CI gate** — fails any commit that
  introduces an absolute-path symlink. Backstop against the
  `.agent/skills` recurrence class.
- **Agent-shape CI gate** via the new `agent-shape` component in
  `nomograph/pipeline@v2.6.0`. Every push runs `jig check
  --binary` to cross-reference `agent-shape.toml` against the
  built binary's `--help` surface, catching CLI drift between
  documented commands and what's actually shipped. Same drift
  class as the schema-CLI parity rule applied at the top-level
  command surface. `nomograph-jig` is now baked into the rust-cli
  image so consumers don't repeat the install on every run.
- **`CONTRIBUTING.md` back-compat policy** — three-layer policy
  (claim format strict, CLI additive within a major, library
  semver) committed to the repo.

### Fixed

- Validator errors now reach the user with structured detail
  (claim type, field name, actual value, expected enum set) instead
  of the opaque `validate claim before append`. The error chain in
  `synthesist`'s top-level handler walks every `source()` so the
  full diagnosis surfaces. The reporter on issue #6 had to run
  `strings` on the v2.1.1 binary to find the schema enums; that
  diagnostic cost should never have been necessary, and now isn't.
  See `nomograph_claim::SchemaError` for the structured variants;
  see `synthesist::schema` for how the consumer-side validator
  composes them.
- `spec update --status` rejects out-of-enum values at clap-parse
  time with a message naming the four schema-permitted values
  (`draft`, `active`, `done`, `superseded`). Issue #6's specific
  symptom (`completed` accepted by CLI, rejected by validator) is
  now a structural impossibility — both reference the same const.
- `.agent/skills` symlink is no longer git-tracked. The
  `make agent-skills-symlink` build step regenerates the relative
  form (`../.claude/skills`) on every build/install, so external
  tools that rewrite symlinks cannot commit a broken absolute
  target. The CI symlink gate enforces the rule.
- `.claude/skills/` is no longer git-tracked either, for the same
  drift class. Skills are materialized by `rune sync` from
  `nomograph/runes` (the source of truth); tracking the synced
  copy created a "rune sync" commit-class where the committed
  copy could drift between syncs. `.claude/rune.lock` (which
  pins versions) stays tracked. Aligns with the pattern rune
  itself uses.

### Changed

- The substrate crates (`nomograph-claim`, `nomograph-workflow`)
  are now type-agnostic for validation. Domain schemas live with
  their owners; substrate stores any well-formed claim regardless
  of `claim_type`. This unlocks future consumers (lattice when
  patent hold lifts; possibly others) defining their own claim
  types without coordinated substrate releases.
- `synthesist::SynthStore` now wraps `nomograph_workflow::Store`
  with a synthesist-side validating `append`. Existing call sites
  that did `store.append(...)` continue to work unchanged through
  inherent method resolution; read-only operations transparently
  delegate via `Deref`. The pre-flight validation runs on every
  append, with structured `SchemaError` propagating up the anyhow
  chain to the operator.

### Attribution

The performance analysis (`docs/perf-{baseline, hot-paths,
options-matrix, recommendation}.md`) and the compaction reference
implementation that drove this release came from Josh Meekhof. The
`claims compact` operator surface, the trial-on-a-copy methodology
that proved compaction safety, and the architectural framing that
distinguishes physical compaction from semantic GC are his work.
The deeper substrate-side performance items in his ranked
recommendation (instrumentation in `nomograph_claim::Store::open`
and `View::rebuild`; batched-write API at the workflow layer;
incremental view materialization with the correctness proof he
flagged as load-bearing) are designed and shipped together with
him in the v2.5 cycle.

## [2.3.0] (2026-04-28)

### Added

- `synthesist serve` polish for the standalone-viewer use case:
  humans landing on the dashboard cold (without an LLM session)
  can now see what's in flight at a glance.
  - "in flight" band at the top of the trees view lists the
    most-recently-active sessions with their tree/spec scope, the
    last claim's relative timestamp, claim type, and one-line
    summary. Sessions without any claims yet are excluded so the
    band only shows sessions that are actually doing something.
  - Tree summary rows roll up task status across all specs in the
    tree as colored pills (`done`, `in-flight`, `ready`, `gated`,
    `blocked`, `other`), so progress is visible without expanding.
    Spec summary rows show the same breakdown for that spec.
    `gated` distinguishes pending-but-human-gated tasks from
    pending-and-ready tasks -- both look like "pending" in raw
    status but only one is actionable for an agent.
  - Recent-activity rows now carry a tree tag inline next to the
    claim type, so cross-tree action is scannable without
    expanding the trees section.

### Changed

- Header mark renders the three dashed scales legibly. At the
  prior 22px display size with stroke-width 2.5 in a 64-unit
  viewBox, the dashed strokes rendered sub-pixel and only the
  curve was visible. Bumped display to 30px and stroke-width to
  3.5 with adjusted dasharrays so the full mark survives at small
  sizes in both light and dark themes.

## [2.2.0] (2026-04-26)

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

### Fixed

- `deny.toml` allow list now includes `CC0-1.0` so cargo-deny accepts
  the `notify` crate that powers the serve dashboard's filesystem
  watcher. Without this, the audit step rejected a valid public-domain
  dedication.
- `.agent/skills` symlink rewritten as relative (`../.claude/skills`)
  so it resolves correctly when the repo is cloned to any path. The
  prior absolute target broke the cargo publish step in CI.

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
