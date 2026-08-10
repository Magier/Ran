# Running a TTP

Ran offers two modes for executing a single technique: the command line and the
interactive web UI.

## CLI: `ran invoke`

Invoke a specific TTP by its ID without starting the full web server:

```sh
ran invoke <ttp-id> --target <namespace>/<pod>
```

Examples:

```sh
# List all pods from within a compromised pod
ran invoke get-pods --target default/my-pod

# Enumerate service accounts cluster-wide
ran invoke get-serviceaccounts --target default/my-pod --param ALL_NS=true

# List all available TTPs first
ran armory
```

The `--target` flag selects the entity the TTP runs against. Initial Access via
an external kubeconfig targets the selected Pod; Resource Development actions
may run locally on the operator side.

## Web UI: `ran emulate`

Start the Ran server and open the UI in your browser:

```sh
ran emulate
# 🚀 Server started on :8080
```

Open `http://localhost:8080`.

The UI is divided into three panels:

- **Left:** the armory, filterable by tactic. Applicable TTPs (preconditions met)
  are highlighted.
- **Centre:** the cluster map — entities discovered so far and the relations between them.
- **Right:** execution panel for the selected TTP — parameters, procedure choice,
  and the output log.

To run a technique: select a target entity in the centre panel → pick a TTP from
the left panel → adjust parameters if needed → click **Execute**.

## Key flags

| Flag | Default | Description |
|---|---|---|
| `--port`, `-p` | `8080` | Port the web UI listens on |
| `--target`, `-t` | — | Initial target: `<namespace>/<pod-or-service>` |
| `--godmode` | `false` | Pre-load all cluster resources from kubeconfig on startup |
| `--armory`, `-a` | — | Path to a custom armory directory |
| `--config` | `ran.yaml` | Path to a custom config file |
