package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
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
		{"stakeholders", "stakeholders"},
		{"dispositions", "dispositions"},
		{"signals", "signals"},
		{"discoveries", "discoveries"},
		{"campaigns_active", "campaign_active"},
		{"campaigns_backlog", "campaign_backlog"},
		{"archives", "archives"},
		{"propagation_chain", "propagation_chain"},
		{"patterns", "patterns"},
		{"transforms", "transforms"},
		{"threads", "threads"},
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
			cols := make([]string, 0, len(row))
			vals := make([]any, 0, len(row))
			placeholders := make([]string, 0, len(row))
			for col, val := range row {
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
