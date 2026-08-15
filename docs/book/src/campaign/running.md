# Running Multi-Step Emulation

Start the server:

```sh
ran emulate
```

In the browser UI:

1. Choose a live pod for the initial-access action.
2. Select an entity in the campaign graph.
3. Choose a TTP whose preconditions are satisfied.
4. Select an eligible Kubernetes identity and procedure, then adjust parameters.
5. Execute and inspect streamed output.
6. Follow newly discovered entities and execution channels.
7. Reset the campaign when finished; available cleanup procedures run first.

For repeatable automation, run a YAML plan:

```sh
ran emulate --plan ./plan.yaml
```

The server prompts for cleanup when the plan completes. Add `--cleanup` for non-interactive cleanup.

At any point, download Ran JSON from the UI or read `GET /api/flow`. MITRE Attack Flow/STIX import and export are planned separately.
