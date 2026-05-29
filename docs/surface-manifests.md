# Surface Manifests

A surface manifest is a TOML file that controls which CLI commands appear in
the synthesist skill output. The skill is generated per manifest:

```
synthesist skill --manifest <path>
```

The default manifest (when no `--manifest` flag is given) is `baseline-v25`,
which exposes the same surface as synthesist 2.5.x.

## File format

```toml
[manifest]
name        = "<short identifier, no spaces>"
description = "<one-line human description>"

[commands]
include = ["<command-key>", ...]   # optional, defaults to []
exclude = ["<command-key>", ...]   # optional, defaults to []
add     = ["<command-key>", ...]   # optional, defaults to []
```

### Fields

| Field | Section | Required | Meaning |
|---|---|---|---|
| `name` | `[manifest]` | yes | Short, lowercase, hyphen-separated identifier used in filenames and log output. |
| `description` | `[manifest]` | yes | One-line prose description shown in `synthesist skill`. |
| `include` | `[commands]` | no | Explicit allowlist of commands in the skill. Empty means "include all baseline commands". |
| `exclude` | `[commands]` | no | Commands to suppress from the skill even if they appear in `include`. Applied after `include`. |
| `add` | `[commands]` | no | Commands beyond the v2.5 baseline to enable (e.g. SPARQL query surface). |

The `[commands]` section itself is optional. A manifest with only `[manifest]`
is valid and produces a skill with all baseline commands included.

### Command keys

Command keys are surface identifiers such as `"status"`, `"task add"`,
`"overlay run"`. Multi-word subcommand paths use a space as the separator
(e.g. `"task done"`, `"session start"`). The full registry is defined in
`src/cli.rs` and interpreted by T5.2.

## Example: baseline-v25

This manifest reproduces the v2.5 surface exactly -- same commands, same
skill output, no graph-query additions.

```toml
# surface/baseline-v25.toml
[manifest]
name        = "baseline-v25"
description = "v2.5-identical surface"

[commands]
include = [
    "status",
    "task add", "task ready", "task done",
    "spec add", "spec show",
    "discovery add",
    "phase set",
    "session start", "session close",
    "tree add",
    "campaign add",
]
exclude = []
add     = []
```

## Example: sparql-exposed

This manifest adds the SPARQL query surface on top of the v2.5 baseline.
Operators using LLM harnesses configured for this manifest can reach the
graph-query commands through the skill file.

```toml
# surface/sparql-exposed.toml
[manifest]
name        = "sparql-exposed"
description = "v2.5 baseline plus graph query surface"

[commands]
include = [
    "status",
    "task add", "task ready", "task done",
    "spec add", "spec show",
    "discovery add",
    "phase set",
    "session start", "session close",
    "tree add",
    "campaign add",
]
exclude = []
add     = ["query", "overlay run", "spec hierarchy"]
```

## Loading a manifest in Rust

```rust
use synthesist::surface::manifest;

let m = manifest::load(path)?;
println!("surface: {} -- {}", m.name, m.description);
```

Or from an in-memory string (useful in tests):

```rust
let m = manifest::parse_str(toml_text, "<inline>")?;
```

Both functions return `anyhow::Result<Manifest>`. On failure the error chain
includes a structured `ManifestError` variant naming the file and the cause:

- `ManifestError::Io` -- file could not be read.
- `ManifestError::Parse` -- TOML is malformed or does not match the schema.
- `ManifestError::MissingField` -- a required field (`name` or `description`)
  is absent.

## What ships where

Initial manifests are committed in T5.4 under `synthesist/surface/`. The
`--manifest` flag wiring lands in T5.3. Command registry filtering lands in
T5.2. This document covers the data format only.
