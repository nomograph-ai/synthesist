package main

import (
	"fmt"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdStakeholder(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth stakeholder <add|list> ...")
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
		return fmt.Errorf("usage: synth stakeholder add <tree> <id> --context 'role' [--name 'Full Name'] [--orgs 'org1,org2']")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

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
		s.DB.Exec("INSERT IGNORE INTO stakeholder_orgs (tree, stakeholder_id, org) VALUES (?, ?, ?)",
			tree, id, strings.TrimSpace(org))
	}

	s.Commit(fmt.Sprintf("landscape(%s): add stakeholder %s", tree, id))
	return jsonOut(map[string]any{"tree": tree, "id": id, "context": context})
}

func cmdStakeholderList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth stakeholder list <tree>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree := args[0]
	rows, err := s.DB.Query("SELECT id, name, context FROM stakeholders WHERE tree = ? ORDER BY id", tree)
	if err != nil {
		return err
	}
	defer rows.Close()

	var stakeholders []map[string]any
	for rows.Next() {
		var id, context string
		var name *string
		rows.Scan(&id, &name, &context)
		sh := map[string]any{"id": id, "context": context}
		if name != nil {
			sh["name"] = *name
		}
		// Get orgs
		orgRows, _ := s.DB.Query("SELECT org FROM stakeholder_orgs WHERE tree = ? AND stakeholder_id = ?", tree, id)
		var orgs []string
		for orgRows.Next() {
			var org string
			orgRows.Scan(&org)
			orgs = append(orgs, org)
		}
		orgRows.Close()
		if len(orgs) > 0 {
			sh["orgs"] = orgs
		}
		stakeholders = append(stakeholders, sh)
	}
	return jsonOut(map[string]any{"tree": tree, "stakeholders": stakeholders})
}

func cmdDisposition(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth disposition <add|list|supersede> ...")
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
		return fmt.Errorf("usage: synth disposition add <tree/spec> <stakeholder> --topic '...' --stance cautious --confidence inferred [--preferred '...']")
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
		rows.Scan(&id)
		ids = append(ids, id)
	}
	rows.Close()
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

	s.Commit(fmt.Sprintf("landscape(%s/%s): disposition %s -- %s is %s on %s", tree, spec, newID, stakeholderID, stance, topic))
	return jsonOut(map[string]any{
		"id": newID, "stakeholder": stakeholderID, "topic": topic,
		"stance": stance, "confidence": confidence,
	})
}

func cmdDispositionList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth disposition list <tree/spec>")
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
		"SELECT id, stakeholder_id, topic, stance, preferred_approach, confidence, valid_from, valid_until FROM dispositions WHERE tree = ? AND spec = ? ORDER BY valid_from DESC",
		tree, spec,
	)
	if err != nil {
		return err
	}
	defer rows.Close()

	var dispositions []map[string]any
	for rows.Next() {
		var id, stakeholder, topic, stance, confidence, validFrom string
		var preferred, validUntil *string
		rows.Scan(&id, &stakeholder, &topic, &stance, &preferred, &confidence, &validFrom, &validUntil)
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
		return fmt.Errorf("usage: synth disposition supersede <tree/spec> <disposition-id> --new-stance supportive [--evidence s1] [--preferred '...']")
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
		rows.Scan(&id)
		ids = append(ids, id)
	}
	rows.Close()
	newID := store.NextID("d", ids)

	today := store.Today()

	// Supersede old
	s.DB.Exec("UPDATE dispositions SET valid_until = ?, superseded_by = ? WHERE tree = ? AND spec = ? AND id = ?",
		today, newID, tree, spec, oldID)

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

	s.DB.Exec(
		"INSERT INTO dispositions (tree, spec, id, stakeholder_id, topic, stance, preferred_approach, confidence, evidence, valid_from) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
		tree, spec, newID, stakeholder, topic, newStance, preferredPtr, confidence, evidencePtr, today,
	)

	commitMsg := fmt.Sprintf("landscape(%s/%s): supersede %s -> %s (%s now %s on %s)", tree, spec, oldID, newID, stakeholder, newStance, topic)
	if evidence != "" {
		commitMsg += fmt.Sprintf(" [evidence: %s]", evidence)
	}
	s.Commit(commitMsg)

	out := map[string]any{
		"old_id": oldID, "new_id": newID, "stakeholder": stakeholder,
		"topic": topic, "old_stance": "superseded", "new_stance": newStance,
	}
	if evidence != "" {
		out["evidence_signal"] = evidence
	}
	return jsonOut(out)
}

func cmdSignal(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth signal <record|list> ...")
	}
	switch args[0] {
	case "record":
		return cmdSignalRecord(args[1:])
	case "list":
		return cmdSignalList(args[1:])
	default:
		return fmt.Errorf("unknown signal subcommand: %s", args[0])
	}
}

func cmdSignalRecord(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synth signal record <tree/spec> <stakeholder> --source 'url' --type pr_comment --content '...' [--date YYYY-MM-DD] [--our-action '...'] [--interpretation '...']")
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
	stakeholderID := args[1]

	var source, sourceType, content, date, ourAction, interpretation string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--source":
			source = args[i+1]
		case "--type":
			sourceType = args[i+1]
		case "--content":
			content = args[i+1]
		case "--date":
			date = args[i+1]
		case "--our-action":
			ourAction = args[i+1]
		case "--interpretation":
			interpretation = args[i+1]
		}
	}
	if source == "" || sourceType == "" || content == "" {
		return fmt.Errorf("--source, --type, and --content are required")
	}
	if date == "" {
		date = store.Today()
	}

	var ids []string
	rows, _ := s.DB.Query("SELECT id FROM signals WHERE tree = ? AND spec = ?", tree, spec)
	for rows.Next() {
		var id string
		rows.Scan(&id)
		ids = append(ids, id)
	}
	rows.Close()
	newID := store.NextID("s", ids)

	var ourActionPtr, interpPtr *string
	if ourAction != "" {
		ourActionPtr = &ourAction
	}
	if interpretation != "" {
		interpPtr = &interpretation
	}

	_, err = s.DB.Exec(
		"INSERT INTO signals (tree, spec, id, stakeholder_id, date, recorded_date, source, source_type, content, our_action, interpretation) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
		tree, spec, newID, stakeholderID, date, store.Today(), source, sourceType, content, ourActionPtr, interpPtr,
	)
	if err != nil {
		return fmt.Errorf("inserting signal: %w", err)
	}

	s.Commit(fmt.Sprintf("landscape(%s/%s): signal %s from %s", tree, spec, newID, stakeholderID))
	return jsonOut(map[string]any{
		"id": newID, "stakeholder": stakeholderID, "date": date,
		"source_type": sourceType, "recorded_date": store.Today(),
	})
}

func cmdSignalList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth signal list <tree/spec>")
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
		"SELECT id, stakeholder_id, date, recorded_date, source, source_type, content, our_action, interpretation FROM signals WHERE tree = ? AND spec = ? ORDER BY date DESC",
		tree, spec,
	)
	if err != nil {
		return err
	}
	defer rows.Close()

	var signals []map[string]any
	for rows.Next() {
		var id, stakeholder, date, recordedDate, source, sourceType, content string
		var ourAction, interpretation *string
		rows.Scan(&id, &stakeholder, &date, &recordedDate, &source, &sourceType, &content, &ourAction, &interpretation)
		sig := map[string]any{
			"id": id, "stakeholder": stakeholder, "date": date,
			"recorded_date": recordedDate, "source": source,
			"source_type": sourceType, "content": content,
		}
		if ourAction != nil {
			sig["our_action"] = *ourAction
		}
		if interpretation != nil {
			sig["interpretation"] = *interpretation
		}
		signals = append(signals, sig)
	}
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "signals": signals})
}

func cmdLandscape(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth landscape show <tree/spec>")
	}
	if args[0] != "show" {
		return fmt.Errorf("unknown landscape subcommand: %s (did you mean 'show'?)", args[0])
	}
	return cmdLandscapeShow(args[1:])
}

func cmdLandscapeShow(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth landscape show <tree/spec>")
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

	result := map[string]any{"tree": tree, "spec": spec}

	// Current dispositions
	rows, _ := s.DB.Query(
		"SELECT d.id, d.stakeholder_id, s.context, d.topic, d.stance, d.preferred_approach, d.confidence, d.valid_from FROM dispositions d JOIN stakeholders s ON d.tree = s.tree AND d.stakeholder_id = s.id WHERE d.tree = ? AND d.spec = ? AND d.valid_until IS NULL ORDER BY d.stakeholder_id",
		tree, spec,
	)
	dispositions := make([]map[string]any, 0)
	for rows.Next() {
		var id, stakeholder, context, topic, stance, confidence, validFrom string
		var preferred *string
		rows.Scan(&id, &stakeholder, &context, &topic, &stance, &preferred, &confidence, &validFrom)
		d := map[string]any{
			"id": id, "stakeholder": stakeholder, "stakeholder_context": context,
			"topic": topic, "stance": stance, "confidence": confidence, "valid_from": validFrom,
		}
		if preferred != nil {
			d["preferred_approach"] = *preferred
		}
		dispositions = append(dispositions, d)
	}
	rows.Close()
	result["dispositions"] = dispositions

	// Signals
	rows, _ = s.DB.Query(
		"SELECT sig.id, sig.stakeholder_id, sh.context, sig.date, sig.source, sig.source_type, sig.content FROM signals sig JOIN stakeholders sh ON sig.tree = sh.tree AND sig.stakeholder_id = sh.id WHERE sig.tree = ? AND sig.spec = ? ORDER BY sig.date DESC LIMIT 20",
		tree, spec,
	)
	signals := make([]map[string]any, 0)
	for rows.Next() {
		var id, stakeholder, context, date, source, sourceType, content string
		rows.Scan(&id, &stakeholder, &context, &date, &source, &sourceType, &content)
		signals = append(signals, map[string]any{
			"id": id, "stakeholder": stakeholder, "stakeholder_context": context,
			"date": date, "source": source, "source_type": sourceType, "content": content,
		})
	}
	rows.Close()
	result["signals"] = signals

	// Influences
	rows, _ = s.DB.Query(
		"SELECT i.stakeholder_id, s.context, i.task_id, i.role FROM influences i JOIN stakeholders s ON i.tree = s.tree AND i.stakeholder_id = s.id WHERE i.tree = ? AND i.spec = ?",
		tree, spec,
	)
	influences := make([]map[string]any, 0)
	for rows.Next() {
		var stakeholder, context, taskID, role string
		rows.Scan(&stakeholder, &context, &taskID, &role)
		influences = append(influences, map[string]any{
			"stakeholder": stakeholder, "context": context, "task": taskID, "role": role,
		})
	}
	rows.Close()
	result["influences"] = influences

	// Directions affecting this spec
	rows, _ = s.DB.Query(`
		SELECT d.id, d.project, d.topic, d.status, d.impact, di.description
		FROM directions d
		JOIN direction_impacts di ON d.tree = di.tree AND d.id = di.direction_id
		WHERE di.affected_tree = ? AND di.affected_spec = ? AND d.valid_until IS NULL
	`, tree, spec)
	directions := make([]map[string]any, 0)
	for rows.Next() {
		var id, project, topic, status, impact, desc string
		rows.Scan(&id, &project, &topic, &status, &impact, &desc)
		directions = append(directions, map[string]any{
			"id": id, "project": project, "topic": topic,
			"status": status, "impact": impact, "impact_on_spec": desc,
		})
	}
	rows.Close()
	result["directions"] = directions

	return jsonOut(result)
}

func cmdStance(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synth stance <stakeholder> [topic]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	stakeholderID := args[0]
	var topic string
	if len(args) > 1 {
		topic = args[1]
	}

	var rows interface{ Next() bool; Scan(...any) error; Close() error }
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
	defer rows.Close()

	var dispositions []map[string]any
	for rows.Next() {
		if topic != "" {
			var tree, spec, id, stance, confidence, validFrom string
			var preferred, validUntil *string
			rows.Scan(&tree, &spec, &id, &stance, &preferred, &confidence, &validFrom, &validUntil)
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
			rows.Scan(&tree, &spec, &id, &dtopic, &stance, &preferred, &confidence, &validFrom)
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
