# Claim log compaction + Git checkpoint (operator runbook)

For estates where **`claims/changes/*.amc`** are **plain Git**, write-once files and the **working tree** is huge (multi‑GB, thousands of files), **`Store::compact()`** folds incrementals into **`claims/snapshot.amc`** and deletes superseded **`changes/*.amc`** on disk — **same logical Automerge document**.

This doc describes a **checkpoint workflow**: track **`snapshot.amc`** so clones can rebuild state **without** materializing every historical `.amc` at HEAD.

**Quick copy-paste** (trial copy + real checkpoint): **[`claim-compaction-copy-paste.md`](claim-compaction-copy-paste.md)**

## Prerequisites

- **Built synthesist** with `claims compact` (this repo: `make build` → `./synthesist` or `target/release/synthesist`).
- Shell env for the estate:
  - **`SYNTHESIST_DIR`** = directory **containing** `claims/` (e.g. zd repo root), **or**
  - **`--data-dir`** same path on each command.

## 1. Backup and branch

```bash
cd /path/to/zd   # parent of claims/

git fetch origin
git switch main
git pull
git switch -c chore/claims-compact-checkpoint-$(date +%Y%m%d)
```

Coordinate with your team: **merge open MRs** that touch `claims/` before checkpoint if possible.

## 2. Stop concurrent writes

Compaction holds **`claims/.lock`**. Avoid running **`synthesist`** writes elsewhere against the same repo path until finished.

## 3. Run compaction (physical layout)

Uses session + **`--force`** so workflow phase does not block maintenance:

```bash
export SYNTHESIST_SESSION="checkpoint-$(hostname)"
export SYNTHESIST_DIR=/path/to/zd

/path/to/synthesist/target/release/synthesist \
  --session="$SYNTHESIST_SESSION" \
  --force \
  claims compact
```

Expect JSON like `{"ok":true,"claims_root":"..."}`.

This creates / refreshes **`claims/snapshot.amc`** and removes **`changes/*.amc`** files that are superseded by that snapshot (see **nomograph-claim**).

Validate read path:

```bash
SYNTHESIST_DIR=/path/to/zd ./target/release/synthesist check
# optional:
SYNTHESIST_DIR=/path/to/zd ./target/release/synthesist status
```

## 4. Track `snapshot.amc` in Git

Your `.gitignore` may list **`claims/snapshot.amc`** (local-cache policy). For checkpoints **either**:

**A)** Remove or comment out that line in `.gitignore` **for this repo**, **or**

**B)** Force-add once:

```bash
git add -f claims/snapshot.amc
```

## 5. Stage deletion of absorbed incrementals + ignore tweak

```bash
git add .gitignore   # if you edited it
git add -u claims/changes/
git status
```

You should see **`claims/snapshot.amc` added** (or modified) and **many deletions** under **`claims/changes/`**.

## 6. Commit as one atomic checkpoint

```bash
git commit -m "claims: checkpoint compact — track snapshot, prune superseded changes/*.amc"
```

**Rule:** never commit **only** deletions under **`changes/`** without **`snapshot.amc`** (or equivalent) present in the same commit — fresh clones must open **genesis + snapshot + remaining changes**.

## 7. Push and open MR

```bash
git push -u origin HEAD
```

## Git object database vs working tree

`git count-objects` staying **~180 MiB pack** while **`claims/changes`** was **~7 GiB** means **history packs** are smaller than **full checkout** of all blobs. Checkpoints **shrink future working trees at HEAD**; **old `.amc` blobs remain in Git history** until (optionally) `git filter-repo` — out of scope here.

## Cadence

Checkpoint **when file count or ops pain warrants**, not on every local `compact()`. Many teams use **monthly** or **when `changes/` exceeds N files**.

## Rollback

Before push: `git reset --hard` and restore working tree from backup branch.

After push: revert MR or restore `snapshot` + `changes/` from prior commit (coordinate with team).
