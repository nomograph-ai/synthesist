package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdDirectionAdd(c *DirectionAddCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree

	// Get next ID
	var ids []string
	rows, err := s.DB.Query("SELECT id FROM directions WHERE tree = ?", tree)
	if err != nil {
		return fmt.Errorf("querying direction IDs: %w", err)
	}
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		ids = append(ids, id)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()
	newID := store.NextID("dir", ids)

	var ownerPtr, timelinePtr *string
	if c.Owner != "" {
		ownerPtr = &c.Owner
	}
	if c.Timeline != "" {
		timelinePtr = &c.Timeline
	}

	_, err = s.DB.Exec(
		"INSERT INTO directions (tree, id, project, topic, status, owner, timeline, impact, valid_from) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
		tree, newID, c.Project, c.Topic, c.Status, ownerPtr, timelinePtr, c.Impact, store.Today(),
	)
	if err != nil {
		return fmt.Errorf("inserting direction: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("landscape(%s): direction %s -- %s in %s (%s)", tree, newID, c.Topic, c.Project, c.Status)); err != nil {
		return err
	}
	return jsonOut(map[string]any{
		"id": newID, "tree": tree, "project": c.Project,
		"topic": c.Topic, "status": c.Status, "impact": c.Impact,
	})
}

func cmdDirectionList(c *DirectionListCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree
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
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	return jsonOut(map[string]any{"tree": tree, "directions": directions})
}

func cmdDirectionImpact(c *DirectionImpactCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree
	directionID := c.DirectionID

	// Verify direction exists
	var topic string
	err = s.DB.QueryRow("SELECT topic FROM directions WHERE tree = ? AND id = ?", tree, directionID).Scan(&topic)
	if err != nil {
		return fmt.Errorf("direction %s not found in tree %s", directionID, tree)
	}

	_, err = s.DB.Exec(
		"INSERT INTO direction_impacts (tree, direction_id, affected_tree, affected_spec, description) VALUES (?, ?, ?, ?, ?)",
		tree, directionID, c.AffectedTree, c.AffectedSpec, c.Description,
	)
	if err != nil {
		return fmt.Errorf("inserting direction impact: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("landscape(%s): direction %s impacts %s/%s", tree, directionID, c.AffectedTree, c.AffectedSpec)); err != nil {
		return err
	}
	return jsonOut(map[string]any{
		"direction_id": directionID, "affected_tree": c.AffectedTree,
		"affected_spec": c.AffectedSpec, "description": c.Description,
	})
}
