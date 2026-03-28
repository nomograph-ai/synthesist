# Changelog

All notable changes to Synthesist are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/). Versions
are not semver -- they represent architectural generations of the framework.

---

## [v5] -- 2026-03-28 (in progress)

### Architecture Decision: Dolt embedded storage

Synthesist v5 replaces direct JSON file manipulation with a Dolt embedded
database as the primary store, managed by the `synth` CLI binary.

**The problem**: v1-v4 stored all state as JSON files that LLM agents
read and wrote directly. This worked for task DAGs but broke down when
we added temporal stakeholder intelligence (dispositions with validity
windows, signal chains, cross-tree pattern queries). Temporal queries
across flat JSON files require loading every file and reconstructing
relationships in memory. LLMs writing raw JSON are trusted to produce
valid state transitions with no enforcement.

**The decision**: Embed Dolt (git-for-data SQL database) in a Go binary
(`synth`). The Dolt database lives inside the project repository,
tracked by git, portable between machines via push/pull. LLM agents
interact exclusively through `synth` CLI commands -- they never touch
data files directly. The binary validates all state transitions,
handles temporal resolution, and manages git commits.

**Why Dolt over SQLite**: Dolt provides git-native diffing on data
(`synth diff` shows table-level changes between commits), branch/merge
on data, and content-addressed storage (unchanged data is stored once
across versions). SQLite would require a separate JSON projection layer
for git tracking -- Dolt eliminates this by being both the database and
the version-controlled artifact.

**Why Dolt over TerminusDB**: TerminusDB is graph-native and has better
graph traversal, but it requires running a server. We need an embedded
database that compiles into a single binary.

**Why a binary at all**: LLMs produce better results when constrained
to well-formed operations (Yegge, Beads 2026). A CLI with typed
commands prevents invalid state transitions, handles computation LLMs
are bad at (temporal resolution, graph traversal, date math), and
provides a stable API that decouples the storage format from the
agent interface.

**Tradeoff accepted**: Human-readable `git diff` on spec data is lost.
`synth diff` replaces it with richer table-level diffs. Given 99% LLM
usage, this is the right tradeoff. `synth status --human` provides
human-readable views on demand.

### Added

- **Temporal specification graph model**: five node types (task,
  stakeholder, disposition, signal, pattern) with eight typed edge types
- **Stakeholder intelligence layer**: dispositions with temporal validity
  windows model what implementation choices a stakeholder will accept.
  `preferred_approach` captures the technical direction they favor.
- **Signal tracking**: immutable, timestamped evidence from stakeholder
  actions (PR comments, reviews, meetings). Evidence chain for
  disposition assessments.
- **Retrospective nodes**: task DAG nodes (type=retro) created at spec
  completion. Carry `arc` (narrative), `transforms` (labeled moves with
  transferability flags), and pattern references. Enable replay: "play
  back this sub-tree from project A onto project B."
- **Pattern registry**: named, reusable approaches per tree. Retro nodes
  reference patterns; patterns track where they've been observed.
  Queryable: "show me all transferable patterns."
- **`synth` CLI binary** (Go + embedded Dolt): single binary for all
  spec graph operations. Replaces direct JSON manipulation, Python
  linter, and manual git commits. Ships with `synth skill` command
  that outputs the LLM behavioral contract.
- **Waiting status**: tasks can be `waiting` with a `waiter` object
  containing a machine-checkable resolution command. For external
  blockers (MRs under review, issues awaiting response).
- **Archive enrichment**: `duration_days`, `patterns`, `contributions`
  fields on archived specs for queryable retrospective data.

### Changed

- **Storage**: JSON files managed by LLM agents -> Dolt embedded
  database managed by `synth` binary
- **Task DAG**: gains `type` field (task vs retro), `created`/`completed`
  timestamps, `waiter` object for waiting status
- **Status enum**: `pending | in_progress | done | blocked` ->
  adds `waiting`
- **Write path**: agents call `synth` commands, not `Write(state.json)`
- **Validation**: Python linter (lint-specs.py) -> `synth check`
- **Git commits**: manual by agent -> automatic by `synth` (configurable)

### Removed

- Direct JSON file manipulation by LLM agents
- `tools/lint-specs.py` (replaced by `synth check`)

---

## [v4] -- 2026-03-27

### Added

- **Concurrent session support**: `active_threads` array in estate.json
  replaces single-slot `last_session`. Each workstream gets its own
  thread entry keyed by `{tree}/{spec}`. Sessions update only their
  own thread, eliminating cross-session overwrites.
- Thread fields: `id`, `tree`, `spec`, `task`, `date`, `summary`
- Pruning rule: threads older than 7 days with no active spec/task
- Contributed upstream to nomograph/synthesist as MR !3

### Changed

- estate.json version 3 -> 4
- Session entry/exit protocol updated for active_threads

### Fixed

- Cross-session overwrites: running 4+ concurrent sessions caused 7
  estate.json overwrites in one day under v3's single-slot design

---

## [v3] -- 2026-03-21

### Added

- **Context trees**: hierarchical directories under specs/ that scope
  agent context to one domain at a time
- **estate.json**: meta-switchboard listing all trees and session state
- **campaign.json**: per-tree active + backlog spec tracking
- **archive.json**: per-tree archived spec records with reasons
  (completed, abandoned, superseded, deferred)
- **Cross-references**: typed refs between specs (`blocked_by`,
  `informs`, `discovered_from`) with cross-tree resolution
- **Sub-trees**: nested tree directories with their own campaign.json
- **Integrity tooling**: `lint-specs.py` validates estate structure,
  cross-references, and state consistency

### Changed

- Flat spec directory -> tree hierarchy
- Single campaign -> per-tree campaigns with archive mechanics

---

## [v2] -- 2026-03-18

### Added

- **Single primary agent**: replaces plan/build split. One agent handles
  the full loop (discuss, draft, iterate, codify, build, verify).
  Trust boundary at human agreement, not tool restrictions.
- **Campaign coordination**: cross-spec dependency tracking with
  active/backlog horizons
- **Concurrent session safety**: task ownership via `owner` field,
  aggressive commits, deconfliction rules
- **Session handoff**: `last_session` in estate for continuity

### Changed

- Two-agent architecture (plan + build) -> single primary agent
- Rationale: handoff between agents lost context and required
  copy-paste. 16+ sessions of real work proved single-agent strictly
  better.

---

## [v1] -- 2026-03-15

### Added

- **Spec format**: `spec.md` (Markdown with YAML frontmatter and XML
  sections) + `state.json` (JSON task DAG with executable acceptance
  criteria)
- **Agent roles**: primary, @explore, @edit, @review, @verify
- **Workflow**: Discuss -> Draft -> Iterate -> Codify -> Build -> Verify
  with human gates
- **Executable acceptance criteria**: every task has a `verify` shell
  command. The @verify agent runs them independently.
- **Cross-model review**: @review uses a different model family than
  primary to catch different failure modes
- **Quality scoring**: review entries with scores and findings

### Design influences

Symphony (OpenAI), BMAD Method, GSD, Gastown (Yegge), Ralph,
Metaswarm. See README for details.
