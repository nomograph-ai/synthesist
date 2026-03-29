package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdDirection(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist direction <add|list|impact> ...") //nolint:staticcheck
	}
	switch args[0] {
	case "add":
		return cmdDirectionAdd(args[1:])
	case "list":
		return cmdDirectionList(args[1:])
	case "impact":
		return cmdDirectionImpact(args[1:])
	default:
		return fmt.Errorf("unknown direction subcommand: %s", args[0])
	}
}

func cmdDirectionAdd(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist direction add <tree> --project \"org/repo\" --topic \"...\" --status proposed --impact \"...\" [--owner stakeholder-id] [--timeline \"...\"]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck //nolint:errcheck

	tree := args[0]

	var project, topic, status, impact, owner, timeline string
	for i := 1; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--project":
			project = args[i+1]
		case "--topic":
			topic = args[i+1]
		case "--status":
			status = args[i+1]
		case "--impact":
			impact = args[i+1]
		case "--owner":
			owner = args[i+1]
		case "--timeline":
			timeline = args[i+1]
		}
	}
	if project == "" || topic == "" || status == "" || impact == "" {
		return fmt.Errorf("--project, --topic, --status, and --impact are required")
	}

	// Get next ID
	var ids []string
	rows, _ := s.DB.Query("SELECT id FROM directions WHERE tree = ?", tree)
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		ids = append(ids, id)
	}
	_ = rows.Close()
	newID := store.NextID("dir", ids)

	var ownerPtr, timelinePtr *string
	if owner != "" {
		ownerPtr = &owner
	}
	if timeline != "" {
		timelinePtr = &timeline
	}

	_, err = s.DB.Exec(
		"INSERT INTO directions (tree, id, project, topic, status, owner, timeline, impact, valid_from) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
		tree, newID, project, topic, status, ownerPtr, timelinePtr, impact, store.Today(),
	)
	if err != nil {
		return fmt.Errorf("inserting direction: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("landscape(%s): direction %s -- %s in %s (%s)", tree, newID, topic, project, status)); err != nil {
		return err
	}
	return jsonOut(map[string]any{
		"id": newID, "tree": tree, "project": project,
		"topic": topic, "status": status, "impact": impact,
	})
}

func cmdDirectionList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist direction list <tree>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := args[0]
	rows, err := s.DB.Query(
		"SELECT id, project, topic, status, owner, timeline, impact, valid_from FROM directions WHERE tree = ? AND valid_until IS NULL ORDER BY valid_from DESC",
		tree,
	)
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	var directions []map[string]any
	for rows.Next() {
		var id, project, topic, status, impact, validFrom string
		var owner, timeline *string
		if err := rows.Scan(&id, &project, &topic, &status, &owner, &timeline, &impact, &validFrom); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		d := map[string]any{
			"id": id, "project": project, "topic": topic,
			"status": status, "impact": impact, "valid_from": validFrom,
		}
		if owner != nil {
			d["owner"] = *owner
		}
		if timeline != nil {
			d["timeline"] = *timeline
		}
		directions = append(directions, d)
	}
	return jsonOut(map[string]any{"tree": tree, "directions": directions})
}

func cmdDirectionImpact(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist direction impact <tree> <direction-id> --affected-tree \"...\" --affected-spec \"...\" --description \"...\"")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := args[0]
	directionID := args[1]

	var affectedTree, affectedSpec, description string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--affected-tree":
			affectedTree = args[i+1]
		case "--affected-spec":
			affectedSpec = args[i+1]
		case "--description":
			description = args[i+1]
		}
	}
	if affectedTree == "" || affectedSpec == "" || description == "" {
		return fmt.Errorf("--affected-tree, --affected-spec, and --description are required")
	}

	// Verify direction exists
	var topic string
	err = s.DB.QueryRow("SELECT topic FROM directions WHERE tree = ? AND id = ?", tree, directionID).Scan(&topic)
	if err != nil {
		return fmt.Errorf("direction %s not found in tree %s", directionID, tree)
	}

	_, err = s.DB.Exec(
		"INSERT INTO direction_impacts (tree, direction_id, affected_tree, affected_spec, description) VALUES (?, ?, ?, ?, ?)",
		tree, directionID, affectedTree, affectedSpec, description,
	)
	if err != nil {
		return fmt.Errorf("inserting direction impact: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("landscape(%s): direction %s impacts %s/%s", tree, directionID, affectedTree, affectedSpec)); err != nil {
		return err
	}
	return jsonOut(map[string]any{
		"direction_id": directionID, "affected_tree": affectedTree,
		"affected_spec": affectedSpec, "description": description,
	})
}
