package main

import (
	"fmt"
	"strings"
)

func cmdStakeholderAdd(c *StakeholderAddCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree
	id := c.ID
	context := c.Context

	var namePtr *string
	if c.Name != "" {
		namePtr = &c.Name
	}

	_, err = s.DB.Exec("INSERT IGNORE INTO stakeholders (tree, id, name, context) VALUES (?, ?, ?, ?)",
		tree, id, namePtr, context)
	if err != nil {
		return fmt.Errorf("inserting stakeholder: %w", err)
	}

	if c.Orgs != "" {
		for _, org := range strings.Split(c.Orgs, ",") {
			if _, err := s.DB.Exec("INSERT IGNORE INTO stakeholder_orgs (tree, stakeholder_id, org) VALUES (?, ?, ?)",
				tree, id, strings.TrimSpace(org)); err != nil {
				return fmt.Errorf("inserting stakeholder org: %w", err)
			}
		}
	}

	if err := s.Commit(fmt.Sprintf("landscape(%s): add stakeholder %s", tree, id)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"tree": tree, "id": id, "context": context})
}

func cmdStakeholderList(c *StakeholderListCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree
	rows, err := s.DB.Query("SELECT id, name, context FROM stakeholders WHERE tree = ? ORDER BY id", tree)
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	var stakeholders []map[string]any
	for rows.Next() {
		var id, context string
		var name *string
		if err := rows.Scan(&id, &name, &context); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		sh := map[string]any{"id": id, "context": context}
		if name != nil {
			sh["name"] = *name
		}
		// Get orgs
		orgRows, _ := s.DB.Query("SELECT org FROM stakeholder_orgs WHERE tree = ? AND stakeholder_id = ?", tree, id)
		var orgs []string
		for orgRows.Next() {
			var org string
			if err := orgRows.Scan(&org); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}
			orgs = append(orgs, org)
		}
		_ = orgRows.Close()
		if len(orgs) > 0 {
			sh["orgs"] = orgs
		}
		stakeholders = append(stakeholders, sh)
	}
	return jsonOut(map[string]any{"tree": tree, "stakeholders": stakeholders})
}
