//! Per-command traversal breakdown for multi-system (multi-hop) execution.
//!
//! Traversal is a presentation/audit annotation, not part of the execution or
//! scoring data model. It is stored separately on the [`Campaign`](crate::Campaign)
//! in a side map keyed by command id, so adding it never forces changes on
//! `ExecTtp`, `ExecutionRecord`, the scorer, or any other subsystem.

use serde::{Deserialize, Serialize};

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
    /// The full command string sent across this segment — what `from_id` runs.
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
}
