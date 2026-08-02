//! MCP server embedded in the Ran axum process.
//!
//! Exposes Ran's knowledge graph, armory, and execution engine as MCP tools so
//! that 3rd-party LLM agents can drive adversary emulation campaigns.
//!
//! # Transport
//!
//! The server uses the Streamable HTTP transport (`/mcp`), which supports every
//! modern MCP client (VS Code Copilot, Claude Desktop via mcp-remote, etc.).
//!
//! # Tools exposed
//!
//! | Category | Tool |
//! |---|---|
//! | Discovery | `get_graph`, `get_entity`, `get_attack_surface`, `resolve_workload` |
//! | Campaign | `get_campaign_state`, `get_attack_flow` |
//! | Armory | `list_ttps`, `get_applicable_ttps`, `get_ttp_detail` |
//! | Execution | `execute_action`, `wait_for_result` |
//! | Goal eval | `check_rbac_goal`, `check_access_level` |
//! | Extension | `add_parser`, `list_parse_audits` |

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use campaign::{CampaignEventBus, ExecuteActionRequest};
use ran_domain::{Entity, ServiceAccount};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, Implementation, JsonObject, ListToolsResult,
    ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{json, Value};

use crate::state_conversions::{campaign_to_campaign_state, campaign_to_graph};
use crate::{ApiError, ApiService, GetArmoryParams};

// ---------------------------------------------------------------------------
// Public configuration type — passed in from the CLI bootstrap
// ---------------------------------------------------------------------------

/// Extra state needed by the MCP server that isn't part of `ApiService`.
#[derive(Clone)]
pub struct McpConfig {
    /// Broadcast bus used by `wait_for_result` to detect TTP completion.
    pub campaign_events: CampaignEventBus,
    /// Directory where dynamically generated parser scripts are written.
    /// Typically `armory/parsers/` (sibling of `armory/TTPs/`).
    pub parsers_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// MCP `ServerHandler` that wraps Ran's `ApiService` + extra MCP state.
#[derive(Clone)]
pub struct RanMcpHandler<S: ApiService> {
    api: S,
    mcp: Arc<McpConfig>,
}

impl<S: ApiService> RanMcpHandler<S> {
    pub fn new(api: S, mcp: Arc<McpConfig>) -> Self {
        Self { api, mcp }
    }
}

// ---------------------------------------------------------------------------
// Helper: convert ApiError → McpError
// ---------------------------------------------------------------------------

fn api_err(e: ApiError) -> McpError {
    McpError::internal_error(e.body.error, None)
}

fn internal(msg: impl Into<String>) -> McpError {
    McpError::internal_error(msg.into(), None)
}

fn invalid_param(msg: impl Into<String>) -> McpError {
    McpError::invalid_params(msg.into(), None)
}

/// Wrap a serialisable value as a `CallToolResult` text block.
fn json_result(value: impl serde::Serialize) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(&value).map_err(|e| internal(e.to_string()))?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

// ---------------------------------------------------------------------------
// Tool input helpers
// ---------------------------------------------------------------------------

/// Extract a required string field from a `Value::Object` argument map.
fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, McpError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_param(format!("missing required argument `{key}`")))
}

/// Extract an optional string field.
fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Convert a `serde_json::Value::Object` into a `JsonObject` (the type `Tool::new` expects).
fn schema(v: Value) -> JsonObject {
    match v {
        Value::Object(m) => m,
        _ => unreachable!("schema must be a JSON object"),
    }
}

// ---------------------------------------------------------------------------
// Individual tool implementations  (async fns called from call_tool dispatch)
// ---------------------------------------------------------------------------

impl<S: ApiService> RanMcpHandler<S> {
    // ---- Discovery ---------------------------------------------------------

    async fn tool_get_graph(&self) -> Result<CallToolResult, McpError> {
        let campaign = self.api.get_campaign().await.map_err(api_err)?;
        json_result(campaign_to_graph(&campaign))
    }

    async fn tool_get_campaign_state(&self) -> Result<CallToolResult, McpError> {
        let campaign = self.api.get_campaign().await.map_err(api_err)?;
        json_result(campaign_to_campaign_state(&campaign))
    }

    async fn tool_get_entity(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let entity_id = req_str(args, "entity_id")?;
        let campaign = self.api.get_campaign().await.map_err(api_err)?;
        let entity = campaign
            .get_entities()
            .into_iter()
            .find(|e| e.entity_id().0 == entity_id)
            .ok_or_else(|| invalid_param(format!("entity `{entity_id}` not found")))?;
        json_result(json!({
            "id": entity.entity_id().0,
            "kind": entity.entity_kind(),
            "name": entity.entity_name(),
            "namespace": entity.namespace(),
        }))
    }

    async fn tool_get_attack_surface(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let entity_id = req_str(args, "entity_id")?;
        let campaign = self.api.get_campaign().await.map_err(api_err)?;

        let inbound: Vec<_> = campaign
            .get_relations()
            .iter()
            .filter(|r| r.target_id == entity_id)
            .map(|r| json!({ "from": r.source_id, "relation": r.name }))
            .collect();

        let outbound: Vec<_> = campaign
            .get_relations()
            .iter()
            .filter(|r| r.source_id == entity_id)
            .map(|r| json!({ "to": r.target_id, "relation": r.name }))
            .collect();

        // Include the entity's own fields for context.
        let entity_detail = campaign
            .get_entities()
            .into_iter()
            .find(|e| e.entity_id().0 == entity_id)
            .map(|e| json!({ "kind": e.entity_kind(), "name": e.entity_name() }))
            .unwrap_or(Value::Null);

        json_result(json!({
            "entity": entity_detail,
            "inbound_relations": inbound,
            "outbound_relations": outbound,
        }))
    }

    async fn tool_resolve_workload(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let name = req_str(args, "name")?.to_ascii_lowercase();
        let campaign = self.api.get_campaign().await.map_err(api_err)?;

        let matches: Vec<_> = campaign
            .get_entities()
            .into_iter()
            .filter(|e| {
                e.entity_name().to_ascii_lowercase().contains(&name)
                    || e.entity_id().0.to_ascii_lowercase().contains(&name)
            })
            .map(|e| {
                json!({
                    "id": e.entity_id().0,
                    "kind": e.entity_kind(),
                    "name": e.entity_name(),
                    "namespace": e.namespace(),
                })
            })
            .collect();

        json_result(json!({ "matches": matches }))
    }

    async fn tool_get_attack_flow(&self) -> Result<CallToolResult, McpError> {
        let campaign = self.api.get_campaign().await.map_err(api_err)?;
        let records: Vec<_> = campaign
            .get_execution_records()
            .iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "ttp_id": r.ttp_id,
                    "ttp_name": r.ttp_name,
                    "tactic": r.tactic,
                    "target_id": r.target_id,
                    "success": r.success,
                    "exit_code": r.exit_code,
                    "fail_reason": r.fail_reason,
                })
            })
            .collect();
        json_result(json!({ "steps": records }))
    }

    // ---- Armory ------------------------------------------------------------

    async fn tool_list_ttps(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let tactic = opt_str(args, "tactic").map(str::to_owned);
        let ttps = self
            .api
            .get_armory(GetArmoryParams { tactic })
            .await
            .map_err(api_err)?;
        json_result(ttps)
    }

    async fn tool_get_applicable_ttps(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let target_id = req_str(args, "target_id")?;
        let all_ttps = self
            .api
            .get_armory(GetArmoryParams { tactic: None })
            .await
            .map_err(api_err)?;
        let campaign = self.api.get_campaign().await.map_err(api_err)?;

        use campaign::ttp_applicability::{resolve_target_context, ttp_applicable_for_target};

        let tc = resolve_target_context(&campaign, target_id).ok_or_else(|| {
            invalid_param(format!("entity `{target_id}` not found. For initial access, use the Cluster entity as target_id (not a pod ID) and pass the pod name as a parameter."))
        })?;

        let applicable: Vec<_> = all_ttps
            .into_iter()
            .filter(|ttp| ttp_applicable_for_target(ttp, &campaign, &tc))
            .collect();

        json_result(applicable)
    }

    async fn tool_get_ttp_detail(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let ttp_id = req_str(args, "ttp_id")?;
        let ttps = self
            .api
            .get_armory(GetArmoryParams { tactic: None })
            .await
            .map_err(api_err)?;
        let ttp = ttps
            .into_iter()
            .find(|t| t.id == ttp_id)
            .ok_or_else(|| invalid_param(format!("TTP `{ttp_id}` not found in armory")))?;
        json_result(ttp)
    }

    // ---- Execution ---------------------------------------------------------

    async fn tool_execute_action(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let action_id = req_str(args, "action_id")?.to_owned();
        let target_id = req_str(args, "target_id")?.to_owned();
        let exec_system_id = opt_str(args, "exec_system_id").map(str::to_owned);
        let procedure_id = opt_str(args, "procedure_id").map(str::to_owned);

        // Validate that target_id is a known entity in the campaign graph.
        // This prevents silent failures when the agent passes a workload/deployment
        // name (e.g. "entry-hall") instead of a real entity ID.
        let known = {
            let campaign = self.api.get_campaign().await.map_err(api_err)?;
            campaign
                .get_entities()
                .into_iter()
                .any(|e| e.entity_id().0 == target_id)
        };
        if !known {
            return Err(invalid_param(format!(
                "entity `{target_id}` is not in the campaign graph. \
                 For initial access use the Cluster entity as target_id \
                 and pass the pod name/namespace as TTP parameters."
            )));
        }

        let extra_args: std::collections::HashMap<String, String> = args
            .get("args")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect()
            })
            .unwrap_or_default();

        let reasoning = opt_str(args, "reasoning")
            .map(str::to_owned)
            .filter(|s| !s.trim().is_empty());

        let result = self
            .api
            .execute_action(ExecuteActionRequest {
                action_id,
                target_id,
                exec_system_id,
                procedure_id,
                args: extra_args,
                reasoning,
            })
            .await
            .map_err(api_err)?;

        json_result(json!({
            "cmd_id": result.cmd_id,
            "queued": true,
        }))
    }

    /// Poll execution records until the given cmd_id appears or timeout (60s).
    async fn tool_wait_for_result(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let cmd_id = req_str(args, "cmd_id")?.to_owned();
        let deadline = Instant::now() + Duration::from_secs(60);

        loop {
            let campaign = self.api.get_campaign().await.map_err(api_err)?;
            if let Some(record) = campaign
                .get_execution_records()
                .iter()
                .find(|r| r.id == cmd_id)
            {
                let parse_audits: Vec<_> = campaign
                    .get_parse_audits()
                    .iter()
                    .filter(|a| a.cmd_id == record.id)
                    .map(|a| {
                        json!({
                            "effect_id": a.effect_id,
                            "result": format!("{:?}", a.parse_result),
                            "detail": a.detail,
                            "raw_preview": a.raw_output_preview,
                        })
                    })
                    .collect();
                return json_result(json!({
                    "cmd_id": record.id,
                    "ttp_id": record.ttp_id,
                    "target_id": record.target_id,
                    "success": record.success,
                    "exit_code": record.exit_code,
                    "stdout": record.results.first().cloned().unwrap_or_default(),
                    "stderr": record.results.get(1).cloned().unwrap_or_default(),
                    "fail_reason": record.fail_reason,
                    "parse_audits": parse_audits,
                }));
            }

            if Instant::now() >= deadline {
                return Err(internal(format!(
                    "timeout: cmd `{cmd_id}` not completed within 60s"
                )));
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    // ---- Goal evaluation ---------------------------------------------------

    async fn tool_check_rbac_goal(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let entity_id = req_str(args, "entity_id")?;
        let required_verbs: Vec<String> = args
            .get("verbs")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| vec!["*".to_owned()]);
        let required_resources: Vec<String> = args
            .get("resources")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(|| vec!["*".to_owned()]);

        // RBAC permissions live on ServiceAccount entities, not on the system
        // entity itself. Look for a SA with this id.
        let campaign = self.api.get_campaign().await.map_err(api_err)?;
        let sa = campaign
            .entities
            .values::<ServiceAccount>()
            .find(|sa| sa.entity_id().0 == entity_id);
        let entitlements = sa.map(|sa| &sa.entitlements[..]).unwrap_or(&[]);

        let mut missing = Vec::new();
        for verb in &required_verbs {
            for resource in &required_resources {
                let has = entitlements.iter().any(|p| {
                    (p.verb == *verb || p.verb == "*" || verb == "*")
                        && (p.resource_type == *resource
                            || p.resource_type == "*"
                            || resource == "*")
                });
                if !has {
                    missing.push(json!({ "verb": verb, "resource": resource }));
                }
            }
        }

        json_result(json!({
            "entity_id": entity_id,
            "achieved": missing.is_empty(),
            "missing_permissions": missing,
            "held_permissions": entitlements,
        }))
    }

    async fn tool_check_access_level(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let entity_id = req_str(args, "entity_id")?;
        let campaign = self.api.get_campaign().await.map_err(api_err)?;

        let entity = campaign.get_system_entity(entity_id).ok_or_else(|| {
            invalid_param(format!(
                "entity `{entity_id}` not found or not a system entity"
            ))
        })?;

        let access_level = entity.entity().system().access_level;
        json_result(json!({
            "entity_id": entity_id,
            "access_level": format!("{access_level:?}"),
        }))
    }

    // ---- Dynamic extension -------------------------------------------------

    async fn tool_list_parse_audits(&self) -> Result<CallToolResult, McpError> {
        let campaign = self.api.get_campaign().await.map_err(api_err)?;
        let audits: Vec<_> = campaign
            .get_parse_audits()
            .iter()
            .map(|a| {
                json!({
                    "effect_id": a.effect_id,
                    "ttp_id": a.ttp_id,
                    "target_id": a.target_id,
                    "result": format!("{:?}", a.parse_result),
                    "detail": a.detail,
                    "raw_preview": a.raw_output_preview,
                })
            })
            .collect();
        json_result(json!({ "audits": audits }))
    }

    async fn tool_add_parser(&self, args: &Value) -> Result<CallToolResult, McpError> {
        let effect_id = req_str(args, "effect_id")?;
        let script_content = req_str(args, "script_content")?;

        let parsers_dir = self.mcp.parsers_dir.as_ref().ok_or_else(|| {
            internal("parsers_dir is not configured — start Ran with a valid armory directory")
        })?;

        // Sanitise the effect_id so it can only produce a safe filename.
        let safe_name = effect_id
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .to_ascii_lowercase();

        if safe_name.is_empty() || safe_name.starts_with('.') {
            return Err(invalid_param("effect_id must be a non-empty identifier"));
        }

        std::fs::create_dir_all(parsers_dir)
            .map_err(|e| internal(format!("failed to create parsers dir: {e}")))?;

        let path = parsers_dir.join(format!("{safe_name}.py"));
        std::fs::write(&path, script_content)
            .map_err(|e| internal(format!("failed to write parser script: {e}")))?;

        json_result(json!({
            "effect_id": effect_id,
            "path": path.display().to_string(),
            "written": true,
        }))
    }

    async fn tool_reset_campaign(&self) -> Result<CallToolResult, McpError> {
        self.api.reset_campaign().await.map_err(api_err)?;
        json_result(json!({ "reset": true }))
    }

    async fn tool_get_initial_access_candidates(
        &self,
        args: &Value,
    ) -> Result<CallToolResult, McpError> {
        let namespace = opt_str(args, "namespace").map(str::to_owned);
        let name_filter = opt_str(args, "name_filter");

        let params = crate::GetRunningPodsParams {
            namespace: namespace.clone(),
        };
        let pods = self.api.get_running_pods(params).await.map_err(api_err)?;

        let candidates: Vec<_> = pods
            .into_iter()
            .filter(|p| {
                if let Some(filter) = name_filter {
                    p.name
                        .to_ascii_lowercase()
                        .contains(&filter.to_ascii_lowercase())
                } else {
                    true
                }
            })
            .filter(|p| p.ready.unwrap_or(true))
            .map(|p| {
                json!({
                    "id": p.id,
                    "name": p.name,
                    "namespace": p.namespace,
                    "phase": p.phase,
                    "ready": p.ready,
                })
            })
            .collect();

        json_result(json!({
            "candidates": candidates,
            "hint": "Use the `id` field as `target_id` when calling execute_action with an InitialAccess TTP."
        }))
    }
}

// ---------------------------------------------------------------------------
// ServerHandler impl
// ---------------------------------------------------------------------------

fn tool_defs() -> Vec<Tool> {
    // Build tool definitions once; the list is static.
    vec![
        Tool::new(
            "get_graph",
            "Return the full knowledge graph: all discovered entities and relations.",
            schema(json!({ "type": "object", "properties": {} })),
        ),
        Tool::new(
            "get_campaign_state",
            "Return the current campaign state: entities with their discovered facts.",
            schema(json!({ "type": "object", "properties": {} })),
        ),
        Tool::new(
            "get_entity",
            "Get details about a specific entity by its ID.",
            schema(json!({
                "type": "object",
                "required": ["entity_id"],
                "properties": {
                    "entity_id": { "type": "string", "description": "Entity ID (e.g. 'ns/default/pod/my-pod')" }
                }
            })),
        ),
        Tool::new(
            "get_attack_surface",
            "Get attack surface for an entity: its inbound and outbound relations (reachability, tokens, RBAC, etc.).",
            schema(json!({
                "type": "object",
                "required": ["entity_id"],
                "properties": {
                    "entity_id": { "type": "string", "description": "Entity ID" }
                }
            })),
        ),
        Tool::new(
            "resolve_workload",
            "Resolve a workload name (partial match) to a list of concrete entity IDs in the graph.",
            schema(json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": { "type": "string", "description": "Workload name or partial name to search for" }
                }
            })),
        ),
        Tool::new(
            "get_attack_flow",
            "Get the ordered attack flow: all executed actions and their causal edges.",
            schema(json!({ "type": "object", "properties": {} })),
        ),
        Tool::new(
            "list_ttps",
            "List TTPs from the armory, optionally filtered by MITRE tactic.",
            schema(json!({
                "type": "object",
                "properties": {
                    "tactic": { "type": "string", "description": "MITRE tactic name (optional, e.g. 'Discovery')" }
                }
            })),
        ),
        Tool::new(
            "get_applicable_ttps",
            "List TTPs that are applicable to a specific entity given its kind, RBAC, and the current campaign state.",
            schema(json!({
                "type": "object",
                "required": ["target_id"],
                "properties": {
                    "target_id": { "type": "string", "description": "Entity ID to check applicability for" }
                }
            })),
        ),
        Tool::new(
            "get_ttp_detail",
            "Get the full schema of a TTP: preconditions, parameters, procedures, and effects.",
            schema(json!({
                "type": "object",
                "required": ["ttp_id"],
                "properties": {
                    "ttp_id": { "type": "string", "description": "TTP ID (e.g. 'get-pods')" }
                }
            })),
        ),
        Tool::new(
            "execute_action",
            "Execute a TTP against a target entity. Returns a cmd_id to track the execution. \
             ALWAYS pass `reasoning` explaining why you chose this action now — it is recorded \
             on the execution timeline and is essential for an auditable, explainable assessment.",
            schema(json!({
                "type": "object",
                "required": ["action_id", "target_id"],
                "properties": {
                    "action_id": { "type": "string", "description": "TTP ID to execute" },
                    "target_id": { "type": "string", "description": "Entity ID to execute against" },
                    "exec_system_id": { "type": "string", "description": "ID of the system to run the command from (optional)" },
                    "procedure_id": { "type": "string", "description": "Specific procedure variant to use (optional)" },
                    "args": { "type": "object", "description": "TTP parameter overrides (key-value string pairs)", "additionalProperties": { "type": "string" } },
                    "reasoning": { "type": "string", "description": "STRONGLY ENCOURAGED. Your rationale for running this action at this point in the assessment: what you expect it to reveal or achieve, why this target, and how it follows from prior findings. Recorded on the execution record for audit and replay. Provide it on every call unless truly trivial." }
                }
            })),
        ),
        Tool::new(
            "wait_for_result",
            "Block until a previously enqueued action completes (up to 60s). Returns stdout, stderr, and success status.",
            schema(json!({
                "type": "object",
                "required": ["cmd_id"],
                "properties": {
                    "cmd_id": { "type": "string", "description": "Command ID returned by execute_action" }
                }
            })),
        ),
        Tool::new(
            "check_rbac_goal",
            "Check if an entity holds sufficient RBAC permissions to satisfy a goal (e.g. cluster-admin).",
            schema(json!({
                "type": "object",
                "required": ["entity_id"],
                "properties": {
                    "entity_id": { "type": "string", "description": "Entity ID (ServiceAccount, Pod, or Node)" },
                    "verbs": { "type": "array", "items": { "type": "string" }, "description": "Required verbs (default: ['*'])" },
                    "resources": { "type": "array", "items": { "type": "string" }, "description": "Required resources (default: ['*'])" }
                }
            })),
        ),
        Tool::new(
            "check_access_level",
            "Check the current access level for an entity (container, container-admin, host, cluster-admin, etc.).",
            schema(json!({
                "type": "object",
                "required": ["entity_id"],
                "properties": {
                    "entity_id": { "type": "string", "description": "Entity ID (must be a Pod or Node)" }
                }
            })),
        ),
        Tool::new(
            "list_parse_audits",
            "List recent parse audit entries, including NoParser and UnknownFormat gaps the agent can fill.",
            schema(json!({ "type": "object", "properties": {} })),
        ),
        Tool::new(
            "add_parser",
            "Write a Python parser script to armory/parsers/{effect_id}.py. Ran will discover and use it for future executions.",
            schema(json!({
                "type": "object",
                "required": ["effect_id", "script_content"],
                "properties": {
                    "effect_id": { "type": "string", "description": "Effect ID the parser handles (e.g. 'k8s.podList')" },
                    "script_content": { "type": "string", "description": "Python script content (reads ExternalParseRequest JSON on stdin, writes ExternalParseResponse JSON on stdout)" }
                }
            })),
        ),
        Tool::new(
            "reset_campaign",
            "Reset the campaign: clear all discovered entities, relations, execution records, and parse audits. Use before starting a fresh run.",
            schema(json!({ "type": "object", "properties": {} })),
        ),
        Tool::new(
            "get_initial_access_candidates",
            "List running pods directly from Kubernetes (not the campaign graph). \
             Use this ONLY to find an initial foothold — it returns live pods the agent \
             can exec into as a first step. Optionally filter by namespace.",
            schema(json!({
                "type": "object",
                "properties": {
                    "namespace": { "type": "string", "description": "Kubernetes namespace to filter by (optional; omit for all namespaces)" },
                    "name_filter": { "type": "string", "description": "Case-insensitive substring to filter pod names (optional)" }
                }
            })),
        ),
    ]
}

impl<S: ApiService> ServerHandler for RanMcpHandler<S> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("ran-mcp", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tool_defs()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let args = Value::Object(request.arguments.unwrap_or_default());

        match request.name.as_ref() {
            "get_graph" => self.tool_get_graph().await,
            "get_campaign_state" => self.tool_get_campaign_state().await,
            "get_entity" => self.tool_get_entity(&args).await,
            "get_attack_surface" => self.tool_get_attack_surface(&args).await,
            "resolve_workload" => self.tool_resolve_workload(&args).await,
            "get_attack_flow" => self.tool_get_attack_flow().await,
            "list_ttps" => self.tool_list_ttps(&args).await,
            "get_applicable_ttps" => self.tool_get_applicable_ttps(&args).await,
            "get_ttp_detail" => self.tool_get_ttp_detail(&args).await,
            "execute_action" => self.tool_execute_action(&args).await,
            "wait_for_result" => self.tool_wait_for_result(&args).await,
            "check_rbac_goal" => self.tool_check_rbac_goal(&args).await,
            "check_access_level" => self.tool_check_access_level(&args).await,
            "list_parse_audits" => self.tool_list_parse_audits().await,
            "add_parser" => self.tool_add_parser(&args).await,
            "reset_campaign" => self.tool_reset_campaign().await,
            "get_initial_access_candidates" => self.tool_get_initial_access_candidates(&args).await,
            other => Err(McpError::invalid_params(
                format!("unknown tool `{other}`"),
                None,
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Axum router factory
// ---------------------------------------------------------------------------

/// Create an axum `Router` that mounts the MCP server at `/mcp`.
///
/// Works with `Router::merge`:
/// ```rust,ignore
/// let app = api::router_with_sse(state.clone())
///     .merge(api::mcp_router(state, mcp_config));
/// ```
pub fn mcp_router<S: ApiService + 'static>(service: S, config: McpConfig) -> axum::Router {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };
    use std::sync::Arc;

    let shared_config = Arc::new(config);

    let session_manager = Arc::new(LocalSessionManager::default());
    let mcp_service = StreamableHttpService::new(
        move || Ok(RanMcpHandler::new(service.clone(), shared_config.clone())),
        session_manager,
        StreamableHttpServerConfig::default(),
    );

    axum::Router::new().route_service("/mcp", mcp_service)
}
