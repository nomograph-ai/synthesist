#!/usr/bin/env bash
# Release discipline for nomograph Rust CLI tools.
#
# Two modes:
#   release.sh audit [tool]          estate-wide or per-tool consistency audit
#   release.sh ship <tool> <x.y.z>   full release cycle (audit gate → ship)
#
# Audit is pure read, safe to run from anywhere. Ship must be run from
# the target tool's repo root.

set -euo pipefail

MODE="${1:-}"
shift || true

# ── Estate definition ──────────────────────────────────────────────────
#
# Single source of truth for the tool list. Update when adding or removing
# a nomograph tool from the kit registry.
#
# Format: tool_name:project_id:default_dir
ESTATE=(
  "rune:81028894:$HOME/gitlab.com/nomograph/rune"
  "muxr:80663080:$HOME/gitlab.com/nomograph/muxr"
  "kit:81066225:$HOME/gitlab.com/nomograph/kit"
  "synthesist:80084971:$HOME/gitlab.com/nomograph/synthesist"
)

ok()   { printf '  ✓ %s\n' "$*"; }
warn() { printf '  ~ %s\n' "$*"; }
bad()  { printf '  ✗ %s\n' "$*"; }
say()  { printf '\n── %s ──\n' "$*"; }
fail() { printf '\n✗ %s\n' "$*" >&2; exit 1; }

# ── Audit ──────────────────────────────────────────────────────────────

audit_tool() {
  local name="$1" pid="$2" dir="$3"
  local issues=0

  say "$name"

  if [[ ! -d "$dir" ]]; then
    warn "not cloned at $dir — skipping"
    return 0
  fi

  cd "$dir"

  local cargo_ver tag_cargo lock_ver binary_ver major
  cargo_ver=$(awk -F'"' '/^version = "/ {print $2; exit}' Cargo.toml)
  major=$(echo "$cargo_ver" | cut -d. -f1)

  # Filter tags to the current major line. Avoids legacy tags from prior
  # rewrites (e.g. synthesist's v5.x Go era) outranking the current v2.x.
  local latest_tag
  latest_tag=$(git tag --list --sort=-v:refname "v${major}.*" 2>/dev/null | head -1)
  if [[ -z "$latest_tag" ]]; then
    # Fall back to any tag if the major-filter returns nothing — handles
    # the unreleased-major case (just bumped to a new major, no tags yet).
    latest_tag=$(git tag --sort=-v:refname | head -1)
  fi
  if [[ -z "$latest_tag" ]]; then
    bad "no tags on $name — tool is unreleased"
    return 1
  fi

  tag_cargo=$(git show "$latest_tag:Cargo.toml" 2>/dev/null \
    | awk -F'"' '/^version = "/ {print $2; exit}')

  local crate_name
  crate_name=$(awk -F'"' '/^name = "/ {print $2; exit}' Cargo.toml)
  lock_ver=$(awk -v crate="$crate_name" '
    $0 ~ "name = \""crate"\"" { getline; print; exit }
  ' Cargo.lock | awk -F'"' '{print $2}')

  binary_ver=$("$HOME/.local/share/mise/shims/$name" --version 2>/dev/null \
    | awk '{print $NF}' | sed 's/^v//')

  printf '  Cargo.toml:   %s\n' "$cargo_ver"
  printf '  Cargo.lock:   %s\n' "$lock_ver"
  printf '  Tagged:       %s (%s)\n' "$tag_cargo" "$latest_tag"
  printf '  Binary:       %s\n' "${binary_ver:-?}"

  [[ "$cargo_ver" == "$lock_ver" ]] \
    || { bad "Cargo.toml ($cargo_ver) != Cargo.lock ($lock_ver)"; issues=$((issues+1)); }
  [[ "$cargo_ver" == "$tag_cargo" ]] \
    || warn "Cargo.toml ($cargo_ver) != tagged Cargo.toml ($tag_cargo) — WIP ahead of tag?"
  [[ -n "$binary_ver" && "$binary_ver" == "$tag_cargo" ]] \
    || warn "binary ($binary_ver) != tagged ($tag_cargo) — run kit sync to align"

  local tag_type
  tag_type=$(git cat-file -t "$latest_tag" 2>/dev/null)
  if [[ "$tag_type" == "tag" ]]; then
    ok "tag $latest_tag is annotated"
  else
    bad "tag $latest_tag is lightweight — delete and recreate with git tag -a"
    issues=$((issues+1))
  fi

  local ahead
  ahead=$(git rev-list "$latest_tag..HEAD" --count 2>/dev/null || echo 0)
  if [[ "$ahead" -eq 0 ]]; then
    ok "tag $latest_tag at HEAD"
  else
    warn "tag $latest_tag is $ahead commits behind HEAD (WIP expected)"
  fi

  if [[ -f .gitlab-ci.yml ]]; then
    local ci_comp
    ci_comp=$(grep -oE 'component:.*@v?[0-9.]+' .gitlab-ci.yml | head -1 | sed 's/.*@//')
    [[ -n "$ci_comp" ]] && ok "pipeline component at $ci_comp"
  fi

  if [[ -f deny.toml ]]; then
    ok "deny.toml present"
  else
    warn "deny.toml missing — cargo-audit gating may be disabled"
  fi

  local pkg_url pkg_http
  pkg_url="https://gitlab.com/api/v4/projects/$pid/packages/generic/$name/$latest_tag/$name-darwin-arm64"
  pkg_http=$(curl -sI -o /dev/null -w '%{http_code}' "$pkg_url" 2>/dev/null || echo "000")
  if [[ "$pkg_http" == "200" ]]; then
    ok "package registry has $latest_tag/darwin-arm64"
  else
    bad "package registry returned $pkg_http for $latest_tag — pipeline may not have published"
    issues=$((issues+1))
  fi

  return $issues
}

audit() {
  local target="${1:-}"
  local total_issues=0

  if [[ -n "$target" ]]; then
    local found=0
    for entry in "${ESTATE[@]}"; do
      IFS=: read -r name pid dir <<< "$entry"
      if [[ "$name" == "$target" ]]; then
        found=1
        audit_tool "$name" "$pid" "$dir" || total_issues=$((total_issues + $?))
        break
      fi
    done
    [[ $found -eq 1 ]] || fail "unknown tool '$target' — not in ESTATE"
    printf '\n'
    if [[ $total_issues -eq 0 ]]; then
      ok "$target clean"
    else
      bad "$target has $total_issues issue(s)"
      return 1
    fi
  else
    for entry in "${ESTATE[@]}"; do
      IFS=: read -r name pid dir <<< "$entry"
      audit_tool "$name" "$pid" "$dir" || total_issues=$((total_issues + $?))
    done
    printf '\n'
    if [[ $total_issues -eq 0 ]]; then
      ok "estate clean — no blockers across all tools"
    else
      bad "estate has $total_issues blocker(s) across all tools"
      return 1
    fi
  fi
}

# ── Ship ────────────────────────────────────────────────────────────────

ship() {
  local target="${1:-}" version="${2:-}"

  [[ -n "$target" && -n "$version" ]] \
    || fail "usage: release.sh ship <tool> <x.y.z>"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || fail "version must be semver x.y.z, got '$version'"

  local tag="v$version"
  local repo_root
  repo_root=$(pwd)
  local tool_name
  tool_name=$(basename "$repo_root")

  [[ "$tool_name" == "$target" ]] \
    || fail "shipping $target but cwd is $tool_name — cd to the correct tool repo"

  # --- Audit gate ---------------------------------------------------
  say "audit $target (gate before ship)"
  audit "$target" || fail "audit failed — fix above before shipping"

  # --- Pre-flight additional to audit -------------------------------
  say "pre-flight for $tag"

  git rev-parse --show-toplevel >/dev/null 2>&1 || fail "not a git repo"
  [[ -f Cargo.toml ]] || fail "no Cargo.toml in $(pwd)"

  local branch
  branch=$(git rev-parse --abbrev-ref HEAD)
  [[ "$branch" == "main" ]] \
    || fail "must ship from main, currently on '$branch'"

  local unrelated
  unrelated=$(git status --porcelain \
    | grep -vE '^\s*[AMD?]+\s+(Cargo\.(toml|lock)|CHANGELOG\.md)\s*$' || true)
  if [[ -n "$unrelated" ]]; then
    echo "$unrelated" >&2
    fail "unrelated uncommitted changes — stash or commit first"
  fi

  if git rev-parse --verify "refs/tags/$tag" >/dev/null 2>&1; then
    fail "$tag already exists locally"
  fi
  git fetch --tags --quiet
  if git ls-remote --tags origin "$tag" | grep -q "$tag"; then
    fail "$tag already on origin — use recovery path in SKILL.md"
  fi

  local cargo_ver
  cargo_ver=$(awk -F'"' '/^version = "/ {print $2; exit}' Cargo.toml)
  if [[ "$cargo_ver" != "$version" ]]; then
    sed -i.bak -E "s/^version = \"$cargo_ver\"/version = \"$version\"/" Cargo.toml
    rm -f Cargo.toml.bak
    ok "bumped Cargo.toml $cargo_ver -> $version"
  else
    ok "Cargo.toml already at $version"
  fi

  say "cargo build --release (refreshes Cargo.lock)"
  cargo build --release --quiet

  local crate_name lock_ver
  crate_name=$(awk -F'"' '/^name = "/ {print $2; exit}' Cargo.toml)
  lock_ver=$(awk -v crate="$crate_name" '
    $0 ~ "name = \""crate"\"" { getline; print; exit }
  ' Cargo.lock | awk -F'"' '{print $2}')
  [[ "$lock_ver" == "$version" ]] \
    || fail "Cargo.lock at $lock_ver after build (expected $version)"
  ok "Cargo.lock matches $version"

  say "cargo test --release"
  cargo test --release --quiet

  say "cargo clippy --all-targets -- -D warnings"
  cargo clippy --all-targets --quiet -- -D warnings 2>&1 | tail -20

  grep -qE "^## (v)?$version\b|\[v?$version\]" CHANGELOG.md 2>/dev/null \
    || fail "CHANGELOG.md has no entry for $version — add one and re-run"
  ok "CHANGELOG entry for $version present"

  # --- Commit + tag + push ------------------------------------------
  say "committing release"
  git add Cargo.toml Cargo.lock CHANGELOG.md
  if git diff --cached --quiet; then
    echo "note: no changes to commit"
  else
    git commit -m "release: $tag

AI-Assisted: yes
AI-Tools: Claude Code"
    ok "committed"
  fi

  say "tagging $tag (annotated)"
  git tag -a "$tag" -m "release $tag"

  say "pushing main + $tag"
  git push origin main
  git push origin "$tag"
  ok "pushed"

  # --- Watch CI -----------------------------------------------------
  local repo_path="nomograph/$target"
  local pid_encoded="nomograph%2F$target"

  say "waiting for $tag pipeline (up to 15 min)"
  sleep 15
  local status
  for i in $(seq 1 90); do
    status=$(glab -R "$repo_path" api \
      "projects/$pid_encoded/pipelines?ref=$tag" 2>/dev/null \
      | head -c 4000 | grep -oE '"status":"[^"]+"' | head -1 \
      | sed 's/.*"\([^"]*\)".*/\1/' || echo unknown)
    case "$status" in
      success) ok "$tag pipeline succeeded"; break ;;
      failed|canceled) fail "$tag pipeline $status — check glab -R $repo_path ci list" ;;
      *) printf '  (%s — %ds)\n' "$status" $((i * 10)); sleep 10 ;;
    esac
    [[ $i -eq 90 ]] && fail "timed out after 15 min"
  done

  # --- Merge kit auto-MR --------------------------------------------
  say "waiting for kit auto-MR"
  local mr_iid=""
  for i in $(seq 1 30); do
    mr_iid=$(glab -R nomograph/kits mr list --per-page 5 2>/dev/null \
      | awk '/^!/ { gsub(/^!/, "", $1); print $1; exit }')
    if [[ -n "$mr_iid" ]] && \
       glab -R nomograph/kits mr diff "$mr_iid" 2>/dev/null \
         | grep -q "tools/$target.toml"; then
      ok "found kit MR !$mr_iid"
      break
    fi
    printf '  (waiting — %ds)\n' $((i * 10))
    sleep 10
    [[ $i -eq 30 ]] && { echo "note: no kit MR after 5 min — tool may not have notify-kits"; mr_iid=""; }
  done

  if [[ -n "$mr_iid" ]]; then
    say "merging kit MR !$mr_iid"
    glab -R nomograph/kits mr merge "$mr_iid" --yes
    ok "merged"
  fi

  # --- Sync + verify ------------------------------------------------
  say "kit sync"
  kit sync --yes

  say "verify"
  local installed
  installed=$("$target" --version 2>/dev/null | head -1 | awk '{print $NF}')
  if [[ "$installed" == "$version" || "$installed" == "v$version" ]]; then
    ok "$target now $installed"
  else
    warn "\$PATH may be cached — new shells will see $version"
    printf '  current: %s\n' "$installed"
  fi

  printf '\n✓ release %s complete\n' "$tag"
}

# ── Dispatch ────────────────────────────────────────────────────────────

case "$MODE" in
  audit) audit "${1:-}" ;;
  ship)  ship "$@" ;;
  ""|help|-h|--help)
    cat <<'EOF'
release — nomograph Rust CLI release discipline

  release.sh audit [tool]        consistency audit (estate-wide or one tool)
  release.sh ship <tool> <ver>   full release cycle (audit → ship → monitor)

See SKILL.md for full details.
EOF
    ;;
  *) fail "unknown mode '$MODE' — expected 'audit' or 'ship'" ;;
esac
