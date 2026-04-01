use ran_domain::{
    Contains, Entity, EntityId, K8sNode, KubeletExecSink, Pod, PodExec, RelationSummary, RunsOn,
    ServiceAccount,
};

use ran_domain::Namespace;

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

/// For every running `Pod` with a known `node_name`, ensure the node entity
/// exists and infer a `runs-on` relation (Pod -> Node).
pub struct PodNodeAnalyzer;

impl Analyzer for PodNodeAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(pod) = entity.as_any().downcast_ref::<Pod>() else {
                continue;
            };

            if !pod.is_running {
                continue;
            }

            let Some(node_name) = pod.node_name.as_deref() else {
                continue;
            };
            if node_name.trim().is_empty() {
                continue;
            }

            let node = K8sNode::new(node_name);
            let node_id = node.entity_id();
            let node_exists = campaign.nodes.contains_key(&node_id)
                || update.new_entities.iter().any(|e| {
                    e.as_any()
                        .downcast_ref::<K8sNode>()
                        .map(|n| n.entity_id() == node_id)
                        .unwrap_or(false)
                });

            if !node_exists {
                inferred.new_entities.push(Box::new(node));
            }
            inferred.new_relations.push(Box::new(RunsOn::new(
                pod.entity_id().0.clone(),
                node_id.0,
            )));
        }

        inferred
    }
}

/// Infer `k8s.can-exec` when an SA has create pods/exec permission in scope
/// and the target pod is running.
pub struct ServiceAccountCanExecAnalyzer;

impl Analyzer for ServiceAccountCanExecAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        let service_accounts = collect_service_accounts(campaign, update);
        let pods = collect_pods(campaign, update);

        for sa in service_accounts {
            for pod in &pods {
                if !pod.is_running {
                    continue;
                }

                let Some(ns) = pod.namespace() else {
                    continue;
                };

                let can_exec = sa.entitlements.iter().any(|perm| {
                    perm.satisfies("create", "pods/exec") && perm.is_in_scope(ns)
                });

                if can_exec {
                    inferred.new_relations.push(Box::new(PodExec::new(
                        sa.entity_id().0.clone(),
                        pod.entity_id().0.clone(),
                    )));
                }
            }
        }

        inferred
    }
}

/// Infer `kubelet-pod-exec` (Node -> Pod) from existing `kubelet-exec`
/// (source pod -> node) and `runs-on` (pod -> node) relations.
pub struct KubeletExecSinkAnalyzer;

impl Analyzer for KubeletExecSinkAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        let pods = collect_pods(campaign, update)
            .into_iter()
            .map(|p| (p.entity_id().0.clone(), p))
            .collect::<std::collections::HashMap<_, _>>();

        let relations = collect_relation_summaries(campaign, update);
        let runs_on = relations
            .iter()
            .filter(|r| r.name == "runs-on")
            .collect::<Vec<_>>();
        let kubelet_sources = relations
            .iter()
            .filter(|r| r.name == "kubelet-exec")
            .collect::<Vec<_>>();

        for source_rel in kubelet_sources {
            let source_pod_id = &source_rel.source_id;
            let node_id = &source_rel.target_id;

            for runs_on_rel in &runs_on {
                if &runs_on_rel.target_id != node_id {
                    continue;
                }

                let target_pod_id = &runs_on_rel.source_id;
                if target_pod_id == source_pod_id {
                    continue;
                }

                let Some(target_pod) = pods.get(target_pod_id) else {
                    continue;
                };
                if !target_pod.is_running {
                    continue;
                }

                inferred
                    .new_relations
                    .push(Box::new(KubeletExecSink::new(node_id.clone(), target_pod_id.clone())));
            }
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
        Box::new(PodNodeAnalyzer),
        Box::new(ServiceAccountCanExecAnalyzer),
        Box::new(KubeletExecSinkAnalyzer),
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

fn collect_pods(campaign: &Campaign, update: &FactsUpdate) -> Vec<Pod> {
    let mut pods = campaign.pods.values().cloned().collect::<Vec<_>>();
    for entity in &update.new_entities {
        if let Some(pod) = entity.as_any().downcast_ref::<Pod>() {
            if let Some(existing) = pods.iter_mut().find(|p| p.entity_id() == pod.entity_id()) {
                *existing = pod.clone();
            } else {
                pods.push(pod.clone());
            }
        }
    }
    pods
}

fn collect_service_accounts(campaign: &Campaign, update: &FactsUpdate) -> Vec<ServiceAccount> {
    let mut sas = campaign.service_accounts.values().cloned().collect::<Vec<_>>();
    for entity in &update.new_entities {
        if let Some(sa) = entity.as_any().downcast_ref::<ServiceAccount>() {
            if let Some(existing) = sas.iter_mut().find(|s| s.entity_id() == sa.entity_id()) {
                *existing = sa.clone();
            } else {
                sas.push(sa.clone());
            }
        }
    }
    sas
}

fn collect_relation_summaries(campaign: &Campaign, update: &FactsUpdate) -> Vec<RelationSummary> {
    let mut rels = campaign.relations.clone();
    for rel in &update.new_relations {
        let summary = RelationSummary::from_relation(rel.as_ref());
        let exists = rels.iter().any(|r| {
            r.name == summary.name
                && r.source_id == summary.source_id
                && r.target_id == summary.target_id
        });
        if !exists {
            rels.push(summary);
        }
    }
    rels
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ran_domain::{
        K8sCluster, KubeletExecSource, Namespace, Pod, RbacPermission, RunsOn, ServiceAccount,
    };

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
            nodes: Default::default(),
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

    #[test]
    fn running_pod_with_node_infers_runs_on_and_node_entity() {
        let campaign = test_campaign();

        let mut pod = Pod::new("api", "default");
        pod.is_running = true;
        pod.node_name = Some("worker-1".to_string());

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(update
            .new_entities
            .iter()
            .any(|e| e.entity_kind() == "Node" && e.entity_name() == "worker-1"));
        assert!(update.new_relations.iter().any(|r| {
            r.relation_name() == "runs-on"
                && r.source_id().0 == pod.entity_id().0
                && r.target_id().0 == "node/worker-1"
        }));
    }

    #[test]
    fn service_account_with_pod_exec_permission_inferrs_can_exec() {
        let mut campaign = test_campaign();

        let mut pod = Pod::new("target", "default");
        pod.is_running = true;
        campaign.pods.insert(pod.entity_id(), pod.clone());

        let mut sa = ServiceAccount::new("operator", "default");
        let mut perm = RbacPermission::new("create", "pods/exec");
        perm.scope = Some("default".to_string());
        sa.entitlements.push(perm);

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(sa.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(update.new_relations.iter().any(|r| {
            r.relation_name() == "k8s.can-exec"
                && r.source_id().0 == sa.entity_id().0
                && r.target_id().0 == pod.entity_id().0
        }));
    }

    #[test]
    fn kubelet_source_and_runs_on_infer_kubelet_sink_for_other_running_pods() {
        let mut campaign = test_campaign();

        let mut src = Pod::new("src", "default");
        src.is_running = true;
        let src_id = src.entity_id().0.clone();
        campaign.pods.insert(src.entity_id(), src.clone());

        let mut target = Pod::new("target", "default");
        target.is_running = true;
        let target_id = target.entity_id().0.clone();
        campaign.pods.insert(target.entity_id(), target.clone());

        campaign
            .relations
            .push(ran_domain::RelationSummary::from_relation(&RunsOn::new(
                src_id.clone(),
                "node/worker-1",
            )));
        campaign
            .relations
            .push(ran_domain::RelationSummary::from_relation(&RunsOn::new(
                target_id.clone(),
                "node/worker-1",
            )));
        campaign
            .relations
            .push(ran_domain::RelationSummary::from_relation(&KubeletExecSource::new(
                src_id.clone(),
                "node/worker-1",
            )));

        let mut update = FactsUpdate::default();
        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(update.new_relations.iter().any(|r| {
            r.relation_name() == "kubelet-pod-exec"
                && r.source_id().0 == "node/worker-1"
                && r.target_id().0 == target_id
        }));
        assert!(!update.new_relations.iter().any(|r| {
            r.relation_name() == "kubelet-pod-exec"
                && r.source_id().0 == "node/worker-1"
                && r.target_id().0 == src_id
        }));
    }
}
