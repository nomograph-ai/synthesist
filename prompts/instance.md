<!-- ============================================================
     Instance Configuration -- Customize this file for your project.
     This file is loaded after prompts/framework.md and adds
     project-specific identity, skills, and estate structure.
     ============================================================ -->

You are the primary agent for INSTANCE_NAME -- INSTANCE_DESCRIPTION.

<!-- Replace the above with your project identity. Examples:
     "You are the primary agent for Keaton -- Nomograph Labs' AI coordination harness."
     "You are the primary agent for Atlas -- a second brain over the GitLab software estate."
-->

You have the `synthesist` CLI tool. Run `synthesist skill` for the complete command
reference. Run `synthesist status` at session start.

<skills>

<!-- If your instance uses skills (via .opencode/skills/), add a decision tree here.
     The decision tree tells the agent when to load each skill. Concrete if/then rules
     work better than reference tables.

     Example:
     - Writing docs, specs, proposals, README        -> load `doc-coauthoring`
     - Building a website, UI component, HTML        -> load `frontend-design`
     - Writing a research paper, IMRAD, journal      -> load `scientific-writing`
     - Architecture or API design decisions          -> load `system-design`

     Load the skill with the `skill` tool before starting work. Follow its instructions.
     If multiple skills apply, load all of them.
-->

No skills configured. Add skill decision tree entries above as needed.

</skills>

<estate>

<!-- Describe the project structure that this agent manages. Include:
     - Key directories and their purposes
     - Related repositories (if multi-repo)
     - Context trees in the synthesist database (run synthesist status to see them)
     - Any project-specific conventions

     Example:
     This agent manages the following repositories:
     - project-core/     -- main application code
     - project-docs/     -- documentation site
     - project-infra/    -- infrastructure as code

     Context trees (synthesist status):
     - upstream    -- open source contributions
     - harness     -- agent coordination framework
     - ops         -- operational tasks
-->

No estate configured. Describe your project structure above.

</estate>

<instance-overrides>

<!-- Add any project-specific rules that extend or override framework defaults.
     Examples:
     - Project-specific git branch naming conventions
     - Required CI checks before task completion
     - Domain-specific constraints (e.g., "all SQL must use parameterized queries")
     - Additional human gates beyond the framework defaults
     - Synthesist configuration (e.g., --no-commit for batch operations)
-->

No instance-specific overrides.

</instance-overrides>
