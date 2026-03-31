pub mod entities;
pub mod identity;
pub mod relation;
pub mod rbac;
pub mod relations;
pub mod types;

// Re-export the most commonly used types so callers can do
// `use ran_domain::{Pod, Namespace, ServiceAccount}` without long paths.
pub use entities::{
    C2Server, Entity, GraphEntity, K8sCluster, Namespace, Pod, PodPhase,
    PodSecurityAdmission, PssLevel, ServiceAccount,
};
pub use identity::{JwToken, ServiceAccountToken};
pub use relation::Relation;
pub use rbac::RbacPermission;
pub use relations::{PodExec, Contains, RelationSummary};
pub use types::{
    AccessLevel, BinaryPresence, Confidence, Container, EntityId, K8sMeta, Mount, OwnerRef,
    Process, SystemInfo,
};

