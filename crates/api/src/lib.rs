include!(concat!(env!("OUT_DIR"), "/openapi_generated.rs"));

mod state_conversions;
mod api_handlers;
mod sse;

pub use api_handlers::frontend_handler;
pub use sse::publish_sse_event;

pub fn router_with_sse<S: ApiService>(service: S) -> axum::Router {
    axum::Router::new()
        .route("/events", axum::routing::get(api_handlers::events_sse_handler::<S>))
        .route("/api/graph", axum::routing::get(api_handlers::graph_handler::<S>))
        .route("/api/armory", axum::routing::get(api_handlers::armory_handler::<S>))
        .route("/api/applicable-ttps", axum::routing::get(api_handlers::applicable_ttps_handler::<S>))
        .route("/api/action/execute", axum::routing::post(api_handlers::execute_action_handler::<S>))
        .route("/api/campaign-state", axum::routing::get(api_handlers::campaign_state_handler::<S>))
        .with_state(service.clone())
        .merge(router(service))
}
