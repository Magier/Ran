pub mod entities;
pub mod identity;
pub mod rbac;
pub mod relation;
pub mod relations;
pub mod types;

// Re-export the most commonly used types so callers can do
// `use ran_domain::{Pod, Namespace, ServiceAccount}` without long paths.
pub use entities::{
    C2Server, ConfigMap, CronJob, DaemonSet, Deployment, Entity, GCPBucket, GCPServiceAccount,
    GcpAccessToken, GraphEntity, Job, K8sCluster, K8sCredential, K8sNode, K8sRole, K8sRoleBinding,
    K8sSecret, Merge, Namespace, Pod, PodPhase, PodSecurityAdmission, PssLevel, RbacSubject,
    ReplicaSet, ServiceAccount, StatefulSet, SystemEntity, UnknownSystem,
};
pub use identity::{JwToken, ServiceAccountToken};
pub use rbac::RbacPermission;
pub use relation::{C2Channel, Relation};
pub use relations::{
    BindsTo, CanReach, ContainerEscape, Contains, Grants, KubeletExecSink, KubeletExecSource,
    ManagesNode, Owns, PodExec, RceCanExec, RelationSummary, RunsOn, SessionChannel, Uses,
};
pub use types::{
    AccessLevel, BinaryPresence, Confidence, Container, EntityId, K8sMeta, Mount, NameConfidence,
    OwnerRef, Process, SessionInfo, SessionStatus, SystemInfo,
};
