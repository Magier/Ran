//! [`EdgeData`] and the relation-weight registry.

use ran_domain::OutputTransformKind;
use serde::{Deserialize, Serialize};

/// Metadata stored on every directed edge in the knowledge graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeData {
    /// Stable relation kind identifier (e.g. `"contains"`, `"k8s.can-exec"`).
    pub relation_name: String,
    /// Edge cost for shortest-path algorithms. Lower = preferred path.
    /// Structural edges (`contains`, `runs-on`, `uses`) carry `0.0`.
    pub weight: f32,
    /// `true` for execution transport, including transit-only endpoints.
    /// Use `grants_target_execution` before treating a destination as a host.
    pub is_exec_channel: bool,
    /// For `rce.can-exec` edges: grounded exploit command template where
    /// `${CMD}` or `${CMD_JSON}` is the placeholder for the inner command.
    /// `None` otherwise.
    pub envelope: Option<String>,
    /// Output post-processing required after routing commands over this edge.
    pub output_transform: Option<OutputTransformKind>,
    /// For `k8s.can-exec` edges: the C2 backend ID of an active persistent
    /// kubectl exec session, if one is currently open. `None` means the channel
    /// is used in one-shot (per-command kubectl exec) mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// `true` when the C2 session backing this exec-channel edge has died (e.g.
    /// the shell closed unexpectedly). The edge is kept - not removed - so it
    /// can be recovered if a session reconnects, but it is treated as
    /// non-traversable by path-finding while broken. `session_id` is retained so
    /// a reconnecting session can be matched back to this edge.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub broken: bool,
}

impl EdgeData {
    /// Native C2 entries are realized by a backend, not a nested command
    /// envelope. Arbitrary capabilities cannot stand in for these transports.
    pub fn is_backend_entry(&self) -> bool {
        self.envelope.is_none()
            && matches!(self.relation_name.as_str(), "k8s.can-exec" | "c2.session")
    }
    /// Kubelet access is transport through a Node, not a shell on that Node.
    pub fn grants_target_execution(&self) -> bool {
        self.is_exec_channel && self.relation_name != "kubelet-exec"
    }

    /// Structural eligibility for a known renderer. Campaign planning checks
    /// entity/tool prerequisites and retains a typed realization. Kubelet
    /// transit is paired with its Pod sink by the route search.
    pub fn is_realizable(&self, target: &ran_domain::EntityId) -> bool {
        if !self.is_exec_channel || self.broken || !self.weight.is_finite() || self.weight < 0.0 {
            return false;
        }
        if self.relation_name == "c2.session" {
            return self.session_id.is_some();
        }
        if self.relation_name == "kubelet-exec" && !target.0.starts_with("node/") {
            return false;
        }
        if self.relation_name == "kubelet-pod-exec" && self.envelope.is_some() {
            return false;
        }
        if let Some(envelope) = &self.envelope {
            return envelope.contains("${CMD}") || envelope.contains("${CMD_JSON}");
        }
        if self.relation_name == "kubelet-exec" {
            return target.0.len() > "node/".len()
                && matches!(
                    self.output_transform,
                    None | Some(OutputTransformKind::JsonEnvelope)
                );
        }
        matches!(
            self.relation_name.as_str(),
            "k8s.can-exec" | "kubelet-pod-exec"
        ) && target.0.starts_with("ns/")
            && target.0.contains("/pod/")
    }

    /// Typed Ranplant realization has the same decoding requirement as an
    /// explicit JSON-envelope channel. Include it before ranking graph routes.
    pub fn realization_output_transform(&self) -> Option<OutputTransformKind> {
        self.output_transform.clone().or_else(|| {
            (self.relation_name == "kubelet-exec" && self.envelope.is_none())
                .then_some(OutputTransformKind::JsonEnvelope)
        })
    }

    pub fn new(relation_name: impl Into<String>, weight: f32, is_exec_channel: bool) -> Self {
        Self {
            relation_name: relation_name.into(),
            weight,
            is_exec_channel,
            envelope: None,
            output_transform: None,
            session_id: None,
            broken: false,
        }
    }

    pub fn with_envelope(mut self, envelope: Option<String>) -> Self {
        self.envelope = envelope;
        self
    }

    pub fn with_output_transform(mut self, output_transform: Option<OutputTransformKind>) -> Self {
        self.output_transform = output_transform;
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
pub fn edge_data_for(
    relation_name: &str,
    envelope: Option<String>,
    output_transform: Option<OutputTransformKind>,
) -> EdgeData {
    let (weight, is_exec_channel) = relation_defaults(relation_name);
    EdgeData {
        relation_name: relation_name.to_string(),
        weight,
        is_exec_channel,
        envelope,
        output_transform,
        session_id: None,
        broken: false,
    }
}
