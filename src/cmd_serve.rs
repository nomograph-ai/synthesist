//! TODO PATH-B: `synthesist serve` is the dashboard HTTP server. It
//! was built on `store.query(...)` projections of every claim type
//! plus a filesystem watcher on `claims/changes/`. The dashboard
//! needs to be rewired on SPARQL (similar to cmd_status) and the
//! watcher repointed at the v3 per-asserter log files. Subsequent
//! agent picks this up.

use anyhow::Result;

pub fn run(_port: Option<u16>, _bind_all: bool) -> Result<()> {
    anyhow::bail!("synthesist serve: TODO PATH-B (dashboard not yet ported to v3 SPARQL)")
}
