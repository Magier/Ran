# Preconditions

Preconditions define what must be true in the campaign before a TTP can run.
Ran evaluates them automatically when you select a target, so the armory shows
only applicable techniques — not every technique regardless of context.

## Declaring preconditions

```yaml
preconditions:
  kind: ServiceAccount
  rbac:
    - verb: create
      resource: roles
  accessLevel: user-exec
```

All precondition keys are optional. A TTP with no `preconditions:` block is always
considered applicable (no constraints).

## Precondition keys

### `kind`

The entity type the TTP targets. When `kind` is set, the TTP only appears in the
armory when the selected target entity is of that type.

Common values: `Pod`, `ServiceAccount`, `Node`, `Deployment`, `System`

`System` means the TTP runs on the operator's local machine rather than inside
the cluster.

```yaml
preconditions:
  kind: ServiceAccount
```

### `rbac`

A list of `{verb, resource}` pairs. At least one service account captured in the
campaign must hold *all* of the listed permissions for the TTP to be considered
applicable.

```yaml
preconditions:
  rbac:
    - verb: create
      resource: roles
    - verb: create
      resource: rolebindings
```

If no service accounts have been captured yet, RBAC-gated techniques are never
surfaced.

### `accessLevel`

Requires the target entity to have at least exec-level access. Set to any of the
values below — all of them resolve to the same check (the target must have
`AccessLevel::Exec`):

`user-exec`, `user-read`, `user-write`, `root-exec`, `root-read`

```yaml
preconditions:
  accessLevel: user-exec
```

Set `accessLevel: none` to explicitly pass regardless of the target's access level.

Three tactics are always exempt from access level checks regardless of what the
YAML declares: **Initial Access**, **Lateral Movement**, and **Resource Development**.

### `exists`

Requires a specific entity kind to be present in the campaign graph. Currently
supports:

- `Listener` — at least one C2 listener must be active

```yaml
preconditions:
  exists:
    - Listener
```

### `has-token`

Requires the target entity to have a captured JWT token.

```yaml
preconditions:
  has-token: true
```

### `related`

Requires a related entity of a given kind — and optionally with a minimum access
level — to exist in the campaign graph. Currently supports:

- Target `ServiceAccount` + related `Pod`: finds pods that mount the SA; if
  `accessLevel` is also set, at least one such pod must have exec access.

```yaml
preconditions:
  related:
    - kind: Pod
      accessLevel: user-exec
```

## How Ran evaluates preconditions

For each entity shown in the cluster map, Ran evaluates all preconditions against
the current campaign state. A TTP becomes applicable when *all* conditions are
satisfied simultaneously. In the UI, applicable techniques light up in the armory
panel; non-applicable ones are greyed out with a tooltip explaining which condition
failed.
