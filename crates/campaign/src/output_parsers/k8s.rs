use std::collections::HashMap;
use std::net::IpAddr;

use ran_domain::{ConfigMap, Deployment, K8sNode, K8sSecret, Mount, Namespace, OwnerRef, Pod, PodPhase, ServiceAccount};

use crate::FactsUpdate;
use super::ParserOutput;

pub(super) fn register(m: &mut HashMap<&'static str, super::ParserFn>) {
    m.insert("k8s.podlist", parse_k8s_pod_list);
    m.insert("k8s.nodelist", parse_k8s_node_list);
    m.insert("k8s.serviceaccountlist", parse_k8s_service_account_list);
    m.insert("k8s.secretlist", parse_k8s_secret_list);
    m.insert("k8s.deploymentlist", parse_k8s_deployment_list);
    m.insert("k8s.configmaplist", parse_k8s_config_map_list);
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
    pub struct Container {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub image: String,
        #[serde(rename = "securityContext", default)]
        pub security_context: Option<ContainerSecCtx>,
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

fn parse_k8s_pod_list(stdout: &str, _stderr: &str) -> ParserOutput {
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

        if let Some(uid) = &item.metadata.uid {
            pod.meta.uid = Some(uid.clone());
        }

        pod.owner_references = item.metadata.owner_references.iter().map(|o| OwnerRef {
            name: o.name.clone(),
            kind: o.kind.clone(),
            uid: o.uid.clone(),
        }).collect();
        pod.node_name = item.spec.node_name.clone();
        pod.service_account_name = item.spec.service_account_name.clone();
        pod.automount_service_account_token = item.spec.automount_service_account_token.into();
        pod.host_pid = item.spec.host_pid.into();
        pod.host_ipc = item.spec.host_ipc.into();
        pod.host_network = item.spec.host_network.into();

        // Security context: any container flagged as privileged makes the pod privileged.
        let all_containers =
            item.spec.containers.iter().chain(item.spec.init_containers.iter());
        for c in all_containers {
            pod.containers.push(ran_domain::Container {
                name: c.name.clone(),
                image: c.image.clone(),
            });
            if let Some(sc) = &c.security_context {
                if sc.privileged == Some(true) {
                    pod.privileged = true.into();
                }
                if let Some(rorf) = sc.read_only_root_fs {
                    pod.read_only_root_fs = rorf.into();
                }
            }
        }

        // Volumes — record host-path mounts.
        for vol in &item.spec.volumes {
            if let Some(hp) = &vol.host_path {
                pod.host_paths.push(hp.path.clone());
                pod.volume_mounts.push(Mount {
                    name: vol.name.clone(),
                    mount_root: hp.path.clone(),
                    mount_point: String::new(), // mountPath lives on VolumeMount, not Volume
                    mount_type: None,
                    is_host_path: true,
                    read_only: false,
                });
            }
        }

        pod.phase = item.status.phase.as_deref().and_then(|p| match p {
            "Pending" => Some(PodPhase::Pending),
            "Running" => Some(PodPhase::Running),
            "Succeeded" => Some(PodPhase::Succeeded),
            "Failed" => Some(PodPhase::Failed),
            _ => Some(PodPhase::Unknown),
        });
        pod.is_running = pod.phase == Some(PodPhase::Running);

        if let Some(ip_str) = &item.status.pod_ip {
            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                pod.system.ips.push(ip);
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

fn parse_k8s_node_list(stdout: &str, _stderr: &str) -> ParserOutput {
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
        let node = K8sNode::new(item.metadata.name.clone());
        facts.new_entities.push(Box::new(node));
    }

    let count = facts.new_entities.len();
    ParserOutput::SuccessWithFacts(facts, format!("parsed {} node(s) from NodeList", count))
}

fn parse_k8s_service_account_list(stdout: &str, _stderr: &str) -> ParserOutput {
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
        format!("parsed {} service account(s) from ServiceAccountList", count),
    )
}

fn parse_k8s_secret_list(stdout: &str, _stderr: &str) -> ParserOutput {
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
    ParserOutput::SuccessWithFacts(
        facts,
        format!("parsed {} secret(s) from SecretList", count),
    )
}

fn parse_k8s_deployment_list(stdout: &str, _stderr: &str) -> ParserOutput {
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
        facts.new_entities.push(Box::new(Deployment::new(name.clone(), ns.clone())));

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

fn parse_k8s_config_map_list(stdout: &str, _stderr: &str) -> ParserOutput {
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

