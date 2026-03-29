package main

import "fmt"

func cmdStance(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist stance <stakeholder> [topic]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	stakeholderID := args[0]
	var topic string
	if len(args) > 1 {
		topic = args[1]
	}

	var rows interface {
		Next() bool
		Scan(...any) error
		Close() error
	}
	if topic != "" {
		// Full history for this person + topic
		r, err := s.DB.Query(
			"SELECT tree, spec, id, stance, preferred_approach, confidence, valid_from, valid_until FROM dispositions WHERE stakeholder_id = ? AND topic = ? ORDER BY valid_from DESC",
			stakeholderID, topic,
		)
		if err != nil {
			return err
		}
		rows = r
	} else {
		// Current dispositions across all specs
		r, err := s.DB.Query(
			"SELECT tree, spec, id, topic, stance, preferred_approach, confidence, valid_from FROM dispositions WHERE stakeholder_id = ? AND valid_until IS NULL ORDER BY valid_from DESC",
			stakeholderID,
		)
		if err != nil {
			return err
		}
		rows = r
	}
	defer rows.Close() //nolint:errcheck

	var dispositions []map[string]any
	for rows.Next() {
		if topic != "" {
			var tree, spec, id, stance, confidence, validFrom string
			var preferred, validUntil *string
			if err := rows.Scan(&tree, &spec, &id, &stance, &preferred, &confidence, &validFrom, &validUntil); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}
			d := map[string]any{
				"tree": tree, "spec": spec, "id": id, "stance": stance,
				"confidence": confidence, "valid_from": validFrom,
				"current": validUntil == nil,
			}
			if preferred != nil {
				d["preferred_approach"] = *preferred
			}
			if validUntil != nil {
				d["valid_until"] = *validUntil
			}
			dispositions = append(dispositions, d)
		} else {
			var tree, spec, id, dtopic, stance, confidence, validFrom string
			var preferred *string
			if err := rows.Scan(&tree, &spec, &id, &dtopic, &stance, &preferred, &confidence, &validFrom); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}
			d := map[string]any{
				"tree": tree, "spec": spec, "id": id, "topic": dtopic,
				"stance": stance, "confidence": confidence, "valid_from": validFrom,
			}
			if preferred != nil {
				d["preferred_approach"] = *preferred
			}
			dispositions = append(dispositions, d)
		}
	}

	result := map[string]any{"stakeholder": stakeholderID, "dispositions": dispositions}
	if topic != "" {
		result["topic"] = topic
		result["mode"] = "history"
	} else {
		result["mode"] = "current"
	}
	return jsonOut(result)
}
