# Handoff: OperatorHost as a real system (without the TTP spam)

Bootstrap doc for continuing this work elsewhere. This file is untracked, so it
will not exist in another worktree — copy it out first:

```
cp <this worktree>/docs/operator_host_handoff.md /Users/me/Dev/Ran/docs/
```

Base commit: `564e772f` (Show C2 listener ports as badges; make Stop Listener
work, #61), plus the redirector branch `implement-redirector-enti`, which added
the `c2.has-tool` gate this work is meant to retire. Line anchors below are from
that branch.

## Goal

Make `OperatorHost` carry a real `SystemInfo` — binaries and IPs — **without**
it becoming a match for `requires.kind: System`.

An earlier attempt at "make OperatorHost a regular SystemEntity" was abandoned
because it lit up every host-oriented TTP against the operator's own laptop.
That was not a mistake in the attempt; it is a modelling gap (below).

The two concrete needs driving this:

1. **Calling certain tools** — knowing whether `labctl`, `socat`, `kubectl` etc.
   exist on the machine running Ran.
2. **Getting the local IP** — what a reverse-shell payload has to point at, and
   which nothing in the model can currently express.

## The core insight (do not re-litigate)

Two properties are tangled that are actually independent, and they already live
in **separate code paths**:

| | what it says | kind | consulted by |
|---|---|---|---|
| `SystemEntity` | is a machine with a `SystemInfo` | trait, per **type** | `get_system_entity`, `best_tool_readiness` |
| target-ness | is a thing the engagement acts on | data, per **instance** | `requires.kind: System`, access-level gating |

- Capability: `Campaign::get_system_entity`
  (`crates/campaign/src/campaign/state.rs:307`) looks up `K8sNode` / `Pod` /
  `UnknownSystem` and returns a `CampaignSystemEntityRef`
  (`crates/campaign/src/campaign/entity_refs.rs:39`).
- Target-ness: an entirely separate `matches!` inside `resolve_target_context`
  (`crates/campaign/src/ttp_applicability.rs:150`).

**The spam came only from the second one.** `kind_matches_target_kind`
(`ttp_applicability.rs:327`) treats `"System"` as a wildcard over anything with
`is_system == true`, so every host post-exploitation TTP became applicable the
moment `OperatorHost` qualified.

Nothing forces you to flip both switches. That is the whole trick.

### Why not a "target" trait

Considered and rejected. Target-ness is per-engagement and per-instance: the
same `Pod` type is in play for this run and not the next, and a rented VPS
redirector is the same `UnknownSystem` type as an in-play host. A trait fixes
the answer at the type level, which is the one thing this never is.

The trait you want already exists — `SystemEntity`
(`crates/domain/entities.rs:42`) — and it is correctly per-type.

### Why this is *more* truthful, not less

The truthful model is not "the operator host is a system like any other" —
asserting that is what produced the spam, and it is false. There are two facts:

- it is a machine with an OS, binaries and IPs (**currently unmodelled** — a lie
  of omission);
- it is not something the engagement acts on (**currently correct, but only by
  accident**, because it is not a `SystemEntity` at all).

The defect is that today you can only assert both by asserting neither.

## Work remaining

### Step 1 — the actual change (small)

1. Give `OperatorHost` (`crates/domain/entities.rs:79`, currently `{ name }`) a
   `system: SystemInfo` field and `impl SystemEntity for OperatorHost`.
   `SystemInfo` is at `crates/domain/types.rs:220` and derives `Default`, so
   `#[serde(default)]` keeps old campaign state loading.
2. Add an `OperatorHost` variant to `CampaignSystemEntityRef` and
   `CampaignSystemEntityMut` (`entity_refs.rs:39` / `:55`), and to
   `get_system_entity` / `get_system_entity_mut` (`state.rs:307` / `:321`).
3. **Deliberately do not** add `OperatorHost` to the `is_system` `matches!` at
   `ttp_applicability.rs:150`. Leave a comment there explaining the asymmetry,
   or the next person will "fix" the inconsistency and reintroduce the spam.
4. Populate binaries once at startup (a `which`-style PATH probe over the tools
   the armory mentions) and the local IPs.

That is the whole spam-free capability win.

### Step 2 — retire `c2.has-tool`

The redirector branch added an operator-side tool gate because there was
nowhere to record "labctl is installed here":

- `ttp_operator_tool_satisfied` (`ttp_applicability.rs:430`) and
  `operator_has_tool` (`:449`), wired last into the `ttp_applicable_for_target`
  chain (`:201`).
- Declared as `c2.has-tool: labctl` in
  `armory/TTPs/Resource Development/create_redirector.yaml`.

Once `OperatorHost` has a binary map, `best_tool_readiness`
(`crates/campaign/src/campaign/execution.rs:2648`) answers this through the
normal path and the special case can go. The semantics differ, deliberately —
decide which you want before deleting:

- `c2.has-tool` resolves `PATH` directly, so there is no "unknown": a missing
  tool **withdraws** the action.
- `best_tool_readiness` is generous: `Present` → 1.0, `Unknown` →
  `UNKNOWN_TOOL_READINESS` (0.7), `Absent` → 0.0, and the gate passes anything
  `> 0.0`. An unprobed tool still offers the action.

If the startup probe always runs, `Unknown` never occurs for the operator host
and the two agree. If it can be skipped, they do not.

### Step 3 — the in-play list (later, not now)

Eventually a flat list of **entity ids + CIDRs** describing what is in play.
Today the rule is just "everything except the operator host", so building it now
is not warranted. What makes it cheap later is that target-ness has exactly
**one definition site** — keep it that way, and the change is:

```rust
// ttp_applicability.rs, resolve_target_context
let is_system = <the matches! as today> && campaign.in_play(&entity.entity_id());
```

`kind: System` stays the right vocabulary throughout, so no armory churn when
the rule tightens.

**The decision to make when you get there:** CIDRs only help entities that have
an IP. A `ServiceAccount`, `Secret` or `Namespace` cannot be CIDR-matched, so
membership for those must come from graph containment (an in-play namespace ⇒
its contents are in play). So: does a newly discovered entity **inherit**
membership, or sit unenrolled until told?

Recommendation: inheritance. A live run stays usable, and the guardrail actually
wanted here is "not my laptop", which the operator-host exclusion already gives.
Explicit enrolment is the stronger guardrail but means a freshly discovered pod
silently offers no actions.

Do not derive membership from graph position (e.g. "anything under the
Cluster") — a second C2 or an out-of-scope jump host quietly changes the answer.

## Related thread: a `LocalShell` backend

Deferred deliberately, but it shares the same root cause, and Step 1 is its
prerequisite. Notes so they are not rediscovered:

- **There is no local-shell execution path in the executor at all.**
  `isLocal: true` only means "don't require a remote exec channel"
  (`execution.rs:2536`, `needs_remote_channel`). The command still falls through
  to `select_backend(cmd).execute(cmd)` (`crates/c2/src/executor.rs:689`), which
  defaults to `BuiltinC2` → pod-exec when `exec_system_id` is empty. So an
  unspecified backend silently means "into a container".
- Everything operator-side is therefore a hardcoded control command dispatched
  by string prefix at the top of `execute_command`
  (`crates/c2/src/executor.rs:256`): `c2.read_local_kubeconfig`,
  `k8sSelfSubjectRulesReview`, `c2.kubectl_exec`, `noop`, `c2.listen`,
  `c2.stop-listener`, `c2.port-forward`, `c2.stop-port-forward`.
- `ExecTtp` already has `execution_timeout_seconds`, and its `exec_chain` doc
  already says *"Empty for purely local/C2-side commands"* — the data model has
  a notion of local execution the executor never implements.
- The `C2Backend` trait (`crates/c2/src/executor.rs:110`) already fits a
  `LocalShell`: `async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted`.

Failure modes to design in, learned the hard way on the redirector:

- **Long-running vs one-shot.** `execute() -> TtpExecuted` cannot express
  "started, still running" — exactly why the control commands exist. Needs a
  managed-process concept, which would then subsume `c2.listen` /
  `c2.stop-listener` / `c2.port-forward` / `c2.stop-port-forward` and the two
  bespoke `HashMap`s in `C2Executor`.
- **Output draining.** A child with piped, unread output wedges at ~64KB.
- **Readiness.** "Process started" ≠ "it works"; labctl needed a specific log
  line before the port was actually open.
- **Argument injection — design this in from commit one.** Parameters are
  substituted into a command string today. Once that string is a real local
  command line, a parameter containing `; curl … | sh` executes on the
  *operator's* machine, holding their kubeconfig and cloud creds. Spawn from an
  argv vector, never a shell string.
- **Environment.** Ran's env is not the operator's login-shell env. Be explicit
  about `PATH`, `HOME`, `KUBECONFIG`.
- **stdin.** `Stdio::null()`, or a tool blocks on a prompt nobody sees.

Deliberately rejected: a generic "run this arbitrary shell string" escape hatch.
It reintroduces the injection problem and makes effects unparseable.

## Context worth knowing

- Project convention: a thing with its own actions gets an entity kind +
  relation, not a UI-level filter.
- Project convention: C2 listener/redirector infrastructure setup belongs in
  **Resource Development**, not Command & Control.
- `${TARGET.IP}` grounding is target-scoped, so there is currently no way for a
  payload-generation TTP to say "call back to *here*". An operator host with
  real IPs is what makes that expressible.
- Changing `kind: System` semantics narrows things armory-wide and invisibly —
  nothing errors, an expected action just stops appearing. Worth a test pinning
  which TTPs stay applicable to a known in-play pod.
