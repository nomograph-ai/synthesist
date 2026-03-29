// Package docs embeds documentation files for use at compile time.
package docs

import (
	_ "embed"
)

// StateMachine contains the workflow state machine specification.
//
//go:embed state-machine.md
var StateMachine string
