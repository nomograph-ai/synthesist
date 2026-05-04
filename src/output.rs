//! Synthesist's JSON output contract.
//!
//! Every command's stdout is JSON. Some commands carry warnings —
//! conditions worth surfacing but not severe enough to fail the
//! command. The `Output` wrapper attaches an optional `warnings`
//! array to any payload so consumers (agents, dashboards) can
//! pattern-match without parsing stderr or special prefixes.
//!
//! Convention: domain warnings (e.g. depending on a cancelled task,
//! falling back to an inferred field) go in `warnings`. Errors that
//! should fail the command return an `Err` from the command function
//! and reach the user via the anyhow chain in `main`.
//!
//! # Example
//!
//! ```ignore
//! use crate::output::{Output, json_out};
//! use serde_json::json;
//!
//! let body = json!({"id": "t3", "depends_on": ["t1", "t2"]});
//! let out = Output::new(body).warn("depending on cancelled task t1");
//! json_out(&out)
//! ```
//!
//! Renders as:
//!
//! ```json
//! {"id":"t3","depends_on":["t1","t2"],"warnings":["depending on cancelled task t1"]}
//! ```
//!
//! Backwards compatibility: when `warnings` is empty, the wrapper
//! merges into the body unchanged, so existing consumers that
//! expected a flat object see no difference.

use anyhow::Result;
use serde_json::{Value, json};

/// A command result with optional warnings.
pub struct Output {
    body: Value,
    warnings: Vec<String>,
}

impl Output {
    pub fn new(body: Value) -> Self {
        Self {
            body,
            warnings: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn warn(mut self, message: impl Into<String>) -> Self {
        self.warnings.push(message.into());
        self
    }

    pub fn warns(mut self, messages: impl IntoIterator<Item = String>) -> Self {
        self.warnings.extend(messages);
        self
    }

    /// Render the output as a JSON value, attaching `warnings: [...]`
    /// only when non-empty (so quiet commands stay flat).
    pub fn into_value(self) -> Value {
        if self.warnings.is_empty() {
            return self.body;
        }
        match self.body {
            Value::Object(mut map) => {
                map.insert("warnings".to_string(), json!(self.warnings));
                Value::Object(map)
            }
            other => json!({
                "result": other,
                "warnings": self.warnings,
            }),
        }
    }
}

/// Print an `Output` to stdout per the synthesist JSON contract.
pub fn emit(out: Output) -> Result<()> {
    let v = out.into_value();
    println!("{}", serde_json::to_string(&v)?);
    Ok(())
}
