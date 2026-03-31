use serde::{Deserialize, Serialize};

use crate::{EntityId, Relation};

// ---------------------------------------------------------------------------
// Contains
// ---------------------------------------------------------------------------

/// A containment edge: the `container` namespace/cluster holds the `object`.
///
/// Example: Namespace "default" → contains → Pod "nginx"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contains {
    pub container_id: EntityId,
    pub object_id: EntityId,
}

impl Contains {
    pub fn new(container_id: impl Into<String>, object_id: impl Into<String>) -> Self {
        Self {
            container_id: EntityId::new(container_id),
            object_id: EntityId::new(object_id),
        }
    }
}

impl Relation for Contains {
    fn relation_name(&self) -> &str {
        "contains"
    }

    fn source_id(&self) -> &EntityId {
        &self.container_id
    }

    fn target_id(&self) -> &EntityId {
        &self.object_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// KubectlExec
// ---------------------------------------------------------------------------

/// An execution-capability edge: `executor` can exec into `target`.
///
/// Example: Pod "attacker" → can-exec → Pod "victim"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodExec {
    pub executor_id: EntityId,
    pub target_id: EntityId,
}

impl PodExec {
    pub fn new(executor_id: impl Into<String>, target_id: impl Into<String>) -> Self {
        Self {
            executor_id: EntityId::new(executor_id),
            target_id: EntityId::new(target_id),
        }
    }
}

impl Relation for PodExec {
    fn relation_name(&self) -> &str {
        "pod-exec"
    }

    fn source_id(&self) -> &EntityId {
        &self.executor_id
    }

    fn target_id(&self) -> &EntityId {
        &self.target_id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// RelationSummary
// ---------------------------------------------------------------------------

/// A lightweight, serialisable snapshot of any relation, suitable for sending
/// over the event bus or API without carrying trait objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationSummary {
    pub name: String,
    pub source_id: String,
    pub target_id: String,
}

impl RelationSummary {
    pub fn from_relation(r: &dyn Relation) -> Self {
        Self {
            name: r.relation_name().to_string(),
            source_id: r.source_id().0.clone(),
            target_id: r.target_id().0.clone(),
        }
    }
}
