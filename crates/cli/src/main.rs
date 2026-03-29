use std::{net::SocketAddr, path::PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::Result;
use armory::Armory;
use axum::Router;
use c2::{C2Handle, C2Manager};
use clap::{Parser, Subcommand};
use tokio::signal;
use tokio::sync::broadcast;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use api::{ApiError, ApiService, GetRunningPodsParams, K8sResource};
use campaign::{
    spawn_c2_event_processor, Campaign, CampaignEvent, CampaignEventBus, ExecuteActionError,
    ExecuteActionRequest, ExecuteActionResult,
};
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

    #[arg(long = "armory")]
    armory: Option<PathBuf>,

    #[arg(long = "namespace")]
    namespace: Option<String>,

    #[arg(short = 'p', long = "port", default_value_t = 8080)]
    port: u16,
}

#[derive(Clone)]
struct AppState {
    k8s: K8sService,
    campaign: Arc<RwLock<Campaign>>,
    c2: C2Handle,
    armory: Armory,
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
        let guard = self
            .campaign
            .read()
            .map_err(|_| ApiError::internal("campaign lock poisoned"))?;
        Ok(guard.clone())
    }

    async fn get_armory(&self, params: api::GetArmoryParams) -> Result<Vec<armory::Ttp>, ApiError> {
        Ok(self.armory.ttps_for_tactic(params.tactic.as_deref()))
    }

    async fn execute_action(&self, cmd: ExecuteActionRequest) -> Result<ExecuteActionResult, ApiError> {
        let action_id = cmd.action_id.clone();
        let target_id = cmd.target_id.clone();
        let exec_system_id = cmd.exec_system_id.clone().unwrap_or_default();
        let procedure_id = cmd.procedure_id.clone().unwrap_or_default();
        let arg_keys = cmd.args.keys().cloned().collect::<Vec<_>>();

        info!(
            action_id = %action_id,
            target_id = %target_id,
            exec_system_id = %exec_system_id,
            procedure_id = %procedure_id,
            arg_keys = ?arg_keys,
            "Executing action"
        );

        let exec = {
            let mut campaign = self
                .campaign
                .write()
                .map_err(|_| {
                    error!("campaign lock poisoned while executing action");
                    ApiError::internal("campaign lock poisoned")
                })?;

            campaign
                .prepare_action(cmd, &self.armory)
                .map_err(|err| {
                match err {
                    ExecuteActionError::InvalidInput(message) => {
                        error!("execute_action invalid input: {}", message);
                        ApiError {
                            status: axum::http::StatusCode::BAD_REQUEST,
                            body: api::ErrorResponse {
                                error: message,
                                details: None,
                            },
                        }
                    }
                    ExecuteActionError::NotFound(message) => {
                        error!("execute_action not found: {}", message);
                        ApiError {
                            status: axum::http::StatusCode::NOT_FOUND,
                            body: api::ErrorResponse {
                                error: message,
                                details: None,
                            },
                        }
                    }
                }
            })?
        };

        self.c2.send(exec.clone()).await.map_err(|message| {
            error!("failed to enqueue exec_ttp command: {}", message);
            ApiError::internal(message)
        })?;

        info!(
            cmd_id = %exec.id,
            action_id = %exec.ttp.id,
            target_id = %exec.target_id,
            "execute_action queued"
        );

        Ok(ExecuteActionResult {
            cmd_id: exec.id.clone(),
            event: campaign::ExecutedActionEvent {
                id: exec.id.clone(),
                cmd_id: exec.id,
                ttp: exec.ttp,
                args: exec.args,
                exec_system_id: exec.exec_system_id,
                success: true,
                fail_reason: String::new(),
            },
        })
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
    let armory_dir = resolve_armory_dir(args.armory)?;
    let armory = Armory::load_from_dir(&armory_dir)?;
    let campaign = Arc::new(RwLock::new(Campaign::bootstrap(
        "Ran",
        K8sCluster::new(target_cluster.name)
            .with_context_name(target_cluster.context_name)
            .with_server(target_cluster.server),
    )));

    let (c2_handle, c2_events, c2_manager) = C2Manager::new(256);
    let campaign_events = CampaignEventBus::new(256);

    tokio::spawn(c2_manager.run());
    spawn_c2_event_processor(campaign.clone(), c2_events, campaign_events.clone());
    tokio::spawn(bridge_campaign_events_to_sse(campaign_events.subscribe()));

    let state = AppState {
        k8s,
        campaign,
        c2: c2_handle,
        armory,
        default_namespace: args.namespace.clone(),
    };
    let campaign_entity_count = state
        .campaign
        .read()
        .map(|c| c.entity_count())
        .unwrap_or_default();
    let armory_count = state.armory.ttps().len();

    let app: Router = api::router_with_sse(state).fallback(api::frontend_handler);
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));

    info!("starting emulate API server");
    info!(kubeconfig = %kubeconfig_path.display(), "using kubeconfig");
    info!(armory_dir = %armory_dir.display(), armory_ttps = armory_count, "armory loaded");
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

fn resolve_armory_dir(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = arg {
        return Ok(path);
    }

    let cwd = std::env::current_dir()?;
    let default = cwd.join("armory").join("TTPs");
    Ok(default)
}

async fn bridge_campaign_events_to_sse(
    mut campaign_rx: broadcast::Receiver<CampaignEvent>,
) {
    loop {
        match campaign_rx.recv().await {
            Ok(CampaignEvent::TtpExecuted {
                cmd_id,
                exec_system_id,
                ttp,
                args,
                success,
                fail_reason,
                results,
                exit_code,
                ..
            }) => {
                let executed_payload = serde_json::json!({
                    "ID": cmd_id,
                    "CmdId": cmd_id,
                    "TTP": ttp,
                    "Args": args,
                    "ExecSystemID": exec_system_id,
                    "Success": success,
                    "FailReason": fail_reason,
                    "Results": results,
                    "ExitCode": exit_code,
                });

                api::publish_sse_event(
                    "ttp-executed",
                    serde_json::json!({
                        "type": "ttp-executed",
                        "data": executed_payload,
                    })
                    .to_string(),
                );
            }
            Ok(CampaignEvent::FactsChanged {
                new_entities,
                new_relations,
                ..
            }) => {
                api::publish_sse_event(
                    "facts-changed",
                    serde_json::json!({
                        "type": "facts-changed",
                        "data": {
                            "newEntities": new_entities,
                            "newRelations": new_relations,
                        },
                    })
                    .to_string(),
                );
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                error!(skipped, "campaign SSE bridge lagged behind campaign event bus");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}
