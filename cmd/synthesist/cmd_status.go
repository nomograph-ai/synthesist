package main

import (
	"encoding/json"
	"fmt"
	"os"
)

func cmdStatus(args []string) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	result := map[string]any{}

	// Trees
	rows, err := s.DB.Query("SELECT name, status, description FROM trees ORDER BY name")
	if err == nil {
		var trees []map[string]any
		for rows.Next() {
			var name, status, desc string
			rows.Scan(&name, &status, &desc)
			trees = append(trees, map[string]any{"name": name, "status": status, "description": desc})
		}
		rows.Close()
		result["trees"] = trees
	}

	// Threads
	rows, err = s.DB.Query("SELECT id, tree, spec, task, date, summary FROM threads ORDER BY date DESC")
	if err == nil {
		var threads []map[string]any
		for rows.Next() {
			var id, tree, date, summary string
			var spec, task *string
			rows.Scan(&id, &tree, &spec, &task, &date, &summary)
			t := map[string]any{"id": id, "tree": tree, "date": date, "summary": summary}
			if spec != nil {
				t["spec"] = *spec
			}
			if task != nil {
				t["task"] = *task
			}
			threads = append(threads, t)
		}
		rows.Close()
		result["threads"] = threads
	}

	// Task summary across all specs
	var pending, inProgress, done, waiting, blocked int
	s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE status = 'pending'").Scan(&pending)
	s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE status = 'in_progress'").Scan(&inProgress)
	s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE status = 'done'").Scan(&done)
	s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE status = 'waiting'").Scan(&waiting)
	s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE status = 'blocked'").Scan(&blocked)
	result["task_counts"] = map[string]int{
		"pending": pending, "in_progress": inProgress, "done": done,
		"waiting": waiting, "blocked": blocked,
	}

	// Ready tasks (across all specs)
	rows, err = s.DB.Query(`
		SELECT t.tree, t.spec, t.id, t.summary, t.gate
		FROM tasks t
		WHERE t.status = 'pending'
		AND NOT EXISTS (
			SELECT 1 FROM task_deps d
			JOIN tasks dep ON d.tree = dep.tree AND d.spec = dep.spec AND d.depends_on = dep.id
			WHERE d.tree = t.tree AND d.spec = t.spec AND d.task_id = t.id
			AND dep.status != 'done'
		)
		ORDER BY t.tree, t.spec, t.id
	`)
	if err == nil {
		var ready []map[string]any
		for rows.Next() {
			var tree, spec, id, summary string
			var gate *string
			rows.Scan(&tree, &spec, &id, &summary, &gate)
			r := map[string]any{"tree": tree, "spec": spec, "id": id, "summary": summary}
			if gate != nil {
				r["gate"] = *gate
			}
			ready = append(ready, r)
		}
		rows.Close()
		result["ready_tasks"] = ready
	}

	// Stakeholder count
	var stakeholderCount int
	s.DB.QueryRow("SELECT COUNT(*) FROM stakeholders").Scan(&stakeholderCount)
	result["stakeholder_count"] = stakeholderCount

	// Pattern count
	var patternCount int
	s.DB.QueryRow("SELECT COUNT(*) FROM patterns").Scan(&patternCount)
	result["pattern_count"] = patternCount

	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	return enc.Encode(result)
}

func cmdCheck(args []string) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	var issues []map[string]string
	addIssue := func(level, msg string) {
		issues = append(issues, map[string]string{"level": level, "message": msg})
	}

	// Check: tasks with waiting status must have waiter fields
	rows, _ := s.DB.Query("SELECT tree, spec, id FROM tasks WHERE status = 'waiting' AND waiter_reason IS NULL")
	for rows.Next() {
		var tree, spec, id string
		rows.Scan(&tree, &spec, &id)
		addIssue("error", fmt.Sprintf("task %s/%s/%s has status=waiting but no waiter_reason", tree, spec, id))
	}
	rows.Close()

	// Check: retro tasks must have arc
	rows, _ = s.DB.Query("SELECT tree, spec, id FROM tasks WHERE type = 'retro' AND (arc IS NULL OR arc = '')")
	for rows.Next() {
		var tree, spec, id string
		rows.Scan(&tree, &spec, &id)
		addIssue("error", fmt.Sprintf("retro task %s/%s/%s missing arc field", tree, spec, id))
	}
	rows.Close()

	// Check: dispositions with valid_until should have superseded_by
	rows, _ = s.DB.Query("SELECT tree, spec, id FROM dispositions WHERE valid_until IS NOT NULL AND superseded_by IS NULL")
	for rows.Next() {
		var tree, spec, id string
		rows.Scan(&tree, &spec, &id)
		addIssue("warn", fmt.Sprintf("disposition %s/%s/%s has valid_until but no superseded_by", tree, spec, id))
	}
	rows.Close()

	// Check: task dependencies reference existing tasks
	rows, _ = s.DB.Query(`
		SELECT d.tree, d.spec, d.task_id, d.depends_on
		FROM task_deps d
		LEFT JOIN tasks t ON d.tree = t.tree AND d.spec = t.spec AND d.depends_on = t.id
		WHERE t.id IS NULL
	`)
	for rows.Next() {
		var tree, spec, taskID, dep string
		rows.Scan(&tree, &spec, &taskID, &dep)
		addIssue("error", fmt.Sprintf("task %s/%s/%s depends on %s which does not exist", tree, spec, taskID, dep))
	}
	rows.Close()

	// Check: influence references existing stakeholders
	rows, _ = s.DB.Query(`
		SELECT i.tree, i.spec, i.stakeholder_id
		FROM influences i
		LEFT JOIN stakeholders s ON i.tree = s.tree AND i.stakeholder_id = s.id
		WHERE s.id IS NULL
	`)
	for rows.Next() {
		var tree, spec, stakeholder string
		rows.Scan(&tree, &spec, &stakeholder)
		addIssue("error", fmt.Sprintf("influence in %s/%s references unknown stakeholder %s", tree, spec, stakeholder))
	}
	rows.Close()

	// Check: disposition references existing stakeholders
	rows, _ = s.DB.Query(`
		SELECT d.tree, d.spec, d.id, d.stakeholder_id
		FROM dispositions d
		LEFT JOIN stakeholders s ON d.tree = s.tree AND d.stakeholder_id = s.id
		WHERE s.id IS NULL
	`)
	for rows.Next() {
		var tree, spec, id, stakeholder string
		rows.Scan(&tree, &spec, &id, &stakeholder)
		addIssue("error", fmt.Sprintf("disposition %s/%s/%s references unknown stakeholder %s", tree, spec, id, stakeholder))
	}
	rows.Close()

	errorCount := 0
	warnCount := 0
	for _, issue := range issues {
		if issue["level"] == "error" {
			errorCount++
		} else {
			warnCount++
		}
	}

	result := map[string]any{
		"errors":   errorCount,
		"warnings": warnCount,
		"issues":   issues,
		"passed":   errorCount == 0,
	}

	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	enc.Encode(result)

	if errorCount > 0 {
		return fmt.Errorf("%d errors found", errorCount)
	}
	return nil
}
