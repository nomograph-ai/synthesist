//! Discovery claim schema.

use nomograph_claim::validation::{obj, opt_str, req_str};
use nomograph_claim::SchemaResult;
use serde_json::Value;

pub const TYPE_NAME: &str = "discovery";

pub fn validate(props: &Value) -> SchemaResult<()> {
    let map = obj(props, TYPE_NAME)?;
    req_str(map, "tree", TYPE_NAME)?;
    req_str(map, "spec", TYPE_NAME)?;
    req_str(map, "id", TYPE_NAME)?;
    req_str(map, "date", TYPE_NAME)?;
    req_str(map, "finding", TYPE_NAME)?;
    opt_str(map, "author", TYPE_NAME)?;
    opt_str(map, "impact", TYPE_NAME)?;
    opt_str(map, "action", TYPE_NAME)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn minimal_valid_discovery_passes() {
        let v = json!({
            "tree": "k",
            "spec": "s",
            "id": "d1",
            "date": "2026-04-28",
            "finding": "x",
        });
        validate(&v).unwrap();
    }
}
