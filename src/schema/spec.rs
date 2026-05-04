//! Spec claim schema.
//!
//! Defines the shape of `Spec` claims and the enum of allowed
//! statuses. The same `STATUSES` constant drives both the validator
//! below and the clap `PossibleValuesParser` on `spec update --status`
//! in `cli.rs`.

use nomograph_claim::validation::{check_enum, obj, opt_str, opt_str_array, req_str, req_str_array};
use nomograph_claim::{SchemaError, SchemaResult};
use serde_json::Value;

/// Claim type name as it appears on disk and in error messages.
pub const TYPE_NAME: &str = "spec";

/// Allowed values for the Spec `status` field.
///
/// Source of truth. Referenced by:
/// - this module's [`validate`] (via `check_enum`)
/// - the CLI's `spec update --status` flag (via
///   `clap::builder::PossibleValuesParser::new(STATUSES)`)
/// - the skill text and README (rendered from this constant)
///
/// To add a status, add the variant here and update consumers in the
/// same commit. CHANGELOG entry under `### Added`.
///
/// Distinct from the `Outcome` claim's status enum — see
/// [`crate::schema::outcome::STATUSES`]. "Completed" / "abandoned" /
/// "deferred" express *what happened* and live on Outcome, not on
/// Spec status. Spec status expresses *what state the spec is in*.
pub const STATUSES: &[&str] = &["draft", "active", "done", "superseded"];

pub fn validate(props: &Value) -> SchemaResult<()> {
    let map = obj(props, TYPE_NAME)?;
    req_str(map, "tree", TYPE_NAME)?;
    req_str(map, "id", TYPE_NAME)?;
    req_str(map, "goal", TYPE_NAME)?;
    opt_str(map, "constraints", TYPE_NAME)?;
    opt_str(map, "decisions", TYPE_NAME)?;
    let status = req_str(map, "status", TYPE_NAME)?;
    check_enum(status, STATUSES, TYPE_NAME, "status")?;
    let topics = req_str_array(map, "topics", TYPE_NAME)?;
    if topics.is_empty() {
        return Err(SchemaError::EmptyArray {
            claim_type: TYPE_NAME.to_string(),
            field: "topics".to_string(),
        });
    }
    opt_str_array(map, "agree_snapshot", TYPE_NAME)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_valid_spec_passes() {
        let v = json!({
            "tree": "keaton",
            "id": "x",
            "goal": "g",
            "status": "active",
            "topics": ["x"],
        });
        validate(&v).unwrap();
    }

    #[test]
    fn rejects_unknown_status_with_full_detail() {
        let v = json!({
            "tree": "keaton",
            "id": "x",
            "goal": "g",
            "status": "completed",
            "topics": ["x"],
        });
        let err = validate(&v).unwrap_err();
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
                assert_eq!(expected, vec!["draft", "active", "done", "superseded"]);
            }
            _ => panic!("expected InvalidEnum"),
        }
    }

    #[test]
    fn rejects_empty_topics() {
        let v = json!({
            "tree": "keaton",
            "id": "x",
            "goal": "g",
            "status": "active",
            "topics": [],
        });
        let err = validate(&v).unwrap_err();
        assert!(matches!(err, SchemaError::EmptyArray { .. }));
    }

    #[test]
    fn cli_statuses_match_validator() {
        for s in STATUSES {
            let v = json!({
                "tree": "k",
                "id": "x",
                "goal": "g",
                "status": s,
                "topics": ["x"],
            });
            validate(&v).unwrap_or_else(|e| panic!("status {s} should be valid: {e}"));
        }
    }
}
