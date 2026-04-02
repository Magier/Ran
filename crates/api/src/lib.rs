use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::response::sse::{Event, Sse};
use campaign::{Campaign, CampaignEntityRef};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::OnceLock;
use std::{convert::Infallible, time::Duration};
use tokio::sync::broadcast;

#[cfg(not(debug_assertions))]
use axum::http::Uri;

use async_stream::stream;

#[cfg(debug_assertions)]
use axum::{
    http::{header::HOST, HeaderMap},
};

#[cfg(debug_assertions)]
use reqwest::Client;

#[cfg(not(debug_assertions))]
use axum::{
    http::{header, StatusCode},
    response::Response,
};

#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;

include!(concat!(env!("OUT_DIR"), "/openapi_generated.rs"));

#[derive(Debug, Clone, serde::Deserialize)]
struct GetApplicableTtpsParams {
    #[serde(rename = "targetId")]
    target_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ExecuteActionCmdPayload {
    #[serde(rename = "actionId")]
    action_id: String,
    #[serde(rename = "execSystemId")]
    exec_system_id: Option<String>,
    #[serde(rename = "targetId")]
    target_id: String,
    #[serde(rename = "procedureId")]
    procedure_id: Option<String>,
    args: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ExecuteActionAck {
    success: bool,
    queued: bool,
    #[serde(rename = "cmdId")]
    cmd_id: String,
}

#[derive(Debug, Clone)]
struct SseEnvelope {
    event: String,
    data: String,
}

static SSE_EVENT_BUS: OnceLock<broadcast::Sender<SseEnvelope>> = OnceLock::new();

fn sse_event_bus() -> &'static broadcast::Sender<SseEnvelope> {
    SSE_EVENT_BUS.get_or_init(|| {
        let (tx, _rx) = broadcast::channel(256);
        tx
    })
}

pub fn publish_sse_event(event: impl Into<String>, data: impl Into<String>) {
    let _ = sse_event_bus().send(SseEnvelope {
        event: event.into(),
        data: data.into(),
    });
}

pub fn router_with_sse<S: ApiService>(service: S) -> axum::Router {
    axum::Router::new()
        .route("/events", axum::routing::get(events_sse_handler::<S>))
        .route("/api/graph", axum::routing::get(graph_handler::<S>))
        .route("/api/armory", axum::routing::get(armory_handler::<S>))
        .route("/api/applicable-ttps", axum::routing::get(applicable_ttps_handler::<S>))
        .route("/api/action/execute", axum::routing::post(execute_action_handler::<S>))
        .route("/api/campaign-state", axum::routing::get(campaign_state_handler::<S>))
        .with_state(service.clone())
        .merge(router(service))
}

async fn events_sse_handler<S: ApiService>(
    State(service): State<S>,
) -> impl axum::response::IntoResponse {
    let armory = service
        .get_armory(GetArmoryParams { tactic: None })
        .await
        .unwrap_or_default();
    events_handler(armory).await
}

async fn graph_handler<S: ApiService>(
    State(service): State<S>,
) -> Result<axum::Json<Graph>, ApiError> {
    let campaign = service.get_campaign().await?;
    let graph = campaign_to_graph(&campaign);
    Ok(axum::Json(graph))
}

async fn armory_handler<S: ApiService>(
    State(service): State<S>,
    Query(params): Query<GetArmoryParams>,
) -> Result<axum::Json<Vec<armory::Ttp>>, ApiError> {
    let ttps = service.get_armory(params).await?;
    Ok(axum::Json(ttps))
}

async fn applicable_ttps_handler<S: ApiService>(
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

async fn execute_action_handler<S: ApiService>(
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

async fn campaign_state_handler<S: ApiService>(
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

fn campaign_to_campaign_state(campaign: &Campaign) -> CampaignState {
    let mut entities = HashMap::new();

    for entity in campaign.get_entities() {
        let id = entity.entity_id().0;
        let mut data = HashMap::new();
        data.insert("id".to_string(), Value::String(id.clone()));
        data.insert(
            "name".to_string(),
            Value::String(entity.entity_name().to_string()),
        );
        data.insert(
            "kind".to_string(),
            Value::String(entity.entity_kind().to_string()),
        );
        if let Some(namespace) = entity.namespace() {
            data.insert("namespace".to_string(), Value::String(namespace.to_string()));
        }
        entities.insert(id, data);
    }

    CampaignState {
        entities,
        relations: campaign
            .get_relations()
            .iter()
            .map(|r| {
                let mut m = HashMap::new();
                m.insert(
                    "id".to_string(),
                    Value::String(format!("rel/{}/{}->{}", r.name, r.source_id, r.target_id)),
                );
                m.insert("name".to_string(), Value::String(r.name.clone()));
                m.insert("sourceId".to_string(), Value::String(r.source_id.clone()));
                m.insert("targetId".to_string(), Value::String(r.target_id.clone()));
                m
            })
            .collect(),
    }
}

fn campaign_to_graph(campaign: &Campaign) -> Graph {
    let entities = campaign.get_entities();
    let namespace_ids: HashSet<String> = entities
        .iter()
        .filter(|e| e.entity_kind() == "Namespace")
        .map(|e| e.entity_id().0)
        .collect();

    let root_node_id = entities
        .iter()
        .find(|e| e.entity_kind() == "C2")
        .map(|e| e.entity_id().0)
        .unwrap_or_default();

    // Single pass over relations: hierarchical ones become compound-node parent
    // pointers (not edges); everything else becomes a GraphEdge.
    // "manages-node" / "owns" always override (high priority);
    // "contains" only fills in if no parent has been set yet (low priority).
    let mut parent_nodes: HashMap<String, String> = HashMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    for r in campaign.get_relations() {
        match r.name.as_str() {
            "manages-node" | "owns" => {
                parent_nodes.insert(r.target_id.clone(), r.source_id.clone());
            }
            "contains" => {
                parent_nodes
                    .entry(r.target_id.clone())
                    .or_insert_with(|| r.source_id.clone());
            }
            _ => {
                edges.push(GraphEdge {
                    id: format!("{}-[{}]->{}", r.source_id, r.name, r.target_id),
                    source_id: r.source_id.clone(),
                    target_id: r.target_id.clone(),
                    name: r.name.clone(),
                    weight: None,
                    relation: None,
                });
            }
        }
    }

    let mut nodes = Vec::with_capacity(campaign.entity_count());

    for entity in entities {
        let id = entity.entity_id().0;
        let kind = entity.entity_kind().to_string();
        // Determine compound-node parent. Explicit relation-based parents take
        // precedence; namespaced resources fall back to their namespace node.
        // C2, Cluster, and Namespace nodes are top-level to avoid nested compound
        // nodes, which fcose cannot handle and will crash with invalid array length.
        let parent = if let Some(p) = parent_nodes.get(&id) {
            Some(p.clone())
        } else {
            match &entity {
                CampaignEntityRef::Pod(_) | CampaignEntityRef::ServiceAccount(_) => entity
                    .namespace()
                    .map(|ns| format!("ns/{}", ns))
                    .filter(|ns_id| namespace_ids.contains(ns_id)),
                _ => None,
            }
        };

        nodes.push(GraphNode {
            id: id.clone(),
            entity_id: id,
            kind,
            name: entity.entity_name().to_string(),
            parent,
            access_level: None,
            compromised: None,
            is_running: None,
            entity: serialize_campaign_entity_map(&entity),
        });
    }

    Graph { root_node_id, nodes, edges }
}

fn serialize_entity_map<T: serde::Serialize>(entity: &T) -> Option<HashMap<String, Value>> {
    match serde_json::to_value(entity).ok()? {
        Value::Object(map) => Some(map.into_iter().collect()),
        _ => None,
    }
}

fn serialize_campaign_entity_map(entity: &CampaignEntityRef<'_>) -> Option<HashMap<String, Value>> {
    match entity {
        CampaignEntityRef::C2Server(e) => serialize_entity_map(e),
        CampaignEntityRef::Cluster(e) => serialize_entity_map(e),
        CampaignEntityRef::Node(e) => serialize_entity_map(e),
        CampaignEntityRef::Namespace(e) => serialize_entity_map(e),
        CampaignEntityRef::Pod(e) => serialize_entity_map(e),
        CampaignEntityRef::ServiceAccount(e) => serialize_entity_map(e),
    }
}

async fn events_handler(armory: Vec<armory::Ttp>) -> impl axum::response::IntoResponse {
    let initial_payload = serde_json::json!({
        "type": "armory-loaded",
        "data": armory,
    })
    .to_string();

    let mut rx = sse_event_bus().subscribe();

    let event_stream = stream! {
        // Keep compatibility with frontend listener registration and message parser.
        yield Ok::<Event, Infallible>(
            Event::default().event("armory-loaded").data(initial_payload),
        );

        loop {
            tokio::select! {
                received = rx.recv() => {
                    match received {
                        Ok(msg) => {
                            yield Ok::<Event, Infallible>(
                                Event::default().event(msg.event).data(msg.data),
                            );
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(15)) => {
                    yield Ok::<Event, Infallible>(
                        Event::default().event("ping").data(r#"{"type":"ping","data":"keepalive"}"#),
                    );
                }
            }
        }
    };

    Sse::new(event_stream)
}

#[cfg(not(debug_assertions))]
#[derive(RustEmbed)]
#[folder = "../../frontend/build"]
struct StaticAssets;

#[cfg(debug_assertions)]
pub async fn frontend_handler(req: Request) -> impl axum::response::IntoResponse {
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
