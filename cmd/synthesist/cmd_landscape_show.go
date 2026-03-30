package main

import "fmt"

func cmdLandscapeShow(c *LandscapeShowCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}

	result := map[string]any{"tree": tree, "spec": spec}

	// Current dispositions — includes both spec-specific AND tree-wide
	// architectural dispositions from the 'stakeholder-preferences' pseudo-spec.
	rows, err := s.DB.Query(
		"SELECT d.id, d.stakeholder_id, s.context, d.topic, d.stance, d.preferred_approach, d.detail, d.confidence, d.valid_from, d.spec FROM dispositions d JOIN stakeholders s ON d.tree = s.tree AND d.stakeholder_id = s.id WHERE d.tree = ? AND (d.spec = ? OR d.spec = 'stakeholder-preferences') AND d.valid_until IS NULL ORDER BY d.stakeholder_id",
		tree, spec,
	)
	if err != nil {
		return fmt.Errorf("querying dispositions: %w", err)
	}
	dispositions := make([]map[string]any, 0)
	for rows.Next() {
		var id, stakeholder, context, topic, stance, confidence, validFrom, fromSpec string
		var preferred, detail *string
		if err := rows.Scan(&id, &stakeholder, &context, &topic, &stance, &preferred, &detail, &confidence, &validFrom, &fromSpec); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		d := map[string]any{
			"id": id, "stakeholder": stakeholder, "stakeholder_context": context,
			"topic": topic, "stance": stance, "confidence": confidence, "valid_from": validFrom,
		}
		if preferred != nil {
			d["preferred_approach"] = *preferred
		}
		if detail != nil {
			d["detail"] = *detail
		}
		if fromSpec != spec {
			d["scope"] = "tree-wide"
		}
		dispositions = append(dispositions, d)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()
	result["dispositions"] = dispositions

	// Signals
	rows, err = s.DB.Query(
		"SELECT sig.id, sig.stakeholder_id, sh.context, sig.date, sig.source, sig.source_type, sig.content FROM signals sig JOIN stakeholders sh ON sig.tree = sh.tree AND sig.stakeholder_id = sh.id WHERE sig.tree = ? AND sig.spec = ? ORDER BY sig.date DESC LIMIT 20",
		tree, spec,
	)
	if err != nil {
		return fmt.Errorf("querying signals: %w", err)
	}
	signals := make([]map[string]any, 0)
	for rows.Next() {
		var id, stakeholder, context, date, source, sourceType, content string
		if err := rows.Scan(&id, &stakeholder, &context, &date, &source, &sourceType, &content); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		signals = append(signals, map[string]any{
			"id": id, "stakeholder": stakeholder, "stakeholder_context": context,
			"date": date, "source": source, "source_type": sourceType, "content": content,
		})
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()
	result["signals"] = signals

	// Influences
	rows, err = s.DB.Query(
		"SELECT i.stakeholder_id, s.context, i.task_id, i.role FROM influences i JOIN stakeholders s ON i.tree = s.tree AND i.stakeholder_id = s.id WHERE i.tree = ? AND i.spec = ?",
		tree, spec,
	)
	if err != nil {
		return fmt.Errorf("querying influences: %w", err)
	}
	influences := make([]map[string]any, 0)
	for rows.Next() {
		var stakeholder, context, taskID, role string
		if err := rows.Scan(&stakeholder, &context, &taskID, &role); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		influences = append(influences, map[string]any{
			"stakeholder": stakeholder, "context": context, "task": taskID, "role": role,
		})
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()
	result["influences"] = influences

	// Directions affecting this spec
	rows, err = s.DB.Query(`
		SELECT d.id, d.project, d.topic, d.status, d.impact, di.description
		FROM directions d
		JOIN direction_impacts di ON d.tree = di.tree AND d.id = di.direction_id
		WHERE di.affected_tree = ? AND di.affected_spec = ? AND d.valid_until IS NULL
	`, tree, spec)
	if err != nil {
		return fmt.Errorf("querying directions: %w", err)
	}
	directions := make([]map[string]any, 0)
	for rows.Next() {
		var id, project, topic, status, impact, desc string
		if err := rows.Scan(&id, &project, &topic, &status, &impact, &desc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		directions = append(directions, map[string]any{
			"id": id, "project": project, "topic": topic,
			"status": status, "impact": impact, "impact_on_spec": desc,
		})
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()
	result["directions"] = directions

	return jsonOut(result)
}
