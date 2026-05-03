use std::collections::HashMap;

use indexmap::IndexSet;
use ran_domain::{
    CanReach, ContainerEscape, CronJob, Entity, EntityId, K8sNode, K8sRole, K8sRoleBinding,
    KubeletExecSource, NameConfidence, OutputTransformKind, Pod, PodExec, RbacPermission,
    RbacSubject, RceCanExec, Relation, RunsOn, ServiceAccount, SessionChannel,
};

use crate::grounding::resolve_template;

type SimpleEffectHandler = fn(&HashMap<String, String>) -> Result<FactsUpdate, String>;
/// Handler for relation-style effects such as `rce.can-exec(src, tgt)`.
/// Receives both the positional args (from parsing the effect string) and the
/// full execution context (the TTP args map) so handlers that need extra
/// context — like `PROCEDURE_CMD` for `rce.can-exec` — can read it without
/// requiring a separate post-hoc injection step.
type RelationEffectHandler = fn(&[&str], &HashMap<String, String>) -> Result<FactsUpdate, String>;

pub struct ParsedStructuralEffect {
    pub updates: FactsUpdate,
    pub handled: bool,
}

#[derive(Debug, Default)]
pub struct FactsUpdate {
    pub new_entities: Vec<Box<dyn Entity + Send + Sync>>,
    pub new_relations: Vec<Box<dyn Relation + Send + Sync>>,
    /// Entity-identity merges: `(stale_id, preferred_id)`.
    ///
    /// When applied, all relations referencing `stale_id` are rewritten to
    /// `preferred_id`, the stale entity's runtime data is merged into the
    /// preferred entity, and the stale entity is removed from campaign state.
    /// Used when a placeholder entity (e.g. IP-derived pod name from a network
    /// scan) is later identified as an already-known named entity.
    pub entity_aliases: IndexSet<(EntityId, EntityId)>,
}

impl FactsUpdate {
    pub fn merge(&mut self, other: Self) {
        // Build O(1)-lookup sets from existing entries so each item from `other`
        // is checked in O(1) rather than O(n), avoiding the previous O(n²) scan.
        let seen_entities: IndexSet<EntityId> =
            self.new_entities.iter().map(|e| e.entity_id()).collect();
        for entity in other.new_entities {
            if !seen_entities.contains(&entity.entity_id()) {
                self.new_entities.push(entity);
            }
        }

        let seen_relations: IndexSet<(String, EntityId, EntityId)> = self
            .new_relations
            .iter()
            .map(|r| {
                (
                    r.relation_name().to_string(),
                    r.source_id().clone(),
                    r.target_id().clone(),
                )
            })
            .collect();
        for rel in other.new_relations {
            let key = (
                rel.relation_name().to_string(),
                rel.source_id().clone(),
                rel.target_id().clone(),
            );
            if !seen_relations.contains(&key) {
                self.new_relations.push(rel);
            }
        }

        // IndexSet::insert handles dedup natively — no scan needed.
        self.entity_aliases.extend(other.entity_aliases);
    }
}

pub fn ground_template(template: &str, args: &HashMap<String, String>) -> String {
    // Pass 1: evaluate template {% if/else/endif %} blocks and {{ Var }} substitutions.
    let mut grounded = resolve_template(template, args);

    // Pass 2: replace ${KEY} placeholders with case-insensitive matching on
    // the placeholder name itself (e.g. ${SRC}, ${src}, ${Src}).
    grounded = substitute_dollar_placeholders_case_insensitive(&grounded, args);

    grounded
}

fn substitute_dollar_placeholders_case_insensitive(
    template: &str,
    args: &HashMap<String, String>,
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut idx = 0usize;

    while let Some(rel_start) = template[idx..].find("${") {
        let start = idx + rel_start;
        out.push_str(&template[idx..start]);

        let name_start = start + 2;
        let Some(rel_end) = template[name_start..].find('}') else {
            // Unterminated placeholder; keep remainder verbatim.
            out.push_str(&template[start..]);
            return out;
        };
        let end = name_start + rel_end;
        let placeholder_name = &template[name_start..end];

        if let Some((_, value)) = args
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(placeholder_name))
        {
            out.push_str(value);
        } else {
            out.push_str(&template[start..=end]);
        }

        idx = end + 1;
    }

    out.push_str(&template[idx..]);
    out
}

pub fn parse_effect(effect: &str, args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    Ok(parse_effect_with_status(effect, args)?.updates)
}

pub fn parse_effect_with_status(
    effect: &str,
    args: &HashMap<String, String>,
) -> Result<ParsedStructuralEffect, String> {
    let normalized = effect.trim();

    if let Some(handler) = resolve_simple_effect_handler(normalized) {
        return Ok(ParsedStructuralEffect {
            updates: handler(args)?,
            handled: true,
        });
    }

    if normalized.contains('(') && normalized.ends_with(')') {
        return parse_relation_effect(normalized, args);
    }

    Ok(ParsedStructuralEffect {
        updates: FactsUpdate::default(),
        handled: false,
    })
}

fn parse_k8s_pod(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.Pod effect requires Namespace argument".to_string())?;

    let pod_name = get_arg(args, &["PodName", "PODNAME", "POD_NAME"])
        .ok_or_else(|| "k8s.Pod effect requires PodName argument".to_string())?;

    let mut pod = Pod::new(pod_name, namespace);

    if let Some(node_name) = get_arg(args, &["NodeName", "NODENAME", "NODE_NAME", "Node", "NODE"]) {
        if !node_name.trim().is_empty() {
            pod.node_name = Some(node_name.to_string());
        }
    }

    if let Some(sa_name) = get_arg(
        args,
        &[
            "ServiceAccount",
            "SERVICEACCOUNT",
            "SERVICE_ACCOUNT",
            "ServiceAccountName",
        ],
    ) {
        if !sa_name.trim().is_empty() {
            pod.service_account_name = Some(sa_name.to_string());
        }
    }

    if let Some(is_running) = get_arg(args, &["IsRunning", "ISRUNNING", "IS_RUNNING"]) {
        pod.is_running = parse_bool_like(is_running);
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(pod)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
    })
}

fn parse_k8s_serviceaccount(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.serviceaccount effect requires Namespace argument".to_string())?;

    let sa_name = get_arg(
        args,
        &["ServiceAccountName", "SA_NAME", "SERVICEACCOUNTNAME"],
    )
    .ok_or_else(|| "k8s.serviceaccount effect requires ServiceAccountName argument".to_string())?;

    let mut sa = ServiceAccount::new(sa_name, namespace);

    if let Some(raw_token) = get_arg(args, &["Token", "TOKEN"]) {
        if !raw_token.is_empty() {
            use ran_domain::{JwToken, ServiceAccountToken};
            sa.token = Some(ServiceAccountToken {
                jwt: JwToken {
                    raw: raw_token.to_string(),
                    ..Default::default()
                },
                namespace: namespace.to_string(),
                service_account_name: sa_name.to_string(),
                ..Default::default()
            });
        }
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(sa)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
    })
}

fn parse_k8s_role(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.role effect requires Namespace argument".to_string())?;

    let role_name = get_arg(args, &["RoleName", "ROLENAME", "ROLE_NAME"])
        .ok_or_else(|| "k8s.role effect requires RoleName argument".to_string())?;

    let mut role = K8sRole::new(role_name, namespace);

    if let Some(rules_json) = get_arg(args, &["Rules", "RULES"]) {
        role.permissions = parse_rules_json(rules_json);
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(role)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
    })
}

fn parse_k8s_rolebinding(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.rolebinding effect requires Namespace argument".to_string())?;

    let binding_name = get_arg(args, &["BindingName", "BINDINGNAME", "BINDING_NAME"])
        .ok_or_else(|| "k8s.rolebinding effect requires BindingName argument".to_string())?;

    let mut binding = K8sRoleBinding::new(binding_name, namespace);

    if let Some(role_ref) = get_arg(args, &["RoleRef", "ROLEREF", "ROLE_REF"]) {
        binding.role_ref = role_ref.to_string();
    }

    if let Some(subjects_json) = get_arg(args, &["Subjects", "SUBJECTS"]) {
        binding.subjects = parse_subjects_json(subjects_json);
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(binding)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
    })
}

fn parse_k8s_cronjob(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"])
        .ok_or_else(|| "k8s.cronjob effect requires Namespace argument".to_string())?;

    let name = get_arg(args, &["CronJobName", "CRONJOBNAME", "CRONJOB_NAME"])
        .ok_or_else(|| "k8s.cronjob effect requires CronJobName argument".to_string())?;

    let mut cj = CronJob::new(name, namespace);

    if let Some(schedule) = get_arg(args, &["Schedule", "SCHEDULE"]) {
        if !schedule.is_empty() {
            cj.schedule = Some(schedule.to_string());
        }
    }

    Ok(FactsUpdate {
        new_entities: vec![Box::new(cj)],
        new_relations: Vec::new(),
        entity_aliases: IndexSet::new(),
    })
}

/// Parse a JSON array of RBAC rule objects into `RbacPermission` entries.
///
/// Expected format (same schema as `k8s.selfsubjectrulesreview` output):
/// ```json
/// [{"verbs":["get","list"],"resources":["pods"],"apiGroups":[""]}]
/// ```
fn parse_rules_json(json: &str) -> Vec<RbacPermission> {
    let json = json.trim();
    if json.is_empty() || json == "[]" || json == "null" {
        return Vec::new();
    }

    #[derive(serde::Deserialize)]
    struct RuleEntry {
        #[serde(default)]
        verbs: Vec<String>,
        #[serde(default)]
        resources: Vec<String>,
        #[serde(rename = "apiGroups", default)]
        api_groups: Vec<String>,
    }

    let Ok(entries) = serde_json::from_str::<Vec<RuleEntry>>(json) else {
        return Vec::new();
    };

    let mut perms = Vec::new();
    for entry in entries {
        for verb in &entry.verbs {
            for resource in &entry.resources {
                let mut perm = RbacPermission::new(verb.clone(), resource.clone());
                perm.api_group = entry.api_groups.first().cloned();
                perms.push(perm);
            }
        }
    }
    perms
}

/// Parse a JSON array of subject objects into `RbacSubject` entries.
///
/// Expected format:
/// ```json
/// [{"kind":"ServiceAccount","name":"my-sa","namespace":"default"}]
/// ```
fn parse_subjects_json(json: &str) -> Vec<RbacSubject> {
    let json = json.trim();
    if json.is_empty() || json == "[]" || json == "null" {
        return Vec::new();
    }

    serde_json::from_str::<Vec<RbacSubject>>(json).unwrap_or_default()
}

fn parse_relation_effect(
    effect: &str,
    ctx: &HashMap<String, String>,
) -> Result<ParsedStructuralEffect, String> {
    let (name, args) = split_relation(effect)?;

    if let Some(handler) = resolve_relation_effect_handler(name) {
        return Ok(ParsedStructuralEffect {
            updates: handler(&args, ctx)?,
            handled: true,
        });
    }

    Ok(ParsedStructuralEffect {
        updates: FactsUpdate::default(),
        handled: false,
    })
}

fn resolve_simple_effect_handler(effect_name: &str) -> Option<SimpleEffectHandler> {
    match normalize_effect_name(effect_name).as_str() {
        "k8s.pod" => Some(parse_k8s_pod),
        "k8s.serviceaccount" => Some(parse_k8s_serviceaccount),
        "k8s.role" => Some(parse_k8s_role),
        "k8s.rolebinding" => Some(parse_k8s_rolebinding),
        "k8s.cronjob" => Some(parse_k8s_cronjob),
        _ => None,
    }
}

fn resolve_relation_effect_handler(effect_name: &str) -> Option<RelationEffectHandler> {
    match normalize_effect_name(effect_name).as_str() {
        "k8s.can-exec" => Some(parse_k8s_can_exec_relation),
        "k8s.can-reach" => Some(parse_k8s_can_reach_relation),
        "k8s.runs-on" | "runs-on" => Some(parse_runs_on_relation),
        "k8s.kubelet-exec-source" | "k8s.kubelet-exec" => Some(parse_kubelet_exec_source_relation),
        "c2.session" => Some(parse_c2_session_relation),
        "rce.can-exec" => Some(parse_rce_can_exec_relation),
        "container.escape" => Some(parse_container_escape_relation),
        _ => None,
    }
}

fn parse_c2_session_relation(
    args: &[&str],
    ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("c2.session effect expects exactly 2 args: backend and target".to_string());
    }

    let backend_raw = args[0].trim();
    if backend_raw.is_empty() {
        return Err("c2.session backend cannot be empty".to_string());
    }

    // `sys` resolves to the entity the TTP executed on.
    let target_id = if args[1].eq_ignore_ascii_case("sys") {
        ctx.get("TARGET_ID")
            .filter(|id| !id.is_empty())
            .map(String::as_str)
            .ok_or_else(|| "c2.session: 'sys' requires TARGET_ID in context".to_string())?
    } else {
        args[1]
    };

    // First arg accepts one of:
    // - `sliver`            => source `c2/sliver`, session backend `session/sliver`
    // - `c2/sliver`         => source `c2/sliver`, session backend `session/c2/sliver`
    // - `session/sliver-1`  => source `c2/sliver-1`, session backend `session/sliver-1`
    let (source_id, session_id) = if let Some(rest) = backend_raw.strip_prefix("session/") {
        let source = if rest.starts_with("c2/") {
            rest.to_string()
        } else {
            format!("c2/{}", rest)
        };
        (source, backend_raw.to_string())
    } else if backend_raw.starts_with("c2/") {
        (backend_raw.to_string(), format!("session/{}", backend_raw))
    } else {
        (
            format!("c2/{}", backend_raw),
            format!("session/{}", backend_raw),
        )
    };

    let rel = SessionChannel::new(source_id, target_id, session_id);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
    })
}

fn parse_k8s_can_exec_relation(
    args: &[&str],
    _ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("k8s.can-exec effect expects exactly 2 args".to_string());
    }
    let rel = PodExec::new(args[0], args[1]);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
    })
}

fn parse_k8s_can_reach_relation(
    args: &[&str],
    _ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("k8s.can-reach effect expects exactly 2 args".to_string());
    }
    let rel = CanReach::new(args[0], args[1]);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
    })
}

fn parse_runs_on_relation(
    args: &[&str],
    _ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("runs-on effect expects exactly 2 args".to_string());
    }
    let rel = RunsOn::new(args[0], args[1]);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
    })
}

fn parse_kubelet_exec_source_relation(
    args: &[&str],
    ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("kubelet-exec effect expects exactly 2 args".to_string());
    }
    // `sys` is a well-known placeholder for the entity that executed the TTP.
    // Resolve it to the actual target entity ID stored in the context.
    let src = if args[0].eq_ignore_ascii_case("sys") {
        ctx.get("TARGET_ID")
            .filter(|id| !id.is_empty())
            .map(String::as_str)
            .ok_or_else(|| {
                "kubelet-exec: arg is 'sys' but TARGET_ID not present in context".to_string()
            })?
    } else {
        args[0]
    };
    // Use PROCEDURE_CMD as the envelope template so subsequent command routing
    // can wrap via RelationSummary::wrap_command with ${CMD} substitution.
    let envelope = ctx
        .get("PROCEDURE_CMD")
        .filter(|v| !v.trim().is_empty())
        .cloned();

    let tgt_raw = args[1].trim();
    let mut rel = KubeletExecSource::new(src, tgt_raw).with_opt_envelope(envelope);
    if rel.envelope.is_some() {
        rel = rel.with_output_transform(OutputTransformKind::JsonEnvelope);
    }
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
    })
}

fn parse_rce_can_exec_relation(
    args: &[&str],
    ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("rce.can-exec effect expects exactly 2 args: source and target".to_string());
    }
    // The execution context may carry PROCEDURE_CMD — the grounded exploit
    // command that was just run to establish this RCE path.  Store it as the
    // wrapping envelope so subsequent commands through this hop re-invoke the
    // same exploit with the new command substituted for ${CMD}.
    let envelope = ctx
        .get("PROCEDURE_CMD")
        .filter(|v| !v.trim().is_empty())
        .cloned();
    let rel = RceCanExec::new(args[0], args[1]).with_opt_envelope(envelope);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
    })
}

fn parse_container_escape_relation(
    args: &[&str],
    ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 1 {
        return Err(
            "container.escape effect expects exactly 1 arg: the source pod \
             (use `sys` or the pod entity ID; the node is resolved automatically)"
                .to_string(),
        );
    }

    // `sys` resolves to the entity the TTP executed on.
    let src = if args[0].eq_ignore_ascii_case("sys") {
        ctx.get("TARGET_ID")
            .filter(|id| !id.is_empty())
            .map(String::as_str)
            .ok_or_else(|| "container.escape: 'sys' requires TARGET_ID in context".to_string())?
    } else {
        args[0]
    };

    // Resolve the node entity ID. The pipeline injects TARGET_NODE_ID from
    // pod.node_name (if known) or from an existing runs-on graph edge.
    // If neither is available, the pod is running on an unknown node — create a
    // placeholder using the pod's short name as the best available guess.
    // The placeholder is aliased to the real node once its name is discovered
    // (e.g. via sys.node-name after running hostname on the host).
    let node_entity_id: String = ctx
        .get("TARGET_NODE_ID")
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
        .unwrap_or_else(|| {
            // Infer the node name from the pod's short name (the segment after
            // "/pod/" in the entity ID). Falls back to the last path component
            // for non-standard entity IDs.
            let pod_name = src
                .split("/pod/")
                .nth(1)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| src.rsplit('/').next().unwrap_or("unknown"));
            format!("node/escape_{}", pod_name)
        });

    // Parse the node name from `node/<name>` to construct the typed entity.
    let node_name = node_entity_id
        .strip_prefix("node/")
        .filter(|n| !n.is_empty())
        .ok_or_else(|| {
            format!(
                "container.escape: '{}' is not a valid node entity ID (expected node/<name>)",
                node_entity_id
            )
        })?
        .to_string();

    // Ensure the node entity exists. If a runs-on relation already references
    // this node, the campaign merges rather than duplicates it.
    // Mark the name authoritative when it came from pod.node_name (K8s API).
    let authoritative = ctx
        .get("TARGET_NODE_AUTHORITATIVE")
        .map(|v| v == "true")
        .unwrap_or(false);
    let mut node = K8sNode::new(&node_name);
    if authoritative {
        node.name_confidence = NameConfidence::Authoritative;
    }

    // PROCEDURE_CMD is the grounded escape command (e.g. `nsenter -t 1 ... ${CMD}`).
    // Store it as the envelope so subsequent commands through this hop are
    // wrapped with the same escape primitive.
    let envelope = ctx
        .get("PROCEDURE_CMD")
        .filter(|v| !v.trim().is_empty())
        .cloned();

    Ok(FactsUpdate {
        new_entities: vec![Box::new(node)],
        new_relations: vec![
            Box::new(RunsOn::new(src, &node_entity_id)),
            Box::new(ContainerEscape::new(src, &node_entity_id).with_opt_envelope(envelope)),
        ],
        entity_aliases: IndexSet::new(),
    })
}

fn split_relation(effect: &str) -> Result<(&str, Vec<&str>), String> {
    let open = effect
        .find('(')
        .ok_or_else(|| format!("invalid relation effect: {}", effect))?;
    let close = effect
        .rfind(')')
        .ok_or_else(|| format!("invalid relation effect: {}", effect))?;

    if close <= open {
        return Err(format!("invalid relation effect: {}", effect));
    }

    let name = effect[..open].trim();
    let body = effect[open + 1..close].trim();

    let args = if body.is_empty() {
        Vec::new()
    } else {
        body.split(',').map(str::trim).collect()
    };

    Ok((name, args))
}

fn get_arg<'a>(args: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(v) = args.get(*key) {
            return Some(v.as_str());
        }
    }

    None
}

fn normalize_effect_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn parse_bool_like(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "running"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ran_domain::{CanReach, KubeletExecSource, OutputTransformKind, SessionChannel};

    fn ctx() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn k8s_can_reach_creates_can_reach_relation() {
        let result = parse_effect("k8s.can-reach(pod/ns/a, pod/ns/b)", &ctx());
        let update = result.expect("should parse");
        assert_eq!(update.new_relations.len(), 1);
        let rel = update.new_relations[0].as_ref();
        assert!(rel.as_any().downcast_ref::<CanReach>().is_some());
        assert_eq!(rel.source_id().0, "pod/ns/a");
        assert_eq!(rel.target_id().0, "pod/ns/b");
        assert_eq!(rel.relation_name(), "can-reach");
    }

    #[test]
    fn k8s_can_reach_wrong_arg_count_returns_err() {
        assert!(parse_effect("k8s.can-reach(pod/ns/a)", &ctx()).is_err());
        assert!(parse_effect("k8s.can-reach(a, b, c)", &ctx()).is_err());
    }

    #[test]
    fn k8s_can_reach_relation_name_is_can_reach() {
        let update = parse_effect("k8s.can-reach(a, b)", &ctx()).unwrap();
        assert_eq!(update.new_relations[0].relation_name(), "can-reach");
    }

    #[test]
    fn k8s_can_reach_ids_containing_slashes_roundtrip_correctly() {
        let update = parse_effect(
            "k8s.can-reach(ns/default/pod/frontend, ns/default/pod/backend)",
            &ctx(),
        )
        .unwrap();
        let rel = &update.new_relations[0];
        assert_eq!(rel.source_id().0, "ns/default/pod/frontend");
        assert_eq!(rel.target_id().0, "ns/default/pod/backend");
    }

    #[test]
    fn c2_session_creates_session_channel_relation() {
        let update = parse_effect("c2.session(sliver, ns/default/pod/victim)", &ctx()).unwrap();
        assert_eq!(update.new_relations.len(), 1);
        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<SessionChannel>()
            .expect("expected SessionChannel relation");
        assert_eq!(rel.source_id().0, "c2/sliver");
        assert_eq!(rel.target_id().0, "ns/default/pod/victim");
        assert_eq!(rel.session_id, "session/sliver");
    }

    #[test]
    fn c2_session_accepts_explicit_session_backend_id() {
        let update = parse_effect(
            "c2.session(session/sliver-operator-1, ns/default/pod/victim)",
            &ctx(),
        )
        .unwrap();
        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<SessionChannel>()
            .expect("expected SessionChannel relation");
        assert_eq!(rel.source_id().0, "c2/sliver-operator-1");
        assert_eq!(rel.session_id, "session/sliver-operator-1");
    }

    #[test]
    fn c2_session_resolves_sys_target_from_context() {
        let mut args = ctx();
        args.insert("TARGET_ID".into(), "node/worker-1".into());

        let update = parse_effect("c2.session(sliver, sys)", &args).unwrap();
        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<SessionChannel>()
            .expect("expected SessionChannel relation");
        assert_eq!(rel.target_id().0, "node/worker-1");
    }

    #[test]
    fn c2_session_wrong_arg_count_returns_err() {
        assert!(parse_effect("c2.session(sliver)", &ctx()).is_err());
        assert!(parse_effect("c2.session(a, b, c)", &ctx()).is_err());
    }

    #[test]
    fn kubelet_exec_source_with_sys_preserves_marker_and_metadata() {
        let mut args = ctx();
        args.insert("TARGET_ID".into(), "ns/default/pod/attacker".into());
        args.insert("PROCEDURE_CMD".into(), "ran-ws -- ${CMD}".into());

        let update = parse_effect("k8s.kubelet-exec-source(sys, all(k8s.node))", &args).unwrap();
        assert_eq!(update.new_relations.len(), 1);

        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<KubeletExecSource>()
            .expect("expected KubeletExecSource relation");
        assert_eq!(rel.source_id().0, "ns/default/pod/attacker");
        assert_eq!(rel.target_id().0, "all(k8s.node)");
        assert_eq!(rel.envelope.as_deref(), Some("ran-ws -- ${CMD}"));
        assert_eq!(
            rel.output_transform,
            Some(OutputTransformKind::JsonEnvelope),
            "envelope-backed kubelet channel should request JSON unwrapping"
        );
    }

    #[test]
    fn kubelet_exec_source_without_envelope_has_no_output_transform() {
        let update = parse_effect(
            "k8s.kubelet-exec-source(ns/default/pod/a, all(k8s.node))",
            &ctx(),
        )
        .unwrap();
        let rel = update.new_relations[0]
            .as_any()
            .downcast_ref::<KubeletExecSource>()
            .expect("expected KubeletExecSource relation");
        assert!(rel.envelope.is_none());
        assert!(rel.output_transform.is_none());
    }

    // --- k8s.serviceaccount ---

    #[test]
    fn k8s_serviceaccount_creates_sa_entity() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("ServiceAccountName".into(), "my-sa".into());
        let update = parse_effect("k8s.serviceaccount", &args).unwrap();
        assert_eq!(update.new_entities.len(), 1);
        let sa = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::ServiceAccount>()
            .unwrap();
        assert_eq!(sa.entity_name(), "my-sa");
        assert_eq!(sa.namespace(), Some("default"));
    }

    #[test]
    fn k8s_serviceaccount_missing_namespace_returns_err() {
        let mut args = ctx();
        args.insert("ServiceAccountName".into(), "my-sa".into());
        assert!(parse_effect("k8s.serviceaccount", &args).is_err());
    }

    #[test]
    fn k8s_serviceaccount_missing_name_returns_err() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        assert!(parse_effect("k8s.serviceaccount", &args).is_err());
    }

    #[test]
    fn k8s_serviceaccount_optional_token_populates_sa_token() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("ServiceAccountName".into(), "my-sa".into());
        args.insert("Token".into(), "eyJhbGciOiJSUzI1NiJ9.test".into());
        let update = parse_effect("k8s.serviceaccount", &args).unwrap();
        let sa = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::ServiceAccount>()
            .unwrap();
        assert_eq!(sa.raw_token(), Some("eyJhbGciOiJSUzI1NiJ9.test"));
    }

    // --- k8s.role ---

    #[test]
    fn k8s_role_creates_role_with_namespace_and_name() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("RoleName".into(), "pod-reader".into());
        let update = parse_effect("k8s.role", &args).unwrap();
        let role = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRole>()
            .unwrap();
        assert_eq!(role.entity_name(), "pod-reader");
        assert_eq!(role.namespace(), Some("default"));
        assert!(role.permissions.is_empty());
    }

    #[test]
    fn k8s_role_parses_rules_json() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("RoleName".into(), "pod-reader".into());
        args.insert(
            "Rules".into(),
            r#"[{"verbs":["get","list"],"resources":["pods"],"apiGroups":[""]}]"#.into(),
        );
        let update = parse_effect("k8s.role", &args).unwrap();
        let role = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRole>()
            .unwrap();
        assert_eq!(role.permissions.len(), 2);
        assert!(role
            .permissions
            .iter()
            .any(|p| p.verb == "get" && p.resource_type == "pods"));
        assert!(role
            .permissions
            .iter()
            .any(|p| p.verb == "list" && p.resource_type == "pods"));
    }

    #[test]
    fn k8s_role_empty_rules_arg_creates_role_with_no_permissions() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("RoleName".into(), "empty-role".into());
        args.insert("Rules".into(), "[]".into());
        let update = parse_effect("k8s.role", &args).unwrap();
        let role = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRole>()
            .unwrap();
        assert!(role.permissions.is_empty());
    }

    #[test]
    fn k8s_role_missing_name_returns_err() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        assert!(parse_effect("k8s.role", &args).is_err());
    }

    // --- k8s.rolebinding ---

    #[test]
    fn k8s_rolebinding_creates_binding_with_role_ref_and_subjects() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("BindingName".into(), "pod-reader-binding".into());
        args.insert("RoleRef".into(), "pod-reader".into());
        args.insert(
            "Subjects".into(),
            r#"[{"kind":"ServiceAccount","name":"my-sa","namespace":"default"}]"#.into(),
        );
        let update = parse_effect("k8s.rolebinding", &args).unwrap();
        let binding = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRoleBinding>()
            .unwrap();
        assert_eq!(binding.entity_name(), "pod-reader-binding");
        assert_eq!(binding.role_ref, "pod-reader");
        assert_eq!(binding.subjects.len(), 1);
        assert_eq!(binding.subjects[0].name, "my-sa");
    }

    #[test]
    fn k8s_rolebinding_missing_binding_name_returns_err() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        assert!(parse_effect("k8s.rolebinding", &args).is_err());
    }

    #[test]
    fn k8s_rolebinding_empty_subjects_creates_binding() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("BindingName".into(), "empty-binding".into());
        args.insert("Subjects".into(), "[]".into());
        let update = parse_effect("k8s.rolebinding", &args).unwrap();
        let binding = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sRoleBinding>()
            .unwrap();
        assert!(binding.subjects.is_empty());
    }

    // --- k8s.cronjob ---

    #[test]
    fn k8s_cronjob_creates_cronjob_entity() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("CronJobName".into(), "cleanup-job".into());
        let update = parse_effect("k8s.cronjob", &args).unwrap();
        let cj = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::CronJob>()
            .unwrap();
        assert_eq!(cj.entity_name(), "cleanup-job");
        assert_eq!(cj.namespace(), Some("default"));
        assert!(cj.schedule.is_none());
    }

    #[test]
    fn k8s_cronjob_optional_schedule_arg_populated() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        args.insert("CronJobName".into(), "nightly".into());
        args.insert("Schedule".into(), "0 2 * * *".into());
        let update = parse_effect("k8s.cronjob", &args).unwrap();
        let cj = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::CronJob>()
            .unwrap();
        assert_eq!(cj.schedule.as_deref(), Some("0 2 * * *"));
    }

    #[test]
    fn k8s_cronjob_missing_namespace_returns_err() {
        let mut args = ctx();
        args.insert("CronJobName".into(), "cleanup-job".into());
        assert!(parse_effect("k8s.cronjob", &args).is_err());
    }

    #[test]
    fn k8s_cronjob_missing_name_returns_err() {
        let mut args = ctx();
        args.insert("Namespace".into(), "default".into());
        assert!(parse_effect("k8s.cronjob", &args).is_err());
    }

    // --- container.escape ---

    #[test]
    fn container_escape_creates_node_and_relations_with_known_node() {
        let mut args = ctx();
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        args.insert(
            "PROCEDURE_CMD".into(),
            "nsenter -t 1 -m -u -i -n -p -- ${CMD}".into(),
        );
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &args).unwrap();

        // Emits a K8sNode entity.
        assert_eq!(update.new_entities.len(), 1);
        let node = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sNode>()
            .unwrap();
        assert_eq!(node.entity_name(), "worker-1");

        // Emits RunsOn + ContainerEscape relations.
        assert_eq!(update.new_relations.len(), 2);
        let runs_on = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "runs-on")
            .unwrap();
        assert_eq!(runs_on.source_id().0, "ns/default/pod/attacker");
        assert_eq!(runs_on.target_id().0, "node/worker-1");

        let escape = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "container.escape")
            .unwrap();
        let escape = escape.as_any().downcast_ref::<ContainerEscape>().unwrap();
        assert_eq!(escape.source_id.0, "ns/default/pod/attacker");
        assert_eq!(escape.target_id.0, "node/worker-1");
        assert_eq!(
            escape.envelope.as_deref(),
            Some("nsenter -t 1 -m -u -i -n -p -- ${CMD}")
        );
        assert!(escape.is_exec_channel());
    }

    #[test]
    fn container_escape_node_is_authoritative_when_target_node_authoritative_flag_set() {
        let mut args = ctx();
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        args.insert("TARGET_NODE_AUTHORITATIVE".into(), "true".into());
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &args).unwrap();
        let node = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sNode>()
            .unwrap();
        assert_eq!(node.name_confidence, NameConfidence::Authoritative);
    }

    #[test]
    fn container_escape_node_is_derived_without_authoritative_flag() {
        let mut args = ctx();
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &args).unwrap();
        let node = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sNode>()
            .unwrap();
        assert_eq!(node.name_confidence, NameConfidence::Derived);
    }

    #[test]
    fn container_escape_creates_placeholder_node_when_node_unknown() {
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &ctx()).unwrap();

        // Should still create a node entity (placeholder) named after the pod's
        // short name — the best available guess before hostname is run.
        assert_eq!(update.new_entities.len(), 1);
        let node = update.new_entities[0]
            .as_any()
            .downcast_ref::<ran_domain::K8sNode>()
            .unwrap();
        assert_eq!(node.entity_name(), "escape_attacker", "placeholder should be escape_<pod-short-name>");
        assert_eq!(node.entity_id().0, "node/escape_attacker");

        // And two relations: RunsOn + ContainerEscape both targeting the placeholder.
        assert_eq!(update.new_relations.len(), 2);
        assert!(update
            .new_relations
            .iter()
            .any(|r| r.relation_name() == "runs-on" && r.target_id().0 == "node/escape_attacker"));
        assert!(update
            .new_relations
            .iter()
            .any(|r| r.relation_name() == "container.escape" && r.target_id().0 == "node/escape_attacker"));
    }

    #[test]
    fn container_escape_sys_resolves_to_target_id() {
        let mut args = ctx();
        args.insert("TARGET_ID".into(), "ns/default/pod/attacker".into());
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        let update = parse_effect("container.escape(sys)", &args).unwrap();
        let escape = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "container.escape")
            .unwrap();
        assert_eq!(escape.source_id().0, "ns/default/pod/attacker");
    }

    #[test]
    fn container_escape_without_procedure_cmd_has_no_envelope() {
        let mut args = ctx();
        args.insert("TARGET_NODE_ID".into(), "node/worker-1".into());
        let update = parse_effect("container.escape(ns/default/pod/attacker)", &args).unwrap();
        let escape = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "container.escape")
            .unwrap()
            .as_any()
            .downcast_ref::<ContainerEscape>()
            .unwrap();
        assert!(escape.envelope.is_none());
    }

    #[test]
    fn container_escape_wrong_arg_count_returns_err() {
        assert!(parse_effect("container.escape(a, b)", &ctx()).is_err());
        assert!(parse_effect("container.escape(a, b, c)", &ctx()).is_err());
        assert!(parse_effect("container.escape()", &ctx()).is_err());
    }
}
