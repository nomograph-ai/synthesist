package main

import (
	"fmt"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdPatternRegister(c *PatternRegisterCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree
	patternID := c.ID

	var transferPtr *string
	if c.Transferability != "" {
		transferPtr = &c.Transferability
	}

	_, err = s.DB.Exec(
		"INSERT INTO patterns (tree, id, name, description, transferability, first_observed) VALUES (?, ?, ?, ?, ?, ?)",
		tree, patternID, c.Name, c.Description, transferPtr, store.Today(),
	)
	if err != nil {
		return fmt.Errorf("inserting pattern: %w", err)
	}

	if c.ObservedIn != "" {
		for _, obs := range strings.Split(c.ObservedIn, ",") {
			if _, err := s.DB.Exec("INSERT INTO pattern_observations (tree, pattern_id, observed_in) VALUES (?, ?, ?)",
				tree, patternID, strings.TrimSpace(obs)); err != nil {
				return fmt.Errorf("inserting pattern observation: %w", err)
			}
		}
	}

	if err := s.Commit(fmt.Sprintf("pattern(%s): register %s -- %s", tree, patternID, c.Name)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"tree": tree, "id": patternID, "name": c.Name})
}

func cmdPatternList(c *PatternListCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree
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
		if err := obsRows.Err(); err != nil {
			return fmt.Errorf("iterating rows: %w", err)
		}
		_ = obsRows.Close()
		if len(obs) > 0 {
			p["observed_in"] = obs
		}
		patterns = append(patterns, p)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	return jsonOut(map[string]any{"tree": tree, "patterns": patterns})
}
