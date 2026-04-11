# Ran — Rust Refactoring Backlog

Tracks maintainability and scalability improvements identified in April 2026.
Issues are ordered by recommended implementation sequence (see bottom).

---

## Issue 1 — `Campaign` is a flat entity registry, not a proper store

**Files:** `crates/campaign/src/campaign/state.rs`, `entity_refs.rs`

`Campaign` has 9 separate `HashMap<EntityId, T>` fields, one per entity type.
Every new entity type (e.g. `Deployment` was recently added) requires touching
`state.rs`, `entity_refs.rs`, `insert_entity()`, `get_entities()`,
`entity_count()`, `get_system_entity()`, and `get_system_entity_mut()` in
lockstep. `insert_entity` is a 50-line if/else chain using `downcast_ref` on
`&dyn Any` (`state.rs:408–454`).

**Plan:** Introduce an `EntityStore` abstraction (a type-erased registry keyed
on `TypeId + EntityId`) or at minimum a macro that generates the per-type
boilerplate. The `CampaignEntityRef` enum in `entity_refs.rs` should be
auto-derived or replaced with a generic visitor trait so new types don't
require manual exhaustive match arms in 5+ places.

- [ ] Define `EntityStore` trait / type-map abstraction
- [ ] Replace the 9 individual `HashMap` fields with it
- [ ] Regenerate `insert_entity` / `get_entities` / `entity_count` from the store
- [ ] Eliminate hand-written arms in `get_system_entity` / `get_system_entity_mut`

---

## ~~Issue 2 — `"c2/ran"` magic string~~ ✅ Done (`e6f0f33`)

`pub const BUILTIN_C2_ID` added to `c2::types`, re-exported from `c2::lib`,
all 20+ literals replaced across `campaign` and `c2`.

---

## ~~Issue 3 — `prepare_action` / `resolve_c2_channel` do too many things~~ ✅ Done (`333d824`)

`prepare_action` decomposed into a six-stage railway-oriented pipeline.
`resolve_c2_channel` replaced with four focused routing methods.

**Pipeline stages:**
1. `validate_request` — empty field checks (free fn)
2. `resolve_ttp_and_defaults` — TTP lookup + param default filling (free fn)
3. `ground_args_from_context` — NS / NODE / TOKEN injection
4. `resolve_lateral_src` — unified `SRC`/`src` injection for Lateral Movement; merged the two old injection sites that could conflict
5. `ground_procedure_and_effects` — Tera + `${}` substitution; warns on ungrounded vars; `${CMD}` excluded (it's the hop-injection slot)
6. `route_exec_channel` — dispatches to `route_caller_supplied`, `route_lateral_movement`, `route_remote`, `route_fallback`

---

## ~~Issue 4 — `output_parsers.rs` is a monolithic 2000+ line file~~ ✅ Done (`8e75df5`)

Split into 4 domain modules under `output_parsers/`:

| Module | Parsers |
|--------|---------|
| `sys.rs` | `sys.envvar`, `sys.ip`, `sys.userid`, `sys.processes`, `sys.has-binary`, `linux.mounts` |
| `k8s.rs` | `k8s.podlist`, `k8s.nodelist`, `k8s.serviceaccountlist`, `k8s.secretlist`, `k8s.deploymentlist`, `k8s.configmaplist` |
| `iam.rs` | `rawserviceaccounttoken`, `k8s.selfsubjectrulesreview` |
| `network.rs` | `rdns` |

`resolve_output_parser` match table replaced with an `OnceLock<HashMap>` registry; each module registers its own parsers via `pub(super) fn register(m: &mut HashMap<...>)` in `mod.rs`'s `get_registry()` initialiser. New parsers can be added without touching `mod.rs`.

---

## ~~Issue 5 — `FactsUpdate::merge` is O(n²)~~ ✅ Done

`entity_aliases` changed from `Vec` to `IndexSet<(EntityId, EntityId)>` — dedup on insert is now O(1).

`merge` for `new_entities` and `new_relations` (which store `Box<dyn Trait>` values and can't be IndexSet directly) now builds an `IndexSet` of existing keys at the start of each call, replacing the inner O(n) scan with an O(1) lookup. Overall merge complexity: O(n+m) instead of O(n×m).

---

## ~~Issue 6 — `direct_foothold_pods` computed from scratch in 3 places~~ ✅ Done (`cb3aef7`)

Extracted `is_system_entity_id()` (pod-or-node check) and
`direct_foothold_systems()` (exec-edge targets whose source is a non-system
entity) as private helpers on `Campaign`. All three inline filter blocks in
`resolve_exec_channel`, `reachable_pods`, and `resolve_exec_source` replaced
with calls to the helper. Generalised from pods-only to pods **and** nodes so
a compromised `K8sNode` is now a valid direct foothold seed.
`resolve_exec_source` priorities 2 and 3 updated to use `get_system_entity()`
covering both types. Two new tests cover the node foothold paths.

---

## ~~Issue 7 — `expect()` in the production action dispatch path~~ ✅ Done (`c53684d`)

`ExecuteActionError::InvariantViolation(String)` added to `types.rs`.
The sole non-test `expect()` in `execution.rs` replaced with `ok_or_else`.
CLI handler maps the new variant to `INTERNAL_SERVER_ERROR`.
Audit confirmed no other `expect()` calls in non-test execution code.

---

## ~~Issue 8 — `CampaignEntityRef` enum has 9-arm delegation in 5 places~~ ✅ Done

**Files:** `crates/campaign/src/campaign/entity_refs.rs`, `crates/domain/entities.rs`

**`GraphEntity` (owned variants) → ambassador `#[delegate(Entity)]`:**
- Added `ambassador = "0.5"` to `ran-domain/Cargo.toml`
- Annotated `Entity` with `#[delegatable_trait]`
- Replaced `impl Entity for GraphEntity` (3 × 9-arm match + manual `as_any`)
  with `#[derive(Delegate)] #[delegate(Entity)]` on the enum
- `as_any` now correctly delegates to each inner type (previously returned `self`,
  which would have given `&GraphEntity` not the concrete type)

**`CampaignEntityRef<'a>` (reference variants) → local `delegate_entity_methods!` macro:**
- `Entity: std::any::Any` implies `'static`; `&'a T` cannot implement `Entity`,
  so ambassador cannot generate `impl Entity for CampaignEntityRef<'a>`
- Wrote a `macro_rules! delegate_entity_methods!` that generates
  `entity_id` / `entity_name` / `entity_kind` from a single variant list
- Adding a new entity variant now only touches the enum definition and the
  one-line macro invocation; `namespace` (partial match) remains explicit

`CampaignSystemEntityRef` / `CampaignSystemEntityMut` have only 2 arms each and
return `&dyn SystemEntity` trait objects (upcast, not delegation) — left as-is.

---

## Issue 9 — `AppState` / `ApiService` impl lives inside the CLI crate

**File:** `crates/cli/src/main.rs:81` (`// TODO: Temporary workaround for MVP wiring`)

`AppState` (the `ApiService` impl) is defined in `crates/cli`, which means the
wiring of k8s + campaign + c2 + armory is owned by the binary crate. This
makes the service untestable without spinning up the full CLI, and couples
bootstrap logic to the HTTP layer.

**Plan:** Extract a `crates/app` (or `crates/server`) crate that owns
`AppState` and the `ApiService` impl. `crates/cli` becomes a thin binary that
calls `app::start()`. Prerequisite for proper integration tests without the
CLI layer.

- [ ] Create `crates/app` crate
- [ ] Move `AppState` and `ApiService` impl into it
- [ ] Reduce `crates/cli/src/main.rs` to argument parsing + `app::start()`
- [ ] Add integration test in `crates/app` that exercises the full service without the CLI

---

## Recommended Sequencing

| # | Issue | Effort | Risk | Benefit |
|---|-------|--------|------|---------|
| 1 | ~~**7** — Remove `expect()` in dispatch path~~ ✅ | XS | Low | Safety |
| 2 | ~~**6** — Extract `direct_foothold_systems()`~~ ✅ | XS | Low | Clarity / DRY |
| 3 | ~~**3** — Decompose `prepare_action` pipeline~~ ✅ | M | Medium | Testability |
| 4 | ~~**5** — `FactsUpdate::merge` O(n²)~~ ✅ | S | Low | Performance |
| 5 | ~~**4** — Split `output_parsers.rs` into modules~~ ✅ | M | Medium | Scalability |
| 6 | ~~**8** — `CampaignEntityRef` delegation macro~~ ✅ | M | Medium | Extensibility |
| 7 | **1** — Entity registry abstraction | L | High | Long-term scalability |
| 8 | **9** — Extract `crates/app` | L | High | Testability / structure |

Issues 4 and 5 are independent of each other and can be done in any order.
Issue 9 is the largest structural change and is a prerequisite for proper integration testing.
