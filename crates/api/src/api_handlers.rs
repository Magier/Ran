use std::collections::HashMap;

use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::debug;

use campaign::ttp_applicability::{
    ttp_access_level_satisfied, ttp_exists_satisfied, ttp_has_token_satisfied, ttp_rbac_satisfied,
    ttp_related_satisfied,
};
use campaign::CampaignEntityRef;
use ran_domain::AccessLevel;

use crate::sse::events_handler;
use crate::state_conversions::{campaign_to_campaign_state, campaign_to_graph};
use crate::{ApiError, ApiService, CampaignState, ErrorResponse, GetArmoryParams, Graph};

#[cfg(debug_assertions)]
use axum::{
    body::to_bytes,
    http::{header::HOST, HeaderMap},
};

#[cfg(debug_assertions)]
use axum::{body::Body, response::IntoResponse};

#[cfg(debug_assertions)]
use reqwest::Client;

#[cfg(not(debug_assertions))]
use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};

#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;

// ---------------------------------------------------------------------------
// OpenAPI spec + Swagger UI (no State needed — purely static content)
// ---------------------------------------------------------------------------

const OPENAPI_SPEC: &str = include_str!("../../../api/openapi.yaml");

const SWAGGER_UI_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Ran API Documentation</title>
  <link rel="stylesheet" type="text/css" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css" />
  <style>
    html { box-sizing: border-box; overflow: -moz-scrollbars-vertical; overflow-y: scroll; }
    *, *:before, *:after { box-sizing: inherit; }
    body { margin: 0; padding: 0; }
  </style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-standalone-preset.js"></script>
  <script>
    window.onload = function() {
      window.ui = SwaggerUIBundle({
        url: "/api/openapi.yaml",
        dom_id: '#swagger-ui',
        deepLinking: true,
        presets: [
          SwaggerUIBundle.presets.apis,
          SwaggerUIStandalonePreset
        ],
        plugins: [
          SwaggerUIBundle.plugins.DownloadUrl
        ],
        layout: "StandaloneLayout"
      });
    };
  </script>
</body>
</html>"#;

pub(crate) async fn openapi_spec_handler() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/yaml")],
        OPENAPI_SPEC,
    )
}

pub(crate) async fn swagger_ui_handler() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        SWAGGER_UI_HTML,
    )
}

// --- Request / response types -----------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct GetApplicableTtpsParams {
    #[serde(rename = "targetId")]
    pub(crate) target_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ExecuteActionCmdPayload {
    #[serde(rename = "actionId")]
    pub(crate) action_id: String,
    #[serde(rename = "execSystemId")]
    pub(crate) exec_system_id: Option<String>,
    #[serde(rename = "targetId")]
    pub(crate) target_id: String,
    #[serde(rename = "procedureId")]
    pub(crate) procedure_id: Option<String>,
    pub(crate) args: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ExecuteActionAck {
    success: bool,
    queued: bool,
    #[serde(rename = "cmdId")]
    cmd_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PodWatchStatus {
    status: String,
}

// --- Handlers ----------------------------------------------------------------

pub(crate) async fn events_sse_handler<S: ApiService>(
    State(service): State<S>,
) -> impl axum::response::IntoResponse {
    let armory = service
        .get_armory(GetArmoryParams { tactic: None })
        .await
        .unwrap_or_default();
    events_handler(armory).await
}

pub(crate) async fn graph_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<Graph>, ApiError> {
    let campaign = service.get_campaign().await?;
    let graph = campaign_to_graph(&campaign);
    Ok(axum::Json(graph))
}

pub(crate) async fn armory_handler<S: ApiService>(
    State(service): State<S>,
    Query(params): Query<GetArmoryParams>,
) -> Result<axum::Json<Vec<armory::Ttp>>, ApiError> {
    let ttps = service.get_armory(params).await?;
    Ok(axum::Json(ttps))
}

pub(crate) async fn applicable_ttps_handler<S: ApiService>(
    State(service): State<S>,
    Query(params): Query<GetApplicableTtpsParams>,
) -> Result<axum::Json<Vec<armory::Ttp>>, ApiError> {
    let all_ttps = service.get_armory(GetArmoryParams { tactic: None }).await?;

    let target_id = params
        .target_id
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();

    if target_id.is_empty() {
        let ttps = all_ttps
            .into_iter()
            .filter(|ttp| !ttp.status.eq_ignore_ascii_case("disabled"))
            .collect::<Vec<_>>();
        return Ok(axum::Json(ttps));
    }

    let campaign = service.get_campaign().await?;

    // Resolve the target entity – we need its kind string (for exact-kind
    // matching), whether it is a SystemEntity (for abstract "System" matching),
    // and its current access level (for the access-level pre-condition check).
    let (target_kind, is_system_target, target_access_level, target_has_token) = {
        let entities = campaign.get_entities();
        let entity = entities
            .into_iter()
            .find(|e| e.entity_id().0 == target_id)
            .ok_or_else(|| ApiError {
                status: axum::http::StatusCode::NOT_FOUND,
                body: ErrorResponse {
                    error: format!("failed to get target entity: {}", target_id),
                    details: None,
                },
            })?;
        let kind = entity.entity_kind().to_string();
        let is_system = matches!(
            &entity,
            CampaignEntityRef::Pod(_)
                | CampaignEntityRef::Node(_)
                | CampaignEntityRef::UnknownSystem(_)
        );
        let access_level = match &entity {
            CampaignEntityRef::Pod(p) => {
                // A reachable pod (kubectl-exec channel exists) implies exec access
                // even before a TTP has explicitly updated the access_level field.
                if p.system.access_level == AccessLevel::None
                    && campaign.reachable_pods().contains(&entity.entity_id().0)
                {
                    AccessLevel::Exec
                } else {
                    p.system.access_level
                }
            }
            CampaignEntityRef::Node(n) => n.system.access_level,
            CampaignEntityRef::UnknownSystem(s) => s.system.access_level,
            _ => AccessLevel::None,
        };
        let has_token = match &entity {
            CampaignEntityRef::ServiceAccount(sa) => sa.raw_token().is_some(),
            _ => false,
        };
        (kind, is_system, access_level, has_token)
    };

    let ttps = all_ttps
        .into_iter()
        .filter(|ttp| {
            ttp_is_applicable_for_target_kind(ttp, &target_kind, is_system_target)
                && ttp_rbac_satisfied(ttp, &campaign)
                && ttp_exists_satisfied(ttp, &campaign)
                && (!is_system_target || ttp_access_level_satisfied(ttp, target_access_level))
                && ttp_has_token_satisfied(ttp, target_has_token)
                && ttp_related_satisfied(ttp, target_id, &target_kind, &campaign)
        })
        .collect::<Vec<_>>();

    Ok(axum::Json(ttps))
}

pub(crate) async fn execute_action_handler<S: ApiService>(
    State(service): State<S>,
    axum::Json(cmd): axum::Json<ExecuteActionCmdPayload>,
) -> Result<axum::Json<ExecuteActionAck>, ApiError> {
    let execution = service
        .execute_action(campaign::ExecuteActionRequest {
            action_id: cmd.action_id,
            exec_system_id: cmd.exec_system_id,
            target_id: cmd.target_id,
            procedure_id: cmd.procedure_id,
            args: cmd.args.unwrap_or_default(),
        })
        .await?;

    Ok(axum::Json(ExecuteActionAck {
        success: true,
        queued: true,
        cmd_id: execution.cmd_id,
    }))
}

// ---------------------------------------------------------------------------
// Execution records
// ---------------------------------------------------------------------------

/// A single execution record joined with the parse audits produced by its
/// effects.  Returned by `GET /api/execution-records` and
/// `GET /api/execution-records/:id`.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ExecutionRecordEntry {
    #[serde(flatten)]
    pub record: campaign::ExecutionRecord,
    #[serde(rename = "parseAudits")]
    pub parse_audits: Vec<campaign::ParseAudit>,
}

pub(crate) async fn execution_records_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<Vec<ExecutionRecordEntry>>, ApiError> {
    let campaign = service.get_campaign().await?;
    let audits = campaign.get_parse_audits();
    let entries = campaign
        .get_execution_records()
        .iter()
        .map(|record| {
            let parse_audits = audits
                .iter()
                .filter(|a| a.cmd_id == record.id)
                .cloned()
                .collect();
            ExecutionRecordEntry {
                record: record.clone(),
                parse_audits,
            }
        })
        .collect();
    Ok(axum::Json(entries))
}

pub(crate) async fn execution_record_by_id_handler<S: ApiService>(
    State(service): State<S>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<axum::Json<ExecutionRecordEntry>, ApiError> {
    let campaign = service.get_campaign().await?;
    let record = campaign
        .get_execution_records()
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or_else(|| ApiError {
            status: axum::http::StatusCode::NOT_FOUND,
            body: ErrorResponse {
                error: format!("execution record '{}' not found", id),
                details: None,
            },
        })?;
    let parse_audits = campaign
        .get_parse_audits()
        .iter()
        .filter(|a| a.cmd_id == id)
        .cloned()
        .collect();
    Ok(axum::Json(ExecutionRecordEntry {
        record,
        parse_audits,
    }))
}

pub(crate) async fn campaign_state_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<CampaignState>, ApiError> {
    let campaign = service.get_campaign().await?;
    let state = campaign_to_campaign_state(&campaign);
    Ok(axum::Json(state))
}

pub(crate) async fn reset_campaign_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::http::StatusCode, ApiError> {
    service.reset_campaign().await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct GetFileContentParams {
    pub(crate) path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct FileContentResponse {
    pub(crate) path: String,
    pub(crate) content: String,
}

pub(crate) async fn file_content_handler<S: ApiService>(
    State(service): State<S>,
    Query(params): Query<GetFileContentParams>,
) -> Result<axum::Json<FileContentResponse>, ApiError> {
    let campaign = service.get_campaign().await?;
    let content = campaign
        .get_file_content(&params.path)
        .ok_or_else(|| ApiError {
            status: axum::http::StatusCode::NOT_FOUND,
            body: ErrorResponse {
                error: format!("file content not found for path: {}", params.path),
                details: None,
            },
        })?
        .to_string();
    Ok(axum::Json(FileContentResponse {
        path: params.path,
        content,
    }))
}

fn ms_to_iso8601(ms: u64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms as i64)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct FlowEdge {
    pub id: String,
    #[serde(rename = "sourceId")]
    pub source_id: String,
    #[serde(rename = "targetId")]
    pub target_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AttackStepTTP {
    pub id: String,
    pub name: String,
    pub tactic: String,
    pub techniques: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AttackStep {
    pub id: String,
    #[serde(rename = "targetId")]
    pub target_id: String,
    pub command: String,
    pub args: std::collections::HashMap<String, String>,
    #[serde(rename = "procedureId")]
    pub procedure_id: String,
    #[serde(rename = "TTP")]
    pub ttp: AttackStepTTP,
    pub results: Vec<String>,
    pub success: bool,
    pub status: &'static str,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    #[serde(rename = "completedAt")]
    pub completed_at: String,
    #[serde(rename = "executedOn")]
    pub executed_on: String,
}

impl From<&campaign::ExecutionRecord> for AttackStep {
    fn from(r: &campaign::ExecutionRecord) -> Self {
        Self {
            id: r.id.clone(),
            target_id: r.target_id.clone(),
            command: r.command.clone(),
            args: r.args.clone(),
            procedure_id: r.procedure_id.clone(),
            ttp: AttackStepTTP {
                id: r.ttp_id.clone(),
                name: r.ttp_name.clone(),
                tactic: r.tactic.clone(),
                techniques: Vec::new(),
                description: String::new(),
            },
            results: {
                let mut results: Vec<String> = r
                    .results
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect();
                if results.is_empty() && !r.fail_reason.is_empty() {
                    results.push(r.fail_reason.clone());
                }
                results
            },
            success: r.success,
            status: if r.success { "Success" } else { "Failed" },
            started_at: ms_to_iso8601(r.started_at_ms),
            completed_at: ms_to_iso8601(r.completed_at_ms),
            executed_on: r.exec_system_id.clone(),
        }
    }
}

impl From<&campaign::ExecTtp> for AttackStep {
    fn from(exec: &campaign::ExecTtp) -> Self {
        Self {
            id: exec.id.clone(),
            target_id: exec.target_id.clone(),
            command: exec.procedure.command.clone(),
            args: exec.args.clone(),
            procedure_id: exec.procedure.id.clone(),
            ttp: AttackStepTTP {
                id: exec.ttp.id.clone(),
                name: exec.ttp.name.clone(),
                tactic: exec.ttp.tactic.clone(),
                techniques: Vec::new(),
                description: String::new(),
            },
            results: Vec::new(),
            success: false,
            status: "Ongoing",
            started_at: ms_to_iso8601(exec.started_at_ms),
            completed_at: String::new(),
            executed_on: exec.exec_entity().to_string(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AttackFlow {
    pub steps: Vec<AttackStep>,
    pub edges: Vec<FlowEdge>,
}

pub(crate) async fn flow_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<AttackFlow>, ApiError> {
    let campaign = service.get_campaign().await?;
    let records = campaign.get_execution_records();
    let open = campaign.get_open_steps();

    let mut steps: Vec<AttackStep> = records.iter().map(AttackStep::from).collect();
    steps.extend(open.iter().map(AttackStep::from));

    let mut edges = Vec::new();
    let mut last_success_id: Option<String> = None;

    for step in &steps {
        if let Some(ref src) = last_success_id {
            edges.push(FlowEdge {
                id: format!("{}->{}", src, step.id),
                source_id: src.clone(),
                target_id: step.id.clone(),
            });
        }

        if step.status == "Success" {
            last_success_id = Some(step.id.clone());
        }
    }

    Ok(axum::Json(AttackFlow { steps, edges }))
}

pub(crate) async fn start_pod_watch_handler<S: ApiService>(
    State(service): State<S>,
    query: Query<HashMap<String, String>>,
) -> Result<axum::Json<PodWatchStatus>, ApiError> {
    let namespace = query
        .0
        .get("namespace")
        .filter(|v| !v.trim().is_empty())
        .cloned();

    debug!(?namespace, "received StartPodWatch request");

    service.start_pod_watch(namespace).await?;

    Ok(axum::Json(PodWatchStatus {
        status: "watching".to_string(),
    }))
}

pub(crate) async fn stop_pod_watch_handler<S: ApiService>(
    State(service): State<S>,
) -> axum::Json<PodWatchStatus> {
    debug!("received StopPodWatch request");

    service.stop_pod_watch().await;

    axum::Json(PodWatchStatus {
        status: "stopped".to_string(),
    })
}

// ---------------------------------------------------------------------------
// Plan handlers
// ---------------------------------------------------------------------------

pub(crate) async fn execute_plan_handler<S: ApiService>(
    State(service): State<S>,
    body: String,
) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let plan_id = service.execute_plan(body).await?;
    Ok(axum::Json(serde_json::json!({ "plan_id": plan_id })))
}

pub(crate) async fn plan_status_handler<S: ApiService>(
    State(service): State<S>,
    axum::extract::Path(plan_id): axum::extract::Path<String>,
) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let status = service.get_plan_status(&plan_id).await?;
    Ok(axum::Json(status))
}

pub(crate) async fn export_plan_handler<S: ApiService>(
    State(service): State<S>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<String, ApiError> {
    let include_failed = params
        .get("include_failed")
        .map(|v| v == "true")
        .unwrap_or(false);
    service.export_plan(include_failed).await
}

pub(crate) fn ttp_is_applicable_for_target_kind(
    ttp: &armory::Ttp,
    target_kind: &str,
    is_system_target: bool,
) -> bool {
    if ttp.status.eq_ignore_ascii_case("disabled") {
        return false;
    }

    let Some(kind_req) = ttp.requires.get("kind") else {
        return true;
    };

    match kind_req {
        Value::String(kind) => kind_matches_target_kind(kind, target_kind, is_system_target),
        Value::Array(kinds) => kinds.iter().any(|k| {
            k.as_str()
                .map(|s| kind_matches_target_kind(s, target_kind, is_system_target))
                .unwrap_or(true)
        }),
        _ => true,
    }
}

/// Returns `true` if `required_kind` (from a TTP's `requires.kind`) is satisfied
/// by the target entity.
///
/// `required_kind == "System"` is an abstract requirement satisfied by any entity
/// that implements [`SystemEntity`] – i.e. wherever `is_system_target` is `true`.
/// This is driven by the trait rather than a hardcoded list of kind strings, so
/// future `SystemEntity` implementors (e.g. `UnknownSystem`) are picked up
/// automatically without touching this function.
fn kind_matches_target_kind(
    required_kind: &str,
    target_kind: &str,
    is_system_target: bool,
) -> bool {
    if required_kind.eq_ignore_ascii_case(target_kind) {
        return true;
    }

    required_kind.eq_ignore_ascii_case("System") && is_system_target
}

#[cfg(test)]
mod tests {
    use super::kind_matches_target_kind;

    #[test]
    fn system_kind_matches_any_system_entity_target() {
        // is_system_target=true represents anything implementing SystemEntity
        assert!(kind_matches_target_kind("System", "Pod", true));
        assert!(kind_matches_target_kind("System", "Node", true));
        // A hypothetical future type also matches as long as it is a SystemEntity
        assert!(kind_matches_target_kind("System", "UnknownSystem", true));
    }

    #[test]
    fn system_kind_does_not_match_non_system_entities() {
        assert!(!kind_matches_target_kind("System", "ServiceAccount", false));
        assert!(!kind_matches_target_kind("System", "Namespace", false));
    }

    #[test]
    fn exact_kind_matching_still_works() {
        assert!(kind_matches_target_kind("Pod", "Pod", false));
        assert!(!kind_matches_target_kind("Pod", "Node", false));
        // is_system_target flag is irrelevant for non-System requirements
        assert!(!kind_matches_target_kind("Pod", "Node", true));
    }
}

// --- Frontend handler -------------------------------------------------------

#[cfg(not(debug_assertions))]
#[derive(RustEmbed)]
#[folder = "../../frontend/build"]
struct StaticAssets;

#[cfg(debug_assertions)]
pub async fn frontend_handler(req: axum::extract::Request) -> impl axum::response::IntoResponse {
    const VITE_ORIGIN: &str = "http://localhost:5173";
    let client = Client::new();

    let (parts, body) = req.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let url = format!("{VITE_ORIGIN}{path_and_query}");

    let mut headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        if name == HOST {
            continue;
        }
        headers.append(name.clone(), value.clone());
    }

    let request_body = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_GATEWAY,
                "failed to read request body for Vite proxy",
            )
                .into_response();
        }
    };

    let upstream = client
        .request(parts.method, &url)
        .headers(headers)
        .body(request_body)
        .send()
        .await;

    match upstream {
        Ok(resp) => {
            let status = resp.status();
            let headers = resp.headers().clone();
            let body = resp.bytes().await.unwrap_or_default();

            let mut response = axum::response::Response::new(Body::from(body));
            *response.status_mut() = status;
            *response.headers_mut() = headers;
            response
        }
        Err(_) => (
            axum::http::StatusCode::BAD_GATEWAY,
            "Vite dev server not running; start it with `pnpm dev` in frontend/",
        )
            .into_response(),
    }
}

#[cfg(not(debug_assertions))]
pub async fn frontend_handler(uri: Uri) -> impl axum::response::IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match StaticAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => {
            if let Some(index) = StaticAssets::get("index.html") {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    index.data.into_owned(),
                )
                    .into_response()
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(axum::body::Body::from("frontend build assets not found"))
                    .unwrap_or_else(|_| StatusCode::NOT_FOUND.into_response())
            }
        }
    }
}
