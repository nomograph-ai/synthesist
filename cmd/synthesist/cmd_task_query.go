package main

import "fmt"

func cmdTaskReady(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist task ready <tree/spec>")
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
	defer rows.Close() //nolint:errcheck

	var ready []map[string]any
	for rows.Next() {
		var id, typ, summary string
		var gate *string
		if err := rows.Scan(&id, &typ, &summary, &gate); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		t := map[string]any{"id": id, "type": typ, "summary": summary}
		if gate != nil {
			t["gate"] = *gate
		}
		ready = append(ready, t)
	}

	return jsonOut(map[string]any{"tree": tree, "spec": spec, "ready": ready, "count": len(ready)})
}

func cmdTaskAcceptance(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task acceptance <tree/spec> <task-id> --criterion '...' --verify 'cmd'")
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
	taskID := args[1]

	var criterion, verify string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--criterion":
			criterion = args[i+1]
		case "--verify":
			verify = args[i+1]
		}
	}
	if criterion == "" || verify == "" {
		return fmt.Errorf("--criterion and --verify are required")
	}

	var maxSeq int
	if err := s.DB.QueryRow("SELECT COALESCE(MAX(seq), 0) FROM acceptance WHERE tree = ? AND spec = ? AND task_id = ?",
		tree, spec, taskID).Scan(&maxSeq); err != nil {
		return fmt.Errorf("scanning max seq: %w", err)
	}

	_, err = s.DB.Exec("INSERT INTO acceptance (tree, spec, task_id, seq, criterion, verify_cmd) VALUES (?, ?, ?, ?, ?, ?)",
		tree, spec, taskID, maxSeq+1, criterion, verify)
	if err != nil {
		return fmt.Errorf("adding acceptance criterion: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): acceptance on %s", tree, spec, taskID)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"task": taskID, "seq": maxSeq + 1, "criterion": criterion})
}
