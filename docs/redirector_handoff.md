# Handoff: Create Redirector TTP (labctl port-forward)

Bootstrap doc for continuing this work in a worktree. Read by absolute path -
this file is untracked, so it won't exist in a fresh worktree:
`/Users/me/Dev/Ran/docs/redirector_handoff.md`

Base commit: `564e772f` (Show C2 listener ports as badges; make Stop Listener work, #61)

## Goal

Add an armory action that stands up a redirector on an iximiuz training cluster,
forwarding a remote port on the playground back to a local C2 listener:

```
labctl port-forward "$PLAY_ID" -R 0.0.0.0:<RPORT>:localhost:<LISTENER_PORT>
```

`PLAY_ID` is a supplied parameter for now (no auto-discovery - the operator gets
it when starting the playground). `labctl` runs on the same machine as Ran
(confirmed by the user), so spawning it from the C2 executor is the right host.

## Decisions already made (do not re-litigate)

1. **Scope: YAML + Rust control command.** A YAML-only entry does not work -
   see "Gotcha 1" below.
2. **No "latest listener" fallback.** The TTP requires `kind: Listener`, so it is
   only offered when the operator has clicked a listener badge. Rationale: there
   is no source of truth for "newest listener" (see Gotcha 2), and a redirector
   silently pointed at a listener you didn't pick is a bad 2am failure mode. The
   `type: Listener` combobox already lets you switch to any other listener.
3. **Redirector becomes a real entity**, with a separate "Stop Redirector"
   action targeting it - the same treatment #61 gave listeners. Not just a
   `cleanup:` block.

## Gotchas discovered (these cost time; don't rediscover them)

**Gotcha 1 - `isLocal: true` does NOT mean "run on the operator host."**
It only means "don't require a remote exec channel"
(`crates/campaign/src/campaign/execution.rs:2537`, `needs_remote_channel`).
The command still falls through to
`select_backend(cmd).execute(cmd)` (`crates/c2/src/executor.rs`, end of
`execute_command`), which is `BuiltinC2` → pod-exec. There is **no local-shell
execution path anywhere in the executor.** Only hardcoded control commands run
operator-side, dispatched by string prefix at the top of `execute_command`
(`executor.rs:242`):

- `c2.read_local_kubeconfig(...)`
- `k8sSelfSubjectRulesReview(...)`
- `c2.kubectl_exec(...)`
- `noop`
- `c2.listen(port, protocol)` → `executor.rs:368`
- `c2.stop-listener(<listener id>)` → `executor.rs:388`

So `c2.port-forward(...)` must be added to that dispatch chain.

**Gotcha 2 - "latest listener" is unrecoverable today.**
`Listener` carries no timestamp, the entity store is a plain `HashMap`
(`crates/campaign/src/campaign/entity_store.rs:65`, no insertion order), and the
C2 node payload is sorted by port (`crates/api/src/state_conversions.rs`, in
`hosted_listeners`). Hence decision 2.

**Gotcha 3 - `${TARGET}` is resolved client-side, not in Rust.**
`frontend/src/lib/modals/ActionParamsModal.svelte:400`. For a non-`string` param
type it becomes the **entity id** (`listener/tcp/4444`); for `type: string` it
becomes the entity **name**. There is no `TARGET` key in the Rust args map -
only `TARGET_ID`, and `ground_entity_ref_vars`
(`crates/campaign/src/grounding.rs:296`) only handles `${TARGET.PROP}` forms.

**Gotcha 4 - `labctl port-forward` is long-running.** Do not `await` it. Spawn
and keep the `Child`. But do poll it for ~1s before reporting success, or a bad
`PLAY_ID` reports a false success (labctl exits non-zero quickly on a bad id).

**Gotcha 5 - signed commits.** `git commit` / `git push` and all `gh` calls need
`dangerouslyDisableSandbox: true` on the first try (ssh-agent is blocked in the
sandbox; the passphrase and TLS errors are red herrings).

## What already exists (built upstream in #61 - reuse, don't rebuild)

Listeners are already real entities with full UI support. This was originally
scoped as part of the redirector work and is now done.

**Domain**
- `crates/domain/entities.rs:131` - `Listener { protocol, port, entry }`.
  Entity id `listener/<proto>/<port>`, kind `"Listener"`, `entity_name()` is the
  canonical `<proto>/<port>` (read via `Listener::entry()`).
- `crates/domain/entities.rs:113` - `C2Server` is now just `{ name }`; the old
  denormalized `listeners: Vec<String>` field is **gone**.
- `crates/domain/entities.rs:198` - `format_listener(port, protocol)`.
- `crates/domain/entities.rs:205` - `listener_port(entry) -> Option<u16>`.
  Uses `rsplit_once('/')`, so it parses `listener/tcp/4444`, `tcp/4444` and a
  bare `4444`. **This is what turns a `Listener` param into a port.**
- `crates/domain/entities.rs:179` - `impl Merge for Listener` is a deliberate
  no-op (protocol + port are the identity).
- `crates/domain/relations.rs:40` - `HostsListener` ("hosts-listener"),
  C2 → Listener. Built with the `structural_relation!` macro at
  `relations.rs:5` - one line to add another such relation.

**Runtime / applicability**
- `crates/campaign/src/runtime.rs:354` - `C2Event::ListenerStarted` inserts the
  `Listener` entity + `HostsListener` relation and publishes `FactsChanged`.
  `C2Event::ListenerStopped` immediately after calls
  `remove_listeners_on_port`. **Mirror this pair for the redirector.**
- `crates/campaign/src/campaign/state.rs:703` - `remove_listeners_on_port`,
  built on the new generic `remove_entity::<T>` at `state.rs:691` (drops the
  graph node and every relation touching it).
- `crates/campaign/src/ttp_applicability.rs:342` - `ttp_exists_satisfied`;
  `:354` is the `exists: [Listener]` arm.
- `crates/campaign/src/ttp_applicability.rs:291` - `ttp_is_applicable_for_target_kind`.
  Note `requires.kind` accepts **either** a string or an array.
- `c2.has-listener` now actually gates (it was silently ignored before #61).

**Executor (the template to copy)**
- `crates/c2/src/executor.rs:19` - `type Listeners = Arc<RwLock<HashMap<u16, AbortHandle>>>`
- `crates/c2/src/executor.rs:83` - the `listeners` field on `C2Executor`
  (initialized at `:163` and `:191`)
- `crates/c2/src/executor.rs:368` - `c2.listen` dispatch
- `crates/c2/src/executor.rs:388` - `c2.stop-listener` dispatch
- `crates/c2/src/executor.rs:429` - `spawn_session_listener`
- `crates/c2/src/executor.rs:465` - `stop_listener` (looks up + aborts the handle,
  publishes `ListenerStopped`, returns a `TtpExecuted`)
- `crates/c2/src/executor.rs:743` - `parse_stop_listener_command`
- `crates/c2/src/executor.rs:676` - `parse_session_listen_command`
- `crates/c2/src/types.rs:103` - `C2Event::ListenerStarted` / `ListenerStopped`
- Existing tests to model new ones on: `parses_stop_listener_control_command`
  (`executor.rs:1125`) and the port-release test around `:1232`.

**Effect parsers**
- `crates/campaign/src/output_parsers/mod.rs:94` (`c2.listen(`) and `:110`
  (`c2.stop-listener(`) - both are event-sourced no-op parsers that return a
  `ParsedEffect` with an empty `FactsUpdate` and an audit line, placed
  **before** the stdout guard so they succeed with no command output.
  **The redirector effects need the same, or they will be logged as unparsed.**

**Frontend**
- `frontend/src/lib/listeners.ts` - `Listener` type, `listenersOf(node)`,
  `allListeners(nodes)`; listeners ride on their C2 node's `listeners` payload.
- `frontend/src/routes/components/listener_badges.svelte` - badge rendering;
  clicking a badge selects the listener and scopes the armory to it.
- `frontend/src/lib/modals/ActionParamsModal.svelte:423` - `type: Listener`
  params render a combobox of every listener in the graph (value = entity id).
- `frontend/src/lib/modals/ActionParamsModal.svelte:629` - the kind list that
  gates entity-typed params; `'Listener'` was added there.
- `crates/api/src/state_conversions.rs` - `hosted_listeners` /
  `attach_hosted_listeners` fold listeners into their C2's node payload
  (sorted by port) rather than drawing them as graph nodes.
- Note: the `node[kind="Listener"]` selector was **removed** from
  `graph_style.ts` in #61 (listeners are badges, not nodes), but
  `elk_layout.ts:35` still has a stale `Listener: 2` rank entry.

## TTP schema reference

- Parser: `crates/armory/src/raw.rs` (`RawTtp` → `Ttp`). Model:
  `crates/armory/src/model.rs:15` (`Procedure`), `:62` (`Ttp`).
- `raw.rs:48` - `isLocal` / `isLocalCommand` are both aliases of the same field.
- `preconditions` is an alias for `requires`; `rbac` is normalized to
  `rbacPermissions` and `resource` → `resourceType`.
- A bare top-level `command:` becomes a single procedure with id `default`.
- Procedure id falls back to `id` → `key` → `tool` → `proc-<n>`.
- Tactic falls back to the parent directory name.
- Good modern examples: `armory/TTPs/Resource Development/create_listener.yaml`
  and `stop_listener.yaml` (the latter was rewritten in #61).

`stop_listener.yaml` is the closest model for the new files:

```yaml
id: stop-listener
name: Stop Listener
tactic: Resource Development
parameters:
  ListenerID:
    type: Listener
    default: "${TARGET}"
    description: the listener to stop, as <protocol>/<port> (e.g. tcp/4444)
    required: true
preconditions:
  kind: Listener
  c2.has-listener: true
procedures:
  - id: ran
    command: "c2.stop-listener(${ListenerID})"
    isLocal: true
effects:
  - "c2.stop-listener(${ListenerID})"
```

## Work remaining

### 1. `Redirector` domain entity - `crates/domain/entities.rs`

Model on `Listener` (`entities.rs:131`). Fields: `play_id`, `remote_port`,
`listener_port`, and a private canonical `entry` written once by `new()` (the
same trick `Listener` uses, because `Entity::entity_name` returns a borrow and
cannot format). Kind `"Redirector"`. Suggested id
`redirector/<play_id>/<remote_port>`. `Merge` is a no-op if id fields are the
identity. Add a `listener_port`-style helper if parsing back out of the id.

Export from `crates/domain/mod.rs` (both the `entities::{…}` and
`relations::{…}` re-export lists).

Relation: one line via `structural_relation!` in `crates/domain/relations.rs`
next to `HostsListener:40` - e.g. `ForwardsTo` ("forwards-to"),
Redirector → Listener (traffic direction).

### 2. C2 events - `crates/c2/src/types.rs:103`

Add `RedirectorStarted { play_id, remote_port, listener_port }` and
`RedirectorStopped { remote_port }` next to the listener events.

### 3. Executor - `crates/c2/src/executor.rs`

- New field mirroring `listeners:83`, e.g.
  `type Redirectors = Arc<RwLock<HashMap<u16, tokio::process::Child>>>` keyed by
  remote port. Initialize at `:163` and `:191`.
- `parse_port_forward_command` / `parse_stop_port_forward_command` next to
  `parse_stop_listener_command:743`.
- Dispatch both in `execute_command` next to `:388`.
- Spawn: `tokio::process::Command::new("labctl")` with args
  `["port-forward", <play_id>, "-R", "0.0.0.0:<RPORT>:localhost:<LPORT>"]`,
  `.kill_on_drop(true)`, stdout/stderr piped. Resolve the listener ref to a port
  with `ran_domain::listener_port`. Poll ~1s (`try_wait`) before reporting
  success so a bad `PLAY_ID` fails honestly; surface labctl's stderr in
  `fail_reason`.
- Stop: remove the `Child` from the map and `kill().await`, publish
  `RedirectorStopped`. Model on `stop_listener:465`.
- Tests: parser unit tests (model on `:1125`) and a spawn/teardown test. Note
  the #61 test lesson - probe the same wildcard address the real thing binds,
  since `SO_REUSEADDR` on BSD stacks lets a specific address coexist with a
  wildcard bind.

### 4. Runtime - `crates/campaign/src/runtime.rs:354`

Handle `RedirectorStarted` (insert `Redirector` + relation, publish
`FactsChanged`) and `RedirectorStopped` (remove the entity). For removal, add a
`remove_redirector_on_port` next to `remove_listeners_on_port`
(`state.rs:703`), reusing the generic `remove_entity::<T>` at `state.rs:691`.

### 5. Effect parsers - `crates/campaign/src/output_parsers/mod.rs:94`

Add event-sourced no-op arms for `c2.port-forward(` and
`c2.stop-port-forward(`, mirroring `:94` and `:110`. Must sit before the stdout
guard.

### 6. Armory YAML

**Rewrite `armory/TTPs/Resource Development/create_redirector.yaml`.** It is
currently stale pre-#61 schema (`command: "CreateRedirector"`, which has no
implementation anywhere - grep confirms zero hits - plus `exists: [Listener]`).
Target shape:

```yaml
id: create-redirector
name: Create Redirector
description: >
  Forward a port on an iximiuz training cluster back to a local C2 listener,
  so implants inside the playground reach the listener without a direct route
  to the operator host.
tactic: Resource Development          # per project convention: C2 infra setup
techniques: ["Acquire Infrastructure"]
parameters:
  PLAY_ID:
    type: string
    required: true
    description: iximiuz playground id, from `labctl playground start`
  RPORT:
    type: string
    default: "1337"
    description: port to open on the playground host (bound 0.0.0.0)
  LISTENER:
    type: Listener
    default: "${TARGET}"
    required: true
    description: the local listener to forward to, as <protocol>/<port>
preconditions:
  kind: Listener
  c2.has-listener: true
procedures:
  - id: labctl
    command: "c2.port-forward(${PLAY_ID}, ${RPORT}, ${LISTENER})"
    isLocal: true
effects:
  - "c2.port-forward(${PLAY_ID}, ${RPORT}, ${LISTENER})"
references:
  - https://labs.iximiuz.com/
```

**New `armory/TTPs/Resource Development/stop_redirector.yaml`** -
`kind: Redirector`, param `RedirectorID` with `default: "${TARGET}"`, procedure
`c2.stop-port-forward(${RedirectorID})`, `isLocal: true`.

### 7. Frontend

Decide how a `Redirector` renders. Two options:

- **Badge on the C2**, like listeners - reuse `hosted_listeners` /
  `attach_hosted_listeners` in `crates/api/src/state_conversions.rs` and the
  `listener_badges.svelte` pattern. Least new code.
- **Its own graph node** - arguably more informative, since a redirector is a
  genuinely separate network hop. Needs a `graph_style.ts` selector, an
  `elk_layout.ts` rank, and an svg in `frontend/static/`.

Either way, add `'Redirector'` to the entity-kind list at
`ActionParamsModal.svelte:629` so a `type: Redirector` param renders, and add a
`type: Redirector` combobox branch near `:423`.

Also worth doing while in here: drop the stale `Listener: 2` entry in
`elk_layout.ts:35` (listeners are badges now, not nodes).

## Context worth knowing

- `scripts/ixiplay.sh` already resolves a `PLAY_ID` by name from
  `labctl playgrounds list` and starts a playground if none is running - the
  reference for a future auto-discovery step (explicitly out of scope now).
- `scripts/iximiuz_update_ran.sh` uses `labctl playground list -f playground=<name> -q`,
  a terser query form.
- Project convention (established): C2 listener/redirector infrastructure setup
  belongs in **Resource Development**, not Command & Control.
- Project convention (established): a thing with its own actions gets an entity
  kind + relation, not a UI-level filter. This is why the redirector is an
  entity.
