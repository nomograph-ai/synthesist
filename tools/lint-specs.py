#!/usr/bin/env python3
"""Synthesist spec estate integrity checker.

Validates the spec tree structure, cross-references, and state consistency.
Exit 0 = clean, exit 1 = issues found.

Usage:
    python3 tools/lint-specs.py [--verbose]
"""

import json
import os
import sys
from pathlib import Path

SPECS_DIR = Path(__file__).parent.parent / "specs"
ESTATE_FILE = SPECS_DIR / "estate.json"
STALE_DAYS = 7

errors = []
warnings = []


def error(msg):
    errors.append(msg)
    print(f"  ERROR: {msg}")


def warn(msg):
    warnings.append(msg)
    print(f"  WARN:  {msg}")


def load_json(path):
    try:
        with open(path) as f:
            return json.load(f)
    except (json.JSONDecodeError, FileNotFoundError) as e:
        error(f"Cannot load {path}: {e}")
        return None


def check_estate():
    """Validate estate.json exists and has valid structure."""
    print("--- estate.json ---")
    if not ESTATE_FILE.exists():
        error("specs/estate.json not found")
        return None

    estate = load_json(ESTATE_FILE)
    if estate is None:
        return None

    if "trees" not in estate:
        error("estate.json missing 'trees' field")
        return None

    if "last_session" not in estate:
        warn("estate.json missing 'last_session' field")

    for name, tree in estate["trees"].items():
        if "path" not in tree:
            error(f"Tree '{name}' missing 'path' field")
        elif not (SPECS_DIR.parent / tree["path"]).exists():
            error(f"Tree '{name}' campaign.json not found at {tree['path']}")
        if "status" not in tree:
            warn(f"Tree '{name}' missing 'status' field")

    print(f"  {len(estate['trees'])} trees found")
    return estate


def check_tree(tree_name, tree_info):
    """Validate a single context tree."""
    print(f"\n--- {tree_name} ---")
    tree_dir = SPECS_DIR / tree_name

    if not tree_dir.is_dir():
        error(f"Tree directory specs/{tree_name}/ not found")
        return

    # Check campaign.json
    campaign_path = tree_dir / "campaign.json"
    campaign = load_json(campaign_path)
    if campaign is None:
        return

    # Check archive.json
    archive_path = tree_dir / "archive.json"
    archive = load_json(archive_path)

    # Check archive/ directory
    archive_dir = tree_dir / "archive"
    if not archive_dir.is_dir():
        warn(f"Tree '{tree_name}' missing archive/ directory")

    # Validate active specs
    for spec in campaign.get("active", []):
        spec_id = spec.get("id", "?")
        spec_path = spec.get("path")
        if spec_path:
            full_path = SPECS_DIR.parent / spec_path
            if not full_path.exists():
                error(f"Active spec '{spec_id}' state.json not found at {spec_path}")
            else:
                check_state_json(full_path, spec_id)

            # Check spec.md exists alongside state.json
            spec_md = full_path.parent / "spec.md"
            if not spec_md.exists():
                warn(f"Active spec '{spec_id}' missing spec.md at {spec_md.parent}")

    # Validate archived specs
    if archive:
        for spec in archive.get("archived", []):
            spec_id = spec.get("id", "?")
            spec_path = spec.get("path")
            if spec_path:
                full_path = SPECS_DIR.parent / spec_path
                if not full_path.exists():
                    error(f"Archived spec '{spec_id}' state.json not found at {spec_path}")

            if "reason" not in spec:
                warn(f"Archived spec '{spec_id}' missing 'reason' field")

    # Check for orphan directories
    known_ids = set()
    for spec in campaign.get("active", []):
        known_ids.add(spec.get("id", "").split("/")[0])
    for spec in campaign.get("backlog", []):
        known_ids.add(spec.get("id", "").split("/")[0])
    if archive:
        for spec in archive.get("archived", []):
            known_ids.add(spec.get("id", "").split("/")[0])

    for entry in tree_dir.iterdir():
        if entry.is_dir() and entry.name not in ("archive",):
            if entry.name not in known_ids:
                # Check if it's a nested spec (e.g., carmine/deploy-smoothing)
                has_state = (entry / "state.json").exists()
                has_sub_state = any((entry / sub / "state.json").exists()
                                   for sub in entry.iterdir() if sub.is_dir())
                if has_state or has_sub_state:
                    warn(f"Directory '{tree_name}/{entry.name}' not in campaign.json or archive.json")


def check_state_json(path, spec_id):
    """Validate a state.json file."""
    state = load_json(path)
    if state is None:
        return

    tasks = state.get("tasks", [])
    if not tasks:
        warn(f"Spec '{spec_id}' has no tasks in state.json")
        return

    for task in tasks:
        tid = task.get("id", "?")

        # Required fields
        for field in ("id", "summary", "status", "acceptance"):
            if field not in task:
                error(f"Spec '{spec_id}' task '{tid}' missing required field '{field}'")

        # Valid status
        status = task.get("status", "")
        if status not in ("pending", "in_progress", "done", "blocked"):
            error(f"Spec '{spec_id}' task '{tid}' invalid status '{status}'")

        # Acceptance criteria must have verify commands
        for ac in task.get("acceptance", []):
            if "verify" not in ac:
                error(f"Spec '{spec_id}' task '{tid}' acceptance missing 'verify' command")
            if "criterion" not in ac:
                warn(f"Spec '{spec_id}' task '{tid}' acceptance missing 'criterion' description")


def check_cross_references(estate):
    """Validate cross-references in spec.md files resolve."""
    print("\n--- Cross-References ---")
    ref_count = 0

    for tree_name in estate.get("trees", {}):
        tree_dir = SPECS_DIR / tree_name
        if not tree_dir.is_dir():
            continue

        for spec_md in tree_dir.rglob("spec.md"):
            try:
                content = spec_md.read_text()
            except Exception:
                continue

            if "<references>" not in content:
                continue

            # Extract references section
            import re
            ref_match = re.search(r"<references>(.*?)</references>", content, re.DOTALL)
            if not ref_match:
                continue

            ref_text = ref_match.group(1)
            for line in ref_text.strip().split("\n"):
                line = line.strip()
                if line.startswith("- spec:"):
                    ref_path = line.split("spec:")[1].strip()
                    ref_count += 1
                    # Check if referenced spec exists in any campaign or archive
                    parts = ref_path.split("/")
                    if len(parts) >= 2:
                        ref_tree = parts[0]
                        ref_spec = "/".join(parts[1:])
                        found = False

                        # Check campaign.json
                        campaign_path = SPECS_DIR / ref_tree / "campaign.json"
                        if campaign_path.exists():
                            campaign = load_json(campaign_path)
                            if campaign:
                                for s in campaign.get("active", []) + campaign.get("backlog", []):
                                    if s.get("id") == ref_spec:
                                        found = True
                                        break

                        # Check archive.json
                        if not found:
                            archive_path = SPECS_DIR / ref_tree / "archive.json"
                            if archive_path.exists():
                                archive = load_json(archive_path)
                                if archive:
                                    for s in archive.get("archived", []):
                                        if s.get("id") == ref_spec:
                                            found = True
                                            break

                        # Check if directory exists directly
                        if not found:
                            direct_path = SPECS_DIR / ref_path
                            if direct_path.is_dir() and (direct_path / "spec.md").exists():
                                found = True

                        if not found:
                            warn(f"Cross-reference '{ref_path}' in {spec_md.relative_to(SPECS_DIR)} not found")

    print(f"  {ref_count} cross-references checked")


def main():
    verbose = "--verbose" in sys.argv or "-v" in sys.argv

    print("=== Synthesist Spec Estate Lint ===\n")

    estate = check_estate()
    if estate is None:
        print(f"\n=== FAILED: {len(errors)} errors ===")
        sys.exit(1)

    for tree_name, tree_info in estate["trees"].items():
        check_tree(tree_name, tree_info)

    check_cross_references(estate)

    print(f"\n=== Results: {len(errors)} errors, {len(warnings)} warnings ===")

    if errors:
        print("\nErrors (must fix):")
        for e in errors:
            print(f"  - {e}")
        sys.exit(1)

    if warnings and verbose:
        print("\nWarnings:")
        for w in warnings:
            print(f"  - {w}")

    sys.exit(0)


if __name__ == "__main__":
    main()
