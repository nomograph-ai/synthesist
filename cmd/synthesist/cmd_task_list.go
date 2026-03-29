package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/olekukonko/tablewriter"
	"github.com/olekukonko/tablewriter/renderer"
)

func cmdTaskList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist task list <tree/spec> [--human]")
	}

	// Check for --human flag
	human := false
	var filteredArgs []string
	for _, a := range args {
		if a == "--human" {
			human = true
		} else {
			filteredArgs = append(filteredArgs, a)
		}
	}

	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(filteredArgs[0])
	if err != nil {
		return err
	}

	rows, err := s.DB.Query(
		"SELECT id, type, summary, status, owner, created, completed, gate FROM tasks WHERE tree = ? AND spec = ? ORDER BY id",
		tree, spec,
	)
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	var tasks []taskListEntry
	for rows.Next() {
		var t taskListEntry
		if err := rows.Scan(&t.id, &t.typ, &t.summary, &t.status, &t.owner, &t.created, &t.completed, &t.gate); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		depRows, _ := s.DB.Query("SELECT depends_on FROM task_deps WHERE tree = ? AND spec = ? AND task_id = ?", tree, spec, t.id)
		for depRows.Next() {
			var d string
			if err := depRows.Scan(&d); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}
			t.deps = append(t.deps, d)
		}
		_ = depRows.Close()
		tasks = append(tasks, t)
	}

	if human {
		return taskListHuman(tree, spec, tasks)
	}

	// JSON output
	var jsonTasks []map[string]any
	for _, t := range tasks {
		m := map[string]any{
			"id": t.id, "type": t.typ, "summary": t.summary,
			"status": t.status, "created": t.created,
		}
		if t.owner != nil {
			m["owner"] = *t.owner
		}
		if t.completed != nil {
			m["completed"] = *t.completed
		}
		if t.gate != nil {
			m["gate"] = *t.gate
		}
		if len(t.deps) > 0 {
			m["depends_on"] = t.deps
		}
		jsonTasks = append(jsonTasks, m)
	}
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "tasks": jsonTasks})
}

// taskListEntry holds parsed task data for rendering.
type taskListEntry struct {
	id, typ, summary, status, created string
	owner, completed, gate            *string
	deps                              []string
}

func taskListHuman(tree, spec string, tasks []taskListEntry) error {
	symbols := map[string]string{
		"pending": "○", "in_progress": "●", "done": "✓",
		"blocked": "⊘", "waiting": "◷", "cancelled": "✗",
	}

	done := 0
	for _, t := range tasks {
		if t.status == "done" {
			done++
		}
	}

	_, _ = fmt.Fprintf(os.Stdout, "**%s/%s** -- %d/%d done\n\n", tree, spec, done, len(tasks))

	var rows [][]string
	for _, t := range tasks {
		sym := symbols[t.status]
		if sym == "" {
			sym = "?"
		}
		gate := ""
		if t.gate != nil && *t.gate != "" {
			gate = "🔒"
		}
		depStr := ""
		if len(t.deps) > 0 {
			depStr = strings.Join(t.deps, ", ")
		}
		rows = append(rows, []string{sym, t.id, gate, t.summary, depStr})
	}

	table := tablewriter.NewTable(os.Stdout,
		tablewriter.WithHeader([]string{"", "ID", "Gate", "Summary", "Deps"}),
		tablewriter.WithRenderer(renderer.NewMarkdown()),
	)
	for _, row := range rows {
		_ = table.Append(row)
	}
	_ = table.Render()
	return nil
}
