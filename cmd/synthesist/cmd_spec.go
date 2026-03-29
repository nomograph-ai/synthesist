package main

import (
	"fmt"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdSpec(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist spec <create|show|update> ...") //nolint:staticcheck
	}
	switch args[0] {
	case "create":
		return cmdSpecCreate(args[1:])
	case "show":
		return cmdSpecShow(args[1:])
	case "update":
		return cmdSpecUpdate(args[1:])
	default:
		return fmt.Errorf("unknown spec subcommand: %s", args[0])
	}
}

func cmdSpecCreate(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist spec create <tree/spec> --goal '...' [--constraints '...'] [--decisions '...']")
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

	var goal, constraints, decisions string
	for i := 1; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--goal":
			goal = args[i+1]
		case "--constraints":
			constraints = args[i+1]
		case "--decisions":
			decisions = args[i+1]
		}
	}

	var goalPtr, constraintsPtr, decisionsPtr *string
	if goal != "" {
		goalPtr = &goal
	}
	if constraints != "" {
		constraintsPtr = &constraints
	}
	if decisions != "" {
		decisionsPtr = &decisions
	}

	_, err = s.DB.Exec(
		"INSERT INTO specs (tree, id, goal, constraints, decisions, created) VALUES (?, ?, ?, ?, ?, ?)",
		tree, spec, goalPtr, constraintsPtr, decisionsPtr, store.Today(),
	)
	if err != nil {
		return fmt.Errorf("creating spec: %w", err)
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): create", tree, spec)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "goal": goal, "created": store.Today()})
}

func cmdSpecShow(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist spec show <tree/spec>")
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

	var goal, constraints, decisions *string
	var created *string
	err = s.DB.QueryRow("SELECT goal, constraints, decisions, created FROM specs WHERE tree = ? AND id = ?",
		tree, spec).Scan(&goal, &constraints, &decisions, &created)
	if err != nil {
		return fmt.Errorf("spec not found: %s/%s", tree, spec)
	}

	result := map[string]any{"tree": tree, "spec": spec}
	if goal != nil {
		result["goal"] = *goal
	}
	if constraints != nil {
		result["constraints"] = *constraints
	}
	if decisions != nil {
		result["decisions"] = *decisions
	}
	if created != nil {
		result["created"] = *created
	}

	// Task summary
	var pending, inProgress, done, blocked, waiting int
	if err := s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE tree = ? AND spec = ? AND status = 'pending'", tree, spec).Scan(&pending); err != nil {
		return fmt.Errorf("scanning count: %w", err)
	}
	if err := s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE tree = ? AND spec = ? AND status = 'in_progress'", tree, spec).Scan(&inProgress); err != nil {
		return fmt.Errorf("scanning count: %w", err)
	}
	if err := s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE tree = ? AND spec = ? AND status = 'done'", tree, spec).Scan(&done); err != nil {
		return fmt.Errorf("scanning count: %w", err)
	}
	if err := s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE tree = ? AND spec = ? AND status = 'blocked'", tree, spec).Scan(&blocked); err != nil {
		return fmt.Errorf("scanning count: %w", err)
	}
	if err := s.DB.QueryRow("SELECT COUNT(*) FROM tasks WHERE tree = ? AND spec = ? AND status = 'waiting'", tree, spec).Scan(&waiting); err != nil {
		return fmt.Errorf("scanning count: %w", err)
	}
	result["task_counts"] = map[string]int{
		"pending": pending, "in_progress": inProgress, "done": done,
		"blocked": blocked, "waiting": waiting,
	}

	// Propagation: what needs updates when this spec changes?
	rows, _ := s.DB.Query(
		"SELECT target_tree, target_spec, seq, description FROM propagation_chain WHERE source_tree = ? AND source_spec = ? ORDER BY seq",
		tree, spec,
	)
	var propagates []map[string]any
	for rows.Next() {
		var targetTree, targetSpec string
		var seq int
		var desc *string
		if err := rows.Scan(&targetTree, &targetSpec, &seq, &desc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		p := map[string]any{"target_tree": targetTree, "target_spec": targetSpec, "seq": seq}
		if desc != nil {
			p["description"] = *desc
		}
		propagates = append(propagates, p)
	}
	_ = rows.Close()
	if len(propagates) > 0 {
		result["propagates_to"] = propagates
	}

	// Propagation: what specs' changes affect this spec?
	rows, _ = s.DB.Query(
		"SELECT source_tree, source_spec, description FROM propagation_chain WHERE target_tree = ? AND target_spec = ? ORDER BY seq",
		tree, spec,
	)
	var affectedBy []map[string]any
	for rows.Next() {
		var sourceTree, sourceSpec string
		var desc *string
		if err := rows.Scan(&sourceTree, &sourceSpec, &desc); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		a := map[string]any{"source_tree": sourceTree, "source_spec": sourceSpec}
		if desc != nil {
			a["description"] = *desc
		}
		affectedBy = append(affectedBy, a)
	}
	_ = rows.Close()
	if len(affectedBy) > 0 {
		result["affected_by"] = affectedBy
	}

	return jsonOut(result)
}

func cmdSpecUpdate(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist spec update <tree/spec> [--goal '...'] [--constraints '...'] [--decisions '...']")
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

	// Check exists
	var existing string
	err = s.DB.QueryRow("SELECT id FROM specs WHERE tree = ? AND id = ?", tree, spec).Scan(&existing)
	if err != nil {
		return fmt.Errorf("spec not found: %s/%s", tree, spec)
	}

	for i := 1; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--goal":
			if _, err := s.DB.Exec("UPDATE specs SET goal = ? WHERE tree = ? AND id = ?", args[i+1], tree, spec); err != nil {
				return fmt.Errorf("updating goal: %w", err)
			}
		case "--constraints":
			if _, err := s.DB.Exec("UPDATE specs SET constraints = ? WHERE tree = ? AND id = ?", args[i+1], tree, spec); err != nil {
				return fmt.Errorf("updating constraints: %w", err)
			}
		case "--decisions":
			if _, err := s.DB.Exec("UPDATE specs SET decisions = ? WHERE tree = ? AND id = ?", args[i+1], tree, spec); err != nil {
				return fmt.Errorf("updating decisions: %w", err)
			}
		}
	}

	if err := s.Commit(fmt.Sprintf("spec(%s/%s): update", tree, spec)); err != nil {
		return err
	}
	return cmdSpecShow([]string{args[0]})
}
