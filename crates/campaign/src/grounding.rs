//! TTP argument and command grounding.
//!
//! Grounding resolves template variables in TTP commands and effect strings:
//!
//! 1. **Context resolution** ([`ground_args_from_context`]) — fills special
//!    parameter names (NS, POD_NAME, NODE, RANDOM) from the campaign's
//!    knowledge of the target entity.
//!
//! 2. **Go-template conditionals** ([`resolve_go_template`]) — evaluates
//!    `{{ if .VAR }}...{{ else }}...{{ end }}` blocks and `{{.Var}}`
//!    substitutions that appear in procedure commands.
//!
//! 3. **`${KEY}` substitution** (already in [`crate::effects::ground_template`])
//!    — replaces `${KEY}` with the corresponding arg value.
//!
//! 4. **Ungrounded variable detection** ([`detect_ungrounded_vars`]) — scans
//!    the final command for any `${…}` patterns that were not resolved, so
//!    callers can log a warning.

use std::collections::HashMap;

use ran_domain::{Entity, EntityId};

use crate::campaign::{CampaignEntityRef, Campaign};

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
            "NS" | "NAMESPACE" => {
                if value.is_empty() || value == "${NS}" || value == "${NAMESPACE}" {
                    *value = target_ns.clone().unwrap_or_default();
                }
            }
            "POD_NAME" | "PODNAME" => {
                if value.contains("${POD_NAME}") {
                    let name = target_name.as_deref().unwrap_or("ran");
                    *value = value.replace("${POD_NAME}", name);
                }
            }
            "NODE" | "NODENAME" | "NODE_NAME" => {
                if value.is_empty() || value == "${NODE_NAME}" {
                    *value = target_node.clone().unwrap_or_default();
                }
            }
            "TOKEN" => {
                if let Some(raw) = resolve_token_arg(value, target.as_ref(), campaign) {
                    *value = raw;
                }
            }
            "API_SERVER" => {
                if value.is_empty() || value == "${API_SERVER}" {
                    *value = "https://kubernetes.default.svc".to_string();
                }
            }
            _ => {}
        }

        if value.contains("${RANDOM}") {
            *value = value.replace("${RANDOM}", &random_id());
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

        tracing::warn!(token_ref = trimmed, "TOKEN arg could not be resolved to a ServiceAccount token");
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
    let sa = campaign.service_accounts.get(&sa_id)?;
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
// Go-template conditional and substitution
// ---------------------------------------------------------------------------

/// Evaluate Go-style `{{ if }}` / `{{ else }}` / `{{ end }}` blocks and
/// `{{.VarName}}` substitutions in `template`.
///
/// Supported directives (the content between `{{` and `}}`):
///
/// | Directive           | Meaning |
/// |---------------------|---------|
/// | `if .VAR`           | Include the block when `VAR` is truthy |
/// | `if not .VAR`       | Include the block when `VAR` is falsy  |
/// | `else`              | Switch to the alternative branch |
/// | `end`               | Close the current conditional block |
/// | `.VarName` / `VarName` | Substitute with `args["VarName"]`  |
///
/// Truthiness: a value is **truthy** unless it is empty, `"false"`, `"0"`,
/// or `"no"` (case-insensitive).
///
/// Unrecognised directives and malformed `{{ }}` blocks are passed through
/// unchanged so as not to corrupt the command.
pub fn resolve_go_template(template: &str, args: &HashMap<String, String>) -> String {
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;

    // Stack of `(condition_satisfied, in_else_branch)`.
    // The block output only when every frame on the stack is "active":
    //   - if-branch  (in_else = false): active when condition is true
    //   - else-branch (in_else = true): active when condition is false
    let mut stack: Vec<(bool, bool)> = Vec::new();

    while i < len {
        // Look for `{{`
        if i + 1 < len && chars[i] == '{' && chars[i + 1] == '{' {
            let start = i + 2;
            // Find the closing `}}`
            let mut j = start;
            while j + 1 < len && !(chars[j] == '}' && chars[j + 1] == '}') {
                j += 1;
            }

            if j + 1 >= len {
                // No closing `}}` — pass through the literal `{{` and advance by 1
                if is_stack_active(&stack) {
                    out.push('{');
                    out.push('{');
                }
                i += 2;
                continue;
            }

            let directive: String = chars[start..j].iter().collect();
            let directive = directive.trim();
            i = j + 2;

            if let Some(rest) = directive.strip_prefix("if ") {
                let cond_str = rest.trim();
                let negated = cond_str.starts_with("not ");
                let var_name = if negated {
                    cond_str[4..].trim().trim_start_matches('.')
                } else {
                    cond_str.trim_start_matches('.')
                };
                let val = args.get(var_name).map(String::as_str).unwrap_or("");
                let truthy = is_truthy(val);
                let satisfied = if negated { !truthy } else { truthy };
                stack.push((satisfied, false));
            } else if directive == "else" {
                if let Some(frame) = stack.last_mut() {
                    frame.1 = true;
                }
            } else if directive == "end" {
                stack.pop();
            } else if is_stack_active(&stack) {
                // Variable substitution: `.VarName` or bare `VarName`
                let var_name = directive.trim_start_matches('.');
                if let Some(val) = args.get(var_name) {
                    out.push_str(val);
                }
                // Unknown directives are silently ignored (not passed through)
            }
        } else {
            if is_stack_active(&stack) {
                out.push(chars[i]);
            }
            i += 1;
        }
    }

    out
}

fn is_stack_active(stack: &[(bool, bool)]) -> bool {
    stack
        .iter()
        .all(|(satisfied, in_else)| if *in_else { !satisfied } else { *satisfied })
}

fn is_truthy(val: &str) -> bool {
    !matches!(
        val.trim().to_ascii_lowercase().as_str(),
        "" | "false" | "0" | "no"
    )
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
        campaign.pods.insert(pod.entity_id(), pod);

        let mut args = HashMap::from([("NS".to_string(), "${NS}".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["NS"], "staging");
    }

    #[test]
    fn ground_args_preserves_explicit_ns() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = pod_in_ns("demo", "staging");
        let target_id = pod.entity_id().0.clone();
        campaign.pods.insert(pod.entity_id(), pod);

        let mut args = HashMap::from([("NS".to_string(), "production".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["NS"], "production");
    }

    #[test]
    fn ground_args_fills_pod_name() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = pod_in_ns("nginx", "default");
        let target_id = pod.entity_id().0.clone();
        campaign.pods.insert(pod.entity_id(), pod);

        let mut args = HashMap::from([("PodName".to_string(), "${POD_NAME}-${RANDOM}".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        let val = &args["PodName"];
        assert!(val.starts_with("nginx-"), "expected 'nginx-...', got '{}'", val);
        assert!(!val.contains("${POD_NAME}"));
        assert!(!val.contains("${RANDOM}"));
    }

    #[test]
    fn ground_args_fills_node_from_pod() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = pod_on_node("runner", "default", "worker-1");
        let target_id = pod.entity_id().0.clone();
        campaign.pods.insert(pod.entity_id(), pod);

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
        campaign.service_accounts.insert(sa_id.clone(), sa);

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
        campaign.pods.insert(pod.entity_id(), pod);

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
        campaign.service_accounts.insert(sa.entity_id(), sa);

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
        campaign.pods.insert(pod.entity_id(), pod);

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
        campaign.service_accounts.insert(sa.entity_id(), sa);

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
        campaign.service_accounts.insert(sa.entity_id(), sa);

        let mut args = HashMap::from([("TOKEN".to_string(), "".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["TOKEN"], "ey.sa.target");
    }

    #[test]
    fn ground_args_keeps_unresolvable_token_reference_unchanged() {
        let mut campaign = Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let pod = Pod::new("runner", "default");
        let target_id = pod.entity_id().0.clone();
        campaign.pods.insert(pod.entity_id(), pod);

        let mut args = HashMap::from([("TOKEN".to_string(), "missing-sa".to_string())]);
        ground_args_from_context(&mut args, &target_id, &campaign);

        assert_eq!(args["TOKEN"], "missing-sa");
    }

    // ------------------------------------------------------------------
    // resolve_go_template
    // ------------------------------------------------------------------

    #[test]
    fn go_template_if_true_includes_block() {
        let args = HashMap::from([("FLAG".to_string(), "true".to_string())]);
        let result = resolve_go_template("before{{ if .FLAG }}INSIDE{{ end }}after", &args);
        assert_eq!(result, "beforeINSIDEafter");
    }

    #[test]
    fn go_template_if_false_excludes_block() {
        let args = HashMap::from([("FLAG".to_string(), "false".to_string())]);
        let result = resolve_go_template("before{{ if .FLAG }}INSIDE{{ end }}after", &args);
        assert_eq!(result, "beforeafter");
    }

    #[test]
    fn go_template_else_branch_taken_when_false() {
        let args = HashMap::from([("ALL_NS".to_string(), "false".to_string())]);
        let result = resolve_go_template(
            "{{ if .ALL_NS }}all{{ else }}single{{ end }}",
            &args,
        );
        assert_eq!(result, "single");
    }

    #[test]
    fn go_template_else_branch_skipped_when_true() {
        let args = HashMap::from([("ALL_NS".to_string(), "true".to_string())]);
        let result = resolve_go_template(
            "{{ if .ALL_NS }}all{{ else }}single{{ end }}",
            &args,
        );
        assert_eq!(result, "all");
    }

    #[test]
    fn go_template_if_not_includes_when_falsy() {
        let args = HashMap::from([("CLUSTER_ROLE".to_string(), "false".to_string())]);
        let result = resolve_go_template(
            "kubectl{{ if not .CLUSTER_ROLE }} -n=${NS}{{ end }}",
            &args,
        );
        assert_eq!(result, "kubectl -n=${NS}");
    }

    #[test]
    fn go_template_substitutes_dot_variable() {
        let args = HashMap::from([("PodName".to_string(), "my-pod".to_string())]);
        let result = resolve_go_template("name={{.PodName}}", &args);
        assert_eq!(result, "name=my-pod");
    }

    #[test]
    fn go_template_mixed_with_dollar_syntax() {
        let args = HashMap::from([
            ("PodName".to_string(), "my-pod".to_string()),
            ("NS".to_string(), "default".to_string()),
        ]);
        // {{ }} style for Go template, ${} style for dollar substitution
        let template = "{{.PodName}}-${NS}";
        let result = resolve_go_template(template, &args);
        // resolve_go_template handles {{}} only; ${} is handled by ground_template
        assert_eq!(result, "my-pod-${NS}");
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
}
