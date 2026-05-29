# Migrating from v2 to v3

This document walks operators through moving an existing synthesist
v2 project (`.amc` claim files under `claims/changes/` and
`claims/snapshot.amc`) to v3 (per-asserter JSON-LD logs at
`claims/<asserter>/log.jsonl`).

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

macOS ARM is the supported target for pre.1. Other platforms may
work but are not tested in the pre-release cycle.

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

The v2 `.amc` files are not deleted. After migration the directory
contains both `claims/changes/` (v2) and `claims/<asserter>/` (v3)
side by side. Both can be committed to git; they will coexist for
the duration of the pre.1 dual-write cycle.

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
`session start`, and the rest) produces both a v2 `.amc` claim and a
v3 JSON-LD log line. v2 remains the source of truth for the pre.1
cycle; v3 is dual-written to validate the thesis.

The two directories coexist:

- `claims/changes/` -- v2 `.amc` claim files (write source of truth
  for pre.1).
- `claims/<asserter>/log.jsonl` -- v3 JSON-LD log (dual-written, will
  become the sole write path at 3.0.0 final).

Both can be committed to git. There is no harm in having both present;
the v3 binary reads either surface depending on the operation.

The v2 write path drops at 3.0.0 final. If you encounter problems
during the pre.1 cycle, the v2 store is always there to fall back to
(see "Rollback" below). File issues at
<https://gitlab.com/nomograph/synthesist/-/issues> with a description
of what broke.

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
```

### 3. Reinstall v2.5.1

```sh
cargo install --git https://gitlab.com/nomograph/synthesist --tag v2.5.1
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
