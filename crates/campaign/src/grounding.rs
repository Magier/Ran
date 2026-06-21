//! TTP argument and command grounding.
//!
//! Grounding resolves template variables in TTP commands and effect strings:
//!
//! 1. **Context resolution** ([`ground_args_from_context`]) — fills special
//!    parameter names (NS, POD_NAME, NODE, RANDOM) from the campaign's
//!    knowledge of the target entity.
//!
//! 2. **Entity-reference property expansion** ([`ground_entity_ref_vars`]) —
//!    expands `${REF.PROP}` placeholders (e.g. `${SRC.MOUNT_PATH}`) using the
//!    entity whose ID is stored in the corresponding arg (`SRC`, `TARGET_ID`).
//!    Must run after `SRC` is injected (Stage 4 of the execution pipeline).
//!
//! 3. **Tera template rendering** ([`resolve_template`]) — evaluates
//!    `{% if VAR %}...{% else %}...{% endif %}` blocks and `{{ VAR }}`
//!    substitutions that appear in procedure commands.
//!
//! 4. **`${KEY}` substitution** (already in [`crate::effects::ground_template`])
//!    — replaces `${KEY}` with the corresponding arg value.
//!
//! 5. **Ungrounded variable detection** ([`detect_ungrounded_vars`]) — scans
//!    the final command for any `${…}` patterns that were not resolved, so
//!    callers can log a warning.

use std::collections::HashMap;

use ran_domain::{Entity, EntityId, Pod, ServiceAccount};
use serde_json::Value as JsonValue;

use crate::campaign::{Campaign, CampaignEntityRef};

// ---------------------------------------------------------------------------
// Context-aware argument resolution
// ---------------------------------------------------------------------------

/// Fill in context-derived values for the well-known special parameters.
///
/// | Key (case-insensitive match) | Resolution |
/// |------------------------------|------------|
/// | `NS` / `NAMESPACE`           | Target entity's namespace. Only replaced when the current value is empty or exactly `"${NS}"` / `"${NAMESPACE}"`. |
/// | `POD_NAME` / `PODNAME`       | Target entity's name. Replaces the `${POD_NAME}` token wherever it appears in the value. |
/// | `NODE` / `NODENAME` / `NODE_NAME` | The pod's scheduled node name. Only replaced when the current value is empty or exactly `"${NODE_NAME}"`. |
/// | `TOKEN`                      | ServiceAccount reference → raw JWT. Accepts SA entity ID (`ns/<ns>/sa/<name>`), SA name (resolved in target namespace), or empty value (resolved from target pod/SA). Raw JWT values are preserved as-is. |
/// | `API_SERVER`                 | Empty or template-var → `https://kubernetes.default.svc`. |
/// | *(any)*                      | Any value containing `${RANDOM}` is replaced with a 5-digit pseudo-random number. |
///
/// Call this **before** [`crate::effects::ground_template`] so that
/// cross-parameter `${KEY}` references in other arguments resolve correctly
/// after this pass.
pub fn ground_args_from_context(
    args: &mut HashMap<String, String>,
    target_id: &str,
    campaign: &Campaign,
) {
    let target = campaign
        .get_entities()
        .into_iter()
        .find(|e| e.entity_id().0 == target_id);

    let target_ns = target.as_ref().and_then(entity_namespace);
    let target_name = target.as_ref().map(|e| e.entity_name().to_string());
    let target_node = target.as_ref().and_then(pod_node_name);

    for (key, value) in args.iter_mut() {
        match key.to_ascii_uppercase().as_str() {
            "NS" | "NAMESPACE"
                if value.is_empty() || value == "${NS}" || value == "${NAMESPACE}" =>
            {
                *value = target_ns.clone().unwrap_or_default();
            }
            "NS" | "NAMESPACE" => {}
            "POD_NAME" | "PODNAME" if value.contains("${POD_NAME}") => {
                let name = target_name.as_deref().unwrap_or("ran");
                *value = value.replace("${POD_NAME}", name);
            }
            "POD_NAME" | "PODNAME" => {}
            "NODE" | "NODENAME" | "NODE_NAME" if value.is_empty() || value == "${NODE_NAME}" => {
                *value = target_node.clone().unwrap_or_default();
            }
            "NODE" | "NODENAME" | "NODE_NAME" => {}
            "TOKEN" => {
                if let Some(raw) = resolve_token_arg(value, target.as_ref(), campaign) {
                    *value = raw;
                }
            }
            "API_SERVER" if value.is_empty() || value == "${API_SERVER}" => {
                *value = "https://kubernetes.default.svc".to_string();
            }
            "API_SERVER" => {}
            _ => {}
        }

        if value.contains("${RANDOM}") {
            *value = value.replace("${RANDOM}", &random_id());
        }
    }

    // Inject well-known defaults for keys that may not be declared as TTP
    // parameters but are referenced in procedure commands.
    args.entry("API_SERVER".to_string())
        .or_insert_with(|| "https://kubernetes.default.svc".to_string());

    // If TOKEN was not a declared parameter at all, try to resolve it from the
    // target entity so that curl-style procedures get a bearer token.
    if !args.contains_key("TOKEN") {
        if let Some(raw) = resolve_token_from_target(target.as_ref(), campaign) {
            args.insert("TOKEN".to_string(), raw);
        }
    }
}

fn entity_namespace(entity: &CampaignEntityRef) -> Option<String> {
    entity.namespace().map(str::to_string)
}

fn pod_node_name(entity: &CampaignEntityRef) -> Option<String> {
    match entity {
        CampaignEntityRef::Pod(pod) => pod.node_name.clone(),
        _ => None,
    }
}

fn resolve_token_arg(
    value: &str,
    target: Option<&CampaignEntityRef>,
    campaign: &Campaign,
) -> Option<String> {
    let trimmed = value.trim();

    // Already a raw JWT provided by the caller; keep it unchanged.
    if looks_like_jwt(trimmed) {
        return Some(trimmed.to_string());
    }

    // 1) Explicit SA entity ID in arg value.
    if !trimmed.is_empty() {
        if let Some(raw) = sa_token_by_entity_id(campaign, trimmed) {
            return Some(raw);
        }

        // 2) SA name in target namespace.
        if let Some(ns) = target.and_then(|t| t.namespace()) {
            let guessed_id = format!("ns/{}/sa/{}", ns, trimmed);
            if let Some(raw) = sa_token_by_entity_id(campaign, guessed_id.as_str()) {
                return Some(raw);
            }
        }

        tracing::warn!(
            token_ref = trimmed,
            "TOKEN arg could not be resolved to a ServiceAccount token"
        );
        return None;
    }

    // 3) Empty TOKEN defaults to the target's ServiceAccount token when known.
    resolve_token_from_target(target, campaign)
}

fn looks_like_jwt(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|p| !p.is_empty())
}

fn sa_token_by_entity_id(campaign: &Campaign, sa_entity_id: &str) -> Option<String> {
    let sa_id = EntityId::new(sa_entity_id);
    let sa = campaign.entities.find::<ServiceAccount>(&sa_id)?;
    let raw = sa.raw_token()?;
    Some(raw.to_string())
}

fn resolve_token_from_target(
    target: Option<&CampaignEntityRef>,
    campaign: &Campaign,
) -> Option<String> {
    let target = target?;
    match target {
        CampaignEntityRef::ServiceAccount(sa) => sa_token_by_entity_id(campaign, &sa.entity_id().0),
        CampaignEntityRef::Pod(pod) => {
            let ns = pod.namespace()?;
            let sa_name = pod.service_account_name.as_deref()?;
            let sa_id = format!("ns/{}/sa/{}", ns, sa_name);
            sa_token_by_entity_id(campaign, &sa_id)
        }
        _ => None,
    }
}

/// A reproducible-ish 5-digit pseudo-random ID derived from nanosecond time.
fn random_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(42_000);
    format!("{:05}", nanos % 100_000)
}

// ---------------------------------------------------------------------------
// Entity-reference property expansion  (${SRC.PROP}, ${TARGET.PROP}, …)
// ---------------------------------------------------------------------------

/// Expand `${<REF>.<PROP>}` placeholders embedded in arg values.
///
/// Runs **after** `SRC` and `TARGET_ID` have been injected into `args` so that
/// the entity IDs are already present.  The following references are supported:
///
/// | REF           | Entity looked up             |
/// |---------------|------------------------------|
/// | `SRC`         | `args["SRC"]` entity ID      |
/// | `TARGET`      | `args["TARGET_ID"]` entity ID|
///
/// | PROP          | Resolution                               |
/// |---------------|------------------------------------------|
/// | `MOUNT_PATH`  | First host-path on a pod (`host_paths`)  |
/// | `IP`          | First IP address on the entity's system  |
/// | `NAME`        | Entity name                              |
/// | `NS` / `NAMESPACE` | Entity namespace                    |
/// | `NODE`        | Scheduled node name (pods only)          |
///
/// When a property resolves to multiple values (e.g. several host paths), the
/// first is used and a warning is logged so the operator knows a choice was
/// made automatically.  Unresolvable references are left as-is.
pub fn ground_entity_ref_vars(args: &mut HashMap<String, String>, campaign: &Campaign) {
    let refs: Vec<(&'static str, String)> = [("SRC", "SRC"), ("TARGET", "TARGET_ID")]
        .iter()
        .filter_map(|(ref_name, arg_key)| args.get(*arg_key).cloned().map(|id| (*ref_name, id)))
        .collect();

    if refs.is_empty() {
        return;
    }

    for value in args.values_mut() {
        for (ref_name, entity_id) in &refs {
            expand_entity_props(value, ref_name, entity_id, campaign);
        }
    }
}

/// Replace all `${REF.PROP}` occurrences in `value` in-place.
fn expand_entity_props(value: &mut String, ref_name: &str, entity_id: &str, campaign: &Campaign) {
    let prefix = format!("${{{ref_name}.");

    if !value.contains(&prefix) {
        return;
    }

    let entity_id = EntityId::new(entity_id);
    let entity = campaign
        .get_entities()
        .into_iter()
        .find(|e| e.entity_id() == entity_id);

    let mut result = String::with_capacity(value.len());
    let mut remaining = value.as_str();

    while let Some(start) = remaining.find(&prefix) {
        result.push_str(&remaining[..start]);
        let after_prefix = &remaining[start + prefix.len()..];
        if let Some(end) = after_prefix.find('}') {
            let prop = &after_prefix[..end];
            let resolved = entity.as_ref().and_then(|e| resolve_entity_prop(e, prop));
            match resolved {
                Some(v) => result.push_str(&v),
                None => {
                    // Leave the original placeholder intact.
                    result.push_str(&prefix);
                    result.push_str(prop);
                    result.push('}');
                }
            }
            remaining = &after_prefix[end + 1..];
        } else {
            // Malformed placeholder — keep as-is.
            result.push_str(&prefix);
            remaining = after_prefix;
        }
    }
    result.push_str(remaining);
    *value = result;
}

/// Resolve a named property from an entity reference.
///
/// Returns `None` when the property is not applicable to the entity kind or
/// the value is unavailable.
fn resolve_entity_prop(entity: &CampaignEntityRef, prop: &str) -> Option<String> {
    match prop.to_ascii_uppercase().as_str() {
        "NAME" => Some(entity.entity_name().to_string()),
        "NS" | "NAMESPACE" => entity.namespace().map(str::to_string),
        "IP" => match entity {
            CampaignEntityRef::Pod(p) => p.system.ips.first().map(|ip| ip.to_string()),
            CampaignEntityRef::Node(n) => n.system.ips.first().map(|ip| ip.to_string()),
            _ => None,
        },
        "NODE" => match entity {
            CampaignEntityRef::Pod(p) => p.node_name.clone(),
            _ => None,
        },
        "MOUNT_PATH" => match entity {
            CampaignEntityRef::Pod(p) => resolve_mount_path(p),
            _ => None,
        },
        _ => None,
    }
}

fn resolve_mount_path(pod: &Pod) -> Option<String> {
    let mount_points = pod.host_path_mount_points();
    match mount_points.len() {
        0 => None,
        1 => Some(mount_points[0].to_string()),
        n => {
            tracing::warn!(
                pod = %pod.entity_name(),
                count = n,
                paths = ?mount_points,
                "multiple host paths found; using the first one"
            );
            Some(mount_points[0].to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Tera template rendering
// ---------------------------------------------------------------------------

/// Render a Tera template with the given args as context variables.
///
/// TTP procedure commands use [Tera](https://keats.github.io/tera/docs/) syntax:
///
/// | Syntax                              | Meaning                            |
/// |-------------------------------------|------------------------------------|
/// | `{% if VAR %}...{% endif %}`        | Include the block when `VAR` is truthy |
/// | `{% if not VAR %}...{% endif %}`    | Include the block when `VAR` is falsy  |
/// | `{% else %}`                        | Switch to the alternative branch   |
/// | `{{ VAR }}`                         | Substitute with `args["VAR"]`      |
///
/// Bool-like string values (`"true"`, `"yes"`, `"1"` / `"false"`, `"no"`,
/// `"0"`) are coerced to actual booleans in the context so that
/// `{% if VAR %}` evaluates correctly for flag parameters.
///
/// Undefined variables silently render as empty / evaluate as false.
/// `${KEY}` dollar-placeholders are **not** processed here — they pass
/// through unchanged and are substituted in the second grounding pass
/// in [`crate::effects::ground_template`].
pub fn resolve_template(template: &str, args: &HashMap<String, String>) -> String {
    let mut tera = tera::Tera::default();

    if let Err(e) = tera.add_raw_template("__ttp__", template) {
        tracing::warn!(error = %e, "Tera template parse failed; returning raw template");
        return template.to_string();
    }

    let mut context = tera::Context::new();

    // Pre-insert `false` for every variable referenced in the template that is
    // absent from `args`, so optional parameters evaluate as falsy rather than
    // causing a "variable not found" error.
    for var_name in collect_tera_var_refs(template) {
        if !args.contains_key(&var_name) {
            context.insert(&var_name, &false);
        }
    }

    // Insert actual arg values, coercing bool-like strings to real booleans so
    // that `{% if FLAG %}` evaluates correctly when FLAG = "false" / "true".
    for (key, value) in args {
        let trimmed = value.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => context.insert(key, &true),
            "false" | "no" | "0" => context.insert(key, &false),
            _ => {
                // Array/object defaults are stored as JSON strings in armory.
                // Decode them so templates can iterate collections via Tera.
                if (trimmed.starts_with('{') || trimmed.starts_with('['))
                    && serde_json::from_str::<JsonValue>(trimmed)
                        .map(|json| context.insert(key, &json))
                        .is_ok()
                {
                    continue;
                }
                context.insert(key, value)
            }
        }
    }

    match tera.render("__ttp__", &context) {
        Ok(result) => {
            // YAML `>-` folded block scalars with extra-indented continuation lines
            // preserve newlines (treating those lines as "literal"), so the template
            // arrives here as a multiline string even though it was authored as a
            // single logical command.  Tera also leaves blank lines behind where
            // `{% if %}` / `{% endif %}` tags were removed.  Collapse the result to
            // a single space-joined line so that `sh -c` receives one command.
            let collapsed: String = result
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            collapsed
        }
        Err(e) => {
            tracing::warn!(error = %e, "Tera template rendering failed; returning raw template");
            template.to_string()
        }
    }
}

/// Scan a Tera template for `{{ VAR }}` output tags and `{% if [not] VAR %}`
/// control tags and return the referenced variable names.
///
/// Used to pre-populate the Tera context with falsy defaults so that optional
/// TTP parameters that are absent from `args` evaluate gracefully.
fn collect_tera_var_refs(template: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let s = template.as_bytes();
    let len = s.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && s[i] == b'{' {
            if s[i + 1] == b'{' {
                // {{ ... }} — variable output tag
                let start = i + 2;
                if let Some(rel) = template[start..].find("}}") {
                    let inner = template[start..start + rel].trim();
                    if !inner.is_empty()
                        && inner
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    {
                        vars.push(inner.to_string());
                    }
                    i = start + rel + 2;
                    continue;
                }
            } else if s[i + 1] == b'%' {
                // {% ... %} — control tag
                let start = i + 2;
                if let Some(rel) = template[start..].find("%}") {
                    let inner = template[start..start + rel].trim();
                    if let Some(rest) = inner.strip_prefix("if ") {
                        let cond = rest.trim();
                        let var_name = cond.strip_prefix("not ").unwrap_or(cond).trim();
                        if !var_name.is_empty()
                            && var_name
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        {
                            vars.push(var_name.to_string());
                        }
                    }
                    i = start + rel + 2;
                    continue;
                }
            }
        }
        i += 1;
    }

    vars
}

// ---------------------------------------------------------------------------
// Ungrounded variable detection
// ---------------------------------------------------------------------------

/// Return the names of any `${VAR}` placeholders still present in `cmd`.
///
/// Call this after all grounding passes and log a warning for each entry —
/// an ungrounded variable usually means a missing required argument.
pub fn detect_ungrounded_vars(cmd: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let bytes = cmd.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            let start = i + 2;
            if let Some(rel) = cmd[start..].find('}') {
                vars.push(cmd[start..start + rel].to_string());
                i = start + rel + 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    vars
}

/// Fraction of a TTP's **required** parameters that current campaign state can
/// supply without the operator guessing, in `[0, 1]`.
///
/// For each required param we seed its declared default, ground it against the
/// campaign + target (the same context the real execution uses), then count it
/// *ready* when the result is non-empty with no residual `${VAR}`. A param the
/// campaign can't fill (e.g. an `nmap` CIDR that hasn't been discovered) stays
/// ungrounded and drags the score down. A TTP with no required params is fully
/// ready (`1.0`).
pub fn input_readiness(ttp: &armory::Ttp, target_id: &str, campaign: &Campaign) -> f32 {
    let required: Vec<&armory::TtpParam> = ttp.params.iter().filter(|p| p.required).collect();
    if required.is_empty() {
        return 1.0;
    }

    // Seed args from declared defaults, plus TARGET_ID so entity-ref vars
    // (e.g. `${TARGET.IP}`) can resolve during grounding.
    let mut args: HashMap<String, String> = required
        .iter()
        .map(|p| (p.name.clone(), p.default.clone()))
        .collect();
    args.insert("TARGET_ID".to_string(), target_id.to_string());

    ground_args_from_context(&mut args, target_id, campaign);
    ground_entity_ref_vars(&mut args, campaign);

    let ready = required
        .iter()
        .filter(|p| {
            args.get(&p.name)
                .map(|v| !v.trim().is_empty() && detect_ungrounded_vars(v).is_empty())
                .unwrap_or(false)
        })
        .count();

    ready as f32 / required.len() as f32
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ran_domain::{Entity, JwToken, K8sCluster, Pod, ServiceAccountToken};

    use crate::campaign::Campaign;

    fn pod_in_ns(name: &str, ns: &str) -> Pod {
        Pod::new(name, ns)
    }

    fn pod_on_node(name: &str, ns: &str, node: &str) -> Pod {
        let mut p = Pod::new(name, ns);
        p.node_name = Some(node.to_string());
        p
    }

    // ------------------------------------------------------------------
    // ground_args_from_context
    // ------------------------------------------------------------------

    #[test]
    fn ground_args_fills_ns_from_pod() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = pod_in_ns("demo", "staging");
        let target_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        let mut args = HashMap::from([("NS".to_string(), "${NS}".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["NS"], "staging");
    }

    #[test]
    fn ground_args_preserves_explicit_ns() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = pod_in_ns("demo", "staging");
        let target_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        let mut args = HashMap::from([("NS".to_string(), "production".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["NS"], "production");
    }

    #[test]
    fn ground_args_fills_pod_name() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = pod_in_ns("nginx", "default");
        let target_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        let mut args =
            HashMap::from([("PodName".to_string(), "${POD_NAME}-${RANDOM}".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        let val = &args["PodName"];
        assert!(
            val.starts_with("nginx-"),
            "expected 'nginx-...', got '{}'",
            val
        );
        assert!(!val.contains("${POD_NAME}"));
        assert!(!val.contains("${RANDOM}"));
    }

    #[test]
    fn ground_args_fills_node_from_pod() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = pod_on_node("runner", "default", "worker-1");
        let target_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        let mut args = HashMap::from([("NodeName".to_string(), "${NODE_NAME}".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["NodeName"], "worker-1");
    }

    #[test]
    fn ground_args_replaces_random_in_any_value() {
        let campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut args = HashMap::from([("Label".to_string(), "ran-${RANDOM}".to_string())]);
        ground_args_from_context(&mut args, "nonexistent", &campaign);

        let val = &args["Label"];
        assert!(val.starts_with("ran-"), "got '{}'", val);
        assert!(!val.contains("${RANDOM}"));
    }

    #[test]
    fn ground_args_resolves_token_from_sa_entity_id() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut sa = ran_domain::ServiceAccount::new("reader", "default");
        sa.token = Some(ServiceAccountToken {
            jwt: JwToken {
                raw: "ey.aa.bb".to_string(),
                ..Default::default()
            },
            service_account_name: "reader".to_string(),
            namespace: "default".to_string(),
            pod_name: None,
            pod_uid: None,
            service_account_uid: None,
            is_bound: false,
        });
        let sa_id = sa.entity_id();
        campaign
            .entities
            .get_mut::<ServiceAccount>()
            .insert(sa_id.clone(), sa);

        let mut args = HashMap::from([("TOKEN".to_string(), sa_id.0)]);
        ground_args_from_context(&mut args, "nonexistent", &campaign);

        assert_eq!(args["TOKEN"], "ey.aa.bb");
    }

    #[test]
    fn ground_args_resolves_empty_token_from_target_pod_service_account() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut pod = Pod::new("runner", "default");
        pod.service_account_name = Some("runner-sa".to_string());
        let target_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        let mut sa = ran_domain::ServiceAccount::new("runner-sa", "default");
        sa.token = Some(ServiceAccountToken {
            jwt: JwToken {
                raw: "ey.pod.token".to_string(),
                ..Default::default()
            },
            service_account_name: "runner-sa".to_string(),
            namespace: "default".to_string(),
            pod_name: None,
            pod_uid: None,
            service_account_uid: None,
            is_bound: false,
        });
        campaign.entities.insert_typed(sa);

        let mut args = HashMap::from([("TOKEN".to_string(), "".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["TOKEN"], "ey.pod.token");
    }

    #[test]
    fn ground_args_preserves_raw_jwt_token_value() {
        let campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut args = HashMap::from([("TOKEN".to_string(), "header.payload.sig".to_string())]);
        ground_args_from_context(&mut args, "nonexistent", &campaign);

        assert_eq!(args["TOKEN"], "header.payload.sig");
    }

    #[test]
    fn ground_args_resolves_token_from_sa_name_in_target_namespace() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = Pod::new("runner", "default");
        let target_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        let mut sa = ran_domain::ServiceAccount::new("reader", "default");
        sa.token = Some(ServiceAccountToken {
            jwt: JwToken {
                raw: "ey.name.ns".to_string(),
                ..Default::default()
            },
            service_account_name: "reader".to_string(),
            namespace: "default".to_string(),
            pod_name: None,
            pod_uid: None,
            service_account_uid: None,
            is_bound: false,
        });
        campaign.entities.insert_typed(sa);

        let mut args = HashMap::from([("TOKEN".to_string(), "reader".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["TOKEN"], "ey.name.ns");
    }

    #[test]
    fn ground_args_resolves_empty_token_from_target_service_account_entity() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut sa = ran_domain::ServiceAccount::new("auditor", "kube-system");
        sa.token = Some(ServiceAccountToken {
            jwt: JwToken {
                raw: "ey.sa.target".to_string(),
                ..Default::default()
            },
            service_account_name: "auditor".to_string(),
            namespace: "kube-system".to_string(),
            pod_name: None,
            pod_uid: None,
            service_account_uid: None,
            is_bound: false,
        });
        let target_id = sa.entity_id().0.clone();
        campaign.entities.insert_typed(sa);

        let mut args = HashMap::from([("TOKEN".to_string(), "".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["TOKEN"], "ey.sa.target");
    }

    #[test]
    fn ground_args_keeps_unresolvable_token_reference_unchanged() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = Pod::new("runner", "default");
        let target_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        let mut args = HashMap::from([("TOKEN".to_string(), "missing-sa".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["TOKEN"], "missing-sa");
    }

    #[test]
    fn ground_args_injects_api_server_when_not_a_declared_param() {
        // TTPs that use ${API_SERVER} in their curl commands may not declare it
        // as an explicit parameter.  It must still be grounded to the default.
        let campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut args: HashMap<String, String> = HashMap::new(); // no API_SERVER key
        ground_args_from_context(&mut args, "nonexistent", &campaign);

        assert_eq!(args["API_SERVER"], "https://kubernetes.default.svc");
    }

    #[test]
    fn ground_args_injects_token_when_not_a_declared_param() {
        // If TOKEN is absent from args entirely (not declared as a TTP param)
        // but the target pod has an SA token, it should still be injected.
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut pod = Pod::new("runner", "default");
        pod.service_account_name = Some("runner-sa".to_string());
        let target_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        let mut sa = ran_domain::ServiceAccount::new("runner-sa", "default");
        sa.token = Some(ServiceAccountToken {
            jwt: JwToken {
                raw: "ey.injected.token".to_string(),
                ..Default::default()
            },
            service_account_name: "runner-sa".to_string(),
            namespace: "default".to_string(),
            pod_name: None,
            pod_uid: None,
            service_account_uid: None,
            is_bound: false,
        });
        campaign.entities.insert_typed(sa);

        let mut args: HashMap<String, String> = HashMap::new(); // no TOKEN key
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["TOKEN"], "ey.injected.token");
    }

    // ------------------------------------------------------------------
    // resolve_template
    // ------------------------------------------------------------------

    #[test]
    fn tera_template_if_true_includes_block() {
        let args = HashMap::from([("FLAG".to_string(), "true".to_string())]);
        let result = resolve_template("before{% if FLAG %}INSIDE{% endif %}after", &args);
        assert_eq!(result, "beforeINSIDEafter");
    }

    #[test]
    fn tera_template_if_false_excludes_block() {
        let args = HashMap::from([("FLAG".to_string(), "false".to_string())]);
        let result = resolve_template("before{% if FLAG %}INSIDE{% endif %}after", &args);
        assert_eq!(result, "beforeafter");
    }

    #[test]
    fn tera_template_else_branch_taken_when_false() {
        let args = HashMap::from([("ALL_NS".to_string(), "false".to_string())]);
        let result = resolve_template("{% if ALL_NS %}all{% else %}single{% endif %}", &args);
        assert_eq!(result, "single");
    }

    #[test]
    fn tera_template_else_branch_skipped_when_true() {
        let args = HashMap::from([("ALL_NS".to_string(), "true".to_string())]);
        let result = resolve_template("{% if ALL_NS %}all{% else %}single{% endif %}", &args);
        assert_eq!(result, "all");
    }

    #[test]
    fn tera_template_if_not_includes_when_falsy() {
        let args = HashMap::from([("CLUSTER_ROLE".to_string(), "false".to_string())]);
        let result = resolve_template(
            "kubectl{% if not CLUSTER_ROLE %} -n=${NS}{% endif %}",
            &args,
        );
        assert_eq!(result, "kubectl -n=${NS}");
    }

    #[test]
    fn tera_template_substitutes_variable() {
        let args = HashMap::from([("PodName".to_string(), "my-pod".to_string())]);
        let result = resolve_template("name={{ PodName }}", &args);
        assert_eq!(result, "name=my-pod");
    }

    #[test]
    fn tera_template_mixed_with_dollar_syntax() {
        let args = HashMap::from([
            ("PodName".to_string(), "my-pod".to_string()),
            ("NS".to_string(), "default".to_string()),
        ]);
        // Tera {{ }} for variable output, ${} style for dollar substitution
        let template = "{{ PodName }}-${NS}";
        let result = resolve_template(template, &args);
        // resolve_tera_template handles {{ }} only; ${} is handled by ground_template
        assert_eq!(result, "my-pod-${NS}");
    }

    #[test]
    fn tera_template_supports_json_object_iteration() {
        let args = HashMap::from([(
            "HEADERS".to_string(),
            r#"{"Metadata-Flavor":"Google","Authorization":"Bearer token"}"#.to_string(),
        )]);
        let template =
            r#"curl{% for name, value in HEADERS %} -H "{{ name }}: {{ value }}"{% endfor %}"#;
        let result = resolve_template(template, &args);

        assert!(result.starts_with("curl"));
        assert!(result.contains(r#"-H "Metadata-Flavor: Google""#));
        assert!(result.contains(r#"-H "Authorization: Bearer token""#));
    }

    #[test]
    fn tera_multiline_template_collapses_to_single_line() {
        // serde_yaml `>-` with extra-indented continuation lines preserves
        // newlines, so commands arrive as multiline strings.  Tera also leaves
        // blank lines where {% if %} tags were removed.  Both should be collapsed
        // to a single space-joined line.
        let args = HashMap::from([("USE_CA".to_string(), "false".to_string())]);
        let template = "wget -O-\n  --header=\"Auth\"\n  {% if USE_CA %}\n  --ca-certificate=/ca.crt\n  {% else %}\n  --no-check-certificate\n  {% endif %}\n  https://example.com";
        let result = resolve_template(template, &args);
        assert_eq!(
            result,
            "wget -O- --header=\"Auth\" --no-check-certificate https://example.com"
        );
    }

    #[test]
    fn tera_multiline_template_collapses_if_true_branch() {
        let args = HashMap::from([("USE_CA".to_string(), "true".to_string())]);
        let template = "wget -O-\n  --header=\"Auth\"\n  {% if USE_CA %}\n  --ca-certificate=/ca.crt\n  {% else %}\n  --no-check-certificate\n  {% endif %}\n  https://example.com";
        let result = resolve_template(template, &args);
        assert_eq!(
            result,
            "wget -O- --header=\"Auth\" --ca-certificate=/ca.crt https://example.com"
        );
    }

    // ------------------------------------------------------------------
    // detect_ungrounded_vars
    // ------------------------------------------------------------------

    #[test]
    fn detect_ungrounded_finds_remaining_vars() {
        let vars = detect_ungrounded_vars("kubectl get pods --token=${TOKEN} -n=${NS}");
        assert_eq!(vars, vec!["TOKEN", "NS"]);
    }

    #[test]
    fn detect_ungrounded_returns_empty_for_clean_command() {
        let vars = detect_ungrounded_vars("kubectl get pods -n=default");
        assert!(vars.is_empty());
    }

    // ------------------------------------------------------------------
    // ground_entity_ref_vars
    // ------------------------------------------------------------------

    fn campaign_with_pod(pod: Pod) -> (Campaign, String) {
        let mut c = Campaign::bootstrap("Ran", ran_domain::K8sCluster::new("test"));
        let id = pod.entity_id().0.clone();
        c.entities.insert_typed(pod);
        (c, id)
    }

    #[test]
    fn src_mount_path_resolves_to_first_host_path() {
        let mut pod = Pod::new("attacker", "default");
        pod.volume_mounts.push(ran_domain::Mount {
            name: "host-root".to_string(),
            mount_root: "/".to_string(),
            mount_point: "/host/root".to_string(),
            mount_type: None,
            is_host_path: true,
            read_only: false,
        });
        let (campaign, pod_id) = campaign_with_pod(pod);

        let mut args = HashMap::from([
            ("SRC".to_string(), pod_id),
            (
                "MOUNT_PATH".to_string(),
                "${SRC.MOUNT_PATH}/etc/kubernetes".to_string(),
            ),
        ]);
        ground_entity_ref_vars(&mut args, &campaign);
        assert_eq!(args["MOUNT_PATH"], "/host/root/etc/kubernetes");
    }

    #[test]
    fn src_mount_path_no_host_paths_leaves_placeholder() {
        let pod = Pod::new("attacker", "default");
        let (campaign, pod_id) = campaign_with_pod(pod);

        let mut args = HashMap::from([
            ("SRC".to_string(), pod_id),
            (
                "MOUNT_PATH".to_string(),
                "${SRC.MOUNT_PATH}/etc".to_string(),
            ),
        ]);
        ground_entity_ref_vars(&mut args, &campaign);
        assert_eq!(args["MOUNT_PATH"], "${SRC.MOUNT_PATH}/etc");
    }

    #[test]
    fn src_name_and_ns_resolve() {
        let pod = Pod::new("pivot-pod", "infra");
        let (campaign, pod_id) = campaign_with_pod(pod);

        let mut args = HashMap::from([
            ("SRC".to_string(), pod_id),
            ("LABEL".to_string(), "${SRC.NAME} in ${SRC.NS}".to_string()),
        ]);
        ground_entity_ref_vars(&mut args, &campaign);
        assert_eq!(args["LABEL"], "pivot-pod in infra");
    }

    #[test]
    fn src_node_resolves_for_pod() {
        let mut pod = Pod::new("worker", "default");
        pod.node_name = Some("node-1".to_string());
        let (campaign, pod_id) = campaign_with_pod(pod);

        let mut args = HashMap::from([
            ("SRC".to_string(), pod_id),
            ("NODE".to_string(), "${SRC.NODE}".to_string()),
        ]);
        ground_entity_ref_vars(&mut args, &campaign);
        assert_eq!(args["NODE"], "node-1");
    }

    #[test]
    fn unknown_ref_leaves_placeholder_intact() {
        let pod = Pod::new("pod", "ns");
        let (campaign, _pod_id) = campaign_with_pod(pod);

        let mut args = HashMap::from([("CMD".to_string(), "${UNKNOWN.PROP}".to_string())]);
        ground_entity_ref_vars(&mut args, &campaign);
        assert_eq!(args["CMD"], "${UNKNOWN.PROP}");
    }

    #[test]
    fn no_src_in_args_is_a_noop() {
        let pod = Pod::new("pod", "ns");
        let (campaign, _) = campaign_with_pod(pod);

        let mut args = HashMap::from([("CMD".to_string(), "${SRC.MOUNT_PATH}/etc".to_string())]);
        ground_entity_ref_vars(&mut args, &campaign);
        // SRC not injected yet — placeholder must survive
        assert_eq!(args["CMD"], "${SRC.MOUNT_PATH}/etc");
    }
}
