package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdSignalRecord(c *SignalRecordCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}
	stakeholderID := c.StakeholderID
	source := c.Source
	sourceType := c.Type
	content := c.Content
	date := c.Date
	if date == "" {
		date = store.Today()
	}

	var ids []string
	rows, err := s.DB.Query("SELECT id FROM signals WHERE tree = ? AND spec = ?", tree, spec)
	if err != nil {
		return fmt.Errorf("querying signal IDs: %w", err)
	}
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		ids = append(ids, id)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()
	newID := store.NextID("s", ids)

	var ourActionPtr, interpPtr *string
	if c.OurAction != "" {
		ourActionPtr = &c.OurAction
	}
	if c.Interpretation != "" {
		interpPtr = &c.Interpretation
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

func cmdSignalList(c *SignalListCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
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
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "signals": signals})
}
