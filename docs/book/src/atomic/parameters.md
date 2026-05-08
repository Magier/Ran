# Parameters

Parameters are the inputs a TTP needs to run. They appear as `parameters:` in the
YAML and as editable fields in the web UI.

## Declaring parameters in YAML

```yaml
parameters:
  TOKEN:
    type: ServiceAccount
    description: The ServiceAccount token to use
    optional: true
  NS:
    type: Namespace
    description: The target namespace
    default: ${NS}
  ALL_NS:
    type: bool
    description: Query all namespaces instead of just one
    default: false
```

Each parameter has a name (the YAML key), a type, a description, an optional flag,
and a default value.

## Parameter types

| Type | What it means |
|---|---|
| `string` (default) | Any text value |
| `Namespace` | A Kubernetes namespace name |
| `ServiceAccount` | A JWT token belonging to a captured service account |
| `bool` | `true` or `false` |
| `int` | An integer |

`ServiceAccount` and `Namespace` parameters in the UI render as dropdowns populated
from entities already discovered in the campaign.

## Required vs optional

Parameters are required by default. Set `optional: true` (or `required: false`) to
make a parameter skippable:

```yaml
KERNEL_VERSION:
  type: string
  required: false
  description: Kernel version of the target node
```

## Special built-in variables

Ran injects a set of variables into every TTP invocation. You can reference them
as defaults or directly in command strings:

| Variable | Value |
|---|---|
| `${NS}` | Namespace of the current target entity |
| `${TOKEN}` | JWT token of the best available service account |
| `${API_SERVER}` | URL of the Kubernetes API server |
| `${TARGET.IP}` | IP address of the current target entity |
| `${TARGET_ID}` | Ran's internal entity ID for the current target |

Example usage in a command:

```yaml
procedures:
  - key: curl
    command: >-
      curl -H "Authorization: Bearer ${TOKEN}"
      "${API_SERVER}/api/v1/namespaces/${NS}/pods"
```

## Overriding parameters on the CLI

```sh
ran invoke get-serviceaccounts --target default/pod \
  --param NS=kube-system \
  --param ALL_NS=true
```

In the web UI, all parameters appear as editable fields above the Execute button.
