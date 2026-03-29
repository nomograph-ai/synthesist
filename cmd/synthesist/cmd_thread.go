package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdThreadCreate(c *ThreadCreateCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	threadID := c.ID
	tree := c.Tree
	summary := c.Summary
	date := c.Date

	var spec, task *string
	if c.Spec != "" {
		v := c.Spec
		spec = &v
	}
	if c.Task != "" {
		v := c.Task
		task = &v
	}
	if date == "" {
		date = store.Today()
	}

	_, err = s.DB.Exec("INSERT INTO threads (id, tree, spec, task, date, summary) VALUES (?, ?, ?, ?, ?, ?)",
		threadID, tree, spec, task, date, summary)
	if err != nil {
		return fmt.Errorf("creating thread: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("estate: create thread %s", threadID)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"id": threadID, "tree": tree, "summary": summary, "date": date})
}

func cmdThreadList() error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	rows, err := s.DB.Query("SELECT id, tree, spec, task, date, summary FROM threads ORDER BY date DESC")
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	threads := make([]map[string]any, 0)
	for rows.Next() {
		var id, tree, date, summary string
		var spec, task *string
		if err := rows.Scan(&id, &tree, &spec, &task, &date, &summary); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		t := map[string]any{"id": id, "tree": tree, "date": date, "summary": summary}
		if spec != nil {
			t["spec"] = *spec
		}
		if task != nil {
			t["task"] = *task
		}
		threads = append(threads, t)
	}
	return jsonOut(map[string]any{"threads": threads})
}
