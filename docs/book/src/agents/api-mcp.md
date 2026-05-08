# Using Ran via API and MCP

Ran exposes its campaign state and invocation surface via a REST API and a Model
Context Protocol (MCP) server. This lets agents drive emulation sessions
programmatically without the web UI.

## REST API

When `ran emulate` is running, the API is available at `http://localhost:8080`
(port configurable with `--port`). A Swagger UI is served at `/api/docs` and the
raw OpenAPI spec at `/api/openapi.yaml`.

### Core endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/graph` | Full knowledge graph: all discovered entities and relations |
| `GET` | `/api/campaign-state` | Campaign state: entities with all their discovered facts |
| `GET` | `/api/armory` | All TTPs in the armory; optional `?tactic=Discovery` filter |
| `GET` | `/api/applicable-ttps` | TTPs filtered by target entity; use `?targetId=<entity_id>` |
| `POST` | `/api/action/execute` | Invoke a TTP against a target entity |
| `GET` | `/api/flow` | Ordered attack flow: completed and in-progress steps with causal edges |
| `GET` | `/api/execution-records` | All execution records (with parse audits) |
| `GET` | `/api/execution-records/{id}` | Single execution record by command ID |
| `POST` | `/api/campaign/reset` | Clear all campaign state (entities, relations, execution records) |
| `GET` | `/api/files` | Read a file captured in campaign state; use `?path=<path>` |
| `GET` | `/api/pods/running` | Live running pods from Kubernetes; use `?namespace=<ns>` for a single namespace |
| `POST` | `/api/pods/watch` | Start a live pod watch; optional `?namespace=<ns>` |
| `DELETE` | `/api/pods/watch` | Stop the live pod watch |
| `GET` | `/events` | Server-sent events stream for real-time campaign updates |

### Invoking a TTP via API

```http
POST /api/action/execute
Content-Type: application/json

{
  "actionId":     "get-pods",
  "targetId":     "ns/default/pod/entry-hall-abc12",
  "execSystemId": "ns/default/pod/entry-hall-abc12",   // optional
  "procedureId":  "shell",                              // optional
  "args": {
    "NS": "kube-system"
  }
}
```

Response:

```json
{
  "success": true,
  "queued":  true,
  "cmdId":   "01HXYZ..."
}
```

The execution is asynchronous. Poll `GET /api/execution-records/{cmdId}` or
subscribe to `GET /events` to receive the result.

### Reading campaign state

`GET /api/campaign-state` returns:

```json
{
  "entities": {
    "<entity_id>": {
      "id":        "ns/default/pod/entry-hall-abc12",
      "name":      "entry-hall-abc12",
      "kind":      "Pod",
      "namespace": "default"
    }
  },
  "relations": [
    {
      "id":       "A-[token]->B",
      "name":     "token",
      "sourceId": "ns/default/pod/entry-hall-abc12",
      "targetId": "ns/default/serviceaccount/entry-hall"
    }
  ]
}
```

`GET /api/graph` returns a graph-layout-ready representation with `nodes` and
`edges` arrays suitable for rendering.

### Execution records

`GET /api/execution-records` returns an array. Each entry includes the execution
record plus any parse audits produced by effect parsers:

```json
[
  {
    "id":          "01HXYZ...",
    "ttp_id":      "get-pods",
    "ttp_name":    "List Pods",
    "tactic":      "Discovery",
    "target_id":   "ns/default/pod/entry-hall-abc12",
    "success":     true,
    "exit_code":   0,
    "results":     ["NAME   READY   STATUS..."],
    "parseAudits": []
  }
]
```

## MCP server

Ran exposes a Model Context Protocol server that agents can connect to as a tool
provider. The MCP server is always started alongside `ran emulate` — no extra flag
is required. It listens on the same port as the REST API using the Streamable HTTP
transport at `/mcp`.

To connect from an MCP client (e.g. Claude Desktop via `mcp-remote`, VS Code
Copilot):

```json
{
  "mcpServers": {
    "ran": {
      "command": "npx",
      "args": ["-y", "mcp-remote", "http://localhost:8080/mcp"]
    }
  }
}
```

### Available MCP tools

| Category | Tool | Required arguments |
|---|---|---|
| Discovery | `get_graph` | — |
| Discovery | `get_entity` | `entity_id` |
| Discovery | `get_attack_surface` | `entity_id` |
| Discovery | `resolve_workload` | `name` |
| Campaign | `get_campaign_state` | — |
| Campaign | `get_attack_flow` | — |
| Armory | `list_ttps` | — (optional: `tactic`) |
| Armory | `get_applicable_ttps` | `target_id` |
| Armory | `get_ttp_detail` | `ttp_id` |
| Execution | `execute_action` | `action_id`, `target_id` |
| Execution | `wait_for_result` | `cmd_id` |
| Goal eval | `check_rbac_goal` | `entity_id` (optional: `verbs`, `resources`) |
| Goal eval | `check_access_level` | `entity_id` |
| Initial access | `get_initial_access_candidates` | — (optional: `namespace`, `name_filter`) |
| Extension | `list_parse_audits` | — |
| Extension | `add_parser` | `effect_id`, `script_content` |
| Campaign | `reset_campaign` | — |

#### Tool details

**`resolve_workload`** — partial-name search across all entities. Use this
instead of guessing entity IDs. Returns a list of `{ id, kind, name, namespace }`
objects.

**`get_initial_access_candidates`** — queries live Kubernetes pods directly (not
the campaign graph). Use only for the first foothold before any entities are
discovered.

**`execute_action`** — validates that `target_id` is a known entity before
queuing the TTP. For initial access use the Cluster entity as `target_id` and
pass the pod name/namespace as TTP parameters. Returns `{ cmd_id, queued: true }`.

**`wait_for_result`** — blocks up to 60 seconds polling for the execution record
identified by `cmd_id`. Returns stdout, stderr, success status, and any parse
audit entries. Use this immediately after `execute_action`.

**`get_applicable_ttps`** — filters the armory by the entity's kind, current
access level, RBAC holdings, and campaign state. Cheaper than iterating
`list_ttps` manually.

**`add_parser`** — writes a Python parser script to `armory/parsers/{effect_id}.py`.
Ran discovers and loads it automatically for future executions of effects with that
ID. The script reads an `ExternalParseRequest` JSON on stdin and writes an
`ExternalParseResponse` JSON on stdout.

**`check_rbac_goal`** — evaluates whether a ServiceAccount entity holds the
specified RBAC verbs and resources. Returns `achieved: true/false` and a list of
missing permissions.

### Recommended agent workflow

1. Call `get_initial_access_candidates` (optionally filtered by namespace) to
   find a pod to exec into as the first foothold.
2. Call `execute_action` with an `InitialAccess` TTP, using the Cluster entity
   as `target_id` and the chosen pod name/namespace as TTP parameters.
3. Call `wait_for_result` with the returned `cmd_id` to confirm the exec channel
   was established.
4. Call `get_campaign_state` or `get_graph` to observe what entities and
   relations were discovered.
5. For each discovered entity, call `get_applicable_ttps` to get a filtered list
   of applicable TTPs given the current campaign state.
6. Pick the most relevant TTP, call `execute_action` against the entity, then
   `wait_for_result` to get the output.
7. If `wait_for_result` returns parse audits with `NoParser` or `UnknownFormat`
   results, call `list_parse_audits` to inspect the raw output, author a parser
   with `add_parser`, and re-run the TTP.

Repeat steps 4–7 until campaign goals are achieved. Use `check_rbac_goal` and
`check_access_level` to evaluate goal conditions without running additional TTPs.

## Subscribing to campaign events

`GET /events` is a Server-Sent Events (SSE) stream that delivers real-time
campaign updates. Connect once and receive events for every TTP execution
completion, entity discovery, and relation update. This is the preferred
alternative to polling `GET /api/execution-records` in a loop.

Each event is a JSON-encoded payload. The stream stays open until the client
disconnects or `ran emulate` exits.
