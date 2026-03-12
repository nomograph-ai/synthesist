# Synthesist

A structured approach to decomposing large projects into spec files, agent roles,
and orchestration patterns that LLMs can follow reliably.

Named for the role aboard the *Theseus* in Peter Watts' *Blindsight* — the one
crew member whose job isn't expertise, but coherence. The Synthesist sits among
specialists and makes their outputs legible, directed, and whole.

No orchestration schema is perfect. Like the Bicameral Order in Watts' *Echopraxia*,
the only honest measure of a framework is its predictive power — does following
this process reliably produce working systems? Synthesist is a bet that the answer
is yes, if the specifications are sharp enough and the roles are clear enough.

---

## What This Is

Synthesist is a set of conventions for [OpenCode](https://opencode.ai) that gives
you:

- **One primary agent** — handles the full loop: discuss, draft specs, iterate,
  build. No plan/build handoff.
- **Four subagents** — `@explore`, `@edit`, `@review`, `@verify`
- **A spec format** — Markdown for human intent, JSON for machine state, with
  executable acceptance criteria on every task
- **A workflow** — Discuss → Draft → Iterate → Codify → Build → Verify, with
  human gates at high-stakes transitions
- **Campaign coordination** — cross-spec dependency tracking with temporal horizons
- **Concurrent session safety** — task ownership, aggressive commits, deconfliction

Clone this repo, customize `prompts/instance.md` for your project, and start working.

## Quick Start

```bash
# Clone into your project
cd your-project
git clone https://gitlab.com/nomograph/synthesist.git

# Or add as a submodule
git submodule add https://gitlab.com/nomograph/synthesist.git

# Customize the instance prompt
cp prompts/instance.md prompts/instance.md.bak
# Edit prompts/instance.md — set your project identity, skills, estate

# Open opencode
opencode

# Describe what you want to build. The primary agent handles the full loop.
```

Edit `opencode.json` to set your preferred models and providers. The defaults
use Anthropic model IDs — swap to whatever your provider offers.

## The Spec Format

Every feature gets two files:

| File | Owner | Purpose |
|------|-------|---------|
| `specs/<feature>/spec.md` | Human / Primary agent | What and why — goal, context, constraints, decisions |
| `specs/<feature>/state.json` | Primary / Verify agent | What to do next — task DAG, status, acceptance criteria |

The separation is intentional. Agents reason over prose in spec.md. They execute
from structured data in state.json. Humans review specs in their editor. Agents
update state via tool calls. Git diff shows both.

Each task in state.json has executable acceptance criteria:

```json
{
  "id": "t1",
  "summary": "Create data model",
  "status": "pending",
  "owner": null,
  "acceptance": [
    {
      "criterion": "Model exports required interface",
      "verify": "grep -q 'export.*XModel' src/models/x.ts"
    },
    {
      "criterion": "Unit tests pass",
      "verify": "npm test -- --grep 'XModel'"
    }
  ]
}
```

The `verify` field is a shell command. Exit 0 means pass. The `@verify` agent
runs every single one and doesn't trust self-reports.

See `specs/SPEC_FORMAT.md` for the full schema, and `specs/_example/` for a
worked example.

## The Workflow

```
1. Discuss    You describe intent. Primary agent asks clarifying questions.
2. Draft      Primary agent writes spec.md + state.json to disk immediately.
3. Iterate    Revise spec on disk until it's right. The file is the workspace.
4. Codify     Write state.json task DAG with verify commands. Get approval.
5. Build      Execute tasks in dependency order. Commit after each completion.
6. Verify     @verify agent runs all acceptance criteria. Resets failures.
```

The primary agent handles the full loop in one context — no handoff between
planning and building. The verify agent trusts nothing and runs tests itself.
This prevents the common failure mode where an agent marks its own work as done
without actually checking.

## Agent Roles

| Agent | Mode | Role | Tools | Steps |
|-------|------|------|-------|-------|
| `primary` | primary | Full loop: discuss, plan, build | full access | — |
| `@explore` | subagent | Fast codebase search with countdown | read-only | 5 |
| `@edit` | subagent | Targeted file changes from spec tasks | write, edit, bash | 15 |
| `@review` | subagent | Cross-model code review | read-only | 15 |
| `@verify` | subagent | Acceptance criteria verification | write, edit, bash | 20 |

The `@review` agent uses a different model family than `primary`. Different
training data catches different bugs.

## Prompt Architecture

The primary agent prompt is composed from two files:

| File | Owner | Content |
|------|-------|---------|
| `prompts/framework.md` | Synthesist framework | Loop, write rules, explore rules, context rules, concurrency rules, session rules |
| `prompts/instance.md` | Your project | Identity, skill tree, estate structure, project-specific overrides |

Framework updates flow without merge conflicts on instance-specific content.
Customize `prompts/instance.md` for your project — leave `framework.md` alone.

## Campaigns

For multi-spec projects, campaigns track cross-spec dependencies with three
temporal horizons:

- **done** — what you shipped
- **active** — what you're working on
- **backlog** — what you're thinking about

Campaign state lives at `specs/<campaign>/campaign.json`. See `specs/SPEC_FORMAT.md`
for the schema.

## Concurrent Sessions

Multiple sessions can work on the same spec tree safely:

- Tasks have an `owner` field — check before claiming
- Commit state.json after every task completion
- Pull before starting work
- Dependencies are respected across sessions

This also enables autonomous execution: plan in one session, walk away, and
agents pick up tasks independently.

## Key Design Decisions

**Why a single primary agent instead of plan/build split?**

The original design had a read-only plan agent and a full-access build agent.
In practice, the handoff between them lost context and required copy-paste.
A single agent handling the full loop — with the trust boundary at human
agreement ("build") rather than tool restrictions — proved strictly better
over 16+ sessions of real work.

**Why Markdown + JSON instead of YAML for everything?**

Research on LLM format comprehension (Tam et al., arXiv 2408.02442) shows no
structured format consistently outperforms others, and schema-constrained formats
can degrade reasoning by 20–40%. We use Markdown (with XML sections) for the
parts agents reason over, and JSON for the parts they update mechanically.

**Why executable acceptance criteria?**

If you can't write a shell command that checks whether a task is done, the task
is underspecified. The verify agent runs every command itself — it doesn't ask
"did you finish?" It checks.

**Why a separate verify agent?**

"Trust nothing, verify everything." The build agent self-reports are unreliable —
not because the model lies, but because it optimizes for completion. A separate
agent with the sole job of running tests catches the gap.

**Why human gates?**

Tasks that touch auth, data models, or public APIs get `"gate": "human"`. The
agent stops and presents what it plans to do. This prevents the expensive class
of errors where an agent makes a reasonable-but-wrong decision and builds on it.

**Why cross-model review?**

Different model families have different failure modes. Using one to review the
other's work is cheap insurance.

## Project Structure

```
synthesist/
├── opencode.json              # Agent configuration (edit model IDs for your provider)
├── AGENTS.md                  # Workflow instructions loaded by all agents
├── .opencode/agents/
│   ├── review.md              # Cross-model reviewer system prompt
│   └── verify.md              # Verification agent system prompt
├── specs/
│   ├── SPEC_FORMAT.md         # Spec schema reference (loaded via instructions)
│   ├── _template/             # Copy for new features
│   │   ├── spec.md
│   │   └── state.json
│   └── _example/              # Worked example
│       ├── spec.md
│       └── state.json
├── prompts/
│   ├── framework.md           # Framework prompt (don't edit — synthesist owns this)
│   └── instance.md            # Instance prompt (customize for your project)
├── staging/                   # Chunked write staging area (gitignored)
│   └── .gitkeep
└── memory/                    # Session handoff (create as needed)
    └── session.md
```

## Configuration

The `opencode.json` ships with Anthropic model IDs as defaults. Adjust for
your provider:

```jsonc
// GitLab Duo
"model": "gitlab/duo-chat-sonnet-4-6"

// Direct Anthropic
"model": "anthropic/claude-sonnet-4"

// Local via Ollama
"model": "ollama/qwen3:32b"
```

The framework is provider-agnostic. The spec format, agent roles, and workflow
work with any LLM that OpenCode supports.

## Sources and Influences

| Project | What we took | What we left |
|---------|-------------|--------------|
| [Symphony](https://github.com/openai/symphony) (OpenAI) | WORKFLOW.md hybrid format, hook lifecycle, stall detection | Polling daemon, Linear integration |
| [BMAD Method](https://github.com/bmad-code-org/BMAD-METHOD) | Scale-adaptive planning, human gates (HALT) | 12+ agent personas, npm packaging |
| [GSD](https://github.com/glittercowboy/get-shit-done) | XML task format with `<verify>`, discuss-before-plan | Full artifact forest, config-driven modes |
| [Gastown](https://github.com/steveyegge/gastown) (Steve Yegge) | Typed dependency graphs, quality scoring, "findings survive context death" | Dolt database, Go CLI, agent swarm |
| [Ralph](https://github.com/snarktank/ralph) | Task sizing discipline, append-only progress log | Bash-loop-only orchestration |
| [Metaswarm](https://github.com/dsifry/metaswarm) | "Trust nothing, verify everything", cross-model review | 18 agent personas, 9-phase SDLC |
| Tam et al. (arXiv 2408.02442) | No structured format consistently wins; use natural language for reasoning | — |

## License

MIT
