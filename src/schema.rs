//! Synthesist schema layer.
//!
//! v1 held SQLite DDL here; v2 has none because `view.sqlite` is a
//! cache rebuilt from the claim log. Schema *validation* runs inside
//! [`crate::store::SynthStore::append`] on every write, which forwards
//! to [`nomograph_claim::schema::validate_claim`]. The v1 standalone
//! `validate()` pre-flight function was removed — validation is always
//! in the append path now, so there is no way for a synthesist command
//! to bypass it.
