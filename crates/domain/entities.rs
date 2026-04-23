use std::collections::HashMap;

use ambassador::{delegatable_trait, Delegate};
use serde::{Deserialize, Serialize};

use crate::identity::ServiceAccountToken;
use crate::rbac::RbacPermission;
use std::net::IpAddr;

use crate::types::{
    Confidence, Container, EntityId, K8sMeta, Mount, NameConfidence, OwnerRef, SystemInfo,
};

// ---------------------------------------------------------------------------
// Entity trait
// ---------------------------------------------------------------------------

/// Core trait implemented by every object that can live in the knowledge graph.
///
/// Unlike Go's `Entity` interface (which was satisfied via duck typing with
/// `GetId`/`GetName`/`GetKind` by any struct), Rust traits are explicit
/// opt-in. Every domain type that implements `Entity` is intentionally
/// expressing that it belongs in the graph.
#[delegatable_trait]
pub trait Entity: std::any::Any + std::fmt::Debug + Send + Sync {
    /// Stable, unique identifier used as the graph node key.
    fn entity_id(&self) -> EntityId;
    /// Human-readable name for display.
    fn entity_name(&self) -> &str;
    /// Kind string (e.g. `"Pod"`, `"Namespace"`, `"ServiceAccount"`).
    fn entity_kind(&self) -> &str;
    /// Returns the entity as `&dyn Any` for downcasting to concrete types.
    fn as_any(&self) -> &dyn std::any::Any;
    /// Confidence in this entity's name/identity.
    ///
    /// Returns [`NameConfidence::Derived`] by default; overridden by entity
    /// types whose names can come from authoritative sources (Pod, K8sNode).
    fn name_confidence(&self) -> NameConfidence {
        NameConfidence::Derived
    }
}

/// Behavior shared by graph entities that represent an executable system
/// (for example, a Pod or Node with runtime access/capability state).
pub trait SystemEntity: Entity {
    fn system(&self) -> &SystemInfo;
    fn system_mut(&mut self) -> &mut SystemInfo;
}

fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_was_dash = false;

    for ch in input.chars() {
        let lc = ch.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() {
            out.push(lc);
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }

    let out = out.trim_matches('-');
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out.to_string()
    }
}

// ---------------------------------------------------------------------------
// C2 System
// ---------------------------------------------------------------------------

/// Local C2 entity representing Ran itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct C2Server {
    pub name: String,
    /// Active listeners on this C2. Populated when listener mechanics are ported;
    /// empty by default so `exists: [Listener]` TTP pre-conditions fail safely.
    #[serde(default)]
    pub listeners: Vec<String>,
}

impl C2Server {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            listeners: Vec::new(),
        }
    }
}

impl Entity for C2Server {
    fn entity_id(&self) -> EntityId {
        EntityId::new(format!("c2/{}", slugify(&self.name)))
    }

    fn entity_name(&self) -> &str {
        &self.name
    }

    fn entity_kind(&self) -> &str {
        "C2"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Kubernetes Cluster
// ---------------------------------------------------------------------------

/// Target Kubernetes cluster from kubeconfig context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sCluster {
    pub name: String,
    pub context_name: Option<String>,
    pub server: Option<String>,
}

impl K8sCluster {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            context_name: None,
            server: None,
        }
    }

    pub fn with_context_name(mut self, context_name: Option<String>) -> Self {
        self.context_name = context_name;
        self
    }

    pub fn with_server(mut self, server: Option<String>) -> Self {
        self.server = server;
        self
    }
}

impl Entity for K8sCluster {
    fn entity_id(&self) -> EntityId {
        EntityId::new(format!("k8s/cluster/{}", slugify(&self.name)))
    }

    fn entity_name(&self) -> &str {
        &self.name
    }

    fn entity_kind(&self) -> &str {
        "Cluster"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// GraphEntity union
// ---------------------------------------------------------------------------

/// Domain-level union of entities that can exist in campaign state.
#[derive(Delegate, Debug, Clone, Serialize, Deserialize)]
#[delegate(Entity)]
pub enum GraphEntity {
    C2(C2Server),
    Cluster(K8sCluster),
    Node(K8sNode),
    Namespace(Namespace),
    Pod(Pod),
    ServiceAccount(ServiceAccount),
    Secret(K8sSecret),
    ConfigMap(ConfigMap),
    Deployment(Deployment),
    ReplicaSet(ReplicaSet),
    StatefulSet(StatefulSet),
    DaemonSet(DaemonSet),
    Job(Job),
}

// ---------------------------------------------------------------------------
// UnknownSystem
// ---------------------------------------------------------------------------

/// A system whose type has not yet been determined (pod, node, VM, …).
///
/// Created when a reverse-shell session connects and we only know the hostname,
/// user, and OS from initial probes. Further reconnaissance TTPs may reclassify
/// this into a `K8sNode`, `Pod`, or other concrete type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownSystem {
    /// Hostname from the initial probe.
    pub name: String,
    #[serde(flatten)]
    pub system: SystemInfo,
}

impl UnknownSystem {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            system: SystemInfo::default(),
        }
    }
}

impl Entity for UnknownSystem {
    fn entity_id(&self) -> EntityId {
        EntityId::new(format!("system/{}", self.name))
    }

    fn entity_name(&self) -> &str {
        &self.name
    }

    fn entity_kind(&self) -> &str {
        "UnknownSystem"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl SystemEntity for UnknownSystem {
    fn system(&self) -> &SystemInfo {
        &self.system
    }

    fn system_mut(&mut self) -> &mut SystemInfo {
        &mut self.system
    }
}

// ---------------------------------------------------------------------------
// Kubernetes Node
// ---------------------------------------------------------------------------

/// A Kubernetes worker/control-plane node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sNode {
    pub name: String,
    #[serde(default)]
    pub name_confidence: NameConfidence,
    #[serde(flatten)]
    pub system: SystemInfo,
}

impl K8sNode {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            name_confidence: NameConfidence::Derived,
            system: SystemInfo::default(),
        }
    }
}

impl Entity for K8sNode {
    fn entity_id(&self) -> EntityId {
        EntityId::new(format!("node/{}", self.name))
    }

    fn entity_name(&self) -> &str {
        &self.name
    }

    fn entity_kind(&self) -> &str {
        "Node"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn name_confidence(&self) -> NameConfidence {
        self.name_confidence
    }
}

impl SystemEntity for K8sNode {
    fn system(&self) -> &SystemInfo {
        &self.system
    }

    fn system_mut(&mut self) -> &mut SystemInfo {
        &mut self.system
    }
}

// ---------------------------------------------------------------------------
// Namespace
// ---------------------------------------------------------------------------

/// Pod Security Standards [enforcement level](https://kubernetes.io/docs/concepts/security/pod-security-standards/).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PssLevel {
    Privileged,
    Baseline,
    Restricted,
}

/// Pod Security Admission (PSA) configuration of a namespace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PodSecurityAdmission {
    pub enforce: Option<PssLevel>,
    pub warn: Option<PssLevel>,
    pub audit: Option<PssLevel>,
}

/// A Kubernetes Namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    pub name: String,
    pub psa: PodSecurityAdmission,
    pub labels: HashMap<String, String>,
}

impl Namespace {
    pub fn new(name: impl Into<String>) -> Self {
        Namespace {
            name: name.into(),
            psa: PodSecurityAdmission::default(),
            labels: HashMap::new(),
        }
    }
}

impl Entity for Namespace {
    fn entity_id(&self) -> EntityId {
        EntityId::new(format!("ns/{}", self.name))
    }
    fn entity_name(&self) -> &str {
        &self.name
    }
    fn entity_kind(&self) -> &str {
        "Namespace"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Pod
// ---------------------------------------------------------------------------

/// Kubernetes pod lifecycle phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PodPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

/// A Kubernetes Pod.
///
/// The pod↔node relationship is a `RunsOn` edge in the knowledge graph,
/// not a field on this struct. `node_name` records which node the pod was
/// scheduled to according to the API server, and is used when building that edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pod {
    pub meta: K8sMeta,
    /// Runtime capabilities collected from this pod (binaries, env vars, etc.)
    #[serde(flatten)]
    pub system: SystemInfo,

    // --- Scheduling ---
    /// `None` means the pod has not been scheduled to a node yet.
    pub node_name: Option<String>,
    /// The IP of the node this pod was scheduled to, as reported by the API server
    /// in `status.hostIP`.  Populated by the `k8s.podlist` parser and used by
    /// `PropagateHostIPAnalyzer` to fill in `node.system.ips` when no explicit
    /// node-list has been run.
    #[serde(default)]
    pub host_ip: Option<IpAddr>,

    // --- Ownership ---
    /// Direct owner references from `metadata.ownerReferences` in the K8s API.
    /// Used by `WorkloadOwnershipAnalyzer` to build workload hierarchy edges.
    #[serde(default)]
    pub owner_references: Vec<OwnerRef>,

    // --- Security context ---
    pub privileged: Confidence,
    pub host_pid: Confidence,
    pub host_ipc: Confidence,
    pub host_network: Confidence,
    pub read_only_root_fs: Confidence,

    // --- Identity ---
    pub service_account_name: Option<String>,
    pub automount_service_account_token: Confidence,

    // --- Composition ---
    pub containers: Vec<Container>,
    pub volume_mounts: Vec<Mount>,
    /// Host filesystem paths that are bind-mounted into this pod.
    pub host_paths: Vec<String>,

    // --- Runtime state ---
    pub phase: Option<PodPhase>,
    pub is_running: bool,
}

impl Pod {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        Pod {
            meta: K8sMeta::namespaced(name, namespace),
            system: SystemInfo::default(),
            node_name: None,
            host_ip: None,
            owner_references: Vec::new(),
            privileged: Confidence::Unknown,
            host_pid: Confidence::Unknown,
            host_ipc: Confidence::Unknown,
            host_network: Confidence::Unknown,
            read_only_root_fs: Confidence::Unknown,
            service_account_name: None,
            automount_service_account_token: Confidence::Unknown,
            containers: Vec::new(),
            volume_mounts: Vec::new(),
            host_paths: Vec::new(),
            phase: None,
            is_running: true,
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }

    /// Returns `true` if the pod has any host-path volume mounts.
    pub fn has_host_paths(&self) -> bool {
        self.volume_mounts.iter().any(|m| m.is_host_path)
    }

    /// Whether this pod runs with any host namespace sharing.
    pub fn shares_host_namespace(&self) -> bool {
        self.host_pid.is_yes() || self.host_ipc.is_yes() || self.host_network.is_yes()
    }
}

impl Entity for Pod {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/pod/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "Pod"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn name_confidence(&self) -> NameConfidence {
        self.meta.name_confidence
    }
}

impl SystemEntity for Pod {
    fn system(&self) -> &SystemInfo {
        &self.system
    }

    fn system_mut(&mut self) -> &mut SystemInfo {
        &mut self.system
    }
}

// ---------------------------------------------------------------------------
// ServiceAccount
// ---------------------------------------------------------------------------

/// A Kubernetes ServiceAccount with its RBAC entitlements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub meta: K8sMeta,
    /// The extracted SA token, if any has been observed.
    pub token: Option<ServiceAccountToken>,
    /// a list of the secrets in the same namespace that pods running using this ServiceAccount are allowed to use.
    pub secret_names: Vec<String>,
    /// a list of references to secrets in the same namespace to use for pulling any images in pods that reference this ServiceAccount.
    pub image_pull_secrets: Vec<String>,
    /// RBAC permissions this SA holds, populated after RBAC discovery.
    pub entitlements: Vec<RbacPermission>,
}

impl ServiceAccount {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        ServiceAccount {
            meta: K8sMeta::namespaced(name, namespace),
            token: None,
            secret_names: Vec::new(),
            image_pull_secrets: Vec::new(),
            entitlements: Vec::new(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }

    /// The raw JWT string, if a token has been extracted.
    pub fn raw_token(&self) -> Option<&str> {
        self.token
            .as_ref()
            .filter(|t| t.has_token())
            .map(|t| t.raw())
    }

    /// Returns the first permission that satisfies the given verb + resource,
    /// or `None` if this SA cannot perform the operation.
    pub fn can(&self, verb: &str, resource: &str) -> Option<&RbacPermission> {
        self.entitlements
            .iter()
            .find(|p| p.satisfies(verb, resource))
    }

    /// Returns `true` if this SA has any cluster-admin equivalent permission.
    pub fn is_cluster_admin(&self) -> bool {
        self.entitlements
            .iter()
            .any(|p| p.verb == "*" && p.resource_type == "*" && p.is_cluster_wide())
    }
}

impl Entity for ServiceAccount {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/sa/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "ServiceAccount"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// K8sSecret
// ---------------------------------------------------------------------------

/// A Kubernetes Secret discovered in the cluster.
///
/// Only the key names of `.data` are stored — never the decoded values —
/// to avoid persisting credentials in campaign state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sSecret {
    pub meta: K8sMeta,
    /// Secret type (e.g. `kubernetes.io/service-account-token`, `Opaque`).
    pub secret_type: String,
    /// Keys present in `.data` / `.stringData`, not the values.
    pub data_keys: Vec<String>,
}

impl K8sSecret {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        K8sSecret {
            meta: K8sMeta::namespaced(name, namespace),
            secret_type: String::new(),
            data_keys: Vec::new(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for K8sSecret {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/secret/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "Secret"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// ConfigMap
// ---------------------------------------------------------------------------

/// A Kubernetes ConfigMap discovered in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigMap {
    pub meta: K8sMeta,
    pub data: HashMap<String, String>,
    pub immutable: bool,
}

impl ConfigMap {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        ConfigMap {
            meta: K8sMeta::namespaced(name, namespace),
            data: HashMap::new(),
            immutable: false,
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for ConfigMap {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/cm/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "ConfigMap"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Deployment
// ---------------------------------------------------------------------------

/// A Kubernetes Deployment discovered in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub meta: K8sMeta,
}

impl Deployment {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        Deployment {
            meta: K8sMeta::namespaced(name, namespace),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for Deployment {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/deployment/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "Deployment"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// K8sRole
// ---------------------------------------------------------------------------

/// A Kubernetes Role or ClusterRole with its aggregated RBAC permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sRole {
    pub meta: K8sMeta,
    pub permissions: Vec<RbacPermission>,
    pub is_cluster_role: bool,
}

impl K8sRole {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        K8sRole {
            meta: K8sMeta::namespaced(name, namespace),
            permissions: Vec::new(),
            is_cluster_role: false,
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for K8sRole {
    fn entity_id(&self) -> EntityId {
        if self.is_cluster_role {
            EntityId::new(format!("clusterrole/{}", self.meta.name))
        } else {
            let ns = self.meta.namespace.as_deref().unwrap_or("");
            EntityId::new(format!("ns/{}/role/{}", ns, self.meta.name))
        }
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        if self.is_cluster_role {
            "ClusterRole"
        } else {
            "Role"
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// K8sRoleBinding
// ---------------------------------------------------------------------------

/// A subject reference inside a `K8sRoleBinding`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RbacSubject {
    pub kind: String,
    pub name: String,
    pub namespace: String,
}

/// A Kubernetes RoleBinding or ClusterRoleBinding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sRoleBinding {
    pub meta: K8sMeta,
    /// Name of the referenced Role or ClusterRole.
    pub role_ref: String,
    /// Kind of the referenced role: "Role" or "ClusterRole".
    #[serde(default)]
    pub role_ref_kind: String,
    pub subjects: Vec<RbacSubject>,
}

impl K8sRoleBinding {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        K8sRoleBinding {
            meta: K8sMeta::namespaced(name, namespace),
            role_ref: String::new(),
            role_ref_kind: String::new(),
            subjects: Vec::new(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for K8sRoleBinding {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        if ns.is_empty() {
            EntityId::new(format!("clusterrolebinding/{}", self.meta.name))
        } else {
            EntityId::new(format!("ns/{}/rolebinding/{}", ns, self.meta.name))
        }
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        if ns.is_empty() {
            "ClusterRoleBinding"
        } else {
            "RoleBinding"
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// CronJob
// ---------------------------------------------------------------------------

/// A Kubernetes CronJob workload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub meta: K8sMeta,
    pub schedule: Option<String>,
}

impl CronJob {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        CronJob {
            meta: K8sMeta::namespaced(name, namespace),
            schedule: None,
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for CronJob {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/cronjob/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "CronJob"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// ReplicaSet
// ---------------------------------------------------------------------------

/// A Kubernetes ReplicaSet workload controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaSet {
    pub meta: K8sMeta,
}

impl ReplicaSet {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        ReplicaSet {
            meta: K8sMeta::namespaced(name, namespace),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for ReplicaSet {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/replicaset/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "ReplicaSet"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// StatefulSet
// ---------------------------------------------------------------------------

/// A Kubernetes StatefulSet workload controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatefulSet {
    pub meta: K8sMeta,
}

impl StatefulSet {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        StatefulSet {
            meta: K8sMeta::namespaced(name, namespace),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for StatefulSet {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/statefulset/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "StatefulSet"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// DaemonSet
// ---------------------------------------------------------------------------

/// A Kubernetes DaemonSet workload controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSet {
    pub meta: K8sMeta,
}

impl DaemonSet {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        DaemonSet {
            meta: K8sMeta::namespaced(name, namespace),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for DaemonSet {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/daemonset/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "DaemonSet"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Job
// ---------------------------------------------------------------------------

/// A Kubernetes Job workload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub meta: K8sMeta,
}

impl Job {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        Job {
            meta: K8sMeta::namespaced(name, namespace),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for Job {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/job/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "Job"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// K8sService
// ---------------------------------------------------------------------------

/// A Kubernetes Service port mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sServicePort {
    pub port: i32,
    pub target_port: String,
    pub protocol: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub node_port: Option<i32>,
}

/// A Kubernetes Service discovered in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sService {
    pub meta: K8sMeta,
    /// Service type: ClusterIP, NodePort, LoadBalancer, ExternalName.
    pub service_type: String,
    /// Virtual ClusterIP assigned by the control plane (`None` for headless / ExternalName).
    #[serde(default)]
    pub cluster_ip: Option<String>,
    pub ports: Vec<K8sServicePort>,
    /// Pod selector labels mapping this service to its backend pods.
    pub selector: HashMap<String, String>,
    /// External IPs assigned to LoadBalancer services.
    #[serde(default)]
    pub external_ips: Vec<String>,
}

impl K8sService {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        K8sService {
            meta: K8sMeta::namespaced(name, namespace),
            service_type: String::new(),
            cluster_ip: None,
            ports: Vec::new(),
            selector: HashMap::new(),
            external_ips: Vec::new(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }

    pub fn is_externally_reachable(&self) -> bool {
        matches!(self.service_type.as_str(), "LoadBalancer" | "NodePort")
    }
}

impl Entity for K8sService {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/svc/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "Service"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Merge for K8sService {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
        if self.service_type.is_empty() && !incoming.service_type.is_empty() {
            self.service_type = incoming.service_type.clone();
        }
        if self.cluster_ip.is_none() {
            self.cluster_ip = incoming.cluster_ip.clone();
        }
        if self.ports.is_empty() {
            self.ports = incoming.ports.clone();
        }
        for (k, v) in &incoming.selector {
            self.selector.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for ip in &incoming.external_ips {
            if !self.external_ips.contains(ip) {
                self.external_ips.push(ip.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// K8sIngress
// ---------------------------------------------------------------------------

/// A single path rule within a K8s Ingress host rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sIngressPath {
    pub path: String,
    pub path_type: String,
    /// Backend service name.
    pub backend_service: String,
    /// Backend service port (number or named port).
    pub backend_port: String,
}

/// A host rule within a K8s Ingress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sIngressRule {
    pub host: Option<String>,
    pub paths: Vec<K8sIngressPath>,
}

/// A TLS block within a K8s Ingress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sIngressTLS {
    pub hosts: Vec<String>,
    pub secret_name: Option<String>,
}

/// A Kubernetes Ingress resource discovered in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sIngress {
    pub meta: K8sMeta,
    /// IngressClass name (e.g. `nginx`, `traefik`).
    #[serde(default)]
    pub ingress_class: Option<String>,
    pub rules: Vec<K8sIngressRule>,
    pub tls: Vec<K8sIngressTLS>,
    /// External addresses assigned by the ingress controller (IPs or hostnames).
    #[serde(default)]
    pub external_addresses: Vec<String>,
}

impl K8sIngress {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        K8sIngress {
            meta: K8sMeta::namespaced(name, namespace),
            ingress_class: None,
            rules: Vec::new(),
            tls: Vec::new(),
            external_addresses: Vec::new(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }

    /// Returns all distinct hostnames declared across all rules.
    pub fn hostnames(&self) -> Vec<&str> {
        self.rules
            .iter()
            .filter_map(|r| r.host.as_deref())
            .collect()
    }
}

impl Entity for K8sIngress {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/ingress/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "Ingress"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Merge for K8sIngress {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
        if self.ingress_class.is_none() {
            self.ingress_class = incoming.ingress_class.clone();
        }
        if self.rules.is_empty() {
            self.rules = incoming.rules.clone();
        }
        if self.tls.is_empty() {
            self.tls = incoming.tls.clone();
        }
        for addr in &incoming.external_addresses {
            if !self.external_addresses.contains(addr) {
                self.external_addresses.push(addr.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// K8sGateway (Gateway API)
// ---------------------------------------------------------------------------

/// A single listener on a Gateway API Gateway resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sGatewayListener {
    pub name: String,
    pub port: i32,
    pub protocol: String,
    #[serde(default)]
    pub hostname: Option<String>,
}

/// A Kubernetes Gateway API Gateway resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sGateway {
    pub meta: K8sMeta,
    /// GatewayClass name (e.g. `istio`, `nginx`, `cilium`).
    pub gateway_class: String,
    pub listeners: Vec<K8sGatewayListener>,
    /// External addresses assigned by the gateway controller.
    #[serde(default)]
    pub external_addresses: Vec<String>,
}

impl K8sGateway {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        K8sGateway {
            meta: K8sMeta::namespaced(name, namespace),
            gateway_class: String::new(),
            listeners: Vec::new(),
            external_addresses: Vec::new(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for K8sGateway {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/gateway/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "Gateway"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Merge for K8sGateway {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
        if self.gateway_class.is_empty() && !incoming.gateway_class.is_empty() {
            self.gateway_class = incoming.gateway_class.clone();
        }
        if self.listeners.is_empty() {
            self.listeners = incoming.listeners.clone();
        }
        for addr in &incoming.external_addresses {
            if !self.external_addresses.contains(addr) {
                self.external_addresses.push(addr.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// K8sHTTPRoute (Gateway API)
// ---------------------------------------------------------------------------

/// A reference to a parent Gateway in an HTTPRoute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sParentRef {
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub section_name: Option<String>,
}

/// A backend service reference within an HTTPRoute rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sHTTPBackend {
    pub service_name: String,
    pub service_port: String,
}

/// A Kubernetes Gateway API HTTPRoute resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sHTTPRoute {
    pub meta: K8sMeta,
    /// Parent Gateways this route attaches to.
    pub parent_refs: Vec<K8sParentRef>,
    /// Hostnames this route matches.
    #[serde(default)]
    pub hostnames: Vec<String>,
    /// Backend services reachable via this route (flattened from all rules).
    #[serde(default)]
    pub backends: Vec<K8sHTTPBackend>,
}

impl K8sHTTPRoute {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        K8sHTTPRoute {
            meta: K8sMeta::namespaced(name, namespace),
            parent_refs: Vec::new(),
            hostnames: Vec::new(),
            backends: Vec::new(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }
}

impl Entity for K8sHTTPRoute {
    fn entity_id(&self) -> EntityId {
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/httproute/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "HTTPRoute"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Merge for K8sHTTPRoute {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
        for pref in &incoming.parent_refs {
            if !self.parent_refs.iter().any(|p| p.name == pref.name) {
                self.parent_refs.push(pref.clone());
            }
        }
        for h in &incoming.hostnames {
            if !self.hostnames.contains(h) {
                self.hostnames.push(h.clone());
            }
        }
        for b in &incoming.backends {
            if !self
                .backends
                .iter()
                .any(|eb| eb.service_name == b.service_name && eb.service_port == b.service_port)
            {
                self.backends.push(b.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// K8sCredential
// ---------------------------------------------------------------------------

/// A Kubernetes API credential extracted from a kubeconfig file.
///
/// Populated by the `file:kubeconfig` and `file:content` output parsers when
/// they detect kubeconfig YAML in captured file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sCredential {
    /// API server URL (e.g. `https://10.96.0.1:6443`).
    pub endpoint: String,
    /// Base64-encoded CA certificate from the kubeconfig cluster entry.
    #[serde(default)]
    pub ca_data: Option<String>,
    /// Bearer token (service-account or user token).  Mutually exclusive with
    /// cert/key auth, but both fields are kept for partial-parse scenarios.
    #[serde(default)]
    pub token: Option<String>,
    /// Base64-encoded client certificate data (mTLS auth).
    #[serde(default)]
    pub cert_data: Option<String>,
    /// Base64-encoded client private key data (mTLS auth).
    #[serde(default)]
    pub key_data: Option<String>,
}

impl K8sCredential {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            ca_data: None,
            token: None,
            cert_data: None,
            key_data: None,
        }
    }
}

impl Entity for K8sCredential {
    fn entity_id(&self) -> EntityId {
        let slug = if self.endpoint.is_empty() {
            "unknown".to_string()
        } else {
            slugify(&self.endpoint)
        };
        EntityId::new(format!("k8s/credential/{}", slug))
    }

    fn entity_name(&self) -> &str {
        &self.endpoint
    }

    fn entity_kind(&self) -> &str {
        "K8sCredential"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// GCP Service Account
// ---------------------------------------------------------------------------

/// Access token returned by the GCP metadata service or `gcloud auth print-access-token`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GcpAccessToken {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub token_type: String,
}

/// A GCP Service Account discovered via workload identity, credential files,
/// or the GCP metadata server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCPServiceAccount {
    /// GCP SA email (e.g. `my-sa@my-project.iam.gserviceaccount.com`).
    pub email: String,
    /// GCP project ID extracted from the email domain, if parseable.
    #[serde(default)]
    pub project: Option<String>,
    /// Access token obtained from the metadata server.
    #[serde(default)]
    pub token: Option<GcpAccessToken>,
    /// Kubernetes ServiceAccount entity ID that has this GCP SA bound via
    /// Workload Identity annotation.
    #[serde(default)]
    pub bound_k8s_sa: Option<String>,
}

impl GCPServiceAccount {
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            project: None,
            token: None,
            bound_k8s_sa: None,
        }
    }
}

impl Entity for GCPServiceAccount {
    fn entity_id(&self) -> EntityId {
        let name = if self.email.is_empty() {
            "default"
        } else {
            &self.email
        };
        EntityId::new(format!("gcp-sa/{}", name))
    }

    fn entity_name(&self) -> &str {
        &self.email
    }

    fn entity_kind(&self) -> &str {
        "GCPServiceAccount"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// GCP Bucket
// ---------------------------------------------------------------------------

/// A GCP Cloud Storage bucket discovered during cloud enumeration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCPBucket {
    /// Unique bucket ID as returned by the GCS API (`id` field).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub location: Option<String>,
    /// IAM policy entries (human-readable or raw JSON strings).
    #[serde(default)]
    pub iam_policies: Vec<String>,
}

impl GCPBucket {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            location: None,
            iam_policies: Vec::new(),
        }
    }
}

impl Entity for GCPBucket {
    fn entity_id(&self) -> EntityId {
        EntityId::new(format!("gcp/bucket/{}", self.id))
    }

    fn entity_name(&self) -> &str {
        &self.name
    }

    fn entity_kind(&self) -> &str {
        "GCPBucket"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Merge trait
// ---------------------------------------------------------------------------

/// Merges facts from an `incoming` entity into an existing one already held in
/// campaign state.
///
/// The general contract — matching Go's `UpdateEntity` / `mergeObjects` — is:
/// - `Option<T>` fields: keep `self` when `Some`, take `incoming` when `self` is `None`.
/// - `Vec` fields: union (append items from `incoming` absent in `self`).
/// - `HashMap` fields: union; `incoming` wins on key collision.
/// - Scalar/enum fields: take `incoming` when it carries a more specific value
///   than the default / zero value already in `self`.
pub trait Merge {
    fn merge_from(&mut self, incoming: &Self);
}

/// Merge K8s metadata: fill gaps in `self` from `incoming`.
fn merge_k8s_meta(existing: &mut K8sMeta, incoming: &K8sMeta) {
    if existing.uid.is_none() {
        existing.uid = incoming.uid.clone();
    }
    if existing.created_at.is_none() {
        existing.created_at = incoming.created_at.clone();
    }
    if existing.owner.is_none() {
        existing.owner = incoming.owner.clone();
    }
    for (k, v) in &incoming.labels {
        existing
            .labels
            .entry(k.clone())
            .or_insert_with(|| v.clone());
    }
    for (k, v) in &incoming.annotations {
        existing
            .annotations
            .entry(k.clone())
            .or_insert_with(|| v.clone());
    }
    // Authoritative always wins over Derived.
    if incoming.name_confidence == NameConfidence::Authoritative {
        existing.name_confidence = NameConfidence::Authoritative;
    }
}

/// Merge a `Confidence` field: `Unknown` is the zero value; any concrete value
/// (`Yes` / `No`) from `incoming` overwrites an `Unknown` in `self`.
/// If `self` already holds a concrete value it is preserved (caller has
/// already observed the fact).
fn merge_confidence(existing: &mut Confidence, incoming: Confidence) {
    if *existing == Confidence::Unknown {
        *existing = incoming;
    }
}

impl Merge for UnknownSystem {
    fn merge_from(&mut self, incoming: &Self) {
        self.system.merge_from(&incoming.system);
    }
}

impl Merge for C2Server {
    fn merge_from(&mut self, incoming: &Self) {
        for l in &incoming.listeners {
            if !self.listeners.contains(l) {
                self.listeners.push(l.clone());
            }
        }
    }
}

impl Merge for K8sCluster {
    fn merge_from(&mut self, incoming: &Self) {
        if self.context_name.is_none() {
            self.context_name = incoming.context_name.clone();
        }
        if self.server.is_none() {
            self.server = incoming.server.clone();
        }
    }
}

impl Merge for K8sNode {
    fn merge_from(&mut self, incoming: &Self) {
        if incoming.name_confidence == NameConfidence::Authoritative {
            self.name_confidence = NameConfidence::Authoritative;
        }
        self.system.merge_from(&incoming.system);
    }
}

impl Merge for Namespace {
    fn merge_from(&mut self, incoming: &Self) {
        if self.psa.enforce.is_none() {
            self.psa.enforce = incoming.psa.enforce;
        }
        if self.psa.warn.is_none() {
            self.psa.warn = incoming.psa.warn;
        }
        if self.psa.audit.is_none() {
            self.psa.audit = incoming.psa.audit;
        }
        for (k, v) in &incoming.labels {
            self.labels.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
}

impl Merge for Pod {
    fn merge_from(&mut self, incoming: &Self) {
        self.system.merge_from(&incoming.system);
        merge_k8s_meta(&mut self.meta, &incoming.meta);

        if self.node_name.is_none() {
            self.node_name = incoming.node_name.clone();
        }
        if self.host_ip.is_none() {
            self.host_ip = incoming.host_ip;
        }
        for oref in &incoming.owner_references {
            if !self.owner_references.iter().any(|o| o.uid == oref.uid) {
                self.owner_references.push(oref.clone());
            }
        }
        if self.service_account_name.is_none() {
            self.service_account_name = incoming.service_account_name.clone();
        }
        if self.phase.is_none() {
            self.phase = incoming.phase;
        }
        // is_running: true is a known positive state — if either side confirms
        // the pod is running, record it. Explicit `false` from incoming is only
        // meaningful when `self` is already known to be running; preserve that
        // signal only through the dedicated "pod stopped" code path.
        if incoming.is_running {
            self.is_running = true;
        }

        merge_confidence(&mut self.privileged, incoming.privileged);
        merge_confidence(&mut self.host_pid, incoming.host_pid);
        merge_confidence(&mut self.host_ipc, incoming.host_ipc);
        merge_confidence(&mut self.host_network, incoming.host_network);
        merge_confidence(&mut self.read_only_root_fs, incoming.read_only_root_fs);
        merge_confidence(
            &mut self.automount_service_account_token,
            incoming.automount_service_account_token,
        );

        for c in &incoming.containers {
            if !self.containers.iter().any(|ec| ec.name == c.name) {
                self.containers.push(c.clone());
            }
        }
        for m in &incoming.volume_mounts {
            if !self
                .volume_mounts
                .iter()
                .any(|em| em.mount_point == m.mount_point)
            {
                self.volume_mounts.push(m.clone());
            }
        }
        for hp in &incoming.host_paths {
            if !self.host_paths.contains(hp) {
                self.host_paths.push(hp.clone());
            }
        }
    }
}

impl Merge for ServiceAccount {
    fn merge_from(&mut self, incoming: &Self) {
        // token: once discovered, never lose it
        if self.token.is_none() {
            self.token = incoming.token.clone();
        }
        // entitlements: additive — union by equality
        for perm in &incoming.entitlements {
            if !self.entitlements.contains(perm) {
                self.entitlements.push(perm.clone());
            }
        }
        for name in &incoming.secret_names {
            if !self.secret_names.contains(name) {
                self.secret_names.push(name.clone());
            }
        }
        for ips in &incoming.image_pull_secrets {
            if !self.image_pull_secrets.contains(ips) {
                self.image_pull_secrets.push(ips.clone());
            }
        }
        merge_k8s_meta(&mut self.meta, &incoming.meta);
    }
}

impl Merge for K8sSecret {
    fn merge_from(&mut self, incoming: &Self) {
        if self.secret_type.is_empty() && !incoming.secret_type.is_empty() {
            self.secret_type = incoming.secret_type.clone();
        }
        for key in &incoming.data_keys {
            if !self.data_keys.contains(key) {
                self.data_keys.push(key.clone());
            }
        }
        merge_k8s_meta(&mut self.meta, &incoming.meta);
    }
}

impl Merge for ConfigMap {
    fn merge_from(&mut self, incoming: &Self) {
        for (k, v) in &incoming.data {
            self.data.entry(k.clone()).or_insert_with(|| v.clone());
        }
        merge_k8s_meta(&mut self.meta, &incoming.meta);
    }
}

impl Merge for Deployment {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
    }
}

impl Merge for K8sRole {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
        for perm in &incoming.permissions {
            if !self.permissions.contains(perm) {
                self.permissions.push(perm.clone());
            }
        }
    }
}

impl Merge for K8sRoleBinding {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
        if self.role_ref.is_empty() {
            self.role_ref = incoming.role_ref.clone();
        }
        for subj in &incoming.subjects {
            if !self.subjects.contains(subj) {
                self.subjects.push(subj.clone());
            }
        }
    }
}

impl Merge for CronJob {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
        if self.schedule.is_none() {
            self.schedule = incoming.schedule.clone();
        }
    }
}

impl Merge for ReplicaSet {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
    }
}

impl Merge for StatefulSet {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
    }
}

impl Merge for DaemonSet {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
    }
}

impl Merge for Job {
    fn merge_from(&mut self, incoming: &Self) {
        merge_k8s_meta(&mut self.meta, &incoming.meta);
    }
}

impl Merge for K8sCredential {
    fn merge_from(&mut self, incoming: &Self) {
        if self.ca_data.is_none() {
            self.ca_data = incoming.ca_data.clone();
        }
        if self.token.is_none() {
            self.token = incoming.token.clone();
        }
        if self.cert_data.is_none() {
            self.cert_data = incoming.cert_data.clone();
        }
        if self.key_data.is_none() {
            self.key_data = incoming.key_data.clone();
        }
    }
}

impl Merge for GCPServiceAccount {
    fn merge_from(&mut self, incoming: &Self) {
        if self.project.is_none() {
            self.project = incoming.project.clone();
        }
        if self.token.is_none() {
            self.token = incoming.token.clone();
        }
        if self.bound_k8s_sa.is_none() {
            self.bound_k8s_sa = incoming.bound_k8s_sa.clone();
        }
    }
}

impl Merge for GCPBucket {
    fn merge_from(&mut self, incoming: &Self) {
        if self.location.is_none() {
            self.location = incoming.location.clone();
        }
        for p in &incoming.iam_policies {
            if !self.iam_policies.contains(p) {
                self.iam_policies.push(p.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Merge tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{JwToken, ServiceAccountToken};
    use crate::rbac::RbacPermission;

    #[test]
    fn service_account_token_preserved_after_entitlements_merge() {
        // Reproduces the reported bug: SA with token should retain it when a
        // second update adds entitlements but carries no token.
        let token = ServiceAccountToken {
            jwt: JwToken {
                raw: "eyJ...".to_string(),
                ..Default::default()
            },
            namespace: "default".to_string(),
            service_account_name: "my-sa".to_string(),
            ..Default::default()
        };

        let mut existing = ServiceAccount::new("my-sa", "default");
        existing.token = Some(token);

        let mut incoming = ServiceAccount::new("my-sa", "default");
        incoming.entitlements = vec![RbacPermission::new("get", "pods")];

        existing.merge_from(&incoming);

        assert!(
            existing.token.is_some(),
            "token must be preserved after merge"
        );
        assert_eq!(existing.entitlements.len(), 1, "entitlements must be added");
    }

    #[test]
    fn service_account_entitlements_are_unioned() {
        let perm_a = RbacPermission::new("get", "pods");
        let perm_b = RbacPermission::new("list", "secrets");

        let mut existing = ServiceAccount::new("sa", "ns");
        existing.entitlements = vec![perm_a.clone()];

        let mut incoming = ServiceAccount::new("sa", "ns");
        incoming.entitlements = vec![perm_a.clone(), perm_b.clone()];

        existing.merge_from(&incoming);

        assert_eq!(
            existing.entitlements.len(),
            2,
            "no duplicates; union gives 2"
        );
    }

    #[test]
    fn pod_system_info_merged_on_second_insert() {
        let mut existing = Pod::new("my-pod", "default");
        existing.system.access_level = crate::types::AccessLevel::Exec;
        existing.system.ips = vec!["10.0.0.1".parse().unwrap()];

        let mut incoming = Pod::new("my-pod", "default");
        incoming
            .system
            .env_vars
            .insert("SECRET".to_string(), "value".to_string());
        incoming.node_name = Some("node-1".to_string());

        existing.merge_from(&incoming);

        assert_eq!(
            existing.system.access_level,
            crate::types::AccessLevel::Exec,
            "access level preserved"
        );
        assert!(!existing.system.ips.is_empty(), "IPs preserved");
        assert_eq!(
            existing.system.env_vars.get("SECRET").map(String::as_str),
            Some("value"),
            "env var added"
        );
        assert_eq!(
            existing.node_name.as_deref(),
            Some("node-1"),
            "node_name filled in"
        );
    }

    #[test]
    fn confidence_unknown_overwritten_by_concrete_value() {
        let mut c = Confidence::Unknown;
        merge_confidence(&mut c, Confidence::Yes);
        assert_eq!(c, Confidence::Yes);

        let mut c2 = Confidence::No;
        merge_confidence(&mut c2, Confidence::Yes);
        assert_eq!(
            c2,
            Confidence::No,
            "existing concrete value must not be overwritten"
        );
    }
}
