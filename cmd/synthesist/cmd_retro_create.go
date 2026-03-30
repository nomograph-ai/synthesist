package main

import (
	"fmt"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdRetroCreate(c *RetroCreateCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}

	arc := c.Arc
	var dependsOn []string
	if c.DependsOn != "" {
		dependsOn = strings.Split(c.DependsOn, ",")
	}

	today := store.Today()

	// Compute duration if possible
	var createdDate string
	if err := s.DB.QueryRow("SELECT MIN(created) FROM tasks WHERE tree = ? AND spec = ? AND type = 'task'",
		tree, spec).Scan(&createdDate); err != nil {
		return fmt.Errorf("scanning created date: %w", err)
	}

	_, err = s.DB.Exec(
		"INSERT INTO tasks (tree, spec, id, type, summary, status, created, completed, arc) VALUES (?, ?, 'retro', 'retro', ?, 'done', ?, ?, ?)",
		tree, spec, "Retrospective: "+spec, today, today, arc,
	)
	if err != nil {
		return fmt.Errorf("inserting retro: %w", err)
	}

	for _, dep := range dependsOn {
		if _, err := s.DB.Exec("INSERT INTO task_deps (tree, spec, task_id, depends_on) VALUES (?, ?, 'retro', ?)",
			tree, spec, strings.TrimSpace(dep)); err != nil {
			return fmt.Errorf("inserting retro dep: %w", err)
		}
	}

	if err := s.Commit(fmt.Sprintf("retro(%s/%s): create retrospective", tree, spec)); err != nil {
		return err
	}
	return jsonOut(map[string]any{
		"id": "retro", "type": "retro", "tree": tree, "spec": spec,
		"arc": arc, "status": "done",
		"next": "use 'synthesist retro transform' to add transforms, then 'synthesist pattern register' for reusable patterns",
	})
}

func cmdRetroTransform(c *RetroTransformCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}

	label := c.Label
	description := c.Description
	transferable := c.Transferable

	// Get next seq
	var maxSeq int
	if err := s.DB.QueryRow("SELECT COALESCE(MAX(seq), 0) FROM transforms WHERE tree = ? AND spec = ? AND task_id = 'retro'",
		tree, spec).Scan(&maxSeq); err != nil {
		return fmt.Errorf("scanning max seq: %w", err)
	}

	_, err = s.DB.Exec(
		"INSERT INTO transforms (tree, spec, task_id, seq, label, description, transferable) VALUES (?, ?, 'retro', ?, ?, ?, ?)",
		tree, spec, maxSeq+1, label, description, transferable,
	)
	if err != nil {
		return fmt.Errorf("inserting transform: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("retro(%s/%s): transform -- %s", tree, spec, label)); err != nil {
		return err
	}
	return jsonOut(map[string]any{
		"seq": maxSeq + 1, "label": label, "transferable": transferable,
	})
}

func cmdRetroShow(c *RetroShowCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}

	var arc string
	var created, completed *string
	err = s.DB.QueryRow("SELECT arc, created, completed FROM tasks WHERE tree = ? AND spec = ? AND id = 'retro'",
		tree, spec).Scan(&arc, &created, &completed)
	if err != nil {
		return fmt.Errorf("no retro found for %s/%s", tree, spec)
	}

	result := map[string]any{"tree": tree, "spec": spec, "arc": arc}
	if created != nil {
		result["created"] = *created
	}
	if completed != nil {
		result["completed"] = *completed
	}

	// Transforms
	rows, err := s.DB.Query(
		"SELECT seq, label, description, transferable FROM transforms WHERE tree = ? AND spec = ? AND task_id = 'retro' ORDER BY seq",
		tree, spec,
	)
	if err != nil {
		return fmt.Errorf("querying transforms: %w", err)
	}
	var transforms []map[string]any
	for rows.Next() {
		var seq int
		var label, desc string
		var transferable bool
		if err := rows.Scan(&seq, &label, &desc, &transferable); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		transforms = append(transforms, map[string]any{
			"seq": seq, "label": label, "description": desc, "transferable": transferable,
		})
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()
	result["transforms"] = transforms

	// Linked patterns
	rows, err = s.DB.Query(
		"SELECT tp.pattern_id, p.name, p.description FROM task_patterns tp JOIN patterns p ON tp.pattern_id = p.id AND tp.tree = p.tree WHERE tp.tree = ? AND tp.spec = ? AND tp.task_id = 'retro'",
		tree, spec,
	)
	if err != nil {
		return fmt.Errorf("querying patterns: %w", err)
	}
	var patterns []map[string]any
	for rows.Next() {
		var id, name, desc string
		if err := rows.Scan(&id, &name, &desc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		patterns = append(patterns, map[string]any{"id": id, "name": name, "description": desc})
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()
	result["patterns"] = patterns

	return jsonOut(result)
}
