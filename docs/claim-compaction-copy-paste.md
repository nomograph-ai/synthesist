# Copy-paste: trial compaction + real checkpoint

Paths match a typical layout: **zd** at `$HOME/projects/github.com/zeel-dev/zd`, **synthesist** next to it under `nomograph/synthesist`. Change **`SYNTHESIST`** / **`ZD`** if yours differ.

---

## A — One block: build + trial (safe, copy only)

Deep-copies the zd **working tree** into **`~/zd-claims-compact-trial-<timestamp>`**, runs **`claims compact`** on that copy only, prints **before/after** `du` + file counts. **Your real zd tree is not modified.**

```bash
SYNTHESIST="$HOME/projects/gitlab.com/nomograph/synthesist"
ZD="$HOME/projects/github.com/zeel-dev/zd"

cd "$SYNTHESIST" && make build

chmod +x "$SYNTHESIST/scripts/claims-compact-trial.sh"
ZD_REPO="$ZD" "$SYNTHESIST/scripts/claims-compact-trial.sh"
```

**Full tree copy** (slower; includes `node_modules`, all `target/`, etc.):

```bash
SYNTHESIST="$HOME/projects/gitlab.com/nomograph/synthesist"
ZD="$HOME/projects/github.com/zeel-dev/zd"

cd "$SYNTHESIST" && make build
chmod +x "$SYNTHESIST/scripts/claims-compact-trial.sh"
FULL_COPY=1 ZD_REPO="$ZD" "$SYNTHESIST/scripts/claims-compact-trial.sh"
```

**Custom trial location:**

```bash
TRIAL_DIR="$HOME/tmp/zd-trial-manual" \
ZD_REPO="$HOME/projects/github.com/zeel-dev/zd" \
"$HOME/projects/gitlab.com/nomograph/synthesist/scripts/claims-compact-trial.sh"
```

When finished:

```bash
rm -rf "$HOME"/zd-claims-compact-trial-*
# or rm -rf the TRIAL_DIR path the script printed
```

---

## B — Real repo checkpoint (after you trust the trial)

Run on **zd** only when coordinated with your team.

```bash
SYNTHESIST="$HOME/projects/gitlab.com/nomograph/synthesist"
ZD="$HOME/projects/github.com/zeel-dev/zd"

cd "$SYNTHESIST" && make build

cd "$ZD"
git fetch origin && git switch main && git pull
git switch -c "chore/claims-compact-checkpoint-$(date +%Y%m%d)"

export SYNTHESIST_SESSION="checkpoint-$(hostname)"
export SYNTHESIST_DIR="$ZD"

"$SYNTHESIST/target/release/synthesist" \
  --session="$SYNTHESIST_SESSION" \
  --force \
  claims compact

SYNTHESIST_DIR="$ZD" "$SYNTHESIST/target/release/synthesist" check
```

**Track snapshot** (zd currently ignores it in `.gitignore` — either comment out `claims/snapshot.amc` **or** force-add):

```bash
cd "$ZD"
git add -f claims/snapshot.amc
git add -u claims/changes/
git add .gitignore   # only if you edited .gitignore for snapshot
git status
git commit -m "claims: checkpoint compact — track snapshot, prune superseded changes/*.amc"
git push -u origin HEAD
```

---

## See also

- Full narrative: [`claim-compaction-git.md`](claim-compaction-git.md)
- Script source: [`scripts/claims-compact-trial.sh`](../scripts/claims-compact-trial.sh)
