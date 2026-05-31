pub mod integrity;
pub mod migrations;
pub mod overlay;
pub mod schema;
mod store;
pub mod surface;
pub mod telemetry;
// wire_format is `pub mod` only so `tests/overlay_e2e.rs` can reach
// `jsonld_context()` as an integration test. It is not part of the
// supported public API; consumers should treat it as an implementation
// detail. `#[doc(hidden)]` signals this in rustdoc.
#[doc(hidden)]
pub mod wire_format;
