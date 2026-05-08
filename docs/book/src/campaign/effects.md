# Effects

An effect is a declaration in a TTP's YAML that tells Ran what the technique
produces when it succeeds. Effects are the bridge between raw command output and
the structured knowledge graph.

## Why effects matter

Without effects, each TTP execution is a dead end — you see the output, but Ran
doesn't know what it means. With effects, a successful *Create Admin Role* run
produces a `K8sRole` entity in the graph, which immediately unlocks every technique
that requires a role to exist.

## Declaring effects

```yaml
effects:
  - k8s.serviceAccountList
  - container.escape(sys)
  - c2.session(sliver, sys)
```

Each string is an effect expression. Ran evaluates them after the TTP completes
successfully.

## Simple effects

Simple effects have no arguments. They extract entities from the execution context
(the parameter values that were active when the TTP ran):

| Effect | Entity created | Required context keys |
|---|---|---|
| `k8s.pod` | `Pod` | `Namespace`, `PodName` (optional: `NodeName`, `ServiceAccount`, `IsRunning`) |
| `k8s.serviceaccount` | `ServiceAccount` | `Namespace`, `ServiceAccountName` (optional: `Token`) |
| `k8s.role` | `K8sRole` | `Namespace`, `RoleName` (optional: `Rules` as JSON) |
| `k8s.rolebinding` | `K8sRoleBinding` | `Namespace`, `BindingName` (optional: `RoleRef`, `Subjects` as JSON) |
| `k8s.cronjob` | `CronJob` | `Namespace`, `CronJobName` (optional: `Schedule`) |

`k8s.serviceAccountList`, `k8s.podList`, and similar list-form effects trigger
the **output parser** pipeline — Ran reads the raw command output (expected to be
a Kubernetes JSON list) and extracts individual entities from it automatically.

## Relation effects

Relation effects take positional arguments and create directed edges in the graph:

| Effect expression | Relation created | Description |
|---|---|---|
| `k8s.can-exec(src, tgt)` | `PodExec` | `src` can kubectl-exec into `tgt` |
| `k8s.can-reach(src, tgt)` | `CanReach` | `src` can reach `tgt` over the network |
| `runs-on(pod, node)` | `RunsOn` | `pod` runs on `node` |
| `k8s.kubelet-exec(src, tgt)` | `KubeletExecSource` | `src` can exec on nodes via the kubelet API |
| `container.escape(src)` | `ContainerEscape` + `RunsOn` | `src` has a proven escape to its host node |
| `rce.can-exec(src, tgt)` | `RceCanExec` | `src` has RCE on `tgt` via an exploit chain |
| `c2.session(backend, tgt)` | `SessionChannel` | An active C2 session from `backend` to `tgt` |

### The `sys` placeholder

In relation effects, `sys` is a special token that resolves to the entity the TTP
ran against (i.e. the current target). Use it instead of hardcoding an entity ID:

```yaml
effects:
  - container.escape(sys)    # the pod that performed the escape
  - c2.session(sliver, sys)  # sliver now has a session to the current target
```

### Envelopes

When a relation effect is applied for a technique that *also* establishes an
execution channel — `container.escape`, `rce.can-exec`, or `k8s.kubelet-exec` —
Ran stores the exact grounded command from the procedure as an **envelope** on that
relation. Subsequent commands routed through that relation are wrapped with the
envelope automatically, so the operator doesn't need to re-enter the exploit for
every follow-on technique.

## Effect evaluation order

1. The TTP's procedure runs.
2. Effect strings are collected from the `effects:` list.
3. Each effect is grounded (parameter placeholders substituted).
4. Ran applies the resulting entity and relation updates to the campaign graph.
5. Inference rules run to completion (up to 8 iterations), deriving any additional
   relations that follow logically from the new state.

The full catalog of effects is in the [Effect Catalog](../reference/effects.md).
