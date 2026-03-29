package main

import (
	"encoding/json"
	"fmt"
	"os"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdInit() error {
	dir, err := os.Getwd()
	if err != nil {
		return err
	}

	// Check if already initialized
	s, err := store.Open(dir)
	if err == nil {
		_ = s.Close()
		return fmt.Errorf("already initialized at %s", dir)
	}

	s, err = store.Init(dir)
	if err != nil {
		return fmt.Errorf("init failed: %w", err)
	}
	defer s.Close() //nolint:errcheck

	result := map[string]any{
		"status":  "initialized",
		"root":    dir,
		"db_path": s.DBPath,
		"tables":  20,
		"version": 5,
	}
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	return enc.Encode(result)
}
