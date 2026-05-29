use std::collections::HashMap;
use std::net::IpAddr;

use ran_domain::{
    ConfigMap, Deployment, K8sGateway, K8sGatewayListener, K8sHTTPBackend, K8sHTTPRoute,
    K8sIngress, K8sIngressPath, K8sIngressRule, K8sIngressTLS, K8sNode, K8sParentRef, K8sRole,
    K8sRoleBinding, K8sSecret, K8sService, K8sServicePort, Mount, NameConfidence, Namespace,
    OwnerRef, Pod, PodPhase, RbacPermission, RbacSubject, ServiceAccount,
};

use super::ParserOutput;
use crate::FactsUpdate;

pub(super) fn register(m: &mut HashMap<&'static str, super::ParserFn>) {
    m.insert("k8s.podlist", parse_k8s_pod_list);
    m.insert("k8s.nodelist", parse_k8s_node_list);
    m.insert("k8s.serviceaccountlist", parse_k8s_service_account_list);
    m.insert("k8s.secretlist", parse_k8s_secret_list);
    m.insert("k8s.deploymentlist", parse_k8s_deployment_list);
    m.insert("k8s.configmaplist", parse_k8s_config_map_list);
    m.insert("k8s.rolelist", parse_k8s_role_list);
    m.insert("k8s.rolebindinglist", parse_k8s_role_binding_list);
    m.insert("k8s.clusterrolelist", parse_k8s_cluster_role_list);
    m.insert("k8s.clusterrolebindinglist", parse_k8s_cluster_role_binding_list);
    m.insert("k8s.servicelist", parse_k8s_service_list);
    m.insert("k8s.ingresslist", parse_k8s_ingress_list);
    m.insert("k8s.gatewaylist", parse_k8s_gateway_list);
    m.insert("k8s.httproutelist", parse_k8s_http_route_list);
}

/// Minimal serde types for deserializing K8s API `kubectl --output=json` responses.
/// These cover only the fields Ran needs — unknown fields are ignored.
mod k8s_json {
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Deserialize, Default)]
    pub struct OwnerReference {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub kind: String,
        #[serde(default)]
        pub uid: String,
    }

    #[derive(Deserialize, Default)]
    pub struct Meta {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub namespace: Option<String>,
        #[serde(default)]
        pub uid: Option<String>,
        #[serde(rename = "ownerReferences", default)]
        pub owner_references: Vec<OwnerReference>,
    }

    // --- Pod ---

    #[derive(Deserialize, Default)]
    pub struct ContainerSecCtx {
        pub privileged: Option<bool>,
        #[serde(rename = "readOnlyRootFilesystem")]
        pub read_only_root_fs: Option<bool>,
    }

    #[derive(Deserialize, Default)]
    pub struct ContainerVolumeMount {
        #[serde(default)]
        pub name: String,
        #[serde(rename = "mountPath", default)]
        pub mount_path: String,
        #[serde(rename = "readOnly", default)]
        pub read_only: bool,
    }

    #[derive(Deserialize, Default)]
    pub struct Container {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub image: String,
        #[serde(rename = "securityContext", default)]
        pub security_context: Option<ContainerSecCtx>,
        #[serde(rename = "volumeMounts", default)]
        pub volume_mounts: Vec<ContainerVolumeMount>,
    }

    #[derive(Deserialize, Default)]
    pub struct HostPathVolume {
        pub path: String,
    }

    #[derive(Deserialize, Default)]
    pub struct Volume {
        #[serde(default)]
        pub name: String,
        #[serde(rename = "hostPath")]
        pub host_path: Option<HostPathVolume>,
    }

    #[derive(Deserialize, Default)]
    pub struct PodSpec {
        #[serde(rename = "nodeName", default)]
        pub node_name: Option<String>,
        #[serde(rename = "serviceAccountName", default)]
        pub service_account_name: Option<String>,
        #[serde(rename = "automountServiceAccountToken", default)]
        pub automount_service_account_token: Option<bool>,
        #[serde(rename = "hostPID", default)]
        pub host_pid: bool,
        #[serde(rename = "hostIPC", default)]
        pub host_ipc: bool,
        #[serde(rename = "hostNetwork", default)]
        pub host_network: bool,
        #[serde(default)]
        pub containers: Vec<Container>,
        #[serde(rename = "initContainers", default)]
        pub init_containers: Vec<Container>,
        #[serde(default)]
        pub volumes: Vec<Volume>,
    }

    #[derive(Deserialize, Default)]
    pub struct PodStatus {
        #[serde(default)]
        pub phase: Option<String>,
        #[serde(rename = "podIP", default)]
        pub pod_ip: Option<String>,
        #[serde(rename = "hostIP", default)]
        pub host_ip: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct PodItem {
        pub metadata: Meta,
        #[serde(default)]
        pub spec: PodSpec,
        #[serde(default)]
        pub status: PodStatus,
    }

    #[derive(Deserialize)]
    pub struct PodList {
        #[serde(default)]
        pub items: Vec<PodItem>,
    }

    // --- Node ---

    #[derive(Deserialize)]
    pub struct NodeItem {
        pub metadata: Meta,
    }

    #[derive(Deserialize)]
    pub struct NodeList {
        #[serde(default)]
        pub items: Vec<NodeItem>,
    }

    // --- ServiceAccount ---

    #[derive(Deserialize, Default)]
    pub struct SecretRef {
        #[serde(default)]
        pub name: String,
    }

    #[derive(Deserialize)]
    pub struct ServiceAccountItem {
        pub metadata: Meta,
        #[serde(default)]
        pub secrets: Vec<SecretRef>,
    }

    #[derive(Deserialize)]
    pub struct ServiceAccountList {
        #[serde(default)]
        pub items: Vec<ServiceAccountItem>,
    }

    // --- Secret ---

    #[derive(Deserialize)]
    pub struct SecretItem {
        pub metadata: Meta,
        #[serde(rename = "type", default)]
        pub secret_type: String,
        /// Keys only — values are base64-encoded credentials; we don't store them.
        #[serde(default)]
        pub data: HashMap<String, serde_json::Value>,
    }

    #[derive(Deserialize)]
    pub struct SecretList {
        #[serde(default)]
        pub items: Vec<SecretItem>,
    }

    // --- ConfigMap ---

    #[derive(Deserialize)]
    pub struct ConfigMapItem {
        pub metadata: Meta,
        #[serde(default)]
        pub data: HashMap<String, String>,
        #[serde(default)]
        pub immutable: Option<bool>,
    }

    #[derive(Deserialize)]
    pub struct ConfigMapList {
        #[serde(default)]
        pub items: Vec<ConfigMapItem>,
    }

    // --- Deployment ---

    #[derive(Deserialize)]
    pub struct DeploymentItem {
        pub metadata: Meta,
    }

    #[derive(Deserialize)]
    pub struct DeploymentList {
        #[serde(default)]
        pub items: Vec<DeploymentItem>,
    }

    // --- Role / ClusterRole ---

    #[derive(Deserialize, Default)]
    pub struct PolicyRule {
        #[serde(default)]
        pub verbs: Vec<String>,
        #[serde(default)]
        pub resources: Vec<String>,
        #[serde(rename = "resourceNames", default)]
        pub resource_names: Vec<String>,
        #[serde(rename = "apiGroups", default)]
        pub api_groups: Vec<String>,
    }

    #[derive(Deserialize)]
    pub struct RoleItem {
        pub metadata: Meta,
        #[serde(default)]
        pub rules: Vec<PolicyRule>,
    }

    #[derive(Deserialize)]
    pub struct RoleList {
        #[serde(default)]
        pub items: Vec<RoleItem>,
    }

    // --- RoleBinding / ClusterRoleBinding ---

    #[derive(Deserialize, Default)]
    pub struct RoleRef {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub kind: String,
    }

    #[derive(Deserialize, Default)]
    pub struct Subject {
        #[serde(default)]
        pub kind: String,
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub namespace: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct RoleBindingItem {
        pub metadata: Meta,
        #[serde(rename = "roleRef", default)]
        pub role_ref: RoleRef,
        #[serde(default)]
        pub subjects: Vec<Subject>,
    }

    #[derive(Deserialize)]
    pub struct RoleBindingList {
        #[serde(default)]
        pub items: Vec<RoleBindingItem>,
    }

    // --- Service ---

    #[derive(Deserialize, Default)]
    pub struct ServicePort {
        #[serde(default)]
        pub name: Option<String>,
        #[serde(default)]
        pub protocol: String,
        #[serde(default)]
        pub port: i32,
        #[serde(rename = "targetPort", default)]
        pub target_port: serde_json::Value,
        #[serde(rename = "nodePort", default)]
        pub node_port: Option<i32>,
    }

    #[derive(Deserialize, Default)]
    pub struct ServiceSpec {
        #[serde(rename = "type", default)]
        pub service_type: String,
        #[serde(rename = "clusterIP", default)]
        pub cluster_ip: Option<String>,
        #[serde(default)]
        pub ports: Vec<ServicePort>,
        #[serde(default)]
        pub selector: HashMap<String, String>,
        #[serde(rename = "externalIPs", default)]
        pub external_ips: Vec<String>,
    }

    #[derive(Deserialize, Default)]
    pub struct ServiceStatus {
        #[serde(rename = "loadBalancer", default)]
        pub load_balancer: LoadBalancerStatus,
    }

    #[derive(Deserialize, Default)]
    pub struct LoadBalancerStatus {
        #[serde(default)]
        pub ingress: Vec<LoadBalancerIngress>,
    }

    #[derive(Deserialize, Default)]
    pub struct LoadBalancerIngress {
        #[serde(default)]
        pub ip: Option<String>,
        #[serde(default)]
        pub hostname: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct ServiceItem {
        pub metadata: Meta,
        #[serde(default)]
        pub spec: ServiceSpec,
        #[serde(default)]
        pub status: ServiceStatus,
    }

    #[derive(Deserialize)]
    pub struct ServiceList {
        #[serde(default)]
        pub items: Vec<ServiceItem>,
    }

    // --- Ingress ---

    #[derive(Deserialize, Default)]
    pub struct IngressBackend {
        #[serde(default)]
        pub service: Option<IngressServiceBackend>,
    }

    #[derive(Deserialize, Default)]
    pub struct IngressServiceBackend {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub port: IngressServicePort,
    }

    #[derive(Deserialize, Default)]
    pub struct IngressServicePort {
        #[serde(default)]
        pub number: Option<i32>,
        #[serde(default)]
        pub name: Option<String>,
    }

    #[derive(Deserialize, Default)]
    pub struct HttpIngressPath {
        #[serde(default)]
        pub path: Option<String>,
        #[serde(rename = "pathType", default)]
        pub path_type: String,
        #[serde(default)]
        pub backend: IngressBackend,
    }

    #[derive(Deserialize, Default)]
    pub struct HttpIngressRuleValue {
        #[serde(default)]
        pub paths: Vec<HttpIngressPath>,
    }

    #[derive(Deserialize, Default)]
    pub struct IngressRule {
        #[serde(default)]
        pub host: Option<String>,
        #[serde(default)]
        pub http: Option<HttpIngressRuleValue>,
    }

    #[derive(Deserialize, Default)]
    pub struct IngressTLS {
        #[serde(default)]
        pub hosts: Vec<String>,
        #[serde(rename = "secretName", default)]
        pub secret_name: Option<String>,
    }

    #[derive(Deserialize, Default)]
    pub struct IngressSpec {
        #[serde(rename = "ingressClassName", default)]
        pub ingress_class_name: Option<String>,
        #[serde(default)]
        pub rules: Vec<IngressRule>,
        #[serde(default)]
        pub tls: Vec<IngressTLS>,
    }

    #[derive(Deserialize, Default)]
    pub struct IngressLoadBalancerIngress {
        #[serde(default)]
        pub ip: Option<String>,
        #[serde(default)]
        pub hostname: Option<String>,
    }

    #[derive(Deserialize, Default)]
    pub struct IngressLoadBalancerStatus {
        #[serde(default)]
        pub ingress: Vec<IngressLoadBalancerIngress>,
    }

    #[derive(Deserialize, Default)]
    pub struct IngressStatus {
        #[serde(rename = "loadBalancer", default)]
        pub load_balancer: IngressLoadBalancerStatus,
    }

    #[derive(Deserialize)]
    pub struct IngressItem {
        pub metadata: Meta,
        #[serde(default)]
        pub spec: IngressSpec,
        #[serde(default)]
        pub status: IngressStatus,
    }

    #[derive(Deserialize)]
    pub struct IngressList {
        #[serde(default)]
        pub items: Vec<IngressItem>,
    }

    // --- Gateway API: Gateway ---

    #[derive(Deserialize, Default)]
    pub struct GatewayListener {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub port: i32,
        #[serde(default)]
        pub protocol: String,
        #[serde(default)]
        pub hostname: Option<String>,
    }

    #[derive(Deserialize, Default)]
    pub struct GatewaySpec {
        #[serde(rename = "gatewayClassName", default)]
        pub gateway_class_name: String,
        #[serde(default)]
        pub listeners: Vec<GatewayListener>,
    }

    #[derive(Deserialize, Default)]
    pub struct GatewayAddress {
        // "type" field ("IPAddress" | "Hostname") omitted — both forms are
        // stored identically in external_addresses.
        #[serde(default)]
        pub value: String,
    }

    #[derive(Deserialize, Default)]
    pub struct GatewayStatus {
        #[serde(default)]
        pub addresses: Vec<GatewayAddress>,
    }

    #[derive(Deserialize)]
    pub struct GatewayItem {
        pub metadata: Meta,
        #[serde(default)]
        pub spec: GatewaySpec,
        #[serde(default)]
        pub status: GatewayStatus,
    }

    #[derive(Deserialize)]
    pub struct GatewayList {
        #[serde(default)]
        pub items: Vec<GatewayItem>,
    }

    // --- Gateway API: HTTPRoute ---

    #[derive(Deserialize, Default)]
    pub struct ParentReference {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub namespace: Option<String>,
        #[serde(rename = "sectionName", default)]
        pub section_name: Option<String>,
    }

    #[derive(Deserialize, Default)]
    pub struct BackendObjectReference {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub port: Option<serde_json::Value>,
    }

    #[derive(Deserialize, Default)]
    pub struct HttpBackendRef {
        #[serde(rename = "backendRef", default)]
        pub backend_ref: Option<BackendObjectReference>,
    }

    #[derive(Deserialize, Default)]
    pub struct HttpRouteRule {
        #[serde(rename = "backendRefs", default)]
        pub backend_refs: Vec<HttpBackendRef>,
    }

    #[derive(Deserialize, Default)]
    pub struct HttpRouteSpec {
        #[serde(rename = "parentRefs", default)]
        pub parent_refs: Vec<ParentReference>,
        #[serde(default)]
        pub hostnames: Vec<String>,
        #[serde(default)]
        pub rules: Vec<HttpRouteRule>,
    }

    #[derive(Deserialize)]
    pub struct HttpRouteItem {
        pub metadata: Meta,
        #[serde(default)]
        pub spec: HttpRouteSpec,
    }

    #[derive(Deserialize)]
    pub struct HttpRouteList {
        #[serde(default)]
        pub items: Vec<HttpRouteItem>,
    }

    // --- API error response (e.g. 403 Forbidden) ---

    /// Subset of the Kubernetes `Status` object returned by the API server
    /// when a request fails (e.g. RBAC denied).  `kind` must equal `"Status"`
    /// for this struct to be treated as an error response.
    #[derive(Deserialize)]
    pub struct StatusError {
        #[serde(default)]
        pub kind: String,
        pub code: Option<i32>,
        #[serde(default)]
        pub message: Option<String>,
    }
}

/// Check whether `stdout` is a Kubernetes API error response.
///
/// The API server returns a `Status` object (not the requested resource) when a
/// call succeeds at the transport level but is rejected (e.g. 403 Forbidden).
/// Mirroring the Go `ParseEffect` logic in `parsers.go` (line 346), any status
/// code ≥ 400 is treated as a failure so it is never mis-classified as a
/// successful but empty result.
fn check_k8s_api_error(stdout: &str) -> Option<ParserOutput> {
    let resp: k8s_json::StatusError = serde_json::from_str(stdout.trim()).ok()?;
    if resp.kind != "Status" {
        return None;
    }
    let code = resp.code?;
    if code >= 400 {
        Some(ParserOutput::KnownFailure(format!(
            "K8s API error {}: {}",
            code,
            resp.message.as_deref().unwrap_or("(no message)")
        )))
    } else {
        None
    }
}

fn parse_k8s_pod_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::PodList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("PodList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }

        let mut pod = Pod::new(name.clone(), ns.clone());
        pod.meta.name_confidence = NameConfidence::Authoritative;

        if let Some(uid) = &item.metadata.uid {
            pod.meta.uid = Some(uid.clone());
        }

        pod.owner_references = item
            .metadata
            .owner_references
            .iter()
            .map(|o| OwnerRef {
                name: o.name.clone(),
                kind: o.kind.clone(),
                uid: o.uid.clone(),
            })
            .collect();
        pod.node_name = item.spec.node_name.clone();
        pod.service_account_name = item.spec.service_account_name.clone();
        pod.automount_service_account_token = item.spec.automount_service_account_token.into();
        pod.host_pid = item.spec.host_pid.into();
        pod.host_ipc = item.spec.host_ipc.into();
        pod.host_network = item.spec.host_network.into();

        // Build a volume-name → host path index (only host-path volumes for now).
        let vol_host_paths: std::collections::HashMap<&str, &str> = item
            .spec
            .volumes
            .iter()
            .filter_map(|v| v.host_path.as_ref().map(|hp| (v.name.as_str(), hp.path.as_str())))
            .collect();

        // Containers: security context + per-container volume mounts.
        for c in item.spec.containers.iter().chain(item.spec.init_containers.iter()) {
            if let Some(sc) = &c.security_context {
                if sc.privileged == Some(true) {
                    pod.privileged = true.into();
                }
                if let Some(rorf) = sc.read_only_root_fs {
                    pod.read_only_root_fs = rorf.into();
                }
            }

            let volume_mounts = c
                .volume_mounts
                .iter()
                .map(|vm| {
                    let (mount_root, is_host_path) = vol_host_paths
                        .get(vm.name.as_str())
                        .map(|hp| (hp.to_string(), true))
                        .unwrap_or_default();
                    Mount {
                        name: vm.name.clone(),
                        mount_point: vm.mount_path.clone(),
                        mount_root,
                        mount_type: None,
                        is_host_path,
                        read_only: vm.read_only,
                    }
                })
                .collect();

            pod.containers.push(ran_domain::Container {
                name: c.name.clone(),
                image: c.image.clone(),
                volume_mounts,
            });
        }

        // Pod-level host-path mounts — derived from containers, de-duplicated by
        // volume name. Used by grounding (SRC.MOUNT_PATH) and has_host_paths().
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for c in &pod.containers {
            for m in &c.volume_mounts {
                if m.is_host_path && seen.insert(m.name.as_str()) {
                    pod.volume_mounts.push(m.clone());
                }
            }
        }

        pod.phase = item.status.phase.as_deref().map(|p| match p {
            "Pending" => PodPhase::Pending,
            "Running" => PodPhase::Running,
            "Succeeded" => PodPhase::Succeeded,
            "Failed" => PodPhase::Failed,
            _ => PodPhase::Unknown,
        });
        pod.is_running = pod.phase == Some(PodPhase::Running);

        if let Some(ip_str) = &item.status.pod_ip {
            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                pod.system.ips.push(ip);
            }
        }
        if let Some(ip_str) = &item.status.host_ip {
            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                pod.host_ip = Some(ip);
            }
        }

        // When the pod's own IP equals the node IP, the pod uses host networking.
        // Detect this as a fact even if spec.hostNetwork is not set explicitly.
        if let (Some(host_ip), Some(&pod_ip)) = (pod.host_ip, pod.system.ips.first()) {
            if host_ip == pod_ip {
                pod.host_network = ran_domain::Confidence::Yes;
            }
        }

        facts.new_entities.push(Box::new(pod));

        // Ensure the namespace exists in the graph.
        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(facts, format!("parsed {} pod(s) from PodList", count))
}

fn parse_k8s_node_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::NodeList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("NodeList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        if item.metadata.name.is_empty() {
            continue;
        }
        let mut node = K8sNode::new(item.metadata.name.clone());
        node.name_confidence = NameConfidence::Authoritative;
        facts.new_entities.push(Box::new(node));
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(facts, format!("parsed {} node(s) from NodeList", count))
}

fn parse_k8s_service_account_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::ServiceAccountList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("ServiceAccountList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let mut sa = ServiceAccount::new(name.clone(), ns.clone());
        sa.secret_names = item.secrets.iter().map(|s| s.name.clone()).collect();
        facts.new_entities.push(Box::new(sa));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(
        facts,
        format!(
            "parsed {} service account(s) from ServiceAccountList",
            count
        ),
    )
}

fn parse_k8s_secret_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::SecretList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("SecretList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let mut secret = K8sSecret::new(name.clone(), ns.clone());
        secret.secret_type = item.secret_type.clone();
        secret.data_keys = item.data.keys().cloned().collect();
        facts.new_entities.push(Box::new(secret));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(facts, format!("parsed {} secret(s) from SecretList", count))
}

fn parse_k8s_deployment_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::DeploymentList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("DeploymentList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        facts
            .new_entities
            .push(Box::new(Deployment::new(name.clone(), ns.clone())));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(
        facts,
        format!("parsed {} deployment(s) from DeploymentList", count),
    )
}

fn rules_to_permissions(rules: &[k8s_json::PolicyRule]) -> Vec<RbacPermission> {
    let mut perms = Vec::new();
    for rule in rules {
        let api_group = rule.api_groups.first().cloned();
        for verb in &rule.verbs {
            for resource in &rule.resources {
                if rule.resource_names.is_empty() {
                    let mut p = RbacPermission::new(verb.clone(), resource.clone());
                    p.api_group = api_group.clone();
                    perms.push(p);
                } else {
                    for rname in &rule.resource_names {
                        let mut p = RbacPermission::new(verb.clone(), resource.clone());
                        p.api_group = api_group.clone();
                        p.resource_name = Some(rname.clone());
                        perms.push(p);
                    }
                }
            }
        }
    }
    perms
}

fn parse_role_binding_item(
    item: &k8s_json::RoleBindingItem,
    is_cluster: bool,
) -> Option<K8sRoleBinding> {
    let name = &item.metadata.name;
    if name.is_empty() {
        return None;
    }
    let ns = if is_cluster {
        String::new()
    } else {
        item.metadata.namespace.as_deref().unwrap_or("").to_string()
    };
    let mut binding = K8sRoleBinding::new(name.clone(), ns);
    binding.role_ref = item.role_ref.name.clone();
    binding.role_ref_kind = item.role_ref.kind.clone();
    binding.subjects = item
        .subjects
        .iter()
        .map(|s| RbacSubject {
            kind: s.kind.clone(),
            name: s.name.clone(),
            namespace: s.namespace.as_deref().unwrap_or("").to_string(),
        })
        .collect();
    Some(binding)
}

fn parse_k8s_config_map_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::ConfigMapList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("ConfigMapList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let mut cm = ConfigMap::new(name.clone(), ns.clone());
        cm.data = item.data.clone();
        cm.immutable = item.immutable.unwrap_or(false);
        facts.new_entities.push(Box::new(cm));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(
        facts,
        format!("parsed {} configmap(s) from ConfigMapList", count),
    )
}

fn parse_k8s_role_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::RoleList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("RoleList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let mut role = K8sRole::new(name.clone(), ns.clone());
        role.is_cluster_role = false;
        role.permissions = rules_to_permissions(&item.rules);
        facts.new_entities.push(Box::new(role));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(facts, format!("parsed {} role(s) from RoleList", count))
}

fn parse_k8s_cluster_role_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::RoleList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("ClusterRoleList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        if name.is_empty() {
            continue;
        }
        let mut role = K8sRole::new(name.clone(), "");
        role.is_cluster_role = true;
        role.permissions = rules_to_permissions(&item.rules);
        facts.new_entities.push(Box::new(role));
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(
        facts,
        format!("parsed {} cluster role(s) from ClusterRoleList", count),
    )
}

fn parse_k8s_role_binding_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::RoleBindingList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("RoleBindingList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        let Some(binding) = parse_role_binding_item(item, false) else {
            continue;
        };
        facts.new_entities.push(Box::new(binding));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(
        facts,
        format!("parsed {} role binding(s) from RoleBindingList", count),
    )
}

fn parse_k8s_service_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::ServiceList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("ServiceList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }

        let mut svc = K8sService::new(name.clone(), ns.clone());
        svc.service_type = if item.spec.service_type.is_empty() {
            "ClusterIP".to_string()
        } else {
            item.spec.service_type.clone()
        };

        // "None" is the headless service sentinel — store as None.
        svc.cluster_ip = item
            .spec
            .cluster_ip
            .as_deref()
            .filter(|ip| !ip.is_empty() && *ip != "None")
            .map(str::to_string);

        svc.ports = item
            .spec
            .ports
            .iter()
            .map(|p| K8sServicePort {
                port: p.port,
                target_port: p.target_port.to_string().trim_matches('"').to_string(),
                protocol: if p.protocol.is_empty() {
                    "TCP".to_string()
                } else {
                    p.protocol.clone()
                },
                name: p.name.clone(),
                node_port: p.node_port,
            })
            .collect();

        svc.selector = item.spec.selector.clone();

        // Collect external IPs from spec and LoadBalancer ingress.
        svc.external_ips = item.spec.external_ips.clone();
        for ingress in &item.status.load_balancer.ingress {
            let addr = ingress.ip.as_deref().or(ingress.hostname.as_deref());
            if let Some(a) = addr {
                if !svc.external_ips.contains(&a.to_string()) {
                    svc.external_ips.push(a.to_string());
                }
            }
        }

        facts.new_entities.push(Box::new(svc));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(facts, format!("parsed {} service(s) from ServiceList", count))
}

fn parse_k8s_cluster_role_binding_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::RoleBindingList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("ClusterRoleBindingList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let Some(binding) = parse_role_binding_item(item, true) else {
            continue;
        };
        facts.new_entities.push(Box::new(binding));
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(
        facts,
        format!(
            "parsed {} cluster role binding(s) from ClusterRoleBindingList",
            count
        ),
    )
}

fn parse_k8s_ingress_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::IngressList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("IngressList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }

        let mut ingress = K8sIngress::new(name.clone(), ns.clone());
        ingress.ingress_class = item.spec.ingress_class_name.clone();

        for rule in &item.spec.rules {
            let paths = rule
                .http
                .as_ref()
                .map(|h| &h.paths)
                .map(|ps| {
                    ps.iter()
                        .filter_map(|p| {
                            let svc = p.backend.service.as_ref()?;
                            let port = svc
                                .port
                                .number
                                .map(|n| n.to_string())
                                .or_else(|| svc.port.name.clone())
                                .unwrap_or_default();
                            Some(K8sIngressPath {
                                path: p.path.clone().unwrap_or_else(|| "/".to_string()),
                                path_type: p.path_type.clone(),
                                backend_service: svc.name.clone(),
                                backend_port: port,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            ingress.rules.push(K8sIngressRule {
                host: rule.host.clone(),
                paths,
            });
        }

        ingress.tls = item
            .spec
            .tls
            .iter()
            .map(|t| K8sIngressTLS {
                hosts: t.hosts.clone(),
                secret_name: t.secret_name.clone(),
            })
            .collect();

        for lb in &item.status.load_balancer.ingress {
            let addr = lb.ip.as_deref().or(lb.hostname.as_deref());
            if let Some(a) = addr {
                ingress.external_addresses.push(a.to_string());
            }
        }

        facts.new_entities.push(Box::new(ingress));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(facts, format!("parsed {} ingress(es) from IngressList", count))
}

fn parse_k8s_gateway_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::GatewayList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("GatewayList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }

        let mut gw = K8sGateway::new(name.clone(), ns.clone());
        gw.gateway_class = item.spec.gateway_class_name.clone();

        gw.listeners = item
            .spec
            .listeners
            .iter()
            .map(|l| K8sGatewayListener {
                name: l.name.clone(),
                port: l.port,
                protocol: l.protocol.clone(),
                hostname: l.hostname.clone(),
            })
            .collect();

        for addr in &item.status.addresses {
            if !addr.value.is_empty() {
                gw.external_addresses.push(addr.value.clone());
            }
        }

        facts.new_entities.push(Box::new(gw));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(facts, format!("parsed {} gateway(s) from GatewayList", count))
}

fn parse_k8s_http_route_list(stdout: &str, _stderr: &str, _args: &HashMap<String, String>) -> ParserOutput {
    if stdout.trim().is_empty() {
        return ParserOutput::KnownFailure("empty stdout".to_string());
    }
    if let Some(err) = check_k8s_api_error(stdout) {
        return err;
    }
    let list: k8s_json::HttpRouteList = match serde_json::from_str(stdout) {
        Ok(l) => l,
        Err(e) => return ParserOutput::UnknownFormat(format!("JSON parse error: {e}")),
    };
    if list.items.is_empty() {
        return ParserOutput::KnownFailure("HTTPRouteList contained no items".to_string());
    }

    let mut facts = FactsUpdate::default();
    for item in &list.items {
        let name = &item.metadata.name;
        let ns = item.metadata.namespace.as_deref().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }

        let mut route = K8sHTTPRoute::new(name.clone(), ns.clone());

        route.parent_refs = item
            .spec
            .parent_refs
            .iter()
            .map(|p| K8sParentRef {
                name: p.name.clone(),
                namespace: p.namespace.clone(),
                section_name: p.section_name.clone(),
            })
            .collect();

        route.hostnames = item.spec.hostnames.clone();

        // Flatten all backend refs from all rules.
        for rule in &item.spec.rules {
            for bref in &rule.backend_refs {
                if let Some(backend) = &bref.backend_ref {
                    if backend.name.is_empty() {
                        continue;
                    }
                    let port = backend
                        .port
                        .as_ref()
                        .map(|p| p.to_string().trim_matches('"').to_string())
                        .unwrap_or_default();
                    route.backends.push(K8sHTTPBackend {
                        service_name: backend.name.clone(),
                        service_port: port,
                    });
                }
            }
        }

        facts.new_entities.push(Box::new(route));

        if !ns.is_empty() {
            facts.new_entities.push(Box::new(Namespace::new(ns)));
        }
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(
        facts,
        format!("parsed {} HTTPRoute(s) from HTTPRouteList", count),
    )
}
