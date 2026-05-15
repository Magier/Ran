use std::collections::HashMap;

use cortex::KnowledgeGraph;
use ran_domain::{
    C2Server, Entity, EntityId, K8sCluster, K8sNode, Pod, PodExec, RelationSummary, SessionStatus,
    UnknownSystem,
};
use serde::{Deserialize, Serialize};

use c2::{ExecTtp, BUILTIN_C2_ID};

use crate::execution_record::ExecutionRecord;
use crate::external_parser::SystemFieldUpdates;
use crate::{external_parser, ParseAudit};

use super::{CampaignSystemEntityMut, CampaignSystemEntityRef, EntityStore, ExecChannel};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub entities: EntityStore,
    /// Topology and relation metadata, backed by a petgraph `StableGraph`.
    #[serde(skip)]
    pub graph: KnowledgeGraph,
    pub parse_audits: Vec<ParseAudit>,
    pub execution_records: Vec<ExecutionRecord>,
    /// Steps that have been dispatched to C2 but not yet completed.
    pub open_steps: Vec<ExecTtp>,
    /// Raw file contents captured by `file:content(path)` effects, keyed by path.
    #[serde(default)]
    pub file_contents: HashMap<String, String>,
}

impl Campaign {
    pub fn bootstrap(ran_name: impl Into<String>, target_cluster: K8sCluster) -> Self {
        let mut entities = EntityStore::default();
        let mut graph = KnowledgeGraph::new();

        let c2 = C2Server::new(ran_name.into());
        let c2_id = c2.entity_id();
        entities.insert_typed(c2);
        graph.ensure_node(c2_id);

        let cluster_id = target_cluster.entity_id();
        entities.insert_typed(target_cluster);
        graph.ensure_node(cluster_id);

        Campaign {
            entities,
            graph,
            parse_audits: Vec::new(),
            execution_records: Vec::new(),
            open_steps: Vec::new(),
            file_contents: HashMap::new(),
        }
    }

    /// Reset all campaign state back to the initial bootstrap state.
    pub fn reset(&mut self, ran_name: impl Into<String>, target_cluster: K8sCluster) {
        *self = Campaign::bootstrap(ran_name, target_cluster);
    }

    pub fn entity_count(&self) -> usize {
        self.entities.entity_count()
    }

    pub fn get_entities(&self) -> Vec<super::CampaignEntityRef<'_>> {
        self.entities.all_entities()
    }

    pub fn get_relations(&self) -> Vec<RelationSummary> {
        self.graph.to_relation_summaries()
    }

    pub fn get_parse_audits(&self) -> &[ParseAudit] {
        &self.parse_audits
    }

    pub fn get_execution_records(&self) -> &[ExecutionRecord] {
        &self.execution_records
    }

    pub fn get_open_steps(&self) -> &[ExecTtp] {
        &self.open_steps
    }

    pub fn store_file_content(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.file_contents.insert(path.into(), content.into());
    }

    pub fn get_file_content(&self, path: &str) -> Option<&str> {
        self.file_contents.get(path).map(String::as_str)
    }

    pub fn add_open_step(&mut self, exec: ExecTtp) {
        self.open_steps.push(exec);
    }

    pub fn complete_open_step(&mut self, id: &str) {
        self.open_steps.retain(|s| s.id != id);
    }

    /// Returns `true` when `id` identifies a system entity (Pod or Node).
    pub(crate) fn is_system_entity_id(&self, id: &EntityId) -> bool {
        self.entities.contains::<Pod>(id)
            || self.entities.contains::<K8sNode>(id)
            || self.entities.contains::<UnknownSystem>(id)
    }

    /// Returns the entity IDs of all systems (Pods and Nodes) that the C2 can
    /// exec into directly — seeds for Dijkstra / BFS path searches.
    pub(crate) fn direct_foothold_systems(&self) -> Vec<EntityId> {
        self.graph
            .exec_edges()
            .into_iter()
            .filter(|(src, tgt, _)| !self.is_system_entity_id(src) && self.is_system_entity_id(tgt))
            .map(|(_, tgt, _)| tgt.clone())
            .collect()
    }

    pub fn get_system_entity(&self, id: &str) -> Option<CampaignSystemEntityRef<'_>> {
        let entity_id = EntityId::new(id);

        if let Some(node) = self.entities.find::<K8sNode>(&entity_id) {
            return Some(CampaignSystemEntityRef::Node(node));
        }
        if let Some(pod) = self.entities.find::<Pod>(&entity_id) {
            return Some(CampaignSystemEntityRef::Pod(pod));
        }
        self.entities
            .find::<UnknownSystem>(&entity_id)
            .map(CampaignSystemEntityRef::Unknown)
    }

    pub fn get_system_entity_mut(&mut self, id: &str) -> Option<CampaignSystemEntityMut<'_>> {
        let entity_id = EntityId::new(id);

        if self.entities.contains::<K8sNode>(&entity_id) {
            return self
                .entities
                .find_mut::<K8sNode>(&entity_id)
                .map(CampaignSystemEntityMut::Node);
        }
        if self.entities.contains::<Pod>(&entity_id) {
            return self
                .entities
                .find_mut::<Pod>(&entity_id)
                .map(CampaignSystemEntityMut::Pod);
        }
        self.entities
            .find_mut::<UnknownSystem>(&entity_id)
            .map(CampaignSystemEntityMut::Unknown)
    }

    /// Apply partial system-info updates from an external parser to a target entity.
    pub fn apply_system_update(
        &mut self,
        target_id: &str,
        updates: &SystemFieldUpdates,
    ) -> Result<usize, String> {
        let Some(mut target) = self.get_system_entity_mut(target_id) else {
            return Err(format!("target '{}' is not a system entity", target_id));
        };
        let sys = target.entity_mut().system_mut();
        Ok(external_parser::apply_system_field_updates(sys, updates))
    }

    /// Query the knowledge graph and return the best execution channel for `target_id`.
    pub fn resolve_exec_channel(&self, target_id: &str) -> Result<ExecChannel, String> {
        self.resolve_exec_channel_inner(target_id, true)
    }

    /// Inner resolver. When `prefer_session` is false the active-session fast-path
    /// is skipped, forcing graph-based channel resolution (used for SA credential
    /// reads that must run inside the container's mount namespace, not a host-side
    /// session that may have escaped the container).
    pub(super) fn resolve_exec_channel_inner(
        &self,
        target_id: &str,
        prefer_session: bool,
    ) -> Result<ExecChannel, String> {
        let target_eid = EntityId::new(target_id);

        // Prefer an Active session on the target system — it is a live shell
        // already exiting into this entity, so no graph traversal is needed.
        if prefer_session {
            let active_session = self.get_system_entity(target_id).and_then(|sys| {
                sys.entity()
                    .system()
                    .sessions
                    .iter()
                    .find(|s| s.status == SessionStatus::Active)
                    .map(|s| s.backend_id())
            });

            if let Some(ref backend_id) = active_session {
                tracing::debug!(
                    target_id = %target_id,
                    backend_id = %backend_id,
                    "resolve_exec_channel: using active session"
                );
                return Ok(ExecChannel {
                    backend_id: backend_id.clone(),
                    hops: vec![],
                    exec_target_id: None,
                });
            }
        } else {
            tracing::debug!(
                target_id = %target_id,
                "resolve_exec_channel: session preference skipped (credential access path)"
            );
        }

        let direct_footholds: std::collections::HashSet<String> = self
            .direct_foothold_systems()
            .into_iter()
            .map(|id| id.0)
            .collect();

        if let Some(source_id) = self
            .execution_records
            .iter()
            .rev()
            .map(|r| r.target_id.as_str())
            .find(|id| direct_footholds.contains(*id))
        {
            let source_eid = EntityId::new(source_id);
            if let Some((_cost, path)) = self.graph.shortest_exec_path(&[source_eid], &target_eid) {
                let hops = path[..path.len().saturating_sub(1)]
                    .iter()
                    .map(|id| id.0.clone())
                    .collect();
                let backend_id = self.resolve_source_backend_id(source_id);
                return Ok(ExecChannel {
                    backend_id,
                    hops,
                    exec_target_id: None,
                });
            }
        }

        if let Some((src, _, _)) = self
            .graph
            .exec_edges()
            .into_iter()
            .find(|(src, tgt, _)| tgt.0 == target_id && !self.is_system_entity_id(src))
        {
            let backend_id = if src.0.starts_with("c2/") {
                src.0.clone()
            } else {
                BUILTIN_C2_ID.to_string()
            };
            return Ok(ExecChannel::direct(backend_id));
        }

        let seeds = self.direct_foothold_systems();
        if let Some((_cost, path)) = self.graph.shortest_exec_path(&seeds, &target_eid) {
            let hops = path[..path.len().saturating_sub(1)]
                .iter()
                .map(|id| id.0.clone())
                .collect();
            let backend_id = path
                .first()
                .map(|id| self.resolve_source_backend_id(&id.0))
                .unwrap_or_else(|| BUILTIN_C2_ID.to_string());
            return Ok(ExecChannel {
                backend_id,
                hops,
                exec_target_id: None,
            });
        }

        let sa_pod_id = self
            .graph
            .incoming(&target_eid)
            .into_iter()
            .find(|(_, d)| d.relation_name == "uses")
            .map(|(src, _)| src.clone());

        if let Some(pod_id) = sa_pod_id {
            // Use graph-based routing (not any active session) for the pod so the command
            // runs inside the container's mount namespace where the SA token is mounted.
            // Active sessions may be in host namespace after a container escape.
            return self
                .resolve_exec_channel_inner(&pod_id.0, false)
                .map(|mut ch| {
                    ch.exec_target_id = Some(pod_id.0);
                    ch
                });
        }

        Err(format!(
            "no viable execution channel to '{}' found in the knowledge graph \
             (no k8s.can-exec or kubelet-pod-exec path reaches this target)",
            target_id
        ))
    }

    pub fn reachable_pods(&self) -> std::collections::HashSet<String> {
        let seeds = self.direct_foothold_systems();
        let mut reachable: std::collections::HashSet<String> =
            seeds.iter().map(|id| id.0.clone()).collect();

        for id in self.graph.reachable_via_exec(&seeds) {
            if self.entities.contains::<Pod>(&id) {
                reachable.insert(id.0);
            }
        }

        reachable
    }

    pub fn resolve_exec_source(&self) -> Result<ExecChannel, String> {
        let direct_reachable: std::collections::HashSet<String> = self
            .direct_foothold_systems()
            .into_iter()
            .map(|id| id.0)
            .collect();

        if direct_reachable.is_empty() {
            return Err(
                "no compromised system available to use as a lateral-movement \
                 exec source; gain initial access first"
                    .to_string(),
            );
        }

        if let Some(system_id) = self
            .execution_records
            .iter()
            .rev()
            .map(|r| &r.target_id)
            .find(|id| direct_reachable.contains(*id))
        {
            let mut ch = ExecChannel::direct(self.resolve_source_backend_id(system_id));
            ch.exec_target_id = Some(system_id.clone());
            return Ok(ch);
        }

        let best_access = direct_reachable
            .iter()
            .filter_map(|id| {
                let sys = self.get_system_entity(id)?;
                let access = sys.entity().system().access_level;
                sys.entity()
                    .system()
                    .can_exec()
                    .then_some((id.clone(), access))
            })
            .max_by_key(|(_, access)| *access as u8)
            .map(|(id, _)| id);

        if let Some(system_id) = best_access {
            let mut ch = ExecChannel::direct(self.resolve_source_backend_id(&system_id));
            ch.exec_target_id = Some(system_id);
            return Ok(ch);
        }

        let any_system = direct_reachable
            .iter()
            .find(|id| self.get_system_entity(id).is_some())
            .cloned();

        if let Some(system_id) = any_system {
            let mut ch = ExecChannel::direct(self.resolve_source_backend_id(&system_id));
            ch.exec_target_id = Some(system_id);
            return Ok(ch);
        }

        Err(
            "no compromised system available to use as a lateral-movement \
             exec source; gain initial access first"
                .to_string(),
        )
    }

    /// Seed a pod into the campaign with a direct kubectl-exec channel from the
    /// C2 server. Used by `ran trigger` to prepare a target without prior
    /// discovery (equivalent to Go's godMode). Returns the pod's entity ID.
    pub fn seed_pod_for_trigger(&mut self, name: &str, namespace: &str) -> EntityId {
        let mut pod = Pod::new(name, namespace);
        pod.is_running = true;
        let pod_id = pod.entity_id();
        self.insert_entity(&pod);
        self.insert_relation(&PodExec::new(BUILTIN_C2_ID, pod_id.0.clone()));
        pod_id
    }

    /// Activate a session on any existing exec-channel edge pointing to `target_id`.
    /// Sets `session_id` on the edge so the frontend can render it as active.
    /// Returns `true` when a matching edge was found; the caller should only
    /// create a new `SessionChannel` relation when this returns `false`.
    pub fn activate_session_on_exec_channel(
        &mut self,
        target_id: &str,
        backend_id: &str,
    ) -> bool {
        let target_eid = EntityId::new(target_id);
        self.graph
            .activate_session_on_incoming_exec(&target_eid, backend_id.to_string())
    }

    /// Clear `session_id` from every exec-channel edge that carries `backend_id`.
    /// Called when a session is lost regardless of how it was established.
    pub fn deactivate_session(&mut self, backend_id: &str) {
        self.graph.deactivate_session(backend_id);
    }

    /// Insert an entity into the store and register its node in the graph.
    pub(crate) fn insert_entity(&mut self, entity: &dyn Entity) {
        let id = entity.entity_id();
        self.graph.ensure_node(id);
        self.entities.insert_entity(entity);
    }

    /// Insert a relation into the graph using the IDs stored on the relation itself.
    pub(crate) fn insert_relation(&mut self, rel: &dyn ran_domain::Relation) {
        let src = rel.source_id().clone();
        let tgt = rel.target_id().clone();
        self.insert_relation_with_ids(&src, &tgt, rel);
    }

    /// Resolve which C2 backend should execute commands on `system_id`.
    ///
    /// Priority:
    /// 1. An active session on the entity (live shell — most direct path).
    /// 2. A direct exec-channel edge from a `c2/<name>` source.
    /// 3. Built-in C2 (fresh kubectl exec).
    ///
    /// Preferring the active session here means that when this entity is used
    /// as an intermediate hop (e.g. pod → node via container.escape), the
    /// nsenter-wrapped command is sent through the interactive shell rather
    /// than a separate one-shot kubectl exec.
    fn resolve_source_backend_id(&self, system_id: &str) -> String {
        let system_eid = EntityId::new(system_id);
        if let Some((src, _)) = self
            .graph
            .incoming(&system_eid)
            .into_iter()
            .find(|(src, d)| {
                d.is_exec_channel && !self.is_system_entity_id(src) && src.0.starts_with("c2/")
            })
        {
            return src.0.clone();
        }

        BUILTIN_C2_ID.to_string()
    }

    pub fn all_entity_ids(&self) -> Vec<String> {
        self.entities
            .all_entities()
            .into_iter()
            .map(|e| e.entity_id().0)
            .collect()
    }

    pub fn entity_has_relation(&self, entity_id: &str, relation: &str) -> bool {
        let eid = EntityId::new(entity_id);
        !self.graph.targets_of(&eid, relation).is_empty()
    }
}

#[cfg(test)]
mod planner_helper_tests {
    use super::*;

    fn minimal_campaign() -> Campaign {
        Campaign {
            entities: EntityStore::default(),
            graph: KnowledgeGraph::new(),
            parse_audits: Vec::new(),
            execution_records: Vec::new(),
            open_steps: Vec::new(),
            file_contents: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn all_entity_ids_returns_empty_for_new_campaign() {
        let c = minimal_campaign();
        let ids = c.all_entity_ids();
        // A new campaign has no entities — the method must not panic.
        assert!(ids.is_empty());
    }

    #[test]
    fn entity_has_relation_false_when_no_relation() {
        let c = minimal_campaign();
        assert!(!c.entity_has_relation("ns/default/pod/nginx-abc", "rce.can-exec"));
    }
}
