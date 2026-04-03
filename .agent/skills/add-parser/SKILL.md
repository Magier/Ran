---
name: add-parser
description: >
  Implement a new output parser in the Ran codebase that transforms raw TTP execution results
  into domain facts. Use this skill whenever the user provides a TTP YAML path, execution
  arguments, and raw output — and wants the output parsed into the campaign graph. Also trigger
  when the user says things like "add a parser for X", "this TTP output isn't being parsed",
  "write a parser for [effect]", "the facts aren't being extracted", or pastes TTP output and
  asks what can be learned from it. Even if the user just mentions a TTP name and says the
  results aren't being recorded — that's this skill.
---

# Add Output Parser

You are adding one or more Rust output parser functions to the Ran red-team platform so that
facts discovered by a TTP execution are written into the campaign graph.

## What you receive

The user gives you:
- **TTP path** — the `.yaml` file in `armory/TTPs/`
- **Runtime args** — the parameter values that were substituted at execution time
- **Results** — the raw strings produced by the TTP (`results[0]` = stdout, `results[1]` = stderr, further indices for multi-output procedures)

Read the TTP YAML before doing anything else. It tells you the command, what domain the output
belongs to, and — critically — whether `effects` is already defined.

## Two starting paths

### Path A — effects already defined

The TTP YAML has an `effects` list with one or more strings like `["k8s.nodeList"]`.
Each string names an output parser to implement. A single TTP may declare multiple effects;
handle all of them in one session.

Jump straight to **Implementing the Parser**.

### Path B — no effects defined

The TTP has an empty `effects` list or the key is absent/commented-out.

1. Reason about the command and the raw output: what kind of facts does it reveal?
2. Propose one or more effect IDs following the naming convention below.
3. Show the user:
   - The proposed effect ID(s) and a one-sentence rationale for each
   - The exact YAML diff (add/replace the `effects:` block)
4. **Wait for explicit approval** before touching the YAML.
5. Once approved, update the YAML and proceed to **Implementing the Parser**.

**Effect naming convention:** `domain.thing` in lowercase.
- `sys.*` — facts about the runtime system: `sys.envvar`, `sys.ip`, `sys.user`, `sys.process`, `sys.mount`
- `k8s.*` — Kubernetes resource facts: `k8s.podList`, `k8s.nodeList`, `k8s.secretList`, `k8s.serviceAccountList`, `k8s.roleList`
- Use plural (`List`) when the output is a collection of resources; use singular when it is one item.

## Domain model checkpoint

Before writing any code, verify that every fact the output reveals can be
represented using the existing domain model. Open `references/domain_types.md`
and go through the output field by field:

- Does the fact belong on an existing entity (`Pod`, `K8sNode`, `Namespace`,
  `ServiceAccount`, `K8sCluster`)? If so, which field?
- Does it belong on `SystemInfo` (attached to pods or nodes)? If so, which
  field (`env_vars`, `user_id`, `username`, `ips`, `processes`, `files`,
  `mounts`, `binaries`, `os`, `access_level`)?
- Does it express a relationship between entities (`RunsOn`, `Contains`,
  `PodExec`, `KubeletExecSource`)? If so, which one?

If every fact maps cleanly: proceed to **Implementing the Parser**.

If something doesn't fit — a fact the output reveals that has no home in the
current domain model — **stop here**. Tell the user:

> "The output contains [X], which can't be represented in the current domain
> model. The existing entity types and SystemInfo fields don't cover this.
> Implementing this parser would require extending the domain model first
> (adding a field, a new entity type, or a new relation). This is outside the
> scope of this skill — would you like to handle that separately before
> continuing?"

Do not improvise a workaround (squashing unknown facts into the wrong field,
inventing ad-hoc structs, etc.). A parser that silently discards or
misrepresents facts is worse than no parser at all.

## Implementing the parser

All output parsers live in:
```
crates/campaign/src/output_parsers.rs
```

Read the full file before writing any code. Then for each effect:

### 1. Write the parser function

Model it on `parse_sys_envvar`. The signature is always:

```rust
fn parse_<effect_slug>(
    campaign: &mut Campaign,
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
) -> ParsedEffect
```

where `<effect_slug>` is the effect ID with `.` replaced by `_` (e.g. `k8s.nodeList` → `parse_k8s_node_list`).

**Standard structure:**

```rust
fn parse_k8s_node_list(
    campaign: &mut Campaign,
    effect_id: &str,
    cmd: &ExecTtp,
    event: &TtpExecuted,
) -> ParsedEffect {
    let Some(stdout) = event.results.first() else {
        return ParsedEffect {
            updates: FactsUpdate::default(),
            audit: build_audit(effect_id, cmd, event, ParseResult::KnownFailure,
                               "missing stdout payload", 0),
        };
    };

    // --- parse stdout into domain facts ---
    // ...

    if /* parse failed */ {
        return ParsedEffect {
            updates: FactsUpdate::default(),
            audit: build_audit(effect_id, cmd, event, ParseResult::UnknownFormat,
                               "reason it failed", 0),
        };
    }

    let mut updates = FactsUpdate::default();
    // push entities / relations into updates.new_entities / updates.new_relations
    let count = updates.new_entities.len();

    ParsedEffect {
        updates,
        audit: build_audit(effect_id, cmd, event, ParseResult::Parsed,
                           "short description of what was parsed", count),
    }
}
```

**Key decisions:**
- Use `ParseResult::KnownFailure` when the output is structurally valid but missing expected fields (e.g. empty stdout).
- Use `ParseResult::UnknownFormat` when the output doesn't look like what you expected (e.g. kubectl JSON came back as plain text).
- Use `ParseResult::ParserBug` only when you catch a Rust-level panic/expect you decided to recover from.
- `inferred_facts_written` should count the net new entities or relation edges (or env-var keys, etc.) that were actually written.

**Parsing JSON output:**
Most k8s TTPs return `kubectl ... --output=json`. Parse with `serde_json`:

```rust
let Ok(root) = serde_json::from_str::<serde_json::Value>(stdout) else {
    return ParsedEffect {
        updates: FactsUpdate::default(),
        audit: build_audit(effect_id, cmd, event, ParseResult::UnknownFormat,
                           "stdout is not valid JSON", 0),
    };
};

let items = root["items"].as_array().cloned().unwrap_or_default();
```

Prefer extracting fields by string key (`.get("name")`) rather than defining a full deserialization type, unless the structure is complex enough to warrant it.

**Writing facts — read `references/domain_types.md`** for the full list of entities, their constructors, and available SystemInfo fields before choosing what to write.

### 2. Register the parser

Add a match arm in `resolve_output_parser`:

```rust
fn resolve_output_parser(effect_name: &str) -> Option<OutputParserHandler> {
    match effect_name.trim().to_ascii_lowercase().as_str() {
        "sys.envvar" => Some(parse_sys_envvar),
        "k8s.nodelist" => Some(parse_k8s_node_list),  // ← add here
        _ => None,
    }
}
```

The match key is the effect ID lowercased (dots preserved, spaces trimmed). This makes lookup case-insensitive, consistent with the existing `sys.envVar` → `sys.envvar` behaviour.

### 3. Add imports if needed

`serde_json` is already a dependency. For new entity types you'll likely need:

```rust
use ran_domain::{K8sNode, Pod, Namespace, ServiceAccount, K8sMeta};
```

Check the existing imports at the top of the file first.

## Writing tests

Tests go in the existing `#[cfg(test)] mod tests` block at the bottom of `output_parsers.rs`.

Every parser needs **at least three tests**:

| # | What to test | Hint |
|---|---|---|
| 1 | Happy path — fixture with realistic output → `ParseResult::Parsed`, correct facts written | Use the actual output the user gave you as the fixture |
| 2 | Missing or empty stdout → `ParseResult::KnownFailure` | Pass `vec![]` or `vec!["".to_string()]` |
| 3 | Malformed/unexpected output → `ParseResult::UnknownFormat` | Pass garbage or wrong-format string |

Use the existing `sample_cmd()` / `sample_event()` helpers as models. For tests that need entities already in the campaign (e.g. a pod to look up), insert them before calling the parser, like the `parse_output_effect_registry_lookup_is_case_insensitive` test does.

Name tests descriptively: `parse_k8s_node_list_happy_path_fixture`, `parse_k8s_node_list_returns_known_failure_on_empty_stdout`, etc.

After writing tests, run:

```bash
cargo test -p campaign
```

Fix any compilation errors before presenting the result to the user.

## Checklist before finishing

- [ ] Read the TTP YAML ✓
- [ ] If no effects: proposed names approved, YAML updated ✓
- [ ] Domain model checkpoint passed (all facts map to existing types) ✓
- [ ] Parser function written for each effect ✓
- [ ] Registered in `resolve_output_parser` match ✓
- [ ] At least 3 tests per parser ✓
- [ ] `cargo test -p campaign` passes ✓
- [ ] Brief summary to the user: which effect IDs were added, which facts they write ✓

## Reference files

- `references/domain_types.md` — entity types, constructors, SystemInfo fields, FactsUpdate
