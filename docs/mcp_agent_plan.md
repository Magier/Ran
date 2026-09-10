# MCP Server & Autonomous Agent - Plan

## Overview

Ran exposes an MCP server (embedded in its axum process) that lets a 3rd-party
LLM agent drive adversary emulation campaigns against Kubernetes clusters.  The
agent can operate fully autonomously toward a goal, follow waypoints, or do
single-step exploration.

The key architectural insight: Ran already has the infrastructure for
**dynamic parser extension** via `ScriptParserRunner` and the
`RAN_PARSER_GENERATOR_WEBHOOK`.  The MCP server connects these pieces so an
LLM can close gaps in Ran's capabilities on the fly - writing new parsers,
crafting TTP YAML, and feeding parsed facts back into the graph - creating a
self-improving feedback loop across runs.

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│  3rd-party LLM Agent  (Python POC)              │
│  ┌──────────────────────────────────────────┐   │
│  │ ReAct loop: observe → reason → act       │   │
│  │ System prompt encodes goal + waypoints   │   │
│  └────────────────────┬─────────────────────┘   │
│                       │ MCP (stdio or HTTP)      │
└───────────────────────┼─────────────────────────┘
                        │
┌───────────────────────▼─────────────────────────┐
│  Ran  (single axum binary)                       │
│                                                  │
│  /mcp   ── MCP tool router (rmcp)                │
│  /api/* ── existing REST + SSE                   │
│  /events ── SSE event stream                     │
│                                                  │
│  ┌─────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Armory  │  │ Campaign │  │ Knowledge     │  │
│  │ (TTPs)  │  │ (engine) │  │ Graph         │  │
│  └─────────┘  └──────────┘  └───────────────┘  │
│                     │                            │
│            ┌────────▼────────┐                   │
│            │ ExternalParser  │←── parser-gap     │
│            │ (script runner/ │    webhook         │
│            │  webhook)       │                   │
│            └─────────────────┘                   │
└──────────────────────────────────────────────────┘
```

---

## Phase 1 - MCP Server (Rust, in `crates/api`)

### 1.1 Add `rmcp` dependency

```toml
# crates/api/Cargo.toml
rmcp = { version = "0.1", features = ["server", "transport-streamable-http-server"] }
```

### 1.2 MCP tool surface

All tools share the `AppState` (same state the REST handlers use).

#### Discovery & context

| Tool | Description | Maps to |
|---|---|---|
| `resolve_workload` | Resolve a workload name (deployment, daemonset, etc.) to concrete pod IDs | New - graph query + k8s lookup |
| `get_graph` | Return the full knowledge graph (entities + relations) | `GET /api/graph` |
| `get_entity` | Get details about a specific entity by ID | New - direct graph lookup |
| `get_attack_surface` | For an entity: reachable services, mounts, RBAC, tokens, relations | New - graph traversal |
| `get_campaign_state` | Full campaign state: sessions, access levels, history | `GET /api/campaign-state` |
| `get_attack_flow` | Ordered execution history with causal edges | `GET /api/flow` |

#### Armory

| Tool | Description | Maps to |
|---|---|---|
| `list_ttps` | List all TTPs, optionally filtered by tactic | `GET /api/armory` |
| `get_applicable_ttps` | TTPs applicable to a given entity (RBAC + precondition check) | `GET /api/applicable-ttps` |
| `get_ttp_detail` | Full schema of a TTP: preconditions, params, procedures, effects | New - armory lookup |

#### Execution

| Tool | Description | Maps to |
|---|---|---|
| `execute_action` | Execute a TTP against a target entity, returns cmd ID | `POST /api/action/execute` |
| `wait_for_result` | Block until a cmd ID completes, return stdout/stderr/parsed facts | New - subscribes to `CampaignEvent` |

#### Goal evaluation

| Tool | Description | Maps to |
|---|---|---|
| `check_rbac_goal` | Check if an entity's RBAC permissions match a target permission set | New - RBAC comparison |
| `check_access_level` | Check current access level for an entity (container / host / cluster-admin) | New - graph lookup |

#### Dynamic extension

| Tool | Description | Maps to |
|---|---|---|
| `add_parser` | Write a Python parser script to `armory/parsers/{effect_id}.py` | New - file write |
| `add_ttp` | Write a TTP YAML file to the armory directory and hot-reload | New - file write + armory reload |
| `list_parse_audits` | Show recent parse results including `NoParser` / `UnknownFormat` gaps | New - campaign query |

### 1.3 MCP resources (read-only context)

| Resource | Description |
|---|---|
| `ran://graph` | The current graph as JSON (subscribable) |
| `ran://armory/ttps` | The loaded TTP catalog |
| `ran://campaign/flow` | The attack flow |

### 1.4 MCP notifications

Subscribe to Ran's existing SSE event bus (`CampaignEventBus`) and forward as
MCP notifications:

- `facts_changed` - graph was updated
- `ttp_executed` - an action completed
- `parse_audited` - a parser ran (or failed / was missing)

### 1.5 Integration point

Mount the MCP router alongside the existing API:

```rust
// crates/api/src/lib.rs
pub fn router_with_sse<S: ApiService>(service: S) -> axum::Router {
    let mcp_router = mcp::create_mcp_router(/* shared state */);

    axum::Router::new()
        .route("/events", ...)
        .route("/api/graph", ...)
        // ... existing routes ...
        .merge(mcp_router)            // adds /mcp/*
        .with_state(service.clone())
        .merge(router(service))
}
```

### 1.6 Transport options

The MCP server supports both transports via feature flags:

- **Streamable HTTP** (`/mcp` route) - for VS Code, web clients
- **stdio** (optional `ran mcp-server` subcommand) - for Claude Desktop

The stdio binary reuses the same `McpToolRouter` but wraps it in
`rmcp::transport::io::stdio_server()` instead of the axum integration.  It takes
`--ran-url http://localhost:8080` and calls the REST API like Option B, but
shares the identical tool definitions.

---

## Phase 2 - Python Agent (POC)

### 2.1 Architecture

```
ran-agent/
├── pyproject.toml
├── ran_agent/
│   ├── __init__.py
│   ├── agent.py          # ReAct loop
│   ├── mcp_client.py     # MCP connection manager
│   ├── prompts.py        # System prompts / prompt templates
│   ├── parser_crafter.py # Generate parser scripts from raw output
│   └── planner.py        # Goal decomposition + waypoint tracking
└── README.md
```

### 2.2 Core loop (`agent.py`)

```
1. Connect to Ran MCP server
2. Parse user goal into structured objective
3. For each step:
   a. OBSERVE: call get_campaign_state, get_attack_surface
   b. REASON: LLM decides next action based on:
      - Current state vs goal/waypoints
      - Available TTPs (get_applicable_ttps)
      - Parse audit gaps (any NoParser results?)
   c. ACT (one of):
      - execute_action → run a TTP
      - add_parser     → craft a parser for a gap the agent noticed
      - add_ttp        → write a new TTP YAML for an unsupported technique
   d. HANDLE RESULT:
      - Success: update internal state, advance to next waypoint if met
      - NoParser: trigger parser crafting sub-loop
      - Failure: reason about why, try alternative TTP or approach
   e. CHECK GOAL: call check_rbac_goal / check_access_level
4. Report results
```

### 2.3 Failure recovery

The agent must handle:

| Failure | Recovery strategy |
|---|---|
| TTP not applicable | Query `get_attack_surface` for alternative vectors; try different entity |
| TTP execution fails (non-zero exit) | Read stderr, reason about root cause, try alternative procedure or different TTP |
| `NoParser` - effect has no parser | Call `list_parse_audits` to get the raw output, then use `parser_crafter` to generate a parser script and `add_parser` to install it. Optionally re-execute the TTP. |
| `UnknownFormat` - parser exists but can't parse | Same as NoParser but the agent also reads the existing parser to understand what format it expects |
| No TTPs available for current entity | Backtrack: explore other entities from the graph, look for lateral movement paths |
| Goal unreachable | Report to user with explanation of what was tried and why it failed |
| MCP connection lost | Reconnect with exponential backoff |

### 2.4 Parser crafting sub-loop

When the agent encounters a `NoParser` or `UnknownFormat` gap:

```
1. Get the raw output from the parse audit (stdout/stderr of the TTP)
2. Get the effect_id expected (from the TTP YAML)
3. Get the ExternalParseRequest/Response schema (hardcoded in prompt)
4. Ask the LLM to write a Python script that:
   - Reads JSON from stdin (ExternalParseRequest schema)
   - Parses the raw output
   - Writes JSON to stdout (ExternalParseResponse schema)
5. Call add_parser(effect_id, script_content)
6. Re-execute the TTP (or wait for next occurrence)
```

The key constraint: the generated parser must conform to the `ExternalParseResponse`
schema exactly.  The system prompt includes the schema and a reference example.

### 2.5 Parser-gap webhook integration

**Alternative to explicit parser crafting:** The agent runs a local webhook
server that Ran calls via `RAN_PARSER_GENERATOR_WEBHOOK=http://localhost:9090/parse-gap`.

When Ran encounters a `NoParser` gap, it POSTs the `ExternalParseRequest` to
the webhook.  The agent's webhook handler:

1. Receives the request with raw output
2. Asks the LLM to parse it inline (returning `ExternalParseResponse` directly)
3. Optionally also writes a persistent parser script for next time

This creates a **fully automatic** feedback loop where the agent doesn't even
need to _notice_ the gap - Ran pushes it to the agent, and the agent responds
with parsed facts.

### 2.6 Goal specification

Goals can be specified at multiple levels of detail:

```python
# Fully autonomous
goal = "Achieve cluster-admin equivalent RBAC permissions starting from pod entry-hall"

# Waypoint-guided
goal = Goal(
    objective="Achieve cluster-admin RBAC",
    entry="entry-hall",  # resolved to pod via resolve_workload
    waypoints=[
        "Compromise entry-hall pod and enumerate the environment",
        "Find Redis and exploit its service account token",
        "Escalate to node-level execution",
        "Achieve cluster-admin RBAC",
    ],
    check=RbacCheck(verbs=["*"], resources=["*"], api_groups=["*"]),
)
```

### 2.7 Dependencies

```toml
[project]
requires-python = ">=3.11"
dependencies = [
    "mcp",           # MCP Python SDK (official)
    "openai",        # or anthropic - for LLM calls
    "httpx",         # async HTTP for webhook server
    "pydantic",      # structured goal/state models
    "rich",          # terminal output
]
```

---

## Phase 3 - Feedback loop & organic growth

### 3.1 The loop

```
                    ┌─────────────────────────────────┐
                    │                                  │
   Prepared ────►  Ran  ────► Agent observes,          │
   Cluster         runs       executes, encounters     │
                   TTPs       gaps                     │
                    │                                  │
                    ▼                                  │
              Parse gap?  ──yes──►  Agent crafts       │
                    │               parser/TTP         │
                   no               │                  │
                    │               ▼                  │
                    │          armory/parsers/          │
                    │          armory/TTPs/             │
                    │               │                  │
                    ▼               │                  │
              Goal reached?  ◄──────┘                  │
                    │                                  │
                   yes                                 │
                    │                                  │
                    ▼                                  │
              Next cluster  ───────────────────────────┘
              / scenario
```

### 3.2 What grows organically

| Artifact | How it grows | Persists across runs |
|---|---|---|
| Parser scripts (`armory/parsers/*.py`) | Agent writes them when encountering `NoParser` | Yes - committed to repo |
| TTP YAML (`armory/TTPs/**/*.yaml`) | Agent writes new TTPs for techniques it needs but doesn't find | Yes - committed to repo |
| Knowledge graph facts | Each emulation discovers new entities, relations, RBAC | Per-campaign (reset between runs) |
| Agent experience | LLM context about what worked/failed | Via conversation history or RAG |

### 3.3 Quality control

Generated parsers and TTPs should be reviewed before committing.  Options:

1. **Staging directory:** Generated artifacts go to `armory/parsers/.generated/`
   and `armory/TTPs/.generated/`.  A human reviews and promotes.
2. **Validation on write:** The `add_parser` MCP tool runs the generated parser
   against the original raw output and verifies it produces valid JSON. The
   `add_ttp` tool validates the YAML against the TTP schema.
3. **Test harness:** After the run, a CI step replays all `ExternalParseRequest`
   fixtures through the generated parsers and checks for regressions.

---

## Implementation order

| Step | What | Where | Depends on |
|---|---|---|---|
| 1 | Add `rmcp` to `crates/api`, implement tool router with 3-4 core tools (`get_graph`, `list_ttps`, `get_applicable_ttps`, `execute_action`) | `crates/api/src/mcp.rs` | - |
| 2 | Add `resolve_workload`, `get_entity`, `get_attack_surface` tools | `crates/api/src/mcp.rs` | Step 1 |
| 3 | Add `wait_for_result` (subscribe to campaign events) | `crates/api/src/mcp.rs` | Step 1 |
| 4 | Add `add_parser`, `add_ttp`, `list_parse_audits` tools | `crates/api/src/mcp.rs` | Step 1 |
| 5 | Add `check_rbac_goal`, `check_access_level` tools | `crates/api/src/mcp.rs` | Step 2 |
| 6 | Add optional `ran mcp-server` stdio subcommand | `crates/cli/src/main.rs` | Step 1 |
| 7 | Python agent POC: MCP client + ReAct loop | `ran-agent/` | Step 1-5 |
| 8 | Python agent: parser-gap webhook server | `ran-agent/` | Step 4, 7 |
| 9 | Python agent: goal specification + waypoint tracking | `ran-agent/` | Step 7 |
| 10 | Integration test with a prepared local cluster | `examples/scenarios/` | Step 7-9 |

---

## Open questions

1. **LLM choice for agent:** OpenAI (tool-calling is mature) vs Anthropic
   (better at code generation for parser crafting)?  The POC should be
   provider-agnostic with a simple adapter.
2. **Parser validation:** How strict?  Run-against-sample validation catches
   obvious errors but not semantic correctness.
3. **TTP hot-reload:** The `Armory` currently loads once at startup.  For
   `add_ttp` to work live, we need `Armory::reload()` or
   `Armory::add_ttp(ttp)`.
4. **Rate limiting:** The agent could run TTPs very fast.  Should there be a
   configurable delay between executions for realism/safety?
