#!/usr/bin/env python3
"""Migrate synthesist data from v5 (Go+Dolt) to v1.0.0 (Rust+SQLite).

This is a disposable migration tool. Remove it after Andrew and Josh
migrate their estates.

Usage:
    # 1. Export from v5 (using the Go binary)
    cd your-project
    synthesist export > v5-export.json

    # 2. Initialize v1.0.0 (using the Rust binary)
    synthesist init

    # 3. Run this migration
    python3 tools/migrate-v5-to-v1.py v5-export.json

    # 4. Import into v1.0.0
    synthesist --session=migration --force import v1-import.json

The migration:
- Reads v5 JSON export format
- Drops tables removed in v1.0.0 (directions, influences, patterns, etc.)
- Maps threads to session_meta
- Maps archives to spec status fields
- Adds PKs to tables that lacked them (task_files, stakeholder_orgs, etc.)
- Writes v1.0.0 JSON import format
"""

import json
import sys
from pathlib import Path


def migrate(v5_data: dict) -> dict:
    """Transform v5 export JSON to v1.0.0 import JSON."""
    v1 = {
        "version": "1",
        "exported": v5_data.get("exported", ""),
    }

    # --- v1.0.0 column definitions (for stripping extra v5 columns) ---
    V1_COLUMNS = {
        "trees": ["name", "status", "description"],
        "task_deps": ["tree", "spec", "task_id", "depends_on"],
        "acceptance": ["tree", "spec", "task_id", "seq", "criterion", "verify_cmd"],
        "stakeholders": ["tree", "id", "name", "context"],
        "stakeholder_orgs": ["tree", "stakeholder_id", "org"],
        "dispositions": ["tree", "spec", "id", "stakeholder_id", "topic", "stance",
                         "preferred_approach", "detail", "confidence", "valid_from",
                         "valid_until", "superseded_by"],
        "signals": ["tree", "spec", "id", "stakeholder_id", "date", "recorded_date",
                     "source", "source_type", "content", "interpretation", "our_action"],
        "campaign_blocked_by": ["tree", "spec_id", "blocked_by"],
        "discoveries": ["tree", "spec", "id", "date", "author", "finding", "impact", "action"],
    }

    def strip_columns(rows, cols):
        """Keep only columns in the v1.0.0 schema."""
        return [{k: row.get(k) for k in cols if k in row} for row in rows]

    # --- Direct copy (strip extra v5 columns) ---
    for table, cols in V1_COLUMNS.items():
        v1[table] = strip_columns(v5_data.get(table, []), cols)

    # v5 uses "campaigns_active" (plural), v1.0.0 uses "campaign_active" (singular).
    # Also strip extra columns (v5 has "path" on campaigns).
    ca_cols = ["tree", "spec_id", "summary", "phase"]
    cb_cols = ["tree", "spec_id", "title", "summary"]
    v1["campaign_active"] = strip_columns(
        v5_data.get("campaigns_active", v5_data.get("campaign_active", [])), ca_cols
    )
    v1["campaign_backlog"] = strip_columns(
        v5_data.get("campaigns_backlog", v5_data.get("campaign_backlog", [])), cb_cols
    )

    # --- Specs: add status and outcome fields from archives ---
    specs = []
    archives = {
        (a.get("tree", ""), a.get("id", "")): a
        for a in v5_data.get("archives", [])
    }
    for spec in v5_data.get("specs", []):
        key = (spec.get("tree", ""), spec.get("id", ""))
        s = dict(spec)
        if key in archives:
            archive = archives[key]
            reason = archive.get("reason", "completed")
            s["status"] = reason if reason in (
                "completed", "abandoned", "superseded", "deferred"
            ) else "completed"
            s["outcome"] = archive.get("outcome", "")
        else:
            s.setdefault("status", "active")
            s.setdefault("outcome", None)
        specs.append(s)
    v1["specs"] = specs

    # --- Tasks: add wait_reason from waiter fields, drop retro-specific fields ---
    tasks = []
    for task in v5_data.get("tasks", []):
        t = {
            "tree": task.get("tree", ""),
            "spec": task.get("spec", ""),
            "id": task.get("id", ""),
            "summary": task.get("summary", ""),
            "description": task.get("description"),
            "status": task.get("status", "pending"),
            "gate": task.get("gate"),
            "owner": task.get("owner"),
            "created": task.get("created", ""),
            "completed": task.get("completed"),
            "failure_note": task.get("failure_note"),
            "wait_reason": task.get("waiter_reason"),
        }
        # Skip retro-type tasks (type == "retro") -- record as discoveries instead
        if task.get("type") == "retro":
            arc = task.get("arc", "")
            if arc:
                discovery = {
                    "tree": t["tree"],
                    "spec": t["spec"],
                    "id": f"retro-{t['id']}",
                    "date": t.get("completed") or t["created"],
                    "author": None,
                    "finding": f"Retrospective: {arc}",
                    "impact": None,
                    "action": None,
                }
                v1.setdefault("discoveries", []).append(discovery)
            continue
        tasks.append(t)
    v1["tasks"] = tasks

    # --- Task files: ensure unique (add PK) ---
    seen_files = set()
    task_files = []
    for tf in v5_data.get("task_files", []):
        key = (tf.get("tree", ""), tf.get("spec", ""), tf.get("task_id", ""), tf.get("path", ""))
        if key not in seen_files:
            seen_files.add(key)
            task_files.append(tf)
    v1["task_files"] = task_files

    # --- Threads -> session_meta ---
    sessions = []
    for thread in v5_data.get("threads", []):
        sessions.append({
            "id": thread.get("id", ""),
            "started": thread.get("date", ""),
            "owner": None,
            "tree": thread.get("tree"),
            "spec": thread.get("spec"),
            "summary": thread.get("summary", ""),
            "status": "merged",  # historical threads are already merged
        })
    v1["session_meta"] = sessions

    # --- Phase: carry forward ---
    v1["phase"] = v5_data.get("phase", [{"id": 1, "name": "orient"}])

    # --- Config ---
    v1["config"] = [
        {"key_name": "schema_version", "value": "1"},
        {"key_name": "auto_commit", "value": "true"},
    ]

    # --- Dropped tables (logged) ---
    dropped = []
    for table in [
        "directions", "direction_refs", "direction_impacts",
        "influences", "patterns", "pattern_observations",
        "transforms", "task_patterns", "task_provenance",
        "archives", "archive_patterns", "archive_contributions",
        "threads",
    ]:
        count = len(v5_data.get(table, []))
        if count > 0:
            dropped.append({"table": table, "rows": count})

    return v1, dropped


def main():
    if len(sys.argv) < 2:
        print("Usage: migrate-v5-to-v1.py <v5-export.json> [output.json]", file=sys.stderr)
        print("\nExport from v5 first: synthesist export > v5-export.json", file=sys.stderr)
        sys.exit(1)

    input_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("v1-import.json")

    with open(input_path) as f:
        content = f.read()
    # v5 Go binary sometimes prints warnings to stdout before JSON.
    # Strip everything before the first '{'.
    json_start = content.find('{')
    if json_start > 0:
        print(f"Note: stripped {json_start} bytes of non-JSON prefix from export file.")
        content = content[json_start:]
    v5_data = json.loads(content)

    version = v5_data.get("version", "unknown")
    print(f"Reading v5 export (version {version})...")

    v1_data, dropped = migrate(v5_data)

    # Summary
    print(f"\nMigration summary:")
    for table in sorted(v1_data.keys()):
        if isinstance(v1_data[table], list):
            print(f"  {table}: {len(v1_data[table])} rows")

    if dropped:
        print(f"\nDropped tables (v5 features deferred in v1.0.0):")
        for d in dropped:
            print(f"  {d['table']}: {d['rows']} rows (data preserved in v5 export)")

    with open(output_path, "w") as f:
        json.dump(v1_data, f, indent=2)

    print(f"\nWritten to {output_path}")
    print(f"\nNext steps:")
    print(f"  1. Initialize v1.0.0:  synthesist init")
    print(f"  2. Import:             synthesist --session=migration --force import {output_path}")
    print(f"  3. Verify:             synthesist status")
    print(f"  4. Commit:             git add synthesist/ && git commit -m 'migrate to synthesist v1.0.0'")


if __name__ == "__main__":
    main()
