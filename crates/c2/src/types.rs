use std::collections::HashMap;

use armory::{Procedure, Ttp};
use ran_domain::OutputTransformKind;
use serde::{Deserialize, Serialize};

/// The backend ID for the built-in Ran C2.
pub const BUILTIN_C2_ID: &str = "c2/ran";

/// `fail_reason` emitted by a [`crate::ShellSession`] when its underlying stream
/// hits EOF mid-command - i.e. the live session died. Used as a stable sentinel
/// so the executor can tell a dead session apart from an ordinary non-zero
/// command exit and signal that the session's exec-channel edge is now broken.
pub const SESSION_CLOSED_UNEXPECTEDLY: &str = "shell session closed unexpectedly";

/// Prefix of the `fail_reason` a [`crate::ShellSession`] emits once it has hit
/// [`SESSION_TIMEOUT_BREAK_THRESHOLD`] consecutive command timeouts with no
/// intervening response. A single timeout is treated as a merely-slow command
/// and leaves the session alone; only sustained unresponsiveness escalates to a
/// session death that breaks the exec-channel edge. Matched by prefix because
/// the full message also carries the timeout counts.
pub const SESSION_UNRESPONSIVE_PREFIX: &str = "shell session unresponsive";

/// Consecutive command timeouts (with no response in between) after which a
/// [`crate::ShellSession`] is considered dead rather than merely slow.
pub const SESSION_TIMEOUT_BREAK_THRESHOLD: u64 = 2;

/// Whether a `fail_reason` denotes a dead session - either an unexpected close
/// or sustained unresponsiveness - as opposed to an ordinary command failure
/// (non-zero exit, a single slow-command timeout). The executor uses this to
/// decide when to signal that the session's exec-channel edge is broken.
pub fn is_session_death_reason(reason: &str) -> bool {
    reason == SESSION_CLOSED_UNEXPECTEDLY || reason.starts_with(SESSION_UNRESPONSIVE_PREFIX)
}
pub const DEFAULT_EXECUTION_TIMEOUT_SECONDS: u64 = 60;

fn default_execution_timeout_seconds() -> u64 {
    DEFAULT_EXECUTION_TIMEOUT_SECONDS
}

/// Alias to the domain-owned output-transform enum.
pub type OutputTransform = OutputTransformKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecTtp {
    pub id: String,
    pub ttp: Ttp,
    pub procedure: Procedure,
    pub args: HashMap<String, String>,
    /// The semantic target entity - the entity whose knowledge graph entry,
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
    /// Maximum wall-clock time allowed for this command to complete.
    #[serde(default = "default_execution_timeout_seconds")]
    pub execution_timeout_seconds: u64,
    /// Output post-processing required before parsers run.
    /// `None` means the raw output can be parsed directly.
    #[serde(default)]
    pub output_transform: Option<OutputTransform>,
    /// True when this command was generated as part of post-emulation cleanup
    /// rather than the primary attack sequence.
    #[serde(default)]
    pub is_cleanup: bool,
    /// Operator/agent rationale for running this command - why this step was
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
    ///
    /// `cmd_id` is the execution that asked for it, so the campaign can attribute
    /// the resulting listener entity to that action rather than inventing an id
    /// no consumer can match.
    ListenerStarted {
        cmd_id: String,
        port: u16,
        protocol: String,
    },
    /// A listener's accept loop was torn down and its port released. Sessions
    /// that connected through it stay live - they are backends in their own
    /// right and do not depend on the listener that accepted them.
    ListenerStopped { cmd_id: String, port: u16 },
    /// A reverse-shell connected, probed, and the session backend is now live.
    SessionConnected {
        backend_id: String,
        /// `node/{hostname}` - the entity this session exits into.
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
