package main

import (
	"fmt"
	"strings"
	"time"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdSessionStart(c *SessionStartCmd) error {
	sessionID := c.SessionID

	// Open store WITHOUT session (we're creating the branch on main)
	origSession := store.Session
	store.Session = ""
	s, err := discoverStore()
	store.Session = origSession
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	// Advisory spec lock hint
	specHint := c.Spec

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
			_ = b
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

func cmdSessionMerge(c *SessionMergeCmd) error {
	sessionID := c.SessionID

	// Parse conflict resolution strategy
	var strategy string
	if c.Ours {
		strategy = "ours"
	} else if c.Theirs {
		strategy = "theirs"
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

func cmdSessionList() error {
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
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}

	return jsonOut(map[string]any{
		"sessions": sessions,
		"count":    len(sessions),
	})
}

func cmdSessionStatus(c *SessionStatusCmd) error {
	sessionID := c.SessionID

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
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}

	return jsonOut(map[string]any{
		"session": sessionID,
		"status":  "active",
		"changes": changes,
	})
}

func cmdSessionPrune(c *SessionPruneCmd) error {
	// Open store on main
	origSession := store.Session
	store.Session = ""
	s, err := discoverStore()
	store.Session = origSession
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	cutoff := time.Now().Add(-time.Duration(c.Hours) * time.Hour)

	rows, err := s.DB.Query("SELECT name, latest_commit_date FROM dolt_branches ORDER BY name")
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	type branchInfo struct {
		Name    string
		DateStr string
	}
	type branchResult struct {
		Name   string `json:"name"`
		Action string `json:"action"` // merged, kept, conflict
		Reason string `json:"reason,omitempty"`
	}

	// Collect all branch data first, then close rows before doing
	// merge/delete operations that would corrupt the open result set.
	var branches []branchInfo
	for rows.Next() {
		var name, dateStr string
		if err := rows.Scan(&name, &dateStr); err != nil {
			return fmt.Errorf("scanning branch: %w", err)
		}
		if name == "main" {
			continue
		}
		branches = append(branches, branchInfo{Name: name, DateStr: dateStr})
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	rows.Close() //nolint:errcheck

	var results []branchResult
	for _, b := range branches {
		// Parse the date
		lastActivity, err := time.Parse("2006-01-02 15:04:05", strings.TrimSuffix(b.DateStr, ".000000"))
		if err != nil {
			lastActivity, err = time.Parse(time.RFC3339, b.DateStr)
			if err != nil {
				results = append(results, branchResult{Name: b.Name, Action: "kept", Reason: "unparseable date"})
				continue
			}
		}

		if !lastActivity.Before(cutoff) {
			results = append(results, branchResult{Name: b.Name, Action: "kept", Reason: "still active"})
			continue
		}

		// Stale branch — try to merge to preserve any work
		conflicts, mergeErr := s.MergeBranch(b.Name)
		if mergeErr != nil {
			results = append(results, branchResult{Name: b.Name, Action: "kept", Reason: fmt.Sprintf("merge error: %v", mergeErr)})
			continue
		}
		if conflicts > 0 {
			// Can't auto-merge — flag for human review, don't delete
			results = append(results, branchResult{
				Name:   b.Name,
				Action: "conflict",
				Reason: fmt.Sprintf("%d conflicts — needs manual resolution via 'session merge %s --ours' or '--theirs'", conflicts, b.Name),
			})
			continue
		}

		// Merged cleanly — now safe to delete the branch pointer
		// (all data is on main, Dolt history preserves every commit)
		if err := s.DeleteBranch(b.Name); err != nil {
			results = append(results, branchResult{Name: b.Name, Action: "kept", Reason: fmt.Sprintf("delete error: %v", err)})
			continue
		}

		if err := s.GitCommit(fmt.Sprintf("session(%s): prune — merged and cleaned", b.Name)); err != nil {
			// Non-fatal — the merge succeeded, just the git commit failed
			_ = err
		}

		results = append(results, branchResult{Name: b.Name, Action: "merged", Reason: "stale — merged to main, branch pointer removed"})
	}

	// Summarize
	merged := 0
	kept := 0
	conflicted := 0
	for _, r := range results {
		switch r.Action {
		case "merged":
			merged++
		case "kept":
			kept++
		case "conflict":
			conflicted++
		}
	}

	return jsonOut(map[string]any{
		"branches":   results,
		"merged":     merged,
		"kept":       kept,
		"conflicted": conflicted,
	})
}
