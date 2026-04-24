use std::collections::HashMap;

use armory::{Procedure, Ttp};
use serde::{Deserialize, Serialize};

/// The backend ID for the built-in Ran C2.
pub const BUILTIN_C2_ID: &str = "c2/ran";

/// Post-processing to apply to the raw command output before any parser sees it.
///
/// Each variant corresponds to a transport-level wrapping that a C2 channel may
/// apply to its output.  The campaign layer inspects this field and unwraps the
/// output before handing it to the output parsers, so parsers never need to know
/// which channel transported the command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputTransform {
    /// The raw output is a JSON response envelope (produced by ran-ws / kubelet-pod-exec).
    /// The actual stdout must be extracted from the JSON before parsing.
    JsonEnvelope,
}

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
    /// Unix timestamp (milliseconds) when the command was dispatched.
    pub started_at_ms: u64,
    /// Output post-processing required before parsers run.
    /// `None` means the raw output can be parsed directly.
    #[serde(default)]
    pub output_transform: Option<OutputTransform>,
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
