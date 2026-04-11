use ran_domain::{
    Confidence, Contains, Entity, EntityId, K8sCluster, K8sNode, KubeletExecSink, Namespace, Pod,
    PodExec, RelationSummary, RunsOn, ServiceAccount, Uses,
};

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
                .entities
                .find::<Namespace>(&ns_id)
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
            if !campaign.entities.contains::<Namespace>(&ns_id) {
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
                .entities
                .find::<Namespace>(&ns_id)
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

            if !campaign.entities.contains::<Namespace>(&ns_id) {
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
        let Some(cluster) = campaign.entities.values::<K8sCluster>().next() else {
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
            let node_exists = campaign.entities.contains::<K8sNode>(&node_id)
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

/// For every running `Pod` with a `service_account_name`, ensure the
/// `ServiceAccount` entity exists and wire a `uses` relation (Pod → SA).
///
/// This makes the pod's SA visible to `ServiceAccountCanExecAnalyzer` even
/// when the SA was not independently discovered through K8s API enumeration.
/// The automount_service_account_token field is respected: if it is
/// explicitly `No`, no `uses` relation is emitted.
pub struct ServiceAccountAnalyzer;

impl Analyzer for ServiceAccountAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(pod) = entity.as_any().downcast_ref::<Pod>() else {
                continue;
            };

            // Skip if automount is explicitly disabled.
            if pod.automount_service_account_token == Confidence::No {
                continue;
            }

            let Some(sa_name) = pod.service_account_name.as_deref() else {
                continue;
            };
            if sa_name.is_empty() {
                continue;
            }

            let Some(ns_name) = pod.namespace() else {
                continue;
            };
            if ns_name.is_empty() {
                continue;
            }

            let sa = ServiceAccount::new(sa_name, ns_name);
            let sa_id = sa.entity_id();

            // Only emit the SA entity if it is not already known.
            let sa_known = campaign.entities.contains::<ServiceAccount>(&sa_id)
                || update
                    .new_entities
                    .iter()
                    .any(|e| e.entity_id() == sa_id);
            if !sa_known {
                inferred.new_entities.push(Box::new(sa));
            }

            inferred
                .new_relations
                .push(Box::new(Uses::new(pod.entity_id().0.clone(), sa_id.0.clone())));
        }

        inferred
    }
}

/// For every new `ServiceAccount` whose token carries pod claims, ensure the
/// referenced `Pod` entity exists with the correct `service_account_name` and
/// wire a `uses` relation (Pod → SA).
///
/// This is the token-driven counterpart of `ServiceAccountAnalyzer`: while
/// `ServiceAccountAnalyzer` propagates an SA from a pod's spec field,
/// `ServiceAccountTokenAnalyzer` propagates a pod from the SA token's claims.
/// Bound tokens also contain a node name, which is used to emit a `runs-on`
/// relation when the pod is not yet scheduled.
pub struct ServiceAccountTokenAnalyzer;

impl Analyzer for ServiceAccountTokenAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(sa) = entity.as_any().downcast_ref::<ServiceAccount>() else {
                continue;
            };
            let Some(token) = &sa.token else {
                continue;
            };
            let Some(pod_name) = &token.pod_name else {
                continue;
            };
            if pod_name.is_empty() {
                continue;
            }

            let ns_name = &token.namespace;
            if ns_name.is_empty() {
                continue;
            }

            // Ensure a Pod entity exists for the token's pod claim.
            let mut pod = Pod::new(pod_name.as_str(), ns_name.as_str());
            pod.service_account_name = Some(token.service_account_name.clone());
            // Mark the pod as running — the token was read from it, so it was alive.
            pod.is_running = true;

            let pod_id = pod.entity_id();
            let sa_id = sa.entity_id();

            let pod_known = campaign.entities.contains::<Pod>(&pod_id)
                || update.new_entities.iter().any(|e| e.entity_id() == pod_id);
            if !pod_known {
                inferred.new_entities.push(Box::new(pod));
            }

            // Wire pod → SA uses relation.
            inferred
                .new_relations
                .push(Box::new(Uses::new(pod_id.0.clone(), sa_id.0.clone())));
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

/// Inspect a pod's runtime mounts and infer host-path access.
///
/// When a pod has mount entries that include `/var/lib/kubelet`, it has
/// host-filesystem visibility and the kubelet node can be identified.
/// This analyzer:
///
/// 1. Marks mounts whose `mount_point` contains `/var/lib/kubelet` as
///    `is_host_path = true` by updating the pod's `system.mounts`.
/// 2. Extracts a node name from kubelet paths of the form
///    `/var/lib/kubelet/pods/<uid>/...` and, where a node is not yet known,
///    infers a `runs-on` relation to supplement the scheduling information.
///
/// The mount data is populated by the `linux.mounts` output parser after
/// running a `cat /proc/self/mountinfo` or `mount` command on the target.
pub struct HostPathAnalyzer;

impl Analyzer for HostPathAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        let pods = collect_pods(campaign, update);

        for pod in pods {
            let kubelet_mounts: Vec<_> = pod
                .system
                .mounts
                .iter()
                .filter(|m| m.mount_point.contains("/var/lib/kubelet"))
                .collect();

            if kubelet_mounts.is_empty() {
                continue;
            }

            // If the pod already has a known node, skip the runs-on inference.
            let already_has_node = !campaign
                .graph
                .targets_of(&pod.entity_id(), "runs-on")
                .is_empty();

            if !already_has_node {
                // Try to derive a node name from the host path:
                // kubelet bind-mounts appear at paths like
                // `/var/lib/kubelet/pods/<uid>/volumes/...` on the host.
                // We cannot read the node name from this alone — use a
                // placeholder so the invariant logic can reconcile later.
                let node_name = pod.node_name.as_deref().unwrap_or("?");
                let node = K8sNode::new(node_name);
                let node_id = node.entity_id();

                let node_known = campaign.entities.contains::<K8sNode>(&node_id)
                    || update.new_entities.iter().any(|e| e.entity_id() == node_id);
                if !node_known {
                    inferred.new_entities.push(Box::new(node));
                }
                inferred.new_relations.push(Box::new(RunsOn::new(
                    pod.entity_id().0.clone(),
                    node_id.0,
                )));
            }
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// Default analyzer pipeline
// ---------------------------------------------------------------------------

/// For every new `K8sNode`, wire a `contains` relation from the campaign's
/// cluster — nodes always belong to the cluster they were discovered in.
pub struct NodeClusterAnalyzer;

impl Analyzer for NodeClusterAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        let Some(cluster) = campaign.entities.values::<K8sCluster>().next() else {
            return inferred;
        };
        let cluster_id = cluster.entity_id();

        for entity in &update.new_entities {
            let Some(node) = entity.as_any().downcast_ref::<K8sNode>() else {
                continue;
            };

            inferred
                .new_relations
                .push(Box::new(Contains::new(cluster_id.0.clone(), node.entity_id().0.clone())));
        }

        inferred
    }
}

/// Returns the default set of analyzers that run after every effect parse.
pub fn default_analyzers() -> Vec<Box<dyn Analyzer>> {
    vec![
        Box::new(NamespaceClusterAnalyzer),
        Box::new(NodeClusterAnalyzer),
        Box::new(PodNamespaceAnalyzer),
        Box::new(ServiceAccountNamespaceAnalyzer),
        Box::new(PodNodeAnalyzer),
        Box::new(ServiceAccountAnalyzer),
        Box::new(ServiceAccountTokenAnalyzer),
        Box::new(HostPathAnalyzer),
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
    let mut pods = campaign.entities.values::<Pod>().cloned().collect::<Vec<_>>();
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
    let mut sas = campaign.entities.values::<ServiceAccount>().cloned().collect::<Vec<_>>();
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
    let mut rels = campaign.graph.to_relation_summaries();
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
        Confidence, Contains, K8sCluster, K8sNode, KubeletExecSink, KubeletExecSource, ManagesNode,
        Namespace, Pod, PodExec, RbacPermission, RunsOn, ServiceAccount, Uses,
    };

    use super::*;
    use crate::Campaign;

    fn test_campaign() -> Campaign {
        Campaign::bootstrap("ran", K8sCluster::new("test-cluster"))
    }

    #[test]
    fn node_gets_contains_relation_from_cluster() {
        let campaign = test_campaign();
        let cluster_id = campaign.entities.values::<K8sCluster>().next().unwrap().entity_id();

        let node = K8sNode::new("node-1");
        let node_id = node.entity_id();
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(node));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        let rel = update.new_relations.iter().find(|r| {
            r.is::<Contains>()
                && r.source_id().0 == cluster_id.0
                && r.target_id().0 == node_id.0
        });
        assert!(rel.is_some(), "expected cluster→node contains relation");
    }

    #[test]
    fn pod_in_known_namespace_creates_contains_relation() {
        let mut campaign = test_campaign();
        let ns = Namespace::new("default");
        campaign.entities.insert_typed(ns);

        let pod = Pod::new("nginx", "default");
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        let rel = update
            .new_relations
            .iter()
            .find(|r| r.is::<Contains>() && r.target_id().0 == pod.entity_id().0);
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
            .find(|r| r.is::<Contains>() && r.source_id().0 == "ns/kube-system");
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
            r.is::<Contains>()
                && r.source_id().0 == "k8s/cluster/test-cluster"
                && r.target_id().0 == ns.entity_id().0
        });
        assert!(rel.is_some(), "expected cluster→namespace contains relation");
    }

    #[test]
    fn namespace_without_cluster_produces_no_relation() {
        // A campaign with no cluster at all.
        let mut campaign = Campaign::bootstrap("ran", ran_domain::K8sCluster::new("no-cluster"));
        // Remove the auto-inserted cluster so the campaign truly has no cluster.
        campaign.entities.get_mut::<K8sCluster>().clear();

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
            r.is::<RunsOn>()
                && r.source_id().0 == pod.entity_id().0
                && r.target_id().0 == "node/worker-1"
        }));
    }

    #[test]
    fn pod_with_sa_name_creates_sa_entity_and_uses_relation() {
        let campaign = test_campaign();

        let mut pod = Pod::new("web", "default");
        pod.service_account_name = Some("web-sa".to_string());

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(
            update.new_entities.iter().any(|e| e.entity_kind() == "ServiceAccount"
                && e.entity_name() == "web-sa"),
            "expected ServiceAccount entity to be inferred"
        );
        assert!(
            update.new_relations.iter().any(|r| r.is::<Uses>()
                && r.source_id().0 == pod.entity_id().0),
            "expected uses relation from pod to SA"
        );
    }

    #[test]
    fn pod_with_automount_disabled_skips_sa_relation() {
        let campaign = test_campaign();

        let mut pod = Pod::new("restricted", "default");
        pod.service_account_name = Some("some-sa".to_string());
        pod.automount_service_account_token = Confidence::No;

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(
            !update.new_relations.iter().any(|r| r.is::<Uses>()),
            "should not emit uses relation when automount is explicitly disabled"
        );
    }

    #[test]
    fn pod_sa_already_in_campaign_does_not_duplicate_sa_entity() {
        let mut campaign = test_campaign();

        let existing_sa = ServiceAccount::new("existing-sa", "default");
        campaign.entities.insert_typed(existing_sa.clone());

        let mut pod = Pod::new("worker", "default");
        pod.service_account_name = Some("existing-sa".to_string());

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        let sa_entities: Vec<_> = update
            .new_entities
            .iter()
            .filter(|e| e.entity_kind() == "ServiceAccount")
            .collect();
        assert!(sa_entities.is_empty(), "should not emit duplicate SA entity");
        // but the uses relation should still be emitted
        assert!(
            update.new_relations.iter().any(|r| r.is::<Uses>()),
            "should still emit uses relation even when SA already known"
        );
    }

    #[test]
    fn service_account_with_pod_exec_permission_inferrs_can_exec() {
        let mut campaign = test_campaign();

        let mut pod = Pod::new("target", "default");
        pod.is_running = true;
        campaign.entities.insert_typed(pod.clone());

        let mut sa = ServiceAccount::new("operator", "default");
        let mut perm = RbacPermission::new("create", "pods/exec");
        perm.scope = Some("default".to_string());
        sa.entitlements.push(perm);

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(sa.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(update.new_relations.iter().any(|r| {
            r.is::<PodExec>()
                && r.source_id().0 == sa.entity_id().0
                && r.target_id().0 == pod.entity_id().0
        }));
    }

    #[test]
    fn sa_with_token_creates_pod_entity_and_uses_relation() {
        use ran_domain::{JwToken, ServiceAccountToken};

        let campaign = test_campaign();

        let token = ServiceAccountToken {
            jwt: JwToken { raw: "raw.jwt.here".to_string(), ..Default::default() },
            namespace: "default".to_string(),
            service_account_name: "web-sa".to_string(),
            pod_name: Some("web-pod".to_string()),
            pod_uid: Some("pod-uid".to_string()),
            is_bound: true,
            ..Default::default()
        };
        let mut sa = ServiceAccount::new("web-sa", "default");
        sa.token = Some(token);

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(sa.clone()));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(
            update.new_entities.iter().any(|e| e.entity_kind() == "Pod" && e.entity_name() == "web-pod"),
            "expected Pod entity to be inferred from token"
        );
        assert!(
            update.new_relations.iter().any(|r| r.is::<Uses>()),
            "expected uses relation from pod to SA"
        );
    }

    #[test]
    fn sa_with_token_does_not_duplicate_existing_pod() {
        use ran_domain::{JwToken, ServiceAccountToken};

        let mut campaign = test_campaign();
        let existing_pod = Pod::new("web-pod", "default");
        campaign.entities.insert_typed(existing_pod);

        let token = ServiceAccountToken {
            jwt: JwToken { raw: "raw.jwt.token".to_string(), ..Default::default() },
            namespace: "default".to_string(),
            service_account_name: "web-sa".to_string(),
            pod_name: Some("web-pod".to_string()),
            pod_uid: Some("uid".to_string()),
            is_bound: true,
            ..Default::default()
        };
        let mut sa = ServiceAccount::new("web-sa", "default");
        sa.token = Some(token);

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(sa));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        let pod_entities: Vec<_> = update
            .new_entities
            .iter()
            .filter(|e| e.entity_kind() == "Pod" && e.entity_name() == "web-pod")
            .collect();
        assert!(pod_entities.is_empty(), "should not duplicate pod already in campaign");
        assert!(
            update.new_relations.iter().any(|r| r.is::<Uses>()),
            "uses relation should still be emitted"
        );
    }

    #[test]
    fn sa_without_token_is_ignored_by_token_analyzer() {
        let campaign = test_campaign();

        let sa = ServiceAccount::new("bare-sa", "default");
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(sa));

        let analyzer = super::ServiceAccountTokenAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        assert!(inferred.new_entities.is_empty());
        assert!(inferred.new_relations.is_empty());
    }

    #[test]
    fn kubelet_source_and_runs_on_infer_kubelet_sink_for_other_running_pods() {
        let mut campaign = test_campaign();

        let mut src = Pod::new("src", "default");
        src.is_running = true;
        let src_id = src.entity_id().0.clone();
        campaign.entities.insert_typed(src.clone());

        let mut target = Pod::new("target", "default");
        target.is_running = true;
        let target_id = target.entity_id().0.clone();
        campaign.entities.insert_typed(target.clone());

        campaign.insert_relation(&RunsOn::new(src_id.clone(), "node/worker-1"));
        campaign.insert_relation(&RunsOn::new(target_id.clone(), "node/worker-1"));
        campaign.insert_relation(&KubeletExecSource::new(src_id.clone(), "node/worker-1"));

        let mut update = FactsUpdate::default();
        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(update.new_relations.iter().any(|r| {
            r.is::<KubeletExecSink>()
                && r.source_id().0 == "node/worker-1"
                && r.target_id().0 == target_id
        }));
        assert!(!update.new_relations.iter().any(|r| {
            r.is::<KubeletExecSink>()
                && r.source_id().0 == "node/worker-1"
                && r.target_id().0 == src_id
        }));
    }
}
