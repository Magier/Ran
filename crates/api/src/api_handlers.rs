use std::collections::HashMap;

use axum::extract::{Query, State};
use serde_json::Value;
use tracing::debug;

use campaign::ttp_applicability::{ttp_exists_satisfied, ttp_rbac_satisfied};
use campaign::CampaignEntityRef;

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

    // Resolve the target entity – we need both its kind string (for exact-kind
    // matching) and whether it is a SystemEntity (for abstract "System" matching).
    let (target_kind, is_system_target) = {
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
        let is_system = match &entity {
            CampaignEntityRef::Pod(_) | CampaignEntityRef::Node(_) => true,
            _ => false,
        };
        (kind, is_system)
    };

    let ttps = all_ttps
        .into_iter()
        .filter(|ttp| {
            ttp_is_applicable_for_target_kind(ttp, &target_kind, is_system_target)
                && ttp_rbac_satisfied(ttp, &campaign)
                && ttp_exists_satisfied(ttp, &campaign)
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

pub(crate) async fn campaign_state_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<CampaignState>, ApiError> {
    let campaign = service.get_campaign().await?;
    let state = campaign_to_campaign_state(&campaign);
    Ok(axum::Json(state))
}

pub(crate) async fn start_pod_watch_handler(
    query: Query<HashMap<String, String>>,
) -> axum::Json<PodWatchStatus> {
    let namespace = query
        .0
        .get("namespace")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("*");

    debug!(namespace, "received StartPodWatch request (compat no-op)");

    axum::Json(PodWatchStatus {
        status: "pod watch start accepted (compat no-op)".to_string(),
    })
}

pub(crate) async fn stop_pod_watch_handler() -> axum::Json<PodWatchStatus> {
    debug!("received StopPodWatch request (compat no-op)");

    axum::Json(PodWatchStatus {
        status: "pod watch stop accepted (compat no-op)".to_string(),
    })
}

fn ttp_is_applicable_for_target_kind(
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
