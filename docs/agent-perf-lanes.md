# Agent performance experiment lanes

Practical playbook for running **isolated git worktrees** (one lane per hypothesis) while iterating on CRDT-safe storage performance work tracked under the spec **`nomograph/crdt-storage-performance`**. Tracking issue: **[GitLab work item #7](https://gitlab.com/nomograph/synthesist/-/work_items/7)**.

## Why lanes

Parallel agents (or sequential experiments) should not share one working tree: merge conflicts and accidental `claims/` state bleed obscure results. A **lane** is a dedicated worktree + branch pair so each hypothesis stays reproducible and reviewable.

## Worktree workflow

From the synthesist repo root (`nomograph/synthesist`):

1. **Create a lane** — use the helper script (recommended):

   ```bash
   ./scripts/git-worktree-perf.sh <hypothesis-slug>
   ```

   Or manually:

   ```bash
   git fetch origin main
   git worktree add -b perf/<hypothesis-slug> ../synthesist-perf-<hypothesis-slug> origin/main
   ```

2. **Work only inside that worktree** for the experiment (`cd` path printed by the script).

3. **Iterate:** implement smallest diff → measure → verify (below) → record results.

4. **Land or discard:** open an MR from `perf/<hypothesis-slug>`, or remove the worktree when done:

   ```bash
   git worktree remove ../synthesist-perf-<hypothesis-slug>
   git branch -d perf/<hypothesis-slug>   # after merge or if abandoned
   ```

### Naming conventions

| Piece | Convention | Example |
|-------|------------|---------|
| Worktree directory | `../synthesist-perf-<hypothesis-slug>` next to the repo | `../synthesist-perf-batch-sync-view` |
| Branch | `perf/<hypothesis-slug>` off current tracking branch | `perf/batch-sync-view` |
| Hypothesis slug | Lowercase `kebab-case`, ASCII letters/digits/hyphens only | `incremental-sql-delta` |

Use one slug per hypothesis; if you retry the same idea later, append a short disambiguator (e.g. `batch-sync-view-v2`).

## Verification checklist (every iteration)

Non-negotiable: **CRDT semantics preserved** — no rewriting claim history, no bypassing merge correctness for speed.

Run from the **lane worktree** root:

1. **`make test`** — full test suite must pass.
2. **`make lint`** — zero Clippy warnings (`-D warnings`).
3. **Benchmarks when relevant** — after a bench target exists for your change (see spec tasks / `Cargo.toml` benches):

   ```bash
   cargo bench -p nomograph-synthesist -- <filter>
   ```

   Use a stable machine where possible; note CPU model and load in your posted results.

4. **Behavioral sanity** (if touching merge, view sync, or supersession): exercise representative fixtures; use `synthesist conflicts` where applicable — see project docs and `synthesist skill`.

## Where to post results

Pick one or both:

- **GitLab** — comment on **[work item #7](https://gitlab.com/nomograph/synthesist/-/work_items/7)** with: hypothesis, commit SHA, wall-clock or bench numbers, hardware note, and whether `make test && make lint` passed. Best for discoverability and discussion.
- **Synthesist discovery** — institutional memory tied to the spec:

  ```bash
  export SYNTHESIST_SESSION=<your-session>
  synthesist discovery add nomograph/crdt-storage-performance \
    --finding "<concise result>" \
    --impact low|medium|high
  ```

Use discovery for durable spec-linked summaries; use GitLab for raw numbers, graphs, and MR links.

## Related

- Helper script: [`scripts/git-worktree-perf.sh`](../scripts/git-worktree-perf.sh)
- Cursor skill (overview): [`.cursor/skills/synthesist-perf-experiments/SKILL.md`](../.cursor/skills/synthesist-perf-experiments/SKILL.md)
