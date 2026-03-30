package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdDiscoveryAdd(c *DiscoveryAddCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}

	finding := c.Finding
	date := c.Date
	if date == "" {
		date = store.Today()
	}

	var ids []string
	rows, err := s.DB.Query("SELECT id FROM discoveries WHERE tree = ? AND spec = ?", tree, spec)
	if err != nil {
		return fmt.Errorf("querying discovery IDs: %w", err)
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
	newID := store.NextID("f", ids)

	var impactPtr, actionPtr, authorPtr *string
	if c.Impact != "" {
		impactPtr = &c.Impact
	}
	if c.Action != "" {
		actionPtr = &c.Action
	}
	if c.Author != "" {
		authorPtr = &c.Author
	}

	_, err = s.DB.Exec(
		"INSERT INTO discoveries (tree, spec, id, date, author, finding, impact, action_taken) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
		tree, spec, newID, date, authorPtr, finding, impactPtr, actionPtr)
	if err != nil {
		return fmt.Errorf("adding discovery: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("discovery(%s/%s): %s", tree, spec, newID)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"id": newID, "tree": tree, "spec": spec, "finding": finding, "date": date})
}

func cmdDiscoveryList(c *DiscoveryListCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}

	rows, err := s.DB.Query(
		"SELECT id, date, author, finding, impact, action_taken FROM discoveries WHERE tree = ? AND spec = ? ORDER BY date DESC, id DESC",
		tree, spec)
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	discoveries := make([]map[string]any, 0)
	for rows.Next() {
		var id, date, finding string
		var author, impact, action *string
		if err := rows.Scan(&id, &date, &author, &finding, &impact, &action); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		d := map[string]any{"id": id, "date": date, "finding": finding}
		if author != nil {
			d["author"] = *author
		}
		if impact != nil {
			d["impact"] = *impact
		}
		if action != nil {
			d["action"] = *action
		}
		discoveries = append(discoveries, d)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "discoveries": discoveries})
}
