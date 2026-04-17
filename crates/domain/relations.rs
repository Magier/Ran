use serde::{Deserialize, Serialize};

use crate::{relation::C2Channel, EntityId, Relation};

// ---------------------------------------------------------------------------
// Contains
// ---------------------------------------------------------------------------

/// A containment edge: the `container` namespace/cluster holds the `object`.
///
/// Example: Namespace "default" → contains → Pod "nginx"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contains {
    pub container_id: EntityId,
    pub object_id: EntityId,
}

impl Contains {
    pub fn new(container_id: impl Into<String>, object_id: impl Into<String>) -> Self {
        Self {
            container_id: EntityId::new(container_id),
            object_id: EntityId::new(object_id),
        }
    }
}

impl Relation for Contains {
    fn relation_name(&self) -> &str {
        "contains"
    }

    fn source_id(&self) -> &EntityId {
        &self.container_id
    }

    fn target_id(&self) -> &EntityId {
        &self.object_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// KubectlExec
// ---------------------------------------------------------------------------

/// An execution-capability edge: `executor` can exec into `target`.
///
/// Example: Pod "attacker" → can-exec → Pod "victim"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodExec {
    pub executor_id: EntityId,
    pub target_id: EntityId,
}

impl PodExec {
    pub fn new(executor_id: impl Into<String>, target_id: impl Into<String>) -> Self {
        Self {
            executor_id: EntityId::new(executor_id),
            target_id: EntityId::new(target_id),
        }
    }
}

impl C2Channel for PodExec {}

impl Relation for PodExec {
    fn relation_name(&self) -> &str {
        "k8s.can-exec"
    }

    fn source_id(&self) -> &EntityId {
        &self.executor_id
    }

    fn target_id(&self) -> &EntityId {
        &self.target_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn is_exec_channel(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// RunsOn
// ---------------------------------------------------------------------------

/// Scheduling relation from Pod to Node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunsOn {
    pub pod_id: EntityId,
    pub node_id: EntityId,
}

impl RunsOn {
    pub const RELATION_NAME: &'static str = "runs-on";

    pub fn new(pod_id: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self {
            pod_id: EntityId::new(pod_id),
            node_id: EntityId::new(node_id),
        }
    }
}

impl Relation for RunsOn {
    fn relation_name(&self) -> &str {
        "runs-on"
    }

    fn source_id(&self) -> &EntityId {
        &self.pod_id
    }

    fn target_id(&self) -> &EntityId {
        &self.node_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// KubeletExecSource / KubeletExecSink
// ---------------------------------------------------------------------------

/// Pod→Node relation indicating source pod can invoke kubelet exec on node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubeletExecSource {
    pub pod_id: EntityId,
    pub node_id: EntityId,
}

impl KubeletExecSource {
    pub fn new(pod_id: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self {
            pod_id: EntityId::new(pod_id),
            node_id: EntityId::new(node_id),
        }
    }
}

impl Relation for KubeletExecSource {
    fn relation_name(&self) -> &str {
        "kubelet-exec"
    }

    fn source_id(&self) -> &EntityId {
        &self.pod_id
    }

    fn target_id(&self) -> &EntityId {
        &self.node_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Node→Pod relation indicating kubelet exec sink path to a target pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubeletExecSink {
    pub node_id: EntityId,
    pub pod_id: EntityId,
}

impl KubeletExecSink {
    pub fn new(node_id: impl Into<String>, pod_id: impl Into<String>) -> Self {
        Self {
            node_id: EntityId::new(node_id),
            pod_id: EntityId::new(pod_id),
        }
    }
}

impl C2Channel for KubeletExecSink {}

impl Relation for KubeletExecSink {
    fn relation_name(&self) -> &str {
        "kubelet-pod-exec"
    }

    fn source_id(&self) -> &EntityId {
        &self.node_id
    }

    fn target_id(&self) -> &EntityId {
        &self.pod_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn is_exec_channel(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// RceCanExec
// ---------------------------------------------------------------------------

/// An RCE execution-channel edge: `source` can execute arbitrary commands on
/// `target` via a remote code execution primitive (e.g. Redis SSRF, exploit).
///
/// Example: Pod "attacker" → rce.can-exec → Pod "redis-victim"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RceCanExec {
    pub source_id: EntityId,
    pub target_id: EntityId,
    /// The grounded exploit command template with `${CMD}` as the inner-command
    /// placeholder, e.g. `redis-cli -h 10.0.0.1 -p 6379 EVAL "..." 0 "${CMD}"`.
    /// Set when the relation is created from a lateral-movement TTP execution so
    /// subsequent commands routed over this edge can re-invoke the exploit.
    pub envelope: Option<String>,
}

impl RceCanExec {
    pub fn new(source_id: impl Into<String>, target_id: impl Into<String>) -> Self {
        Self {
            source_id: EntityId::new(source_id),
            target_id: EntityId::new(target_id),
            envelope: None,
        }
    }

    pub fn with_envelope(mut self, envelope: impl Into<String>) -> Self {
        self.envelope = Some(envelope.into());
        self
    }

    pub fn with_opt_envelope(mut self, envelope: Option<String>) -> Self {
        self.envelope = envelope;
        self
    }
}

impl C2Channel for RceCanExec {}

impl Relation for RceCanExec {
    fn relation_name(&self) -> &str {
        "rce.can-exec"
    }

    fn source_id(&self) -> &EntityId {
        &self.source_id
    }

    fn target_id(&self) -> &EntityId {
        &self.target_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn is_exec_channel(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// ContainerEscape
// ---------------------------------------------------------------------------

/// An execution-channel edge representing a container escape: the `source` pod
/// can execute commands on the `target` node by breaking out of its container
/// namespace (e.g. via nsenter, chroot, or a privileged container mount).
///
/// Carries an `envelope` — the grounded escape command template with `${CMD}`
/// as the placeholder for the inner command, e.g.
/// `nsenter -t 1 -m -u -i -n -p -- ${CMD}`.
///
/// Example: Pod "attacker" → container.escape → Node "worker-1"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerEscape {
    pub source_id: EntityId,
    pub target_id: EntityId,
    /// The grounded escape command template with `${CMD}` as the inner-command
    /// placeholder, e.g. `nsenter -t 1 -m -u -i -n -p -- ${CMD}` or
    /// `chroot /host ${CMD}`.
    pub envelope: Option<String>,
}

impl ContainerEscape {
    pub fn new(source_id: impl Into<String>, target_id: impl Into<String>) -> Self {
        Self {
            source_id: EntityId::new(source_id),
            target_id: EntityId::new(target_id),
            envelope: None,
        }
    }

    pub fn with_envelope(mut self, envelope: impl Into<String>) -> Self {
        self.envelope = Some(envelope.into());
        self
    }

    pub fn with_opt_envelope(mut self, envelope: Option<String>) -> Self {
        self.envelope = envelope;
        self
    }
}

impl C2Channel for ContainerEscape {}

impl Relation for ContainerEscape {
    fn relation_name(&self) -> &str {
        "container.escape"
    }

    fn source_id(&self) -> &EntityId {
        &self.source_id
    }

    fn target_id(&self) -> &EntityId {
        &self.target_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn is_exec_channel(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Uses
// ---------------------------------------------------------------------------

/// Workload-identity relation: the subject (pod) uses the object (service account).
///
/// Created when a pod has `service_account_name` set and automounting is not
/// explicitly disabled, indicating the pod's containers receive the SA token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Uses {
    pub subject_id: EntityId,
    pub object_id: EntityId,
}

impl Uses {
    pub fn new(subject_id: impl Into<String>, object_id: impl Into<String>) -> Self {
        Self {
            subject_id: EntityId::new(subject_id),
            object_id: EntityId::new(object_id),
        }
    }
}

impl Relation for Uses {
    fn relation_name(&self) -> &str {
        "uses"
    }

    fn source_id(&self) -> &EntityId {
        &self.subject_id
    }

    fn target_id(&self) -> &EntityId {
        &self.object_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ManagesNode
// ---------------------------------------------------------------------------

/// Cluster-membership relation: the cluster manages (owns) the node.
///
/// High-priority compound-node relation — the graph renderer nests the node
/// inside the cluster compound node when this edge is present.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagesNode {
    pub cluster_id: EntityId,
    pub node_id: EntityId,
}

impl ManagesNode {
    pub fn new(cluster_id: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self {
            cluster_id: EntityId::new(cluster_id),
            node_id: EntityId::new(node_id),
        }
    }
}

impl Relation for ManagesNode {
    fn relation_name(&self) -> &str {
        "manages-node"
    }

    fn source_id(&self) -> &EntityId {
        &self.cluster_id
    }

    fn target_id(&self) -> &EntityId {
        &self.node_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// CanReach
// ---------------------------------------------------------------------------

/// Network-reachability relation: `source` can reach `target` over the network.
///
/// This is a *precondition* edge, not an execution channel.  Emitted by network
/// scan parsers (e.g. nmap) to record that a host is reachable from another.
/// Does **not** implement `C2Channel` — reachability alone is not an exec path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanReach {
    pub source_id: EntityId,
    pub target_id: EntityId,
}

impl CanReach {
    pub fn new(source_id: impl Into<String>, target_id: impl Into<String>) -> Self {
        Self {
            source_id: EntityId::new(source_id),
            target_id: EntityId::new(target_id),
        }
    }
}

impl Relation for CanReach {
    fn relation_name(&self) -> &str {
        "can-reach"
    }

    fn source_id(&self) -> &EntityId {
        &self.source_id
    }

    fn target_id(&self) -> &EntityId {
        &self.target_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Owns
// ---------------------------------------------------------------------------

/// Workload ownership relation: a workload controller owns a pod.
///
/// Emitted by `WorkloadOwnershipAnalyzer` from `metadata.ownerReferences` on
/// pods.  Source is the workload (ReplicaSet, StatefulSet, DaemonSet, Job);
/// target is the owned Pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Owns {
    pub owner_id: EntityId,
    pub object_id: EntityId,
}

impl Owns {
    pub fn new(owner_id: impl Into<String>, object_id: impl Into<String>) -> Self {
        Self {
            owner_id: EntityId::new(owner_id),
            object_id: EntityId::new(object_id),
        }
    }
}

impl Relation for Owns {
    fn relation_name(&self) -> &str {
        "owns"
    }

    fn source_id(&self) -> &EntityId {
        &self.owner_id
    }

    fn target_id(&self) -> &EntityId {
        &self.object_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// RelationSummary
// ---------------------------------------------------------------------------

/// A lightweight, serialisable snapshot of any relation, suitable for sending
/// over the event bus or API without carrying trait objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationSummary {
    pub name: String,
    pub source_id: String,
    pub target_id: String,
    /// `true` when the source entity can execute commands on the target (i.e.
    /// the originating relation implements [`C2Channel`]).
    pub is_exec_channel: bool,
    /// For `rce.can-exec` edges: the grounded exploit command template with
    /// `${CMD}` as the placeholder for the command to inject.  `None` for all
    /// other relation types.
    pub envelope: Option<String>,
    /// Edge cost for shortest-path queries. Populated from the graph layer;
    /// defaults to `0.0` for structural relations and positive values for
    /// exec-channel relations (lower = preferred path).
    #[serde(default)]
    pub weight: f32,
}

impl RelationSummary {
    pub fn from_relation(r: &dyn Relation) -> Self {
        let envelope = r
            .as_any()
            .downcast_ref::<RceCanExec>()
            .and_then(|rce| rce.envelope.clone())
            .or_else(|| {
                r.as_any()
                    .downcast_ref::<ContainerEscape>()
                    .and_then(|e| e.envelope.clone())
            });
        Self {
            name: r.relation_name().to_string(),
            source_id: r.source_id().0.clone(),
            target_id: r.target_id().0.clone(),
            is_exec_channel: r.is_exec_channel(),
            envelope,
            weight: 0.0,
        }
    }

    /// Wrap `cmd` with the appropriate execution primitive for this channel.
    ///
    /// - If the relation carries an `envelope` (e.g. a grounded `redis-cli … ${CMD}` exploit
    ///   template), substitutes `${CMD}` with `cmd`.
    /// - Otherwise falls back to `kubectl exec -n <ns> <name> -- <cmd>` by parsing the
    ///   target entity ID in the canonical `ns/<ns>/pod/<name>` format.
    pub fn wrap_command(&self, cmd: &str) -> String {
        if let Some(ref envelope) = self.envelope {
            return envelope.replace("${CMD}", cmd);
        }
        // Default: kubectl exec into the target pod
        if let Some((ns, name)) = Self::split_pod_entity_id(&self.target_id) {
            format!("kubectl exec -n {} {} -- {}", ns, name, cmd)
        } else {
            cmd.to_string()
        }
    }

    fn split_pod_entity_id(entity_id: &str) -> Option<(&str, &str)> {
        let mut parts = entity_id.splitn(5, '/');
        let kind_a = parts.next()?;
        let namespace = parts.next()?;
        let kind_b = parts.next()?;
        let pod_name = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        if kind_a != "ns" || kind_b != "pod" || namespace.is_empty() || pod_name.is_empty() {
            return None;
        }
        Some((namespace, pod_name))
    }
}
