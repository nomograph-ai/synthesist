package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
)

func parseTreeSpec(s string) (string, string, error) {
	parts := strings.SplitN(s, "/", 2)
	if len(parts) != 2 {
		return "", "", fmt.Errorf("expected tree/spec format, got %q", s)
	}
	if parts[0] == "" || parts[1] == "" {
		return "", "", fmt.Errorf("expected tree/spec format with non-empty components, got %q", s)
	}
	return parts[0], parts[1], nil
}

func jsonOut(v any) error {
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	return enc.Encode(v)
}
