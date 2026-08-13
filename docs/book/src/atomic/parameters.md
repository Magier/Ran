# Parameters

Parameters are the inputs a TTP needs to run. They appear as `parameters:` in the
YAML and as editable fields in the web UI.

## Declaring parameters in YAML

```yaml
parameters:
  K8S_AUTH:
    type: K8sAuth
    description: The Kubernetes identity selected by Authenticate As
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
| `K8sAuth` | An eligible captured ServiceAccount or active K8sCredential |
| `bool` | `true` or `false` |
| `int` | An integer |

`K8sAuth`, `ServiceAccount`, and `Namespace` parameters render as dropdowns populated
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
| `${API_SERVER}` | URL of the Kubernetes API server |
| `${TARGET.IP}` | IP address of the current target entity |
| `${TARGET_ID}` | Ran's internal entity ID for the current target |

Kubernetes API actions declare their identity selector explicitly:

```yaml
parameters:
  K8S_AUTH:
    type: K8sAuth
    description: The Kubernetes identity selected by Authenticate As
procedures:
  - key: kubectl
    command: kubectl ${K8S_AUTH} get pods -n=${NS}
```

The selected entity ID is transported as `authIdentityId`; `${K8S_AUTH}` then
grounds to the appropriate kubectl flag or structured-request authentication.
Procedures using the already-active local Kubernetes client without referencing
`${K8S_AUTH}` do not need to declare this parameter.

An explicit `TOKEN` parameter remains valid for non-Kubernetes APIs such as a
direct kubelet endpoint or a cloud provider bearer-token API.

## Overriding parameters on the CLI

```sh
ran invoke get-serviceaccounts --target default/pod \
  --param NS=kube-system \
  --param ALL_NS=true
```

In the web UI, all parameters appear as editable fields above the Execute button.
