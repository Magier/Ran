pub mod config;

use std::{
    collections::BTreeSet,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use anyhow::Result;
use armory::Armory;
use axum::Router;
use c2::{C2Handle, C2Manager};
use reqwest::Url;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use api::{ApiError, ApiService, GetRunningPodsParams, K8sResource};
use campaign::{
    spawn_c2_event_processor_with_external_parser, Campaign, CampaignEvent, CampaignEventBus,
    EntitySummary, ExecuteActionError, ExecuteActionRequest, ExecuteActionResult,
    ExternalParseRequest, ExternalParseResponse, ExternalParser, InitialClusterKnowledge,
    InitialKnowledge, InitialKubeconfigKnowledge, KnowledgeProvenance,
};
use config::{NamespaceFilter, SeedKnowledgeConfig};
use k8s::{kubeconfig_path_or_err, resolve_kubeconfig, Client, ResolvedKubeconfig};
use ran_domain::{Entity, K8sCluster, K8sCredential, Pod, RelationSummary};

// ---------------------------------------------------------------------------
// AppState — the ApiService implementation
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    k8s: Client,
    campaign: Arc<RwLock<Campaign>>,
    c2: C2Handle,
    armory: Armory,
    namespace_filter: NamespaceFilter,
    /// Live scoring profile — mutable at runtime via the tuning API.
    scoring_profile: Arc<RwLock<utility_ai::Profile>>,
    /// Configured base profile (from ran.yaml), used by reset.
    scoring_base: utility_ai::Profile,
    /// Sidecar file persisting tuned overrides across restarts.
    scoring_sidecar: Option<PathBuf>,
    /// Feature flag enabling the frontend tuning UI.
    scoring_tuning: bool,
    /// Live-captured operator decisions (pre-action candidate set + the chosen
    /// action) for calibration. Appended on every executed action.
    decision_log: Arc<RwLock<Vec<utility_ai::DecisionPoint>>>,
    /// Sidecar file (JSONL) persisting the decision log across restarts.
    decisions_sidecar: Option<PathBuf>,
    ran_name: String,
    initial_knowledge: InitialKnowledge,
    campaign_events: CampaignEventBus,
    plan_executors:
        Arc<Mutex<std::collections::HashMap<String, Arc<Mutex<planner::PlanExecutor>>>>>,
    /// Directory pre-defined plans are listed and loaded from by the web UI.
    plans_dir: PathBuf,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        k8s: Client,
        campaign: Arc<RwLock<Campaign>>,
        c2: C2Handle,
        armory: Armory,
        namespace_filter: NamespaceFilter,
        scoring_profile: utility_ai::Profile,
        scoring_base: utility_ai::Profile,
        scoring_sidecar: Option<PathBuf>,
        scoring_tuning: bool,
        ran_name: String,
        initial_knowledge: InitialKnowledge,
        campaign_events: CampaignEventBus,
        plans_dir: PathBuf,
    ) -> Self {
        // The decision log lives next to the scoring sidecar so both share the
        // config's directory and lifecycle.
        let decisions_sidecar = scoring_sidecar
            .as_ref()
            .map(|p| p.with_extension("decisions.jsonl"));
        let decision_log = decisions_sidecar
            .as_ref()
            .map(|p| load_decision_log(p))
            .unwrap_or_default();
        Self {
            k8s,
            campaign,
            c2,
            armory,
            namespace_filter,
            scoring_profile: Arc::new(RwLock::new(scoring_profile)),
            scoring_base,
            scoring_sidecar,
            scoring_tuning,
            decision_log: Arc::new(RwLock::new(decision_log)),
            decisions_sidecar,
            ran_name,
            initial_knowledge,
            campaign_events,
            plan_executors: Arc::new(Mutex::new(std::collections::HashMap::new())),
            plans_dir,
        }
    }

    /// Append a captured operator decision to the in-memory log and, if a sidecar
    /// is configured, the JSONL file. Best-effort: a persistence failure is logged,
    /// never fatal to the action being executed.
    fn record_decision(&self, dp: utility_ai::DecisionPoint) {
        if let Some(path) = &self.decisions_sidecar {
            match serde_json::to_string(&dp) {
                Ok(line) => {
                    use std::io::Write;
                    match std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                    {
                        Ok(mut f) => {
                            if let Err(e) = writeln!(f, "{line}") {
                                warn!(path = %path.display(), error = %e, "failed to append decision log");
                            }
                        }
                        Err(e) => {
                            warn!(path = %path.display(), error = %e, "failed to open decision log")
                        }
                    }
                }
                Err(e) => warn!(error = %e, "failed to serialize captured decision"),
            }
        }
        if let Ok(mut log) = self.decision_log.write() {
            log.push(dp);
        }
    }

    /// Fit a scoring profile from the operator decisions captured so far, so the
    /// utility AI reproduces those choices under the same conditions. Returns
    /// `None` if nothing has been captured yet.
    pub fn calibrate(&self) -> Option<utility_ai::Calibration> {
        let log = self.decision_log.read().ok()?;
        if log.is_empty() {
            return None;
        }
        let names = utility_ai::utility_consideration_names();
        let calibration = utility_ai::fit(&names, &log, &utility_ai::FitOptions::default());

        // `fit` drops decisions whose feature width doesn't match the current
        // consideration set (a log spanning a considerations change). If none
        // remain, there's nothing meaningful to calibrate from.
        let used = calibration.per_decision.len();
        if used == 0 {
            warn!(
                captured = log.len(),
                "no captured decisions match the current consideration set (stale decision log); nothing to calibrate"
            );
            return None;
        }
        if used < log.len() {
            warn!(
                used,
                captured = log.len(),
                "calibrating on decisions matching the current consideration set; older/mismatched entries dropped"
            );
        }
        Some(calibration)
    }

    /// Build, dispatch, and await cleanup actions for everything executed so far
    /// (TTPs that declare a `cleanup` procedure — e.g. deleting created pods).
    /// Shared by `reset_campaign` and the launch-time plan runner. Waits up to
    /// 30s for the cleanup results to be recorded, then returns.
    pub(crate) async fn run_cleanup(&self) -> Result<(), ApiError> {
        let cleanup_actions: Vec<c2::ExecTtp> = {
            let mut campaign = self
                .campaign
                .write()
                .map_err(|_| ApiError::internal("campaign lock poisoned"))?;
            campaign.build_cleanup_actions(&self.armory)
            // write lock released here
        };

        if cleanup_actions.is_empty() {
            info!("no cleanup actions to run");
            return Ok(());
        }

        let cleanup_ids: std::collections::HashSet<String> =
            cleanup_actions.iter().map(|e| e.id.clone()).collect();

        info!(count = cleanup_ids.len(), "dispatching cleanup actions");

        // Cleanup ExecTtps are intentionally not registered in open_steps —
        // we track completion by polling execution_records instead.
        for exec in cleanup_actions {
            if let Err(e) = self.c2.send(exec).await {
                warn!("failed to dispatch cleanup action: {}", e);
            }
        }

        // Wait for all cleanup results to be recorded by the C2 event processor.
        // Poll with 200 ms intervals, 30 s deadline.
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
        loop {
            let completed: std::collections::HashSet<String> = {
                let guard = self
                    .campaign
                    .read()
                    .map_err(|_| ApiError::internal("campaign lock poisoned"))?;
                guard
                    .execution_records
                    .iter()
                    .filter(|r| r.is_cleanup)
                    .map(|r| r.id.clone())
                    .collect()
            };
            if cleanup_ids.is_subset(&completed) {
                info!("all cleanup actions completed");
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                warn!(
                    remaining = cleanup_ids.difference(&completed).count(),
                    "cleanup timed out after 30s"
                );
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }

        Ok(())
    }
}

/// Lightweight summary of a plan file on disk, listed by the web UI so the
/// operator can pick one to load without downloading the full YAML.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PlanSummary {
    /// File name within the plans directory (what `load_plan` expects).
    pub filename: String,
    /// Plan `id` from the YAML.
    pub id: String,
    /// Human-readable plan name.
    pub name: String,
    /// Optional plan description.
    pub description: Option<String>,
    /// Number of steps in the plan.
    pub steps: usize,
}

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

    fn scoring_profile(&self) -> utility_ai::Profile {
        self.scoring_profile
            .read()
            .map(|p| p.clone())
            .unwrap_or_default()
    }

    fn set_scoring_profile(&self, profile: utility_ai::Profile) {
        if let Ok(mut guard) = self.scoring_profile.write() {
            *guard = profile;
        }
    }

    fn save_scoring_profile(&self) -> Result<(), String> {
        let path = self
            .scoring_sidecar
            .as_ref()
            .ok_or_else(|| "no scoring sidecar path configured".to_string())?;
        let profile = self
            .scoring_profile
            .read()
            .map_err(|_| "scoring profile lock poisoned".to_string())?
            .clone();
        let yaml = serde_yaml::to_string(&profile).map_err(|e| e.to_string())?;
        std::fs::write(path, yaml).map_err(|e| format!("failed to write {}: {e}", path.display()))
    }

    fn reset_scoring_profile(&self) -> utility_ai::Profile {
        let base = self.scoring_base.clone();
        if let Ok(mut guard) = self.scoring_profile.write() {
            *guard = base.clone();
        }
        // Drop persisted overrides so the reset survives a restart too.
        if let Some(path) = &self.scoring_sidecar {
            let _ = std::fs::remove_file(path);
        }
        base
    }

    fn scoring_tuning_enabled(&self) -> bool {
        self.scoring_tuning
    }

    fn calibrate_scoring(&self) -> Option<utility_ai::Calibration> {
        self.calibrate()
    }

    async fn reset_campaign(&self) -> Result<(), ApiError> {
        // Phase 1: run cleanup actions for everything executed so far.
        self.run_cleanup().await?;

        // -----------------------------------------------------------------------
        // Phase 2: wipe campaign state
        // -----------------------------------------------------------------------
        let mut campaign = self
            .campaign
            .write()
            .map_err(|_| ApiError::internal("campaign lock poisoned"))?;
        campaign.reset_with_knowledge(self.ran_name.clone(), self.initial_knowledge.clone());
        let _ = self.campaign_events.publish(CampaignEvent::Reset);
        Ok(())
    }

    async fn get_armory(&self, params: api::GetArmoryParams) -> Result<Vec<armory::Ttp>, ApiError> {
        Ok(self.armory.ttps_for_tactic(params.tactic.as_deref()))
    }

    async fn execute_action(
        &self,
        cmd: ExecuteActionRequest,
    ) -> Result<ExecuteActionResult, ApiError> {
        if let Err(error) = self.stage_live_initial_access_target(&cmd).await {
            return Err(self.record_preparation_error(&cmd, error));
        }

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

        let request_ctx = cmd.clone();

        let exec = {
            let mut campaign = self.campaign.write().map_err(|_| {
                error!("campaign lock poisoned while executing action");
                ApiError::internal("campaign lock poisoned")
            })?;

            let exec = match campaign.prepare_action(cmd, &self.armory) {
                Ok(exec) => exec,
                Err(err) => {
                    drop(campaign);
                    return Err(self.record_preparation_error(&request_ctx, err));
                }
            };
            // Capture the decision under the operator's *actual* pre-action
            // conditions (zero reconstruction) for calibration. `prepare_action`
            // only grounded the command — no effects applied yet — so this is the
            // exact state the choice was made in.
            let captured = utility_ai::decision_point(
                &campaign,
                self.armory.ttps(),
                &exec.ttp.id,
                &exec.target_id,
            );
            campaign.add_open_step(exec.clone());
            (exec, captured)
        };
        let (exec, captured) = exec;
        if let Some(dp) = captured {
            self.record_decision(dp);
        }

        publish_ttp_dispatched(&exec);

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

    async fn execute_plan(&self, plan_yaml: String) -> Result<String, ApiError> {
        let plan: planner::PlanDefinition =
            serde_yaml::from_str(&plan_yaml).map_err(|e| ApiError::bad_request(e.to_string()))?;
        let plan_id = plan.id.clone();
        let executor =
            planner::PlanExecutor::new(plan).map_err(|e| ApiError::bad_request(e.to_string()))?;
        let executor = Arc::new(Mutex::new(executor));

        self.plan_executors
            .lock()
            .unwrap()
            .insert(plan_id.clone(), executor.clone());

        let this = self.clone();
        let plan_id_bg = plan_id.clone();
        tokio::spawn(async move {
            // How long to wait for a step outcome before treating the plan as
            // stalled (no in-flight work, yet steps remain that can't resolve).
            const PLAN_STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
            let mut events = this.campaign_events.subscribe();

            loop {
                // Run tick
                let dispatches = {
                    let campaign = this.campaign.read().unwrap();
                    executor.lock().unwrap().tick(&campaign)
                };

                for dispatch in dispatches {
                    let step_id = dispatch.step_id.clone();
                    let action_id = dispatch.request.action_id.clone();
                    let target_id = dispatch.request.target_id.clone();
                    match this.execute_action(dispatch.request).await {
                        Ok(result) => {
                            info!(
                                plan_id = %plan_id_bg,
                                step_id = %step_id,
                                action_id = %action_id,
                                target_id = %target_id,
                                cmd_id = %result.cmd_id,
                                "plan step dispatched"
                            );
                            executor
                                .lock()
                                .unwrap()
                                .record_dispatched(&step_id, vec![result.cmd_id.clone()]);
                            let _ =
                                this.campaign_events
                                    .publish(CampaignEvent::PlanStepDispatched {
                                        plan_id: plan_id_bg.clone(),
                                        step_id,
                                        exec_count: 1,
                                    });
                        }
                        Err(e) => {
                            tracing::error!(
                                "plan dispatch error for step {}: {}",
                                step_id,
                                e.body.error
                            );
                        }
                    }
                }

                if executor.lock().unwrap().is_complete() {
                    break;
                }

                // Wait for the next TtpExecuted event, with a stall deadline so a
                // plan whose remaining steps can never resolve (missing target,
                // unmet graph predicate) terminates instead of hanging forever.
                // Also watch for Ctrl-C: axum's graceful shutdown waits for SSE
                // connections to drain, so the runtime may not drop this task
                // promptly — we need to observe the signal ourselves.
                let next = loop {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            tracing::info!(plan_id = %plan_id_bg, "plan runner stopping on shutdown signal");
                            return;
                        }
                        result = tokio::time::timeout(PLAN_STALL_TIMEOUT, events.recv()) => {
                            match result {
                                Ok(Ok(CampaignEvent::TtpExecuted {
                                    cmd_id, success, ..
                                })) => break Some((cmd_id, success)),
                                Ok(Ok(_)) => continue,
                                Ok(Err(_)) => return, // event channel closed
                                Err(_) => break None, // timed out
                            }
                        }
                    }
                };

                let plan_events = match next {
                    Some((cmd_id, success)) => {
                        let (effective_success, expect_reason) = {
                            let expect = executor.lock().unwrap().expectation_for_cmd(&cmd_id);
                            if let Some(expect) = expect {
                                if expect.min_facts_written > 0 {
                                    let facts_written: usize = this
                                        .campaign
                                        .read()
                                        .map(|c| {
                                            c.parse_audits
                                                .iter()
                                                .filter(|a| a.cmd_id == cmd_id)
                                                .filter(|a| {
                                                    matches!(
                                                        a.parse_result,
                                                        campaign::ParseResult::Parsed
                                                    )
                                                })
                                                .map(|a| a.inferred_facts_written)
                                                .sum()
                                        })
                                        .unwrap_or(0);
                                    if facts_written < expect.min_facts_written {
                                        let reason = format!(
                                            "expectation unmet: inferred_facts_written={} < required {}",
                                            facts_written, expect.min_facts_written
                                        );
                                        (false, Some(reason))
                                    } else {
                                        (success, None)
                                    }
                                } else {
                                    (success, None)
                                }
                            } else {
                                (success, None)
                            }
                        };

                        if let Some(reason) = expect_reason {
                            tracing::warn!(plan_id = %plan_id_bg, cmd_id = %cmd_id, %reason, "plan step expectation failed");
                        }

                        let armory = this.armory.clone();
                        executor.lock().unwrap().on_ttp_executed(
                            &cmd_id,
                            effective_success,
                            None,
                            &armory,
                        )
                    }
                    None => {
                        // An action may genuinely still be running — keep waiting.
                        // Otherwise the plan is stalled: drive the unresolvable
                        // steps terminal so it can complete.
                        if executor.lock().unwrap().has_in_flight() {
                            continue;
                        }
                        tracing::warn!(
                            plan_id = %plan_id_bg,
                            "plan stalled with no in-flight actions; failing unresolvable steps"
                        );
                        let campaign = this.campaign.read().unwrap();
                        executor.lock().unwrap().fail_stalled(&campaign)
                    }
                };

                for event in &plan_events {
                    use planner::PlanEvent;
                    match event {
                        PlanEvent::StepCompleted { step_id, success } => {
                            info!(
                                plan_id = %plan_id_bg,
                                step_id = %step_id,
                                success = *success,
                                "plan step completed"
                            );
                        }
                        PlanEvent::StepSkipped { step_id, reason } => {
                            info!(
                                plan_id = %plan_id_bg,
                                step_id = %step_id,
                                reason = %reason,
                                "plan step skipped"
                            );
                        }
                        PlanEvent::StepFailed { step_id, reason } => {
                            warn!(
                                plan_id = %plan_id_bg,
                                step_id = %step_id,
                                reason = %reason,
                                "plan step failed"
                            );
                        }
                        PlanEvent::PlanComplete => {
                            info!(plan_id = %plan_id_bg, "plan complete");
                        }
                        _ => {}
                    }

                    let campaign_event = match event {
                        PlanEvent::StepCompleted { step_id, success } => {
                            Some(CampaignEvent::PlanStepCompleted {
                                plan_id: plan_id_bg.clone(),
                                step_id: step_id.clone(),
                                success: *success,
                            })
                        }
                        PlanEvent::StepSkipped { step_id, reason } => {
                            Some(CampaignEvent::PlanStepSkipped {
                                plan_id: plan_id_bg.clone(),
                                step_id: step_id.clone(),
                                reason: reason.clone(),
                            })
                        }
                        PlanEvent::StepFailed { step_id, reason } => {
                            Some(CampaignEvent::PlanStepFailed {
                                plan_id: plan_id_bg.clone(),
                                step_id: step_id.clone(),
                                reason: reason.clone(),
                            })
                        }
                        PlanEvent::PlanComplete => Some(CampaignEvent::PlanComplete {
                            plan_id: plan_id_bg.clone(),
                        }),
                        _ => None,
                    };
                    if let Some(e) = campaign_event {
                        let _ = this.campaign_events.publish(e);
                    }
                }

                if executor.lock().unwrap().is_complete() {
                    break;
                }
            }
        });

        Ok(plan_id)
    }

    async fn get_plan_status(&self, plan_id: &str) -> Result<serde_json::Value, ApiError> {
        let executors = self.plan_executors.lock().unwrap();
        let executor = executors
            .get(plan_id)
            .ok_or_else(|| ApiError::not_found(format!("plan '{}' not found", plan_id)))?;
        let executor = executor.lock().unwrap();
        Ok(serde_json::json!({
            "plan_id": plan_id,
            "is_complete": executor.is_complete(),
        }))
    }

    async fn export_plan(&self, include_failed: bool) -> Result<String, ApiError> {
        let campaign = self
            .campaign
            .read()
            .map_err(|_| ApiError::internal("campaign lock poisoned"))?;
        let opts = planner::ExportOptions { include_failed };
        let plan = planner::export_plan(&campaign.execution_records, &opts);
        let yaml = serde_yaml::to_string(&plan).map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(yaml)
    }

    async fn list_plans(&self) -> Result<Vec<serde_json::Value>, ApiError> {
        // Recursively collect candidate YAML files. A missing plans directory is
        // not an error — just an empty list.
        let mut files = Vec::new();
        match collect_yaml_files(&self.plans_dir, &mut files) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(ApiError::internal(format!(
                    "failed to read plans directory {}: {}",
                    self.plans_dir.display(),
                    e
                )))
            }
        }

        let mut summaries = Vec::new();
        for path in files {
            // `filename` is the path relative to the plans directory, using `/`
            // separators, so nested plans round-trip through `load_plan`.
            let Some(filename) = path
                .strip_prefix(&self.plans_dir)
                .ok()
                .and_then(|rel| rel.to_str())
                .map(|s| s.replace('\\', "/"))
            else {
                continue;
            };

            // Best-effort: skip files that don't parse as a plan.
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "failed to read plan file; skipping");
                    continue;
                }
            };
            let plan = match serde_yaml::from_str::<planner::PlanDefinition>(&text) {
                Ok(p) => p,
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "not a valid plan; skipping");
                    continue;
                }
            };

            let summary = PlanSummary {
                filename,
                id: plan.id,
                name: plan.name,
                description: plan.description,
                steps: plan.steps.len(),
            };
            match serde_json::to_value(summary) {
                Ok(v) => summaries.push(v),
                Err(e) => warn!(error = %e, "failed to serialize plan summary; skipping"),
            }
        }

        // Stable order so the UI list doesn't shuffle between requests.
        summaries.sort_by(|a, b| {
            a.get("filename")
                .and_then(|v| v.as_str())
                .cmp(&b.get("filename").and_then(|v| v.as_str()))
        });
        Ok(summaries)
    }

    async fn load_plan(&self, filename: String) -> Result<String, ApiError> {
        let path = resolve_plan_path(&self.plans_dir, &filename)?;
        let yaml = match std::fs::read_to_string(&path) {
            Ok(y) => y,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ApiError::not_found(format!("plan '{filename}' not found")))
            }
            Err(e) => {
                return Err(ApiError::internal(format!(
                    "failed to read plan {}: {}",
                    path.display(),
                    e
                )))
            }
        };

        // Seed root-step targets from the live cluster (same as the CLI launch
        // path) so plans loaded from the UI behave identically.
        if let Ok(plan) = serde_yaml::from_str::<planner::PlanDefinition>(&yaml) {
            seed_initial_access_targets(self, &plan).await;
        }

        self.execute_plan(yaml).await
    }
}

// ---------------------------------------------------------------------------
// ScriptParserRunner — external parser backed by executable scripts
// ---------------------------------------------------------------------------

/// Looks for scripts in `{parsers_dir}/{effect_name}.{ext}` and executes them
/// with JSON context on stdin.  Scripts must print a JSON response to stdout.
///
/// Supported extensions, tried in order: `.py`, `.sh`.
pub struct ScriptParserRunner {
    parsers_dir: PathBuf,
    generator_webhook: Option<Url>,
    webhook_explicit: bool,
    webhook_client: reqwest::Client,
}

impl ScriptParserRunner {
    pub fn new(parsers_dir: PathBuf) -> Self {
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
            .map(|c| {
                if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
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
                let stdout_preview =
                    String::from_utf8_lossy(&output.stdout[..output.stdout.len().min(512)]);
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
    matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1")
    )
}

fn credential_from_resolved(
    resolved: &ResolvedKubeconfig,
    name: impl Into<String>,
) -> K8sCredential {
    let mut credential =
        K8sCredential::new(resolved.server.clone().unwrap_or_default()).with_name(name);
    credential.context_name = Some(resolved.context_name.clone());
    credential.default_namespace = resolved.default_namespace.clone();
    credential.user_name = resolved.user_name.clone();
    credential.auth_method = resolved.auth_method.clone();
    credential.has_token = resolved.has_token;
    credential.has_client_certificate = resolved.has_client_certificate;
    credential.has_client_key = resolved.has_client_key;
    credential.ca_data = resolved.ca_data.clone();
    credential.token = resolved.token.clone();
    credential.cert_data = resolved.cert_data.clone();
    credential.key_data = resolved.key_data.clone();
    credential
}

fn single_origin(origin: KnowledgeProvenance) -> BTreeSet<KnowledgeProvenance> {
    BTreeSet::from([origin])
}

fn clusters_match(a: &K8sCluster, b: &K8sCluster) -> bool {
    match (a.server.as_deref(), b.server.as_deref()) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => a == b,
        _ => a.name.eq_ignore_ascii_case(&b.name),
    }
}

fn credentials_match(a: &K8sCredential, b: &K8sCredential) -> bool {
    a.endpoint == b.endpoint && a.context_name == b.context_name && a.user_name == b.user_name
}

fn deduplicate_initial_clusters(
    initial: &mut InitialKnowledge,
) -> std::collections::HashMap<ran_domain::EntityId, ran_domain::EntityId> {
    let mut deduplicated: Vec<InitialClusterKnowledge> = Vec::new();
    let mut aliases = std::collections::HashMap::new();

    for mut entry in std::mem::take(&mut initial.clusters) {
        let original_id = entry.cluster.entity_id();
        if let Some(existing) = deduplicated
            .iter_mut()
            .find(|existing| clusters_match(&existing.cluster, &entry.cluster))
        {
            if existing.cluster.server.is_none() {
                existing.cluster.server = entry.cluster.server.take();
            }
            if existing.cluster.context_name.is_none() {
                existing.cluster.context_name = entry.cluster.context_name.take();
            }
            existing.provenance.extend(entry.provenance);
            aliases.insert(original_id, existing.cluster.entity_id());
        } else {
            aliases.insert(original_id.clone(), original_id);
            deduplicated.push(entry);
        }
    }

    initial.clusters = deduplicated;
    for credential in &mut initial.kubeconfigs {
        if let Some(preferred) = aliases.get(&credential.cluster_id) {
            credential.cluster_id = preferred.clone();
        }
    }
    aliases
}

fn build_initial_knowledge(
    active: &ResolvedKubeconfig,
    seeds: &[SeedKnowledgeConfig],
) -> Result<InitialKnowledge> {
    let mut initial = InitialKnowledge::default();
    let mut cluster_aliases: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    // Register declared cluster aliases first so credential entries are order-independent.
    for seed in seeds {
        let SeedKnowledgeConfig::Cluster(config) = seed else {
            continue;
        };
        let cluster = K8sCluster::new(config.name.as_deref().unwrap_or(&config.id))
            .with_id(&config.id)
            .with_context_name(config.context_name.clone())
            .with_server(config.server.clone());
        if let Some(existing) = initial.clusters.iter().find(|entry| {
            entry.cluster.entity_id() == cluster.entity_id()
                && entry.cluster.server != cluster.server
        }) {
            anyhow::bail!(
                "cluster seeds '{}' and '{}' resolve to the same entity with conflicting servers",
                existing.cluster.entity_id(),
                config.id
            );
        }
        if let Some((idx, entry)) = initial
            .clusters
            .iter_mut()
            .enumerate()
            .find(|(_, entry)| clusters_match(&entry.cluster, &cluster))
        {
            entry.provenance.insert(config.provenance);
            cluster_aliases.insert(config.id.clone(), idx);
        } else {
            cluster_aliases.insert(config.id.clone(), initial.clusters.len());
            initial.clusters.push(InitialClusterKnowledge {
                cluster,
                provenance: single_origin(config.provenance),
            });
        }
    }

    let active_cluster = K8sCluster::new(&active.cluster_name)
        .with_context_name(Some(active.context_name.clone()))
        .with_server(active.server.clone());
    let active_cluster_idx = if let Some((idx, existing)) = initial
        .clusters
        .iter_mut()
        .enumerate()
        .find(|(_, entry)| clusters_match(&entry.cluster, &active_cluster))
    {
        if existing.cluster.server.is_none() {
            existing.cluster.server = active_cluster.server.clone();
        }
        if existing.cluster.context_name.is_none() {
            existing.cluster.context_name = active_cluster.context_name.clone();
        }
        existing.provenance.insert(KnowledgeProvenance::Operator);
        idx
    } else {
        let idx = initial.clusters.len();
        initial.clusters.push(InitialClusterKnowledge {
            cluster: active_cluster,
            provenance: single_origin(KnowledgeProvenance::Operator),
        });
        idx
    };

    for seed in seeds {
        let SeedKnowledgeConfig::Credential(config) = seed else {
            continue;
        };
        let resolved = resolve_kubeconfig(&config.path, config.context.as_deref())?;
        let resolved_cluster = K8sCluster::new(&resolved.cluster_name)
            .with_context_name(Some(resolved.context_name.clone()))
            .with_server(resolved.server.clone());

        let cluster_idx = if let Some(alias) = config.cluster.as_deref() {
            let idx = *cluster_aliases.get(alias).ok_or_else(|| {
                anyhow::anyhow!(
                    "seed credential '{}' references unknown cluster '{}'",
                    config.id,
                    alias
                )
            })?;
            let declared = &mut initial.clusters[idx];
            if let (Some(expected), Some(actual)) = (
                declared.cluster.server.as_deref(),
                resolved.server.as_deref(),
            ) {
                if expected != actual {
                    anyhow::bail!(
                        "seed credential '{}' resolves to server '{}' but cluster '{}' declares '{}'",
                        config.id,
                        actual,
                        alias,
                        expected
                    );
                }
            }
            if declared.cluster.server.is_none() {
                declared.cluster.server = resolved.server.clone();
            }
            if declared.cluster.context_name.is_none() {
                declared.cluster.context_name = Some(resolved.context_name.clone());
            }
            declared.provenance.insert(config.provenance);
            idx
        } else if let Some((idx, entry)) = initial
            .clusters
            .iter_mut()
            .enumerate()
            .find(|(_, entry)| clusters_match(&entry.cluster, &resolved_cluster))
        {
            entry.provenance.insert(config.provenance);
            idx
        } else {
            let idx = initial.clusters.len();
            initial.clusters.push(InitialClusterKnowledge {
                cluster: resolved_cluster,
                provenance: single_origin(config.provenance),
            });
            idx
        };

        let credential = credential_from_resolved(&resolved, &config.id);
        if let Some(existing) = initial
            .kubeconfigs
            .iter_mut()
            .find(|entry| credentials_match(&entry.credential, &credential))
        {
            existing.provenance.insert(config.provenance);
        } else {
            initial.kubeconfigs.push(InitialKubeconfigKnowledge {
                credential,
                cluster_id: initial.clusters[cluster_idx].cluster.entity_id(),
                provenance: single_origin(config.provenance),
            });
        }
    }

    let active_cluster_id = initial.clusters[active_cluster_idx].cluster.entity_id();
    let cluster_aliases = deduplicate_initial_clusters(&mut initial);
    let active_cluster_id = cluster_aliases
        .get(&active_cluster_id)
        .cloned()
        .unwrap_or(active_cluster_id);

    let active_name = active
        .user_name
        .as_deref()
        .unwrap_or(&active.context_name)
        .to_string();
    let mut active_credential = credential_from_resolved(active, active_name);
    active_credential.active = true;
    if let Some(existing) = initial
        .kubeconfigs
        .iter_mut()
        .find(|entry| credentials_match(&entry.credential, &active_credential))
    {
        existing.provenance.insert(KnowledgeProvenance::Operator);
        existing.credential.active = true;
    } else {
        initial.kubeconfigs.push(InitialKubeconfigKnowledge {
            credential: active_credential,
            cluster_id: active_cluster_id,
            provenance: single_origin(KnowledgeProvenance::Operator),
        });
    }

    Ok(initial)
}

#[cfg(test)]
mod initial_knowledge_tests {
    use super::*;
    use crate::config::{SeedClusterConfig, SeedCredentialConfig};

    const KUBECONFIG: &str = r#"apiVersion: v1
kind: Config
clusters:
- name: demo
  cluster:
    server: https://demo.example
contexts:
- name: demo-context
  context:
    cluster: demo
    user: developer
    namespace: default
current-context: demo-context
users:
- name: developer
  user:
    token: secret
"#;

    #[test]
    fn seeded_active_kubeconfig_deduplicates_and_merges_provenance() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config");
        std::fs::write(&path, KUBECONFIG).unwrap();
        let active = resolve_kubeconfig(&path, None).unwrap();
        let seeds = vec![
            SeedKnowledgeConfig::Cluster(SeedClusterConfig {
                id: "target-cluster".into(),
                name: None,
                server: None,
                context_name: None,
                provenance: KnowledgeProvenance::Scenario,
            }),
            SeedKnowledgeConfig::Credential(SeedCredentialConfig {
                credential_type: "kubeconfig".into(),
                id: "developer-kubeconfig".into(),
                path,
                context: None,
                cluster: Some("target-cluster".into()),
                provenance: KnowledgeProvenance::Scenario,
            }),
        ];

        let initial = build_initial_knowledge(&active, &seeds).unwrap();
        assert_eq!(initial.clusters.len(), 1);
        assert_eq!(
            initial.clusters[0].cluster.id.as_deref(),
            Some("target-cluster")
        );
        assert_eq!(initial.kubeconfigs.len(), 1);
        assert!(initial.kubeconfigs[0].credential.active);
        assert_eq!(
            initial.kubeconfigs[0]
                .credential
                .default_namespace
                .as_deref(),
            Some("default")
        );
        assert_eq!(
            initial.kubeconfigs[0].credential.entity_name(),
            "developer-kubeconfig"
        );
        assert_eq!(
            initial.kubeconfigs[0].provenance,
            BTreeSet::from([KnowledgeProvenance::Scenario, KnowledgeProvenance::Operator,])
        );
    }

    #[test]
    fn conflicting_declared_cluster_fails_initialization() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config");
        std::fs::write(&path, KUBECONFIG).unwrap();
        let active = resolve_kubeconfig(&path, None).unwrap();
        let seeds = vec![
            SeedKnowledgeConfig::Cluster(SeedClusterConfig {
                id: "target".into(),
                name: None,
                server: Some("https://other.example".into()),
                context_name: None,
                provenance: KnowledgeProvenance::Scenario,
            }),
            SeedKnowledgeConfig::Credential(SeedCredentialConfig {
                credential_type: "kubeconfig".into(),
                id: "developer".into(),
                path,
                context: None,
                cluster: Some("target".into()),
                provenance: KnowledgeProvenance::Scenario,
            }),
        ];

        assert!(build_initial_knowledge(&active, &seeds).is_err());
    }
}

// ---------------------------------------------------------------------------
// Server bootstrap
// ---------------------------------------------------------------------------

/// Configuration required to start the Ran emulation server.
pub struct ServerConfig {
    /// Path to kubeconfig file. Defaults to the standard kubeconfig location.
    pub kubeconfig: Option<PathBuf>,
    /// Path to the armory TTPs directory. Defaults to `./armory/TTPs`.
    pub armory_dir: Option<PathBuf>,
    /// TCP port to listen on.
    pub port: u16,
    /// Namespace visibility filter loaded from `ran.yaml`.
    pub namespace_filter: NamespaceFilter,
    /// Action-selection scoring configuration loaded from `ran.yaml`.
    pub scoring: config::ScoringConfig,
    /// Path to the config file, used to locate the scoring sidecar
    /// (`ran.scoring.yaml`). Defaults to `ran.yaml` when `None`.
    pub config_path: Option<PathBuf>,
    /// Optional plan YAML to execute automatically once the server is up — an
    /// alternative to `POST /api/plans`. When the plan finishes, the operator is
    /// offered cleanup (or it runs automatically if `auto_cleanup` is set), and
    /// the server shuts down after cleanup completes.
    pub plan: Option<PathBuf>,
    /// Run cleanup automatically when the launch-time plan finishes, instead of
    /// prompting on the terminal. Only meaningful together with `plan`.
    pub auto_cleanup: bool,
    /// Directory the web UI lists pre-defined plans from (`ran.yaml`'s
    /// `plans.dir`, defaulting to `plans` in the current working directory).
    pub plans_dir: PathBuf,
    /// Scenario knowledge loaded from the existing ran.yaml configuration.
    pub seed_knowledge: Vec<SeedKnowledgeConfig>,
}

/// Locate the scoring sidecar file (tuned-profile persistence) next to the
/// config file: e.g. `ran.yaml` → `ran.scoring.yaml`.
fn scoring_sidecar_path(config_path: Option<&std::path::Path>) -> PathBuf {
    config_path
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("ran.yaml"))
        .with_extension("scoring.yaml")
}

/// Load a persisted decision log (one JSON [`utility_ai::DecisionPoint`] per
/// line). Missing file → empty log; unparseable lines are skipped with a warning
/// so a partially-corrupt log doesn't lose everything.
fn load_decision_log(path: &std::path::Path) -> Vec<utility_ai::DecisionPoint> {
    let Ok(data) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, line) in data.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<utility_ai::DecisionPoint>(line) {
            Ok(dp) => out.push(dp),
            Err(e) => {
                warn!(path = %path.display(), line = i + 1, error = %e, "skipping unparseable decision log entry")
            }
        }
    }
    out
}

fn load_sidecar_profile(path: &std::path::Path) -> Option<utility_ai::Profile> {
    let data = std::fs::read(path).ok()?;
    match serde_yaml::from_slice::<utility_ai::Profile>(&data) {
        Ok(p) => Some(p),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "ignoring unparseable scoring sidecar");
            None
        }
    }
}

impl AppState {
    async fn stage_live_initial_access_target(
        &self,
        request: &ExecuteActionRequest,
    ) -> Result<(), ExecuteActionError> {
        let Some(ttp) = self.armory.get_ttp(&request.action_id) else {
            return Ok(());
        };
        if ttp.id != armory::VALID_ACCOUNTS_KUBECONFIG_ID {
            return Ok(());
        }

        let (namespace, pod_name) = parse_pod_target_id(&request.target_id)
            .map_err(|error| ExecuteActionError::InvalidInput(error.to_string()))?;
        let pod_id = ran_domain::EntityId::new(&request.target_id);
        let already_staged = self
            .campaign
            .read()
            .map_err(|_| {
                ExecuteActionError::InvariantViolation("campaign lock poisoned".to_string())
            })?
            .entities
            .contains::<Pod>(&pod_id);
        if already_staged {
            return Ok(());
        }

        let candidates = self
            .k8s
            .get_running_pods(Some(&namespace))
            .await
            .map_err(|error| ExecuteActionError::NotFound(error.to_string()))?;
        require_ready_initial_access_pod(&candidates, &request.target_id)?;

        let mut campaign = self.campaign.write().map_err(|_| {
            ExecuteActionError::InvariantViolation("campaign lock poisoned".to_string())
        })?;
        if !campaign.entities.contains::<Pod>(&pod_id) {
            campaign.stage_initial_access_pod(&pod_name, &namespace);
            info!(target_id = %request.target_id, "staged live initial-access Pod");
        }
        Ok(())
    }

    fn record_preparation_error(
        &self,
        request: &ExecuteActionRequest,
        error: ExecuteActionError,
    ) -> ApiError {
        let mut campaign = match self.campaign.write() {
            Ok(campaign) => campaign,
            Err(_) => return ApiError::internal("campaign lock poisoned"),
        };
        let (record, ttp) = campaign.record_preparation_failure(request, &self.armory, &error);
        drop(campaign);

        let _ = self.campaign_events.publish(CampaignEvent::TtpExecuted {
            cmd_id: record.id,
            action_id: record.ttp_id,
            target_id: record.target_id,
            exec_system_id: record.exec_system_id,
            ttp: Box::new(ttp),
            args: record.args,
            success: false,
            fail_reason: record.fail_reason,
            results: record.results,
            exit_code: -1,
        });

        match error {
            ExecuteActionError::InvalidInput(message) => ApiError::bad_request(message),
            ExecuteActionError::NotFound(message) => ApiError::not_found(message),
            ExecuteActionError::NoExecChannel(message) => ApiError {
                status: axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                body: api::ErrorResponse {
                    error: message,
                    details: None,
                },
            },
            ExecuteActionError::InvariantViolation(message) => ApiError::internal(message),
        }
    }
}

/// Start the Ran emulation API server. This is the primary entry point for
/// the app layer; the CLI calls this after argument parsing.
pub async fn start(cfg: ServerConfig) -> Result<()> {
    let kubeconfig_path = kubeconfig_path_or_err(cfg.kubeconfig)?;
    let active_kubeconfig = resolve_kubeconfig(kubeconfig_path.clone(), None)?;
    let k8s = Client::from_resolved_kubeconfig(&active_kubeconfig).await?;
    let initial_knowledge = build_initial_knowledge(&active_kubeconfig, &cfg.seed_knowledge)?;
    let (armory, user_armory_dir) = load_armory(cfg.armory_dir)?;

    // External script parsers live in armory/parsers/ (sibling to TTPs/).
    // Only available when the user provides an armory directory.
    let external_parser: Option<Arc<dyn ExternalParser>> =
        user_armory_dir.as_deref().and_then(|ttps_dir| {
            let parsers_dir = ttps_dir.parent().unwrap_or(ttps_dir).join("parsers");
            if parsers_dir.is_dir() {
                info!(dir = %parsers_dir.display(), "script parser directory found");
                Some(Arc::new(ScriptParserRunner::new(parsers_dir)) as Arc<dyn ExternalParser>)
            } else {
                info!(dir = %parsers_dir.display(), "no script parser directory; external parsers disabled");
                None
            }
        });

    let campaign = Arc::new(RwLock::new(Campaign::bootstrap_with_knowledge(
        "Ran",
        initial_knowledge.clone(),
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

    // Base profile from ran.yaml; if a tuned sidecar exists, it overrides it.
    let scoring_base = cfg.scoring.to_profile();
    let sidecar = scoring_sidecar_path(cfg.config_path.as_deref());
    let live_profile = load_sidecar_profile(&sidecar).unwrap_or_else(|| scoring_base.clone());
    if live_profile.name == "tuned" {
        info!(path = %sidecar.display(), "loaded tuned scoring profile from sidecar");
    }

    let state = AppState::new(
        k8s,
        campaign,
        c2_handle,
        armory,
        cfg.namespace_filter,
        live_profile,
        scoring_base,
        Some(sidecar),
        cfg.scoring.tuning_ui,
        "Ran".to_string(),
        initial_knowledge,
        campaign_events.clone(),
        cfg.plans_dir.clone(),
    );

    let campaign_entity_count = state
        .campaign
        .read()
        .map(|c| c.entity_count())
        .unwrap_or_default();
    let armory_count = state.armory.ttps().len();

    let mcp_parsers_dir = user_armory_dir
        .as_deref()
        .map(|d| d.parent().unwrap_or(d).join("parsers"))
        .filter(|p| p.is_dir());

    let mcp_config = api::McpConfig {
        campaign_events: campaign_events.clone(),
        parsers_dir: mcp_parsers_dir,
    };

    // Clone the state for the launch-time plan runner before the router takes
    // ownership of it. Cheap — AppState is a bundle of Arcs.
    let orchestrator_state = state.clone();

    let addr = SocketAddr::from(([127, 0, 0, 1], cfg.port));
    let app: Router =
        api::router_with_sse_and_mcp(state, mcp_config).fallback(api::frontend_handler);

    info!("starting emulate API server");
    info!(kubeconfig = %kubeconfig_path.display(), "using kubeconfig");
    info!(
        armory_dir = %user_armory_dir.as_deref().map(|p| p.display().to_string()).unwrap_or_else(|| "<bundled>".to_string()),
        armory_ttps = armory_count,
        "armory loaded"
    );
    info!(
        campaign_entities = campaign_entity_count,
        "campaign initialized"
    );
    info!(%addr, "listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Programmatic shutdown trigger, fired by the launch-time plan runner once
    // cleanup is done. Ctrl-C still works independently.
    let shutdown = Arc::new(tokio::sync::Notify::new());

    if let Some(plan_path) = cfg.plan.clone() {
        let events = campaign_events.clone();
        let trigger = shutdown.clone();
        tokio::spawn(run_launch_plan(
            orchestrator_state,
            events,
            plan_path,
            cfg.auto_cleanup,
            trigger,
        ));
    }

    let shutdown_fut = {
        let shutdown = shutdown.clone();
        async move {
            tokio::select! {
                _ = shutdown_signal() => {}
                _ = shutdown.notified() => info!("shutting down after plan cleanup"),
            }
        }
    };

    // Race the server against the shutdown signal. `with_graceful_shutdown` alone
    // is not enough: it stops accepting new connections but waits for *all* open
    // ones (including long-lived SSE streams) to drain before returning, so the
    // process would hang indefinitely when the browser is still connected. By
    // selecting directly, we drop the server — and close every connection — the
    // moment the signal fires.
    tokio::select! {
        result = axum::serve(listener, app) => { result?; }
        _ = shutdown_fut => { info!("server stopped"); }
    }

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("received shutdown signal");
}

/// Wait until cluster discovery has populated the campaign and stopped growing,
/// so a launch-time plan's first targets resolve. Bounded to ~20s.
async fn wait_for_discovery(state: &AppState) {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(20);
    let mut last = 0usize;
    let mut stable = 0u8;
    loop {
        let count = state.campaign.read().map(|c| c.entity_count()).unwrap_or(0);
        // Bootstrap starts with 2 entities (Cluster + C2). Consider discovery
        // settled only once at least one additional entity appears.
        if count > 2 && count == last {
            stable += 1;
            if stable >= 3 {
                break;
            }
        } else {
            stable = 0;
        }
        last = count;
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    info!(entities = last, "cluster discovery settled; launching plan");
}

/// Seed campaign Pod entities for the root steps of a plan (steps with no
/// `depends_on`). Only pods in the step's declared namespace whose names match
/// the step's target pattern are inserted — everything else stays undiscovered
/// so the emulation can find it organically.
/// Recursively collect files with a `.yaml`/`.yml` extension under `dir`,
/// appending their absolute paths to `out`. Symlinked directories are not
/// followed. Propagates the top-level read error (so a missing plans directory
/// surfaces as `NotFound`); errors reading nested subdirectories are logged and
/// skipped so one unreadable folder doesn't hide the rest.
fn collect_yaml_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if let Err(e) = collect_yaml_files(&path, out) {
                warn!(path = %path.display(), error = %e, "failed to read plans subdirectory; skipping");
            }
        } else if file_type.is_file() {
            let is_yaml = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
                .unwrap_or(false);
            if is_yaml {
                out.push(path);
            }
        }
    }
    Ok(())
}

/// Resolve a (possibly nested) plan filename against the plans directory,
/// rejecting anything that could escape it. Subdirectory separators are allowed;
/// absolute paths and `..` components are not.
fn resolve_plan_path(plans_dir: &std::path::Path, filename: &str) -> Result<PathBuf, ApiError> {
    use std::path::Component;
    let rel = PathBuf::from(filename);
    let invalid = filename.is_empty()
        || rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir));
    if invalid {
        return Err(ApiError::bad_request(format!(
            "invalid plan filename: {filename}"
        )));
    }
    Ok(plans_dir.join(&rel))
}

async fn seed_initial_access_targets(state: &AppState, plan: &planner::PlanDefinition) {
    // Collect unique (namespace, pattern) pairs from root steps.
    let root_targets: Vec<_> = plan
        .steps
        .iter()
        .filter(|s| s.depends_on.is_empty())
        .filter_map(|s| {
            let ns = s.target.namespace.as_deref().filter(|n| !n.is_empty())?;
            let kind = s.target.kind.to_ascii_lowercase();
            if !kind.is_empty() && kind != "pod" {
                return None; // only pod targets need seeding
            }
            let pattern = if s.target.name.is_empty() {
                ".*".to_string()
            } else {
                s.target.name.clone()
            };
            Some((s.id.clone(), ns.to_string(), pattern))
        })
        .collect();

    for (step_id, ns, pattern) in root_targets {
        // Build a fake entity-id list from cluster pods and use the planner's
        // resolver to match names — avoids a direct `regex` dep in this crate.
        let pods = match state.k8s.get_running_pods(Some(&ns)).await {
            Ok(p) => p,
            Err(e) => {
                warn!(step_id = %step_id, namespace = %ns, error = %e,
                    "failed to query cluster for initial target seed");
                continue;
            }
        };
        let candidate_ids: Vec<String> = pods
            .iter()
            .filter(|pod| pod.ready == Some(true))
            .map(|p| {
                let pod_ns = p.namespace.as_deref().unwrap_or(&ns);
                format!("ns/{}/pod/{}", pod_ns, p.name)
            })
            .collect();

        let query = planner::TargetQuery {
            namespace: Some(ns.clone()),
            name: pattern.clone(),
            select: Some(planner::SelectStrategy::All),
            ..Default::default()
        };
        let matched = planner::resolve_target(&query, &candidate_ids);

        if matched.is_empty() {
            warn!(step_id = %step_id, namespace = %ns, pattern = %pattern,
                "no cluster pods matched initial access target pattern");
            continue;
        }

        let mut campaign = match state.campaign.write() {
            Ok(c) => c,
            Err(_) => break,
        };

        for entity_id in &matched {
            // entity_id is "ns/<ns>/pod/<name>"
            let pod_name = entity_id.rsplit('/').next().unwrap_or_default();
            campaign.stage_initial_access_pod(pod_name, &ns);
            info!(step_id = %step_id, pod = %pod_name, namespace = %ns,
                "seeded initial access target from cluster");
        }
    }
}

enum PromptAnswer {
    Yes,
    No,
    Interrupted,
}

/// Prompt the operator on the terminal for a yes/no answer.
///
/// Returns `No` on EOF / non-interactive stdin.
async fn prompt_yes_no(prompt: &str) -> PromptAnswer {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut stdout = tokio::io::stdout();
    let _ = stdout.write_all(prompt.as_bytes()).await;
    let _ = stdout.flush().await;

    let mut line = String::new();
    let mut stdin_reader = BufReader::new(tokio::io::stdin());

    // Keep the prompt responsive to Ctrl-C so launch-time plan flows cannot
    // get stuck waiting on stdin after shutdown has already started.
    let read_result = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("cleanup prompt interrupted by shutdown signal");
            return PromptAnswer::Interrupted;
        }
        res = stdin_reader.read_line(&mut line) => res,
    };

    if read_result.unwrap_or(0) == 0 {
        return PromptAnswer::No;
    }

    if matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        PromptAnswer::Yes
    } else {
        PromptAnswer::No
    }
}

/// Run a plan supplied on the CLI: wait for discovery, execute it, wait for it
/// to finish, then offer/auto-run cleanup and trigger shutdown afterwards.
async fn run_launch_plan(
    state: AppState,
    events_bus: CampaignEventBus,
    plan_path: PathBuf,
    auto_cleanup: bool,
    shutdown: Arc<tokio::sync::Notify>,
) {
    let yaml = match std::fs::read_to_string(&plan_path) {
        Ok(y) => y,
        Err(e) => {
            error!(path = %plan_path.display(), error = %e, "failed to read plan file");
            return;
        }
    };

    // Parse the plan to find root steps and seed only their matching pods from
    // the live cluster. Everything else stays undiscovered until the emulation
    // finds it, preserving the discovery narrative.
    if let Ok(plan) = serde_yaml::from_str::<planner::PlanDefinition>(&yaml) {
        seed_initial_access_targets(&state, &plan).await;
    }

    wait_for_discovery(&state).await;

    // Subscribe before dispatching so we don't miss the completion event.
    let mut events = events_bus.subscribe();

    let plan_id = match state.execute_plan(yaml).await {
        Ok(id) => id,
        Err(e) => {
            error!(error = %e.body.error, "failed to start launch-time plan");
            return;
        }
    };
    info!(%plan_id, "launch-time plan started");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!(%plan_id, "launch-time plan runner interrupted by shutdown signal");
                return;
            }
            result = events.recv() => {
                match result {
                    Ok(CampaignEvent::PlanComplete { plan_id: done }) if done == plan_id => break,
                    Ok(_) => continue,
                    Err(_) => {
                        warn!("event bus closed before plan completed");
                        return;
                    }
                }
            }
        }
    }
    info!(%plan_id, "launch-time plan complete");

    let do_cleanup = if auto_cleanup {
        info!("--cleanup set: running cleanup automatically");
        true
    } else {
        match prompt_yes_no("\nPlan complete. Run cleanup and shut down? [y/N] ").await {
            PromptAnswer::Yes => true,
            PromptAnswer::No => false,
            PromptAnswer::Interrupted => {
                info!("cleanup prompt interrupted; shutting down emulation");
                shutdown.notify_one();
                return;
            }
        }
    };

    if !do_cleanup {
        info!("leaving emulation running for inspection; press Ctrl-C to stop");
        return;
    }

    if let Err(e) = state.run_cleanup().await {
        error!(error = %e.body.error, "cleanup failed; leaving emulation running");
        return;
    }

    info!("cleanup complete; stopping emulation");
    shutdown.notify_one();
}

/// Resolve the armory and the directory to search for external parsers.
///
/// Release builds (`bundled-armory`): built-in TTPs are always loaded; if
/// `armory_dir` is given, its TTPs are appended (union, same as Go).
///
/// Dev builds: loads exclusively from `armory_dir` or the default
/// `./armory/TTPs` fallback.
///
/// The returned `PathBuf` is the user directory (if any), used to locate the
/// sibling `parsers/` directory for external script parsers.
fn load_armory(armory_dir: Option<PathBuf>) -> Result<(Armory, Option<PathBuf>)> {
    #[cfg(not(feature = "bundled-armory"))]
    let resolved_dir = Some(armory_dir.unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_default()
            .join("armory")
            .join("TTPs")
    }));

    #[cfg(feature = "bundled-armory")]
    let resolved_dir = armory_dir;

    let armory = Armory::load(resolved_dir.as_deref())?;
    Ok((armory, resolved_dir))
}

// ---------------------------------------------------------------------------
// Trigger — atomic one-shot execution mode
// ---------------------------------------------------------------------------

/// Configuration for a single atomic TTP execution (`ran trigger`).
pub struct TriggerConfig {
    /// Path to kubeconfig file. Defaults to the standard kubeconfig location.
    pub kubeconfig: Option<PathBuf>,
    /// Path to the armory TTPs directory. Defaults to `./armory/TTPs`.
    pub armory_dir: Option<PathBuf>,
    /// Namespace visibility filter loaded from `ran.yaml`.
    pub namespace_filter: NamespaceFilter,
    pub seed_knowledge: Vec<SeedKnowledgeConfig>,
    /// TTP ID to execute (from the armory).
    pub action_id: String,
    /// Target entity ID in the form `ns/<namespace>/pod/<name>`.
    pub target_id: String,
    /// Override the execution system ID (optional).
    pub exec_system_id: Option<String>,
    /// Override the procedure ID (optional).
    pub procedure_id: Option<String>,
    /// TTP parameters as key=value pairs.
    pub args: std::collections::HashMap<String, String>,
}

/// Execute a single TTP atomically and print results with discovered facts.
/// Seeds the target pod into the campaign (equivalent to Go's godMode), runs
/// the full parser + analyzer + rules pipeline, then exits.
pub async fn trigger(cfg: TriggerConfig) -> Result<()> {
    let kubeconfig_path = kubeconfig_path_or_err(cfg.kubeconfig)?;
    let active_kubeconfig = resolve_kubeconfig(kubeconfig_path.clone(), None)?;
    let k8s = Client::from_resolved_kubeconfig(&active_kubeconfig).await?;
    let initial_knowledge = build_initial_knowledge(&active_kubeconfig, &cfg.seed_knowledge)?;
    let (armory, user_armory_dir) = load_armory(cfg.armory_dir)?;

    let external_parser: Option<Arc<dyn ExternalParser>> =
        user_armory_dir.as_deref().and_then(|ttps_dir| {
            let parsers_dir = ttps_dir.parent().unwrap_or(ttps_dir).join("parsers");
            parsers_dir
                .is_dir()
                .then(|| Arc::new(ScriptParserRunner::new(parsers_dir)) as Arc<dyn ExternalParser>)
        });

    let campaign = Arc::new(RwLock::new(Campaign::bootstrap_with_knowledge(
        "Ran",
        initial_knowledge,
    )));

    let (c2_handle, c2_events, c2_manager) = C2Manager::new(256, k8s);
    let campaign_events = CampaignEventBus::new(256);

    // Subscribe before spawning the processor so no events are dropped.
    let mut event_rx = campaign_events.subscribe();

    tokio::spawn(c2_manager.run());
    spawn_c2_event_processor_with_external_parser(
        campaign.clone(),
        c2_events,
        campaign_events,
        external_parser,
    );

    // Parse target ID and validate format.
    let (pod_namespace, pod_name) = parse_pod_target_id(&cfg.target_id)?;

    // Seed the target pod with a direct kubectl-exec channel from the C2 server.
    let pod_id = {
        let mut c = campaign
            .write()
            .map_err(|_| anyhow::anyhow!("campaign lock poisoned"))?;
        c.seed_pod_for_trigger(&pod_name, &pod_namespace)
    };

    info!(
        action_id = %cfg.action_id,
        target_id = %pod_id,
        kubeconfig = %kubeconfig_path.display(),
        armory_ttps = armory.ttps().len(),
        "triggering action"
    );

    // Prepare and dispatch the action.
    let exec = {
        let mut c = campaign
            .write()
            .map_err(|_| anyhow::anyhow!("campaign lock poisoned"))?;
        let exec = c
            .prepare_action(
                ExecuteActionRequest {
                    action_id: cfg.action_id.clone(),
                    target_id: pod_id.0.clone(),
                    exec_system_id: cfg.exec_system_id,
                    auth_identity_id: None,
                    procedure_id: cfg.procedure_id,
                    args: cfg.args,
                    reasoning: Some("cli trigger".to_string()),
                },
                &armory,
            )
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        c.add_open_step(exec.clone());
        exec
    };

    publish_ttp_dispatched(&exec);

    let cmd_id = exec.id.clone();
    let grounded_command = exec.procedure.command.clone();

    println!("Triggering {} on {}", cfg.action_id, pod_id);
    println!("Command: {}", grounded_command);

    c2_handle
        .send(exec)
        .await
        .map_err(|e| anyhow::anyhow!("failed to dispatch action: {}", e))?;

    // Phase 1: wait up to 60s for TtpExecuted with our cmd_id.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let result = loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            anyhow::bail!("timed out waiting for action result after 60s");
        }
        match tokio::time::timeout(remaining, event_rx.recv()).await {
            Ok(Ok(CampaignEvent::TtpExecuted {
                cmd_id: eid,
                results,
                success,
                fail_reason,
                ..
            })) if eid == cmd_id => {
                break TtpResult {
                    results,
                    success,
                    fail_reason,
                };
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => {
                anyhow::bail!("timed out waiting for action result after 60s");
            }
        }
    };

    // Phase 2: collect FactsChanged events for a short window. The processor
    // publishes them synchronously right after TtpExecuted, so 500ms is ample.
    let mut new_entities: Vec<EntitySummary> = Vec::new();
    let mut new_relations: Vec<RelationSummary> = Vec::new();
    let facts_deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    loop {
        let remaining = facts_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, event_rx.recv()).await {
            Ok(Ok(CampaignEvent::FactsChanged {
                cmd_id: eid,
                new_entities: ne,
                new_relations: nr,
            })) if eid == cmd_id => {
                new_entities.extend(ne);
                new_relations.extend(nr);
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => break,
        }
    }

    println!("\n--- Output ---");
    if result.results.is_empty() {
        println!("(no output)");
    } else {
        for line in &result.results {
            println!("{}", line);
        }
    }

    println!("\n--- Discovered Facts ---");
    // Filter out the seeded pod itself — it was already known.
    let discovered_entities: Vec<_> = new_entities.iter().filter(|e| e.id != pod_id).collect();
    println!("Entities ({}):", discovered_entities.len());
    for e in &discovered_entities {
        println!("  [{}] {}", e.kind, e.id);
    }
    println!("Relations ({}):", new_relations.len());
    for r in &new_relations {
        println!("  {} --{}--> {}", r.source_id, r.name, r.target_id);
    }

    if result.success {
        println!("\n✓ Success");
    } else {
        println!("\n✗ Failed: {}", result.fail_reason);
        std::process::exit(1);
    }

    Ok(())
}

struct TtpResult {
    results: Vec<String>,
    success: bool,
    fail_reason: String,
}

/// Parse `ns/<namespace>/pod/<name>` into `(namespace, name)`.
fn parse_pod_target_id(target_id: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = target_id.split('/').collect();
    if parts.len() == 4
        && parts[0] == "ns"
        && !parts[1].is_empty()
        && parts[2] == "pod"
        && !parts[3].is_empty()
    {
        return Ok((parts[1].to_string(), parts[3].to_string()));
    }
    anyhow::bail!(
        "invalid target format '{}'; expected ns/<namespace>/pod/<name>",
        target_id
    )
}

fn require_ready_initial_access_pod<'a>(
    candidates: &'a [k8s::RunningPod],
    target_id: &str,
) -> Result<&'a k8s::RunningPod, ExecuteActionError> {
    let candidate = candidates
        .iter()
        .find(|pod| pod.id == target_id)
        .ok_or_else(|| {
            ExecuteActionError::NotFound(format!(
                "live initial-access Pod '{}' was not found",
                target_id
            ))
        })?;
    if candidate.ready != Some(true) {
        return Err(ExecuteActionError::InvalidInput(format!(
            "live initial-access Pod '{}' is not ready",
            target_id
        )));
    }
    Ok(candidate)
}

#[cfg(test)]
mod initial_access_target_tests {
    use super::*;

    fn candidate(id: &str, ready: Option<bool>) -> k8s::RunningPod {
        k8s::RunningPod {
            id: id.to_string(),
            name: id.rsplit('/').next().unwrap_or_default().to_string(),
            namespace: Some("default".to_string()),
            phase: Some("Running".to_string()),
            ready,
            state_reason: None,
        }
    }

    #[test]
    fn selects_only_exact_ready_initial_access_candidate() {
        let pods = vec![
            candidate("ns/default/pod/not-ready", Some(false)),
            candidate("ns/default/pod/target", Some(true)),
        ];
        assert_eq!(
            require_ready_initial_access_pod(&pods, "ns/default/pod/target")
                .expect("exact ready Pod")
                .name,
            "target"
        );
        assert!(matches!(
            require_ready_initial_access_pod(&pods, "ns/default/pod/not-ready"),
            Err(ExecuteActionError::InvalidInput(_))
        ));
        assert!(matches!(
            require_ready_initial_access_pod(&pods, "ns/default/pod/missing"),
            Err(ExecuteActionError::NotFound(_))
        ));
    }

    #[test]
    fn rejects_non_pod_and_extra_path_target_ids() {
        assert!(parse_pod_target_id("k8s/cluster/demo").is_err());
        assert!(parse_pod_target_id("ns/default/pod/target/extra").is_err());
    }
}

/// Publish a `ttp-dispatched` SSE event the moment an action is enqueued, so the
/// UI can show it as in-progress before the completing `ttp-executed` arrives.
/// Field names mirror the `ttp-executed` payload so the frontend handlers stay
/// symmetric. Emitted from every dispatch path that registers an open step.
fn publish_ttp_dispatched(exec: &c2::ExecTtp) {
    let differs = !exec.exec_system_id.is_empty() && exec.exec_system_id != exec.target_id;
    api::publish_sse_event(
        "ttp-dispatched",
        serde_json::json!({
            "type": "ttp-dispatched",
            "data": {
                "ID": exec.id,
                "CmdId": exec.id,
                "TTP": exec.ttp,
                "Args": exec.args,
                "TargetID": exec.target_id,
                "ExecSystemID": if differs { exec.exec_system_id.clone() } else { String::new() },
                "StartedAtMs": exec.started_at_ms,
            },
        })
        .to_string(),
    );
}

async fn bridge_campaign_events_to_sse(mut campaign_rx: broadcast::Receiver<CampaignEvent>) {
    loop {
        match campaign_rx.recv().await {
            Ok(CampaignEvent::TtpExecuted {
                cmd_id,
                target_id,
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
                    "TargetID": target_id,
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
                cmd_id,
                new_entities,
                new_relations,
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

                for entity in &new_entities {
                    let category = match entity.kind.as_str() {
                        "Secret" | "K8sCredential" => "credential",
                        _ => "discovery",
                    };
                    api::publish_sse_event(
                        "entity-discovered",
                        serde_json::json!({
                            "type": "entity-discovered",
                            "data": {
                                "entityId": entity.id.0,
                                "entityName": entity.name,
                                "entityKind": entity.kind,
                                "category": category,
                                "cmdId": cmd_id,
                            },
                        })
                        .to_string(),
                    );
                }
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
            Ok(CampaignEvent::PlanStepDispatched {
                plan_id,
                step_id,
                exec_count,
            }) => {
                api::publish_sse_event(
                    "plan-step-dispatched",
                    serde_json::json!({
                        "type": "plan-step-dispatched",
                        "data": { "planId": plan_id, "stepId": step_id, "execCount": exec_count },
                    })
                    .to_string(),
                );
            }
            Ok(CampaignEvent::PlanStepCompleted {
                plan_id,
                step_id,
                success,
            }) => {
                api::publish_sse_event(
                    "plan-step-completed",
                    serde_json::json!({
                        "type": "plan-step-completed",
                        "data": { "planId": plan_id, "stepId": step_id, "success": success },
                    })
                    .to_string(),
                );
            }
            Ok(CampaignEvent::PlanStepSkipped {
                plan_id,
                step_id,
                reason,
            }) => {
                api::publish_sse_event(
                    "plan-step-skipped",
                    serde_json::json!({
                        "type": "plan-step-skipped",
                        "data": { "planId": plan_id, "stepId": step_id, "reason": reason },
                    })
                    .to_string(),
                );
            }
            Ok(CampaignEvent::PlanStepFailed {
                plan_id,
                step_id,
                reason,
            }) => {
                api::publish_sse_event(
                    "plan-step-failed",
                    serde_json::json!({
                        "type": "plan-step-failed",
                        "data": { "planId": plan_id, "stepId": step_id, "reason": reason },
                    })
                    .to_string(),
                );
            }
            Ok(CampaignEvent::PlanComplete { plan_id }) => {
                api::publish_sse_event(
                    "plan-complete",
                    serde_json::json!({
                        "type": "plan-complete",
                        "data": { "planId": plan_id },
                    })
                    .to_string(),
                );
            }
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                error!(
                    skipped,
                    "campaign SSE bridge lagged behind campaign event bus"
                );
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}
