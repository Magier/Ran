# Preconditions in Depth

The [Preconditions](../campaign/preconditions.md) chapter in Campaign Emulation
explains what preconditions *do*. This chapter shows how to write them accurately
in your custom TTPs.

## Complete preconditions reference

All precondition keys are optional. Combine them freely; all must be satisfied
simultaneously for the TTP to be applicable.

### `kind`

Restricts the TTP to a specific entity type. The value is the entity kind as a
PascalCase string.

```yaml
preconditions:
  kind: Pod                  # runs against pods
  # kind: ServiceAccount    # runs against service accounts
  # kind: Node              # runs against nodes
  # kind: System            # runs locally on the operator machine (no cluster target)
```

### `rbac`

Requires at least one captured service account to hold all listed permissions.
Use the singular `verb` and `resource` keys (Ran normalises these internally).

```yaml
preconditions:
  rbac:
    - verb: create
      resource: roles
    - verb: create
      resource: rolebindings
    - verb: get
      resource: secrets
```

Wildcard permissions (`verb: "*"` or `resource: "*"`) in a captured service account
satisfy any specific requirement.

### `accessLevel`

Requires the target entity to have interactive execution access. All declared
values (except `none`) resolve to the same check:

```yaml
preconditions:
  accessLevel: user-exec   # or user-read, user-write, root-exec, root-read
```

Set `accessLevel: none` to pass regardless of access. Omitting the key entirely
has the same effect.

Initial Access, Lateral Movement, and Resource Development techniques are exempt
from access level checks unconditionally.

### `exists`

Requires a particular entity kind to be present in the campaign graph.

```yaml
preconditions:
  exists:
    - Listener    # at least one active C2 listener
```

### `has-token`

Requires the target entity to carry a captured JWT:

```yaml
preconditions:
  has-token: true
```

### `related`

Requires a related entity to exist in the campaign:

```yaml
preconditions:
  related:
    - kind: Pod
      accessLevel: user-exec   # at least one pod mounting this SA must have exec
```

Currently the only supported combination is `ServiceAccount` target + `Pod` related.

## Common patterns

### "Requires kubectl exec into a pod"

```yaml
preconditions:
  kind: Pod
  accessLevel: user-exec
```

### "Requires a captured high-privilege token"

```yaml
preconditions:
  kind: ServiceAccount
  has-token: true
  rbac:
    - verb: "*"
      resource: "*"
```

### "Resource Development setup step (no cluster access needed)"

```yaml
preconditions:
  kind: System
```

Or simply omit preconditions entirely for Resource Development TTPs, since that
tactic is unconditionally exempt from access level checks.
