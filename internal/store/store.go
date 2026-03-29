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
	Root       string // project root
	DBPath     string // path to .synth/ directory
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
		_ = bootDB.Close()
		return nil, fmt.Errorf("creating synthesist database: %w", err)
	}
	_ = bootDB.Close()

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
	if err := s.DoltCommit("synthesist init: create schema"); err != nil {
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

// DoltCommit creates a Dolt commit (internal database versioning).
func (s *Store) DoltCommit(message string) error {
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

// Commit commits to Dolt, and to git only if on main branch.
// Session branches only commit to Dolt — git commit happens on merge.
func (s *Store) Commit(message string) error {
	if err := s.DoltCommit(message); err != nil {
		return err
	}
	// Skip git commit when on a session branch
	if Session != "" {
		return nil
	}
	return s.GitCommit(message)
}

// --- Session / Branch operations ---

// Session holds the active session ID. When set, all operations happen
// on a Dolt branch named after the session. Set via --session flag or
// SYNTHESIST_SESSION env var in main.go.
//
// NOTE: This is global mutable state. Some commands (session start, merge,
// list, prune) temporarily save/restore Session to open the store on main:
//
//   origSession := store.Session
//   store.Session = ""
//   s, err := discoverStore()
//   store.Session = origSession
//
// This pattern is fragile (not goroutine-safe, not panic-safe) but acceptable
// for a single-threaded CLI. Not worth restructuring for v5.
var Session string

// CreateBranch creates a new Dolt branch from current HEAD.
func (s *Store) CreateBranch(name string) error {
	_, err := s.DB.Exec("CALL dolt_branch(?)", name)
	if err != nil {
		return fmt.Errorf("creating branch %s: %w", name, err)
	}
	return nil
}

// SwitchBranch checks out a Dolt branch for the current connection.
func (s *Store) SwitchBranch(name string) error {
	_, err := s.DB.Exec("CALL dolt_checkout(?)", name)
	if err != nil {
		return fmt.Errorf("switching to branch %s: %w", name, err)
	}
	return nil
}

// DeleteBranch deletes a Dolt branch.
func (s *Store) DeleteBranch(name string) error {
	_, err := s.DB.Exec("CALL dolt_branch('-D', ?)", name)
	if err != nil {
		return fmt.Errorf("deleting branch %s: %w", name, err)
	}
	return nil
}

// MergeBranch merges a named branch into main. Returns conflict count.
func (s *Store) MergeBranch(name string) (int, error) {
	if err := s.SwitchBranch("main"); err != nil {
		return 0, err
	}
	var hash string
	var ff, conflicts int
	var message string
	if err := s.DB.QueryRow("CALL dolt_merge(?)", name).Scan(&hash, &ff, &conflicts, &message); err != nil {
		return 0, fmt.Errorf("merging branch %s: %w", name, err)
	}
	return conflicts, nil
}

// ListBranches returns all Dolt branch names.
func (s *Store) ListBranches() ([]string, error) {
	rows, err := s.DB.Query("SELECT name FROM dolt_branches ORDER BY name")
	if err != nil {
		return nil, err
	}
	defer rows.Close() //nolint:errcheck
	var branches []string
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			return nil, fmt.Errorf("scanning branch: %w", err)
		}
		branches = append(branches, name)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterating rows: %w", err)
	}
	return branches, nil
}

// ActiveBranch returns the current Dolt branch name.
func (s *Store) ActiveBranch() (string, error) {
	var branch string
	err := s.DB.QueryRow("SELECT active_branch()").Scan(&branch)
	return branch, err
}

// EnsureSession switches to the session branch if Session is set.
// Called after Open/Discover to set up session isolation.
// Special case: Session="main" operates directly on main (no branch switch).
func (s *Store) EnsureSession() error {
	if Session == "" || Session == "main" {
		return nil
	}
	branches, err := s.ListBranches()
	if err != nil {
		return err
	}
	for _, b := range branches {
		if b == Session {
			return s.SwitchBranch(Session)
		}
	}
	// Session branch doesn't exist — warn and fall back to main so that
	// read-only commands still work (e.g. status, list) even if the session
	// was merged or pruned.
	fmt.Fprintf(os.Stderr, "warning: session %q branch not found — falling back to main\n", Session)
	return nil
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

		// --- Specs layer (intent and institutional memory) ---
		`CREATE TABLE IF NOT EXISTS specs (
			tree VARCHAR(255) NOT NULL,
			id VARCHAR(255) NOT NULL,
			goal TEXT,
			constraints TEXT,
			decisions TEXT,
			created DATE,
			PRIMARY KEY (tree, id)
		)`,

		// --- Propagation chains (cross-spec data dependencies) ---
		`CREATE TABLE IF NOT EXISTS propagation_chain (
			source_tree VARCHAR(255) NOT NULL,
			source_spec VARCHAR(255) NOT NULL,
			target_tree VARCHAR(255) NOT NULL,
			target_spec VARCHAR(255) NOT NULL,
			seq INT NOT NULL,
			description TEXT,
			PRIMARY KEY (source_tree, source_spec, target_tree, target_spec)
		)`,

		// --- Discoveries layer ---
		`CREATE TABLE IF NOT EXISTS discoveries (
			tree VARCHAR(255) NOT NULL,
			spec VARCHAR(255) NOT NULL,
			id VARCHAR(64) NOT NULL,
			date DATE NOT NULL,
			author VARCHAR(255),
			finding TEXT NOT NULL,
			impact TEXT,
			action_taken TEXT,
			PRIMARY KEY (tree, spec, id)
		)`,

		// --- Phase (workflow state machine) ---
		`CREATE TABLE IF NOT EXISTS phase (
			id INT PRIMARY KEY DEFAULT 1,
			name VARCHAR(32) NOT NULL DEFAULT 'orient',
			updated DATETIME DEFAULT CURRENT_TIMESTAMP
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
	if _, err := s.DB.Exec("INSERT IGNORE INTO config (key_name, value) VALUES ('version', '5')"); err != nil {
		return fmt.Errorf("setting default version: %w", err)
	}
	if _, err := s.DB.Exec("INSERT IGNORE INTO config (key_name, value) VALUES ('auto_commit', 'true')"); err != nil {
		return fmt.Errorf("setting default auto_commit: %w", err)
	}

	return nil
}
