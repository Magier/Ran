mod config;

use std::{net::SocketAddr, path::PathBuf, time::Duration};
use std::sync::{Arc, Mutex, RwLock};

use config::NamespaceFilter;

use anyhow::Result;
use armory::Armory;
use axum::Router;
use c2::{C2Handle, C2Manager};
use clap::{Parser, Subcommand};
use reqwest::Url;
use tokio::signal;
use tokio::sync::broadcast;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use api::{ApiError, ApiService, GetRunningPodsParams, K8sResource};
use campaign::{
    spawn_c2_event_processor_with_external_parser, Campaign, CampaignEvent, CampaignEventBus,
    ExecuteActionError, ExecuteActionRequest, ExecuteActionResult,
    ExternalParseRequest, ExternalParseResponse, ExternalParser,
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
    Armory(ArmoryArgs),
}

#[derive(Debug, Clone, Parser)]
#[command(about = "Show the contents of the armory")]
struct ArmoryArgs {
    /// Path to the armory TTPs directory (default: ./armory/TTPs).
    #[arg(long = "armory")]
    armory: Option<PathBuf>,
}

#[derive(Debug, Clone, Parser)]
struct EmulateArgs {
    #[arg(long = "kubeconfig")]
    kubeconfig: Option<PathBuf>,

    #[arg(long = "armory")]
    armory: Option<PathBuf>,

    /// Path to ran.yaml config file (default: ./ran.yaml).
    #[arg(long = "config")]
    config: Option<PathBuf>,

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
    namespace_filter: NamespaceFilter,
    ran_name: String,
    target_cluster: K8sCluster,
    campaign_events: CampaignEventBus,
    pod_watch: Arc<Mutex<Option<k8s::WatchHandle>>>,
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
        // A --namespace flag on the CLI scopes the listing to one namespace and
        // bypasses the config filter (it acts as an implicit whitelist of one).
        // Treat an empty string the same as absent — don't bypass the filter.
        let scope_ns = params.namespace.as_deref().filter(|ns| !ns.is_empty());

        let pods = self
            .k8s
            .get_running_pods(scope_ns)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;

        Ok(pods
            .into_iter()
            .filter(|p| {
                // When scoped to a single namespace the filter is already applied
                // by the k8s call above; skip filtering to avoid double-checking.
                if scope_ns.is_some() {
                    return true;
                }
                match p.namespace.as_deref() {
                    Some(ns) => self.namespace_filter.should_include(ns),
                    None => true,
                }
            })
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

    async fn reset_campaign(&self) -> Result<(), ApiError> {
        let mut campaign = self
            .campaign
            .write()
            .map_err(|_| ApiError::internal("campaign lock poisoned"))?;
        campaign.reset(self.ran_name.clone(), self.target_cluster.clone());
        let _ = self.campaign_events.publish(CampaignEvent::Reset);
        Ok(())
    }

    async fn get_armory(&self, params: api::GetArmoryParams) -> Result<Vec<armory::Ttp>, ApiError> {
        Ok(self.armory.ttps_for_tactic(params.tactic.as_deref()))
    }

    async fn start_pod_watch(&self, namespace: Option<String>) -> Result<(), ApiError> {
        let mut guard = self
            .pod_watch
            .lock()
            .map_err(|_| ApiError::internal("pod_watch lock poisoned"))?;

        // Drop any existing watch (WatchHandle::drop aborts the background task).
        *guard = None;

        let k8s = self.k8s.clone();
        let ns_filter = self.namespace_filter.clone();
        let scope_ns = namespace.as_deref().filter(|v| !v.is_empty()).map(String::from);

        let handle = k8s.watch_pods(namespace, move |pods| {
            let filtered: Vec<serde_json::Value> = pods
                .into_iter()
                .filter(|p| {
                    // When scoped to one namespace the k8s query already filtered it.
                    if scope_ns.is_some() {
                        return true;
                    }
                    match p.namespace.as_deref() {
                        Some(ns) => ns_filter.should_include(ns),
                        None => true,
                    }
                })
                .map(|p| {
                    serde_json::json!({
                        "id": p.id,
                        "name": p.name,
                        "namespace": p.namespace,
                        "kind": "pod",
                        "phase": p.phase,
                        "ready": p.ready,
                        "stateReason": p.state_reason,
                    })
                })
                .collect();

            api::publish_sse_event(
                "pods-changed",
                serde_json::json!({
                    "type": "pods-changed",
                    "data": { "pods": filtered },
                })
                .to_string(),
            );
        });

        *guard = Some(handle);
        Ok(())
    }

    async fn stop_pod_watch(&self) {
        if let Ok(mut guard) = self.pod_watch.lock() {
            *guard = None;
        }
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

            let exec = campaign
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
                    ExecuteActionError::NoExecChannel(message) => {
                        error!("execute_action no exec channel: {}", message);
                        ApiError {
                            status: axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                            body: api::ErrorResponse {
                                error: message,
                                details: None,
                            },
                        }
                    }
                    ExecuteActionError::InvariantViolation(message) => {
                        error!("execute_action invariant violation: {}", message);
                        ApiError {
                            status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            body: api::ErrorResponse {
                                error: message,
                                details: None,
                            },
                        }
                    }
                }
            })?;
            campaign.add_open_step(exec.clone());
            exec
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
        Commands::Armory(args) => run_show_armory(args),
    }
}

// ---------------------------------------------------------------------------
// ScriptParserRunner — external parser backed by executable scripts
// ---------------------------------------------------------------------------

/// Looks for scripts in `{parsers_dir}/{effect_name}.{ext}` and executes them
/// with JSON context on stdin.  Scripts must print a JSON response to stdout.
///
/// Supported extensions, tried in order: `.py`, `.sh`.
struct ScriptParserRunner {
    parsers_dir: PathBuf,
    generator_webhook: Option<Url>,
    webhook_explicit: bool,
    webhook_client: reqwest::Client,
}

impl ScriptParserRunner {
    fn new(parsers_dir: PathBuf) -> Self {
        let configured_webhook = std::env::var("RAN_PARSER_GENERATOR_WEBHOOK").ok();
        let webhook_explicit = configured_webhook.is_some();

        let generator_webhook = configured_webhook
            .as_deref()
            .and_then(|raw| {
                if matches!(raw, "off" | "none" | "disabled") {
                    return None;
                }

                match Url::parse(raw) {
                    Ok(url) if is_loopback_url(&url) => Some(url),
                    Ok(url) => {
                        warn!(
                            webhook = %url,
                            "Ignoring parser-gap webhook because only localhost/loopback endpoints are allowed"
                        );
                        None
                    }
                    Err(e) => {
                        warn!(error = %e, value = %raw, "Invalid RAN_PARSER_GENERATOR_WEBHOOK URL");
                        None
                    }
                }
            });

        if let Some(url) = &generator_webhook {
            info!(webhook = %url, "parser-gap generator webhook enabled");
        }

        let webhook_client = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            parsers_dir,
            generator_webhook,
            webhook_explicit,
            webhook_client,
        }
    }

    /// Find the first matching script for the given effect id.
    fn find_script(&self, effect_id: &str) -> Option<PathBuf> {
        // Normalise effect id using the same sanitisation as add_parser:
        // keep alphanumerics, '.', '-', '_'; replace everything else with '_'.
        // Then lowercase so lookups are case-insensitive.
        let name = effect_id
            .trim()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
            .collect::<String>()
            .to_ascii_lowercase();

        for ext in &["py", "sh"] {
            let candidate = self.parsers_dir.join(format!("{}.{}", name, ext));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    async fn run_script(
        &self,
        script_path: &PathBuf,
        request: &ExternalParseRequest,
    ) -> Option<ExternalParseResponse> {
        let input_json = match serde_json::to_string(request) {
            Ok(j) => j,
            Err(e) => {
                warn!(error = %e, "failed to serialise external parse request");
                return None;
            }
        };

        let interpreter = match script_path.extension().and_then(|e| e.to_str()) {
            Some("py") => "python3",
            Some("sh") => "sh",
            _ => return None,
        };

        info!(
            effect_id = %request.effect_id,
            script = %script_path.display(),
            "invoking external script parser"
        );

        let result = tokio::process::Command::new(interpreter)
            .arg(script_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();

        let mut child = match result {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    script = %script_path.display(),
                    error = %e,
                    "failed to spawn external parser script"
                );
                return None;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = stdin.write_all(input_json.as_bytes()).await {
                warn!(error = %e, "failed to write to script stdin");
                return None;
            }
            drop(stdin);
        }

        let output = match child.wait_with_output().await {
            Ok(o) => o,
            Err(e) => {
                warn!(error = %e, "external parser script failed");
                return None;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(
                exit_code = output.status.code().unwrap_or(-1),
                stderr = %stderr,
                "external parser script exited with error"
            );
            return None;
        }

        match serde_json::from_slice::<ExternalParseResponse>(&output.stdout) {
            Ok(response) => Some(response),
            Err(e) => {
                let stdout_preview = String::from_utf8_lossy(
                    &output.stdout[..output.stdout.len().min(512)],
                );
                warn!(
                    error = %e,
                    stdout_preview = %stdout_preview,
                    "external parser script produced invalid JSON"
                );
                None
            }
        }
    }

    async fn notify_generator(
        &self,
        request: &ExternalParseRequest,
    ) -> Option<ExternalParseResponse> {
        let Some(url) = &self.generator_webhook else {
            return None;
        };

        let response = match self
            .webhook_client
            .post(url.clone())
            .json(request)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                if self.webhook_explicit {
                    warn!(
                        webhook = %url,
                        error = %e,
                        "parser-gap webhook unavailable; continuing without generator"
                    );
                }
                return None;
            }
        };

        if !response.status().is_success() {
            if self.webhook_explicit {
                warn!(
                    webhook = %url,
                    status = %response.status(),
                    "parser-gap webhook returned non-success"
                );
            }
            return None;
        }

        let body: serde_json::Value = match response.json().await {
            Ok(v) => v,
            Err(_) => {
                // Empty/non-JSON body is valid for "generated parser written".
                return None;
            }
        };

        if body.get("system").is_some() {
            match serde_json::from_value::<ExternalParseResponse>(body.clone()) {
                Ok(parsed) => return Some(parsed),
                Err(e) => {
                    warn!(error = %e, "webhook returned invalid ExternalParseResponse JSON");
                }
            }
        }

        if let Some(parse_value) = body.get("parse") {
            match serde_json::from_value::<ExternalParseResponse>(parse_value.clone()) {
                Ok(parsed) => return Some(parsed),
                Err(e) => {
                    warn!(error = %e, "webhook 'parse' field has invalid shape");
                }
            }
        }

        None
    }
}

#[async_trait::async_trait]
impl ExternalParser for ScriptParserRunner {
    async fn try_parse(&self, request: ExternalParseRequest) -> Option<ExternalParseResponse> {
        if let Some(script_path) = self.find_script(&request.effect_id) {
            if let Some(parsed) = self.run_script(&script_path, &request).await {
                return Some(parsed);
            }
        }

        // Give an optional external generator process a chance to react to the
        // parser gap. It can either return parsed facts directly, or create a
        // script on disk that we'll discover in the retry below.
        if let Some(parsed) = self.notify_generator(&request).await {
            return Some(parsed);
        }

        // Retry once in case the webhook generated a new script.
        if let Some(script_path) = self.find_script(&request.effect_id) {
            return self.run_script(&script_path, &request).await;
        }

        None
    }
}

fn is_loopback_url(url: &Url) -> bool {
    match url.host_str() {
        Some("localhost") | Some("127.0.0.1") | Some("::1") => true,
        _ => false,
    }
}

fn run_show_armory(args: ArmoryArgs) -> Result<()> {
    let armory_dir = resolve_armory_dir(args.armory)?;
    let armory = Armory::load_from_dir(&armory_dir)?;
    let ttps = armory.ttps();

    // Column widths
    let id_w = ttps.iter().map(|t| t.id.len()).max().unwrap_or(6).max(6);
    let name_w = ttps.iter().map(|t| t.name.len()).max().unwrap_or(4).max(4);
    let tactic_w = ttps.iter().map(|t| t.tactic.len()).max().unwrap_or(6).max(6);
    let status_w = ttps.iter().map(|t| t.status.len()).max().unwrap_or(6).max(6);
    let desc_w = 60usize;

    let sep = format!(
        "+-{}-+-{}-+-{}-+-{}-+-{}-+",
        "-".repeat(id_w),
        "-".repeat(name_w),
        "-".repeat(tactic_w),
        "-".repeat(status_w),
        "-".repeat(desc_w),
    );

    println!("{sep}");
    println!(
        "| {:id_w$} | {:name_w$} | {:tactic_w$} | {:status_w$} | {:desc_w$} |",
        "TTP ID", "Name", "Tactic", "Status", "Description",
        id_w = id_w, name_w = name_w, tactic_w = tactic_w, status_w = status_w, desc_w = desc_w,
    );
    println!("{sep}");

    for ttp in ttps {
        let desc = if ttp.description.len() > desc_w {
            format!("{}…", &ttp.description[..desc_w - 1])
        } else {
            ttp.description.clone()
        };
        println!(
            "| {:id_w$} | {:name_w$} | {:tactic_w$} | {:status_w$} | {:desc_w$} |",
            ttp.id, ttp.name, ttp.tactic, ttp.status, desc,
            id_w = id_w, name_w = name_w, tactic_w = tactic_w, status_w = status_w, desc_w = desc_w,
        );
    }

    println!("{sep}");
    println!("{} TTPs loaded from {}", ttps.len(), armory_dir.display());

    Ok(())
}

async fn run_emulate(args: EmulateArgs) -> Result<()> {
    let cfg = config::load(args.config)?;
    let kubeconfig_path = kubeconfig_path_or_err(args.kubeconfig)?;
    let k8s = K8sService::from_kubeconfig(Some(kubeconfig_path.clone())).await?;
    let target_cluster = target_cluster_from_kubeconfig(Some(kubeconfig_path.clone()))?;
    let armory_dir = resolve_armory_dir(args.armory)?;
    let armory = Armory::load_from_dir(&armory_dir)?;

    // External script parsers live in armory/parsers/ (sibling to TTPs/).
    let parsers_dir = armory_dir.parent().unwrap_or(&armory_dir).join("parsers");
    let external_parser: Option<Arc<dyn ExternalParser>> = if parsers_dir.is_dir() {
        info!(dir = %parsers_dir.display(), "script parser directory found");
        Some(Arc::new(ScriptParserRunner::new(parsers_dir.clone())))
    } else {
        info!(dir = %parsers_dir.display(), "no script parser directory; external parsers disabled");
        None
    };

    let campaign_cluster = K8sCluster::new(target_cluster.name)
        .with_context_name(target_cluster.context_name)
        .with_server(target_cluster.server);

    let campaign = Arc::new(RwLock::new(Campaign::bootstrap(
        "Ran",
        campaign_cluster.clone(),
    )));

    let (c2_handle, c2_events, c2_manager) = C2Manager::new(256, k8s.clone());
    let campaign_events = CampaignEventBus::new(256);

    tokio::spawn(c2_manager.run());
    spawn_c2_event_processor_with_external_parser(
        campaign.clone(),
        c2_events,
        campaign_events.clone(),
        external_parser,
    );
    tokio::spawn(bridge_campaign_events_to_sse(campaign_events.subscribe()));

    let state = AppState {
        k8s,
        campaign,
        c2: c2_handle,
        armory,
        namespace_filter: cfg.namespaces,
        ran_name: "Ran".to_string(),
        target_cluster: campaign_cluster,
        campaign_events: campaign_events.clone(),
        pod_watch: Arc::new(Mutex::new(None)),
    };
    let campaign_entity_count = state
        .campaign
        .read()
        .map(|c| c.entity_count())
        .unwrap_or_default();
    let armory_count = state.armory.ttps().len();

    let mcp_config = api::McpConfig {
        campaign_events: campaign_events.clone(),
        parsers_dir: Some(parsers_dir),
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let ns_filter_log = state.namespace_filter.clone();
    let app: Router = api::router_with_sse_and_mcp(state, mcp_config)
        .fallback(api::frontend_handler);

    info!("starting emulate API server");
    info!(kubeconfig = %kubeconfig_path.display(), "using kubeconfig");
    info!(armory_dir = %armory_dir.display(), armory_ttps = armory_count, "armory loaded");
    info!(campaign_entities = campaign_entity_count, "campaign initialized");
    info!(%addr, namespace = ?args.namespace, "listening");
    if !ns_filter_log.included.is_empty() {
        info!(namespaces = ?ns_filter_log.included, "namespace whitelist active");
    } else if !ns_filter_log.excluded.is_empty() {
        info!(namespaces = ?ns_filter_log.excluded, "namespace blacklist active");
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn init_tracing() {
    let mut filter = EnvFilter::try_from_env("RAN_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // Keep Ran logs configurable via RAN_LOG while muting very chatty HTTP internals.
    // This prevents flooding from lines like "connecting to 127.0.0.1:5173".
    for directive in [
        "hyper=info",
        "hyper_util=info",
        "h2=info",
        "reqwest=info",
        "tower=info",
        "tower_http=info",
    ] {
        if let Ok(parsed) = directive.parse() {
            filter = filter.add_directive(parsed);
        }
    }

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
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
            Ok(CampaignEvent::ParseAudited { audits, .. }) => {
                api::publish_sse_event(
                    "parse-audited",
                    serde_json::json!({
                        "type": "parse-audited",
                        "data": {
                            "audits": audits,
                        },
                    })
                    .to_string(),
                );
            }
            Ok(CampaignEvent::Reset) => {
                api::publish_sse_event(
                    "reset-campaign",
                    serde_json::json!({ "type": "reset-campaign" }).to_string(),
                );
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                error!(skipped, "campaign SSE bridge lagged behind campaign event bus");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}
