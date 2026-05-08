# Precondition Types

Complete reference for all supported precondition keys.

---

## `kind`

**Type:** string

The entity type the TTP targets. Must match the kind of the selected target entity.

**Common values:**

| Value | Matches |
|---|---|
| `Pod` | Pod entities |
| `ServiceAccount` | ServiceAccount entities |
| `Node` | Node entities |
| `Deployment` | Deployment entities |
| `System` | No cluster entity — runs on the operator's machine |

Omitting `kind` means the TTP is applicable to any entity type.

---

## `rbac`

**Type:** list of `{verb, resource}` objects

At least one service account captured in the campaign must hold *all* listed
permissions. Use `resource` as the YAML key (the runtime normalises it to
`resourceType` internally).

```yaml
rbac:
  - verb: create
    resource: roles
  - verb: delete
    resource: events
```

- If no service accounts have been captured yet, RBAC-gated TTPs are never surfaced.
- Wildcard SA entitlements (`verb: "*"` or `resource: "*"`) satisfy any specific requirement.
- An empty `rbac: []` list is treated as satisfied (no RBAC requirement).

---

## `accessLevel`

**Type:** string

Requires the target entity to have `AccessLevel::Exec` — the ability to run
arbitrary commands on the entity.

**Any of these values enforce the check:** `user-exec`, `user-read`, `user-write`,
`root-exec`, `root-read`

**Exempt from this check regardless of the declared value:**

- Tactic `Initial Access`
- Tactic `Lateral Movement`
- Tactic `Resource Development`

Set `accessLevel: none` to explicitly pass without access. Omitting the key
entirely has the same effect.

---

## `exists`

**Type:** list of strings

Requires a specific entity kind to be present in the campaign graph.

**Supported values:**

| Value | Requirement |
|---|---|
| `Listener` | At least one `C2Server` entity must have a non-empty `listeners` list |

```yaml
exists:
  - Listener
```

Unknown values fail safe (the TTP is not surfaced).

---

## `has-token`

**Type:** bool

When `true`, the target entity must carry a captured JWT token.

```yaml
has-token: true
```

`false` or omitting the key: always satisfied.

---

## `related`

**Type:** list of `{kind, accessLevel?}` objects

Requires a related entity to exist in the campaign graph.

**Currently supported combinations:**

| Target kind | Related `kind` | Behaviour |
|---|---|---|
| `ServiceAccount` | `Pod` | At least one pod in the campaign must mount this SA. If `accessLevel` is set, at least one such pod must have exec access or be reachable via kubectl exec. |

Unknown `(target, related)` combinations pass by default (fail open, so future
relationships can be added to YAML files before the code lands).

```yaml
related:
  - kind: Pod
    accessLevel: user-exec
```
