package main

import (
	"fmt"
	"strings"
)

func cmdCampaignActive(c *CampaignActiveCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree
	specID := c.SpecID

	var phasePtr *string
	if c.Phase != "" {
		phasePtr = &c.Phase
	}

	_, err = s.DB.Exec("INSERT INTO campaign_active (tree, spec_id, summary, phase) VALUES (?, ?, ?, ?)",
		tree, specID, c.Summary, phasePtr)
	if err != nil {
		return fmt.Errorf("adding to active campaign: %w", err)
	}

	if c.BlockedBy != "" {
		for _, b := range strings.Split(c.BlockedBy, ",") {
			if _, err := s.DB.Exec("INSERT INTO campaign_blocked_by (tree, spec_id, blocked_by) VALUES (?, ?, ?)",
				tree, specID, strings.TrimSpace(b)); err != nil {
				return fmt.Errorf("inserting blocked_by: %w", err)
			}
		}
	}

	if err := s.Commit(fmt.Sprintf("campaign(%s): add active %s", tree, specID)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"tree": tree, "spec_id": specID, "status": "active"})
}

func cmdCampaignBacklog(c *CampaignBacklogCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree
	specID := c.SpecID

	_, err = s.DB.Exec("INSERT INTO campaign_backlog (tree, spec_id, title, summary) VALUES (?, ?, ?, ?)",
		tree, specID, c.Title, c.Summary)
	if err != nil {
		return fmt.Errorf("adding to backlog: %w", err)
	}

	if c.BlockedBy != "" {
		for _, b := range strings.Split(c.BlockedBy, ",") {
			if _, err := s.DB.Exec("INSERT INTO campaign_blocked_by (tree, spec_id, blocked_by) VALUES (?, ?, ?)",
				tree, specID, strings.TrimSpace(b)); err != nil {
				return fmt.Errorf("inserting blocked_by: %w", err)
			}
		}
	}

	if err := s.Commit(fmt.Sprintf("campaign(%s): add backlog %s", tree, specID)); err != nil {
		return err
	}
	return jsonOut(map[string]any{"tree": tree, "spec_id": specID, "status": "backlog"})
}

func cmdCampaignList(c *CampaignListCmd) error {
	s, err := discoverStore()
	if err != nil {
		return err
	}
	defer s.Close() //nolint:errcheck

	tree := c.Tree

	// Active
	rows, err := s.DB.Query("SELECT spec_id, summary, phase FROM campaign_active WHERE tree = ? ORDER BY spec_id", tree)
	if err != nil {
		return fmt.Errorf("querying active campaigns: %w", err)
	}
	active := make([]map[string]any, 0)
	for rows.Next() {
		var specID, summary string
		var phase *string
		if err := rows.Scan(&specID, &summary, &phase); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		a := map[string]any{"spec_id": specID, "summary": summary}
		if phase != nil {
			a["phase"] = *phase
		}
		// blocked_by
		bRows, bErr := s.DB.Query("SELECT blocked_by FROM campaign_blocked_by WHERE tree = ? AND spec_id = ?", tree, specID)
		if bErr != nil {
			return fmt.Errorf("querying blocked_by: %w", bErr)
		}
		var blocked []string
		for bRows.Next() {
			var b string
			if err := bRows.Scan(&b); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}
			blocked = append(blocked, b)
		}
		if err := bRows.Err(); err != nil {
			return fmt.Errorf("iterating rows: %w", err)
		}
		_ = bRows.Close()
		if len(blocked) > 0 {
			a["blocked_by"] = blocked
		}
		active = append(active, a)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()

	// Backlog
	rows, err = s.DB.Query("SELECT spec_id, title, summary FROM campaign_backlog WHERE tree = ? ORDER BY spec_id", tree)
	if err != nil {
		return fmt.Errorf("querying backlog campaigns: %w", err)
	}
	backlog := make([]map[string]any, 0)
	for rows.Next() {
		var specID, title, summary string
		if err := rows.Scan(&specID, &title, &summary); err != nil {
			return fmt.Errorf("scanning row: %w", err)
		}
		b := map[string]any{"spec_id": specID, "title": title, "summary": summary}
		bRows, bErr := s.DB.Query("SELECT blocked_by FROM campaign_blocked_by WHERE tree = ? AND spec_id = ?", tree, specID)
		if bErr != nil {
			return fmt.Errorf("querying blocked_by: %w", bErr)
		}
		var blocked []string
		for bRows.Next() {
			var bl string
			if err := bRows.Scan(&bl); err != nil {
				return fmt.Errorf("scanning row: %w", err)
			}
			blocked = append(blocked, bl)
		}
		if err := bRows.Err(); err != nil {
			return fmt.Errorf("iterating rows: %w", err)
		}
		_ = bRows.Close()
		if len(blocked) > 0 {
			b["blocked_by"] = blocked
		}
		backlog = append(backlog, b)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterating rows: %w", err)
	}
	_ = rows.Close()

	return jsonOut(map[string]any{"tree": tree, "active": active, "backlog": backlog})
}
