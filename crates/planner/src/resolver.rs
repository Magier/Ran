use crate::model::{SelectStrategy, TargetQuery};
use regex::Regex;

/// Derive the regex (unanchored body) that matches the generated pod names of a
/// controller, given only its kind and name. This is the fallback used when live
/// owner references aren't available (name-only discovery or statically-defined
/// environments), where the controlling workload can only be inferred from the
/// Kubernetes generated-name conventions:
///
/// | workload      | pod name shape         |
/// |---------------|------------------------|
/// | Deployment    | `<name>-<hash>-<rand5>`|
/// | ReplicaSet    | `<name>-<rand5>`       |
/// | DaemonSet/Job | `<name>-<rand5>`       |
/// | StatefulSet   | `<name>-<ordinal>`     |
/// | Pod (bare)    | `<name>`               |
///
/// The workload name is regex-escaped; the generated segments contain no hyphens,
/// so segment structure keeps e.g. Deployment `web` from matching pods of `web-api`.
pub fn derive_pod_pattern(workload_kind: &str, workload_name: &str) -> String {
    let n = regex::escape(workload_name);
    match workload_kind.to_ascii_lowercase().as_str() {
        "deployment" => format!("{n}-[a-z0-9]+-[a-z0-9]{{5}}"),
        "replicaset" | "daemonset" | "job" => format!("{n}-[a-z0-9]{{5}}"),
        "statefulset" => format!("{n}-[0-9]+"),
        // Bare pod, or an unknown controller: match the name exactly rather than
        // risk over-matching unrelated entities.
        _ => n,
    }
}

/// Extract the entity "kind" from an entity ID string.
/// Entity ID formats:
///   ns/{namespace}/pod/{name}           → "pod"
///   node/{name}                         → "node"
///   ns/{namespace}/sa/{name}            → "serviceaccount"
pub fn entity_kind(entity_id: &str) -> &str {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["node", ..] => "node",
        ["k8s", "cluster", ..] => "cluster",
        ["ns", _, "sa", ..] => "serviceaccount",
        ["ns", _, kind, ..] => kind,
        _ => "unknown",
    }
}

fn entity_namespace(entity_id: &str) -> Option<&str> {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["ns", ns, ..] => Some(ns),
        _ => None,
    }
}

fn entity_name(entity_id: &str) -> &str {
    entity_id.rsplit('/').next().unwrap_or(entity_id)
}

/// Resolve a TargetQuery against a list of entity ID strings.
/// Returns the matched entity IDs after applying the select strategy.
///
/// Resolution precedence:
///   1. `id`       — exact entity-id match (one result).
///   2. `workload` — derive the controller's pod-name pattern and match by name.
///   3. `name`     — name regex (wildcard mode).
///   4. kind-only  — every entity of `kind` (+ namespace).
///
/// select=None defaults to Random (returns one element — the first match).
pub fn resolve_target(query: &TargetQuery, entity_ids: &[String]) -> Vec<String> {
    // 1. Explicit id wins outright.
    if let Some(id) = query.id.as_deref() {
        return if entity_ids.iter().any(|e| e == id) {
            vec![id.to_string()]
        } else {
            vec![]
        };
    }

    // Determine the kind to match and the name pattern to apply.
    let (kind, name_pattern) = match &query.workload {
        // 2. Workload mode: pods of the named controller. Kind defaults to Pod.
        Some(w) => {
            let kind = if query.kind.is_empty() {
                "pod".to_string()
            } else {
                query.kind.clone()
            };
            (kind, derive_pod_pattern(&w.kind, &w.name))
        }
        None => {
            // 3/4. Name regex, or kind-only (empty name → match any name).
            // Kind defaults to "pod" when omitted — the overwhelmingly common case.
            let kind = if query.kind.is_empty() {
                "pod".to_string()
            } else {
                query.kind.clone()
            };
            let pat = if query.name.is_empty() {
                ".*".to_string()
            } else {
                query.name.clone()
            };
            (kind, pat)
        }
    };

    let pattern = match Regex::new(&format!("^{}$", name_pattern)) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("invalid target name pattern {:?}: {}", name_pattern, e);
            return vec![];
        }
    };

    let mut matches: Vec<String> = entity_ids
        .iter()
        .filter(|id| {
            entity_kind(id).eq_ignore_ascii_case(&kind)
                && query
                    .namespace
                    .as_deref()
                    .map(|ns| entity_namespace(id) == Some(ns))
                    .unwrap_or(true)
                && pattern.is_match(entity_name(id))
        })
        .cloned()
        .collect();

    // Strict workload-derived pod-name matching can miss discovery placeholders
    // (for example `redis.10-0-0-13`) when full controller-generated names are
    // unavailable. Fall back to a prefix-compatible name match inside the same
    // kind/namespace scope.
    if matches.is_empty() {
        if let Some(w) = &query.workload {
            let wl = w.name.as_str();
            matches = entity_ids
                .iter()
                .filter(|id| {
                    if !entity_kind(id).eq_ignore_ascii_case(&kind) {
                        return false;
                    }
                    if !query
                        .namespace
                        .as_deref()
                        .map(|ns| entity_namespace(id) == Some(ns))
                        .unwrap_or(true)
                    {
                        return false;
                    }
                    let name = entity_name(id);
                    if name == wl || name.starts_with(&format!("{}.", wl)) {
                        return true;
                    }

                    // Allow dash placeholders only when they are clearly IP-ish,
                    // e.g. `redis-10-0-0-13`; this avoids matching sibling
                    // deployment names like `app-api` for workload `app`.
                    let Some(suffix) = name.strip_prefix(&format!("{}-", wl)) else {
                        return false;
                    };
                    suffix
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
        }
    }

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
    use crate::model::{SelectStrategy, TargetQuery, WorkloadRef};

    fn query(
        kind: &str,
        ns: Option<&str>,
        name: &str,
        select: Option<SelectStrategy>,
    ) -> TargetQuery {
        TargetQuery {
            kind: kind.into(),
            namespace: ns.map(Into::into),
            name: name.into(),
            select,
            ..Default::default()
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
        let entity_ids = ids(&["ns/default/pod/nginx-abc", "ns/kube-system/pod/nginx-def"]);
        let q = query("Pod", Some("default"), "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "ns/default/pod/nginx-abc");
    }

    #[test]
    fn no_namespace_matches_all_but_random_returns_one() {
        let entity_ids = ids(&["ns/default/pod/nginx-abc", "ns/kube-system/pod/nginx-def"]);
        let q = query("Pod", None, "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 1); // Random returns 1
    }

    #[test]
    fn no_namespace_select_all_returns_all() {
        let entity_ids = ids(&["ns/default/pod/nginx-abc", "ns/kube-system/pod/nginx-def"]);
        let q = query("Pod", None, "nginx-.*", Some(SelectStrategy::All));
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn select_first_returns_one() {
        let entity_ids = ids(&["ns/default/pod/nginx-bbb", "ns/default/pod/nginx-aaa"]);
        let q = query("Pod", None, "nginx-.*", Some(SelectStrategy::First));
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "ns/default/pod/nginx-aaa"); // lexicographically first
    }

    #[test]
    fn select_all_returns_all() {
        let entity_ids = ids(&["ns/default/pod/nginx-aaa", "ns/default/pod/nginx-bbb"]);
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
        assert_eq!(entity_kind("ns/default/sa/my-sa"), "serviceaccount");
        assert_eq!(entity_kind("k8s/cluster/ixi-prod"), "cluster");
    }

    #[test]
    fn resolves_cluster_by_kind() {
        let entity_ids = ids(&["k8s/cluster/ixi-prod", "c2/Ran"]);
        let q = TargetQuery {
            kind: "Cluster".into(),
            ..Default::default()
        };
        assert_eq!(resolve_target(&q, &entity_ids), vec!["k8s/cluster/ixi-prod"]);
    }

    fn workload_query(kind: &str, ns: Option<&str>, wl_kind: &str, wl_name: &str) -> TargetQuery {
        TargetQuery {
            kind: kind.into(),
            namespace: ns.map(Into::into),
            workload: Some(WorkloadRef {
                kind: wl_kind.into(),
                name: wl_name.into(),
            }),
            select: Some(SelectStrategy::All),
            ..Default::default()
        }
    }

    #[test]
    fn explicit_id_matches_exactly() {
        let entity_ids = ids(&["ns/web/pod/app-7d4b9f-xk2jp", "ns/web/pod/app-7d4b9f-aa111"]);
        let q = TargetQuery {
            id: Some("ns/web/pod/app-7d4b9f-xk2jp".into()),
            ..Default::default()
        };
        assert_eq!(resolve_target(&q, &entity_ids), vec!["ns/web/pod/app-7d4b9f-xk2jp"]);
    }

    #[test]
    fn explicit_id_absent_returns_empty() {
        let entity_ids = ids(&["ns/web/pod/app-7d4b9f-xk2jp"]);
        let q = TargetQuery {
            id: Some("ns/web/pod/nope".into()),
            ..Default::default()
        };
        assert!(resolve_target(&q, &entity_ids).is_empty());
    }

    #[test]
    fn workload_deployment_matches_generated_pod_names() {
        let entity_ids = ids(&[
            "ns/web/pod/app-7d4b9f-xk2jp",
            "ns/web/pod/app-7d4b9f-aa111",
            "ns/web/pod/app-api-5c8d7-bb222", // different deployment (app-api)
            "ns/web/pod/other-1a2b3-cc333",
        ]);
        let q = workload_query("Pod", Some("web"), "Deployment", "app");
        let mut got = resolve_target(&q, &entity_ids);
        got.sort();
        assert_eq!(got, vec!["ns/web/pod/app-7d4b9f-aa111", "ns/web/pod/app-7d4b9f-xk2jp"]);
    }

    #[test]
    fn workload_kind_defaults_to_pod() {
        let entity_ids = ids(&["ns/web/pod/app-7d4b9f-xk2jp"]);
        let q = TargetQuery {
            workload: Some(WorkloadRef {
                kind: "Deployment".into(),
                name: "app".into(),
            }),
            select: Some(SelectStrategy::All),
            ..Default::default()
        };
        assert_eq!(resolve_target(&q, &entity_ids), vec!["ns/web/pod/app-7d4b9f-xk2jp"]);
    }

    #[test]
    fn workload_statefulset_matches_ordinal_names() {
        let entity_ids = ids(&[
            "ns/db/pod/postgres-0",
            "ns/db/pod/postgres-1",
            "ns/db/pod/postgres-7d4b9f-xk2jp", // not a stateful pod name
        ]);
        let q = workload_query("Pod", Some("db"), "StatefulSet", "postgres");
        let mut got = resolve_target(&q, &entity_ids);
        got.sort();
        assert_eq!(got, vec!["ns/db/pod/postgres-0", "ns/db/pod/postgres-1"]);
    }

    #[test]
    fn workload_does_not_overmatch_similarly_named_deployment() {
        let entity_ids = ids(&["ns/web/pod/app-api-5c8d7-bb222"]);
        // Querying Deployment "app" must NOT match pods of Deployment "app-api".
        let q = workload_query("Pod", Some("web"), "Deployment", "app");
        assert!(resolve_target(&q, &entity_ids).is_empty());
    }

    #[test]
    fn workload_fallback_matches_dot_and_dash_prefixed_placeholders() {
        let entity_ids = ids(&[
            "ns/oopservability/pod/redis.10-0-0-13",
            "ns/oopservability/pod/redis-10-0-0-44",
            "ns/oopservability/pod/notredis-abc",
        ]);
        let q = workload_query("Pod", Some("oopservability"), "Deployment", "redis");
        let mut got = resolve_target(&q, &entity_ids);
        got.sort();
        assert_eq!(
            got,
            vec![
                "ns/oopservability/pod/redis-10-0-0-44".to_string(),
                "ns/oopservability/pod/redis.10-0-0-13".to_string(),
            ]
        );
    }

    #[test]
    fn kind_only_matches_all_of_kind() {
        let entity_ids = ids(&["ns/web/pod/a-1", "ns/web/pod/b-2", "ns/web/sa/c"]);
        let q = TargetQuery {
            kind: "Pod".into(),
            select: Some(SelectStrategy::All),
            ..Default::default()
        };
        assert_eq!(resolve_target(&q, &entity_ids).len(), 2);
    }

    #[test]
    fn derive_pod_pattern_shapes() {
        assert_eq!(derive_pod_pattern("Deployment", "app"), "app-[a-z0-9]+-[a-z0-9]{5}");
        assert_eq!(derive_pod_pattern("DaemonSet", "node-exp"), "node\\-exp-[a-z0-9]{5}");
        assert_eq!(derive_pod_pattern("StatefulSet", "pg"), "pg-[0-9]+");
        assert_eq!(derive_pod_pattern("Pod", "lone"), "lone");
    }
}
