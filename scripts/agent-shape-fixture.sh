#!/usr/bin/env bash
#
# Seed a realistic synthesist fixture for the jig agent-shape battery.
#
# Produces fixtures/agent-shape-realistic/claims/ populated with:
#   - 3 trees (keaton, nomograph-release, outreach)
#   - 5 specs spanning the trees, some referencing 'gkg'
#   - two closed sessions (lever-audit, kit-locality)
#   - one in-flight session (treatment-design)
#   - a couple of discoveries
#
# Idempotent: wipes and rebuilds the fixture on every invocation so
# each trial starts from identical state.
#
# Usage: run by jig during fixture setup. Can also be run manually
# from any cwd; the script computes paths relative to itself.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FIXTURE_DIR="$REPO_ROOT/fixtures/agent-shape-realistic"

# Strip any inherited session/dir env vars so this script never
# accidentally writes to a different synthesist instance.
unset SYNTHESIST_SESSION
unset SYNTHESIST_DIR

# Hard reset.
rm -rf "$FIXTURE_DIR"
mkdir -p "$FIXTURE_DIR"
cd "$FIXTURE_DIR"

# From here on every synthesist call uses this fixture's claims/.
synthesist init > /dev/null

# --- bootstrap session: trees, specs, tasks ---
synthesist session start bootstrap --summary "seed realistic estate" > /dev/null
export SYNTHESIST_SESSION=bootstrap
synthesist --force phase set plan > /dev/null

synthesist tree add keaton --description "Meta-project: keaton harness management and migration" > /dev/null
synthesist tree add nomograph-release --description "Main campaign: papers, tools, benchmarks, website, brand" > /dev/null
synthesist tree add outreach --description "External collaboration and community engagement" > /dev/null

synthesist spec add keaton/lever-compliance \
  --goal "Apply Lever primitives to estate tools" > /dev/null
synthesist spec add keaton/tool-surface-conformity \
  --goal "Kit and rune parity: shared color module, shared skill output" > /dev/null
synthesist spec add nomograph-release/gkg-bench \
  --goal "Benchmark GKG representation; Phase 5 placement study" > /dev/null
synthesist spec add nomograph-release/gkg-test-gen \
  --goal "Generate gkg test cases from real issues" > /dev/null
synthesist spec add outreach/gkg-outreach \
  --goal "Comment series on gkg issues #317, #318, #271, #395, #397" > /dev/null

synthesist task add keaton/lever-compliance \
  "Add deny(warnings, clippy::all) to synthesist main.rs" > /dev/null
synthesist task add keaton/lever-compliance \
  "Clean up 13 allow(dead_code) annotations in kit" --depends-on t1 > /dev/null
synthesist task add keaton/tool-surface-conformity \
  "Extract shared color module from rune" > /dev/null
synthesist task add nomograph-release/gkg-bench \
  "Run placement P5 experiments on Sonnet 4.6" > /dev/null
synthesist task add nomograph-release/gkg-bench \
  "Write FINDINGS-placement-p5.md" --depends-on t1 > /dev/null
synthesist task add nomograph-release/gkg-test-gen \
  "Blog outline: architectural thinking for the agentic future" > /dev/null

synthesist session close bootstrap > /dev/null

# --- closed session: lever-audit ---
synthesist session start lever-audit \
  --tree keaton --spec lever-compliance \
  --summary "Audit synthesist, rune, kit, muxr against lever primitives" > /dev/null
export SYNTHESIST_SESSION=lever-audit
synthesist --force phase set execute > /dev/null
synthesist task claim keaton/lever-compliance t1 > /dev/null
synthesist discovery add keaton/lever-compliance \
  --finding "rune already compliant; kit has 13 allow(dead_code); synthesist needs crate-level deny" \
  --impact medium > /dev/null
synthesist task done keaton/lever-compliance t1 > /dev/null
synthesist session close lever-audit > /dev/null

# --- closed session: kit-locality ---
synthesist session start kit-locality \
  --tree keaton --spec tool-surface-conformity \
  --summary "Kit locality: ensure state is project-local" > /dev/null
export SYNTHESIST_SESSION=kit-locality
synthesist --force phase set plan > /dev/null
synthesist task add keaton/tool-surface-conformity \
  "Kit: add color.rs module (copy rune's ANSI module)" > /dev/null
synthesist session close kit-locality > /dev/null

# --- in-flight session: treatment-design (deliberately left open) ---
synthesist session start treatment-design \
  --tree keaton --spec lever-compliance \
  --summary "Designing prescriptive error structure for synthesist CLI" > /dev/null
export SYNTHESIST_SESSION=treatment-design
synthesist --force phase set plan > /dev/null
synthesist task add keaton/lever-compliance \
  "Design prescriptive error shape with next-action field" > /dev/null
synthesist --force phase set execute > /dev/null
synthesist task claim keaton/lever-compliance t3 > /dev/null

unset SYNTHESIST_SESSION
echo "fixture seeded at $FIXTURE_DIR"
