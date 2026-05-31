//! TODO PATH-B: helpers were SQL-backed; the v3 ports inline the
//! load/mutate/append pattern in cmd_task.rs::live_tasks. This module
//! stays in the tree only to keep the `mod` declaration in main.rs
//! compiling; subsequent agent removes the file entirely or rebuilds
//! the helpers on SPARQL.

#![allow(dead_code)]

use anyhow::Result;
use serde_json::Value;

use crate::store::SynthStore;

/// TODO PATH-B: shipped as a stub returning empty so `task_dag` (which
/// also references this) still compiles.
pub fn load_all_current(_store: &SynthStore, _tree: &str, _spec: &str) -> Result<Vec<Value>> {
    Ok(Vec::new())
}
