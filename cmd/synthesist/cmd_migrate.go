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

	var migrated []string

	// v4 -> v5 migration
	if current == "4" || current == "0" {
		// Add phase table if it doesn't exist
		_, err := s.DB.Exec(`CREATE TABLE IF NOT EXISTS phase (
			id INT PRIMARY KEY DEFAULT 1,
			name VARCHAR(32) NOT NULL DEFAULT 'orient',
			updated DATETIME DEFAULT CURRENT_TIMESTAMP
		)`)
		if err != nil {
			return fmt.Errorf("creating phase table: %w", err)
		}
		migrated = append(migrated, "phase table")

		// Add specs table if it doesn't exist (for older v4 databases)
		_, err = s.DB.Exec(`CREATE TABLE IF NOT EXISTS specs (
			tree VARCHAR(255) NOT NULL,
			id VARCHAR(255) NOT NULL,
			goal TEXT,
			constraints TEXT,
			decisions TEXT,
			created DATE,
			PRIMARY KEY (tree, id)
		)`)
		if err != nil {
			return fmt.Errorf("creating specs table: %w", err)
		}
		migrated = append(migrated, "specs table")

		// Update version to 5
		_, err = s.DB.Exec("REPLACE INTO config (key_name, value) VALUES ('version', '5')")
		if err != nil {
			return fmt.Errorf("updating version: %w", err)
		}
		migrated = append(migrated, "version -> 5")

		if err := s.DoltCommit("migrate: v4 -> v5 (phase + specs tables)"); err != nil {
			return fmt.Errorf("committing migration: %w", err)
		}

		return jsonOut(map[string]any{
			"status":   "migrated",
			"from":     current,
			"to":       expectedVersion,
			"migrated": migrated,
			"message":  fmt.Sprintf("migrated v%s -> v%s", current, expectedVersion),
		})
	}

	return jsonOut(map[string]any{
		"status":  "unsupported",
		"current": current,
		"target":  expectedVersion,
		"message": fmt.Sprintf("no migration path from v%s to v%s", current, expectedVersion),
	})
}
