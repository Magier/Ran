use std::collections::HashMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ran_domain::{
    Contains, Entity, JwToken, K8sCredential, K8sNode, NameConfidence, Namespace, Pod,
    RbacPermission, RunsOn, ServiceAccount, ServiceAccountToken, Uses,
};
use serde::Deserialize;

use super::ParserOutput;
use crate::FactsUpdate;

pub(super) fn register(m: &mut HashMap<&'static str, super::ParserFn>) {
    m.insert("rawserviceaccounttoken", parse_raw_service_account_token);
}

/// Internal structs for deserializing the Kubernetes JWT payload.
#[derive(Debug, Deserialize)]
struct JwtPayload {
    sub: Option<String>,
    #[serde(default)]
    aud: serde_json::Value,
    iss: Option<String>,
    exp: Option<i64>,
    iat: Option<i64>,
    #[serde(rename = "kubernetes.io")]
    kubernetes: Option<KubernetesPayload>,
    // Legacy (non-projected) SA token fields.
    #[serde(rename = "kubernetes.io/serviceaccount/namespace")]
    legacy_namespace: Option<String>,
    #[serde(rename = "kubernetes.io/serviceaccount/service-account.name")]
    legacy_sa_name: Option<String>,
    #[serde(rename = "kubernetes.io/serviceaccount/service-account.uid")]
    legacy_sa_uid: Option<String>,
    #[serde(rename = "kubernetes.io/serviceaccount/pod.name")]
    legacy_pod_name: Option<String>,
    #[serde(rename = "kubernetes.io/serviceaccount/pod.uid")]
    legacy_pod_uid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KubernetesPayload {
    namespace: Option<String>,
    pod: Option<ResourceRef>,
    node: Option<ResourceRef>,
    serviceaccount: Option<ResourceRef>,
}

#[derive(Debug, Deserialize)]
struct ResourceRef {
    name: Option<String>,
    uid: Option<String>,
}

/// Deserializable form of the Kubernetes `SelfSubjectRulesReview` API response.
#[derive(Debug, Deserialize)]
struct SsrrResponse {
    status: Option<SsrrStatus>,
    code: Option<u32>,
    message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SsrrStatus {
    #[serde(rename = "resourceRules", default)]
    resource_rules: Vec<SsrrResourceRule>,
    #[serde(rename = "nonResourceRules", default)]
    non_resource_rules: Vec<SsrrNonResourceRule>,
    #[serde(default)]
    incomplete: bool,
}

#[derive(Debug, Deserialize)]
struct SsrrResourceRule {
    #[serde(default)]
    verbs: Vec<String>,
    #[serde(rename = "apiGroups", default)]
    api_groups: Vec<String>,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(rename = "resourceNames", default)]
    resource_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SsrrNonResourceRule {
    #[serde(default)]
    verbs: Vec<String>,
    #[serde(rename = "nonResourceURLs", default)]
    non_resource_urls: Vec<String>,
}

/// Parse a raw Kubernetes ServiceAccount JWT from stdout and produce new
/// entities (ServiceAccount, Namespace, Pod, Node) and relations.
///
/// Mirrors Go's `parseRawServiceAccountToken` + `analyzeServiceAccountToken`.
///
/// Handles multi-line stdout: searches for the first line containing `ey`
/// and `.`, which is the hallmark of a base64url-encoded JWT.
fn parse_raw_service_account_token(
    stdout: &str,
    _stderr: &str,
    args: &HashMap<String, String>,
) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty output — no token provided".to_string());
    }

    // Find the JWT within possibly multi-line output.
    let token_str = find_jwt_in_output(stdout);
    if token_str.is_empty() {
        return ParserOutput::KnownFailure("could not locate a JWT token in output".to_string());
    }

    // Decode the JWT payload (second of three dot-separated segments).
    let parts: Vec<&str> = token_str.splitn(3, '.').collect();
    if parts.len() != 3 {
        return ParserOutput::UnknownFormat(format!(
            "expected 3 JWT segments, got {}",
            parts.len()
        ));
    }

    let payload_bytes = match URL_SAFE_NO_PAD.decode(parts[1]) {
        Ok(b) => b,
        Err(e) => {
            return ParserOutput::UnknownFormat(format!("failed to base64-decode JWT payload: {e}"))
        }
    };

    let payload: JwtPayload = match serde_json::from_slice(&payload_bytes) {
        Ok(p) => p,
        Err(e) => {
            return ParserOutput::UnknownFormat(format!("failed to parse JWT payload JSON: {e}"))
        }
    };

    // Resolve namespace and SA name from either projected or legacy claims.
    let (namespace, sa_name, sa_uid, pod_name, pod_uid, node_name) = resolve_k8s_claims(&payload);

    if namespace.is_empty() || sa_name.is_empty() {
        return ParserOutput::UnknownFormat(
            "JWT payload missing required kubernetes namespace or serviceaccount claims"
                .to_string(),
        );
    }

    // Guard against false positives: when the action target is a specific
    // ServiceAccount entity (ns/<ns>/sa/<name>), the decoded token must match
    // that identity. Otherwise the command likely ran on the wrong pod.
    if let Some(expected_target) = args.get("TARGET_ID") {
        if let Some((expected_sa_name, expected_ns)) =
            parse_sa_identity_from_target(expected_target)
        {
            if expected_sa_name != sa_name || expected_ns != namespace {
                return ParserOutput::KnownFailure(format!(
                    "decoded SA token for {}/{} but target is {}/{}",
                    namespace, sa_name, expected_ns, expected_sa_name
                ));
            }
        }

        if let Some((expected_pod_name, expected_pod_ns)) =
            parse_pod_identity_from_target(expected_target)
        {
            if let Some(decoded_pod_name) = pod_name.as_deref() {
                if !decoded_pod_name.is_empty()
                    && !is_ip_placeholder_pod_name(expected_pod_name)
                    && (decoded_pod_name != expected_pod_name || namespace != expected_pod_ns)
                {
                    return ParserOutput::KnownFailure(format!(
                        "decoded SA token for pod {}/{} but target pod is {}/{}",
                        namespace, decoded_pod_name, expected_pod_ns, expected_pod_name
                    ));
                }
            }
        }
    }

    // Build the audience list for JwToken.
    let audience = match &payload.aud {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        serde_json::Value::String(s) => vec![s.clone()],
        _ => vec![],
    };

    let jwt = JwToken {
        raw: token_str.to_string(),
        subject: payload.sub.clone(),
        audience,
        issuer: payload.iss.clone(),
        expires_at: payload.exp,
        issued_at: payload.iat,
    };

    let is_bound = pod_uid.as_deref().map(|u| !u.is_empty()).unwrap_or(false);

    let token = ServiceAccountToken {
        jwt,
        namespace: namespace.clone(),
        service_account_name: sa_name.clone(),
        service_account_uid: sa_uid,
        pod_name: pod_name.clone(),
        pod_uid,
        is_bound,
    };

    // Assemble FactsUpdate.
    let mut facts = FactsUpdate::default();

    // Namespace.
    let ns = Namespace::new(&namespace);
    let ns_id = ns.entity_id();
    facts.new_entities.push(Box::new(ns));

    // ServiceAccount (with token).
    let mut sa = ServiceAccount::new(&sa_name, &namespace);
    sa.token = Some(token);
    let sa_id = sa.entity_id();
    facts.new_entities.push(Box::new(sa));

    // Contains: namespace → SA.
    facts
        .new_relations
        .push(Box::new(Contains::new(ns_id.0.clone(), sa_id.0.clone())));

    // Pod (if the token carries pod claims — always true for bound tokens and
    // most legacy tokens that include pod info).
    if let Some(pod_name) = &pod_name {
        if !pod_name.is_empty() {
            let mut pod = Pod::new(pod_name.as_str(), namespace.as_str());
            pod.meta.name_confidence = NameConfidence::Authoritative;
            pod.service_account_name = Some(sa_name.clone());
            pod.is_running = true;
            let pod_id = pod.entity_id();

            // If bound, attach the node name.
            if let Some(node_name) = &node_name {
                if !node_name.is_empty() {
                    pod.node_name = Some(node_name.clone());

                    let mut node = K8sNode::new(node_name.as_str());
                    node.name_confidence = NameConfidence::Authoritative;
                    let node_id = node.entity_id();
                    facts.new_entities.push(Box::new(node));
                    facts
                        .new_relations
                        .push(Box::new(RunsOn::new(pod_id.0.clone(), node_id.0.clone())));
                }
            }

            facts.new_entities.push(Box::new(pod));

            // Uses: pod → SA.
            facts
                .new_relations
                .push(Box::new(Uses::new(pod_id.0.clone(), sa_id.0.clone())));
        }
    }

    let entity_count = facts.new_entities.len();
    let relation_count = facts.new_relations.len();
    let detail = format!(
        "decoded SA token for {}/{}: {} entities, {} relations",
        namespace, sa_name, entity_count, relation_count
    );

    ParserOutput::SuccessWithFacts(facts, detail)
}

/// Extract the JWT string from possibly multi-line output.
///
/// A JWT starts with a base64url-encoded header, so the first segment always
/// starts with `ey` (base64 of `{"`).  The token must also contain at least
/// two `.` separators.
fn find_jwt_in_output(stdout: &str) -> &str {
    // Single-line (common case): the whole trimmed output is the token.
    let trimmed = stdout.trim();
    if !trimmed.contains('\n') {
        return trimmed;
    }

    // Multi-line: search for the JWT line.
    for line in stdout.lines() {
        let line = line.trim();
        if line.contains("ey") && line.contains('.') {
            return line;
        }
    }

    ""
}

/// Resolve Kubernetes claims from a JWT payload, supporting both projected
/// (new-style `kubernetes.io` claim) and legacy flat claim formats.
///
/// Returns `(namespace, sa_name, sa_uid, pod_name, pod_uid, node_name)`.
fn resolve_k8s_claims(
    payload: &JwtPayload,
) -> (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    if let Some(k8s) = &payload.kubernetes {
        let namespace = k8s.namespace.clone().unwrap_or_default();
        let sa_name = k8s
            .serviceaccount
            .as_ref()
            .and_then(|sa| sa.name.clone())
            .unwrap_or_default();
        let sa_uid = k8s.serviceaccount.as_ref().and_then(|sa| sa.uid.clone());
        let pod_name = k8s.pod.as_ref().and_then(|p| p.name.clone());
        let pod_uid = k8s.pod.as_ref().and_then(|p| p.uid.clone());
        let node_name = k8s.node.as_ref().and_then(|n| n.name.clone());
        (namespace, sa_name, sa_uid, pod_name, pod_uid, node_name)
    } else {
        // Legacy flat claims.
        let namespace = payload.legacy_namespace.clone().unwrap_or_default();
        let sa_name = payload.legacy_sa_name.clone().unwrap_or_default();
        let sa_uid = payload.legacy_sa_uid.clone();
        let pod_name = payload.legacy_pod_name.clone();
        let pod_uid = payload.legacy_pod_uid.clone();
        (namespace, sa_name, sa_uid, pod_name, pod_uid, None)
    }
}

/// Parse a `k8s.SelfSubjectRulesReview` effect output into RBAC entitlements
/// on the target ServiceAccount.
///
/// Supports two output formats:
/// - JSON: the raw Kubernetes API response from `curl … /selfsubjectrulesreviews`
/// - Pretty: the tabular output of `kubectl auth can-i --list`
pub(super) fn parse_self_subject_rules_review(
    stdout: &str,
    _stderr: &str,
    auth_identity_id: &str,
    namespace_arg: &str,
) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty output".to_string());
    }

    // Determine format and extract rules.
    let rules = if serde_json::from_str::<serde_json::Value>(stdout.trim()).is_ok() {
        let resp: SsrrResponse = match serde_json::from_str(stdout.trim()) {
            Ok(r) => r,
            Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
        };
        if resp.code.map(|c| c >= 400).unwrap_or(false) {
            return ParserOutput::KnownFailure(format!(
                "SelfSubjectRulesReview API error (code {}): {}",
                resp.code.unwrap_or(0),
                resp.message.unwrap_or_default()
            ));
        }
        let status = resp.status.unwrap_or_default();
        if status.incomplete {
            tracing::warn!("SelfSubjectRulesReview results are incomplete");
        }
        (status.resource_rules, status.non_resource_rules)
    } else {
        match parse_kubectl_ssrr_table(stdout) {
            Ok(rules) => rules,
            Err(e) => return ParserOutput::UnknownFormat(format!("pretty-print parse error: {e}")),
        }
    };
    let (resource_rules, non_resource_rules) = rules;

    // Kubeconfig reviews are attributed directly to the selected credential.
    // Service-account reviews retain the existing JWT/target-ID resolution.
    let credential_name = auth_identity_id.strip_prefix("k8s/credential/");
    let service_account = if credential_name.is_some() {
        None
    } else {
        parse_sa_identity_from_target(auth_identity_id)
    };
    if credential_name.is_none() && service_account.is_none() {
        return ParserOutput::KnownFailure(format!(
            "cannot resolve RBAC identity '{auth_identity_id}': \
             Authenticate As must reference a K8sCredential or ServiceAccount"
        ));
    }
    let permission_namespace = credential_name
        .map(|_| namespace_arg.trim())
        .or_else(|| {
            service_account
                .as_ref()
                .map(|(_, namespace)| namespace.as_str())
        })
        .unwrap_or("");
    let mut entitlements: Vec<RbacPermission> = Vec::new();

    for rule in &resource_rules {
        for verb in &rule.verbs {
            for resource in &rule.resources {
                let api_groups: &[String] = if rule.api_groups.is_empty() {
                    &[]
                } else {
                    &rule.api_groups
                };

                // Treat empty api_groups slice as a single entry with the core group ("").
                let effective_groups: Vec<&str> = if api_groups.is_empty() {
                    vec![""]
                } else {
                    api_groups.iter().map(String::as_str).collect()
                };

                for api_group in &effective_groups {
                    let scope = if is_namespaced_resource(resource, api_group)
                        && !permission_namespace.is_empty()
                    {
                        Some(permission_namespace.to_string())
                    } else {
                        None
                    };

                    if rule.resource_names.is_empty() {
                        let mut perm = RbacPermission::new(verb, resource);
                        perm.api_group = Some(api_group.to_string());
                        perm.scope = scope;
                        entitlements.push(perm);
                    } else {
                        for resource_name in &rule.resource_names {
                            let mut perm = RbacPermission::new(verb, resource);
                            perm.api_group = Some(api_group.to_string());
                            perm.resource_name = Some(resource_name.clone());
                            perm.scope = scope.clone();
                            entitlements.push(perm);
                        }
                    }
                }
            }
        }
    }

    for rule in &non_resource_rules {
        for verb in &rule.verbs {
            for url in &rule.non_resource_urls {
                let mut perm = RbacPermission::new(verb, "");
                perm.resource_name = Some(url.clone());
                entitlements.push(perm);
            }
        }
    }

    let perm_count = entitlements.len();
    let mut facts = FactsUpdate::default();
    if let Some(name) = credential_name {
        let mut credential = K8sCredential::new("").with_name(name);
        credential.entitlements = entitlements;
        facts.new_entities.push(Box::new(credential));
    } else if let Some((sa_name, sa_namespace)) = service_account {
        let mut sa = ServiceAccount::new(sa_name, sa_namespace);
        sa.entitlements = entitlements;
        facts.new_entities.push(Box::new(sa));
    }

    ParserOutput::SuccessWithFacts(
        facts,
        format!(
            "parsed {} RBAC permission(s) from SelfSubjectRulesReview",
            perm_count
        ),
    )
}

/// Parse the tabular output of `kubectl auth can-i --list` into resource and
/// non-resource rule lists.
///
/// The format uses `[...]` delimiters for three of the four columns:
/// ```text
/// Resources   Non-Resource URLs   Resource Names   Verbs
/// pods        []                  []               [get list]
///             [/api]              []               [get]
/// ```
fn parse_kubectl_ssrr_table(
    data: &str,
) -> Result<(Vec<SsrrResourceRule>, Vec<SsrrNonResourceRule>), String> {
    let mut resource_rules = Vec::new();
    let mut non_resource_rules = Vec::new();

    let mut lines = data.lines();
    // Skip the header row.
    let _ = lines.next();

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }

        // The three bracketed columns are delimited by `[`.  Split into at most 4
        // parts: [resources_col, urls_col, names_col, verbs_col].
        let parts: Vec<&str> = line.splitn(4, '[').collect();
        if parts.len() < 4 {
            continue;
        }

        fn strip_bracket_and_split(s: &str) -> Vec<String> {
            let cleaned = s.trim().trim_end_matches(']');
            if cleaned.is_empty() {
                vec![]
            } else {
                cleaned.split_whitespace().map(String::from).collect()
            }
        }

        let resources_raw = parts[0].trim();
        let non_resource_urls = strip_bracket_and_split(parts[1]);
        let resource_names = strip_bracket_and_split(parts[2]);
        let verbs = strip_bracket_and_split(parts[3]);

        if verbs.is_empty() {
            continue;
        }

        if resources_raw.is_empty() {
            // Non-resource rule — the resources column is blank.
            non_resource_rules.push(SsrrNonResourceRule {
                verbs,
                non_resource_urls,
            });
        } else {
            // Resource rule — split `resource.apiGroup` from the resources column.
            let (resource, api_group) = split_resource_api_group(resources_raw);
            resource_rules.push(SsrrResourceRule {
                verbs,
                api_groups: vec![api_group],
                resources: vec![resource],
                resource_names,
            });
        }
    }

    Ok((resource_rules, non_resource_rules))
}

/// Split a `resource[.apiGroup]` string from kubectl pretty output into
/// `(resource, apiGroup)`.
///
/// | Input | resource | apiGroup |
/// |-------|----------|----------|
/// | `*.*` | `*` | `*` |
/// | `pods` | `pods` | `""` |
/// | `pods/exec` | `pods/exec` | `""` |
/// | `selfsubjectrulesreviews.authorization.k8s.io` | `selfsubjectrulesreviews` | `authorization.k8s.io` |
fn split_resource_api_group(s: &str) -> (String, String) {
    if s == "*.*" {
        return ("*".to_string(), "*".to_string());
    }
    if let Some(dot) = s.find('.') {
        // Only treat `.` as an apiGroup separator when the resource part has no
        // `/` (subresource), e.g. `pods/exec` should not be split.
        let resource_part = &s[..dot];
        if !resource_part.contains('/') {
            return (resource_part.to_string(), s[dot + 1..].to_string());
        }
    }
    (s.to_string(), String::new())
}

/// Parse a ServiceAccount identity `(sa_name, namespace)` from an entity ID string.
///
/// Handles the canonical `ns/{namespace}/sa/{name}` format. Returns `None` for
/// pod IDs or any other format — those require a TOKEN arg to identify the SA.
fn parse_sa_identity_from_target(target_id: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = target_id.splitn(5, '/').collect();
    if parts.len() == 4 && parts[0] == "ns" && parts[2] == "sa" {
        Some((parts[3].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

/// Parse a pod identity `(pod_name, namespace)` from an entity ID string.
///
/// Handles the canonical `ns/{namespace}/pod/{name}` format.
fn parse_pod_identity_from_target(target_id: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = target_id.splitn(5, '/').collect();
    if parts.len() == 4 && parts[0] == "ns" && parts[2] == "pod" {
        Some((parts[3], parts[1]))
    } else {
        None
    }
}

/// Heuristic for placeholder pod IDs derived from network discovery, e.g.
/// `redis.10-0-0-35`.
fn is_ip_placeholder_pod_name(name: &str) -> bool {
    let Some((_, suffix)) = name.rsplit_once('.') else {
        return false;
    };
    let octets: Vec<&str> = suffix.split('-').collect();
    octets.len() == 4
        && octets
            .iter()
            .all(|o| !o.is_empty() && o.chars().all(|c| c.is_ascii_digit()))
}

/// Returns `true` when `resource` in `api_group` is namespaced.
///
/// Unknown resources default to `true` (namespaced).  Wildcards (`"*"`) span
/// both scopes — treated as cluster-scoped (`false`) to avoid over-constraining
/// the permission scope.
fn is_namespaced_resource(resource: &str, api_group: &str) -> bool {
    if resource == "*" || api_group == "*" {
        return false;
    }

    let name = resource.to_ascii_lowercase();
    let group = api_group.to_ascii_lowercase();

    let cluster_scoped: &[(&str, &[&str])] = &[
        (
            "",
            &[
                "componentstatuses",
                "componentstatus",
                "namespaces",
                "namespace",
                "nodes",
                "node",
                "persistentvolumes",
                "persistentvolume",
            ],
        ),
        (
            "admissionregistration.k8s.io",
            &[
                "mutatingwebhookconfigurations",
                "mutatingwebhookconfiguration",
                "validatingadmissionpolicies",
                "validatingadmissionpolicy",
                "validatingadmissionpolicybindings",
                "validatingadmissionpolicybinding",
                "validatingwebhookconfigurations",
                "validatingwebhookconfiguration",
            ],
        ),
        (
            "apiextensions.k8s.io",
            &["customresourcedefinitions", "customresourcedefinition"],
        ),
        ("apiregistration.k8s.io", &["apiservices", "apiservice"]),
        (
            "authentication.k8s.io",
            &[
                "selfsubjectreviews",
                "selfsubjectreview",
                "tokenreviews",
                "tokenreview",
            ],
        ),
        (
            "authorization.k8s.io",
            &[
                "selfsubjectaccessreviews",
                "selfsubjectaccessreview",
                "selfsubjectrulesreviews",
                "selfsubjectrulesreview",
                "subjectaccessreviews",
                "subjectaccessreview",
            ],
        ),
        (
            "certificates.k8s.io",
            &["certificatesigningrequests", "certificatesigningrequest"],
        ),
        (
            "flowcontrol.apiserver.k8s.io",
            &[
                "flowschemas",
                "flowschema",
                "prioritylevelconfigurations",
                "prioritylevelconfiguration",
            ],
        ),
        (
            "networking.k8s.io",
            &[
                "ingressclasses",
                "ingressclass",
                "ipaddresses",
                "ipaddress",
                "servicecidrs",
                "servicecidr",
            ],
        ),
        ("node.k8s.io", &["runtimeclasses", "runtimeclass"]),
        (
            "rbac.authorization.k8s.io",
            &[
                "clusterrolebindings",
                "clusterrolebinding",
                "clusterroles",
                "clusterrole",
            ],
        ),
        (
            "resource.k8s.io",
            &[
                "deviceclasses",
                "deviceclass",
                "resourceslices",
                "resourceslice",
            ],
        ),
        ("scheduling.k8s.io", &["priorityclasses", "priorityclass"]),
        (
            "storage.k8s.io",
            &[
                "csidrivers",
                "csidriver",
                "csinodes",
                "csinode",
                "storageclasses",
                "storageclass",
                "volumeattachments",
                "volumeattachment",
                "volumeattributesclasses",
                "volumeattributesclass",
            ],
        ),
    ];

    for (g, names) in cluster_scoped {
        if group == *g && names.contains(&name.as_str()) {
            return false; // cluster-scoped
        }
    }

    true // default: namespaced
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use ran_domain::{Contains, RunsOn, ServiceAccount};

    /// Build a minimal JWT string with the given JSON payload (no real signature).
    fn make_jwt(payload_json: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload_json);
        format!("{}.{}.fakesig", header, payload)
    }

    #[test]
    fn parse_raw_sa_token_projected_creates_entities_and_relations() {
        let payload = r#"{
            "aud": ["https://kubernetes.default.svc.cluster.local"],
            "exp": 9999999999,
            "iat": 1000000000,
            "iss": "https://kubernetes.default.svc.cluster.local",
            "kubernetes.io": {
                "namespace": "prod",
                "node": {"name": "worker-1", "uid": "node-uid-1"},
                "pod": {"name": "api-pod", "uid": "pod-uid-1"},
                "serviceaccount": {"name": "api-sa", "uid": "sa-uid-1"}
            },
            "sub": "system:serviceaccount:prod:api-sa"
        }"#;
        let jwt = make_jwt(payload);
        let result = parse_raw_service_account_token(&jwt, "", &HashMap::new());

        let ParserOutput::SuccessWithFacts(facts, detail) = result else {
            panic!("expected SuccessWithFacts, got {:?}", result);
        };

        assert!(detail.contains("prod/api-sa"));

        // Namespace, ServiceAccount, Pod, K8sNode
        assert_eq!(facts.new_entities.len(), 4);
        assert!(facts
            .new_entities
            .iter()
            .any(|e| e.entity_kind() == "Namespace" && e.entity_name() == "prod"));
        assert!(facts
            .new_entities
            .iter()
            .any(|e| e.entity_kind() == "ServiceAccount" && e.entity_name() == "api-sa"));
        assert!(facts
            .new_entities
            .iter()
            .any(|e| e.entity_kind() == "Pod" && e.entity_name() == "api-pod"));
        assert!(facts
            .new_entities
            .iter()
            .any(|e| e.entity_kind() == "Node" && e.entity_name() == "worker-1"));

        // Contains (ns→sa), Uses (pod→sa), RunsOn (pod→node)
        assert_eq!(facts.new_relations.len(), 3);
        assert!(facts.new_relations.iter().any(|r| r.is::<Contains>()));
        assert!(facts.new_relations.iter().any(|r| r.is::<Uses>()));
        assert!(facts.new_relations.iter().any(|r| r.is::<RunsOn>()));
    }

    #[test]
    fn parse_raw_sa_token_legacy_creates_entities_without_node() {
        let payload = r#"{
            "iss": "kubernetes/serviceaccount",
            "kubernetes.io/serviceaccount/namespace": "default",
            "kubernetes.io/serviceaccount/service-account.name": "default-sa",
            "kubernetes.io/serviceaccount/service-account.uid": "abc123",
            "sub": "system:serviceaccount:default:default-sa"
        }"#;
        let jwt = make_jwt(payload);
        let result = parse_raw_service_account_token(&jwt, "", &HashMap::new());

        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };

        assert!(facts
            .new_entities
            .iter()
            .any(|e| e.entity_kind() == "ServiceAccount" && e.entity_name() == "default-sa"));
        // No node entity since legacy tokens don't carry node info.
        assert!(!facts.new_entities.iter().any(|e| e.entity_kind() == "Node"));
        // No RunsOn relation.
        assert!(!facts.new_relations.iter().any(|r| r.is::<RunsOn>()));
    }

    #[test]
    fn parse_raw_sa_token_token_is_set_on_sa_entity() {
        let payload = r#"{
            "kubernetes.io": {
                "namespace": "kube-system",
                "pod": {"name": "coredns", "uid": "uid-1"},
                "serviceaccount": {"name": "coredns"}
            },
            "sub": "system:serviceaccount:kube-system:coredns"
        }"#;
        let jwt = make_jwt(payload);
        let result = parse_raw_service_account_token(&jwt, "", &HashMap::new());

        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts");
        };

        let sa_entity = facts
            .new_entities
            .iter()
            .find(|e| e.entity_kind() == "ServiceAccount")
            .expect("SA entity must be present");

        let sa = sa_entity
            .as_any()
            .downcast_ref::<ran_domain::ServiceAccount>()
            .expect("must downcast to ServiceAccount");

        let token = sa.token.as_ref().expect("token must be set on SA");
        assert!(!token.raw().is_empty(), "raw JWT must be stored in token");
        assert_eq!(token.service_account_name, "coredns");
        assert_eq!(token.namespace, "kube-system");
        assert!(token.is_bound, "pod uid present → token is bound");
    }

    #[test]
    fn parse_raw_sa_token_multiline_output_finds_jwt() {
        let payload = r#"{"kubernetes.io":{"namespace":"test","serviceaccount":{"name":"test-sa"}},"sub":"system:serviceaccount:test:test-sa"}"#;
        let jwt = make_jwt(payload);
        // Wrap the token in noisy multi-line output.
        let stdout = format!("some noise\n{jwt}\nmore noise\n");
        let result = parse_raw_service_account_token(&stdout, "", &HashMap::new());
        assert!(matches!(result, ParserOutput::SuccessWithFacts(_, _)));
    }

    #[test]
    fn parse_raw_sa_token_empty_input_returns_known_failure() {
        let result = parse_raw_service_account_token("", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::KnownFailure(_)));
    }

    #[test]
    fn parse_raw_sa_token_invalid_base64_returns_unknown_format() {
        let result = parse_raw_service_account_token("eyXXX.!!!.sig", "", &HashMap::new());
        assert!(matches!(result, ParserOutput::UnknownFormat(_)));
    }

    #[test]
    fn parse_raw_sa_token_missing_k8s_claims_returns_unknown_format() {
        // Valid JWT but payload has no kubernetes claims.
        let payload = r#"{"sub": "some-subject", "exp": 99999}"#;
        let jwt = make_jwt(payload);
        let result = parse_raw_service_account_token(&jwt, "", &HashMap::new());
        assert!(matches!(result, ParserOutput::UnknownFormat(_)));
    }

    #[test]
    fn parse_raw_sa_token_rejects_mismatch_with_target_serviceaccount() {
        let payload = r#"{
            "kubernetes.io": {
                "namespace": "dungeon",
                "serviceaccount": {"name": "player"}
            },
            "sub": "system:serviceaccount:dungeon:player"
        }"#;
        let jwt = make_jwt(payload);
        let mut args = HashMap::new();
        args.insert(
            "TARGET_ID".to_string(),
            "ns/argocd/sa/argocd-application-controller".to_string(),
        );

        let result = parse_raw_service_account_token(&jwt, "", &args);
        assert!(
            matches!(result, ParserOutput::KnownFailure(_)),
            "expected KnownFailure on SA identity mismatch, got {:?}",
            result
        );
    }

    #[test]
    fn parse_raw_sa_token_rejects_mismatch_with_target_pod() {
        let payload = r#"{
            "kubernetes.io": {
                "namespace": "dungeon",
                "pod": {"name": "entry-hall-abc"},
                "serviceaccount": {"name": "player"}
            },
            "sub": "system:serviceaccount:dungeon:player"
        }"#;
        let jwt = make_jwt(payload);
        let mut args = HashMap::new();
        args.insert(
            "TARGET_ID".to_string(),
            "ns/argocd/pod/argocd-application-controller-0".to_string(),
        );

        let result = parse_raw_service_account_token(&jwt, "", &args);
        assert!(
            matches!(result, ParserOutput::KnownFailure(_)),
            "expected KnownFailure on pod identity mismatch, got {:?}",
            result
        );
    }

    #[test]
    fn parse_raw_sa_token_allows_ip_placeholder_target_pod_name() {
        let payload = r#"{
            "kubernetes.io": {
                "namespace": "oopservability",
                "pod": {"name": "redis-665c68c95d-49pfq"},
                "serviceaccount": {"name": "redis"}
            },
            "sub": "system:serviceaccount:oopservability:redis"
        }"#;
        let jwt = make_jwt(payload);
        let mut args = HashMap::new();
        args.insert(
            "TARGET_ID".to_string(),
            "ns/oopservability/pod/redis.10-0-0-35".to_string(),
        );

        let result = parse_raw_service_account_token(&jwt, "", &args);
        assert!(
            matches!(result, ParserOutput::SuccessWithFacts(_, _)),
            "expected Parsed for placeholder pod target, got {:?}",
            result
        );
    }

    /// Authenticate As identifies the SA even when the semantic target is another entity.
    #[test]
    fn ssrr_parser_attributes_to_service_account_auth_identity() {
        let kubectl_output =
            "Resources                Non-Resource URLs   Resource Names   Verbs\n\
            pods                     []                  []               [get list watch]\n\
            secrets                  []                  []               [get]\n";

        let result = parse_self_subject_rules_review(kubectl_output, "", "ns/default/sa/mysa", "");

        let ParserOutput::SuccessWithFacts(facts, detail) = result else {
            panic!("expected SuccessWithFacts, got {:?}", result);
        };

        assert!(detail.contains("RBAC permission"), "detail: {detail}");
        assert_eq!(facts.new_entities.len(), 1);

        let updated_sa = facts.new_entities[0]
            .as_any()
            .downcast_ref::<ServiceAccount>()
            .expect("should be a ServiceAccount");

        assert_eq!(updated_sa.entity_id().0, "ns/default/sa/mysa");
        assert!(
            !updated_sa.entitlements.is_empty(),
            "entitlements should be populated"
        );
        assert!(updated_sa
            .entitlements
            .iter()
            .any(|p| p.verb == "get" && p.resource_type == "pods"));
    }

    #[test]
    fn ssrr_parser_reads_sa_identity_from_auth_identity_id() {
        let kubectl_output = "Resources   Non-Resource URLs   Resource Names   Verbs\n\
            pods        []                  []               [get list]\n";

        let result = parse_self_subject_rules_review(kubectl_output, "", "ns/default/sa/mysa", "");

        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts, got {:?}", result);
        };

        assert_eq!(facts.new_entities.len(), 1);
        let updated_sa = facts.new_entities[0]
            .as_any()
            .downcast_ref::<ServiceAccount>()
            .expect("should be ServiceAccount");
        assert_eq!(updated_sa.entity_id().0, "ns/default/sa/mysa");
        assert!(!updated_sa.entitlements.is_empty());
    }

    #[test]
    fn ssrr_parser_rejects_non_identity_auth_id() {
        let kubectl_output = "Resources   Non-Resource URLs   Resource Names   Verbs\n\
            pods        []                  []               [get list]\n";

        let result =
            parse_self_subject_rules_review(kubectl_output, "", "ns/default/pod/some-pod", "");

        assert!(
            matches!(result, ParserOutput::KnownFailure(_)),
            "expected KnownFailure for a non-identity Authenticate As value, got {:?}",
            result
        );
    }

    #[test]
    fn ssrr_parser_attributes_permissions_to_kubeconfig_credential() {
        let response = r#"{
            "status": {
                "resourceRules": [{
                    "verbs": ["get", "list"],
                    "apiGroups": [""],
                    "resources": ["pods"],
                    "resourceNames": []
                }],
                "nonResourceRules": [],
                "incomplete": false
            }
        }"#;

        let result = parse_self_subject_rules_review(
            response,
            "",
            "k8s/credential/developer-kubeconfig",
            "dungeon",
        );

        let ParserOutput::SuccessWithFacts(facts, _) = result else {
            panic!("expected SuccessWithFacts, got {:?}", result);
        };
        let credential = facts.new_entities[0]
            .as_any()
            .downcast_ref::<K8sCredential>()
            .expect("permissions should update the K8sCredential");
        assert_eq!(
            credential.entity_id().0,
            "k8s/credential/developer-kubeconfig"
        );
        assert!(credential.entitlements.iter().any(|permission| {
            permission.verb == "list"
                && permission.resource_type == "pods"
                && permission.scope.as_deref() == Some("dungeon")
        }));
    }
}
