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
}

#[derive(Default)]
pub struct TtpExecutionProcessing {
    pub updates: FactsUpdate,
    pub parse_audits: Vec<ParseAudit>,
}
