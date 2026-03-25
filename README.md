<p align="center">
  <img src="./docs/Ran.svg" width="110" alt="Ran"/>
</p>

<h1 align="center">Ran</h1>

<p align="center">
  <strong>Adversary emulation tool for Kubernetes clusters</strong><br/>
  <sub>Named after Rán — Norse goddess of the sea, whose net ensnares the unwary into the depths</sub>
</p>

<p align="center">
  <a href="https://github.com/magier/ran/actions/workflows/build.yaml"><img src="https://img.shields.io/github/actions/workflow/status/magier/ran/build.yaml?label=build&logo=github" alt="Build Status"/></a>
  <a href="https://github.com/magier/ran/releases/latest"><img src="https://img.shields.io/github/v/release/magier/ran?logo=github" alt="Latest Release"/></a>
  <a href="https://github.com/magier/ran/pkgs/container/ran"><img src="https://img.shields.io/badge/container-ghcr.io-blue?logo=github" alt="Container Registry"/></a>
  <img src="https://img.shields.io/badge/go-1.24-00ADD8?logo=go" alt="Go 1.24"/>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-green" alt="Apache 2.0 License"/></a>
</p>

> [!CAUTION]
> This tool is intended for **educational and authorized demonstration purposes only**. Any other usage is not endorsed by the authors.

> [!WARNING]
> Ran is early-stage and highly experimental. Use at your own risk. See [Milestones](./Milestones.md) for the planned roadmap.

<p align="center">
  <img src="./docs/ui_example.png" alt="Ran UI" width="800"/>
</p>

---

## What is Ran?

Ran is an adversary emulation platform for modern Kubernetes environments with two core objectives:

- **Realistic TTP emulation** — execute predefined adversary techniques mapped to the MITRE ATT&CK framework against your own cluster
- **Living knowledge base** — a curated armory of known Kubernetes attack vectors, ready to run

Ran covers the full MITRE ATT&CK tactic spectrum for Kubernetes: from *Initial Access* and *Discovery* through *Privilege Escalation*, *Lateral Movement*, and *Impact*.

### Why Ran?

A common security cliché:
> *an attacker only has to be right once, but a defender has to be right every time*

This holds for Initial Access — but the dynamic flips afterwards. Post-IA, defenders have full environmental visibility while the attacker must explore. This is a major defensive advantage that purely atomic, single-event detections fail to leverage.

Ran encourages **micro-emulation**: multi-step sequences where a simulated adversary discovers and adapts to your environment, surfacing detection gaps that atomic tests miss entirely.

#### For defenders

- View your cluster through an adversary's lens to find gaps in visibility and detection coverage
- Record and replay attacker step sequences to validate detection logic
- Export full attack trails as [MITRE Attack Flow](https://ctid.mitre.org/projects/attack-flow) (STIX 2) for documentation and threat-informed defense

---

## Installation

### Download a release binary (recommended)

Pre-built binaries for Linux, macOS (Intel & Apple Silicon), and Windows are available on the [Releases page](https://github.com/magier/ran/releases/latest).

```sh
# macOS (Apple Silicon)
curl -sL https://github.com/magier/ran/releases/latest/download/ran-darwin-arm64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/

# macOS (Intel)
curl -sL https://github.com/magier/ran/releases/latest/download/ran-darwin-amd64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/

# Linux (amd64)
curl -sL https://github.com/magier/ran/releases/latest/download/ran-linux-amd64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/
```

Verify:
```sh
ran --help
```

### Docker

```sh
docker pull ghcr.io/magier/ran:latest
```

Run against your local kubeconfig:
```sh
docker run --rm -it \
  -v ~/.kube:/root/.kube:ro \
  -p 8080:8080 \
  ghcr.io/magier/ran:latest emulate --port 8080
```

Then open `http://localhost:8080` in your browser.

### Build from source

**Prerequisites:** Go 1.24+, Node.js 20+, pnpm

```sh
git clone https://github.com/magier/ran.git
cd ran
make build
./dist/ran --help
```

---

## Quick Start

Ran reads your local `~/.kube/config` to discover and target cluster resources. Ensure your kubeconfig is configured and points at the cluster you want to emulate against.

> [!IMPORTANT]
> Only run Ran against clusters you own or have explicit written permission to test.

### Interactive emulation mode

Start the Ran server and open the web UI to run TTPs interactively:

```sh
ran emulate
# 🚀 Server started on :8080
```

Open `http://localhost:8080` to explore and execute techniques from the armory.

**Key flags:**

| Flag | Default | Description |
|---|---|---|
| `--port, -p` | `8080` | Port to listen on |
| `--target, -t` | — | Initial target: `<namespace>/<pod-or-service>` |
| `--godmode` | `false` | Use local kubeconfig to load all cluster resources |
| `--armory, -a` | — | Path to a custom armory directory |
| `--config` | `ran.yaml` | Path to a custom config file |

### Atomic testing (single TTP)

Run a single TTP directly from the command line without the UI:

```sh
# List all available TTPs in the armory
ran armory

# Execute a specific TTP by ID
ran invoke <ttp-id> --target <namespace>/<pod>

# Example: list pods from within a compromised pod
ran invoke get-pods --target default/my-pod
```

### Configuration

Ran looks for `ran.yaml` in the current working directory. Copy the example to get started:

```sh
cp ran.yaml.example ran.yaml
```

```yaml
namespaces:
  # Blacklist mode: hide noisy system namespaces
  excluded:
    - kube-system
    - kube-public
    - kube-node-lease

  # Whitelist mode: show only specific namespaces (takes precedence over excluded)
  # included:
  #   - default
  #   - production
```

---

## Architecture

Ran is built around two major components:

| Component | Responsibility |
|---|---|
| **Actuator** | Executes TTPs against the cluster, tracks results, and builds the audit trail |
| **Planner / Reasoner** | Decides which actions to run and in what order |

### Armory

The armory is Ran's library of executable TTPs. Each TTP is a YAML file describing the technique, its MITRE mapping, required preconditions (RBAC, access level), and one or more execution procedures.

```yaml
id: get-pods
name: Get Pods via K8s API
tactic: Discovery
techniques: ["Container and Resource Discovery", T1613]
preconditions:
  rbac:
    - verb: list
      resource: pods
procedures:
  - key: kubectl
    command: kubectl get pods --token=${TOKEN} -n=${NS}
  - key: curl
    command: >-
      curl -H "Authorization: Bearer ${TOKEN}"
      "${API_SERVER}/api/v1/namespaces/${NS}/pods"
```

TTPs are organized by MITRE tactic:

```
armory/TTPs/
├── CommandAndControl/
├── CredentialAccess/
├── Defense Evasion/
├── Discovery/
├── Execution/
├── Impact/
├── InitialAccess/
├── Lateral Movement/
├── Persistence/
├── Privilege Escalation/
└── Resource Development/
```

### Planner approaches

Ran is designed to support progressively more autonomous planning:

| Mode | Status | Description |
|---|---|---|
| **Human operator** | ✅ Available | Manually select and invoke individual TTPs |
| **Imperative plan** | 🔄 In progress | Follow a pre-defined [Attack Flow](https://ctid.mitre.org/projects/attack-flow) runbook |
| **Classical AI** | 🗺️ Planned | Behavior Trees, HTN, GOAP |
| **Modern AI** | 🔭 Future | Reinforcement learning, Active Inference |

*Planning approaches are inspired by [📖 Artificial Intelligence: A Modern Approach](https://aima.cs.berkeley.edu/) and motivated by MITRE's [📄 Automated Adversary Emulation: A Case for Planning and Acting with Unknowns](https://www.mitre.org/sites/default/files/2021-11/prs-18-0944-1-automated-adversary-emulation-planning-acting.pdf).*

---

## Roadmap

See [Milestones.md](./Milestones.md) for the full roadmap. Key upcoming work:

- [ ] Cleanup logic for every TTP
- [ ] Attack Flow as an executable plan input
- [ ] Derive STIX Observables from TTP execution
- [ ] [D3FEND](https://d3fend.mitre.org/) mapping
- [ ] [MCP](https://modelcontextprotocol.io) server support 🤖
- [ ] Autonomous emulation via Behavior Trees

---

## Similar Projects

| Tool | Focus |
|---|---|
| [Caldera](https://github.com/mitre/caldera) | General adversary emulation platform |
| [Peirates](https://github.com/inguardians/peirates) | Kubernetes penetration testing |
| [Kubesploit](https://github.com/cyberark/kubesploit) | Kubernetes post-exploitation |
| [kube-hunter](https://github.com/aquasecurity/kube-hunter) | Kubernetes weakness discovery |
| [Leonidas](https://github.com/WithSecureLabs/leonidas) | AWS/K8s attack simulation |
| [IceKube](https://github.com/WithSecureLabs/IceKube) | Kubernetes attack path analysis |
| [Stratus Red Team](https://stratus-red-team.cloud) | Cloud-native attack techniques |
| [CDK](https://github.com/cdk-team/CDK/) | Container/K8s penetration toolkit |
| [kdigger](https://github.com/quarkslab/kdigger) | In-cluster context discovery |
| [red-kube](https://github.com/lightspin-tech/red-kube) | Kubernetes red team scripts |
| [MKAT](https://github.com/DataDog/managed-kubernetes-auditing-toolkit/) | Managed Kubernetes auditing |
| [clusterfuck](https://bsssq.xyz/posts/kube/) | Kubernetes exploitation |

For a detailed feature comparison see [docs/tool_comparison.md](docs/tool_comparison.md).

---

## References

- [MITRE ATT&CK for Containers](https://attack.mitre.org/matrices/enterprise/containers/)
- [MITRE — Automated Adversary Emulation: A Case for Planning and Acting with Unknowns](https://www.mitre.org/sites/default/files/2021-11/prs-18-0944-1-automated-adversary-emulation-planning-acting.pdf)
- [Raesene's Kubernetes Security Lab](https://github.com/raesene/kube_security_lab)
- [BishopFox — BadPods: Kubernetes Pod Privilege Escalation](https://bishopfox.com/blog/kubernetes-pod-privilege-escalation)
- [D3FEND](https://d3fend.mitre.org)

---

## Contributing

Contributions are welcome — especially new TTPs in the armory, bug reports, and documentation improvements. Please open an issue or pull request on [GitHub](https://github.com/magier/ran).

## License

Ran is released under the [Apache 2.0 License](LICENSE).


