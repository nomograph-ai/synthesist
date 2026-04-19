//! Synthesist's Store surface.
//!
//! v2.1 folded the adapter layer into
//! [`nomograph_workflow::Store`](nomograph_workflow::Store) so
//! synthesist and lattice no longer maintain parallel thin adapters.
//! This module is now a re-export shim: every synthesist command
//! continues to `use crate::store::SynthStore` but the real type
//! lives in `workflow`.
//!
//! When the substrate's `Store` API evolves, changes propagate here
//! through one upstream file rather than two parallel copies.

pub use nomograph_workflow::{
    CLAIMS_DIR, Store as SynthStore, find_legacy_v1_db, json_out, legacy_migration_error,
    parse_tree_spec, today,
};

/// Back-compat alias retained from the v2 rewrite. Prefer `SynthStore`
/// at call sites.
pub type Store = SynthStore;
