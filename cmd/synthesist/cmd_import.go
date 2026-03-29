package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"regexp"
	"strings"
)

func cmdImport(c *ImportCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	// Read JSON from file or stdin.
	var raw []byte
	if c.File == "" {
		raw, err = io.ReadAll(os.Stdin)
	} else {
		raw, err = os.ReadFile(c.File)
	}
	if err != nil {
		return fmt.Errorf("reading import data: %w", err)
	}

	var data map[string]any
	if err := json.Unmarshal(raw, &data); err != nil {
		return fmt.Errorf("parsing import JSON: %w", err)
	}

	// Version check: warn if the export version doesn't match expected.
	if v, ok := data["version"]; ok {
		if vs, ok := v.(string); ok && vs != "5" {
			fmt.Fprintf(os.Stderr, "warning: import data version is %q, expected \"5\" — proceeding anyway\n", vs)
		}
	}

	// The export JSON keys and the SQL table names they map to.
	// Keys match the export command's output.
	tables := []struct {
		jsonKey  string
		sqlTable string
	}{
		{"trees", "trees"},
		{"specs", "specs"},
		{"tasks", "tasks"},
		{"task_deps", "task_deps"},
		{"task_files", "task_files"},
		{"acceptance", "acceptance"},
		{"task_patterns", "task_patterns"},
		{"stakeholders", "stakeholders"},
		{"stakeholder_orgs", "stakeholder_orgs"},
		{"dispositions", "dispositions"},
		{"signals", "signals"},
		{"influences", "influences"},
		{"discoveries", "discoveries"},
		{"campaigns_active", "campaign_active"},
		{"campaigns_backlog", "campaign_backlog"},
		{"campaign_blocked_by", "campaign_blocked_by"},
		{"archives", "archives"},
		{"archive_patterns", "archive_patterns"},
		{"archive_contributions", "archive_contributions"},
		{"propagation_chain", "propagation_chain"},
		{"patterns", "patterns"},
		{"pattern_observations", "pattern_observations"},
		{"transforms", "transforms"},
		{"threads", "threads"},
		{"directions", "directions"},
		{"direction_refs", "direction_refs"},
		{"direction_impacts", "direction_impacts"},
		{"task_provenance", "task_provenance"},
		{"config", "config"},
	}

	// Column name allowlist per table: only these columns may appear in
	// imported data. This prevents SQL injection via crafted JSON keys.
	// Regex validates column names are safe identifiers as an extra guard.
	validColName := regexp.MustCompile(`^[a-z][a-z0-9_]*$`)
	allowedColumns := map[string]map[string]bool{
		"trees":                 {"name": true, "path": true, "status": true, "description": true},
		"specs":                 {"tree": true, "id": true, "goal": true, "constraints": true, "decisions": true, "created": true},
		"tasks":                 {"tree": true, "spec": true, "id": true, "type": true, "summary": true, "description": true, "status": true, "gate": true, "owner": true, "created": true, "completed": true, "failure_note": true, "waiter_reason": true, "waiter_external": true, "waiter_check": true, "waiter_check_after": true, "arc": true, "duration_days": true},
		"task_deps":             {"tree": true, "spec": true, "task_id": true, "depends_on": true},
		"task_files":            {"tree": true, "spec": true, "task_id": true, "path": true},
		"acceptance":            {"tree": true, "spec": true, "task_id": true, "seq": true, "criterion": true, "verify_cmd": true},
		"task_patterns":         {"tree": true, "spec": true, "task_id": true, "pattern_id": true},
		"stakeholders":          {"tree": true, "id": true, "name": true, "context": true},
		"stakeholder_orgs":      {"tree": true, "stakeholder_id": true, "org": true},
		"dispositions":          {"tree": true, "spec": true, "id": true, "stakeholder_id": true, "topic": true, "stance": true, "preferred_approach": true, "detail": true, "confidence": true, "evidence": true, "valid_from": true, "valid_until": true, "superseded_by": true},
		"signals":               {"tree": true, "spec": true, "id": true, "stakeholder_id": true, "date": true, "recorded_date": true, "source": true, "source_type": true, "content": true, "interpretation": true, "our_action": true},
		"influences":            {"tree": true, "spec": true, "stakeholder_id": true, "task_id": true, "role": true},
		"discoveries":           {"tree": true, "spec": true, "id": true, "date": true, "author": true, "finding": true, "impact": true, "action_taken": true},
		"campaign_active":       {"tree": true, "spec_id": true, "path": true, "summary": true, "phase": true},
		"campaign_backlog":      {"tree": true, "spec_id": true, "title": true, "summary": true, "path": true},
		"campaign_blocked_by":   {"tree": true, "spec_id": true, "blocked_by": true},
		"archives":              {"tree": true, "spec_id": true, "path": true, "summary": true, "archived": true, "reason": true, "outcome": true, "duration_days": true},
		"archive_patterns":      {"tree": true, "spec_id": true, "pattern_id": true},
		"archive_contributions": {"tree": true, "spec_id": true, "contribution_path": true},
		"propagation_chain":     {"source_tree": true, "source_spec": true, "target_tree": true, "target_spec": true, "seq": true, "description": true},
		"patterns":              {"tree": true, "id": true, "name": true, "description": true, "transferability": true, "first_observed": true},
		"pattern_observations":  {"tree": true, "pattern_id": true, "observed_in": true},
		"transforms":            {"tree": true, "spec": true, "task_id": true, "seq": true, "label": true, "description": true, "transferable": true},
		"threads":               {"id": true, "tree": true, "spec": true, "task": true, "date": true, "summary": true, "waiter_reason": true, "waiter_external": true, "waiter_check": true, "waiter_check_after": true},
		"directions":            {"tree": true, "id": true, "project": true, "topic": true, "status": true, "owner": true, "timeline": true, "detail": true, "impact": true, "valid_from": true, "valid_until": true, "superseded_by": true},
		"direction_refs":        {"tree": true, "direction_id": true, "reference": true},
		"direction_impacts":     {"tree": true, "direction_id": true, "affected_tree": true, "affected_spec": true, "description": true},
		"task_provenance":       {"source_tree": true, "source_spec": true, "source_task": true, "target_tree": true, "target_spec": true, "target_task": true, "note": true},
		"config":                {"key_name": true, "value": true},
		"phase":                 {"id": true, "name": true, "updated": true},
	}

	imported := map[string]int{}

	for _, tbl := range tables {
		arr, ok := data[tbl.jsonKey]
		if !ok {
			imported[tbl.jsonKey] = 0
			continue
		}
		rows, ok := arr.([]any)
		if !ok {
			imported[tbl.jsonKey] = 0
			continue
		}

		count := 0
		for _, rowAny := range rows {
			row, ok := rowAny.(map[string]any)
			if !ok {
				continue
			}
			if len(row) == 0 {
				continue
			}

			// Build column list and placeholders deterministically
			// by iterating sorted-ish from the map. We collect all
			// columns from each row since JSON rows may vary.
			// Validate every column against the allowlist to prevent SQL injection.
			allowed := allowedColumns[tbl.sqlTable]
			cols := make([]string, 0, len(row))
			vals := make([]any, 0, len(row))
			placeholders := make([]string, 0, len(row))
			for col, val := range row {
				if !validColName.MatchString(col) {
					return fmt.Errorf("importing into %s: invalid column name %q", tbl.sqlTable, col)
				}
				if allowed != nil && !allowed[col] {
					return fmt.Errorf("importing into %s: column %q not in allowlist", tbl.sqlTable, col)
				}
				cols = append(cols, col)
				placeholders = append(placeholders, "?")
				// JSON null → SQL NULL (nil in Go)
				if val == nil {
					vals = append(vals, nil)
				} else {
					vals = append(vals, val)
				}
			}

			colList := strings.Join(cols, ", ")
			phList := strings.Join(placeholders, ", ")
			query := fmt.Sprintf("REPLACE INTO %s (%s) VALUES (%s)", tbl.sqlTable, colList, phList) //nolint:gosec

			if _, err := s.DB.Exec(query, vals...); err != nil {
				return fmt.Errorf("importing into %s: %w", tbl.sqlTable, err)
			}
			count++
		}
		imported[tbl.jsonKey] = count
	}

	if err := s.Commit("import: restore data from export"); err != nil {
		return fmt.Errorf("committing import: %w", err)
	}

	return jsonOut(map[string]any{
		"imported": imported,
		"status":   "complete",
	})
}
