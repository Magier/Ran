use ran_domain::{
    BindsTo, Confidence, Contains, DaemonSet, Deployment, Entity, EntityId, GCPServiceAccount,
    Grants, Job, K8sCluster, K8sCredential, K8sGateway, K8sHTTPRoute, K8sIngress, K8sNode, K8sRole,
    K8sRoleBinding, K8sService, KubeletExecSink, KubeletExecSource, NameConfidence, Namespace,
    Owns, Pod, PodExec, RbacPermission, RunsOn, ServiceAccount, StatefulSet, UnknownSystem, Uses,
};

use crate::rules::InferenceRule;
use crate::{Campaign, FactsUpdate, PendingView};

// ---------------------------------------------------------------------------
// Built-in analyzers
// ---------------------------------------------------------------------------

/// For every new `Pod`, ensure the namespace entity exists and wire a
/// `contains` relation from the Namespace to the Pod.
pub struct PodNamespaceAnalyzer;

impl InferenceRule for PodNamespaceAnalyzer {
    fn name(&self) -> &'static str {
        "pod.namespace"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

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

            let (ns_id, new_ns) = view.ensure_namespace(ns_name);
            if let Some(ns) = new_ns {
                inferred.new_entities.push(Box::new(ns));
            }

            // When the pod has an owner (ReplicaSet, StatefulSet, etc.) the
            // hierarchy goes Namespace → Workload → Pod via the Owns relation.
            // WorkloadOwnershipAnalyzer emits Contains(ns → workload), so we
            // must not also wire Contains(ns → pod) or the pod appears twice.
            if pod.owner_references.is_empty() {
                inferred.new_relations.push(Box::new(Contains::new(
                    ns_id.0.clone(),
                    pod.entity_id().0.clone(),
                )));
            }
        }

        inferred
    }
}

/// For every new `ServiceAccount`, ensure its namespace entity exists and wire
/// a `contains` relation from the Namespace to the ServiceAccount.
pub struct ServiceAccountNamespaceAnalyzer;

impl InferenceRule for ServiceAccountNamespaceAnalyzer {
    fn name(&self) -> &'static str {
        "serviceaccount.namespace"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

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

            let (ns_id, new_ns) = view.ensure_namespace(ns_name);
            if let Some(ns) = new_ns {
                inferred.new_entities.push(Box::new(ns));
            }
            inferred.new_relations.push(Box::new(Contains::new(
                ns_id.0.clone(),
                sa.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

/// For every new `Namespace`, wire a `contains` relation from the single known
/// cluster to that namespace.  If no cluster is known yet the relation is
/// silently skipped (the namespace will be re-linked once the cluster is
/// discovered).
pub struct NamespaceClusterAnalyzer;

impl InferenceRule for NamespaceClusterAnalyzer {
    fn name(&self) -> &'static str {
        "namespace.cluster"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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

impl InferenceRule for PodNodeAnalyzer {
    fn name(&self) -> &'static str {
        "pod.node"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

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
            if !view.contains::<K8sNode>(&node_id) {
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

impl InferenceRule for ServiceAccountAnalyzer {
    fn name(&self) -> &'static str {
        "pod.serviceaccount"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

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
            if !view.contains::<ServiceAccount>(&sa_id) {
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

impl InferenceRule for ServiceAccountTokenAnalyzer {
    fn name(&self) -> &'static str {
        "serviceaccount.token"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        for entity in &update.new_entities {
            let Some(sa) = entity.as_any().downcast_ref::<ServiceAccount>() else {
                continue;
            };
            let Some(token) = &sa.token else { continue };
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
            pod.is_running = true;

            let pod_id = pod.entity_id();
            let sa_id = sa.entity_id();

            if !view.contains::<Pod>(&pod_id) {
                inferred.new_entities.push(Box::new(pod));
            }
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

impl InferenceRule for ServiceAccountCanExecAnalyzer {
    fn name(&self) -> &'static str {
        "serviceaccount.can-exec"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        let service_accounts = view.collect::<ServiceAccount>();
        let pods = view.collect::<Pod>();

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

impl InferenceRule for KubeletExecSinkAnalyzer {
    fn name(&self) -> &'static str {
        "kubelet.exec-sink"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        let pods = view
            .collect::<Pod>()
            .into_iter()
            .map(|p| (p.entity_id().0.clone(), p))
            .collect::<std::collections::HashMap<_, _>>();

        let relations = view.relations();
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

impl InferenceRule for HostPathAnalyzer {
    fn name(&self) -> &'static str {
        "pod.host-path"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        for pod in view.collect::<Pod>() {
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
// KubeletMountAnalyzer helpers
// ---------------------------------------------------------------------------

fn is_valid_pod_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8usize, 4, 4, 4, 12];
    parts
        .iter()
        .zip(expected_lens.iter())
        .all(|(p, &len)| p.len() == len && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_generic_volume_name(name: &str) -> bool {
    name.starts_with("kube-api-access-")
}

fn longest_common_prefix(names: &[&str]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let first = names[0];
    let common_len = first
        .char_indices()
        .take_while(|&(i, c)| names.iter().all(|&n| n.get(i..).unwrap_or("").starts_with(c)))
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    first[..common_len].to_string()
}

fn derive_pod_display_name(uid: &str, volume_names: &[String]) -> String {
    let first_segment = uid.split('-').next().unwrap_or(uid);

    let non_generic: Vec<&str> = volume_names
        .iter()
        .filter(|n| !is_generic_volume_name(n))
        .map(|s| s.as_str())
        .collect();

    if non_generic.is_empty() {
        return first_segment.to_string();
    }

    let lcp = longest_common_prefix(&non_generic);
    if lcp.is_empty() {
        return first_segment.to_string();
    }

    if lcp.ends_with('-') {
        format!("{}{}", lcp, first_segment)
    } else {
        format!("{}-{}", lcp, first_segment)
    }
}

// ---------------------------------------------------------------------------
// KubeletMountAnalyzer
// ---------------------------------------------------------------------------

/// Discovers pods running on the same node as the observing pod by parsing
/// kubelet volume mount paths from `system.mounts`.
///
/// For every mount path matching `/var/lib/kubelet/pods/{uid}/volumes/{type}/{name}`,
/// it extracts the pod UID, groups volume names per UID, derives a display name,
/// and emits a `Pod` entity with namespace `"?"` and a `RunsOn` relation to
/// the same node the observing pod runs on.
pub struct KubeletMountAnalyzer;

impl InferenceRule for KubeletMountAnalyzer {
    fn name(&self) -> &'static str {
        "kubelet.mount-pods"
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        for pod in view.collect::<Pod>() {
            // Group volume names by pod UID discovered from kubelet mount paths.
            let mut pods_by_uid: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();

            for mount in &pod.system.mounts {
                let Some(rest) = mount.mount_point.strip_prefix("/var/lib/kubelet/pods/") else {
                    continue;
                };

                let parts: Vec<&str> = rest.split('/').collect();
                if parts.len() < 4 {
                    tracing::warn!(
                        mount_point = %mount.mount_point,
                        "kubelet mount path has fewer than 4 segments after prefix, skipping"
                    );
                    continue;
                }

                let uid_str = parts[0];
                if !is_valid_pod_uuid(uid_str) {
                    tracing::warn!(
                        mount_point = %mount.mount_point,
                        uid = uid_str,
                        "kubelet mount path has non-UUID pod segment, skipping"
                    );
                    continue;
                }

                let vol_name = parts[3].to_string();
                pods_by_uid
                    .entry(uid_str.to_string())
                    .or_default()
                    .push(vol_name);
            }

            if pods_by_uid.is_empty() {
                continue;
            }

            // Determine node (same logic as HostPathAnalyzer).
            let node_name = pod.node_name.as_deref().unwrap_or("?");
            let node = K8sNode::new(node_name);
            let node_id = node.entity_id();
            let node_known = campaign.entities.contains::<K8sNode>(&node_id)
                || update.new_entities.iter().any(|e| e.entity_id() == node_id)
                || inferred.new_entities.iter().any(|e| e.entity_id() == node_id);
            if !node_known {
                inferred.new_entities.push(Box::new(node));
            }

            // Emit one Pod entity + RunsOn per discovered pod UID.
            for (uid, vol_names) in pods_by_uid {
                let display_name = derive_pod_display_name(&uid, &vol_names);
                let mut discovered = Pod::new(&display_name, "?");
                discovered.meta.uid = Some(uid);

                let discovered_id = discovered.entity_id();
                let already_known = campaign.entities.contains::<Pod>(&discovered_id)
                    || update
                        .new_entities
                        .iter()
                        .any(|e| e.entity_id() == discovered_id)
                    || inferred
                        .new_entities
                        .iter()
                        .any(|e| e.entity_id() == discovered_id);

                if already_known {
                    continue;
                }

                inferred.new_relations.push(Box::new(RunsOn::new(
                    discovered_id.0.clone(),
                    node_id.0.clone(),
                )));
                inferred.new_entities.push(Box::new(discovered));
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

impl InferenceRule for CanExecAccessAnalyzer {
    fn name(&self) -> &'static str {
        "kubelet.can-exec-access"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        let exec_target_ids: Vec<ran_domain::EntityId> = update
            .new_relations
            .iter()
            .filter(|r| r.is_exec_channel())
            .map(|r| r.target_id().clone())
            .collect();

        for eid in exec_target_ids {
            // Determine whether the target is a Pod or Node and read its current
            // access level. Skip if it is neither (not a system entity).
            let (is_pod, current_level): (bool, ran_domain::AccessLevel) =
                if let Some(pod) = view.find::<ran_domain::Pod>(&eid) {
                    (true, pod.system.access_level)
                } else if let Some(node) = view.find::<ran_domain::K8sNode>(&eid) {
                    (false, node.system.access_level)
                } else {
                    continue;
                };

            // Only emit if access_level is not already Exec — the merge takes
            // max, so this is a no-op for already-Exec entities, but skipping
            // avoids a needless clone.
            if current_level >= ran_domain::AccessLevel::Exec {
                continue;
            }

            // Emit a cloned entity with access_level = Exec. When committed via
            // apply_facts → insert_entity → merge_from the max(access_level)
            // rule raises the stored level.
            if is_pod {
                let mut full_pod = view.find_or_stub::<ran_domain::Pod>(&eid, || {
                    let name = eid.0.rsplit('/').next().unwrap_or(&eid.0);
                    ran_domain::Pod::new(name, "")
                });
                full_pod.system.access_level = ran_domain::AccessLevel::Exec;
                inferred.new_entities.push(Box::new(full_pod));
            } else {
                let mut full_node = view.find_or_stub::<ran_domain::K8sNode>(&eid, || {
                    let name = eid.0.strip_prefix("node/").unwrap_or(&eid.0);
                    ran_domain::K8sNode::new(name)
                });
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
/// - `ReplicaSet` → strips the trailing hash suffix and creates a `Deployment` entity + `Owns(Deployment → Pod)`
/// - `StatefulSet` → creates a `StatefulSet` entity + `Owns(StatefulSet → Pod)`
/// - `DaemonSet` → creates a `DaemonSet` entity + `Owns(DaemonSet → Pod)`
/// - `Job` → creates a `Job` entity + `Owns(Job → Pod)`
///
/// Trigger: new `Pod` entities with non-empty `owner_references`.
pub struct WorkloadOwnershipAnalyzer;

impl InferenceRule for WorkloadOwnershipAnalyzer {
    fn name(&self) -> &'static str {
        "workload.ownership"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

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
                        let deploy_name = owner_ref
                            .name
                            .rsplit_once('-')
                            .map(|(prefix, _)| prefix)
                            .unwrap_or(&owner_ref.name);
                        let deploy = Deployment::new(deploy_name, &ns);
                        let deploy_id = deploy.entity_id();
                        if !view.contains::<Deployment>(&deploy_id)
                            && !inferred
                                .new_entities
                                .iter()
                                .any(|e| e.entity_id() == deploy_id)
                        {
                            inferred.new_entities.push(Box::new(deploy));
                            inferred.new_relations.push(Box::new(Contains::new(
                                format!("ns/{}", ns),
                                deploy_id.0.clone(),
                            )));
                        }
                        inferred
                            .new_relations
                            .push(Box::new(Owns::new(deploy_id.0, pod_id.clone())));
                    }
                    "StatefulSet" => {
                        let ss = StatefulSet::new(&owner_ref.name, &ns);
                        let ss_id = ss.entity_id();
                        if !view.contains::<StatefulSet>(&ss_id)
                            && !inferred.new_entities.iter().any(|e| e.entity_id() == ss_id)
                        {
                            inferred.new_entities.push(Box::new(ss));
                            inferred.new_relations.push(Box::new(Contains::new(
                                format!("ns/{}", ns),
                                ss_id.0.clone(),
                            )));
                        }
                        inferred
                            .new_relations
                            .push(Box::new(Owns::new(ss_id.0, pod_id.clone())));
                    }
                    "DaemonSet" => {
                        let ds = DaemonSet::new(&owner_ref.name, &ns);
                        let ds_id = ds.entity_id();
                        if !view.contains::<DaemonSet>(&ds_id)
                            && !inferred.new_entities.iter().any(|e| e.entity_id() == ds_id)
                        {
                            inferred.new_entities.push(Box::new(ds));
                            inferred.new_relations.push(Box::new(Contains::new(
                                format!("ns/{}", ns),
                                ds_id.0.clone(),
                            )));
                        }
                        inferred
                            .new_relations
                            .push(Box::new(Owns::new(ds_id.0, pod_id.clone())));
                    }
                    "Job" => {
                        let job = Job::new(&owner_ref.name, &ns);
                        let job_id = job.entity_id();
                        if !view.contains::<Job>(&job_id)
                            && !inferred
                                .new_entities
                                .iter()
                                .any(|e| e.entity_id() == job_id)
                        {
                            inferred.new_entities.push(Box::new(job));
                            inferred.new_relations.push(Box::new(Contains::new(
                                format!("ns/{}", ns),
                                job_id.0.clone(),
                            )));
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
/// 1. **New Pod with `host_ip` newly discovered** — fires only when the pod was
///    not previously in the campaign, or it was present but without `host_ip`.
///    Re-parsing a pod whose `host_ip` was already recorded is a no-op.  If a
///    `runs-on` edge exists in the campaign graph (or in the current update from
///    `PodNodeAnalyzer`), the IP is propagated to the target node immediately.
/// 2. **New `runs-on` relation** — if the source pod (in campaign state or the
///    current update) already has `host_ip` set, the IP is propagated to the
///    target node.  This covers the case where the pod entity arrives first,
///    a `runs-on` edge is later wired (e.g. by `PodNodeAnalyzer`), and
///    `PropagateHostIPAnalyzer` runs after them in the same pipeline pass.
///
/// No update is emitted when the IP is already present in the node's `system.ips`.
pub struct PropagateHostIPAnalyzer;

impl InferenceRule for PropagateHostIPAnalyzer {
    fn name(&self) -> &'static str {
        "pod.host-ip"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        // --- Case 1: new Pod with host_ip ----------------------------------
        for entity in &update.new_entities {
            let Some(pod) = entity.as_any().downcast_ref::<Pod>() else {
                continue;
            };
            let Some(host_ip) = pod.host_ip else { continue };

            let pod_id = pod.entity_id();

            // Only propagate when host_ip is newly discovered on this pod: skip
            // if the campaign already recorded host_ip for it (a re-parse with
            // no new information should not re-trigger propagation).
            if campaign
                .entities
                .find::<Pod>(&pod_id)
                .and_then(|p| p.host_ip)
                .is_some()
            {
                continue;
            }

            for node_id in campaign
                .graph
                .targets_of(&pod_id, "runs-on")
                .into_iter()
                .cloned()
            {
                propagate_ip_to_node(&mut inferred, &view, &node_id, host_ip);
            }
            for rel in &update.new_relations {
                if rel.relation_name() == "runs-on" && rel.source_id() == &pod_id {
                    propagate_ip_to_node(&mut inferred, &view, rel.target_id(), host_ip);
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

            let Some(host_ip) = view.find::<Pod>(pod_id).and_then(|p| p.host_ip) else {
                continue;
            };
            propagate_ip_to_node(&mut inferred, &view, &node_id, host_ip);
        }

        inferred
    }
}

/// Emit an updated `K8sNode` with `host_ip` added to `system.ips`, unless the
/// IP is already present in the node's stored IPs or in a pending inferred update.
fn propagate_ip_to_node(
    inferred: &mut FactsUpdate,
    view: &PendingView<'_>,
    node_id: &EntityId,
    host_ip: std::net::IpAddr,
) {
    // Already committed to campaign state?
    if let Some(node) = view.find::<K8sNode>(node_id) {
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

    let mut node = view.find_or_stub::<K8sNode>(node_id, || {
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

impl InferenceRule for ClusterRoleClusterAnalyzer {
    fn name(&self) -> &'static str {
        "clusterrole.cluster"
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

/// For every new `K8sRoleBinding` with an empty namespace (i.e. a
/// ClusterRoleBinding), wire a `contains` relation from the cluster to the
/// ClusterRoleBinding.
pub struct ClusterRoleBindingClusterAnalyzer;

impl InferenceRule for ClusterRoleBindingClusterAnalyzer {
    fn name(&self) -> &'static str {
        "clusterrolebinding.cluster"
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

/// For every new namespace-scoped `K8sRole`, ensure its namespace entity exists
/// and wire a `contains` relation from the Namespace to the Role.
pub struct RoleNamespaceAnalyzer;

impl InferenceRule for RoleNamespaceAnalyzer {
    fn name(&self) -> &'static str {
        "role.namespace"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

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

            let (ns_id, new_ns) = view.ensure_namespace(ns_name);
            if let Some(ns) = new_ns {
                inferred.new_entities.push(Box::new(ns));
            }
            inferred.new_relations.push(Box::new(Contains::new(
                ns_id.0.clone(),
                role.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

/// For every new namespace-scoped `K8sRoleBinding`, ensure its namespace entity
/// exists and wire a `contains` relation from the Namespace to the RoleBinding.
pub struct RoleBindingNamespaceAnalyzer;

impl InferenceRule for RoleBindingNamespaceAnalyzer {
    fn name(&self) -> &'static str {
        "rolebinding.namespace"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

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

            let (ns_id, new_ns) = view.ensure_namespace(ns_name);
            if let Some(ns) = new_ns {
                inferred.new_entities.push(Box::new(ns));
            }
            inferred.new_relations.push(Box::new(Contains::new(
                ns_id.0.clone(),
                binding.entity_id().0.clone(),
            )));
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// Generic namespace-contains analyzer macro
// ---------------------------------------------------------------------------

/// Generates a namespace-contains analyzer for a namespaced K8s resource type.
///
/// Every namespaced resource must have a `Contains(namespace → resource)`
/// relation in the graph.  The logic is identical across types — only the
/// concrete type and struct name differ — so this macro eliminates the
/// boilerplate.
macro_rules! ns_contains_analyzer {
    ($analyzer:ident, $entity_type:ty, $rule_name:literal) => {
        pub struct $analyzer;

        impl InferenceRule for $analyzer {
            fn name(&self) -> &'static str {
                $rule_name
            }
            fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
                let mut inferred = FactsUpdate::default();
                let view = PendingView::new(campaign, update);
                for entity in &update.new_entities {
                    let Some(e) = entity.as_any().downcast_ref::<$entity_type>() else {
                        continue;
                    };
                    let Some(ns_name) = e.namespace() else {
                        continue;
                    };
                    if ns_name.is_empty() {
                        continue;
                    }
                    let (ns_id, new_ns) = view.ensure_namespace(ns_name);
                    if let Some(ns) = new_ns {
                        inferred.new_entities.push(Box::new(ns));
                    }
                    inferred.new_relations.push(Box::new(Contains::new(
                        ns_id.0.clone(),
                        e.entity_id().0.clone(),
                    )));
                }
                inferred
            }
        }
    };
}

ns_contains_analyzer!(ServiceNamespaceAnalyzer, K8sService, "service.namespace");
ns_contains_analyzer!(IngressNamespaceAnalyzer, K8sIngress, "ingress.namespace");
ns_contains_analyzer!(GatewayNamespaceAnalyzer, K8sGateway, "gateway.namespace");
ns_contains_analyzer!(
    HTTPRouteNamespaceAnalyzer,
    K8sHTTPRoute,
    "httproute.namespace"
);

// ---------------------------------------------------------------------------
// Default analyzer pipeline
// ---------------------------------------------------------------------------

/// For every new `K8sNode`, wire a `contains` relation from the campaign's
/// cluster — nodes always belong to the cluster they were discovered in.
pub struct NodeClusterAnalyzer;

impl InferenceRule for NodeClusterAnalyzer {
    fn name(&self) -> &'static str {
        "node.cluster"
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

impl InferenceRule for RoleBindingAnalyzer {
    fn name(&self) -> &'static str {
        "rolebinding.permissions"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
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

impl InferenceRule for GCPServiceAccountAnalyzer {
    fn name(&self) -> &'static str {
        "serviceaccount.gcp"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        let gcp_sas: Vec<GCPServiceAccount> = campaign
            .entities
            .values::<GCPServiceAccount>()
            .cloned()
            .collect();

        if gcp_sas.is_empty() {
            return inferred;
        }

        let pods = view.collect::<Pod>();

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

// ---------------------------------------------------------------------------
// IpBasedSystemMergeAnalyzer
// ---------------------------------------------------------------------------

/// Merge `UnknownSystem` entities into the concrete Pod or Node that shares
/// their IP address.
///
/// When a target is first reached via a network/port scan, an `UnknownSystem`
/// keyed by its IP is created.  Later, the Kubernetes API server reveals the
/// true pod/node identity including the same IP in `system.ips`.  This analyzer
/// matches the two by IP and emits an `entity_alias` so `apply_facts` can
/// transplant all relations and merge the accumulated runtime data.
///
/// Guard: a Pod's `host_ip` field is the IP of the *node* it runs on.  For
/// pods with `host_network: Yes` that IP is shared with the node and therefore
/// not a unique pod identifier.  IPs equal to `pod.host_ip` are skipped.
pub struct IpBasedSystemMergeAnalyzer;

impl InferenceRule for IpBasedSystemMergeAnalyzer {
    fn name(&self) -> &'static str {
        "system.ip-merge"
    }
    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        let unknown_systems: Vec<&UnknownSystem> =
            campaign.entities.values::<UnknownSystem>().collect();

        for entity in &update.new_entities {
            let Some(pod) = entity.as_any().downcast_ref::<Pod>() else {
                // Match new Nodes against existing UnknownSystems.
                if let Some(node) = entity.as_any().downcast_ref::<K8sNode>() {
                    let node_id = node.entity_id();
                    for unknown in &unknown_systems {
                        let unknown_id = unknown.entity_id();
                        if already_aliased(&inferred, &unknown_id) {
                            continue;
                        }
                        for &ip in &unknown.system.ips {
                            if node.system.ips.contains(&ip) {
                                tracing::info!(
                                    unknown = %unknown_id.0,
                                    node = %node_id.0,
                                    %ip,
                                    "merging UnknownSystem into Node by IP match"
                                );
                                inferred
                                    .entity_aliases
                                    .insert((unknown_id, node_id.clone()));
                                break;
                            }
                        }
                    }
                }
                continue;
            };

            let pod_id = pod.entity_id();

            // Match new Pods against existing UnknownSystems.
            for unknown in &unknown_systems {
                let unknown_id = unknown.entity_id();
                if already_aliased(&inferred, &unknown_id) {
                    continue;
                }
                for &ip in &unknown.system.ips {
                    // Skip the node IP — hostNetwork pods share it with the node.
                    if pod.host_ip == Some(ip) {
                        continue;
                    }
                    if pod.system.ips.contains(&ip) {
                        tracing::info!(
                            unknown = %unknown_id.0,
                            pod = %pod_id.0,
                            %ip,
                            "merging UnknownSystem into Pod by IP match"
                        );
                        inferred.entity_aliases.insert((unknown_id, pod_id.clone()));
                        break;
                    }
                }
            }

            // Match new authoritative Pods against existing placeholder Pods by IP.
            // Placeholder pods (name_confidence != Authoritative) are created by
            // network scanners before the real pod identity is known from the API server.
            if pod.meta.name_confidence != NameConfidence::Authoritative {
                continue;
            }
            for existing in campaign.entities.values::<Pod>() {
                let existing_id = existing.entity_id();
                if existing_id == pod_id {
                    continue;
                }
                if existing.meta.name_confidence == NameConfidence::Authoritative {
                    continue;
                }
                if already_aliased(&inferred, &existing_id) {
                    continue;
                }
                for &ip in &existing.system.ips {
                    // Guard: skip IPs that are the new pod's node IP — a hostNetwork
                    // pod shares its node IP but is distinct from the K8sNode.
                    if pod.host_ip == Some(ip) && !pod.system.ips.contains(&ip) {
                        continue;
                    }
                    if pod.system.ips.contains(&ip) {
                        tracing::info!(
                            placeholder = %existing_id.0,
                            pod = %pod_id.0,
                            %ip,
                            "merging placeholder Pod into authoritative Pod by IP match"
                        );
                        inferred
                            .entity_aliases
                            .insert((existing_id, pod_id.clone()));
                        break;
                    }
                }
            }
        }

        // Match new placeholder Pods against authoritative Pods already in campaign.
        // Handles the case where the API server was queried before the network scan.
        for entity in &update.new_entities {
            let Some(new_pod) = entity.as_any().downcast_ref::<Pod>() else {
                continue;
            };
            if new_pod.meta.name_confidence == NameConfidence::Authoritative {
                continue;
            }
            let new_pod_id = new_pod.entity_id();
            if already_aliased(&inferred, &new_pod_id) {
                continue;
            }
            for auth_pod in campaign.entities.values::<Pod>() {
                let auth_id = auth_pod.entity_id();
                if auth_pod.meta.name_confidence != NameConfidence::Authoritative {
                    continue;
                }
                for &ip in &new_pod.system.ips {
                    if auth_pod.host_ip == Some(ip) && !auth_pod.system.ips.contains(&ip) {
                        continue;
                    }
                    if auth_pod.system.ips.contains(&ip) {
                        tracing::info!(
                            placeholder = %new_pod_id.0,
                            pod = %auth_id.0,
                            %ip,
                            "merging placeholder Pod into authoritative Pod by IP match"
                        );
                        inferred
                            .entity_aliases
                            .insert((new_pod_id.clone(), auth_id.clone()));
                        break;
                    }
                }
            }
        }

        inferred
    }
}

fn already_aliased(update: &FactsUpdate, id: &EntityId) -> bool {
    update.entity_aliases.iter().any(|(stale, _)| stale == id)
}

// ---------------------------------------------------------------------------
// KubeletExecSourceAnalyzer
// ---------------------------------------------------------------------------

/// Expand `kubelet-exec(sys, all(k8s.Node))` marker relations into concrete
/// `KubeletExecSource(pod -> node)` edges for all known nodes.
pub struct KubeletExecSourceAnalyzer;

impl InferenceRule for KubeletExecSourceAnalyzer {
    fn name(&self) -> &'static str {
        "kubelet.exec-source"
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        let nodes: Vec<EntityId> = view
            .collect::<K8sNode>()
            .into_iter()
            .map(|n| n.entity_id())
            .collect();

        if nodes.is_empty() {
            return inferred;
        }

        let relations = view.relations();
        let kubelet_markers = relations
            .iter()
            .filter(|r| {
                r.name == "kubelet-exec" && r.target_id.eq_ignore_ascii_case("all(k8s.node)")
            })
            .collect::<Vec<_>>();

        for marker in kubelet_markers {
            let pod_id = EntityId::new(&marker.source_id);
            for node_id in &nodes {
                if campaign
                    .graph
                    .targets_of(&pod_id, "kubelet-exec")
                    .iter()
                    .any(|existing| *existing == node_id)
                {
                    continue;
                }

                let mut rel = KubeletExecSource::new(pod_id.0.clone(), node_id.0.clone())
                    .with_opt_envelope(marker.envelope.clone());
                if let Some(ref transform) = marker.output_transform {
                    rel = rel.with_output_transform(transform.clone());
                }
                inferred.new_relations.push(Box::new(rel));
            }
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// RoleBindingGraphAnalyzer
// ---------------------------------------------------------------------------

/// Emit `BindsTo(binding → role)` and `Grants(binding → sa)` edges for every
/// new `K8sRoleBinding` / `K8sClusterRoleBinding`.  Creates stub role and SA
/// entities when they are not yet known in the campaign so the graph stays
/// connected even if discovery runs out of order.
pub struct RoleBindingGraphAnalyzer;

impl InferenceRule for RoleBindingGraphAnalyzer {
    fn name(&self) -> &'static str {
        "rolebinding.graph"
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();
        let view = PendingView::new(campaign, update);

        for entity in &update.new_entities {
            let Some(binding) = entity.as_any().downcast_ref::<K8sRoleBinding>() else {
                continue;
            };

            let binding_id = binding.entity_id();
            let binding_ns = binding.meta.namespace.as_deref().unwrap_or("");
            let is_cluster_scoped = binding_ns.is_empty();

            let ref_is_cluster = binding.role_ref_kind.eq_ignore_ascii_case("ClusterRole")
                || (binding.role_ref_kind.is_empty() && is_cluster_scoped);

            let role_entity_id = if ref_is_cluster {
                EntityId(format!("clusterrole/{}", binding.role_ref))
            } else {
                EntityId(format!("ns/{}/role/{}", binding_ns, binding.role_ref))
            };

            if !view.contains::<K8sRole>(&role_entity_id) {
                let mut stub = K8sRole::new(&binding.role_ref, binding_ns);
                stub.is_cluster_role = ref_is_cluster;
                inferred.new_entities.push(Box::new(stub));
            }

            inferred.new_relations.push(Box::new(BindsTo::new(
                binding_id.0.clone(),
                role_entity_id.0,
            )));

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

                if !view.contains::<ServiceAccount>(&sa_entity_id) {
                    inferred
                        .new_entities
                        .push(Box::new(ServiceAccount::new(&subject.name, &sa_ns)));
                }
                inferred
                    .new_relations
                    .push(Box::new(Grants::new(binding_id.0.clone(), sa_entity_id.0)));
            }
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// KubeconfigCredentialAnalyzer
// ---------------------------------------------------------------------------

/// When a `K8sCredential` is discovered (via kubeconfig file read), derive a
/// `K8sCluster` entity from its endpoint so that follow-on TTPs that target
/// `kind: Cluster` (e.g. `use_external_kubeconfig`) become applicable.
pub struct KubeconfigCredentialAnalyzer;

impl InferenceRule for KubeconfigCredentialAnalyzer {
    fn name(&self) -> &'static str {
        "kubeconfig.credential"
    }

    fn infer(&self, campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        let mut inferred = FactsUpdate::default();

        for entity in &update.new_entities {
            let Some(cred) = entity.as_any().downcast_ref::<K8sCredential>() else {
                continue;
            };

            let cluster_name = if cred.endpoint.is_empty() {
                "discovered".to_string()
            } else {
                cred.endpoint.clone()
            };

            let cluster = K8sCluster::new(&cluster_name).with_server(Some(cred.endpoint.clone()));
            let cluster_id = cluster.entity_id();

            if !campaign.entities.contains::<K8sCluster>(&cluster_id)
                && !inferred
                    .new_entities
                    .iter()
                    .any(|e| e.entity_id() == cluster_id)
            {
                inferred.new_entities.push(Box::new(cluster));
            }
        }

        inferred
    }
}

// ---------------------------------------------------------------------------
// Default rule pipeline
// ---------------------------------------------------------------------------

/// Returns the default set of inference rules that run in the fixpoint loop.
pub fn default_rules() -> Vec<Box<dyn InferenceRule>> {
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
        Box::new(KubeletMountAnalyzer),
        Box::new(ServiceAccountCanExecAnalyzer),
        Box::new(KubeletExecSourceAnalyzer),
        Box::new(KubeletExecSinkAnalyzer),
        Box::new(CanExecAccessAnalyzer),
        Box::new(WorkloadOwnershipAnalyzer),
        Box::new(ClusterRoleClusterAnalyzer),
        Box::new(ClusterRoleBindingClusterAnalyzer),
        Box::new(RoleNamespaceAnalyzer),
        Box::new(RoleBindingNamespaceAnalyzer),
        Box::new(RoleBindingAnalyzer),
        Box::new(RoleBindingGraphAnalyzer),
        Box::new(GCPServiceAccountAnalyzer),
        Box::new(ServiceNamespaceAnalyzer),
        Box::new(IngressNamespaceAnalyzer),
        Box::new(GatewayNamespaceAnalyzer),
        Box::new(HTTPRouteNamespaceAnalyzer),
        Box::new(IpBasedSystemMergeAnalyzer),
        Box::new(KubeconfigCredentialAnalyzer),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ran_domain::{
        AccessLevel, Confidence, Contains, EntityId, K8sCluster, K8sNode, K8sRole, K8sRoleBinding,
        KubeletExecSink, KubeletExecSource, Namespace, OutputTransformKind, Pod, PodExec,
        RbacPermission, RbacSubject, RceCanExec, RunsOn, ServiceAccount, Uses,
    };

    use super::*;
    use crate::rules::run_rules_fixpoint;
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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

        assert!(update.new_relations.is_empty());
    }

    #[test]
    fn new_namespace_gets_cluster_contains_relation() {
        let campaign = test_campaign(); // has cluster "k8s/cluster/test-cluster"

        let ns = Namespace::new("default");
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(ns.clone()));

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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
        let inferred = analyzer.infer(&campaign, &update);

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
        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

    #[test]
    fn kubelet_source_marker_expands_to_all_nodes_and_copies_metadata() {
        let mut campaign = test_campaign();

        let pod = Pod::new("attacker", "default");
        let pod_id = pod.entity_id().0.clone();
        campaign.entities.insert_typed(pod);

        campaign.entities.insert_typed(K8sNode::new("worker-a"));
        campaign.entities.insert_typed(K8sNode::new("worker-b"));

        let marker = KubeletExecSource::new(pod_id.clone(), "all(k8s.node)")
            .with_envelope("ran-ws -- ${CMD}")
            .with_output_transform(OutputTransformKind::JsonEnvelope);
        let mut update = FactsUpdate::default();
        update.new_relations.push(Box::new(marker));

        let inferred = KubeletExecSourceAnalyzer.infer(&campaign, &update);

        let concrete: Vec<&KubeletExecSource> = inferred
            .new_relations
            .iter()
            .filter_map(|r| r.as_any().downcast_ref::<KubeletExecSource>())
            .collect();

        assert_eq!(concrete.len(), 2, "expected one edge per known node");
        assert!(concrete.iter().all(|r| r.pod_id.0 == pod_id));
        assert!(concrete
            .iter()
            .all(|r| r.envelope.as_deref() == Some("ran-ws -- ${CMD}")));
        assert!(concrete
            .iter()
            .all(|r| r.output_transform == Some(OutputTransformKind::JsonEnvelope)));
        assert!(concrete.iter().any(|r| r.node_id.0 == "node/worker-a"));
        assert!(concrete.iter().any(|r| r.node_id.0 == "node/worker-b"));
    }

    // ---------------------------------------------------------------------------
    // CanExecAccessAnalyzer tests
    // ---------------------------------------------------------------------------

    fn run_can_exec_access(campaign: &Campaign, update: &FactsUpdate) -> FactsUpdate {
        CanExecAccessAnalyzer.infer(campaign, update)
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
    fn pod_owned_by_replicaset_creates_deployment_entity_and_owns_relation() {
        use ran_domain::{Deployment, Owns};
        let campaign = test_campaign();
        // ReplicaSet name is "my-deploy-<hash>"; expected Deployment name is "my-deploy"
        let pod = make_pod_with_owner("my-pod-abc", "default", "ReplicaSet", "my-deploy-7d9f4b");

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzer = WorkloadOwnershipAnalyzer;
        let inferred = analyzer.infer(&campaign, &update);

        assert!(
            inferred
                .new_entities
                .iter()
                .any(|e| e.entity_kind() == "Deployment" && e.entity_name() == "my-deploy"),
            "expected Deployment entity to be created from ReplicaSet owner"
        );
        let deploy = Deployment::new("my-deploy", "default");
        assert!(
            inferred.new_relations.iter().any(|r| {
                r.is::<Owns>()
                    && r.source_id().0 == deploy.entity_id().0
                    && r.target_id().0 == pod.entity_id().0
            }),
            "expected Owns(Deployment → Pod) relation"
        );
        assert!(
            inferred.new_relations.iter().any(|r| {
                r.is::<Contains>()
                    && r.source_id().0 == "ns/default"
                    && r.target_id().0 == deploy.entity_id().0
            }),
            "expected Contains(ns/default → Deployment) relation"
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
        let inferred = analyzer.infer(&campaign, &update);

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
        assert!(
            inferred.new_relations.iter().any(|r| {
                r.is::<Contains>()
                    && r.source_id().0 == "ns/default"
                    && r.target_id().0 == ss.entity_id().0
            }),
            "expected Contains(ns/default → StatefulSet) relation"
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
        let inferred = analyzer.infer(&campaign, &update);

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
        assert!(
            inferred.new_relations.iter().any(|r| {
                r.is::<Contains>()
                    && r.source_id().0 == "ns/kube-system"
                    && r.target_id().0 == ds.entity_id().0
            }),
            "expected Contains(ns/kube-system → DaemonSet) relation"
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
        let inferred = analyzer.infer(&campaign, &update);

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
        assert!(
            inferred.new_relations.iter().any(|r| {
                r.is::<Contains>()
                    && r.source_id().0 == "ns/default"
                    && r.target_id().0 == job.entity_id().0
            }),
            "expected Contains(ns/default → Job) relation"
        );
    }

    #[test]
    fn already_known_deployment_not_duplicated_owns_still_emitted() {
        use ran_domain::{Deployment, Owns};
        let mut campaign = test_campaign();
        // Pre-populate the Deployment (suffix stripped from "existing-deploy-abc123")
        let deploy = Deployment::new("existing-deploy", "default");
        let deploy_id = deploy.entity_id();
        campaign.entities.insert_typed(deploy);

        let pod = make_pod_with_owner("my-pod", "default", "ReplicaSet", "existing-deploy-abc123");
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod.clone()));

        let analyzer = WorkloadOwnershipAnalyzer;
        let inferred = analyzer.infer(&campaign, &update);

        assert!(
            inferred
                .new_entities
                .iter()
                .all(|e| e.entity_kind() != "Deployment"),
            "should not emit Deployment entity when already in campaign"
        );
        assert!(
            inferred.new_relations.iter().any(|r| {
                r.is::<Owns>()
                    && r.source_id().0 == deploy_id.0
                    && r.target_id().0 == pod.entity_id().0
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
        let inferred = analyzer.infer(&campaign, &update);

        assert!(inferred.new_entities.is_empty());
        assert!(inferred.new_relations.is_empty());
    }

    #[test]
    fn owned_pod_does_not_get_direct_namespace_contains_relation() {
        let campaign = test_campaign();
        let pod = make_pod_with_owner("rs-pod-abc", "default", "ReplicaSet", "my-rs");
        let pod_id = pod.entity_id();

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

        assert!(
            !update
                .new_relations
                .iter()
                .any(|r| { r.is::<Contains>() && r.target_id().0 == pod_id.0 }),
            "owned pod must not receive a direct Contains(ns → pod) relation"
        );
        assert!(
            update
                .new_relations
                .iter()
                .any(|r| { r.is::<Contains>() && r.source_id().0 == "ns/default" }),
            "namespace must still contain the workload owner"
        );
    }

    // ---------------------------------------------------------------------------
    // KubeconfigCredentialAnalyzer tests
    // ---------------------------------------------------------------------------

    #[test]
    fn k8s_credential_triggers_cluster_entity() {
        let campaign = test_campaign();
        let mut cred = K8sCredential::new("https://10.96.0.1:6443");
        cred.token = Some("tok".to_string());

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(cred));

        let inferred = KubeconfigCredentialAnalyzer.infer(&campaign, &update);

        assert!(
            inferred
                .new_entities
                .iter()
                .any(|e| e.entity_kind() == "Cluster"),
            "expected K8sCluster entity to be derived from K8sCredential"
        );
    }

    #[test]
    fn already_known_cluster_not_duplicated_by_credential_analyzer() {
        let mut campaign = test_campaign();
        let cluster = K8sCluster::new("https://10.96.0.1:6443");
        let cluster_id = cluster.entity_id();
        campaign.entities.insert_typed(cluster);

        let mut cred = K8sCredential::new("https://10.96.0.1:6443");
        cred.token = Some("tok".to_string());

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(cred));

        let inferred = KubeconfigCredentialAnalyzer.infer(&campaign, &update);

        assert!(
            inferred
                .new_entities
                .iter()
                .all(|e| e.entity_id() != cluster_id),
            "should not re-emit K8sCluster when already in campaign"
        );
    }

    // ---------------------------------------------------------------------------
    // PropagateHostIPAnalyzer tests
    // ---------------------------------------------------------------------------

    #[test]
    fn host_ip_propagated_to_node_when_runs_on_exists_in_campaign() {
        use std::net::IpAddr;
        let mut campaign = test_campaign();

        // Pod is NOT yet in campaign (first discovery).
        let mut pod = Pod::new("web-pod", "default");
        pod.host_ip = Some("192.168.1.5".parse::<IpAddr>().unwrap());
        pod.is_running = true;
        let pod_id = pod.entity_id();

        let node = K8sNode::new("worker-1");
        let node_id = node.entity_id();
        campaign.entities.insert_typed(node);
        campaign.insert_relation(&RunsOn::new(pod_id.0.clone(), node_id.0.clone()));

        // Pod arrives for the first time, triggering the analyzer.
        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let analyzer = PropagateHostIPAnalyzer;
        let inferred = analyzer.infer(&campaign, &update);

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
        // Pod is new (not in campaign), but the node already knows the IP.
        let mut campaign = test_campaign();

        let host_ip: IpAddr = "10.0.0.1".parse().unwrap();

        let mut pod = Pod::new("my-pod", "default");
        pod.host_ip = Some(host_ip);
        pod.is_running = true;
        let pod_id = pod.entity_id();

        let mut node = K8sNode::new("node-1");
        node.system.ips.push(host_ip);
        let node_id = node.entity_id();
        campaign.entities.insert_typed(node);
        campaign.insert_relation(&RunsOn::new(pod_id.0.clone(), node_id.0.clone()));

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let analyzer = PropagateHostIPAnalyzer;
        let inferred = analyzer.infer(&campaign, &update);

        assert!(
            inferred.new_entities.is_empty(),
            "no update should be emitted when IP already present in node"
        );
    }

    #[test]
    fn no_update_on_reparsed_pod_when_host_ip_was_already_known() {
        use std::net::IpAddr;
        // Pod already in campaign with host_ip — a re-parse must not re-trigger.
        let mut campaign = test_campaign();

        let host_ip: IpAddr = "10.0.0.2".parse().unwrap();

        let mut pod = Pod::new("my-pod", "default");
        pod.host_ip = Some(host_ip);
        pod.is_running = true;
        let pod_id = pod.entity_id();
        campaign.entities.insert_typed(pod.clone());

        let node = K8sNode::new("node-1");
        let node_id = node.entity_id();
        campaign.entities.insert_typed(node);
        campaign.insert_relation(&RunsOn::new(pod_id.0.clone(), node_id.0.clone()));

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod));

        let inferred = PropagateHostIPAnalyzer.infer(&campaign, &update);
        assert!(
            inferred.new_entities.is_empty(),
            "re-parsing a pod whose host_ip was already known must not emit a node update"
        );
    }

    #[test]
    fn host_ip_propagated_when_pod_gains_host_ip_for_first_time() {
        use std::net::IpAddr;
        // Pod was in campaign without host_ip; now arrives with it.
        let mut campaign = test_campaign();

        let host_ip: IpAddr = "10.0.0.3".parse().unwrap();

        let mut pod_no_ip = Pod::new("my-pod", "default");
        pod_no_ip.is_running = true;
        let pod_id = pod_no_ip.entity_id();
        campaign.entities.insert_typed(pod_no_ip);

        let node = K8sNode::new("node-1");
        let node_id = node.entity_id();
        campaign.entities.insert_typed(node);
        campaign.insert_relation(&RunsOn::new(pod_id.0.clone(), node_id.0.clone()));

        let mut pod_with_ip = Pod::new("my-pod", "default");
        pod_with_ip.host_ip = Some(host_ip);
        pod_with_ip.is_running = true;

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(pod_with_ip));

        let inferred = PropagateHostIPAnalyzer.infer(&campaign, &update);
        let updated = inferred.new_entities.iter().find_map(|e| {
            e.as_any()
                .downcast_ref::<K8sNode>()
                .filter(|n| n.entity_id() == node_id)
        });
        assert!(
            updated
                .map(|n| n.system.ips.contains(&host_ip))
                .unwrap_or(false),
            "node should gain the IP when pod gains host_ip for the first time"
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
        let inferred = analyzer.infer(&campaign, &update);

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
        let inferred = analyzer.infer(&campaign, &update);

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
        let inferred = analyzer.infer(&campaign, &update);

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

        let inferred = RoleBindingAnalyzer.infer(&campaign, &update);

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

        let inferred = RoleBindingAnalyzer.infer(&campaign, &update);

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

        let inferred = RoleBindingAnalyzer.infer(&campaign, &update);

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

        let inferred = RoleBindingAnalyzer.infer(&campaign, &update);

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

        let inferred = RoleBindingAnalyzer.infer(&campaign, &update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

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

        let rules = default_rules();
        update = run_rules_fixpoint(&campaign, &rules, update);

        assert!(
            update.new_relations.iter().any(|r| {
                r.is::<Contains>()
                    && r.source_id().0 == cluster_id.0
                    && r.target_id().0 == binding_id.0
            }),
            "expected Contains(cluster → clusterrolebinding)"
        );
    }

    // ---------------------------------------------------------------------------
    // Fixpoint tests (moved from rules.rs)
    // ---------------------------------------------------------------------------

    #[test]
    fn node_cluster_rule_infers_contains_relation() {
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
            r.is::<Contains>() && r.source_id().0 == cluster_id.0 && r.target_id().0 == node_id.0
        });
        assert!(
            rel.is_some(),
            "expected contains relation from cluster to node"
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

    // --- KubeletMountAnalyzer helpers ---

    #[test]
    fn valid_pod_uuid_accepted() {
        assert!(super::is_valid_pod_uuid(
            "84cc979b-9ad8-4418-8b97-24a959833ce7"
        ));
        assert!(super::is_valid_pod_uuid(
            "293aba3c-f29f-4cd7-a4fe-233b4d111654"
        ));
    }

    #[test]
    fn invalid_pod_uuid_rejected() {
        assert!(!super::is_valid_pod_uuid("not-a-uuid"));
        assert!(!super::is_valid_pod_uuid(""));
        assert!(!super::is_valid_pod_uuid(
            "84cc979b-9ad8-4418-8b97-24a959833ce"  // 11 chars in last segment
        ));
        assert!(!super::is_valid_pod_uuid(
            "84cc979b-9ad8-4418-8b97-24a959833ceg"  // non-hex char
        ));
        assert!(!super::is_valid_pod_uuid(
            "gggggggg-9ad8-4418-8b97-24a959833ce7"  // non-hex first segment
        ));
    }

    #[test]
    fn lcp_of_multiple_names() {
        assert_eq!(
            super::longest_common_prefix(&["argocd-dex-server-tls", "argocd-repo-server-tls"]),
            "argocd-"
        );
        assert_eq!(
            super::longest_common_prefix(&["clustermesh-secrets", "hubble-tls"]),
            ""
        );
        assert_eq!(
            super::longest_common_prefix(&["argocd-dex-server-tls"]),
            "argocd-dex-server-tls"
        );
        assert_eq!(super::longest_common_prefix(&[]), "");
    }

    #[test]
    fn generic_volume_names_identified() {
        assert!(super::is_generic_volume_name("kube-api-access-b245w"));
        assert!(super::is_generic_volume_name("kube-api-access-"));
        assert!(!super::is_generic_volume_name("clustermesh-secrets"));
        assert!(!super::is_generic_volume_name("argocd-dex-server-tls"));
        assert!(!super::is_generic_volume_name("hubble-tls"));
    }

    #[test]
    fn display_name_with_shared_prefix() {
        let names = vec![
            "argocd-dex-server-tls".to_string(),
            "argocd-repo-server-tls".to_string(),
            "kube-api-access-28sp8".to_string(),
        ];
        assert_eq!(
            super::derive_pod_display_name("84cc979b-9ad8-4418-8b97-24a959833ce7", &names),
            "argocd-84cc979b"
        );
    }

    #[test]
    fn display_name_with_no_shared_prefix() {
        let names = vec![
            "clustermesh-secrets".to_string(),
            "hubble-tls".to_string(),
        ];
        assert_eq!(
            super::derive_pod_display_name("293aba3c-f29f-4cd7-a4fe-233b4d111654", &names),
            "293aba3c"
        );
    }

    #[test]
    fn display_name_all_generic() {
        let names = vec!["kube-api-access-z7h85".to_string()];
        assert_eq!(
            super::derive_pod_display_name("430772bd-a94b-40c0-a21e-075a62ff46cc", &names),
            "430772bd"
        );
    }

    #[test]
    fn display_name_single_non_generic_no_trailing_dash() {
        // LCP of a single name is that name; it doesn't end with '-' so one is inserted
        let names = vec!["hubble-tls".to_string()];
        assert_eq!(
            super::derive_pod_display_name("293aba3c-f29f-4cd7-a4fe-233b4d111654", &names),
            "hubble-tls-293aba3c"
        );
    }

    fn make_mount(mount_point: &str) -> ran_domain::Mount {
        ran_domain::Mount {
            name: String::new(),
            mount_point: mount_point.to_string(),
            mount_root: String::new(),
            mount_type: None,
            read_only: false,
            is_host_path: false,
        }
    }

    #[test]
    fn kubelet_mount_analyzer_discovers_pods_from_mounts() {
        let campaign = test_campaign();

        // Observing pod with kubelet mounts for two sibling pods
        let mut observer = Pod::new("observer", "default");
        observer.system.mounts = vec![
            // Pod 84cc979b: two named secret mounts + one generic SA token
            make_mount("/var/lib/kubelet/pods/84cc979b-9ad8-4418-8b97-24a959833ce7/volumes/kubernetes.io~secret/argocd-dex-server-tls"),
            make_mount("/var/lib/kubelet/pods/84cc979b-9ad8-4418-8b97-24a959833ce7/volumes/kubernetes.io~secret/argocd-repo-server-tls"),
            make_mount("/var/lib/kubelet/pods/84cc979b-9ad8-4418-8b97-24a959833ce7/volumes/kubernetes.io~projected/kube-api-access-28sp8"),
            // Pod 430772bd: only a generic SA token
            make_mount("/var/lib/kubelet/pods/430772bd-a94b-40c0-a21e-075a62ff46cc/volumes/kubernetes.io~projected/kube-api-access-z7h85"),
            // Unrelated mount — should be ignored
            make_mount("/proc/sys/fs/binfmt_misc"),
        ];

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(observer));

        let rules = default_rules();
        let update = run_rules_fixpoint(&campaign, &rules, update);

        // Expect two discovered Pod entities
        let pod_names: Vec<&str> = update
            .new_entities
            .iter()
            .filter(|e| e.entity_kind() == "Pod")
            .map(|e| e.entity_name())
            .collect();

        assert!(
            pod_names.contains(&"argocd-84cc979b"),
            "expected argocd-84cc979b pod, got: {:?}",
            pod_names
        );
        assert!(
            pod_names.contains(&"430772bd"),
            "expected 430772bd pod, got: {:?}",
            pod_names
        );

        // Expect RunsOn relations for both discovered pods
        assert!(
            update.new_relations.iter().any(|r| {
                r.is::<RunsOn>()
                    && r.source_id().0 == "ns/?/pod/argocd-84cc979b"
                    && r.target_id().0 == "node/?"
            }),
            "expected RunsOn(argocd-84cc979b → node/?)"
        );
        assert!(
            update.new_relations.iter().any(|r| {
                r.is::<RunsOn>()
                    && r.source_id().0 == "ns/?/pod/430772bd"
                    && r.target_id().0 == "node/?"
            }),
            "expected RunsOn(430772bd → node/?)"
        );
    }

    #[test]
    fn kubelet_mount_analyzer_uses_observer_node_name_when_known() {
        let campaign = test_campaign();

        let mut observer = Pod::new("observer", "default");
        observer.node_name = Some("worker-1".to_string());
        observer.system.mounts = vec![make_mount(
            "/var/lib/kubelet/pods/293aba3c-f29f-4cd7-a4fe-233b4d111654/volumes/kubernetes.io~projected/kube-api-access-b245w",
        )];

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(observer));

        let rules = default_rules();
        let update = run_rules_fixpoint(&campaign, &rules, update);

        assert!(
            update.new_relations.iter().any(|r| {
                r.is::<RunsOn>()
                    && r.source_id().0 == "ns/?/pod/293aba3c"
                    && r.target_id().0 == "node/worker-1"
            }),
            "expected RunsOn to node/worker-1"
        );
    }

    #[test]
    fn kubelet_mount_analyzer_skips_malformed_paths_with_warning() {
        // Just verifying no panic and no entity emitted for malformed paths.
        // Warning emission is verified by log inspection in manual testing.
        let campaign = test_campaign();

        let mut observer = Pod::new("observer", "default");
        observer.system.mounts = vec![
            // Too few segments after prefix
            make_mount("/var/lib/kubelet/pods/84cc979b-9ad8-4418-8b97-24a959833ce7/volumes"),
            // Non-UUID pod segment
            make_mount("/var/lib/kubelet/pods/not-a-uuid/volumes/kubernetes.io~projected/kube-api-access-abc"),
        ];

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(observer));

        let rules = default_rules();
        let update = run_rules_fixpoint(&campaign, &rules, update);

        let discovered_pods: Vec<_> = update
            .new_entities
            .iter()
            .filter(|e| e.entity_kind() == "Pod" && e.entity_name() != "observer")
            .collect();
        assert!(
            discovered_pods.is_empty(),
            "expected no pods from malformed paths, got: {:?}",
            discovered_pods.iter().map(|e| e.entity_name()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn kubelet_mount_analyzer_does_not_duplicate_existing_pod() {
        let mut campaign = test_campaign();

        // Pre-insert the pod that would be discovered
        let existing = Pod::new("argocd-84cc979b", "?");
        campaign.entities.insert_typed(existing);

        let mut observer = Pod::new("observer", "default");
        observer.system.mounts = vec![make_mount(
            "/var/lib/kubelet/pods/84cc979b-9ad8-4418-8b97-24a959833ce7/volumes/kubernetes.io~secret/argocd-dex-server-tls",
        )];

        let mut update = FactsUpdate::default();
        update.new_entities.push(Box::new(observer));

        let rules = default_rules();
        let update = run_rules_fixpoint(&campaign, &rules, update);

        let new_pods: Vec<_> = update
            .new_entities
            .iter()
            .filter(|e| e.entity_kind() == "Pod" && e.entity_name() == "argocd-84cc979b")
            .collect();
        assert!(
            new_pods.is_empty(),
            "should not re-emit a pod already in the campaign"
        );
    }
}
