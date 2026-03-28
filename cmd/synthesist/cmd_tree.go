package main

import (
	"fmt"
)

func cmdTree(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist tree <create|list> ...")
	}
	switch args[0] {
	case "create":
		return cmdTreeCreate(args[1:])
	case "list":
		return cmdTreeList(args[1:])
	default:
		return fmt.Errorf("unknown tree subcommand: %s", args[0])
	}
}

func cmdTreeCreate(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist tree create <name> [--description '...'] [--status active]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	name := args[0]
	description := ""
	status := "active"
	for i := 1; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--description":
			description = args[i+1]
		case "--status":
			status = args[i+1]
		}
	}

	_, err = s.DB.Exec("INSERT IGNORE INTO trees (name, path, status, description) VALUES (?, ?, ?, ?)",
		name, "specs/"+name+"/campaign.json", status, description)
	if err != nil {
		return fmt.Errorf("creating tree: %w", err)
	}

	s.Commit(fmt.Sprintf("estate: create tree %s", name))
	return jsonOut(map[string]any{"name": name, "status": status, "description": description})
}

func cmdTreeList(args []string) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	rows, err := s.DB.Query("SELECT name, status, description FROM trees ORDER BY name")
	if err != nil {
		return err
	}
	defer rows.Close()

	trees := make([]map[string]any, 0)
	for rows.Next() {
		var name, status, desc string
		rows.Scan(&name, &status, &desc)
		trees = append(trees, map[string]any{"name": name, "status": status, "description": desc})
	}
	return jsonOut(map[string]any{"trees": trees})
}
