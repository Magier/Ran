# App Services and Service Packs

## Status

Design proposal. This document defines a generic model for observed network
services and a future extension format for application-specific content. It is
intended to guide implementation; names and serialization formats are not yet
stable APIs.

## Motivation

Ran currently knows that one system can reach another, but it does not retain
which port was reachable or which application answered there. The nmap parser,
for example, turns a live host into a system entity and a `can-reach` relation,
while discarding open-port, product, version, banner, and CPE observations.

This is not enough to decide whether an application-specific TTP is relevant.
The Redis CVE-2022-0543 TTP illustrates the problem: discovering a pod and
having an execution foothold does not prove that the pod exposes Redis, that
the foothold can connect to its Redis endpoint, or that the Redis build is
vulnerable.

The solution must support Redis without making Redis a core-domain special
case. The same model should support PostgreSQL, Jenkins, Kafka, databases,
message brokers, web servers, control planes, and applications added later by
users.

## Design principles

1. **Separate observation levels.** An open port, an identified application,
   an application capability, and a confirmed vulnerability are distinct
   facts.
2. **Require positive evidence.** An unknown application must not satisfy an
   application-specific prerequisite.
3. **Keep parser output vendor-neutral.** TTPs consume normalized facts rather
   than depending on a particular scanner's output format.
4. **Preserve provenance and confidence.** Configuration, inference, banners,
   protocol handshakes, and vulnerability probes have different evidentiary
   strength.
5. **Model connectivity at endpoint granularity.** Host-level reachability does
   not prove that a particular port is reachable.
6. **Keep the core extension contract declarative where practical.** Installing
   application knowledge should not automatically grant arbitrary code
   execution inside the Ran process.

## Core domain model

### `AppService` entity

An app service represents an observed or inferred application listening
endpoint. The name distinguishes it from Kubernetes `Service`, which is desired
cluster configuration and may front several app-service endpoints.

```rust
pub struct AppService {
    pub id: Option<String>,
    pub address: String,
    pub port: u16,
    pub transport: Transport,       // Tcp | Udp | Sctp | Unknown
    pub state: EndpointState,       // Open | Closed | Filtered | Unknown
    pub product: Option<String>,    // canonical name, e.g. "redis"
    pub version: Option<String>,
    pub cpes: Vec<String>,
    pub banner: Option<String>,
    pub tls: Option<TlsObservation>,
    pub capabilities: Vec<String>,
    pub vulnerabilities: Vec<VulnerabilityObservation>,
    pub confidence: Confidence,
    pub observed_at_ms: Option<u64>,
}
```

Suggested stable ID:

```text
app-service/<transport>/<normalized-address>/<port>
```

The address is part of the endpoint identity. A later reconciliation rule can
merge an IP-derived service with a DNS- or workload-derived identity while
preserving aliases and evidence.

`product` values must be canonical, lowercase identifiers owned by the service
catalog (`redis`, `postgresql`, `http`, `jenkins`), not scanner-specific display
strings.

### Relations

```text
System        --hosts-service--> AppService
System        --can-connect----> AppService
K8sService    --routes-to-------> AppService
```

- `hosts-service` identifies the system believed to run the endpoint.
- `can-connect` is source- and endpoint-specific network reachability. It does
  not confer execution and must not implement `C2Channel`.
- `routes-to` associates configured Kubernetes service ports with observed
  backend endpoints when selectors, EndpointSlices, or probes provide enough
  evidence.

The existing host-level `can-reach` relation remains useful for broad network
knowledge. It must not be treated as proof that every endpoint on the target is
connectable.

### Evidence levels

The first implementation can reuse Ran's existing provenance and confidence
types, but the model should preserve the source of each material observation.

| Evidence | Example | Typical confidence |
|---|---|---|
| Configuration | Kubernetes port named `redis` | Low |
| Port convention | TCP/6379 | Low |
| Banner or scanner fingerprint | nmap identifies Redis | Medium |
| Protocol handshake | Valid Redis `PING` response | High |
| Authenticated query | Redis `INFO` reports version | High |
| Vulnerability probe | Safe CVE-specific test succeeds | High |

Port numbers and names may produce candidates, but should not independently
satisfy a strict product prerequisite unless the TTP explicitly accepts low
confidence.

Capabilities and vulnerabilities must be namespaced and independently
evidenced:

```text
redis.lua
redis.auth-required
CVE-2022-0543
```

Application identity does not imply vulnerability. Version-based inference may
produce a vulnerability observation with lower confidence than a direct probe.

## TTP prerequisites

Add a normalized `appService` requirement. A single object is sufficient
initially; the schema can later accept an array for AND/OR combinations.

```yaml
preconditions:
  kind: System
  appService:
    product: redis
    transport: tcp
    port: 6379
    reachableFrom: execution-source
    minConfidence: medium
    vulnerability: CVE-2022-0543
```

All declared fields are conjunctive. Missing observed fields do not match.
`reachableFrom: execution-source` means that at least one viable execution
source has a `can-connect` relation to the matching endpoint.

Applicability evaluation should return the matching endpoint as a **witness**,
not just a boolean. Grounding then obtains `TARGET`, `PORT`, transport, and any
other endpoint arguments from the same witness that satisfied the requirement.
This prevents applicability and execution from selecting different endpoints.

The UI should show unmet requirements in the existing prerequisite tooltip:

```text
Service: redis/tcp
Port: 6379
Reachable from: execution source
Vulnerability: CVE-2022-0543
```

Applicable-only mode hides a TTP when required facts are unknown. Show-all mode
may still display it with unmet prerequisites, allowing an operator to see
which discovery action would advance the campaign.

## Discovery and inference pipeline

### Direct observations

Parsers should emit `AppService` entities and relations from:

- nmap XML and service/version detection (`-sV`)
- greppable nmap output when it contains ports
- protocol-specific probes
- local socket/process enumeration
- Kubernetes EndpointSlices and Services
- application commands such as Redis `INFO`

The nmap parser should stop reducing results to `(ip, hostname)`. Its internal
result should retain host status plus a list of port observations, including
state, transport, service name, product, version, CPE, and banner where present.

### Inference

Inference rules may enrich but must not overstate observations:

- Kubernetes `appProtocol`, named ports, labels, and service names may suggest
  a product with low confidence.
- EndpointSlices can connect a Kubernetes Service port to pod/IP endpoints.
- A local listening socket plus a known process can identify the hosted service.
- Product and version catalogs can infer candidate vulnerabilities.

Newer, stronger observations should upgrade weaker fields without erasing
their provenance. Closed-port observations should expire or supersede stale
open-port observations rather than permanently coexisting as truth.

## Service packs

### Purpose

A service pack adds application knowledge without requiring changes to Ran's
core. Packs may contain fingerprints, parsers, TTPs, inference rules, and UI
metadata for one product family.

```text
redis/
├── service-pack.yaml
├── fingerprints/
│   └── redis.yaml
├── parsers/
│   ├── info.yaml
│   └── scan.yaml
├── TTPs/
│   ├── enumerate.yaml
│   └── exploit-cve-2022-0543.yaml
├── rules/
│   └── vulnerabilities.yaml
└── assets/
    └── redis.svg
```

### Manifest

```yaml
apiVersion: ran.manifold.security/v1alpha1
kind: ServicePack
metadata:
  id: redis
  name: Redis
  version: 0.1.0
  publisher: example
compatibility:
  ran: ">=0.3.0 <0.4.0"
provides:
  products: [redis]
  capabilities:
    - redis.lua
    - redis.auth-required
  vulnerabilities:
    - CVE-2022-0543
content:
  fingerprints: fingerprints/
  parsers: parsers/
  ttps: TTPs/
  rules: rules/
```

IDs declared by a pack are namespaced or catalog-validated. A pack cannot
silently redefine core products, capabilities, parser IDs, or TTP IDs owned by
another pack.

### Loading and lifecycle

Service packs should be loaded alongside the armory through explicit configured
paths. Loading consists of:

1. Validate manifest and compatibility.
2. Validate all referenced files and schemas.
3. Register catalog identifiers and detect collisions.
4. Register declarative fingerprints, parsers, and rules.
5. Merge enabled TTPs into the armory.
6. Report pack health and rejected content through the API/UI.

Campaign execution records should retain the pack ID and version responsible
for a TTP or parser so old results remain explainable after upgrades.

### Security boundary

The initial service-pack format should support only declarative content and the
existing constrained external-parser mechanism. Arbitrary native libraries or
in-process scripts should not be loaded from a pack.

If executable probes or parsers are later supported, the manifest must declare
their permissions and they should run out-of-process with:

- explicit operator enablement
- bounded time, CPU, memory, and output
- no inherited secrets by default
- controlled filesystem and network access
- signature and publisher information
- an audit record for every invocation

Installing a pack and authorizing its active probes are separate decisions.

## Redis migration example

The Redis exploit should eventually require:

```yaml
preconditions:
  kind: System
  appService:
    product: redis
    reachableFrom: execution-source
    minConfidence: medium
    vulnerability: CVE-2022-0543
  source:
    tool: redis-cli
```

This replaces the current over-broad behavior. The selected system remains the
semantic exploit target, while applicability selects both:

- an execution-source system holding `redis-cli`
- a Redis endpoint hosted by the selected target and reachable from that source

Grounding derives `TARGET` and `PORT` from the endpoint witness. Successful
execution creates the existing `rce.can-exec(source, target)` relation.

## Delivery plan

### Phase 1: Preserve endpoint observations

- Add `AppService`, merge semantics, serialization, API conversion, and
  graph styling.
- Add `hosts-service` and `can-connect` relations.
- Extend nmap parsing to retain open ports without yet requiring product data.
- Keep existing `can-reach` output for compatibility.

### Phase 2: Product identification

- Parse nmap product, version, CPE, and banners.
- Add canonical product catalog and normalization.
- Add confidence-aware merging and tests for conflicting observations.
- Add protocol fingerprint effects/parsers.

### Phase 3: Applicability and grounding

- Add the `appService` requirement schema and evaluator.
- Return requirement witnesses and ground endpoint arguments from them.
- Add source-tool and endpoint-connectivity checks.
- Expose satisfied/unmet evidence in prerequisite explanations.

### Phase 4: Redis reference pack

- Build Redis as the first in-tree service pack.
- Add safe Redis fingerprint and enumeration actions.
- Migrate CVE-2022-0543 with precise source, endpoint, and vulnerability
  requirements.
- Use the pack as the compatibility and validation test fixture.

### Phase 5: General service-pack loading

- Add manifest discovery, compatibility checks, collision handling, and pack
  health reporting.
- Add provenance fields for pack ID/version to TTPs and parser audits.
- Document installation and trust behavior.
- Validate the abstraction with a second substantially different pack, such as
  PostgreSQL or Jenkins, before declaring the API stable.

### Phase 6: Freshness and active probing

- Add observation expiry and explicit negative observations.
- Add controlled active probes and resource limits.
- Add signed-pack and publisher trust support if packs are distributed.

## Decisions to validate during implementation

1. Whether endpoint entities appear as first-class graph nodes by default or
   remain collapsed beneath their host to avoid graph noise.
2. Whether evidence must become field-level immediately or entity-level
   provenance is sufficient for the first phase.
3. How service observations expire in long-running campaigns.
4. Which version and CPE comparison library is appropriate for vulnerability
   inference.
5. Whether service-pack parsers remain declarative only in v1 or may invoke the
   existing out-of-process parser mechanism.

The recommended defaults are collapsed endpoint nodes, field-level evidence if
it can be introduced without delaying Phase 1, explicit observation timestamps,
and declarative-only packs for the first public format.

## Phase 1 implementation specification

This section is the handoff contract for the first implementation session. If
it conflicts with exploratory text above, this narrower specification wins for
Phase 1.

### Objective

Retain observed network endpoints as first-class campaign facts. After an nmap
scan, Ran must know which address, port, and transport were observed, which
system hosts the endpoint, and which execution source could connect to it.

Phase 1 does **not** change TTP applicability. It establishes the facts that a
later phase will consume.

### Fixed decisions

1. The entity is named `AppService`; its `Entity::entity_kind()` is
   `"AppService"`.
2. Phase 1 uses entity-level `KnowledgeProvenance`. Field-level evidence is
   deferred, but `confidence` and `observed_at_ms` remain on the entity so the
   wire format can evolve without replacing the type.
3. Endpoint identity is `(normalized address, transport, port)`. Product and
   version are mutable observations and never participate in identity.
4. IP addresses are canonicalized through `std::net::IpAddr`. DNS names are
   lowercase with a trailing dot removed.
5. The stable entity ID is
   `app-service/<transport>/<normalized-address>/<port>`. Supported addresses
   cannot contain `/`; reject rather than silently rewriting an invalid value.
6. An `AppService` is a graph entity, not a `SystemEntity`, and never carries an
   access level or execution session.
7. `hosts-service` and `can-connect` are non-execution relations. Neither may
   implement `C2Channel` or create an executable graph path.
8. Retain existing host-level `can-reach` facts for backward compatibility.
9. App-service nodes are serialized through the existing graph API. The UI may
   initially render them as ordinary nodes; automatic collapsing is desirable
   but is not an acceptance criterion for Phase 1.
10. Entity merge behavior is monotonic for identity and descriptive fields:
    newer non-empty product/version/banner/CPE observations enrich or replace
    weaker values. `observed_at_ms` takes the newest timestamp. Endpoint state
    follows the newest timestamp. Expiry is deferred.

### Minimal Phase 1 type

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppService {
    pub address: String,
    pub port: u16,
    pub transport: Transport,
    pub state: EndpointState,
    pub product: Option<String>,
    pub version: Option<String>,
    pub cpes: Vec<String>,
    pub banner: Option<String>,
    pub confidence: Confidence,
    pub observed_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Udp,
    Sctp,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EndpointState {
    Open,
    Closed,
    Filtered,
    Unknown,
}
```

Use the repository's established serde naming conventions. Add constructors
that validate address and port rather than allowing callers to construct an
invalid stable ID. Port `0` is invalid for an observed service.

TLS metadata, capabilities, and vulnerabilities from the broader model are
explicitly deferred.

### Relations

Add typed relations in `crates/domain/relations.rs`:

```rust
HostsService::new(system_id, app_service_id) // "hosts-service"
CanConnect::new(source_id, app_service_id)   // "can-connect"
```

Both relations use ordinary zero-cost structural graph edges and are exported
from `crates/domain/mod.rs`. Relation summaries and campaign serialization
must round-trip without relation-specific special cases.

### Nmap parser behavior

Refactor the internal nmap representation from host tuples into structured
observations:

```rust
struct NmapHostObservation {
    address: IpAddr,
    hostname: Option<String>,
    ports: Vec<NmapPortObservation>,
}

struct NmapPortObservation {
    port: u16,
    transport: Transport,
    state: EndpointState,
    product: Option<String>,
    version: Option<String>,
    cpes: Vec<String>,
    banner: Option<String>,
}
```

For each accepted host:

1. Preserve the current system classification and IP/CIDR filtering.
2. Preserve the current source-to-system `can-reach` relation.
3. For each parsed port, emit an `AppService`.
4. Emit system-to-service `hosts-service`.
5. If `source_id` is present and the endpoint state is `Open`, emit
   source-to-service `can-connect`.
6. Do not emit `can-connect` for closed, filtered, or unknown ports.

Support all three currently accepted nmap families:

- XML: parse port state, protocol, service product/name, version, CPE, and
  banner/extrainfo when present.
- Greppable: parse `port/state/protocol` and any available service columns.
- Standard `-sV` output: parse the `PORT STATE SERVICE VERSION` table associated
  with the current host.

Host-only scan output remains valid and simply produces no `AppService` facts.
Malformed individual port entries are skipped without losing other valid hosts
or ports. If hosts are valid but no ports are valid, retain the successful host
discovery behavior.

### Repository touchpoints

The implementing agent should expect to update at least:

| Area | Files |
|---|---|
| Domain type and merge behavior | `crates/domain/entities.rs`, `crates/domain/types.rs`, `crates/domain/mod.rs` |
| Typed relations | `crates/domain/relations.rs`, `crates/domain/mod.rs` |
| Campaign entity registration | `crates/campaign/src/campaign/entity_refs.rs`, `crates/campaign/src/campaign/entity_store.rs` |
| Nmap observations and emitted facts | `crates/campaign/src/output_parsers/network.rs` |
| API entity serialization | `crates/api/src/state_conversions.rs` |
| Graph/API contract if required | `api/openapi.yaml`, generated frontend API types |
| UI presentation | graph style/category helpers and entity-info rendering under `frontend/src/routes/components/` |

Also search exhaustive `CampaignEntityRef` matches; Rust compilation will find
many, but API/MCP summaries and namespace helpers may require intentional
handling rather than a placeholder arm.

Do not edit files under `examples/` for this work.

### Compatibility and migration

- Add `#[serde(default)]` for the new entity-store slot so older serialized
  campaigns deserialize with no app services.
- Existing nmap host and `can-reach` behavior must remain unchanged.
- No existing entity IDs or relation names may change.
- No existing TTP becomes applicable or inapplicable solely because Phase 1 is
  installed.
- The graph API's generic entity payload may carry `AppService` without a new
  top-level endpoint. Add an OpenAPI component only if a typed public schema is
  introduced.

### Acceptance tests

The implementation is complete only when all of the following are covered:

1. `AppService` ID normalization is deterministic for IPv4, IPv6, and DNS.
2. Invalid addresses and port `0` are rejected.
3. Entity merge keeps one endpoint identity and enriches product/version/CPE.
4. Newer endpoint state wins; an older observation cannot overwrite it.
5. Campaign state serializes and deserializes the new entity-store slot.
6. `hosts-service` and `can-connect` round-trip as relation summaries and are
   never execution channels.
7. Nmap greppable input `6379/open/tcp` produces an open TCP app service.
8. Standard `-sV` output produces product and version when present.
9. Nmap XML produces product, version, CPE, and endpoint relations.
10. A host with two open ports produces two distinct app services.
11. Closed or filtered ports are retained as observations but do not create
    `can-connect`.
12. Host-only scans preserve current discovery and reachability behavior.
13. CIDR filtering excludes both the out-of-scope host and its app services.
14. The campaign graph/API includes an `AppService` node with its entity data.
15. Existing domain, campaign, API, and frontend tests still pass.

### Explicit non-goals

Do not implement any of the following in Phase 1:

- `appService` TTP prerequisite evaluation or witness grounding
- Redis-specific applicability changes
- vulnerability or capability inference
- Kubernetes Service/EndpointSlice correlation
- service-pack manifests or loading
- active protocol probes
- observation expiry
- arbitrary executable plugin code
- a full product normalization catalog

Raw scanner product names may be normalized conservatively (trimmed and
lowercase) for Phase 1. Catalog aliases belong to Phase 2.

### Recommended handoff prompt

> Implement Phase 1 from `docs/app_services_and_service_packs.md`, treating the
> “Phase 1 implementation specification” as authoritative. Do not implement
> later phases or touch `examples/`. Preserve unrelated working-tree changes,
> run the relevant Rust and frontend suites, and report any design conflict
> before expanding scope.
