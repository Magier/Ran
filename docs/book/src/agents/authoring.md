# Authoring TTPs as an Agent

This chapter provides the precise, unambiguous contract an AI agent needs to
produce valid, runnable Ran TTP YAML files.

## Validity rules

A YAML file is a valid Ran TTP if and only if:

1. It is valid YAML.
2. It contains a non-empty `name:` string at the top level.
3. Every `${PARAM_NAME}` placeholder used in a procedure command or effect string
   has a matching entry in `parameters:` or is a built-in variable. Declared
   parameters that do not appear in any command are allowed (they may serve as
   context for effect grounding or have defaults).
4. Every effect expression references a valid built-in effect (see [Effect Catalog](../reference/effects.md)).
5. Every precondition key is from the supported set (see [Precondition Types](../reference/preconditions.md)).

## Minimal runnable TTP

```yaml
name: My Custom Technique
tactic: Discovery
procedures:
  - key: shell
    command: id
```

## Required fields checklist

Before submitting a TTP YAML, verify:

- [ ] `name:` is present and non-empty
- [ ] `tactic:` matches a valid MITRE ATT&CK tactic or the TTP is placed in the
  correct subdirectory
- [ ] At least one entry in `procedures:` is non-empty
- [ ] Every `${PARAM}` placeholder in procedure commands has a corresponding entry
  in `parameters:` (or is a built-in variable: `${NS}`, `${TOKEN}`, `${API_SERVER}`,
  `${TARGET.IP}`, `${TARGET_ID}`, `${CMD}`)
- [ ] Every effect string is a known effect expression (see [Effect Catalog](../reference/effects.md))
- [ ] Every precondition key is from the supported set

## Parameter contract

```yaml
parameters:
  PARAM_NAME:
    type: string          # string | Namespace | ServiceAccount | bool | int
    description: "..."    # required: non-empty
    default: ""           # may use ${BUILT_IN_VAR} syntax
    required: false       # omit for required parameters
```

Parameter names are case-sensitive. Built-in variables (`${NS}`, `${TOKEN}`, etc.)
are always available — do not redeclare them unless overriding the default.

## Procedure contract

Every procedure must have at least one of:

- A non-empty `command:` string
- A non-null `k8s_request:` map
- A non-null `http_request:` map
- A non-empty `steps:` list

A procedure with all fields absent or empty is filtered out during YAML parsing.
A TTP that has no remaining procedures after filtering will fail at dispatch.

## Effect expression grammar

```
effect      := simple_effect | relation_effect
simple_effect  := effect_name                          # e.g. k8s.pod
relation_effect := effect_name "(" arg ("," arg)* ")"  # e.g. container.escape(sys)
arg         := entity_id | "sys" | wildcard
entity_id   := <any non-empty string>
wildcard    := "all(" kind ")"
```

The `sys` token resolves to the entity the TTP executed against (`TARGET_ID`).

## Common authoring mistakes

| Mistake | Fix |
|---|---|
| Using `${VAR}` without declaring the parameter | Declare the param or use a built-in variable |
| Declaring `rbac:` without any entries | Remove the key; empty `rbac: []` is treated as no restriction |
| Setting `accessLevel: root-exec` expecting root check | All declared `accessLevel` values (except `none`) resolve to the same `Exec` check |
| Using `container.escape(src, tgt)` with two args | `container.escape` takes exactly one arg (`sys` or a pod entity ID) |
| Effect `k8s.pod` without `Namespace` and `PodName` in context | Ensure those parameters exist and are populated at execution time |
| Setting `status: disabled` and expecting the TTP to appear in `ran armory` | Disabled TTPs are excluded from listings; use `draft` for in-progress work |

## Example: complete custom TTP

```yaml
name: Enumerate Node Filesystem via Escape
description: >
  After a container escape, enumerate the host node's /etc directory
  to identify sensitive files.
tactic: Discovery
techniques: ["T1083"]
preconditions:
  kind: Node
  accessLevel: user-exec
parameters:
  PATH:
    type: string
    description: Directory to enumerate
    default: /etc
procedures:
  - key: shell
    command: find ${PATH} -maxdepth 2 -type f 2>/dev/null
references:
  - https://attack.mitre.org/techniques/T1083/
```
