use std::collections::HashMap;

use axum::extract::{Query, State};
use serde_json::Value;

use crate::state_conversions::{campaign_to_campaign_state, campaign_to_graph};
use crate::sse::events_handler;
use crate::{ApiError, ApiService, CampaignState, ErrorResponse, GetArmoryParams, Graph};

#[cfg(debug_assertions)]
use axum::{
    body::to_bytes,
    http::{header::HOST, HeaderMap},
};

#[cfg(debug_assertions)]
use axum::{
    body::Body,
    response::IntoResponse,
};

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
    let all_ttps = service
        .get_armory(GetArmoryParams { tactic: None })
        .await?;

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
    let target_kind = campaign
        .get_entities()
        .into_iter()
        .find(|entity| entity.entity_id().0 == target_id)
        .map(|entity| entity.entity_kind().to_string())
        .ok_or_else(|| ApiError {
            status: axum::http::StatusCode::NOT_FOUND,
            body: ErrorResponse {
                error: format!("failed to get target entity: {}", target_id),
                details: None,
            },
        })?;

    let ttps = all_ttps
        .into_iter()
        .filter(|ttp| ttp_is_applicable_for_target_kind(ttp, &target_kind))
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

pub(crate) async fn campaign_state_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<CampaignState>, ApiError> {
    let campaign = service.get_campaign().await?;
    let state = campaign_to_campaign_state(&campaign);
    Ok(axum::Json(state))
}

fn ttp_is_applicable_for_target_kind(ttp: &armory::Ttp, target_kind: &str) -> bool {
    if ttp.status.eq_ignore_ascii_case("disabled") {
        return false;
    }

    // TODO(migration): extend applicability with RBAC/access-level/entitlement checks,
    // matching legacy Go Requires.Satisfied behavior.

    let Some(kind_req) = ttp.requires.get("kind") else {
        return true;
    };

    match kind_req {
        Value::String(kind) => kind.eq_ignore_ascii_case(target_kind),
        Value::Array(kinds) => kinds.iter().any(|k| {
            k.as_str()
                .map(|s| s.eq_ignore_ascii_case(target_kind))
                .unwrap_or(false)
        }),
        _ => true,
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
