use std::collections::HashMap;

use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use tracing::debug;

use campaign::ttp_applicability::{resolve_target_context, ttp_applicable_for_target};

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
pub(crate) struct GetRecommendationsParams {
    /// Optional: restrict recommendations to a single target entity.
    #[serde(rename = "targetId")]
    pub(crate) target_id: Option<String>,
    /// Optional: cap the number of ranked candidates returned.
    pub(crate) limit: Option<usize>,
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
    /// Optional free-text rationale for this step, recorded on the resulting
    /// execution record. Strongly encouraged when driving the campaign
    /// programmatically so the timeline explains itself.
    pub(crate) reasoning: Option<String>,
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

    // Resolve the target's facts once, then gate every candidate TTP through the
    // shared aggregate applicability check (campaign::ttp_applicability).
    let Some(tc) = resolve_target_context(&campaign, target_id) else {
        return Err(ApiError {
            status: axum::http::StatusCode::NOT_FOUND,
            body: ErrorResponse {
                error: format!("failed to get target entity: {}", target_id),
                details: None,
            },
        });
    };

    let ttps = all_ttps
        .into_iter()
        .filter(|ttp| ttp_applicable_for_target(ttp, &campaign, &tc))
        .collect::<Vec<_>>();

    Ok(axum::Json(ttps))
}

/// Rank applicable `(TTP × target)` actions by utility for the current campaign
/// state, using the default scoring profile. Advisory: the caller chooses what
/// (if anything) to execute. Each candidate carries a per-consideration
/// breakdown for explainability.
pub(crate) async fn recommendations_handler<S: ApiService>(
    State(service): State<S>,
    Query(params): Query<GetRecommendationsParams>,
) -> Result<axum::Json<Vec<campaign::ScoredCandidate>>, ApiError> {
    let all_ttps = service.get_armory(GetArmoryParams { tactic: None }).await?;
    let campaign = service.get_campaign().await?;

    let scorer = campaign::Scorer::with_defaults(service.scoring_profile());
    let mut ranked = scorer.rank(&campaign, &all_ttps);

    if let Some(target_id) = params
        .target_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        ranked.retain(|c| c.target_id == target_id);
    }

    if let Some(limit) = params.limit {
        ranked.truncate(limit);
    }

    Ok(axum::Json(ranked))
}

// ---------------------------------------------------------------------------
// Scoring profile (response-curve / weight tuning)
// ---------------------------------------------------------------------------

/// One consideration's tunable config, tagged with its name.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct NamedConsideration {
    pub name: String,
    pub weight: f32,
    pub curve: campaign::ResponseCurve,
    pub enabled: bool,
    pub veto: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ScoringProfileResponse {
    pub combination: campaign::CombinationMode,
    #[serde(rename = "tuningEnabled")]
    pub tuning_enabled: bool,
    pub considerations: Vec<NamedConsideration>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ScoringProfileUpdate {
    pub combination: campaign::CombinationMode,
    pub considerations: Vec<NamedConsideration>,
}

fn profile_to_response(
    profile: &campaign::Profile,
    tuning_enabled: bool,
) -> ScoringProfileResponse {
    let considerations = campaign::consideration_names()
        .into_iter()
        .map(|name| {
            let cfg = profile.config(name);
            NamedConsideration {
                name: name.to_string(),
                weight: cfg.weight,
                curve: cfg.curve,
                enabled: cfg.enabled,
                veto: cfg.veto,
            }
        })
        .collect();
    ScoringProfileResponse {
        combination: profile.combination,
        tuning_enabled,
        considerations,
    }
}

/// Return the live scoring profile: combination mode, the tuning feature flag,
/// and every registered consideration's current weight/curve/enabled/veto.
pub(crate) async fn get_scoring_profile_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<ScoringProfileResponse>, ApiError> {
    let profile = service.scoring_profile();
    Ok(axum::Json(profile_to_response(
        &profile,
        service.scoring_tuning_enabled(),
    )))
}

/// Replace the live scoring profile. Gated on the tuning feature flag. Returns
/// the resulting profile so the client can confirm what was applied.
pub(crate) async fn update_scoring_profile_handler<S: ApiService>(
    State(service): State<S>,
    axum::Json(update): axum::Json<ScoringProfileUpdate>,
) -> Result<axum::Json<ScoringProfileResponse>, ApiError> {
    require_tuning(&service)?;

    let considerations = update
        .considerations
        .into_iter()
        .map(|c| {
            (
                c.name,
                campaign::ConsiderationConfig {
                    weight: c.weight,
                    curve: c.curve,
                    enabled: c.enabled,
                    veto: c.veto,
                },
            )
        })
        .collect();

    let profile = campaign::Profile {
        name: "tuned".to_string(),
        combination: update.combination,
        considerations,
    };
    service.set_scoring_profile(profile.clone());

    Ok(axum::Json(profile_to_response(&profile, true)))
}

fn require_tuning<S: ApiService>(service: &S) -> Result<(), ApiError> {
    if service.scoring_tuning_enabled() {
        Ok(())
    } else {
        Err(ApiError {
            status: axum::http::StatusCode::FORBIDDEN,
            body: ErrorResponse {
                error: "scoring tuning is disabled (set scoring.tuning_ui: true in ran.yaml)"
                    .to_string(),
                details: None,
            },
        })
    }
}

/// Persist the live scoring profile to its sidecar file so it survives restarts.
pub(crate) async fn save_scoring_profile_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<ScoringProfileResponse>, ApiError> {
    require_tuning(&service)?;
    service.save_scoring_profile().map_err(ApiError::internal)?;
    Ok(axum::Json(profile_to_response(
        &service.scoring_profile(),
        true,
    )))
}

/// Revert the live scoring profile to the configured base and drop persisted
/// overrides.
pub(crate) async fn reset_scoring_profile_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<ScoringProfileResponse>, ApiError> {
    require_tuning(&service)?;
    let profile = service.reset_scoring_profile();
    Ok(axum::Json(profile_to_response(&profile, true)))
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
            reasoning: cmd.reasoning,
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

/// One segment of a multi-hop command traversal, surfaced to the timeline UI.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AttackStepHop {
    #[serde(rename = "fromId")]
    pub from_id: String,
    #[serde(rename = "toId")]
    pub to_id: String,
    pub relation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope: Option<String>,
    pub command: String,
}

impl From<&campaign::TraversalHop> for AttackStepHop {
    fn from(h: &campaign::TraversalHop) -> Self {
        Self {
            from_id: h.from_id.clone(),
            to_id: h.to_id.clone(),
            relation: h.relation.clone(),
            envelope: h.envelope.clone(),
            command: h.command.clone(),
        }
    }
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
    /// Per-hop traversal breakdown for multi-system commands, outermost (C2) →
    /// innermost (target). Empty for direct/single-hop commands.
    pub traversal: Vec<AttackStepHop>,
    /// The bare inner command as it runs on the final target, before envelopes.
    /// Empty when there is no multi-hop traversal.
    #[serde(rename = "innerCommand")]
    pub inner_command: String,
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
            // Traversal is joined separately from the campaign side map by id.
            traversal: Vec::new(),
            inner_command: String::new(),
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
            // Traversal is joined separately from the campaign side map by id.
            traversal: Vec::new(),
            inner_command: String::new(),
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

    // Join the multi-hop traversal breakdown (campaign side map, keyed by
    // command id) onto each step — kept off the execution record itself.
    for step in &mut steps {
        if let Some(ct) = campaign.command_traversal(&step.id) {
            step.traversal = ct.hops.iter().map(AttackStepHop::from).collect();
            step.inner_command = ct.inner_command.clone();
        }
    }

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
