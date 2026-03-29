# internal/store

Dolt embedded database. Single write path for all spec graph data.

- **Depends on**: `internal/types` (domain model), `github.com/dolthub/driver` (Dolt)
- **Depended on by**: `cmd/synthesist` (every command opens a store)
- **Schema**: defined in `createSchema()` — 29 tables, Dolt provides Git-like versioning
- **Convention**: raw SQL via `s.DB.Query/Exec`, no ORM. SQL is the source of truth.
- **Commits**: `Commit()` does both `dolt commit` and `git commit` on the `.synth/` directory
