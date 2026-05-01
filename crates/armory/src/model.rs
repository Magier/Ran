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
    #[serde(default)]
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(rename = "isLocalCommand", skip_serializing_if = "Option::is_none")]
    pub is_local_command: Option<bool>,
    /// Structured HTTP request spec. When present, the runtime materializes
    /// this into a concrete curl/wget shell command. Takes precedence over
    /// `command` for `http-request` procedures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_request: Option<JsonValue>,
    /// Ordered list of typed steps (fetch, chmod, run, …). When present the
    /// runtime compiles each step into a shell snippet and joins them with
    /// `&&`. Takes precedence over `command`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<JsonValue>,
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cleanup: Option<Procedure>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub references: Vec<String>,
}
