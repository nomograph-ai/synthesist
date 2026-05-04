//! Outcome claim schema.
//!
//! Distinct from Spec status. Spec status answers "what state is this
//! spec in" (`draft`, `active`, `done`, `superseded`). Outcome answers
//! "what happened to it" (`completed`, `abandoned`, `deferred`,
//! `superseded_by`). The two are independently asserted: a spec
//! reaches `done` when its tasks are delivered; an Outcome claim
//! records the disposition with a separate timestamp, asserter, and
//! optional note. Issue #6 closed with the workaround of using
//! `--status superseded --outcome "..."` on the spec; v2.4.0
//! surfaces Outcome as a first-class CLI surface so the workflow is
//! discoverable.

use nomograph_claim::validation::{check_enum, obj, opt_str, req_str};
use nomograph_claim::SchemaResult;
use serde_json::Value;

pub const TYPE_NAME: &str = "outcome";

/// Allowed values for the Outcome `status` field.
///
/// - `completed` — the spec's intent was achieved. Most common case.
/// - `abandoned` — work was stopped without delivery; usually a
///   strategic call (no longer relevant, scope folded elsewhere).
/// - `deferred` — postponed; intent stands but timeline is not now.
/// - `superseded_by` — intent absorbed by a different spec; pair with
///   the new spec id in the optional `linked_spec` field.
pub const STATUSES: &[&str] = &["completed", "abandoned", "deferred", "superseded_by"];

pub fn validate(props: &Value) -> SchemaResult<()> {
    let map = obj(props, TYPE_NAME)?;
    req_str(map, "tree", TYPE_NAME)?;
    req_str(map, "spec", TYPE_NAME)?;
    let status = req_str(map, "status", TYPE_NAME)?;
    check_enum(status, STATUSES, TYPE_NAME, "status")?;
    opt_str(map, "note", TYPE_NAME)?;
    let linked_spec = opt_str(map, "linked_spec", TYPE_NAME)?;
    opt_str(map, "date", TYPE_NAME)?;
    // Domain coupling rule: `superseded_by` is meaningless without a
    // pointer to the absorbing spec. Live at schema level so every
    // consumer (CLI, library, future writers) sees the same rule.
    if status == "superseded_by" && linked_spec.is_none() {
        return Err(nomograph_claim::SchemaError::Other {
            claim_type: TYPE_NAME.to_string(),
            message:
                "status `superseded_by` requires non-empty `linked_spec` naming the absorbing spec"
                    .to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_valid_outcome_passes() {
        let v = json!({
            "tree": "k",
            "spec": "s",
            "status": "completed",
        });
        validate(&v).unwrap();
    }

    #[test]
    fn each_listed_status_accepted() {
        for s in STATUSES {
            let mut v = json!({"tree": "k", "spec": "s", "status": s});
            // `superseded_by` requires linked_spec by domain rule.
            if *s == "superseded_by" {
                v.as_object_mut()
                    .unwrap()
                    .insert("linked_spec".to_string(), json!("other/spec"));
            }
            validate(&v).unwrap_or_else(|e| panic!("outcome status {s}: {e}"));
        }
    }

    #[test]
    fn superseded_by_without_linked_spec_rejected() {
        let v = json!({"tree": "k", "spec": "s", "status": "superseded_by"});
        let err = validate(&v).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("linked_spec"), "{msg}");
    }

    #[test]
    fn superseded_by_with_linked_spec_accepted() {
        let v = json!({
            "tree": "k", "spec": "s", "status": "superseded_by",
            "linked_spec": "other/spec",
        });
        validate(&v).unwrap();
    }

    #[test]
    fn rejects_unknown_status() {
        let v = json!({
            "tree": "k",
            "spec": "s",
            "status": "kicked-out",
        });
        let err = validate(&v).unwrap_err();
        assert!(matches!(
            err,
            nomograph_claim::SchemaError::InvalidEnum { .. }
        ));
    }
}
