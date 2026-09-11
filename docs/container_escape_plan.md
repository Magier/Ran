# Container Escape - Interactive Sessions (remaining work)

## Context

Ran models attack paths through Kubernetes clusters as a knowledge graph of entities and relations. Execution channels (`PodExec`, `KubeletExecSink`, `RceCanExec`) let the campaign route commands to compromised systems, and a container escape moves execution from a compromised pod to its underlying host node.

Phase 1 of this plan shipped: the envelope-based `ContainerEscape` relation (nsenter/chroot wrapping, the `container.escape` effect, node `AccessLevel` updates, hop wrapping and binary grounding for node targets). See `crates/domain/relations.rs`, `crates/campaign/src/effects.rs` and `crates/campaign/src/campaign/execution.rs` for the implementation.

What follows is the part that is still open: escapes that establish a live shell on the host rather than wrapping each command in an envelope.

---

## Interactive Sessions (Bind/Reverse Shell, Implant)

When a container escape establishes a **real shell** on the host (not just an envelope), the shell needs to be registered as a C2 backend so commands can be sent directly to the node.

### Concept

The escape TTP runs inside the container and opens a reverse shell or deploys an implant on the host. That shell connects back to a Ran-side listener (or Ran connects to a bind shell). This creates a new `C2Backend` instance that can execute commands on the node directly - no envelope wrapping, no routing through the container.

### Step 1: C2 - Dynamic backend registration

**File: `crates/c2/src/executor.rs`**

Add a method to `C2Manager` for runtime registration:

```rust
pub fn register_backend(&mut self, id: String, backend: Arc<dyn C2Backend>) {
    self.backends.insert(id, backend);
}
```

Expose through `C2Handle` (the sender side) so the campaign can register backends after a TTP executes.

### Step 2: C2 - `ShellSession` backend

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

### Step 3: Campaign - effect handler for `escape.shell`

**File: `crates/campaign/src/effects.rs`**

New effect: `escape.shell(${SRC}, ${NODE_ID})`

Unlike `container.escape` (which stores an envelope), this effect:
1. Creates a `ContainerEscape` relation with `backend_id` instead of `envelope`
2. Signals the C2 layer to expect/accept an incoming shell connection for the node

This requires the effect handler to communicate with the C2 layer - a new `FactsUpdate` field or a side-channel message.

### Step 4: Graph - entity association via graph, not backend

The same `backend_id` that was originally associated with the pod (the container channel) is NOT reused. The live shell is a NEW backend with its own `backend_id` like `"shell/node/<name>/<uuid>"`. The `ContainerEscape` relation carries this `backend_id`. When `resolve_exec_channel()` finds the escape edge, it returns an `ExecChannel` with this backend_id and no hops - commands go directly to the shell.

The graph is the source of truth for which entity a session is "on":
- Before escape: `C2Server -> [PodExec] -> Pod` - session is on the pod
- After escape: `Pod -> [ContainerEscape] -> Node` - session is on the node
- `resolve_exec_channel()` for the node returns the shell backend - no hops needed

### Step 5: `resolve_exec_channel` updates

**File: `crates/campaign/src/campaign/state.rs`** - `resolve_exec_channel()`

When target is a node, check for `ContainerEscape` edges incoming to the node. If one exists with a `backend_id`, return `ExecChannel::direct(backend_id)`. If one exists with an envelope, return the standard hop-wrapped channel through the source pod.

---

---

## Critical files to modify

| File | Change |
|---|---|
| `crates/campaign/src/campaign/state.rs` | `resolve_exec_channel()` for node targets |
| `crates/c2/src/executor.rs` | Dynamic backend registration |
| `crates/c2/src/shell_session.rs` | New `ShellSession` C2Backend |

---

## Verification

Deferred; depends on a concrete shell transport (TCP listener, Sliver gRPC, etc.).
