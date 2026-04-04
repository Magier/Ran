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

macro_rules! impl_relation_dyn {
    ($t:ty) => {
        impl $t {
            /// Returns `true` if this relation is of type `T`.
            ///
            /// Prefer this over comparing `relation_name()` strings wherever the
            /// concrete relation type is known.
            pub fn is<T: 'static>(&self) -> bool {
                self.as_any().downcast_ref::<T>().is_some()
            }

            /// Downcast to a concrete relation type, returning `None` if the
            /// type does not match.
            pub fn downcast<T: 'static>(&self) -> Option<&T> {
                self.as_any().downcast_ref::<T>()
            }
        }
    };
}

impl_relation_dyn!(dyn Relation);
impl_relation_dyn!(dyn Relation + Send);
impl_relation_dyn!(dyn Relation + Send + Sync);