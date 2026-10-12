//! Exercise the production embedded assets over HTTP, not the debug Vite proxy.
//! Run after `pnpm --prefix frontend build`: cargo test -p api --release --test frontend_assets
#![cfg(not(debug_assertions))]

use reqwest::Client;

async fn server() -> (String, tokio::task::JoinHandle<()>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, api::frontend_router()).await.unwrap();
    });
    (origin, task)
}

fn graph_asset() -> String {
    let nodes = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../frontend/build/_app/immutable/nodes");
    let name = std::fs::read_dir(nodes)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .find(|name| name.starts_with("2.") && name.ends_with(".js"))
        .expect("build the frontend before testing embedded assets");
    format!("/_app/immutable/nodes/{name}")
}

#[tokio::test]
async fn compressed_assets_round_trip_and_negotiate() {
    let (origin, task) = server().await;
    let path = graph_asset();
    let raw = Client::builder().no_gzip().no_brotli().build().unwrap();
    let identity = raw
        .get(format!("{origin}{path}"))
        .header("Accept-Encoding", "identity")
        .send()
        .await
        .unwrap();
    assert_eq!(identity.status(), 200);
    assert!(identity.headers().get("content-encoding").is_none());
    assert_eq!(
        identity.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    let original = identity.bytes().await.unwrap();
    for encoding in ["gzip", "br"] {
        let compressed = raw
            .get(format!("{origin}{path}"))
            .header("Accept-Encoding", encoding)
            .send()
            .await
            .unwrap();
        assert_eq!(compressed.headers()["content-encoding"], encoding);
        assert!(compressed.headers()["vary"]
            .to_str()
            .unwrap()
            .contains("accept-encoding"));
        let bytes = compressed.bytes().await.unwrap();
        assert!(bytes.len() < original.len() / 2);
        // reqwest decompresses the real HTTP response, checking byte-for-byte integrity.
        let decoded = Client::new()
            .get(format!("{origin}{path}"))
            .header("Accept-Encoding", encoding)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(decoded, original);
        println!(
            "{path}: identity={} {encoding}={}",
            original.len(),
            bytes.len()
        );
    }
    let refused = raw
        .get(format!("{origin}{path}"))
        .header("Accept-Encoding", "gzip;q=0, br;q=0, identity;q=1")
        .send()
        .await
        .unwrap();
    assert!(refused.headers().get("content-encoding").is_none());
    let preferred = raw
        .get(format!("{origin}{path}"))
        .header("Accept-Encoding", "gzip;q=1, br;q=0.5")
        .send()
        .await
        .unwrap();
    assert_eq!(preferred.headers()["content-encoding"], "gzip");
    task.abort();
}

#[tokio::test]
async fn html_metadata_and_missing_assets_are_not_immutable() {
    let (origin, task) = server().await;
    let client = Client::new();
    for path in ["/", "/unknown-route", "/_app/version.json"] {
        let response = client.get(format!("{origin}{path}")).send().await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "no-cache");
    }
    let missing = client
        .get(format!("{origin}/_app/immutable/missing.js"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    assert!(!missing
        .headers()
        .get("cache-control")
        .is_some_and(|h| h.to_str().unwrap().contains("immutable")));
    task.abort();
}

/// A real Rust static server for browser/resource-timing probes, with no campaign API.
#[tokio::test]
#[ignore]
async fn serve_static_assets() {
    let port = std::env::var("RAN_ASSET_PROBE_PORT").unwrap_or_else(|_| "4178".into());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    println!("Rust embedded-asset probe listening on http://127.0.0.1:{port}");
    axum::serve(listener, api::frontend_router()).await.unwrap();
}
