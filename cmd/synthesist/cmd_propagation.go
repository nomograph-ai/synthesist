package main

import (
	"fmt"
	"strconv"
)

func cmdPropagation(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist propagation <add|list|check> ...") //nolint:staticcheck
	}
	switch args[0] {
	case "add":
		return cmdPropagationAdd(args[1:])
	case "list":
		return cmdPropagationList(args[1:])
	case "check":
		return cmdPropagationCheck(args[1:])
	default:
		return fmt.Errorf("unknown propagation subcommand: %s", args[0])
	}
}

func cmdPropagationAdd(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist propagation add <source-tree/spec> <target-tree/spec> --seq N [--description '...']")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	sourceTree, sourceSpec, err := parseTreeSpec(args[0])
	if err != nil {
		return fmt.Errorf("source: %w", err)
	}
	targetTree, targetSpec, err := parseTreeSpec(args[1])
	if err != nil {
		return fmt.Errorf("target: %w", err)
	}

	seq := 0
	var description string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--seq":
			seq, _ = strconv.Atoi(args[i+1])
		case "--description":
			description = args[i+1]
		}
	}

	var descPtr *string
	if description != "" {
		descPtr = &description
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

func cmdPropagationList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist propagation list <tree/spec>")
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
func cmdPropagationCheck(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist propagation check <tree/spec>")
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
	_ = rows.Close()

	return jsonOut(map[string]any{
		"spec":          tree + "/" + spec,
		"stale_targets": stale,
		"count":         len(stale),
	})
}
