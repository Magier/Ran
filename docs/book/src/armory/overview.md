# What is the Armory?

The armory is Ran's library of executable techniques. Each entry is a YAML file
that describes a single adversary action: what it does, which MITRE ATT&CK tactic
and technique it maps to, what conditions must be true before it can run, how to
execute it, and what it reveals about the target environment once it succeeds.

## Organisation

Techniques are grouped by MITRE tactic:

```text
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
