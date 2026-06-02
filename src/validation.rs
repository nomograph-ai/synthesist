//! Validation primitives for synthesist's claim schemas.
//!
//! claim is type-agnostic: the substrate stores any well-formed claim
//! regardless of `@type`. Validation is synthesist's responsibility,
//! applied at its CLI / API boundary. This module provides the
//! field-extraction and enum-check building blocks the per-type
//! validators in [`crate::schema`] compose, so each one does not
//! reinvent the logic.
//!
//! Ported into synthesist from the (now retired) `nomograph_claim::
//! validation` so the substrate can stay vocabulary-agnostic.
//!
//! Pattern:
//!
//! ```ignore
//! use crate::validation::{obj, req_str, check_enum, SchemaError};
//! use serde_json::Value;
//!
//! const STATUSES: &[&str] = &["draft", "active", "done", "superseded"];
//!
//! pub fn validate_spec(props: &Value) -> Result<(), SchemaError> {
//!     let map = obj(props, "spec")?;
//!     req_str(map, "tree", "spec")?;
//!     req_str(map, "id", "spec")?;
//!     let status = req_str(map, "status", "spec")?;
//!     check_enum(status, STATUSES, "spec", "status")?;
//!     Ok(())
//! }
//! ```
//!
//! Errors are structured: callers can pattern-match on [`SchemaError`]
//! variants for retry logic, agent-friendly diagnostics, or human
//! formatting. The `Display` impl produces a one-line message that
//! names the claim type, field, actual value, and expected set when
//! applicable.

use std::fmt;

use serde_json::{Map, Value};

/// Structured validation error for synthesist's schemas.
///
/// Each variant carries the claim type name, the offending field, and
/// (where applicable) the actual value or expected set, so callers can
/// diagnose without re-reading the schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// Top-level value is not a JSON object.
    NotAnObject {
        claim_type: String,
    },
    /// A required field is missing.
    MissingField {
        claim_type: String,
        field: String,
    },
    /// A required string field is present but empty.
    EmptyString {
        claim_type: String,
        field: String,
    },
    /// A required array field is empty.
    EmptyArray {
        claim_type: String,
        field: String,
    },
    /// A field is the wrong JSON type (e.g. expected string, got
    /// number).
    WrongType {
        claim_type: String,
        field: String,
        expected: &'static str,
    },
    /// A field's value is not in the allowed enum set.
    InvalidEnum {
        claim_type: String,
        field: String,
        actual: String,
        expected: Vec<String>,
    },
    /// A free-form schema violation that doesn't fit the structured
    /// variants. Avoid when possible; prefer adding a structured
    /// variant.
    Other {
        claim_type: String,
        message: String,
    },
}

impl SchemaError {
    /// Name of the claim type the error pertains to.
    pub fn claim_type(&self) -> &str {
        match self {
            Self::NotAnObject { claim_type }
            | Self::MissingField { claim_type, .. }
            | Self::EmptyString { claim_type, .. }
            | Self::EmptyArray { claim_type, .. }
            | Self::WrongType { claim_type, .. }
            | Self::InvalidEnum { claim_type, .. }
            | Self::Other { claim_type, .. } => claim_type,
        }
    }

    /// Field name when applicable (None for `NotAnObject` and `Other`).
    pub fn field(&self) -> Option<&str> {
        match self {
            Self::MissingField { field, .. }
            | Self::EmptyString { field, .. }
            | Self::EmptyArray { field, .. }
            | Self::WrongType { field, .. }
            | Self::InvalidEnum { field, .. } => Some(field),
            Self::NotAnObject { .. } | Self::Other { .. } => None,
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject { claim_type } => {
                write!(f, "{claim_type} props must be a JSON object")
            }
            Self::MissingField { claim_type, field } => {
                write!(f, "{claim_type} requires '{field}' field")
            }
            Self::EmptyString { claim_type, field } => {
                write!(f, "{claim_type} requires non-empty '{field}' field")
            }
            Self::EmptyArray { claim_type, field } => {
                write!(f, "{claim_type} requires non-empty '{field}' field")
            }
            Self::WrongType {
                claim_type,
                field,
                expected,
            } => {
                write!(f, "{claim_type} field '{field}' must be {expected}")
            }
            Self::InvalidEnum {
                claim_type,
                field,
                actual,
                expected,
            } => {
                write!(
                    f,
                    "{claim_type} field '{field}' is '{actual}' but must be one of: {}",
                    expected.join(", ")
                )
            }
            Self::Other {
                claim_type,
                message,
            } => write!(f, "{claim_type}: {message}"),
        }
    }
}

impl std::error::Error for SchemaError {}

/// Result alias for validation functions.
pub type SchemaResult<T> = Result<T, SchemaError>;

/// Coerce `props` into a JSON object, or fail with `NotAnObject`.
pub fn obj<'a>(props: &'a Value, claim_type: &str) -> SchemaResult<&'a Map<String, Value>> {
    props.as_object().ok_or_else(|| SchemaError::NotAnObject {
        claim_type: claim_type.to_string(),
    })
}

/// Extract a required, non-empty string field.
pub fn req_str<'a>(
    map: &'a Map<String, Value>,
    field: &str,
    claim_type: &str,
) -> SchemaResult<&'a str> {
    let v = map.get(field).ok_or_else(|| SchemaError::MissingField {
        claim_type: claim_type.to_string(),
        field: field.to_string(),
    })?;
    let s = v.as_str().ok_or_else(|| SchemaError::WrongType {
        claim_type: claim_type.to_string(),
        field: field.to_string(),
        expected: "a string",
    })?;
    if s.is_empty() {
        return Err(SchemaError::EmptyString {
            claim_type: claim_type.to_string(),
            field: field.to_string(),
        });
    }
    Ok(s)
}

/// Extract an optional string field. Missing or null -> `None`.
pub fn opt_str<'a>(
    map: &'a Map<String, Value>,
    field: &str,
    claim_type: &str,
) -> SchemaResult<Option<&'a str>> {
    match map.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(SchemaError::WrongType {
            claim_type: claim_type.to_string(),
            field: field.to_string(),
            expected: "a string",
        }),
    }
}

/// Extract a required array of strings. The array itself must be
/// present and may be empty (callers check `is_empty()` if they want
/// to require non-empty).
pub fn req_str_array<'a>(
    map: &'a Map<String, Value>,
    field: &str,
    claim_type: &str,
) -> SchemaResult<Vec<&'a str>> {
    let v = map.get(field).ok_or_else(|| SchemaError::MissingField {
        claim_type: claim_type.to_string(),
        field: field.to_string(),
    })?;
    let items = v.as_array().ok_or_else(|| SchemaError::WrongType {
        claim_type: claim_type.to_string(),
        field: field.to_string(),
        expected: "an array of strings",
    })?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let s = item.as_str().ok_or_else(|| SchemaError::WrongType {
            claim_type: claim_type.to_string(),
            field: field.to_string(),
            expected: "an array of strings",
        })?;
        out.push(s);
    }
    Ok(out)
}

/// Extract an optional array of strings. Missing or null -> `None`.
pub fn opt_str_array<'a>(
    map: &'a Map<String, Value>,
    field: &str,
    claim_type: &str,
) -> SchemaResult<Option<Vec<&'a str>>> {
    match map.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let s = item.as_str().ok_or_else(|| SchemaError::WrongType {
                    claim_type: claim_type.to_string(),
                    field: field.to_string(),
                    expected: "an array of strings",
                })?;
                out.push(s);
            }
            Ok(Some(out))
        }
        Some(_) => Err(SchemaError::WrongType {
            claim_type: claim_type.to_string(),
            field: field.to_string(),
            expected: "an array of strings",
        }),
    }
}

/// Verify that a value is in the allowed enum set. Caller passes the
/// same `&[&str]` constant that drives clap's `PossibleValuesParser`,
/// so CLI-accepts-iff-schema-accepts is structural.
pub fn check_enum(
    value: &str,
    allowed: &[&str],
    claim_type: &str,
    field: &str,
) -> SchemaResult<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(SchemaError::InvalidEnum {
            claim_type: claim_type.to_string(),
            field: field.to_string(),
            actual: value.to_string(),
            expected: allowed.iter().map(|s| s.to_string()).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn req_str_returns_value() {
        let v = json!({"tree": "keaton"});
        let map = v.as_object().unwrap();
        assert_eq!(req_str(map, "tree", "spec").unwrap(), "keaton");
    }

    #[test]
    fn req_str_rejects_missing() {
        let v = json!({});
        let map = v.as_object().unwrap();
        let err = req_str(map, "tree", "spec").unwrap_err();
        assert!(matches!(
            err,
            SchemaError::MissingField { ref claim_type, ref field }
                if claim_type == "spec" && field == "tree"
        ));
    }

    #[test]
    fn req_str_rejects_empty() {
        let v = json!({"tree": ""});
        let map = v.as_object().unwrap();
        let err = req_str(map, "tree", "spec").unwrap_err();
        assert!(matches!(err, SchemaError::EmptyString { .. }));
    }

    #[test]
    fn req_str_rejects_wrong_type() {
        let v = json!({"tree": 42});
        let map = v.as_object().unwrap();
        let err = req_str(map, "tree", "spec").unwrap_err();
        assert!(matches!(err, SchemaError::WrongType { .. }));
    }

    #[test]
    fn check_enum_accepts() {
        assert!(check_enum("active", &["draft", "active", "done"], "spec", "status").is_ok());
    }

    #[test]
    fn check_enum_rejects_with_full_detail() {
        let err =
            check_enum("completed", &["draft", "active", "done"], "spec", "status").unwrap_err();
        match err {
            SchemaError::InvalidEnum {
                claim_type,
                field,
                actual,
                expected,
            } => {
                assert_eq!(claim_type, "spec");
                assert_eq!(field, "status");
                assert_eq!(actual, "completed");
                assert_eq!(expected, vec!["draft", "active", "done"]);
            }
            _ => panic!("expected InvalidEnum"),
        }
    }

    #[test]
    fn invalid_enum_display_includes_actual_and_expected() {
        let err = check_enum("foo", &["a", "b"], "spec", "status").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("foo"));
        assert!(msg.contains("a, b"));
        assert!(msg.contains("status"));
    }
}
