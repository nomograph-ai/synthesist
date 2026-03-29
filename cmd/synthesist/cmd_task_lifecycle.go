package main

import (
	"fmt"
	"os/exec"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdTaskClaim(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task claim <tree/spec> <task-id>")
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
	taskID := args[1]

	// Check deps are done (before attempting claim)
	depRows, err := s.DB.Query(
		"SELECT d.depends_on, t.status FROM task_deps d JOIN tasks t ON d.tree = t.tree AND d.spec = t.spec AND d.depends_on = t.id WHERE d.tree = ? AND d.spec = ? AND d.task_id = ?",
		tree, spec, taskID,
	)
	if err != nil {
		return Wrap("checking dependencies", err)
	}
	defer depRows.Close() //nolint:errcheck
	for depRows.Next() {
		var depID, depStatus string
		if err := depRows.Scan(&depID, &depStatus); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		if depStatus != "done" {
			return ErrWrongState("dependency", depID, depStatus, "done")
		}
	}

	// Atomic claim: UPDATE only if status=pending and no owner.
	// Rows affected = 0 means either not found, wrong state, or already owned.
	ownerName := "synthesist"
	res, err := s.DB.Exec(
		"UPDATE tasks SET status = 'in_progress', owner = ? WHERE tree = ? AND spec = ? AND id = ? AND status = 'pending' AND (owner IS NULL OR owner = '')",
		ownerName, tree, spec, taskID)
	if err != nil {
		return err
	}
	affected, _ := res.RowsAffected()
	if affected == 0 {
		// Determine why: not found, wrong state, or already owned
		var status string
		var owner *string
		err = s.DB.QueryRow("SELECT status, owner FROM tasks WHERE tree = ? AND spec = ? AND id = ?",
			tree, spec, taskID).Scan(&status, &owner)
		if err != nil {
			return ErrNotFound("task", fmt.Sprintf("%s/%s/%s", tree, spec, taskID))
		}
		if owner != nil && *owner != "" {
			return ErrAlreadyOwned(taskID, *owner)
		}
		return ErrWrongState("task", taskID, status, "pending")
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): claim %s", tree, spec, taskID)); err != nil {
		return err
	}

	return jsonOut(map[string]any{"id": taskID, "status": "in_progress", "owner": ownerName})
}

func cmdTaskDone(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task done <tree/spec> <task-id>")
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
	taskID := args[1]

	skipVerify := false
	for _, arg := range args[2:] {
		if arg == "--skip-verify" {
			skipVerify = true
		}
	}

	var results []map[string]any
	allPass := true

	if !skipVerify {
		// Run acceptance criteria
		acRows, err := s.DB.Query(
			"SELECT seq, criterion, verify_cmd FROM acceptance WHERE tree = ? AND spec = ? AND task_id = ? ORDER BY seq",
			tree, spec, taskID,
		)
		if err != nil {
			return err
		}
		defer acRows.Close() //nolint:errcheck

		for acRows.Next() {
			var seq int
			var criterion, verifyCmd string
			if err := acRows.Scan(&seq, &criterion, &verifyCmd); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}

			cmd := exec.Command("sh", "-c", verifyCmd)
			cmd.Dir = s.Root
			output, err := cmd.CombinedOutput()
			passed := err == nil

			result := map[string]any{
				"criterion": criterion,
				"verify":    verifyCmd,
				"passed":    passed,
			}
			if !passed {
				allPass = false
				result["output"] = strings.TrimSpace(string(output))
			}
			results = append(results, result)
		}
	}

	today := store.Today()
	if allPass {
		_, err = s.DB.Exec(
			"UPDATE tasks SET status = 'done', completed = ?, owner = NULL, failure_note = NULL WHERE tree = ? AND spec = ? AND id = ?",
			today, tree, spec, taskID,
		)
		if err != nil {
			return err
		}
		if err := s.Commit(fmt.Sprintf("spec(%s/%s): %s done", tree, spec, taskID)); err != nil {
			return err
		}
	} else {
		note := "acceptance criteria failed"
		if _, err = s.DB.Exec(
			"UPDATE tasks SET status = 'pending', owner = NULL, failure_note = ? WHERE tree = ? AND spec = ? AND id = ?",
			note, tree, spec, taskID,
		); err != nil {
			return err
		}
	}

	return jsonOut(map[string]any{
		"id": taskID, "all_passed": allPass,
		"status":   map[bool]string{true: "done", false: "pending"}[allPass],
		"criteria": results,
	})
}

func cmdTaskBlock(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task block <tree/spec> <task-id>")
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
	taskID := args[1]

	_, err = s.DB.Exec("UPDATE tasks SET status = 'blocked' WHERE tree = ? AND spec = ? AND id = ?",
		tree, spec, taskID)
	if err != nil {
		return err
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): %s blocked", tree, spec, taskID)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"id": taskID, "status": "blocked"})
}

func cmdTaskWait(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task wait <tree/spec> <task-id> --reason '...' --external 'url' --check 'cmd' [--check-after YYYY-MM-DD]")
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
	taskID := args[1]

	var reason, external, check string
	var checkAfter *string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--reason":
			reason = args[i+1]
		case "--external":
			external = args[i+1]
		case "--check":
			check = args[i+1]
		case "--check-after":
			v := args[i+1]
			checkAfter = &v
		}
	}

	if reason == "" || external == "" || check == "" {
		return fmt.Errorf("--reason, --external, and --check are required")
	}

	_, err = s.DB.Exec(
		"UPDATE tasks SET status = 'waiting', waiter_reason = ?, waiter_external = ?, waiter_check = ?, waiter_check_after = ? WHERE tree = ? AND spec = ? AND id = ?",
		reason, external, check, checkAfter, tree, spec, taskID,
	)
	if err != nil {
		return err
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): %s waiting -- %s", tree, spec, taskID, reason)); err != nil {
		return err
	}

	return jsonOut(map[string]any{"id": taskID, "status": "waiting", "reason": reason, "external": external})
}

func cmdTaskCancel(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task cancel <tree/spec> <task-id> [--reason '...']")
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
	taskID := args[1]

	var reason string
	for i := 2; i < len(args)-1; i += 2 {
		if args[i] == "--reason" {
			reason = args[i+1]
		}
	}

	var notePtr *string
	if reason != "" {
		notePtr = &reason
	}

	_, err = s.DB.Exec("UPDATE tasks SET status = 'cancelled', failure_note = ? WHERE tree = ? AND spec = ? AND id = ?",
		notePtr, tree, spec, taskID)
	if err != nil {
		return err
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): cancel %s", tree, spec, taskID)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"id": taskID, "status": "cancelled"})
}
