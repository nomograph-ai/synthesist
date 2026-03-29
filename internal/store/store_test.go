package store

import (
	"os"
	"path/filepath"
	"testing"
)

func tempDir(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	return dir
}

func TestInit(t *testing.T) {
	dir := tempDir(t)
	s, err := Init(dir)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	defer s.Close() //nolint:errcheck

	// Verify .synth directory created
	if _, err := os.Stat(filepath.Join(dir, ".synth", "synthesist", ".dolt")); err != nil {
		t.Fatalf("Dolt database not created: %v", err)
	}

	// Verify tables exist by querying one
	var count int
	err = s.DB.QueryRow("SELECT COUNT(*) FROM tasks").Scan(&count)
	if err != nil {
		t.Fatalf("Query tasks table failed: %v", err)
	}
	if count != 0 {
		t.Fatalf("Expected 0 tasks, got %d", count)
	}
}

func TestOpen(t *testing.T) {
	dir := tempDir(t)
	s, err := Init(dir)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	_ = s.Close()

	// Reopen
	s2, err := Open(dir)
	if err != nil {
		t.Fatalf("Open failed: %v", err)
	}
	defer s2.Close() //nolint:errcheck

	var count int
	err = s2.DB.QueryRow("SELECT COUNT(*) FROM tasks").Scan(&count)
	if err != nil {
		t.Fatalf("Query after reopen failed: %v", err)
	}
}

func TestOpenNotInitialized(t *testing.T) {
	dir := tempDir(t)
	_, err := Open(dir)
	if err == nil {
		t.Fatal("Expected error opening uninitialized directory")
	}
}

func TestTaskCRUD(t *testing.T) {
	dir := tempDir(t)
	s, err := Init(dir)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	s.AutoCommit = false // don't try git operations in tests
	defer s.Close()      //nolint:errcheck

	// Create a task
	_, err = s.DB.Exec(
		"INSERT INTO tasks (tree, spec, id, type, summary, status, created) VALUES (?, ?, ?, ?, ?, ?, ?)",
		"harness", "test-spec", "t1", "task", "First task", "pending", Today(),
	)
	if err != nil {
		t.Fatalf("Insert task failed: %v", err)
	}

	// Read it back
	var summary, status string
	err = s.DB.QueryRow("SELECT summary, status FROM tasks WHERE tree = ? AND spec = ? AND id = ?",
		"harness", "test-spec", "t1").Scan(&summary, &status)
	if err != nil {
		t.Fatalf("Query task failed: %v", err)
	}
	if summary != "First task" {
		t.Errorf("Expected 'First task', got %q", summary)
	}
	if status != "pending" {
		t.Errorf("Expected 'pending', got %q", status)
	}

	// Update status
	_, err = s.DB.Exec("UPDATE tasks SET status = 'in_progress', owner = 'test' WHERE tree = ? AND spec = ? AND id = ?",
		"harness", "test-spec", "t1")
	if err != nil {
		t.Fatalf("Update task failed: %v", err)
	}

	err = s.DB.QueryRow("SELECT status FROM tasks WHERE tree = ? AND spec = ? AND id = ?",
		"harness", "test-spec", "t1").Scan(&status)
	if err != nil {
		t.Fatalf("Query updated task failed: %v", err)
	}
	if status != "in_progress" {
		t.Errorf("Expected 'in_progress', got %q", status)
	}
}

func TestTaskDependencies(t *testing.T) {
	dir := tempDir(t)
	s, err := Init(dir)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	s.AutoCommit = false
	defer s.Close() //nolint:errcheck

	// Create two tasks
	if _, err := s.DB.Exec("INSERT INTO tasks (tree, spec, id, type, summary, status, created) VALUES (?, ?, ?, ?, ?, ?, ?)",
		"h", "s", "t1", "task", "First", "pending", Today()); err != nil {
		t.Fatalf("Insert t1 failed: %v", err)
	}
	if _, err := s.DB.Exec("INSERT INTO tasks (tree, spec, id, type, summary, status, created) VALUES (?, ?, ?, ?, ?, ?, ?)",
		"h", "s", "t2", "task", "Second", "pending", Today()); err != nil {
		t.Fatalf("Insert t2 failed: %v", err)
	}
	if _, err := s.DB.Exec("INSERT INTO task_deps (tree, spec, task_id, depends_on) VALUES (?, ?, ?, ?)",
		"h", "s", "t2", "t1"); err != nil {
		t.Fatalf("Insert task_dep failed: %v", err)
	}

	// t1 should be ready (no deps), t2 should not (depends on t1)
	var readyCount int
	err = s.DB.QueryRow(`
		SELECT COUNT(*) FROM tasks t
		WHERE t.tree = 'h' AND t.spec = 's' AND t.status = 'pending'
		AND NOT EXISTS (
			SELECT 1 FROM task_deps d
			JOIN tasks dep ON d.tree = dep.tree AND d.spec = dep.spec AND d.depends_on = dep.id
			WHERE d.tree = t.tree AND d.spec = t.spec AND d.task_id = t.id
			AND dep.status != 'done'
		)
	`).Scan(&readyCount)
	if err != nil {
		t.Fatalf("Ready query failed: %v", err)
	}
	if readyCount != 1 {
		t.Errorf("Expected 1 ready task, got %d", readyCount)
	}

	// Complete t1
	if _, err := s.DB.Exec("UPDATE tasks SET status = 'done' WHERE tree = 'h' AND spec = 's' AND id = 't1'"); err != nil {
		t.Fatalf("Complete t1 failed: %v", err)
	}

	// Now both should show t2 as ready
	err = s.DB.QueryRow(`
		SELECT COUNT(*) FROM tasks t
		WHERE t.tree = 'h' AND t.spec = 's' AND t.status = 'pending'
		AND NOT EXISTS (
			SELECT 1 FROM task_deps d
			JOIN tasks dep ON d.tree = dep.tree AND d.spec = dep.spec AND d.depends_on = dep.id
			WHERE d.tree = t.tree AND d.spec = t.spec AND d.task_id = t.id
			AND dep.status != 'done'
		)
	`).Scan(&readyCount)
	if err != nil {
		t.Fatalf("Ready query after t1 done failed: %v", err)
	}
	if readyCount != 1 {
		t.Errorf("Expected 1 ready task (t2), got %d", readyCount)
	}
}

func TestStakeholderAndDisposition(t *testing.T) {
	dir := tempDir(t)
	s, err := Init(dir)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	s.AutoCommit = false
	defer s.Close() //nolint:errcheck

	// Add stakeholder
	_, err = s.DB.Exec("INSERT INTO stakeholders (tree, id, context) VALUES (?, ?, ?)",
		"upstream", "mwilson", "auth-service maintainer")
	if err != nil {
		t.Fatalf("Insert stakeholder failed: %v", err)
	}

	// Add disposition
	_, err = s.DB.Exec(
		"INSERT INTO dispositions (tree, spec, id, stakeholder_id, topic, stance, confidence, valid_from) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
		"upstream", "upstream/auth-api", "d1", "mwilson",
		"API versioning strategy", "cautious", "inferred", Today(),
	)
	if err != nil {
		t.Fatalf("Insert disposition failed: %v", err)
	}

	// Query current stance
	var stance, confidence string
	err = s.DB.QueryRow(
		"SELECT stance, confidence FROM dispositions WHERE stakeholder_id = ? AND valid_until IS NULL ORDER BY valid_from DESC LIMIT 1",
		"mwilson",
	).Scan(&stance, &confidence)
	if err != nil {
		t.Fatalf("Query disposition failed: %v", err)
	}
	if stance != "cautious" {
		t.Errorf("Expected 'cautious', got %q", stance)
	}

	// Supersede with new disposition
	if _, err = s.DB.Exec(
		"UPDATE dispositions SET valid_until = ?, superseded_by = 'd2' WHERE tree = 'upstream' AND spec = 'upstream/auth-api' AND id = 'd1'",
		Today(),
	); err != nil {
		t.Fatalf("Supersede old disposition failed: %v", err)
	}
	if _, err = s.DB.Exec(
		"INSERT INTO dispositions (tree, spec, id, stakeholder_id, topic, stance, confidence, valid_from) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
		"upstream", "upstream/auth-api", "d2", "mwilson",
		"API versioning strategy", "supportive", "documented", Today(),
	); err != nil {
		t.Fatalf("Insert new disposition failed: %v", err)
	}

	// Query should return the new one
	err = s.DB.QueryRow(
		"SELECT stance FROM dispositions WHERE stakeholder_id = ? AND valid_until IS NULL ORDER BY valid_from DESC LIMIT 1",
		"mwilson",
	).Scan(&stance)
	if err != nil {
		t.Fatalf("Query superseded disposition failed: %v", err)
	}
	if stance != "supportive" {
		t.Errorf("Expected 'supportive' after supersession, got %q", stance)
	}
}

func TestSignalBiTemporal(t *testing.T) {
	dir := tempDir(t)
	s, err := Init(dir)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	s.AutoCommit = false
	defer s.Close() //nolint:errcheck

	if _, err := s.DB.Exec("INSERT INTO stakeholders (tree, id, context) VALUES (?, ?, ?)",
		"upstream", "testuser", "test maintainer"); err != nil {
		t.Fatalf("Insert stakeholder failed: %v", err)
	}

	// Record a signal from 2 weeks ago, discovered today
	_, err = s.DB.Exec(
		"INSERT INTO signals (tree, spec, id, stakeholder_id, date, recorded_date, source, source_type, content) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
		"upstream", "test-spec", "s1", "testuser",
		"2026-03-14", Today(), // event time vs record time
		"https://github.com/test/pr/1#comment", "pr_comment",
		"We should not do X because of Y",
	)
	if err != nil {
		t.Fatalf("Insert signal failed: %v", err)
	}

	// Verify both dates stored
	var eventDate, recordDate string
	err = s.DB.QueryRow("SELECT date, recorded_date FROM signals WHERE id = 's1' AND tree = 'upstream' AND spec = 'test-spec'").
		Scan(&eventDate, &recordDate)
	if err != nil {
		t.Fatalf("Query signal failed: %v", err)
	}
	if eventDate == recordDate {
		t.Errorf("Event date and record date should differ: event=%s, record=%s", eventDate, recordDate)
	}
}

func TestDirections(t *testing.T) {
	dir := tempDir(t)
	s, err := Init(dir)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	s.AutoCommit = false
	defer s.Close() //nolint:errcheck

	_, err = s.DB.Exec(
		"INSERT INTO directions (tree, id, project, topic, status, impact, valid_from) VALUES (?, ?, ?, ?, ?, ?, ?)",
		"upstream", "d1", "upstream-org/auth-service",
		"migrate to REST v3", "committed",
		"v2 endpoints will be deprecated -- don't build new integrations against them",
		Today(),
	)
	if err != nil {
		t.Fatalf("Insert direction failed: %v", err)
	}

	// Add impact link
	_, err = s.DB.Exec(
		"INSERT INTO direction_impacts (tree, direction_id, affected_tree, affected_spec, description) VALUES (?, ?, ?, ?, ?)",
		"upstream", "d1", "upstream", "upstream/api-client",
		"client library must support v3 endpoints before migration",
	)
	if err != nil {
		t.Fatalf("Insert direction impact failed: %v", err)
	}

	// Query directions affecting a spec
	var topic, impact string
	err = s.DB.QueryRow(`
		SELECT d.topic, di.description
		FROM directions d
		JOIN direction_impacts di ON d.tree = di.tree AND d.id = di.direction_id
		WHERE di.affected_tree = 'upstream' AND di.affected_spec = 'upstream/api-client'
		AND d.valid_until IS NULL
	`).Scan(&topic, &impact)
	if err != nil {
		t.Fatalf("Query direction impact failed: %v", err)
	}
	if topic != "migrate to REST v3" {
		t.Errorf("Unexpected topic: %q", topic)
	}
}

func TestBranchOperations(t *testing.T) {
	dir := tempDir(t)
	s, err := Init(dir)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	defer s.Close() //nolint:errcheck

	// Note: SetMaxOpenConns(1) is set in Open() for production use.
	// Branch isolation is tested via merge behavior, not mid-connection checkout,
	// because the golden tests run the binary as a subprocess and need the
	// connection pool to be available.

	// List branches — should only have main
	branches, err := s.ListBranches()
	if err != nil {
		t.Fatalf("ListBranches: %v", err)
	}
	if len(branches) != 1 || branches[0] != "main" {
		t.Fatalf("Expected [main], got %v", branches)
	}

	// Create a branch
	if err := s.CreateBranch("test-session"); err != nil {
		t.Fatalf("CreateBranch: %v", err)
	}

	// List should now have 2
	branches, err = s.ListBranches()
	if err != nil {
		t.Fatalf("ListBranches after create: %v", err)
	}
	if len(branches) != 2 {
		t.Fatalf("Expected 2 branches, got %v", branches)
	}

	// Merge (empty branch, fast-forward)
	conflicts, err := s.MergeBranch("test-session")
	if err != nil {
		t.Fatalf("MergeBranch: %v", err)
	}
	if conflicts != 0 {
		t.Fatalf("Expected 0 conflicts, got %d", conflicts)
	}

	// Delete the merged branch
	if err := s.DeleteBranch("test-session"); err != nil {
		t.Fatalf("DeleteBranch: %v", err)
	}

	// Should be back to 1 branch
	branches, err = s.ListBranches()
	if err != nil {
		t.Fatalf("ListBranches after delete: %v", err)
	}
	if len(branches) != 1 {
		t.Fatalf("Expected 1 branch after delete, got %v", branches)
	}
}

func TestEnsureSession(t *testing.T) {
	dir := tempDir(t)
	s, err := Init(dir)
	if err != nil {
		t.Fatalf("Init failed: %v", err)
	}
	defer s.Close() //nolint:errcheck

	// EnsureSession with empty Session should be a no-op
	Session = ""
	if err := s.EnsureSession(); err != nil {
		t.Fatalf("EnsureSession with empty session: %v", err)
	}

	// EnsureSession with non-existent branch should warn and fall back to main (no error)
	Session = "nonexistent"
	if err := s.EnsureSession(); err != nil {
		t.Fatalf("EnsureSession with nonexistent branch should warn and fall back, got error: %v", err)
	}

	// Create branch, then EnsureSession should succeed
	Session = ""
	if err := s.CreateBranch("my-session"); err != nil {
		t.Fatalf("CreateBranch: %v", err)
	}
	Session = "my-session"
	if err := s.EnsureSession(); err != nil {
		t.Fatalf("EnsureSession with existing branch: %v", err)
	}

	// Clean up
	Session = ""
}

func TestNextID(t *testing.T) {
	tests := []struct {
		prefix   string
		existing []string
		want     string
	}{
		{"t", nil, "t1"},
		{"t", []string{"t1"}, "t2"},
		{"t", []string{"t1", "t2", "t5"}, "t6"},
		{"d", []string{"d1", "d3"}, "d4"},
		{"s", []string{}, "s1"},
	}
	for _, tt := range tests {
		got := NextID(tt.prefix, tt.existing)
		if got != tt.want {
			t.Errorf("NextID(%q, %v) = %q, want %q", tt.prefix, tt.existing, got, tt.want)
		}
	}
}
