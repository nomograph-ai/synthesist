//! nomograph-claim -- bi-temporal CRDT claim substrate.
//!
//! v3 stores claims as JSON-LD documents in per-asserter append-only
//! logs (`log::LogWriter` / `log::LogReader`), indexed by the gamma
//! typed-query index (`gamma`). The legacy v2 Automerge store survives
//! only as a read-only shim (`store::Store::open` + `load_claims`) used
//! by the v2-to-v3 migration to drain old `claims/changes/*.amc` trees.

pub mod claim;
pub mod error;
pub mod store;

// v3 substrate modules.
pub mod asserter;
pub mod gamma;
pub mod heads;
pub mod jsonld;
pub mod log;
pub mod ontology;
pub mod prov;

#[allow(deprecated)]
pub use claim::{AsserterId, Claim, ClaimId, ClaimType};
pub use error::{Error, Result};
#[allow(deprecated)]
pub use store::Store;
