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
    /// Structured Kubernetes API request spec. When present, the runtime
    /// materializes this into a concrete kubectl/curl shell command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k8s_request: Option<JsonValue>,
    /// Ordered list of typed steps (fetch, chmod, run, …). When present the
    /// runtime compiles each step into a shell snippet and joins them with
    /// `&&`. Takes precedence over `command`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<JsonValue>,
}

impl Procedure {
    /// Construct a minimal shell-command procedure with all optional fields
    /// set to `None`. Use struct update syntax to override specific fields:
    ///
    /// ```ignore
    /// Procedure { tool: Some("curl".into()), ..Procedure::new("curl", "") }
    /// ```
    pub fn new(id: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            command: command.into(),
            tool: None,
            is_local_command: None,
            http_request: None,
            k8s_request: None,
            steps: None,
        }
    }
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
    /// When set, this TTP acts as a tool implementation for the named slot
    /// (e.g. `"http-request"`). Procedures in other TTPs that reference this
    /// slot name via their `tool` field will be expanded into one concrete
    /// procedure per tool that fills the slot.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_slot: Option<String>,
}

impl Ttp {
    /// Construct a TTP with only the three required identifiers. All other
    /// fields default to empty / `None`. Use struct update syntax to set
    /// specific fields without spelling out the full struct:
    ///
    /// ```ignore
    /// Ttp {
    ///     procedures: vec![Procedure::new("shell", "id")],
    ///     status: "enabled".to_string(),
    ///     ..Ttp::new("list-pods", "List Pods", "Discovery")
    /// }
    /// ```
    pub fn new(id: impl Into<String>, name: impl Into<String>, tactic: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            tactic: tactic.into(),
            techniques: vec![],
            status: "stable".to_string(),
            params: vec![],
            requires: JsonMap::new(),
            effects: vec![],
            procedures: vec![],
            cleanup: None,
            references: vec![],
            tool_slot: None,
        }
    }
}
