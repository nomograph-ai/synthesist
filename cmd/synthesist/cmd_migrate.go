package main

import (
	"database/sql"
	"fmt"
)

const expectedVersion = "5"

func cmdMigrate() error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	var current string
	err = s.DB.QueryRow("SELECT value FROM config WHERE key_name = 'version'").Scan(&current)
	if err == sql.ErrNoRows {
		current = "0"
	} else if err != nil {
		return fmt.Errorf("reading schema version: %w", err)
	}

	if current == expectedVersion {
		return jsonOut(map[string]any{
			"status":  "current",
			"version": expectedVersion,
			"message": fmt.Sprintf("database is current (v%s)", expectedVersion),
		})
	}

	return jsonOut(map[string]any{
		"status":  "needs_migration",
		"current": current,
		"target":  expectedVersion,
		"message": fmt.Sprintf("migration needed: v%s → v%s", current, expectedVersion),
		"note":    "automatic migration not yet implemented",
	})
}
