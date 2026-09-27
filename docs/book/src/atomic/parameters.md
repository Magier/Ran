# Parameters

TTP inputs are declared under `parameters:` and rendered as fields in the browser UI.

```yaml
parameters:
  K8S_AUTH:
    type: K8sAuth
    description: Kubernetes identity selected by the operator
  NS:
    type: Namespace
    valueField: name
    default: ${NS}
  ALL_NS:
    type: bool
    default: false
```

Common types include `string`, `bool`, `int`, `Namespace`, `ServiceAccount`, and `K8sAuth`. Entity-backed types are populated from campaign knowledge.

Entity-backed candidates have a canonical Ran ID and a procedure-facing name.
Set `valueField: name` when the procedure expects a Kubernetes resource name,
such as `agent-system`. Keep the default `valueField: id` when Ran needs a
stable entity reference, such as `listener/tcp/1337`. Resolution candidates
carry the projected value in `value` and retain the canonical entity ID under
`source.entityId`; API clients must submit `value`. See the
[YAML field catalog](../reference/yaml-fields.md#entity-backed-values-and-valuefield)
for the full contract.

Parameters are required by default. Use `required: false` or `optional: true` for optional input.

Built-in values include:

| Variable        | Value                     |
| --------------- | ------------------------- |
| `${NS}`         | Current target namespace  |
| `${API_SERVER}` | Kubernetes API server URL |
| `${TARGET.IP}`  | Current target IP         |
| `${TARGET_ID}`  | Current target entity ID  |

Override values atomically by repeating `--arg`:

```sh
ran trigger get-serviceaccounts \
  --target ns/default/pod/my-pod \
  --arg NS=kube-system \
  --arg ALL_NS=true
```
