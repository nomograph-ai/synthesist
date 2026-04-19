# Migrating from v1 to v2

This document walks operators through moving an existing synthesist
v1 project (SQLite at `.synth/main.db` with file-copy sessions) to
v2 (a shared CRDT claim log at `claims/`).

The migration is a one-shot, idempotent transform. Every v1 row lands
in the v2 log as one or more typed claims, original timestamps
preserved. Nothing is lost; nothing is overwritten in place.

Related reading:

- [`SYNC.md`](https://gitlab.com/nomograph/claim/-/blob/main/SYNC.md)
  for how the v2 log replicates across git and the optional beacon.
- [`IDENTITY.md`](https://gitlab.com/nomograph/claim/-/blob/main/IDENTITY.md)
  for the v0.1 asserter trust model.

## Before you migrate

Prerequisites:

- You are on a synthesist v2.0.0 or later binary. Check with
  `synthesist --version`. If you were running v1 yesterday, update
  the binary before anything else; the migration subcommand ships as
  part of v2.
- The project has a clean working tree, or all pending work is
  committed. The migrator writes new files and deletes nothing, but
  a clean state makes rollback trivial.
- You are in the project root, not inside `.synth/`.

Strongly recommended:

- `git commit` or stash any uncommitted changes.
- Take an external backup of `.synth/main.db` in addition to the
  auto-backup the migrator creates. `cp -R .synth .synth.backup` is
  fine.

## Step by step

### 1. Dry run

Always dry-run first. The migrator reads the full v1 database,
builds every v2 claim in memory, validates them, and prints a
summary without writing anything.

```bash
synthesist migrate v1-to-v2 \
    --from .synth/main.db \
    --to claims/ \
    --dry-run
```

Read the count summary carefully:

```text
would migrate:
  trees         3
  specs         12
  tasks         84   (48 done, 30 open, 6 cancelled)
  discoveries   17
  dispositions  4    (WARNING: moved to lattice; not migrated)
  phase         1    (scoped to session = "migrated-singleton")
```

Verify the counts match your expectations. If a number is
surprisingly low, abort and inspect the source database before
proceeding. If the migrator reports validation errors on any claim,
fix them in the source db first (see "If migration fails" below).

### 2. Real run

When the dry-run summary is correct, drop the flag:

```bash
synthesist migrate v1-to-v2 \
    --from .synth/main.db \
    --to claims/
```

The migrator:

1. Backs up `.synth/main.db` to `.synth/main.db.v1-backup-<unix_ts>`.
2. Creates `claims/` with `genesis.amc` and `config.toml`.
3. Appends every claim in dependency order (trees before specs,
   specs before tasks, etc.) so reads remain coherent mid-run.
4. Prints a final summary and exits 0 on success.

### 3. Post-migration checks

```bash
synthesist check
```

`check` revalidates every claim in the new log against the schema
and surfaces any orphaned references. It should exit 0.

Then:

```bash
synthesist status
```

The output should show your trees, spec counts, and open tasks.
Spot-check a single spec:

```bash
synthesist task list <tree>/<spec>
```

Task counts and statuses should match what you had in v1. Open
tasks remain open; done tasks remain done. Cancelled tasks stay in
the log but are filtered by default.

### 4. Commit

```bash
git add claims/
git commit -m "migrate synthesist to v2"
```

Do not delete `.synth/main.db` from the tree yet. Leave it in place
for one working day in case of surprises.

## What is preserved

- `created_at` on every v1 row becomes `asserted_at` and
  `valid_from` on the equivalent v2 claim.
- All tasks, including done and cancelled, land in the log. Status
  is a field on the Task claim; read-side filters decide what to
  surface.
- Task acceptance criteria come across as a nested array on the
  Task claim, not separate rows.
- Disposition supersession chains are preserved as v2 supersession
  links. Note that disposition claims themselves live in `lattice`
  in v2; if you used dispositions heavily, see the lattice README
  for its own import path.
- The v1 `phase` row, if present, becomes a single Phase claim
  scoped to `session_id = "migrated-singleton"`. You can discard it
  once you start a real v2 session.

## What changes

- **Sessions are now attribution labels, not file copies.** In v1,
  `session start` cloned the database; merging reconciled changes.
  In v2, `--session=<id>` tags every claim so origin is recoverable,
  but the underlying log is shared. CRDT handles concurrent writes.
- **Landscape commands have moved to `lattice`.** The v2 synthesist
  binary prints a pointer when you invoke `stakeholder`,
  `disposition`, `signal`, `topic`, `stance`, or `landscape`. See
  the [lattice README](https://gitlab.com/nomograph/lattice) for
  install and usage.
- **`session merge` and `session discard` are gone.** Use
  `session close` instead. Close supersedes the open Session claim
  non-destructively; the log preserves everything.
- **Phase is per-session.** Every v2 command that reads or writes
  phase takes `--session=<id>` (or reads `SYNTHESIST_SESSION` from
  the environment). Concurrent sessions can be in different phases.

## If migration fails mid-way

The migrator is transactional at the claim level: either every
claim is appended or none are. A failure mid-run means `claims/`
was rolled back and the v1 database is untouched.

1. The v1 db is backed up at `.synth/main.db.v1-backup-<unix_ts>`.
   Nothing was lost.
2. Examine stderr for the specific claim that failed validation.
   The error names the field, for example:

   ```text
   error: Task claim rejected at row 47:
          acceptance_criteria item 2 missing required "text" field
   ```

3. Fix the source database (usually a stray row with a null on a
   required column):

   ```bash
   sqlite3 .synth/main.db
   ```

4. Retry the migration. If `claims/` exists from a previous
   partial attempt, pass `--overwrite`:

   ```bash
   synthesist migrate v1-to-v2 --from .synth/main.db --to claims/ --overwrite
   ```

## Rollback

If you need to abandon v2 entirely and resume work on v1:

```bash
# 1. Drop the staged claims directory
git checkout -- .
rm -rf claims/

# 2. Restore the v1 database from the auto-backup if needed
cp .synth/main.db.v1-backup-<unix_ts> .synth/main.db

# 3. Downgrade to the last v1 release
mise install http:synthesist@1.x.y   # or your install path
```

File an issue if rollback was necessary. The migrator should
handle every v1 shape cleanly, and the fact that yours did not is
worth investigating.

## Verification

A thorough operator verifies three invariants before calling the
migration done:

1. `synthesist status` in v2 shows the same trees and open task
   counts you had in v1.
2. `synthesist task list <tree>/<spec>` on any spec shows the same
   task set (including done tasks) you had in v1.
3. If you saved `synthesist check` output from v1, run `check`
   again in v2 and diff. Discrepancies should only be in phase
   scoping (singleton → per-session) and location of observation
   data (gone from synthesist; now in lattice).

## FAQ

**Q: Can I run migrate v1-to-v2 twice?**
Yes. It is idempotent. A second run on an already-migrated repo
detects the existing `claims/` log and exits with a no-op message.
Pass `--overwrite` to force a rebuild, which is only useful after a
partial failure.

**Q: Does this change my git history?**
No. The migrator writes new files under `claims/` and leaves
`.synth/` in place. Whether you commit the result in one commit or
many is up to you.

**Q: What happens to my dispositions and signals?**
They are skipped by the synthesist migrator with a warning. Install
`lattice` and follow its import path if you want them in the new
log.

**Q: My project has multiple v1 databases (one per session). How do
I migrate?**
Pick the canonical one (usually `main.db`) and migrate it. The
session-copy model does not round-trip to v2; merge your v1
sessions first with v1 tooling, then migrate the result.

**Q: Can I keep running v1 and v2 in parallel during the cutover?**
Not reliably. The two use different on-disk formats and do not
share a log. Treat the migration as a point-in-time cutover: pick a
quiet moment, migrate, commit, move on.
