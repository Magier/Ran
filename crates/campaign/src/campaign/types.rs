use std::collections::HashMap;

use armory::Ttp;
use serde::{Deserialize, Serialize};

use crate::{FactsUpdate, ParseAudit};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteActionRequest {
    pub action_id: String,
    pub exec_system_id: Option<String>,
    #[serde(default)]
    pub auth_identity_id: Option<String>,
    pub target_id: String,
    pub procedure_id: Option<String>,
    pub args: HashMap<String, String>,
    /// Free-text rationale for choosing this action at this point in the
    /// assessment — why this TTP against this target now. Captured for the
    /// audit trail; carried through to the [`ExecutionRecord`]. Optional, but
    /// strongly encouraged when driving the campaign programmatically (API /
    /// MCP) so the resulting timeline is self-explaining.
    ///
    /// [`ExecutionRecord`]: crate::ExecutionRecord
    #[serde(default)]
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedActionEvent {
    pub id: String,
    pub cmd_id: String,
    pub ttp: Ttp,
    pub args: HashMap<String, String>,
    pub exec_system_id: String,
    pub success: bool,
    pub fail_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteActionResult {
    pub cmd_id: String,
    pub event: ExecutedActionEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecuteActionError {
    InvalidInput(String),
    NotFound(String),
    /// No viable execution channel was found in the knowledge graph for the target.
    NoExecChannel(String),
    /// An internal invariant was violated. Indicates a programming error, not a
    /// user-facing condition — surfaces as a 500 rather than panicking.
    InvariantViolation(String),
}

/// A resolved execution channel for a TTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecChannel {
    /// C2 backend ID forwarded to the C2Manager (e.g. [`c2::BUILTIN_C2_ID`]).
    pub backend_id: String,
    /// Ordered list of intermediate pod entity IDs to kubectl-exec through,
    /// from the C2 side outward.
    ///
    /// - `[]` — direct path; the C2 can reach the exec target without any hop.
    /// - `[p1]` — one hop: C2 execs into p1, p1 runs the command on the target.
    /// - `[p1, p2, p3]` — three hops: C2 → p1 → p2 → p3 → target, each step
    ///   via a nested `kubectl exec`.
    ///
    /// The first hop is the pod entity ID that `BuiltinC2` will directly exec
    /// into; all subsequent hops plus the final `exec_target_id` are nested as
    /// kubectl exec wrappers inside the procedure command.
    pub hops: Vec<String>,
    /// Overrides the exec target entity ID when the requested target (e.g. a
    /// service account) was resolved to a concrete pod that should receive the
    /// command. `None` means use the original `request.target_id`.
    pub exec_target_id: Option<String>,
}

impl ExecChannel {
    pub fn direct(backend_id: impl Into<String>) -> Self {
        Self {
            backend_id: backend_id.into(),
            hops: vec![],
            exec_target_id: None,
        }
    }

    /// Convenience constructor for a single-hop channel.
    pub fn via(backend_id: impl Into<String>, intermediate_id: impl Into<String>) -> Self {
        Self {
            backend_id: backend_id.into(),
            hops: vec![intermediate_id.into()],
            exec_target_id: None,
        }
    }
}

#[derive(Default)]
pub struct TtpExecutionProcessing {
    pub updates: FactsUpdate,
    pub parse_audits: Vec<ParseAudit>,
    /// Effective success flag — may differ from `TtpExecuted.success` when a
    /// parser detected a semantic failure in an otherwise successful transport
    /// response (e.g. a Kubernetes API 403 Forbidden inside an HTTP 200 body).
    pub effective_success: bool,
    /// Human-readable reason for the overridden failure, if any.
    pub effective_fail_reason: String,
}
