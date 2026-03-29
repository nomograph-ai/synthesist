package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdThread(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist thread <create|list|update|prune> ...") //nolint:staticcheck
	}
	switch args[0] {
	case "create":
		return cmdThreadCreate(args[1:])
	case "list":
		return cmdThreadList(args[1:])
	default:
		return fmt.Errorf("unknown thread subcommand: %s", args[0])
	}
}

func cmdThreadCreate(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist thread create <id> --tree <tree> --summary '...' [--spec id] [--task id] [--date YYYY-MM-DD]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	threadID := args[0]
	var tree, summary, date string
	var spec, task *string
	for i := 1; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--tree":
			tree = args[i+1]
		case "--summary":
			summary = args[i+1]
		case "--spec":
			v := args[i+1]
			spec = &v
		case "--task":
			v := args[i+1]
			task = &v
		case "--date":
			date = args[i+1]
		}
	}
	if tree == "" || summary == "" {
		return fmt.Errorf("--tree and --summary are required")
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

func cmdThreadList(args []string) error {
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
