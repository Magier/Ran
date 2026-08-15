<p align="center">
  <img src="./docs/Ran.svg" width="110" alt="Ran"/>
</p>

<h1 align="center">Ran</h1>

<p align="center">
  <strong>Adversary emulation for Kubernetes clusters</strong>
</p>

<p align="center">
  <a href="https://github.com/magier/ran/actions/workflows/build-rust.yaml"><img src="https://img.shields.io/github/actions/workflow/status/magier/ran/build-rust.yaml?label=build&logo=github" alt="Build Status"/></a>
  <a href="https://github.com/magier/ran/releases/latest"><img src="https://img.shields.io/github/v/release/magier/ran?logo=github" alt="Latest Release"/></a>
  <a href="https://github.com/magier/ran/pkgs/container/ran"><img src="https://img.shields.io/badge/container-ghcr.io-blue?logo=github" alt="Container Registry"/></a>
  <img src="https://img.shields.io/badge/rust-1.93-CE422B?logo=rust" alt="Rust 1.93"/>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-green" alt="Apache 2.0 License"/></a>
</p>

> [!CAUTION]
> Run Ran only in environments you own or have explicit permission to test.

> [!WARNING]
> Ran is early-stage and experimental. See the [roadmap](./Milestones.md).

<p align="center">
  <img src="./docs/ui_example.png" alt="Ran browser UI" width="800"/>
</p>

## What is Ran?

Ran is a Rust application for executing Kubernetes-focused adversary techniques mapped to MITRE ATT&CK. Its YAML armory describes executable TTPs, preconditions, effects, and cleanup. The browser UI builds a campaign graph as techniques run, while the CLI also supports one-shot atomic execution and YAML plans.

Current interfaces are:

- `ran emulate` for the REST/SSE server, browser UI, and optional YAML plan execution
- `ran trigger` for one TTP against a target pod
- `ran armory` for listing available TTPs
- REST under `/api` and server-sent events at `/events`
- Ran JSON campaign-flow export from `GET /api/flow` or **Save Ran JSON** in the UI

Ran does not currently expose WebSocket RPC or a Sliver backend. MITRE Attack Flow/STIX is not the current flow format.

## Installation

### Release binary

Pre-built binaries are available on the [Releases page](https://github.com/magier/ran/releases/latest).

```sh
# Linux amd64
curl -sL https://github.com/magier/ran/releases/latest/download/ran-linux-amd64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/

# macOS Apple Silicon
curl -sL https://github.com/magier/ran/releases/latest/download/ran-darwin-arm64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/
```

### Container

```sh
docker pull ghcr.io/magier/ran:latest
docker run --rm -it \
  -v ~/.kube:/root/.kube:ro \
  -p 8080:8080 \
  ghcr.io/magier/ran:latest emulate --port 8080
```

Open <http://localhost:8080>.

### Build from source

Prerequisites: Rust 1.93+, Node.js 24+, and pnpm.

```sh
git clone https://github.com/magier/ran.git
cd ran
make build
./target/release/ran --help
```

For a source-building development image, run `make docker-local`.

## Quick start

Ran uses the active kubeconfig context unless `--kubeconfig` is supplied.

```sh
# Interactive browser UI and API
ran emulate

# Use a custom armory or config
ran emulate --armory /path/to/TTPs --config ./ran.yaml

# Execute a YAML plan, prompting for cleanup when complete
ran emulate --plan ./plan.yaml

# Execute one enabled TTP
ran trigger get-pods \
  --target ns/default/pod/my-pod \
  --arg NS=default

# Browse the armory (disabled design sketches are labelled)
ran armory
```

The target accepted by `ran trigger` is a canonical pod entity ID:
`ns/<namespace>/pod/<name>`.

A minimal namespace filter in `ran.yaml` looks like:

```yaml
namespaces:
  excluded:
    - kube-system
    - kube-public
```

An `included` list takes precedence over `excluded`. See
[namespace filtering](docs/NAMESPACE_FILTERING.md).

## Architecture

| Component | Responsibility                                                              |
| --------- | --------------------------------------------------------------------------- |
| Armory    | Loads YAML TTPs and validates their procedures and contracts                |
| Campaign  | Tracks entities, relations, applicability, effects, and execution history   |
| C2        | Runs commands through Kubernetes exec or established reverse-shell sessions |
| API       | Serves REST, SSE, MCP, the browser UI, and campaign-flow JSON               |
| CLI       | Exposes `emulate`, `trigger`, and `armory`                                  |

REST is the supported request/response transport. SSE at `/events` streams state changes. The reverse-shell listener provides the current session-based command channel.

## Flow export

`GET /api/flow` returns Ran's native campaign-flow JSON:

```json
{
  "steps": [],
  "edges": []
}
```

This is the same payload downloaded by the UI. It is not a STIX bundle and no compatibility with a historical format is promised.

## Roadmap

The implementation-neutral roadmap includes:

- native Rust Sliver RPC support;
- MITRE Attack Flow/STIX import and export;
- a newly designed WebSocket RPC protocol, without compatibility constraints from the retired implementation;
- broader cleanup coverage and planning strategies.

See [Milestones.md](./Milestones.md) for more.

## Contributing

Contributions are welcome, especially new armory TTPs, tests, and documentation improvements.

## License

Ran is released under the [Apache 2.0 License](LICENSE).
