# Domain Types Reference

All types live in `crates/domain/`. Import via `use ran_domain::{...}`.

---

## Entities

All entities implement `Entity` (which requires `entity_id() -> EntityId`).
Push them into `FactsUpdate.new_entities` via `updates.new_entities.push(Box::new(entity))`.

### Pod
```rust
Pod {
    meta: K8sMeta { name, namespace: Some(ns), uid, labels, annotations, created_at, owner },
    system: SystemInfo::default(),
    node_name: Option<String>,
    privileged: Confidence::Unknown,
    host_pid: Confidence::Unknown,
    host_ipc: Confidence::Unknown,
    host_network: Confidence::Unknown,
    read_only_root_fs: Confidence::Unknown,
    service_account_name: Option<String>,
    automount_service_account_token: Confidence::Unknown,
    containers: vec![Container { name, image }],
    volume_mounts: vec![],
    host_paths: vec![],
    phase: Option<PodPhase>,   // Running | Pending | Succeeded | Failed | Unknown
    is_running: bool,
}
```
Entity ID: `ns/{namespace}/pod/{name}`
Convenience constructor: `Pod::new(name, namespace)` (fills defaults).

### K8sNode
```rust
K8sNode {
    name: String,
    system: SystemInfo::default(),
}
```
Entity ID: `node/{name}`
Constructor: `K8sNode::new(name)`.

### Namespace
```rust
Namespace {
    name: String,
    psa: PodSecurityAdmission::default(),
    labels: HashMap::new(),
}
```
Entity ID: `ns/{name}`
Constructor: `Namespace::new(name)`.

### ServiceAccount
```rust
ServiceAccount {
    meta: K8sMeta { name, namespace: Some(ns), .. },
    token: None,
    secret_names: vec![],
    entitlements: vec![],
}
```
Entity ID: `ns/{namespace}/sa/{name}`
Constructor: `ServiceAccount::new(name, namespace)`.

### K8sCluster
```rust
K8sCluster { name: String, context_name: None, server: None }
```
Entity ID: `k8s/cluster/{slugified_name}`
Constructor: `K8sCluster::new(name)`.

---

## Supporting Types

### K8sMeta
```rust
K8sMeta {
    name: String,
    namespace: Option<String>,
    uid: Option<String>,
    labels: HashMap<String, String>,
    annotations: HashMap<String, String>,
    created_at: Option<String>,
    owner: Option<OwnerRef>,  // { kind, name, uid }
}
```
Constructor: `K8sMeta::new(name, namespace)`.

### SystemInfo
Fields you can populate to record facts about a running system (pod or node):
```rust
SystemInfo {
    os: Option<String>,
    ips: Vec<IpAddr>,              // std::net::IpAddr
    user_id: Option<u32>,
    username: Option<String>,
    env_vars: HashMap<String, String>,
    binaries: HashMap<String, BinaryPresence>,  // Unknown | Absent | Present(path)
    files: Vec<String>,
    missing_files: Vec<String>,
    processes: Vec<Process>,
    mounts: Vec<Mount>,
    access_level: AccessLevel,     // None < UserRead < UserExec < RootRead < RootExec
}
```
Extend fields with `.extend()` / `.push()` rather than overwriting them.

### Confidence
```rust
Confidence::Unknown  // fact not yet observed
Confidence::No       // explicitly observed as false
Confidence::Yes      // explicitly observed as true
```

### Container
```rust
Container { name: String, image: String }
```

### Mount
```rust
Mount {
    name: String,
    mount_point: String,   // path inside container
    mount_root: String,    // path on host
    mount_type: Option<String>,
    read_only: bool,
    is_host_path: bool,
}
```

### Process
```rust
Process {
    pid: u32,
    parent_pid: u32,
    name: String,
    cmd: String,
    user: Option<String>,
    start_time: Option<String>,
}
```

---

## Relations

Push into `FactsUpdate.new_relations` via `updates.new_relations.push(Box::new(rel))`.
All live in `ran_domain::relations`.

```rust
// Namespace contains Pod / Cluster contains Namespace
Contains { container_id: EntityId, object_id: EntityId }

// Pod can exec into another Pod
PodExec { executor_id: EntityId, target_id: EntityId }

// Pod runs on Node
RunsOn { pod_id: EntityId, node_id: EntityId }

// Pod is the kubelet exec source for a Node
KubeletExecSource { pod_id: EntityId, node_id: EntityId }

// Node can exec into a Pod via kubelet
KubeletExecSink { node_id: EntityId, pod_id: EntityId }
```

---

## FactsUpdate

```rust
pub struct FactsUpdate {
    pub new_entities: Vec<Box<dyn Entity + Send + Sync>>,
    pub new_relations: Vec<Box<dyn Relation + Send + Sync>>,
}
```

`FactsUpdate::default()` gives an empty update.
Inference rules run after the parser so you don't need to manually add `Contains` relations
— those are derived automatically for pods and namespaces.

---

## ParsedEffect / Helpers

```rust
pub struct ParsedEffect {
    pub updates: FactsUpdate,
    pub audit: ParseAudit,
}
```

Use `build_audit()` (private, already in scope within the module):

```rust
fn build_audit(
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
    parse_result: ParseResult,
    detail: &str,
    inferred_facts_written: usize,
) -> ParseAudit
```

Use `build_parse_audit()` (public alias for the same function, callable from tests).

Use `get_parse_target_system(campaign, cmd)` to get a `CampaignSystemEntityMut` when you
need to write into a pod or node's `SystemInfo`. It tries `cmd.target_id` first, then falls
back to `cmd.exec_system_id`.

---

## Key file paths

| Purpose | Path |
|---------|------|
| Output parsers (edit here) | `crates/campaign/src/output_parsers.rs` |
| Structural effects | `crates/campaign/src/effects.rs` |
| Domain entities | `crates/domain/entities.rs` |
| Domain types | `crates/domain/types.rs` |
| Domain relations | `crates/domain/relations.rs` |
| Campaign struct | `crates/campaign/src/lib.rs` |
| TTP YAML files | `armory/TTPs/**/*.yaml` |
