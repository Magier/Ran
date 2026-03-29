use ran_domain::{Contains, Entity, EntityId, Namespace, Pod, ServiceAccount};

use crate::{Campaign, FactsUpdate};

// ---------------------------------------------------------------------------
// Analyzer trait
// ---------------------------------------------------------------------------

/// An `Analyzer` inspects newly-parsed entities and infers additional facts
/// from existing campaign state.  Analyzers are cheap to construct and purely
/// functional: they receive a read-only view of the campaign and the pending
/// update, and return a supplementary `FactsUpdate` to be merged in.
pub trait Analyzer: Send + Sync {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate;
}

// ---------------------------------------------------------------------------
// Built-in analyzers
// ---------------------------------------------------------------------------

/// For every new `Pod`, ensure the namespace entity exists and wire a
/// `contains` relation from the Namespace to the Pod.
pub struct PodNamespaceAnalyzer;

impl Analyzer for PodNamespaceAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(pod) = entity.as_any().downcast_ref::<Pod>() else {
                continue;
            };
            let Some(ns_name) = pod.namespace() else {
                continue;
            };
            if ns_name.is_empty() {
                continue;
            }

            let ns_id = EntityId::new(format!("ns/{}", ns_name));

            // Resolve namespace: prefer what is already in the campaign, then
            // check whether a namespace was freshly added in this same update,
            // and finally fall back to creating a minimal one.
            let ns = campaign
                .namespaces
                .get(&ns_id)
                .cloned()
                .or_else(|| {
                    update.new_entities.iter().find_map(|e| {
                        e.as_any()
                            .downcast_ref::<Namespace>()
                            .filter(|n| n.entity_id() == ns_id)
                            .cloned()
                    })
                })
                .unwrap_or_else(|| Namespace::new(ns_name));

            let rel = Contains::new(ns_id.0.clone(), pod.entity_id().0.clone());

            // Only emit the namespace entity if it was not already known.
            if !campaign.namespaces.contains_key(&ns_id) {
                inferred.new_entities.push(Box::new(ns));
            }
            inferred.new_relations.push(Box::new(rel));
        }

        inferred
    }
}

/// For every new `ServiceAccount`, ensure its namespace entity exists and wire
/// a `contains` relation from the Namespace to the ServiceAccount.
pub struct ServiceAccountNamespaceAnalyzer;

impl Analyzer for ServiceAccountNamespaceAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(sa) = entity.as_any().downcast_ref::<ServiceAccount>() else {
                continue;
            };
            let Some(ns_name) = sa.namespace() else {
                continue;
            };
            if ns_name.is_empty() {
                continue;
            }

            let ns_id = EntityId::new(format!("ns/{}", ns_name));

            let ns = campaign
                .namespaces
                .get(&ns_id)
                .cloned()
                .or_else(|| {
                    update.new_entities.iter().find_map(|e| {
                        e.as_any()
                            .downcast_ref::<Namespace>()
                            .filter(|n| n.entity_id() == ns_id)
                            .cloned()
                    })
                })
                .unwrap_or_else(|| Namespace::new(ns_name));

            let rel = Contains::new(ns_id.0.clone(), sa.entity_id().0.clone());

            if !campaign.namespaces.contains_key(&ns_id) {
                inferred.new_entities.push(Box::new(ns));
            }
            inferred.new_relations.push(Box::new(rel));
        }

        inferred
    }
}

/// For every new `Namespace`, wire a `contains` relation from the single known
/// cluster to that namespace.  If no cluster is known yet the relation is
/// silently skipped (the namespace will be re-linked once the cluster is
/// discovered).
pub struct NamespaceClusterAnalyzer;

impl Analyzer for NamespaceClusterAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        // Only one cluster is currently supported; bail out if none is known.
        let Some(cluster) = campaign.clusters.values().next() else {
            return inferred;
        };
        let cluster_id = cluster.entity_id();

        for entity in &update.new_entities {
            let Some(ns) = entity.as_any().downcast_ref::<Namespace>() else {
                continue;
            };

            inferred
                .new_relations
                .push(Box::new(Contains::new(cluster_id.0.clone(), ns.entity_id().0.clone())));
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// Default analyzer pipeline
// ---------------------------------------------------------------------------

/// Returns the default set of analyzers that run after every effect parse.
pub fn default_analyzers() -> Vec<Box<dyn Analyzer>> {
    vec![
        Box::new(NamespaceClusterAnalyzer),
        Box::new(PodNamespaceAnalyzer),
        Box::new(ServiceAccountNamespaceAnalyzer),
    ]
}

/// Run every analyzer against the current campaign state and accumulate their
/// inferred updates into `base`.  Analyzers run against the *original* state
/// so that their individual outputs combine additively without order-dependency.
pub fn run_analyzers(campaign: &Campaign, analyzers: &[Box<dyn Analyzer>], base: &mut FactsUpdate) {
    for analyzer in analyzers {
        let inferred = analyzer.analyze(campaign, base);
        base.merge(inferred);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ran_domain::{K8sCluster, Namespace, Pod};

    use super::*;
    use crate::Campaign;

    fn test_campaign() -> Campaign {
        Campaign::bootstrap("ran", K8sCluster::new("test-cluster"))
    }

    #[test]
    fn pod_in_known_namespace_creates_contains_relation() {
        let mut campaign = test_campaign();
        let ns = Namespace::new("default");
        campaign.namespaces.insert(ns.entity_id(), ns);

        let pod = Pod::new("nginx", "default");
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        let rel = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "contains" && r.target_id().0 == pod.entity_id().0);
        assert!(rel.is_some(), "expected contains relation for pod");
        // namespace was already known – should not be duplicated in new_entities
        assert!(
            update.new_entities.iter().all(|e| e.entity_kind() != "Namespace"),
            "should not emit namespace when it is already in campaign"
        );
    }

    #[test]
    fn pod_in_unknown_namespace_creates_namespace_and_relation() {
        let campaign = test_campaign();

        let pod = Pod::new("attacker-pod", "kube-system");
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        let ns_entity = update
            .new_entities
            .iter()
            .find(|e| e.entity_kind() == "Namespace" && e.entity_name() == "kube-system");
        assert!(ns_entity.is_some(), "expected a new Namespace entity");

        let rel = update
            .new_relations
            .iter()
            .find(|r| r.relation_name() == "contains" && r.source_id().0 == "ns/kube-system");
        assert!(rel.is_some(), "expected contains relation from namespace");
    }

    #[test]
    fn pod_without_namespace_is_ignored() {
        let campaign = test_campaign();

        // Build a pod with no namespace
        let mut pod = Pod::new("bare-pod", "");
        pod.meta.namespace = None;
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(update.new_relations.is_empty());
    }

    #[test]
    fn new_namespace_gets_cluster_contains_relation() {
        let campaign = test_campaign(); // has cluster "k8s/cluster/test-cluster"

        let ns = Namespace::new("default");
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(ns.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        let rel = update.new_relations.iter().find(|r| {
            r.relation_name() == "contains"
                && r.source_id().0 == "k8s/cluster/test-cluster"
                && r.target_id().0 == ns.entity_id().0
        });
        assert!(rel.is_some(), "expected cluster→namespace contains relation");
    }

    #[test]
    fn namespace_without_cluster_produces_no_relation() {
        // A campaign with no cluster at all.
        let campaign = Campaign {
            c2_servers: Default::default(),
            clusters: Default::default(),
            namespaces: Default::default(),
            pods: Default::default(),
            service_accounts: Default::default(),
            relations: Default::default(),
        };

        let ns = Namespace::new("default");
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(ns));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(update.new_relations.is_empty());
    }
}
