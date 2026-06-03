//! nomograph-claim -- per-asserter JSON-LD claim substrate with asserter attribution.
//!
//! v3 stores claims as JSON-LD documents in per-asserter append-only
//! logs (`log::LogWriter` / `log::LogReader`), indexed by the gamma
//! typed-query index (`gamma`). The legacy v2 Automerge store survives
//! as a shim (`store::Store`) used by the v2-to-v3 migration to drain old
//! `claims/changes/*.amc` trees; its read path (`open` + `load_claims`)
//! is primary, with `init`/`append` retained to build migration fixtures.

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
