//! Per-claim-type JSON schema validation.
//!
//! Every [`Claim::props`](crate::Claim::props) value is validated against a
//! schema selected by [`Claim::claim_type`](crate::Claim::claim_type) before
//! the claim reaches the storage layer. Validation at the boundary
//! (BUILDING-lever-principles.md) means `Store::append` can assume a
//! well-formed claim and does not need to re-validate.

use serde_json::Value;

use crate::claim::{Claim, ClaimType};
use crate::error::{Error, Result};

/// Validate `claim.props` against the schema for `claim.claim_type`.
///
/// Returns `Ok(())` on success. On failure, the error message names the
/// offending field and the expected shape so the caller can fix the input
/// without re-reading the schema.
pub fn validate_claim(claim: &Claim) -> Result<()> {
    let props = &claim.props;
    match claim.claim_type {
        ClaimType::Tree => validate_tree(props),
        ClaimType::Spec => validate_spec(props),
        ClaimType::Task => validate_task(props),
        ClaimType::Discovery => validate_discovery(props),
        ClaimType::Campaign => validate_campaign(props),
        ClaimType::Session => validate_session(props),
        ClaimType::Phase => validate_phase(props),
        ClaimType::Intent => validate_intent(props),
        ClaimType::Heartbeat => validate_heartbeat(props),
        ClaimType::Outcome => validate_outcome(props),
        ClaimType::Directive => validate_directive(props),
        ClaimType::Stakeholder => validate_stakeholder(props),
        ClaimType::Topic => validate_topic(props),
        ClaimType::Signal => validate_signal(props),
        ClaimType::Disposition => validate_disposition(props),
    }
}

// --- helpers --------------------------------------------------------------

fn schema_err(msg: impl Into<String>) -> Error {
    Error::Schema(msg.into())
}

fn obj<'a>(props: &'a Value, type_name: &str) -> Result<&'a serde_json::Map<String, Value>> {
    props
        .as_object()
        .ok_or_else(|| schema_err(format!("{type_name} props must be a JSON object")))
}

fn req_str<'a>(
    map: &'a serde_json::Map<String, Value>,
    field: &str,
    type_name: &str,
) -> Result<&'a str> {
    let v = map
        .get(field)
        .ok_or_else(|| schema_err(format!("{type_name} requires '{field}' field")))?;
    let s = v
        .as_str()
        .ok_or_else(|| schema_err(format!("{type_name} field '{field}' must be a string")))?;
    if s.is_empty() {
        return Err(schema_err(format!(
            "{type_name} requires non-empty '{field}' field"
        )));
    }
    Ok(s)
}

fn opt_str<'a>(
    map: &'a serde_json::Map<String, Value>,
    field: &str,
    type_name: &str,
) -> Result<Option<&'a str>> {
    match map.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(schema_err(format!(
            "{type_name} field '{field}' must be a string"
        ))),
    }
}

fn opt_str_array<'a>(
    map: &'a serde_json::Map<String, Value>,
    field: &str,
    type_name: &str,
) -> Result<Option<Vec<&'a str>>> {
    match map.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let s = item.as_str().ok_or_else(|| {
                    schema_err(format!(
                        "{type_name} field '{field}' must contain only strings"
                    ))
                })?;
                out.push(s);
            }
            Ok(Some(out))
        }
        Some(_) => Err(schema_err(format!(
            "{type_name} field '{field}' must be an array of strings"
        ))),
    }
}

fn req_str_array<'a>(
    map: &'a serde_json::Map<String, Value>,
    field: &str,
    type_name: &str,
) -> Result<Vec<&'a str>> {
    let v = map
        .get(field)
        .ok_or_else(|| schema_err(format!("{type_name} requires '{field}' field")))?;
    let items = v.as_array().ok_or_else(|| {
        schema_err(format!(
            "{type_name} field '{field}' must be an array of strings"
        ))
    })?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let s = item.as_str().ok_or_else(|| {
            schema_err(format!(
                "{type_name} field '{field}' must contain only strings"
            ))
        })?;
        out.push(s);
    }
    Ok(out)
}

fn check_enum(value: &str, allowed: &[&str], type_name: &str, field: &str) -> Result<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(schema_err(format!(
            "{type_name} field '{field}' must be one of {allowed:?}"
        )))
    }
}

// --- per-type validators --------------------------------------------------

fn validate_tree(props: &Value) -> Result<()> {
    let map = obj(props, "Tree")?;
    req_str(map, "name", "Tree")?;
    opt_str(map, "description", "Tree")?;
    Ok(())
}

fn validate_spec(props: &Value) -> Result<()> {
    let map = obj(props, "Spec")?;
    req_str(map, "tree", "Spec")?;
    req_str(map, "id", "Spec")?;
    req_str(map, "goal", "Spec")?;
    opt_str(map, "constraints", "Spec")?;
    opt_str(map, "decisions", "Spec")?;
    let status = req_str(map, "status", "Spec")?;
    check_enum(
        status,
        &["draft", "active", "done", "superseded"],
        "Spec",
        "status",
    )?;
    let topics = req_str_array(map, "topics", "Spec")?;
    if topics.is_empty() {
        return Err(schema_err("Spec requires non-empty 'topics' field"));
    }
    opt_str_array(map, "agree_snapshot", "Spec")?;
    Ok(())
}

fn validate_task(props: &Value) -> Result<()> {
    let map = obj(props, "Task")?;
    req_str(map, "tree", "Task")?;
    req_str(map, "spec", "Task")?;
    req_str(map, "id", "Task")?;
    req_str(map, "summary", "Task")?;
    opt_str(map, "description", "Task")?;
    opt_str(map, "owner", "Task")?;
    let status = req_str(map, "status", "Task")?;
    check_enum(
        status,
        &[
            "pending",
            "in_progress",
            "done",
            "blocked",
            "waiting",
            "cancelled",
        ],
        "Task",
        "status",
    )?;
    if let Some(gate) = opt_str(map, "gate", "Task")? {
        check_enum(gate, &["human"], "Task", "gate")?;
    }
    opt_str_array(map, "depends_on", "Task")?;
    opt_str_array(map, "files", "Task")?;
    if let Some(v) = map.get("acceptance") {
        if !v.is_null() {
            let items = v
                .as_array()
                .ok_or_else(|| schema_err("Task field 'acceptance' must be an array"))?;
            for item in items {
                let inner = item.as_object().ok_or_else(|| {
                    schema_err("Task field 'acceptance' entries must be objects")
                })?;
                req_str(inner, "criterion", "Task.acceptance")?;
                req_str(inner, "verify_cmd", "Task.acceptance")?;
            }
        }
    }
    Ok(())
}

fn validate_discovery(props: &Value) -> Result<()> {
    let map = obj(props, "Discovery")?;
    req_str(map, "tree", "Discovery")?;
    req_str(map, "spec", "Discovery")?;
    req_str(map, "id", "Discovery")?;
    req_str(map, "date", "Discovery")?;
    req_str(map, "finding", "Discovery")?;
    opt_str(map, "author", "Discovery")?;
    opt_str(map, "impact", "Discovery")?;
    opt_str(map, "action", "Discovery")?;
    Ok(())
}

fn validate_campaign(props: &Value) -> Result<()> {
    let map = obj(props, "Campaign")?;
    req_str(map, "tree", "Campaign")?;
    req_str(map, "spec", "Campaign")?;
    let kind = req_str(map, "kind", "Campaign")?;
    check_enum(kind, &["active", "backlog"], "Campaign", "kind")?;
    opt_str(map, "summary", "Campaign")?;
    opt_str(map, "title", "Campaign")?;
    opt_str_array(map, "blocked_by", "Campaign")?;
    Ok(())
}

fn validate_session(props: &Value) -> Result<()> {
    let map = obj(props, "Session")?;
    req_str(map, "id", "Session")?;
    opt_str(map, "tree", "Session")?;
    opt_str(map, "spec", "Session")?;
    opt_str(map, "summary", "Session")?;
    Ok(())
}

fn validate_phase(props: &Value) -> Result<()> {
    let map = obj(props, "Phase")?;
    req_str(map, "session_id", "Phase")?;
    let name = req_str(map, "name", "Phase")?;
    check_enum(
        name,
        &[
            "orient", "plan", "agree", "execute", "reflect", "replan", "report",
        ],
        "Phase",
        "name",
    )?;
    Ok(())
}

fn validate_intent(props: &Value) -> Result<()> {
    let map = obj(props, "Intent")?;
    req_str(map, "target", "Intent")?;
    opt_str_array(map, "scope", "Intent")?;
    opt_str(map, "reason", "Intent")?;
    let expires = map
        .get("expires_at")
        .ok_or_else(|| schema_err("Intent requires 'expires_at' field"))?;
    if !matches!(expires, Value::Number(n) if n.is_i64() || n.is_u64()) {
        return Err(schema_err(
            "Intent field 'expires_at' must be an integer (unix ms)",
        ));
    }
    opt_str_array(map, "respects", "Intent")?;
    Ok(())
}

fn validate_heartbeat(props: &Value) -> Result<()> {
    let map = obj(props, "Heartbeat")?;
    req_str(map, "intent_id", "Heartbeat")?;
    let status = req_str(map, "status", "Heartbeat")?;
    check_enum(
        status,
        &["working", "blocked", "ready-for-review"],
        "Heartbeat",
        "status",
    )?;
    opt_str(map, "note", "Heartbeat")?;
    Ok(())
}

fn validate_outcome(props: &Value) -> Result<()> {
    let map = obj(props, "Outcome")?;
    req_str(map, "intent_id", "Outcome")?;
    let outcome = req_str(map, "outcome", "Outcome")?;
    check_enum(
        outcome,
        &["completed", "blocked", "abandoned"],
        "Outcome",
        "outcome",
    )?;
    opt_str_array(map, "evidence", "Outcome")?;
    opt_str_array(map, "needs_review_from", "Outcome")?;
    opt_str(map, "suggests_next_asserter", "Outcome")?;
    Ok(())
}

fn validate_directive(props: &Value) -> Result<()> {
    let map = obj(props, "Directive")?;
    req_str(map, "target", "Directive")?;
    let action = req_str(map, "action", "Directive")?;
    check_enum(
        action,
        &["pause", "resume", "abort"],
        "Directive",
        "action",
    )?;
    opt_str(map, "reason", "Directive")?;
    Ok(())
}

fn validate_stakeholder(props: &Value) -> Result<()> {
    let map = obj(props, "Stakeholder")?;
    req_str(map, "id", "Stakeholder")?;
    opt_str(map, "name", "Stakeholder")?;
    opt_str(map, "context", "Stakeholder")?;
    opt_str_array(map, "orgs", "Stakeholder")?;
    Ok(())
}

fn validate_topic(props: &Value) -> Result<()> {
    let map = obj(props, "Topic")?;
    req_str(map, "name", "Topic")?;
    Ok(())
}

fn validate_signal(props: &Value) -> Result<()> {
    let map = obj(props, "Signal")?;
    req_str(map, "stakeholder_id", "Signal")?;
    req_str(map, "source", "Signal")?;
    let source_type = req_str(map, "source_type", "Signal")?;
    check_enum(
        source_type,
        &[
            "pr_comment",
            "issue_comment",
            "review",
            "commit_message",
            "chat",
            "meeting",
            "email",
            "other",
        ],
        "Signal",
        "source_type",
    )?;
    req_str(map, "content", "Signal")?;
    req_str(map, "event_date", "Signal")?;
    opt_str(map, "record_date", "Signal")?;
    opt_str(map, "interpretation", "Signal")?;
    opt_str(map, "our_action", "Signal")?;
    Ok(())
}

fn validate_disposition(props: &Value) -> Result<()> {
    let map = obj(props, "Disposition")?;
    req_str(map, "stakeholder_id", "Disposition")?;
    req_str(map, "topic", "Disposition")?;
    let stance = req_str(map, "stance", "Disposition")?;
    check_enum(
        stance,
        &["supportive", "cautious", "opposed", "neutral", "unknown"],
        "Disposition",
        "stance",
    )?;
    let confidence = req_str(map, "confidence", "Disposition")?;
    check_enum(
        confidence,
        &["documented", "verified", "inferred", "speculative"],
        "Disposition",
        "confidence",
    )?;
    opt_str(map, "preferred_approach", "Disposition")?;
    opt_str(map, "detail", "Disposition")?;
    Ok(())
}

// --- tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::Claim;
    use serde_json::json;

    fn mk(ct: ClaimType, props: Value) -> Claim {
        Claim::new(ct, props, "user:test")
    }

    fn tree_fixture() -> Value {
        json!({"name": "keaton"})
    }

    fn spec_fixture() -> Value {
        json!({
            "tree": "keaton",
            "id": "sch-1",
            "goal": "validate claims at boundary",
            "status": "active",
            "topics": ["schema"],
        })
    }

    fn task_fixture() -> Value {
        json!({
            "tree": "keaton",
            "spec": "sch-1",
            "id": "t1",
            "summary": "wire dispatch",
            "status": "pending",
        })
    }

    fn discovery_fixture() -> Value {
        json!({
            "tree": "keaton",
            "spec": "sch-1",
            "id": "d1",
            "date": "2026-04-18",
            "finding": "dispatch is cheap",
        })
    }

    fn campaign_fixture() -> Value {
        json!({"tree": "keaton", "spec": "sch-1", "kind": "active"})
    }

    fn session_fixture() -> Value {
        json!({"id": "sess-1"})
    }

    fn phase_fixture() -> Value {
        json!({"session_id": "sess-1", "name": "execute"})
    }

    fn intent_fixture() -> Value {
        json!({"target": "src/schema.rs", "expires_at": 1_700_000_000_000u64})
    }

    fn heartbeat_fixture() -> Value {
        json!({"intent_id": "i1", "status": "working"})
    }

    fn outcome_fixture() -> Value {
        json!({"intent_id": "i1", "outcome": "completed"})
    }

    fn directive_fixture() -> Value {
        json!({"target": "i1", "action": "pause"})
    }

    fn stakeholder_fixture() -> Value {
        json!({"id": "sh-1"})
    }

    fn topic_fixture() -> Value {
        json!({"name": "claim-substrate"})
    }

    fn signal_fixture() -> Value {
        json!({
            "stakeholder_id": "sh-1",
            "source": "https://gitlab.com/example/-/issues/1#note_1",
            "source_type": "issue_comment",
            "content": "looks good",
            "event_date": "2026-04-18",
        })
    }

    fn disposition_fixture() -> Value {
        json!({
            "stakeholder_id": "sh-1",
            "topic": "claim-substrate",
            "stance": "supportive",
            "confidence": "verified",
        })
    }

    // --- happy paths ------------------------------------------------------

    #[test]
    fn tree_ok() {
        validate_claim(&mk(ClaimType::Tree, tree_fixture())).unwrap();
    }
    #[test]
    fn spec_ok() {
        validate_claim(&mk(ClaimType::Spec, spec_fixture())).unwrap();
    }
    #[test]
    fn task_ok() {
        validate_claim(&mk(ClaimType::Task, task_fixture())).unwrap();
    }
    #[test]
    fn discovery_ok() {
        validate_claim(&mk(ClaimType::Discovery, discovery_fixture())).unwrap();
    }
    #[test]
    fn campaign_ok() {
        validate_claim(&mk(ClaimType::Campaign, campaign_fixture())).unwrap();
    }
    #[test]
    fn session_ok() {
        validate_claim(&mk(ClaimType::Session, session_fixture())).unwrap();
    }
    #[test]
    fn phase_ok() {
        validate_claim(&mk(ClaimType::Phase, phase_fixture())).unwrap();
    }
    #[test]
    fn intent_ok() {
        validate_claim(&mk(ClaimType::Intent, intent_fixture())).unwrap();
    }
    #[test]
    fn heartbeat_ok() {
        validate_claim(&mk(ClaimType::Heartbeat, heartbeat_fixture())).unwrap();
    }
    #[test]
    fn outcome_ok() {
        validate_claim(&mk(ClaimType::Outcome, outcome_fixture())).unwrap();
    }
    #[test]
    fn directive_ok() {
        validate_claim(&mk(ClaimType::Directive, directive_fixture())).unwrap();
    }
    #[test]
    fn stakeholder_ok() {
        validate_claim(&mk(ClaimType::Stakeholder, stakeholder_fixture())).unwrap();
    }
    #[test]
    fn topic_ok() {
        validate_claim(&mk(ClaimType::Topic, topic_fixture())).unwrap();
    }
    #[test]
    fn signal_ok() {
        validate_claim(&mk(ClaimType::Signal, signal_fixture())).unwrap();
    }
    #[test]
    fn disposition_ok() {
        validate_claim(&mk(ClaimType::Disposition, disposition_fixture())).unwrap();
    }

    // --- blank-node rejection --------------------------------------------

    fn reject_blank(ct: ClaimType) {
        let c = mk(ct, json!({}));
        assert!(
            validate_claim(&c).is_err(),
            "blank props must fail for {:?}",
            c.claim_type
        );
    }

    #[test]
    fn tree_blank_fails() {
        reject_blank(ClaimType::Tree);
    }
    #[test]
    fn spec_blank_fails() {
        reject_blank(ClaimType::Spec);
    }
    #[test]
    fn task_blank_fails() {
        reject_blank(ClaimType::Task);
    }
    #[test]
    fn discovery_blank_fails() {
        reject_blank(ClaimType::Discovery);
    }
    #[test]
    fn campaign_blank_fails() {
        reject_blank(ClaimType::Campaign);
    }
    #[test]
    fn session_blank_fails() {
        reject_blank(ClaimType::Session);
    }
    #[test]
    fn phase_blank_fails() {
        reject_blank(ClaimType::Phase);
    }
    #[test]
    fn intent_blank_fails() {
        reject_blank(ClaimType::Intent);
    }
    #[test]
    fn heartbeat_blank_fails() {
        reject_blank(ClaimType::Heartbeat);
    }
    #[test]
    fn outcome_blank_fails() {
        reject_blank(ClaimType::Outcome);
    }
    #[test]
    fn directive_blank_fails() {
        reject_blank(ClaimType::Directive);
    }
    #[test]
    fn stakeholder_blank_fails() {
        reject_blank(ClaimType::Stakeholder);
    }
    #[test]
    fn topic_blank_fails() {
        reject_blank(ClaimType::Topic);
    }
    #[test]
    fn signal_blank_fails() {
        reject_blank(ClaimType::Signal);
    }
    #[test]
    fn disposition_blank_fails() {
        reject_blank(ClaimType::Disposition);
    }
}

