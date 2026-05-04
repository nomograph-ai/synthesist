//! Phase claim schema.

use nomograph_claim::validation::{check_enum, obj, req_str};
use nomograph_claim::SchemaResult;
use serde_json::Value;

pub const TYPE_NAME: &str = "phase";

/// Allowed values for the Phase `name` field. Source of truth for the
/// 7-phase workflow state machine. Referenced by the validator and by
/// `phase set <name>` clap parser.
pub const NAMES: &[&str] = &[
    "orient", "plan", "agree", "execute", "reflect", "replan", "report",
];

pub fn validate(props: &Value) -> SchemaResult<()> {
    let map = obj(props, TYPE_NAME)?;
    req_str(map, "session_id", TYPE_NAME)?;
    let name = req_str(map, "name", TYPE_NAME)?;
    check_enum(name, NAMES, TYPE_NAME, "name")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_listed_name_is_accepted() {
        for n in NAMES {
            let v = json!({"session_id": "s1", "name": n});
            validate(&v).unwrap_or_else(|e| panic!("phase {n}: {e}"));
        }
    }

    #[test]
    fn rejects_unknown_name() {
        let v = json!({"session_id": "s1", "name": "shipping"});
        assert!(validate(&v).is_err());
    }
}
