# Inference Engine Plan: Rust Knowledge Graph with Rule-Based Reasoning

## 1. Problem Statement

Ran needs a principled system for:
- **Fact ingestion**: New Kubernetes entities and relations arrive incrementally (from discovery scans, C2 beacons, TTP execution effects).
- **Inference**: When new facts arrive, derive all follow-up facts that logically follow (e.g., a Pod runs on a Node → the Pod can access that Node's host filesystem).
- **Invariant enforcement**: Structural constraints that must never be violated (e.g., a Pod runs on at most one Node).
- **Entity merging**: When incomplete information resolves (e.g., a placeholder node `?` is later identified as `worker-node`), the graph must be restructured without data loss.

The Go codebase solved this with two ad-hoc mechanisms:
1. **Analyzers** (`analyzers.go`): Procedural per-entity-type handlers that infer containment, identity resolution, and entity merging. Worked directly on the graph with `UpdateEntity`/`transplantEdges`.
2. **Rule Engine** (`rules.go`, `rules_builtin.go`): A loop-based forward-chaining engine with `Match`/`Build`/`Apply` phases, capped at 10 fixpoint iterations.

### Pain points in the Go version
| Issue | Impact |
|---|---|
| Boolean merge ambiguity (`false` vs "unset") | Manual overrides scattered across analyzers |
| Identity fragmentation (RBAC in separate map) | Queries couldn't see the full picture |
| Heuristic fixpoint cap (10 iterations) | Risk of incomplete inference, no proof of termination |
| No invariant enforcement | Violations discovered late, via broken UI or wrong attack paths |
| Rules can't do entity merging | Forced merge logic into analyzers, mixing concerns |
| No dependency tracking between rules | Couldn't reason about rule ordering or detect cycles |

---

## 2. Current Rust State

The Rust codebase (as of MVP-1) has:
- **Domain layer** (`crates/domain/`): `Entity` trait with `entity_id()`/`entity_name()`/`entity_kind()`, concrete types `Pod`, `Namespace`, `ServiceAccount`, `K8sCluster`, `C2Server`. Relations: `Contains`, `PodExec`.
- **Campaign** (`crates/campaign/`): `FactsUpdate` (vec of entities + relations), `Analyzer` trait with three built-in analyzers (`PodNamespaceAnalyzer`, `ServiceAccountNamespaceAnalyzer`, `NamespaceClusterAnalyzer`), and an effect parser.
- **Storage**: Flat `HashMap<EntityId, T>` per entity type + `Vec<RelationSummary>`. No graph library yet.

Missing: a graph backend, invariant enforcement, inference rules, entity merging, fixpoint computation.

The `oxidation.md` design document already specifies the target architecture:
- A `KnowledgeGraph` trait abstracting storage + queries
- `Invariant` trait for hard constraint enforcement
- `InferenceRule` trait for forward-chaining derivation
- petgraph as the initial backend, with the option to swap to a Datalog/graph DB later

---

## 3. Evaluation of Approaches

### 3.1 Ascent (Datalog-in-Rust via proc macros)

**What it is**: A Datalog language embedded in Rust via `ascent!` proc macros. Compiles Datalog rules directly into Rust code at compile time. Uses semi-naïve evaluation for fixpoint computation.

**Key features**:
- Declarative rules compiled to native Rust → zero runtime overhead
- Semi-naïve evaluation (only processes new facts each iteration) → efficient fixpoint
- Lattices for monotone aggregation (e.g., `shortest_path`, `max_access_level`)
- Stratified negation and aggregation
- Parallel execution via `ascent_par!`
- BYODS: plug custom data structures (e.g., union-find for transitive closure)
- Composable via `ascent_source!` / `include_source!`
- WASM-compatible (relevant for MVP-3 shared evaluator)
- 540 ★, active maintenance, academic backing (CC'22 + OOPSLA papers)

**Fit for Ran**:
```rust
ascent! {
    // Base relations (asserted from discovery/effects)
    relation pod(EntityId, String, String);         // id, name, namespace
    relation node(EntityId, String);                // id, name
    relation runs_on(EntityId, EntityId);            // pod_id, node_id
    relation namespace_of(EntityId, EntityId);       // entity_id, ns_id
    relation sa_has_permission(EntityId, String, String, String); // sa, verb, resource, ns
    relation pod_uses_sa(EntityId, EntityId);        // pod, sa
    relation host_pid(EntityId);                     // pod with hostPID
    relation privileged(EntityId);                   // privileged container

    // Inferred relations
    relation can_access_host_fs(EntityId, EntityId); // pod, node
    relation can_exec_on(EntityId, EntityId);        // source, target_pod
    relation can_escape_to_node(EntityId, EntityId); // pod, node

    // Inference rules
    can_access_host_fs(pod, node) <-- runs_on(pod, node), privileged(pod);
    can_access_host_fs(pod, node) <-- runs_on(pod, node), host_pid(pod);
    can_escape_to_node(pod, node) <-- can_access_host_fs(pod, node);

    can_exec_on(src, tgt) <--
        pod_uses_sa(src, sa),
        sa_has_permission(sa, "create", "pods/exec", ns),
        namespace_of(tgt, ns_id),
        node(ns_id, ns);
}
```

**Pros**:
- Rules are compile-time checked Rust - exhaustive, type-safe, no stringly-typed DSL at runtime
- Semi-naïve evaluation gives correct, guaranteed fixpoint (no arbitrary iteration cap)
- Lattices solve the "merge conflicting info" problem natively (e.g., `AccessLevel` as a lattice where `join = max`)
- Composable: split K8s-domain rules and attack-graph rules into separate `ascent_source!` modules
- WASM target enables the shared evaluator goal from MVP-3

**Cons**:
- Rules are static (compile-time). Adding user-defined rules at runtime requires a different mechanism
- No built-in concept of "entity merging" or "retraction" - Datalog is monotonic (facts can be added, not removed)
- Graph is implicit (stored as relations); ad-hoc structural queries (e.g., "all neighbors of node X") require explicit relation traversal
- Compile times increase with rule complexity (proc macro expansion)

**Verdict**: **Strong fit for the inference/rule engine layer.** Handles fixpoint computation, forward chaining, and lattice-based merging correctly and efficiently. Does NOT replace the need for an explicit graph structure for entity management, merging, and invariant enforcement.

---

### 3.2 CozoDB (Embedded Datalog database)

**What it is**: A full relational-graph database using a Datalog query language (CozoScript). Embeddable in Rust, supports in-memory, SQLite, and RocksDB backends. Provides graph algorithms (PageRank, shortest path, community detection), time travel, HNSW vector search.

**Key features**:
- Full database: stored relations, transactions, ACID
- Recursive Datalog with aggregation through safe recursion
- Built-in graph algorithms (Dijkstra, PageRank, connected components, etc.)
- Time travel (immutable history)
- Triggers on stored relations (could implement reactive inference)
- Multiple storage backends (in-memory, SQLite, RocksDB, TiKV)
- 3.9k ★, but last release Dec 2023 (v0.7.6, 2+ years ago), pre-1.0

**Fit for Ran**:
```
# CozoScript example
:create pod {id: String => name: String, namespace: String, node_name: String?}
:create runs_on {pod_id: String, node_id: String}
:create node {id: String => name: String}

# Query: which pods can escape to their node?
?[pod, node] := *runs_on{pod_id: pod, node_id: node},
                *pod{id: pod, privileged: true}
```

**Pros**:
- Full database semantics: transactions, persistence, time travel
- Graph algorithms out of the box (attack path analysis, reachability)
- Triggers could implement reactive inference
- The Datalog query language is powerful and composable
- Could serve as both graph store AND inference engine

**Cons**:
- **Stale project**: Last release 2+ years ago, pre-1.0, unclear maintenance trajectory
- **String-based query language**: Rules are CozoScript strings, not type-checked Rust. Errors at runtime, not compile time
- **Heavy dependency**: RocksDB C++ linkage, 58k SLoC. Significant build complexity
- **Impedance mismatch**: Domain entities must be serialized into CozoDB's row format and deserialized back. The domain layer can't own its Rust types naturally
- **No true incremental evaluation**: Queries re-evaluate; there's no built-in incremental maintenance of derived relations
- **Single maintainer**: 18 contributors but effectively one author (zh217)

**Verdict**: **Overkill and risky.** The graph algorithms and time travel are appealing for future use, but the stale maintenance, runtime query strings, and serialization overhead make it a poor fit as the core knowledge base. Could be considered as a *secondary* analytical backend in the future, but not as the primary graph store.

---

### 3.3 Datafrog (Lightweight Datalog engine)

**What it is**: A minimal, no-runtime Datalog engine by Frank McSherry, used by the Rust compiler's Polonius borrow checker. You manually define variables and join rules in Rust code.

**Key features**:
- Extremely minimal (1k SLoC, zero dependencies)
- Used in production by the Rust compiler
- Manual iteration loop: `while iteration.changed() { ... }`
- Semi-naïve evaluation via differential dataflow concepts
- 867 ★, maintained by rust-lang org

**Fit for Ran**:
```rust
let mut iteration = Iteration::new();
let runs_on = iteration.variable::<(EntityId, EntityId)>("runs_on");
let privileged = iteration.variable::<(EntityId,)>("privileged");
let can_escape = iteration.variable::<(EntityId, EntityId)>("can_escape");

runs_on.insert(/* ... */);
privileged.insert(/* ... */);

while iteration.changed() {
    can_escape.from_join(&runs_on, &privileged, |_pod, &node, &()| (pod, node));
}
```

**Pros**:
- Battle-tested in the Rust compiler
- Zero overhead, zero dependencies
- Semantically correct semi-naïve evaluation

**Cons**:
- **Very low-level**: No macro DSL, you manually write join operations. Rules with 3+ body atoms require manual intermediate joins
- **No negation or aggregation**: Pure monotone Datalog only
- **No lattice support**: Can't express "merge to max access level"
- **Ergonomics**: Writing complex rules is tedious and error-prone compared to declarative syntax

**Verdict**: **Too low-level.** The manual join API would make rule maintenance harder than the Go version. Ascent provides the same fixpoint guarantees with much better ergonomics.

---

### 3.4 Crepe (Datalog compiler via proc macro)

**What it is**: A Datalog proc macro similar to Ascent, with semi-naïve evaluation, stratified negation, and automatic index generation. By Eric Zhang.

**Key features**:
- Clean Datalog syntax via `crepe!` macro
- Semi-naïve evaluation
- Stratified negation
- Comparable performance to compiled Souffle
- 511 ★, recently updated (v0.2.0, Dec 2025)

**Fit**: Very similar to Ascent in concept.

**Pros**:
- Clean syntax, lightweight
- Good performance benchmarks

**Cons**:
- **No lattice support** (critical for Ran's access-level and info-merging use cases)
- **No BYODS**: Can't plug custom data structures
- **No `ascent_source!` equivalent**: Can't compose rules across modules
- **No parallel evaluation**
- **Smaller ecosystem**: Less feature-rich than Ascent across the board

**Verdict**: **Ascent is strictly superior** for Ran's use case. Crepe lacks lattices, composability, and parallelism.

---

### 3.5 Custom Trait-Based Engine (the `oxidation.md` design)

The approach already sketched in `oxidation.md`: hand-written `InferenceRule` trait + `Invariant` trait + petgraph backend.

**Pros**:
- Full control over entity merging, retraction, and graph restructuring
- No external DSL - everything is idiomatic Rust
- Invariants as hard errors
- Direct integration with the domain model (no serialization boundary)

**Cons**:
- Must implement fixpoint computation manually (risk of bugs, no termination guarantee)
- Must implement semi-naïve evaluation manually (or accept redundant work)
- Each new rule requires a new Rust struct + trait impl (verbose)
- No stratification, negation, or lattice support unless hand-coded

**Verdict**: **Necessary for the graph management layer** (entity merging, invariants, graph restructuring), but **insufficient alone for the inference layer** (lacks fixpoint correctness guarantees).

---

### 3.6 Summary Matrix

| Capability | Ascent | CozoDB | Datafrog | Crepe | Custom Traits |
|---|---|---|---|---|---|
| Compile-time type safety | ✅ | ❌ (strings) | ✅ | ✅ | ✅ |
| Semi-naïve fixpoint | ✅ | ✅ | ✅ | ✅ | ❌ (manual) |
| Lattices | ✅ | ❌ | ❌ | ❌ | ❌ (manual) |
| Stratified negation | ✅ | ✅ | ❌ | ✅ | ❌ (manual) |
| Entity merging/retraction | ❌ | ✅ (delete) | ❌ | ❌ | ✅ |
| Invariant enforcement | ❌ | ❌ | ❌ | ❌ | ✅ |
| Graph algorithms | ❌ | ✅ | ❌ | ❌ | ✅ (petgraph) |
| WASM compatible | ✅ | ✅ (in-mem) | ✅ | ✅ | ✅ |
| Composability | ✅ (source!) | ✅ (rules) | ❌ | ❌ | ✅ |
| Runtime rule addition | ❌ | ✅ | ❌ | ❌ | ✅ (rhai) |
| Maintenance health | ✅ (active) | ⚠️ (stale) | ✅ (stable) | ✅ (active) | N/A |
| Dependency weight | Light | **Heavy** | Minimal | Light | Minimal |

---

## 4. Recommended Architecture: Hybrid (Ascent + Custom Graph Layer)

Neither approach alone covers all requirements. The right answer is a **two-layer architecture** where each layer handles what it's best at:

```
┌─────────────────────────────────────────────────────┐
│                   Campaign Engine                    │
│                                                     │
│  ┌───────────────────────────────────────────────┐  │
│  │  Layer 2: Inference Engine (Ascent)           │  │
│  │  • Declarative Datalog rules                  │  │
│  │  • Semi-naïve fixpoint evaluation             │  │
│  │  • Lattice-based merging (access levels)      │  │
│  │  • Forward-chaining derivation                │  │
│  │  • Stratified negation                        │  │
│  │  Input: base facts    Output: derived facts   │  │
│  └───────────────┬───────────────────────────────┘  │
│                  │                                   │
│  ┌───────────────▼───────────────────────────────┐  │
│  │  Layer 1: Knowledge Graph (Custom + petgraph) │  │
│  │  • Entity storage (typed HashMap/petgraph)    │  │
│  │  • Relation storage (directed graph edges)    │  │
│  │  • Invariant enforcement (hard errors)        │  │
│  │  • Entity merging (transplant edges)          │  │
│  │  • Graph queries (neighbors, reachability)    │  │
│  │  • Change tracking (what was added/removed)   │  │
│  └───────────────────────────────────────────────┘  │
│                                                     │
│  Analyzers: procedural pre-processing               │
│  (identity resolution, entity promotion,             │
│   placeholder deduplication)                         │
└─────────────────────────────────────────────────────┘
```

### Data Flow

```
New facts arrive (discovery, effects, C2)
        │
        ▼
   ┌─────────┐
   │Analyzers │  Procedural pre-processing:
   │          │  • Identity resolution (is this pod the same system?)
   │          │  • Placeholder dedup (node "?" → named node)
   │          │  • Containment wiring (pod → namespace → cluster)
   └────┬─────┘
        │ produces FactsUpdate (entities + relations)
        ▼
   ┌──────────────────┐
   │ Knowledge Graph   │  Structural integration:
   │ (Layer 1)         │  • Insert entities (enforce uniqueness)
   │                   │  • Insert relations (enforce invariants)
   │                   │  • Merge entities if needed (transplant edges)
   │                   │  • Track changes (ChangeSet)
   └────┬──────────────┘
        │ emits ChangeSet (what's new)
        ▼
   ┌──────────────────┐
   │ Inference Engine  │  Logical derivation:
   │ (Layer 2, Ascent) │  • Project base facts into Ascent relations
   │                   │  • Run fixpoint to saturation
   │                   │  • Diff: new derived facts vs previously derived
   │                   │  • Return new derived facts as FactsUpdate
   └────┬──────────────┘
        │ new derived facts
        ▼
   ┌──────────────────┐
   │ Knowledge Graph   │  Insert derived facts
   │ (back to Layer 1) │  (may trigger another round if invariants
   │                   │   cause merges that change base facts)
   └──────────────────┘
```

---

## 5. Detailed Design

### 5.1 Layer 1: Knowledge Graph (`crates/graph/`)

This is the custom graph layer from `oxidation.md`, owning storage, invariants, and structural mutations.

```rust
// crates/graph/src/lib.rs

pub trait KnowledgeGraph {
    // -- Mutations --
    fn insert_entity(&mut self, entity: EntityKind) -> Result<EntityId, GraphError>;
    fn insert_relation(&mut self, relation: RelationKind) -> Result<(), GraphError>;
    fn remove_entity(&mut self, id: &EntityId) -> Result<(), GraphError>;
    fn merge_entities(&mut self, keep: &EntityId, discard: &EntityId) -> Result<MergeResult, GraphError>;

    // -- Queries --
    fn entity(&self, id: &EntityId) -> Option<&EntityKind>;
    fn outgoing(&self, id: &EntityId) -> Vec<(&RelationKind, &EntityId)>;
    fn incoming(&self, id: &EntityId) -> Vec<(&EntityId, &RelationKind)>;
    fn entities_of_kind(&self, kind: &str) -> Vec<&EntityKind>;

    // -- Bulk export for inference engine --
    fn all_base_facts(&self) -> BaseFacts;

    // -- Change tracking --
    fn apply_changeset(&mut self, changes: ChangeSet) -> Result<ChangeSet, GraphError>;
}

pub struct MergeResult {
    pub kept: EntityId,
    pub discarded: EntityId,
    pub relocated_relations: usize,
}

pub struct ChangeSet {
    pub added_entities: Vec<EntityKind>,
    pub removed_entities: Vec<EntityId>,
    pub added_relations: Vec<RelationKind>,
    pub removed_relations: Vec<(EntityId, RelationKind, EntityId)>,
}
```

**Invariants** are checked inside `insert_relation` and `merge_entities`:

```rust
pub trait Invariant: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, graph: &dyn KnowledgeGraph, change: &GraphChange) -> Result<(), Violation>;
}

// Example:
pub struct PodSingleNode;
impl Invariant for PodSingleNode {
    fn name(&self) -> &str { "pod-single-node" }
    fn check(&self, graph: &dyn KnowledgeGraph, change: &GraphChange) -> Result<(), Violation> {
        if let GraphChange::AddRelation(RelationKind::RunsOn { pod, node }) = change {
            let existing = graph.outgoing(pod)
                .iter()
                .filter(|(r, _)| matches!(r, RelationKind::RunsOn { .. }))
                .count();
            if existing > 0 {
                return Err(Violation::new(self.name(), 
                    format!("Pod {} already runs on a node", pod)));
            }
        }
        Ok(())
    }
}
```

### 5.2 Layer 2: Inference Engine (`crates/graph/src/inference.rs`)

This layer uses **Ascent** for declarative rule evaluation with guaranteed fixpoint convergence.

**Key design**: The Ascent program operates on a **projection** of the knowledge graph into flat relations. It doesn't mutate the graph directly - it outputs derived facts that are fed back into Layer 1.

```rust
// crates/graph/src/inference.rs

use ascent::ascent;

/// Flat representation of graph facts for Ascent consumption.
pub struct BaseFacts {
    pub pods: Vec<(EntityId, String, String)>,           // id, name, ns
    pub nodes: Vec<(EntityId, String)>,                  // id, name
    pub namespaces: Vec<(EntityId, String)>,              // id, name
    pub service_accounts: Vec<(EntityId, String, String)>,// id, name, ns
    pub runs_on: Vec<(EntityId, EntityId)>,               // pod, node
    pub contains: Vec<(EntityId, EntityId)>,               // parent, child
    pub pod_uses_sa: Vec<(EntityId, EntityId)>,           // pod, sa
    pub sa_permission: Vec<(EntityId, String, String, String)>, // sa, verb, resource, ns
    pub privileged_pod: Vec<EntityId>,
    pub host_pid_pod: Vec<EntityId>,
    pub host_network_pod: Vec<EntityId>,
    // ...
}

/// Derived facts produced by inference.
pub struct DerivedFacts {
    pub can_access_host_fs: Vec<(EntityId, EntityId)>,
    pub can_escape_to_node: Vec<(EntityId, EntityId)>,
    pub can_exec: Vec<(EntityId, EntityId)>,
    pub can_reach: Vec<(EntityId, EntityId)>,
    pub attack_path: Vec<(EntityId, EntityId, String)>,  // from, to, via_technique
    // ...
}

pub fn run_inference(base: &BaseFacts) -> DerivedFacts {
    // The ascent_run! macro evaluates inline and returns the program struct,
    // from which we extract the derived relations.
    let result = ascent_run! {
        // ── Base relations (loaded from Knowledge Graph) ──
        relation pod(EntityId, String, String) = base.pods.clone();
        relation node(EntityId, String) = base.nodes.clone();
        relation runs_on(EntityId, EntityId) = base.runs_on.clone();
        relation privileged(EntityId) = base.privileged_pod.iter().map(|id| (id.clone(),)).collect();
        relation host_pid(EntityId) = base.host_pid_pod.iter().map(|id| (id.clone(),)).collect();
        relation host_network(EntityId) = base.host_network_pod.iter().map(|id| (id.clone(),)).collect();
        relation sa_permission(EntityId, String, String, String) = base.sa_permission.clone();
        relation pod_uses_sa(EntityId, EntityId) = base.pod_uses_sa.clone();

        // ── Derived relations ──
        relation can_access_host_fs(EntityId, EntityId);
        relation can_escape_to_node(EntityId, EntityId);
        relation can_exec(EntityId, EntityId);

        // ── Inference rules ──

        // Privileged pods can access the host filesystem
        can_access_host_fs(p, n) <-- runs_on(p, n), privileged(p);
        can_access_host_fs(p, n) <-- runs_on(p, n), host_pid(p);

        // Host filesystem access implies node escape
        can_escape_to_node(p, n) <-- can_access_host_fs(p, n);
        can_escape_to_node(p, n) <-- runs_on(p, n), host_network(p);

        // Pod exec via RBAC
        can_exec(src, tgt) <--
            pod_uses_sa(src, sa),
            sa_permission(sa, verb, resource, ns),
            if verb == "create" || verb == "*",
            if resource == "pods/exec" || resource == "*",
            pod(tgt, _, tgt_ns),
            if tgt_ns == ns || ns == "*";

        // Transitive: if you can exec into a pod, you inherit its escape capabilities
        can_escape_to_node(src, n) <--
            can_exec(src, tgt),
            can_escape_to_node(tgt, n);
    };

    DerivedFacts {
        can_access_host_fs: result.can_access_host_fs,
        can_escape_to_node: result.can_escape_to_node,
        can_exec: result.can_exec,
        ..Default::default()
    }
}
```

### 5.3 Analyzers (unchanged role, refined scope)

Analyzers keep their current role: **procedural pre-processing** that doesn't fit declarative rules. Specifically:

1. **Identity resolution**: Determining if two entities refer to the same real-world object (e.g., a pod seen via scan vs. via token). This requires heuristic string matching and priority logic.
2. **Entity merging / placeholder resolution**: The pod-node singleton merge (node `?` → named node). Requires graph restructuring (edge transplant, entity removal).
3. **Containment wiring**: Pod → Namespace → Cluster hierarchy. *(Currently handled by analyzers, could migrate to Ascent rules over time.)*

Analyzers run **before** the inference engine. They produce a `FactsUpdate` that is applied to the Knowledge Graph (Layer 1), which then feeds into Ascent (Layer 2).

### 5.4 The Pod-Node Singleton Constraint

This constraint spans both layers:

**Layer 1 (Invariant)**: `PodSingleNode` invariant rejects any attempt to add a second `RunsOn` edge from a pod to a different node.

**Analyzer (pre-processing)**: When a pod arrives with `node_name = "worker-node"` but already has a `RunsOn` to node `?`:
1. Analyzer detects the conflict
2. Calls `graph.merge_entities(named_node_id, placeholder_node_id)`
3. `merge_entities` transplants all edges from placeholder to named node
4. The invariant is never violated because the merge happens atomically

**Layer 2 (Inference)**: After the merge, Ascent sees a clean `runs_on(pod, "worker-node")` and derives downstream facts (host access, escape paths) correctly.

### 5.5 Module/Crate Structure

```
crates/
├── domain/              # Entity/Relation types (no changes needed)
│   └── src/
│       ├── entities.rs  # Pod, Node, Namespace, ...
│       ├── relations.rs # Contains, RunsOn, PodExec, CanAccessHostFs, ...
│       └── types.rs     # EntityKind enum, RelationKind enum
│
├── graph/               # NEW: Knowledge Graph + Inference
│   ├── Cargo.toml       # depends on: ran-domain, petgraph, ascent
│   └── src/
│       ├── lib.rs        # KnowledgeGraph trait
│       ├── store.rs      # petgraph-backed implementation
│       ├── invariants.rs # Invariant trait + built-in invariants
│       ├── inference.rs  # Ascent-based rule engine
│       ├── change.rs     # ChangeSet, GraphChange types
│       └── merge.rs      # Entity merge logic
│
├── campaign/            # Campaign orchestration
│   └── src/
│       ├── lib.rs
│       ├── analyzers.rs  # Pre-processing (identity, merge, containment)
│       ├── effects.rs    # TTP effect → FactsUpdate
│       └── engine.rs     # Orchestrates: effects → analyzers → graph → inference
```

---

## 6. Implementation Phases

### Phase A: Graph Crate Foundation
- Create `crates/graph/` with `KnowledgeGraph` trait
- Implement petgraph-backed store (`PetgraphStore`)
- Add `EntityKind` / `RelationKind` enums in domain (replacing `Box<dyn Entity>`)
- Implement `ChangeSet` tracking
- Migrate `Campaign` storage from flat HashMaps to `KnowledgeGraph`

### Phase B: Invariant Enforcement
- Implement `Invariant` trait
- Add built-in invariants: `PodSingleNode`, `SecretSingleNamespace`, `NoCycles`
- Wire invariant checks into `insert_relation` / `merge_entities`

### Phase C: Entity Merging
- Implement `merge_entities` with edge transplantation
- Port the pod-node singleton merge from the Go plan into an analyzer
- Add `choosePreferredNode` logic

### Phase D: Ascent Inference Engine
- Add `ascent` dependency to `crates/graph/`
- Implement `BaseFacts` projection from `KnowledgeGraph`
- Write initial rule set:
  - `can_access_host_fs` from privileged/hostPID/hostNetwork
  - `can_escape_to_node` from host access
  - `can_exec` from RBAC permissions
  - `can_reach` transitive closure
- Wire inference into the campaign update pipeline
- Remove the Go-style fixed-iteration-cap loop

### Phase E: Advanced Rules & Lattices
- Add `AccessLevel` as an Ascent lattice for monotone access propagation
- Add attack path computation rules
- Add reachability rules considering network policies
- Split rules into composable `ascent_source!` modules:
  - `k8s_structural_rules` (containment, scheduling)
  - `k8s_security_rules` (escape, privilege escalation)
  - `attack_graph_rules` (reachability, lateral movement)

### Phase F: WASM Shared Evaluator (MVP-3)
- Extract inference rules into a separate crate with `#[cfg(target_arch = "wasm32")]` support
- Compile to WASM for frontend preview
- Backend remains authoritative

---

## 7. Key Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Inference engine | **Ascent** | Compile-time type safety, semi-naïve fixpoint, lattices, WASM support, composability |
| Graph storage | **petgraph** (behind trait) | Proven, lightweight, swappable later |
| Entity types | **Enums** (`EntityKind::Pod(...)`) | Exhaustive match, no `dyn Any` downcasting |
| Invariants | **Hard errors** | Violations indicate upstream bugs, not config issues |
| Entity merging | **Analyzer-level** (not in rules) | Requires non-monotonic graph restructuring |
| Inference trigger | **Incremental** (on change) | Only re-derive after graph mutations |
| Runtime rules | **Deferred** (post-MVP) | Can add `rhai` scripting later for user-defined rules |
| CozoDB | **Not adopted** | Stale maintenance, heavy dep, string-based queries |
| Datafrog | **Not adopted** | Too low-level, no lattices, poor ergonomics |
| Crepe | **Not adopted** | Ascent is strictly superior (lattices, BYODS, composability) |

---

## 8. Example: Full Lifecycle of the Pod-Node Fact

```
1. Discovery scan finds pod "nginx" in namespace "default", node_name = ""
   → Analyzer: creates Pod entity, Namespace entity, Contains(ns, pod)
   → No RunsOn (node unknown)

2. C2 beacon from inside "nginx" reports hostname "worker-1"
   → Effect: produces Pod update with node_name = "worker-1"
   → Analyzer: creates Node("worker-1"), RunsOn(nginx, worker-1)
   → Graph Layer 1: inserts Node, inserts RunsOn
     → PodSingleNode invariant: ✅ (no prior RunsOn)
   → Inference (Layer 2):
     → Checks: is nginx privileged? Yes.
     → Derives: can_access_host_fs(nginx, worker-1)
     → Derives: can_escape_to_node(nginx, worker-1)
   → Derived facts inserted back into graph

3. TTP "mount-host-fs" discovers mounts on unknown node "?"
   → Analyzer: Pod already has RunsOn to "worker-1"
     → choosePreferredNode("worker-1", "?") → keep "worker-1"
     → merge_entities(worker-1, ?) → transplant MountsHostPaths edge
   → No new RunsOn (merge resolved the placeholder)
   → Inference: no change (worker-1 already derived)

4. Another pod "redis" also runs on "worker-1"
   → Inference derives can_exec transitivity if RBAC allows
   → Attack path: nginx → can_exec → redis (via SA permissions)
```

---

## 9. Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Ascent compile times with many rules | Split rules into `ascent_source!` modules; benchmark after each phase |
| Ascent's monotonicity vs entity retraction | Keep retraction in Layer 1 (graph); Ascent only sees the current state, not deltas |
| Complexity of two-layer interaction | Clear contract: Layer 1 owns structure, Layer 2 owns logic. Inference never mutates the graph directly |
| Rule debugging difficulty | Use `#![measure_rule_times]` for profiling; add tracing spans around rule evaluation |
| Domain model migration to enums | Do incrementally; keep `Box<dyn Entity>` as a fallback during transition |
