//! Asserter parsing and validation for the v3 substrate.
//!
//! An asserter identifies the entity that wrote a claim. The string form
//! is `<class>:<scope>:<id>[:<session>]` where:
//!
//! - `class` is `user`, `agent`, or `ingest`.
//! - `scope` is typically `local`, a forge name (`gitlab`, `github`),
//!   or an adapter name.
//! - `id` is the entity's identifier within the scope.
//! - `session` is an optional named session suffix, available for
//!   `user` and `agent` classes only. `ingest` has no session segment.
//!
//! Examples:
//!   `user:local:agd`
//!   `user:local:agd:edc-bootstrap`
//!   `agent:claude-opus-4-7:sess-abc123`
//!   `ingest:gitlab:nomograph-keaton`
//!
//! ## IRI form
//!
//! The IRI form prepends `asserter:` to the string:
//!   `asserter:user:local:agd`
//!   `asserter:ingest:gitlab:nomograph-keaton`
//!
//! ## Directory name convention
//!
//! `dir_name()` replaces colons with hyphens for filesystem safety.
//! Both macOS (HFS+/APFS) and Linux (ext4, etc.) allow colons in file
//! names but they are awkward in shell commands, tab-completion, and
//! path splitting. Hyphens are universally safe. The mapping is
//! deterministic: `user:local:agd:edc-bootstrap` -> `user-local-agd-edc-bootstrap`.

use thiserror::Error;

/// Structured error type for asserter parsing. Every variant names the
/// specific field or condition that failed so callers can surface useful
/// diagnostics.
#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    #[error("empty asserter string")]
    Empty,

    #[error("unknown class '{0}'; expected 'user', 'agent', or 'ingest'")]
    UnknownClass(String),

    #[error("missing 'scope' segment in '{0}'")]
    MissingScope(String),

    #[error("scope is empty in '{0}'")]
    EmptyScope(String),

    #[error("missing 'id' segment in '{0}'")]
    MissingId(String),

    #[error("id is empty in '{0}'")]
    EmptyId(String),

    #[error("'ingest' class does not accept a session suffix, found in '{0}'")]
    IngestSessionForbidden(String),

    #[error("session segment is empty in '{0}'")]
    EmptySession(String),

    #[error("path-unsafe segment '{segment}' in '{full}'")]
    PathUnsafeSegment { segment: String, full: String },
}

/// Reject a single asserter segment (scope, id, or session) that would be
/// unsafe once mapped to a filesystem directory name.
///
/// A parsed asserter is converted to a directory name via
/// [`Asserter::dir_name`] (colons -> hyphens) and joined onto `claims/`.
/// A segment containing a path separator, a `..` traversal token, a NUL,
/// or a control character could redirect a write outside the claims tree,
/// so it is rejected at parse time. This keeps the parsed path (migration,
/// any future caller) as safe as the raw path guarded in
/// [`crate::log::dir_name_for_asserter`].
fn segment_is_path_safe(seg: &str) -> bool {
    if seg == ".." || seg == "." {
        return false;
    }
    !seg
        .chars()
        .any(|c| c == '/' || c == '\\' || c == '\0' || c.is_control())
}

/// The class of an asserter, indicating what kind of entity wrote the claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsserterClass {
    /// A human operator identified by scope and id.
    User,
    /// An automated agent (LLM or other).
    Agent,
    /// A data-ingestion adapter (no session segment allowed).
    Ingest,
}

impl AsserterClass {
    fn as_str(&self) -> &'static str {
        match self {
            AsserterClass::User => "user",
            AsserterClass::Agent => "agent",
            AsserterClass::Ingest => "ingest",
        }
    }
}

/// A parsed and validated asserter.
///
/// Construct via [`parse`]. Convert back to the canonical string form
/// with [`Asserter::to_iri`], or obtain a filesystem-safe directory
/// name with [`Asserter::dir_name`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asserter {
    class: AsserterClass,
    scope: String,
    id: String,
    session: Option<String>,
}

impl std::fmt::Display for Asserter {
    /// The canonical asserter string (without the `asserter:` IRI prefix).
    /// This is the form stored in git logs and emitted by `synthesist status`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.class.as_str(), self.scope, self.id)?;
        if let Some(sess) = &self.session {
            write!(f, ":{sess}")?;
        }
        Ok(())
    }
}

impl Asserter {
    /// The IRI form: `asserter:<class>:<scope>:<id>[:<session>]`.
    ///
    /// This is the value written to `prov:wasAttributedTo` in JSON-LD claims.
    pub fn to_iri(&self) -> String {
        crate::jsonld::asserter_iri(&self.to_string())
    }

    /// A filesystem-safe directory name for this asserter.
    ///
    /// Colons are replaced with hyphens. The mapping is deterministic
    /// and produces identical output on macOS and Linux because it is
    /// a pure string transformation with no locale or case folding.
    ///
    /// Example: `user:local:agd:edc-bootstrap` -> `user-local-agd-edc-bootstrap`
    pub fn dir_name(&self) -> String {
        self.to_string().replace(':', "-")
    }

    /// Access the asserter class.
    pub fn class(&self) -> &AsserterClass {
        &self.class
    }

    /// Access the scope segment.
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// Access the id segment.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Access the session segment, if present.
    pub fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }
}

/// Parse an asserter string into a validated [`Asserter`].
///
/// Accepts `user:<scope>:<id>[:<session>]`, `agent:<scope>:<id>[:<session>]`,
/// and `ingest:<scope>:<id>` (no session for ingest).
///
/// Returns [`ParseError`] with the specific segment at fault.
pub fn parse(s: &str) -> Result<Asserter, ParseError> {
    if s.is_empty() {
        return Err(ParseError::Empty);
    }

    let parts: Vec<&str> = s.splitn(5, ':').collect();

    let class = match parts[0] {
        "user" => AsserterClass::User,
        "agent" => AsserterClass::Agent,
        "ingest" => AsserterClass::Ingest,
        other => return Err(ParseError::UnknownClass(other.to_string())),
    };

    let scope = match parts.get(1) {
        None => return Err(ParseError::MissingScope(s.to_string())),
        Some(&"") => return Err(ParseError::EmptyScope(s.to_string())),
        Some(seg) if !segment_is_path_safe(seg) => {
            return Err(ParseError::PathUnsafeSegment {
                segment: seg.to_string(),
                full: s.to_string(),
            });
        }
        Some(seg) => seg.to_string(),
    };

    let id = match parts.get(2) {
        None => return Err(ParseError::MissingId(s.to_string())),
        Some(&"") => return Err(ParseError::EmptyId(s.to_string())),
        Some(seg) if !segment_is_path_safe(seg) => {
            return Err(ParseError::PathUnsafeSegment {
                segment: seg.to_string(),
                full: s.to_string(),
            });
        }
        Some(seg) => seg.to_string(),
    };

    // Session handling: forbidden for ingest, optional for user/agent.
    let session = match parts.get(3) {
        None => None,
        Some(seg) => {
            if matches!(class, AsserterClass::Ingest) {
                return Err(ParseError::IngestSessionForbidden(s.to_string()));
            }
            if seg.is_empty() {
                return Err(ParseError::EmptySession(s.to_string()));
            }
            if !segment_is_path_safe(seg) {
                return Err(ParseError::PathUnsafeSegment {
                    segment: seg.to_string(),
                    full: s.to_string(),
                });
            }
            Some(seg.to_string())
        }
    };

    Ok(Asserter { class, scope, id, session })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Round-trip tests: parse then to_iri for representative strings --

    #[test]
    fn round_trip_user_local_no_session() {
        let s = "user:local:agd";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.class(), &AsserterClass::User);
        assert_eq!(a.scope(), "local");
        assert_eq!(a.id(), "agd");
        assert_eq!(a.session(), None);
    }

    #[test]
    fn round_trip_user_local_with_session() {
        let s = "user:local:agd:edc-bootstrap";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.session(), Some("edc-bootstrap"));
    }

    #[test]
    fn round_trip_user_gitlab() {
        let s = "user:gitlab:andrewdunn";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.scope(), "gitlab");
    }

    #[test]
    fn round_trip_user_github_with_session() {
        let s = "user:github:agd:overnight-deploy";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.session(), Some("overnight-deploy"));
    }

    #[test]
    fn round_trip_agent_model_no_session() {
        let s = "agent:claude-opus-4-7:sess-abc123";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.class(), &AsserterClass::Agent);
        assert_eq!(a.scope(), "claude-opus-4-7");
        assert_eq!(a.id(), "sess-abc123");
    }

    #[test]
    fn round_trip_agent_with_session() {
        let s = "agent:claude-sonnet-4-6:worker-01:plan-review";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.session(), Some("plan-review"));
    }

    #[test]
    fn round_trip_agent_local_adapter() {
        let s = "agent:local:automation-bot";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.scope(), "local");
    }

    #[test]
    fn round_trip_ingest_gitlab() {
        let s = "ingest:gitlab:nomograph-keaton";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.class(), &AsserterClass::Ingest);
        assert_eq!(a.scope(), "gitlab");
        assert_eq!(a.id(), "nomograph-keaton");
        assert_eq!(a.session(), None);
    }

    #[test]
    fn round_trip_ingest_github() {
        let s = "ingest:github:some-repo";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
    }

    #[test]
    fn round_trip_ingest_adapter() {
        let s = "ingest:jira:project-tracker";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
    }

    #[test]
    fn round_trip_user_local_andrew_dunn() {
        let s = "user:local:andrewdunn";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
    }

    #[test]
    fn round_trip_user_local_with_complex_session() {
        let s = "user:local:agd:secondbreakfast";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.session(), Some("secondbreakfast"));
    }

    #[test]
    fn round_trip_user_local_cms_session() {
        let s = "user:local:agd:cms-t5";
        let a = parse(s).unwrap();
        assert_eq!(a.to_iri(), format!("asserter:{s}"));
        assert_eq!(a.session(), Some("cms-t5"));
    }

    // -- Error tests: invalid forms name the field at fault --

    #[test]
    fn error_empty_string() {
        assert_eq!(parse(""), Err(ParseError::Empty));
    }

    #[test]
    fn error_unknown_class() {
        let err = parse("system:local:bot").unwrap_err();
        assert!(matches!(err, ParseError::UnknownClass(c) if c == "system"));
    }

    #[test]
    fn error_class_only_no_scope() {
        let err = parse("user").unwrap_err();
        assert!(matches!(err, ParseError::MissingScope(_)));
    }

    #[test]
    fn error_empty_scope() {
        let err = parse("user::agd").unwrap_err();
        assert!(matches!(err, ParseError::EmptyScope(_)));
    }

    #[test]
    fn error_missing_id() {
        let err = parse("user:local").unwrap_err();
        assert!(matches!(err, ParseError::MissingId(_)));
    }

    #[test]
    fn error_empty_id() {
        let err = parse("user:local:").unwrap_err();
        assert!(matches!(err, ParseError::EmptyId(_)));
    }

    #[test]
    fn error_ingest_with_session() {
        let err = parse("ingest:gitlab:repo:some-session").unwrap_err();
        assert!(matches!(err, ParseError::IngestSessionForbidden(_)));
    }

    #[test]
    fn error_empty_session_user() {
        let err = parse("user:local:agd:").unwrap_err();
        assert!(matches!(err, ParseError::EmptySession(_)));
    }

    #[test]
    fn error_empty_session_agent() {
        let err = parse("agent:claude-opus-4-7:worker:").unwrap_err();
        assert!(matches!(err, ParseError::EmptySession(_)));
    }

    // -- Security: parse rejects path-unsafe segments --

    #[test]
    fn error_path_unsafe_scope_traversal() {
        let err = parse("user:..:agd").unwrap_err();
        assert!(matches!(err, ParseError::PathUnsafeSegment { .. }));
    }

    #[test]
    fn error_path_unsafe_id_separator() {
        let err = parse("user:local:a/b").unwrap_err();
        assert!(matches!(err, ParseError::PathUnsafeSegment { .. }));
    }

    #[test]
    fn error_path_unsafe_id_backslash() {
        let err = parse("user:local:a\\b").unwrap_err();
        assert!(matches!(err, ParseError::PathUnsafeSegment { .. }));
    }

    #[test]
    fn error_path_unsafe_session_traversal() {
        let err = parse("user:local:agd:..").unwrap_err();
        assert!(matches!(err, ParseError::PathUnsafeSegment { .. }));
    }

    #[test]
    fn error_path_unsafe_id_nul_and_control() {
        assert!(matches!(
            parse("user:local:a\0b").unwrap_err(),
            ParseError::PathUnsafeSegment { .. }
        ));
        assert!(matches!(
            parse("user:local:a\nb").unwrap_err(),
            ParseError::PathUnsafeSegment { .. }
        ));
    }

    // -- dir_name tests: deterministic, colon-free, same on macOS and Linux --

    #[test]
    fn dir_name_user_no_session() {
        let a = parse("user:local:agd").unwrap();
        assert_eq!(a.dir_name(), "user-local-agd");
    }

    #[test]
    fn dir_name_user_with_session() {
        let a = parse("user:local:agd:edc-bootstrap").unwrap();
        assert_eq!(a.dir_name(), "user-local-agd-edc-bootstrap");
    }

    #[test]
    fn dir_name_agent() {
        let a = parse("agent:claude-opus-4-7:sess-abc123").unwrap();
        assert_eq!(a.dir_name(), "agent-claude-opus-4-7-sess-abc123");
    }

    #[test]
    fn dir_name_ingest() {
        let a = parse("ingest:gitlab:nomograph-keaton").unwrap();
        assert_eq!(a.dir_name(), "ingest-gitlab-nomograph-keaton");
    }

    #[test]
    fn dir_name_is_deterministic() {
        // Same input always produces the same output -- no runtime state involved.
        let s = "user:local:agd:overnight-deploy";
        let a = parse(s).unwrap();
        let b = parse(s).unwrap();
        assert_eq!(a.dir_name(), b.dir_name());
    }

    #[test]
    fn dir_name_contains_no_colons() {
        let cases = [
            "user:local:agd",
            "user:local:agd:edc-bootstrap",
            "agent:claude-opus-4-7:worker",
            "ingest:gitlab:my-repo",
        ];
        for s in cases {
            let a = parse(s).unwrap();
            assert!(
                !a.dir_name().contains(':'),
                "dir_name for '{s}' contains a colon: '{}'",
                a.dir_name()
            );
        }
    }
}
