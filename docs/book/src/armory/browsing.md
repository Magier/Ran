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
