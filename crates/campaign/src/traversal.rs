//! Per-command traversal breakdown for multi-system (multi-hop) execution.
//!
//! Traversal is a presentation/audit annotation, not part of the execution or
//! scoring data model. It is stored separately on the [`Campaign`](crate::Campaign)
//! in a side map keyed by command id, so adding it never forces changes on
//! `ExecTtp`, `ExecutionRecord`, the scorer, or any other subsystem.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RouteWarningKind {
    BrokenSessionSkipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteWarning {
    pub kind: RouteWarningKind,
    pub message: String,
}

impl RouteWarning {
    pub fn broken_session_skipped() -> Self {
        Self {
            kind: RouteWarningKind::BrokenSessionSkipped,
            message: "A broken session edge to the target was skipped.".to_string(),
        }
    }
}

/// One segment of a multi-hop command traversal.
///
/// As a command is routed across intermediate systems, each hop wraps the inner
/// command in an envelope (e.g. `ran-ws … -- ${CMD}`, `kubectl exec … --
/// ${CMD}`). A `TraversalHop` records a single such segment: the command as it
/// is handed from `from_id` to `to_id`, plus the envelope template applied at
/// this layer. Hops are ordered from the C2 entry point (outermost) to the
/// final target (innermost).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalHop {
    /// Entity executing this segment. The C2 backend id for the first hop.
    pub from_id: String,
    /// Entity reached by this segment.
    pub to_id: String,
    /// Relation/channel name driving this hop (e.g. `kubelet-exec`,
    /// `rce.can-exec`, `kubectl-exec`, or `builtin-exec` for the C2 entry).
    pub relation: String,
    /// The command-wrapping template with `${CMD}` placeholder applied at this
    /// hop, when the hop wraps the inner command. `None` for the C2 entry hop
    /// and plain pass-through segments.
    pub envelope: Option<String>,
    /// The full command string sent across this segment - what `from_id` runs.
    pub command: String,
}

/// The traversal breakdown for a single dispatched command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandTraversal {
    /// Per-hop breakdown, ordered C2 entry (outermost) → target (innermost).
    pub hops: Vec<TraversalHop>,
    /// The bare inner command as it runs on the final target system, before any
    /// hop envelopes wrap it.
    pub inner_command: String,
    #[serde(default)]
    pub warnings: Vec<RouteWarning>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_traversal_reason_deserializes_without_a_warning() {
        let traversal: CommandTraversal = serde_json::from_value(serde_json::json!({
            "hops": [],
            "inner_command": "id",
            "reason": "Direct exec from c2/ran"
        }))
        .expect("legacy traversal should remain readable");

        assert!(traversal.warnings.is_empty());
    }

    #[test]
    fn broken_session_warning_has_a_stable_wire_kind() {
        let warning = RouteWarning::broken_session_skipped();
        let value = serde_json::to_value(warning).expect("warning should serialize");

        assert_eq!(value["kind"], "broken-session-skipped");
    }
}
