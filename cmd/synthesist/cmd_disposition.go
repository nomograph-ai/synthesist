package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdDisposition(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist disposition <add|list|supersede> ...") //nolint:staticcheck
	}
	switch args[0] {
	case "add":
		return cmdDispositionAdd(args[1:])
	case "list":
		return cmdDispositionList(args[1:])
	case "supersede":
		return cmdDispositionSupersede(args[1:])
	default:
		return fmt.Errorf("unknown disposition subcommand: %s", args[0])
	}
}

func cmdDispositionAdd(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist disposition add <tree/spec> <stakeholder> --topic '...' --stance cautious --confidence inferred [--preferred '...']")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	stakeholderID := args[1]

	var topic, stance, confidence, preferred string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--topic":
			topic = args[i+1]
		case "--stance":
			stance = args[i+1]
		case "--confidence":
			confidence = args[i+1]
		case "--preferred":
			preferred = args[i+1]
		}
	}
	if topic == "" || stance == "" || confidence == "" {
		return fmt.Errorf("--topic, --stance, and --confidence are required")
	}

	// Get next ID
	var ids []string
	rows, _ := s.DB.Query("SELECT id FROM dispositions WHERE tree = ? AND spec = ?", tree, spec)
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		ids = append(ids, id)
	}
	_ = rows.Close()
	newID := store.NextID("d", ids)

	var preferredPtr *string
	if preferred != "" {
		preferredPtr = &preferred
	}

	_, err = s.DB.Exec(
		"INSERT INTO dispositions (tree, spec, id, stakeholder_id, topic, stance, preferred_approach, confidence, valid_from) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
		tree, spec, newID, stakeholderID, topic, stance, preferredPtr, confidence, store.Today(),
	)
	if err != nil {
		return fmt.Errorf("inserting disposition: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("landscape(%s/%s): disposition %s -- %s is %s on %s", tree, spec, newID, stakeholderID, stance, topic)); err != nil {
		return err
	}
	return jsonOut(map[string]any{
		"id": newID, "stakeholder": stakeholderID, "topic": topic,
		"stance": stance, "confidence": confidence,
	})
}

func cmdDispositionList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist disposition list <tree/spec>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}

	rows, err := s.DB.Query(
		"SELECT id, stakeholder_id, topic, stance, preferred_approach, confidence, valid_from, valid_until FROM dispositions WHERE tree = ? AND spec = ? ORDER BY valid_from DESC",
		tree, spec,
	)
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	var dispositions []map[string]any
	for rows.Next() {
		var id, stakeholder, topic, stance, confidence, validFrom string
		var preferred, validUntil *string
		if err := rows.Scan(&id, &stakeholder, &topic, &stance, &preferred, &confidence, &validFrom, &validUntil); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		d := map[string]any{
			"id": id, "stakeholder": stakeholder, "topic": topic,
			"stance": stance, "confidence": confidence, "valid_from": validFrom,
			"current": validUntil == nil,
		}
		if preferred != nil {
			d["preferred_approach"] = *preferred
		}
		if validUntil != nil {
			d["valid_until"] = *validUntil
		}
		dispositions = append(dispositions, d)
	}
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "dispositions": dispositions})
}

func cmdDispositionSupersede(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist disposition supersede <tree/spec> <disposition-id> --new-stance supportive [--evidence s1] [--preferred '...']")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	oldID := args[1]

	var newStance, preferred, evidence string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--new-stance":
			newStance = args[i+1]
		case "--preferred":
			preferred = args[i+1]
		case "--evidence":
			evidence = args[i+1]
		}
	}
	if newStance == "" {
		return fmt.Errorf("--new-stance is required")
	}

	// Read old disposition
	var stakeholder, topic, confidence string
	var oldPreferred *string
	err = s.DB.QueryRow(
		"SELECT stakeholder_id, topic, confidence, preferred_approach FROM dispositions WHERE tree = ? AND spec = ? AND id = ?",
		tree, spec, oldID,
	).Scan(&stakeholder, &topic, &confidence, &oldPreferred)
	if err != nil {
		return fmt.Errorf("disposition %s not found", oldID)
	}

	// Generate new ID
	var ids []string
	rows, _ := s.DB.Query("SELECT id FROM dispositions WHERE tree = ? AND spec = ?", tree, spec)
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		ids = append(ids, id)
	}
	_ = rows.Close()
	newID := store.NextID("d", ids)

	today := store.Today()

	// Supersede old
	if _, err := s.DB.Exec("UPDATE dispositions SET valid_until = ?, superseded_by = ? WHERE tree = ? AND spec = ? AND id = ?",
		today, newID, tree, spec, oldID); err != nil {
		return fmt.Errorf("superseding disposition: %w", err)
	}

	// Insert new
	var preferredPtr *string
	if preferred != "" {
		preferredPtr = &preferred
	} else {
		preferredPtr = oldPreferred
	}

	var evidencePtr *string
	if evidence != "" {
		evidencePtr = &evidence
	}

	if _, err := s.DB.Exec(
		"INSERT INTO dispositions (tree, spec, id, stakeholder_id, topic, stance, preferred_approach, confidence, evidence, valid_from) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
		tree, spec, newID, stakeholder, topic, newStance, preferredPtr, confidence, evidencePtr, today,
	); err != nil {
		return fmt.Errorf("inserting new disposition: %w", err)
	}

	commitMsg := fmt.Sprintf("landscape(%s/%s): supersede %s -> %s (%s now %s on %s)", tree, spec, oldID, newID, stakeholder, newStance, topic)
	if evidence != "" {
		commitMsg += fmt.Sprintf(" [evidence: %s]", evidence)
	}
	if err := s.Commit(commitMsg); err != nil {
		return err
	}

	out := map[string]any{
		"old_id": oldID, "new_id": newID, "stakeholder": stakeholder,
		"topic": topic, "old_stance": "superseded", "new_stance": newStance,
	}
	if evidence != "" {
		out["evidence_signal"] = evidence
	}
	return jsonOut(out)
}
