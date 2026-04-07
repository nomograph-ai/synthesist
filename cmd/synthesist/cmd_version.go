package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"
)

// checkLatestVersion queries the GitLab API for the latest release tag.
// Returns the tag name (e.g. "v5.3.4") and release URL, or empty strings on failure.
// Non-blocking: times out after 3 seconds so it never stalls the CLI.
func checkLatestVersion() (tag string, url string) {
	client := &http.Client{Timeout: 3 * time.Second}
	resp, err := client.Get("https://gitlab.com/api/v4/projects/nomograph%2Fsynthesist/releases?per_page=1")
	if err != nil {
		return "", ""
	}
	defer resp.Body.Close() //nolint:errcheck

	if resp.StatusCode != http.StatusOK {
		return "", ""
	}

	var releases []struct {
		TagName string `json:"tag_name"`
		Links   struct {
			Self string `json:"self"`
		} `json:"_links"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&releases); err != nil || len(releases) == 0 {
		return "", ""
	}

	rel := releases[0]
	releaseURL := rel.Links.Self
	if releaseURL == "" {
		releaseURL = fmt.Sprintf("https://gitlab.com/nomograph/synthesist/-/releases/%s", rel.TagName)
	}
	return rel.TagName, releaseURL
}

// newerAvailable compares two semver-like version strings.
// Returns true if latest is newer than current. Handles "dev" as always outdated.
func newerAvailable(current, latest string) bool {
	if current == "dev" || current == "" {
		return latest != ""
	}
	// Strip leading 'v' and any -dirty/-N-gXXX suffix for comparison.
	cur := cleanVersion(current)
	lat := cleanVersion(latest)
	if cur == "" || lat == "" {
		return false
	}
	return lat > cur // works for dotted semver when zero-padded or same width
}

// cleanVersion strips v prefix and build metadata, returning just the semver core.
func cleanVersion(v string) string {
	v = strings.TrimPrefix(v, "v")
	// Strip -dirty, -N-gXXX (git describe suffixes)
	if idx := strings.IndexByte(v, '-'); idx != -1 {
		v = v[:idx]
	}
	return v
}

// versionInfo builds the version check result map.
// Pass offline=true to skip the network check entirely.
// Also respects SYNTHESIST_OFFLINE=1 env var for CI and testing.
func versionInfo(offline bool) map[string]any {
	result := map[string]any{"version": version}

	if offline || os.Getenv("SYNTHESIST_OFFLINE") == "1" {
		return result
	}

	latest, url := checkLatestVersion()
	if latest == "" {
		return result
	}

	result["latest"] = latest
	result["update_available"] = newerAvailable(version, latest)
	result["update_url"] = url
	return result
}
