use std::collections::HashMap;

use ran_domain::{Entity, EntityId, KubeletExecSource, Pod, PodExec, RceCanExec, Relation, RunsOn};

use crate::grounding::resolve_go_template;

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
    pub entity_aliases: Vec<(EntityId, EntityId)>,
}

impl FactsUpdate {
    pub fn merge(&mut self, other: Self) {
        for entity in other.new_entities {
            let id = entity.entity_id();
            let exists = self.new_entities.iter().any(|e| e.entity_id() == id);
            if !exists {
                self.new_entities.push(entity);
            }
        }

        for rel in other.new_relations {
            let name = rel.relation_name().to_string();
            let source_id = rel.source_id().0.clone();
            let target_id = rel.target_id().0.clone();

            let exists = self.new_relations.iter().any(|r| {
                r.relation_name() == name
                    && r.source_id().0 == source_id
                    && r.target_id().0 == target_id
            });
            if !exists {
                self.new_relations.push(rel);
            }
        }

        for alias in other.entity_aliases {
            let exists = self.entity_aliases.iter().any(|(s, p)| *s == alias.0 && *p == alias.1);
            if !exists {
                self.entity_aliases.push(alias);
            }
        }
    }
}

pub fn ground_template(template: &str, args: &HashMap<String, String>) -> String {
    // Pass 1: evaluate Go-style {{ if/else/end }} blocks and {{.Var}} substitutions.
    let mut grounded = resolve_go_template(template, args);

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
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"]).ok_or_else(|| {
        "k8s.Pod effect requires Namespace argument".to_string()
    })?;

    let pod_name = get_arg(args, &["PodName", "PODNAME", "POD_NAME"]).ok_or_else(|| {
        "k8s.Pod effect requires PodName argument".to_string()
    })?;

    let mut pod = Pod::new(pod_name, namespace);

    if let Some(node_name) = get_arg(args, &[
        "NodeName",
        "NODENAME",
        "NODE_NAME",
        "Node",
        "NODE",
    ]) {
        if !node_name.trim().is_empty() {
            pod.node_name = Some(node_name.to_string());
        }
    }

    if let Some(sa_name) = get_arg(args, &[
        "ServiceAccount",
        "SERVICEACCOUNT",
        "SERVICE_ACCOUNT",
        "ServiceAccountName",
    ]) {
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
        entity_aliases: Vec::new(),
    })
}

fn parse_relation_effect(effect: &str, ctx: &HashMap<String, String>) -> Result<ParsedStructuralEffect, String> {
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
        _ => None,
    }
}

fn resolve_relation_effect_handler(effect_name: &str) -> Option<RelationEffectHandler> {
    match normalize_effect_name(effect_name).as_str() {
        "k8s.can-exec" => Some(parse_k8s_can_exec_relation),
        "k8s.runs-on" | "runs-on" => Some(parse_runs_on_relation),
        "k8s.kubelet-exec-source" | "k8s.kubelet-exec" => Some(parse_kubelet_exec_source_relation),
        "rce.can-exec" => Some(parse_rce_can_exec_relation),
        _ => None,
    }
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
        entity_aliases: Vec::new(),
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
        entity_aliases: Vec::new(),
    })
}

fn parse_kubelet_exec_source_relation(
    args: &[&str],
    _ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("kubelet-exec effect expects exactly 2 args".to_string());
    }
    let rel = KubeletExecSource::new(args[0], args[1]);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: Vec::new(),
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
    let envelope = ctx.get("PROCEDURE_CMD").filter(|v| !v.trim().is_empty()).cloned();
    let rel = RceCanExec::new(args[0], args[1]).with_opt_envelope(envelope);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: Vec::new(),
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
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "running")
}
