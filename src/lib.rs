//! nomograph-claim -- bi-temporal CRDT claim substrate.
//!
//! Storage (per project at `<repo_root>/claims/`):
//!
//! ```text
//!   genesis.amc          git-tracked, bootstrap
//!   changes/<hash>.amc   git-tracked, content-addressed, append-only
//!   snapshot.amc         GITIGNORED, local compaction cache
//!   view.sqlite          GITIGNORED, local SQL cache of current state
//!   view.heads           GITIGNORED, stale-check key
//!   config.toml          git-tracked, schema version etc.
//! ```
//!
//! E2EE key lives OUT-OF-TREE at `~/.config/nomograph/keys/<project>.key`.
//!
//! See architecture-v2 + overnight-2026-04-18/09-decision-document.md.

pub mod claim;
pub mod crypto;
pub mod error;
pub mod session;
pub mod store;
pub mod validation;
pub mod view;

// v3-alpha substrate modules. These ship alongside the v0.2 substrate
// during the v3-alpha-integration build; the v2 modules above remain
// until synthesist and any other consumer migrate to v3.
pub mod asserter;
pub mod jsonld;

#[cfg(feature = "beacon")]
pub mod beacon_client;

pub use claim::{AsserterId, Claim, ClaimId, ClaimType};
pub use error::{Error, Result};
pub use session::{Session, SessionHandle};
pub use store::Store;
pub use validation::{SchemaError, SchemaResult};
pub use view::View;
