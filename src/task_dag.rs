//! Pure-function operations on the task DAG for one (tree, spec).
//!
//! Operates on a snapshot of current task heads as `Vec<serde_json::Value>`
//! to keep the module IO-free and trivially testable. Callers fetch the
//! snapshot via `task_dag::TaskDag::from_snapshot(tasks)` after loading
//! current task claims through their store helper.
//!
//! The same primitives serve every consumer that needs DAG operations:
//! `task ready` (ready set), `task update --depends-on` (cycle and
//! self-dependency checks), and any future cross-task command (rename,
//! reparent, dependents-of). One implementation, no inline DFS scattered
//! across command files.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// A snapshot of the current task heads for one (tree, spec).
pub struct TaskDag<'a> {
    tasks: &'a [Value],
}

/// Result of validating a proposed `depends_on` list against the
/// current DAG. Hard errors are returned via `Err`; soft warnings are
/// surfaced as a list the caller may include in JSON output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DepValidation {
    /// Dep ids that exist but are in `cancelled` status. The proposed
    /// edit is allowed but the new dep will be a dead node in the
    /// DAG. Surface to the user as a warning.
    pub cancelled_deps: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DepError {
    #[error("cannot depend on self: {0}")]
    SelfDependency(String),
    #[error(
        "dependency {dep} not found in {tree}/{spec} -- depends_on must reference existing task IDs in the same spec"
    )]
    UnknownDep {
        dep: String,
        tree: String,
        spec: String,
    },
    #[error("cycle: {path}")]
    Cycle { path: String },
    #[error("task {0} not found in current task heads")]
    TaskNotFound(String),
}

impl<'a> TaskDag<'a> {
    pub fn from_snapshot(tasks: &'a [Value]) -> Self {
        Self { tasks }
    }

    /// Set of all known task ids in this snapshot.
    pub fn ids(&self) -> HashSet<&'a str> {
        self.tasks
            .iter()
            .filter_map(|t| t.get("id").and_then(|v| v.as_str()))
            .collect()
    }

    /// `(id -> status)` map for status-based queries (ready, blocked-by,
    /// cancelled-dep warnings).
    pub fn status_by_id(&self) -> HashMap<&'a str, &'a str> {
        self.tasks
            .iter()
            .filter_map(|t| {
                let id = t.get("id").and_then(|v| v.as_str())?;
                let st = t.get("status").and_then(|v| v.as_str())?;
                Some((id, st))
            })
            .collect()
    }

    /// `(id -> depends_on)` map keyed by task id.
    pub fn deps_by_id(&self) -> HashMap<&'a str, Vec<&'a str>> {
        self.tasks
            .iter()
            .filter_map(|t| {
                let id = t.get("id").and_then(|v| v.as_str())?;
                let deps: Vec<&str> = t
                    .get("depends_on")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                    .unwrap_or_default();
                Some((id, deps))
            })
            .collect()
    }

    /// Tasks in `pending` status whose `depends_on` are all `done`.
    /// Mirrors `task ready` semantics, exposed as a primitive so other
    /// consumers (the dashboard, the `claims status` summary) can use
    /// the same definition.
    #[allow(dead_code)]
    pub fn ready(&self) -> Vec<&'a Value> {
        let status = self.status_by_id();
        self.tasks
            .iter()
            .filter(|t| {
                let s = t.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if s != "pending" {
                    return false;
                }
                t.get("depends_on")
                    .and_then(|v| v.as_array())
                    .map(|deps| {
                        deps.iter()
                            .filter_map(|d| d.as_str())
                            .all(|d| status.get(d).copied() == Some("done"))
                    })
                    .unwrap_or(true)
            })
            .collect()
    }

    /// Validate a proposed `depends_on` list for `task_id`.
    ///
    /// Hard errors (returned `Err`):
    /// - self-dependency
    /// - dependency id not in the same spec
    /// - the proposed list, applied, would close a cycle reaching `task_id`
    ///
    /// Soft warnings (returned in `DepValidation.cancelled_deps`):
    /// - dependency on a task already in `cancelled` status (allowed
    ///   because rewiring away from cancelled predecessors is the
    ///   primary use case for editing deps)
    pub fn validate_proposed_deps(
        &self,
        task_id: &str,
        proposed: &[String],
        tree: &str,
        spec: &str,
    ) -> Result<DepValidation, DepError> {
        if !self.ids().contains(task_id) {
            return Err(DepError::TaskNotFound(task_id.to_string()));
        }

        for dep in proposed {
            if dep == task_id {
                return Err(DepError::SelfDependency(task_id.to_string()));
            }
            if !self.ids().contains(dep.as_str()) {
                return Err(DepError::UnknownDep {
                    dep: dep.clone(),
                    tree: tree.to_string(),
                    spec: spec.to_string(),
                });
            }
        }

        // Cycle check: build effective DAG with proposed deps swapped
        // in for `task_id`, DFS from each proposed dep looking for a
        // path back to `task_id`.
        let mut deps_map: HashMap<&str, Vec<&str>> = self.deps_by_id();
        let proposed_refs: Vec<&str> = proposed.iter().map(|s| s.as_str()).collect();
        deps_map.insert(task_id, proposed_refs);

        for dep in proposed {
            if let Some(path) = find_path(&deps_map, dep.as_str(), task_id) {
                return Err(DepError::Cycle { path });
            }
        }

        let status = self.status_by_id();
        let cancelled_deps: Vec<String> = proposed
            .iter()
            .filter(|d| status.get(d.as_str()).copied() == Some("cancelled"))
            .cloned()
            .collect();

        Ok(DepValidation { cancelled_deps })
    }
}

/// Iterative DFS — find a path from `start` to `target` through the
/// `deps_map`. Returns the path as a human-readable arrow chain
/// (e.g. `t1 -> t2 -> t3`) when found, else `None`.
fn find_path(deps_map: &HashMap<&str, Vec<&str>>, start: &str, target: &str) -> Option<String> {
    let mut stack: Vec<(&str, Vec<String>)> = vec![(start, vec![start.to_string()])];
    let mut seen: HashSet<&str> = HashSet::new();
    while let Some((node, path)) = stack.pop() {
        if node == target {
            return Some(path.join(" -> "));
        }
        if !seen.insert(node) {
            continue;
        }
        if let Some(deps) = deps_map.get(node) {
            for dep in deps {
                let mut next = path.clone();
                next.push(dep.to_string());
                stack.push((dep, next));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task(id: &str, status: &str, deps: &[&str]) -> Value {
        json!({
            "id": id,
            "status": status,
            "depends_on": deps,
        })
    }

    #[test]
    fn ready_returns_pending_with_no_deps() {
        let tasks = vec![task("t1", "pending", &[])];
        let dag = TaskDag::from_snapshot(&tasks);
        let ready = dag.ready();
        assert_eq!(ready.len(), 1);
    }

    #[test]
    fn ready_excludes_pending_with_unfinished_deps() {
        let tasks = vec![
            task("t1", "pending", &[]),
            task("t2", "pending", &["t1"]),
        ];
        let dag = TaskDag::from_snapshot(&tasks);
        let ready: Vec<&str> = dag
            .ready()
            .iter()
            .map(|t| t.get("id").and_then(|v| v.as_str()).unwrap())
            .collect();
        assert_eq!(ready, vec!["t1"]);
    }

    #[test]
    fn ready_includes_pending_with_done_deps() {
        let tasks = vec![
            task("t1", "done", &[]),
            task("t2", "pending", &["t1"]),
        ];
        let dag = TaskDag::from_snapshot(&tasks);
        let ready: Vec<&str> = dag
            .ready()
            .iter()
            .map(|t| t.get("id").and_then(|v| v.as_str()).unwrap())
            .collect();
        assert_eq!(ready, vec!["t2"]);
    }

    #[test]
    fn validate_rejects_self_dep() {
        let tasks = vec![task("t1", "pending", &[])];
        let dag = TaskDag::from_snapshot(&tasks);
        let err = dag
            .validate_proposed_deps("t1", &["t1".to_string()], "k", "s")
            .unwrap_err();
        assert!(matches!(err, DepError::SelfDependency(_)));
    }

    #[test]
    fn validate_rejects_unknown_dep() {
        let tasks = vec![task("t1", "pending", &[])];
        let dag = TaskDag::from_snapshot(&tasks);
        let err = dag
            .validate_proposed_deps("t1", &["t99".to_string()], "k", "s")
            .unwrap_err();
        assert!(matches!(err, DepError::UnknownDep { .. }));
    }

    #[test]
    fn validate_rejects_cycle() {
        let tasks = vec![
            task("t1", "pending", &[]),
            task("t2", "pending", &["t1"]),
            task("t3", "pending", &["t2"]),
        ];
        let dag = TaskDag::from_snapshot(&tasks);
        // Make t1 depend on t3; would close t1 -> t3 -> t2 -> t1.
        let err = dag
            .validate_proposed_deps("t1", &["t3".to_string()], "k", "s")
            .unwrap_err();
        assert!(matches!(err, DepError::Cycle { .. }));
    }

    #[test]
    fn validate_warns_on_cancelled_dep() {
        let tasks = vec![
            task("t1", "cancelled", &[]),
            task("t2", "pending", &[]),
        ];
        let dag = TaskDag::from_snapshot(&tasks);
        let result = dag
            .validate_proposed_deps("t2", &["t1".to_string()], "k", "s")
            .unwrap();
        assert_eq!(result.cancelled_deps, vec!["t1".to_string()]);
    }

    #[test]
    fn validate_accepts_legal_deps() {
        let tasks = vec![
            task("t1", "pending", &[]),
            task("t2", "pending", &[]),
            task("t3", "pending", &[]),
        ];
        let dag = TaskDag::from_snapshot(&tasks);
        let result = dag
            .validate_proposed_deps("t3", &["t1".to_string(), "t2".to_string()], "k", "s")
            .unwrap();
        assert_eq!(result.cancelled_deps, Vec::<String>::new());
    }

    // --- Cycle detection edge cases (issue #5 hardening) -----------------

    #[test]
    fn cycle_detected_through_immediate_back_edge() {
        // t1 -> t2; trying to add t1 as dep of t2 closes 2-cycle.
        let tasks = vec![task("t1", "pending", &["t2"]), task("t2", "pending", &[])];
        let dag = TaskDag::from_snapshot(&tasks);
        let err = dag
            .validate_proposed_deps("t2", &["t1".to_string()], "k", "s")
            .unwrap_err();
        assert!(matches!(err, DepError::Cycle { .. }));
    }

    #[test]
    fn cycle_detected_through_long_chain() {
        // t1->t2->t3->t4->t5; adding t1 as dep of t5 creates 5-cycle.
        let tasks = vec![
            task("t1", "pending", &["t2"]),
            task("t2", "pending", &["t3"]),
            task("t3", "pending", &["t4"]),
            task("t4", "pending", &["t5"]),
            task("t5", "pending", &[]),
        ];
        let dag = TaskDag::from_snapshot(&tasks);
        let err = dag
            .validate_proposed_deps("t5", &["t1".to_string()], "k", "s")
            .unwrap_err();
        match err {
            DepError::Cycle { path } => {
                // Path should walk all five.
                for id in ["t1", "t2", "t3", "t4", "t5"] {
                    assert!(path.contains(id), "cycle path missing {id}: {path}");
                }
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn cycle_detected_through_shared_subtree() {
        // t1 -> {t2, t3} -> t4 (shared subtree).
        // Adding t1 as a dep of t4 closes a cycle through both branches.
        // The DFS must find a path; the exact path depends on traversal
        // order, but it must be a Cycle error.
        let tasks = vec![
            task("t1", "pending", &["t2", "t3"]),
            task("t2", "pending", &["t4"]),
            task("t3", "pending", &["t4"]),
            task("t4", "pending", &[]),
        ];
        let dag = TaskDag::from_snapshot(&tasks);
        let err = dag
            .validate_proposed_deps("t4", &["t1".to_string()], "k", "s")
            .unwrap_err();
        assert!(matches!(err, DepError::Cycle { .. }));
    }

    #[test]
    fn no_cycle_when_diamond_dag_is_legal() {
        // Diamond: t1 depends on t2 and t3; t2 and t3 both depend on t4.
        // No cycle. Adding a fresh dep that doesn't close a cycle is fine.
        let tasks = vec![
            task("t1", "pending", &["t2", "t3"]),
            task("t2", "pending", &["t4"]),
            task("t3", "pending", &["t4"]),
            task("t4", "pending", &[]),
            task("t5", "pending", &[]),
        ];
        let dag = TaskDag::from_snapshot(&tasks);
        // Adding t4 as a dep of t5: no cycle.
        dag.validate_proposed_deps("t5", &["t4".to_string()], "k", "s")
            .unwrap();
        // Replacing t1's deps with [t4]: no cycle.
        dag.validate_proposed_deps("t1", &["t4".to_string()], "k", "s")
            .unwrap();
    }

    #[test]
    fn replacing_deps_doesnt_inherit_old_deps() {
        // t3 originally depends on t2; replacement edit should compute
        // cycles against the NEW dep list, not the old one combined.
        let tasks = vec![
            task("t1", "pending", &[]),
            task("t2", "pending", &["t1"]),
            task("t3", "pending", &["t2"]),
        ];
        let dag = TaskDag::from_snapshot(&tasks);
        // Replace t3's deps with just [t1]: no cycle (t1 has no deps).
        dag.validate_proposed_deps("t3", &["t1".to_string()], "k", "s")
            .unwrap();
        // Replace with [t1, t2]: still no cycle, t2 still depends on t1
        // but t1 doesn't reach back to t3.
        dag.validate_proposed_deps("t3", &["t1".to_string(), "t2".to_string()], "k", "s")
            .unwrap();
    }

    #[test]
    fn empty_dep_list_validates() {
        // Clearing deps must succeed.
        let tasks = vec![task("t1", "pending", &["t2"]), task("t2", "pending", &[])];
        let dag = TaskDag::from_snapshot(&tasks);
        let result = dag
            .validate_proposed_deps("t1", &[], "k", "s")
            .unwrap();
        assert_eq!(result.cancelled_deps, Vec::<String>::new());
    }

    #[test]
    fn done_dep_is_not_a_cancelled_warning() {
        // Done deps are normal (the typical case for a ready task).
        // Only cancelled deps trigger the warning.
        let tasks = vec![task("t1", "done", &[]), task("t2", "pending", &[])];
        let dag = TaskDag::from_snapshot(&tasks);
        let result = dag
            .validate_proposed_deps("t2", &["t1".to_string()], "k", "s")
            .unwrap();
        assert_eq!(result.cancelled_deps, Vec::<String>::new());
    }

    #[test]
    fn validate_against_unknown_target_task_errors() {
        let tasks = vec![task("t1", "pending", &[])];
        let dag = TaskDag::from_snapshot(&tasks);
        let err = dag
            .validate_proposed_deps("nonexistent", &[], "k", "s")
            .unwrap_err();
        assert!(matches!(err, DepError::TaskNotFound(_)));
    }
}
