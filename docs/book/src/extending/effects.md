# Effects in Depth

The [Effects](../campaign/effects.md) chapter explains the effects system. This
chapter is a practical guide to writing effects in your custom TTPs.

## When to add effects

Add an `effects:` list whenever the TTP produces something that future techniques
should know about:

- It discovers a new entity (a pod, a service account, a node name)
- It establishes a new execution path (exec access, container escape, C2 session)
- It modifies an entity in a way that changes what's possible next (creates a role,
  binds a service account)

Effects on read-only discovery techniques (listing pods, enumerating RBAC) are
handled by the output parser pipeline — you typically don't need to write explicit
entity effects for these, only the list-form effect that triggers the parser:

```yaml
effects:
  - k8s.serviceAccountList    # triggers the service account output parser
  - k8s.podList               # triggers the pod list output parser
```

## Writing simple entity effects

Simple effects extract values from the active parameter context (the parameter
map at execution time). Make sure the required parameters exist in your TTP:

```yaml
parameters:
  Namespace:
    type: Namespace
    default: ${NS}
  ServiceAccountName:
    type: string
    description: Name of the service account that was created

effects:
  - k8s.serviceaccount
```

At runtime, Ran reads `Namespace` and `ServiceAccountName` from the parameter
map and creates a `ServiceAccount` entity.

## Writing relation effects

Relation effects take explicit entity IDs as arguments. Use `sys` for the
current execution target:

```yaml
# After an exploit grants exec on another pod:
effects:
  - rce.can-exec(sys, ns/target-ns/pod/victim-pod)

# After escaping the container:
effects:
  - container.escape(sys)

# After deploying a C2 implant:
effects:
  - c2.session(sliver, sys)
```

### Chaining escape paths

When `container.escape(sys)` is applied, Ran:

1. Creates a `K8sNode` entity for the host node (or a placeholder if the node
   name is not yet known)
2. Creates a `RunsOn` relation
3. Creates a `ContainerEscape` relation storing the escape command as an envelope

If the escape command contains `${CMD}` (the standard slot for the wrapped payload),
Ran stores it as the envelope and subsequent technique commands are wrapped with it
automatically.

Example — an nsenter-based escape:

```yaml
procedures:
  - key: nsenter
    command: nsenter -t 1 -m -u -i -n -p -- ${CMD}

effects:
  - container.escape(sys)
```

## Effect evaluation and the `sys` placeholder

`sys` resolves to `TARGET_ID` — the campaign entity ID of the target at execution
time. It is available in all relation-style effects. Do not use `sys` in the first
argument of `k8s.can-exec` unless the source of the exec-ability is the current
target itself.

## Combining effects

List multiple effects to update several parts of the graph in one step:

```yaml
effects:
  - k8s.serviceaccount          # SA created
  - k8s.rolebinding             # binding created
  - k8s.can-exec(sys, ns/default/pod/victim)   # exec path established
```

Effects are applied in order, but all run within the same graph update cycle before
inference rules fire.
