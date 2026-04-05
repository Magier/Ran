use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use c2::{ExecTtp, TtpExecuted};
use serde::{Deserialize, Serialize};

/// A single recorded execution — the grounded command, its arguments, and the
/// raw results returned by the C2 backend.  This forms the append-only audit
/// trail for a campaign session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// Unique command identifier (same as `ExecTtp.id` / `TtpExecuted.id`).
    pub id: String,
    /// TTP identifier (e.g. `"k8s.exec-into-pod"`).
    pub ttp_id: String,
    /// Human-readable TTP name.
    pub ttp_name: String,
    /// MITRE tactic (e.g. `"Execution"`).
    pub tactic: String,
    /// ID of the entity that was targeted.
    pub target_id: String,
    /// ID of the C2 system that ran the command (empty string for direct builtin exec).
    pub exec_system_id: String,
    /// ID of the procedure variant that was selected.
    pub procedure_id: String,
    /// The fully-grounded command string that was sent to the C2 backend.
    pub command: String,
    /// Resolved arguments after default-filling and template substitution.
    pub args: HashMap<String, String>,
    /// Whether the command exited successfully (exit code 0).
    pub success: bool,
    /// Raw exit code returned by the process.
    pub exit_code: i32,
    /// Raw output lines from the C2 backend (stdout first, then stderr).
    pub results: Vec<String>,
    /// Human-readable reason for failure, if applicable.
    pub fail_reason: String,
    /// Unix timestamp (milliseconds) when the command was dispatched.
    pub started_at_ms: u64,
    /// Unix timestamp (milliseconds) when the result was received.
    pub completed_at_ms: u64,
}

impl ExecutionRecord {
    pub fn from_execution(cmd: &ExecTtp, event: &TtpExecuted) -> Self {
        let completed_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            id: cmd.id.clone(),
            ttp_id: cmd.ttp.id.clone(),
            ttp_name: cmd.ttp.name.clone(),
            tactic: cmd.ttp.tactic.clone(),
            target_id: cmd.target_id.clone(),
            exec_system_id: cmd.exec_system_id.clone(),
            procedure_id: cmd.procedure.id.clone(),
            command: cmd.procedure.command.clone(),
            args: cmd.args.clone(),
            success: event.success,
            exit_code: event.exit_code,
            results: event.results.clone(),
            fail_reason: event.fail_reason.clone(),
            started_at_ms: cmd.started_at_ms,
            completed_at_ms,
        }
    }
}
