package golden_test

import (
	"encoding/json"
	"flag"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

var update = flag.Bool("update", false, "update .golden files")

// runSynth executes the synthesist binary in the given working directory
// and returns its stdout. The binary must be built before running tests
// (make build && make test).
func runSynth(t *testing.T, dir string, args ...string) string {
	t.Helper()

	// Find binary relative to repo root
	binary, err := filepath.Abs(filepath.Join("..", "..", "synthesist"))
	if err != nil {
		t.Fatalf("resolving binary path: %v", err)
	}
	if _, err := os.Stat(binary); err != nil {
		t.Fatalf("binary not found at %s — run 'make build' first", binary)
	}

	cmd := exec.Command(binary, args...)
	cmd.Dir = dir
	cmd.Env = append(os.Environ(), "NO_COLOR=1", "SYNTHESIST_SESSION=main")
	out, err := cmd.CombinedOutput()
	if err != nil {
		// Some commands return non-zero on purpose (e.g., check with errors)
		// Only fail if there's no output at all
		if len(out) == 0 {
			t.Fatalf("synthesist %s: %v", strings.Join(args, " "), err)
		}
	}
	return string(out)
}

// initTestDB creates a temp directory with an initialized synthesist database.
// Golden tests set SYNTHESIST_SESSION=test to satisfy session enforcement.
// No real Dolt branch is created — we use a special "test" value that
// EnsureSession treats as a no-op when the branch doesn't exist in a
// fresh database. Session isolation is tested by integration tests.
func initTestDB(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	runSynth(t, dir, "init")
	return dir
}

// runSynthWrite runs synthesist for write operations in golden tests.
func runSynthWrite(t *testing.T, dir string, args ...string) string {
	t.Helper()
	return runSynth(t, dir, args...)
}

// normalizeJSON re-marshals JSON to ensure consistent formatting.
func normalizeJSON(t *testing.T, raw string) string {
	t.Helper()
	var v any
	if err := json.Unmarshal([]byte(raw), &v); err != nil {
		// Not JSON — return as-is (e.g., error messages)
		return raw
	}
	b, _ := json.MarshalIndent(v, "", "  ")
	return string(b) + "\n"
}

// golden compares output against a .golden file. If -update is passed,
// overwrites the golden file with the current output.
func golden(t *testing.T, name string, got string) {
	t.Helper()
	path := filepath.Join("testdata", name+".golden")

	if *update {
		if err := os.MkdirAll("testdata", 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(got), 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}

	want, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("golden file %s not found — run with -update to create", path)
	}
	if got != string(want) {
		t.Errorf("output mismatch for %s\n--- want (golden) ---\n%s\n--- got ---\n%s", name, string(want), got)
	}
}

func TestGolden_TreeCreate(t *testing.T) {
	dir := initTestDB(t)
	out := runSynthWrite(t, dir, "tree", "create", "test-tree", "--description", "A test tree")
	golden(t, "tree_create", normalizeJSON(t, out))
}

func TestGolden_TreeList(t *testing.T) {
	dir := initTestDB(t)
	runSynthWrite(t, dir, "tree", "create", "alpha", "--description", "First tree")
	runSynthWrite(t, dir, "tree", "create", "beta", "--description", "Second tree")
	out := runSynthWrite(t, dir, "tree", "list")
	golden(t, "tree_list", normalizeJSON(t, out))
}

func TestGolden_TaskCreate(t *testing.T) {
	dir := initTestDB(t)
	runSynthWrite(t, dir, "tree", "create", "test", "--description", "Test tree", "--no-commit")
	runSynthWrite(t, dir, "task", "create", "test/myspec", "Build the widget", "--id", "t1", "--no-commit")
	out := runSynthWrite(t, dir, "task", "create", "test/myspec", "Test the widget", "--id", "t2", "--depends-on", "t1", "--gate", "human", "--no-commit")
	golden(t, "task_create", normalizeJSON(t, out))
}

func TestGolden_TaskList(t *testing.T) {
	dir := initTestDB(t)
	runSynthWrite(t, dir, "tree", "create", "test", "--description", "Test", "--no-commit")
	runSynthWrite(t, dir, "task", "create", "test/spec", "First task", "--id", "t1", "--no-commit")
	runSynthWrite(t, dir, "task", "create", "test/spec", "Second task", "--id", "t2", "--depends-on", "t1", "--no-commit")
	out := runSynthWrite(t, dir, "task", "list", "test/spec")
	golden(t, "task_list", normalizeJSON(t, out))
}

func TestGolden_Status(t *testing.T) {
	dir := initTestDB(t)
	runSynthWrite(t, dir, "tree", "create", "test", "--description", "Test tree", "--no-commit")
	runSynthWrite(t, dir, "task", "create", "test/spec", "A task", "--id", "t1", "--no-commit")
	out := runSynthWrite(t, dir, "status")
	golden(t, "status", normalizeJSON(t, out))
}

func TestGolden_SpecCreate(t *testing.T) {
	dir := initTestDB(t)
	runSynthWrite(t, dir, "tree", "create", "test", "--description", "Test", "--no-commit")
	out := runSynthWrite(t, dir, "spec", "create", "test/myspec", "--goal", "Build something great", "--decisions", "Use Go", "--no-commit")
	golden(t, "spec_create", normalizeJSON(t, out))
}

func TestGolden_Check(t *testing.T) {
	dir := initTestDB(t)
	runSynthWrite(t, dir, "tree", "create", "test", "--description", "Test", "--no-commit")
	runSynthWrite(t, dir, "task", "create", "test/spec", "A task", "--id", "t1", "--no-commit")
	out := runSynthWrite(t, dir, "check")
	golden(t, "check", normalizeJSON(t, out))
}

func TestGolden_SessionList(t *testing.T) {
	dir := initTestDB(t)
	out := runSynth(t, dir, "session", "list")
	golden(t, "session_list", normalizeJSON(t, out))
}

// runSynthInSession executes synthesist with SYNTHESIST_SESSION set to the
// given session ID. Use this for write operations that must target a specific
// session branch.
func runSynthInSession(t *testing.T, dir, session string, args ...string) string {
	t.Helper()

	binary, err := filepath.Abs(filepath.Join("..", "..", "synthesist"))
	if err != nil {
		t.Fatalf("resolving binary path: %v", err)
	}
	if _, err := os.Stat(binary); err != nil {
		t.Fatalf("binary not found at %s — run 'make build' first", binary)
	}

	cmd := exec.Command(binary, args...)
	cmd.Dir = dir
	cmd.Env = append(os.Environ(), "NO_COLOR=1", "SYNTHESIST_SESSION="+session)
	out, err := cmd.CombinedOutput()
	if err != nil {
		if len(out) == 0 {
			t.Fatalf("synthesist %s: %v", strings.Join(args, " "), err)
		}
	}
	return string(out)
}

func TestIntegration_TwoSessionsMerge(t *testing.T) {
	// 1. Init a fresh DB
	dir := initTestDB(t)

	// 2. Start session-a
	runSynth(t, dir, "session", "start", "session-a")

	// 3. Create tree on session-a
	runSynthInSession(t, dir, "session-a", "tree", "create", "test", "--description", "Test")

	// 4. Create task in test/spec-a on session-a
	runSynthInSession(t, dir, "session-a", "task", "create", "test/spec-a", "Task A", "--id", "t1")

	// 5. Start session-b
	runSynth(t, dir, "session", "start", "session-b")

	// 6. Create task in test/spec-b on session-b
	runSynthInSession(t, dir, "session-b", "task", "create", "test/spec-b", "Task B", "--id", "t1")

	// 7. Merge session-a into main
	runSynth(t, dir, "session", "merge", "session-a")

	// 8. Merge session-b into main
	runSynth(t, dir, "session", "merge", "session-b")

	// 9. Verify both tasks exist
	outA := runSynth(t, dir, "task", "list", "test/spec-a")
	outB := runSynth(t, dir, "task", "list", "test/spec-b")

	// Parse JSON to verify task counts
	var resultA map[string]any
	if err := json.Unmarshal([]byte(outA), &resultA); err != nil {
		t.Fatalf("parsing spec-a task list: %v\noutput: %s", err, outA)
	}
	tasksA, ok := resultA["tasks"].([]any)
	if !ok || len(tasksA) != 1 {
		t.Errorf("expected 1 task in test/spec-a, got %d\noutput: %s", len(tasksA), outA)
	}

	var resultB map[string]any
	if err := json.Unmarshal([]byte(outB), &resultB); err != nil {
		t.Fatalf("parsing spec-b task list: %v\noutput: %s", err, outB)
	}
	tasksB, ok := resultB["tasks"].([]any)
	if !ok || len(tasksB) != 1 {
		t.Errorf("expected 1 task in test/spec-b, got %d\noutput: %s", len(tasksB), outB)
	}
}

func TestIntegration_SameSpecDifferentTasks(t *testing.T) {
	// Two sessions create different tasks in the SAME spec,
	// then merge. Dolt should merge at the row level.
	dir := initTestDB(t)

	// Both sessions need the tree to exist on main first
	runSynthWrite(t, dir, "tree", "create", "test", "--description", "Test")

	// Start two sessions
	runSynth(t, dir, "session", "start", "writer-1")
	runSynth(t, dir, "session", "start", "writer-2")

	// Writer-1 creates task t1 in test/shared-spec
	runSynthInSession(t, dir, "writer-1", "task", "create", "test/shared-spec", "Task from writer 1", "--id", "t1")

	// Writer-2 creates task t2 in test/shared-spec
	runSynthInSession(t, dir, "writer-2", "task", "create", "test/shared-spec", "Task from writer 2", "--id", "t2")

	// Merge writer-1
	runSynth(t, dir, "session", "merge", "writer-1")

	// Merge writer-2 — this is the key test: row-level merge of different tasks
	out := runSynth(t, dir, "session", "merge", "writer-2")
	var mergeResult map[string]any
	if err := json.Unmarshal([]byte(out), &mergeResult); err != nil {
		t.Fatalf("parsing merge result: %v\noutput: %s", err, out)
	}
	// Should have 0 conflicts since different rows
	conflicts, _ := mergeResult["conflicts"].(float64)
	if conflicts != 0 {
		t.Errorf("expected 0 conflicts, got %v\nmerge output: %s", conflicts, out)
	}

	// Verify both tasks exist on main
	taskOut := runSynth(t, dir, "task", "list", "test/shared-spec")
	var taskResult map[string]any
	if err := json.Unmarshal([]byte(taskOut), &taskResult); err != nil {
		t.Fatalf("parsing task list: %v\noutput: %s", err, taskOut)
	}
	tasks, ok := taskResult["tasks"].([]any)
	if !ok || len(tasks) != 2 {
		t.Errorf("expected 2 tasks in test/shared-spec, got %d\noutput: %s", len(tasks), taskOut)
	}
}

func TestGolden_Scaffold(t *testing.T) {
	// Scaffold on a completely fresh directory (no init, no CLAUDE.md)
	dir := t.TempDir()
	out := runSynth(t, dir, "scaffold")
	golden(t, "scaffold", normalizeJSON(t, out))

	// Verify files were created
	if _, err := os.Stat(filepath.Join(dir, "CLAUDE.md")); err != nil {
		t.Errorf("CLAUDE.md not created")
	}
	if _, err := os.Stat(filepath.Join(dir, ".mise.toml")); err != nil {
		t.Errorf(".mise.toml not created")
	}
	if _, err := os.Stat(filepath.Join(dir, ".synth", "synthesist", ".dolt")); err != nil {
		t.Errorf(".synth not created")
	}

	// Scaffold again — should skip everything
	out2 := runSynth(t, dir, "scaffold")
	golden(t, "scaffold_skip", normalizeJSON(t, out2))
}

func TestGolden_ScaffoldAppend(t *testing.T) {
	dir := t.TempDir()
	// Create existing CLAUDE.md
	if err := os.WriteFile(filepath.Join(dir, "CLAUDE.md"), []byte("# My Project\n\nExisting content.\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	out := runSynth(t, dir, "scaffold")
	var result map[string]any
	if err := json.Unmarshal([]byte(out), &result); err != nil {
		t.Fatalf("parsing scaffold output: %v", err)
	}
	if result["claude_md"] != "appended" {
		t.Errorf("expected claude_md=appended, got %v", result["claude_md"])
	}
	// Verify original content preserved
	content, _ := os.ReadFile(filepath.Join(dir, "CLAUDE.md"))
	if !strings.Contains(string(content), "# My Project") {
		t.Errorf("original CLAUDE.md content lost")
	}
	if !strings.Contains(string(content), "## Synthesist") {
		t.Errorf("synthesist section not appended")
	}
}

func TestGolden_Export(t *testing.T) {
	dir := initTestDB(t)
	runSynthWrite(t, dir, "tree", "create", "test", "--description", "Export test")
	runSynthWrite(t, dir, "task", "create", "test/spec", "A task", "--id", "t1")
	out := runSynth(t, dir, "export")
	var result map[string]any
	if err := json.Unmarshal([]byte(out), &result); err != nil {
		t.Fatalf("parsing export: %v\noutput: %s", err, out)
	}
	if result["version"] != "5" {
		t.Errorf("expected version 5, got %v", result["version"])
	}
	trees, ok := result["trees"].([]any)
	if !ok || len(trees) != 1 {
		t.Errorf("expected 1 tree in export, got %v", result["trees"])
	}
	tasks, ok := result["tasks"].([]any)
	if !ok || len(tasks) != 1 {
		t.Errorf("expected 1 task in export, got %v", result["tasks"])
	}
}

func TestGolden_Migrate(t *testing.T) {
	dir := initTestDB(t)
	out := runSynth(t, dir, "migrate")
	golden(t, "migrate", normalizeJSON(t, out))
}

func TestGolden_PhaseSetShow(t *testing.T) {
	dir := initTestDB(t)
	// Default phase should be orient
	out := runSynth(t, dir, "phase", "show")
	golden(t, "phase_show", normalizeJSON(t, out))

	// Set to plan
	runSynthWrite(t, dir, "phase", "set", "plan")
	out2 := runSynth(t, dir, "phase", "show")
	var result map[string]any
	if err := json.Unmarshal([]byte(out2), &result); err != nil {
		t.Fatalf("parsing phase show: %v", err)
	}
	if result["phase"] != "plan" {
		t.Errorf("expected phase=plan, got %v", result["phase"])
	}
}
