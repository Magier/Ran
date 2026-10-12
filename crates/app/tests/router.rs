//! Guard the production router composition: frontend assets are compressed,
//! API and SSE responses are not.
//! Run after `pnpm --prefix frontend build`: cargo test -p app --release --test router
#![cfg(not(debug_assertions))]

use std::sync::{Arc, RwLock};

/// A real `AppState` with no Kubernetes client. Nothing here contacts a cluster.
fn state() -> (app::AppState, campaign::CampaignEventBus) {
    let cluster = ran_domain::K8sCluster::new("test".to_string());
    let campaign = Arc::new(RwLock::new(campaign::Campaign::bootstrap(
        "Test",
        cluster.clone(),
    )));
    let (c2_handle, _c2_events, _c2_manager) =
        c2::C2Manager::new(32, None, std::collections::HashMap::new());
    let campaign_events = campaign::CampaignEventBus::new(32);
    let state = app::AppState::new(
        None,
        campaign,
        c2_handle,
        armory::Armory::from_ttps(Vec::new()),
        app::config::NamespaceFilter::default(),
        utility_ai::Profile::default(),
        utility_ai::Profile::default(),
        None,
        false,
        false,
        "Test".to_string(),
        campaign::InitialKnowledge {
            clusters: vec![campaign::InitialClusterKnowledge {
                cluster,
                provenance: std::collections::BTreeSet::from([
                    campaign::KnowledgeProvenance::Operator,
                ]),
            }],
            ..Default::default()
        },
        campaign_events.clone(),
        std::path::PathBuf::from("plans"),
        kubetier::Catalog::embedded(),
    );
    (state, campaign_events)
}

#[tokio::test]
async fn only_frontend_assets_are_compressed() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (state, campaign_events) = state();
    let router = app::router(
        state,
        api::McpConfig {
            campaign_events,
            parsers_dir: None,
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    // Keep Content-Encoding visible even when workspace feature unification
    // turns on reqwest's gzip/brotli decoding.
    let client = reqwest::Client::builder()
        .no_gzip()
        .no_brotli()
        .build()
        .unwrap();
    let get = |path: &str| {
        client
            .get(format!("{origin}{path}"))
            .header("Accept-Encoding", "br, gzip")
            .send()
    };

    let frontend = get("/").await.unwrap();
    assert_eq!(frontend.status(), 200);
    assert_eq!(frontend.headers()["content-encoding"], "br");

    let api = get("/api/ui-config").await.unwrap();
    assert_eq!(api.status(), 200);
    assert_eq!(api.headers()["content-type"], "application/json");
    assert!(api.headers().get("content-encoding").is_none());
    assert!(api.headers().get("cache-control").is_none());

    let sse = get("/events").await.unwrap();
    assert_eq!(sse.status(), 200);
    assert!(sse.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
    assert!(sse.headers().get("content-encoding").is_none());

    server.abort();
}
