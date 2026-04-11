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

// `Entity: std::any::Any` implies `'static`, so `&'a T` variants cannot
// implement `Entity` and ambassador's #[delegate] cannot apply here.
// A declarative macro gives the same single-definition-site property:
// adding a new entity variant only requires updating the enum and the
// variant list in the macro invocation below.
macro_rules! delegate_entity_methods {
    ($($variant:ident),+ $(,)?) => {
        pub fn entity_id(&self) -> EntityId {
            match self { $(Self::$variant(e) => e.entity_id(),)+ }
        }
        pub fn entity_name(&self) -> &str {
            match self { $(Self::$variant(e) => e.entity_name(),)+ }
        }
        pub fn entity_kind(&self) -> &str {
            match self { $(Self::$variant(e) => e.entity_kind(),)+ }
        }
    };
}

impl<'a> CampaignEntityRef<'a> {
    delegate_entity_methods!(
        C2Server, Cluster, Node, Namespace, Pod, ServiceAccount, Secret, ConfigMap, Deployment,
    );

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
