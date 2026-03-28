use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtpParam {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub description: String,
    pub required: bool,
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Procedure {
    pub id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(rename = "isLocalCommand", skip_serializing_if = "Option::is_none")]
    pub is_local_command: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ttp {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tactic: String,
    pub techniques: Vec<String>,
    pub status: String,
    pub params: Vec<TtpParam>,
    pub requires: JsonMap<String, JsonValue>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub effects: Vec<String>,
    pub procedures: Vec<Procedure>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub references: Vec<String>,
}
