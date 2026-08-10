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
    GcpAccessToken, GraphEntity, Job, K8sCluster, K8sCredential, K8sGateway, K8sGatewayListener,
    K8sHTTPBackend, K8sHTTPRoute, K8sIngress, K8sIngressPath, K8sIngressRule, K8sIngressTLS,
    K8sNode, K8sParentRef, K8sRole, K8sRoleBinding, K8sSecret, K8sService, K8sServicePort, Merge,
    Namespace, Pod, PodPhase, PodSecurityAdmission, PssLevel, RbacSubject, ReplicaSet,
    ServiceAccount, StatefulSet, SystemEntity, UnknownSystem,
};
pub use identity::{JwToken, ServiceAccountToken};
pub use rbac::RbacPermission;
pub use relation::{C2Channel, Relation};
pub use relations::{
    AuthenticatesTo, BindsTo, CanReach, ContainerEscape, Contains, Grants, KubeletExecSink,
    KubeletExecSource, ManagesNode, Owns, PodExec, RceCanExec, RelationSummary, RunsOn,
    SessionChannel, Uses,
};
pub use types::{
    AccessLevel, BinaryPresence, Confidence, Container, EntityId, K8sMeta, Mount, NameConfidence,
    OutputTransformKind, OwnerRef, Process, SessionInfo, SessionStatus, SystemInfo,
};
