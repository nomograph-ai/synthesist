package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdDiscovery(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist discovery <add|list> ...")
	}
	switch args[0] {
	case "add":
		return cmdDiscoveryAdd(args[1:])
	case "list":
		return cmdDiscoveryList(args[1:])
	default:
		return fmt.Errorf("unknown discovery subcommand: %s", args[0])
	}
}

func cmdDiscoveryAdd(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist discovery add <tree/spec> --finding '...' [--impact '...'] [--action '...'] [--author agent] [--date YYYY-MM-DD]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}

	var finding, impact, action, author, date string
	for i := 1; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--finding":
			finding = args[i+1]
		case "--impact":
			impact = args[i+1]
		case "--action":
			action = args[i+1]
		case "--author":
			author = args[i+1]
		case "--date":
			date = args[i+1]
		}
	}
	if finding == "" {
		return fmt.Errorf("--finding is required")
	}
	if date == "" {
		date = store.Today()
	}

	var ids []string
	rows, _ := s.DB.Query("SELECT id FROM discoveries WHERE tree = ? AND spec = ?", tree, spec)
	for rows.Next() {
		var id string
		rows.Scan(&id)
		ids = append(ids, id)
	}
	rows.Close()
	newID := store.NextID("f", ids)

	var impactPtr, actionPtr, authorPtr *string
	if impact != "" {
		impactPtr = &impact
	}
	if action != "" {
		actionPtr = &action
	}
	if author != "" {
		authorPtr = &author
	}

	_, err = s.DB.Exec(
		"INSERT INTO discoveries (tree, spec, id, date, author, finding, impact, action_taken) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
		tree, spec, newID, date, authorPtr, finding, impactPtr, actionPtr)
	if err != nil {
		return fmt.Errorf("adding discovery: %w", err)
	}

	s.Commit(fmt.Sprintf("discovery(%s/%s): %s", tree, spec, newID))
	return jsonOut(map[string]any{"id": newID, "tree": tree, "spec": spec, "finding": finding, "date": date})
}

func cmdDiscoveryList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist discovery list <tree/spec>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}

	rows, err := s.DB.Query(
		"SELECT id, date, author, finding, impact, action_taken FROM discoveries WHERE tree = ? AND spec = ? ORDER BY date DESC, id DESC",
		tree, spec)
	if err != nil {
		return err
	}
	defer rows.Close()

	discoveries := make([]map[string]any, 0)
	for rows.Next() {
		var id, date, finding string
		var author, impact, action *string
		rows.Scan(&id, &date, &author, &finding, &impact, &action)
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
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "discoveries": discoveries})
}
