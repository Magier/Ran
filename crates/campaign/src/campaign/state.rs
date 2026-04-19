use cortex::KnowledgeGraph;
use ran_domain::{C2Server, Entity, EntityId, K8sCluster, K8sNode, Pod, RelationSummary, SessionStatus, UnknownSystem};
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
            .filter(|(src, tgt, _)| {
                !self.is_system_entity_id(src) && self.is_system_entity_id(tgt)
            })
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
        let target_eid = EntityId::new(target_id);

        // Prefer an Active session on the target system — it is a live shell
        // already exiting into this entity, so no graph traversal is needed.
        let active_session = self
            .get_system_entity(target_id)
            .and_then(|sys| {
                sys.entity()
                    .system()
                    .sessions
                    .iter()
                    .find(|s| s.status == SessionStatus::Active)
                    .map(|s| s.backend_id())
            });

        if let Some(backend_id) = active_session {
            return Ok(ExecChannel {
                backend_id,
                hops: vec![],
                exec_target_id: None,
            });
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
            if let Some((_cost, path)) =
                self.graph.shortest_exec_path(&[source_eid], &target_eid)
            {
                let hops = path[..path.len().saturating_sub(1)]
                    .iter()
                    .map(|id| id.0.clone())
                    .collect();
                return Ok(ExecChannel {
                    backend_id: BUILTIN_C2_ID.to_string(),
                    hops,
                    exec_target_id: None,
                });
            }
        }

        let direct = self.graph.exec_edges().into_iter().any(|(src, tgt, _)| {
            tgt == &target_eid && !self.is_system_entity_id(src)
        });
        if direct {
            return Ok(ExecChannel::direct(BUILTIN_C2_ID));
        }

        let seeds = self.direct_foothold_systems();
        if let Some((_cost, path)) = self.graph.shortest_exec_path(&seeds, &target_eid) {
            let hops = path[..path.len().saturating_sub(1)]
                .iter()
                .map(|id| id.0.clone())
                .collect();
            return Ok(ExecChannel {
                backend_id: BUILTIN_C2_ID.to_string(),
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
            return self.resolve_exec_channel(&pod_id.0).map(|mut ch| {
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
            let mut ch = ExecChannel::direct(BUILTIN_C2_ID);
            ch.exec_target_id = Some(system_id.clone());
            return Ok(ch);
        }

        let best_access = direct_reachable
            .iter()
            .filter_map(|id| {
                let sys = self.get_system_entity(id)?;
                let access = sys.entity().system().access_level;
                sys.entity().system().can_exec().then_some((id.clone(), access))
            })
            .max_by_key(|(_, access)| *access as u8)
            .map(|(id, _)| id);

        if let Some(system_id) = best_access {
            let mut ch = ExecChannel::direct(BUILTIN_C2_ID);
            ch.exec_target_id = Some(system_id);
            return Ok(ch);
        }

        let any_system = direct_reachable
            .iter()
            .find(|id| self.get_system_entity(id).is_some())
            .cloned();

        if let Some(system_id) = any_system {
            let mut ch = ExecChannel::direct(BUILTIN_C2_ID);
            ch.exec_target_id = Some(system_id);
            return Ok(ch);
        }

        Err(
            "no compromised system available to use as a lateral-movement \
             exec source; gain initial access first"
                .to_string(),
        )
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
}
