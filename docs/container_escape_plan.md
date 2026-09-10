# Container Escape - Implementation Plan

## Context

Ran models attack paths through Kubernetes clusters as a knowledge graph of entities and relations. Execution channels (`PodExec`, `KubeletExecSink`, `RceCanExec`) let the campaign route commands to compromised systems. Today, the graph only supports pod-to-pod and node-to-pod execution. There is no way to represent a **container escape**: moving from a compromised pod to its underlying host node.

This plan adds container escape support in two phases:
1. **Phase 1**: Envelope-based `ContainerEscape` relation (nsenter/chroot wrapping) - follows the exact `RceCanExec` pattern, no C2 layer changes. **COMPLETE.**
2. **Phase 2**: Interactive session model - for bind/reverse shells and implants that establish a live session on the node, requiring session registration in the C2 layer.

---

## Phase 1: Envelope-Based `ContainerEscape` Relation - COMPLETE

The escape relation works identically to `RceCanExec`: it carries an envelope string (e.g. `nsenter -t 1 -m -u -i -n -p -- ${CMD}`), is a C2Channel, and the existing hop-wrapping machinery routes commands through it.

### Step 1: Domain - Add `ContainerEscape` struct

**File: `crates/domain/relations.rs`**

Add after `RceCanExec`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerEscape {
    pub source_id: EntityId,   // pod entity
    pub target_id: EntityId,   // node entity
    pub envelope: Option<String>,
}
```

- Builder methods: `new()`, `with_envelope()`, `with_opt_envelope()` - same pattern as `RceCanExec`
- `impl C2Channel for ContainerEscape {}`
- `impl Relation` with `relation_name() -> "container.escape"`, `is_exec_channel() -> true`

**File: `crates/domain/mod.rs`** - add `ContainerEscape` to re-exports.

### Step 2: Graph - Register weight

**File: `crates/graph/src/edge.rs`** - add to `relation_defaults()`:

```rust
"container.escape" => (2.0, true),
```

Weight 2.0: more reliable than RCE (2.5) since it's a local operation, but more expensive than direct kubectl exec (1.0) since it requires prior compromise + capabilities.

### Step 3: Campaign - Envelope extraction in `insert_relation_with_ids`

**File: `crates/campaign/src/campaign/execution.rs`** (~line 762)

Currently only downcasts to `RceCanExec` for envelope extraction. Add `ContainerEscape`:

```rust
let envelope = rel
    .as_any()
    .downcast_ref::<RceCanExec>()
    .and_then(|r| r.envelope.clone())
    .or_else(|| {
        rel.as_any()
            .downcast_ref::<ContainerEscape>()
            .and_then(|r| r.envelope.clone())
    });
```

Similarly update `RelationSummary::from_relation()` in `crates/domain/relations.rs` (~line 461) to also extract envelopes from `ContainerEscape`.

### Step 4: Campaign - AccessLevel update for nodes

In the same `insert_relation_with_ids` method (~line 771), the current code only upgrades `AccessLevel` on pods. Extend to also upgrade nodes:

```rust
if rel.is_exec_channel() {
    if let Some(pod) = self.entities.find_mut::<Pod>(tgt) {
        if pod.system.access_level == ran_domain::AccessLevel::None {
            pod.system.access_level = ran_domain::AccessLevel::Exec;
        }
    }
    // Container escape targets a node - mark it as exec-accessible
    if let Some(node) = self.entities.find_mut::<K8sNode>(tgt) {
        if node.system.access_level == ran_domain::AccessLevel::None {
            node.system.access_level = ran_domain::AccessLevel::Exec;
        }
    }
}
```

### Step 5: Effect parser - `container.escape` handler

**File: `crates/campaign/src/effects.rs`**

Add to `resolve_relation_effect_handler()`:

```rust
"container.escape" => Some(parse_container_escape_relation),
```

Handler:

```rust
fn parse_container_escape_relation(
    args: &[&str],
    ctx: &HashMap<String, String>,
) -> Result<FactsUpdate, String> {
    if args.len() != 2 {
        return Err("container.escape expects 2 args: source pod and target node".into());
    }
    let envelope = ctx.get("PROCEDURE_CMD")
        .filter(|v| !v.trim().is_empty())
        .cloned();
    let rel = ContainerEscape::new(args[0], args[1])
        .with_opt_envelope(envelope);
    Ok(FactsUpdate {
        new_entities: Vec::new(),
        new_relations: vec![Box::new(rel)],
        entity_aliases: IndexSet::new(),
    })
}
```

### Step 6: `wrap_command` - handle node entity IDs

**File: `crates/domain/relations.rs`** - `RelationSummary::wrap_command()` (~line 482)

The current fallback only handles `ns/<ns>/pod/<name>` targets. When the envelope is present (which it always will be for `container.escape`), this isn't hit - the envelope does the wrapping. The existing else branch already returns the command unmodified for non-pod targets. No change needed - just noted for awareness.

### Step 7: `wrap_command_for_hops` - binary grounding for nodes

**File: `crates/campaign/src/campaign/execution.rs`** - `wrap_command_for_hops()` (~line 430)

Currently only grounds binaries against pods. After an escape, the "source" might be a node entity. Update to use `get_system_entity()` which covers both pods and nodes:

```rust
// Ground binaries against the target's known paths
let tgt_id = EntityId::new(tgt);
if let Some(sys) = self.get_system_entity(tgt) {
    procedure.command = ground_binary_in_cmd(&procedure.command, &sys.entity().system().binaries);
}
```

Same pattern for the source binary grounding at line ~464.

### Step 8: Tests

**File: `crates/campaign/src/effects.rs`** - add tests:

- `container_escape_creates_relation_with_envelope` - verify relation + envelope extraction from `PROCEDURE_CMD`
- `container_escape_wrong_arg_count` - error case

**File: `crates/campaign/src/campaign/tests.rs`** - integration test:

- Insert a pod with a `RunsOn` to a node, add a `ContainerEscape` relation pod->node with an nsenter envelope, verify `resolve_exec_channel()` can route to the node, verify command wrapping produces `nsenter ... ${CMD}` envelope.

---

## Phase 2: Interactive Sessions (Bind/Reverse Shell, Implant)

When a container escape establishes a **real shell** on the host (not just an envelope), the shell needs to be registered as a C2 backend so commands can be sent directly to the node.

### Concept

The escape TTP runs inside the container and opens a reverse shell or deploys an implant on the host. That shell connects back to a Ran-side listener (or Ran connects to a bind shell). This creates a new `C2Backend` instance that can execute commands on the node directly - no envelope wrapping, no routing through the container.

### Step 9: C2 - Dynamic backend registration

**File: `crates/c2/src/executor.rs`**

Add a method to `C2Manager` for runtime registration:

```rust
pub fn register_backend(&mut self, id: String, backend: Arc<dyn C2Backend>) {
    self.backends.insert(id, backend);
}
```

Expose through `C2Handle` (the sender side) so the campaign can register backends after a TTP executes.

### Step 10: C2 - `ShellSession` backend

**New file: `crates/c2/src/shell_session.rs`**

A generic `C2Backend` implementation that wraps a live shell connection:

```rust
pub struct ShellSession {
    kind: ShellKind,        // BindShell { addr } | ReverseShell { conn } | Implant { client }
    target_entity_id: String, // "node/<name>" - for display/debugging only
}

#[async_trait]
impl C2Backend for ShellSession {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted { ... }
}
```

This is a generic shell session backend - not escape-specific. It could also be used for Sliver sessions, bind shells on pods, etc. The escape just happens to be the delivery mechanism.

### Step 11: Campaign - effect handler for `escape.shell`

**File: `crates/campaign/src/effects.rs`**

New effect: `escape.shell(${SRC}, ${NODE_ID})`

Unlike `container.escape` (which stores an envelope), this effect:
1. Creates a `ContainerEscape` relation with `backend_id` instead of `envelope`
2. Signals the C2 layer to expect/accept an incoming shell connection for the node

This requires the effect handler to communicate with the C2 layer - a new `FactsUpdate` field or a side-channel message.

### Step 12: Graph - entity association via graph, not backend

The same `backend_id` that was originally associated with the pod (the container channel) is NOT reused. The live shell is a NEW backend with its own `backend_id` like `"shell/node/<name>/<uuid>"`. The `ContainerEscape` relation carries this `backend_id`. When `resolve_exec_channel()` finds the escape edge, it returns an `ExecChannel` with this backend_id and no hops - commands go directly to the shell.

The graph is the source of truth for which entity a session is "on":
- Before escape: `C2Server -> [PodExec] -> Pod` - session is on the pod
- After escape: `Pod -> [ContainerEscape] -> Node` - session is on the node
- `resolve_exec_channel()` for the node returns the shell backend - no hops needed

### Step 13: `resolve_exec_channel` updates

**File: `crates/campaign/src/campaign/state.rs`** - `resolve_exec_channel()`

When target is a node, check for `ContainerEscape` edges incoming to the node. If one exists with a `backend_id`, return `ExecChannel::direct(backend_id)`. If one exists with an envelope, return the standard hop-wrapped channel through the source pod.

---

## Critical files to modify

| File | Phase | Change |
|---|---|---|
| `crates/domain/relations.rs` | 1 | Add `ContainerEscape` struct |
| `crates/domain/mod.rs` | 1 | Export `ContainerEscape` |
| `crates/graph/src/edge.rs` | 1 | Add `"container.escape"` to `relation_defaults()` |
| `crates/campaign/src/effects.rs` | 1 | Add `container.escape` effect parser |
| `crates/campaign/src/campaign/execution.rs` | 1 | Envelope extraction + node AccessLevel + binary grounding |
| `crates/domain/relations.rs` | 1 | `RelationSummary::from_relation()` envelope extraction |
| `crates/campaign/src/campaign/state.rs` | 2 | `resolve_exec_channel()` for node targets |
| `crates/c2/src/executor.rs` | 2 | Dynamic backend registration |
| `crates/c2/src/shell_session.rs` | 2 | New `ShellSession` C2Backend |

---

## Verification

### Phase 1
1. `cargo test` - all existing tests pass
2. New unit tests in `effects.rs` for `container.escape` parsing
3. Integration test: pod -> node escape with nsenter envelope -> verify command wrapping produces `nsenter -t 1 ... -- <cmd>`
4. Integration test: `resolve_exec_channel()` routes to node through escape edge
5. Verify `direct_foothold_systems()` includes the node after escape

### Phase 2
- Deferred; depends on a concrete shell transport (TCP listener, Sliver gRPC, etc.)
