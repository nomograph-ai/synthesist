//! Tree claim schema.

use nomograph_claim::validation::{obj, opt_str, req_str};
use nomograph_claim::SchemaResult;
use serde_json::Value;

pub const TYPE_NAME: &str = "tree";

pub fn validate(props: &Value) -> SchemaResult<()> {
    let map = obj(props, TYPE_NAME)?;
    req_str(map, "name", TYPE_NAME)?;
    opt_str(map, "description", TYPE_NAME)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_valid_tree_passes() {
        validate(&json!({"name": "keaton"})).unwrap();
    }

    #[test]
    fn rejects_missing_name() {
        let err = validate(&json!({})).unwrap_err();
        assert_eq!(err.field(), Some("name"));
    }

    #[test]
    fn rejects_empty_name() {
        let err = validate(&json!({"name": ""})).unwrap_err();
        assert!(matches!(
            err,
            nomograph_claim::SchemaError::EmptyString { .. }
        ));
    }
}
