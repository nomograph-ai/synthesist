//! Schema creation and migration via rusqlite_migration.
//!
//! Migrations are SQL files in src/migrations/, embedded at compile time.
//! The framework uses SQLite's user_version pragma to track schema version
//! (no metadata table). On every database open, `to_latest()` runs any
//! pending migrations forward.
//!
//! To add a migration:
//! 1. Create src/migrations/NNNN_description.sql
//! 2. Add M::up(include_str!("migrations/NNNN_description.sql")) to MIGRATIONS
//! 3. Optionally add .down("...") for rollback support

use rusqlite_migration::{M, Migrations};

/// All schema migrations, in order. The framework tracks which have been
/// applied via SQLite's user_version pragma.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        // v1.0.0: initial 16-table schema
        M::up(include_str!("migrations/0001_initial.sql")),
        // Future migrations go here:
        // M::up(include_str!("migrations/0002_holdout_scenarios.sql"))
        //  .down("DROP TABLE IF EXISTS holdout_scenarios; ..."),
    ])
}
