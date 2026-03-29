# Synthesist

Specification graph manager. Dolt-backed database with Git-like versioning.

## Build

Use `make` for everything. Never call `go build`, `go test`, etc. directly —
the Makefile sets CGO_ENABLED, ICU include/lib paths, and version ldflags
that raw commands miss.

```
make build          # compile binary
make test           # build + run all tests
make lint           # go vet
make check          # build + run synthesist check
make golden-update  # regenerate golden test files
make skill          # build + output skill definition
```

## Conventions

- **Errors**: use constructors from `cmd/synthesist/errors.go`, never inline `fmt.Errorf`
- **Output**: all commands emit JSON via `jsonOut()`
- **SQL**: raw SQL in store methods, no ORM. SQL is the source of truth.
- **File size**: 400 LOC max per file, one concern per file
- **Tests**: golden file tests in `cmd/synthesist/testdata/*.golden`
- **Single verification**: `make build && make test && make lint`
