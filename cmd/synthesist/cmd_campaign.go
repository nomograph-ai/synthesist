package main

import (
	"fmt"
	"strings"
)

func cmdCampaign(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist campaign <active|backlog|list> ...")
	}
	switch args[0] {
	case "active":
		return cmdCampaignActive(args[1:])
	case "backlog":
		return cmdCampaignBacklog(args[1:])
	case "list":
		return cmdCampaignList(args[1:])
	default:
		return fmt.Errorf("unknown campaign subcommand: %s", args[0])
	}
}

func cmdCampaignActive(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist campaign active <tree> <spec-id> [--summary '...'] [--phase '...'] [--blocked-by spec1,spec2]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree := args[0]
	specID := args[1]
	var summary, phase string
	var blockedBy []string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--summary":
			summary = args[i+1]
		case "--phase":
			phase = args[i+1]
		case "--blocked-by":
			blockedBy = strings.Split(args[i+1], ",")
		}
	}

	var phasePtr *string
	if phase != "" {
		phasePtr = &phase
	}

	_, err = s.DB.Exec("INSERT INTO campaign_active (tree, spec_id, summary, phase) VALUES (?, ?, ?, ?)",
		tree, specID, summary, phasePtr)
	if err != nil {
		return fmt.Errorf("adding to active campaign: %w", err)
	}

	for _, b := range blockedBy {
		s.DB.Exec("INSERT INTO campaign_blocked_by (tree, spec_id, blocked_by) VALUES (?, ?, ?)",
			tree, specID, strings.TrimSpace(b))
	}

	s.Commit(fmt.Sprintf("campaign(%s): add active %s", tree, specID))
	return jsonOut(map[string]any{"tree": tree, "spec_id": specID, "status": "active"})
}

func cmdCampaignBacklog(args []string) error {
	if len(args) < 2 {
		return fmt.Errorf("usage: synthesist campaign backlog <tree> <spec-id> [--title '...'] [--summary '...'] [--blocked-by spec1,spec2]")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree := args[0]
	specID := args[1]
	var title, summary string
	var blockedBy []string
	for i := 2; i < len(args)-1; i += 2 {
		switch args[i] {
		case "--title":
			title = args[i+1]
		case "--summary":
			summary = args[i+1]
		case "--blocked-by":
			blockedBy = strings.Split(args[i+1], ",")
		}
	}

	_, err = s.DB.Exec("INSERT INTO campaign_backlog (tree, spec_id, title, summary) VALUES (?, ?, ?, ?)",
		tree, specID, title, summary)
	if err != nil {
		return fmt.Errorf("adding to backlog: %w", err)
	}

	for _, b := range blockedBy {
		s.DB.Exec("INSERT INTO campaign_blocked_by (tree, spec_id, blocked_by) VALUES (?, ?, ?)",
			tree, specID, strings.TrimSpace(b))
	}

	s.Commit(fmt.Sprintf("campaign(%s): add backlog %s", tree, specID))
	return jsonOut(map[string]any{"tree": tree, "spec_id": specID, "status": "backlog"})
}

func cmdCampaignList(args []string) error {
	if len(args) < 1 {
		return fmt.Errorf("usage: synthesist campaign list <tree>")
	}
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close()

	tree := args[0]

	// Active
	rows, _ := s.DB.Query("SELECT spec_id, summary, phase FROM campaign_active WHERE tree = ? ORDER BY spec_id", tree)
	active := make([]map[string]any, 0)
	for rows.Next() {
		var specID, summary string
		var phase *string
		rows.Scan(&specID, &summary, &phase)
		a := map[string]any{"spec_id": specID, "summary": summary}
		if phase != nil {
			a["phase"] = *phase
		}
		// blocked_by
		bRows, _ := s.DB.Query("SELECT blocked_by FROM campaign_blocked_by WHERE tree = ? AND spec_id = ?", tree, specID)
		var blocked []string
		for bRows.Next() {
			var b string
			bRows.Scan(&b)
			blocked = append(blocked, b)
		}
		bRows.Close()
		if len(blocked) > 0 {
			a["blocked_by"] = blocked
		}
		active = append(active, a)
	}
	rows.Close()

	// Backlog
	rows, _ = s.DB.Query("SELECT spec_id, title, summary FROM campaign_backlog WHERE tree = ? ORDER BY spec_id", tree)
	backlog := make([]map[string]any, 0)
	for rows.Next() {
		var specID, title, summary string
		rows.Scan(&specID, &title, &summary)
		b := map[string]any{"spec_id": specID, "title": title, "summary": summary}
		bRows, _ := s.DB.Query("SELECT blocked_by FROM campaign_blocked_by WHERE tree = ? AND spec_id = ?", tree, specID)
		var blocked []string
		for bRows.Next() {
			var bl string
			bRows.Scan(&bl)
			blocked = append(blocked, bl)
		}
		bRows.Close()
		if len(blocked) > 0 {
			b["blocked_by"] = blocked
		}
		backlog = append(backlog, b)
	}
	rows.Close()

	return jsonOut(map[string]any{"tree": tree, "active": active, "backlog": backlog})
}
