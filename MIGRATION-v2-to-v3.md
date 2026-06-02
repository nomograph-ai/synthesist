# Migrating from v2 to v3

This document walks operators through moving an existing synthesist
v2 project (`.amc` claim files under `claims/changes/` and
`claims/snapshot.amc`) to v3 (per-asserter JSON-LD logs at
`claims/<asserter>/log.jsonl`).

v3 is v3-native: there is no dual-write. After migration you have a
v3-only synthesist -- writes go to the per-asserter JSON-LD logs and
reads go through a disposable redb gamma index (`claims/_view.gamma`)
rebuilt from those logs. The v2 `.amc` write path is gone;
`claims/changes/*.amc` stop being written. A minimal v2-read shim in
`nomograph-claim` remains, used only so this migration can read your
old `.amc` estate.

The migration is a one-shot transform. Every v2 claim lands in the
v3 log with original timestamps, asserter attribution, and supersession
links preserved. A tarball of the v2 state is written before any files
change so rollback is always available.

Related reading:

- `CHANGELOG.md` for a full list of what changed in 3.0.0-pre.1.
- `MIGRATION.md` for the earlier v1-to-v2 playbook if you need it.

## Pre-flight

### 1. Confirm your current binary version

```sh
synthesist --version
```

You must be on `synthesist 2.5.1` or later. If the command is not on
`$PATH`, install it before proceeding. Versions before 2.5.1 do not
produce the claim shapes the migrator expects.

If you have the standalone `claim-migrate` binary from v2.5 on your
`$PATH`, uninstall or rename it now. It is not needed and its
presence can cause confusion once the v3 binary is installed.

### 2. Estimate storage requirements

v3 JSON-LD logs are dramatically smaller than v2 `.amc` files. The
storr corpus (143 claims) measured 35.1 MB v2 versus 100.9 KB v3
(348x). Typical ratios are 200-400x.

```sh
du -sb claims/
```

Divide the result by 300 for a conservative v3 estimate. In practice
you will need less space for the new logs than the current store uses,
so available disk is rarely a concern. The migration also writes a
`.synthesist-v2-backup.tar.gz` of the source directory; allow for
one additional copy of the current `claims/` size for the tarball.

### 3. Commit any pending claim writes

The migrator tarballs the current state of the claims directory as
its first action. A clean working tree makes any post-migration git
diff readable and ensures the tarball matches what you have.

```sh
git status claims/
```

If there are uncommitted files, commit or stash them:

```sh
git add claims/
git commit -m "wip: clean up before v3 migration"
```

## Install v3

Install synthesist 3.0.0-pre.1:

```sh
cargo install --git https://gitlab.com/nomograph/synthesist --tag v3.0.0-pre.1
```

macOS ARM and Linux ARM64 are supported. (2.5.2 shipped Linux ARM64
on the v2 line; pre.1 carries that platform support forward.)

After installation, confirm the version:

```sh
synthesist --version
# synthesist 3.0.0-pre.1
```

## Dry run

Run the migration in dry-run mode first. The migrator reads every v2
claim, builds the v3 JSON-LD documents in memory, and prints a summary
without writing anything.

```sh
synthesist migrate v2-to-v3 --dry-run
```

The output looks like:

```text
dry run -- no files written

would migrate:
  tasks         107
  phases         16
  specs           6
  campaigns       4
  discoveries     4
  sessions        4
  trees           2
  total         143

estimated v3 log size: ~101 KB  (v2 store: ~35 MB)
backup would write: .synthesist-v2-backup.tar.gz
schema file would write: claims/_schema.json
```

Check that the counts match your expectations. If a type is missing
or a count is far from what you expect, do not proceed; inspect the
v2 log with `synthesist claims show` or the `claim` debug binary.

If the dry run reports `UnsupportedClaimType` for any claim, see the
"Lattice claims" section at the end of this document before
proceeding.

## Real migration

Drop `--dry-run` to run the migration:

```sh
synthesist migrate v2-to-v3
```

The migrator:

1. Writes `.synthesist-v2-backup.tar.gz` in the project root. This
   tarball contains the full `claims/` directory as it stood before
   any v3 files were written.
2. Writes per-asserter JSON-LD logs to `claims/<asserter>/log.jsonl`.
   Each asserter that appears in the v2 store gets its own log file.
3. Writes `claims/_schema.json` with the store schema version.
4. Prints a final summary and exits 0.

The migration does not delete the v2 `.amc` files; it only reads
them. After migration the directory still contains the old
`claims/changes/` alongside the new `claims/<asserter>/` logs. The
v3 runtime ignores the `.amc` files entirely -- they are retained
only so the tarball-free rollback path below stays available. Once
you are confident in the v3 estate you can delete `claims/changes/`
(and `claims/snapshot.amc`); the gamma index never reads them.

Commit the per-asserter `claims/<asserter>/log.jsonl` logs and
`claims/_schema.json` to git. Do NOT commit `claims/_view.gamma`:
it is a disposable, gitignored redb index the runtime rebuilds from
the logs on demand.

## Verify

### Check claim counts

```sh
synthesist status
```

The claim counts and tree structure should match what you had before
the migration.

### Check the schema file

```sh
cat claims/_schema.json
```

You should see:

```json
{"schema_version": "3.0.0-pre.1", ...}
```

If the file is missing or shows a different version, the migration
did not complete. Restore from the tarball (see "Rollback" below)
and file an issue.

### Check migration status

```sh
synthesist migrate status
```

The command reports the current store schema version. A fully
migrated store reads "store is at 3.0.0-pre.1" (or equivalent
wording from the binary).

### Optional: inspect a v3 log directly

```sh
head -n 1 claims/<asserter>/log.jsonl | jq .
```

Each line is a self-contained JSON-LD document. You should see the
inline `@context` with `synthesist`, `nomograph`, `prov`, and `xsd`
prefix bindings, followed by the claim fields in lowerCamelCase.

## Going forward

Once migrated, every write command (`synthesist task add`, `spec add`,
`session start`, and the rest) appends a single v3 JSON-LD log line to
the writing asserter's `claims/<asserter>/log.jsonl`. There is no
`.amc` write. Reads (`status`, `task ready`, `check`, `conflicts`,
overlays) run as typed queries against the gamma index, which the
runtime rebuilds from the logs whenever their heads have moved.

The relevant on-disk surfaces after migration:

- `claims/<asserter>/log.jsonl` -- v3 JSON-LD logs. The multi-user
  source of truth. Commit these to git.
- `claims/_schema.json` -- schema version record. Commit it.
- `claims/_view.gamma` -- disposable redb gamma index. Gitignored;
  safe to delete at any time (costs only a one-time rebuild).
- `claims/changes/`, `claims/snapshot.amc` -- leftover v2 `.amc`
  files. Read by nothing in v3; kept only for rollback. Delete once
  you no longer need the in-place rollback path.

If you encounter problems before you have deleted the v2 files, the
v2 store is still there to fall back to (see "Rollback" below). File
issues at <https://gitlab.com/nomograph/synthesist/-/issues> with a
description of what broke.

## Rollback

The v2-to-v3 migration is forward-only; it does not modify or delete
v2 files in place. If you need to return to a pure v2 state:

### 1. Restore the v2 tarball

```sh
tar -xzf .synthesist-v2-backup.tar.gz
```

This restores the `claims/` directory to the state it was in before
the migration. The v3 log files added during and after the migration
are overwritten.

### 2. Remove any v3-only files

```sh
rm -f claims/_schema.json
rm -rf claims/*/log.jsonl   # removes per-asserter v3 logs
rm -f claims/_view.gamma    # removes the disposable gamma index
```

### 3. Reinstall the latest v2 line

```sh
cargo install --git https://gitlab.com/nomograph/synthesist --tag v2.5.2
```

### 4. Retain the tarball

Keep `.synthesist-v2-backup.tar.gz` for at least 90 days before
deleting it. The pre.1 cycle is intentionally returnable. The tarball
is the only copy of the v2 state once you have deleted or overwritten
files, so do not discard it prematurely.

## Lattice claims

If your v2 store contains claims with lattice-named types
(`Stakeholder`, `Topic`, `Signal`, `Disposition`, `Intent`,
`Heartbeat`, `Directive`), the migration aborts before writing any
files with:

```text
error: UnsupportedClaimType: <type> is not in the v3 synthesist
surface; lattice claim types are not migrated by this command
```

Lattice is not part of the pre.1 public surface. The migration does
not attempt to route these claims, and there is no automated path
for them in the current release. If you have a store with lattice
claims, contact the nomograph team before migrating.
