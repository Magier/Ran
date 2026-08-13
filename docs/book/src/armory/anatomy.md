# TTP Anatomy

Every TTP in the armory is a YAML file. Here is a representative example — the
*Get ServiceAccounts* technique from the Discovery tactic:

```yaml
name: Get ServiceAccounts
description: Get a list of ServiceAccounts via the API server
tactic: Discovery
techniques: ["Container and Resource Discovery", T1613]
preconditions:
  rbac:
    - verb: get
      resource: serviceaccounts
parameters:
  K8S_AUTH:
    type: K8sAuth
    description: The Kubernetes identity selected by Authenticate As
  NS:
    type: Namespace
    description: The namespace to query
    default: ${NS}
  ALL_NS:
    type: bool
    description: Query all namespaces
    default: false
procedures:
  - key: kubectl
    command: >-
      kubectl ${K8S_AUTH} get serviceaccounts -n=${NS} -A=${ALL_NS}
      --output=json
  - key: k8s-request
    k8s_request:
      authentication: ${K8S_AUTH}
      api_server: ${API_SERVER}
      api: /api/v1
      resource: serviceaccounts
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
effects:
  - k8s.serviceAccountList
```

## Top-level fields

| Field | Required | Description |
|---|---|---|
| `name` | yes | Human-readable display name |
| `id` | no | Stable identifier used for `ran invoke`; auto-derived from `name` if omitted (kebab-case) |
| `description` | no | One or two sentences explaining what the technique does |
| `tactic` | no | MITRE ATT&CK tactic; defaults to the parent directory name |
| `techniques` | no | List of MITRE technique names and/or IDs (e.g. `["T1613"]`) |
| `status` | no | `enabled` (default), `stable`, `draft`, or `disabled` |
| `parameters` | no | Named input variables (see [Parameters](../atomic/parameters.md)) |
| `preconditions` | no | Constraints that gate when this TTP is applicable (see [Preconditions](../campaign/preconditions.md)) |
| `procedures` | yes* | One or more execution methods |
| `cleanup` | no | A single procedure to undo the technique's side-effects |
| `effects` | no | What the technique reveals or establishes (see [Effects](../campaign/effects.md)) |
| `references` | no | URLs to CVE write-ups, ATT&CK pages, research, etc. |

*A TTP with no procedures is valid but cannot be invoked.

## Procedures at a glance

A TTP can offer multiple procedures — one per available tool or method. Ran shows
them as selectable options in the UI. The full authoring guide is in
[Writing Procedures](../extending/procedures.md); the short version:

- **Shell command** — `command: kubectl get pods …`
- **Structured K8s API call** — `k8s_request:` block; Ran materialises this into a
  kubectl or curl command at runtime
- **Structured HTTP request** — `http_request:` block
- **Step sequence** — `steps:` list of typed actions (fetch, chmod, run, …)

## What comes next

When you understand the anatomy, the next step is to run a technique. Head to
[Running a TTP](../atomic/running.md).
