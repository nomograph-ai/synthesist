package main

import (
	"fmt"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdTaskCreate(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task create <tree/spec> <summary> [--depends-on t1,t2] [--gate human] [--files f1,f2]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	summary := args[1]

	// Parse optional flags
	var dependsOn []string
	var gate *string
	var files []string
	var statusFlag, idFlag, createdFlag string
	var completedFlag *string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--depends-on":
			dependsOn = strings.Split(args[i+1], ",")
		case "--gate":
			v := args[i+1]
			gate = &v
		case "--files":
			files = strings.Split(args[i+1], ",")
		case "--status":
			statusFlag = args[i+1]
		case "--id":
			idFlag = args[i+1]
		case "--created":
			createdFlag = args[i+1]
		case "--completed":
			v := args[i+1]
			completedFlag = &v
		}
	}

	// Get next ID (or use provided)
	var newID string
	if idFlag != "" {
		newID = idFlag
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

	today := createdFlag
	if today == "" {
		today = store.Today()
	}

	status := statusFlag
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
