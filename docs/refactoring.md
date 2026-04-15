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

## ~~Issue 9 — `AppState` / `ApiService` impl lives inside the CLI crate~~ ✅ Done

**Files:** `crates/app/src/lib.rs` (new), `crates/app/src/config.rs` (moved from cli), `crates/cli/src/main.rs`

`AppState` and its `ApiService` impl extracted from `crates/cli` into a new
`crates/app` library crate. `crates/cli` is now a thin binary (~140 lines)
that parses arguments and calls `app::start()`.

**`crates/app` public surface:**
- `pub mod config` — `Config`, `NamespaceFilter`, `load()` (moved from cli)
- `pub struct AppState` with a `pub fn new(...)` constructor
- `impl ApiService for AppState` — full service implementation
- `pub struct ServerConfig` — kubeconfig, armory dir, port, namespace filter
- `pub async fn start(cfg: ServerConfig) -> Result<()>` — complete server bootstrap
- `pub struct ScriptParserRunner` — external script-based effect parser

**`crates/cli` reduced to:**
- CLI arg structs + `main()`
- `run_emulate()` — loads config, delegates to `app::start()`
- `run_show_armory()` — armory table display (unchanged)
- `init_tracing()`, `resolve_armory_dir()`

**Integration tests in `crates/app/tests/service.rs`:**
- `namespace_filter_blacklist_excludes_system_namespaces` — default filter excludes kube-system
- `namespace_filter_whitelist_only_allows_listed` — whitelist mode takes precedence
- `config_load_returns_defaults_when_file_missing` — missing ran.yaml returns defaults
- `app_state_get_and_reset_campaign_without_cli` — full AppState via ApiService trait (requires kubeconfig, `#[ignore]` by default); run with `cargo test -p app -- --ignored`

- [x] Create `crates/app` crate
- [x] Move `AppState` and `ApiService` impl into it
- [x] Reduce `crates/cli/src/main.rs` to argument parsing + `app::start()`
- [x] Add integration test in `crates/app` that exercises the full service without the CLI

---

## ~~Issue 10a — `sys.files` and `sys.hasfile(PATH)`~~ ✅ Done

**Go source:** `src/campaign/parsers.go`  
**File:** `crates/campaign/src/output_parsers/sys.rs`  
**Dependencies:** none

Two small sys-level parsers following the same patterns as the existing `sys.*` family.

**`sys.files`** — parses a line-delimited file listing (e.g. `find / -maxdepth 3` output) and populates `system.files`. Lines ending in `*` (from `find -perm /111` or `ls -F` with executable marker) are also recorded in `system.binaries` as present with an empty path (name-only, same as `sys.has-binary` absent-path sentinel meaning "known present but path unresolved").

**`sys.hasfile(PATH)`** — parametric effect, same dispatch pattern as `sys.has-binary(PATH)`. The path is extracted from the effect ID string. If stdout is non-empty / exit code 0, the file is marked present in `system.files`. If exit code is non-zero or stdout is empty, the file is marked absent.

- [x] Added `parse_sys_files` to `output_parsers/sys.rs` and registered it
- [x] Added parametric `sys.hasfile(...)` dispatch to `parse_output_effect` in `output_parsers/mod.rs`
- [x] 6 tests written and passing

---

## ~~Issue 10b — `k8s.can-reach(src, tgt)` relation effect~~ ✅ Done

**Go source:** `src/campaign/parsers.go`  
**Files:** `crates/domain/relations.rs`, `crates/campaign/src/effects.rs`  
**Dependencies:** none

A two-argument relation effect declaring that one entity can reach another over the network. Pattern is identical to the existing `k8s.can-exec(src, tgt)` handler.

**`CanReach` relation:** non-exec-channel edge, relation name `"can-reach"`. Carries source and target entity IDs. Does not implement `C2Channel` — reachability is a precondition for attacks, not itself an execution channel.

- [x] Added `CanReach` struct to `crates/domain/relations.rs` (non-exec-channel)
- [x] Re-exported from `crates/domain/mod.rs`
- [x] Added `"k8s.can-reach"` arm to `resolve_relation_effect_handler` in `effects.rs`
- [x] 4 tests written and passing

---

## ~~Issue 10c — `nmap` network scan parser~~ ✅ Done

**Go source:** `src/campaign/parsers.go`  
**File:** `crates/campaign/src/output_parsers/network.rs`  
**Dependencies:** Issue 10b (`CanReach` relation must exist)

Parses nmap output and discovers reachable hosts as placeholder `Pod` entities. Two input formats are supported:

**Greppable (`-oG`):**
```
Host: 10.0.0.5 ()	Status: Up
Host: 10.0.0.6 (redis.default.svc.cluster.local)	Ports: 6379/open/tcp
```
Each `Status: Up` or `Ports:` line yields one Pod placeholder named `pod-<ip-kebab>` with the IP set in `system.ips`. A `CanReach` relation is emitted from the effect's `TARGET_ID` arg (the scanning pod) to each discovered placeholder.

**XML (`-oX`):** parse `<host>` elements, extract address and hostname; same entity/relation output.

Placeholder pods discovered by `rdns` later (which uses the same `pod-<ip-kebab>` naming convention) will naturally merge with nmap-discovered placeholders via entity ID collision — no explicit alias needed.

**Tests to write:**
- Greppable: single `Status: Up` line → one Pod + one CanReach
- Greppable: multiple hosts → one Pod + one CanReach per host
- Greppable: hostname present → used as pod name instead of IP-kebab
- XML: `<host>` with address element → same Pod + CanReach output
- Empty / malformed output → `KnownFailure`
- Hosts with no open ports / `Status: Down` → skipped

- [x] Added `parse_nmap` to `output_parsers/network.rs`; wired in `parse_output_effect` with `source_id` from `cmd.target_id`
- [x] 6 tests passing (greppable single/multi host, hostname, XML, empty, Status: Down)

---

## ~~Issue 10d — `k8s.serviceaccount` entity effect~~ ✅ Done

**Go source:** `src/campaign/parsers.go`  
**File:** `crates/campaign/src/effects.rs`  
**Dependencies:** none (`ServiceAccount` entity already exists)

Simple effect that creates a single `ServiceAccount` entity from TTP args. Mirrors the existing `k8s.pod` handler exactly.

Args: `Namespace` (required), `ServiceAccountName` / `SA_NAME` (required), optionally `Token` (sets `sa.token.jwt.raw`).

**Tests to write:**
- Valid `Namespace` + `ServiceAccountName` args → `ServiceAccount` entity with correct namespace and name
- Missing `Namespace` → `Err`
- Missing `ServiceAccountName` → `Err`
- Optional `Token` arg → `sa.token` populated

- [x] Added `parse_k8s_serviceaccount` to `effects.rs`, registered in `resolve_simple_effect_handler`
- [x] 4 tests passing

---

## ~~Issue 10e — `k8s.role` and `k8s.rolebinding` entity effects~~ ✅ Done

**Go source:** `src/campaign/parsers.go`  
**Files:** `crates/domain/entities.rs`, `crates/campaign/src/effects.rs`  
**Dependencies:** Issue 1 ✅ (new entity types are now a single `register` call)  
**Unblocks:** Issue 12d (`RoleBindingAnalyzer`)

**`k8s.role`** — creates a `K8sRole` entity. Args: `Namespace`, `RoleName`. The role carries a list of `RbacPermission` entries parsed from a `Rules` arg (JSON array of `{"verbs":[],"resources":[]}` objects, same schema as `k8s.selfsubjectrulesreview` output).

**`k8s.rolebinding`** — creates a `K8sRoleBinding` entity. Args: `Namespace`, `BindingName`, `RoleRef` (name of the referenced role), `Subjects` (JSON array of `{"kind":"ServiceAccount","name":"...","namespace":"..."}` objects). The `RoleBindingAnalyzer` (Issue 12d) is what converts a `K8sRoleBinding` entity into SA entitlements — the effect itself only creates the entity.

**Domain types:**
- `K8sRole { name, namespace, permissions: Vec<RbacPermission> }`
- `K8sRoleBinding { name, namespace, role_ref: String, subjects: Vec<RbacSubject> }`
- `RbacSubject { kind: String, name: String, namespace: String }`

**Tests to write:**
- `k8s.role`: valid args → `K8sRole` with correct name, namespace, parsed permissions
- `k8s.role`: missing `RoleName` → `Err`
- `k8s.role`: empty `Rules` arg → role created with empty permissions (not an error)
- `k8s.rolebinding`: valid args → `K8sRoleBinding` with correct role_ref and subjects list
- `k8s.rolebinding`: missing `BindingName` → `Err`
- `k8s.rolebinding`: empty `Subjects` → binding created with empty subjects (not an error)

- [x] Added `K8sRole`, `K8sRoleBinding`, `RbacSubject` to `crates/domain/entities.rs` with `Merge` impls
- [x] Registered in `EntityStore` default + `CampaignEntityRef` variants
- [x] Added `parse_k8s_role` and `parse_k8s_rolebinding` to `effects.rs`
- [x] 6 tests passing (valid args, missing name Err, empty rules/subjects, parsed permissions)

---

## ~~Issue 10f — `k8s.cronjob` entity effect~~ ✅ Done

**Go source:** `src/campaign/parsers.go`  
**Files:** `crates/domain/entities.rs`, `crates/campaign/src/effects.rs`  
**Dependencies:** Issue 1 ✅  
**Note:** `WorkloadOwnershipAnalyzer` (Issue 12c) also needs `CronJob` — coordinate or do together

Creates a `CronJob` entity. Args: `Namespace` (required), `CronJobName` / `CRONJOB_NAME` (required), optionally `Schedule` (cron expression string).

**Tests to write:**
- Valid args → `CronJob` entity with correct namespace and name
- Optional `Schedule` arg populated correctly
- Missing `Namespace` → `Err`
- Missing `CronJobName` → `Err`

- [x] Added `CronJob { meta, schedule }` to `crates/domain/entities.rs` with `Merge` impl
- [x] Registered in `EntityStore` default + `CampaignEntityRef` variant
- [x] Added `parse_k8s_cronjob` to `effects.rs`
- [x] 4 tests passing

---

## Issue 10g — `file:content` and `file:kubeconfig`

**Go source:** `src/campaign/parsers.go`  
**Files:** `crates/campaign/src/output_parsers/file.rs` (new module), `crates/domain/entities.rs`  
**Dependencies:** Issue 1 ✅ (new `K8sCredential` entity type)

The most complex of the file parsers. Two effects that chain together.

**`file:content`** — stores raw file content in `system.files` keyed by path. The effect ID carries the path: `file:content(/etc/kubernetes/admin.conf)`. After storing, heuristically checks whether the content looks like a kubeconfig (contains `apiVersion: v1`, `kind: Config`, and `clusters:`) and if so, runs the kubeconfig sub-parser to create a `K8sCredential` entity.

**`file:kubeconfig`** — can also be declared explicitly as an effect. Parses kubeconfig YAML:
- Extracts `clusters[].cluster.server` (endpoint) and `clusters[].cluster.certificate-authority-data` (CA)
- Extracts `users[].user.token` (bearer token) or `users[].user.client-certificate-data` + `client-key-data` (mTLS)
- Creates a `K8sCredential { endpoint, ca_data, token: Option<String>, cert_data: Option<String>, key_data: Option<String> }` entity
- Emits a `Uses` relation from the target system to the `K8sCredential`

**Tests to write:**
- `file:content(/tmp/foo)`: plain text → content stored in `system.files`, no credential entity
- `file:content(/etc/kubernetes/admin.conf)`: valid kubeconfig YAML → content stored AND `K8sCredential` entity emitted
- `file:content`: kubeconfig with token auth → `K8sCredential.token` populated
- `file:content`: kubeconfig with cert auth → `K8sCredential.cert_data` + `key_data` populated
- `file:kubeconfig`: explicitly declared, same YAML input → same `K8sCredential` output
- `file:kubeconfig`: malformed YAML → `UnknownFormat`
- `file:content`: empty stdout → `KnownFailure`
- Path extraction from effect ID with colons and slashes (`file:content(/var/run/secrets/token)`)

- [ ] Add `K8sCredential` entity to `crates/domain/entities.rs` and register in `EntityStore`
- [ ] Create `output_parsers/file.rs`, register `file:kubeconfig` in the module registry
- [ ] Handle parametric `file:content(...)` dispatch in `parse_output_effect` (mirrors `sys.has-binary` pattern)
- [ ] Write tests for the eight cases above

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

## ~~Issue 12a — `CanExecAccessAnalyzer`~~ ✅ Done

**Go source:** `src/campaign/rules_builtin.go`  
**File:** `crates/campaign/src/analyzers.rs`  
**Dependencies:** none

When a system entity receives an incoming exec-channel relation (any relation where `is_exec_channel()` returns true — covers `PodExec`, `KubeletExecSink`, `RceCanExec`, and future types), set its `system.access_level` to `Exec`. Already-`Exec` entities are unaffected (merge takes max). This ensures access level propagates through lateral movement paths discovered after initial compromise, not only from `sys.userid` output.

- [x] Added `CanExecAccessAnalyzer` to `analyzers.rs`, using `r.is_exec_channel()` instead of a name allowlist
- [x] Added to `default_analyzers()`
- [x] 5 tests written and passing (sets Exec, idempotent for existing Exec, kubelet-exec-sink, rce-can-exec, non-system target ignored)

---

## ~~Issue 12b — `PropagateHostIPAnalyzer`~~ ✅ Done

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

- [x] Add `host_ip: Option<IpAddr>` field to `Pod` in `crates/domain/entities.rs`; populated from `status.hostIP` by `k8s.podlist` parser
- [x] Add `PropagateHostIPAnalyzer` to `analyzers.rs` (placed after `PodNodeAnalyzer` in pipeline)
- [x] Add to `default_analyzers()`
- [x] 4 tests passing (host_ip→existing runs-on, already-present IP no-op, no host_ip no update, runs-on arrives after pod)

---

## ~~Issue 12c — `WorkloadOwnershipAnalyzer`~~ ✅ Done

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

- [x] Add `owner_references: Vec<OwnerRef>` field to `Pod` (populated from `k8s.podlist` JSON `metadata.ownerReferences`)
- [x] Add `ReplicaSet`, `StatefulSet`, `DaemonSet`, `Job` entity types to `crates/domain/entities.rs`; registered in `EntityStore` default + `CampaignEntityRef` variants
- [x] Add `Owns` relation type to `crates/domain/relations.rs`
- [x] Add `WorkloadOwnershipAnalyzer` to `analyzers.rs`
- [x] Add to `default_analyzers()`
- [x] 6 tests passing (ReplicaSet, StatefulSet, DaemonSet, Job owners; already-known owner no duplicate; no owner refs emits nothing)

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

## ~~Issue 14 — Execution records API endpoint~~ ✅ Done

**Files:** `crates/api/src/api_handlers.rs`, `crates/api/src/lib.rs`

`GET /api/execution-records` and `GET /api/execution-records/:id` added.

Each response entry is an `ExecutionRecordEntry` — the raw `ExecutionRecord` (flattened,
includes full stdout in `results`) joined with its `Vec<ParseAudit>` under `parseAudits`.
Parse audits are correlated by `cmd_id`. No new trait method needed — handlers call the
existing `get_campaign()` and read `execution_records` + `parse_audits` directly.

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
| 7 | ~~**14** — Execution records API endpoint~~ ✅ | XS | Low | Self-improving loop unblocked |
| 8 | ~~**10a** — `sys.files` + `sys.hasfile`~~ ✅ | XS | Low | File enumeration parser coverage |
| 9 | ~~**10b** — `k8s.can-reach` + `CanReach` type~~ ✅ | XS | Low | Network reachability effect |
| 10 | ~~**12a** — `CanExecAccessAnalyzer`~~ ✅ | XS | Low | Access level propagation via lateral movement |
| 11 | ~~**12b** — `PropagateHostIPAnalyzer`~~ ✅ | XS | Low | Node IP visibility for kubelet TTPs |
| 12 | ~~**10c** — `nmap` parser~~ ✅ | S | Low | Network host discovery |
| 13 | ~~**10d** — `k8s.serviceaccount` effect~~ ✅ | XS | Low | Single SA entity creation |
| 14 | ~~**10e** — `k8s.role` + `k8s.rolebinding` effects~~ ✅ | S | Low | RBAC entities; unblocks 12d |
| 15 | ~~**10f** — `k8s.cronjob` effect~~ ✅ | XS | Low | CronJob entity (coordinate with 12c) |
| 16 | ~~**12c** — `WorkloadOwnershipAnalyzer`~~ ✅ | S | Low | Workload hierarchy in graph |
| 17 | **12d** — `RoleBindingAnalyzer` | S | Low | RBAC facts from binding data (needs 10e) |
| 18 | **10g** — `file:content` + `file:kubeconfig` | M | Low | Credential extraction from files |
| 19 | **11** — GCP support | M | Low | Cloud coverage |
| 20 | **13** — MITRE / AttackFlow export | L | Low | Reporting |
| 21 | ~~**9** — Extract `crates/app`~~ ✅ | L | High | Testability / structure |

Issues 1, 9, 14, 10a–10f, 12a ✅ done.
12b has no remaining dependencies — can start immediately.
10g is the most complex remaining item; 12c, 12d are now unblocked by 10e ✅ and 10f ✅.
10e unblocks 12d; 10f and 12c share the CronJob entity type and can be done together.
10g is the most complex parser and can be deferred without blocking anything else.
