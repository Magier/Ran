use std::collections::HashMap;

use armory::{Procedure, Ttp};
use ran_domain::OutputTransformKind;
use serde::{Deserialize, Serialize};

/// The backend ID for the built-in Ran C2.
pub const BUILTIN_C2_ID: &str = "c2/ran";

/// Alias to the domain-owned output-transform enum.
pub type OutputTransform = OutputTransformKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecTtp {
    pub id: String,
    pub ttp: Ttp,
    pub procedure: Procedure,
    pub args: HashMap<String, String>,
    /// The semantic target entity — the entity whose knowledge graph entry,
    /// system info, and execution records are updated by this command.
    /// Always the entity the operator is working with (e.g. a K8sNode after
    /// a container escape, or a ServiceAccount being exploited).
    pub target_id: String,
    /// Ordered execution chain: first element = what BuiltinC2 execs into,
    /// last element = where the command actually runs. Empty for purely
    /// local/C2-side commands.
    pub exec_chain: Vec<String>,
    pub exec_system_id: String,
    /// Authentication identity selected for Kubernetes API/kubectl operations.
    /// This is an entity ID only; credential material is never serialized here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_identity_id: Option<String>,
    /// Unix timestamp (milliseconds) when the command was dispatched.
    pub started_at_ms: u64,
    /// Output post-processing required before parsers run.
    /// `None` means the raw output can be parsed directly.
    #[serde(default)]
    pub output_transform: Option<OutputTransform>,
    /// True when this command was generated as part of post-emulation cleanup
    /// rather than the primary attack sequence.
    #[serde(default)]
    pub is_cleanup: bool,
    /// Operator/agent rationale for running this command — why this step was
    /// chosen. Set from `ExecuteActionRequest.reasoning`; empty when none was
    /// supplied. Carried through to the audit record.
    #[serde(default)]
    pub reasoning: String,
}

impl ExecTtp {
    /// The entity BuiltinC2 directly execs into (first hop for routing).
    pub fn exec_entity(&self) -> &str {
        self.exec_chain.first().map(String::as_str).unwrap_or("")
    }
    /// The final entity where the command actually runs (for attribution).
    pub fn exec_target(&self) -> &str {
        self.exec_chain.last().map(String::as_str).unwrap_or("")
    }
}

/// Session probe data collected when a synchronous exec session is opened as
/// part of a TTP execution (e.g. `c2.kubectl_exec()`).
/// Embedded in `TtpExecuted` so the campaign can apply it after TTP effects,
/// avoiding the ordering problems of a separate `SessionConnected` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConnectedData {
    pub backend_id: String,
    pub target_entity_id: String,
    pub hostname: String,
    pub user: String,
    pub os: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtpExecuted {
    pub id: String,
    pub success: bool,
    pub results: Vec<String>,
    pub exit_code: i32,
    pub fail_reason: String,
    /// Populated when a synchronous exec session was opened during TTP execution.
    /// The campaign processes this after applying TTP effects so the exec-channel
    /// edge created by those effects is available for session activation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_connected: Option<SessionConnectedData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum C2Event {
    TtpExecuted {
        cmd: Box<ExecTtp>,
        event: TtpExecuted,
    },
    /// A TCP listener was successfully bound on the given port.
    ListenerStarted { port: u16, protocol: String },
    /// A reverse-shell connected, probed, and the session backend is now live.
    SessionConnected {
        backend_id: String,
        /// `node/{hostname}` — the entity this session exits into.
        target_entity_id: String,
        hostname: String,
        user: String,
        os: String,
        port: Option<u16>,
    },
    /// A session backend lost its connection.
    SessionLost {
        backend_id: String,
        target_entity_id: String,
    },
}
