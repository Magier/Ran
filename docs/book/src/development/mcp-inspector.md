# Debugging the MCP Server with MCP Inspector

Start Ran's Rust server:

```sh
ran emulate
```

Then connect MCP Inspector to the Streamable HTTP endpoint:

```sh
npx @modelcontextprotocol/inspector
```

Use `http://localhost:8080/mcp` as the server URL. The MCP endpoint shares the same process and campaign state as the REST API and browser UI.

For protocol-level diagnostics, set `RAN_LOG=debug` before starting Ran. Browser campaign updates use SSE at `/events`; there is no WebSocket RPC endpoint.
