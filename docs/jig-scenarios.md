# Jig Scenario Format

**Status**: Draft
**Date**: 2026-05-28
**Companion to**: `docs/proposals/002-implementation-plan.md` (T7.2)

## Purpose

A jig scenario is a self-contained description of a synthesist workflow task that
`synthesist jig run` can attempt under a given surface manifest. Each scenario
specifies:

- A starting state (fixture commands or a fixture data path)
- A goal expressed as plain-language criteria
- A structured scoring rubric that yields a numeric score (0-100)
- The expected artifacts that should exist at completion

Scenarios are authored in TOML. The jig runner loads a scenario file, sets up the
starting state, dispatches the session to an LLM-driven operator, then scores the
result using the rubric.

---

## File location

Scenarios live under `jig/scenarios/<name>.toml` in the synthesist repo. The
`<name>` is the canonical scenario identifier used with
`synthesist jig run --scenario <name>`.

---

## Top-level fields

### `[scenario]`

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Short kebab-case identifier matching the filename stem. |
| `description` | string | yes | One-sentence summary of what the scenario tests. |
| `version` | string | yes | Semver string. Increment when the rubric or starting state changes in a way that makes scores non-comparable with previous runs. |
| `tags` | array of strings | no | Freeform labels for grouping runs (e.g., `["planning", "phase-1"]`). |

---

### `[starting_state]`

Describes how to set up a fresh synthesist workspace before the operator session
begins. Exactly one of `fixture_path` or `setup_commands` must be present.

| Field | Type | Required | Description |
|---|---|---|---|
| `fixture_path` | string | one-of | Path relative to repo root for a pre-built claims fixture directory. The jig copies this into a temp workspace before running. |
| `setup_commands` | array of strings | one-of | An ordered list of `synthesist ...` CLI commands the jig runner executes (via shell) to construct the starting state. Each command is run in sequence; failure aborts setup. |
| `description` | string | yes | Human-readable summary of what the starting state represents. |

When `setup_commands` is used, the jig runner initializes a fresh synthesist
workspace (`synthesist init`) before executing the commands.

---

### `[goal]`

Describes what a correct session outcome looks like. This section is provided
verbatim to the LLM operator in the session prompt.

| Field | Type | Required | Description |
|---|---|---|---|
| `prompt` | string | yes | The full prompt given to the LLM operator. Should describe the task, any constraints, and the expected deliverable. Use TOML multi-line string syntax (`"""`). |
| `success_criterion` | string | yes | One sentence naming the observable state that constitutes success. Used by human reviewers and automated checks. |

---

### `[[rubric]]`

An array of weighted scoring criteria. Each entry is one criterion. Scores across
all entries are combined into a total score of 0-100.

Each criterion:

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Short slug identifying this criterion. Unique within the scenario. |
| `description` | string | yes | Plain-language statement of what is being measured. |
| `weight` | integer | yes | Relative weight. The scorer normalizes all weights to sum to 100 points. Must be a positive integer. |
| `check` | string | yes | How this criterion is evaluated. One of: `"manual"` (human reviewer assigns 0-100 per criterion), `"cli"` (the jig runner executes a command and checks exit code and output pattern), or `"artifact"` (the jig checks that a file or claim exists and matches a pattern). |
| `check_command` | string | no | Required when `check = "cli"`. The `synthesist ...` command to run. Exit 0 = pass (full weight); nonzero = fail (0). |
| `check_pattern` | string | no | Optional regex applied to stdout when `check = "cli"`. If present, both exit 0 AND pattern match are required for full weight. |
| `check_artifact` | string | no | Required when `check = "artifact"`. A glob relative to the workspace root. At least one match = pass. |
| `partial_credit` | bool | no | Default `false`. When `true` and `check = "manual"`, the reviewer assigns a partial score 0-100 for this criterion rather than 0 or full weight. |

**Score computation**: for each criterion, the raw score is 0-100 (0 = fail,
100 = full pass, or intermediate when `partial_credit = true`). The weighted
score is `(raw / 100) * weight`. The total scenario score is
`sum(weighted scores) / sum(weights) * 100`, yielding a final 0-100.

---

### `[expected_artifacts]`

Describes the files and/or claim types that should be present at the end of a
successful session. Used by the jig runner to populate the `artifact_check`
field of the result JSON. These are non-scoring checks; the rubric carries the
score. Artifact presence is reported separately so human reviewers can quickly
spot incomplete runs.

| Field | Type | Required | Description |
|---|---|---|---|
| `files` | array of strings | no | Glob patterns (relative to workspace root) for files that should exist. |
| `claim_types` | array of strings | no | `synthesist:` claim types (e.g., `"synthesist:Spec"`, `"synthesist:Discovery"`) that should appear at least once in the claims log. |
| `claim_count_min` | integer | no | Minimum number of new claims (any type) that should be present at the end of the session vs the starting state. |

---

## Template

```toml
[scenario]
name = "scenario-name"
description = "One sentence describing what this scenario tests."
version = "0.1.0"
tags = ["example"]

[starting_state]
description = "Describe the starting workspace state."
setup_commands = [
  "synthesist tree add --id example --title 'Example Tree'",
]

[goal]
prompt = """
You are a synthesist operator. Your task is:

<describe the task here>

Use the synthesist CLI to complete the task. When done, confirm
the final state with `synthesist status`.
"""
success_criterion = "The workspace contains X with property Y."

[[rubric]]
id = "task-created"
description = "The required artifact exists and is valid."
weight = 50
check = "cli"
check_command = "synthesist status"
check_pattern = "some-expected-pattern"

[[rubric]]
id = "quality"
description = "The artifact meets quality criteria defined in the goal."
weight = 50
check = "manual"
partial_credit = true

[expected_artifacts]
claim_types = ["synthesist:Spec"]
claim_count_min = 1
```

---

## Scoring summary

The final score for a run is a 0-100 integer stored in the result JSON at
`_jig/<run_id>.json` under the key `score`. Scores across runs on the same
scenario and manifest are averaged to produce the aggregate signal used in
jig comparisons.

A score of 70 or above on a scenario is treated as a pass. The threshold is
intentionally generous for the alpha jig phase; it will be tightened as
baselines accumulate.

Human-reviewed criteria (`check = "manual"`) are submitted via
`synthesist jig score --run <run_id>` after the session ends. Automated criteria
are evaluated by the jig runner immediately at session close.
