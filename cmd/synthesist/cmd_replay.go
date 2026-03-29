package main

import "fmt"

func cmdReplay(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist replay <tree/spec>")
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

	result := map[string]any{"tree": tree, "spec": spec}

	// Task DAG shape
	rows, _ := s.DB.Query(
		"SELECT id, type, summary, status, arc FROM tasks WHERE tree = ? AND spec = ? ORDER BY id", tree, spec)
	var tasks []map[string]any
	for rows.Next() {
		var id, typ, summary, status string
		var arc *string
		if err := rows.Scan(&id, &typ, &summary, &status, &arc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		t := map[string]any{"id": id, "type": typ, "summary": summary, "status": status}
		if arc != nil {
			t["arc"] = *arc
		}
		// Deps
		depRows, _ := s.DB.Query("SELECT depends_on FROM task_deps WHERE tree = ? AND spec = ? AND task_id = ?", tree, spec, id)
		var deps []string
		for depRows.Next() {
			var d string
			if err := depRows.Scan(&d); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}
			deps = append(deps, d)
		}
		_ = depRows.Close()
		if len(deps) > 0 {
			t["depends_on"] = deps
		}
		tasks = append(tasks, t)
	}
	_ = rows.Close()
	result["task_dag"] = tasks

	// Retro transforms
	tRows, _ := s.DB.Query(
		"SELECT label, description, transferable FROM transforms WHERE tree = ? AND spec = ? AND task_id = 'retro' ORDER BY seq",
		tree, spec,
	)
	var transforms []map[string]any
	for tRows.Next() {
		var label, desc string
		var transferable bool
		if err := tRows.Scan(&label, &desc, &transferable); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		transforms = append(transforms, map[string]any{
			"label": label, "description": desc, "transferable": transferable,
		})
	}
	_ = tRows.Close()
	result["transforms"] = transforms

	// Patterns
	rows, _ = s.DB.Query(
		"SELECT tp.pattern_id, p.name, p.description FROM task_patterns tp JOIN patterns p ON tp.tree = p.tree AND tp.pattern_id = p.id WHERE tp.tree = ? AND tp.spec = ? AND tp.task_id = 'retro'",
		tree, spec,
	)
	var patterns []map[string]any
	for rows.Next() {
		var id, name, desc string
		if err := rows.Scan(&id, &name, &desc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		patterns = append(patterns, map[string]any{"id": id, "name": name, "description": desc})
	}
	_ = rows.Close()
	result["patterns"] = patterns

	// Landscape summary
	rows, _ = s.DB.Query(
		"SELECT d.stakeholder_id, d.topic, d.stance, d.confidence, d.preferred_approach FROM dispositions d WHERE d.tree = ? AND d.spec = ? AND d.valid_until IS NULL",
		tree, spec,
	)
	var landscape []map[string]any
	for rows.Next() {
		var stakeholder, topic, stance, confidence string
		var preferred *string
		if err := rows.Scan(&stakeholder, &topic, &stance, &confidence, &preferred); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		l := map[string]any{"stakeholder": stakeholder, "topic": topic, "stance": stance, "confidence": confidence}
		if preferred != nil {
			l["preferred_approach"] = *preferred
		}
		landscape = append(landscape, l)
	}
	_ = rows.Close()
	result["landscape"] = landscape

	return jsonOut(result)
}
