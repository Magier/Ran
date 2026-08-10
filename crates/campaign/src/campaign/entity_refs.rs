use ran_domain::{
    C2Server, ConfigMap, CronJob, DaemonSet, Deployment, Entity, EntityId, GCPBucket,
    GCPServiceAccount, Job, K8sCluster, K8sCredential, K8sGateway, K8sHTTPRoute, K8sIngress,
    K8sNode, K8sRole, K8sRoleBinding, K8sSecret, K8sService, Namespace, Pod, ReplicaSet,
    ServiceAccount, StatefulSet, SystemEntity, UnknownSystem,
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
    Role(&'a K8sRole),
    RoleBinding(&'a K8sRoleBinding),
    CronJob(&'a CronJob),
    ReplicaSet(&'a ReplicaSet),
    StatefulSet(&'a StatefulSet),
    DaemonSet(&'a DaemonSet),
    Job(&'a Job),
    GCPServiceAccount(&'a GCPServiceAccount),
    GCPBucket(&'a GCPBucket),
    K8sCredential(&'a K8sCredential),
    UnknownSystem(&'a UnknownSystem),
    Service(&'a K8sService),
    Ingress(&'a K8sIngress),
    Gateway(&'a K8sGateway),
    HTTPRoute(&'a K8sHTTPRoute),
}

pub enum CampaignSystemEntityRef<'a> {
    Node(&'a K8sNode),
    Pod(&'a Pod),
    Unknown(&'a UnknownSystem),
}

impl<'a> CampaignSystemEntityRef<'a> {
    pub fn entity(&self) -> &'a dyn SystemEntity {
        match self {
            CampaignSystemEntityRef::Node(e) => *e,
            CampaignSystemEntityRef::Pod(e) => *e,
            CampaignSystemEntityRef::Unknown(e) => *e,
        }
    }
}

pub enum CampaignSystemEntityMut<'a> {
    Node(&'a mut K8sNode),
    Pod(&'a mut Pod),
    Unknown(&'a mut UnknownSystem),
}

impl<'a> CampaignSystemEntityMut<'a> {
    pub fn entity_mut(&mut self) -> &mut dyn SystemEntity {
        match self {
            CampaignSystemEntityMut::Node(e) => *e,
            CampaignSystemEntityMut::Pod(e) => *e,
            CampaignSystemEntityMut::Unknown(e) => *e,
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
        C2Server,
        Cluster,
        Node,
        Namespace,
        Pod,
        ServiceAccount,
        Secret,
        ConfigMap,
        Deployment,
        Role,
        RoleBinding,
        CronJob,
        ReplicaSet,
        StatefulSet,
        DaemonSet,
        Job,
        GCPServiceAccount,
        GCPBucket,
        K8sCredential,
        UnknownSystem,
        Service,
        Ingress,
        Gateway,
        HTTPRoute,
    );

    pub fn namespace(&self) -> Option<&str> {
        match self {
            // A Namespace is its own namespace context. This matters when an
            // action targets the namespace node itself (for example, listing
            // pods in that namespace).
            CampaignEntityRef::Namespace(e) => Some(&e.name),
            CampaignEntityRef::Pod(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::ServiceAccount(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::Secret(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::ConfigMap(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::Deployment(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::Role(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::RoleBinding(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::CronJob(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::ReplicaSet(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::StatefulSet(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::DaemonSet(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::Job(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::Service(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::Ingress(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::Gateway(e) => e.meta.namespace.as_deref(),
            CampaignEntityRef::HTTPRoute(e) => e.meta.namespace.as_deref(),
            _ => None,
        }
    }
}
