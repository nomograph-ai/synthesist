// Package store manages the Dolt embedded database.
// All spec graph data flows through here. The store is the single
// write path -- LLMs and humans use synthesist commands, synthesist uses store.
package store

import (
	"database/sql"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	_ "github.com/dolthub/driver"
)

// Store manages the Dolt database and git operations.
type Store struct {
	Root       string  // project root
	DBPath     string  // path to .synth/ directory
	DB         *sql.DB
	AutoCommit bool
}

// Open opens an existing synthesist database, or returns an error if not initialized.
func Open(root string) (*Store, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return nil, fmt.Errorf("resolving root: %w", err)
	}
	dbPath := filepath.Join(abs, ".synth")
	// Dolt creates .synth/synthesist/.dolt/ (database name = subdirectory)
	if _, err := os.Stat(filepath.Join(dbPath, "synthesist", ".dolt")); os.IsNotExist(err) {
		return nil, fmt.Errorf("no synthesist database at %s -- run 'synthesist init'", dbPath)
	}
	dsn := "file://" + dbPath + "?commitname=synthesist&commitemail=synthesist@synthesist&database=synthesist"
	db, err := sql.Open("dolt", dsn)
	if err != nil {
		return nil, fmt.Errorf("opening dolt database: %w", err)
	}
	return &Store{
		Root:       abs,
		DBPath:     dbPath,
		DB:         db,
		AutoCommit: true,
	}, nil
}

// Init creates a new synthesist database with the schema.
func Init(root string) (*Store, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return nil, fmt.Errorf("resolving root: %w", err)
	}
	dbPath := filepath.Join(abs, ".synth")
	if err := os.MkdirAll(dbPath, 0o755); err != nil {
		return nil, fmt.Errorf("creating .synth directory: %w", err)
	}

	// First open without database to create it
	bootDSN := "file://" + dbPath + "?commitname=synthesist&commitemail=synthesist@synthesist&create=true"
	bootDB, err := sql.Open("dolt", bootDSN)
	if err != nil {
		return nil, fmt.Errorf("opening dolt for bootstrap: %w", err)
	}
	if _, err := bootDB.Exec("CREATE DATABASE IF NOT EXISTS synthesist"); err != nil {
		bootDB.Close()
		return nil, fmt.Errorf("creating synthesist database: %w", err)
	}
	bootDB.Close()

	// Reopen with database selected
	dsn := "file://" + dbPath + "?commitname=synthesist&commitemail=synthesist@synthesist&database=synthesist"
	db, err := sql.Open("dolt", dsn)
	if err != nil {
		return nil, fmt.Errorf("opening dolt database: %w", err)
	}
	s := &Store{
		Root:       abs,
		DBPath:     dbPath,
		DB:         db,
		AutoCommit: true,
	}
	if err := s.createSchema(); err != nil {
		return nil, fmt.Errorf("creating schema: %w", err)
	}
	if err := s.doltCommit("synthesist init: create schema"); err != nil {
		return nil, fmt.Errorf("initial dolt commit: %w", err)
	}
	return s, nil
}

// Discover walks up from cwd to find a .synth/ directory.
func Discover() (*Store, error) {
	dir, err := os.Getwd()
	if err != nil {
		return nil, err
	}
	// Resolve symlinks (e.g. /tmp -> /private/tmp on macOS)
	dir, err = filepath.EvalSymlinks(dir)
	if err != nil {
		return nil, err
	}
	for {
		check := filepath.Join(dir, ".synth", "synthesist", ".dolt")
		if _, err := os.Stat(check); err == nil {
			return Open(dir)
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return nil, fmt.Errorf("no .synth database found in any parent directory -- run 'synthesist init'")
		}
		dir = parent
	}
}

// Close closes the database connection.
func (s *Store) Close() error {
	return s.DB.Close()
}

// doltCommit creates a Dolt commit (internal database versioning).
func (s *Store) doltCommit(message string) error {
	_, err := s.DB.Exec("CALL DOLT_ADD('-A')")
	if err != nil {
		return fmt.Errorf("dolt add: %w", err)
	}
	_, err = s.DB.Exec("CALL DOLT_COMMIT('-m', ?)", message)
	if err != nil {
		// "nothing to commit" is not an error
		if strings.Contains(err.Error(), "nothing to commit") {
			return nil
		}
		return fmt.Errorf("dolt commit: %w", err)
	}
	return nil
}

// GitCommit stages .synth/ and commits to the outer git repo.
func (s *Store) GitCommit(message string) error {
	if !s.AutoCommit {
		return nil
	}
	gitDir := filepath.Join(s.Root, ".git")
	if _, err := os.Stat(gitDir); os.IsNotExist(err) {
		return nil
	}
	cmd := exec.Command("git", "add", ".synth/")
	cmd.Dir = s.Root
	if out, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("git add: %s", string(out))
	}
	cmd = exec.Command("git", "commit", "-m", message)
	cmd.Dir = s.Root
	if out, err := cmd.CombinedOutput(); err != nil {
		if strings.Contains(string(out), "nothing to commit") {
			return nil
		}
		return fmt.Errorf("git commit: %s", string(out))
	}
	return nil
}

// Commit commits to both Dolt and git.
func (s *Store) Commit(message string) error {
	if err := s.doltCommit(message); err != nil {
		return err
	}
	return s.GitCommit(message)
}

// Today returns today's date as YYYY-MM-DD.
func Today() string {
	return time.Now().Format("2006-01-02")
}

// NextID generates the next sequential ID for a given prefix and existing items.
func NextID(prefix string, existing []string) string {
	max := 0
	for _, id := range existing {
		if strings.HasPrefix(id, prefix) {
			numStr := strings.TrimPrefix(id, prefix)
			var n int
			if _, err := fmt.Sscanf(numStr, "%d", &n); err == nil {
				if n > max {
					max = n
				}
			}
		}
	}
	return fmt.Sprintf("%s%d", prefix, max+1)
}

// createSchema sets up all tables.
func (s *Store) createSchema() error {
	stmts := []string{
		// --- Estate layer ---
		`CREATE TABLE IF NOT EXISTS trees (
			name VARCHAR(255) PRIMARY KEY,
			path VARCHAR(512) NOT NULL,
			status VARCHAR(32) NOT NULL DEFAULT 'active',
			description TEXT
		)`,
		`CREATE TABLE IF NOT EXISTS threads (
			id VARCHAR(512) PRIMARY KEY,
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255),
			task VARCHAR(64),
			date DATE NOT NULL,
			summary TEXT NOT NULL,
			waiter_reason TEXT,
			waiter_external TEXT,
			waiter_check TEXT,
			waiter_check_after DATE
		)`,

		// --- Task DAG layer ---
		`CREATE TABLE IF NOT EXISTS tasks (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			id VARCHAR(64) NOT NULL,
			type VARCHAR(16) NOT NULL DEFAULT 'task',
			summary TEXT NOT NULL,
			description TEXT,
			status VARCHAR(32) NOT NULL DEFAULT 'pending',
			gate VARCHAR(32),
			owner VARCHAR(255),
			created DATE NOT NULL,
			completed DATE,
			failure_note TEXT,
			waiter_reason TEXT,
			waiter_external TEXT,
			waiter_check TEXT,
			waiter_check_after DATE,
			arc TEXT,
			duration_days INT,
			PRIMARY KEY (tree, spec, id)
		)`,
		`CREATE TABLE IF NOT EXISTS task_deps (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			task_id VARCHAR(64) NOT NULL,
			depends_on VARCHAR(64) NOT NULL,
			PRIMARY KEY (tree, spec, task_id, depends_on)
		)`,
		`CREATE TABLE IF NOT EXISTS task_files (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			task_id VARCHAR(64) NOT NULL,
			path TEXT NOT NULL
		)`,
		`CREATE TABLE IF NOT EXISTS acceptance (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			task_id VARCHAR(64) NOT NULL,
			seq INT NOT NULL,
			criterion TEXT NOT NULL,
			verify_cmd TEXT NOT NULL,
			PRIMARY KEY (tree, spec, task_id, seq)
		)`,
		`CREATE TABLE IF NOT EXISTS transforms (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			task_id VARCHAR(64) NOT NULL,
			seq INT NOT NULL,
			label VARCHAR(255) NOT NULL,
			description TEXT NOT NULL,
			transferable BOOLEAN NOT NULL DEFAULT FALSE,
			PRIMARY KEY (tree, spec, task_id, seq)
		)`,
		`CREATE TABLE IF NOT EXISTS task_patterns (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			task_id VARCHAR(64) NOT NULL,
			pattern_id VARCHAR(255) NOT NULL,
			PRIMARY KEY (tree, spec, task_id, pattern_id)
		)`,

		// --- Landscape layer ---
		`CREATE TABLE IF NOT EXISTS stakeholders (
			tree VARCHAR(255) NOT NULL,
			id VARCHAR(255) NOT NULL,
			name VARCHAR(512),
			context TEXT NOT NULL,
			PRIMARY KEY (tree, id)
		)`,
		`CREATE TABLE IF NOT EXISTS stakeholder_orgs (
			tree VARCHAR(255) NOT NULL,
			stakeholder_id VARCHAR(255) NOT NULL,
			org VARCHAR(512) NOT NULL
		)`,
		`CREATE TABLE IF NOT EXISTS dispositions (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			id VARCHAR(64) NOT NULL,
			stakeholder_id VARCHAR(255) NOT NULL,
			topic TEXT NOT NULL,
			stance VARCHAR(32) NOT NULL,
			preferred_approach TEXT,
			detail TEXT,
			confidence VARCHAR(32) NOT NULL,
			evidence VARCHAR(64),
			valid_from DATE NOT NULL,
			valid_until DATE,
			superseded_by VARCHAR(64),
			PRIMARY KEY (tree, spec, id)
		)`,
		`CREATE TABLE IF NOT EXISTS signals (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			id VARCHAR(64) NOT NULL,
			stakeholder_id VARCHAR(255) NOT NULL,
			date DATE NOT NULL,
			recorded_date DATE NOT NULL,
			source TEXT NOT NULL,
			source_type VARCHAR(32) NOT NULL,
			content TEXT NOT NULL,
			interpretation TEXT,
			our_action TEXT,
			PRIMARY KEY (tree, spec, id)
		)`,
		`CREATE TABLE IF NOT EXISTS influences (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			stakeholder_id VARCHAR(255) NOT NULL,
			task_id VARCHAR(64) NOT NULL,
			role VARCHAR(32) NOT NULL,
			PRIMARY KEY (tree, spec, stakeholder_id, task_id)
		)`,

		// --- Pattern layer ---
		`CREATE TABLE IF NOT EXISTS patterns (
			tree VARCHAR(255) NOT NULL,
			id VARCHAR(255) NOT NULL,
			name VARCHAR(512) NOT NULL,
			description TEXT NOT NULL,
			transferability TEXT,
			first_observed DATE NOT NULL,
			PRIMARY KEY (tree, id)
		)`,
		`CREATE TABLE IF NOT EXISTS pattern_observations (
			tree VARCHAR(255) NOT NULL,
			pattern_id VARCHAR(255) NOT NULL,
			observed_in VARCHAR(512) NOT NULL
		)`,

		// --- Direction layer (upstream technical trajectories) ---
		`CREATE TABLE IF NOT EXISTS directions (
			tree VARCHAR(255) NOT NULL,
			id VARCHAR(64) NOT NULL,
			project VARCHAR(512) NOT NULL,
			topic TEXT NOT NULL,
			status VARCHAR(32) NOT NULL,
			owner VARCHAR(255),
			timeline TEXT,
			detail TEXT,
			impact TEXT NOT NULL,
			valid_from DATE NOT NULL,
			valid_until DATE,
			superseded_by VARCHAR(64),
			PRIMARY KEY (tree, id)
		)`,
		`CREATE TABLE IF NOT EXISTS direction_refs (
			tree VARCHAR(255) NOT NULL,
			direction_id VARCHAR(64) NOT NULL,
			reference TEXT NOT NULL
		)`,
		`CREATE TABLE IF NOT EXISTS direction_impacts (
			tree VARCHAR(255) NOT NULL,
			direction_id VARCHAR(64) NOT NULL,
			affected_tree VARCHAR(255) NOT NULL,
			affected_spec VARCHAR(255) NOT NULL,
			description TEXT NOT NULL
		)`,

		// --- Provenance layer (causal edges) ---
		`CREATE TABLE IF NOT EXISTS task_provenance (
			source_tree VARCHAR(255) NOT NULL,
			source_spec VARCHAR(255) NOT NULL,
			source_task VARCHAR(64) NOT NULL,
			target_tree VARCHAR(255) NOT NULL,
			target_spec VARCHAR(255) NOT NULL,
			target_task VARCHAR(64) NOT NULL,
			note TEXT
		)`,

		// --- Campaign layer ---
		`CREATE TABLE IF NOT EXISTS campaign_active (
			tree VARCHAR(255) NOT NULL,
			spec_id VARCHAR(255) NOT NULL,
			path TEXT,
			summary TEXT,
			phase VARCHAR(255),
			PRIMARY KEY (tree, spec_id)
		)`,
		`CREATE TABLE IF NOT EXISTS campaign_backlog (
			tree VARCHAR(255) NOT NULL,
			spec_id VARCHAR(255) NOT NULL,
			title TEXT,
			summary TEXT,
			path TEXT,
			PRIMARY KEY (tree, spec_id)
		)`,
		`CREATE TABLE IF NOT EXISTS campaign_blocked_by (
			tree VARCHAR(255) NOT NULL,
			spec_id VARCHAR(255) NOT NULL,
			blocked_by VARCHAR(512) NOT NULL
		)`,

		// --- Archive layer ---
		`CREATE TABLE IF NOT EXISTS archives (
			tree VARCHAR(255) NOT NULL,
			spec_id VARCHAR(255) NOT NULL,
			path TEXT,
			summary TEXT,
			archived DATE NOT NULL,
			reason VARCHAR(32) NOT NULL,
			outcome TEXT,
			duration_days INT,
			PRIMARY KEY (tree, spec_id)
		)`,
		`CREATE TABLE IF NOT EXISTS archive_patterns (
			tree VARCHAR(255) NOT NULL,
			spec_id VARCHAR(255) NOT NULL,
			pattern_id VARCHAR(255) NOT NULL
		)`,
		`CREATE TABLE IF NOT EXISTS archive_contributions (
			tree VARCHAR(255) NOT NULL,
			spec_id VARCHAR(255) NOT NULL,
			contribution_path TEXT NOT NULL
		)`,

		// --- Meta ---
		`CREATE TABLE IF NOT EXISTS config (
			key_name VARCHAR(255) PRIMARY KEY,
			value TEXT NOT NULL
		)`,
	}

	for _, stmt := range stmts {
		if _, err := s.DB.Exec(stmt); err != nil {
			return fmt.Errorf("executing schema: %w\nstatement: %s", err, stmt[:80])
		}
	}

	// Set defaults
	s.DB.Exec("INSERT IGNORE INTO config (key_name, value) VALUES ('version', '5')")
	s.DB.Exec("INSERT IGNORE INTO config (key_name, value) VALUES ('auto_commit', 'true')")

	return nil
}
