package store

import (
	"fmt"
	"os"
)

// Session holds the active session ID. When set, all operations happen
// on a Dolt branch named after the session. Set via --session flag or
// SYNTHESIST_SESSION env var in main.go.
//
// NOTE: This is global mutable state. Some commands (session start, merge,
// list, prune) temporarily save/restore Session to open the store on main:
//
//	origSession := store.Session
//	store.Session = ""
//	s, err := discoverStore()
//	store.Session = origSession
//
// This pattern is fragile (not goroutine-safe, not panic-safe) but acceptable
// for a single-threaded CLI. Not worth restructuring for v5.
var Session string

// CreateBranch creates a new Dolt branch from current HEAD.
func (s *Store) CreateBranch(name string) error {
	_, err := s.DB.Exec("CALL dolt_branch(?)", name)
	if err != nil {
		return fmt.Errorf("creating branch %s: %w", name, err)
	}
	return nil
}

// SwitchBranch checks out a Dolt branch for the current connection.
func (s *Store) SwitchBranch(name string) error {
	_, err := s.DB.Exec("CALL dolt_checkout(?)", name)
	if err != nil {
		return fmt.Errorf("switching to branch %s: %w", name, err)
	}
	return nil
}

// DeleteBranch deletes a Dolt branch.
func (s *Store) DeleteBranch(name string) error {
	_, err := s.DB.Exec("CALL dolt_branch('-D', ?)", name)
	if err != nil {
		return fmt.Errorf("deleting branch %s: %w", name, err)
	}
	return nil
}

// MergeBranch merges a named branch into main. Returns conflict count.
func (s *Store) MergeBranch(name string) (int, error) {
	if err := s.SwitchBranch("main"); err != nil {
		return 0, err
	}
	var hash string
	var ff, conflicts int
	var message string
	if err := s.DB.QueryRow("CALL dolt_merge(?)", name).Scan(&hash, &ff, &conflicts, &message); err != nil {
		return 0, fmt.Errorf("merging branch %s: %w", name, err)
	}
	return conflicts, nil
}

// ListBranches returns all Dolt branch names.
func (s *Store) ListBranches() ([]string, error) {
	rows, err := s.DB.Query("SELECT name FROM dolt_branches ORDER BY name")
	if err != nil {
		return nil, err
	}
	defer rows.Close() //nolint:errcheck
	var branches []string
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			return nil, fmt.Errorf("scanning branch: %w", err)
		}
		branches = append(branches, name)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterating rows: %w", err)
	}
	return branches, nil
}

// ActiveBranch returns the current Dolt branch name.
func (s *Store) ActiveBranch() (string, error) {
	var branch string
	err := s.DB.QueryRow("SELECT active_branch()").Scan(&branch)
	return branch, err
}

// EnsureSession switches to the session branch if Session is set.
// Called after Open/Discover to set up session isolation.
// Special case: Session="main" operates directly on main (no branch switch).
func (s *Store) EnsureSession() error {
	if Session == "" || Session == "main" {
		return nil
	}
	branches, err := s.ListBranches()
	if err != nil {
		return err
	}
	for _, b := range branches {
		if b == Session {
			return s.SwitchBranch(Session)
		}
	}
	// Session branch doesn't exist — warn and fall back to main so that
	// read-only commands still work (e.g. status, list) even if the session
	// was merged or pruned.
	fmt.Fprintf(os.Stderr, "warning: session %q branch not found — falling back to main\n", Session)
	return nil
}
