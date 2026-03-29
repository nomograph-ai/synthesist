# internal/types

Core domain model. These types ARE the schema — JSON tags are the wire format.

- **Depends on**: nothing (leaf package)
- **Depended on by**: `internal/store`, `cmd/synthesist`
- **Convention**: enum types as string constants with exported values (Status, Stance, Confidence, etc.)
- **Types**: Task, Spec, Tree, Thread, Stakeholder, Disposition, Signal, Discovery, Direction, etc.
- **Config**: `DefaultConfig()` provides the initial `.synth/` configuration
