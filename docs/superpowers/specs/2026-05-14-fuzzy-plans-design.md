# Fuzzy Plan Execution

**Date:** 2026-05-14
**Status:** Approved for implementation

## Overview

A plan is a YAML/JSON document that describes a sequence of TTP executions against targets, with fuzzy target resolution, conditional execution, and parallelism. Plans are executed by an online planner that continuously resolves targets and evaluates conditions against the live campaign graph as the emulation proceeds.

The format is inspired by the MITRE ATT&CK Flow concepts (action → condition → branching) without requiring full STIX 2.1 compliance.

There is no separate asset registry. Each step declares its target inline; the planner resolves it against the live campaign graph at dispatch time.

---

## Plan Document Format

### Top-level structure

```yaml
id: nginx-container-escape
name: Nginx Container Escape
description: Exec into nginx pod, verify capabilities, escape to host
version: "1.0"

steps:
  - <StepDefinition>
  ...
```

### Step definitions

```yaml
steps:
  - id: exec_pod
    action: k8s.exec-into-pod        # TTP ID from the armory
    target:
      kind: Pod                      # entity kind: Pod | Node | ServiceAccount | ...
      namespace: default             # optional; omit to match any namespace
      name: "nginx-.*"               # regex against the name component of entity IDs
      select: random                 # random (default) | first | all
    args:
      interactive: "true"            # passed through to ExecuteActionRequest.args
    procedure: stealth-exec          # preferred procedure_id; falls back if not applicable
    retry: next_procedure            # on failure, try next applicable procedure in TTP order
    depends_on:
      - step: prior_step
        require: success             # hard: prior_step must have succeeded (exit code 0)
      - step: other_step             # soft: wait for completion, any outcome
      - graph: "step:prior_step has:rce.can-exec"   # graph predicate on a prior step's target
```

#### `target` fields

| Field | Required | Description |
|---|---|---|
| `kind` | yes | Entity kind to match (case-insensitive) |
| `namespace` | no | Namespace filter; omit to match across all namespaces |
| `name` | yes | Regex pattern matched against the name component of the entity ID |
| `select` | no | `random` (default) \| `first` \| `all` |

`name` is a full regex (e.g., `nginx-.*`, `^jump-[a-z0-9]+$`). The planner matches it against the name component of entity IDs: `ns/{ns}/pod/{name}` → matches against `{name}`.

#### `depends_on` reference

| Form | Meaning |
|---|---|
| `step: X` | Wait for step X to complete (any outcome) |
| `step: X, require: success` | Step X must have `success: true` |
| `step: X, require: any_success` | At least one fan-out instance of X succeeded |
| `step: X, require: all_success` | All fan-out instances of X succeeded |
| `graph: "step:X has:<relation>"` | The entity targeted by step X must have this relation in the campaign graph |
| `graph: "step:X all_have:<relation>"` | All entities targeted by step X (fan-out) must have this relation |

All `depends_on` entries are AND-ed.

#### `retry` options

| Value | Behaviour |
|---|---|
| `next_procedure` | On step failure, advance to the next applicable procedure in the TTP's ordered list |
| _(absent)_ | No retry; step is marked failed immediately |

### Full example

```yaml
id: nginx-recon-and-escape
name: Network Recon then Nginx Escape
version: "1.0"

steps:
  # Recon and enumeration run in parallel (no depends_on)
  - id: net_discovery
    action: recon.network-scan
    target:
      kind: Pod
      namespace: default
      name: "jump-.*"
      select: random
    args:
      subnet: "10.0.0.0/24"

  - id: enum_secrets
    action: k8s.list-secrets
    target:
      kind: Pod
      namespace: default
      name: "jump-.*"
      select: random

  - id: enum_env
    action: container.read-env
    target:
      kind: Pod
      namespace: default
      name: "jump-.*"
      select: random

  # Exec into nginx — waits for net_discovery to have populated the graph
  # (stays Pending until a Pod matching nginx-.* appears in the campaign)
  - id: exec_nginx
    action: k8s.exec-into-pod
    target:
      kind: Pod
      namespace: default
      name: "nginx-.*"
      select: first
    args:
      interactive: "true"
    procedure: stealth-exec
    retry: next_procedure
    depends_on:
      - step: net_discovery
        require: success

  # Check capabilities — hard dep on exec succeeding
  - id: check_caps
    action: container.check-capabilities
    target:
      kind: Pod
      namespace: default
      name: "nginx-.*"
      select: first
    depends_on:
      - step: exec_nginx
        require: success

  # Escape — hard dep on check_caps AND graph condition on exec_nginx's target
  - id: escape
    action: container.escape-to-host
    target:
      kind: Pod
      namespace: default
      name: "nginx-.*"
      select: first
    depends_on:
      - step: check_caps
        require: success
      - graph: "step:exec_nginx has:rce.can-exec"

  # Post-enum join — waits for both enum steps (soft deps)
  - id: post_enum
    action: reporting.summarize
    target:
      kind: Pod
      namespace: default
      name: "jump-.*"
      select: random
    depends_on:
      - step: enum_secrets
      - step: enum_env
```

---

## Target Resolution

Step targets are resolved against the campaign `EntityStore` at the moment a step becomes eligible for dispatch.

**Resolution algorithm:**

1. Filter entities by `kind` (case-insensitive)
2. Filter by `namespace` if specified
3. Apply regex `name` against the name component of the entity ID (`ns/{ns}/pod/{name}` → match against `{name}`)
4. If no entities match → step remains `Pending`; re-evaluated after each subsequent step completes
5. Apply `select` strategy to the matching set

**Select strategies:**

| strategy | behaviour |
|---|---|
| `random` (default) | one entity chosen at random |
| `first` | lexicographically first match |
| `all` | fan-out: one `ExecuteActionRequest` per entity, all dispatched in parallel |

**Fan-out and joins:** when a step uses `select: all`, downstream steps that `depend_on` it default to `require: any_success`. Override with `require: all_success` to require every instance succeeded.

---

## Online Dispatch Loop

The planner is **online** — it re-evaluates all pending steps after every step completes and after the campaign graph updates.

```
on plan start:
  build step DAG from depends_on edges
  tick()

tick(campaign):
  for each Pending step:
    if all depends_on satisfied:
      entities = resolve_target(step.target, campaign)
      if entities non-empty:
        dispatch ExecuteActionRequest(s)
        mark step Dispatched

on TtpExecuted(record):
  update step status (Completed or retry)
  if retry=next_procedure and failed:
    find next applicable procedure via armory applicability checks
    if found: re-dispatch with new procedure_id
    if none remain: mark step Failed
  propagate skips to dependents whose only unsatisfied deps were hard-failed steps
  tick(campaign)
```

**Skip propagation:** when a hard requirement (`require: success`) is unmet because the prerequisite is `Failed` or `Skipped`, the dependent step is marked `Skipped`. Skipped steps do not block downstream steps that have alternative satisfied paths.

---

## Execution Engine Architecture

New crate: **`crates/planner/`**, depending on `crates/campaign` and `crates/armory`.

```
crates/planner/
  src/
    model.rs      — PlanDefinition, TargetQuery, StepDefinition, Dependency, SelectStrategy
    resolver.rs   — regex resolution against campaign EntityStore
    executor.rs   — PlanExecutor, dispatch loop, retry logic
    state.rs      — PlanExecutionState, StepStatus
```

### Key types

```rust
// model.rs
pub struct PlanDefinition {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub steps: Vec<StepDefinition>,
}

pub struct TargetQuery {
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,               // regex pattern
    pub select: SelectStrategy,
}

pub enum SelectStrategy { Random, First, All }

pub struct StepDefinition {
    pub id: String,
    pub action: String,
    pub target: TargetQuery,
    pub args: HashMap<String, String>,
    pub procedure: Option<String>,
    pub retry: RetryStrategy,
    pub depends_on: Vec<Dependency>,
}

pub enum Dependency {
    Step { id: String, require: Require },
    Graph { step_ref: String, relation: String, all: bool },
}

pub enum Require { Completion, Success, AnySuccess, AllSuccess }
pub enum RetryStrategy { None, NextProcedure }

// state.rs
pub enum StepStatus {
    Pending,
    Dispatched { exec_ids: Vec<String> },
    Completed { outcomes: Vec<bool> },
    Failed { reason: String },
    Skipped { reason: String },
}
```

### API integration

- `POST /campaigns/{id}/plans` — submit a plan YAML/JSON body, returns a plan execution ID
- `GET /campaigns/{id}/plans/{plan_id}` — returns current `PlanExecutionState`
- Events streamed on the existing `CampaignEvent` bus as `CampaignEvent::PlanStepDispatched`, `PlanStepCompleted`, `PlanStepSkipped`
- MCP tool: `execute_plan` (parallel to the existing `execute_action` tool)

### Procedure applicability

The applicability checks currently in the UI (filter procedures where required tools are absent) move server-side into the planner's retry logic. On first dispatch, the planner selects the first applicable procedure from the TTP's ordered list. On `retry: next_procedure`, it advances through the list using the same applicability functions from `crates/campaign/src/ttp_applicability.rs`.

---

## Open Questions

None — all design decisions resolved during brainstorming.
