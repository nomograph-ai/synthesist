# cmd/synthesist

CLI entry point. Dispatches commands to handler functions.

- **Depends on**: `internal/store` (database), `internal/types` (domain model)
- **Depended on by**: nothing (this is the top of the call graph)
- **Convention**: one file per command group, one function per subcommand
- **Errors**: use constructors from `errors.go`, never inline `fmt.Errorf` for user-facing errors
- **Output**: all commands emit JSON via `jsonOut()` (in `cmd_task.go`)
