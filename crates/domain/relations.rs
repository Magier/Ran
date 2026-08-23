use serde::{Deserialize, Serialize};

use crate::{relation::C2Channel, EntityId, OutputTransformKind, Relation};

macro_rules! structural_relation {
    ($name:ident, $relation_name:literal) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct $name {
            pub source_id: EntityId,
            pub target_id: EntityId,
        }
        impl $name {
            pub fn new(source_id: impl Into<String>, target_id: impl Into<String>) -> Self {
                Self {
                    source_id: EntityId::new(source_id),
                    target_id: EntityId::new(target_id),
                }
            }
        }
        impl Relation for $name {
            fn relation_name(&self) -> &str {
                $relation_name
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
    };
}

structural_relation!(HostsService, "hosts-service");

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
///
/// Carries the same `envelope` / `output_transform` fields as `RceCanExec` so
/// the routing engine can wrap inner commands and post-process output
/// without matching on the relation name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubeletExecSource {
    pub pod_id: EntityId,
    pub node_id: EntityId,
    /// Command template with `${CMD}` as the inner-command placeholder, e.g.
    /// `ran-ws --url "wss://…" --token … -- ${CMD}`.
    /// Stored from `PROCEDURE_CMD` at effect-parse time so routing can call
    /// `rel.wrap_command(inner_cmd)` without knowing about ran-ws.
    pub envelope: Option<String>,
    /// Output post-processing required for commands routed over this channel.
    pub output_transform: Option<OutputTransformKind>,
}

impl KubeletExecSource {
    pub fn new(pod_id: impl Into<String>, node_id: impl Into<String>) -> Self {
        Self {
            pod_id: EntityId::new(pod_id),
            node_id: EntityId::new(node_id),
            envelope: None,
            output_transform: None,
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

    pub fn with_output_transform(mut self, t: OutputTransformKind) -> Self {
        self.output_transform = Some(t);
        self
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

// ---------------------------------------------------------------------------
// AuthenticatesTo
// ---------------------------------------------------------------------------

/// A credential is valid for authentication to a Kubernetes cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatesTo {
    pub credential_id: EntityId,
    pub cluster_id: EntityId,
}

impl AuthenticatesTo {
    pub fn new(credential_id: impl Into<String>, cluster_id: impl Into<String>) -> Self {
        Self {
            credential_id: EntityId::new(credential_id),
            cluster_id: EntityId::new(cluster_id),
        }
    }
}

impl Relation for AuthenticatesTo {
    fn relation_name(&self) -> &str {
        "authenticates-to"
    }

    fn source_id(&self) -> &EntityId {
        &self.credential_id
    }

    fn target_id(&self) -> &EntityId {
        &self.cluster_id
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
// SessionChannel
// ---------------------------------------------------------------------------

/// An active reverse-shell session: `source` (C2Server) has a live shell
/// into `target` (K8sNode or Pod), identified by the C2 backend `session_id`.
///
/// Implements `C2Channel` — commands can be routed through this edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionChannel {
    pub source_id: EntityId,
    pub target_id: EntityId,
    /// The C2 backend id for this session (e.g. `session/c2-ran-1337`).
    pub session_id: String,
}

impl SessionChannel {
    pub fn new(
        source_id: impl Into<String>,
        target_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            source_id: EntityId::new(source_id),
            target_id: EntityId::new(target_id),
            session_id: session_id.into(),
        }
    }
}

impl C2Channel for SessionChannel {}

impl Relation for SessionChannel {
    fn relation_name(&self) -> &str {
        "c2.session"
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
// BindsTo
// ---------------------------------------------------------------------------

/// RBAC binding edge: a RoleBinding or ClusterRoleBinding references a Role or
/// ClusterRole. Source is the binding; target is the role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindsTo {
    pub binding_id: EntityId,
    pub role_id: EntityId,
}

impl BindsTo {
    pub fn new(binding_id: impl Into<String>, role_id: impl Into<String>) -> Self {
        Self {
            binding_id: EntityId::new(binding_id),
            role_id: EntityId::new(role_id),
        }
    }
}

impl Relation for BindsTo {
    fn relation_name(&self) -> &str {
        "binds-to"
    }

    fn source_id(&self) -> &EntityId {
        &self.binding_id
    }

    fn target_id(&self) -> &EntityId {
        &self.role_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Grants
// ---------------------------------------------------------------------------

/// RBAC grant edge: a RoleBinding or ClusterRoleBinding grants permissions to a
/// subject (ServiceAccount). Source is the binding; target is the subject.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grants {
    pub binding_id: EntityId,
    pub subject_id: EntityId,
}

impl Grants {
    pub fn new(binding_id: impl Into<String>, subject_id: impl Into<String>) -> Self {
        Self {
            binding_id: EntityId::new(binding_id),
            subject_id: EntityId::new(subject_id),
        }
    }
}

impl Relation for Grants {
    fn relation_name(&self) -> &str {
        "grants"
    }

    fn source_id(&self) -> &EntityId {
        &self.binding_id
    }

    fn target_id(&self) -> &EntityId {
        &self.subject_id
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
    /// For exec-channel edges: the grounded command template with `${CMD}` as
    /// the placeholder for the inner command.  `None` for structural relations.
    pub envelope: Option<String>,
    /// Output post-processing required after a command is routed over this
    /// channel.  Read by the execution pipeline; `None` means raw output.
    pub output_transform: Option<OutputTransformKind>,
    /// Edge cost for shortest-path queries. Populated from the graph layer;
    /// defaults to `0.0` for structural relations and positive values for
    /// exec-channel relations (lower = preferred path).
    #[serde(default)]
    pub weight: f32,
    /// For `k8s.can-exec` edges: C2 backend ID of the active persistent kubectl
    /// exec session, if one is open. `None` = one-shot per-command exec mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

impl RelationSummary {
    pub fn from_relation(r: &dyn Relation) -> Self {
        // Extract envelope from known relation types that carry one.
        let envelope = r
            .as_any()
            .downcast_ref::<RceCanExec>()
            .and_then(|rce| rce.envelope.clone())
            .or_else(|| {
                r.as_any()
                    .downcast_ref::<ContainerEscape>()
                    .and_then(|e| e.envelope.clone())
            })
            .or_else(|| {
                r.as_any()
                    .downcast_ref::<KubeletExecSource>()
                    .and_then(|k| k.envelope.clone())
            });

        // Extract output_transform from KubeletExecSource (the only type that
        // carries one today; extend here for future channel types).
        let output_transform = r
            .as_any()
            .downcast_ref::<KubeletExecSource>()
            .and_then(|k| k.output_transform.clone());

        let session_id = r
            .as_any()
            .downcast_ref::<SessionChannel>()
            .map(|s| s.session_id.clone());

        Self {
            name: r.relation_name().to_string(),
            source_id: r.source_id().0.clone(),
            target_id: r.target_id().0.clone(),
            is_exec_channel: r.is_exec_channel(),
            envelope,
            output_transform,
            weight: 0.0,
            session_id,
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

#[cfg(test)]
mod app_service_relation_tests {
    use super::*;

    #[test]
    fn endpoint_relations_are_structural_summaries() {
        for relation in [
            RelationSummary::from_relation(&HostsService::new(
                "system/a",
                "app-service/tcp/host/80",
            )),
            RelationSummary::from_relation(&CanReach::new("system/b", "app-service/tcp/host/80")),
        ] {
            assert!(!relation.is_exec_channel);
            assert!(matches!(
                relation.name.as_str(),
                "hosts-service" | "can-reach"
            ));
        }
    }
}
