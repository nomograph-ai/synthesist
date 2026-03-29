package main

import (
	"fmt"
)

func cmdTreeCreate(c *TreeCreateCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	name := c.Name
	description := c.Description
	status := c.Status

	_, err = s.DB.Exec("INSERT IGNORE INTO trees (name, path, status, description) VALUES (?, ?, ?, ?)",
		name, "specs/"+name+"/campaign.json", status, description)
	if err != nil {
		return fmt.Errorf("creating tree: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("estate: create tree %s", name)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"name": name, "status": status, "description": description})
}

func cmdTreeList() error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	rows, err := s.DB.Query("SELECT name, status, description FROM trees ORDER BY name")
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	trees := make([]map[string]any, 0)
	for rows.Next() {
		var name, status, desc string
		if err := rows.Scan(&name, &status, &desc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		trees = append(trees, map[string]any{"name": name, "status": status, "description": desc})
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	return jsonOut(map[string]any{"trees": trees})
}
