package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdSignal(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist signal <record|list> ...") //nolint:staticcheck
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
		return fmt.Errorf("usage: synthesist signal record <tree/spec> <stakeholder> --source 'url' --type pr_comment --content '...' [--date YYYY-MM-DD] [--our-action '...'] [--interpretation '...']")
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
		if err := rows.Scan(&id); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		ids = append(ids, id)
	}
	_ = rows.Close()
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

	if err := s.Commit(fmt.Sprintf("landscape(%s/%s): signal %s from %s", tree, spec, newID, stakeholderID)); err != nil {
		return err
	}
	return jsonOut(map[string]any{
		"id": newID, "stakeholder": stakeholderID, "date": date,
		"source_type": sourceType, "recorded_date": store.Today(),
	})
}

func cmdSignalList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist signal list <tree/spec>")
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
		"SELECT id, stakeholder_id, date, recorded_date, source, source_type, content, our_action, interpretation FROM signals WHERE tree = ? AND spec = ? ORDER BY date DESC",
		tree, spec,
	)
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	var signals []map[string]any
	for rows.Next() {
		var id, stakeholder, date, recordedDate, source, sourceType, content string
		var ourAction, interpretation *string
		if err := rows.Scan(&id, &stakeholder, &date, &recordedDate, &source, &sourceType, &content, &ourAction, &interpretation); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
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
