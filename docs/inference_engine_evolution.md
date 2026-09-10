# Inference Engine Evolution: When to Move, What to Do First

Companion to [`inference_engine.md`](inference_engine.md) (current implementation) and [`inference_engine_plan.md`](inference_engine_plan.md) (original design exploration).

This note answers a recurring question: *should we move the hand-coded rules/analyzers to a logic engine (Datalog, rete, differential dataflow) or a richer knowledge graph?* The answer is **not yet** - but there are two concrete refactors that should happen first and would deliver most of the benefit without the paradigm shift.

---

## 1. Where we are today

| | Count | LOC | Execution model |
|---|---|---|---|
| `rules.rs` | 14 rules | ~1,070 | Fixpoint loop, 8-iter cap |
| `analyzers.rs` | 24 analyzers | ~2,690 | Single ordered pass |
| `ttp_applicability.rs` | | ~11,200 | Precondition predicates |
| `effects.rs` | | ~33,100 | Grounding + parser glue |

`cortex` (in `crates/graph/`) backs both: `StableGraph<EntityId, EdgeData>` with an `O(1)` id→node index. No relation-name index, no multi-hop join, no inference primitives.

### Shape of the logic

- **~60% pattern-match-then-emit** (graph joins: "pod with SA that has verb=create on pods/exec → emit PodExec").
- **~25% entity stamping / permission propagation** (clone RbacPermission with scope & source_role stamped).
- **~15% stub-wiring boilerplate** (ensure Namespace/Role/Pod exists when a dependent fact arrives).

### Duplication and implicit coupling

- **Rules ↔ analyzers emit the same facts in several cases.** `rolebinding.permissions` rule ≈ `RoleBindingAnalyzer`. Six namespace-wiring pairs (Pod, ServiceAccount, Role, RoleBinding, Service-family…) exist in both files.
- **"Resolve entity from campaign ∪ update" is inlined 8+ times.** Same pattern: `.find::<T>(id).cloned().or_else(|| update.new_entities.iter().find_map(...)).unwrap_or_else(|| T::stub(...))`.
- **Multi-hop joins are hand-rolled.** `KubeletExecSinkAnalyzer`, `ServiceAccountCanExecAnalyzer`, and `CanExecAccessAnalyzer` all loop over two relation sets to compute a two-hop derivation. Cortex provides `targets_of(id, rel)` (single hop) only.
- **Analyzer order is implicit.** `RoleBindingAnalyzer` must run after `RoleNamespaceAnalyzer`; this is enforced by the order of `Vec<Box<dyn Analyzer>>`, not by a declared dependency.

---

## 2. When a logic engine starts to pay off

Rough thresholds, in the order they'll be hit:

1. **Rule count ≳ 80–100.** Currently at ~40. Below ~80, hand-coded Rust with good helpers stays readable and debuggable.
2. **Order-dependency bugs start landing.** An analyzer produces wrong output because another analyzer ran before/after it, and unit tests can't easily catch the ordering assumption. Not observed yet - the rules fixpoint absorbs most of it.
3. **Provenance ("why is this derived?") becomes a product requirement.** When the UI or a user needs to trace a TTP applicability back to the facts that caused it. Datalog engines (`ascent`, `crepe`, Soufflé, differential-dataflow) give this nearly for free; hand-coded analyzers require a second bookkeeping pass to reconstruct it.
4. **Incremental recomputation dominates runtime.** When a single new fact should re-fire only the rules that touch it, not the whole pipeline. At hundreds of entities this does not matter; at millions it does.
5. **Path/reachability queries get richer than exec-channel BFS.** Cortex already has Dijkstra on exec edges. If we want "shortest path where every intermediate node satisfies predicate X", Datalog with aggregation wins.

### What does *not* fit a logic engine, at any scale

These stay imperative regardless of engine choice:

- Effect grounding and parser glue (`effects.rs`, `ttp_applicability.rs` predicates over free-form output).
- IP-based `UnknownSystem` merging (fuzzy identity reconciliation).
- Mutation-style RBAC permission stamping (scope + source_role fields).
- Stub-creation ("Namespace exists, just in case"): expressible as default rules but awkward.

A partial migration leaves two systems to reason about. Migrate all of the declarative part or none of it.

### Anti-recommendations

- **Don't adopt a rete-style rules engine** (Drools, clara). Object-mutation semantics are a worse fit than Datalog for this data model, and ~40 rules do not justify the runtime complexity.
- **Don't migrate for aesthetics.** The ugly parts of the codebase (IP merging, stubs, RBAC stamping) are exactly the parts that resist a declarative rewrite.

---

## 3. Concrete plan: two refactors that pay off now

Together these probably halve the logic LOC in `analyzers.rs`, eliminate the rules/analyzers duplication, and leave cortex as a richer primitive - so that *if* we later migrate to Datalog, the substrate is already there.

---

### 3.1 Refactor A - Collapse rules and analyzers into one mechanism

**Status: complete** *(as of branch `oxidation`, April 2026)*

**Goal.** One trait, one pipeline, one place to reason about order. Delete the rules/analyzers duplicates.

**Chosen direction.** Keep fixpoint semantics (currently in rules.rs) for everything. Analyzers become rules. The single-pass analyzer model is the accidental one - running PodNodeAnalyzer before PropagateHostIPAnalyzer is an implicit dependency that fixpoint would resolve naturally.

**Steps - what was done.**

1. ✅ **Trait unified.** `rules.rs` defines `pub trait InferenceRule { fn name(); fn infer(); }`. All analyzers in `analyzers.rs` implement this trait. There is no separate `Analyzer` trait.
2. ✅ **Pipeline merged.** `execution.rs` makes a single call to `run_rules_fixpoint(self, &rules, updates)`. There is no prior `apply_analyzers(...)` stage.
3. ✅ **Namespace-wiring duplicates deleted.** `rules.rs` contains no inline namespace-wiring rules. All logic lives in `analyzers.rs`; a `ns_contains_analyzer!` macro eliminates the repetition for the Service/Ingress/Gateway/HTTPRoute family.
4. ✅ **RoleBinding duplicate deleted.** Only `RoleBindingAnalyzer` (in `analyzers.rs`) survives; the old `rolebinding.permissions` rule in `rules.rs` is gone.
5. ⬜ **Explicit ordering not implemented.** The fixpoint loop absorbs all ordering dependencies naturally - `ServiceAccountCanExecAnalyzer` running before `RoleBindingAnalyzer` in pass 1 produces nothing; pass 2 picks up the stamped permissions. No ordering bugs have been observed. Revisit only if a rule is added whose output must never feed back into another rule (i.e. true stratification is needed).
6. ✅ **Tests migrated.** The 40+ analyzer tests assert on `InferenceRule::infer` output.

**Actual shrinkage.** `rules.rs` is now ~46 LOC (trait + fixpoint runner only). All logic is in `analyzers.rs`. The cross-file duplication is gone; the remaining boilerplate inside `analyzers.rs` is the target of Refactor B.

**Risk / rollback.** Fixpoint-for-everything risks hiding nontermination. Mitigated by the 8-iteration cap. See [`fixpoint.md`](fixpoint.md) for the full concept and termination argument.

---

### 3.2 Refactor B - Push repeated boilerplate into cortex

**Goal.** Kill the "resolve entity from campaign ∪ update" and hand-rolled two-hop-join patterns by giving cortex the primitives they rely on.

**Scope: three additions to `crates/graph/`.**

#### B.1 Relation-name index

Add a secondary index on `EdgeData.relation_name`:

```rust
pub struct KnowledgeGraph {
    graph: StableGraph<EntityId, EdgeData>,
    node_index: HashMap<EntityId, NodeIndex>,
    by_relation: HashMap<String, HashSet<EdgeIndex>>,  // new
}
```

Maintained in `insert_edge` / `remove_edges`. Unlocks:

- `edges_of_kind(&str) -> impl Iterator<Item = (EntityId, EntityId, &EdgeData)>`
- Faster `targets_of` for high-fan-out nodes (no linear scan over all outgoing edges).

Touches `crates/graph/src/graph.rs` only. No API break.

#### B.2 Two-hop join primitive

```rust
impl KnowledgeGraph {
    /// All (a, b, c) triples such that a -rel1-> b -rel2-> c.
    pub fn join(&self, rel1: &str, rel2: &str)
        -> impl Iterator<Item = (&EntityId, &EntityId, &EntityId)>;

    /// Variant: a known, walk rel1 then rel2.
    pub fn join_from(&self, src: &EntityId, rel1: &str, rel2: &str)
        -> impl Iterator<Item = (&EntityId, &EntityId)>;  // (mid, tgt)
}
```

Kills the manual nested loops in `KubeletExecSinkAnalyzer`, `ServiceAccountCanExecAnalyzer`, and `CanExecAccessAnalyzer`. Implementation is straightforward given B.1.

Scope deliberately limited to two hops. General path queries live in a future refactor (and are where Datalog genuinely starts winning).

#### B.3 `PendingView<T>` helper for campaign ∪ update lookups

Move the inlined "find in committed state, then in pending update, then fall back to stub" pattern into one place:

```rust
// new helper, in campaign/src/campaign/pending_view.rs
pub struct PendingView<'a> {
    pub campaign: &'a Campaign,
    pub update: &'a FactsUpdate,
}

impl<'a> PendingView<'a> {
    pub fn find<T: EntityType>(&self, id: &EntityId) -> Option<Cow<'a, T>>;
    pub fn contains<T: EntityType>(&self, id: &EntityId) -> bool;
    pub fn resolve_or_stub<T>(&self, id: &EntityId, stub: impl FnOnce() -> T) -> Cow<'a, T>
        where T: EntityType;
    pub fn relations_of(&self, id: &EntityId, relation_name: &str)
        -> impl Iterator<Item = &EntityId>;  // committed + pending
}
```

Campaign-layer, not cortex - it knows about both committed and pending state. Every rule gets a `PendingView` instead of `(&Campaign, &FactsUpdate)`.

#### Order of execution

1. **B.1 (relation-name index)** first - isolated, no call-site changes.
2. **B.2 (two-hop join)** - add the method, then migrate the three hand-rolled joins. Each migration is a small, reviewable diff.
3. **B.3 (`PendingView`)** - adopt incrementally. Change the `InferenceRule` trait signature after Refactor A lands, then migrate one rule per PR.

**Risk / rollback.** B.1 adds memory proportional to edge count; at current scale negligible. B.2 and B.3 are additive - call sites that don't migrate keep working. Worst case: abandon B.3 if the `Cow` plumbing proves annoying and stay on the inlined pattern.

**Expected shrinkage in rules/analyzers.** Each hand-rolled two-hop join (3 sites) drops ~20–30 LOC. Each `resolve-or-stub` site (8+) drops ~8 LOC. Combined with Refactor A, rules+analyzers land around **~2,000 LOC total** - roughly half the current size - with the declarative shape made visible instead of buried in boilerplate.

---

## 4. When to revisit the Datalog question

After Refactor A + B land. At that point:

- If the rule count is still ~40 and the code feels clean, do nothing.
- If rule count has crossed ~80, or the product roadmap adds "explain why this TTP applies", prototype an `ascent!`-based engine for a **single** rule family (e.g. RBAC permission derivation) as a side-by-side implementation. Measure LOC, correctness against a frozen campaign replay, and ergonomics. Only then decide whether to port the rest.

Refactors A and B make that later migration *easier*, because:

- One trait, one pipeline, one fixpoint model to replace.
- Relation-name index and two-hop join map directly onto Datalog atom indexing.
- `PendingView` draws a clean boundary between "state queried during derivation" and "state mutated by commit" - the exact boundary differential-dataflow expects.

None of that work is wasted if Datalog never happens.
