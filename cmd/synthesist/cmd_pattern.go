package main

import (
	"fmt"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdPattern(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist pattern <register|list> ...") //nolint:staticcheck
	}
	switch args[0] {
	case "register":
		return cmdPatternRegister(args[1:])
	case "list":
		return cmdPatternList(args[1:])
	default:
		return fmt.Errorf("unknown pattern subcommand: %s", args[0])
	}
}

func cmdPatternRegister(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist pattern register <tree> <id> --name '...' --description '...' [--transferability '...'] [--observed-in spec1,spec2]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := args[0]
	patternID := args[1]

	var name, description, transferability string
	var observedIn []string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--name":
			name = args[i+1]
		case "--description":
			description = args[i+1]
		case "--transferability":
			transferability = args[i+1]
		case "--observed-in":
			observedIn = strings.Split(args[i+1], ",")
		}
	}
	if name == "" || description == "" {
		return fmt.Errorf("--name and --description are required")
	}

	var transferPtr *string
	if transferability != "" {
		transferPtr = &transferability
	}

	_, err = s.DB.Exec(
		"INSERT INTO patterns (tree, id, name, description, transferability, first_observed) VALUES (?, ?, ?, ?, ?, ?)",
		tree, patternID, name, description, transferPtr, store.Today(),
	)
	if err != nil {
		return fmt.Errorf("inserting pattern: %w", err)
	}

	for _, obs := range observedIn {
		if _, err := s.DB.Exec("INSERT INTO pattern_observations (tree, pattern_id, observed_in) VALUES (?, ?, ?)",
			tree, patternID, strings.TrimSpace(obs)); err != nil {
			return fmt.Errorf("inserting pattern observation: %w", err)
		}
	}

	if err := s.Commit(fmt.Sprintf("pattern(%s): register %s -- %s", tree, patternID, name)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"tree": tree, "id": patternID, "name": name})
}

func cmdPatternList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist pattern list <tree>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := args[0]
	rows, err := s.DB.Query("SELECT id, name, description, transferability, first_observed FROM patterns WHERE tree = ? ORDER BY first_observed DESC", tree)
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	var patterns []map[string]any
	for rows.Next() {
		var id, name, desc, firstObs string
		var transferability *string
		if err := rows.Scan(&id, &name, &desc, &transferability, &firstObs); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		p := map[string]any{"id": id, "name": name, "description": desc, "first_observed": firstObs}
		if transferability != nil {
			p["transferability"] = *transferability
		}
		// Get observations
		obsRows, _ := s.DB.Query("SELECT observed_in FROM pattern_observations WHERE tree = ? AND pattern_id = ?", tree, id)
		var obs []string
		for obsRows.Next() {
			var o string
			if err := obsRows.Scan(&o); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}
			obs = append(obs, o)
		}
		_ = obsRows.Close()
		if len(obs) > 0 {
			p["observed_in"] = obs
		}
		patterns = append(patterns, p)
	}
	return jsonOut(map[string]any{"tree": tree, "patterns": patterns})
}
