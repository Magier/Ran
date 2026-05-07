---
title: Ran mdBook Documentation
date: 2026-05-07
status: approved
---

# Ran mdBook Documentation — Design Spec

## Goal

Build a user-facing documentation site for Ran using mdBook, hosted at `docs/book/` in the repository. The audience is both human operators and AI agents. The docs explain how to use Ran — from running a single technique to orchestrating multi-step campaign emulation — and serve as a reference for building custom TTP YAML definitions when the built-in armory is insufficient.

The docs are explicitly **not** a technical description of the codebase or internal crates.

## Audience

Two distinct audiences, each supported with dedicated sections but sharing a common foundation:

- **Human operators and security practitioners** — want to run atomic tests, explore the armory, and understand how campaign emulation works
- **AI agents** — need a precise, structured description of TTP YAML authoring and the Ran API/MCP surface so they can author new techniques and drive campaigns programmatically

## Location

`docs/book/` — a standard mdBook project inside the existing repository, alongside the `docs/` scratchpad folder.

## Structure

The book follows a journey-based progression: start simple (run a single technique), advance to the core capability (campaign emulation), then go deeper only when needed (custom TTP authoring). The Armory is introduced as a first-class concept early and stays relevant throughout.

```
Introduction
  - What is Ran?
  - Key concepts: TTPs, MITRE tactics, campaigns (brief glossary)

Getting Started
  - Installation (binary, Docker, build from source)
  - Connecting to a cluster (kubeconfig)

The Armory
  - What is the armory?
  - Browsing available TTPs (`ran armory`)
  - TTP anatomy: id, name, tactic, technique, parameters
  - Using a custom armory directory

Atomic Testing
  - Running a single TTP (`ran invoke`)
  - Targeting: namespaces, pods, service accounts
  - Parameters and how they resolve
  - Cleanup

Campaign Emulation  ← the core of the book
  - What is a campaign?
  - The context model: entities and the knowledge graph
  - How TTPs update the world (effects)
  - Preconditions: how Ran selects applicable TTPs from the armory
  - Running a multi-step emulation
  - The C2 layer: sessions and lateral movement
  - Reading the attack trail

Extending the Armory  ← only when built-ins aren't enough
  - When (and when not) to write a custom TTP
  - Writing procedures: shell, k8s-request, http-request, steps
  - Preconditions in depth
  - Effects in depth

Reference
  - YAML field catalog (all TTP fields, all parameter types)
  - Effect catalog (all built-in effects and their argument signatures)
  - Precondition types

For Agents
  - Authoring TTPs as an agent
  - Using Ran via API / MCP
```

## Chapter Content Guidance

### Introduction
A short narrative: what Ran is, why micro-emulation over atomic tests, the MITRE ATT&CK framing. Ends with a minimal glossary of recurring terms (TTP, tactic, technique, campaign, entity, effect, precondition).

### Getting Started
Mirrors the current README installation section. Covers binary, Docker, and source builds. Explains kubeconfig requirements and the authorization warning.

### The Armory
Introduces the armory as Ran's library of executable techniques. Explains the YAML structure at a high level (enough to read and run a TTP, not yet to write one). Shows `ran armory` output and how to navigate by tactic. Explains custom armory directories.

### Atomic Testing
Covers `ran invoke` and `ran emulate` (interactive UI). Explains targeting syntax (`namespace/pod`), how parameters are resolved (defaults, overrides, special variables like `${TOKEN}`, `${NS}`, `${API_SERVER}`), and what cleanup does and when to run it.

### Campaign Emulation
The central section. Explains the campaign as a live, evolving world model rather than a list of executed commands.

- **Context model**: entities (Pod, ServiceAccount, Node, Role, RoleBinding, etc.) and relations (can-exec, can-reach, runs-on, container.escape, c2.session, rce.can-exec). Explains entity IDs and the graph structure.
- **Effects**: how a TTP declares what it discovers or establishes, how the runtime parses effect strings and updates the campaign state.
- **Preconditions**: how `kind`, `rbac`, and `accessLevel` constraints gate which TTPs are applicable at any given moment. How Ran matches the current context against preconditions to surface the applicable TTP set.
- **Multi-step emulation**: the interactive UI workflow — how the operator selects a target, sees applicable TTPs, executes, and watches the knowledge graph grow.
- **C2 layer**: what C2 backends are (Sliver integration), how sessions appear as `c2.session` relations, how the campaign routes subsequent commands through established channels (envelopes).
- **Attack trail**: how the execution record and the entity/relation graph together form the audit trail, and how to export it.

### Extending the Armory
Addresses the reader who needs a technique that doesn't exist yet. Starts with guidance on when *not* to write a custom TTP (prefer parameterizing an existing one). Then walks through a complete YAML from scratch:

- `procedures`: shell commands, `k8s_request` blocks (structured API calls), `http_request` blocks, `steps` sequences
- `preconditions`: `kind` (entity type the TTP runs on), `rbac` (required verbs/resources), `accessLevel`
- `effects`: all built-in effect strings, argument conventions, the `sys` placeholder

### Reference
Complete, machine-readable field-by-field documentation. Intended as a lookup table for both humans and agents.

- Every TTP YAML field with type, required/optional, and meaning
- Every built-in effect with its argument signature and what entities/relations it creates
- Every precondition type

### For Agents
Dedicated section targeting AI agents. Two parts:
1. **Authoring TTPs** — precise rules for valid YAML (field requirements, valid effect strings, valid precondition shapes), common patterns, validation checklist
2. **Driving campaigns via API/MCP** — the available endpoints, the MCP tool surface, how to read campaign state and select next actions

## Technical Setup

- Standard mdBook project (`book.toml`, `src/SUMMARY.md`, chapter files)
- No custom themes initially — default mdBook theme is sufficient
- Code blocks throughout for YAML examples and CLI invocations
- Cross-references between chapters (e.g., "Preconditions in depth" in Extending links back to the Campaign Emulation preconditions explanation)
- `book/` output excluded from git (`.gitignore`)

## Success Criteria

- A new user can install Ran, connect to a cluster, and run their first atomic TTP by following Getting Started + Atomic Testing alone
- An operator can understand what a campaign is and run a multi-step emulation without reading Extending the Armory
- An AI agent can author a valid, runnable custom TTP using only the Extending the Armory + Reference sections
- The For Agents section gives a structured, unambiguous description of the YAML contract
