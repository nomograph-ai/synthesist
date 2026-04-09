# Using Synthesist with Cursor

Synthesist is **agent-agnostic**: the LLM drives everything through **`synthesist` CLI**
commands (shell). [Cursor](https://cursor.com) agents can use it the same way as any
other harness, with a few project-level conventions described below.

## What stays the same

- Install the binary ([README.md](../README.md) -- mise, or build from source).
- Run **`synthesist init`** in the project root once.
- Use **sessions** for concurrent work: `synthesist session start <name>`,
  `SYNTHESIST_SESSION` or `--session`, then `synthesist session merge <name>`.
- Follow the **7-phase state machine** (ORIENT -> PLAN -> AGREE -> EXECUTE <-> REFLECT ->
  REPORT, with REPLAN). Rules: [docs/state-machine.md](state-machine.md).
- Enforce phases with **`synthesist phase set`**; the binary rejects invalid transitions.
- The full command reference + embedded rules: **`synthesist skill`** (pipe or paste
  into project instructions).

## Cursor-specific setup

### 1. Shell access

The agent must be able to run **`synthesist`** from the **project root** (where
`synthesist/` lives). In Cursor, use the integrated terminal or agent **run command**
capabilities so paths and `cwd` match the repo root.

### 2. Install the skill output into the project

Cursor does not load `synthesist skill` automatically. Capture it once and reference it
from persistent instructions:

```bash
cd your-project
synthesist skill > docs/synthesist-skill.md   # or .cursor/rules/synthesist-skill.mdc
```

Then in **Cursor Rules** (e.g. `.cursor/rules/`) or a root **`AGENTS.md`**, tell the
agent to follow that file **and** to run `synthesist skill` again after upgrading
Synthesist, so the command tree stays in sync.

Alternatively, without committing the dump:

- Add a rule: *"Before planning Synthesist work, run `synthesist skill` and treat the
  output as the command reference."*

### 3. Concurrent agents

Synthesist supports multiple agents working in the same project simultaneously.
Each agent starts its own session (per-file SQLite copy), writes are isolated,
and merges are PK-aware three-way. See [README.md](../README.md#sessions) for
the full architecture.

**In Cursor:** each tab or sub-agent that does Synthesist work should start its
own session. Assign different specs to different agents for zero-contention
parallel execution.

### 4. Minimal agent checklist

1. **`synthesist session start <name>`** before mutating state.
2. **`synthesist phase set <phase>`** before operations that are phase-gated.
3. **`synthesist session merge <name>`** when the session is complete.
4. On upgrade: refresh instructions from **`synthesist skill`**.

## Contributing improvements

Edits to this file should stay **Cursor-specific** (harness integration). Behavioral
rules for all agents belong in **docs/state-machine.md** and the generated skill output.
