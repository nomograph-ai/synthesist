package main

import (
	"fmt"
	"strings"
)

func cmdStakeholder(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist stakeholder <add|list> ...") //nolint:staticcheck
	}
	switch args[0] {
	case "add":
		return cmdStakeholderAdd(args[1:])
	case "list":
		return cmdStakeholderList(args[1:])
	default:
		return fmt.Errorf("unknown stakeholder subcommand: %s", args[0])
	}
}

func cmdStakeholderAdd(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist stakeholder add <tree> <id> --context 'role' [--name 'Full Name'] [--orgs 'org1,org2']")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := args[0]
	id := args[1]

	var context, name string
	var orgs []string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--context":
			context = args[i+1]
		case "--name":
			name = args[i+1]
		case "--orgs":
			orgs = strings.Split(args[i+1], ",")
		}
	}
	if context == "" {
		return fmt.Errorf("--context is required")
	}

	var namePtr *string
	if name != "" {
		namePtr = &name
	}

	_, err = s.DB.Exec("INSERT IGNORE INTO stakeholders (tree, id, name, context) VALUES (?, ?, ?, ?)",
		tree, id, namePtr, context)
	if err != nil {
		return fmt.Errorf("inserting stakeholder: %w", err)
	}

	for _, org := range orgs {
		if _, err := s.DB.Exec("INSERT IGNORE INTO stakeholder_orgs (tree, stakeholder_id, org) VALUES (?, ?, ?)",
			tree, id, strings.TrimSpace(org)); err != nil {
			return fmt.Errorf("inserting stakeholder org: %w", err)
		}
	}

	if err := s.Commit(fmt.Sprintf("landscape(%s): add stakeholder %s", tree, id)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"tree": tree, "id": id, "context": context})
}

func cmdStakeholderList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist stakeholder list <tree>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := args[0]
	rows, err := s.DB.Query("SELECT id, name, context FROM stakeholders WHERE tree = ? ORDER BY id", tree)
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck //nolint:errcheck

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
