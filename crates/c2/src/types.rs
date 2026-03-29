use std::collections::HashMap;

use armory::{Procedure, Ttp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecTtp {
    pub id: String,
    pub ttp: Ttp,
    pub procedure: Procedure,
    pub args: HashMap<String, String>,
    pub target_id: String,
    pub exec_system_id: String,
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
    TtpExecuted { cmd: ExecTtp, event: TtpExecuted },
}
