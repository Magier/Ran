pub mod entities;
pub mod identity;
pub mod rbac;
pub mod types;

// Re-export the most commonly used types so callers can do
// `use ran_domain::{Pod, Namespace, ServiceAccount}` without long paths.
pub use entities::{
    C2Server, GraphEntity, K8sCluster, Namespace, Pod, PodPhase, PodSecurityAdmission, PssLevel,
    ServiceAccount,
};
pub use identity::{JwToken, ServiceAccountToken};
pub use rbac::RbacPermission;
pub use types::{
    AccessLevel, BinaryPresence, Confidence, Container, EntityId, K8sMeta, Mount, OwnerRef,
    Process, SystemInfo,
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
pub trait Entity {
    /// Stable, unique identifier used as the graph node key.
    fn entity_id(&self) -> EntityId;
    /// Human-readable name for display.
    fn entity_name(&self) -> &str;
    /// Kind string (e.g. `"Pod"`, `"Namespace"`, `"ServiceAccount"`).
    fn entity_kind(&self) -> &str;
}
