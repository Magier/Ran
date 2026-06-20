# Utility AI for Action Selection — Implementation Plan

## Goal

Given the current campaign state and the armory of TTPs (each with preconditions
and effects), produce a **ranked list of grounded actions** `(TTP × target)` by
**utility**, and support **different operator preferences** (stealthy, aggressive,
recon-heavy, …) as data-driven weight profiles.

Scope decisions (locked):

- **No GOAP.** No backward chaining from goals over preconditions/effects.
- **No MCTS / belief-tree search.** This is a POMDP (state is a *belief* — only
  discovered entities are known). We embrace that: utility AI scores the belief
  state directly, and **information-gain** considerations are first-class precisely
  because reducing uncertainty has value. No tree search.
- Approach: **utility scoring of grounded candidates**, optionally extended with
  **one-step effect lookahead** (Phase 5). Greedy selection on top.

## Current architecture (verified)

| Concern | Where |
|---|---|
| World/belief state | `crates/campaign/src/campaign/state.rs` — `Campaign { entities, graph, execution_records, … }` |
| Applicability predicates | `crates/campaign/src/ttp_applicability.rs` — `ttp_rbac_satisfied`, `ttp_exists_satisfied`, `ttp_access_level_satisfied`, `ttp_has_token_satisfied`, `ttp_related_satisfied` |
| TTP model (actions) | `crates/armory/src/model.rs` — `Ttp { requires, effects: Vec<String>, procedures, … }` |
| Effect parsing → state delta | `crates/campaign/src/effects.rs` — `parse_effect(&str, args) -> FactsUpdate { new_entities, new_relations, … }` |
| Target resolution (regex/select) | `crates/planner/src/resolver.rs` — `resolve_target`, `entity_kind` |
| Graph queries | `KnowledgeGraph` — `shortest_exec_path`, `reachable_via_exec`; `Campaign::reachable_pods`, `entity_has_relation`, `direct_foothold_systems` |
| **Current candidate filter (target-centric)** | `crates/api/src/api_handlers.rs:184-236` and `crates/api/src/mcp.rs:249-268` (duplicated subset) |

**Key gap:** the applicability filter today answers *"given THIS target, which TTPs
apply?"* There is no cross-target candidate enumeration and no scoring. Utility AI
inverts this to *"across all targets, what is the best `(TTP, target)` to do next?"*

Effects today are dispatched by two stringly-typed `match` statements in
`effects.rs` (`resolve_simple_effect_handler`, `resolve_relation_effect_handler`).
The set is **incomplete and growing** — not a deliberately closed vocabulary:
- Simple: `k8s.pod`, `k8s.serviceaccount`, `k8s.role`, `k8s.rolebinding`, `k8s.cronjob`
- Relations: `k8s.can-exec`, `k8s.can-reach`, `runs-on`, `k8s.kubelet-exec(-source)`,
  `c2.session`, `rce.can-exec`, `container.escape`

Because it will grow, the scorer must **not** carry a parallel value table keyed by
effect-name strings — that would silently drift from the parser. Instead, Phase 2
introduces **one canonical effect taxonomy** that both the parser and the scorer
consume, so adding an effect is a single edit the compiler propagates to both.

---

## Phase 0 — Consolidate applicability (refactor, no behavior change)

**Why first:** the scorer's candidate generator must run the *same* applicability
gate the API already uses, per target. Today that logic + the per-target context
resolution live inline in `api_handlers.rs` and are partially duplicated in
`mcp.rs`. Extract once, reuse three times.

**Changes**
1. In `crates/campaign/src/ttp_applicability.rs`, add:
   ```rust
   pub struct TargetContext {
       pub target_id: String,
       pub target_kind: String,
       pub is_system: bool,
       pub access_level: AccessLevel,
       pub has_token: bool,
   }

   /// Resolve the per-target facts the applicability predicates need.
   /// Mirrors api_handlers.rs:184-224 (incl. the reachable-pod ⇒ Exec inference).
   pub fn resolve_target_context(campaign: &Campaign, target_id: &str) -> Option<TargetContext>;

   /// Single aggregate gate — the AND of all five predicates + kind match.
   pub fn ttp_applicable_for_target(
       ttp: &armory::Ttp,
       campaign: &Campaign,
       tc: &TargetContext,
   ) -> bool;
   ```
   (`ttp_is_applicable_for_target_kind` currently lives in `api`; move it here too.)
2. Rewrite `api_handlers.rs` applicable-TTPs handler and `mcp.rs` to call the
   aggregate. Net: delete duplicated inline logic.

**Acceptance:** existing API/MCP behavior unchanged; existing tests pass; new unit
tests for `resolve_target_context` and `ttp_applicable_for_target`.

**Size:** S.

---

## Phase 1 — Core scoring engine (advisory, structural signals only)

New module: `crates/campaign/src/scoring/` (kept in `campaign` for now — it needs
deep state access and the crate-private graph helpers; can be promoted to its own
`crates/scoring` crate later without API change).

### Types

```rust
// scoring/context.rs
pub struct ScoringContext<'a> {
    pub campaign: &'a Campaign,
    pub ttp: &'a armory::Ttp,
    pub tc: &'a TargetContext,
}

// scoring/curve.rs — response curves map a raw measurement to [0,1]
pub enum ResponseCurve {
    Linear { slope: f32, intercept: f32 },
    Polynomial { exponent: f32, slope: f32, intercept: f32 },
    Logistic { steepness: f32, midpoint: f32 },
    Step { threshold: f32 },
}
impl ResponseCurve { pub fn apply(&self, x: f32) -> f32; } // clamps to [0,1]

// scoring/consideration.rs
pub trait Consideration: Send + Sync {
    fn name(&self) -> &'static str;
    /// Raw, curve-free measurement normalized to [0,1].
    fn measure(&self, ctx: &ScoringContext) -> f32;
}

// scoring/profile.rs — preferences are data
pub struct ConsiderationConfig { pub weight: f32, pub curve: ResponseCurve, pub enabled: bool, pub veto: bool }
pub struct Profile { pub name: String, pub configs: HashMap<String, ConsiderationConfig> }

// scoring/scorer.rs
pub struct ConsiderationScore { pub name: &'static str, pub raw: f32, pub curved: f32, pub weighted: f32 }
pub struct ScoredCandidate {
    pub ttp_id: String,
    pub target_id: String,
    pub utility: f32,
    pub breakdown: Vec<ConsiderationScore>, // explainability + UI + tuning
}
pub struct Scorer { considerations: Vec<Box<dyn Consideration>>, profile: Profile }
impl Scorer {
    /// Enumerate (applicable TTP × target) candidates and rank them.
    pub fn rank(&self, campaign: &Campaign, armory: &[armory::Ttp]) -> Vec<ScoredCandidate>;
}
```

### Candidate enumeration (`scoring/candidate.rs`)

For each entity in `campaign.get_entities()` → `resolve_target_context` → for each
TTP where `ttp_applicable_for_target` → emit a candidate. (Targets are concrete
entities, so no regex resolution needed here — that's a plan-authoring concern.)

### Combination

Default: **weighted average of curved scores**, with optional **veto** factors that
multiply (a `veto: true` consideration scoring 0 zeroes the candidate — used for
hard-ish gates like reliability=0):

```
base    = Σ(weightᵢ · curvedᵢ) / Σ weightᵢ      over non-veto, enabled considerations
vetoMul = Π curvedⱼ                              over veto considerations
utility = base · vetoMul + ttp.base_value        (base_value = small per-TTP tiebreak)
```

This makes "preferences" intuitive: a profile is just a weight vector. (Alternative
— IAUS multiplicative model with compensation factor — documented in an appendix;
the trait/curve design supports swapping the combinator.)

### Initial considerations (no lookahead, all derivable today)

| Consideration | `measure` source | Notes |
|---|---|---|
| `privilege_gain` | effect taxonomy (Phase 2) → does any effect raise access / add exec/escape edge? | dominant signal |
| `information_gain` | effect taxonomy: discovery effects (`k8s.serviceaccount`, lists, `k8s.pod`) | decays as similar entities already known (POMDP uncertainty reduction) |
| `reachability` | effects creating `c2.session` / exec edges; new foothold | |
| `reliability` | `ttp.status` (`enabled`/`stable`/`disabled`) + success rate from `campaign.execution_records` | candidate for `veto` |
| `cost` | procedure shape: local cmd vs multi-step/`steps`/payload | inverted (cheaper → higher) |
| `novelty` | `campaign.execution_records` — penalize same `(action_id, target_id)` already run | prevents loops |

Each is a small pure fn over `ScoringContext`. Ship 4-6; they're independently
unit-testable with hand-built campaigns (see `ttp_applicability.rs` test helpers).

**Acceptance:** `Scorer::rank` returns a deterministic ordering on a fixture
campaign; per-consideration unit tests; a golden test asserting a privilege-raising
TTP outranks a redundant discovery TTP on a fixture.

**Size:** M.

---

## Phase 2 — Canonical effect taxonomy (shared by parser + scorer)

The effect set is incomplete and growing, so valuation must be a **property of the
effect taxonomy itself**, not a separate string-keyed table that drifts. Introduce
one canonical `EffectKind` and route both the parser and the scorer through it.

```rust
// crates/campaign/src/effects/kind.rs  (the single source of truth)
pub enum EffectKind {
    // simple (entity-producing)
    K8sPod, K8sServiceAccount, K8sRole, K8sRoleBinding, K8sCronJob,
    // relation-producing
    K8sCanExec, K8sCanReach, RunsOn, KubeletExecSource,
    C2Session, RceCanExec, ContainerEscape,
}

impl EffectKind {
    /// Canonical name parsing (absorbs normalize_effect_name + relation-name split).
    pub fn parse(effect: &str) -> Option<Self>;

    /// Structural category of what executing this effect produces. The scorer
    /// derives value from the category (+ profile) rather than per-effect magic
    /// numbers — so a new effect that reuses a category is valued automatically.
    pub fn category(&self) -> EffectCategory;
}

pub enum EffectCategory {
    Discovery,        // adds belief about the world (entities/facts) → information
    PrivilegeEdge,    // adds exec/escape capability → privilege
    Reachability,     // adds session/route to new systems → reachability
    Persistence,      // future
    // …extend here; `category()` match is exhaustive → compiler-enforced
}
```

- **Refactor `effects.rs`** so `resolve_simple_effect_handler` /
  `resolve_relation_effect_handler` dispatch *through* `EffectKind::parse` (one name
  table, not two). The handler lookup and the category live next to each other; the
  exhaustive `match` in `category()` means you **cannot add a kind without
  classifying it** — drift is a compile error, not a silent zero.
- The scorer's `privilege_gain` / `information_gain` / `reachability` considerations
  map `ttp.effects` → `EffectKind::parse` → `category()` and aggregate. No grounding
  of `${…}` needed for this structural signal.
- Per-category weights are tunable (and can be overridden per profile in Phase 3).

**Acceptance:** every existing effect string round-trips through
`EffectKind::parse`; parser behavior unchanged (existing `effects.rs` tests pass);
a test asserts the parser dispatch table and `EffectKind` variants stay in sync
(an unparseable-but-handled, or handled-but-unclassified, effect is impossible).
An unrecognized effect string `parse`s to `None` and is surfaced (`warn!`), matching
today's fail-soft `handled: false`.

**Size:** M (touches the parser dispatch; mechanical but central).

---

## Phase 3 — Preference profiles as data

- Define profiles in YAML alongside the armory (e.g. `armory/profiles/*.yaml`):
  ```yaml
  name: stealthy-recon
  considerations:
    information_gain: { weight: 1.2, curve: { logistic: { steepness: 8, midpoint: 0.4 } } }
    privilege_gain:   { weight: 0.6 }
    noise:            { weight: 2.0 }   # see note
    reliability:      { weight: 1.0, veto: true }
    cost:             { weight: 0.8, curve: { linear: { slope: -1, intercept: 1 } } }
  ```
- `Profile::from_yaml`, a built-in `Profile::default()`, and a small registry/loader.
- Ship 3 profiles: `default`, `stealthy-recon`, `fast-aggressive`.
- **Noise/stealth consideration** needs per-TTP annotation. Add an optional
  `noise: low|medium|high` (or numeric) field to the TTP YAML + `Ttp` struct
  (`#[serde(default)]`, backward compatible). Only matters once a profile weights it.

**Acceptance:** same fixture campaign ranked under two profiles yields different top
choices; missing consideration keys fall back to profile/engine defaults.

**Size:** M.

---

## Phase 4 — API + UI surface (advisory mode)

- New handler `GET /api/recommendations?profile=<name>&limit=N` → `Vec<ScoredCandidate>`
  (reuse `ApiService::get_campaign` + armory). Optional `target_id` to scope to one
  entity (superset of today's applicable-TTPs endpoint).
- Selection policy is a separate, swappable step (kept out of `Scorer`):
  `argmax` (default) | `softmax(temperature)` | `epsilon_greedy` — so exploration
  behavior changes without touching scoring.
- Frontend: surface the ranked list with the per-consideration `breakdown`
  ("chosen because privilege_gain 0.9 × goal_progress 0.7, despite noise 0.3").
  Fits the existing operation-timeline UI. **Human still selects/executes** — zero
  autonomy risk while curves are tuned against real campaigns.

**Acceptance:** endpoint returns ranked candidates with breakdowns; UI renders top-N
with explainability; executing a recommendation goes through the existing
`execute_action` path unchanged.

**Size:** M.

---

## Phase 5 — One-step effect lookahead (optional; design now)

Upgrade `privilege_gain`/`reachability` from static taxonomy to true state-delta.

1. Factor effect application into a **pure** function (no C2, no I/O):
   `apply_facts_update(state: &mut Campaign, FactsUpdate)` — most of this exists in
   the effects-application path; extract the side-effect-free core.
2. Define a state value function `V(&Campaign) -> f32` (weighted sum: # root
   footholds, RBAC breadth, distinct reachable systems, secrets/tokens captured,
   `−` graph distance to objective).
3. New consideration `state_delta`: clone campaign → simulate `parse_effect`
   results applied → `V(state') − V(state)`. Replaces or augments the static
   privilege/reachability considerations.

Caveat: effects carry ungrounded `${…}`; for simulation, ground against the
candidate's `TargetContext` (target id/ns/token already known) — same inputs the
real executor uses. Where grounding is impossible, fall back to the Phase-2 static
value (fail-soft).

**Acceptance:** on a fixture where action A discovers nothing new but action B opens
a node, `state_delta(B) > state_delta(A)`; simulation never mutates real state
(asserted by campaign-equality before/after).

**Size:** L.

---

## Phase 6 — Goal-distance consideration (optional)

If/when campaigns declare an objective (reach cluster-admin, exfil secret X):
- Add `goal_progress`: reduction in `shortest_exec_path` distance (or graph distance
  to the objective entity) if the action's effects are applied.
- This is what lets *greedy* selection behave goal-directed without a planner.

**Size:** M (depends on objective representation, TBD).

---

## Sequencing & dependencies

```
Phase 0 (refactor) ──► Phase 1 (engine) ──► Phase 2 (taxonomy) ──► Phase 3 (profiles) ──► Phase 4 (API/UI)
                                              └──────────► Phase 5 (lookahead) ──► Phase 6 (goals)
```

Phases 0–4 deliver a usable, explainable, advisory recommender. 5–6 are principled
upgrades that reuse the same trait/curve/profile machinery — no rework.

## Cross-cutting

- **Testing:** reuse `ttp_applicability.rs` fixture style (`Campaign::bootstrap`,
  `insert_typed`). Every consideration + curve + the combinator gets unit tests;
  golden ranking tests per phase.
- **Determinism:** no `Date::now`/`rand` in scoring; softmax/epsilon live in the
  policy layer and take an injected RNG/seed.
- **Fail-soft + visible:** unknown effect kinds, missing profile keys, ungroundable
  effects degrade gracefully and `warn!` — never silently zero a candidate.
- **Performance:** `rank` is O(entities × TTPs); fine at current scale. If needed,
  pre-filter by kind before the full predicate AND.

## Open decisions

1. Module vs crate: start as `campaign::scoring`; promote to `crates/scoring` if
   reuse outside `campaign` emerges. (Recommend: module first.)
2. Combinator default: weighted-average + veto (recommended) vs IAUS multiplicative
   + compensation. Trait design supports both; pick during Phase 1.
3. Noise annotation granularity: enum vs numeric on the TTP YAML (Phase 3).
4. Objective representation for Phase 6 (campaign-level goal field) — defer.
