use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::identity::ServiceAccountToken;
use crate::rbac::RbacPermission;
use crate::types::{Confidence, Container, EntityId, K8sMeta, Mount, SystemInfo};

// ---------------------------------------------------------------------------
// Entity trait
// ---------------------------------------------------------------------------

/// Core trait implemented by every object that can live in the knowledge graph.
///
/// Unlike Go's `Entity` interface (which was satisfied via duck typing with
/// `GetId`/`GetName`/`GetKind` by any struct), Rust traits are explicit
/// opt-in. Every domain type that implements `Entity` is intentionally
/// expressing that it belongs in the graph.
pub trait Entity: std::any::Any + std::fmt::Debug + Send + Sync {
    /// Stable, unique identifier used as the graph node key.
    fn entity_id(&self) -> EntityId;
    /// Human-readable name for display.
    fn entity_name(&self) -> &str;
    /// Kind string (e.g. `"Pod"`, `"Namespace"`, `"ServiceAccount"`).
    fn entity_kind(&self) -> &str;
    /// Returns the entity as `&dyn Any` for downcasting to concrete types.
    fn as_any(&self) -> &dyn std::any::Any;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphEntity {
    C2(C2Server),
    Cluster(K8sCluster),
    Node(K8sNode),
    Namespace(Namespace),
    Pod(Pod),
    ServiceAccount(ServiceAccount),
}

impl Entity for GraphEntity {
    fn entity_id(&self) -> EntityId {
        match self {
            GraphEntity::C2(e) => e.entity_id(),
            GraphEntity::Cluster(e) => e.entity_id(),
            GraphEntity::Node(e) => e.entity_id(),
            GraphEntity::Namespace(e) => e.entity_id(),
            GraphEntity::Pod(e) => e.entity_id(),
            GraphEntity::ServiceAccount(e) => e.entity_id(),
        }
    }

    fn entity_name(&self) -> &str {
        match self {
            GraphEntity::C2(e) => e.entity_name(),
            GraphEntity::Cluster(e) => e.entity_name(),
            GraphEntity::Node(e) => e.entity_name(),
            GraphEntity::Namespace(e) => e.entity_name(),
            GraphEntity::Pod(e) => e.entity_name(),
            GraphEntity::ServiceAccount(e) => e.entity_name(),
        }
    }

    fn entity_kind(&self) -> &str {
        match self {
            GraphEntity::C2(e) => e.entity_kind(),
            GraphEntity::Cluster(e) => e.entity_kind(),
            GraphEntity::Node(e) => e.entity_kind(),
            GraphEntity::Namespace(e) => e.entity_kind(),
            GraphEntity::Pod(e) => e.entity_kind(),
            GraphEntity::ServiceAccount(e) => e.entity_kind(),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Kubernetes Node
// ---------------------------------------------------------------------------

/// A Kubernetes worker/control-plane node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sNode {
    pub name: String,
    #[serde(flatten)]
    pub system: SystemInfo,
}

impl K8sNode {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
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
            is_running: false,
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
    /// Names of Secrets associated with this SA (from SA `.secrets` field).
    pub secret_names: Vec<String>,
    /// RBAC permissions this SA holds, populated after RBAC discovery.
    pub entitlements: Vec<RbacPermission>,
}

impl ServiceAccount {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        ServiceAccount {
            meta: K8sMeta::namespaced(name, namespace),
            token: None,
            secret_names: Vec::new(),
            entitlements: Vec::new(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        self.meta.namespace.as_deref()
    }

    /// The raw JWT string, if a token has been extracted.
    pub fn raw_token(&self) -> Option<&str> {
        self.token.as_ref().filter(|t| t.has_token()).map(|t| t.raw())
    }

    /// Returns the first permission that satisfies the given verb + resource,
    /// or `None` if this SA cannot perform the operation.
    pub fn can(&self, verb: &str, resource: &str) -> Option<&RbacPermission> {
        self.entitlements.iter().find(|p| p.satisfies(verb, resource))
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
