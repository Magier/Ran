# Ran — Rust Refactoring Backlog

Tracks maintainability and scalability improvements identified in April 2026.
Issues are ordered by recommended implementation sequence (see bottom).

---

## ~~Issue 1 — `Campaign` is a flat entity registry, not a proper store~~ ✅ Done

**Files:** `crates/campaign/src/campaign/entity_store.rs` (new), `state.rs`, and every caller.

Replaced 9 individual `HashMap<EntityId, T>` fields on `Campaign` with a single
`pub entities: EntityStore` field.

**`EntityStore` design:**
- `HashMap<TypeId, Box<dyn ErasedSlot>>` where each `Slot<T>` stores `HashMap<EntityId, T>`
  plus a HRTB fn pointer `for<'a> fn(&'a T) -> CampaignEntityRef<'a>` for type-erased iteration
- `EntityType` blanket-impl supertrait collects `Entity + Merge + Clone + Serialize + DeserializeOwned + Debug + Send + Sync + 'static`
- `Default` impl is the single registration point — adding a new entity type requires one `s.register::<NewType>(...)` call and one variant in `CampaignEntityRef`; nothing else changes
- Custom `Serialize`/`Deserialize` preserves the old flat JSON wire format (`"pods"`, `"c2_servers"`, …) — existing serialised campaign state remains compatible
- `Clone` forwarded via `clone_box()` on `ErasedSlot` so `Campaign: Clone` still holds

**Public API:** `get::<T>()`, `get_mut::<T>()`, `insert_typed::<T>()`, `insert_entity(&dyn Entity)`, `find()`, `find_mut()`, `contains()`, `values()`, `entity_count()`, `all_entities()`

**Callers updated:** `state.rs`, `execution.rs`, `tests.rs`, `grounding.rs`, `analyzers.rs`, `rules.rs`, `output_parsers/mod.rs`, `ttp_applicability.rs`, `api/src/mcp.rs` — ~120 call sites migrated.

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

## Issue 10 — Missing output parsers (file content, kubeconfig, nmap, individual k8s entity effects)

**Files:** `crates/campaign/src/output_parsers/`, `crates/campaign/src/effects.rs`

Several parsers from the Go implementation have no Rust equivalent:

**File content / kubeconfig** (`src/campaign/parsers.go: file:content, file:kubeconfig`):
- `file:content` — caches arbitrary file content on the entity; auto-detects kubeconfig YAML and forwards to `file:kubeconfig`
- `file:kubeconfig` — parses kubeconfig YAML, extracts cluster endpoint + CA, user credentials (token or cert); creates a `K8sCredential` entity and wires it to the cluster

**System** (`src/campaign/parsers.go: sys.files, sys.hasfile`):
- `sys.files` — populates `system.files` from a line-delimited file list, marks executables as binaries
- `sys.hasfile(PATH)` — parametric effect (same pattern as `sys.has-binary`): marks path present/absent in `system.files`

**Network** (`src/campaign/parsers.go: nmap, k8s.can-reach`):
- `nmap` — parses nmap XML or greppable output; creates `Pod` placeholder entities from open-port IPs and links them with `CanReach` relations
- `k8s.can-reach(src, tgt)` — explicit reachability effect: creates a `CanReach` relation between two entity IDs

**Individual k8s entity creation effects** (`src/campaign/parsers.go`):
- `k8s.serviceaccount` — creates a single `ServiceAccount` entity from inline YAML/JSON
- `k8s.role` / `k8s.rolebinding` — creates `K8sRole` / `K8sRoleBinding` entities; `rolebinding` also injects parsed RBAC permissions into the referenced SA's entitlements
- `k8s.cronjob` — creates a `CronJob` entity with schedule and namespace

- [ ] Add `file:content` / `file:kubeconfig` parsers (new `file.rs` module under `output_parsers/`)
- [ ] Add `sys.files` and parametric `sys.hasfile(...)` to `output_parsers/sys.rs`
- [ ] Add `nmap` parser to `output_parsers/network.rs`
- [ ] Add `k8s.can-reach(src, tgt)` effect to `effects.rs`
- [ ] Add `k8s.serviceaccount`, `k8s.role`, `k8s.rolebinding`, `k8s.cronjob` effects to `effects.rs`

---

## Issue 11 — Missing GCP support

**Go source:** `src/campaign/gcp/gcp_parser.go`, `src/domain/gcp_entities.go`

No GCP entity types or parsers exist in the Rust codebase.

**Domain types needed:**
- `GCPServiceAccount` — GCP SA with email, project, roles, bound K8s SA reference
- `GCPBucket` — bucket name, IAM policy entries

**Parsers needed:**
- `gcp.serviceaccount` — parses `gcloud iam service-accounts describe` JSON output
- `gcp.buckets` — parses `gsutil ls -L` or JSON bucket listing

**Analyzer needed:**
- `GCPServiceAccountAnalyzer` — when a pod's env contains `GOOGLE_APPLICATION_CREDENTIALS` or a known GCP SA email, wire a `Uses` relation to the GCP SA entity

- [ ] Add `GCPServiceAccount` and `GCPBucket` to `crates/domain/entities.rs`
- [ ] Add `gcp.rs` module under `output_parsers/` with `gcp.serviceaccount` and `gcp.buckets`
- [ ] Add `GCPServiceAccountAnalyzer` to `analyzers.rs`

---

## Issue 12a — `CanExecAccessAnalyzer`

**Go source:** `src/campaign/rules_builtin.go`  
**File:** `crates/campaign/src/analyzers.rs`  
**Dependencies:** none

When a system entity receives an incoming `PodExec`, `RceCanExec`, or `KubeletExecSink` relation, set its `system.access_level` to `UserExec` — unless it is already `Exec` (root), which must never be downgraded. This ensures access level propagates through lateral movement paths discovered after initial compromise, not only from `sys.userid` output.

Trigger: new relations whose name is `can-exec`, `rce-can-exec`, or `kubelet-pod-exec`. The target entity of each relation is the system whose access level is updated.

**Tests to write:**
- Pod with `AccessLevel::Unknown` + incoming `PodExec` → access level becomes `UserExec`
- Pod with `AccessLevel::Exec` (root) + incoming `PodExec` → access level unchanged (no downgrade)
- Pod with `AccessLevel::UserExec` + second incoming `PodExec` → no change (idempotent)
- `KubeletExecSink` relation → target pod gets `UserExec`
- `RceCanExec` relation → target gets `UserExec`
- Non-system entity as target (e.g. `Namespace`) → no update emitted

- [ ] Add `CanExecAccessAnalyzer` to `analyzers.rs`
- [ ] Add to `default_analyzers()`
- [ ] Write tests covering the six cases above

---

## Issue 12b — `PropagateHostIPAnalyzer`

**Go source:** `src/campaign/rules_builtin.go`  
**File:** `crates/campaign/src/analyzers.rs`  
**Dependencies:** none

When a `Pod` has a non-empty `host_ip` field and a `runs-on` relation to a `K8sNode`, copy the `host_ip` into `node.system.ips` if not already present. Node-targeted TTPs (kubelet API calls) need the node's real IP; this is the only way to populate it when no `k8s.nodelist` has been run.

Trigger: new `Pod` entities with `host_ip` set, or new `RunsOn` relations where the source pod has `host_ip` set.

**Tests to write:**
- Pod with `host_ip` + existing `RunsOn` to node → node gains that IP in `system.ips`
- Pod with `host_ip` already present in node's IPs → no duplicate added, facts written = 0
- Pod with no `host_ip` + `RunsOn` → no update emitted
- `RunsOn` relation added to a pod that already has `host_ip` (relation arrives after entity) → node still gets the IP

- [ ] Add `host_ip` field to `Pod` in `crates/domain/entities.rs` (populated by `k8s.podlist` parser)
- [ ] Add `PropagateHostIPAnalyzer` to `analyzers.rs`
- [ ] Add to `default_analyzers()`
- [ ] Write tests covering the four cases above

---

## Issue 12c — `WorkloadOwnershipAnalyzer`

**Go source:** `src/campaign/analyzers.go: analyzeWorkloadOwnership`  
**File:** `crates/campaign/src/analyzers.rs`  
**Dependencies:** Issue 1 (new entity types: `ReplicaSet`, `StatefulSet`, `DaemonSet`, `Job` are simpler to add after entity registry abstraction)

When a `Pod` carries owner references (populated by the `k8s.podlist` parser from `metadata.ownerReferences`), walk the ownership chain and emit `Owns` relations up to the workload root:

```
Pod → ReplicaSet → Deployment
Pod → StatefulSet
Pod → DaemonSet
Pod → Job → CronJob
```

Create each intermediate entity if not already known. This makes workload-level entities visible in the graph so TTPs can target a `Deployment` rather than individual pods.

Trigger: new `Pod` entities with non-empty `owner_references`.

**Tests to write:**
- Pod owned by `ReplicaSet` → `ReplicaSet` entity created + `Owns(ReplicaSet→Pod)`
- Pod owned by `StatefulSet` → `StatefulSet` entity + `Owns`
- Pod owned by `DaemonSet` → `DaemonSet` entity + `Owns`
- Pod owned by `Job` → `Job` entity + `Owns`
- Already-known `ReplicaSet` as owner → no duplicate entity emitted, `Owns` still emitted
- Pod with no owner references → no output

- [ ] Add `owner_references` field to `Pod` (populated from `k8s.podlist` JSON)
- [ ] Add `ReplicaSet`, `StatefulSet`, `DaemonSet`, `Job` entity types to `crates/domain/entities.rs`
- [ ] Add `Owns` relation type to `crates/domain/relations.rs`
- [ ] Add `WorkloadOwnershipAnalyzer` to `analyzers.rs`
- [ ] Add to `default_analyzers()`
- [ ] Write tests covering the six cases above

---

## Issue 12d — `RoleBindingAnalyzer`

**Go source:** `src/campaign/analyzers.go: analyzeRoleBinding`  
**File:** `crates/campaign/src/analyzers.rs`  
**Dependencies:** Issue 10 (`k8s.rolebinding` effect must exist to produce `K8sRoleBinding` entities)

When a `K8sRoleBinding` entity arrives, resolve its subjects and inject the referenced role's permissions into each subject `ServiceAccount`'s `entitlements`. This is what converts raw RBAC YAML into actionable `ServiceAccount.Can()` facts — without it, RBAC-gated TTPs never unlock from binding data alone.

`ClusterRoleBinding` subjects receive permissions with a wildcard namespace scope (`*`). Namespace-scoped `RoleBinding` subjects receive permissions scoped to the binding's namespace.

Trigger: new `K8sRoleBinding` entities.

**Tests to write:**
- `RoleBinding` references a known `ServiceAccount` → SA's `entitlements` extended with role's permissions
- `RoleBinding` references an unknown SA → SA entity created with entitlements set
- `ClusterRoleBinding` → permissions have `scope = *` (cluster-wide)
- `RoleBinding` in namespace `"default"` → permissions have `scope = "default"`
- Multiple subjects in one binding → each SA receives the permissions
- `RoleBinding` with no matching role permissions → no entitlements emitted (not a crash)

- [ ] Add `K8sRoleBinding` entity type to `crates/domain/entities.rs` (if not added by Issue 10)
- [ ] Add `RoleBindingAnalyzer` to `analyzers.rs`
- [ ] Add to `default_analyzers()`
- [ ] Write tests covering the six cases above

---

## Issue 13 — MITRE domain types and AttackFlow export

**Go source:** `src/mitre/`, `src/campaign/audit_trail.go`

No MITRE types or attack flow serialization exist in the Rust codebase. The `ExecutionRecord` struct already captures all the raw data needed; what is missing is the conversion layer.

**Domain types needed (`crates/domain/` or new `crates/mitre/`):**

- `Tactic` enum — 14 ATT&CK tactics (Reconnaissance through Impact)
- `DefendTactic` enum — 7 D3FEND tactics (Model through Restore)
- STIX2 bundle types: `StixBundle`, `AttackFlow`, `AttackAction`, `AttackAsset`, `Relationship`, `Indicator`
- Technique/tactic ID mapping tables (STIX IDs for each tactic and technique name)

**Conversion function:**
- `execution_records_to_attack_flow(records: &[ExecutionRecord]) -> StixBundle`  
  Maps each `ExecutionRecord` to an `AttackAction` STIX object; links them in sequence; wraps in a signed `StixBundle`.

**API endpoint:**
- `GET /api/attack-flow` — returns the current campaign's execution history as a STIX2 AttackFlow bundle (JSON)

- [ ] Add `Tactic` and `DefendTactic` enums to `crates/domain/`
- [ ] Add STIX2 / AttackFlow types (new `crates/mitre/` or `crates/domain/mitre.rs`)
- [ ] Implement `execution_records_to_attack_flow()` converter
- [ ] Add `GET /api/attack-flow` endpoint

---

## Issue 14 — Execution records API endpoint (blocker for self-improving loop)

**File:** `crates/api/src/lib.rs`, `crates/api/src/api_handlers.rs`

`ExecutionRecord` objects (full stdout, args, parse audits, timing) are stored in `Campaign` but are not accessible via the HTTP API. The self-improving loop's Gap 2 scanner needs to inspect raw stdout of past executions to detect undeclared output. The SSE stream delivers `ParseAudit` in real time but not the full results.

**Needed:**
- `GET /api/execution-records` — returns `Vec<ExecutionRecord>` for the current campaign session (full stdout in `results` field)
- Optional: `GET /api/execution-records/:id` — single record by command ID

- [ ] Add `get_execution_records()` method to `ApiService` trait
- [ ] Implement it on `AppState` (reads from `campaign.execution_records`)
- [ ] Wire `GET /api/execution-records` route
- [ ] Add `GET /api/execution-records/:id` route for targeted lookups

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
| 7 | **1** — Entity registry abstraction 🔄 | L | High | Required before adding new entity types (Issues 10–12) |
| 8 | **14** — Execution records API endpoint | XS | Low | Self-improving loop unblocked |
| 9 | **10** — Missing output parsers | M | Low | Parser coverage |
| 10 | **12a** — `CanExecAccessAnalyzer` | XS | Low | Access level propagation via lateral movement |
| 11 | **12b** — `PropagateHostIPAnalyzer` | XS | Low | Node IP visibility for kubelet TTPs |
| 12 | **12c** — `WorkloadOwnershipAnalyzer` | S | Low | Workload hierarchy in graph (needs Issue 1) |
| 13 | **12d** — `RoleBindingAnalyzer` | S | Low | RBAC facts from binding data (needs Issue 10) |
| 14 | **11** — GCP support | M | Low | Cloud coverage |
| 15 | **13** — MITRE / AttackFlow export | L | Low | Reporting |
| 16 | **9** — Extract `crates/app` | L | High | Testability / structure |

Issues 4 and 5 are independent of each other and can be done in any order.
Issue 1 should land before Issues 10, 12c — each adds new entity types and the registry abstraction makes that a one-liner instead of a 6-file change.
Issues 12a and 12b have no dependencies and can be done immediately after Issue 14.
Issue 12d depends on Issue 10 (`k8s.rolebinding` effect).
Issue 9 is the largest structural change and is a prerequisite for proper integration testing.
Issues 10, 11, 12a–12d, 13 are independent of each other and can be parallelized once Issue 1 is done.
