use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// EntityId
// ---------------------------------------------------------------------------

/// A stable, unique identifier for every entity in the knowledge graph.
///
/// Newtype over `String` to prevent accidental mixing of IDs with other strings
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(pub String);

impl EntityId {
    pub fn new(s: impl Into<String>) -> Self {
        EntityId(s.into())
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Confidence
// ---------------------------------------------------------------------------

/// Tri-state confidence value for a fact about a system.
///
/// A fact is either unknown (not yet observed), definitely false, or definitely true.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Confidence {
    /// No information has been collected yet.
    #[default]
    Unknown,
    No,
    Yes,
}

impl Confidence {
    pub fn is_yes(&self) -> bool {
        matches!(self, Confidence::Yes)
    }
    pub fn is_unknown(&self) -> bool {
        matches!(self, Confidence::Unknown)
    }
}

impl From<bool> for Confidence {
    fn from(b: bool) -> Self {
        if b { Confidence::Yes } else { Confidence::No }
    }
}

impl From<Option<bool>> for Confidence {
    fn from(opt: Option<bool>) -> Self {
        match opt {
            Some(true) => Confidence::Yes,
            Some(false) => Confidence::No,
            None => Confidence::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// AccessLevel
// ---------------------------------------------------------------------------

/// The access level an operator holds on a system.
///
/// Ordered so that `current >= AccessLevel::UserExec` comparisons work directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessLevel {
    #[default]
    None,
    UserRead,
    UserExec,
    RootRead,
    RootExec,
}

// ---------------------------------------------------------------------------
// K8sMeta
// ---------------------------------------------------------------------------

/// Common metadata carried by every Kubernetes resource.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct K8sMeta {
    pub name: String,
    /// `None` for cluster-scoped resources (Nodes, ClusterRoles, etc.).
    pub namespace: Option<String>,
    pub uid: Option<String>,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub created_at: Option<String>,
    pub owner: Option<OwnerRef>,
}

impl K8sMeta {
    pub fn new(name: impl Into<String>) -> Self {
        K8sMeta { name: name.into(), ..Default::default() }
    }

    pub fn namespaced(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        K8sMeta {
            name: name.into(),
            namespace: Some(namespace.into()),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// OwnerRef
// ---------------------------------------------------------------------------

/// Reference to the K8s owner of a resource (e.g. ReplicaSet → Pod).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerRef {
    pub name: String,
    pub kind: String,
    pub uid: String,
}

// ---------------------------------------------------------------------------
// SystemInfo
// ---------------------------------------------------------------------------

/// Runtime capabilities gathered about a system (pod or node).
///
/// Composed as a plain value field inside `Pod` and `K8sNode`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: Option<String>,
    pub ips: Vec<IpAddr>,
    pub user_id: Option<u32>,
    pub username: Option<String>,
    pub env_vars: HashMap<String, String>,
    /// Maps binary name → presence on this system.
    pub binaries: HashMap<String, BinaryPresence>,
    pub files: Vec<String>,
    pub missing_files: Vec<String>,
    pub processes: Vec<Process>,
    pub mounts: Vec<Mount>,
    pub access_level: AccessLevel,
}

impl SystemInfo {
    pub fn has_binary(&self, name: &str) -> BinaryPresence {
        self.binaries.get(name).cloned().unwrap_or(BinaryPresence::Unknown)
    }

    pub fn can_exec(&self) -> bool {
        self.access_level >= AccessLevel::UserExec
    }

    /// Adds or updates the known path for a binary.
    pub fn set_binary(&mut self, name: impl Into<String>, path: impl Into<String>) {
        let path = path.into();
        let presence = if path.is_empty() {
            BinaryPresence::Absent
        } else {
            BinaryPresence::Present(path)
        };
        self.binaries.insert(name.into(), presence);
    }
}

// ---------------------------------------------------------------------------
// BinaryPresence
// ---------------------------------------------------------------------------

/// Whether a binary is known to exist on a system and, if so, where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryPresence {
    /// No attempt has been made to discover this binary.
    Unknown,
    Absent,
    /// Known to exist at this path.
    Present(String),
}

// ---------------------------------------------------------------------------
// Container
// ---------------------------------------------------------------------------

/// A container within a pod.
///
/// Domain-owned type. The Go version embedded `v1.Container` from the
/// Kubernetes API machinery — a large struct full of fields irrelevant to
/// adversary emulation. The k8s client layer translates to this slim type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub name: String,
    pub image: String,
}

// ---------------------------------------------------------------------------
// Mount
// ---------------------------------------------------------------------------

/// A volume mount, either a projected volume or a host-path bind mount.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Mount {
    pub name: String,
    /// Path inside the container where the volume appears.
    pub mount_point: String,
    /// Path on the host that is bound (for hostPath mounts).
    pub mount_root: String,
    pub mount_type: Option<String>,
    pub read_only: bool,
    pub is_host_path: bool,
}

// ---------------------------------------------------------------------------
// Process
// ---------------------------------------------------------------------------

/// A running process observed on a system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
    pub cmd: String,
    pub user: Option<String>,
    pub start_time: Option<String>,
}
