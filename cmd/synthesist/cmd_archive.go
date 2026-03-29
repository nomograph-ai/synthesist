package main

import (
	"fmt"
	"strings"

	"gitlab.com/nomograph/synthesist/internal/store"
)

func cmdArchiveAdd(c *ArchiveAddCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree, spec, err := parseTreeSpec(c.TreeSpec)
	if err != nil {
		return err
	}

	reason := c.Reason
	archived := c.Archived
	if archived == "" {
		archived = store.Today()
	}

	var outcomePtr *string
	if c.Outcome != "" {
		outcomePtr = &c.Outcome
	}

	_, err = s.DB.Exec("INSERT INTO archives (tree, spec_id, archived, reason, outcome) VALUES (?, ?, ?, ?, ?)",
		tree, spec, archived, reason, outcomePtr)
	if err != nil {
		return fmt.Errorf("archiving: %w", err)
	}

	if c.Patterns != "" {
		for _, p := range strings.Split(c.Patterns, ",") {
			if _, err := s.DB.Exec("INSERT INTO archive_patterns (tree, spec_id, pattern_id) VALUES (?, ?, ?)",
				tree, spec, strings.TrimSpace(p)); err != nil {
				return fmt.Errorf("inserting archive pattern: %w", err)
			}
		}
	}

	if err := s.Commit(fmt.Sprintf("archive(%s/%s): %s", tree, spec, reason)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"tree": tree, "spec": spec, "reason": reason, "archived": archived})
}

func cmdArchiveList(c *ArchiveListCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree
	rows, err := s.DB.Query("SELECT spec_id, archived, reason, outcome FROM archives WHERE tree = ? ORDER BY archived DESC", tree)
	if err != nil {
		return err
	}
	defer rows.Close() //nolint:errcheck

	archives := make([]map[string]any, 0)
	for rows.Next() {
		var specID, archived, reason string
		var outcome *string
		if err := rows.Scan(&specID, &archived, &reason, &outcome); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		a := map[string]any{"spec_id": specID, "archived": archived, "reason": reason}
		if outcome != nil {
			a["outcome"] = *outcome
		}
		archives = append(archives, a)
	}
	return jsonOut(map[string]any{"tree": tree, "archives": archives})
}
