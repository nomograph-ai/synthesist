//! Task claim schema.

use nomograph_claim::validation::{check_enum, obj, opt_str, opt_str_array, req_str};
use nomograph_claim::{SchemaError, SchemaResult};
use serde_json::Value;

pub const TYPE_NAME: &str = "task";

/// Allowed values for the Task `status` field.
///
/// Source of truth referenced by the validator and by
/// `task --status`-style flags in the CLI.
pub const STATUSES: &[&str] = &[
    "pending",
    "in_progress",
    "done",
    "blocked",
    "waiting",
    "cancelled",
];

/// Allowed values for the Task `gate` field. Gates are optional;
/// when present, only `human` is supported (a task that requires
/// human approval before it transitions out of pending).
pub const GATES: &[&str] = &["human"];

pub fn validate(props: &Value) -> SchemaResult<()> {
    let map = obj(props, TYPE_NAME)?;
    req_str(map, "tree", TYPE_NAME)?;
    req_str(map, "spec", TYPE_NAME)?;
    req_str(map, "id", TYPE_NAME)?;
    req_str(map, "summary", TYPE_NAME)?;
    opt_str(map, "description", TYPE_NAME)?;
    opt_str(map, "owner", TYPE_NAME)?;
    let status = req_str(map, "status", TYPE_NAME)?;
    check_enum(status, STATUSES, TYPE_NAME, "status")?;
    if let Some(gate) = opt_str(map, "gate", TYPE_NAME)? {
        check_enum(gate, GATES, TYPE_NAME, "gate")?;
    }
    opt_str_array(map, "depends_on", TYPE_NAME)?;
    opt_str_array(map, "files", TYPE_NAME)?;
    if let Some(v) = map.get("acceptance")
        && !v.is_null()
    {
        let items = v.as_array().ok_or_else(|| SchemaError::WrongType {
            claim_type: TYPE_NAME.to_string(),
            field: "acceptance".to_string(),
            expected: "an array of objects",
        })?;
        for item in items {
            let inner = item.as_object().ok_or_else(|| SchemaError::WrongType {
                claim_type: TYPE_NAME.to_string(),
                field: "acceptance".to_string(),
                expected: "an array of objects",
            })?;
            req_str(inner, "criterion", "task.acceptance")?;
            req_str(inner, "verify_cmd", "task.acceptance")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_valid_task_passes() {
        let v = json!({
            "tree": "k",
            "spec": "s",
            "id": "t1",
            "summary": "do x",
            "status": "pending",
        });
        validate(&v).unwrap();
    }

    #[test]
    fn rejects_unknown_status() {
        let v = json!({
            "tree": "k",
            "spec": "s",
            "id": "t1",
            "summary": "do x",
            "status": "shipped",
        });
        let err = validate(&v).unwrap_err();
        assert!(matches!(err, SchemaError::InvalidEnum { .. }));
    }

    #[test]
    fn rejects_unknown_gate() {
        let v = json!({
            "tree": "k",
            "spec": "s",
            "id": "t1",
            "summary": "do x",
            "status": "pending",
            "gate": "robot",
        });
        let err = validate(&v).unwrap_err();
        assert!(matches!(err, SchemaError::InvalidEnum { .. }));
    }
}
