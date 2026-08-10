include!(concat!(env!("OUT_DIR"), "/openapi_generated.rs"));

mod api_handlers;
pub mod mcp;
mod sse;
mod state_conversions;

pub use api_handlers::frontend_handler;
pub use mcp::McpConfig;
pub use sse::publish_sse_event;

pub fn router_with_sse<S: ApiService>(service: S) -> axum::Router {
    axum::Router::new()
        .route(
            "/events",
            axum::routing::get(api_handlers::events_sse_handler::<S>),
        )
        .route(
            "/api/pods/watch",
            axum::routing::post(api_handlers::start_pod_watch_handler::<S>),
        )
        .route(
            "/api/pods/watch",
            axum::routing::delete(api_handlers::stop_pod_watch_handler::<S>),
        )
        .route(
            "/api/graph",
            axum::routing::get(api_handlers::graph_handler::<S>),
        )
        .route(
            "/api/armory",
            axum::routing::get(api_handlers::armory_handler::<S>),
        )
        .route(
            "/api/applicable-ttps",
            axum::routing::get(api_handlers::applicable_ttps_handler::<S>),
        )
        .route(
            "/api/eligible-auth-identities",
            axum::routing::get(api_handlers::eligible_auth_identities_handler::<S>),
        )
        .route(
            "/api/recommendations",
            axum::routing::get(api_handlers::recommendations_handler::<S>),
        )
        .route(
            "/api/scoring/profile",
            axum::routing::get(api_handlers::get_scoring_profile_handler::<S>)
                .put(api_handlers::update_scoring_profile_handler::<S>),
        )
        .route(
            "/api/scoring/profile/save",
            axum::routing::post(api_handlers::save_scoring_profile_handler::<S>),
        )
        .route(
            "/api/scoring/profile/reset",
            axum::routing::post(api_handlers::reset_scoring_profile_handler::<S>),
        )
        .route(
            "/api/scoring/calibrate",
            axum::routing::post(api_handlers::calibrate_scoring_handler::<S>),
        )
        .route(
            "/api/action/execute",
            axum::routing::post(api_handlers::execute_action_handler::<S>),
        )
        .route(
            "/api/campaign-state",
            axum::routing::get(api_handlers::campaign_state_handler::<S>),
        )
        .route(
            "/api/campaign/reset",
            axum::routing::post(api_handlers::reset_campaign_handler::<S>),
        )
        .route(
            "/api/flow",
            axum::routing::get(api_handlers::flow_handler::<S>),
        )
        .route(
            "/api/execution-records",
            axum::routing::get(api_handlers::execution_records_handler::<S>),
        )
        .route(
            "/api/execution-records/{id}",
            axum::routing::get(api_handlers::execution_record_by_id_handler::<S>),
        )
        .route(
            "/api/openapi.yaml",
            axum::routing::get(api_handlers::openapi_spec_handler),
        )
        .route(
            "/api/docs",
            axum::routing::get(api_handlers::swagger_ui_handler),
        )
        .route(
            "/api/files",
            axum::routing::get(api_handlers::file_content_handler::<S>),
        )
        .route(
            "/api/plans",
            axum::routing::post(api_handlers::execute_plan_handler::<S>),
        )
        .route(
            "/api/plans/available",
            axum::routing::get(api_handlers::list_plans_handler::<S>),
        )
        .route(
            "/api/plans/load",
            axum::routing::post(api_handlers::load_plan_handler::<S>),
        )
        .route(
            "/api/plans/export",
            axum::routing::get(api_handlers::export_plan_handler::<S>),
        )
        .route(
            "/api/plans/{plan_id}",
            axum::routing::get(api_handlers::plan_status_handler::<S>),
        )
        .with_state(service.clone())
        .merge(router(service))
}

pub fn router_with_sse_and_mcp<S: ApiService + 'static>(
    service: S,
    mcp_config: McpConfig,
) -> axum::Router {
    router_with_sse(service.clone()).merge(mcp::mcp_router(service, mcp_config))
}
