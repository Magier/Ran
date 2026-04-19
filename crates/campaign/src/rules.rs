use ran_domain::{
    BindsTo, Contains, Entity, EntityId, Grants, K8sCluster, K8sNode, K8sRole, K8sRoleBinding,
    KubeletExecSink, ManagesNode, Namespace, Pod, PodExec, RbacPermission,
    RelationSummary, RunsOn, ServiceAccount,
};

use crate::{Campaign, FactsUpdate};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuleTrigger {
    Always,
    EntityKind(String),
    RelationName(String),
}

pub trait InferenceRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn triggers(&self) -> Vec<RuleTrigger>;
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate;
}

pub struct NamespaceClusterRule;
pub struct NodeClusterRule;
pub struct PodNamespaceRule;
pub struct ServiceAccountNamespaceRule;
pub struct PodNodeRule;
pub struct ServiceAccountCanExecRule;
pub struct KubeletExecSinkRule;
pub struct RoleNamespaceRule;
pub struct RoleBindingNamespaceRule;
pub struct ClusterRoleClusterRule;
pub struct ClusterRoleBindingClusterRule;
pub struct RoleBindingPermissionsRule;
pub struct RoleBindingGraphRule;

impl InferenceRule for NamespaceClusterRule {
    fn name(&self) -> &'static str {
        "namespace.cluster"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![RuleTrigger::EntityKind("Namespace".to_string())]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let Some(cluster) = campaign.entities.values::<K8sCluster>().next() else {
            return inferred;
        };
        let cluster_id = cluster.entity_id();

        for entity in &update.new_entities {
            let Some(ns) = entity.as_any().downcast_ref::<Namespace>() else {
                continue;
            };

            inferred.new_relations.push(Box::new(Contains::new(
                cluster_id.0.clone(),
                ns.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

impl InferenceRule for NodeClusterRule {
    fn name(&self) -> &'static str {
        "node.cluster"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![RuleTrigger::EntityKind("Node".to_string())]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let Some(cluster) = campaign.entities.values::<K8sCluster>().next() else {
            return inferred;
        };
        let cluster_id = cluster.entity_id();

        for entity in &update.new_entities {
            let Some(node) = entity.as_any().downcast_ref::<K8sNode>() else {
                continue;
            };

            inferred.new_relations.push(Box::new(ManagesNode::new(
                cluster_id.0.clone(),
                node.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

impl InferenceRule for PodNamespaceRule {
    fn name(&self) -> &'static str {
        "pod.namespace"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![RuleTrigger::EntityKind("Pod".to_string())]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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
            let ns_exists = campaign.entities.contains::<Namespace>(&ns_id)
                || update.new_entities.iter().any(|e| {
                    e.as_any()
                        .downcast_ref::<Namespace>()
                        .map(|n| n.entity_id() == ns_id)
                        .unwrap_or(false)
                });

            if !ns_exists {
                inferred
                    .new_entities
                    .push(Box::new(Namespace::new(ns_name)));
            }

            inferred.new_relations.push(Box::new(Contains::new(
                ns_id.0.clone(),
                pod.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

impl InferenceRule for ServiceAccountNamespaceRule {
    fn name(&self) -> &'static str {
        "serviceaccount.namespace"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![RuleTrigger::EntityKind("ServiceAccount".to_string())]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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
            let ns_exists = campaign.entities.contains::<Namespace>(&ns_id)
                || update.new_entities.iter().any(|e| {
                    e.as_any()
                        .downcast_ref::<Namespace>()
                        .map(|n| n.entity_id() == ns_id)
                        .unwrap_or(false)
                });

            if !ns_exists {
                inferred
                    .new_entities
                    .push(Box::new(Namespace::new(ns_name)));
            }

            inferred.new_relations.push(Box::new(Contains::new(
                ns_id.0.clone(),
                sa.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

impl InferenceRule for PodNodeRule {
    fn name(&self) -> &'static str {
        "pod.node"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![RuleTrigger::EntityKind("Pod".to_string())]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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

            inferred
                .new_relations
                .push(Box::new(RunsOn::new(pod.entity_id().0.clone(), node_id.0)));
        }

        inferred
    }
}

impl InferenceRule for ServiceAccountCanExecRule {
    fn name(&self) -> &'static str {
        "serviceaccount.can-exec"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![
            RuleTrigger::EntityKind("Pod".to_string()),
            RuleTrigger::RelationName("can".to_string()),
        ]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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

                let can_exec = sa
                    .entitlements
                    .iter()
                    .any(|perm| perm.satisfies("create", "pods/exec") && perm.is_in_scope(ns));

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

impl InferenceRule for KubeletExecSinkRule {
    fn name(&self) -> &'static str {
        "kubelet.exec-sink"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![
            RuleTrigger::RelationName("kubelet-exec".to_string()),
            RuleTrigger::RelationName("runs-on".to_string()),
        ]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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

                inferred.new_relations.push(Box::new(KubeletExecSink::new(
                    node_id.clone(),
                    target_pod_id.clone(),
                )));
            }
        }

        inferred
    }
}

impl InferenceRule for RoleNamespaceRule {
    fn name(&self) -> &'static str {
        "role.namespace"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![RuleTrigger::EntityKind("Role".to_string())]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(role) = entity.as_any().downcast_ref::<K8sRole>() else {
                continue;
            };
            if role.is_cluster_role {
                continue;
            }
            let Some(ns_name) = role.namespace() else {
                continue;
            };
            if ns_name.is_empty() {
                continue;
            }

            let ns_id = EntityId::new(format!("ns/{}", ns_name));
            let ns_exists = campaign.entities.contains::<Namespace>(&ns_id)
                || update.new_entities.iter().any(|e| {
                    e.as_any()
                        .downcast_ref::<Namespace>()
                        .map(|n| n.entity_id() == ns_id)
                        .unwrap_or(false)
                });

            if !ns_exists {
                inferred
                    .new_entities
                    .push(Box::new(Namespace::new(ns_name)));
            }

            inferred.new_relations.push(Box::new(Contains::new(
                ns_id.0.clone(),
                role.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

impl InferenceRule for RoleBindingNamespaceRule {
    fn name(&self) -> &'static str {
        "rolebinding.namespace"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![RuleTrigger::EntityKind("RoleBinding".to_string())]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(binding) = entity.as_any().downcast_ref::<K8sRoleBinding>() else {
                continue;
            };
            let Some(ns_name) = binding.namespace() else {
                continue;
            };
            if ns_name.is_empty() {
                continue;
            }

            let ns_id = EntityId::new(format!("ns/{}", ns_name));
            let ns_exists = campaign.entities.contains::<Namespace>(&ns_id)
                || update.new_entities.iter().any(|e| {
                    e.as_any()
                        .downcast_ref::<Namespace>()
                        .map(|n| n.entity_id() == ns_id)
                        .unwrap_or(false)
                });

            if !ns_exists {
                inferred
                    .new_entities
                    .push(Box::new(Namespace::new(ns_name)));
            }

            inferred.new_relations.push(Box::new(Contains::new(
                ns_id.0.clone(),
                binding.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

impl InferenceRule for ClusterRoleClusterRule {
    fn name(&self) -> &'static str {
        "clusterrole.cluster"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![RuleTrigger::EntityKind("ClusterRole".to_string())]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        let Some(cluster) = campaign.entities.values::<K8sCluster>().next() else {
            return inferred;
        };
        let cluster_id = cluster.entity_id();

        for entity in &update.new_entities {
            let Some(role) = entity.as_any().downcast_ref::<K8sRole>() else {
                continue;
            };
            if !role.is_cluster_role {
                continue;
            }
            inferred.new_relations.push(Box::new(Contains::new(
                cluster_id.0.clone(),
                role.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

impl InferenceRule for ClusterRoleBindingClusterRule {
    fn name(&self) -> &'static str {
        "clusterrolebinding.cluster"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![RuleTrigger::EntityKind("ClusterRoleBinding".to_string())]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        let Some(cluster) = campaign.entities.values::<K8sCluster>().next() else {
            return inferred;
        };
        let cluster_id = cluster.entity_id();

        for entity in &update.new_entities {
            let Some(binding) = entity.as_any().downcast_ref::<K8sRoleBinding>() else {
                continue;
            };
            let ns = binding.meta.namespace.as_deref().unwrap_or("");
            if !ns.is_empty() {
                continue;
            }
            inferred.new_relations.push(Box::new(Contains::new(
                cluster_id.0.clone(),
                binding.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

/// Resolve `K8sRoleBinding` → `K8sRole` → `ServiceAccount` entitlement injection.
///
/// For every new binding, finds the referenced role (in campaign state or the
/// current update batch), stamps the scope and source_role onto each permission,
/// and emits a minimal `ServiceAccount` carrying those entitlements.
/// The entity store merges the entitlements into any existing SA record.
impl InferenceRule for RoleBindingPermissionsRule {
    fn name(&self) -> &'static str {
        "rolebinding.permissions"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![
            RuleTrigger::EntityKind("RoleBinding".to_string()),
            RuleTrigger::EntityKind("ClusterRoleBinding".to_string()),
        ]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(binding) = entity.as_any().downcast_ref::<K8sRoleBinding>() else {
                continue;
            };

            let binding_ns = binding.meta.namespace.as_deref().unwrap_or("");
            let scope: Option<String> = if binding_ns.is_empty() {
                Some("*".to_string())
            } else {
                Some(binding_ns.to_string())
            };

            let role_perms: Vec<RbacPermission> =
                find_role_permissions(campaign, update, &binding.role_ref);
            if role_perms.is_empty() {
                continue;
            }

            let stamped: Vec<RbacPermission> = role_perms
                .into_iter()
                .map(|mut p| {
                    p.scope = scope.clone();
                    p.source_role = Some(binding.role_ref.clone());
                    p
                })
                .collect();

            for subject in &binding.subjects {
                if !subject.kind.eq_ignore_ascii_case("ServiceAccount") {
                    continue;
                }
                let sa_ns = if subject.namespace.is_empty() {
                    binding_ns.to_string()
                } else {
                    subject.namespace.clone()
                };
                let mut sa = ServiceAccount::new(&subject.name, &sa_ns);
                sa.entitlements = stamped.clone();
                inferred.new_entities.push(Box::new(sa));
            }
        }

        inferred
    }
}

/// Emit `BindsTo(binding → role)` and `Grants(binding → sa)` edges for every
/// new `K8sRoleBinding` / `K8sClusterRoleBinding`.  Creates stub role and SA
/// entities when they are not yet known in the campaign so the graph stays
/// connected even if discovery runs out of order.
impl InferenceRule for RoleBindingGraphRule {
    fn name(&self) -> &'static str {
        "rolebinding.graph"
    }

    fn triggers(&self) -> Vec<RuleTrigger> {
        vec![
            RuleTrigger::EntityKind("RoleBinding".to_string()),
            RuleTrigger::EntityKind("ClusterRoleBinding".to_string()),
        ]
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(binding) = entity.as_any().downcast_ref::<K8sRoleBinding>() else {
                continue;
            };

            let binding_id = binding.entity_id();
            let binding_ns = binding.meta.namespace.as_deref().unwrap_or("");
            let is_cluster_scoped = binding_ns.is_empty();

            // Determine whether the roleRef points to a ClusterRole or a Role.
            let ref_is_cluster = binding.role_ref_kind.eq_ignore_ascii_case("ClusterRole")
                || (binding.role_ref_kind.is_empty() && is_cluster_scoped);

            // Build the target role entity_id the same way K8sRole::entity_id() does.
            let role_entity_id = if ref_is_cluster {
                EntityId(format!("clusterrole/{}", binding.role_ref))
            } else {
                EntityId(format!("ns/{}/role/{}", binding_ns, binding.role_ref))
            };

            // Create a stub role if not yet known.
            let role_known = campaign
                .entities
                .values::<K8sRole>()
                .any(|r| r.entity_id() == role_entity_id)
                || update.new_entities.iter().any(|e| {
                    e.as_any()
                        .downcast_ref::<K8sRole>()
                        .map(|r| r.entity_id() == role_entity_id)
                        .unwrap_or(false)
                });

            if !role_known {
                let mut stub = K8sRole::new(&binding.role_ref, binding_ns);
                stub.is_cluster_role = ref_is_cluster;
                inferred.new_entities.push(Box::new(stub));
            }

            inferred
                .new_relations
                .push(Box::new(BindsTo::new(binding_id.0.clone(), role_entity_id.0)));

            // Emit Grants edges for ServiceAccount subjects.
            for subject in &binding.subjects {
                if !subject.kind.eq_ignore_ascii_case("ServiceAccount") {
                    continue;
                }
                let sa_ns = if subject.namespace.is_empty() {
                    binding_ns.to_string()
                } else {
                    subject.namespace.clone()
                };
                let sa_entity_id = EntityId(format!("ns/{}/sa/{}", sa_ns, subject.name));

                let sa_known = campaign
                    .entities
                    .values::<ServiceAccount>()
                    .any(|sa| sa.entity_id() == sa_entity_id)
                    || update.new_entities.iter().any(|e| {
                        e.as_any()
                            .downcast_ref::<ServiceAccount>()
                            .map(|sa| sa.entity_id() == sa_entity_id)
                            .unwrap_or(false)
                    });

                if !sa_known {
                    let stub = ServiceAccount::new(&subject.name, &sa_ns);
                    inferred.new_entities.push(Box::new(stub));
                }

                inferred.new_relations.push(Box::new(Grants::new(
                    binding_id.0.clone(),
                    sa_entity_id.0,
                )));
            }
        }

        inferred
    }
}

fn find_role_permissions(
    campaign: &Campaign,
    update: &FactsUpdate,
    role_name: &str,
) -> Vec<RbacPermission> {
    if let Some(role) = campaign
        .entities
        .values::<K8sRole>()
        .find(|r| r.meta.name == role_name)
    {
        return role.permissions.clone();
    }
    if let Some(role) = update.new_entities.iter().find_map(|e| {
        e.as_any()
            .downcast_ref::<K8sRole>()
            .filter(|r| r.meta.name == role_name)
    }) {
        return role.permissions.clone();
    }
    Vec::new()
}

pub fn default_rules() -> Vec<Box<dyn InferenceRule>> {
    vec![
        Box::new(NamespaceClusterRule),
        Box::new(NodeClusterRule),
        Box::new(PodNamespaceRule),
        Box::new(ServiceAccountNamespaceRule),
        Box::new(PodNodeRule),
        Box::new(ServiceAccountCanExecRule),
        Box::new(KubeletExecSinkRule),
        Box::new(RoleNamespaceRule),
        Box::new(RoleBindingNamespaceRule),
        Box::new(ClusterRoleClusterRule),
        Box::new(ClusterRoleBindingClusterRule),
        Box::new(RoleBindingPermissionsRule),
        Box::new(RoleBindingGraphRule),
    ]
}

pub fn run_rules_fixpoint(
    campaign: &Campaign,
    rules: &[Box<dyn InferenceRule>],
    initial: FactsUpdate,
) -> FactsUpdate {
    let mut acc = initial;
    let mut iteration = 0;
    let max_iterations = 8;

    loop {
        if iteration >= max_iterations {
            break;
        }

        let mut changed = false;
        let mut next = FactsUpdate::default();

        for rule in rules {
            let inferred = rule.infer(campaign, &acc);
            if !inferred.new_entities.is_empty() || !inferred.new_relations.is_empty() {
                changed = true;
                next.merge(inferred);
            }
        }

        if !changed {
            break;
        }

        acc.merge(next);
        iteration += 1;
    }

    acc
}

fn collect_pods(campaign: &Campaign, update: &FactsUpdate) -> Vec<Pod> {
    let mut pods = campaign
        .entities
        .values::<Pod>()
        .cloned()
        .collect::<Vec<_>>();
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
    let mut sas = campaign
        .entities
        .values::<ServiceAccount>()
        .cloned()
        .collect::<Vec<_>>();
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
    // Start from the committed graph state.
    let mut rels = campaign.graph.to_relation_summaries();
    // Append pending relations from the in-flight update (not yet committed).
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

#[cfg(test)]
mod tests {
    use ran_domain::{
        K8sCluster, K8sNode, KubeletExecSink, KubeletExecSource, ManagesNode, Pod, PodExec,
        RbacPermission, RunsOn, ServiceAccount,
    };

    use super::*;
    use crate::Campaign;

    #[test]
    fn node_cluster_rule_infers_manages_node_relation() {
        let campaign = Campaign::bootstrap("ran", K8sCluster::new("test-cluster"));
        let cluster_id = campaign
            .entities
            .values::<K8sCluster>()
            .next()
            .unwrap()
            .entity_id();

        let node = K8sNode::new("node-1");
        let node_id = node.entity_id();
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(node));

        let rules = default_rules();
        let all = run_rules_fixpoint(&campaign, &rules, update);

        let rel = all.new_relations.iter().find(|r| {
            r.is::<ManagesNode>() && r.source_id().0 == cluster_id.0 && r.target_id().0 == node_id.0
        });
        assert!(
            rel.is_some(),
            "expected manages-node relation from cluster to node"
        );
    }

    #[test]
    fn fixpoint_runner_infers_runs_on_and_kubelet_sink_chain() {
        let campaign = Campaign::bootstrap("ran", K8sCluster::new("test-cluster"));

        let mut update = FactsUpdate::default();
        let mut pod = Pod::new("target-pod", "ns");
        pod.node_name = Some("node-a".to_string());
        pod.is_running = true;
        let target_pod_id = pod.entity_id();
        update.new_entities.push(Box::new(pod));

        let source = EntityId::new("pod/ns:attacker");
        update.new_relations.push(Box::new(KubeletExecSource::new(
            source.0.clone(),
            "node/node-a",
        )));

        let rules = default_rules();
        let all = run_rules_fixpoint(&campaign, &rules, update);

        let has_runs_on = all.new_relations.iter().any(|r| {
            r.is::<RunsOn>() && r.source_id() == &target_pod_id && r.target_id().0 == "node/node-a"
        });
        let has_sink = all.new_relations.iter().any(|r| {
            r.is::<KubeletExecSink>()
                && r.source_id().0 == "node/node-a"
                && r.target_id() == &target_pod_id
        });

        assert!(has_runs_on, "expected runs-on to be inferred in fixpoint");
        assert!(
            has_sink,
            "expected kubelet-pod-exec to be inferred through fixpoint chaining"
        );
    }

    #[test]
    fn fixpoint_runner_infers_serviceaccount_can_exec() {
        let campaign = Campaign::bootstrap("ran", K8sCluster::new("test-cluster"));

        let mut update = FactsUpdate::default();
        let mut pod = Pod::new("nginx", "default");
        pod.service_account_name = Some("sa-a".to_string());
        pod.is_running = true;
        let target_pod_id = pod.entity_id();
        update.new_entities.push(Box::new(pod));

        let mut sa = ServiceAccount::new("sa-a", "default");
        let mut perm = RbacPermission::new("create", "pods/exec");
        perm.scope = Some("default".to_string());
        sa.entitlements.push(perm);
        let sa_id = sa.entity_id();
        update.new_entities.push(Box::new(sa));

        let rules = default_rules();
        let all = run_rules_fixpoint(&campaign, &rules, update);

        let has_can_exec = all.new_relations.iter().any(|r| {
            r.is::<PodExec>() && r.source_id().0 == sa_id.0 && r.target_id() == &target_pod_id
        });

        assert!(has_can_exec, "expected k8s.can-exec inference in fixpoint");
    }
}
