use crate::types::EntityId;

/// Core trait implemented by every directed edge in the knowledge graph.
///
/// Every relation has a name (e.g. `"contains"`, `"can-exec"`), a source
/// entity id, and a target entity id. Concrete relation types carry richer
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