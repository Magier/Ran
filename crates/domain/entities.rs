use std::collections::HashMap;

use ambassador::{delegatable_trait, Delegate};
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
        let ns = self.meta.namespace.as_deref().unwrap_or("");
        EntityId::new(format!("ns/{}/role/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "K8sRole"
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
    pub subjects: Vec<RbacSubject>,
}

impl K8sRoleBinding {
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        K8sRoleBinding {
            meta: K8sMeta::namespaced(name, namespace),
            role_ref: String::new(),
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
        EntityId::new(format!("ns/{}/rolebinding/{}", ns, self.meta.name))
    }
    fn entity_name(&self) -> &str {
        &self.meta.name
    }
    fn entity_kind(&self) -> &str {
        "K8sRoleBinding"
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
        existing.labels.entry(k.clone()).or_insert_with(|| v.clone());
    }
    for (k, v) in &incoming.annotations {
        existing.annotations.entry(k.clone()).or_insert_with(|| v.clone());
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
        merge_confidence(&mut self.automount_service_account_token, incoming.automount_service_account_token);

        for c in &incoming.containers {
            if !self.containers.iter().any(|ec| ec.name == c.name) {
                self.containers.push(c.clone());
            }
        }
        for m in &incoming.volume_mounts {
            if !self.volume_mounts.iter().any(|em| em.mount_point == m.mount_point) {
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
            jwt: JwToken { raw: "eyJ...".to_string(), ..Default::default() },
            namespace: "default".to_string(),
            service_account_name: "my-sa".to_string(),
            ..Default::default()
        };

        let mut existing = ServiceAccount::new("my-sa", "default");
        existing.token = Some(token);

        let mut incoming = ServiceAccount::new("my-sa", "default");
        incoming.entitlements = vec![RbacPermission::new("get", "pods")];

        existing.merge_from(&incoming);

        assert!(existing.token.is_some(), "token must be preserved after merge");
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

        assert_eq!(existing.entitlements.len(), 2, "no duplicates; union gives 2");
    }

    #[test]
    fn pod_system_info_merged_on_second_insert() {
        let mut existing = Pod::new("my-pod", "default");
        existing.system.access_level = crate::types::AccessLevel::Exec;
        existing.system.ips = vec!["10.0.0.1".parse().unwrap()];

        let mut incoming = Pod::new("my-pod", "default");
        incoming.system.env_vars.insert("SECRET".to_string(), "value".to_string());
        incoming.node_name = Some("node-1".to_string());

        existing.merge_from(&incoming);

        assert_eq!(existing.system.access_level, crate::types::AccessLevel::Exec, "access level preserved");
        assert!(!existing.system.ips.is_empty(), "IPs preserved");
        assert_eq!(existing.system.env_vars.get("SECRET").map(String::as_str), Some("value"), "env var added");
        assert_eq!(existing.node_name.as_deref(), Some("node-1"), "node_name filled in");
    }

    #[test]
    fn confidence_unknown_overwritten_by_concrete_value() {
        let mut c = Confidence::Unknown;
        merge_confidence(&mut c, Confidence::Yes);
        assert_eq!(c, Confidence::Yes);

        let mut c2 = Confidence::No;
        merge_confidence(&mut c2, Confidence::Yes);
        assert_eq!(c2, Confidence::No, "existing concrete value must not be overwritten");
    }
}
