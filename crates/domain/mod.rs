pub mod entities;
pub mod identity;
pub mod rbac;
pub mod relations;
pub mod types;

// Re-export the most commonly used types so callers can do
// `use ran_domain::{Pod, Namespace, ServiceAccount}` without long paths.
pub use entities::{
    C2Server, GraphEntity, K8sCluster, Namespace, Pod, PodPhase, PodSecurityAdmission, PssLevel,
    ServiceAccount,
};
pub use identity::{JwToken, ServiceAccountToken};
pub use rbac::RbacPermission;
pub use relations::{CanExec, Contains, RelationSummary};
pub use types::{
    AccessLevel, BinaryPresence, Confidence, Container, EntityId, K8sMeta, Mount, OwnerRef,
    Process, SystemInfo,
};

// ---------------------------------------------------------------------------
// Relation trait
// ---------------------------------------------------------------------------

/// Core trait implemented by every directed edge in the knowledge graph.
///
/// Every relation has a name (e.g. `"contains"`, `"can-exec"`), a source
/// entity id, and a target entity id.  Concrete relation types carry richer
/// typed fields; this trait provides the common accessor surface.
pub trait Relation: std::any::Any + std::fmt::Debug + Send + Sync {
    /// Short, stable name used to identify the relation kind (e.g. `"contains"`).
    fn relation_name(&self) -> &str;
    /// Id of the source (container / subject) entity.
    fn source_id(&self) -> &EntityId;
    /// Id of the target (object) entity.
    fn target_id(&self) -> &EntityId;
    /// Returns the relation as `&dyn Any` for downcasting.
    fn as_any(&self) -> &dyn std::any::Any;
}

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
