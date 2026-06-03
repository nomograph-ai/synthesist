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

    #[error("too many segments in '{0}'; expected at most class:scope:id:session (4 segments)")]
    TooManySegments(String),
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
    !seg.chars()
        .any(|c| c == '/' || c == '\\' || c == '\0' || c.is_control())
}

/// Map a single asserter segment to a path-safe form by replacing any
/// unsafe character with `-` and collapsing a bare `..`/`.` traversal
/// token to `-`. Mirror of the rejection rule in [`segment_is_path_safe`]:
/// every input that function would reject becomes safe here, and every
/// input it accepts is returned unchanged.
///
/// Migration/import only -- see [`normalize_legacy`].
fn normalize_segment(seg: &str) -> String {
    if seg == ".." {
        return "-".to_string();
    }
    if seg == "." {
        return "-".to_string();
    }
    seg.chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '\0' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect()
}

/// Normalize a known-legacy v2 asserter string into the strict v3 grammar.
///
/// **Migration/import only.** Maps known legacy v2 asserter shapes into the
/// strict v3 grammar so historical claims are not dropped. The result is
/// still validated by [`parse`]; the live write path never calls this.
///
/// This function does NOT relax [`parse`]. It is a pure, deterministic
/// string transform that produces a string which is THEN handed to the
/// strict parser. Anything it cannot map into the grammar stays
/// unparseable and is honestly skipped by the caller.
///
/// Two legacy shapes are repaired, matching real v2.5.x production exports:
///
/// 1. **2-segment legacy asserter** (`class:name`, no scope/id split) -- an
///    artifact of an earlier v1->v2 migration, e.g. `user:migration-v1-v2`.
///    The missing scope is defaulted to `local`, keeping the trailing name
///    as the id: `user:migration-v1-v2` -> `user:local:migration-v1-v2`.
///    Applied identically to `agent:` and `ingest:`.
///
/// 2. **Path-unsafe characters in any segment** -- e.g. a `/` in a session
///    suffix (`user:local:alex:ops/ps-168-rollout`), which the strict
///    grammar rejects as path-unsafe. Every unsafe character (`/`, `\`,
///    NUL, control) is replaced with `-`, and a segment equal to `..`/`.`
///    becomes `-`, so the result is path-safe:
///    `user:local:alex:ops/ps-168-rollout`
///    -> `user:local:alex:ops-ps-168-rollout`.
///
/// The transform is idempotent on already-valid v3 asserters (a no-op),
/// which lets the import path route both v2 and v3 inputs through one
/// code path.
pub fn normalize_legacy(raw: &str) -> String {
    // An empty string or a string with no recognized class is left
    // untouched -- it is real junk and must stay unparseable.
    //
    // Split on EVERY colon (not `splitn(5)`): a bare `..`/`.` token sitting
    // past the 4th colon must still be normalized, not collapsed into one
    // un-inspected field. A 5+-segment result stays over-length and is
    // rejected by `parse` (TooManySegments) -- an honest skip, never a
    // parse-accepted-but-append-rejected silent drop. This transform is
    // also non-injective (e.g. `a/b` and `a-b` both map to `a-b`): it can
    // merge two source asserters' attribution under one v3 identity, but it
    // never drops a claim.
    let parts: Vec<&str> = raw.split(':').collect();
    let class = match parts.first() {
        Some(&"user") | Some(&"agent") | Some(&"ingest") => parts[0],
        _ => return raw.to_string(),
    };

    // Shape (1): a 2-segment `class:name` legacy asserter. Insert the
    // default scope `local`, keeping `name` as the id. Recurse so the
    // newly-3-segment string also gets per-segment path-safe normalization.
    if parts.len() == 2 {
        return normalize_legacy(&format!("{class}:local:{}", parts[1]));
    }

    // Shape (2): per-segment path-safe normalization of scope/id/session.
    // (parts[0] is the already-validated class; segments 1.. are content.)
    let mut out = String::with_capacity(raw.len());
    out.push_str(class);
    for seg in &parts[1..] {
        out.push(':');
        out.push_str(&normalize_segment(seg));
    }
    out
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
/// and `ingest:<scope>:<id>` (no session for ingest). At most four
/// colon-separated segments; a fifth segment is rejected
/// ([`ParseError::TooManySegments`]) rather than silently truncated, so
/// `parse` and the downstream write guard (`log::dir_name_for_asserter`)
/// agree on what is writable -- a 5+-segment asserter is honestly skipped
/// by the import/migration caller, not parse-accepted then append-rejected.
///
/// Returns [`ParseError`] with the specific segment at fault.
pub fn parse(s: &str) -> Result<Asserter, ParseError> {
    if s.is_empty() {
        return Err(ParseError::Empty);
    }

    // Split on every colon (not `splitn(5)`): a 5th segment must be a hard
    // error, not collapsed into the session field and ignored. Sessions may
    // not contain a colon, so the grammar tops out at four segments.
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() > 4 {
        return Err(ParseError::TooManySegments(s.to_string()));
    }

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

    Ok(Asserter {
        class,
        scope,
        id,
        session,
    })
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

    // -- Over-length: a 5th segment is a hard error, not silent truncation --

    #[test]
    fn error_too_many_segments() {
        // Four segments (with session) is the maximum and must still parse.
        assert!(parse("user:local:agd:sess").is_ok());
        // A fifth segment is rejected, not truncated to the session field.
        assert!(matches!(
            parse("user:local:agd:sess:extra").unwrap_err(),
            ParseError::TooManySegments(_)
        ));
    }

    #[test]
    fn normalize_legacy_then_parse_agree_on_overlong_traversal() {
        // A 6-segment asserter carrying a bare `..` past the 4th colon: the
        // normalizer must sanitize EVERY segment (no bare `..` survives in a
        // collapsed field), and parse must then reject the over-length string
        // -- an honest skip, never parse-accept-then-append-reject.
        let n = normalize_legacy("user:a:b:c:d:..");
        assert!(
            !n.split(':').any(|seg| seg == ".."),
            "no bare `..` segment survives: {n}"
        );
        assert!(matches!(
            parse(&n).unwrap_err(),
            ParseError::TooManySegments(_)
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

    // -- normalize_legacy: migration/import-only legacy-shape repair --

    #[test]
    fn normalize_two_segment_user_inserts_local_scope() {
        // The real v1->v2 migration artifact: `user:migration-v1-v2`.
        let n = normalize_legacy("user:migration-v1-v2");
        assert_eq!(n, "user:local:migration-v1-v2");
        // The normalized string parses, and keeps the name as the id.
        let a = parse(&n).unwrap();
        assert_eq!(a.class(), &AsserterClass::User);
        assert_eq!(a.scope(), "local");
        assert_eq!(a.id(), "migration-v1-v2");
        assert_eq!(a.session(), None);
    }

    #[test]
    fn normalize_two_segment_agent_and_ingest_insert_local() {
        assert_eq!(normalize_legacy("agent:some-bot"), "agent:local:some-bot");
        assert_eq!(
            normalize_legacy("ingest:some-feed"),
            "ingest:local:some-feed"
        );
        assert!(parse(&normalize_legacy("agent:some-bot")).is_ok());
        assert!(parse(&normalize_legacy("ingest:some-feed")).is_ok());
    }

    #[test]
    fn normalize_slash_in_session_is_hyphenated_and_parses() {
        // The real path-unsafe-session shapes.
        let cases = [
            (
                "user:local:alexromano:ops/ps-168-rollout",
                "user:local:alexromano:ops-ps-168-rollout",
                "ops-ps-168-rollout",
            ),
            (
                "user:local:User21:ng-fusion/infra-terraform-migration",
                "user:local:User21:ng-fusion-infra-terraform-migration",
                "ng-fusion-infra-terraform-migration",
            ),
            (
                "user:local:User1.baron:legacy-website/dev-environment-ecs",
                "user:local:User1.baron:legacy-website-dev-environment-ecs",
                "legacy-website-dev-environment-ecs",
            ),
        ];
        for (raw, expected, sess) in cases {
            let n = normalize_legacy(raw);
            assert_eq!(n, expected, "normalize {raw}");
            let a = parse(&n).unwrap_or_else(|e| panic!("normalized {n} must parse: {e}"));
            assert_eq!(a.session(), Some(sess));
        }
    }

    #[test]
    fn normalize_path_unsafe_in_scope_and_id() {
        // Unsafe chars in any segment (not just session) are hyphenated.
        assert_eq!(normalize_legacy("user:a/b:c\\d"), "user:a-b:c-d");
        assert!(parse(&normalize_legacy("user:a/b:c\\d")).is_ok());
        // A bare traversal token collapses to a single hyphen.
        assert_eq!(normalize_legacy("user:..:agd"), "user:-:agd");
        assert!(parse(&normalize_legacy("user:..:agd")).is_ok());
    }

    #[test]
    fn normalize_idempotent_on_valid_v3() {
        // Already-valid v3 asserters are unchanged (no-op), so the import
        // path can route v2 and v3 through one normalizer.
        let valid = [
            "user:local:agd",
            "user:local:agd:edc-bootstrap",
            "agent:claude-opus-4-7:sess-abc123",
            "ingest:gitlab:nomograph-keaton",
            "user:github:agd:overnight-deploy",
        ];
        for s in valid {
            assert_eq!(normalize_legacy(s), s, "must be a no-op for {s}");
            assert!(parse(&normalize_legacy(s)).is_ok());
            // Idempotent: normalizing twice equals normalizing once.
            assert_eq!(normalize_legacy(&normalize_legacy(s)), normalize_legacy(s));
        }
    }

    #[test]
    fn normalize_known_shapes_always_parse() {
        // For every known legacy shape, the normalized result parses.
        let known = [
            "user:migration-v1-v2",
            "agent:legacy-bot",
            "ingest:legacy-feed",
            "user:local:alex:ops/ps-168-rollout",
            "user:local:User1.baron:legacy-website/dev-environment-ecs",
        ];
        for s in known {
            let n = normalize_legacy(s);
            assert!(parse(&n).is_ok(), "normalized {s} -> {n} must parse");
        }
    }

    #[test]
    fn normalize_junk_stays_unparseable() {
        // Real junk: empty, class-only, unknown class. Normalization is a
        // no-op (or local-scope insert that still cannot satisfy the
        // grammar), and parse still rejects it -- honestly skipped.
        assert_eq!(normalize_legacy(""), "");
        assert!(parse(&normalize_legacy("")).is_err());
        // class-only (1 segment): unchanged, still missing scope+id.
        assert_eq!(normalize_legacy("user"), "user");
        assert!(parse(&normalize_legacy("user")).is_err());
        // unknown class: left untouched, still rejected.
        assert_eq!(normalize_legacy("system:local:bot"), "system:local:bot");
        assert!(parse(&normalize_legacy("system:local:bot")).is_err());
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
