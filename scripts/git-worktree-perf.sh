#!/usr/bin/env bash
#
# Create an isolated git worktree for a performance experiment lane.
# Non-destructive: refuses if the target path already exists or slug is invalid.
#
# Usage: ./scripts/git-worktree-perf.sh <hypothesis-slug>
# Example: ./scripts/git-worktree-perf.sh batch-sync-view

set -euo pipefail

usage() {
  echo "Usage: ${0##*/} <hypothesis-slug>" >&2
  echo "  Creates ../synthesist-perf-<hypothesis-slug> with branch perf/<hypothesis-slug>." >&2
  exit 1
}

[[ $# -eq 1 ]] || usage

SLUG_RAW="$1"
if [[ ! "$SLUG_RAW" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]]; then
  echo "error: hypothesis slug must be kebab-case (lowercase letters, digits, hyphens): got '$SLUG_RAW'" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PARENT="$(cd "$REPO_ROOT/.." && pwd)"
WT_PATH="$PARENT/synthesist-perf-${SLUG_RAW}"
BRANCH_NAME="perf/${SLUG_RAW}"

if [[ -e "$WT_PATH" ]]; then
  echo "error: path already exists, refusing to overwrite: $WT_PATH" >&2
  exit 1
fi

cd "$REPO_ROOT"

if git show-ref --verify --quiet "refs/heads/${BRANCH_NAME}"; then
  echo "error: branch already exists locally: $BRANCH_NAME" >&2
  echo "  Remove the branch or pick a different slug." >&2
  exit 1
fi

git worktree add -b "$BRANCH_NAME" "$WT_PATH"

echo ""
echo "Worktree ready:"
echo "  Path:    $WT_PATH"
echo "  Branch:  $BRANCH_NAME"
echo ""
echo "Next steps:"
echo "  cd \"$WT_PATH\""
echo "  # Implement your experiment; then each iteration:"
echo "  make test && make lint"
echo "  # When benchmarking applies:"
echo "  cargo bench -p nomograph-synthesist -- <filter>"
echo ""
echo "Post results to https://gitlab.com/nomograph/synthesist/-/work_items/7 and/or:"
echo "  synthesist discovery add nomograph/crdt-storage-performance --finding \"...\" --impact medium"
echo ""
