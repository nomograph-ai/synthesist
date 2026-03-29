# Changelog

All notable changes to Synthesist are documented here.

Versions represent architectural generations, not semver.

---

## [v5.0.0] -- 2026-03-29

Synthesist is now a Go binary with an embedded Dolt database. The
repository contains only the binary source, its tests, and documentation.
All legacy file-based scaffolding (specs/, prompts/, tools/, staging/,
.opencode/) has been removed.

### Architecture

Dolt embedded database replaces JSON files as the storage layer. LLM
agents interact exclusively through `synthesist` CLI commands. The binary
validates state transitions, handles temporal resolution, and auto-commits
to git. The Dolt database at `.synth/` is git-tracked and portable.

Why Dolt: git-native data diffing, branch/merge on data, content-addressed
storage. Why a binary: LLMs produce better results when constrained to
well-formed operations (Yegge, Beads 2026). A CLI decouples storage
format from the agent interface.

### Data model

Six node types connected by eight typed edges:

- **Task** -- unit of work with executable acceptance criteria and a DAG
- **Stakeholder** -- human actor, registered per-tree
- **Disposition** -- temporal assessment of a stakeholder's stance on a
  technical direction (what implementation choices they will accept)
- **Signal** -- immutable, bi-temporal evidence from stakeholder actions
- **Direction** -- upstream technical trajectory with impact links to specs
- **Pattern** -- named, reusable approach discovered through retrospectives

Retrospective nodes (type=retro) sit in the task DAG and carry labeled
transforms for cross-project replay.

### Commands

27 commands across 6 domains: estate, task DAG, landscape (stakeholders,
dispositions, signals), directions, retro + patterns, and queries
(landscape show, stance, replay). `synthesist skill` outputs the full
LLM behavioral contract.

### Kong migration

Command tree defined as Go structs with Kong struct tags. Typed flag
parsing replaces manual flag registration. The `synthesist skill` command
generates the LLM skill file from struct reflection -- the skill file is
always in sync with the actual command tree.

### Session infrastructure

Concurrent sessions built on Dolt branching. Each session gets its own
Dolt branch; merges reconcile data when sessions complete.

- `synthesist session start/merge/list/status/prune` commands
- `--session` flag and `SYNTHESIST_SESSION` environment variable
- Atomic task claim prevents TOCTOU race when multiple agents claim tasks
  concurrently across sessions

### Workflow state machine

7-phase LLM behavioral contract:
ORIENT -> PLAN -> AGREE -> EXECUTE <-> REFLECT -> REPORT (with REPLAN).

- `synthesist phase` command for phase declaration and validation
- Phase rules enforced: no task claims in PLAN, no task creation in
  EXECUTE, mandatory AGREE checkpoint between PLAN and EXECUTE
- Full behavioral contract (display rules, phase rules, error protocol)
  embedded in the skill file output
- Specification: [docs/state-machine.md](docs/state-machine.md)

### LLM-maintainability conventions

- Centralized error constructors in `cmd/synthesist/errors.go` -- all
  command errors use typed constructors, never inline `fmt.Errorf`
- Package-level README files explain each package's purpose and key types
- Golden tests in `tests/golden/` with `make golden-update` for regeneration
- golangci-lint replaces `go vet` -- errcheck, staticcheck, bodyclose
  enabled. Zero-warning policy enforced by `make lint`
- 400 LOC limit per file enforced by `make loc-check`

### File splitting

Large command files split into single-concern files (13 files from 3):

- `cmd_landscape.go` -> `cmd_landscape_show.go`, `cmd_disposition.go`,
  `cmd_signal.go`, `cmd_stakeholder.go`, `cmd_stance.go`
- `cmd_task.go` -> `cmd_task_create.go`, `cmd_task_lifecycle.go`,
  `cmd_task_list.go`, `cmd_task_query.go`, `cmd_task_helpers.go`
- `cmd_retro.go` -> `cmd_retro_create.go`, `cmd_replay.go`, `cmd_pattern.go`

### Quality

- 9 unit tests covering store layer (init, CRUD, dependencies,
  dispositions, bi-temporal signals, directions)
- Integration test exercising the full task lifecycle
- LLM tool use simulation: 4 parallel Sonnet instances evaluated the
  interface, found 15 issues, 10 fixed before release

### Removed (from repository)

- `specs/` -- templates, examples, SPEC_FORMAT.md, estate.json (all
  replaced by schema in the binary and `synthesist init`)
- `prompts/` -- framework.md and instance.md (replaced by `synthesist skill`)
- `.opencode/` -- OpenCode agent configs (tool is agent-agnostic now)
- `opencode.json` -- OpenCode configuration
- `tools/lint-specs.py` -- replaced by `synthesist check`
- `staging/` -- OpenCode chunked write staging

---

## Prior versions (v1-v4)

Synthesist v1-v4 was a file-based specification framework for OpenCode.
Agents read and wrote JSON files directly. The evolution:

- **v1** (2026-03-15): Spec format (spec.md + state.json), agent roles,
  executable acceptance criteria, cross-model review
- **v2** (2026-03-18): Single primary agent (replaced plan/build split),
  campaign coordination, concurrent session safety
- **v3** (2026-03-21): Context trees, estate.json meta-switchboard,
  campaign/archive mechanics, cross-references, integrity tooling
- **v4** (2026-03-27): Concurrent sessions via active_threads array,
  contributed upstream as MR !3

v5 is a ground-up rebuild that preserves the conceptual model (specs,
tasks, trees, campaigns) while replacing the implementation (files to
database, manual JSON to CLI, Python linter to Go binary).
