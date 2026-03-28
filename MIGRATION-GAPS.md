# Migration Gaps -- Must Fix Before Keaton Migration

Identified 2026-03-28. All gaps must be closed before keaton
transition (TRANSITION.md in keaton repo).

## Critical (blocking migration)

### G1: No `tree create` command
Trees table has no INSERT path. 2 keaton trees cannot be created.
**Fix:** `synthesist tree create <name> --description "..." [--status active]`

### G2: No `thread create` command
Threads table has no INSERT path. 7 active threads cannot be migrated.
**Fix:** `synthesist thread create <tree> [--spec id] [--task id] --summary "..." [--date YYYY-MM-DD]`

### G3: No campaign commands
campaign_active, campaign_backlog, campaign_blocked_by tables have no
CLI write path. 10 active + 8 backlog specs cannot be migrated.
**Fix:**
- `synthesist campaign active <tree> <spec-id> --summary "..." [--blocked-by spec1,spec2]`
- `synthesist campaign backlog <tree> <spec-id> --title "..." --summary "..." [--blocked-by spec1,spec2]`

### G4: No archive command
archives table has no INSERT path.
**Fix:** `synthesist archive add <tree/spec> --reason completed --outcome "..." [--archived YYYY-MM-DD]`

### G5: task create always inserts status=pending
60+ already-done tasks cannot be migrated with correct status.
**Fix:** Add `--status done|pending|blocked|waiting|cancelled` flag to task create.

### G6: task done always runs verify commands
No way to mark a task done without executing acceptance criteria.
**Fix:** Add `--skip-verify` flag to task done.

### G7: No command to add acceptance criteria
acceptance table has no INSERT path. Every task's verify commands
cannot be set.
**Fix:** `synthesist task acceptance <tree/spec> <task-id> --criterion "..." --verify "cmd"`

## High (data quality)

### G8: No cancelled status
arxiv/t4 is cancelled. Status enum doesn't include it.
**Fix:** Add `cancelled` to status enum. Add `synthesist task cancel`.

### G9: task create auto-increments IDs, no --id flag
gkg-bench starts at t0. NextID starts at t1. Cannot preserve IDs.
**Fix:** Add `--id` flag to task create.

### G10: No historical dates
task create/done always use today(). Historical created/completed
dates lost.
**Fix:** Add `--created YYYY-MM-DD` and `--completed YYYY-MM-DD` flags.

### G12: No discovery content storage
discovery.md files have institutional memory with no database home.
**Fix:** Add `discovery` TEXT column to specs table, or a separate
discoveries table with timestamped entries.

## Low (lossy but workable)

### G11: task claim hardcodes owner
Owner field always "synthesist". Low impact (keaton data has null owners).

### G13: propagation_chain loses rich metadata
number_propagation has file/role/numbers fields not in schema.
Description field can approximate.

### G14: No spec created date override
spec create always uses today(). Low impact.

### G15: No spec context_path field
spec.md file linkage unrepresentable. Low impact since files are deleted.
