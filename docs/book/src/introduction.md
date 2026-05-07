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
