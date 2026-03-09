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

We're using Synthesist to iterate the [nomograph](https://gitlab.com/nomograph)
project estate — it's both the framework and our primary test case.

---

## What This Is

Synthesist is a set of conventions for [OpenCode](https://opencode.ai) that gives
you:

- **Two primary agents** — `plan` (read-only, writes specs) and `build` (full
  access, executes specs)
- **Five subagents** — `@explore`, `@edit`, `@review`, `@verify`, `@test`
- **A spec format** — Markdown for human intent, JSON for machine state, with
  executable acceptance criteria on every task
- **A workflow** — Discuss → Plan → Build → Verify, with human gates at
  high-stakes transitions

Drop this repo into a project as a subdirectory. OpenCode picks up the
`opencode.json` and the agents are available.

## Quick Start

```bash
# Clone into your project
cd your-project
git clone https://gitlab.com/nomograph/synthesist.git orchestration

# Or add as a submodule
git submodule add https://gitlab.com/nomograph/synthesist.git orchestration

# Open opencode from the orchestration directory
cd orchestration
opencode

# Tab switches between plan (read-only) and build (full access)
# Start by describing what you want to build to the plan agent
```

Edit `opencode.json` to set your preferred models and providers. The defaults
use Anthropic model IDs — swap to whatever your provider offers.

## The Spec Format

Every feature gets two files:

| File | Owner | Purpose |
|------|-------|---------|
| `specs/<feature>/spec.md` | Human / Plan agent | What and why — goal, context, constraints, decisions |
| `specs/<feature>/state.json` | Build / Verify agent | What to do next — task DAG, status, acceptance criteria |

The separation is intentional. Agents reason over prose in spec.md. They execute
from structured data in state.json. Humans review specs in their editor. Agents
update state via tool calls. Git diff shows both.

Each task in state.json has executable acceptance criteria:

```json
{
  "id": "t1",
  "summary": "Create data model",
  "status": "pending",
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
1. Discuss    You describe intent. Plan agent asks clarifying questions.
2. Plan       Plan agent reads codebase, writes spec.md + state.json.
3. Build      Build agent executes tasks in dependency order.
4. Review     @review agent (different model family) checks the work.
5. Verify     @verify agent runs all acceptance criteria. Resets failures.
6. Complete   All tasks done, all criteria pass, quality scored.
```

The plan agent cannot modify files — it only reads and writes specs. The build
agent reads specs and implements. The verify agent trusts nothing and runs tests
itself. This separation prevents the common failure mode where an agent marks its
own work as done without actually checking.

## Agent Roles

| Agent | Mode | Role | Tools |
|-------|------|------|-------|
| `plan` | primary | Spec writing, architecture, research | read-only |
| `build` | primary | Implementation, refactoring | full access |
| `@explore` | subagent | Fast codebase search | read-only |
| `@edit` | subagent | Targeted file changes from spec tasks | write, edit |
| `@review` | subagent | Cross-model code review | read-only |
| `@verify` | subagent | Acceptance criteria verification | write, bash |
| `@test` | subagent | Test generation and execution | write, bash |

The `@review` agent should use a different model family than `build`. Different
training data catches different bugs. A review that passes both families is more
reliable than one that passes either alone.

## Key Design Decisions

**Why Markdown + JSON instead of YAML for everything?**

Research on LLM format comprehension (Tam et al., arXiv 2408.02442) shows no
structured format consistently outperforms others, and schema-constrained formats
can degrade reasoning by 20–40%. Anthropic's own guidance recommends XML tags for
structuring prompts and JSON for machine-readable state. We use Markdown (with XML
sections) for the parts agents reason over, and JSON for the parts they update
mechanically.

**Why executable acceptance criteria?**

Inspired by GSD's `<verify>` tags and Ralph's `passes` field. If you can't write a
shell command that checks whether a task is done, the task is underspecified. The
verify agent runs every command itself — it doesn't ask the build agent "did you
finish?" It checks.

**Why a separate verify agent?**

From Metaswarm's principle: "Trust nothing, verify everything." The build agent
self-reports are unreliable — not because the model lies, but because it optimizes
for completion. A separate agent with the sole job of running tests catches the
gap between "I think I'm done" and "the tests actually pass."

**Why human gates?**

Tasks that touch auth, data models, or public APIs get `"gate": "human"` in
state.json. The build agent stops and presents what it plans to do. You approve
or redirect. This prevents the expensive class of errors where an agent makes a
reasonable-but-wrong architectural decision and builds three more tasks on top of it.

**Why cross-model review?**

Different model families have different failure modes. GPT and Claude disagree on
different things. Using one to review the other's work is cheap insurance.

## Sources and Influences

Synthesist draws on ideas from several projects in the multi-agent coding space
as of early 2026:

| Project | What we took | What we left |
|---------|-------------|--------------|
| [Symphony](https://github.com/openai/symphony) (OpenAI) | WORKFLOW.md hybrid format (YAML frontmatter + Markdown body), hook lifecycle, stall detection concept | The polling daemon architecture, Linear integration, Codex-specific protocol |
| [BMAD Method](https://github.com/bmad-code-org/BMAD-METHOD) | Scale-adaptive planning, human gates (HALT), step-based workflows with frontmatter state tracking | 12+ agent personas, module system, npm packaging |
| [GSD (Get Shit Done)](https://github.com/glittercowboy/get-shit-done) | XML task format with `<verify>` and `<done>` fields, discuss phase before planning, wave-based parallel execution, fresh context per executor | Full artifact forest (PROJECT.md, REQUIREMENTS.md, ROADMAP.md, etc.), config-driven mode system |
| [Gastown](https://github.com/steveyegge/gastown) (Steve Yegge) | Typed dependency graphs, quality scoring (0.0–1.0), validation records, "findings survive context death" pattern, Refinery merge-queue verification | Dolt database, Go CLI, TOML formulas, 20–30 concurrent agent swarm, git worktrees per agent |
| [Ralph](https://github.com/snarktank/ralph) | Task sizing discipline ("each story fits in one context window"), append-only progress log as institutional memory, loop-until-done verification | Bash-loop-only orchestration, no planning phase |
| [Multi-Agent Coding System](https://github.com/Danau5tin/multi-agent-coding-system) | Strict role separation (orchestrator cannot touch code), context refs for selective knowledge injection | Custom XML+YAML task format, RL-trained model |
| [Metaswarm](https://github.com/dsifry/metaswarm) | "Trust nothing, verify everything" — orchestrator runs tests itself, cross-model adversarial review | 18 agent personas, 9-phase SDLC, JSONL knowledge base |
| [Adversarial Spec](https://github.com/zscole/adversarial-spec) | Multi-model debate for spec hardening, early agreement skepticism | Full consensus protocol (we use single cross-model review instead) |
| Anthropic prompt engineering docs | XML tags for structured prompting, JSON for state tracking, `tests.json` pattern for acceptance criteria | — |
| Tam et al. (arXiv 2408.02442) | Evidence that no structured format consistently wins; schema constraints degrade reasoning; use natural language for reasoning, structured formats for state | — |

The general pattern: take the spec format and verification ideas, leave the
infrastructure complexity. Synthesist assumes you already have a coding agent
runtime (OpenCode) and focuses on the coordination layer between agents.

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
│   └── _example/              # Worked example: API key authentication
│       ├── spec.md
│       └── state.json
└── prompts/
    └── plan.md                # Plan agent system prompt supplement
```

## Configuration

The `opencode.json` ships with Anthropic and OpenAI model IDs as defaults.
Adjust for your provider:

```jsonc
// GitLab Duo
"model": "gitlab/claude-sonnet-4"

// Direct Anthropic
"model": "anthropic/claude-sonnet-4"

// Local via Ollama
"model": "ollama/qwen3:32b"
```

The framework is provider-agnostic. The spec format, agent roles, and workflow
work with any LLM that OpenCode supports.

## License

MIT
