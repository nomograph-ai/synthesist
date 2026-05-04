//! Session claim schema.

use nomograph_claim::validation::{obj, opt_str, req_str};
use nomograph_claim::SchemaResult;
use serde_json::Value;

pub const TYPE_NAME: &str = "session";

pub fn validate(props: &Value) -> SchemaResult<()> {
    let map = obj(props, TYPE_NAME)?;
    req_str(map, "id", TYPE_NAME)?;
    opt_str(map, "tree", TYPE_NAME)?;
    opt_str(map, "spec", TYPE_NAME)?;
    opt_str(map, "summary", TYPE_NAME)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_valid_session_passes() {
        validate(&json!({"id": "s1"})).unwrap();
    }
}
