package main

import (
	"fmt"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdTaskCreate(c *TaskCreateCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}
	summary := c.Summary

	// Parse optional flags
	var dependsOn []string
	if c.DependsOn != "" {
		dependsOn = strings.Split(c.DependsOn, ",")
	}
	var gate *string
	if c.Gate != "" {
		v := c.Gate
		gate = &v
	}
	var files []string
	if c.Files != "" {
		files = strings.Split(c.Files, ",")
	}
	var completedFlag *string
	if c.Completed != "" {
		v := c.Completed
		completedFlag = &v
	}

	// Get next ID (or use provided)
	var newID string
	if c.ID != "" {
		newID = c.ID
	} else {
		var ids []string
		rows, err := s.DB.Query("SELECT id FROM tasks WHERE tree = ? AND spec = ?", tree, spec)
		if err != nil {
			return err
		}
		defer rows.Close() //nolint:errcheck
		for rows.Next() {
			var id string
			if err := rows.Scan(&id); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}
			ids = append(ids, id)
		}
		newID = store.NextID("t", ids)
	}

	today := c.Created
	if today == "" {
		today = store.Today()
	}

	status := c.Status
	if status == "" {
		status = "pending"
	}

	_, err = s.DB.Exec(
		"INSERT INTO tasks (tree, spec, id, type, summary, status, gate, created, completed) VALUES (?, ?, ?, 'task', ?, ?, ?, ?, ?)",
		tree, spec, newID, summary, status, gate, today, completedFlag,
	)
	if err != nil {
		return fmt.Errorf("inserting task: %w", err)
	}

	for _, dep := range dependsOn {
		if _, err := s.DB.Exec("INSERT INTO task_deps (tree, spec, task_id, depends_on) VALUES (?, ?, ?, ?)",
			tree, spec, newID, strings.TrimSpace(dep)); err != nil {
			return fmt.Errorf("inserting task dep: %w", err)
		}
	}
	for _, f := range files {
		if _, err := s.DB.Exec("INSERT INTO task_files (tree, spec, task_id, path) VALUES (?, ?, ?, ?)",
			tree, spec, newID, strings.TrimSpace(f)); err != nil {
			return fmt.Errorf("inserting task file: %w", err)
		}
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): add task %s -- %s", tree, spec, newID, summary)); err != nil {
		return err
	}

	result := map[string]any{"id": newID, "tree": tree, "spec": spec, "summary": summary, "status": status}
	return jsonOut(result)
}
