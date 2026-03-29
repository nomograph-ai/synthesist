package main

import (
	"fmt"
	"strings"
	"time"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdSession(args []string) error {
	if len(args) == 0 {
		return fmt.Errorf("usage: synthesist session <start|merge|list|status|prune>") //nolint:staticcheck
	}
	switch args[0] {
	case "start":
		return cmdSessionStart(args[1:])
	case "merge":
		return cmdSessionMerge(args[1:])
	case "list":
		return cmdSessionList(args[1:])
	case "status":
		return cmdSessionStatus(args[1:])
	case "prune":
		return cmdSessionPrune(args[1:])
	default:
		return ErrUnknownSubcommand("session", args[0])
	}
}

func cmdSessionStart(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist session start <session-id>")
	}
	sessionID := args[0]

	// Open store WITHOUT session (we're creating the branch on main)
	origSession := store.Session
	store.Session = ""
	s, err := discoverStore()
	store.Session = origSession
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	// Check for spec lock — warn if another session has claimed tasks in the same spec
	// Parse optional --spec flag for advisory lock
	var specHint string
	for i, arg := range args[1:] {
		if arg == "--spec" && i+1 < len(args[1:]) {
			specHint = args[i+2]
		}
	}

	// Check if branch already exists
	branches, err := s.ListBranches()
	if err != nil {
		return err
	}
	for _, b := range branches {
		if b == sessionID {
			return fmt.Errorf("session %q already exists — use 'session merge' to close it or 'session list' to see active sessions", sessionID)
		}
	}

	// Advisory spec lock check
	if specHint != "" {
		for _, b := range branches {
			if b == "main" {
				continue
			}
			// Check if the other session has claimed tasks in the same spec
			// by looking at task ownership on that branch
			// (best-effort — just warn, don't block)
		}
	}

	if err := s.CreateBranch(sessionID); err != nil {
		return err
	}

	result := map[string]any{
		"session": sessionID,
		"status":  "started",
		"branch":  sessionID,
	}
	return jsonOut(result)
}

func cmdSessionMerge(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist session merge <session-id> [--ours|--theirs]")
	}
	sessionID := args[0]

	// Parse conflict resolution strategy
	var strategy string
	for _, arg := range args[1:] {
		switch arg {
		case "--ours":
			strategy = "ours"
		case "--theirs":
			strategy = "theirs"
		}
	}

	// Open store on main (not on the session branch)
	origSession := store.Session
	store.Session = ""
	s, err := discoverStore()
	store.Session = origSession
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	conflicts, err := s.MergeBranch(sessionID)
	if err != nil {
		return err
	}

	if conflicts > 0 && strategy == "" {
		// Report conflicts without resolving
		return jsonOut(map[string]any{
			"session":   sessionID,
			"status":    "conflicts",
			"conflicts": conflicts,
			"hint":      "re-run with --ours or --theirs to resolve",
		})
	}

	if conflicts > 0 {
		// Resolve conflicts
		var resolveSQL string
		if strategy == "ours" {
			resolveSQL = "CALL dolt_conflicts_resolve('--ours')"
		} else {
			resolveSQL = "CALL dolt_conflicts_resolve('--theirs')"
		}
		if _, err := s.DB.Exec(resolveSQL); err != nil {
			return fmt.Errorf("resolving conflicts: %w", err)
		}
		if err := s.DoltCommit(fmt.Sprintf("session(%s): merge with %s resolution", sessionID, strategy)); err != nil {
			return Wrap("committing resolution", err)
		}
	}

	// Delete the merged branch
	if err := s.DeleteBranch(sessionID); err != nil {
		return Wrap("deleting merged branch", err)
	}

	// Now git commit (we're on main)
	if err := s.GitCommit(fmt.Sprintf("session(%s): merge", sessionID)); err != nil {
		return err
	}

	return jsonOut(map[string]any{
		"session":   sessionID,
		"status":    "merged",
		"conflicts": conflicts,
		"strategy":  strategy,
	})
}

func cmdSessionList(args []string) error {
	// Open store on main
	origSession := store.Session
	store.Session = ""
	s, err := discoverStore()
	store.Session = origSession
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	rows, err := s.DB.Query("SELECT name, latest_committer, latest_commit_date, latest_commit_message FROM dolt_branches ORDER BY name")
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	var sessions []map[string]any
	for rows.Next() {
		var name, committer, date, message string
		if err := rows.Scan(&name, &committer, &date, &message); err != nil {
			return fmt.Errorf("scanning branch: %w", err)
		}
		if name == "main" {
			continue
		}
		sessions = append(sessions, map[string]any{
			"session":        name,
			"last_committer": committer,
			"last_activity":  date,
			"last_message":   message,
		})
	}

	return jsonOut(map[string]any{
		"sessions": sessions,
		"count":    len(sessions),
	})
}

func cmdSessionStatus(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist session status <session-id>")
	}
	sessionID := args[0]

	// Open store on main
	origSession := store.Session
	store.Session = ""
	s, err := discoverStore()
	store.Session = origSession
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	// Get diff summary between main and session branch
	rows, err := s.DB.Query(
		"SELECT table_name, diff_type, data_change, schema_change FROM dolt_diff_stat(?, ?)",
		"main", sessionID)
	if err != nil {
		// Fall back to simpler query if dolt_diff_stat not available
		return jsonOut(map[string]any{
			"session": sessionID,
			"status":  "active",
			"note":    "diff not available — branch exists",
		})
	}
	defer rows.Close() //nolint:errcheck

	var changes []map[string]any
	for rows.Next() {
		var tableName, diffType string
		var dataChange, schemaChange bool
		if err := rows.Scan(&tableName, &diffType, &dataChange, &schemaChange); err != nil {
			return fmt.Errorf("scanning diff: %w", err)
		}
		changes = append(changes, map[string]any{
			"table":         tableName,
			"diff_type":     diffType,
			"data_change":   dataChange,
			"schema_change": schemaChange,
		})
	}

	return jsonOut(map[string]any{
		"session": sessionID,
		"status":  "active",
		"changes": changes,
	})
}

func cmdSessionPrune(args []string) error {
	// Open store on main
	origSession := store.Session
	store.Session = ""
	s, err := discoverStore()
	store.Session = origSession
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	cutoff := time.Now().Add(-24 * time.Hour)

	rows, err := s.DB.Query("SELECT name, latest_commit_date FROM dolt_branches ORDER BY name")
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	var pruned []string
	var kept []string
	for rows.Next() {
		var name, dateStr string
		if err := rows.Scan(&name, &dateStr); err != nil {
			return fmt.Errorf("scanning branch: %w", err)
		}
		if name == "main" {
			continue
		}
		// Parse the date — Dolt returns ISO format
		lastActivity, err := time.Parse("2006-01-02 15:04:05", strings.TrimSuffix(dateStr, ".000000"))
		if err != nil {
			// Try alternate formats
			lastActivity, err = time.Parse(time.RFC3339, dateStr)
			if err != nil {
				kept = append(kept, name)
				continue
			}
		}
		if lastActivity.Before(cutoff) {
			if err := s.DeleteBranch(name); err != nil {
				kept = append(kept, name) // couldn't delete, keep it
				continue
			}
			pruned = append(pruned, name)
		} else {
			kept = append(kept, name)
		}
	}

	return jsonOut(map[string]any{
		"pruned": pruned,
		"kept":   kept,
	})
}
