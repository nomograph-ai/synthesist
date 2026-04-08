# Synthesist v1.0.0 Architecture

**Date**: 2026-04-07
**Status**: Accepted

## Motivation

Rewrite from Go + embedded Dolt to Rust + SQLite. Stabilize the schema
for a v1.0.0 release.

- CGo + ICU build chain replaced by rusqlite (bundled, C only, no system deps)
- 30 tables reduced to 16; several v5 features lacked empirical validation
- Schema stability: v1.0.0 commits to the data model

The paper "Context Asymmetry Is a Representation Problem" (Dunn, 2026,
draft) describes the theoretical foundation. Tool and paper co-evolve.

## Design Principles

### 1. Distinct concerns get distinct structures

Orthogonal simple structures outperform unified complex schemas. Each
concern gets its own table. We do not overload tables to avoid creating
new ones.

*MAGMA (Jiang et al., 2026); Hindsight (Vectorize, 2025); PlugMem
(Microsoft, 2026).*

### 2. Abstracted knowledge over raw traces

Distilled knowledge units outperform raw episodic memory. Discoveries
are findings with interpretation, not transcripts. Dispositions are
assessed stances with confidence, not signal collections.

*PlugMem (Microsoft, 2026); Learning to Commit (Li et al., 2026).*

### 3. Scope preferences explicitly

Agents treat unscoped preferences as globally enforceable rules, causing
misapplication. Dispositions must be scoped to topics.

*BenchPreS (Yoon et al., 2026).*

## Storage

### SQLite (journal_mode=DELETE)

- DuckDB rejected: single-writer file lock, no cross-DB transactions in
  ATTACH, no valid row-hash syntax for merge
- SQLite WAL rejected: WAL checkpoint requires quiescing all readers,
  incompatible with frequent git commits
- DELETE mode: writes directly to .db file, clean git commits, short-lived
  connections serialize acceptably

### Per-Session Database Copies

```
synthesist/
  main.db            # canonical, git-tracked
  sessions/          # gitignored
    factory-01.db    # copy at branch time + snapshot tables
```

Each session works on its own file. Zero contention. SQLite ATTACH supports
cross-database transactions (unlike DuckDB), enabling atomic merge.

Alternatives rejected:
- Git worktrees: duplicate entire repo per session
- Schema-level sessions: pollutes every query with session filters
- Shared DB: loses the AGREE-phase review gate

### Three-Way Merge

Session databases include `_snapshot_<table>` tables capturing base state.
At merge: diff session vs snapshot (session changes), diff main vs snapshot
(concurrent changes), auto-merge non-conflicting column changes, flag
conflicts.

Supersession model follows Kumiho (2026): immutable revisions with mutable
pointers, satisfying AGM belief revision postulates.

### Git Integration

- `synthesist/` directory (no dot prefix, visible, full name)
- `main.db` tracked, `sessions/` gitignored
- Never VACUUM between commits (defeats delta compression)
- Network sync via git push/pull only (SQLite over NFS corrupts)

### LLM Containment

Binary format prevents direct LLM access. All reads and writes go through
the CLI. `synthesist sql` provides a sanctioned query escape hatch.

## Cutline

### Tier 1: Ships in v1.0.0

| Feature | Rationale |
|---------|-----------|
| Task DAG (specs, tasks, deps, files, acceptance) | Core product |
| Sessions (per-file + three-way merge) | Concurrent agent isolation |
| Phase enforcement (7-phase state machine) | Paper Algorithm 1; AGREE gate enforces human approval |
| Discoveries | Institutional memory; used in ORIENT and REPORT phases |
| Disposition graph (stakeholders, dispositions, signals) | Paper core contribution; temporal supersession with confidence tiers |
| Campaigns (cross-repo spec grouping) | Cross-tree coordination cannot be derived from task state |

### Tier 2: Deferred

Each feature has a re-entry criterion. Ships as a point release with
schema migration when the criterion is met.

| Feature | Why deferred | Re-entry criterion |
|---------|-------------|-------------------|
| Influences | No evidence agents query role-to-task mappings | Simulation showing role queries change agent behavior beyond disposition queries |
| Directions | Potentially redundant with dispositions | Scenario where project-level trajectory is unrecoverable from stakeholder dispositions |
| Patterns | Literature says conventions belong in context files, not the DB | Validated discovery-to-context-file promotion workflow |
| Retros/transforms | Unclear if structured retro adds value over spec outcome + discoveries | Case where retro entity provides unreproducible information |
| Propagation chains | Staleness detection mechanism underspecified | Specified algorithm with evidence of catching real staleness |
| Archives | Spec status field sufficient | Need for structured metadata beyond status + reason |
| Waiters (structured) | Over-specified for common case | Automated external dependency polling implemented |
| Quality/validations | Unimplemented in v5 | Defined review workflow |

## Schema (16 tables)

**Estate**: trees

**Specs**: specs (with status, outcome fields)

**Task DAG**: tasks (with wait_reason), task_deps, task_files, acceptance

**Discoveries**: discoveries

**Disposition graph**: stakeholders, stakeholder_orgs, dispositions, signals

**Campaigns**: campaign_active, campaign_backlog, campaign_blocked_by

**Sessions**: session_meta

**Workflow**: phase

**Config**: config

## CLI Conventions

- Data directory: `synthesist/` (visible, full name)
- Creation verb: `add` everywhere
- Path format: `tree/spec` everywhere
- `--session` shown in all write command signatures
- ~40 command paths (down from ~55)

## Skill Emission

Generalized across execution systems (Claude Code, Cursor, IDE extensions):

- Data model explanation (Estate > Trees > Specs > Tasks)
- Command examples, not just flag signatures
- Error case documentation
- Phase enforcement table (paper Table 1)
- No tool-specific assumptions

## Migration (v5 to v1.0.0)

1. Merge all active Dolt sessions
2. `synthesist export` (v5 Go) produces JSON
3. Disposable migration tool transforms v5 JSON to v1.0.0 schema
4. `synthesist import` (v1.0.0 Rust) loads into SQLite

Schema version tracked in `config` table. Future migrations are
forward-only via `synthesist migrate`.

## References

- Jiang et al. (2026). MAGMA. arXiv:2601.03236.
- Park (2026). Kumiho. arXiv:2603.17244.
- Vectorize (2025). Hindsight. arXiv:2512.12818.
- Microsoft (2026). PlugMem. arXiv:2603.03296.
- Yoon et al. (2026). BenchPreS. arXiv:2603.16557.
- Li et al. (2026). Learning to Commit. arXiv:2603.26664.
- Vasilopoulos (2026). Codified Context. arXiv:2602.20478.
- Dunn (2026). Context Asymmetry. Draft.
