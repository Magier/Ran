//! Typed realization of discovered transports. Graph capabilities need not
//! contain shell templates, but every selected route must have a renderer.

use ran_domain::{BinaryPresence, EntityId, K8sNode, Pod};

use super::Campaign;

/// Realization of an exact source -> Node -> Pod pair. No bearer token is
/// retained: authentication uses the selected source's mounted credential.
#[derive(Debug, Clone, PartialEq)]
pub struct KubeletExecPlan {
    pub(crate) source_id: String,
    pub(crate) node_id: String,
    pub(crate) pod_id: String,
    endpoint: String,
    namespace: String,
    pod: String,
    container: String,
    client: String,
    confirmed_client: bool,
    confirmed_container: bool,
    shell: BinaryPresence,
    token_file: String,
    ca_file: String,
}

impl KubeletExecPlan {
    pub(crate) fn is_confirmed(&self) -> bool {
        self.confirmed_client
            && self.confirmed_container
            && matches!(self.shell, BinaryPresence::Present(_))
    }
    /// Procedure commands are shell programs, including builtins, quoting,
    /// substitutions and pipelines. Preserve that contract with explicit argv,
    /// never one concatenated kubelet executable name.
    pub(crate) fn command_argv(&self, command: &str) -> Result<Vec<String>, String> {
        let shell = match &self.shell {
            BinaryPresence::Present(path) => path.as_str(),
            BinaryPresence::Unknown => "sh",
            BinaryPresence::Absent => {
                return Err(
                    "kubelet payload requires a shell known to be absent from the Pod".into(),
                )
            }
        };
        Ok(vec![shell.into(), "-c".into(), command.into()])
    }

    pub(crate) fn render(&self, command: &str) -> Result<String, String> {
        let mut url = url::Url::parse(&self.endpoint).map_err(|error| error.to_string())?;
        url.path_segments_mut()
            .map_err(|_| "kubelet endpoint cannot hold a path")?
            .extend(["exec", &self.namespace, &self.pod, &self.container]);
        url.query_pairs_mut()
            .extend_pairs([("output", "1"), ("error", "1")]);
        for arg in self.command_argv(command)? {
            url.query_pairs_mut().append_pair("command", &arg);
        }
        Ok(format!(
            "{} kubelet-exec --url {} --token-file {} --ca-file {}",
            shell_words::quote(&self.client),
            shell_words::quote(url.as_str()),
            shell_words::quote(&self.token_file),
            shell_words::quote(&self.ca_file),
        ))
    }
}

impl Campaign {
    /// Source-side prerequisites are checked during route search. A concrete
    /// discovered edge is authorization evidence; an unrelated selected action
    /// identity must not change this transport's ambient token source.
    pub(super) fn kubelet_source_endpoint(
        &self,
        source: &EntityId,
        node: &EntityId,
    ) -> Option<String> {
        // This adapter uses Pod-mounted credentials. Other source environments
        // require a different explicit credential binding, not these paths.
        let source = self.entities.find::<Pod>(source)?;
        if source.automount_service_account_token == ran_domain::Confidence::No {
            return None;
        }
        if source.system.has_binary("ranplant") == BinaryPresence::Absent {
            return None;
        }
        let node = self.entities.find::<K8sNode>(node)?;
        let host = node
            .system
            .ips
            .first()
            .map(ToString::to_string)
            .unwrap_or_else(|| node.name.clone());
        let host = match host.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V6(ip)) => format!("[{ip}]"),
            _ => host,
        };
        let endpoint = url::Url::parse(&format!("wss://{host}:10250/")).ok()?;
        if endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.path() != "/"
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return None;
        }
        Some(endpoint.to_string())
    }

    pub(super) fn plan_kubelet_pair(
        &self,
        source: &EntityId,
        node: &EntityId,
        target: &EntityId,
    ) -> Option<KubeletExecPlan> {
        let endpoint = self.kubelet_source_endpoint(source, node)?;
        let pod = self.entities.find::<Pod>(target)?;
        let namespace = pod.meta.namespace.as_ref()?.clone();
        if namespace.is_empty() || pod.meta.name.is_empty() {
            return None;
        }
        let source_system = self.get_system_entity(&source.0)?;
        let client = match source_system.entity().system().has_binary("ranplant") {
            BinaryPresence::Present(path) => path,
            BinaryPresence::Unknown => "ranplant".into(),
            BinaryPresence::Absent => return None,
        };
        Some(KubeletExecPlan {
            source_id: source.0.clone(),
            node_id: node.0.clone(),
            pod_id: target.0.clone(),
            endpoint,
            namespace,
            pod: pod.meta.name.clone(),
            // Preserve the legacy default for incompletely discovered Pods.
            // Full discovery supplies the concrete first container.
            container: pod
                .containers
                .first()
                .map(|c| c.name.clone())
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| "main".into()),
            client,
            confirmed_client: matches!(
                source_system.entity().system().has_binary("ranplant"),
                BinaryPresence::Present(_)
            ),
            confirmed_container: pod.containers.first().is_some_and(|c| !c.name.is_empty()),
            shell: pod.system.has_binary("sh"),
            token_file: "/var/run/secrets/kubernetes.io/serviceaccount/token".into(),
            ca_file: "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt".into(),
        })
    }
}

pub(super) struct PlannedExecPath {
    pub cost: f32,
    pub nodes: Vec<EntityId>,
    pub edges: Vec<cortex::SelectedExecEdge>,
    pub kubelet_plans: Vec<KubeletExecPlan>,
}
