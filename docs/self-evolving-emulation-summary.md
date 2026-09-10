# Self-Evolving Adversary Emulation - Architecture & Starting Plan

A design summary for adding continuous self-improvement to a Kubernetes adversary-emulation tool (Rust codebase, API server + state, an armory of actions, a world model). The agent drives emulations *and* helps evolve the tool; agents are treated as a means, not the architecture.

---

## 1. Guiding principles (the conceptual spine)

- **The agent is a policy, not the architecture.** The tool is the center of gravity. An LLM agent is one implementation of `policy(observation) → action`; behaviour trees and active inference are others. Design so the driver is swappable.
- **Self-evolution is only as reliable as the verifier behind the generation.** The model proposes; a verifier decides whether the proposal is real. Order all work by verifier cost - start where the verifier is cheap and hard, build the verifier *before* generation where it is expensive or fuzzy.
- **Generation outputs artifacts, never weights or prompt-state.** Evolved capability must land as data/code the tool (and a future non-LLM driver) can read. This is the one cheap discipline that keeps the system "LLM-now" without becoming "LLM-forever."
- **Molten → frozen.** Provisional artifacts are hot-loaded at runtime to unblock a run; a human QA gate promotes them to the canonical, versioned, trusted form. (This is the loop you already run for parsers.)

---

## 2. MCP vs. own agent vs. Skills - they are different layers

These are not alternatives; they answer different needs:

| Layer | What it is | Needed when |
|---|---|---|
| **Skills** | Knowledge / instructions (e.g. `SKILL.md`) | Encoding domain methodology; only a *format* if you own the agent |
| **MCP server** | The interface - exposes state + armory as callable tools | An *external* agent must drive the tool (interop) |
| **Your own agent** | The control loop - plans and chooses actions | You ship a self-contained product agent |

If you own both ends (closed product), you can skip MCP and Skills-as-a-format. Two engineering concerns survive regardless and you will hand-roll them if you DIY:

1. **Interop** - the moment anything external drives the sim, you are re-inventing MCP.
2. **Context cost at scale** - a large armory forces progressive disclosure (tiered tool exposure / retrieval), which is the idea Skills encode.

---

## 3. Driver-agnostic surface (protects the BT / active-inference outlook)

Expose an **environment / POMDP-style surface**: `observe → enumerate available actions (with preconditions) → execute → observation`. Every driver is a policy over it.

- Make **observations partial**, not a ground-truth state dump - the attacker doesn't have ground truth, and partial observation is what active inference wants anyway.
- Decouple **procedure (operator with effect)** from **realization (mechanism)** - this is exactly the operators-with-effects shape a planner (BT/active inference) needs.

Self-evolution makes the action space **non-stationary**, which is hostile to BTs and active inference. Resolve with **versioning**: the armory is stationary *within a version*; evolution only advances versions at assimilation boundaries; drivers pin a version.

---

## 4. The two kinds of gap

The tool faces two orthogonal axes, each split by epistemic mode and verifier cost.

### Axis A - Interpretation ("what happened?")

Making sense of action output: success / tool-missing / blocked-by-control / partial. Across distros and shells.

- **Mode: crystallize** (the knowledge exists, latent in the LLM + recoverable from real output).
- **Do not** distill the LLM's *imagination* of what a failure looks like (stale, approximate). **Do** distill the LLM as an *interpreter of real captured output*, into a deterministic classifier. The environment is ground truth; the LLM is the labeler.
- Open-world tail: add an explicit **`unknown` class** that escalates to the LLM at runtime - and that escalation is the trigger to harvest the new sample and grow the corpus.
- **Verifier:** agreement with a labeled corpus harvested from real outputs.

### Axis B - Capability ("what can the tool do at all?")

Two distinct sub-problems, often bundled as "an epistemic gap" but at very different verifier cost:

- **Missing realization** (e.g. action assumes `curl`, but the container has none). This is a *binding* problem, not a mystery - see §5.
- **Missing effect / domain knowledge** (e.g. NetworkPolicy not modeled; `hostNetwork` pods are exempt). This is genuine discovery.

For discovery: the LLM's value is **knowing what to test (the hypothesis space)**, not the answer - CNI-specific behavior (Calico/Cilium/Antrea, version gaps) is the *least* reliable part of model knowledge, and ground truth is the point of the tool.

- Probe actions are **controlled experiments**, not one-shot probes:
  - *hypothesis* (LLM): hostNetwork bypasses egress NetworkPolicy
  - *setup*: deny-egress policy in a test namespace
  - *control*: normal pod egress → predict **BLOCKED**
  - *treatment*: hostNetwork pod egress → predict **ALLOWED**
  - *infer*: control-blocked **and** treatment-allowed ⇒ confirmed
- **The control condition is non-negotiable.** Without "the normal pod was actually blocked," an allowed treatment may just mean the policy never enforced - you'd learn a superstition.
- **Output is two artifacts:** the reusable probe action, *and* a **world-model rule** ("hostNetwork ⇒ exempt"). The rule is the more valuable one - it lets the driver reason about the bypass - and it is **defeasible**: tag it with the environment it was confirmed under (CNI, version). A rule learned on Calico must not silently apply on Cilium.

---

## 5. The capability axis in detail: decouple procedure from realization

A `curl`-not-found failure means the action **baked the mechanism into the intent**. Fix it structurally:

- **Procedure** = intent + parameters + a defined, observable **effect** (e.g. "deliver bytes B to host:port", "read the SA token"). Preconditions on *world state*.
- **Realization** = mechanism (`curl` | `wget` | `bash /dev/tcp` | `python` socket | `nc`). Preconditions on *environment capability* (requires-curl, requires-bash, …).

"Use something other than curl" then becomes selecting the realization whose environment-preconditions hold. The interpreter (Axis A) **routes** by failure class:

- `capability-missing` → try another realization, else synthesize one. **Cheap.**
- `blocked-by-control` → not a realization problem; needs world-model discovery (a *bypass* procedure). **Expensive.**

**Realizations have a near-free verifier - effect-equivalence:** a new mechanism is correct iff it produces the procedure's defined effect (did the bytes land at the listener?). So realization-synthesis fits the same molten → QA → frozen loop as parsers.

Capability-axis queue, by verifier cost:

1. **Realization selection** - registry of mechanisms with capability-preconditions; pick one that fits. Often the gap is just an untried `wget`. Needs lightweight, read-only **capability probes** (what's in `PATH`, is there a shell, is `python` present).
2. **Realization synthesis** - no existing mechanism fits; author one, verify by effect-equivalence. *(The cheap epistemic gap.)*
3. **Net-new effect** - the tool has no notion of the technique; define the effect, design a controlled experiment. *(The expensive epistemic gap - the real frontier.)*

---

## 6. The world model: a binding-time problem, not a structural one

Current world model = **analyzers** (observation→fact) + **inference rules** (fact→fact) + **entities/relations**, all compiled Rust. The decomposition is already correct - analyzers vs rules maps exactly onto Axis A vs Axis B. The only gap: **it is all compiled, so it cannot grow at runtime.**

Move the **inference rules** from *code the tool is* to *data the engine evaluates*. The engine stays Rust; the rule base becomes loadable, versioned data.

- Rules-as-data is also **required for defeasibility metadata** - per-rule provenance, environment scope, confidence - which cannot be bolted onto a hardcoded Rust rule as queryable data.
- Declarative rules can be **checked for contradiction** against the existing base before promotion - a consistency gate impossible with imperative rules.

What migrates, and what doesn't (Occam):

| Component | Disposition |
|---|---|
| **Inference rules** | → data (high change rate, low structural risk - do first) |
| **Entities / relations** | → stay Rust types for now; a new entity = human-gated schema bump |
| **Analyzers** | → mostly code; only the open-world classifier tail becomes loadable |

A discovered rule splits by tier: a rule over **existing** entities can land autonomously through the gate; a rule needing a **new** entity escalates to a human + schema change.

**Recursion is the fork** between a real Datalog engine and a flat evaluator: transitive attack-path reachability is recursive and would earn a fixpoint engine its keep.

---

## 7. Runtime-evaluable engine vs. compile-time (Ascent)

| | Compile-time (Ascent, Crepe) | Runtime engine (Cozo / hand-rolled / differential) |
|---|---|---|
| Rules are… | proc-macro, generated Rust, frozen in binary | data, parsed & evaluated at runtime |
| Type check | **compile-time, against domain types** | load-time validation at best |
| Change a rule | edit source + recompile | hot-reload |
| Introspect / version / provenance | no | yes |
| Speed | fast | interpretation overhead (usually irrelevant - loop is I/O-bound) |

**Decision (given parsers are frozen to Rust for *trust + type integration*, not speed): assimilate rules to Ascent, mirroring parsers.** Ascent delivers type integration for rules - a rule naming a nonexistent relation or mistyping a binding is a *compile error*. This yields **one type system across the whole world model**: parsers emit typed facts, rules consume/derive typed facts, one compiler checks both. A runtime rule engine would fracture this into a second type universe - exactly what "type integration" exists to prevent.

- Recompile-on-promotion is benign: promotion is a **gated release event**, not a runtime operation. All during-emulation agility lives in the molten tier.
- **Two-stage pipeline, one-way seam:** canonical rules (Ascent, compiled) run their fixpoint → emit facts → provisional rules (runtime engine, hot-loaded) consume those facts. Dependency only flows canonical → provisional (canonical rules predate provisional ones, so cannot reference them). At the QA gate a confirmed provisional rule moves into Ascent source on the next release.
- **Lighter fallback** (if the two-evaluator seam stops paying off): one runtime engine that validates canonical rules against the schema at load time - keeps most of the type-integration value, loses the split, at a weaker guarantee and a different assimilation philosophy than parsers.

*(If the parser freeze were about speed rather than type integration, the call would flip to a single runtime engine - rule evaluation is microseconds against an I/O-bound loop.)*

---

## 8. Recommended starting point

**You already have the loop - for parsers** (hot-reload at runtime → human QA gate → assimilate to Rust). The missing piece is the **authoring step**: if hot-loaded parsers are hand-written today, have the *system* author the provisional one at the moment of failure. Close that one gap, on existing machinery, on the cheapest-verifier case.

First loop (reuses the whole parser pipeline, adds one capability):

1. **Tap the failure you already detect.** A parse failure (threw / invalid output / interpreter returned `unknown`) is the trigger - already an error path in your code. Capture raw sample + context (distro, shell, action, exit code, stderr) as a structured gap event.
2. **Generate into the format you already hot-reload.** On the event, the LLM authors a provisional parser in the existing runtime-loadable form.
3. **Verify before use** - `generate → validate → repair`: run against the captured sample (schema-valid? agrees with label?). On failure feed the error back, retry N times, drop to a human queue. The verifier is near-free because the artifact is a pure function and you hold the sample.
4. **Use provisionally, frozen** - hot-load to unblock the run; version + provenance-tag immediately (sample, model, context) for replayability.
5. **Feed the existing QA gate** - provisional artifact + its sample (now a regression fixture) flow into the human QA → Rust assimilation. *Self-improvement that writes its own tests.*

**First milestone:** not "the system learned NetworkPolicy," but *"the system authored a parser, it passed the verifier, a human promoted it."* Once that round-trips once - with zero new infrastructure and zero cluster risk - the loop exists. Everything after is widening it.

Keep the synthesizer a **narrow function, not "an agent."** A failure fires it; the driving agent isn't involved yet. The "one agent that drives and evolves" vision is the *limit* of widening this, not the thing built first.

---

## 9. Roadmap - widen rightward along verifier cost

| Step | Verifier | Infra cost |
|---|---|---|
| 1. Parsers (start here) | sample + schema (free) | none - existing machinery |
| 2. Failure-mode classifiers | labeled corpus; add `unknown` escalation | low |
| 3. Realization selection / synthesis | effect-equivalence | medium - capability probes, realization registry |
| 4. Read-only probe actions | controlled experiment | **high - experiment harness, rules-as-data engine, Ascent assimilation** |
| 5. State-changing actions | sandbox + effect check | highest - isolation required |

The big infrastructure bills (experiment harness with controls, rules-as-data runtime engine, Ascent assimilation) come due at step 4 - deliberately deferred until the loop concept is proven cheaply.

---

## 10. Open decisions

- **Does the armory bake mechanism into the action today, or is there already a seam between "what" and "how"?** Decides whether the procedure/realization split (§5) is a one-time refactor or mostly done.
- **Does the driver need transitive reachability** (chained bypasses), or only single-step derived facts? Decides Datalog-proper vs. a flat evaluator (§6).
- **Two-evaluator pipeline vs. single runtime engine + load-time validation** for rules (§7) - default to the former for consistency with parsers; fall back if the seam isn't worth it.
