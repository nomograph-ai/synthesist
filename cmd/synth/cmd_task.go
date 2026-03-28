package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdTaskCreate(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synth task create <tree/spec> <summary> [--depends-on t1,t2] [--gate human] [--files f1,f2]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	summary := args[1]

	// Parse optional flags
	var dependsOn []string
	var gate *string
	var files []string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--depends-on":
			dependsOn = strings.Split(args[i+1], ",")
		case "--gate":
			v := args[i+1]
			gate = &v
		case "--files":
			files = strings.Split(args[i+1], ",")
		}
	}

	// Get next ID
	var ids []string
	rows, err := s.DB.Query("SELECT id FROM tasks WHERE tree = ? AND spec = ?", tree, spec)
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
		var id string
		rows.Scan(&id)
		ids = append(ids, id)
	}
	newID := store.NextID("t", ids)
	today := store.Today()

	_, err = s.DB.Exec(
		"INSERT INTO tasks (tree, spec, id, type, summary, status, gate, created) VALUES (?, ?, ?, 'task', ?, 'pending', ?, ?)",
		tree, spec, newID, summary, gate, today,
	)
	if err != nil {
		return fmt.Errorf("inserting task: %w", err)
	}

	for _, dep := range dependsOn {
		s.DB.Exec("INSERT INTO task_deps (tree, spec, task_id, depends_on) VALUES (?, ?, ?, ?)",
			tree, spec, newID, strings.TrimSpace(dep))
	}
	for _, f := range files {
		s.DB.Exec("INSERT INTO task_files (tree, spec, task_id, path) VALUES (?, ?, ?, ?)",
			tree, spec, newID, strings.TrimSpace(f))
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): add task %s -- %s", tree, spec, newID, summary)); err != nil {
		return err
	}

	result := map[string]any{"id": newID, "tree": tree, "spec": spec, "summary": summary, "status": "pending"}
	return jsonOut(result)
}

func cmdTaskList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth task list <tree/spec>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
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
	defer rows.Close()

	var tasks []map[string]any
	for rows.Next() {
		var id, typ, summary, status, created string
		var owner, completed, gate *string
		rows.Scan(&id, &typ, &summary, &status, &owner, &created, &completed, &gate)
		t := map[string]any{
			"id": id, "type": typ, "summary": summary,
			"status": status, "created": created,
		}
		if owner != nil {
			t["owner"] = *owner
		}
		if completed != nil {
			t["completed"] = *completed
		}
		if gate != nil {
			t["gate"] = *gate
		}

		// Get deps
		depRows, _ := s.DB.Query("SELECT depends_on FROM task_deps WHERE tree = ? AND spec = ? AND task_id = ?", tree, spec, id)
		var deps []string
		for depRows.Next() {
			var d string
			depRows.Scan(&d)
			deps = append(deps, d)
		}
		depRows.Close()
		if len(deps) > 0 {
			t["depends_on"] = deps
		}

		tasks = append(tasks, t)
	}

	return jsonOut(map[string]any{"tree": tree, "spec": spec, "tasks": tasks})
}

func cmdTaskClaim(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synth task claim <tree/spec> <task-id>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	taskID := args[1]

	// Check current status
	var status string
	var owner *string
	err = s.DB.QueryRow("SELECT status, owner FROM tasks WHERE tree = ? AND spec = ? AND id = ?",
		tree, spec, taskID).Scan(&status, &owner)
	if err != nil {
		return fmt.Errorf("task not found: %s/%s/%s", tree, spec, taskID)
	}
	if status != "pending" {
		return fmt.Errorf("task %s is %s, not pending", taskID, status)
	}
	if owner != nil && *owner != "" {
		return fmt.Errorf("task %s already owned by %s", taskID, *owner)
	}

	// Check deps are done
	depRows, _ := s.DB.Query(
		"SELECT d.depends_on, t.status FROM task_deps d JOIN tasks t ON d.tree = t.tree AND d.spec = t.spec AND d.depends_on = t.id WHERE d.tree = ? AND d.spec = ? AND d.task_id = ?",
		tree, spec, taskID,
	)
	defer depRows.Close()
	for depRows.Next() {
		var depID, depStatus string
		depRows.Scan(&depID, &depStatus)
		if depStatus != "done" {
			return fmt.Errorf("dependency %s is %s, not done", depID, depStatus)
		}
	}

	ownerName := "synth"
	_, err = s.DB.Exec("UPDATE tasks SET status = 'in_progress', owner = ? WHERE tree = ? AND spec = ? AND id = ?",
		ownerName, tree, spec, taskID)
	if err != nil {
		return err
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): claim %s", tree, spec, taskID)); err != nil {
		return err
	}

	return jsonOut(map[string]any{"id": taskID, "status": "in_progress", "owner": ownerName})
}

func cmdTaskDone(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synth task done <tree/spec> <task-id>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	taskID := args[1]

	// Run acceptance criteria
	acRows, err := s.DB.Query(
		"SELECT seq, criterion, verify_cmd FROM acceptance WHERE tree = ? AND spec = ? AND task_id = ? ORDER BY seq",
		tree, spec, taskID,
	)
	if err != nil {
		return err
	}
	defer acRows.Close()

	var results []map[string]any
	allPass := true
	for acRows.Next() {
		var seq int
		var criterion, verifyCmd string
		acRows.Scan(&seq, &criterion, &verifyCmd)

		cmd := exec.Command("sh", "-c", verifyCmd)
		cmd.Dir = s.Root
		output, err := cmd.CombinedOutput()
		passed := err == nil

		result := map[string]any{
			"criterion": criterion,
			"verify":    verifyCmd,
			"passed":    passed,
		}
		if !passed {
			allPass = false
			result["output"] = strings.TrimSpace(string(output))
		}
		results = append(results, result)
	}

	today := store.Today()
	if allPass {
		_, err = s.DB.Exec(
			"UPDATE tasks SET status = 'done', completed = ?, owner = NULL, failure_note = NULL WHERE tree = ? AND spec = ? AND id = ?",
			today, tree, spec, taskID,
		)
		if err != nil {
			return err
		}
		if err := s.Commit(fmt.Sprintf("spec(%s/%s): %s done", tree, spec, taskID)); err != nil {
			return err
		}
	} else {
		note := "acceptance criteria failed"
		_, err = s.DB.Exec(
			"UPDATE tasks SET status = 'pending', owner = NULL, failure_note = ? WHERE tree = ? AND spec = ? AND id = ?",
			note, tree, spec, taskID,
		)
	}

	return jsonOut(map[string]any{
		"id": taskID, "all_passed": allPass,
		"status": map[bool]string{true: "done", false: "pending"}[allPass],
		"criteria": results,
	})
}

func cmdTaskWait(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synth task wait <tree/spec> <task-id> --reason '...' --external 'url' --check 'cmd' [--check-after YYYY-MM-DD]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	taskID := args[1]

	var reason, external, check string
	var checkAfter *string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--reason":
			reason = args[i+1]
		case "--external":
			external = args[i+1]
		case "--check":
			check = args[i+1]
		case "--check-after":
			v := args[i+1]
			checkAfter = &v
		}
	}

	if reason == "" || external == "" || check == "" {
		return fmt.Errorf("--reason, --external, and --check are required")
	}

	_, err = s.DB.Exec(
		"UPDATE tasks SET status = 'waiting', waiter_reason = ?, waiter_external = ?, waiter_check = ?, waiter_check_after = ? WHERE tree = ? AND spec = ? AND id = ?",
		reason, external, check, checkAfter, tree, spec, taskID,
	)
	if err != nil {
		return err
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): %s waiting -- %s", tree, spec, taskID, reason)); err != nil {
		return err
	}

	return jsonOut(map[string]any{"id": taskID, "status": "waiting", "reason": reason, "external": external})
}

func cmdTaskBlock(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synth task block <tree/spec> <task-id>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	taskID := args[1]

	_, err = s.DB.Exec("UPDATE tasks SET status = 'blocked' WHERE tree = ? AND spec = ? AND id = ?",
		tree, spec, taskID)
	if err != nil {
		return err
	}

	s.Commit(fmt.Sprintf("spec(%s/%s): %s blocked", tree, spec, taskID))
	return jsonOut(map[string]any{"id": taskID, "status": "blocked"})
}

func cmdTaskReady(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth task ready <tree/spec>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}

	// Tasks that are pending AND have all dependencies done (or no dependencies)
	rows, err := s.DB.Query(`
		SELECT t.id, t.type, t.summary, t.gate
		FROM tasks t
		WHERE t.tree = ? AND t.spec = ? AND t.status = 'pending'
		AND NOT EXISTS (
			SELECT 1 FROM task_deps d
			JOIN tasks dep ON d.tree = dep.tree AND d.spec = dep.spec AND d.depends_on = dep.id
			WHERE d.tree = t.tree AND d.spec = t.spec AND d.task_id = t.id
			AND dep.status != 'done'
		)
		ORDER BY t.id
	`, tree, spec)
	if err != nil {
		return err
	}
	defer rows.Close()

	var ready []map[string]any
	for rows.Next() {
		var id, typ, summary string
		var gate *string
		rows.Scan(&id, &typ, &summary, &gate)
		t := map[string]any{"id": id, "type": typ, "summary": summary}
		if gate != nil {
			t["gate"] = *gate
		}
		ready = append(ready, t)
	}

	return jsonOut(map[string]any{"tree": tree, "spec": spec, "ready": ready, "count": len(ready)})
}

// --- Helpers ---

func parseTreeSpec(s string) (string, string, error) {
	parts := strings.SplitN(s, "/", 2)
	if len(parts) != 2 {
		return "", "", fmt.Errorf("expected tree/spec format, got %q", s)
	}
	return parts[0], parts[1], nil
}

func jsonOut(v any) error {
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	return enc.Encode(v)
}
