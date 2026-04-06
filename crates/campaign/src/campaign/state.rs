use std::collections::{HashMap, VecDeque};

use ran_domain::{
    C2Server, Entity, EntityId, K8sCluster, K8sNode, Namespace, Pod, RelationSummary,
    ServiceAccount,
};
use serde::{Deserialize, Serialize};

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
    pub relations: Vec<RelationSummary>,
    pub parse_audits: Vec<ParseAudit>,
    pub execution_records: Vec<ExecutionRecord>,
}

impl Campaign {
    pub fn bootstrap(ran_name: impl Into<String>, target_cluster: K8sCluster) -> Self {
        let mut c2_servers = HashMap::new();
        let mut clusters = HashMap::new();

        let c2 = C2Server::new(ran_name.into());
        c2_servers.insert(c2.entity_id(), c2);

        clusters.insert(target_cluster.entity_id(), target_cluster);

        Campaign {
            c2_servers,
            clusters,
            nodes: HashMap::new(),
            namespaces: HashMap::new(),
            pods: HashMap::new(),
            service_accounts: HashMap::new(),
            relations: Vec::new(),
            parse_audits: Vec::new(),
            execution_records: Vec::new(),
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

        entities
    }

    pub fn get_relations(&self) -> &[RelationSummary] {
        &self.relations
    }

    pub fn get_parse_audits(&self) -> &[ParseAudit] {
        &self.parse_audits
    }

    pub fn get_execution_records(&self) -> &[ExecutionRecord] {
        &self.execution_records
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
    /// 1. **Direct**: a non-pod entity has a `k8s.can-exec` / `kubelet-pod-exec`
    ///    relation to `target_id` — the C2 reaches the target without any hop.
    ///
    /// 2. **Multi-hop BFS**: starting from every pod reachable by the C2 (via
    ///    `k8s.can-exec`, `kubelet-pod-exec` from a non-pod source, or
    ///    transitively via `rce.can-exec` from prior lateral movement), follow
    ///    all three edge types to find the shortest path to the target.  The
    ///    resulting `ExecChannel.hops` contains the full ordered list of
    ///    intermediate pods, from the C2 side outward; there is no depth limit.
    ///
    /// 3. **Service-account indirection**: when the target is an SA, find the
    ///    pod that `uses` it and resolve for that pod instead.
    ///
    /// Returns `Err` when no path can be found.
    pub fn resolve_exec_channel(&self, target_id: &str) -> Result<ExecChannel, String> {
        // Priority 1: direct — a non-pod entity has an exec relation to the
        // target (e.g. the C2's own SA has kubectl exec rights).
        let direct = self.relations.iter().any(|r| {
            r.is_exec_channel
                && r.target_id == target_id
                && !self.pods.contains_key(&EntityId::new(&r.source_id))
        });
        if direct {
            return Ok(ExecChannel::direct("c2/ran"));
        }

        // Priority 2: BFS through exec relations from reachable pods.
        //
        // `visited` maps a pod entity ID to the complete ordered hop path that
        // leads the C2 to it (including the pod itself as the last element).
        //
        // Seed: every pod reachable from the C2 (direct k8s.can-exec/
        // kubelet-pod-exec from non-pod source, or transitively via
        // rce.can-exec from prior lateral movement).
        let mut visited: HashMap<String, Vec<String>> = HashMap::new();
        let mut queue: VecDeque<String> = VecDeque::new();

        // Seed only from pods the C2 can reach in one hop (exec-channel from
        // a non-pod source).  Transitively reachable pods must NOT be pre-seeded
        // here — their paths must be discovered by the BFS so that `hops` is
        // built correctly from the C2 side outward.
        for r in &self.relations {
            if r.is_exec_channel
                && !self.pods.contains_key(&EntityId::new(&r.source_id))
                && self.pods.contains_key(&EntityId::new(&r.target_id))
                && !visited.contains_key(&r.target_id)
            {
                visited.insert(r.target_id.clone(), vec![r.target_id.clone()]);
                queue.push_back(r.target_id.clone());
            }
        }

        while let Some(current) = queue.pop_front() {
            let path_to_current = visited[&current].clone();

            for r in &self.relations {
                if !r.is_exec_channel || r.source_id != current {
                    continue;
                }
                let next = &r.target_id;

                // Found the target — return the hops leading to `current`;
                // the caller's wrapping logic will add the final kubectl exec
                // into `next` (the exec target).
                if next == target_id {
                    return Ok(ExecChannel {
                        backend_id: "c2/ran".to_string(),
                        hops: path_to_current,
                        exec_target_id: None,
                    });
                }

                // Continue BFS only through unvisited pod entities.
                let next_id = next.clone();
                if self.pods.contains_key(&EntityId::new(&next_id))
                    && !visited.contains_key(&next_id)
                {
                    let mut next_path = path_to_current.clone();
                    next_path.push(next_id.clone());
                    visited.insert(next_id.clone(), next_path);
                    queue.push_back(next_id);
                }
            }
        }

        // Priority 3: target is a service account — find the pod `uses`-ing it
        // and resolve the exec channel for that pod instead.
        let sa_pod_id = self
            .relations
            .iter()
            .find(|r| r.name == "uses" && r.target_id == target_id)
            .map(|r| r.source_id.clone());
        if let Some(pod_id) = sa_pod_id {
            return self.resolve_exec_channel(&pod_id).map(|mut ch| {
                ch.exec_target_id = Some(pod_id);
                ch
            });
        }

        Err(format!(
            "no viable execution channel to '{}' found in the knowledge graph \
             (no k8s.can-exec or kubelet-pod-exec path reaches this target)",
            target_id
        ))
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
    /// 1. **Most recently used reachable pod** — the last execution record whose
    ///    `target_id` falls within the reachable set.  Keeps lateral movement in
    ///    the same foothold the operator was just working in.
    ///
    /// 2. **Highest `access_level` among reachable pods** — prefers pods where
    ///    we have proven interactive access (`UserExec` / `RootExec`).
    ///
    /// 3. **Any reachable pod** — fallback when no access-level information is
    ///    available yet (e.g. initial access via `k8s.can-exec` relation only).
    ///
    /// "Reachable" = the C2 can get code running on the pod.  This includes:
    /// - Pods targeted by a `k8s.can-exec` or `kubelet-pod-exec` relation from
    ///   a **non-pod** source (the C2 itself has kubectl exec rights).
    /// - Pods transitively reachable via `rce.can-exec` from the above set
    ///   (pods compromised through prior lateral movement).
    /// Collect all pod entity IDs that the C2 can run code on, either directly
    /// or transitively through prior lateral movement.
    ///
    /// A pod is considered reachable if:
    /// - A `k8s.can-exec` or `kubelet-pod-exec` relation points to it from a
    ///   **non-pod** source (the C2 itself has kubectl exec rights), **or**
    /// - It is reachable from any of the above via a chain of `rce.can-exec`
    ///   edges left by prior lateral movement TTPs (no depth limit).
    ///
    /// The returned set contains pod entity IDs only.
    pub fn reachable_pods(&self) -> std::collections::HashSet<String> {
        // Seed: pods that C2 can directly exec into via a non-pod source.
        let mut reachable: std::collections::HashSet<String> = self
            .relations
            .iter()
            .filter(|r| {
                r.is_exec_channel
                    && !self.pods.contains_key(&EntityId::new(&r.source_id))
                    && self.pods.contains_key(&EntityId::new(&r.target_id))
            })
            .map(|r| r.target_id.clone())
            .collect();

        // BFS over exec-channel edges to include pods compromised via lateral movement.
        let mut bfs_queue: VecDeque<String> = reachable.iter().cloned().collect();
        while let Some(current) = bfs_queue.pop_front() {
            for r in &self.relations {
                if r.is_exec_channel
                    && r.source_id == current
                    && self.pods.contains_key(&EntityId::new(&r.target_id))
                    && !reachable.contains(&r.target_id)
                {
                    reachable.insert(r.target_id.clone());
                    bfs_queue.push_back(r.target_id.clone());
                }
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
    /// 1. **Most recently used reachable pod** — the last execution record whose
    ///    `target_id` falls within the reachable set.  Keeps lateral movement in
    ///    the same foothold the operator was just working in.
    ///
    /// 2. **Highest `access_level` among reachable pods** — prefers pods where
    ///    we have proven interactive access (`UserExec` / `RootExec`).
    ///
    /// 3. **Any reachable pod** — fallback when no access-level information is
    ///    available yet (e.g. initial access via `k8s.can-exec` relation only).
    ///
    /// Returns `Err` when no reachable pod can be found.
    pub fn resolve_exec_source(&self) -> Result<ExecChannel, String> {
        let reachable = self.reachable_pods();

        if reachable.is_empty() {
            return Err(
                "no compromised system available to use as a lateral-movement \
                 exec source; gain initial access first"
                    .to_string(),
            );
        }

        // --- Pick the best pod among reachable ones ---

        // Priority 1: most recently used reachable pod.
        if let Some(pod_id) = self
            .execution_records
            .iter()
            .rev()
            .map(|r| &r.target_id)
            .find(|id| reachable.contains(*id))
        {
            let mut ch = ExecChannel::direct("c2/ran");
            ch.exec_target_id = Some(pod_id.clone());
            return Ok(ch);
        }

        // Priority 2: pod with highest proven access level.
        let best_access = self
            .pods
            .values()
            .filter(|p| reachable.contains(&p.entity_id().0) && p.system.can_exec())
            .max_by_key(|p| p.system.access_level as u8);

        if let Some(pod) = best_access {
            let mut ch = ExecChannel::direct("c2/ran");
            ch.exec_target_id = Some(pod.entity_id().0.clone());
            return Ok(ch);
        }

        // Priority 3: any reachable pod (initial access only, no exec confirmed yet).
        let any_pod = reachable
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
        let any = entity.as_any();
        if let Some(e) = any.downcast_ref::<Pod>() {
            self.pods.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<Namespace>() {
            self.namespaces.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<K8sCluster>() {
            self.clusters.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<K8sNode>() {
            self.nodes.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<C2Server>() {
            self.c2_servers.insert(e.entity_id(), e.clone());
        } else if let Some(e) = any.downcast_ref::<ServiceAccount>() {
            self.service_accounts.insert(e.entity_id(), e.clone());
        }
    }
}
