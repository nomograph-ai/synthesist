package main

import (
	"fmt"
	"time"
)

func cmdExport() error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	result := map[string]any{
		"version":  "5",
		"exported": time.Now().UTC().Format(time.RFC3339),
	}

	tables := []string{
		"trees", "specs", "tasks", "task_deps", "task_files",
		"acceptance", "task_patterns",
		"stakeholders", "stakeholder_orgs",
		"dispositions", "signals", "influences", "discoveries",
		"campaigns_active", "campaigns_backlog", "campaign_blocked_by",
		"archives", "archive_patterns", "archive_contributions",
		"propagation_chain", "patterns", "pattern_observations",
		"transforms", "threads",
		"directions", "direction_refs", "direction_impacts",
		"task_provenance", "config",
	}

	// Map table names to their actual SQL table names where they differ.
	sqlNames := map[string]string{
		"campaigns_active":  "campaign_active",
		"campaigns_backlog": "campaign_backlog",
	}

	for _, table := range tables {
		sqlTable := table
		if mapped, ok := sqlNames[table]; ok {
			sqlTable = mapped
		}
		rows, err := s.DB.Query(fmt.Sprintf("SELECT * FROM %s", sqlTable)) //nolint:gosec
		if err != nil {
			// Table might not exist in older schemas — emit empty array
			result[table] = []any{}
			continue
		}
		cols, err := rows.Columns()
		if err != nil {
			_ = rows.Close()
			result[table] = []any{}
			continue
		}

		var records []map[string]any
		for rows.Next() {
			vals := make([]any, len(cols))
			ptrs := make([]any, len(cols))
			for i := range vals {
				ptrs[i] = &vals[i]
			}
			if err := rows.Scan(ptrs...); err != nil {
				_ = rows.Close()
				return fmt.Errorf("scanning %s: %w", sqlTable, err)
			}
			row := make(map[string]any, len(cols))
			for i, col := range cols {
				v := vals[i]
				// Convert []byte to string for JSON marshalling
				if b, ok := v.([]byte); ok {
					v = string(b)
				}
				row[col] = v
			}
			records = append(records, row)
		}
		if err := rows.Err(); err != nil {
			_ = rows.Close()
			return fmt.Errorf("iterating %s rows: %w", sqlTable, err)
		}
		_ = rows.Close()
		if records == nil {
			records = []map[string]any{}
		}
		result[table] = records
	}

	return jsonOut(result)
}
