# Roadmap

## Available today

- Rust CLI: `ran emulate`, `ran trigger`, and `ran armory`
- Browser UI backed by REST and SSE
- YAML armory with Kubernetes-focused TTPs
- YAML campaign plans and campaign reset/cleanup support
- Kubernetes exec and reverse-shell session channels
- Ran JSON campaign flow from `GET /api/flow`
- MCP tools over the Rust API

## Near term

- Expand cleanup coverage across the armory
- Improve execution tracing, failure classification, and audit presentation
- Continue imperative plan and hierarchical TTP support
- Add additional Kubernetes techniques and D3FEND mappings
- Explore Behavior Tree and goal-directed planning

## Future integrations

These capabilities will be designed natively for the Rust runtime. They are not constrained by retired implementation contracts.

- Native Rust Sliver RPC backend
- MITRE Attack Flow/STIX import and export
- WebSocket RPC with a newly designed protocol
- Observable extraction and optional detection-content generation

Sliver-only TTPs remain in the armory as disabled design sketches until a backend exists.
