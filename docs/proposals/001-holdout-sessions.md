# Proposal 001: Holdout Scenarios via Session Separation

**Status**: Draft
**Date**: 2026-04-08
**Author**: Andrew Dunn

## Problem

Agents that write both the implementation and the tests can reward-hack:
optimize for passing the test rather than genuine correctness. StrongDM
documented this concretely -- agents would write `return true` to satisfy
narrowly scoped tests [1].

The broader problem: when the same agent session has visibility into
acceptance criteria, holdout scenarios, and implementation artifacts,
there's no information barrier preventing the agent from gaming the
verification rather than solving the problem.

## Proposed Architecture

Use synthesist's existing session model to create information barriers
between agent roles. Each session is an isolated database copy -- extend
this from concurrency isolation to honesty isolation.

### Three Session Roles

**spec-author** (interactive shift)
- Sees: dispositions, signals, discoveries, prior specs, estate state
- Writes: specs, tasks, acceptance criteria, holdout scenarios
- Cannot see: implementation artifacts
- Phase: ORIENT, PLAN, AGREE
- Human reviews and approves in AGREE phase

**implementer** (non-interactive shift)
- Sees: specs, task summaries, task dependencies
- Cannot see: holdout scenarios, acceptance verify commands
- Writes: claims tasks, marks done (without running acceptance criteria)
- Phase: EXECUTE, REFLECT

**validator** (adversarial)
- Sees: holdout scenarios, running artifact (as a service, not source)
- Cannot see: implementation source, spec intent details
- Runs: holdout scenarios against the built artifact
- Reports: satisfaction score per scenario
- Failures become: signals that feed back into dispositions

### Information Barriers

Current sessions see the full schema. This proposal requires scoped
visibility -- a session that can read `tasks.summary` but not
`acceptance.verify_cmd`, or can read specs but not implementation
file paths.

Options for implementing scoped visibility:

**Option A: Schema-level separation.** Holdout scenarios live in a
separate table (`holdout_scenarios`) that is excluded from implementer
session snapshots. At session start, the snapshot creation skips
tables the role shouldn't see. Simple but coarse-grained.

**Option B: Column-level filtering.** Implementer sessions get snapshots
that omit specific columns (e.g., `acceptance.verify_cmd` is nulled
in the implementer's copy). Finer control but more complex snapshot
logic.

**Option C: Convention-enforced via skill file.** The implementer's
skill file simply doesn't document holdout commands. The agent doesn't
know they exist. No schema enforcement -- relies on the LLM not
discovering commands it wasn't told about. Weakest barrier but
zero implementation cost.

**Option D: Separate databases.** Holdout scenarios live in a separate
database file entirely, only accessible to the validator session.
Strongest barrier. Most operational complexity.

### Workflow

```
1. Human starts spec-author session
2. spec-author: ORIENT -> PLAN -> creates specs, tasks, acceptance criteria, holdout scenarios
3. Human: AGREE -> approves the plan
4. System: creates implementer session (snapshot excludes holdout data)
5. Implementer: EXECUTE -> claims tasks, writes code, marks done
6. System: creates validator session (snapshot includes only holdout scenarios)
7. Validator: runs scenarios against built artifact, reports satisfaction
8. If satisfaction < threshold: failures become signals -> dispositions -> next cycle
9. If satisfaction >= threshold: merge implementer session to main
```

### Schema Changes Required

New table for holdout scenarios (distinct from acceptance criteria):

```sql
CREATE TABLE IF NOT EXISTS holdout_scenarios (
    tree      TEXT NOT NULL,
    spec      TEXT NOT NULL,
    id        TEXT NOT NULL,
    narrative TEXT NOT NULL,    -- natural language scenario description
    category  TEXT NOT NULL,    -- happy_path, edge_case, failure_mode
    verify    TEXT,             -- optional: executable check command
    PRIMARY KEY (tree, spec, id)
);
```

New column on `session_meta`:

```sql
ALTER TABLE session_meta ADD COLUMN role TEXT;
-- values: spec_author, implementer, validator, null (legacy/unrestricted)
```

Session start gains `--role` flag:

```
synthesist session start impl-01 --role implementer
```

### Migration

This is a v1.1.0 schema change. The migration mechanism (schema_version
detection + ordered migrations) is in place. The migration adds the
`holdout_scenarios` table and the `role` column to `session_meta`.

### Open Questions

1. **How strict should the information barrier be?** Option A (table-level)
   vs Option C (convention) vs Option D (separate files). The right answer
   may depend on how sophisticated the agents become at circumventing
   soft barriers.

2. **Should the validator see the spec intent?** Woolley's model says no --
   the validator should evaluate purely against holdout scenarios without
   knowing the spec's reasoning. This prevents the validator from being
   "sympathetic" to the implementation. But it also means the validator
   can't contextualize failures.

3. **How do holdout scenarios differ from acceptance criteria?** Acceptance
   criteria are visible to the implementer and checked during `task done`.
   Holdout scenarios are hidden and checked externally. The implementer
   knows *what* must be true (acceptance) but not *how it will be tested*
   (holdout). This is the train/test split.

4. **Satisfaction scoring.** Woolley uses probabilistic satisfaction (97/100
   runs pass). We currently use boolean pass/fail. For non-deterministic
   agentic output, probabilistic may be more appropriate. How does this
   interact with the existing acceptance criteria model?

5. **Where does the validator run?** It needs access to the built artifact
   as a running service, not as source code. This implies a CI/CD
   integration that synthesist doesn't currently have.

## References

[1] StrongDM AI Team, "The Attractor Pattern," 2026.

[2] C. Woolley, "The Software Factory: A Practitioner's Guide," Feb. 2026.

[3] S. Willison, "Visiting StrongDM," Oct. 2025.
