package main

import (
	"fmt"
	"os/exec"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdTaskClaim(c *TaskClaimCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}
	taskID := c.TaskID

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

func cmdTaskDone(c *TaskDoneCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}
	taskID := c.TaskID
	skipVerify := c.SkipVerify

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

func cmdTaskBlock(c *TaskBlockCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}
	taskID := c.TaskID

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

func cmdTaskWait(c *TaskWaitCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}
	taskID := c.TaskID

	reason := c.Reason
	external := c.External
	check := c.Check
	var checkAfter *string
	if c.CheckAfter != "" {
		v := c.CheckAfter
		checkAfter = &v
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

func cmdTaskCancel(c *TaskCancelCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}
	taskID := c.TaskID

	var notePtr *string
	if c.Reason != "" {
		notePtr = &c.Reason
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
