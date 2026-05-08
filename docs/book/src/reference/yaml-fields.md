# YAML Field Catalog

Complete reference for every field in a Ran TTP YAML file. All fields are optional
except `name`. Fields marked **repeatable** accept a list.

---

## Top-level fields

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | **yes** | Human-readable display name. Used to derive `id` if omitted. |
| `id` | string | no | Stable kebab-case identifier. Auto-derived from `name` if absent. |
| `description` | string | no | One or two sentences describing the technique. |
| `tactic` | string | no | MITRE ATT&CK tactic. Defaults to the parent directory name. |
| `techniques` | list | no | MITRE technique names and/or IDs, e.g. `["T1613", "Container and Resource Discovery"]`. |
| `status` | string | no | `enabled` (default), `draft`, or `disabled`. |
| `parameters` | map | no | Named input variables. See [Parameters](#parameters). |
| `preconditions` | map | no | Gate conditions. Also accepted as `requires:`. See [Preconditions](#preconditions). |
| `procedures` | list | no | Execution methods. See [Procedures](#procedures). |
| `cleanup` | map | no | A single procedure to undo the technique. Same structure as one procedure entry. |
| `effects` | list | no | Effect expressions applied after a successful run. See [Effects](#effects). |
| `references` | list | no | URLs to CVEs, ATT&CK pages, write-ups, etc. |
| `tool_slot` | string | no | Marks this TTP as a tool implementation for the named slot (e.g. `"http-request"`). Advanced use. |

---

## Parameters

Declared as a map under `parameters:`. Each key becomes the parameter name.

```yaml
parameters:
  MY_PARAM:
    type: string
    description: What this parameter means
    default: some-default
    required: false
```

| Field | Type | Default | Description |
|---|---|---|---|
| `type` | string | `string` | Parameter type. One of: `string`, `Namespace`, `ServiceAccount`, `bool`, `int`. |
| `description` | string | `""` | Shown in the UI tooltip and CLI help. |
| `default` | any | `""` | Default value. Can reference built-in variables like `${NS}`. |
| `required` | bool | `true` | Whether the parameter must be provided. Set to `false` to make it optional. |

**Built-in variable defaults:**

| Variable | Value at runtime |
|---|---|
| `${NS}` | Namespace of the target entity |
| `${TOKEN}` | Best available service account JWT |
| `${API_SERVER}` | Kubernetes API server URL |
| `${TARGET.IP}` | IP address of the target entity |
| `${TARGET_ID}` | Ran entity ID of the target (e.g. `pod/default/nginx`) |

---

## Preconditions

Declared as a map under `preconditions:` (alias: `requires:`).

| Key | Type | Description |
|---|---|---|
| `kind` | string | Entity type the TTP targets. Common: `Pod`, `ServiceAccount`, `Node`, `System`. |
| `rbac` | list | `{verb, resource}` pairs. At least one captured SA must hold all permissions. |
| `accessLevel` | string | Requires exec access on the target. Any value except `none` enforces the check. |
| `exists` | list | Entity kinds that must be present in the campaign graph. Supports: `Listener`. |
| `has-token` | bool | `true` — target entity must have a captured JWT token. |
| `related` | list | `{kind, accessLevel?}` — related entity requirements. |

---

## Procedures

Each procedure entry supports the following fields:

| Field | Type | Description |
|---|---|---|
| `key` / `id` | string | Display name and identifier. Doubles as the `tool` name if not set separately. |
| `tool` | string | Tool TTP slot to use for execution (e.g. `curl`, `wget`). |
| `command` | string | Shell command to run. Supports parameter placeholders. |
| `isLocal` / `isLocalCommand` | bool | Run on the operator's machine instead of the target. |
| `http_request` | map | Structured HTTP request. See [HTTP Request](#http-request). |
| `k8s_request` | map | Structured Kubernetes API request. See [K8s Request](#k8s-request). |
| `steps` | list | Ordered step sequence. See [Steps](#steps). |

### HTTP Request

```yaml
http_request:
  method: POST
  url: http://${TARGET}:${PORT}/endpoint
  headers:
    Content-Type: application/json
  body: '{"key":"${VALUE}"}'
```

| Field | Type | Description |
|---|---|---|
| `method` | string | HTTP method: `GET`, `POST`, `PUT`, `DELETE`, etc. |
| `url` | string | Target URL. Supports parameter placeholders. |
| `headers` | map | Optional HTTP headers as key-value pairs. |
| `body` | string | Optional request body. Supports parameter placeholders. |

### K8s Request

```yaml
k8s_request:
  api_server: ${API_SERVER}
  api: /api/v1                # or /apis/apps/v1, etc.
  resource: pods
  namespace: ${NS}
  cluster_scoped: false       # true = all namespaces
  query: limit=500            # appended as URL query string
  token: ${TOKEN}
  use_ca: false               # skip CA verification
  ca_path: /var/run/secrets/kubernetes.io/serviceaccount/ca.crt
```

| Field | Type | Description |
|---|---|---|
| `api_server` | string | Kubernetes API server URL. Defaults to `${API_SERVER}`. |
| `api` | string | API group path, e.g. `/api/v1` or `/apis/apps/v1`. |
| `resource` | string | Resource type, e.g. `pods`, `secrets`, `serviceaccounts`. |
| `namespace` | string | Target namespace. Omit for cluster-scoped resources. |
| `cluster_scoped` | bool | If `true`, list resources across all namespaces. |
| `query` | string | URL query string appended to the request, e.g. `limit=500`. |
| `token` | string | Bearer token for authentication. Defaults to `${TOKEN}`. |
| `use_ca` | bool | If `true`, verify the server CA certificate. Default: `false`. |
| `ca_path` | string | Path to CA bundle for server verification when `use_ca: true`. |

### Steps

```yaml
steps:
  - fetch:
      url: https://example.com/binary
      dest: /tmp/binary
  - chmod:
      path: /tmp/binary
      mode: "0755"
  - run:
      command: /tmp/binary --flag ${PARAM}
```

Supported step types: `fetch`, `chmod`, `run`.

---

## Effects

Effect strings declared in the `effects:` list. Full catalog: [Effect Catalog](effects.md).

```yaml
effects:
  - k8s.serviceAccountList          # trigger output parser
  - k8s.pod                         # extract pod entity from parameters
  - container.escape(sys)           # record a container escape path
  - c2.session(sliver, sys)         # record a C2 session
  - rce.can-exec(sys, target-id)    # record an RCE execution path
```
