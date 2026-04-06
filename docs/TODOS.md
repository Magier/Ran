# TODOs


- entity info entitlement -> cluster-scoped perms shouldn't show NS 
- campaign reset doesn't work properly when the pod name changes in the meantime
    - it tries to add an edge for an outdated pod??

- cmd fail due to RBAC 403 shows `success` in the attack flow, but is a failure 

## Replace `RelationSummary` with typed trait objects (Option 2)

**Context:** `Campaign.relations` stores `Vec<RelationSummary>` — a plain serialisable struct with
only `name`, `source_id`, `target_id`, and `is_exec_channel`. The concrete relation type is
erased at insertion time (`RelationSummary::from_relation`).

**Why this matters:** The `C2Channel` marker trait and `is_exec_channel: bool` on `RelationSummary`
are a workaround for the fact that `Box<dyn Relation>` is not serialisable or cloneable. Any new
semantic property added to a concrete relation type (e.g. port, credentials, RCE envelope string on
`RceCanExec`) is silently dropped. Rule inference and channel resolution are limited to the
information that fits in the three-field summary.

**What option 2 would look like:**
- Add `typetag` to `ran-domain` and annotate `impl Relation` on each concrete type with
  `#[typetag::serde]`. This makes `Box<dyn Relation>` serialisable with a `type` discriminant
  in the JSON.
- Add `dyn_clone` and derive `Clone` on each concrete type; a blanket impl gives
  `Box<dyn Relation>: Clone`.
- Change `Campaign.relations` from `Vec<RelationSummary>` to `Vec<Box<dyn Relation>>`.
- `C2Channel` then works purely as a marker trait — `is_exec_channel()` dispatches
  polymorphically with no stored flag and no sync risk.
- Delete `RelationSummary` entirely.

**Cost / risk:**
- `typetag` changes the JSON shape (adds a `"type"` discriminant to every relation).
  Existing serialised campaign snapshots become incompatible.
- All code constructing `RelationSummary {}` literals (tests, rule inference, runtime) must be
  updated to use concrete types.
- Medium-sized refactor; touching `campaign`, `api`, and all tests that push relations directly.

**Current state:** `is_exec_channel: bool` on `RelationSummary` bridges the gap. The `C2Channel`
marker trait in `domain/relation.rs` is already the authoritative list — adding a new exec-channel
relation is `impl C2Channel for MyType {}` plus `fn is_exec_channel(&self) -> bool { true }` in
its `Relation` impl.


- files are always interpreted as binaries?

- make session an attribute of a system
- model the container escape as a switch from one system to another
- upload binary:
    support local file picker
- fix `grep` hack in c2.go when executing TTP
- pivot mechanism
    - have primary targeted system 

- make sure the pod spawned by the TTP to get an SA is considered `IsRunning=false`
- support variables in effects

- mounts: how to identify if it's a directory? 
    - don't provide potion to read file, if it's a directory

- get volumemount with hostpath -> kubelet gets more nodes, with unknown node
    - after getting proper node info, update all the other relations and entities as well (instead of pointing to unknown node)

- [UI] decouple the `Tree` UI component from the `Mount` types
- for chained execution channel, properly wrap the commands and the returned errors.

- [Sliver] sliver-c2-channel should go from listener to the target, not from sliver itself

- create callbacks for entitlement-related relations
    - identified entitlements: analyze against entities in KB
    - register callback for new entities of same type
    - when new entities are added, check if they match the entitlements (see `syncCapabilities`)

- properly parse effects from
    - SideCar Injection

- [Tracing] establish link between results from an executed TTP and the input for a follow-up TTP
    - maybe make this explicit as "Condition" nodes in between the actions in the UI?


- [UI] track which entities are under C2 control
   - new entities added to the target environment (and may need to be cleaned up) 
   - which ones are only known, but not yet interacted with


- [UI] abstract the regular permissions associated with system:authenticated into the respective groups
    - [Docs](https://kubernetes.io/docs/reference/access-authn-authz/rbac/#default-roles-and-role-bindings)


- [UI] Use state to improve UX
    - [UI] order procedures depending of the availability of necessary binaries
    - [UI] suggest tokens based on the prerequisites of the TTP
        - e.g. if a TTP requires a token with `rbac: can get pods`, then suggest tokens that have this permission
        - support SAs, which are not yet "appraised" 


- support array of strings when parsing TTPs
    - e.g. `command`: `["a", "b"]` instead of `a b`

- explore the `kubectl attach` command for interactive sessions
- explore the `kubectl debug` to copy and modify a pod

- in `kubectl debug` profiles, show options options  and support constraints of each
    - e.g. if PSA `baseline` is applied on the NS, then at most `baseline` profile will work, not `sysadmin`, etc.

- using K8s-API to get resource kind should do a sync instead of just adding the resources
    - e.g. if a pod was deleted in the maintime, the `k get pods` TTP should not return it

- rework the targeting system:
    - execute from "closest" compromised container (if any)
    - what to do with selected target depends on TTP
        - e.g. create workload to get token for a role targets the role, but is executed on the "closest" compromised container

- make TTP based on others with pre-filled args
- properly implement loading TTPs from `tools`



- trying to enumerate SA names: parse the error message  
```
Error from server (Forbidden): pods "developer-70634" is forbidden: error looking up service account default/rbac-manager: serviceaccount "rbac-manager" not found
command terminated with exit code 1: 'Error from server (Forbidden): pods "developer-70634" is forbidden: error looking up service account default/rbac-manager: serviceaccount "rbac-manager" not found
```