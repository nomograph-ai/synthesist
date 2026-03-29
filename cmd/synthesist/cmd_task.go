package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"strings"

	"github.com/olekukonko/tablewriter"
	"github.com/olekukonko/tablewriter/renderer"
	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdTaskCreate(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task create <tree/spec> <summary> [--depends-on t1,t2] [--gate human] [--files f1,f2]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	summary := args[1]

	// Parse optional flags
	var dependsOn []string
	var gate *string
	var files []string
	var statusFlag, idFlag, createdFlag string
	var completedFlag *string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--depends-on":
			dependsOn = strings.Split(args[i+1], ",")
		case "--gate":
			v := args[i+1]
			gate = &v
		case "--files":
			files = strings.Split(args[i+1], ",")
		case "--status":
			statusFlag = args[i+1]
		case "--id":
			idFlag = args[i+1]
		case "--created":
			createdFlag = args[i+1]
		case "--completed":
			v := args[i+1]
			completedFlag = &v
		}
	}

	// Get next ID (or use provided)
	var newID string
	if idFlag != "" {
		newID = idFlag
	} else {
		var ids []string
		rows, err := s.DB.Query("SELECT id FROM tasks WHERE tree = ? AND spec = ?", tree, spec)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var id string
			rows.Scan(&id)
			ids = append(ids, id)
		}
		newID = store.NextID("t", ids)
	}

	today := createdFlag
	if today == "" {
		today = store.Today()
	}

	status := statusFlag
	if status == "" {
		status = "pending"
	}

	_, err = s.DB.Exec(
		"INSERT INTO tasks (tree, spec, id, type, summary, status, gate, created, completed) VALUES (?, ?, ?, 'task', ?, ?, ?, ?, ?)",
		tree, spec, newID, summary, status, gate, today, completedFlag,
	)
	if err != nil {
		return fmt.Errorf("inserting task: %w", err)
	}

	for _, dep := range dependsOn {
		s.DB.Exec("INSERT INTO task_deps (tree, spec, task_id, depends_on) VALUES (?, ?, ?, ?)",
			tree, spec, newID, strings.TrimSpace(dep))
	}
	for _, f := range files {
		s.DB.Exec("INSERT INTO task_files (tree, spec, task_id, path) VALUES (?, ?, ?, ?)",
			tree, spec, newID, strings.TrimSpace(f))
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): add task %s -- %s", tree, spec, newID, summary)); err != nil {
		return err
	}

	result := map[string]any{"id": newID, "tree": tree, "spec": spec, "summary": summary, "status": status}
	return jsonOut(result)
}

func cmdTaskList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist task list <tree/spec> [--human]")
	}

	// Check for --human flag
	human := false
	var filteredArgs []string
	for _, a := range args {
		if a == "--human" {
			human = true
		} else {
			filteredArgs = append(filteredArgs, a)
		}
	}

	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(filteredArgs[0])
	if err != nil {
		return err
	}

	rows, err := s.DB.Query(
		"SELECT id, type, summary, status, owner, created, completed, gate FROM tasks WHERE tree = ? AND spec = ? ORDER BY id",
		tree, spec,
	)
	if err != nil {
		return err
	}
	defer rows.Close()

	var tasks []taskListEntry
	for rows.Next() {
		var t taskListEntry
		rows.Scan(&t.id, &t.typ, &t.summary, &t.status, &t.owner, &t.created, &t.completed, &t.gate)
		depRows, _ := s.DB.Query("SELECT depends_on FROM task_deps WHERE tree = ? AND spec = ? AND task_id = ?", tree, spec, t.id)
		for depRows.Next() {
			var d string
			depRows.Scan(&d)
			t.deps = append(t.deps, d)
		}
		depRows.Close()
		tasks = append(tasks, t)
	}

	if human {
		return taskListHuman(tree, spec, tasks)
	}

	// JSON output
	var jsonTasks []map[string]any
	for _, t := range tasks {
		m := map[string]any{
			"id": t.id, "type": t.typ, "summary": t.summary,
			"status": t.status, "created": t.created,
		}
		if t.owner != nil {
			m["owner"] = *t.owner
		}
		if t.completed != nil {
			m["completed"] = *t.completed
		}
		if t.gate != nil {
			m["gate"] = *t.gate
		}
		if len(t.deps) > 0 {
			m["depends_on"] = t.deps
		}
		jsonTasks = append(jsonTasks, m)
	}
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "tasks": jsonTasks})
}

// taskListEntry holds parsed task data for rendering.
type taskListEntry struct {
	id, typ, summary, status, created string
	owner, completed, gate            *string
	deps                              []string
}

func taskListHuman(tree, spec string, tasks []taskListEntry) error {
	symbols := map[string]string{
		"pending": "○", "in_progress": "●", "done": "✓",
		"blocked": "⊘", "waiting": "◷", "cancelled": "✗",
	}

	done := 0
	for _, t := range tasks {
		if t.status == "done" {
			done++
		}
	}

	fmt.Fprintf(os.Stdout, "**%s/%s** -- %d/%d done\n\n", tree, spec, done, len(tasks))

	var rows [][]string
	for _, t := range tasks {
		sym := symbols[t.status]
		if sym == "" {
			sym = "?"
		}
		gate := ""
		if t.gate != nil && *t.gate != "" {
			gate = "🔒"
		}
		depStr := ""
		if len(t.deps) > 0 {
			depStr = strings.Join(t.deps, ", ")
		}
		rows = append(rows, []string{sym, t.id, gate, t.summary, depStr})
	}

	table := tablewriter.NewTable(os.Stdout,
		tablewriter.WithHeader([]string{"", "ID", "Gate", "Summary", "Deps"}),
		tablewriter.WithRenderer(renderer.NewMarkdown()),
	)
	for _, row := range rows {
		table.Append(row)
	}
	table.Render()
	return nil
}

func cmdTaskClaim(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task claim <tree/spec> <task-id>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	taskID := args[1]

	// Check current status
	var status string
	var owner *string
	err = s.DB.QueryRow("SELECT status, owner FROM tasks WHERE tree = ? AND spec = ? AND id = ?",
		tree, spec, taskID).Scan(&status, &owner)
	if err != nil {
		return fmt.Errorf("task not found: %s/%s/%s", tree, spec, taskID)
	}
	if status != "pending" {
		return fmt.Errorf("task %s is %s, not pending", taskID, status)
	}
	if owner != nil && *owner != "" {
		return fmt.Errorf("task %s already owned by %s", taskID, *owner)
	}

	// Check deps are done
	depRows, _ := s.DB.Query(
		"SELECT d.depends_on, t.status FROM task_deps d JOIN tasks t ON d.tree = t.tree AND d.spec = t.spec AND d.depends_on = t.id WHERE d.tree = ? AND d.spec = ? AND d.task_id = ?",
		tree, spec, taskID,
	)
	defer depRows.Close()
	for depRows.Next() {
		var depID, depStatus string
		depRows.Scan(&depID, &depStatus)
		if depStatus != "done" {
			return fmt.Errorf("dependency %s is %s, not done", depID, depStatus)
		}
	}

	ownerName := "synthesist"
	_, err = s.DB.Exec("UPDATE tasks SET status = 'in_progress', owner = ? WHERE tree = ? AND spec = ? AND id = ?",
		ownerName, tree, spec, taskID)
	if err != nil {
		return err
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
	defer s.Close()

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
		defer acRows.Close()

		for acRows.Next() {
			var seq int
			var criterion, verifyCmd string
			acRows.Scan(&seq, &criterion, &verifyCmd)

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
		_, err = s.DB.Exec(
			"UPDATE tasks SET status = 'pending', owner = NULL, failure_note = ? WHERE tree = ? AND spec = ? AND id = ?",
			note, tree, spec, taskID,
		)
	}

	return jsonOut(map[string]any{
		"id": taskID, "all_passed": allPass,
		"status": map[bool]string{true: "done", false: "pending"}[allPass],
		"criteria": results,
	})
}

func cmdTaskWait(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task wait <tree/spec> <task-id> --reason '...' --external 'url' --check 'cmd' [--check-after YYYY-MM-DD]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

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

func cmdTaskBlock(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task block <tree/spec> <task-id>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

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

	s.Commit(fmt.Sprintf("spec(%s/%s): %s blocked", tree, spec, taskID))
	return jsonOut(map[string]any{"id": taskID, "status": "blocked"})
}

func cmdTaskReady(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist task ready <tree/spec>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}

	// Tasks that are pending AND have all dependencies done (or no dependencies)
	rows, err := s.DB.Query(`
		SELECT t.id, t.type, t.summary, t.gate
		FROM tasks t
		WHERE t.tree = ? AND t.spec = ? AND t.status = 'pending'
		AND NOT EXISTS (
			SELECT 1 FROM task_deps d
			JOIN tasks dep ON d.tree = dep.tree AND d.spec = dep.spec AND d.depends_on = dep.id
			WHERE d.tree = t.tree AND d.spec = t.spec AND d.task_id = t.id
			AND dep.status != 'done'
		)
		ORDER BY t.id
	`, tree, spec)
	if err != nil {
		return err
	}
	defer rows.Close()

	var ready []map[string]any
	for rows.Next() {
		var id, typ, summary string
		var gate *string
		rows.Scan(&id, &typ, &summary, &gate)
		t := map[string]any{"id": id, "type": typ, "summary": summary}
		if gate != nil {
			t["gate"] = *gate
		}
		ready = append(ready, t)
	}

	return jsonOut(map[string]any{"tree": tree, "spec": spec, "ready": ready, "count": len(ready)})
}

func cmdTaskAcceptance(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task acceptance <tree/spec> <task-id> --criterion '...' --verify 'cmd'")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree, spec, err := parseTreeSpec(args[0])
	if err != nil {
		return err
	}
	taskID := args[1]

	var criterion, verify string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--criterion":
			criterion = args[i+1]
		case "--verify":
			verify = args[i+1]
		}
	}
	if criterion == "" || verify == "" {
		return fmt.Errorf("--criterion and --verify are required")
	}

	var maxSeq int
	s.DB.QueryRow("SELECT COALESCE(MAX(seq), 0) FROM acceptance WHERE tree = ? AND spec = ? AND task_id = ?",
		tree, spec, taskID).Scan(&maxSeq)

	_, err = s.DB.Exec("INSERT INTO acceptance (tree, spec, task_id, seq, criterion, verify_cmd) VALUES (?, ?, ?, ?, ?, ?)",
		tree, spec, taskID, maxSeq+1, criterion, verify)
	if err != nil {
		return fmt.Errorf("adding acceptance criterion: %w", err)
	}

	s.Commit(fmt.Sprintf("spec(%s/%s): acceptance on %s", tree, spec, taskID))
	return jsonOut(map[string]any{"task": taskID, "seq": maxSeq + 1, "criterion": criterion})
}

func cmdTaskCancel(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist task cancel <tree/spec> <task-id> [--reason '...']")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

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

	s.Commit(fmt.Sprintf("spec(%s/%s): cancel %s", tree, spec, taskID))
	return jsonOut(map[string]any{"id": taskID, "status": "cancelled"})
}

// --- Helpers ---

func parseTreeSpec(s string) (string, string, error) {
	parts := strings.SplitN(s, "/", 2)
	if len(parts) != 2 {
		return "", "", fmt.Errorf("expected tree/spec format, got %q", s)
	}
	return parts[0], parts[1], nil
}

func jsonOut(v any) error {
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	return enc.Encode(v)
}
