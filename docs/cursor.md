# Using Synthesist with Cursor

Synthesist is **agent-agnostic**: the LLM drives everything through **`synthesist` CLI**
commands (shell). [Cursor](https://cursor.com) agents can use it the same way as any
other harness, with a few project-level conventions described below.

This document applies to **v5+** (Dolt-backed binary). Legacy **v1–v4** was a file-based
bundle oriented at OpenCode; v5 removed `opencode.json`, `.opencode/`, and direct JSON
file edits—state lives in `.synth/` and is accessed only via the CLI.

## What stays the same

- Install the binary ([README.md](../README.md) — mise, or build from source).
- Run **`synthesist init`** in the project root once.
- Use **sessions** for concurrent work: `synthesist session start <name>`,
  `SYNTHESIST_SESSION` or `--session`, then `synthesist session merge <name>`.
- Follow the **7-phase state machine** (ORIENT → PLAN → AGREE → EXECUTE ↔ REFLECT →
  REPORT, with REPLAN). Rules: [docs/state-machine.md](state-machine.md).
- Enforce phases with **`synthesist phase set`**; the binary rejects invalid transitions.
- The full command reference + embedded rules: **`synthesist skill`** (pipe or paste
  into project instructions).

## Cursor-specific setup

### 1. Shell access

The agent must be able to run **`synthesist`** from the **project root** (where
`.synth/` lives). In Cursor, use the integrated terminal or agent **run command**
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

### 3. No OpenCode configuration

v5 does not use **`opencode.json`** or OpenCode subagents. Model choice is entirely
**Cursor’s** (subscription / API keys in Cursor settings). Synthesist does not select
or configure the LLM.

### 4. Optional: stances instead of named subagents

If you still have **legacy prompts** that mention `@explore`, `@edit`, `@review`, or
`@verify` (v1–v4 style), map them in your Cursor rules to **modes** in one session or
separate chats—for example read-only search vs edit vs running acceptance checks—
without expecting Cursor to dispatch OpenCode-style subagents by name.

## Concurrent agents

Synthesist supports multiple agents working in the same project simultaneously.
Each agent starts its own session (Dolt branch), writes are isolated, and merges
are row-level. See [Concurrent Sessions](../README.md#concurrent-sessions) for
the full architecture and examples.

**In Cursor:** each tab or sub-agent that does Synthesist work should start its
own session. Assign different specs to different agents for zero-contention
parallel execution.

## Minimal agent checklist

1. **`synthesist session start <name>`** before mutating estate (when using sessions).
2. **`synthesist phase set <phase>`** before operations that are phase-gated.
3. Use **`--human`** when presenting output to the human; default JSON for scripting.
4. **`synthesist session merge <name>`** when the session is complete.
5. On upgrade: refresh instructions from **`synthesist skill`**.

## Contributing improvements

Edits to this file should stay **Cursor-specific** (harness integration). Behavioral
rules for all agents belong in **docs/state-machine.md** and the generated skill output.
