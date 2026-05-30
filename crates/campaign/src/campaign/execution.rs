use std::collections::HashMap;

use armory::{Armory, Procedure, Ttp};
use c2::{ExecTtp, OutputTransform, TtpExecuted, BUILTIN_C2_ID};
use ran_domain::{BinaryPresence, EntityId, K8sNode, Merge, NameConfidence, Pod, UnknownSystem};
use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::analyzers::default_rules;
use crate::effects::{ground_template, parse_effect_with_status};
use crate::external_parser::SystemFieldUpdates;
use crate::failure_analyzers::{classify_failure, FAILURE_ANALYZER_EFFECT_ID};
use crate::grounding::{
    detect_ungrounded_vars, ground_args_from_context, ground_entity_ref_vars, resolve_template,
};
use crate::output_parsers::{build_no_parser_audit, build_parse_audit, parse_output_effect};
use crate::rules::run_rules_fixpoint;
use crate::shell_cmd::ground_binaries;
use crate::{FactsUpdate, ParseResult};

use crate::execution_record::ExecutionRecord;

use super::{
    Campaign, CampaignSystemEntityRef, ExecChannel, ExecuteActionError, ExecuteActionRequest,
    ExecuteActionResult, ExecutedActionEvent, TtpExecutionProcessing,
};

// ---------------------------------------------------------------------------
// Pipeline stage helpers (free functions)
// ---------------------------------------------------------------------------

fn validate_request(request: &ExecuteActionRequest) -> Result<(), ExecuteActionError> {
    if request.action_id.trim().is_empty() || request.target_id.trim().is_empty() {
        return Err(ExecuteActionError::InvalidInput(
            "actionId and targetId are required".to_string(),
        ));
    }
    Ok(())
}

fn resolve_ttp_and_defaults(
    action_id: &str,
    mut args: HashMap<String, String>,
    armory: &Armory,
) -> Result<(Ttp, HashMap<String, String>), ExecuteActionError> {
    let ttp = armory.get_ttp(action_id).cloned().ok_or_else(|| {
        ExecuteActionError::NotFound(format!("No TTP with ID '{}' found", action_id))
    })?;
    for p in &ttp.params {
        if !args.contains_key(&p.name) && !p.default.is_empty() {
            args.insert(p.name.clone(), p.default.clone());
        }
    }
    Ok((ttp, args))
}

/// Normalise the caller-supplied `exec_system_id` hint.
///
/// Treats missing, whitespace-only, or "same as target" values as unspecified
/// so that channel resolution can apply its own logic without special-casing
/// the common UI default of echoing the target ID back.
fn normalise_exec_hint(exec_system_id: Option<&str>, target_id: &str) -> Option<String> {
    exec_system_id
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != target_id)
        .map(str::to_string)
}

/// Ground the procedure command and all TTP effects with the collected args.
///
/// Emits a structured warning for every variable that remains unresolved after
/// all passes (except `${CMD}`, which is intentionally preserved as the
/// hop-injection slot).
///
/// Also mints `PROCEDURE_CMD` in `args`: the command template grounded with all
/// args *except* CMD, so that effect handlers (e.g. `rce.can-exec`) can read
/// the full executed-command string from the args context.
fn ground_procedure_and_effects(
    procedure: &mut Procedure,
    effects: &mut [String],
    args: &mut HashMap<String, String>,
    ttp_id: &str,
) {
    let envelope_args: HashMap<_, _> = args
        .iter()
        .filter(|(k, _)| k.to_uppercase() != "CMD")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let envelope = ground_template(&procedure.command, &envelope_args);
    args.entry("PROCEDURE_CMD".to_string()).or_insert(envelope);

    procedure.command = ground_template(&procedure.command, args);
    if let Some(http_req) = procedure.http_request.as_mut() {
        ground_json_value(http_req, args);
    }
    if let Some(k8s_req) = procedure.k8s_request.as_mut() {
        ground_json_value(k8s_req, args);
    }
    if let Some(steps) = procedure.steps.as_mut() {
        ground_json_value(steps, args);
    }
    for effect in effects.iter_mut() {
        *effect = ground_template(effect, args);
    }

    for var in detect_ungrounded_vars(&procedure.command)
        .into_iter()
        .filter(|v| v != "CMD")
    {
        tracing::warn!(
            ttp_id,
            var,
            "ungrounded variable in procedure command — \
             check TTP params or target entity context"
        );
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BoolOrString {
    Bool(bool),
    Str(String),
}

impl BoolOrString {
    fn is_true(&self) -> bool {
        match self {
            BoolOrString::Bool(b) => *b,
            BoolOrString::Str(s) => s.trim().eq_ignore_ascii_case("true"),
        }
    }
}

impl Default for BoolOrString {
    fn default() -> Self {
        BoolOrString::Bool(true)
    }
}

#[derive(Debug, Deserialize)]
struct HttpRequestSpec {
    url: String,
    #[serde(default = "default_http_method")]
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: String,
    #[serde(default = "default_timeout_seconds")]
    timeout_seconds: u64,
    #[serde(default)]
    use_ca: BoolOrString,
    #[serde(default)]
    ca_path: String,
    /// Save response body to this path; empty string means stdout.
    /// The alias `to` matches the `steps: [{fetch:}]` field name.
    #[serde(default, alias = "to")]
    output: String,
    #[serde(default)]
    follow_redirects: bool,
}

#[derive(Debug, Deserialize)]
struct KubernetesRequestSpec {
    api_server: String,
    api: String,
    resource: String,
    #[serde(default)]
    namespace: String,
    #[serde(default = "default_cluster_scoped")]
    cluster_scoped: BoolOrString,
    #[serde(default)]
    query: String,
    #[serde(default)]
    token: String,
    #[serde(default = "default_http_method")]
    method: String,
    #[serde(default)]
    use_ca: BoolOrString,
    #[serde(default)]
    ca_path: String,
    #[serde(default = "default_timeout_seconds")]
    timeout_seconds: u64,
}

fn build_k8s_url(spec: &KubernetesRequestSpec) -> String {
    let api_server = spec.api_server.trim_end_matches('/');
    let api = spec.api.trim_matches('/');
    let resource = spec.resource.trim_matches('/');

    let base = format!("{}/{}", api_server, api);

    let resource_path = if spec.cluster_scoped.is_true() || spec.namespace.trim().is_empty() {
        format!("{}/{}", base, resource)
    } else {
        format!("{}/namespaces/{}/{}", base, spec.namespace.trim(), resource)
    };

    if spec.query.trim().is_empty() {
        resource_path
    } else {
        format!("{}?{}", resource_path, spec.query.trim())
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StepSpec {
    Fetch { fetch: HttpRequestSpec },
    Chmod { chmod: String },
    Run { run: String },
}

/// Recursively apply template grounding to all string values inside a JSON value.
fn ground_json_value(val: &mut JsonValue, args: &HashMap<String, String>) {
    match val {
        JsonValue::String(s) => *s = ground_template(s, args),
        JsonValue::Object(map) => {
            for v in map.values_mut() {
                ground_json_value(v, args);
            }
        }
        JsonValue::Array(arr) => {
            for v in arr.iter_mut() {
                ground_json_value(v, args);
            }
        }
        _ => {}
    }
}

fn default_http_method() -> String {
    "GET".to_string()
}

fn default_timeout_seconds() -> u64 {
    30
}

fn default_cluster_scoped() -> BoolOrString {
    BoolOrString::Bool(false)
}

/// Render an HTTP request command using the tool TTP identified by `tool_id`.
///
/// Maps `spec` fields to the tool TTP's parameter names, renders the Tera
/// template from the tool's first procedure, and returns the fully grounded
/// shell command.  Returns `None` when the tool is not found in the armory.
fn render_http_via_tool(spec: &HttpRequestSpec, tool_id: &str, armory: &Armory) -> Option<String> {
    let tool_ttp = armory.get_tool_ttp(tool_id)?;
    let tool_proc = tool_ttp.procedures.first()?;

    let headers_json = serde_json::to_string(&spec.headers).unwrap_or_else(|_| "{}".to_string());

    let mut args: HashMap<String, String> = HashMap::new();
    args.insert("URL".to_string(), spec.url.clone());
    args.insert(
        "METHOD".to_string(),
        if spec.method.trim().is_empty() {
            "GET".to_string()
        } else {
            spec.method.trim().to_string()
        },
    );
    args.insert("HEADERS".to_string(), headers_json);
    args.insert("PAYLOAD".to_string(), spec.body.clone());
    args.insert(
        "TIMEOUT".to_string(),
        spec.timeout_seconds.max(1).to_string(),
    );
    args.insert("USE_CA".to_string(), spec.use_ca.is_true().to_string());
    args.insert("CA_PATH".to_string(), spec.ca_path.clone());
    args.insert("OUTPUT".to_string(), spec.output.clone());
    args.insert(
        "FOLLOW_REDIRECTS".to_string(),
        spec.follow_redirects.to_string(),
    );

    let rendered = resolve_template(&tool_proc.command, &args);
    Some(ground_template(&rendered, &args))
}

fn materialize_steps(procedure: &mut Procedure, armory: &Armory) -> Result<(), ExecuteActionError> {
    let steps_value = match procedure.steps.take() {
        Some(v) => v,
        None => return Ok(()),
    };

    let steps: Vec<StepSpec> = serde_json::from_value(steps_value).map_err(|e| {
        ExecuteActionError::InvalidInput(format!(
            "invalid steps in procedure '{}': {}",
            procedure.id, e
        ))
    })?;

    if steps.is_empty() {
        return Err(ExecuteActionError::InvalidInput(format!(
            "invalid steps in procedure '{}': empty steps list",
            procedure.id
        )));
    }

    let tool_id = procedure.tool.as_deref().unwrap_or("curl");

    let mut parts: Vec<String> = Vec::with_capacity(steps.len());
    for step in steps {
        match step {
            StepSpec::Fetch { fetch } => {
                let cmd = render_http_via_tool(&fetch, tool_id, armory).ok_or_else(|| {
                    ExecuteActionError::InvalidInput(format!(
                        "tool '{}' not found for fetch step in procedure '{}'",
                        tool_id, procedure.id
                    ))
                })?;
                parts.push(cmd);
            }
            StepSpec::Chmod { chmod } => {
                parts.push(format!("chmod {}", chmod.trim()));
            }
            StepSpec::Run { run } => {
                parts.push(run.trim().to_string());
            }
        }
    }

    procedure.command = parts.join(" && ");
    Ok(())
}

pub(super) fn materialize_k8s_request(
    procedure: &mut Procedure,
    armory: &Armory,
) -> Result<(), ExecuteActionError> {
    let k8s_req_val = match procedure.k8s_request.take() {
        Some(v) => v,
        None => return Ok(()),
    };

    let spec: KubernetesRequestSpec = serde_json::from_value(k8s_req_val).map_err(|e| {
        ExecuteActionError::InvalidInput(format!(
            "invalid k8s_request in procedure '{}': {}",
            procedure.id, e
        ))
    })?;

    let url = build_k8s_url(&spec);

    let mut headers = HashMap::new();
    headers.insert("Accept".to_string(), "application/json".to_string());
    if !spec.token.trim().is_empty() {
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", spec.token.trim()),
        );
    }

    let http_spec = HttpRequestSpec {
        url,
        method: if spec.method.trim().is_empty() {
            "GET".to_string()
        } else {
            spec.method.trim().to_string()
        },
        headers,
        body: String::new(),
        timeout_seconds: spec.timeout_seconds,
        use_ca: spec.use_ca,
        ca_path: spec.ca_path,
        output: String::new(),
        follow_redirects: false,
    };

    let tool_id = procedure.tool.as_deref().unwrap_or("curl");

    procedure.command = render_http_via_tool(&http_spec, tool_id, armory).ok_or_else(|| {
        ExecuteActionError::InvalidInput(format!(
            "tool '{}' not found for k8s_request in procedure '{}'",
            tool_id, procedure.id
        ))
    })?;

    Ok(())
}

fn materialize_abstract_http_request(
    procedure: &mut Procedure,
    armory: &Armory,
) -> Result<(), ExecuteActionError> {
    let http_req_val = match procedure.http_request.take() {
        Some(v) => v,
        None => return Ok(()),
    };

    let spec: HttpRequestSpec = serde_json::from_value(http_req_val).map_err(|e| {
        ExecuteActionError::InvalidInput(format!(
            "invalid http_request in procedure '{}': {}",
            procedure.id, e
        ))
    })?;

    let tool_id = procedure.tool.as_deref().unwrap_or("curl");

    procedure.command = render_http_via_tool(&spec, tool_id, armory).ok_or_else(|| {
        ExecuteActionError::InvalidInput(format!(
            "tool '{}' not found for http_request in procedure '{}'",
            tool_id, procedure.id
        ))
    })?;

    Ok(())
}

/// Route a Lateral Movement action to the pre-resolved execution source.
///
/// Lateral Movement TTPs CREATE the exec edge to the victim rather than
/// requiring one to exist; they run FROM the compromised source.  The channel
/// was resolved (and `SRC` injected) in [`Campaign::resolve_lateral_src`]
/// before grounding so that effect strings like
/// `rce.can-exec(${SRC}, ${TARGET_ID})` are fully grounded.
///
fn route_lateral_movement(
    lateral_src: Option<ExecChannel>,
    target_id: &str,
) -> Result<(String, String, Vec<String>, Option<OutputTransform>), ExecuteActionError> {
    let ch = lateral_src.ok_or_else(|| {
        ExecuteActionError::InvariantViolation(
            "lateral movement exec source should have been resolved before routing".to_string(),
        )
    })?;
    let exec_entity = ch
        .exec_target_id
        .clone()
        .unwrap_or_else(|| target_id.to_string());

    tracing::info!(
        target_id = %target_id,
        selected_source = %exec_entity,
        backend_id = %ch.backend_id,
        chain = %format_exec_chain(ch.backend_id.as_str(), &ch.hops, exec_entity.as_str()),
        "selected lateral-movement execution chain"
    );
    Ok((
        ch.backend_id,
        target_id.to_string(),
        vec![exec_entity.clone()],
        None,
    ))
}

fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Campaign impl — action preparation pipeline
// ---------------------------------------------------------------------------

impl Campaign {
    /// Prepare a TTP action for execution via a clean six-stage pipeline.
    ///
    /// ```text
    /// validate_request        — reject empty IDs immediately
    ///   → assert_target_exists  — target must be in the campaign
    ///   → resolve_ttp_and_defaults — TTP lookup + param default filling
    ///   → [delegate to prepare_action_with_ttp for stages 2-6]
    /// ```
    ///
    /// Each stage takes typed inputs and returns typed outputs; errors short-
    /// circuit via `?`.  `${SRC}` is injected exactly once (in
    /// `resolve_lateral_src`) so there is no risk of the two old injection sites
    /// conflicting.
    pub fn prepare_action(
        &mut self,
        request: ExecuteActionRequest,
        armory: &Armory,
    ) -> Result<ExecTtp, ExecuteActionError> {
        // Stage 1: validate inputs and look up static data.
        validate_request(&request)?;
        self.assert_target_exists(&request.target_id)?;
        let (ttp, args) = resolve_ttp_and_defaults(&request.action_id, request.args, armory)?;
        self.prepare_action_with_ttp(
            request.target_id,
            request.exec_system_id,
            request.procedure_id,
            ttp,
            args,
            armory,
        )
    }

    /// Internal pipeline: stages 2-6 of action preparation.
    ///
    /// Called by both `prepare_action` (normal attack steps) and
    /// `build_cleanup_actions` (synthesized cleanup TTPs).  Does NOT validate
    /// that the target entity is in the campaign — the caller is responsible
    /// for deciding whether to skip missing targets.
    pub(super) fn prepare_action_with_ttp(
        &mut self,
        target_id: String,
        exec_system_id: Option<String>,
        procedure_id: Option<String>,
        mut ttp: Ttp,
        mut args: HashMap<String, String>,
        armory: &Armory,
    ) -> Result<ExecTtp, ExecuteActionError> {
        // Fill param defaults that weren't already in args.
        for p in &ttp.params {
            if !args.contains_key(&p.name) && !p.default.is_empty() {
                args.insert(p.name.clone(), p.default.clone());
            }
        }

        // Stage 2: normalise the caller-supplied routing hint.
        let exec_hint = normalise_exec_hint(exec_system_id.as_deref(), &target_id);

        // Stage 3: inject context args (NS / NODE / TOKEN) from the target entity
        // before template substitution so cross-param references like `${NS}` in
        // arg defaults resolve correctly.
        ground_args_from_context(&mut args, &target_id, self);

        // Stage 4: resolve lateral-movement source and inject SRC — single,
        // authoritative site.  For non-lateral TTPs this is a no-op.
        let lateral_src = self.resolve_lateral_src(&ttp.tactic, exec_hint.as_deref(), &mut args)?;

        // For non-lateral TTPs resolve_lateral_src leaves SRC unset, but TTP
        // authors may still use ${SRC.PROP} to reference properties of the
        // executing entity (e.g. ${SRC.MOUNT_PATH} for host-path mounts).
        // For non-lateral TTPs the command runs ON the target, so SRC = target.
        if !args.contains_key("SRC") {
            args.insert("SRC".to_string(), target_id.clone());
        }

        // Stage 4.5: expand ${REF.PROP} placeholders in arg values now that SRC
        // and TARGET_ID are both present (e.g. ${SRC.MOUNT_PATH} → first host path,
        // ${TARGET.IP} → target entity's IP address).
        // TARGET_ID must be injected before ground_entity_ref_vars so that
        // ${TARGET.*} references in parameter defaults (e.g. CIDR = "${TARGET.IP}/24")
        // are resolved correctly.
        args.entry("TARGET_ID".to_string())
            .or_insert_with(|| target_id.clone());
        ground_entity_ref_vars(&mut args, self);

        // Stage 5: ground the procedure command and effects.
        let mut procedure = self.select_procedure(&ttp, procedure_id.as_deref())?;
        ground_procedure_and_effects(&mut procedure, &mut ttp.effects, &mut args, &ttp.id);
        materialize_k8s_request(&mut procedure, armory)?;
        materialize_steps(&mut procedure, armory)?;
        materialize_abstract_http_request(&mut procedure, armory)?;

        // Stage 6: resolve C2 channel (may wrap procedure.command for multi-hop).
        let (exec_system_id, target_id, exec_chain, output_transform) = self.route_exec_channel(
            &target_id,
            &ttp.tactic,
            &mut procedure,
            exec_hint.as_deref(),
            lateral_src,
        )?;

        Ok(ExecTtp {
            id: generate_cmd_id(),
            ttp,
            procedure,
            args,
            target_id,
            exec_chain,
            exec_system_id,
            started_at_ms: current_time_millis(),
            output_transform,
            is_cleanup: false,
        })
    }

    pub fn build_cleanup_actions(&mut self, armory: &Armory) -> Vec<ExecTtp> {
        let records: Vec<crate::execution_record::ExecutionRecord> = self.execution_records.clone();

        let mut actions = Vec::new();

        for record in records.iter().rev() {
            if record.is_cleanup {
                continue;
            }

            let Some(ttp) = armory.get_ttp(&record.ttp_id) else {
                continue;
            };

            let Some(ref cleanup_proc) = ttp.cleanup else {
                continue;
            };

            let cleanup_ttp = Ttp {
                description: ttp.description.clone(),
                techniques: ttp.techniques.clone(),
                status: ttp.status.clone(),
                params: ttp.params.clone(),
                requires: ttp.requires.clone(),
                procedures: vec![cleanup_proc.clone()],
                ..Ttp::new(
                    format!("{}_cleanup", ttp.id),
                    format!("{} Cleanup", ttp.name),
                    ttp.tactic.clone(),
                )
            };

            match self.prepare_action_with_ttp(
                record.target_id.clone(),
                None,
                Some(cleanup_proc.id.clone()),
                cleanup_ttp,
                record.args.clone(),
                armory,
            ) {
                Ok(mut exec) => {
                    exec.is_cleanup = true;
                    actions.push(exec);
                }
                Err(e) => {
                    tracing::warn!(
                        ttp_id = %record.ttp_id,
                        target_id = %record.target_id,
                        error = ?e,
                        "failed to build cleanup action; skipping"
                    );
                }
            }
        }

        actions
    }

    fn assert_target_exists(&self, target_id: &str) -> Result<(), ExecuteActionError> {
        let exists = self
            .get_entities()
            .into_iter()
            .any(|entity| entity.entity_id().0 == target_id);
        if !exists {
            return Err(ExecuteActionError::NotFound(format!(
                "failed to get target entity: {}",
                target_id
            )));
        }
        Ok(())
    }

    /// Resolve the lateral-movement execution source and inject `SRC`/`src`
    /// into `args`.
    ///
    /// This is the **single** place where `${SRC}` is injected for Lateral
    /// Movement TTPs.  The old code had two separate injection sites — one for a
    /// caller-supplied entity hint and one for the graph-resolved source — that
    /// could conflict when both conditions were true.  They are unified here.
    ///
    /// Returns the resolved [`ExecChannel`] so [`route_exec_channel`] can reuse
    /// it without hitting the graph a second time.  Returns `Ok(None)` for
    /// non-lateral TTPs.
    fn resolve_lateral_src(
        &self,
        tactic: &str,
        exec_hint: Option<&str>,
        args: &mut HashMap<String, String>,
    ) -> Result<Option<ExecChannel>, ExecuteActionError> {
        if !is_lateral_movement_tactic(tactic) {
            return Ok(None);
        }

        // Case A: caller explicitly nominated a source that is a known system entity.
        if let Some(hint) = exec_hint {
            if self.get_system_entity(hint).is_some() {
                args.insert("SRC".to_string(), hint.to_string());
                args.insert("src".to_string(), hint.to_string());
                return Ok(Some(ExecChannel {
                    backend_id: BUILTIN_C2_ID.to_string(),
                    exec_target_id: Some(hint.to_string()),
                    hops: vec![],
                }));
            }
        }

        // Case B: auto-resolve from the graph (no hint, or hint is not a known
        // system entity — treated as a backend ID, which doesn't give us a SRC).
        let ch = self
            .resolve_exec_source()
            .map_err(ExecuteActionError::NoExecChannel)?;
        if let Some(ref src_id) = ch.exec_target_id {
            args.insert("SRC".to_string(), src_id.clone());
            args.insert("src".to_string(), src_id.clone());
        }
        Ok(Some(ch))
    }

    /// Select a C2 backend and return `(backend_id, semantic_target_id, exec_chain, output_transform)`.
    ///
    /// - `semantic_target_id` is always the original `target_id` from the request — used for
    ///   attribution (execution records, effect context `TARGET_ID`, knowledge graph updates).
    /// - `exec_chain` is the ordered list of physical execution hops from the BuiltinC2 entry
    ///   point to the final destination.
    /// - `output_transform` is set when the channel wraps its output (e.g. ran-ws JSON envelope)
    ///   and the raw result must be post-processed before parsers run.
    ///
    /// Decision order (first matching branch wins):
    /// 1. Caller supplied a non-empty exec hint → [`route_caller_supplied`].
    /// 2. Lateral Movement tactic → [`route_lateral_movement`] (uses pre-resolved src).
    /// 3. Remote channel needed (tactic / procedure flag) → [`route_remote`].
    /// 4. Everything else → [`route_fallback`] (pod targets get in-cluster source).
    fn route_exec_channel(
        &mut self,
        target_id: &str,
        tactic: &str,
        procedure: &mut Procedure,
        exec_hint: Option<&str>,
        lateral_src: Option<ExecChannel>,
    ) -> Result<(String, String, Vec<String>, Option<OutputTransform>), ExecuteActionError> {
        if let Some(hint) = exec_hint.filter(|s| !s.trim().is_empty()) {
            return self.route_caller_supplied(hint, target_id);
        }

        if is_lateral_movement_tactic(tactic) {
            return route_lateral_movement(lateral_src, target_id);
        }

        if needs_remote_channel(procedure, tactic) {
            // Credential Access TTPs read container-filesystem paths (e.g. SA tokens
            // mounted at /var/run/secrets/...).  Active sessions may be running in host
            // namespace after a container escape, so force a fresh kubectl exec that
            // always targets the container's own mount namespace.
            let prefer_session = normalize_tactic(tactic) != "credential access";
            return self.route_remote(target_id, procedure, prefer_session);
        }

        self.route_fallback(target_id)
    }

    /// Route to a caller-supplied system entity or C2 backend ID.
    ///
    /// If the hint resolves to a known system entity, or looks like an entity ID
    /// (starts with `ns/` or `node/` — handles stale/merged pod references),
    /// it becomes the exec entity via the builtin C2.  Otherwise it is treated as
    /// an explicit C2 backend ID and the logical target is kept as the exec entity.
    ///
    fn route_caller_supplied(
        &self,
        hint: &str,
        target_id: &str,
    ) -> Result<(String, String, Vec<String>, Option<OutputTransform>), ExecuteActionError> {
        let hint_is_exec_entity = self.get_system_entity(hint).is_some()
            || hint.starts_with("ns/")
            || hint.starts_with("node/");

        if hint_is_exec_entity {
            tracing::info!(
                logical_target = %target_id,
                selected_source = %hint,
                backend_id = %BUILTIN_C2_ID,
                chain = %format_exec_chain(BUILTIN_C2_ID, &[], hint),
                "using caller-supplied exec source entity"
            );
            Ok((
                BUILTIN_C2_ID.to_string(),
                target_id.to_string(),
                vec![hint.to_string()],
                None,
            ))
        } else {
            tracing::info!(
                target_id = %target_id,
                backend_id = %hint,
                chain = %format_exec_chain(hint, &[], target_id),
                "using caller-supplied exec backend"
            );
            Ok((
                hint.to_string(),
                target_id.to_string(),
                vec![target_id.to_string()],
                None,
            ))
        }
    }

    /// Route through a graph-resolved exec channel, wrapping the command for
    /// any intermediate hops.
    ///
    /// The chain's first element is what BuiltinC2 directly execs into (first hop for
    /// multi-hop paths), and the last element is the final pod where the command runs.
    fn route_remote(
        &mut self,
        target_id: &str,
        procedure: &mut Procedure,
        prefer_session: bool,
    ) -> Result<(String, String, Vec<String>, Option<OutputTransform>), ExecuteActionError> {
        let ch = self
            .resolve_exec_channel_inner(target_id, prefer_session)
            .map_err(ExecuteActionError::NoExecChannel)?;
        let exec_target = ch
            .exec_target_id
            .clone()
            .unwrap_or_else(|| target_id.to_string());

        tracing::warn!(
            target_id = %target_id,
            exec_target = %exec_target,
            "resolved exec channel for action target",
        );
        tracing::info!("channel backend: {}, hops: {:?}", ch.backend_id, ch.hops);
        tracing::info!(
            target_id = %target_id,
            backend_id = %ch.backend_id,
            chain = %format_exec_chain(ch.backend_id.as_str(), &ch.hops, exec_target.as_str()),
            "selected remote execution chain"
        );

        if ch.hops.is_empty() {
            // Direct path: C2 can reach the target without any hop.
            // Ground all command-name occurrences against the target pod's binary
            // map so non-standard install paths (e.g. /tmp/kubectl) are used correctly.
            let tgt_id = EntityId::new(exec_target.as_str());
            if let Some(pod) = self.entities.find::<Pod>(&tgt_id) {
                procedure.command = ground_binaries(&procedure.command, &pod.system.binaries);
            }
            Ok((
                ch.backend_id,
                target_id.to_string(),
                vec![exec_target.clone()],
                None,
            ))
        } else {
            let output_transform =
                self.wrap_command_for_hops(procedure, &ch.hops, exec_target.as_str());
            let exec_chain: Vec<String> = ch
                .hops
                .iter()
                .cloned()
                .chain(std::iter::once(exec_target))
                .collect();
            Ok((
                ch.backend_id,
                target_id.to_string(),
                exec_chain,
                output_transform,
            ))
        }
    }

    /// Safety fallback: pod targets get an in-cluster execution source when no
    /// explicit channel was selected; all other targets get an empty backend
    /// (the C2 side will execute directly against the target).
    fn route_fallback(
        &self,
        target_id: &str,
    ) -> Result<(String, String, Vec<String>, Option<OutputTransform>), ExecuteActionError> {
        let target_eid = EntityId::new(target_id);
        if self.entities.contains::<Pod>(&target_eid) {
            tracing::warn!(
                target_id = %target_id,
                "no explicit exec channel selected for pod target; falling back to in-cluster source"
            );
            let ch = self
                .resolve_exec_source()
                .map_err(ExecuteActionError::NoExecChannel)?;
            let exec_entity = ch.exec_target_id.unwrap_or_else(|| target_id.to_string());
            tracing::info!(
                target_id = %target_id,
                selected_source = %exec_entity,
                backend_id = %ch.backend_id,
                chain = %format_exec_chain(ch.backend_id.as_str(), &ch.hops, exec_entity.as_str()),
                "pod fallback source selected"
            );
            Ok((
                ch.backend_id,
                target_id.to_string(),
                vec![exec_entity],
                None,
            ))
        } else {
            Ok((String::new(), target_id.to_string(), vec![], None))
        }
    }

    /// Wrap `procedure.command` through each hop in reverse order so BuiltinC2
    /// can exec into the first hop and the nested command traverses the rest of
    /// the chain to the final execution target.
    ///
    /// Returns the `OutputTransform` required to decode the raw output, if any.
    /// Currently only `kubelet-pod-exec` hops produce wrapped output (ran-ws JSON envelope).
    fn wrap_command_for_hops(
        &self,
        procedure: &mut Procedure,
        hops: &[String],
        exec_target: &str,
    ) -> Option<OutputTransform> {
        let full_chain: Vec<&str> = hops
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(exec_target))
            .collect();

        let mut output_transform: Option<OutputTransform> = None;

        // Wrap from innermost (last pair) to outermost (second pair;
        // hops[0] is handled by BuiltinC2 itself).
        for i in (1..full_chain.len()).rev() {
            let src = full_chain[i - 1];
            let tgt = full_chain[i];

            // Ground all command-name occurrences in the inner command against
            // the target system's binary map before embedding it in the envelope.
            if let Some(sys) = self.get_system_entity(tgt) {
                procedure.command =
                    ground_binaries(&procedure.command, &sys.entity().system().binaries);
            }

            let src_eid = EntityId::new(src);
            let tgt_eid_inner = EntityId::new(tgt);
            let found = self
                .graph
                .outgoing(&src_eid)
                .into_iter()
                .find(|(t, d)| *t == &tgt_eid_inner && d.is_exec_channel)
                .map(|(t, d)| ran_domain::RelationSummary {
                    name: d.relation_name.clone(),
                    source_id: src.to_string(),
                    target_id: t.0.clone(),
                    is_exec_channel: true,
                    envelope: d.envelope.clone(),
                    output_transform: d.output_transform.clone(),
                    weight: d.weight,
                    session_id: d.session_id.clone(),
                });
            procedure.command = match found {
                Some(ref rel) => {
                    if let Some(ref transform) = rel.output_transform {
                        output_transform = Some(transform.clone());
                    }
                    rel.wrap_command(&procedure.command)
                }
                None => {
                    // Fallback: try kubectl exec via target entity ID.
                    if let Some((ns, name)) = split_pod_entity_id(tgt) {
                        format!("kubectl exec -n {} {} -- {}", ns, name, procedure.command)
                    } else {
                        procedure.command.clone()
                    }
                }
            };

            // After wrapping, ground the outer tool (first word of the wrapped
            // command) against the source system's binary map.
            if let Some(sys) = self.get_system_entity(src) {
                procedure.command =
                    ground_binary_in_cmd(&procedure.command, &sys.entity().system().binaries);
            }
        }

        output_transform
    }

    pub fn on_ttp_executed(
        &mut self,
        cmd: &ExecTtp,
        event: &TtpExecuted,
    ) -> Result<TtpExecutionProcessing, ExecuteActionError> {
        let mut updates = FactsUpdate::default();
        let mut parse_audits = Vec::new();

        // If the channel wrapped its output (e.g. ran-ws JSON envelope from kubelet-pod-exec),
        // unwrap it here before any parser sees the result.
        let event_owned;
        let event: &TtpExecuted = if cmd.output_transform == Some(OutputTransform::JsonEnvelope)
            && event.success
            && !event.results.is_empty()
        {
            let raw = &event.results[0];
            let (unwrapped, err) = crate::output_parsers::unwrap_kubelet_json_response(raw);
            let mut patched = event.clone();
            patched.results[0] = unwrapped;
            if let Some(msg) = err {
                patched.success = false;
                patched.fail_reason = msg;
            }
            event_owned = patched;
            &event_owned
        } else {
            event
        };

        if !event.success {
            let classified = classify_failure(cmd, event);
            parse_audits.push(build_parse_audit(
                FAILURE_ANALYZER_EFFECT_ID,
                cmd,
                event,
                classified.parse_result,
                &classified.detail,
                0,
            ));

            // If the binary was not found, record it as Absent in the system's
            // binary map so the procedure selector can automatically fall back to
            // an alternative procedure next time.
            //
            // Prefer the name extracted from the error output (more reliable when
            // the procedure runs a wrapper that calls a different binary), then
            // fall back to the procedure's declared tool name.
            if classified.is_binary_missing {
                let binary = classified
                    .extracted_binary
                    .as_deref()
                    .or_else(|| procedure_binary_name(&cmd.procedure));

                if let Some(binary) = binary {
                    let system_id = cmd
                        .exec_chain
                        .iter()
                        .rev()
                        .map(String::as_str)
                        .find(|id| self.get_system_entity(id).is_some())
                        .or_else(|| {
                            let target_id_arg =
                                cmd.args.get("TARGET_ID").map(String::as_str).unwrap_or("");
                            self.get_system_entity(target_id_arg).map(|_| target_id_arg)
                        })
                        .or_else(|| {
                            self.get_system_entity(&cmd.target_id)
                                .map(|_| cmd.target_id.as_str())
                        });
                    if let Some(id) = system_id {
                        // Empty path → BinaryPresence::Absent; only written when
                        // currently Unknown (apply_system_update's existing guard).
                        let absent_update = SystemFieldUpdates {
                            binaries: std::collections::HashMap::from([(
                                binary.to_string(),
                                String::new(),
                            )]),
                            ..Default::default()
                        };
                        let _ = self.apply_system_update(id, &absent_update);
                    }
                }
            }

            self.parse_audits.extend(parse_audits.clone());
            self.execution_records
                .push(ExecutionRecord::from_execution(cmd, event));
            self.complete_open_step(&cmd.id);
            return Ok(TtpExecutionProcessing {
                updates,
                parse_audits,
                effective_success: false,
                effective_fail_reason: event.fail_reason.clone(),
            });
        }

        // Even when exit code is 0 some shells (busybox sh) swallow the real
        // exit status and emit "not found" into stdout/stderr instead.
        // Detect this before any inference so we don't incorrectly record the
        // tool as Present and immediately return a failure result.
        let early_missing = classify_failure(cmd, event);
        if early_missing.is_binary_missing {
            let binary = early_missing
                .extracted_binary
                .as_deref()
                .or_else(|| procedure_binary_name(&cmd.procedure));
            if let Some(binary) = binary {
                let system_id = cmd
                    .exec_chain
                    .iter()
                    .rev()
                    .map(String::as_str)
                    .find(|id| self.get_system_entity(id).is_some())
                    .or_else(|| {
                        let target_id_arg =
                            cmd.args.get("TARGET_ID").map(String::as_str).unwrap_or("");
                        self.get_system_entity(target_id_arg).map(|_| target_id_arg)
                    })
                    .or_else(|| {
                        self.get_system_entity(&cmd.target_id)
                            .map(|_| cmd.target_id.as_str())
                    });
                if let Some(id) = system_id {
                    let absent_update = SystemFieldUpdates {
                        binaries: std::collections::HashMap::from([(
                            binary.to_string(),
                            String::new(),
                        )]),
                        ..Default::default()
                    };
                    let _ = self.apply_system_update(id, &absent_update);
                }
            }
            let parse_audits = vec![build_parse_audit(
                FAILURE_ANALYZER_EFFECT_ID,
                cmd,
                event,
                early_missing.parse_result,
                &early_missing.detail,
                0,
            )];
            self.parse_audits.extend(parse_audits.clone());
            let mut record = ExecutionRecord::from_execution(cmd, event);
            record.success = false;
            record.fail_reason = early_missing.detail.clone();
            self.execution_records.push(record);
            self.complete_open_step(&cmd.id);
            return Ok(TtpExecutionProcessing {
                updates: FactsUpdate::default(),
                parse_audits,
                effective_success: false,
                effective_fail_reason: early_missing.detail,
            });
        }

        // Build the effect-parsing context: start with the TTP args and add
        // PROCEDURE_CMD so relation-effect handlers (e.g. rce.can-exec) that
        // need the executed command template can read it without special-casing.
        // TARGET_ID resolves the `sys` placeholder used in relation effects like
        // `k8s.kubelet-exec(sys, all(k8s.Node))` to the actual executing entity.
        let mut effect_ctx = cmd.args.clone();
        effect_ctx
            .entry("PROCEDURE_CMD".to_string())
            .or_insert_with(|| cmd.procedure.command.clone());
        effect_ctx
            .entry("TARGET_ID".to_string())
            .or_insert_with(|| cmd.target_id.clone());
        // TARGET_NODE_ID is the entity ID of the node the executing pod runs on.
        // Used by kubelet-exec and container.escape effects.
        // Resolution order:
        //   1. pod.node_name (set when the pod was parsed from the K8s API)
        //   2. runs-on graph edge from the pod (set when a RunsOn relation exists)
        if let Some(CampaignSystemEntityRef::Pod(pod)) = self.get_system_entity(&cmd.target_id) {
            let from_node_name = pod.node_name.is_some();
            let node_id = pod
                .node_name
                .as_ref()
                .map(|n| format!("node/{}", n))
                .or_else(|| {
                    let target_eid = EntityId::new(&cmd.target_id);
                    self.graph
                        .targets_of(&target_eid, ran_domain::RunsOn::RELATION_NAME)
                        .first()
                        .map(|n| n.0.clone())
                });
            if let Some(node_id) = node_id {
                effect_ctx
                    .entry("TARGET_NODE_ID".to_string())
                    .or_insert(node_id);
                if from_node_name {
                    effect_ctx
                        .entry("TARGET_NODE_AUTHORITATIVE".to_string())
                        .or_insert_with(|| "true".to_string());
                }
            }
        }

        for effect in &cmd.ttp.effects {
            if let Some(parsed_output) = parse_output_effect(self, effect, cmd, event) {
                updates.merge(parsed_output.updates);
                parse_audits.push(parsed_output.audit);
                continue;
            }

            match parse_effect_with_status(effect, &effect_ctx) {
                Ok(parsed_structural) if parsed_structural.handled => {
                    updates.merge(parsed_structural.updates);
                    parse_audits.push(build_parse_audit(
                        effect,
                        cmd,
                        event,
                        ParseResult::Parsed,
                        "parsed by structural effect handler",
                        0,
                    ));
                }
                Ok(_) => {
                    parse_audits.push(build_no_parser_audit(effect, cmd, event));
                }
                Err(err) => {
                    parse_audits.push(build_parse_audit(
                        effect,
                        cmd,
                        event,
                        ParseResult::ParserBug,
                        &err,
                        0,
                    ));
                }
            }
        }

        // Record binary presence before running the fixpoint so that rules like
        // KubeletExecSourceRule can see the updated binary map in campaign state.
        // Only records if currently Unknown — preserves more precise paths set by
        // sys.has-binary(${OUTPUT}) or from a real parser.
        if let Some(tool) = procedure_tool(&cmd.procedure) {
            let system_id = cmd
                .exec_chain
                .iter()
                .rev()
                .map(String::as_str)
                .find(|id| self.get_system_entity(id).is_some())
                .or_else(|| {
                    self.get_system_entity(&cmd.target_id)
                        .map(|_| cmd.target_id.as_str())
                });

            if let Some(id) = system_id {
                let already_known = self
                    .get_system_entity(id)
                    .map(|e| e.entity().system().has_binary(tool) != BinaryPresence::Unknown)
                    .unwrap_or(false);

                if !already_known {
                    let binary_updates = SystemFieldUpdates {
                        binaries: std::collections::HashMap::from([(
                            tool.to_string(),
                            tool.to_string(),
                        )]),
                        ..Default::default()
                    };
                    let _ = self.apply_system_update(id, &binary_updates);
                }
            }
        }

        // A successful command execution on a pod is direct evidence that the
        // pod is currently running. Emit an in-flight pod update before the
        // rule fixpoint so `KubeletExecSourceRule` can qualify it.
        let exec_system_id = cmd
            .exec_chain
            .iter()
            .rev()
            .map(String::as_str)
            .find(|id| self.get_system_entity(id).is_some())
            .or_else(|| {
                self.get_system_entity(&cmd.target_id)
                    .map(|_| cmd.target_id.as_str())
            });

        if let Some(system_id) = exec_system_id {
            if let Some(CampaignSystemEntityRef::Pod(pod)) = self.get_system_entity(system_id) {
                if !pod.is_running {
                    let mut running_pod = pod.clone();
                    running_pod.is_running = true;
                    updates.new_entities.push(Box::new(running_pod));
                }
            }
        }

        // Detect when a TTP ran against an IP-placeholder pod and the output
        // revealed the real pod identity (e.g. via a service-account token).
        // Record the alias so apply_facts can transplant all relations.
        self.detect_pod_identity_merge(cmd, &mut updates);

        let rules = default_rules();
        updates = run_rules_fixpoint(self, &rules, updates);

        self.apply_facts(&updates);
        self.parse_audits.extend(parse_audits.clone());

        // If a parser detected a semantic failure inside an otherwise-successful
        // transport response (e.g. Kubernetes API 403 Forbidden returned as HTTP
        // 200 with a Status body), override the recorded success flag so the
        // audit log and /api/flow reflect the real outcome.
        let api_error = parse_audits.iter().find(|a| {
            matches!(a.parse_result, ParseResult::KnownFailure)
                && a.detail.starts_with("K8s API error ")
        });
        let (effective_success, effective_fail_reason) = if let Some(err_audit) = api_error {
            let mut record = ExecutionRecord::from_execution(cmd, event);
            record.success = false;
            record.fail_reason = err_audit.detail.clone();
            self.execution_records.push(record);
            (false, err_audit.detail.clone())
        } else {
            self.execution_records
                .push(ExecutionRecord::from_execution(cmd, event));
            (event.success, event.fail_reason.clone())
        };
        self.complete_open_step(&cmd.id);

        Ok(TtpExecutionProcessing {
            updates,
            parse_audits,
            effective_success,
            effective_fail_reason,
        })
    }

    pub fn execute_action(
        &mut self,
        request: ExecuteActionRequest,
        armory: &Armory,
    ) -> Result<ExecuteActionResult, ExecuteActionError> {
        let exec = self.prepare_action(request, armory)?;
        Ok(ExecuteActionResult {
            cmd_id: exec.id.clone(),
            event: ExecutedActionEvent {
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

    fn apply_facts(&mut self, updates: &FactsUpdate) {
        for entity in &updates.new_entities {
            self.insert_entity(entity.as_ref());
        }

        // Merge entity aliases: transplant graph edges and entity data.
        // Runs after insert_entity so the preferred node already exists.
        for (stale_id, preferred_id) in &updates.entity_aliases {
            // Graph: retarget all edges from stale → preferred.
            self.graph.merge_entities(preferred_id, stale_id);
            // Entity maps: merge runtime data (IPs, access level, binaries, etc.).
            // Dispatch to the correct merge function based on entity kind.
            if stale_id.0.starts_with("system/") {
                // UnknownSystem → Pod or Node cross-type merge.
                self.merge_unknown_into_system(&preferred_id.0, &stale_id.0);
            } else if preferred_id.0.starts_with("node/") || stale_id.0.starts_with("node/") {
                self.merge_node_entities(&preferred_id.0, &stale_id.0);
            } else {
                self.merge_pod_entities(&preferred_id.0, &stale_id.0);
            }
        }

        for rel in &updates.new_relations {
            // Resolve stale entity IDs before inserting into the graph.
            let (src, tgt) = updates.entity_aliases.iter().fold(
                (rel.source_id().clone(), rel.target_id().clone()),
                |(src, tgt), (stale, preferred)| {
                    let src = if src == *stale {
                        preferred.clone()
                    } else {
                        src
                    };
                    let tgt = if tgt == *stale {
                        preferred.clone()
                    } else {
                        tgt
                    };
                    (src, tgt)
                },
            );

            // runs-on: when the new node differs, pick the preferred one and
            // merge the stale node entity into it.
            if rel.relation_name() == "runs-on" {
                let existing_node = self
                    .graph
                    .targets_of(&src, "runs-on")
                    .first()
                    .cloned()
                    .cloned();

                if let Some(old_node) = existing_node {
                    if old_node != tgt {
                        // Prefer the node whose name is Authoritative; fall back to
                        // keeping the existing node when both have equal confidence.
                        let old_confidence = self
                            .entities
                            .find::<K8sNode>(&old_node)
                            .map(|n| n.name_confidence)
                            .unwrap_or(NameConfidence::Derived);
                        let tgt_confidence = self
                            .entities
                            .find::<K8sNode>(&tgt)
                            .map(|n| n.name_confidence)
                            .unwrap_or(NameConfidence::Derived);
                        let preferred_node = if tgt_confidence == NameConfidence::Authoritative
                            && old_confidence != NameConfidence::Authoritative
                        {
                            tgt.clone()
                        } else {
                            old_node.clone()
                        };
                        let stale_node = if preferred_node == old_node {
                            tgt.clone()
                        } else {
                            old_node
                        };
                        self.graph.merge_entities(&preferred_node, &stale_node);
                        self.merge_node_entities(&preferred_node.0, &stale_node.0);
                        // Insert edge to preferred node (graph PodSingleNode
                        // invariant removes the old runs-on automatically).
                        self.insert_relation_with_ids(&src, &preferred_node, rel.as_ref());
                        continue;
                    }
                    // Same node — nothing to do (PodSingleNode invariant will
                    // replace the edge anyway, but we skip the insert).
                    continue;
                }
            }

            // Common path: no alias resolution changed the IDs — use the
            // public `insert_relation` so it gets a live production call site.
            if src == *rel.source_id() && tgt == *rel.target_id() {
                self.insert_relation(rel.as_ref());
            } else {
                self.insert_relation_with_ids(&src, &tgt, rel.as_ref());
            }
        }
    }

    /// Insert a relation into the graph using explicit (possibly alias-resolved)
    /// source and target IDs rather than the relation's own stored IDs.
    pub(super) fn insert_relation_with_ids(
        &mut self,
        src: &EntityId,
        tgt: &EntityId,
        rel: &dyn ran_domain::Relation,
    ) {
        use cortex::edge_data_for;
        let summary = ran_domain::RelationSummary::from_relation(rel);
        let data = edge_data_for(
            rel.relation_name(),
            summary.envelope,
            summary.output_transform,
        );
        self.graph.insert_edge(src, tgt, data);

        // When a C2 channel relation is added to a system entity, ensure
        // access_level reflects at least Exec so the field stays consistent.
        if rel.is_exec_channel() {
            if let Some(pod) = self.entities.find_mut::<Pod>(tgt) {
                if pod.system.access_level == ran_domain::AccessLevel::None {
                    pod.system.access_level = ran_domain::AccessLevel::Exec;
                }
            }
            if let Some(node) = self.entities.find_mut::<K8sNode>(tgt) {
                if node.system.access_level == ran_domain::AccessLevel::None {
                    node.system.access_level = ran_domain::AccessLevel::Exec;
                }
            }
        }
    }

    fn merge_node_entities(&mut self, preferred_id: &str, stale_id: &str) {
        if preferred_id == stale_id {
            return;
        }

        let preferred = EntityId::new(preferred_id);
        let stale = EntityId::new(stale_id);

        let Some(stale_node) = self.entities.get_mut::<K8sNode>().remove(&stale) else {
            return;
        };

        if let Some(preferred_node) = self.entities.find_mut::<K8sNode>(&preferred) {
            preferred_node.merge_from(&stale_node);
        } else {
            self.entities
                .get_mut::<K8sNode>()
                .insert(preferred, stale_node);
        }
    }

    /// Merge a stale (placeholder) pod into the preferred (real-named) pod.
    ///
    /// All facts accumulated on the stale entity are folded into the preferred
    /// one via `Pod::merge_from`.  The stale entity is removed regardless.
    fn merge_pod_entities(&mut self, preferred_id: &str, stale_id: &str) {
        if preferred_id == stale_id {
            return;
        }

        let preferred = EntityId::new(preferred_id);
        let stale = EntityId::new(stale_id);

        let Some(stale_pod) = self.entities.get_mut::<Pod>().remove(&stale) else {
            return;
        };

        if let Some(preferred_pod) = self.entities.find_mut::<Pod>(&preferred) {
            preferred_pod.merge_from(&stale_pod);
        } else {
            // Preferred entity not yet in the campaign (shouldn't happen in the
            // normal flow, but handle gracefully by keeping the stale data).
            self.entities.get_mut::<Pod>().insert(preferred, stale_pod);
        }
    }

    /// Merge an `UnknownSystem` into its now-identified Pod or Node counterpart.
    ///
    /// Called when `IpBasedSystemMergeAnalyzer` matched the two by a shared IP.
    /// The stale `UnknownSystem`'s `SystemInfo` (IPs, binaries, access level,
    /// sessions, etc.) is folded into the preferred entity, then the
    /// `UnknownSystem` slot is cleared.
    fn merge_unknown_into_system(&mut self, preferred_id: &str, stale_id: &str) {
        if preferred_id == stale_id {
            return;
        }

        let stale = EntityId::new(stale_id);
        let preferred = EntityId::new(preferred_id);

        let Some(stale_unknown) = self.entities.get_mut::<UnknownSystem>().remove(&stale) else {
            return;
        };

        if preferred_id.starts_with("node/") {
            if let Some(node) = self.entities.find_mut::<K8sNode>(&preferred) {
                node.system.merge_from(&stale_unknown.system);
            }
        } else if let Some(pod) = self.entities.find_mut::<Pod>(&preferred) {
            pod.system.merge_from(&stale_unknown.system);
        }
    }

    /// Detect when a TTP ran against a derived-name pod and the output
    /// revealed the real pod identity (e.g. from a service-account token).
    ///
    /// A "derived-name" pod is one whose `name_confidence` is [`NameConfidence::Derived`] —
    /// for example a pod whose name was inferred from its IP address during a
    /// network scan.  When a subsequent TTP produces a `Pod` entity whose name
    /// is [`NameConfidence::Authoritative`] (e.g. from a service-account JWT),
    /// this function records an alias `(stale_id, preferred_id)` in `updates`
    /// so that `apply_facts` can transplant all relations to the real entity.
    fn detect_pod_identity_merge(&self, cmd: &ExecTtp, updates: &mut FactsUpdate) {
        // For multi-hop TTPs the C2 sets `cmd.target_id` to the first hop (the
        // pod it kubectl-execs into), NOT the logical target of the TTP.  The
        // original request target is always preserved in args["TARGET_ID"].
        let logical_target = cmd
            .args
            .get("TARGET_ID")
            .map(String::as_str)
            .unwrap_or(&cmd.target_id);

        let stale_id = EntityId::new(logical_target);

        // Only proceed when the execution target is a pod with a derived name.
        let Some(exec_pod) = self.entities.find::<Pod>(&stale_id) else {
            return;
        };
        if exec_pod.meta.name_confidence == NameConfidence::Authoritative {
            return;
        }

        let ns = exec_pod.meta.namespace.as_deref().unwrap_or("");
        if ns.is_empty() {
            return;
        }
        let ns_pod_prefix = format!("ns/{}/pod/", ns);

        // Strategy 1: a new Pod entity with an authoritative name appeared in updates.
        let preferred_from_entity = updates
            .new_entities
            .iter()
            .find(|e| {
                if e.entity_kind() != "Pod" {
                    return false;
                }
                let id = e.entity_id();
                if id == stale_id {
                    return false;
                }
                if !id.0.starts_with(&ns_pod_prefix) {
                    return false;
                }
                // Accept any pod in updates whose name is authoritative.
                e.name_confidence() == NameConfidence::Authoritative
            })
            .map(|e| e.entity_id());

        // Strategy 2: a `uses` relation from an authoritative pod appeared in
        // updates (SA token analysis won't re-emit the pod entity if already known).
        let preferred_from_relation = if preferred_from_entity.is_none() {
            updates
                .new_relations
                .iter()
                .filter(|r| r.relation_name() == "uses")
                .map(|r| r.source_id().clone())
                .find(|id| {
                    *id != stale_id
                        && id.0.starts_with(&ns_pod_prefix)
                        && self
                            .entities
                            .find::<Pod>(id)
                            .map(|p| p.meta.name_confidence == NameConfidence::Authoritative)
                            .unwrap_or(false)
                })
        } else {
            None
        };

        let Some(preferred_id) = preferred_from_entity.or(preferred_from_relation) else {
            return;
        };

        tracing::info!(
            stale = %stale_id.0,
            preferred = %preferred_id.0,
            "merging IP-placeholder pod with discovered real pod identity"
        );
        updates.entity_aliases.insert((stale_id, preferred_id));
    }

    fn select_procedure(
        &self,
        ttp: &Ttp,
        procedure_id: Option<&str>,
    ) -> Result<Procedure, ExecuteActionError> {
        if let Some(proc_id) = procedure_id {
            return ttp
                .procedures
                .iter()
                .find(|p| p.id == proc_id)
                .cloned()
                .ok_or_else(|| {
                    ExecuteActionError::InvalidInput(format!(
                        "procedure '{}' not found for action '{}'",
                        proc_id, ttp.id
                    ))
                });
        }

        ttp.procedures.first().cloned().ok_or_else(|| {
            ExecuteActionError::InvalidInput(format!("No procedure found for action '{}'", ttp.id))
        })
    }
}

/// Parse a pod entity ID in the canonical form `ns/<namespace>/pod/<name>` and
/// return `(namespace, pod_name)`, or `None` if the format doesn't match.
/// Used to build the inner `kubectl exec` when routing via an intermediate pod.
fn split_pod_entity_id(entity_id: &str) -> Option<(&str, &str)> {
    let mut parts = entity_id.splitn(5, '/');
    let kind_a = parts.next()?;
    let namespace = parts.next()?;
    let kind_b = parts.next()?;
    let pod_name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if kind_a != "ns" || kind_b != "pod" || namespace.is_empty() || pod_name.is_empty() {
        return None;
    }
    Some((namespace, pod_name))
}

/// Returns `true` when the tactic creates a new execution edge rather than
/// requiring one to exist.  For these tactics the command is run FROM an
/// already-compromised source, not TO the target.
fn is_lateral_movement_tactic(tactic: &str) -> bool {
    normalize_tactic(tactic) == "lateral movement"
}

/// Returns `true` when the procedure requires a remote execution channel.
///
/// Local commands (`is_local_command = true`) and operator-side tactics
/// (Reconnaissance, Resource Development) run on the C2 side and do not
/// need a channel to a target system.
fn needs_remote_channel(procedure: &Procedure, tactic: &str) -> bool {
    if procedure.is_local_command == Some(true) {
        return false;
    }
    !matches!(
        normalize_tactic(tactic).as_str(),
        "reconnaissance" | "resource development"
    )
}

fn normalize_tactic(tactic: &str) -> String {
    tactic
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_exec_chain(backend_id: &str, hops: &[String], exec_target: &str) -> String {
    let mut parts: Vec<String> = vec![backend_id.to_string()];
    parts.extend(hops.iter().cloned());
    if parts.last().map(|p| p.as_str()) != Some(exec_target) {
        parts.push(exec_target.to_string());
    }
    parts.join(" -> ")
}

fn generate_cmd_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("cmd-{}", millis)
}

/// Return the tool name for a procedure, if one is set and non-empty.
/// Matches Go's `Procedure.GetTool()`.
fn procedure_tool(procedure: &Procedure) -> Option<&str> {
    procedure.tool.as_deref().filter(|t| !t.trim().is_empty())
}

/// Return the name of the binary a procedure invokes, for use when recording
/// binary presence/absence.
///
/// Resolution order:
/// 1. `procedure.tool` — explicit annotation (e.g. `tool: cat`)
/// 2. `procedure.id` — when it is a single bare word (e.g. key `nmap`, `curl`)
/// 3. First word of `procedure.command` — final fallback
fn procedure_binary_name(procedure: &Procedure) -> Option<&str> {
    if let Some(tool) = procedure_tool(procedure) {
        return Some(tool);
    }

    // Use the procedure ID only when it looks like a bare binary name
    // (no spaces, no path separators).
    let id = procedure.id.trim();
    if !id.is_empty() && !id.contains(' ') && !id.contains('/') {
        return Some(id);
    }

    // Fall back to the first word of the command.
    procedure.command.split_whitespace().next()
}

/// Resolve the first word of `cmd` using a system's binary map.
///
/// If the first word of `cmd` is a known binary with a `Present` path that
/// differs from the bare name (e.g. `kubectl` → `/tmp/kubectl`), the first
/// occurrence of the bare name is replaced with the resolved path.
///
/// Words that already contain `/` are skipped — they are already absolute
/// paths and do not need further resolution.
///
/// Mirrors Go's `groundUsedTool` in `campaign/campaign.go`.
fn ground_binary_in_cmd(
    cmd: &str,
    binaries: &std::collections::HashMap<String, ran_domain::BinaryPresence>,
) -> String {
    use ran_domain::BinaryPresence;

    let first_word = match cmd.split_whitespace().next() {
        Some(w) => w,
        None => return cmd.to_string(),
    };

    // Already an absolute/relative path — nothing to resolve.
    if first_word.contains('/') {
        return cmd.to_string();
    }

    if let Some(BinaryPresence::Present(path)) = binaries.get(first_word) {
        if !path.is_empty() && path.as_str() != first_word {
            // Replace the first occurrence of `first_word` in `cmd`, which is
            // guaranteed to be at a word boundary since it is the first token.
            if let Some(pos) = cmd.find(first_word) {
                let mut result = cmd.to_string();
                result.replace_range(pos..pos + first_word.len(), path.as_str());
                return result;
            }
        }
    }

    cmd.to_string()
}
