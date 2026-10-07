use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// Origin of a fact in campaign knowledge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeProvenance {
    Scenario,
    Operator,
    Action,
    Inference,
}

// ---------------------------------------------------------------------------
// EntityId
// ---------------------------------------------------------------------------

/// A stable, unique identifier for every entity in the knowledge graph.
///
/// Newtype over `String` to prevent accidental mixing of IDs with other strings
/// and to ensure case-insensitive comparisons.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(pub String);

impl EntityId {
    pub fn new(s: impl Into<String>) -> Self {
        EntityId(s.into().to_lowercase())
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

// ---------------------------------------------------------------------------
// NameConfidence
// ---------------------------------------------------------------------------

/// Confidence in an entity's name/identity.
///
/// Every entity starts as `Derived` until an authoritative source confirms it.
/// Authoritative sources include the Kubernetes API server, SA token JWTs, and
/// output parsers that read the real name from the system (e.g. `sys.node-name`).
///
/// `Derived` covers heuristic and placeholder names: IP-derived pod names from
/// network scans, escape-host placeholder nodes, names inferred from TTP effects,
/// and any other name that was not directly observed from an authoritative source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NameConfidence {
    /// Name came from an authoritative source (K8s API, SA token JWT, etc.)
    Authoritative,
    /// Name is heuristic, placeholder, or inferred - not directly confirmed.
    #[default]
    Derived,
}

/// A graded software identity observed on an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoftwareFact {
    /// Package URL identifying the observed software. It may include a version;
    /// the separate field keeps "known product, unknown version" representable
    /// and avoids making clients parse the purl for ordinary comparisons.
    pub purl: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub confidence: NameConfidence,
    pub provenance: KnowledgeProvenance,
    pub source: String,
}

impl SoftwareFact {
    pub fn new(
        purl: impl Into<String>,
        version: Option<String>,
        confidence: NameConfidence,
        provenance: KnowledgeProvenance,
        source: impl Into<String>,
    ) -> Self {
        Self {
            purl: purl.into(),
            version,
            confidence,
            provenance,
            source: source.into(),
        }
    }
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
        if b {
            Confidence::Yes
        } else {
            Confidence::No
        }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessLevel {
    #[default]
    None,
    Exec,
}

/// Physical execution provenance. Unknown legacy locations stay unknown;
/// an API resource or credential identity must never be used as a fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionEnvironment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_id: Option<String>,
    /// Actual client binary executed, absent for native API operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Authentication identity consumed by this execution layer. This is
    /// distinct from the action-selected identity when a transport uses an
    /// ambient credential, such as a source Pod's mounted ServiceAccount.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_identity_id: Option<String>,
}

// ---------------------------------------------------------------------------
// K8sMeta
// ---------------------------------------------------------------------------

/// Common metadata carried by every Kubernetes resource.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct K8sMeta {
    pub name: String,
    /// Confidence in this resource's name.  Defaults to `Derived`; set to
    /// `Authoritative` when the name comes from the K8s API or a JWT claim.
    #[serde(default)]
    pub name_confidence: NameConfidence,
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
        K8sMeta {
            name: name.into(),
            ..Default::default()
        }
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

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    #[default]
    Connecting,
    Active,
    Lost,
}

/// A live (or pending) shell session that exits into this system.
///
/// Stored as a value inside `SystemInfo.sessions` - sessions are attributes of
/// the system they provide access to, not independent graph entities.  The `id`
/// doubles as the C2 backend key: the backend is registered as `session/<id>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    /// Protocol / kind: "tcp", "mtls", "http", …
    pub kind: String,
    /// For listener sessions: the TCP port Ran is listening on.
    pub port: Option<u16>,
    pub status: SessionStatus,
}

impl SessionInfo {
    pub fn new_connecting(
        id: impl Into<String>,
        kind: impl Into<String>,
        port: Option<u16>,
    ) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            port,
            status: SessionStatus::Connecting,
        }
    }

    pub fn backend_id(&self) -> String {
        format!("session/{}", self.id)
    }
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
    #[serde(rename = "envVars")]
    pub env_vars: HashMap<String, String>,
    /// Maps binary name → presence on this system.
    pub binaries: HashMap<String, BinaryPresence>,
    pub files: Vec<String>,
    /// Directories discovered on this system.
    #[serde(default)]
    pub directories: Vec<String>,
    /// Directories whose immediate children have been enumerated.
    #[serde(default, rename = "listedDirectories")]
    pub listed_directories: Vec<String>,
    pub processes: Vec<Process>,
    pub mounts: Vec<Mount>,
    #[serde(rename = "accessLevel")]
    pub access_level: AccessLevel,
    /// Live or pending shell sessions that exit into this system.
    pub sessions: Vec<SessionInfo>,
    /// Software identity observations, ordered by discovery time.
    #[serde(default)]
    pub software: Vec<SoftwareFact>,
}

impl SystemInfo {
    pub fn has_binary(&self, name: &str) -> BinaryPresence {
        self.binaries
            .get(name)
            .cloned()
            .unwrap_or(BinaryPresence::Unknown)
    }

    pub fn can_exec(&self) -> bool {
        self.access_level >= AccessLevel::Exec
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

    /// Merge facts from `incoming` into `self`.
    ///
    /// Rules:
    /// - `Option<T>` fields: keep `self` when already `Some`, take `incoming` when `self` is `None`.
    /// - `Vec` fields: union - append items from `incoming` not already present in `self`.
    /// - `HashMap` fields: union - `incoming` wins on key collision (newer observation is more specific).
    /// - `AccessLevel`: take the higher of the two.
    pub fn merge_from(&mut self, incoming: &Self) {
        if self.os.is_none() {
            self.os = incoming.os.clone();
        }
        if self.user_id.is_none() {
            self.user_id = incoming.user_id;
        }
        if self.username.is_none() {
            self.username = incoming.username.clone();
        }

        for ip in &incoming.ips {
            if !self.ips.contains(ip) {
                self.ips.push(*ip);
            }
        }

        // env_vars and binaries: incoming wins on collision (newer data is more specific)
        for (k, v) in &incoming.env_vars {
            self.env_vars.insert(k.clone(), v.clone());
        }
        for (k, v) in &incoming.binaries {
            self.binaries.insert(k.clone(), v.clone());
        }

        for f in &incoming.files {
            if !self.files.contains(f) {
                self.files.push(f.clone());
            }
        }
        for directory in &incoming.directories {
            if !self.directories.contains(directory) {
                self.directories.push(directory.clone());
            }
        }
        for directory in &incoming.listed_directories {
            if !self.listed_directories.contains(directory) {
                self.listed_directories.push(directory.clone());
            }
        }
        for proc in &incoming.processes {
            if !self.processes.iter().any(|p| p.pid == proc.pid) {
                self.processes.push(proc.clone());
            }
        }
        for mount in &incoming.mounts {
            if !self
                .mounts
                .iter()
                .any(|m| m.mount_point == mount.mount_point)
            {
                self.mounts.push(mount.clone());
            }
        }

        merge_software_facts(&mut self.software, &incoming.software);

        if incoming.access_level > self.access_level {
            self.access_level = incoming.access_level;
        }

        // Sessions: merge by id, only allow forward status transitions.
        for incoming_s in &incoming.sessions {
            if let Some(existing) = self.sessions.iter_mut().find(|s| s.id == incoming_s.id) {
                use SessionStatus::*;
                match (&existing.status, &incoming_s.status) {
                    (Connecting, Active) | (Connecting, Lost) | (Active, Lost) => {
                        existing.status = incoming_s.status.clone();
                    }
                    _ => {}
                }
            } else {
                self.sessions.push(incoming_s.clone());
            }
        }
    }
}

/// Merge software observations without hiding contradictions. Exact duplicate
/// observations collapse, while facts from different sources remain visible.
pub fn merge_software_facts(existing: &mut Vec<SoftwareFact>, incoming: &[SoftwareFact]) {
    for fact in incoming {
        if !existing.contains(fact) {
            existing.push(fact.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// BinaryPresence
// ---------------------------------------------------------------------------

// Whether a binary is known to exist on a system and, if so, where.

// ---------------------------------------------------------------------------
// OutputTransformKind
// ---------------------------------------------------------------------------

/// Post-processing to apply to the raw command output before any parser sees it.
///
/// Stored on exec-channel graph edges so the routing layer can read the
/// required transform directly from the relation, rather than pattern-matching
/// on relation names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputTransformKind {
    /// The raw output is a JSON response envelope (produced by Ranplant /
    /// kubelet-pod-exec).  The actual stdout must be extracted from the JSON
    /// before parsing.
    JsonEnvelope,
    /// The raw output is a JSON object whose named string field contains the
    /// command's stdout. Used by structured HTTP RCE channels that wrap the
    /// executed command's output in an application response.
    JsonField { field: String },
}

// ---------------------------------------------------------------------------
// BinaryPresence
// ---------------------------------------------------------------------------

/// Whether a binary is known to exist on a system and, if so, where.
///
/// Serialized as an object with an explicit `status` so API clients cannot
/// mistake a probed, absent binary for one that is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryPresence {
    /// No attempt has been made to discover this binary.
    Unknown,
    Absent,
    /// Known to exist at this path.
    Present(String),
}

impl serde::Serialize for BinaryPresence {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        match self {
            BinaryPresence::Present(path) if path.is_empty() => {
                Err(<S::Error as serde::ser::Error>::custom(
                    "binary presence with status 'present' requires a non-empty path",
                ))
            }
            BinaryPresence::Present(path) => {
                let mut state = s.serialize_struct("BinaryPresence", 2)?;
                state.serialize_field("status", "present")?;
                state.serialize_field("path", path)?;
                state.end()
            }
            BinaryPresence::Absent => {
                let mut state = s.serialize_struct("BinaryPresence", 1)?;
                state.serialize_field("status", "absent")?;
                state.end()
            }
            BinaryPresence::Unknown => {
                let mut state = s.serialize_struct("BinaryPresence", 1)?;
                state.serialize_field("status", "unknown")?;
                state.end()
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for BinaryPresence {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;

        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Status {
            Present,
            Absent,
            Unknown,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum WireFormat {
            Explicit {
                status: Status,
                #[serde(default)]
                path: Option<String>,
            },
            // Accept the former string/null representation when reading old
            // snapshots. New payloads are always serialized explicitly.
            Legacy(Option<String>),
        }

        match WireFormat::deserialize(d)? {
            WireFormat::Explicit {
                status: Status::Present,
                path: Some(path),
            } if !path.is_empty() => Ok(BinaryPresence::Present(path)),
            WireFormat::Explicit {
                status: Status::Present,
                ..
            } => Err(D::Error::custom(
                "binary presence with status 'present' requires a non-empty path",
            )),
            WireFormat::Explicit {
                status: Status::Absent,
                path: None,
            } => Ok(BinaryPresence::Absent),
            WireFormat::Explicit {
                status: Status::Unknown,
                path: None,
            } => Ok(BinaryPresence::Unknown),
            WireFormat::Explicit { .. } => Err(D::Error::custom(
                "binary presence path is only valid when status is 'present'",
            )),
            WireFormat::Legacy(None) => Ok(BinaryPresence::Unknown),
            WireFormat::Legacy(Some(path)) if path.is_empty() => Ok(BinaryPresence::Absent),
            WireFormat::Legacy(Some(path)) => Ok(BinaryPresence::Present(path)),
        }
    }
}

#[cfg(test)]
mod binary_presence_tests {
    use super::BinaryPresence;

    #[test]
    fn serializes_all_states_explicitly() {
        assert_eq!(
            serde_json::to_value(BinaryPresence::Present("/bin/ls".into())).unwrap(),
            serde_json::json!({"status": "present", "path": "/bin/ls"})
        );
        assert_eq!(
            serde_json::to_value(BinaryPresence::Absent).unwrap(),
            serde_json::json!({"status": "absent"})
        );
        assert_eq!(
            serde_json::to_value(BinaryPresence::Unknown).unwrap(),
            serde_json::json!({"status": "unknown"})
        );
    }

    #[test]
    fn deserializes_legacy_states_for_snapshot_compatibility() {
        assert_eq!(
            serde_json::from_value::<BinaryPresence>(serde_json::json!("/bin/ls")).unwrap(),
            BinaryPresence::Present("/bin/ls".into())
        );
        assert_eq!(
            serde_json::from_value::<BinaryPresence>(serde_json::json!("")).unwrap(),
            BinaryPresence::Absent
        );
        assert_eq!(
            serde_json::from_value::<BinaryPresence>(serde_json::Value::Null).unwrap(),
            BinaryPresence::Unknown
        );
    }

    #[test]
    fn rejects_empty_paths_in_the_explicit_format() {
        assert!(serde_json::to_value(BinaryPresence::Present(String::new())).is_err());
        assert!(serde_json::from_value::<BinaryPresence>(
            serde_json::json!({"status": "present", "path": ""})
        )
        .is_err());
    }
}

// ---------------------------------------------------------------------------
// Container
// ---------------------------------------------------------------------------

/// A container within a pod.
///
/// Domain-owned type containing only fields relevant to adversary emulation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub name: String,
    pub image: String,
    /// Arguments passed to the container entrypoint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<ContainerPort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volume_mounts: Vec<Mount>,
}

/// A port declared on a Kubernetes container. This is configuration, not
/// evidence that the endpoint is listening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerPort {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "containerPort")]
    pub port: u16,
    pub protocol: String,
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
