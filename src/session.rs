//! Session claim semantics (D14).
//!
//! Deprecated: v3 sessions are claims on the log; no separate session substrate.

#![allow(deprecated)]
//!
//! A session is a tagged span of writes. [`Session::start`] writes a
//! `Session` claim whose id becomes the session handle's anchor; every
//! subsequent write performed through [`SessionHandle::tag`] inherits the
//! session's asserter string (`<base>:<id>`) so downstream tools can
//! isolate a unit of work. Unlike the synthesist v1 design, there is no
//! file-copy: isolation is logical, not physical.
//!
//! Close by calling [`SessionHandle::close`], which writes a superseding
//! `Session` claim pointing at the opening claim. Live sessions are the
//! `Session` claims whose id does not appear as a `supersedes` target of
//! any later `Session` claim (see [`Session::list_live`]).

use serde_json::{Map, Value};

use crate::claim::{Claim, ClaimId, ClaimType};
use crate::error::{Error, Result};
use crate::store::Store;

/// In-memory handle to an open session.
///
/// Returned by [`Session::start`]. The handle owns the session's logical
/// id, its asserter base, and the claim id of the opening `Session`
/// claim so [`Self::close`] can write a superseding claim.
#[deprecated(
    since = "3.0.0-pre.1",
    note = "v3 sessions are claims on the log; no separate session substrate."
)]
#[derive(Debug)]
pub struct SessionHandle {
    id: String,
    asserter_base: String,
    start_claim_id: ClaimId,
}

impl SessionHandle {
    /// Logical session id (the user-facing string, not the claim hash).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Fully-qualified asserter string for writes made inside this session.
    ///
    /// Format: `"<asserter_base>:<session_id>"`.
    pub fn asserter(&self) -> String {
        format!("{}:{}", self.asserter_base, self.id)
    }

    /// Overwrite `claim.asserted_by` with this session's asserter string.
    ///
    /// Recomputes `claim.id` because [`Claim::compute_id`] hashes
    /// `asserted_by`. Does NOT append; the caller runs `store.append`.
    pub fn tag(&self, claim: Claim) -> Claim {
        let asserted_by = self.asserter();
        let new_id = Claim::compute_id(
            &claim.claim_type,
            &claim.props,
            claim.valid_from,
            &asserted_by,
            claim.asserted_at,
        );
        Claim {
            id: new_id,
            asserted_by,
            ..claim
        }
    }

    /// Close the session by writing a superseding `Session` claim.
    ///
    /// The close claim carries the same props as the opener (rebuilt from
    /// the store) and sets `supersedes = Some(start_claim_id)`. No
    /// `valid_until` is written; lifecycle bounds are future work.
    pub fn close(self, store: &mut Store) -> Result<()> {
        // Rebuild the opening claim's props from the store so the close
        // claim carries the same tree/spec/summary tags the opener had.
        let opener_props = find_session_props(store, &self.start_claim_id)?;
        let asserted_by = self.asserter();
        let close = Claim::new(ClaimType::Session, opener_props, asserted_by)
            .with_supersedes(self.start_claim_id);
        store.append(&close)?;
        Ok(())
    }
}

/// Namespace for session lifecycle operations against a [`Store`].
#[deprecated(
    since = "3.0.0-pre.1",
    note = "v3 sessions are claims on the log; no separate session substrate."
)]
pub struct Session;

impl Session {
    /// Start a new session.
    ///
    /// Validates `id` is non-empty, builds a `Session` claim whose
    /// `asserted_by` is `<asserter_base>:<id>`, appends it, and returns
    /// an in-memory [`SessionHandle`] pointing at that claim.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when `id` is empty or when the assembled
    /// claim fails schema validation (e.g. empty `tree` / `spec`).
    pub fn start(
        store: &mut Store,
        id: &str,
        asserter_base: &str,
        tree: Option<&str>,
        spec: Option<&str>,
        summary: Option<&str>,
    ) -> Result<SessionHandle> {
        if id.is_empty() {
            return Err(Error::Invalid(
                "Session id must be non-empty; pass e.g. `sess-2026-04-18-abc`".to_string(),
            ));
        }
        if asserter_base.is_empty() {
            return Err(Error::Invalid(
                "Session asserter_base must be non-empty; pass e.g. `user:gitlab:andunn`"
                    .to_string(),
            ));
        }

        let mut props = Map::new();
        props.insert("id".to_string(), Value::String(id.to_string()));
        if let Some(t) = tree {
            props.insert("tree".to_string(), Value::String(t.to_string()));
        }
        if let Some(s) = spec {
            props.insert("spec".to_string(), Value::String(s.to_string()));
        }
        if let Some(s) = summary {
            props.insert("summary".to_string(), Value::String(s.to_string()));
        }

        let asserted_by = format!("{}:{}", asserter_base, id);
        let claim = Claim::new(ClaimType::Session, Value::Object(props), asserted_by);
        let start_claim_id = claim.id.clone();
        store.append(&claim)?;

        // TODO: persist session-id for crash recovery (05 §S10)

        Ok(SessionHandle {
            id: id.to_string(),
            asserter_base: asserter_base.to_string(),
            start_claim_id,
        })
    }

    /// Return all `Session` claims whose id is not superseded by a later
    /// `Session` claim.
    pub fn list_live(store: &mut Store) -> Result<Vec<SessionClaim>> {
        let claims = store.load_claims()?;

        let mut superseded: std::collections::HashSet<ClaimId> = std::collections::HashSet::new();
        for c in &claims {
            if !matches!(c.claim_type, ClaimType::Session) {
                continue;
            }
            if let Some(prior) = &c.supersedes {
                superseded.insert(prior.clone());
            }
        }

        let mut out = Vec::new();
        for c in claims {
            if !matches!(c.claim_type, ClaimType::Session) {
                continue;
            }
            if superseded.contains(&c.id) {
                continue;
            }
            // A Session claim that itself supersedes something is a close
            // claim; skip it. Only openers (no `supersedes`) that remain
            // unsuperseded are live.
            if c.supersedes.is_some() {
                continue;
            }
            let props = c.props.as_object().ok_or_else(|| {
                Error::Corrupt(format!(
                    "Session claim {} props is not an object; re-run Store::init to reset",
                    c.id
                ))
            })?;
            let id = props
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::Corrupt(format!(
                        "Session claim {} missing 'id' string; re-run Store::init to reset",
                        c.id
                    ))
                })?
                .to_string();
            let tree = props.get("tree").and_then(Value::as_str).map(String::from);
            let spec = props.get("spec").and_then(Value::as_str).map(String::from);
            let summary = props
                .get("summary")
                .and_then(Value::as_str)
                .map(String::from);

            // asserter_base is everything before the final ':<id>' suffix.
            let suffix = format!(":{}", id);
            let asserter_base = c
                .asserted_by
                .strip_suffix(&suffix)
                .unwrap_or(c.asserted_by.as_str())
                .to_string();

            out.push(SessionClaim {
                id,
                tree,
                spec,
                summary,
                asserter_base,
                start_id: c.id,
            });
        }

        Ok(out)
    }
}

/// Decoded view of a live `Session` claim returned by [`Session::list_live`].
#[deprecated(
    since = "3.0.0-pre.1",
    note = "v3 sessions are claims on the log; no separate session substrate."
)]
pub struct SessionClaim {
    /// Logical session id (from `props.id`).
    pub id: String,
    /// Optional tree tag.
    pub tree: Option<String>,
    /// Optional spec tag.
    pub spec: Option<String>,
    /// Optional summary.
    pub summary: Option<String>,
    /// Asserter base — the asserter string minus the `:<id>` suffix.
    pub asserter_base: String,
    /// Claim id of the opening `Session` claim.
    pub start_id: ClaimId,
}

/// Look up a `Session` claim by its claim id and return a clone of its props.
fn find_session_props(store: &mut Store, claim_id: &str) -> Result<Value> {
    for c in store.load_claims()? {
        if c.id == claim_id && matches!(c.claim_type, ClaimType::Session) {
            return Ok(c.props);
        }
    }
    Err(Error::Corrupt(format!(
        "Session claim {} not found in store; cannot close a session whose opener is missing",
        claim_id
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_store() -> (TempDir, Store) {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path().join("claims");
        let store = Store::init(&root).expect("init");
        (tmp, store)
    }

    #[test]
    fn start_writes_session_claim() {
        let (_tmp, mut store) = fresh_store();
        let _h = Session::start(
            &mut store,
            "sess-1",
            "user:gitlab:andunn",
            Some("keaton"),
            Some("sch-1"),
            Some("first light"),
        )
        .expect("start");
        let claims = store.load_claims().expect("load");
        let sessions: Vec<_> = claims
            .iter()
            .filter(|c| matches!(c.claim_type, ClaimType::Session))
            .collect();
        assert_eq!(sessions.len(), 1, "expected exactly one Session claim");
        let only = sessions[0];
        assert_eq!(only.asserted_by, "user:gitlab:andunn:sess-1");
        assert_eq!(only.props["id"], "sess-1");
        assert_eq!(only.props["tree"], "keaton");
        assert_eq!(only.props["spec"], "sch-1");
        assert_eq!(only.props["summary"], "first light");
    }

    #[test]
    fn handle_asserter_format() {
        let (_tmp, mut store) = fresh_store();
        let h = Session::start(&mut store, "sess-7", "user:gitlab:andunn", None, None, None)
            .expect("start");
        assert_eq!(h.id(), "sess-7");
        assert_eq!(h.asserter(), "user:gitlab:andunn:sess-7");
    }

    #[test]
    fn close_supersedes_start() {
        let (_tmp, mut store) = fresh_store();
        let h = Session::start(
            &mut store,
            "sess-2",
            "user:gitlab:andunn",
            Some("keaton"),
            None,
            None,
        )
        .expect("start");
        let start_id = {
            let claims = store.load_claims().expect("load");
            claims
                .iter()
                .find(|c| matches!(c.claim_type, ClaimType::Session))
                .expect("opener")
                .id
                .clone()
        };
        h.close(&mut store).expect("close");
        let claims = store.load_claims().expect("load");
        let sessions: Vec<_> = claims
            .iter()
            .filter(|c| matches!(c.claim_type, ClaimType::Session))
            .collect();
        assert_eq!(sessions.len(), 2, "opener + close");
        let closer = sessions
            .iter()
            .find(|c| c.supersedes.is_some())
            .expect("close claim");
        assert_eq!(closer.supersedes.as_deref(), Some(start_id.as_str()));
        assert_eq!(closer.asserted_by, "user:gitlab:andunn:sess-2");
    }

    #[test]
    fn start_with_empty_id_fails() {
        let (_tmp, mut store) = fresh_store();
        let err = Session::start(&mut store, "", "user:gitlab:andunn", None, None, None)
            .expect_err("empty id must fail");
        match err {
            Error::Invalid(msg) => assert!(msg.contains("non-empty"), "msg was: {msg}"),
            other => panic!("wrong variant: {other}"),
        }
    }
}
