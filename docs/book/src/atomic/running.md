# Running a TTP

Ran supports atomic execution from the CLI and interactive execution in the browser UI.

## CLI: `ran trigger`

```sh
ran armory

ran trigger get-pods \
  --target ns/default/pod/my-pod

ran trigger get-serviceaccounts \
  --target ns/default/pod/my-pod \
  --arg ALL_NS=true
```

The first positional argument is the TTP ID. `--target` must be the canonical pod entity ID `ns/<namespace>/pod/<name>`. Repeat `--arg KEY=VALUE` to override TTP parameters. `--procedure` and `--exec-system` select a specific execution path when needed.

## Browser UI: `ran emulate`

```sh
ran emulate
```

Open <http://localhost:8080>. Select an entity, choose an applicable TTP, set its parameters and procedure, then execute it. REST carries commands and SSE at `/events` carries live campaign updates.

Useful server options include:

| Option                | Description                                     |
| --------------------- | ----------------------------------------------- |
| `--port 8080`         | HTTP port                                       |
| `--kubeconfig <path>` | Kubeconfig override                             |
| `--armory <path>`     | Custom TTP directory                            |
| `--config <path>`     | Ran YAML configuration                          |
| `--plan <path>`       | Run a YAML plan on startup                      |
| `--cleanup`           | Automatically clean up after a launch-time plan |
