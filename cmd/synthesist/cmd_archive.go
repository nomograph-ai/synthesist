package main

import (
	"fmt"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdArchive(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist archive <add|list> ...")
	}
	switch args[0] {
	case "add":
		return cmdArchiveAdd(args[1:])
	case "list":
		return cmdArchiveList(args[1:])
	default:
		return fmt.Errorf("unknown archive subcommand: %s", args[0])
	}
}

func cmdArchiveAdd(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist archive add <tree/spec> --reason completed [--outcome '...'] [--archived YYYY-MM-DD]")
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

	var reason, outcome, archived string
	var patterns []string
	for i := 1; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--reason":
			reason = args[i+1]
		case "--outcome":
			outcome = args[i+1]
		case "--archived":
			archived = args[i+1]
		case "--patterns":
			patterns = strings.Split(args[i+1], ",")
		}
	}
	if reason == "" {
		return fmt.Errorf("--reason is required (completed, abandoned, superseded, deferred)")
	}
	if archived == "" {
		archived = store.Today()
	}

	var outcomePtr *string
	if outcome != "" {
		outcomePtr = &outcome
	}

	_, err = s.DB.Exec("INSERT INTO archives (tree, spec_id, archived, reason, outcome) VALUES (?, ?, ?, ?, ?)",
		tree, spec, archived, reason, outcomePtr)
	if err != nil {
		return fmt.Errorf("archiving: %w", err)
	}

	for _, p := range patterns {
		s.DB.Exec("INSERT INTO archive_patterns (tree, spec_id, pattern_id) VALUES (?, ?, ?)",
			tree, spec, strings.TrimSpace(p))
	}

	s.Commit(fmt.Sprintf("archive(%s/%s): %s", tree, spec, reason))
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "reason": reason, "archived": archived})
}

func cmdArchiveList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist archive list <tree>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree := args[0]
	rows, err := s.DB.Query("SELECT spec_id, archived, reason, outcome FROM archives WHERE tree = ? ORDER BY archived DESC", tree)
	if err != nil {
		return err
	}
	defer rows.Close()

	archives := make([]map[string]any, 0)
	for rows.Next() {
		var specID, archived, reason string
		var outcome *string
		rows.Scan(&specID, &archived, &reason, &outcome)
		a := map[string]any{"spec_id": specID, "archived": archived, "reason": reason}
		if outcome != nil {
			a["outcome"] = *outcome
		}
		archives = append(archives, a)
	}
	return jsonOut(map[string]any{"tree": tree, "archives": archives})
}
