# Handoff: OperatorHost as a real system (without the TTP spam)

Status: **Step 1 is done** (commit `dd70b412`, branch `operator-host-regular-sys`).
Step 2 is evaluated and deliberately deferred. Step 3 is untouched. The
`LocalShell` thread at the bottom is now unblocked.

This file is tracked, so it travels with every worktree; the old "copy it out
first" instruction is gone. Line anchors below are from `d1587010` (Create
Redirector, #70) plus `dd70b412`.

## Goal

Make `OperatorHost` carry a real `SystemInfo`, binaries and IPs, **without** it
becoming a match for `requires.kind: System`.

An earlier attempt at "make OperatorHost a regular SystemEntity" was abandoned
because it lit up every host-oriented TTP against the operator's own laptop.
That was not a mistake in the attempt; it is a modelling gap (below).

The two concrete needs driving this:

1. **Calling certain tools**: knowing whether `labctl`, `socat`, `kubectl` etc.
   exist on the machine running Ran.
2. **Getting the local IP**: what a reverse-shell payload has to point at, and
   which nothing in the model could previously express.

## The core insight (do not re-litigate)

Two properties are tangled that are actually independent, and they already live
in **separate code paths**:

| | what it says | kind | consulted by |
|---|---|---|---|
| `SystemEntity` | is a machine with a `SystemInfo` | trait, per **type** | `get_system_entity`, `best_tool_readiness` |
| target-ness | is a thing the engagement acts on | data, per **instance** | `requires.kind: System`, access-level gating |

- Capability: `Campaign::get_system_entity`
  (`crates/campaign/src/campaign/state.rs:368`) looks up `K8sNode` / `Pod` /
  `UnknownSystem` / `OperatorHost` and returns a `CampaignSystemEntityRef`
  (`crates/campaign/src/campaign/entity_refs.rs:39`).
- Target-ness: an entirely separate `matches!` inside `resolve_target_context`
  (`crates/campaign/src/ttp_applicability.rs:169`).

**The spam came only from the second one.** `kind_matches_target_kind`
(`ttp_applicability.rs:346`) treats `"System"` as a wildcard over anything with
`is_system == true`, so every host post-exploitation TTP became applicable the
moment `OperatorHost` qualified.

Nothing forces you to flip both switches. That is the whole trick.

### Why not a "target" trait

Considered and rejected. Target-ness is per-engagement and per-instance: the
same `Pod` type is in play for this run and not the next, and a rented VPS
redirector is the same `UnknownSystem` type as an in-play host. A trait fixes
the answer at the type level, which is the one thing this never is.

The trait you want already exists, `SystemEntity`
(`crates/domain/entities.rs:42`), and it is correctly per-type.

### Why this is *more* truthful, not less

The truthful model is not "the operator host is a system like any other";
asserting that is what produced the spam, and it is false. There are two facts,
and the point of Step 1 was being able to assert them separately:

- it is a machine with an OS, binaries and IPs;
- it is not something the engagement acts on.

Before Step 1 you could only assert both by asserting neither.

## Step 1: done

Landed in `dd70b412`, +431/-16 across 8 files, 8 tests.

1. `OperatorHost` (`crates/domain/entities.rs:87`) carries a `SystemInfo` and
   implements `SystemEntity` (`:125`).
2. `OperatorHost` variants added to `CampaignSystemEntityRef` / `Mut`
   (`entity_refs.rs:39` / `:59`), to `get_system_entity` / `get_system_entity_mut`
   (`state.rs:368` / `:385`), and to `is_system_entity_id`.
3. `OperatorHost` is **deliberately absent** from the `is_system` `matches!`
   (`ttp_applicability.rs:169`). The asymmetry is commented at three sites: the
   `OperatorHost` doc comment, the `TargetContext::is_system` field, and the
   computation site. Two paired tests pin both halves:
   `operator_host_is_a_system_entity_for_capability_lookups` and
   `operator_host_is_not_a_target_of_system_ttps`, the latter also asserting the
   same TTP still applies to an in-play pod, so the exclusion stays targeted
   rather than narrowing `kind: System` generally.
4. Startup populates both maps in `start()` and `trigger()`:
   `local_tool_binaries` (`crates/app/src/lib.rs:1235`) walks `PATH` in-process
   for every tool the armory names, and `local_ips` (`:1302`) records
   non-loopback, non-link-local addresses. New dependency: `if-addrs` on the
   `app` crate, because there is no std API for interface enumeration.

Two deliberate deviations from the original plan:

- **`#[serde(flatten)]`, not a nested field with `#[serde(default)]`.** Flatten
  matches `Pod` / `K8sNode` / `UnknownSystem`, and both
  `prune_entity_payload_for_ui` and the frontend read `binaries` / `ips` /
  `accessLevel` as top-level keys, so nesting would hide them from the UI. The
  old-state-loading concern does not apply: campaign state is never
  deserialized from disk anywhere in the tree.
- **`is_system_entity_id` includes `OperatorHost`.** It answers the capability
  question, so omitting it would have been the real inconsistency. Safe because
  no exec-channel edge ever points at the operator host, so it cannot become a
  lateral-movement foothold; the test asserting `resolve_exec_source()` errors
  at bootstrap still passes.

The probe list is keyed on procedure `tool:` fields only. Do not broaden it to
`procedure_binary_name`, which falls back to the procedure id: operator-side ids
are labels (`ran`, `read-kubeconfig`, `k8s-client`, `copyfail-poc`), and
recording those as `Absent` would withdraw working local actions. One
consequence: `labctl` is declared as `c2.has-tool` in `requires` rather than as
a procedure `tool:`, so it is not probed and stays `Unknown`.

## Step 2: retire `c2.has-tool`. Evaluated, deferred.

`c2.has-tool` (`ttp_operator_tool_satisfied` at `ttp_applicability.rs:449`,
`operator_has_tool` at `:468`, wired last into the `ttp_applicable_for_target`
chain) stays as it is. Do not migrate it to the binary map and do not add a UI
affordance for it unasked.

First, the original premise was wrong. `best_tool_readiness`
(`execution.rs:2779`) cannot absorb this as written, for two reasons: it resolves
`get_system_entity(target_id)` and Create Redirector targets a `Listener`, so
there is no system entity and it returns `1.0`; and `procedure_readiness`
(`:2757`) returns `1.0` before consulting any binary map whenever
`needs_remote_channel` (`:2667`) is false, which is every `isLocal` procedure
and everything under Reconnaissance / Resource Development. So Step 2 is not
deleting a special case, it is a change to the readiness path.

Second, and the actual reason for deferring, three differences that the number
of local tools does not resolve:

1. **Live vs snapshot.** `c2.has-tool` stats `PATH` at applicability time; the
   binary map is a startup snapshot. Installing the tool while Ran runs would
   leave the action withheld until restart, with no hint why.
2. **Strict vs generous.** `c2.has-tool` has no "unknown": missing withdraws the
   action. The readiness path maps `Unknown` to `UNKNOWN_TOOL_READINESS` (0.7)
   and the gate passes anything `> 0.0`. They agree only because the startup
   probe always runs, a coupling between distant code.
3. **On PATH vs usable.** Neither answers what `labctl` actually needs, which is
   a configured and authenticated session. PATH-presence is the wrong question
   for the general case.

`labctl` is also local test tooling for the maintainer rather than a product
surface, so it does not justify the abstraction.

If it is ever revisited, the shape is: `procedure_readiness` takes both the
target's and the operator host's `SystemInfo` and picks by `needs_remote_channel`;
`best_tool_readiness` stops short-circuiting to `1.0` on a non-system target;
and the operator-side branch consults **only** an explicit `tool:`, never
`procedure_binary_name`'s id/command fallback, for the reason given at the end of
Step 1. Blast radius is one file: no operator-side procedure declares a `tool:`
today, so `create_redirector.yaml` would swap `c2.has-tool: labctl` for
`tool: labctl` and nothing else changes.

## Step 3: the in-play list (later, not now)

Eventually a flat list of **entity ids + CIDRs** describing what is in play.
Today the rule is just "everything except the operator host", so building it now
is not warranted. What makes it cheap later is that target-ness has exactly
**one definition site**. Keep it that way, and the change is:

```rust
// ttp_applicability.rs, resolve_target_context
let is_system = <the matches! as today> && campaign.in_play(&entity.entity_id());
```

`kind: System` stays the right vocabulary throughout, so no armory churn when
the rule tightens.

**The decision to make when you get there:** CIDRs only help entities that have
an IP. A `ServiceAccount`, `Secret` or `Namespace` cannot be CIDR-matched, so
membership for those must come from graph containment (an in-play namespace
implies its contents are in play). So: does a newly discovered entity
**inherit** membership, or sit unenrolled until told?

Recommendation: inheritance. A live run stays usable, and the guardrail actually
wanted here is "not my laptop", which the operator-host exclusion already gives.
Explicit enrolment is the stronger guardrail but means a freshly discovered pod
silently offers no actions.

Do not derive membership from graph position (e.g. "anything under the
Cluster"): a second C2 or an out-of-scope jump host quietly changes the answer.

## Related thread: a `LocalShell` backend

Deferred deliberately, but it shares the same root cause, and Step 1 was its
prerequisite, so it is now unblocked. Notes so they are not rediscovered:

- **There is no local-shell execution path in the executor at all.**
  `isLocal: true` only means "don't require a remote exec channel"
  (`execution.rs:2667`, `needs_remote_channel`). The command still falls through
  to `select_backend(cmd).execute(cmd)` (`crates/c2/src/executor.rs:786`), which
  defaults to `BuiltinC2` and then pod-exec when `exec_system_id` is empty. So an
  unspecified backend silently means "into a container".
- Everything operator-side is therefore a hardcoded control command dispatched
  by string prefix at the top of `execute_command`
  (`crates/c2/src/executor.rs:266`): `c2.read_local_kubeconfig`,
  `k8sSelfSubjectRulesReview`, `c2.kubectl_exec`, `noop`, `c2.listen`,
  `c2.stop-listener`, `c2.port-forward`, `c2.stop-port-forward`.
- Consequence worth stating plainly: after Step 1 Ran *knows* which local tools
  exist but still cannot invoke them. `labctl` runs only because it is wired as
  `c2.port-forward`.
- `ExecTtp` already has `execution_timeout_seconds`, and its `exec_chain` doc
  already says *"Empty for purely local/C2-side commands"*, so the data model has
  a notion of local execution the executor never implements.
- The `C2Backend` trait (`crates/c2/src/executor.rs:120`) already fits a
  `LocalShell`: `async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted`.

Failure modes to design in, learned the hard way on the redirector:

- **Long-running vs one-shot.** `execute() -> TtpExecuted` cannot express
  "started, still running", which is exactly why the control commands exist.
  Needs a managed-process concept, which would then subsume `c2.listen` /
  `c2.stop-listener` / `c2.port-forward` / `c2.stop-port-forward` and the two
  bespoke `HashMap`s in `C2Executor`.
- **Output draining.** A child with piped, unread output wedges at ~64KB.
- **Readiness.** "Process started" is not "it works"; labctl needed a specific
  log line before the port was actually open.
- **Argument injection, design this in from commit one.** Parameters are
  substituted into a command string today. Once that string is a real local
  command line, a parameter containing `; curl ... | sh` executes on the
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
- No em-dashes or en-dashes in added lines; `.githooks/pre-commit` rejects them
  in staged additions and also runs `cargo fmt --check` and
  `cargo clippy --workspace -- -D warnings`.
- `${TARGET.IP}` grounding is target-scoped, so there is still no way for a
  payload-generation TTP to say "call back to *here*". The operator host now has
  real IPs, which makes that expressible, but nothing consumes them yet.
- A callback address is **not** derivable from the operator host's IPs. It is a
  property of the return path from a specific target, so route-based or
  interface-based guessing produces a plausible wrong answer when the target
  cannot reach the operator at all. That is what the redirector is for; see
  `redirector_handoff.md`.
- Changing `kind: System` semantics narrows things armory-wide and invisibly:
  nothing errors, an expected action just stops appearing.
