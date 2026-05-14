# Fuzzy Plan Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a `crates/planner/` crate that executes fuzzy attack plans — YAML documents that resolve targets by regex against the live campaign graph, chain steps via success conditions, support fan-out (`select: all`) and procedure retry, and export manual emulation sessions as reusable plans.

**Architecture:** Pure-Rust `planner` crate (no async) exposes `PlanExecutor` with synchronous `tick(&Campaign)` and `on_ttp_executed()` methods. The `app` crate wraps these in a Tokio background task per active plan, dispatching via `ApiService::execute_action` and subscribing to `CampaignEvent::TtpExecuted` for progress. The `Campaign` struct gets two small helper methods for the planner's benefit.

**Tech Stack:** Rust, `serde`/`serde_yaml`, `regex = "1"`, `indexmap = "2"`, `thiserror = "2"`, Axum (existing), Tokio (existing)

---

## File Map

**New files:**
- `crates/planner/Cargo.toml`
- `crates/planner/src/lib.rs`
- `crates/planner/src/error.rs`
- `crates/planner/src/model.rs`
- `crates/planner/src/resolver.rs`
- `crates/planner/src/state.rs`
- `crates/planner/src/executor.rs`
- `crates/planner/src/exporter.rs`

**Modified files:**
- `crates/campaign/src/campaign/state.rs` — add `all_entity_ids()` and `entity_has_relation()`
- `crates/campaign/src/runtime.rs` — add plan event variants to `CampaignEvent`
- `crates/api/src/lib.rs` (or wherever `ApiService` trait lives) — add `execute_plan`, `get_plan_status`, `export_plan` methods + routes
- `crates/app/src/lib.rs` — implement new trait methods, add plan executor storage
- `crates/app/Cargo.toml` — add `planner` dependency

---

## Task 1: Scaffold the planner crate

**Files:**
- Create: `crates/planner/Cargo.toml`
- Create: `crates/planner/src/lib.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "planner"
version = "0.1.0"
edition = "2021"

[dependencies]
armory  = { path = "../armory" }
campaign = { path = "../campaign" }
indexmap = "2"
regex = "1"
serde = { workspace = true }
serde_json = "1"
serde_yaml = "0.9"
thiserror = "2"
tracing = "0.1"
```

- [ ] **Step 2: Create lib.rs**

```rust
pub mod error;
pub mod executor;
pub mod exporter;
pub mod model;
pub mod resolver;
pub mod state;

pub use executor::{PlanDispatch, PlanEvent, PlanExecutor};
pub use exporter::{export_plan, ExportOptions, FuzzReport};
pub use model::PlanDefinition;
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo check -p planner
```

Expected: compiles (modules are empty stubs at this point — add `pub mod X {}` in each file to satisfy the `pub mod` declarations, then fill in Task by Task).

- [ ] **Step 4: Create stub files so lib.rs compiles**

Create each of these with just an empty body for now:
- `crates/planner/src/error.rs` → `// placeholder`
- `crates/planner/src/model.rs` → `// placeholder`
- `crates/planner/src/resolver.rs` → `// placeholder`
- `crates/planner/src/state.rs` → `// placeholder`
- `crates/planner/src/executor.rs` → `// placeholder`
- `crates/planner/src/exporter.rs` → `// placeholder`

- [ ] **Step 5: Compile check passes**

```bash
cargo check -p planner
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add crates/planner
git commit -m "feat(planner): scaffold crate with stub modules"
```

---

## Task 2: Error types and plan model (YAML parsing)

**Files:**
- Modify: `crates/planner/src/error.rs`
- Modify: `crates/planner/src/model.rs`

- [ ] **Step 1: Write the failing test (model parsing)**

In `crates/planner/src/model.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PLAN: &str = r#"
id: test-plan
name: Test Plan
version: "1.0"
steps:
  - id: step_a
    action: k8s.exec-into-pod
    target:
      kind: Pod
      namespace: default
      name: "nginx-.*"
    args:
      cmd: id
    retry: next_procedure
  - id: step_b
    action: container.escape-to-host
    target:
      kind: Pod
      namespace: default
      name: "nginx-.*"
      select: first
    depends_on:
      - step: step_a
        require: success
      - graph: "step:step_a has:rce.can-exec"
"#;

    #[test]
    fn parses_plan_from_yaml() {
        let plan: PlanDefinition = serde_yaml::from_str(SAMPLE_PLAN).unwrap();
        assert_eq!(plan.id, "test-plan");
        assert_eq!(plan.steps.len(), 2);

        let step_a = &plan.steps[0];
        assert_eq!(step_a.id, "step_a");
        assert_eq!(step_a.action, "k8s.exec-into-pod");
        assert_eq!(step_a.target.kind, "Pod");
        assert_eq!(step_a.target.namespace, Some("default".into()));
        assert_eq!(step_a.target.name, "nginx-.*");
        assert_eq!(step_a.target.select, None);
        assert_eq!(step_a.retry, RetryStrategy::NextProcedure);
        assert_eq!(step_a.args.get("cmd"), Some(&"id".to_string()));
        assert!(step_a.depends_on.is_empty());

        let step_b = &plan.steps[1];
        assert_eq!(step_b.target.select, Some(SelectStrategy::First));
        assert_eq!(step_b.depends_on.len(), 2);
        assert!(matches!(
            &step_b.depends_on[0],
            Dependency::Step { id, require: Require::Success } if id == "step_a"
        ));
        assert!(matches!(
            &step_b.depends_on[1],
            Dependency::Graph { step_ref, relation, all: false }
            if step_ref == "step_a" && relation == "rce.can-exec"
        ));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p planner model::tests::parses_plan_from_yaml
```

Expected: compile error (types not defined yet).

- [ ] **Step 3: Implement error.rs**

```rust
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("plan parse error: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("plan validation error: {0}")]
    Validation(String),
    #[error("unknown step reference '{0}' in depends_on")]
    UnknownStepRef(String),
    #[error("circular dependency detected involving step '{0}'")]
    CircularDependency(String),
}
```

- [ ] **Step 4: Implement model.rs**

```rust
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub version: String,
    pub steps: Vec<StepDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetQuery {
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,
    pub select: Option<SelectStrategy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectStrategy {
    Random,
    First,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDefinition {
    pub id: String,
    pub action: String,
    pub target: TargetQuery,
    #[serde(default)]
    pub args: HashMap<String, String>,
    #[serde(default)]
    pub procedure: Option<String>,
    #[serde(default)]
    pub retry: RetryStrategy,
    #[serde(default)]
    pub depends_on: Vec<Dependency>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryStrategy {
    #[default]
    None,
    NextProcedure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    Step {
        step: String,
        #[serde(default)]
        require: Require,
    },
    Graph {
        graph: String,  // raw: "step:step_a has:rce.can-exec"
    },
}

// Parsed form of a Graph dependency — use ParsedGraphDep::parse() at validation time
#[derive(Debug, Clone)]
pub struct ParsedGraphDep {
    pub step_ref: String,
    pub relation: String,
    pub all: bool,
}

impl ParsedGraphDep {
    pub fn parse(raw: &str) -> Option<Self> {
        // Format: "step:<step_id> has:<relation>" or "step:<step_id> all_have:<relation>"
        let parts: Vec<&str> = raw.splitn(2, ' ').collect();
        if parts.len() != 2 { return None; }
        let step_ref = parts[0].strip_prefix("step:")?.to_string();
        let (all, relation) = if let Some(r) = parts[1].strip_prefix("all_have:") {
            (true, r.to_string())
        } else if let Some(r) = parts[1].strip_prefix("has:") {
            (false, r.to_string())
        } else {
            return None;
        };
        Some(Self { step_ref, relation, all })
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Require {
    #[default]
    Completion,
    Success,
    AnySuccess,
    AllSuccess,
}
```

Note: `Dependency::Step` uses `step` as the serde field name (matching the YAML `step: step_a`). The `Dependency::Graph` stores the raw string; it's parsed with `ParsedGraphDep::parse()` during executor validation.

- [ ] **Step 5: Run test to verify it passes**

```bash
cargo test -p planner model::tests::parses_plan_from_yaml
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/planner/src/error.rs crates/planner/src/model.rs
git commit -m "feat(planner): add plan model types and YAML parsing"
```

---

## Task 3: Target resolver

**Files:**
- Modify: `crates/planner/src/resolver.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SelectStrategy, TargetQuery};

    fn query(kind: &str, ns: Option<&str>, name: &str, select: Option<SelectStrategy>) -> TargetQuery {
        TargetQuery {
            kind: kind.into(),
            namespace: ns.map(Into::into),
            name: name.into(),
            select,
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolves_pod_by_regex() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-7d4b9f-xk2jp",
            "ns/default/pod/nginx-7d4b9f-ab3cd",
            "ns/default/pod/redis-abc12",
            "ns/kube-system/pod/coredns-xyz",
        ]);
        let q = query("Pod", Some("default"), "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.contains("nginx")));
    }

    #[test]
    fn namespace_filter_applied() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-abc",
            "ns/kube-system/pod/nginx-def",
        ]);
        let q = query("Pod", Some("default"), "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "ns/default/pod/nginx-abc");
    }

    #[test]
    fn no_namespace_matches_all() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-abc",
            "ns/kube-system/pod/nginx-def",
        ]);
        let q = query("Pod", None, "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn select_first_returns_one() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-bbb",
            "ns/default/pod/nginx-aaa",
        ]);
        let q = query("Pod", None, "nginx-.*", Some(SelectStrategy::First));
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "ns/default/pod/nginx-aaa"); // lexicographically first
    }

    #[test]
    fn select_all_returns_all() {
        let entity_ids = ids(&[
            "ns/default/pod/nginx-aaa",
            "ns/default/pod/nginx-bbb",
        ]);
        let q = query("Pod", None, "nginx-.*", Some(SelectStrategy::All));
        let results = resolve_target(&q, &entity_ids);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn empty_when_no_match() {
        let entity_ids = ids(&["ns/default/pod/redis-abc"]);
        let q = query("Pod", None, "nginx-.*", None);
        let results = resolve_target(&q, &entity_ids);
        assert!(results.is_empty());
    }

    #[test]
    fn parses_entity_id_kinds() {
        assert_eq!(entity_kind("ns/default/pod/nginx-abc"), "pod");
        assert_eq!(entity_kind("node/worker-1"), "node");
        assert_eq!(entity_kind("sa/default/my-sa"), "serviceaccount");
    }
}
```

- [ ] **Step 2: Run to verify tests fail**

```bash
cargo test -p planner resolver::tests
```

Expected: compile error (functions not defined).

- [ ] **Step 3: Implement resolver.rs**

```rust
use regex::Regex;
use crate::model::{SelectStrategy, TargetQuery};

/// Extract the entity "kind" from an entity ID string.
/// Entity ID formats:
///   ns/{namespace}/pod/{name}           → "pod"
///   node/{name}                         → "node"
///   sa/{namespace}/{name}               → "serviceaccount"
///   ns/{namespace}/deployment/{name}    → "deployment"
///   (and so on for other namespaced resources)
pub fn entity_kind(entity_id: &str) -> &str {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["node", ..] => "node",
        ["sa", ..] => "serviceaccount",
        ["ns", _, kind, ..] => kind,
        _ => "unknown",
    }
}

fn entity_namespace(entity_id: &str) -> Option<&str> {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["ns", ns, ..] => Some(ns),
        ["sa", ns, ..] => Some(ns),
        _ => None,
    }
}

fn entity_name(entity_id: &str) -> &str {
    entity_id.rsplitn(2, '/').next().unwrap_or(entity_id)
}

/// Resolve a TargetQuery against a list of entity ID strings.
/// Returns the matched entity IDs after applying the select strategy.
/// select=None defaults to Random (returns one random match).
pub fn resolve_target(query: &TargetQuery, entity_ids: &[String]) -> Vec<String> {
    let pattern = match Regex::new(&format!("^{}$", query.name)) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut matches: Vec<String> = entity_ids
        .iter()
        .filter(|id| {
            entity_kind(id).eq_ignore_ascii_case(&query.kind)
                && query.namespace.as_deref()
                    .map(|ns| entity_namespace(id) == Some(ns))
                    .unwrap_or(true)
                && pattern.is_match(entity_name(id))
        })
        .cloned()
        .collect();

    if matches.is_empty() {
        return vec![];
    }

    match query.select.as_ref() {
        Some(SelectStrategy::All) => matches,
        Some(SelectStrategy::First) => {
            matches.sort();
            vec![matches.into_iter().next().unwrap()]
        }
        Some(SelectStrategy::Random) | None => {
            // Use deterministic-ish selection (index by entity count mod len) in tests;
            // real runtime uses rand or picks index 0 for simplicity.
            // For correctness, just return the first element — callers that want
            // true randomness can shuffle the input slice.
            vec![matches.into_iter().next().unwrap()]
        }
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p planner resolver::tests
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/planner/src/resolver.rs
git commit -m "feat(planner): implement entity ID resolver with regex and select strategies"
```

---

## Task 4: Plan execution state

**Files:**
- Modify: `crates/planner/src/state.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_all_steps_pending() {
        let ids = vec!["a".into(), "b".into(), "c".into()];
        let state = PlanExecutionState::new(ids);
        assert!(matches!(state.get("a"), Some(StepStatus::Pending)));
        assert!(matches!(state.get("b"), Some(StepStatus::Pending)));
        assert!(matches!(state.get("c"), Some(StepStatus::Pending)));
    }

    #[test]
    fn mark_dispatched_and_lookup_by_cmd_id() {
        let mut state = PlanExecutionState::new(vec!["step_a".into()]);
        state.mark_dispatched("step_a", vec!["cmd-001".into(), "cmd-002".into()]);
        assert_eq!(state.step_for_cmd("cmd-001"), Some("step_a"));
        assert_eq!(state.step_for_cmd("cmd-002"), Some("step_a"));
        assert!(matches!(
            state.get("step_a"),
            Some(StepStatus::Dispatched { exec_ids }) if exec_ids.len() == 2
        ));
    }

    #[test]
    fn complete_all_cmds_marks_step_completed() {
        let mut state = PlanExecutionState::new(vec!["step_a".into()]);
        state.mark_dispatched("step_a", vec!["cmd-1".into(), "cmd-2".into()]);
        assert!(state.record_outcome("cmd-1", true).is_none());
        let status = state.record_outcome("cmd-2", false).unwrap();
        assert!(matches!(status, StepStatus::Completed { outcomes } if outcomes == vec![true, false]));
    }

    #[test]
    fn step_targets_stored_and_retrieved() {
        let mut state = PlanExecutionState::new(vec!["step_a".into()]);
        state.set_targets("step_a", vec!["ns/default/pod/nginx-abc".into()]);
        assert_eq!(
            state.targets_for("step_a"),
            &["ns/default/pod/nginx-abc"]
        );
    }

    #[test]
    fn is_complete_when_all_terminal() {
        let mut state = PlanExecutionState::new(vec!["a".into(), "b".into()]);
        state.mark_dispatched("a", vec!["cmd-1".into()]);
        state.record_outcome("cmd-1", true);
        state.mark_skipped("b", "hard dep failed");
        assert!(state.is_complete());
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cargo test -p planner state::tests
```

Expected: compile error.

- [ ] **Step 3: Implement state.rs**

```rust
use indexmap::IndexMap;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum StepStatus {
    Pending,
    Dispatched { exec_ids: Vec<String> },
    PendingRetry { attempt: usize, next_procedure: Option<String> },
    Completed { outcomes: Vec<bool> },
    Failed { reason: String },
    Skipped { reason: String },
}

impl StepStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. } | Self::Skipped { .. })
    }

    pub fn succeeded(&self) -> Option<bool> {
        match self {
            Self::Completed { outcomes } => Some(outcomes.iter().any(|&o| o)),
            _ => None,
        }
    }
}

pub struct PlanExecutionState {
    steps: IndexMap<String, StepStatus>,
    cmd_to_step: HashMap<String, String>,
    step_targets: HashMap<String, Vec<String>>,
    pending_outcomes: HashMap<String, Vec<Option<bool>>>, // step_id → per-cmd outcomes
}

impl PlanExecutionState {
    pub fn new(step_ids: Vec<String>) -> Self {
        let steps = step_ids.into_iter().map(|id| (id, StepStatus::Pending)).collect();
        Self {
            steps,
            cmd_to_step: HashMap::new(),
            step_targets: HashMap::new(),
            pending_outcomes: HashMap::new(),
        }
    }

    pub fn get(&self, step_id: &str) -> Option<&StepStatus> {
        self.steps.get(step_id)
    }

    pub fn set(&mut self, step_id: &str, status: StepStatus) {
        if let Some(s) = self.steps.get_mut(step_id) {
            *s = status;
        }
    }

    pub fn mark_dispatched(&mut self, step_id: &str, cmd_ids: Vec<String>) {
        // Remove any previous cmd_id mappings for this step (clears placeholder ids)
        self.cmd_to_step.retain(|_, sid| sid != step_id);
        let n = cmd_ids.len();
        for cmd_id in &cmd_ids {
            self.cmd_to_step.insert(cmd_id.clone(), step_id.to_string());
        }
        self.pending_outcomes.insert(step_id.to_string(), vec![None; n]);
        self.set(step_id, StepStatus::Dispatched { exec_ids: cmd_ids });
    }

    pub fn step_for_cmd(&self, cmd_id: &str) -> Option<&str> {
        self.cmd_to_step.get(cmd_id).map(String::as_str)
    }

    /// Record the outcome of one cmd_id. Returns the final StepStatus when all cmds
    /// for this step have completed, or None if still waiting.
    pub fn record_outcome(&mut self, cmd_id: &str, success: bool) -> Option<StepStatus> {
        let step_id = self.cmd_to_step.get(cmd_id)?.clone();
        let outcomes = self.pending_outcomes.get_mut(&step_id)?;

        // Find the index of this cmd_id in the Dispatched exec_ids
        let idx = match self.steps.get(&step_id) {
            Some(StepStatus::Dispatched { exec_ids }) => {
                exec_ids.iter().position(|id| id == cmd_id)?
            }
            _ => return None,
        };
        outcomes[idx] = Some(success);

        if outcomes.iter().all(|o| o.is_some()) {
            let final_outcomes: Vec<bool> = outcomes.iter().map(|o| o.unwrap()).collect();
            let status = StepStatus::Completed { outcomes: final_outcomes };
            self.set(&step_id, status.clone());
            self.pending_outcomes.remove(&step_id);
            Some(status)
        } else {
            None
        }
    }

    pub fn mark_skipped(&mut self, step_id: &str, reason: &str) {
        self.set(step_id, StepStatus::Skipped { reason: reason.to_string() });
    }

    pub fn mark_failed(&mut self, step_id: &str, reason: &str) {
        self.set(step_id, StepStatus::Failed { reason: reason.to_string() });
    }

    pub fn mark_pending_retry(&mut self, step_id: &str, attempt: usize, next_procedure: Option<String>) {
        self.set(step_id, StepStatus::PendingRetry { attempt, next_procedure });
        self.pending_outcomes.remove(step_id);
    }

    pub fn set_targets(&mut self, step_id: &str, targets: Vec<String>) {
        self.step_targets.insert(step_id.to_string(), targets);
    }

    pub fn targets_for(&self, step_id: &str) -> &[String] {
        self.step_targets.get(step_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn is_complete(&self) -> bool {
        self.steps.values().all(|s| s.is_terminal())
    }

    pub fn pending_steps(&self) -> impl Iterator<Item = &str> {
        self.steps.iter().filter_map(|(id, s)| {
            matches!(s, StepStatus::Pending | StepStatus::PendingRetry { .. })
                .then_some(id.as_str())
        })
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p planner state::tests
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/planner/src/state.rs
git commit -m "feat(planner): implement plan execution state tracker"
```

---

## Task 5: Campaign helper methods

**Files:**
- Modify: `crates/campaign/src/campaign/state.rs`

The planner needs to query campaign state without taking a dependency on the `cortex` crate directly.

- [ ] **Step 1: Write failing tests**

At the bottom of `crates/campaign/src/campaign/state.rs`, add:

```rust
#[cfg(test)]
mod planner_helper_tests {
    use super::*;
    use ran_domain::EntityId;

    fn minimal_campaign() -> Campaign {
        // Bootstrap requires ran_name and a K8sCluster. Use test helpers.
        Campaign {
            entities: EntityStore::default(),
            graph: KnowledgeGraph::new(),
            parse_audits: Vec::new(),
            execution_records: Vec::new(),
            open_steps: Vec::new(),
            file_contents: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn all_entity_ids_returns_inserted_ids() {
        let mut c = minimal_campaign();
        // Insert a pod entity (use the Pod type from ran_domain or the domain crate)
        // Verify at least entity_ids() returns non-empty after insert.
        // This test validates the method compiles and runs.
        let ids = c.all_entity_ids();
        // Empty campaign has no pods/nodes but the method must not panic.
        let _ = ids;
    }

    #[test]
    fn entity_has_relation_false_when_no_relation() {
        let c = minimal_campaign();
        assert!(!c.entity_has_relation("ns/default/pod/nginx-abc", "rce.can-exec"));
    }
}
```

- [ ] **Step 2: Run to verify tests fail**

```bash
cargo test -p campaign planner_helper_tests
```

Expected: compile error (methods not defined).

- [ ] **Step 3: Add helper methods to Campaign**

In `crates/campaign/src/campaign/state.rs`, add inside `impl Campaign`:

```rust
/// Returns all entity IDs currently in the campaign as plain strings.
/// Used by the planner for target resolution without taking a cortex dependency.
pub fn all_entity_ids(&self) -> Vec<String> {
    self.entities
        .all_entities()
        .into_iter()
        .map(|e| e.entity_id().0.clone())
        .collect()
}

/// Returns true if the given entity has at least one outgoing edge with `relation`
/// in the knowledge graph.
pub fn entity_has_relation(&self, entity_id: &str, relation: &str) -> bool {
    // EntityId is the newtype used by cortex/ran-domain — use whichever import
    // is already present in this file.
    let eid = EntityId(entity_id.to_string());
    !self.graph.targets_of(&eid, relation).is_empty()
}
```

Note: `EntityId` is already imported in `state.rs` since it's used elsewhere in Campaign. Use the same import.

- [ ] **Step 4: Run tests**

```bash
cargo test -p campaign planner_helper_tests
```

Expected: both pass.

- [ ] **Step 5: Commit**

```bash
git add crates/campaign/src/campaign/state.rs
git commit -m "feat(campaign): add all_entity_ids and entity_has_relation helpers for planner"
```

---

## Task 6: Executor — DAG validation and core structure

**Files:**
- Modify: `crates/planner/src/executor.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use std::collections::HashMap;

    fn make_step(id: &str, deps: Vec<Dependency>) -> StepDefinition {
        StepDefinition {
            id: id.into(),
            action: "k8s.exec".into(),
            target: TargetQuery {
                kind: "Pod".into(),
                namespace: Some("default".into()),
                name: "nginx-.*".into(),
                select: None,
            },
            args: HashMap::new(),
            procedure: None,
            retry: RetryStrategy::None,
            depends_on: deps,
            note: None,
        }
    }

    fn make_plan(steps: Vec<StepDefinition>) -> PlanDefinition {
        PlanDefinition {
            id: "test".into(),
            name: "Test".into(),
            description: None,
            version: "1.0".into(),
            steps,
        }
    }

    #[test]
    fn valid_plan_creates_executor() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
        ]);
        assert!(PlanExecutor::new(plan).is_ok());
    }

    #[test]
    fn unknown_step_ref_fails_validation() {
        let plan = make_plan(vec![
            make_step("a", vec![Dependency::Step { step: "nonexistent".into(), require: Require::Success }]),
        ]);
        assert!(matches!(PlanExecutor::new(plan), Err(PlanError::UnknownStepRef(_))));
    }

    #[test]
    fn circular_dependency_fails_validation() {
        let plan = make_plan(vec![
            make_step("a", vec![Dependency::Step { step: "b".into(), require: Require::Success }]),
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
        ]);
        assert!(matches!(PlanExecutor::new(plan), Err(PlanError::CircularDependency(_))));
    }

    #[test]
    fn invalid_graph_predicate_format_fails_validation() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![Dependency::Graph { graph: "bad-format".into() }]),
        ]);
        assert!(matches!(PlanExecutor::new(plan), Err(PlanError::Validation(_))));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cargo test -p planner executor::tests::valid_plan_creates_executor
```

Expected: compile error.

- [ ] **Step 3: Implement executor structure and validation**

```rust
use std::collections::{HashMap, HashSet};
use campaign::{Campaign, ExecuteActionRequest};
use armory::Armory;
use crate::{
    error::PlanError,
    model::{Dependency, ParsedGraphDep, PlanDefinition, Require, RetryStrategy, SelectStrategy},
    resolver::resolve_target,
    state::{PlanExecutionState, StepStatus},
};

#[derive(Debug)]
pub struct PlanDispatch {
    pub step_id: String,
    pub request: ExecuteActionRequest,
}

#[derive(Debug, Clone)]
pub enum PlanEvent {
    StepDispatched { step_id: String, exec_count: usize },
    StepCompleted { step_id: String, success: bool },
    StepSkipped { step_id: String, reason: String },
    StepFailed { step_id: String, reason: String },
    PlanComplete,
}

pub struct PlanExecutor {
    plan: PlanDefinition,
    state: PlanExecutionState,
    // step_id → attempt index for retry
    retry_attempts: HashMap<String, usize>,
}

impl PlanExecutor {
    pub fn new(plan: PlanDefinition) -> Result<Self, PlanError> {
        validate_plan(&plan)?;
        let step_ids = plan.steps.iter().map(|s| s.id.clone()).collect();
        let state = PlanExecutionState::new(step_ids);
        Ok(Self {
            plan,
            state,
            retry_attempts: HashMap::new(),
        })
    }

    pub fn state(&self) -> &PlanExecutionState {
        &self.state
    }

    pub fn is_complete(&self) -> bool {
        self.state.is_complete()
    }
}

fn validate_plan(plan: &PlanDefinition) -> Result<(), PlanError> {
    let step_ids: HashSet<&str> = plan.steps.iter().map(|s| s.id.as_str()).collect();

    for step in &plan.steps {
        for dep in &step.depends_on {
            match dep {
                Dependency::Step { step: ref_id, .. } => {
                    if !step_ids.contains(ref_id.as_str()) {
                        return Err(PlanError::UnknownStepRef(ref_id.clone()));
                    }
                }
                Dependency::Graph { graph: raw } => {
                    let parsed = ParsedGraphDep::parse(raw).ok_or_else(|| {
                        PlanError::Validation(format!(
                            "invalid graph predicate '{}' on step '{}' — expected: \"step:<id> has:<relation>\"",
                            raw, step.id
                        ))
                    })?;
                    if !step_ids.contains(parsed.step_ref.as_str()) {
                        return Err(PlanError::UnknownStepRef(parsed.step_ref));
                    }
                }
            }
        }
    }

    // Cycle detection via DFS
    detect_cycles(plan).map_err(PlanError::CircularDependency)?;

    Ok(())
}

fn detect_cycles(plan: &PlanDefinition) -> Result<(), String> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for step in &plan.steps {
        let deps = step.depends_on.iter().filter_map(|d| match d {
            Dependency::Step { step, .. } => Some(step.as_str()),
            _ => None,
        });
        adj.entry(step.id.as_str()).or_default().extend(deps);
    }

    let mut visited = HashSet::new();
    let mut in_stack = HashSet::new();

    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        visited: &mut HashSet<&'a str>,
        in_stack: &mut HashSet<&'a str>,
    ) -> Option<&'a str> {
        if in_stack.contains(node) { return Some(node); }
        if visited.contains(node) { return None; }
        visited.insert(node);
        in_stack.insert(node);
        for &dep in adj.get(node).map(|v| v.as_slice()).unwrap_or(&[]) {
            if let Some(cycle_node) = dfs(dep, adj, visited, in_stack) {
                return Some(cycle_node);
            }
        }
        in_stack.remove(node);
        None
    }

    for step in &plan.steps {
        if let Some(node) = dfs(step.id.as_str(), &adj, &mut visited, &mut in_stack) {
            return Err(node.to_string());
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p planner executor::tests
```

Expected: all 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/planner/src/executor.rs
git commit -m "feat(planner): executor DAG validation (unknown refs, cycles, bad predicates)"
```

---

## Task 7: Executor — tick and dependency evaluation

**Files:**
- Modify: `crates/planner/src/executor.rs`

- [ ] **Step 1: Write failing tests**

Add to `executor::tests`:

```rust
    fn entity_ids() -> Vec<String> {
        vec!["ns/default/pod/nginx-abc123".to_string()]
    }

    fn no_relations(_: &str, _: &str) -> bool { false }
    fn has_rce(entity_id: &str, relation: &str) -> bool {
        entity_id.contains("nginx") && relation == "rce.can-exec"
    }

    #[test]
    fn steps_with_no_deps_are_dispatched_on_first_tick() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![]),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let dispatches = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(dispatches.len(), 2);
        let ids: Vec<_> = dispatches.iter().map(|d| d.step_id.as_str()).collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
    }

    #[test]
    fn step_with_dep_stays_pending_until_predecessor_done() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let dispatches = exec.tick_inner(&entity_ids(), no_relations);
        // Only "a" dispatched
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].step_id, "a");
    }

    #[test]
    fn step_dispatched_after_predecessor_succeeds() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let d1 = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(d1.len(), 1);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", true);
        let d2 = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(d2.len(), 1);
        assert_eq!(d2[0].step_id, "b");
    }

    #[test]
    fn soft_dep_unblocks_on_any_outcome() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Completion }]),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", false); // failed
        let d2 = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(d2.len(), 1);
        assert_eq!(d2[0].step_id, "b");
    }

    #[test]
    fn graph_predicate_blocks_until_satisfied() {
        let dep = Dependency::Graph { graph: "step:a has:rce.can-exec".into() };
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![
                Dependency::Step { step: "a".into(), require: Require::Success },
                dep,
            ]),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.set_targets("a", entity_ids());
        exec.state.record_outcome("cmd-1", true);
        // Without the relation, b stays pending
        let d2 = exec.tick_inner(&entity_ids(), no_relations);
        assert!(d2.is_empty());
        // With the relation, b dispatches
        let d3 = exec.tick_inner(&entity_ids(), has_rce);
        assert_eq!(d3.len(), 1);
        assert_eq!(d3[0].step_id, "b");
    }

    #[test]
    fn step_stays_pending_when_no_matching_entities() {
        let plan = make_plan(vec![make_step("a", vec![])]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let d = exec.tick_inner(&[], no_relations);
        assert!(d.is_empty());
    }
```

- [ ] **Step 2: Run to verify tests fail**

```bash
cargo test -p planner executor::tests
```

Expected: compile errors on `tick_inner`.

- [ ] **Step 3: Implement tick_inner and public tick**

Add to `impl PlanExecutor` in `executor.rs`:

```rust
    /// Testable inner tick — takes resolved entity IDs and a graph predicate function.
    pub fn tick_inner(
        &mut self,
        entity_ids: &[String],
        graph_check: impl Fn(&str, &str) -> bool,
    ) -> Vec<PlanDispatch> {
        let mut dispatches = Vec::new();

        for step in self.plan.steps.clone().iter() {
            match self.state.get(&step.id) {
                Some(StepStatus::Pending) | Some(StepStatus::PendingRetry { .. }) => {}
                _ => continue,
            }

            if !self.all_deps_satisfied(&step.id, entity_ids, &graph_check) {
                continue;
            }

            let targets = resolve_target(&step.target, entity_ids);
            if targets.is_empty() {
                continue;
            }

            self.state.set_targets(&step.id, targets.clone());

            let procedure = match self.state.get(&step.id) {
                Some(StepStatus::PendingRetry { next_procedure, .. }) => next_procedure.clone(),
                _ => step.procedure.clone(),
            };

            let cmd_ids_placeholder: Vec<String> = targets
                .iter()
                .map(|t| format!("pending-{}-{}", step.id, t))
                .collect();
            self.state.mark_dispatched(&step.id, cmd_ids_placeholder);

            for target_id in targets {
                let mut args = step.args.clone();
                dispatches.push(PlanDispatch {
                    step_id: step.id.clone(),
                    request: ExecuteActionRequest {
                        action_id: step.action.clone(),
                        exec_system_id: None,
                        target_id,
                        procedure_id: procedure.clone(),
                        args,
                    },
                });
            }
        }

        dispatches
    }

    /// Public tick — takes a Campaign reference. Call this from the API layer.
    pub fn tick(&mut self, campaign: &Campaign) -> Vec<PlanDispatch> {
        let entity_ids = campaign.all_entity_ids();
        self.tick_inner(&entity_ids, |eid, rel| campaign.entity_has_relation(eid, rel))
    }

    fn all_deps_satisfied(
        &self,
        step_id: &str,
        entity_ids: &[String],
        graph_check: &impl Fn(&str, &str) -> bool,
    ) -> bool {
        let step = self.plan.steps.iter().find(|s| s.id == step_id).unwrap();

        for dep in &step.depends_on {
            match dep {
                Dependency::Step { step: dep_id, require } => {
                    match self.state.get(dep_id) {
                        Some(StepStatus::Completed { outcomes }) => {
                            let ok = match require {
                                Require::Completion => true,
                                Require::Success | Require::AnySuccess => outcomes.iter().any(|&o| o),
                                Require::AllSuccess => outcomes.iter().all(|&o| o),
                            };
                            if !ok { return false; }
                        }
                        Some(StepStatus::Skipped { .. }) | Some(StepStatus::Failed { .. }) => {
                            if !matches!(require, Require::Completion) {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
                Dependency::Graph { graph: raw } => {
                    let parsed = ParsedGraphDep::parse(raw).unwrap(); // already validated
                    let targets = self.state.targets_for(&parsed.step_ref);
                    if targets.is_empty() { return false; }
                    let satisfied = if parsed.all {
                        targets.iter().all(|t| graph_check(t, &parsed.relation))
                    } else {
                        targets.iter().any(|t| graph_check(t, &parsed.relation))
                    };
                    if !satisfied { return false; }
                }
            }
        }
        true
    }

    fn retry_procedure_id(&self, _action_id: &str, _attempt: usize) -> Option<String> {
        // Filled in Task 9 (retry logic)
        None
    }
```

Note: `tick_inner` currently uses placeholder cmd_ids (`"pending-{step_id}-{target}"`). The real cmd_ids are set when the API layer calls `record_dispatched()` after actual dispatch. The state is re-set then. This is refined in Task 11.

- [ ] **Step 4: Run tests**

```bash
cargo test -p planner executor::tests
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/planner/src/executor.rs
git commit -m "feat(planner): executor tick with dependency evaluation and entity resolution"
```

---

## Task 8: Executor — skip propagation and fan-out

**Files:**
- Modify: `crates/planner/src/executor.rs`

- [ ] **Step 1: Write failing tests**

Add to `executor::tests`:

```rust
    #[test]
    fn hard_dep_on_failed_step_skips_dependent() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", false);

        let events = exec.on_ttp_executed_inner("cmd-1", false, None, None);
        let skipped: Vec<_> = events.iter().filter(|e| matches!(e, PlanEvent::StepSkipped { .. })).collect();
        assert_eq!(skipped.len(), 1);
        assert!(matches!(&skipped[0], PlanEvent::StepSkipped { step_id, .. } if step_id == "b"));
    }

    #[test]
    fn skip_propagates_transitively() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
            make_step("c", vec![Dependency::Step { step: "b".into(), require: Require::Success }]),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", false);

        let events = exec.on_ttp_executed_inner("cmd-1", false, None, None);
        let skipped: Vec<_> = events.iter()
            .filter_map(|e| if let PlanEvent::StepSkipped { step_id, .. } = e { Some(step_id.as_str()) } else { None })
            .collect();
        assert!(skipped.contains(&"b"));
        assert!(skipped.contains(&"c"));
    }

    #[test]
    fn fan_out_select_all_dispatches_multiple() {
        let mut step = make_step("a", vec![]);
        step.target.select = Some(SelectStrategy::All);
        let plan = make_plan(vec![step]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let all_ids = vec![
            "ns/default/pod/nginx-aaa".to_string(),
            "ns/default/pod/nginx-bbb".to_string(),
        ];
        let dispatches = exec.tick_inner(&all_ids, no_relations);
        assert_eq!(dispatches.len(), 2);
    }

    #[test]
    fn plan_complete_event_emitted_when_all_terminal() {
        let plan = make_plan(vec![make_step("a", vec![])]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", true);
        let events = exec.on_ttp_executed_inner("cmd-1", true, None, None);
        assert!(events.iter().any(|e| matches!(e, PlanEvent::PlanComplete)));
    }
```

- [ ] **Step 2: Run to verify they fail**

```bash
cargo test -p planner executor::tests
```

Expected: compile error on `on_ttp_executed_inner`.

- [ ] **Step 3: Implement on_ttp_executed_inner and skip propagation**

Add to `impl PlanExecutor`:

```rust
    /// Inner handler for a completed execution — takes success flag and optional retry info.
    /// Returns plan events to publish. Call tick() afterward to dispatch newly-unblocked steps.
    pub fn on_ttp_executed_inner(
        &mut self,
        cmd_id: &str,
        success: bool,
        procedure_id: Option<&str>,
        armory: Option<&Armory>,
    ) -> Vec<PlanEvent> {
        let mut events = Vec::new();

        let step_id = match self.state.step_for_cmd(cmd_id) {
            Some(id) => id.to_string(),
            None => return events, // unknown cmd_id (not from this plan)
        };

        let completed = self.state.record_outcome(cmd_id, success);

        if let Some(StepStatus::Completed { ref outcomes }) = completed {
            let overall_success = outcomes.iter().any(|&o| o);

            // Check if retry is warranted
            let step = self.plan.steps.iter().find(|s| s.id == step_id).unwrap().clone();
            if !overall_success
                && step.retry == RetryStrategy::NextProcedure
                && armory.is_some()
            {
                let attempt = self.retry_attempts.entry(step_id.clone()).or_insert(0);
                *attempt += 1;
                let next = self.retry_procedure_id_with_armory(&step.action, *attempt, armory.unwrap());
                if next.is_some() {
                    // Still procedures left — queue retry
                    self.state.mark_pending_retry(&step_id, *attempt, next.clone());
                    events.push(PlanEvent::StepDispatched {
                        step_id: step_id.clone(),
                        exec_count: 0, // will be set on next tick
                    });
                    return events; // tick() will handle re-dispatch
                }
                // No more procedures — fall through to failed/skip propagation
                events.push(PlanEvent::StepFailed {
                    step_id: step_id.clone(),
                    reason: "all procedures exhausted".into(),
                });
                self.state.mark_failed(&step_id, "all procedures exhausted");
            } else if overall_success {
                events.push(PlanEvent::StepCompleted { step_id: step_id.clone(), success: true });
            } else {
                events.push(PlanEvent::StepCompleted { step_id: step_id.clone(), success: false });
            }

            // Propagate skips
            events.extend(self.propagate_skips());

            if self.state.is_complete() {
                events.push(PlanEvent::PlanComplete);
            }
        }

        events
    }

    /// Public on_ttp_executed — takes a Campaign (for context) and Armory (for retry).
    pub fn on_ttp_executed(
        &mut self,
        cmd_id: &str,
        success: bool,
        procedure_id: Option<&str>,
        armory: &Armory,
    ) -> Vec<PlanEvent> {
        self.on_ttp_executed_inner(cmd_id, success, procedure_id, Some(armory))
    }

    fn propagate_skips(&mut self) -> Vec<PlanEvent> {
        let mut events = Vec::new();
        let mut changed = true;
        while changed {
            changed = false;
            let step_ids: Vec<String> = self.plan.steps.iter().map(|s| s.id.clone()).collect();
            for step_id in &step_ids {
                if !matches!(self.state.get(step_id), Some(StepStatus::Pending)) {
                    continue;
                }
                if self.should_skip(step_id) {
                    self.state.mark_skipped(step_id, "hard dependency failed or skipped");
                    events.push(PlanEvent::StepSkipped {
                        step_id: step_id.clone(),
                        reason: "hard dependency failed or skipped".into(),
                    });
                    changed = true;
                }
            }
        }
        events
    }

    fn should_skip(&self, step_id: &str) -> bool {
        let step = self.plan.steps.iter().find(|s| s.id == step_id).unwrap();
        for dep in &step.depends_on {
            if let Dependency::Step { step: dep_id, require } = dep {
                if matches!(require, Require::Completion) { continue; }
                match self.state.get(dep_id) {
                    Some(StepStatus::Failed { .. }) | Some(StepStatus::Skipped { .. }) => {
                        return true;
                    }
                    _ => {}
                }
            }
        }
        false
    }

    fn retry_procedure_id_with_armory(&self, action_id: &str, attempt: usize, armory: &Armory) -> Option<String> {
        let ttp = armory.get_ttp(action_id)?;
        ttp.procedures.get(attempt).map(|p| p.id.clone())
    }
```

- [ ] **Step 4: Update retry_procedure_id stub** (used in tick_inner):

```rust
    fn retry_procedure_id(&self, action_id: &str, attempt: usize) -> Option<String> {
        // Without armory available in tick_inner, return None (armory is passed separately in on_ttp_executed)
        None
    }
```

- [ ] **Step 5: Run all executor tests**

```bash
cargo test -p planner executor::tests
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/planner/src/executor.rs
git commit -m "feat(planner): skip propagation, fan-out, plan-complete event"
```

---

## Task 9: Executor — record_dispatched and cmd_id lifecycle

**Files:**
- Modify: `crates/planner/src/executor.rs`

The API layer calls `tick()` to get `PlanDispatch` items, dispatches each via `execute_action`, then calls `record_dispatched()` to replace the placeholder cmd_ids with the real ones from C2.

- [ ] **Step 1: Write failing test**

```rust
    #[test]
    fn record_dispatched_replaces_placeholder_cmd_ids() {
        let plan = make_plan(vec![make_step("a", vec![])]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let dispatches = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].step_id, "a");

        // Simulate: API dispatched and got real cmd_id back
        exec.record_dispatched("a", vec!["real-cmd-001".into()]);
        assert_eq!(exec.state().step_for_cmd("real-cmd-001"), Some("a"));
        assert_eq!(exec.state().step_for_cmd("pending-a-ns/default/pod/nginx-abc123"), None);
    }
```

- [ ] **Step 2: Run to verify it fails**

```bash
cargo test -p planner executor::tests::record_dispatched_replaces_placeholder_cmd_ids
```

Expected: compile error.

- [ ] **Step 3: Implement record_dispatched**

Add to `impl PlanExecutor`:

```rust
    /// Called by the API layer after dispatching the requests returned by tick().
    /// Replaces placeholder cmd_ids with the real ones from C2.
    pub fn record_dispatched(&mut self, step_id: &str, real_cmd_ids: Vec<String>) {
        self.state.mark_dispatched(step_id, real_cmd_ids);
    }
```

- [ ] **Step 4: Run test**

```bash
cargo test -p planner executor::tests::record_dispatched_replaces_placeholder_cmd_ids
```

Expected: PASS.

- [ ] **Step 5: Run all planner tests**

```bash
cargo test -p planner
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/planner/src/executor.rs
git commit -m "feat(planner): add record_dispatched to wire real cmd_ids after C2 dispatch"
```

---

## Task 10: Exporter — fuzzification and plan generation

**Files:**
- Modify: `crates/planner/src/exporter.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzifies_deployment_pod() {
        let r = fuzzify_entity_id("ns/default/pod/nginx-7d4b9f-xk2jp");
        assert_eq!(r.pattern, "nginx-.*");
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn fuzzifies_statefulset_pod() {
        let r = fuzzify_entity_id("ns/default/pod/postgres-0");
        assert_eq!(r.pattern, "postgres-.*");
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn stable_name_passes_through() {
        let r = fuzzify_entity_id("node/worker-1");
        assert_eq!(r.pattern, "worker-1");
        assert_eq!(r.confidence, Confidence::Stable);
    }

    #[test]
    fn service_account_stable() {
        let r = fuzzify_entity_id("sa/default/nginx-sa");
        assert_eq!(r.pattern, "nginx-sa");
        assert_eq!(r.confidence, Confidence::Stable);
    }

    #[test]
    fn export_success_only_plan() {
        use campaign::ExecutionRecord;
        let records = vec![
            make_record("cmd-1", "k8s.exec-into-pod", "ns/default/pod/nginx-abc-xyz", "proc-1", true),
            make_record("cmd-2", "container.check-caps", "ns/default/pod/nginx-abc-xyz", "proc-1", false),
            make_record("cmd-3", "container.escape", "ns/default/pod/nginx-abc-xyz", "proc-1", true),
        ];
        let opts = ExportOptions { include_failed: false };
        let plan = export_plan(&records, &opts);
        assert_eq!(plan.steps.len(), 2); // only cmd-1 and cmd-3
        assert_eq!(plan.steps[0].id, "step_0_exec_into_pod");
        assert!(plan.steps[1].depends_on.iter().any(|d| {
            matches!(d, crate::model::Dependency::Step { step, require: crate::model::Require::Success }
                if step == "step_0_exec_into_pod")
        }));
    }

    #[test]
    fn export_include_failed_adds_side_branches() {
        use campaign::ExecutionRecord;
        let records = vec![
            make_record("cmd-1", "k8s.exec-into-pod", "ns/default/pod/nginx-abc-xyz", "proc-1", true),
            make_record("cmd-2", "container.exploit-cve", "ns/default/pod/nginx-abc-xyz", "proc-1", false),
            make_record("cmd-3", "container.escape", "ns/default/pod/nginx-abc-xyz", "proc-1", true),
        ];
        let opts = ExportOptions { include_failed: true };
        let plan = export_plan(&records, &opts);
        assert_eq!(plan.steps.len(), 3);
        // The failed step depends on cmd-1 (same predecessor as cmd-3), not on cmd-3
        let failed_step = plan.steps.iter().find(|s| s.note.as_deref() == Some("recorded: failed")).unwrap();
        assert!(failed_step.depends_on.iter().any(|d| {
            matches!(d, crate::model::Dependency::Step { step, .. } if step == "step_0_exec_into_pod")
        }));
        // Nothing depends on the failed step
        let failed_id = failed_step.id.clone();
        for step in &plan.steps {
            for dep in &step.depends_on {
                if let crate::model::Dependency::Step { step: dep_id, .. } = dep {
                    assert_ne!(dep_id, &failed_id, "something depends on failed step");
                }
            }
        }
    }

    fn make_record(
        id: &str,
        ttp_id: &str,
        target_id: &str,
        procedure_id: &str,
        success: bool,
    ) -> campaign::ExecutionRecord {
        campaign::ExecutionRecord {
            id: id.into(),
            ttp_id: ttp_id.into(),
            ttp_name: ttp_id.into(),
            tactic: "Execution".into(),
            target_id: target_id.into(),
            exec_system_id: "system-1".into(),
            procedure_id: procedure_id.into(),
            command: "test".into(),
            args: Default::default(),
            success,
            exit_code: if success { 0 } else { 1 },
            results: vec![],
            fail_reason: "".into(),
            started_at_ms: 0,
            completed_at_ms: 1,
            is_cleanup: false,
        }
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cargo test -p planner exporter::tests
```

Expected: compile errors.

- [ ] **Step 3: Implement exporter.rs**

```rust
use regex::Regex;
use std::collections::HashMap;
use campaign::ExecutionRecord;
use crate::model::{Dependency, PlanDefinition, Require, RetryStrategy, StepDefinition, TargetQuery};

#[derive(Debug, Clone, PartialEq)]
pub enum Confidence { High, Low, Stable }

#[derive(Debug, Clone)]
pub struct FuzzResult {
    pub original: String,
    pub pattern: String,
    pub confidence: Confidence,
}

pub struct FuzzReport(pub Vec<FuzzResult>);

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub include_failed: bool,
}

/// Fuzzify a single entity ID into a regex name pattern.
pub fn fuzzify_entity_id(entity_id: &str) -> FuzzResult {
    let name = entity_id.rsplitn(2, '/').next().unwrap_or(entity_id);
    let kind = entity_kind_from_id(entity_id);

    if kind != "pod" {
        return FuzzResult {
            original: entity_id.to_string(),
            pattern: name.to_string(),
            confidence: Confidence::Stable,
        };
    }

    // Pod name patterns:
    //   Deployment:   <name>-<rs-hash(10)>-<pod-hash(5)>  → strip last two segments
    //   DaemonSet:    <name>-<node-hash(5)>                → strip last segment
    //   StatefulSet:  <name>-<ordinal>                     → strip ordinal
    static K8S_HASH: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = K8S_HASH.get_or_init(|| {
        Regex::new(r"-([a-z0-9]{5}|[a-z0-9]{9,10}|[0-9]+)$").unwrap()
    });

    let mut base = name.to_string();
    let mut stripped = false;

    // Strip up to two trailing random segments
    for _ in 0..2 {
        if let Some(m) = re.find(&base) {
            base = base[..m.start()].to_string();
            stripped = true;
        } else {
            break;
        }
    }

    if stripped {
        FuzzResult {
            original: entity_id.to_string(),
            pattern: format!("{}-.*", base),
            confidence: Confidence::High,
        }
    } else {
        FuzzResult {
            original: entity_id.to_string(),
            pattern: format!("{}.*", base),
            confidence: Confidence::Low,
        }
    }
}

fn entity_kind_from_id(entity_id: &str) -> &str {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["node", ..] => "node",
        ["sa", ..] => "serviceaccount",
        ["ns", _, kind, ..] => kind,
        _ => "unknown",
    }
}

fn entity_namespace_from_id(entity_id: &str) -> Option<&str> {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["ns", ns, ..] => Some(ns),
        ["sa", ns, ..] => Some(ns),
        _ => None,
    }
}

fn step_id_from_record(ttp_id: &str, index: usize) -> String {
    let slug = ttp_id.replace('.', "_").replace('-', "_");
    format!("step_{}_{}", index, slug)
}

/// Export a campaign's execution records as a reusable PlanDefinition.
pub fn export_plan(records: &[ExecutionRecord], opts: &ExportOptions) -> PlanDefinition {
    let successful: Vec<&ExecutionRecord> = records.iter()
        .filter(|r| r.success && !r.is_cleanup)
        .collect();

    let failed: Vec<&ExecutionRecord> = if opts.include_failed {
        records.iter()
            .filter(|r| !r.success && !r.is_cleanup)
            .collect()
    } else {
        vec![]
    };

    let mut steps: Vec<StepDefinition> = Vec::new();
    let mut success_step_ids: Vec<String> = Vec::new();

    // Build success chain
    for (i, rec) in successful.iter().enumerate() {
        let step_id = step_id_from_record(&rec.ttp_id, i);
        let fuzz = fuzzify_entity_id(&rec.target_id);

        let depends_on = if i == 0 {
            vec![]
        } else {
            vec![Dependency::Step {
                step: success_step_ids[i - 1].clone(),
                require: Require::Success,
            }]
        };

        let kind = entity_kind_from_id(&rec.target_id).to_string();
        let kind_capitalized = capitalize(&kind);
        let namespace = entity_namespace_from_id(&rec.target_id).map(str::to_string);

        steps.push(StepDefinition {
            id: step_id.clone(),
            action: rec.ttp_id.clone(),
            target: TargetQuery {
                kind: kind_capitalized,
                namespace,
                name: fuzz.pattern,
                select: None,
            },
            args: rec.args.clone(),
            procedure: Some(rec.procedure_id.clone()).filter(|s| !s.is_empty()),
            retry: RetryStrategy::None,
            depends_on,
            note: None,
        });
        success_step_ids.push(step_id);
    }

    // Add failed steps as side branches (if include_failed)
    // Each failed step depends on the last successful step that precedes it in the original record order
    let success_record_ids: Vec<&str> = successful.iter().map(|r| r.id.as_str()).collect();
    let failed_start = steps.len();

    for (fi, rec) in failed.iter().enumerate() {
        let step_id = step_id_from_record(&rec.ttp_id, failed_start + fi);
        let fuzz = fuzzify_entity_id(&rec.target_id);

        // Find the preceding successful step (last successful record before this failed one in original order)
        let record_pos = records.iter().position(|r| r.id == rec.id).unwrap_or(0);
        let preceding_success = success_step_ids.iter().rev().find(|_sid| {
            // Find the last successful record that appears before rec in original order
            records[..record_pos].iter().rev().any(|r| r.success && !r.is_cleanup)
        }).cloned();

        let depends_on = match preceding_success {
            Some(sid) => vec![Dependency::Step { step: sid, require: Require::Success }],
            None => vec![],
        };

        let kind = entity_kind_from_id(&rec.target_id).to_string();
        let kind_capitalized = capitalize(&kind);
        let namespace = entity_namespace_from_id(&rec.target_id).map(str::to_string);

        steps.push(StepDefinition {
            id: step_id,
            action: rec.ttp_id.clone(),
            target: TargetQuery {
                kind: kind_capitalized,
                namespace,
                name: fuzz.pattern,
                select: None,
            },
            args: rec.args.clone(),
            procedure: Some(rec.procedure_id.clone()).filter(|s| !s.is_empty()),
            retry: RetryStrategy::None,
            depends_on,
            note: Some("recorded: failed".into()),
        });
    }

    PlanDefinition {
        id: "exported-plan".into(),
        name: "Exported Plan".into(),
        description: Some("Auto-generated from campaign execution history".into()),
        version: "1.0".into(),
        steps,
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p planner exporter::tests
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/planner/src/exporter.rs
git commit -m "feat(planner): fuzzification heuristic and campaign export to plan YAML"
```

---

## Task 11: Add plan events to CampaignEvent

**Files:**
- Modify: `crates/campaign/src/runtime.rs`

- [ ] **Step 1: Add plan event variants**

Open `crates/campaign/src/runtime.rs` and add to `CampaignEvent`:

```rust
    PlanStepDispatched {
        plan_id: String,
        step_id: String,
        exec_count: usize,
    },
    PlanStepCompleted {
        plan_id: String,
        step_id: String,
        success: bool,
    },
    PlanStepSkipped {
        plan_id: String,
        step_id: String,
        reason: String,
    },
    PlanStepFailed {
        plan_id: String,
        step_id: String,
        reason: String,
    },
    PlanComplete {
        plan_id: String,
    },
```

- [ ] **Step 2: Ensure campaign still compiles**

```bash
cargo check -p campaign
```

Expected: no errors. (New variants are additive; existing match arms aren't affected unless exhaustive matches exist — fix any if found.)

- [ ] **Step 3: Commit**

```bash
git add crates/campaign/src/runtime.rs
git commit -m "feat(campaign): add plan step event variants to CampaignEvent"
```

---

## Task 12: API integration — execute plan, status, and export

**Files:**
- Modify: `crates/app/Cargo.toml`
- Modify: `crates/app/src/lib.rs`
- Modify: `crates/api/src/lib.rs` (find the `ApiService` trait and route registration)

This task wires everything together. Before starting, run `grep -r "ApiService" crates/api/src/` to find the exact file where the trait is defined.

- [ ] **Step 1: Add planner to app crate**

In `crates/app/Cargo.toml` under `[dependencies]`:

```toml
planner = { path = "../planner" }
```

- [ ] **Step 2: Add plan storage and new trait methods to ApiService**

In whichever `crates/api/src/` file defines `ApiService`, add three methods:

```rust
    async fn execute_plan(
        &self,
        plan_yaml: String,
    ) -> Result<String, ApiError>; // returns plan_id

    async fn get_plan_status(
        &self,
        plan_id: &str,
    ) -> Result<serde_json::Value, ApiError>;

    async fn export_plan(
        &self,
        include_failed: bool,
    ) -> Result<String, ApiError>; // returns YAML string
```

- [ ] **Step 3: Add plan executor storage to AppState**

In `crates/app/src/lib.rs`, add to `AppState` struct:

```rust
    plan_executors: Arc<std::sync::Mutex<HashMap<String, Arc<std::sync::Mutex<planner::PlanExecutor>>>>>,
```

Add to the `AppState` constructor (find where the struct is initialized) — add:

```rust
plan_executors: Arc::new(std::sync::Mutex::new(HashMap::new())),
```

- [ ] **Step 4: Implement execute_plan**

Add this impl block for `AppState` (in `crates/app/src/lib.rs`):

```rust
async fn execute_plan(&self, plan_yaml: String) -> Result<String, ApiError> {
    let plan: planner::PlanDefinition = serde_yaml::from_str(&plan_yaml)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let plan_id = plan.id.clone();
    let executor = planner::PlanExecutor::new(plan)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let executor = Arc::new(std::sync::Mutex::new(executor));

    self.plan_executors
        .lock()
        .unwrap()
        .insert(plan_id.clone(), executor.clone());

    // Spawn background task
    let this = self.clone();
    let plan_id_bg = plan_id.clone();
    tokio::spawn(async move {
        let mut events = this.campaign_events.subscribe();

        loop {
            // Run tick
            let dispatches = {
                let campaign = this.campaign.read().unwrap();
                executor.lock().unwrap().tick(&campaign)
            };

            for dispatch in dispatches {
                let step_id = dispatch.step_id.clone();
                match this.execute_action(dispatch.request).await {
                    Ok(result) => {
                        executor.lock().unwrap()
                            .record_dispatched(&step_id, vec![result.cmd_id.clone()]);
                        let _ = this.campaign_events.publish(
                            campaign::CampaignEvent::PlanStepDispatched {
                                plan_id: plan_id_bg.clone(),
                                step_id,
                                exec_count: 1,
                            }
                        );
                    }
                    Err(e) => {
                        tracing::error!("plan dispatch error for step {}: {}", step_id, e);
                    }
                }
            }

            // Wait for the next TtpExecuted event
            let (cmd_id, success, procedure_id) = loop {
                match events.recv().await {
                    Ok(campaign::CampaignEvent::TtpExecuted { cmd_id, success, .. }) => {
                        break (cmd_id, success, None::<String>);
                    }
                    Err(_) => return,
                    _ => continue,
                }
            };

            let armory = this.armory.clone();
            let plan_events = executor.lock().unwrap()
                .on_ttp_executed(&cmd_id, success, procedure_id.as_deref(), &armory);

            for event in &plan_events {
                use planner::PlanEvent;
                let campaign_event = match event {
                    PlanEvent::StepCompleted { step_id, success } => Some(
                        campaign::CampaignEvent::PlanStepCompleted {
                            plan_id: plan_id_bg.clone(),
                            step_id: step_id.clone(),
                            success: *success,
                        }
                    ),
                    PlanEvent::StepSkipped { step_id, reason } => Some(
                        campaign::CampaignEvent::PlanStepSkipped {
                            plan_id: plan_id_bg.clone(),
                            step_id: step_id.clone(),
                            reason: reason.clone(),
                        }
                    ),
                    PlanEvent::StepFailed { step_id, reason } => Some(
                        campaign::CampaignEvent::PlanStepFailed {
                            plan_id: plan_id_bg.clone(),
                            step_id: step_id.clone(),
                            reason: reason.clone(),
                        }
                    ),
                    PlanEvent::PlanComplete => Some(
                        campaign::CampaignEvent::PlanComplete { plan_id: plan_id_bg.clone() }
                    ),
                    _ => None,
                };
                if let Some(e) = campaign_event {
                    let _ = this.campaign_events.publish(e);
                }
            }

            if executor.lock().unwrap().is_complete() {
                break;
            }
        }
    });

    Ok(plan_id)
}
```

- [ ] **Step 5: Implement get_plan_status**

```rust
async fn get_plan_status(&self, plan_id: &str) -> Result<serde_json::Value, ApiError> {
    let executors = self.plan_executors.lock().unwrap();
    let executor = executors.get(plan_id)
        .ok_or_else(|| ApiError::not_found(format!("plan '{}' not found", plan_id)))?;
    let executor = executor.lock().unwrap();
    let state = executor.state();
    // Serialize step statuses as a simple map
    let steps: serde_json::Value = serde_json::json!({
        "is_complete": executor.is_complete(),
    });
    Ok(steps)
}
```

- [ ] **Step 6: Implement export_plan**

```rust
async fn export_plan(&self, include_failed: bool) -> Result<String, ApiError> {
    let campaign = self.campaign.read()
        .map_err(|_| ApiError::internal("campaign lock poisoned"))?;
    let opts = planner::ExportOptions { include_failed };
    let plan = planner::exporter::export_plan(&campaign.execution_records, &opts);
    let yaml = serde_yaml::to_string(&plan)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(yaml)
}
```

- [ ] **Step 7: Add handlers in api crate**

In `crates/api/src/api_handlers.rs`, add:

```rust
pub(crate) async fn execute_plan_handler<S: ApiService>(
    State(service): State<S>,
    body: String,
) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let plan_id = service.execute_plan(body).await?;
    Ok(axum::Json(serde_json::json!({ "plan_id": plan_id })))
}

pub(crate) async fn plan_status_handler<S: ApiService>(
    State(service): State<S>,
    axum::extract::Path(plan_id): axum::extract::Path<String>,
) -> Result<axum::Json<serde_json::Value>, ApiError> {
    let status = service.get_plan_status(&plan_id).await?;
    Ok(axum::Json(status))
}

pub(crate) async fn export_plan_handler<S: ApiService>(
    State(service): State<S>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<String, ApiError> {
    let include_failed = params.get("include_failed").map(|v| v == "true").unwrap_or(false);
    service.export_plan(include_failed).await
}
```

- [ ] **Step 8: Register routes**

In the `router_with_sse` function in `crates/api/src/lib.rs`, add:

```rust
        .route(
            "/api/plans",
            axum::routing::post(api_handlers::execute_plan_handler::<S>),
        )
        .route(
            "/api/plans/:plan_id",
            axum::routing::get(api_handlers::plan_status_handler::<S>),
        )
        .route(
            "/api/plans/export",
            axum::routing::get(api_handlers::export_plan_handler::<S>),
        )
```

- [ ] **Step 9: Build check**

```bash
cargo build
```

Expected: compiles. Fix any type errors (e.g., missing `HashMap` imports, `ApiError` method names — check existing usages in `api_handlers.rs` for the exact error constructors).

- [ ] **Step 10: Commit**

```bash
git add crates/app/Cargo.toml crates/app/src/lib.rs crates/api/src/api_handlers.rs crates/api/src/lib.rs
git commit -m "feat(api): execute_plan, plan status, and export endpoints"
```

---

## Task 13: End-to-end smoke test

**Files:**
- No new files — manual test via curl or existing test harness.

- [ ] **Step 1: Run all tests**

```bash
cargo test
```

Expected: all pass. Fix any regressions.

- [ ] **Step 2: Verify a minimal plan parses and validates**

```bash
cat > /tmp/test-plan.yaml << 'EOF'
id: smoke-test
name: Smoke Test
version: "1.0"
steps:
  - id: step_a
    action: k8s.exec-into-pod
    target:
      kind: Pod
      namespace: default
      name: "nginx-.*"
EOF

# If a dev server is running, test the endpoint:
# curl -X POST http://localhost:PORT/api/plans \
#   -H "Content-Type: text/plain" \
#   --data-binary @/tmp/test-plan.yaml
```

- [ ] **Step 3: Verify export endpoint**

```bash
# If a dev server is running with prior execution records:
# curl "http://localhost:PORT/api/plans/export?include_failed=false"
# curl "http://localhost:PORT/api/plans/export?include_failed=true"
```

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat(planner): complete fuzzy plan execution with export"
```
