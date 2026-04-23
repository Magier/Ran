//! [`EdgeData`] and the relation-weight registry.

use serde::{Deserialize, Serialize};

/// Metadata stored on every directed edge in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeData {
    /// Stable relation kind identifier (e.g. `"contains"`, `"k8s.can-exec"`).
    pub relation_name: String,
    /// Edge cost for shortest-path algorithms. Lower = preferred path.
    /// Structural edges (`contains`, `runs-on`, `uses`) carry `0.0`.
    pub weight: f32,
    /// `true` when traversing this edge grants command execution on the target.
    pub is_exec_channel: bool,
    /// For `rce.can-exec` edges: grounded exploit command template where
    /// `${CMD}` is the placeholder for the inner command. `None` otherwise.
    pub envelope: Option<String>,
}

impl EdgeData {
    pub fn new(relation_name: impl Into<String>, weight: f32, is_exec_channel: bool) -> Self {
        Self {
            relation_name: relation_name.into(),
            weight,
            is_exec_channel,
            envelope: None,
        }
    }

    pub fn with_envelope(mut self, envelope: Option<String>) -> Self {
        self.envelope = envelope;
        self
    }
}

/// Return the default `(weight, is_exec_channel)` for a given relation name.
///
/// Relations not listed here are treated as structural (weight `0.0`, not exec).
pub fn relation_defaults(name: &str) -> (f32, bool) {
    match name {
        "k8s.can-exec" => (1.0, true),
        "kubelet-exec" => (1.25, true),
        "kubelet-pod-exec" => (1.5, true),
        "container.escape" => (2.0, true),
        "rce.can-exec" => (2.5, true),
        _ => (0.0, false),
    }
}

/// Build an [`EdgeData`] from a relation name using [`relation_defaults`].
pub fn edge_data_for(relation_name: &str, envelope: Option<String>) -> EdgeData {
    let (weight, is_exec_channel) = relation_defaults(relation_name);
    EdgeData {
        relation_name: relation_name.to_string(),
        weight,
        is_exec_channel,
        envelope,
    }
}
