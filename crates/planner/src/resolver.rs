use regex::Regex;
use crate::model::{SelectStrategy, TargetQuery};

/// Extract the entity "kind" from an entity ID string.
/// Entity ID formats:
///   ns/{namespace}/pod/{name}           → "pod"
///   node/{name}                         → "node"
///   sa/{namespace}/{name}               → "serviceaccount"
pub fn entity_kind(entity_id: &str) -> &str {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["node", ..] => "node",
        ["sa", ..] => "serviceaccount",
        ["ns", _, kind, ..] => kind,
        _ => "unknown",
    }
}

fn entity_namespace(entity_id: &str) -> Option<&str> {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["ns", ns, ..] => Some(ns),
        ["sa", ns, ..] => Some(ns),
        _ => None,
    }
}

fn entity_name(entity_id: &str) -> &str {
    entity_id.rsplitn(2, '/').next().unwrap_or(entity_id)
}

/// Resolve a TargetQuery against a list of entity ID strings.
/// Returns the matched entity IDs after applying the select strategy.
/// select=None defaults to Random (returns one element — the first match).
pub fn resolve_target(query: &TargetQuery, entity_ids: &[String]) -> Vec<String> {
    let pattern = match Regex::new(&format!("^{}$", query.name)) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut matches: Vec<String> = entity_ids
        .iter()
        .filter(|id| {
            entity_kind(id).eq_ignore_ascii_case(&query.kind)
                && query.namespace.as_deref()
                    .map(|ns| entity_namespace(id) == Some(ns))
                    .unwrap_or(true)
                && pattern.is_match(entity_name(id))
        })
        .cloned()
        .collect();

    if matches.is_empty() {
        return vec![];
    }

    match query.select.as_ref() {
        Some(SelectStrategy::All) => matches,
        Some(SelectStrategy::First) => {
            matches.sort();
            vec![matches.into_iter().next().unwrap()]
        }
        Some(SelectStrategy::Random) | None => {
            vec![matches.into_iter().next().unwrap()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SelectStrategy, TargetQuery};

    fn query(kind: &str, ns: Option<&str>, name: &str, select: Option<SelectStrategy>) -> TargetQuery {
        TargetQuery {
            kind: kind.into(),
            namespace: ns.map(Into::into),
            name: name.into(),
            select,
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolves_pod_by_regex() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-7d4b9f-xk2jp",
            "ns/default/pod/nginx-7d4b9f-ab3cd",
            "ns/default/pod/redis-abc12",
            "ns/kube-system/pod/coredns-xyz",
        ]);
        let q = query("Pod", Some("default"), "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 1); // None/Random returns one
        assert!(results[0].contains("nginx"));
    }

    #[test]
    fn namespace_filter_applied() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-abc",
            "ns/kube-system/pod/nginx-def",
        ]);
        let q = query("Pod", Some("default"), "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "ns/default/pod/nginx-abc");
    }

    #[test]
    fn no_namespace_matches_all_but_random_returns_one() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-abc",
            "ns/kube-system/pod/nginx-def",
        ]);
        let q = query("Pod", None, "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 1); // Random returns 1
    }

    #[test]
    fn no_namespace_select_all_returns_all() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-abc",
            "ns/kube-system/pod/nginx-def",
        ]);
        let q = query("Pod", None, "nginx-.*", Some(SelectStrategy::All));
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn select_first_returns_one() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-bbb",
            "ns/default/pod/nginx-aaa",
        ]);
        let q = query("Pod", None, "nginx-.*", Some(SelectStrategy::First));
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "ns/default/pod/nginx-aaa"); // lexicographically first
    }

    #[test]
    fn select_all_returns_all() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-aaa",
            "ns/default/pod/nginx-bbb",
        ]);
        let q = query("Pod", None, "nginx-.*", Some(SelectStrategy::All));
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn empty_when_no_match() {
        let entity_ids = ids(&["ns/default/pod/redis-abc"]);
        let q = query("Pod", None, "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert!(results.is_empty());
    }

    #[test]
    fn parses_entity_id_kinds() {
        assert_eq!(entity_kind("ns/default/pod/nginx-abc"), "pod");
        assert_eq!(entity_kind("node/worker-1"), "node");
        assert_eq!(entity_kind("sa/default/my-sa"), "serviceaccount");
    }
}
