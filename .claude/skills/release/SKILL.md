---
name: release
description: Release discipline for nomograph Rust CLI tools (synthesist, rune, kit, muxr, dkg, gapvec, glean). Two modes — `audit` runs estate-wide or per-tool consistency checks (version alignment, Cargo.lock, tag quality, pipeline component, package registry, release artifacts). `ship` runs the full release cycle (pre-flight audit → bump → commit → tag → push → watch CI → merge kit auto-MR → kit sync → verify). Use whenever cutting a new version of a nomograph tool OR running a hygiene check across the estate. Catches the Cargo.lock-out-of-sync class of errors that otherwise fail the pipeline.
allowed-tools: Bash(cargo *) Bash(cargo) Bash(git *) Bash(git) Bash(glab *) Bash(glab) Bash(kit *) Bash(kit) Bash(./release.sh *) Bash(./release.sh) Bash(curl *) Bash(curl) Bash(jq *) Bash(jq)
---

# Release

Discipline encoded as a script. Covers the full lifecycle — hygiene check,
release execution, post-release propagation through the nomograph ecosystem.

## When to use

- User says "release <tool> v<x.y.z>", "ship <tool>", "tag a new version".
- User says "preflight", "check the estate", "what's drifted in nomograph".
- Before and after any release that touched multiple tools.
- When `<tool> --version` reports something unexpected.

## Tools covered

Seven nomograph Rust CLI tools today. The script reads the list from config
(the `ESTATE` array in `release.sh`) — keep that authoritative, don't edit
the list here.

| Tool | Repo |
|------|------|
| rune | nomograph/rune |
| muxr | nomograph/muxr |
| kit | nomograph/kit |
| synthesist | nomograph/synthesist |
| dkg | (private) |
| gapvec | (private) |
| glean | (private) |

If a tool isn't cloned locally, audit skips it with a warning.

## Two modes

### `release.sh audit [tool]`

Consistency check across the estate. Pure read, no changes.

Per tool, verifies ALL of:

1. **Version chain**: `Cargo.toml` == `Cargo.lock` (`nomograph-<tool>` entry) == tagged `Cargo.toml` == `<tool> --version` binary output == kit/mise-installed version.
2. **Tag quality**: latest tag is annotated (not lightweight), at HEAD (or gap is explicit).
3. **Pipeline component version** referenced in `.gitlab-ci.yml`.
4. **Supply chain**: `deny.toml` present.
5. **Package registry**: HEAD request for `darwin-arm64` binary returns 200.

Any failure is a blocker. Output is a per-tool section with checkmarks and
a summary at the end.

Run modes:
- `release.sh audit` — all tools in the estate.
- `release.sh audit <tool>` — single tool (e.g. `release.sh audit synthesist`).

### `release.sh ship <tool> <x.y.z>`

Full release cycle. Runs `audit <tool>` first as a hard gate. Aborts
immediately if audit fails.

Then:

1. **Pre-flight** (additional checks beyond audit):
   - Working tree is clean except for Cargo.toml / Cargo.lock / CHANGELOG.md.
   - On main branch.
   - Target tag `v<x.y.z>` not already taken (local or remote).
   - `Cargo.toml` version matches target (auto-bumps if not).
   - After bump, `cargo build --release` succeeds (regenerates Cargo.lock).
   - `Cargo.lock` now reflects target version.
   - `cargo test --release` passes.
   - `cargo clippy --all-targets -- -D warnings` passes.
   - `CHANGELOG.md` has an entry for this version.

2. **Commit**: Cargo.toml + Cargo.lock + CHANGELOG.md as a single
   `release: v<x.y.z>` commit.

3. **Tag** annotated `v<x.y.z>` at the release commit.

4. **Push** main + tag.

5. **Watch CI**: poll the tag pipeline until success. Abort on failure;
   the release isn't shipped until CI passes.

6. **Merge the kit auto-MR** when it opens (usually within a minute of
   the tag pipeline's notify-kits job finishing).

7. **`kit sync --yes`** to install the new version on this machine.

8. **Verify** `<tool> --version` reports the new version.

## Usage

```
cd ~/gitlab.com/nomograph/<tool>
./release.sh audit              # audit whole estate
./release.sh audit synthesist   # audit one tool
./release.sh ship synthesist 2.1.2
```

The skill is available in nomograph tool repos that subscribe to it via
`.claude/rune.toml`:

```toml
[skills]
release = "nomograph/runes"
```

## If audit fails

| Finding | Fix |
|---|---|
| `Cargo.lock` != `Cargo.toml` | `cargo build --release` then commit |
| tagged `Cargo.toml` != latest tag | Tag predates version bump — delete and retag |
| Binary `--version` mismatch | `kit sync --yes`, or `mise uninstall` + `mise install` |
| Mise config stale | `kit sync --yes` |
| Lightweight tag | Delete and recreate with `git tag -a` |
| Stale pipeline component | Update `.gitlab-ci.yml`, commit |
| `deny.toml` missing | Create from template (breaks cargo-audit gating) |
| Package registry 404 | Pipeline didn't publish — check `glab -R nomograph/<tool> ci list` |

## If ship fails mid-flow

The script hard-fails at the first issue to avoid half-landings:

- **Pre-flight failure** → nothing committed, fix and re-run.
- **Commit succeeded, tag failed** → re-run, tag step is idempotent.
- **Tag pushed, CI failed** → see recovery below.
- **CI passed, kit MR didn't open** → check nomograph/kits; if absent, tool may not have notify-kits configured.
- **Kit MR merged, install failed** → manual `kit sync --yes` from a fresh shell; PATH caching is common.

## Tag-rewrite recovery

```
glab -R nomograph/<tool> api --method DELETE \
  "projects/nomograph%2F<tool>/repository/tags/v<x.y.z>"
# fix the issue, then:
git tag -d v<x.y.z>
git tag -a v<x.y.z> -m "release v<x.y.z>"
git push origin main
git push origin v<x.y.z>
```

## Version philosophy

- Standard semver — patch for fixes, minor for features, major for breaking changes.
- Don't cut a version for WIP — `cargo install --path .` for local iteration.
- Budget: one milestone release per tool per week unless a critical fix forces a patch.

## What this skill won't do

- Author CHANGELOG entries (human judgement).
- Pick the version number (semver judgement on what changed).
- Recover from a tag whose artifacts are already in the package registry — bump and tag a successor.
- Release non-nomograph projects — for general GitLab/GitHub releases, use the `release` skill in `andrewdunndev/arcana`.
