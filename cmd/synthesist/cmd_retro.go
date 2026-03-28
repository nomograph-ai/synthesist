package main

import (
	"fmt"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdRetro(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist retro <create|show|transform> ...")
	}
	switch args[0] {
	case "create":
		return cmdRetroCreate(args[1:])
	case "show":
		return cmdRetroShow(args[1:])
	case "transform":
		return cmdRetroTransform(args[1:])
	default:
		return fmt.Errorf("unknown retro subcommand: %s", args[0])
	}
}

func cmdRetroCreate(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist retro create <tree/spec> --arc '...' --depends-on t8[,t9]")
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

	var arc string
	var dependsOn []string
	for i := 1; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--arc":
			arc = args[i+1]
		case "--depends-on":
			dependsOn = strings.Split(args[i+1], ",")
		}
	}
	if arc == "" {
		return fmt.Errorf("--arc is required")
	}

	today := store.Today()

	// Compute duration if possible
	var createdDate string
	s.DB.QueryRow("SELECT MIN(created) FROM tasks WHERE tree = ? AND spec = ? AND type = 'task'",
		tree, spec).Scan(&createdDate)

	_, err = s.DB.Exec(
		"INSERT INTO tasks (tree, spec, id, type, summary, status, created, completed, arc) VALUES (?, ?, 'retro', 'retro', ?, 'done', ?, ?, ?)",
		tree, spec, "Retrospective: "+spec, today, today, arc,
	)
	if err != nil {
		return fmt.Errorf("inserting retro: %w", err)
	}

	for _, dep := range dependsOn {
		s.DB.Exec("INSERT INTO task_deps (tree, spec, task_id, depends_on) VALUES (?, ?, 'retro', ?)",
			tree, spec, strings.TrimSpace(dep))
	}

	s.Commit(fmt.Sprintf("retro(%s/%s): create retrospective", tree, spec))
	return jsonOut(map[string]any{
		"id": "retro", "type": "retro", "tree": tree, "spec": spec,
		"arc": arc, "status": "done",
		"next": "use 'synthesist retro transform' to add transforms, then 'synthesist pattern register' for reusable patterns",
	})
}

func cmdRetroTransform(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist retro transform <tree/spec> --label '...' --description '...' [--transferable]")
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

	var label, description string
	transferable := false
	for i := 1; i < len(args); i++ {
		switch args[i] {
		case "--label":
			if i+1 < len(args) {
				label = args[i+1]
				i++
			}
		case "--description":
			if i+1 < len(args) {
				description = args[i+1]
				i++
			}
		case "--transferable":
			transferable = true
		}
	}
	if label == "" || description == "" {
		return fmt.Errorf("--label and --description are required")
	}

	// Get next seq
	var maxSeq int
	s.DB.QueryRow("SELECT COALESCE(MAX(seq), 0) FROM transforms WHERE tree = ? AND spec = ? AND task_id = 'retro'",
		tree, spec).Scan(&maxSeq)

	_, err = s.DB.Exec(
		"INSERT INTO transforms (tree, spec, task_id, seq, label, description, transferable) VALUES (?, ?, 'retro', ?, ?, ?, ?)",
		tree, spec, maxSeq+1, label, description, transferable,
	)
	if err != nil {
		return fmt.Errorf("inserting transform: %w", err)
	}

	s.Commit(fmt.Sprintf("retro(%s/%s): transform -- %s", tree, spec, label))
	return jsonOut(map[string]any{
		"seq": maxSeq + 1, "label": label, "transferable": transferable,
	})
}

func cmdRetroShow(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist retro show <tree/spec>")
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

	var arc string
	var created, completed *string
	err = s.DB.QueryRow("SELECT arc, created, completed FROM tasks WHERE tree = ? AND spec = ? AND id = 'retro'",
		tree, spec).Scan(&arc, &created, &completed)
	if err != nil {
		return fmt.Errorf("no retro found for %s/%s", tree, spec)
	}

	result := map[string]any{"tree": tree, "spec": spec, "arc": arc}
	if created != nil {
		result["created"] = *created
	}
	if completed != nil {
		result["completed"] = *completed
	}

	// Transforms
	rows, _ := s.DB.Query(
		"SELECT seq, label, description, transferable FROM transforms WHERE tree = ? AND spec = ? AND task_id = 'retro' ORDER BY seq",
		tree, spec,
	)
	var transforms []map[string]any
	for rows.Next() {
		var seq int
		var label, desc string
		var transferable bool
		rows.Scan(&seq, &label, &desc, &transferable)
		transforms = append(transforms, map[string]any{
			"seq": seq, "label": label, "description": desc, "transferable": transferable,
		})
	}
	rows.Close()
	result["transforms"] = transforms

	// Linked patterns
	rows, _ = s.DB.Query(
		"SELECT tp.pattern_id, p.name, p.description FROM task_patterns tp JOIN patterns p ON tp.pattern_id = p.id AND tp.tree = p.tree WHERE tp.tree = ? AND tp.spec = ? AND tp.task_id = 'retro'",
		tree, spec,
	)
	var patterns []map[string]any
	for rows.Next() {
		var id, name, desc string
		rows.Scan(&id, &name, &desc)
		patterns = append(patterns, map[string]any{"id": id, "name": name, "description": desc})
	}
	rows.Close()
	result["patterns"] = patterns

	return jsonOut(result)
}

func cmdPattern(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist pattern <register|list> ...")
	}
	switch args[0] {
	case "register":
		return cmdPatternRegister(args[1:])
	case "list":
		return cmdPatternList(args[1:])
	default:
		return fmt.Errorf("unknown pattern subcommand: %s", args[0])
	}
}

func cmdPatternRegister(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist pattern register <tree> <id> --name '...' --description '...' [--transferability '...'] [--observed-in spec1,spec2]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree := args[0]
	patternID := args[1]

	var name, description, transferability string
	var observedIn []string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--name":
			name = args[i+1]
		case "--description":
			description = args[i+1]
		case "--transferability":
			transferability = args[i+1]
		case "--observed-in":
			observedIn = strings.Split(args[i+1], ",")
		}
	}
	if name == "" || description == "" {
		return fmt.Errorf("--name and --description are required")
	}

	var transferPtr *string
	if transferability != "" {
		transferPtr = &transferability
	}

	_, err = s.DB.Exec(
		"INSERT INTO patterns (tree, id, name, description, transferability, first_observed) VALUES (?, ?, ?, ?, ?, ?)",
		tree, patternID, name, description, transferPtr, store.Today(),
	)
	if err != nil {
		return fmt.Errorf("inserting pattern: %w", err)
	}

	for _, obs := range observedIn {
		s.DB.Exec("INSERT INTO pattern_observations (tree, pattern_id, observed_in) VALUES (?, ?, ?)",
			tree, patternID, strings.TrimSpace(obs))
	}

	s.Commit(fmt.Sprintf("pattern(%s): register %s -- %s", tree, patternID, name))
	return jsonOut(map[string]any{"tree": tree, "id": patternID, "name": name})
}

func cmdPatternList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist pattern list <tree>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree := args[0]
	rows, err := s.DB.Query("SELECT id, name, description, transferability, first_observed FROM patterns WHERE tree = ? ORDER BY first_observed DESC", tree)
	if err != nil {
		return err
	}
	defer rows.Close()

	var patterns []map[string]any
	for rows.Next() {
		var id, name, desc, firstObs string
		var transferability *string
		rows.Scan(&id, &name, &desc, &transferability, &firstObs)
		p := map[string]any{"id": id, "name": name, "description": desc, "first_observed": firstObs}
		if transferability != nil {
			p["transferability"] = *transferability
		}
		// Get observations
		obsRows, _ := s.DB.Query("SELECT observed_in FROM pattern_observations WHERE tree = ? AND pattern_id = ?", tree, id)
		var obs []string
		for obsRows.Next() {
			var o string
			obsRows.Scan(&o)
			obs = append(obs, o)
		}
		obsRows.Close()
		if len(obs) > 0 {
			p["observed_in"] = obs
		}
		patterns = append(patterns, p)
	}
	return jsonOut(map[string]any{"tree": tree, "patterns": patterns})
}

func cmdReplay(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist replay <tree/spec>")
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

	result := map[string]any{"tree": tree, "spec": spec}

	// Task DAG shape
	rows, _ := s.DB.Query(
		"SELECT id, type, summary, status, arc FROM tasks WHERE tree = ? AND spec = ? ORDER BY id", tree, spec)
	var tasks []map[string]any
	for rows.Next() {
		var id, typ, summary, status string
		var arc *string
		rows.Scan(&id, &typ, &summary, &status, &arc)
		t := map[string]any{"id": id, "type": typ, "summary": summary, "status": status}
		if arc != nil {
			t["arc"] = *arc
		}
		// Deps
		depRows, _ := s.DB.Query("SELECT depends_on FROM task_deps WHERE tree = ? AND spec = ? AND task_id = ?", tree, spec, id)
		var deps []string
		for depRows.Next() {
			var d string
			depRows.Scan(&d)
			deps = append(deps, d)
		}
		depRows.Close()
		if len(deps) > 0 {
			t["depends_on"] = deps
		}
		tasks = append(tasks, t)
	}
	rows.Close()
	result["task_dag"] = tasks

	// Retro transforms
	tRows, _ := s.DB.Query(
		"SELECT label, description, transferable FROM transforms WHERE tree = ? AND spec = ? AND task_id = 'retro' ORDER BY seq",
		tree, spec,
	)
	var transforms []map[string]any
	for tRows.Next() {
		var label, desc string
		var transferable bool
		tRows.Scan(&label, &desc, &transferable)
		transforms = append(transforms, map[string]any{
			"label": label, "description": desc, "transferable": transferable,
		})
	}
	tRows.Close()
	result["transforms"] = transforms

	// Patterns
	rows, _ = s.DB.Query(
		"SELECT tp.pattern_id, p.name, p.description FROM task_patterns tp JOIN patterns p ON tp.tree = p.tree AND tp.pattern_id = p.id WHERE tp.tree = ? AND tp.spec = ? AND tp.task_id = 'retro'",
		tree, spec,
	)
	var patterns []map[string]any
	for rows.Next() {
		var id, name, desc string
		rows.Scan(&id, &name, &desc)
		patterns = append(patterns, map[string]any{"id": id, "name": name, "description": desc})
	}
	rows.Close()
	result["patterns"] = patterns

	// Landscape summary
	rows, _ = s.DB.Query(
		"SELECT d.stakeholder_id, d.topic, d.stance, d.confidence, d.preferred_approach FROM dispositions d WHERE d.tree = ? AND d.spec = ? AND d.valid_until IS NULL",
		tree, spec,
	)
	var landscape []map[string]any
	for rows.Next() {
		var stakeholder, topic, stance, confidence string
		var preferred *string
		rows.Scan(&stakeholder, &topic, &stance, &confidence, &preferred)
		l := map[string]any{"stakeholder": stakeholder, "topic": topic, "stance": stance, "confidence": confidence}
		if preferred != nil {
			l["preferred_approach"] = *preferred
		}
		landscape = append(landscape, l)
	}
	rows.Close()
	result["landscape"] = landscape

	return jsonOut(result)
}
