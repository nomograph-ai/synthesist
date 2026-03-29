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
