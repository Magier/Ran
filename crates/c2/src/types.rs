use std::collections::HashMap;

use armory::{Procedure, Ttp};
use serde::{Deserialize, Serialize};

/// The backend ID for the built-in Ran C2.
pub const BUILTIN_C2_ID: &str = "c2/ran";

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
    /// The physical entity the C2 backend execs into to deliver the command.
    /// Equals `target_id` for direct pod targets; differs when the semantic
    /// target is not itself an exec-capable system (e.g. a ServiceAccount is
    /// resolved to its pod, or a K8sNode is reached via a container escape
    /// hop through a pod).
    pub exec_entity_id: String,
    pub exec_system_id: String,
    /// Unix timestamp (milliseconds) when the command was dispatched.
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtpExecuted {
    pub id: String,
    pub success: bool,
    pub results: Vec<String>,
    pub exit_code: i32,
    pub fail_reason: String,
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
