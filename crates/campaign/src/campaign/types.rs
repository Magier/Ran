use std::collections::HashMap;

use armory::Ttp;
use serde::{Deserialize, Serialize};

use crate::{FactsUpdate, ParseAudit};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteActionRequest {
    pub action_id: String,
    pub exec_system_id: Option<String>,
    pub target_id: String,
    pub procedure_id: Option<String>,
    pub args: HashMap<String, String>,
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
}

/// A resolved execution channel for a TTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecChannel {
    /// C2 backend ID forwarded to the C2Manager (e.g. `"c2/ran"`).
    pub backend_id: String,
    /// Intermediate entity to proxy through, if any.
    /// `None` = reach target directly. `Some(id)` = route via a compromised
    /// intermediate system (reserved for future agent-based backends).
    pub via: Option<String>,
}

impl ExecChannel {
    pub fn direct(backend_id: impl Into<String>) -> Self {
        Self {
            backend_id: backend_id.into(),
            via: None,
        }
    }

    pub fn via(backend_id: impl Into<String>, intermediate_id: impl Into<String>) -> Self {
        Self {
            backend_id: backend_id.into(),
            via: Some(intermediate_id.into()),
        }
    }
}

#[derive(Default)]
pub struct TtpExecutionProcessing {
    pub updates: FactsUpdate,
    pub parse_audits: Vec<ParseAudit>,
}
