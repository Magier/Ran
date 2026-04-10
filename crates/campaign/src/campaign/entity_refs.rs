use ran_domain::{
    C2Server, ConfigMap, Deployment, Entity, EntityId, K8sCluster, K8sNode, K8sSecret, Namespace,
    Pod, ServiceAccount, SystemEntity,
};

pub enum CampaignEntityRef<'a> {
    C2Server(&'a C2Server),
    Cluster(&'a K8sCluster),
    Node(&'a K8sNode),
    Namespace(&'a Namespace),
    Pod(&'a Pod),
    ServiceAccount(&'a ServiceAccount),
    Secret(&'a K8sSecret),
    ConfigMap(&'a ConfigMap),
    Deployment(&'a Deployment),
}

pub enum CampaignSystemEntityRef<'a> {
    Node(&'a K8sNode),
    Pod(&'a Pod),
}

impl<'a> CampaignSystemEntityRef<'a> {
    pub fn entity(&self) -> &'a dyn SystemEntity {
        match self {
            CampaignSystemEntityRef::Node(e) => *e,
            CampaignSystemEntityRef::Pod(e) => *e,
        }
    }
}

pub enum CampaignSystemEntityMut<'a> {
    Node(&'a mut K8sNode),
    Pod(&'a mut Pod),
}

impl<'a> CampaignSystemEntityMut<'a> {
    pub fn entity_mut(&mut self) -> &mut dyn SystemEntity {
        match self {
            CampaignSystemEntityMut::Node(e) => *e,
            CampaignSystemEntityMut::Pod(e) => *e,
        }
    }
}

impl<'a> CampaignEntityRef<'a> {
    pub fn entity_id(&self) -> EntityId {
        match self {
            CampaignEntityRef::C2Server(e) => e.entity_id(),
            CampaignEntityRef::Cluster(e) => e.entity_id(),
            CampaignEntityRef::Node(e) => e.entity_id(),
            CampaignEntityRef::Namespace(e) => e.entity_id(),
            CampaignEntityRef::Pod(e) => e.entity_id(),
            CampaignEntityRef::ServiceAccount(e) => e.entity_id(),
            CampaignEntityRef::Secret(e) => e.entity_id(),
            CampaignEntityRef::ConfigMap(e) => e.entity_id(),
            CampaignEntityRef::Deployment(e) => e.entity_id(),
        }
    }

    pub fn entity_name(&self) -> &str {
        match self {
            CampaignEntityRef::C2Server(e) => e.entity_name(),
            CampaignEntityRef::Cluster(e) => e.entity_name(),
            CampaignEntityRef::Node(e) => e.entity_name(),
            CampaignEntityRef::Namespace(e) => e.entity_name(),
            CampaignEntityRef::Pod(e) => e.entity_name(),
            CampaignEntityRef::ServiceAccount(e) => e.entity_name(),
            CampaignEntityRef::Secret(e) => e.entity_name(),
            CampaignEntityRef::ConfigMap(e) => e.entity_name(),
            CampaignEntityRef::Deployment(e) => e.entity_name(),
        }
    }

    pub fn entity_kind(&self) -> &str {
        match self {
            CampaignEntityRef::C2Server(e) => e.entity_kind(),
            CampaignEntityRef::Cluster(e) => e.entity_kind(),
            CampaignEntityRef::Node(e) => e.entity_kind(),
            CampaignEntityRef::Namespace(e) => e.entity_kind(),
            CampaignEntityRef::Pod(e) => e.entity_kind(),
            CampaignEntityRef::ServiceAccount(e) => e.entity_kind(),
            CampaignEntityRef::Secret(e) => e.entity_kind(),
            CampaignEntityRef::ConfigMap(e) => e.entity_kind(),
            CampaignEntityRef::Deployment(e) => e.entity_kind(),
        }
    }

    pub fn namespace(&self) -> Option<&str> {
        match self {
            CampaignEntityRef::Pod(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::ServiceAccount(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::Secret(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::ConfigMap(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::Deployment(e) => e.meta.namespace.as_deref(),
            _ => None,
        }
    }
}
