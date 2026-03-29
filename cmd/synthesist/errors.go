// Package main defines named error constants for the synthesist CLI.
// LLMs: pick from this catalog instead of inventing error strings.
// Every user-facing error should use one of these constructors.
package main

import (
	"fmt"
	"strings"
)

// Sentinel errors — use errors.Is() to check these.
var (
	ErrNotInitialized = fmt.Errorf("no .synth database found in any parent directory -- run 'synthesist init'")
	ErrAlreadyInit    = fmt.Errorf("database already initialized")
)

// NotFoundError indicates a resource does not exist.
type NotFoundError struct {
	Kind string // "task", "spec", "disposition", "direction", "retro", "pattern", "stakeholder"
	ID   string // e.g. "nomograph-release/gvsets/t3"
}

func (e *NotFoundError) Error() string {
	return fmt.Sprintf("%s not found: %s", e.Kind, e.ID)
}

func ErrNotFound(kind, id string) error {
	return &NotFoundError{Kind: kind, ID: id}
}

// WrongStateError indicates a resource is in an unexpected state.
type WrongStateError struct {
	Kind     string // "task", "dependency"
	ID       string
	Got      string // current state
	Expected string // required state
}

func (e *WrongStateError) Error() string {
	return fmt.Sprintf("%s %s is %s, not %s", e.Kind, e.ID, e.Got, e.Expected)
}

func ErrWrongState(kind, id, got, expected string) error {
	return &WrongStateError{Kind: kind, ID: id, Got: got, Expected: expected}
}

// AlreadyOwnedError indicates a task is already claimed.
type AlreadyOwnedError struct {
	TaskID string
	Owner  string
}

func (e *AlreadyOwnedError) Error() string {
	return fmt.Sprintf("task %s already owned by %s", e.TaskID, e.Owner)
}

func ErrAlreadyOwned(taskID, owner string) error {
	return &AlreadyOwnedError{TaskID: taskID, Owner: owner}
}

// InvalidFormatError indicates a malformed input.
type InvalidFormatError struct {
	Input   string
	Expected string
}

func (e *InvalidFormatError) Error() string {
	return fmt.Sprintf("expected %s format, got %q", e.Expected, e.Input)
}

func ErrInvalidFormat(input, expected string) error {
	return &InvalidFormatError{Input: input, Expected: expected}
}

// MissingFlagsError indicates required flags were not provided.
type MissingFlagsError struct {
	Flags []string
}

func (e *MissingFlagsError) Error() string {
	return fmt.Sprintf("--%s required", strings.Join(e.Flags, ", --"))
}

func ErrMissingFlags(flags ...string) error {
	return &MissingFlagsError{Flags: flags}
}

// UnknownSubcommandError indicates an unrecognized subcommand.
type UnknownSubcommandError struct {
	Command    string
	Subcommand string
}

func (e *UnknownSubcommandError) Error() string {
	return fmt.Sprintf("unknown %s subcommand: %s", e.Command, e.Subcommand)
}

func ErrUnknownSubcommand(command, subcommand string) error {
	return &UnknownSubcommandError{Command: command, Subcommand: subcommand}
}

// Wrap wraps a low-level error with a context string.
// Use for DB/store errors: return Wrap("inserting task", err)
func Wrap(context string, err error) error {
	return fmt.Errorf("%s: %w", context, err)
}
