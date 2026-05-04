//! Campaign claim schema.

use nomograph_claim::validation::{check_enum, obj, opt_str, opt_str_array, req_str};
use nomograph_claim::SchemaResult;
use serde_json::Value;

pub const TYPE_NAME: &str = "campaign";

/// Allowed values for the Campaign `kind` field.
pub const KINDS: &[&str] = &["active", "backlog"];

pub fn validate(props: &Value) -> SchemaResult<()> {
    let map = obj(props, TYPE_NAME)?;
    req_str(map, "tree", TYPE_NAME)?;
    req_str(map, "spec", TYPE_NAME)?;
    let kind = req_str(map, "kind", TYPE_NAME)?;
    check_enum(kind, KINDS, TYPE_NAME, "kind")?;
    opt_str(map, "summary", TYPE_NAME)?;
    opt_str(map, "title", TYPE_NAME)?;
    opt_str_array(map, "blocked_by", TYPE_NAME)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_valid_campaign_passes() {
        let v = json!({
            "tree": "k",
            "spec": "s",
            "kind": "active",
        });
        validate(&v).unwrap();
    }

    #[test]
    fn rejects_unknown_kind() {
        let v = json!({
            "tree": "k",
            "spec": "s",
            "kind": "draft",
        });
        let err = validate(&v).unwrap_err();
        assert!(matches!(
            err,
            nomograph_claim::SchemaError::InvalidEnum { .. }
        ));
    }
}
