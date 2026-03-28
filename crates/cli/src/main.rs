use std::{net::SocketAddr, path::PathBuf};

use anyhow::Result;
use axum::Router;
use clap::{Parser, Subcommand};
use tokio::signal;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use api::{ApiError, ApiService, GetRunningPodsParams, K8sResource};
use campaign::Campaign;
use k8s::{kubeconfig_path_or_err, target_cluster_from_kubeconfig, K8sService};
use ran_domain::K8sCluster;

#[derive(Debug, Parser)]
#[command(name = "ran", about = "Ran CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Emulate(EmulateArgs),
}

#[derive(Debug, Clone, Parser)]
struct EmulateArgs {
    #[arg(long = "kubeconfig")]
    kubeconfig: Option<PathBuf>,

    #[arg(long = "namespace")]
    namespace: Option<String>,

    #[arg(short = 'p', long = "port", default_value_t = 8080)]
    port: u16,
}

#[derive(Clone)]
struct AppState {
    k8s: K8sService,
    campaign: Campaign,
    default_namespace: Option<String>,
}

// TODO: Temporary workaround for MVP wiring.
// Move this ApiService adapter out of CLI into a dedicated app/adapter layer
// once we split bootstrap concerns from API/data orchestration.
#[async_trait::async_trait]
impl ApiService for AppState {
    async fn get_running_pods(
        &self,
        params: GetRunningPodsParams,
    ) -> Result<Vec<K8sResource>, ApiError> {
        let ns = params
            .namespace
            .as_deref()
            .or(self.default_namespace.as_deref());

        let pods = self
            .k8s.get_running_pods(ns).await.map_err(|e| ApiError::internal(e.to_string()))?;

        Ok(pods
            .into_iter()
            .map(|p| K8sResource {
                id: p.id,
                name: p.name,
                namespace: p.namespace,
                kind: "pod".to_string(),
                phase: p.phase,
                ready: p.ready,
                state_reason: p.state_reason,
            })
            .collect())
    }

    async fn get_campaign(&self) -> Result<Campaign, ApiError> {
        Ok(self.campaign.clone())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    match cli.command {
        Commands::Emulate(args) => run_emulate(args).await,
    }
}

async fn run_emulate(args: EmulateArgs) -> Result<()> {
    let kubeconfig_path = kubeconfig_path_or_err(args.kubeconfig)?;
    let k8s = K8sService::from_kubeconfig(Some(kubeconfig_path.clone())).await?;
    let target_cluster = target_cluster_from_kubeconfig(Some(kubeconfig_path.clone()))?;
    let campaign = Campaign::bootstrap(
        "Ran",
        K8sCluster::new(target_cluster.name)
            .with_context_name(target_cluster.context_name)
            .with_server(target_cluster.server),
    );

    let state = AppState {
        k8s,
        campaign,
        default_namespace: args.namespace.clone(),
    };
    let campaign_entity_count = state.campaign.entity_count();

    let app: Router = api::router_with_sse(state).fallback(api::frontend_handler);
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));

    info!("starting emulate API server");
    info!(kubeconfig = %kubeconfig_path.display(), "using kubeconfig");
    info!(campaign_entities = campaign_entity_count, "campaign initialized");
    info!(%addr, namespace = ?args.namespace, "listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn init_tracing() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}

async fn shutdown_signal() {
    let _ = signal::ctrl_c().await;
    info!("received shutdown signal");
}
