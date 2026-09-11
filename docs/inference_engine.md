# Inference Engine: Implementation Reference

This document describes the inference engine as it exists in the Rust codebase.

---

## Overview

The inference engine derives new facts (entities, relations) from already-known facts. It is composed of two mechanically similar but operationally distinct layers:

| Layer | Where | When it runs | Fixpoint? |
|---|---|---|---|
| **Analyzers** | `crates/campaign/src/analyzers.rs` | During output parsing, before facts are committed | No - single pass |
| **Inference Rules** | `crates/campaign/src/rules.rs` | After TTP execution, on the batch of new facts | Yes - up to 8 iterations |

Both layers read `Campaign` state plus a pending `FactsUpdate`, and emit a new `FactsUpdate`. Neither mutates campaign state directly - the caller applies the result via `apply_facts`.

---

## Core Data Structure: `FactsUpdate`

```rust
pub struct FactsUpdate {
    pub new_entities: Vec<Box<dyn Entity + Send + Sync>>,
    pub new_relations: Vec<Box<dyn Relation + Send + Sync>>,
    pub entity_aliases: IndexSet<(EntityId, EntityId)>,
}
```

- **`new_entities`** - typed entities to add or merge into the campaign's `EntityStore`.
- **`new_relations`** - directed graph edges to add to the `KnowledgeGraph`.
- **`entity_aliases`** - `(stale_id, preferred_id)` pairs. When applied, all graph edges touching `stale_id` are retargeted to `preferred_id` and entity data is merged. Used to reconcile placeholder entities (e.g. an IP-derived `UnknownSystem`) with later-discovered named entities.

`FactsUpdate::merge` deduplicates via `IndexSet`-based seen-sets on both entities and relations. `apply_facts` in `execution.rs` commits a `FactsUpdate` to the campaign in three steps: insert entities (via `Merge::merge_from` on collision), process entity aliases (edge retargeting + data merge), insert relations.

---

## Knowledge Graph

`KnowledgeGraph` in `crates/graph/src/graph.rs` wraps a `petgraph::StableGraph<EntityId, EdgeData>` with an `O(1)` `HashMap<EntityId, NodeIndex>` index. Nodes carry only `EntityId`; actual entity data lives in `EntityStore`.

### Edge Metadata

```rust
pub struct EdgeData {
    pub relation_name: String,
    pub weight: f32,           // cost for Dijkstra; 0.0 = structural
    pub is_exec_channel: bool, // true = traversal grants execution on target
    pub envelope: Option<String>, // command template for hop-wrapping
}
```

Exec-channel edges represent capabilities that grant code execution on the target:

| Relation | Weight | Notes |
|---|---|---|
| `k8s.can-exec` (PodExec) | 1.0 | `kubectl exec` via RBAC |
| `kubelet-exec` (KubeletExecSink) | 1.5 | Node-mediated exec into co-located pod |
| `container.escape` (ContainerEscape) | 2.0 | Container breakout to host node |
| `rce.can-exec` (RceCanExec) | 2.5 | Exploit-based RCE |

All other relations (containment, ownership, RBAC graph) have weight 0.0 and `is_exec_channel = false`.

### Graph Invariants

- **NoSelfEdge**: `insert_edge` returns `None` if `src == tgt`.
- **PodSingleNode**: inserting a new `runs-on` edge from a pod removes any prior `runs-on` edge from that pod first (models pod rescheduling).

### Key Query Methods

| Method | Description |
|---|---|
| `targets_of(id, relation)` | All entities that `id` points to via the named relation |
| `sources_of(id, relation)` | All entities with a named edge pointing at `id` |
| `exec_edges()` | All exec-channel edges |
| `reachable_via_exec(seeds)` | BFS from seed set over exec-channel edges |
| `shortest_exec_path(seeds, target)` | Dijkstra over exec-channel edges; returns `(cost, path)` |
| `merge_entities(keep, discard)` | Retargets all edges, deduplicating by relation name |

---

## Entity Store

`EntityStore` holds one `HashMap<EntityId, T>` per registered entity type (24 total). Insert semantics call `Merge::merge_from` on collision, so entities accumulate data incrementally.

**Registered types**: `C2Server`, `K8sCluster`, `K8sNode`, `Namespace`, `Pod`, `ServiceAccount`, `K8sSecret`, `ConfigMap`, `Deployment`, `K8sRole`, `K8sRoleBinding`, `CronJob`, `ReplicaSet`, `StatefulSet`, `DaemonSet`, `Job`, `GCPServiceAccount`, `GCPBucket`, `K8sCredential`, `UnknownSystem`, `K8sService`, `K8sIngress`, `K8sGateway`, `K8sHTTPRoute`.

**Entity ID scheme**:

| Type | ID Format |
|---|---|
| Pod | `ns/<namespace>/pod/<name>` |
| Node | `node/<name>` |
| Namespace | `ns/<name>` |
| ServiceAccount | `ns/<namespace>/sa/<name>` |
| Role | `ns/<namespace>/role/<name>` or `clusterrole/<name>` |
| RoleBinding | `ns/<namespace>/rolebinding/<name>` or `clusterrolebinding/<name>` |
| Cluster | `k8s/cluster/<name>` |
| UnknownSystem (IP) | `system/<ip>` |

---

## Analyzers

Analyzers run during output parsing via `run_analyzers`. Each analyzer receives the accumulated `FactsUpdate` (including prior analyzers' outputs in the same pass), so ordering matters for same-pass chaining. There is no fixpoint - a single pass is made.

```rust
pub fn run_analyzers(campaign: &Campaign, analyzers: &[Box<dyn Analyzer>], base: &mut FactsUpdate) {
    for analyzer in analyzers {
        let inferred = analyzer.analyze(campaign, base);
        base.merge(inferred);
    }
}
```

### Built-in Analyzers (23 total)

#### Topology / Containment Wiring

| Analyzer | Trigger | Emits |
|---|---|---|
| `NamespaceClusterAnalyzer` | New `Namespace` | `Contains(cluster → namespace)` |
| `NodeClusterAnalyzer` | New `K8sNode` | `Contains(cluster → node)` |
| `PodNamespaceAnalyzer` | New `Pod` | `Contains(namespace → pod)`; creates namespace stub if needed |
| `ServiceAccountNamespaceAnalyzer` | New `ServiceAccount` | `Contains(namespace → sa)`; creates namespace stub if needed |
| `ClusterRoleClusterAnalyzer` | New `K8sRole` with `is_cluster_role = true` | `Contains(cluster → clusterrole)` |
| `ClusterRoleBindingClusterAnalyzer` | New `K8sRoleBinding` with empty namespace | `Contains(cluster → clusterrolebinding)` |
| `RoleNamespaceAnalyzer` | New namespaced `K8sRole` | `Contains(namespace → role)` |
| `RoleBindingNamespaceAnalyzer` | New namespaced `K8sRoleBinding` | `Contains(namespace → rolebinding)` |
| `ServiceNamespaceAnalyzer` | New `K8sService` | `Contains(namespace → service)` |
| `IngressNamespaceAnalyzer` | New `K8sIngress` | `Contains(namespace → ingress)` |
| `GatewayNamespaceAnalyzer` | New `K8sGateway` | `Contains(namespace → gateway)` |
| `HTTPRouteNamespaceAnalyzer` | New `K8sHTTPRoute` | `Contains(namespace → httproute)` |

#### Pod Placement

| Analyzer | Trigger | Emits |
|---|---|---|
| `PodNodeAnalyzer` | Running pod with `node_name` set | `RunsOn(pod → node)`; creates node stub if needed |

#### Service Account Inference

| Analyzer | Trigger | Emits |
|---|---|---|
| `ServiceAccountAnalyzer` | Pod with `service_account_name` | `Uses(pod → sa)`; creates SA stub. Suppressed if `automount = Confidence::No` |
| `ServiceAccountTokenAnalyzer` | New SA with bound JWT token containing pod claims | `Pod` entity (marked running) + `Uses(pod → sa)` |
| `ServiceAccountCanExecAnalyzer` | SA with `create pods/exec` entitlement in scope | `PodExec(sa → pod)` for all running pods in that namespace |

`ServiceAccountCanExecAnalyzer` is the primary RBAC → exec-edge inference path.

#### Kubelet Exec Sink (Analyzer)

| Analyzer | Trigger | Emits |
|---|---|---|
| `KubeletExecSinkAnalyzer` | New `kubelet-exec` relation (source pod → node) | `KubeletExecSink(node → pod)` for each running pod co-located on that node (excluding source) |

This analyzer only fans out `KubeletExecSink` edges - it does **not** create the `KubeletExecSource` relation. `KubeletExecSource` (the precondition: a pod with `ran-ws` + a SA with `GET nodes/proxy`) is exclusively created by `KubeletExecSourceRule` in the rules layer. See the [Analyzer vs Rule Duplication](#analyzer-vs-rule-duplication) section for why both layers cover the sink side.

#### Node IP Propagation

| Analyzer | Trigger | Emits |
|---|---|---|
| `PropagateHostIPAnalyzer` | Pod with `host_ip` + existing `runs-on` edge, **or** new `runs-on` relation with existing pod that has `host_ip` | Copies pod's `host_ip` into the node's `system.ips` |

#### Access Level Propagation

| Analyzer | Trigger | Emits |
|---|---|---|
| `CanExecAccessAnalyzer` | New relation with `is_exec_channel = true` | Sets `access_level = Exec` on the target entity. Idempotent. |

#### Workload Ownership

| Analyzer | Trigger | Emits |
|---|---|---|
| `WorkloadOwnershipAnalyzer` | Pod with `owner_references` | Creates owning workload entity (`ReplicaSet`, `StatefulSet`, `DaemonSet`, or `Job`) + `Owns(workload → pod)` |

#### GCP Integration

| Analyzer | Trigger | Emits |
|---|---|---|
| `GCPServiceAccountAnalyzer` | Pod with GCP SA email in env vars or `GOOGLE_APPLICATION_CREDENTIALS` | `Uses(pod → gcp_sa)` |

#### Entity Identity Reconciliation

| Analyzer | Trigger | Emits |
|---|---|---|
| `IpBasedSystemMergeAnalyzer` | New `Pod` or `K8sNode` | Checks for `UnknownSystem` entries matching by IP. On match, records `entity_alias(unknown_system_id → real_entity_id)`. Guards against host-network pods sharing node IPs. |

---

## Inference Rules

Rules run after TTP execution via `run_rules_fixpoint`. Each iteration runs all rules against the accumulated facts; the loop breaks when no rule produces new facts or 8 iterations are reached.

```rust
pub fn run_rules_fixpoint(campaign: &Campaign, rules: &[Box<dyn InferenceRule>], initial: FactsUpdate) -> FactsUpdate {
    let mut acc = initial;
    for _ in 0..8 {
        let mut changed = false;
        let mut next = FactsUpdate::default();
        for rule in rules {
            let inferred = rule.infer(campaign, &acc);
            if !inferred.is_empty() {
                changed = true;
                next.merge(inferred);
            }
        }
        if !changed { break; }
        acc.merge(next);
    }
    acc
}
```

Rules carry a `RuleTrigger` (currently advisory - all rules run regardless):
```rust
pub enum RuleTrigger {
    Always,
    EntityKind(String),
    RelationName(String),
}
```

### Built-in Rules (18 total)

#### Containment / Topology (mirrors the analyzer set, runs at fixpoint)

| Rule | Trigger | Emits |
|---|---|---|
| `NamespaceClusterRule` | `EntityKind("Namespace")` | `Contains(cluster → ns)` |
| `NodeClusterRule` | `EntityKind("Node")` | `ManagesNode(cluster → node)` |
| `PodNamespaceRule` | `EntityKind("Pod")` | `Contains(ns → pod)`; creates namespace stub |
| `ServiceAccountNamespaceRule` | `EntityKind("ServiceAccount")` | `Contains(ns → sa)`; creates namespace stub |
| `PodNodeRule` | `EntityKind("Pod")` | `RunsOn(pod → node)` for running pods with `node_name` |
| `RoleNamespaceRule` | `EntityKind("Role")` | `Contains(ns → role)` for namespaced roles |
| `RoleBindingNamespaceRule` | `EntityKind("RoleBinding")` | `Contains(ns → rolebinding)` for namespaced bindings |
| `ClusterRoleClusterRule` | `EntityKind("ClusterRole")` | `Contains(cluster → clusterrole)` |
| `ClusterRoleBindingClusterRule` | `EntityKind("ClusterRoleBinding")` | `Contains(cluster → clusterrolebinding)` |
| `ServiceNamespaceRule` | `EntityKind("Service")` | `Contains(ns → service)` |
| `IngressNamespaceRule` | `EntityKind("Ingress")` | `Contains(ns → ingress)` |
| `GatewayNamespaceRule` | `EntityKind("Gateway")` | `Contains(ns → gateway)` |
| `HTTPRouteNamespaceRule` | `EntityKind("HTTPRoute")` | `Contains(ns → httproute)` |

#### RBAC and Exec-Channel Inference

| Rule | Trigger | What it does |
|---|---|---|
| `ServiceAccountCanExecRule` | `EntityKind("Pod")`, `RelationName("can")` | For each SA with `create pods/exec` in scope: emits `PodExec(sa → pod)` for all matching running pods |
| `RoleBindingPermissionsRule` | `EntityKind("RoleBinding")`, `EntityKind("ClusterRoleBinding")` | Injects stamped `RbacPermission` entries into the referenced `ServiceAccount` entity |
| `RoleBindingGraphRule` | Same | Emits `BindsTo(binding → role)` and `Grants(binding → sa)` graph edges; creates stub role/SA entities if absent |

#### Kubelet Exec Source and Sink (Rules)

| Rule | Trigger | What it does |
|---|---|---|
| `KubeletExecSourceRule` | `EntityKind("Pod")`, `EntityKind("Node")`, `EntityKind("ServiceAccount")` | Fires when any SA has `GET nodes/proxy` AND any pod has the `ran-ws` binary present. Emits `KubeletExecSource(pod → node)` for all qualifying pods × all known nodes. |
| `KubeletExecSinkRule` | `RelationName("kubelet-exec")`, `RelationName("runs-on")` | For each `kubelet-exec` (pod → node), emits `KubeletExecSink(node → pod)` for each co-located running pod |

**Three components, two layers**: `KubeletExecSourceRule` creates the source relation (requires binary + SA permission); `KubeletExecSinkRule` fans out sink edges from a source relation; `KubeletExecSinkAnalyzer` does the same fan-out in the analyzer layer for facts arriving via output parsers. There is no `KubeletExecSourceAnalyzer` - source creation only happens in the rules layer.

**`KubeletExecSourceRule` detail**: this rule is the most complex. It gates on two independent conditions that may arrive in separate iterations - the SA permission and the binary presence - which is exactly why fixpoint evaluation is needed here. Binary presence must be committed to the campaign *before* the fixpoint runs, otherwise the rule sees `Unknown` even after a successful download.

---

## Effects System

Effects are strings attached to TTP definitions that declare what campaign state changes result from a successful execution. They are parsed in `effects.rs`.

### Structural Effects (entity creation)

| Effect Key | Required Args | Optional Args | Produces |
|---|---|---|---|
| `k8s.pod` | `Namespace`, `PodName` | `NodeName`, `ServiceAccount`, `IsRunning` | `Pod` entity |
| `k8s.serviceaccount` | `Namespace`, `ServiceAccountName` | `Token` | `ServiceAccount` with optional JWT |
| `k8s.role` | `Namespace`, `RoleName` | `Rules` (JSON) | `K8sRole` entity |
| `k8s.rolebinding` | `Namespace`, `BindingName` | `RoleRef`, `Subjects` (JSON) | `K8sRoleBinding` entity |
| `k8s.cronjob` | `Namespace`, `CronJobName` | `Schedule` | `CronJob` entity |

### Relation Effects

| Effect Syntax | Args | Produces |
|---|---|---|
| `k8s.can-exec(src, tgt)` | 2 entity IDs | `PodExec` relation (exec-channel, weight 1.0) |
| `k8s.can-reach(src, tgt)` | 2 entity IDs | `CanReach` relation (non-exec) |
| `k8s.runs-on(pod, node)` | 2 entity IDs | `RunsOn` relation |
| `k8s.kubelet-exec-source(pod, node)` | 2 entity IDs, or `sys` / `all(k8s.Node)` | `KubeletExecSource` relation. `all(k8s.Node)` is a no-op; actual per-node edges come from `KubeletExecSourceRule` |
| `rce.can-exec(src, tgt)` | 2 entity IDs | `RceCanExec` relation (exec-channel, weight 2.5); stores `PROCEDURE_CMD` as envelope |
| `container.escape(src)` | 1 entity ID or `sys` | `K8sNode` entity + `RunsOn` + `ContainerEscape` relation (exec-channel, weight 2.0); stores `PROCEDURE_CMD` as envelope |

**Special placeholders**:
- `sys` → resolves to `args["TARGET_ID"]`, the entity the TTP executed on
- `all(k8s.Node)` in kubelet-exec → no-op; rule-engine handles the fan-out

### Templating

Effects go through two substitution passes:
1. Tera-style `{% if/else/endif %}` blocks and `{{ Var }}` substitutions
2. Case-insensitive `${KEY}` → `args[KEY]` substitution

---

## Campaign Execution Pipeline

`Campaign::prepare_action` builds an action through a 6-stage pipeline:

```
1. validate_request         - reject empty action_id / target_id
2. assert_target_exists     - target must exist in EntityStore
3. resolve_ttp_and_defaults - TTP lookup + fill param defaults
4. ground_args_from_context - inject NS, NODE, TOKEN, API_SERVER, RANDOM from entity
5. resolve_lateral_src      - for Lateral Movement: inject SRC, resolve ExecChannel
6. ground_procedure_and_effects - Tera + ${} substitution; mint PROCEDURE_CMD
   route_exec_channel       - select C2 backend and optional hop wrapping
```

### Arg Grounding (`grounding.rs`)

`ground_args_from_context` injects well-known keys before template substitution:

| Key (case-insensitive) | Resolution |
|---|---|
| `NS` / `NAMESPACE` | Target entity's namespace |
| `POD_NAME` / `PODNAME` | Target entity's name |
| `NODE` / `NODENAME` / `NODE_NAME` | Pod's `node_name` field |
| `TOKEN` | SA reference → raw JWT (accepts SA entity ID, SA name, or resolves from pod) |
| `API_SERVER` | Defaults to `https://kubernetes.default.svc` |
| any value with `${RANDOM}` | Replaced with a 5-digit pseudo-random number |

### `on_ttp_executed` (post-execution)

1. **Failure path**: runs `classify_failure`; records binary-absent facts if the tool was missing.
2. **Effect parsing**: for each effect string in `cmd.ttp.effects`:
   - Tries output parsers first (`sys.*`, `k8s.*`, `gcp.*`, `file.*`, `network.*`) to parse stdout.
   - Falls back to structural effects (`parse_effect_with_status`).
   - Unrecognized effects produce a `NoParser` audit entry.
3. **Rules fixpoint**: calls `run_rules_fixpoint` on the merged `FactsUpdate`.
4. **Identity merge detection**: `detect_pod_identity_merge` checks if execution revealed the real identity of an IP-placeholder pod; records aliases accordingly.
5. **Binary presence**: marks the tool as `Present` if previously `Unknown`.
6. **`apply_facts`**: commits everything to campaign state.

### Exec Channel Resolution

`resolve_exec_channel` selects how to reach the target, in priority order:

1. Active session on target (live shell)
2. Most-recently-targeted system that is a direct foothold
3. Shortest exec-path from that system to target (Dijkstra)
4. Direct C2 → target exec edge in graph
5. Shortest exec-path from all direct footholds to target
6. Indirect via `uses` (SA → pod resolution)
7. Error: no viable channel

### Command Hop Wrapping

`wrap_command_for_hops` builds multi-hop execution by iterating hops from innermost to outermost. For each hop, it looks up the exec-channel edge's envelope template and substitutes the inner command into the `${CMD}` slot. Falls back to `kubectl exec -n <ns> <pod> -- <cmd>` for pod-format entity IDs.

---

## Failure Analysis

`classify_failure` runs a chain of analyzers on failed TTP output:

| Analyzer | Detects |
|---|---|
| `InvalidTargetFailureAnalyzer` | "invalid pod target id" |
| `RbacDeniedFailureAnalyzer` | "forbidden", "permission denied", RBAC error strings |
| `ConnectivityFailureAnalyzer` | timeout, connection refused, DNS failures |
| `CommandNotFoundFailureAnalyzer` | "command not found", "No such file"; extracts binary name |
| `NotWriteableFailureAnalyzer` | "read-only file system", write-path permission denied |

When `is_binary_missing = true`, the pipeline records `Absent` for that binary in the executing system, enabling fallback procedure selection.

---

## Relation Reference

| Relation | Direction | Exec Channel | Weight | Purpose |
|---|---|---|---|---|
| `Contains` | cluster/ns → child | No | 0.0 | Structural containment |
| `ManagesNode` | cluster → node | No | 0.0 | Cluster manages node (rules variant) |
| `RunsOn` | pod → node | No | 0.0 | Pod placement |
| `Uses` | pod → sa | No | 0.0 | Pod uses a service account |
| `BindsTo` | rolebinding → role | No | 0.0 | RBAC graph edge |
| `Grants` | rolebinding → sa | No | 0.0 | RBAC graph edge |
| `Owns` | workload → pod | No | 0.0 | Workload owns pod |
| `CanReach` | src → tgt | No | 0.0 | Network reachability (no exec) |
| `PodExec` | sa/c2 → pod | **Yes** | 1.0 | `kubectl exec` capability |
| `KubeletExecSink` | node → pod | **Yes** | 1.5 | Node-mediated exec into co-located pod |
| `ContainerEscape` | pod → node | **Yes** | 2.0 | Container breakout to host |
| `RceCanExec` | attacker → victim | **Yes** | 2.5 | Exploit-based RCE |
| `KubeletExecSource` | pod → node | No | 0.0 | Precondition for `KubeletExecSink` inference |

Note: `KubeletExecSource` is not itself an exec channel - it is a structural fact that `KubeletExecSinkAnalyzer` / `KubeletExecSinkRule` use to derive `KubeletExecSink` edges on co-located pods.

---

## Analyzer vs Rule Duplication

Several inference steps appear in both layers (e.g. `PodNamespace` exists as both an analyzer and a rule). This is intentional:

- **Analyzers** handle facts arriving via output parsers, where the campaign graph is already committed and only the delta is in flight.
- **Rules** handle facts arriving from TTP effects, where chains of derived facts may need multiple passes to reach fixpoint.

The duplication ensures correctness regardless of which ingestion path introduces a fact.
