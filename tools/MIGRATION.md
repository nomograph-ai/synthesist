# Migrating to synthesist v1.0.0

## Pre-flight

Before migrating, ensure:

1. All active synthesist sessions are merged or discarded
2. No harnesses are actively writing to the database
3. The v5 binary is still in PATH (for export)
4. The v1.0.0 binary is built or installed

## Migration prompt

Copy this prompt to each harness. It handles all edge cases.

---

I need to migrate this project from synthesist v5 (Go+Dolt) to v1.0.0
(Rust+SQLite). Follow these steps exactly. Stop and report if any step
fails.

### Step 1: Verify preconditions

```bash
# Confirm v5 database exists
ls .synth/synthesist/.dolt/ || echo "ERROR: no v5 database found"

# Confirm no active sessions
synthesist session list
# Must show 0 sessions or all sessions merged/discarded.
# If active sessions exist, merge or discard them first:
#   synthesist session merge <id>
#   synthesist session discard <id>
```

### Step 2: Export v5 data

```bash
synthesist export > v5-export.json
# Verify the file is valid JSON (v5 may print warnings to stdout)
python3 -c "import json; d=json.load(open('v5-export.json')); print(f'OK: {len(d)} tables')" 2>&1
# If the above fails with JSON error, strip the warning:
#   sed -i '' '1,/^{/{ /^{/!d; }' v5-export.json
```

### Step 3: Run migration

```bash
python3 ~/gitlab.com/nomograph/synthesist/tools/migrate-v5-to-v1.py v5-export.json
# This writes v1-import.json. Review the summary output:
# - tasks, specs, discoveries should match v5 counts
# - retro tasks become discoveries (count increases)
# - dropped tables are logged (directions, patterns, etc.)
```

### Step 4: Back up and remove v5 data

```bash
# Safety backup (the v5-export.json IS the backup, but belt and suspenders)
cp -r .synth .synth-backup-v5

# Remove v5 database
rm -rf .synth
```

### Step 5: Initialize v1.0.0 and import

```bash
# This must use the v1.0.0 binary, not the v5 binary.
# Verify: synthesist version should show v1.0.0
synthesist version

# Initialize
synthesist init

# Import (FK checks disabled during import, re-enabled after)
synthesist --session=migration --force import v1-import.json

# Verify integrity
synthesist check
```

### Step 6: Verify data

```bash
synthesist status
# Compare task_counts with v5 export. Should match except:
# - Retro tasks are gone (became discoveries)
# - Archives are gone (mapped to spec status fields)

synthesist sql "SELECT COUNT(*) as tasks FROM tasks"
synthesist sql "SELECT COUNT(*) as discoveries FROM discoveries"
synthesist sql "SELECT COUNT(*) as stakeholders FROM stakeholders"
```

### Step 7: Update project files

Update CLAUDE.md (or AGENTS.md) in this project:
- Change `.synth/` references to `synthesist/`
- Change "Dolt database" to "SQLite database"
- Change "Never read or write data files in .synth/" to
  "Never read or write files in synthesist/"

### Step 8: Commit

```bash
git add synthesist/
git rm -r .synth/ 2>/dev/null || true
git add CLAUDE.md AGENTS.md 2>/dev/null || true
git commit -m "migrate to synthesist v1.0.0 (Rust+SQLite)"
```

### Step 9: Clean up

```bash
# Remove backup after verifying everything works
rm -rf .synth-backup-v5
rm v5-export.json v1-import.json
```

---

## Edge cases

**"JSON decode error on export"**: The v5 binary sometimes prints lock
warnings to stdout. The migration script strips these automatically
(looks for first `{`). If it still fails, manually clean:
`sed -i '' '1,/^{/{ /^{/!d; }' v5-export.json`

**"FOREIGN KEY constraint failed on import"**: Fixed in the v1.0.0
import command (FK checks disabled during import). If you see this,
ensure you're using the v1.0.0 binary, not v5.

**"session 'migration' already exists"**: The import session name
collides with a previous failed attempt. Either:
- `rm -rf synthesist/` and re-run from step 5, or
- Use a different session name: `--session=migration2`

**"table X has no column named Y"**: The migration script strips
v5-only columns. If you see this, the migration script may be outdated.
Pull the latest from the synthesist repo.

**Active sessions at migration time**: Merge or discard ALL sessions
before exporting. Session data on Dolt branches is not included in
`synthesist export` -- only main branch data is exported.

**Multiple projects with .synth/**: Run the migration independently in
each project directory. Each .synth/ is an independent database.
