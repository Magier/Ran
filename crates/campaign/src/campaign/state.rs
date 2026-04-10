use std::collections::HashMap;

use cortex::KnowledgeGraph;
use ran_domain::{
    C2Server, ConfigMap, Deployment, Entity, EntityId, K8sCluster, K8sNode, K8sSecret, Merge,
    Namespace, Pod, RelationSummary, ServiceAccount,
};
use serde::{Deserialize, Serialize};

use c2::ExecTtp;

use crate::execution_record::ExecutionRecord;
use crate::external_parser::SystemFieldUpdates;
use crate::{external_parser, ParseAudit};

use super::{CampaignEntityRef, CampaignSystemEntityMut, CampaignSystemEntityRef, ExecChannel};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub c2_servers: HashMap<EntityId, C2Server>,
    pub clusters: HashMap<EntityId, K8sCluster>,
    pub nodes: HashMap<EntityId, K8sNode>,
    pub namespaces: HashMap<EntityId, Namespace>,
    pub pods: HashMap<EntityId, Pod>,
    pub service_accounts: HashMap<EntityId, ServiceAccount>,
    pub secrets: HashMap<EntityId, K8sSecret>,
    pub config_maps: HashMap<EntityId, ConfigMap>,
    pub deployments: HashMap<EntityId, Deployment>,
    /// Topology and relation metadata, backed by a petgraph `StableGraph`.
    /// Replaces the former `Vec<RelationSummary>` flat list.
    #[serde(skip)]
    pub graph: KnowledgeGraph,
    pub parse_audits: Vec<ParseAudit>,
    pub execution_records: Vec<ExecutionRecord>,
    /// Steps that have been dispatched to C2 but not yet completed.
    pub open_steps: Vec<ExecTtp>,
}

impl Campaign {
    pub fn bootstrap(ran_name: impl Into<String>, target_cluster: K8sCluster) -> Self {
        let mut c2_servers = HashMap::new();
        let mut clusters = HashMap::new();

        let c2 = C2Server::new(ran_name.into());
        let c2_id = c2.entity_id();
        c2_servers.insert(c2_id.clone(), c2);

        let cluster_id = target_cluster.entity_id();
        clusters.insert(cluster_id.clone(), target_cluster);

        let mut graph = KnowledgeGraph::new();
        graph.ensure_node(c2_id);
        graph.ensure_node(cluster_id);

        Campaign {
            c2_servers,
            clusters,
            nodes: HashMap::new(),
            namespaces: HashMap::new(),
            pods: HashMap::new(),
            service_accounts: HashMap::new(),
            secrets: HashMap::new(),
            config_maps: HashMap::new(),
            deployments: HashMap::new(),
            graph,
            parse_audits: Vec::new(),
            execution_records: Vec::new(),
            open_steps: Vec::new(),
        }
    }

    /// Reset all campaign state back to the initial bootstrap state.
    ///
    /// All entities, relations, audit trail and execution records are cleared.
    /// The C2 server and target cluster are re-seeded from the provided values.
    pub fn reset(&mut self, ran_name: impl Into<String>, target_cluster: K8sCluster) {
        *self = Campaign::bootstrap(ran_name, target_cluster);
    }

    pub fn entity_count(&self) -> usize {
        self.c2_servers.len()
            + self.clusters.len()
            + self.nodes.len()
            + self.namespaces.len()
            + self.pods.len()
            + self.service_accounts.len()
            + self.secrets.len()
            + self.config_maps.len()
            + self.deployments.len()
    }

    pub fn get_entities(&self) -> Vec<CampaignEntityRef<'_>> {
        let mut entities = Vec::with_capacity(self.entity_count());

        entities.extend(self.c2_servers.values().map(CampaignEntityRef::C2Server));
        entities.extend(self.clusters.values().map(CampaignEntityRef::Cluster));
        entities.extend(self.nodes.values().map(CampaignEntityRef::Node));
        entities.extend(self.namespaces.values().map(CampaignEntityRef::Namespace));
        entities.extend(self.pods.values().map(CampaignEntityRef::Pod));
        entities.extend(
            self.service_accounts
                .values()
                .map(CampaignEntityRef::ServiceAccount),
        );
        entities.extend(self.secrets.values().map(CampaignEntityRef::Secret));
        entities.extend(self.config_maps.values().map(CampaignEntityRef::ConfigMap));
        entities.extend(self.deployments.values().map(CampaignEntityRef::Deployment));

        entities
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

    pub fn get_system_entity(&self, id: &str) -> Option<CampaignSystemEntityRef<'_>> {
        let entity_id = EntityId::new(id);

        if let Some(node) = self.nodes.get(&entity_id) {
            return Some(CampaignSystemEntityRef::Node(node));
        }

        self.pods.get(&entity_id).map(CampaignSystemEntityRef::Pod)
    }

    pub fn get_system_entity_mut(&mut self, id: &str) -> Option<CampaignSystemEntityMut<'_>> {
        let entity_id = EntityId::new(id);

        if let Some(node) = self.nodes.get_mut(&entity_id) {
            return Some(CampaignSystemEntityMut::Node(node));
        }

        self.pods
            .get_mut(&entity_id)
            .map(CampaignSystemEntityMut::Pod)
    }

    /// Apply partial system-info updates from an external parser to a target
    /// entity. Returns the number of new facts written, or an error if the
    /// target is not a system entity.
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
    ///
    /// Resolution order:
    ///
    /// 1. **Follow-up from last direct foothold**: if the most recently used
    ///    directly C2-reachable pod has an exec-channel path to `target_id`,
    ///    prefer that chain so follow-up actions keep using the established
    ///    in-cluster channel.
    ///
    /// 2. **Direct**: a non-pod entity has a `k8s.can-exec` / `kubelet-pod-exec`
    ///    relation to `target_id` — the C2 reaches the target without any hop.
    ///
    /// 3. **Shortest-path (Dijkstra)**: from every pod directly reachable by the
    ///    C2 (seeds), follow exec-channel edges with their weights to find the
    ///    minimum-cost path to the target.  The path is returned as
    ///    `ExecChannel.hops`, ordered from the C2 side outward.
    ///
    /// 4. **Service-account indirection**: when the target is an SA, find the
    ///    pod that `uses` it and resolve for that pod instead.
    ///
    /// Returns `Err` when no path can be found.
    pub fn resolve_exec_channel(&self, target_id: &str) -> Result<ExecChannel, String> {
        let target_eid = EntityId::new(target_id);

        // Priority 1: prefer continuing from the most recently used direct
        // foothold when it can reach the target via exec-channel edges.
        let direct_footholds: std::collections::HashSet<String> = self
            .graph
            .exec_edges()
            .into_iter()
            .filter(|(src, tgt, _)| !self.pods.contains_key(*src) && self.pods.contains_key(*tgt))
            .map(|(_, tgt, _)| tgt.0.clone())
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
                return Ok(ExecChannel {
                    backend_id: "c2/ran".to_string(),
                    hops,
                    exec_target_id: None,
                });
            }
        }

        // Priority 2: direct — a non-pod entity has an exec relation to the
        // target (e.g. the C2's own SA has kubectl exec rights).
        let direct = self.graph.exec_edges().into_iter().any(|(src, tgt, _)| {
            tgt == &target_eid && !self.pods.contains_key(src)
        });
        if direct {
            return Ok(ExecChannel::direct("c2/ran"));
        }

        // Priority 3: Dijkstra from C2-reachable pods (seeds) to target.
        //
        // Seeds = pods that a non-pod entity can exec into directly.
        let seeds: Vec<EntityId> = self
            .graph
            .exec_edges()
            .into_iter()
            .filter(|(src, tgt, _)| {
                !self.pods.contains_key(*src) && self.pods.contains_key(*tgt)
            })
            .map(|(_, tgt, _)| tgt.clone())
            .collect();

        if let Some((_cost, path)) = self.graph.shortest_exec_path(&seeds, &target_eid) {
            // path = [seed, …, target].  hops = everything before target.
            let hops = path[..path.len().saturating_sub(1)]
                .iter()
                .map(|id| id.0.clone())
                .collect();
            return Ok(ExecChannel { backend_id: "c2/ran".to_string(), hops, exec_target_id: None });
        }

        // Priority 4: target is a service account — resolve for the pod using it.
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

    /// Collect all pod entity IDs that the C2 can run code on, either directly
    /// or transitively through prior lateral movement.
    ///
    /// A pod is reachable if:
    /// - A `k8s.can-exec` or `kubelet-pod-exec` relation points to it from a
    ///   **non-pod** source (the C2 itself has kubectl exec rights), **or**
    /// - It is reachable from any of the above via a chain of exec-channel
    ///   edges left by prior lateral movement TTPs (no depth limit).
    pub fn reachable_pods(&self) -> std::collections::HashSet<String> {
        // Seeds: pods directly reachable from non-pod exec sources.
        let seeds: Vec<EntityId> = self
            .graph
            .exec_edges()
            .into_iter()
            .filter(|(src, tgt, _)| {
                !self.pods.contains_key(*src) && self.pods.contains_key(*tgt)
            })
            .map(|(_, tgt, _)| tgt.clone())
            .collect();

        let mut reachable: std::collections::HashSet<String> =
            seeds.iter().map(|id| id.0.clone()).collect();

        // BFS through exec edges for transitively reachable pods.
        for id in self.graph.reachable_via_exec(&seeds) {
            if self.pods.contains_key(&id) {
                reachable.insert(id.0);
            }
        }

        reachable
    }

    /// Find the best compromised pod to use as a lateral-movement exec source.
    ///
    /// Lateral Movement TTPs create a new execution edge rather than require
    /// one — so they must run FROM an already-compromised system, not TO the
    /// victim.  Returns an [`ExecChannel`] whose `exec_target_id` is set to the
    /// source pod entity ID so the C2 backend can `kubectl exec` into it.
    ///
    /// Resolution priority (first match wins):
    ///
    /// 1. **Most recently used directly reachable pod** — the last execution
    ///    record whose `target_id` falls within the direct foothold set. Keeps
    ///    lateral movement in
    ///    the same foothold the operator was just working in.
    ///
    /// 2. **Highest `access_level` among directly reachable pods** — prefers
    ///    pods where we have proven interactive access (`UserExec` / `RootExec`).
    ///
    /// 3. **Any directly reachable pod** — fallback when no access-level information is
    ///    available yet (e.g. initial access via `k8s.can-exec` relation only).
    ///
    /// Returns `Err` when no directly reachable pod can be found.
    pub fn resolve_exec_source(&self) -> Result<ExecChannel, String> {
        let direct_reachable: std::collections::HashSet<String> = self
            .graph
            .exec_edges()
            .into_iter()
            .filter(|(src, tgt, _)| {
                !self.pods.contains_key(*src) && self.pods.contains_key(*tgt)
            })
            .map(|(_, tgt, _)| tgt.0.clone())
            .collect();

        if direct_reachable.is_empty() {
            return Err(
                "no compromised system available to use as a lateral-movement \
                 exec source; gain initial access first"
                    .to_string(),
            );
        }

        // --- Pick the best pod among directly reachable footholds ---

        // Priority 1: most recently used directly reachable pod.
        if let Some(pod_id) = self
            .execution_records
            .iter()
            .rev()
            .map(|r| &r.target_id)
            .find(|id| direct_reachable.contains(*id))
        {
            let mut ch = ExecChannel::direct("c2/ran");
            ch.exec_target_id = Some(pod_id.clone());
            return Ok(ch);
        }

        // Priority 2: pod with highest proven access level.
        let best_access = self
            .pods
            .values()
            .filter(|p| direct_reachable.contains(&p.entity_id().0) && p.system.can_exec())
            .max_by_key(|p| p.system.access_level as u8);

        if let Some(pod) = best_access {
            let mut ch = ExecChannel::direct("c2/ran");
            ch.exec_target_id = Some(pod.entity_id().0.clone());
            return Ok(ch);
        }

        // Priority 3: any directly reachable pod (initial access only, no exec confirmed yet).
        let any_pod = direct_reachable
            .iter()
            .find(|id| self.pods.contains_key(&EntityId::new(*id)));

        if let Some(pod_id) = any_pod {
            let mut ch = ExecChannel::direct("c2/ran");
            ch.exec_target_id = Some(pod_id.clone());
            return Ok(ch);
        }

        Err(
            "no compromised system available to use as a lateral-movement \
             exec source; gain initial access first"
                .to_string(),
        )
    }

    pub(crate) fn insert_entity(&mut self, entity: &dyn Entity) {
        let id = entity.entity_id();
        // Register the node in the graph topology (entity data lives in the maps).
        self.graph.ensure_node(id.clone());

        // Each arm uses entry().and_modify().or_insert_with() so that when an
        // entity with the same ID already exists its accumulated facts are
        // preserved via Merge::merge_from rather than being silently overwritten.
        let any = entity.as_any();
        if let Some(e) = any.downcast_ref::<Pod>() {
            self.pods
                .entry(id)
                .and_modify(|existing| existing.merge_from(e))
                .or_insert_with(|| e.clone());
        } else if let Some(e) = any.downcast_ref::<ServiceAccount>() {
            self.service_accounts
                .entry(id)
                .and_modify(|existing| existing.merge_from(e))
                .or_insert_with(|| e.clone());
        } else if let Some(e) = any.downcast_ref::<Namespace>() {
            self.namespaces
                .entry(id)
                .and_modify(|existing| existing.merge_from(e))
                .or_insert_with(|| e.clone());
        } else if let Some(e) = any.downcast_ref::<K8sCluster>() {
            self.clusters
                .entry(id)
                .and_modify(|existing| existing.merge_from(e))
                .or_insert_with(|| e.clone());
        } else if let Some(e) = any.downcast_ref::<K8sNode>() {
            self.nodes
                .entry(id)
                .and_modify(|existing| existing.merge_from(e))
                .or_insert_with(|| e.clone());
        } else if let Some(e) = any.downcast_ref::<C2Server>() {
            self.c2_servers
                .entry(id)
                .and_modify(|existing| existing.merge_from(e))
                .or_insert_with(|| e.clone());
        } else if let Some(e) = any.downcast_ref::<K8sSecret>() {
            self.secrets
                .entry(id)
                .and_modify(|existing| existing.merge_from(e))
                .or_insert_with(|| e.clone());
        } else if let Some(e) = any.downcast_ref::<ConfigMap>() {
            self.config_maps
                .entry(id)
                .and_modify(|existing| existing.merge_from(e))
                .or_insert_with(|| e.clone());
        } else if let Some(e) = any.downcast_ref::<Deployment>() {
            self.deployments
                .entry(id)
                .and_modify(|existing| existing.merge_from(e))
                .or_insert_with(|| e.clone());
        }
    }

    /// Insert a relation into the graph using the IDs stored on the relation itself.
    ///
    /// For cases where IDs need to be alias-resolved first, use
    /// [`insert_relation_with_ids`] directly.
    pub(crate) fn insert_relation(&mut self, rel: &dyn ran_domain::Relation) {
        let src = rel.source_id().clone();
        let tgt = rel.target_id().clone();
        self.insert_relation_with_ids(&src, &tgt, rel);
    }
}
