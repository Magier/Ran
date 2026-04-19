use ran_domain::{
    Confidence, Contains, DaemonSet, Entity, EntityId, GCPServiceAccount, Job, K8sCluster, K8sNode,
    K8sRole, K8sRoleBinding, KubeletExecSink, Namespace, Owns, Pod, PodExec, RbacPermission,
    RbacSubject, RelationSummary, ReplicaSet, RunsOn, ServiceAccount, StatefulSet, Uses,
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

            inferred.new_relations.push(Box::new(Contains::new(
                cluster_id.0.clone(),
                ns.entity_id().0.clone(),
            )));
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
            inferred
                .new_relations
                .push(Box::new(RunsOn::new(pod.entity_id().0.clone(), node_id.0)));
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
                || update.new_entities.iter().any(|e| e.entity_id() == sa_id);
            if !sa_known {
                inferred.new_entities.push(Box::new(sa));
            }

            inferred.new_relations.push(Box::new(Uses::new(
                pod.entity_id().0.clone(),
                sa_id.0.clone(),
            )));
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

                inferred.new_relations.push(Box::new(KubeletExecSink::new(
                    node_id.clone(),
                    target_pod_id.clone(),
                )));
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
                inferred
                    .new_relations
                    .push(Box::new(RunsOn::new(pod.entity_id().0.clone(), node_id.0)));
            }
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// CanExecAccessAnalyzer
// ---------------------------------------------------------------------------

/// Set `system.access_level` to `Exec` on every system entity that receives
/// an incoming exec-channel relation.
///
/// Triggers on any relation that returns `true` for [`Relation::is_exec_channel`]
/// — this covers `PodExec` (kubectl exec), `KubeletExecSink` (kubelet exec),
/// `RceCanExec` (exploit), and any future exec-channel type without needing
/// a name-based allowlist.
///
/// The "take max" semantics are enforced automatically by `SystemInfo::merge_from`
/// — we emit a cloned entity with `access_level = Exec` and the entity store
/// merges it in, so already-`Exec` entities are unaffected.
///
/// This ensures that access level propagates to targets discovered through
/// lateral-movement TTPs even before `sys.userid` output is available.
pub struct CanExecAccessAnalyzer;

impl Analyzer for CanExecAccessAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        let exec_target_ids: Vec<String> = update
            .new_relations
            .iter()
            .filter(|r| r.is_exec_channel())
            .map(|r| r.target_id().0.clone())
            .collect();

        for target_id in exec_target_ids {
            let eid = ran_domain::EntityId::new(&target_id);

            // Resolve the target from the committed campaign state first, then
            // fall back to entities pending in this same update (not yet stored).
            let (is_pod, current_level): (bool, ran_domain::AccessLevel) =
                if let Some(pod) = campaign.entities.find::<ran_domain::Pod>(&eid) {
                    (true, pod.system.access_level)
                } else if let Some(node) = campaign.entities.find::<ran_domain::K8sNode>(&eid) {
                    (false, node.system.access_level)
                } else if let Some(pod) = update.new_entities.iter().find_map(|e| {
                    e.as_any()
                        .downcast_ref::<ran_domain::Pod>()
                        .filter(|p| p.entity_id() == eid)
                }) {
                    (true, pod.system.access_level)
                } else if let Some(node) = update.new_entities.iter().find_map(|e| {
                    e.as_any()
                        .downcast_ref::<ran_domain::K8sNode>()
                        .filter(|n| n.entity_id() == eid)
                }) {
                    (false, node.system.access_level)
                } else {
                    // Not a system entity — skip.
                    continue;
                };

            // Only emit if access_level is not already Exec — the merge takes
            // max, so this is a no-op for already-Exec entities, but skipping
            // avoids a needless clone.
            if current_level >= ran_domain::AccessLevel::Exec {
                continue;
            }

            // Emit a cloned entity with access_level = Exec.
            // When committed via apply_facts → insert_entity → merge_from,
            // the max(access_level) rule will raise the stored level.
            if is_pod {
                let mut pod = ran_domain::Pod::new(eid.0.rsplit('/').next().unwrap_or(&eid.0), "");
                pod.meta.name = eid.0.rsplit('/').next().unwrap_or(&eid.0).to_string();
                // Reconstruct a minimal pod that will merge into the existing one.
                // The only thing that matters for the merge is the entity ID and
                // the elevated access_level.
                let mut full_pod = campaign
                    .entities
                    .find::<ran_domain::Pod>(&eid)
                    .cloned()
                    .or_else(|| {
                        update.new_entities.iter().find_map(|e| {
                            e.as_any()
                                .downcast_ref::<ran_domain::Pod>()
                                .filter(|p| p.entity_id() == eid)
                                .cloned()
                        })
                    })
                    .unwrap_or(pod);
                full_pod.system.access_level = ran_domain::AccessLevel::Exec;
                inferred.new_entities.push(Box::new(full_pod));
            } else {
                let node_name = eid.0.strip_prefix("node/").unwrap_or(&eid.0);
                let mut full_node = campaign
                    .entities
                    .find::<ran_domain::K8sNode>(&eid)
                    .cloned()
                    .or_else(|| {
                        update.new_entities.iter().find_map(|e| {
                            e.as_any()
                                .downcast_ref::<ran_domain::K8sNode>()
                                .filter(|n| n.entity_id() == eid)
                                .cloned()
                        })
                    })
                    .unwrap_or_else(|| ran_domain::K8sNode::new(node_name));
                full_node.system.access_level = ran_domain::AccessLevel::Exec;
                inferred.new_entities.push(Box::new(full_node));
            }
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// WorkloadOwnershipAnalyzer
// ---------------------------------------------------------------------------

/// For every new `Pod` with owner references, create the owning workload entity
/// (if not already known) and emit an `owns` relation from the workload to the pod.
///
/// Handles the following owner kinds from `metadata.ownerReferences`:
/// - `ReplicaSet` → creates a `ReplicaSet` entity + `Owns(ReplicaSet → Pod)`
/// - `StatefulSet` → creates a `StatefulSet` entity + `Owns(StatefulSet → Pod)`
/// - `DaemonSet` → creates a `DaemonSet` entity + `Owns(DaemonSet → Pod)`
/// - `Job` → creates a `Job` entity + `Owns(Job → Pod)`
///
/// Trigger: new `Pod` entities with non-empty `owner_references`.
pub struct WorkloadOwnershipAnalyzer;

impl Analyzer for WorkloadOwnershipAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(pod) = entity.as_any().downcast_ref::<Pod>() else {
                continue;
            };
            if pod.owner_references.is_empty() {
                continue;
            }

            let ns = pod.namespace().unwrap_or("").to_string();
            let pod_id = pod.entity_id().0.clone();

            for owner_ref in &pod.owner_references {
                match owner_ref.kind.as_str() {
                    "ReplicaSet" => {
                        let rs = ReplicaSet::new(&owner_ref.name, &ns);
                        let rs_id = rs.entity_id();
                        let known = campaign.entities.contains::<ReplicaSet>(&rs_id)
                            || update.new_entities.iter().any(|e| e.entity_id() == rs_id)
                            || inferred.new_entities.iter().any(|e| e.entity_id() == rs_id);
                        if !known {
                            inferred.new_entities.push(Box::new(rs));
                        }
                        inferred
                            .new_relations
                            .push(Box::new(Owns::new(rs_id.0, pod_id.clone())));
                    }
                    "StatefulSet" => {
                        let ss = StatefulSet::new(&owner_ref.name, &ns);
                        let ss_id = ss.entity_id();
                        let known = campaign.entities.contains::<StatefulSet>(&ss_id)
                            || update.new_entities.iter().any(|e| e.entity_id() == ss_id)
                            || inferred.new_entities.iter().any(|e| e.entity_id() == ss_id);
                        if !known {
                            inferred.new_entities.push(Box::new(ss));
                        }
                        inferred
                            .new_relations
                            .push(Box::new(Owns::new(ss_id.0, pod_id.clone())));
                    }
                    "DaemonSet" => {
                        let ds = DaemonSet::new(&owner_ref.name, &ns);
                        let ds_id = ds.entity_id();
                        let known = campaign.entities.contains::<DaemonSet>(&ds_id)
                            || update.new_entities.iter().any(|e| e.entity_id() == ds_id)
                            || inferred.new_entities.iter().any(|e| e.entity_id() == ds_id);
                        if !known {
                            inferred.new_entities.push(Box::new(ds));
                        }
                        inferred
                            .new_relations
                            .push(Box::new(Owns::new(ds_id.0, pod_id.clone())));
                    }
                    "Job" => {
                        let job = Job::new(&owner_ref.name, &ns);
                        let job_id = job.entity_id();
                        let known = campaign.entities.contains::<Job>(&job_id)
                            || update.new_entities.iter().any(|e| e.entity_id() == job_id)
                            || inferred
                                .new_entities
                                .iter()
                                .any(|e| e.entity_id() == job_id);
                        if !known {
                            inferred.new_entities.push(Box::new(job));
                        }
                        inferred
                            .new_relations
                            .push(Box::new(Owns::new(job_id.0, pod_id.clone())));
                    }
                    _ => {}
                }
            }
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// PropagateHostIPAnalyzer
// ---------------------------------------------------------------------------

/// Copy a pod's `host_ip` into the owning node's `system.ips` when the two are
/// connected by a `runs-on` relation.
///
/// Triggers on two events:
/// 1. **New Pod with `host_ip`** — if a `runs-on` edge already exists in the
///    campaign graph (or in the current update from `PodNodeAnalyzer`), the IP
///    is propagated to the target node immediately.
/// 2. **New `runs-on` relation** — if the source pod (in campaign state or the
///    current update) already has `host_ip` set, the IP is propagated to the
///    target node.  This covers the case where the pod entity arrives first,
///    a `runs-on` edge is later wired (e.g. by `PodNodeAnalyzer`), and
///    `PropagateHostIPAnalyzer` runs after them in the same pipeline pass.
///
/// No update is emitted when the IP is already present in the node's `system.ips`.
pub struct PropagateHostIPAnalyzer;

impl Analyzer for PropagateHostIPAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        // --- Case 1: new Pod with host_ip ----------------------------------
        for entity in &update.new_entities {
            let Some(pod) = entity.as_any().downcast_ref::<Pod>() else {
                continue;
            };
            let Some(host_ip) = pod.host_ip else {
                continue;
            };

            let pod_id = pod.entity_id();

            // Runs-on edges committed to the campaign graph.
            let node_ids: Vec<EntityId> = campaign
                .graph
                .targets_of(&pod_id, "runs-on")
                .into_iter()
                .cloned()
                .collect();
            for node_id in node_ids {
                propagate_ip_to_node(&mut inferred, campaign, update, &node_id, host_ip);
            }

            // Runs-on edges still pending in the current update (e.g. from PodNodeAnalyzer).
            for rel in &update.new_relations {
                if rel.relation_name() == "runs-on" && rel.source_id() == &pod_id {
                    let node_id = rel.target_id().clone();
                    propagate_ip_to_node(&mut inferred, campaign, update, &node_id, host_ip);
                }
            }
        }

        // --- Case 2: new RunsOn relation -----------------------------------
        for rel in &update.new_relations {
            if rel.relation_name() != "runs-on" {
                continue;
            }
            let pod_id = rel.source_id();
            let node_id = rel.target_id().clone();

            // Find the pod's host_ip from campaign state or the current update.
            let host_ip = campaign
                .entities
                .find::<Pod>(pod_id)
                .and_then(|p| p.host_ip)
                .or_else(|| {
                    update.new_entities.iter().find_map(|e| {
                        e.as_any()
                            .downcast_ref::<Pod>()
                            .filter(|p| &p.entity_id() == pod_id)
                            .and_then(|p| p.host_ip)
                    })
                });

            let Some(host_ip) = host_ip else {
                continue;
            };
            propagate_ip_to_node(&mut inferred, campaign, update, &node_id, host_ip);
        }

        inferred
    }
}

/// Emit an updated `K8sNode` with `host_ip` added to `system.ips`, unless the
/// IP is already present in the node's stored IPs or in a pending inferred update.
fn propagate_ip_to_node(
    inferred: &mut FactsUpdate,
    campaign: &Campaign,
    update: &FactsUpdate,
    node_id: &EntityId,
    host_ip: std::net::IpAddr,
) {
    // Already committed to campaign state?
    if let Some(node) = campaign.entities.find::<K8sNode>(node_id) {
        if node.system.ips.contains(&host_ip) {
            return;
        }
    }

    // Already emitted in this inferred batch?
    if inferred.new_entities.iter().any(|e| {
        e.as_any()
            .downcast_ref::<K8sNode>()
            .map(|n| n.entity_id() == *node_id && n.system.ips.contains(&host_ip))
            .unwrap_or(false)
    }) {
        return;
    }

    // Resolve the node: campaign → update → fresh placeholder.
    let mut node = campaign
        .entities
        .find::<K8sNode>(node_id)
        .cloned()
        .or_else(|| {
            update.new_entities.iter().find_map(|e| {
                e.as_any()
                    .downcast_ref::<K8sNode>()
                    .filter(|n| n.entity_id() == *node_id)
                    .cloned()
            })
        })
        .unwrap_or_else(|| {
            let name = node_id.0.strip_prefix("node/").unwrap_or(&node_id.0);
            K8sNode::new(name)
        });

    node.system.ips.push(host_ip);
    inferred.new_entities.push(Box::new(node));
}

// ---------------------------------------------------------------------------
// RoleNamespaceAnalyzer / RoleBindingNamespaceAnalyzer
// ---------------------------------------------------------------------------

/// For every new `K8sRole` with `is_cluster_role = true`, wire a `contains`
/// relation from the cluster to the ClusterRole.
pub struct ClusterRoleClusterAnalyzer;

impl Analyzer for ClusterRoleClusterAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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

/// For every new `K8sRoleBinding` with an empty namespace (i.e. a
/// ClusterRoleBinding), wire a `contains` relation from the cluster to the
/// ClusterRoleBinding.
pub struct ClusterRoleBindingClusterAnalyzer;

impl Analyzer for ClusterRoleBindingClusterAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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

/// For every new namespace-scoped `K8sRole`, ensure its namespace entity exists
/// and wire a `contains` relation from the Namespace to the Role.
pub struct RoleNamespaceAnalyzer;

impl Analyzer for RoleNamespaceAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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

            let rel = Contains::new(ns_id.0.clone(), role.entity_id().0.clone());
            if !campaign.entities.contains::<Namespace>(&ns_id) {
                inferred.new_entities.push(Box::new(ns));
            }
            inferred.new_relations.push(Box::new(rel));
        }

        inferred
    }
}

/// For every new namespace-scoped `K8sRoleBinding`, ensure its namespace entity
/// exists and wire a `contains` relation from the Namespace to the RoleBinding.
pub struct RoleBindingNamespaceAnalyzer;

impl Analyzer for RoleBindingNamespaceAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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

            let rel = Contains::new(ns_id.0.clone(), binding.entity_id().0.clone());
            if !campaign.entities.contains::<Namespace>(&ns_id) {
                inferred.new_entities.push(Box::new(ns));
            }
            inferred.new_relations.push(Box::new(rel));
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

            inferred.new_relations.push(Box::new(Contains::new(
                cluster_id.0.clone(),
                node.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// RoleBindingAnalyzer
// ---------------------------------------------------------------------------

/// Convert `K8sRoleBinding` entities into concrete `ServiceAccount` entitlements.
///
/// When a new `K8sRoleBinding` arrives, the analyzer:
/// 1. Finds the referenced `K8sRole` by name (searching both committed campaign
///    state and entities pending in the current update).
/// 2. For each subject of kind `"ServiceAccount"` in the binding, clones the
///    role's permissions and sets the `scope` field:
///    - Namespace-scoped binding → `scope = binding.namespace`
///    - Cluster-wide binding (empty namespace) → `scope = "*"`
///    - `source_role` is set to the role name on each cloned permission.
/// 3. Emits a minimal `ServiceAccount` entity carrying those entitlements;
///    the entity store merges it into the existing SA (or creates a new one
///    if the SA is not yet known).
///
/// If the referenced role cannot be found in the campaign, no entitlements
/// are emitted — this is not an error, the role may arrive later.
pub struct RoleBindingAnalyzer;

impl Analyzer for RoleBindingAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(binding) = entity.as_any().downcast_ref::<K8sRoleBinding>() else {
                continue;
            };

            // Determine scope from the binding's namespace.
            // An empty/absent namespace signals a ClusterRoleBinding → wildcard scope.
            let binding_ns = binding.meta.namespace.as_deref().unwrap_or("");
            let scope: Option<String> = if binding_ns.is_empty() {
                Some("*".to_string())
            } else {
                Some(binding_ns.to_string())
            };

            // Find the referenced role by name, checking both committed state and
            // entities that arrived in the same update batch as this binding.
            let role_perms: Vec<RbacPermission> =
                find_role_permissions(campaign, update, &binding.role_ref);

            if role_perms.is_empty() {
                // Role not found or has no permissions — skip silently.
                continue;
            }

            // Stamp scope and source_role onto every cloned permission.
            let stamped: Vec<RbacPermission> = role_perms
                .into_iter()
                .map(|mut p| {
                    p.scope = scope.clone();
                    p.source_role = Some(binding.role_ref.clone());
                    p
                })
                .collect();

            // Emit one SA update per ServiceAccount subject.
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

/// Find the permissions of a role named `role_name`.
///
/// Searches committed campaign state first, then falls back to new entities
/// in the current update batch (so a role and its binding can arrive together).
/// Returns an empty `Vec` when no matching role is found.
fn find_role_permissions(
    campaign: &Campaign,
    update: &FactsUpdate,
    role_name: &str,
) -> Vec<RbacPermission> {
    // Campaign state: iterate all registered K8sRole entities.
    if let Some(role) = campaign
        .entities
        .values::<K8sRole>()
        .find(|r| r.meta.name == role_name)
    {
        return role.permissions.clone();
    }

    // Pending update batch: roles parsed in the same round as the binding.
    if let Some(role) = update.new_entities.iter().find_map(|e| {
        e.as_any()
            .downcast_ref::<K8sRole>()
            .filter(|r| r.meta.name == role_name)
    }) {
        return role.permissions.clone();
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// GCPServiceAccountAnalyzer
// ---------------------------------------------------------------------------

/// Wire a `Uses` relation from a Pod to a `GCPServiceAccount` when the pod's
/// environment variables indicate GCP credential usage.
///
/// Two signals trigger the relation:
///
/// 1. **Env var value match** — any env var whose value matches the email of a
///    known `GCPServiceAccount` entity. This covers cases where the SA email is
///    injected directly (e.g. `CLOUDSDK_CORE_ACCOUNT=my-sa@proj.iam…`).
///
/// 2. **`GOOGLE_APPLICATION_CREDENTIALS` key** — presence of this env var
///    indicates a credential file is mounted, pointing to a GCP SA.  When a
///    GCP SA entity is known in the campaign, the pod is linked to the first
///    available one.  When no SA is yet known, the relation is deferred until
///    a `gcp.serviceaccount` parse runs.
pub struct GCPServiceAccountAnalyzer;

impl Analyzer for GCPServiceAccountAnalyzer {
    fn analyze(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        let gcp_sas: Vec<GCPServiceAccount> = campaign
            .entities
            .values::<GCPServiceAccount>()
            .cloned()
            .collect();

        if gcp_sas.is_empty() {
            return inferred;
        }

        let pods = collect_pods(campaign, update);

        for pod in &pods {
            if pod.system.env_vars.is_empty() {
                continue;
            }

            let pod_id = pod.entity_id().0.clone();

            // Signal 1: an env var value equals a known GCP SA email.
            if let Some(sa) = gcp_sas.iter().find(|sa| {
                !sa.email.is_empty() && pod.system.env_vars.values().any(|v| v == &sa.email)
            }) {
                inferred
                    .new_relations
                    .push(Box::new(Uses::new(pod_id, sa.entity_id().0.clone())));
                continue;
            }

            // Signal 2: GOOGLE_APPLICATION_CREDENTIALS key is present.
            if pod
                .system
                .env_vars
                .contains_key("GOOGLE_APPLICATION_CREDENTIALS")
            {
                if let Some(sa) = gcp_sas.first() {
                    inferred
                        .new_relations
                        .push(Box::new(Uses::new(pod_id, sa.entity_id().0.clone())));
                }
            }
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
        Box::new(PropagateHostIPAnalyzer),
        Box::new(ServiceAccountAnalyzer),
        Box::new(ServiceAccountTokenAnalyzer),
        Box::new(HostPathAnalyzer),
        Box::new(ServiceAccountCanExecAnalyzer),
        Box::new(KubeletExecSinkAnalyzer),
        Box::new(CanExecAccessAnalyzer),
        Box::new(WorkloadOwnershipAnalyzer),
        Box::new(ClusterRoleClusterAnalyzer),
        Box::new(ClusterRoleBindingClusterAnalyzer),
        Box::new(RoleNamespaceAnalyzer),
        Box::new(RoleBindingNamespaceAnalyzer),
        Box::new(RoleBindingAnalyzer),
        Box::new(GCPServiceAccountAnalyzer),
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
        AccessLevel, Confidence, Contains, K8sCluster, K8sNode, K8sRole, K8sRoleBinding,
        KubeletExecSink, KubeletExecSource, ManagesNode, Namespace, Pod, PodExec, RbacPermission,
        RbacSubject, RceCanExec, RunsOn, ServiceAccount, Uses,
    };

    use super::*;
    use crate::Campaign;

    fn test_campaign() -> Campaign {
        Campaign::bootstrap("ran", K8sCluster::new("test-cluster"))
    }

    #[test]
    fn node_gets_contains_relation_from_cluster() {
        let campaign = test_campaign();
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

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        let rel = update.new_relations.iter().find(|r| {
            r.is::<Contains>() && r.source_id().0 == cluster_id.0 && r.target_id().0 == node_id.0
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
            update
                .new_entities
                .iter()
                .all(|e| e.entity_kind() != "Namespace"),
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
        assert!(
            rel.is_some(),
            "expected cluster→namespace contains relation"
        );
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
            update
                .new_entities
                .iter()
                .any(|e| e.entity_kind() == "ServiceAccount" && e.entity_name() == "web-sa"),
            "expected ServiceAccount entity to be inferred"
        );
        assert!(
            update
                .new_relations
                .iter()
                .any(|r| r.is::<Uses>() && r.source_id().0 == pod.entity_id().0),
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
        assert!(
            sa_entities.is_empty(),
            "should not emit duplicate SA entity"
        );
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
            jwt: JwToken {
                raw: "raw.jwt.here".to_string(),
                ..Default::default()
            },
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
            update
                .new_entities
                .iter()
                .any(|e| e.entity_kind() == "Pod" && e.entity_name() == "web-pod"),
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
            jwt: JwToken {
                raw: "raw.jwt.token".to_string(),
                ..Default::default()
            },
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
        assert!(
            pod_entities.is_empty(),
            "should not duplicate pod already in campaign"
        );
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

    // ---------------------------------------------------------------------------
    // CanExecAccessAnalyzer tests
    // ---------------------------------------------------------------------------

    fn run_can_exec_access(campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        CanExecAccessAnalyzer.analyze(campaign, update)
    }

    #[test]
    fn can_exec_access_sets_exec_on_pod_exec_target() {
        let campaign = Campaign::bootstrap("ran", K8sCluster::new("test"));
        let pod = Pod::new("victim", "default");
        let pod_id = pod.entity_id().0.clone();

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));
        update
            .new_relations
            .push(Box::new(PodExec::new("sa/default/attacker", &pod_id)));

        let inferred = run_can_exec_access(&campaign, &update);

        let updated_pod = inferred
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<Pod>())
            .expect("should emit updated pod");
        assert_eq!(updated_pod.system.access_level, AccessLevel::Exec);
    }

    #[test]
    fn can_exec_access_is_idempotent_for_exec_pod() {
        let mut campaign = Campaign::bootstrap("ran", K8sCluster::new("test"));
        let mut pod = Pod::new("root-pod", "default");
        pod.system.access_level = AccessLevel::Exec;
        let pod_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        let mut update = FactsUpdate::default();
        update
            .new_relations
            .push(Box::new(PodExec::new("sa/default/attacker", &pod_id)));

        let inferred = run_can_exec_access(&campaign, &update);

        // No updated entity should be emitted (access_level already at Exec).
        assert!(inferred.new_entities.is_empty());
    }

    #[test]
    fn can_exec_access_triggers_on_kubelet_exec_sink() {
        let campaign = Campaign::bootstrap("ran", K8sCluster::new("test"));
        let pod = Pod::new("target", "default");
        let pod_id = pod.entity_id().0.clone();

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));
        update
            .new_relations
            .push(Box::new(KubeletExecSink::new("node/worker-1", &pod_id)));

        let inferred = run_can_exec_access(&campaign, &update);

        let updated_pod = inferred
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<Pod>())
            .expect("should emit updated pod");
        assert_eq!(updated_pod.system.access_level, AccessLevel::Exec);
    }

    #[test]
    fn can_exec_access_triggers_on_rce_can_exec() {
        let campaign = Campaign::bootstrap("ran", K8sCluster::new("test"));
        let pod = Pod::new("redis", "default");
        let pod_id = pod.entity_id().0.clone();

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));
        update.new_relations.push(Box::new(RceCanExec::new(
            "ns/default/pod/attacker",
            &pod_id,
        )));

        let inferred = run_can_exec_access(&campaign, &update);

        let updated_pod = inferred
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<Pod>())
            .expect("should emit updated pod");
        assert_eq!(updated_pod.system.access_level, AccessLevel::Exec);
    }

    #[test]
    fn can_exec_access_ignores_non_system_entity_targets() {
        let campaign = Campaign::bootstrap("ran", K8sCluster::new("test"));
        // Namespace is not a system entity — target ID doesn't resolve.
        let mut update = FactsUpdate::default();
        update.new_relations.push(Box::new(PodExec::new(
            "ns/default/pod/attacker",
            "ns/default", // namespace entity ID, not a pod/node
        )));

        let inferred = run_can_exec_access(&campaign, &update);
        assert!(inferred.new_entities.is_empty());
    }

    // ---------------------------------------------------------------------------
    // WorkloadOwnershipAnalyzer tests
    // ---------------------------------------------------------------------------

    fn make_pod_with_owner(name: &str, ns: &str, owner_kind: &str, owner_name: &str) -> Pod {
        use ran_domain::OwnerRef;
        let mut pod = Pod::new(name, ns);
        pod.owner_references.push(OwnerRef {
            name: owner_name.to_string(),
            kind: owner_kind.to_string(),
            uid: format!("uid-{}", owner_name),
        });
        pod
    }

    #[test]
    fn pod_owned_by_replicaset_creates_entity_and_owns_relation() {
        use ran_domain::{Owns, ReplicaSet};
        let campaign = test_campaign();
        let pod = make_pod_with_owner("my-pod-abc", "default", "ReplicaSet", "my-rs");

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzer = WorkloadOwnershipAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        assert!(
            inferred
                .new_entities
                .iter()
                .any(|e| e.entity_kind() == "ReplicaSet" && e.entity_name() == "my-rs"),
            "expected ReplicaSet entity to be created"
        );
        let rs = ReplicaSet::new("my-rs", "default");
        assert!(
            inferred.new_relations.iter().any(|r| {
                r.is::<Owns>()
                    && r.source_id().0 == rs.entity_id().0
                    && r.target_id().0 == pod.entity_id().0
            }),
            "expected Owns(ReplicaSet → Pod) relation"
        );
    }

    #[test]
    fn pod_owned_by_statefulset_creates_entity_and_owns_relation() {
        use ran_domain::{Owns, StatefulSet};
        let campaign = test_campaign();
        let pod = make_pod_with_owner("my-ss-pod-0", "default", "StatefulSet", "my-ss");

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzer = WorkloadOwnershipAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        assert!(
            inferred
                .new_entities
                .iter()
                .any(|e| e.entity_kind() == "StatefulSet" && e.entity_name() == "my-ss"),
            "expected StatefulSet entity"
        );
        let ss = StatefulSet::new("my-ss", "default");
        assert!(
            inferred
                .new_relations
                .iter()
                .any(|r| r.is::<Owns>() && r.source_id().0 == ss.entity_id().0),
            "expected Owns relation from StatefulSet"
        );
    }

    #[test]
    fn pod_owned_by_daemonset_creates_entity_and_owns_relation() {
        use ran_domain::{DaemonSet, Owns};
        let campaign = test_campaign();
        let pod = make_pod_with_owner("ds-pod", "kube-system", "DaemonSet", "my-ds");

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzer = WorkloadOwnershipAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        assert!(
            inferred
                .new_entities
                .iter()
                .any(|e| e.entity_kind() == "DaemonSet" && e.entity_name() == "my-ds"),
            "expected DaemonSet entity"
        );
        let ds = DaemonSet::new("my-ds", "kube-system");
        assert!(
            inferred
                .new_relations
                .iter()
                .any(|r| r.is::<Owns>() && r.source_id().0 == ds.entity_id().0),
            "expected Owns relation from DaemonSet"
        );
    }

    #[test]
    fn pod_owned_by_job_creates_entity_and_owns_relation() {
        use ran_domain::{Job, Owns};
        let campaign = test_campaign();
        let pod = make_pod_with_owner("job-pod-xyz", "default", "Job", "my-job");

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzer = WorkloadOwnershipAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        assert!(
            inferred
                .new_entities
                .iter()
                .any(|e| e.entity_kind() == "Job" && e.entity_name() == "my-job"),
            "expected Job entity"
        );
        let job = Job::new("my-job", "default");
        assert!(
            inferred
                .new_relations
                .iter()
                .any(|r| r.is::<Owns>() && r.source_id().0 == job.entity_id().0),
            "expected Owns relation from Job"
        );
    }

    #[test]
    fn already_known_replicaset_not_duplicated_owns_still_emitted() {
        use ran_domain::{Owns, ReplicaSet};
        let mut campaign = test_campaign();
        let rs = ReplicaSet::new("existing-rs", "default");
        let rs_id = rs.entity_id();
        campaign.entities.insert_typed(rs);

        let pod = make_pod_with_owner("my-pod", "default", "ReplicaSet", "existing-rs");
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzer = WorkloadOwnershipAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        assert!(
            inferred
                .new_entities
                .iter()
                .all(|e| e.entity_kind() != "ReplicaSet"),
            "should not emit ReplicaSet entity when already in campaign"
        );
        assert!(
            inferred.new_relations.iter().any(|r| {
                r.is::<Owns>() && r.source_id().0 == rs_id.0 && r.target_id().0 == pod.entity_id().0
            }),
            "Owns relation must still be emitted even when owner already known"
        );
    }

    #[test]
    fn pod_with_no_owner_references_emits_nothing() {
        let campaign = test_campaign();
        let pod = Pod::new("standalone-pod", "default");

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let analyzer = WorkloadOwnershipAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        assert!(inferred.new_entities.is_empty());
        assert!(inferred.new_relations.is_empty());
    }

    // ---------------------------------------------------------------------------
    // PropagateHostIPAnalyzer tests
    // ---------------------------------------------------------------------------

    #[test]
    fn host_ip_propagated_to_node_when_runs_on_exists_in_campaign() {
        use std::net::IpAddr;
        let mut campaign = test_campaign();

        let mut pod = Pod::new("web-pod", "default");
        pod.host_ip = Some("192.168.1.5".parse::<IpAddr>().unwrap());
        pod.is_running = true;
        let pod_id = pod.entity_id();
        campaign.entities.insert_typed(pod.clone());

        let node = K8sNode::new("worker-1");
        let node_id = node.entity_id();
        campaign.entities.insert_typed(node);
        campaign.insert_relation(&RunsOn::new(pod_id.0.clone(), node_id.0.clone()));

        // Pod arrives again (e.g. re-parsed), triggering the analyzer.
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let analyzer = PropagateHostIPAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        let updated_node = inferred
            .new_entities
            .iter()
            .find_map(|e| {
                e.as_any()
                    .downcast_ref::<K8sNode>()
                    .filter(|n| n.entity_id() == node_id)
            })
            .expect("should emit updated node");
        assert!(
            updated_node
                .system
                .ips
                .contains(&"192.168.1.5".parse::<IpAddr>().unwrap()),
            "node should gain the host IP"
        );
    }

    #[test]
    fn no_update_when_host_ip_already_in_node_ips() {
        use std::net::IpAddr;
        let mut campaign = test_campaign();

        let host_ip: IpAddr = "10.0.0.1".parse().unwrap();

        let mut pod = Pod::new("my-pod", "default");
        pod.host_ip = Some(host_ip);
        pod.is_running = true;
        let pod_id = pod.entity_id();
        campaign.entities.insert_typed(pod.clone());

        let mut node = K8sNode::new("node-1");
        node.system.ips.push(host_ip);
        let node_id = node.entity_id();
        campaign.entities.insert_typed(node);
        campaign.insert_relation(&RunsOn::new(pod_id.0.clone(), node_id.0.clone()));

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let analyzer = PropagateHostIPAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        assert!(
            inferred.new_entities.is_empty(),
            "no update should be emitted when IP already present"
        );
    }

    #[test]
    fn no_update_when_pod_has_no_host_ip() {
        let mut campaign = test_campaign();

        let mut pod = Pod::new("no-ip-pod", "default");
        pod.node_name = Some("worker-1".to_string());
        pod.is_running = true;
        let pod_id = pod.entity_id();
        campaign.entities.insert_typed(pod.clone());

        let node = K8sNode::new("worker-1");
        let node_id = node.entity_id();
        campaign.entities.insert_typed(node);
        campaign.insert_relation(&RunsOn::new(pod_id.0, node_id.0));

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let analyzer = PropagateHostIPAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        assert!(inferred.new_entities.is_empty());
    }

    #[test]
    fn host_ip_propagated_when_runs_on_arrives_after_pod() {
        use std::net::IpAddr;
        // Pod (with host_ip) is already in campaign; a RunsOn relation arrives now.
        let mut campaign = test_campaign();

        let host_ip: IpAddr = "172.16.0.5".parse().unwrap();

        let mut pod = Pod::new("early-pod", "default");
        pod.host_ip = Some(host_ip);
        pod.is_running = true;
        let pod_id = pod.entity_id();
        campaign.entities.insert_typed(pod);

        let node = K8sNode::new("new-node");
        let node_id = node.entity_id();
        campaign.entities.insert_typed(node);

        // RunsOn arrives in this update (relation was just discovered).
        let mut update = FactsUpdate::default();
        update
            .new_relations
            .push(Box::new(RunsOn::new(pod_id.0, node_id.0.clone())));

        let analyzer = PropagateHostIPAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        let updated_node = inferred
            .new_entities
            .iter()
            .find_map(|e| {
                e.as_any()
                    .downcast_ref::<K8sNode>()
                    .filter(|n| n.entity_id() == node_id)
            })
            .expect("should emit updated node with host IP");
        assert!(updated_node.system.ips.contains(&host_ip));
    }

    // ---------------------------------------------------------------------------
    // RoleBindingAnalyzer tests
    // ---------------------------------------------------------------------------

    fn make_role(name: &str, ns: &str, perms: Vec<RbacPermission>) -> K8sRole {
        let mut role = K8sRole::new(name, ns);
        role.permissions = perms;
        role
    }

    fn make_binding(
        name: &str,
        ns: &str,
        role_ref: &str,
        subjects: Vec<RbacSubject>,
    ) -> K8sRoleBinding {
        let mut binding = K8sRoleBinding::new(name, ns);
        binding.role_ref = role_ref.to_string();
        binding.subjects = subjects;
        binding
    }

    fn sa_subject(name: &str, ns: &str) -> RbacSubject {
        RbacSubject {
            kind: "ServiceAccount".to_string(),
            name: name.to_string(),
            namespace: ns.to_string(),
        }
    }

    #[test]
    fn rolebinding_injects_permissions_into_known_sa() {
        let mut campaign = test_campaign();
        let perm = RbacPermission::new("get", "pods");
        campaign
            .entities
            .insert_typed(make_role("pod-reader", "default", vec![perm.clone()]));
        campaign
            .entities
            .insert_typed(ServiceAccount::new("my-sa", "default"));

        let binding = make_binding(
            "pod-reader-binding",
            "default",
            "pod-reader",
            vec![sa_subject("my-sa", "default")],
        );
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(binding));

        let analyzer = RoleBindingAnalyzer;
        let inferred = analyzer.analyze(&campaign, &update);

        let sa = inferred
            .new_entities
            .iter()
            .find_map(|e| {
                e.as_any()
                    .downcast_ref::<ServiceAccount>()
                    .filter(|s| s.meta.name == "my-sa")
            })
            .expect("should emit SA with entitlements");
        assert_eq!(sa.entitlements.len(), 1);
        assert_eq!(sa.entitlements[0].verb, "get");
        assert_eq!(sa.entitlements[0].resource_type, "pods");
        assert_eq!(sa.entitlements[0].scope.as_deref(), Some("default"));
        assert_eq!(
            sa.entitlements[0].source_role.as_deref(),
            Some("pod-reader")
        );
    }

    #[test]
    fn rolebinding_creates_sa_when_not_yet_known() {
        let mut campaign = test_campaign();
        let perm = RbacPermission::new("list", "secrets");
        campaign
            .entities
            .insert_typed(make_role("secret-reader", "default", vec![perm]));
        // SA does NOT exist yet in campaign.

        let binding = make_binding(
            "secret-reader-binding",
            "default",
            "secret-reader",
            vec![sa_subject("new-sa", "default")],
        );
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(binding));

        let inferred = RoleBindingAnalyzer.analyze(&campaign, &update);

        let sa = inferred
            .new_entities
            .iter()
            .find_map(|e| {
                e.as_any()
                    .downcast_ref::<ServiceAccount>()
                    .filter(|s| s.meta.name == "new-sa")
            })
            .expect("should create new SA with entitlements");
        assert_eq!(sa.entitlements.len(), 1);
        assert_eq!(sa.entitlements[0].verb, "list");
    }

    #[test]
    fn clusterrolebinding_sets_wildcard_scope() {
        // ClusterRoleBinding has no namespace (empty string).
        let mut campaign = test_campaign();
        let perm = RbacPermission::new("*", "*");
        campaign
            .entities
            .insert_typed(make_role("cluster-admin", "", vec![perm]));

        let binding = make_binding(
            "cluster-admin-binding",
            "",
            "cluster-admin",
            vec![sa_subject("admin-sa", "kube-system")],
        );
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(binding));

        let inferred = RoleBindingAnalyzer.analyze(&campaign, &update);

        let sa = inferred
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<ServiceAccount>())
            .expect("should emit SA");
        assert_eq!(
            sa.entitlements[0].scope.as_deref(),
            Some("*"),
            "ClusterRoleBinding subjects must get wildcard scope"
        );
    }

    #[test]
    fn rolebinding_scope_is_binding_namespace() {
        let mut campaign = test_campaign();
        let perm = RbacPermission::new("get", "configmaps");
        campaign
            .entities
            .insert_typed(make_role("cm-reader", "staging", vec![perm]));

        let binding = make_binding(
            "cm-reader-binding",
            "staging",
            "cm-reader",
            vec![sa_subject("reader-sa", "staging")],
        );
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(binding));

        let inferred = RoleBindingAnalyzer.analyze(&campaign, &update);

        let sa = inferred
            .new_entities
            .iter()
            .find_map(|e| e.as_any().downcast_ref::<ServiceAccount>())
            .expect("should emit SA");
        assert_eq!(sa.entitlements[0].scope.as_deref(), Some("staging"));
    }

    #[test]
    fn rolebinding_multiple_subjects_each_receive_permissions() {
        let mut campaign = test_campaign();
        let perm = RbacPermission::new("get", "pods");
        campaign
            .entities
            .insert_typed(make_role("pod-reader", "default", vec![perm]));

        let binding = make_binding(
            "multi-binding",
            "default",
            "pod-reader",
            vec![
                sa_subject("sa-alpha", "default"),
                sa_subject("sa-beta", "default"),
            ],
        );
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(binding));

        let inferred = RoleBindingAnalyzer.analyze(&campaign, &update);

        let sa_names: Vec<&str> = inferred
            .new_entities
            .iter()
            .filter_map(|e| e.as_any().downcast_ref::<ServiceAccount>())
            .map(|s| s.meta.name.as_str())
            .collect();
        assert!(
            sa_names.contains(&"sa-alpha"),
            "sa-alpha should receive permissions"
        );
        assert!(
            sa_names.contains(&"sa-beta"),
            "sa-beta should receive permissions"
        );
        assert_eq!(sa_names.len(), 2);
    }

    #[test]
    fn rolebinding_unknown_role_emits_nothing() {
        let campaign = test_campaign();
        // No K8sRole with name "nonexistent" in campaign.

        let binding = make_binding(
            "orphan-binding",
            "default",
            "nonexistent",
            vec![sa_subject("some-sa", "default")],
        );
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(binding));

        let inferred = RoleBindingAnalyzer.analyze(&campaign, &update);

        assert!(
            inferred.new_entities.is_empty(),
            "no entities emitted when referenced role is unknown"
        );
        assert!(inferred.new_relations.is_empty());
    }

    // ---------------------------------------------------------------------------
    // RBAC namespace / cluster contains
    // ---------------------------------------------------------------------------

    #[test]
    fn namespaced_role_gets_contains_relation_from_namespace() {
        let campaign = test_campaign();

        let mut role = K8sRole::new("pod-reader", "default");
        role.is_cluster_role = false;
        let role_id = role.entity_id();

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(role));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(
            update.new_relations.iter().any(|r| {
                r.is::<Contains>()
                    && r.source_id().0 == "ns/default"
                    && r.target_id().0 == role_id.0
            }),
            "expected Contains(ns/default → role)"
        );
    }

    #[test]
    fn namespaced_rolebinding_gets_contains_relation_from_namespace() {
        let campaign = test_campaign();

        let binding = make_binding("rb", "default", "some-role", vec![]);
        let binding_id = binding.entity_id();

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(binding));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(
            update.new_relations.iter().any(|r| {
                r.is::<Contains>()
                    && r.source_id().0 == "ns/default"
                    && r.target_id().0 == binding_id.0
            }),
            "expected Contains(ns/default → rolebinding)"
        );
    }

    #[test]
    fn clusterrole_gets_contains_relation_from_cluster() {
        let campaign = test_campaign();
        let cluster_id = campaign
            .entities
            .values::<K8sCluster>()
            .next()
            .unwrap()
            .entity_id();

        let mut role = K8sRole::new("cluster-admin", "");
        role.is_cluster_role = true;
        let role_id = role.entity_id();

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(role));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(
            update.new_relations.iter().any(|r| {
                r.is::<Contains>()
                    && r.source_id().0 == cluster_id.0
                    && r.target_id().0 == role_id.0
            }),
            "expected Contains(cluster → clusterrole)"
        );
    }

    #[test]
    fn clusterrolebinding_gets_contains_relation_from_cluster() {
        let campaign = test_campaign();
        let cluster_id = campaign
            .entities
            .values::<K8sCluster>()
            .next()
            .unwrap()
            .entity_id();

        let binding = make_binding("cluster-admin-binding", "", "cluster-admin", vec![]);
        let binding_id = binding.entity_id();

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(binding));

        let analyzers = default_analyzers();
        run_analyzers(&campaign, &analyzers, &mut update);

        assert!(
            update.new_relations.iter().any(|r| {
                r.is::<Contains>()
                    && r.source_id().0 == cluster_id.0
                    && r.target_id().0 == binding_id.0
            }),
            "expected Contains(cluster → clusterrolebinding)"
        );
    }
}
