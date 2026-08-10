# Targeting

Most TTPs run *against* a specific cluster resource — a pod, a service account, a
node. Ran calls this the **target entity**. The target determines which resource
receives the technique's commands and whose credentials are used for API calls.

## Targeting syntax

On the CLI, targets are specified as `<namespace>/<name>`:

```sh
ran invoke get-pods --target default/compromised-pod
ran invoke get-secrets --target production/attacker-sa
```

In the web UI, click any entity in the cluster map to select it as the current
target before invoking a TTP.

## Entity kinds as targets

Different TTPs expect different target kinds. A Discovery TTP that runs `kubectl`
inside a pod requires a **Pod** target. A technique that tests API server permissions
for a captured service account requires a **ServiceAccount** target.

The armory's [preconditions](../campaign/preconditions.md) define what kind each
TTP expects. In the UI, selecting a target automatically filters the armory to show
only techniques applicable to that entity type.

## Operator-side techniques

Some tactics run on the operator side rather than against an existing system:

- **Resource Development** — setting up C2 infrastructure runs on the operator's
  machine, not inside the cluster
- **Lateral Movement** — the movement itself establishes the new foothold

Initial Access through an external kubeconfig targets the chosen **Pod**. Agents
and plans may select a live candidate before it is present in campaign knowledge;
Ran stages only that Pod and grants access only after the TTP succeeds.

## Special entity IDs

In [effects](../campaign/effects.md) and some parameter defaults, `sys` refers to
the entity the TTP is currently running against. You'll see this in effect strings
like `container.escape(sys)` — meaning "the escape happened from the current target."
