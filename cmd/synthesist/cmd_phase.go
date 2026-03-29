package main

import "fmt"

// validPhases enumerates the workflow state machine phases.
var validPhases = map[string]bool{
	"orient":  true,
	"plan":    true,
	"agree":   true,
	"execute": true,
	"reflect": true,
	"replan":  true,
	"report":  true,
}

func cmdPhaseSet(c *PhaseSetCmd) error {
	if !validPhases[c.Name] {
		return fmt.Errorf("invalid phase %q: must be one of orient, plan, agree, execute, reflect, replan, report", c.Name)
	}

	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	// Upsert the single phase row.
	_, err = s.DB.Exec(
		"REPLACE INTO phase (id, name, updated) VALUES (1, ?, CURRENT_TIMESTAMP)",
		c.Name,
	)
	if err != nil {
		return Wrap("setting phase", err)
	}

	if err := s.Commit(fmt.Sprintf("phase: set %s", c.Name)); err != nil {
		return err
	}

	return jsonOut(map[string]any{"phase": c.Name})
}

func cmdPhaseShow() error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	var name string
	err = s.DB.QueryRow("SELECT name FROM phase WHERE id = 1").Scan(&name)
	if err != nil {
		// No phase set yet — default to orient.
		name = "orient"
	}

	return jsonOut(map[string]any{"phase": name})
}
