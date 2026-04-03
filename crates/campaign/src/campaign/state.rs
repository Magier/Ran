use std::collections::HashMap;

use ran_domain::{
    C2Server, Entity, EntityId, K8sCluster, K8sNode, Namespace, Pod, RelationSummary,
    ServiceAccount,
};
use serde::{Deserialize, Serialize};

use crate::external_parser::SystemFieldUpdates;
use crate::{external_parser, ParseAudit};

use super::{CampaignEntityRef, CampaignSystemEntityMut, CampaignSystemEntityRef};

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
        }
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
