use std::collections::HashMap;

use ran_domain::{PodExec, Entity, Pod, Relation};

#[derive(Default)]
pub struct FactsUpdate {
    pub new_entities: Vec<Box<dyn Entity + Send + Sync>>,
    pub new_relations: Vec<Box<dyn Relation + Send + Sync>>,
}

impl FactsUpdate {
    pub fn merge(&mut self, other: Self) {
        self.new_entities.extend(other.new_entities);
        self.new_relations.extend(other.new_relations);
    }
}

pub fn ground_template(template: &str, args: &HashMap<String, String>) -> String {
    let mut grounded = template.to_string();

    for (k, v) in args {
        let key = format!("${{{}}}", k.to_uppercase());
        grounded = grounded.replace(&key, v);
    }

    grounded
}

pub fn parse_effect(effect: &str, args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let normalized = effect.trim();

    if normalized.eq_ignore_ascii_case("k8s.pod") {
        return parse_k8s_pod(args);
    }

    if normalized.contains('(') && normalized.ends_with(')') {
        return parse_relation_effect(normalized);
    }

    Ok(FactsUpdate::default())
}

fn parse_k8s_pod(args: &HashMap<String, String>) -> Result<FactsUpdate, String> {
    let namespace = get_arg(args, &["Namespace", "NAMESPACE"]).ok_or_else(|| {
        "k8s.Pod effect requires Namespace argument".to_string()
    })?;

    let pod_name = get_arg(args, &["PodName", "PODNAME", "POD_NAME"]).ok_or_else(|| {
        "k8s.Pod effect requires PodName argument".to_string()
    })?;

    let pod = Pod::new(pod_name, namespace);

    Ok(FactsUpdate {
        new_entities: vec![Box::new(pod)],
        new_relations: Vec::new(),
    })
}

fn parse_relation_effect(effect: &str) -> Result<FactsUpdate, String> {
    let (name, args) = split_relation(effect)?;

    if name.eq_ignore_ascii_case("k8s.can-exec") {
        if args.len() != 2 {
            return Err("k8s.can-exec effect expects exactly 2 args".to_string());
        }

        let rel = PodExec::new(args[0], args[1]);

        return Ok(FactsUpdate {
            new_entities: Vec::new(),
            new_relations: vec![Box::new(rel)],
        });
    }

    Ok(FactsUpdate::default())
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
