package main

import (
	"fmt"
)

func cmdPropagationAdd(c *PropagationAddCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	sourceTree, sourceSpec, err := parseTreeSpec(c.Source)
	if err != nil {
		return fmt.Errorf("source: %w", err)
	}
	targetTree, targetSpec, err := parseTreeSpec(c.Target)
	if err != nil {
		return fmt.Errorf("target: %w", err)
	}

	seq := c.Seq

	var descPtr *string
	if c.Description != "" {
		descPtr = &c.Description
	}

	_, err = s.DB.Exec(
		"INSERT INTO propagation_chain (source_tree, source_spec, target_tree, target_spec, seq, description) VALUES (?, ?, ?, ?, ?, ?)",
		sourceTree, sourceSpec, targetTree, targetSpec, seq, descPtr,
	)
	if err != nil {
		return fmt.Errorf("adding propagation link: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("propagation: %s/%s -> %s/%s (seq %d)", sourceTree, sourceSpec, targetTree, targetSpec, seq)); err != nil {
		return err
	}
	return jsonOut(map[string]any{
		"source": sourceTree + "/" + sourceSpec,
		"target": targetTree + "/" + targetSpec,
		"seq":    seq,
	})
}

func cmdPropagationList(c *PropagationListCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}

	// Downstream: what needs updates when this spec changes
	rows, _ := s.DB.Query(
		"SELECT target_tree, target_spec, seq, description FROM propagation_chain WHERE source_tree = ? AND source_spec = ? ORDER BY seq",
		tree, spec,
	)
	downstream := make([]map[string]any, 0)
	for rows.Next() {
		var targetTree, targetSpec string
		var seq int
		var desc *string
		if err := rows.Scan(&targetTree, &targetSpec, &seq, &desc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		d := map[string]any{"target": targetTree + "/" + targetSpec, "seq": seq}
		if desc != nil {
			d["description"] = *desc
		}
		downstream = append(downstream, d)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()

	// Upstream: what specs' changes affect this one
	rows, _ = s.DB.Query(
		"SELECT source_tree, source_spec, seq, description FROM propagation_chain WHERE target_tree = ? AND target_spec = ? ORDER BY seq",
		tree, spec,
	)
	upstream := make([]map[string]any, 0)
	for rows.Next() {
		var sourceTree, sourceSpec string
		var seq int
		var desc *string
		if err := rows.Scan(&sourceTree, &sourceSpec, &seq, &desc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		u := map[string]any{"source": sourceTree + "/" + sourceSpec, "seq": seq}
		if desc != nil {
			u["description"] = *desc
		}
		upstream = append(upstream, u)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()

	return jsonOut(map[string]any{
		"spec":       tree + "/" + spec,
		"downstream": downstream,
		"upstream":   upstream,
	})
}

// cmdPropagationCheck reports which downstream specs may need updates
// based on whether the source spec has tasks completed more recently
// than the target spec's last task completion.
func cmdPropagationCheck(c *PropagationCheckCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}

	// Get source's latest completion
	var sourceLastCompleted *string
	if err := s.DB.QueryRow("SELECT MAX(completed) FROM tasks WHERE tree = ? AND spec = ? AND status = 'done'",
		tree, spec).Scan(&sourceLastCompleted); err != nil {
		return fmt.Errorf("scanning source completion: %w", err)
	}

	if sourceLastCompleted == nil {
		return jsonOut(map[string]any{"spec": tree + "/" + spec, "stale_targets": []any{}, "message": "no completed tasks in source"})
	}

	// Check each downstream target
	rows, _ := s.DB.Query(
		"SELECT target_tree, target_spec, seq, description FROM propagation_chain WHERE source_tree = ? AND source_spec = ? ORDER BY seq",
		tree, spec,
	)
	var stale []map[string]any
	for rows.Next() {
		var targetTree, targetSpec string
		var seq int
		var desc *string
		if err := rows.Scan(&targetTree, &targetSpec, &seq, &desc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}

		var targetLastCompleted *string
		if err := s.DB.QueryRow("SELECT MAX(completed) FROM tasks WHERE tree = ? AND spec = ? AND status = 'done'",
			targetTree, targetSpec).Scan(&targetLastCompleted); err != nil {
			return fmt.Errorf("scanning target completion: %w", err)
		}

		if targetLastCompleted == nil || *targetLastCompleted < *sourceLastCompleted {
			entry := map[string]any{
				"target": targetTree + "/" + targetSpec,
				"seq":    seq,
				"reason": "source updated more recently than target",
			}
			if desc != nil {
				entry["description"] = *desc
			}
			if targetLastCompleted != nil {
				entry["target_last_completed"] = *targetLastCompleted
			}
			entry["source_last_completed"] = *sourceLastCompleted
			stale = append(stale, entry)
		}
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()

	return jsonOut(map[string]any{
		"spec":          tree + "/" + spec,
		"stale_targets": stale,
		"count":         len(stale),
	})
}
