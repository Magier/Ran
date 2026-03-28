use std::collections::HashMap;

use ran_domain::{C2Server, Entity, EntityId, K8sCluster, Namespace, Pod, ServiceAccount};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignRelation {
    pub id: String,
    pub name: String,
    pub source_id: String,
    pub target_id: String,
}

pub enum CampaignEntityRef<'a> {
    C2Server(&'a C2Server),
    Cluster(&'a K8sCluster),
    Namespace(&'a Namespace),
    Pod(&'a Pod),
    ServiceAccount(&'a ServiceAccount),
}

impl<'a> CampaignEntityRef<'a> {
    pub fn entity_id(&self) -> EntityId {
        match self {
            CampaignEntityRef::C2Server(e) => e.entity_id(),
            CampaignEntityRef::Cluster(e) => e.entity_id(),
            CampaignEntityRef::Namespace(e) => e.entity_id(),
            CampaignEntityRef::Pod(e) => e.entity_id(),
            CampaignEntityRef::ServiceAccount(e) => e.entity_id(),
        }
    }

    pub fn entity_name(&self) -> &str {
        match self {
            CampaignEntityRef::C2Server(e) => e.entity_name(),
            CampaignEntityRef::Cluster(e) => e.entity_name(),
            CampaignEntityRef::Namespace(e) => e.entity_name(),
            CampaignEntityRef::Pod(e) => e.entity_name(),
            CampaignEntityRef::ServiceAccount(e) => e.entity_name(),
        }
    }

    pub fn entity_kind(&self) -> &str {
        match self {
            CampaignEntityRef::C2Server(e) => e.entity_kind(),
            CampaignEntityRef::Cluster(e) => e.entity_kind(),
            CampaignEntityRef::Namespace(e) => e.entity_kind(),
            CampaignEntityRef::Pod(e) => e.entity_kind(),
            CampaignEntityRef::ServiceAccount(e) => e.entity_kind(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        match self {
            CampaignEntityRef::Pod(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::ServiceAccount(e) => e.meta.namespace.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub c2_servers: HashMap<EntityId, C2Server>,
    pub clusters: HashMap<EntityId, K8sCluster>,
    pub namespaces: HashMap<EntityId, Namespace>,
    pub pods: HashMap<EntityId, Pod>,
    pub service_accounts: HashMap<EntityId, ServiceAccount>,
    pub relations: Vec<CampaignRelation>,
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
            namespaces: HashMap::new(),
            pods: HashMap::new(),
            service_accounts: HashMap::new(),
            relations: Vec::new(),
        }
    }

    pub fn entity_count(&self) -> usize {
        self.c2_servers.len()
            + self.clusters.len()
            + self.namespaces.len()
            + self.pods.len()
            + self.service_accounts.len()
    }

    pub fn get_entities(&self) -> Vec<CampaignEntityRef<'_>> {
        let mut entities = Vec::with_capacity(self.entity_count());

        entities.extend(self.c2_servers.values().map(CampaignEntityRef::C2Server));
        entities.extend(self.clusters.values().map(CampaignEntityRef::Cluster));
        entities.extend(self.namespaces.values().map(CampaignEntityRef::Namespace));
        entities.extend(self.pods.values().map(CampaignEntityRef::Pod));
        entities.extend(
            self.service_accounts
                .values()
                .map(CampaignEntityRef::ServiceAccount),
        );

        entities
    }

    pub fn get_relations(&self) -> &[CampaignRelation] {
        &self.relations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_contains_c2_and_cluster_entities() {
        let campaign = Campaign::bootstrap(
            "Ran",
            K8sCluster::new("dev-cluster")
                .with_context_name(Some("dev-context".to_string()))
                .with_server(Some("https://127.0.0.1:6443".to_string())),
        );

        assert_eq!(campaign.entity_count(), 2);
        assert!(campaign.c2_servers.contains_key(&EntityId::new("c2/ran")));
        assert!(campaign
            .clusters
            .contains_key(&EntityId::new("k8s/cluster/dev-cluster")));
    }
}
