#!/usr/bin/env bash
# Deep-copy a repo's working tree (default: zd), run physical compaction on the COPY only,
# print before/after sizes. Safe to experiment — does not modify the source tree.
#
# Usage:
#   cd /path/to/synthesist
#   ./scripts/claims-compact-trial.sh
#
# Override paths:
#   ZD_REPO=/var/home/you/projects/github.com/zeel-dev/zd \
#   TRIAL_DIR=/var/home/you/tmp/my-zd-trial \
#   ./scripts/claims-compact-trial.sh
#
# Full bitwise copy (no rsync excludes — slower, includes node_modules etc.):
#   FULL_COPY=1 ./scripts/claims-compact-trial.sh

set -euo pipefail

SYNTHESIST_REPO="${SYNTHESIST_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
SYNTHESIST_BIN="${SYNTHESIST_BIN:-$SYNTHESIST_REPO/target/release/synthesist}"

ZD_REPO="${ZD_REPO:-$HOME/projects/github.com/zeel-dev/zd}"
TRIAL_DIR="${TRIAL_DIR:-$HOME/zd-claims-compact-trial-$(date +%Y%m%d-%H%M%S)}"
SESSION="${SYNTHESIST_SESSION:-trial-$(hostname)-$$}"

if [[ ! -x "$SYNTHESIST_BIN" ]]; then
  echo "error: missing executable: $SYNTHESIST_BIN"
  echo "  fix: cd $SYNTHESIST_REPO && make build"
  exit 1
fi

if [[ ! -f "$ZD_REPO/claims/genesis.amc" ]]; then
  echo "error: ZD_REPO must point at the repo root that contains claims/genesis.amc"
  echo "  got ZD_REPO=$ZD_REPO"
  exit 1
fi

echo "== Source (unchanged) =="
echo "ZD_REPO=$ZD_REPO"
du -sh "$ZD_REPO/claims/changes" 2>/dev/null || true
find "$ZD_REPO/claims/changes" -name '*.amc' 2>/dev/null | wc -l | xargs echo "source change files (.amc):"

echo ""
echo "== Trial directory (new deep copy) =="
echo "TRIAL_DIR=$TRIAL_DIR"
mkdir -p "$TRIAL_DIR"

if [[ "${FULL_COPY:-0}" == "1" ]]; then
  echo "FULL_COPY=1 — rsync everything (may take a long time)."
  rsync -a "$ZD_REPO/" "$TRIAL_DIR/"
else
  echo "rsync with excludes (node_modules, .next, target, …). FULL_COPY=1 for full tree."
  rsync -a \
    --exclude=node_modules \
    --exclude=.pnpm-store \
    --exclude=.next \
    --exclude=dist \
    --exclude='**/target' \
    "$ZD_REPO/" "$TRIAL_DIR/"
fi

echo ""
echo "== Before compaction (trial copy only) =="
du -sh "$TRIAL_DIR/claims/changes" 2>/dev/null || true
find "$TRIAL_DIR/claims/changes" -name '*.amc' 2>/dev/null | wc -l | xargs echo "trial change files (.amc):"

echo ""
echo "== Running: synthesist claims compact (trial only) =="
SYNTHESIST_DIR="$TRIAL_DIR" SYNTHESIST_SESSION="$SESSION" \
  "$SYNTHESIST_BIN" --session="$SESSION" --force claims compact

echo ""
echo "== Verify store (trial) =="
SYNTHESIST_DIR="$TRIAL_DIR" "$SYNTHESIST_BIN" check

echo ""
echo "== After compaction (trial) =="
du -sh "$TRIAL_DIR/claims/changes" 2>/dev/null || true
if [[ -f "$TRIAL_DIR/claims/snapshot.amc" ]]; then
  du -sh "$TRIAL_DIR/claims/snapshot.amc"
else
  echo "(no claims/snapshot.amc — unexpected)"
fi
find "$TRIAL_DIR/claims/changes" -name '*.amc' 2>/dev/null | wc -l | xargs echo "trial change files (.amc):"

echo ""
echo "== Git view in trial (checkpoint dry-run) =="
if [[ -d "$TRIAL_DIR/.git" ]]; then
  (cd "$TRIAL_DIR" && git status --short claims/ 2>/dev/null | head -50) || true
  echo ""
  echo "To practice a checkpoint commit in the trial (optional):"
  echo "  cd \"$TRIAL_DIR\""
  echo "  # Uncomment snapshot in .gitignore or: git add -f claims/snapshot.amc"
  echo "  git add -u claims/changes/ && git add -f claims/snapshot.amc && git status"
else
  echo "(no .git — rsync may have missed it; use FULL_COPY=1 if you need git metadata in trial)"
fi

echo ""
echo "== Done =="
echo "Trial copy (safe to delete):"
echo "  rm -rf \"$TRIAL_DIR\""
echo "Source repo was not modified."
