# Kubelet Mount Pod Extraction — Design Spec

**Date:** 2026-05-04
**Branch:** oxidation

## Context

When Ran collects `/proc/1/mountinfo` from a privileged pod on a node, the output contains
entries for every pod volume mounted by kubelet on that node. The legacy Go implementation
(`src/campaign/analyzers.go`, `createPodFromKubeletMounts`) already extracted pod identity
from these paths. This spec defines the Rust port.

## Scope

A new `KubeletMountAnalyzer` in `crates/campaign/src/analyzers.rs` that:

1. Fires on any pod whose `system.mounts` contains kubelet pod volume paths
2. Discovers other pods running on the same node by parsing those paths
3. Derives a human-readable display name for each discovered pod
4. Emits `Pod` entities and `RunsOn` relations into the campaign graph

The mount parser (`crates/campaign/src/output_parsers/sys.rs`) is **unchanged**.

## Mount Path Structure

Kubelet pod volume mounts follow this pattern:

```
/var/lib/kubelet/pods/{pod-uid}/volumes/{volume-type}/{volume-name}
```

Examples from real data:

```
/var/lib/kubelet/pods/84cc979b-9ad8-4418-8b97-24a959833ce7/volumes/kubernetes.io~projected/kube-api-access-28sp8
/var/lib/kubelet/pods/1a2d455a-b8a5-46a9-bd9d-c376a9b50575/volumes/kubernetes.io~secret/argocd-dex-server-tls
/var/lib/kubelet/pods/1a2d455a-b8a5-46a9-bd9d-c376a9b50575/volumes/kubernetes.io~secret/argocd-repo-server-tls
/var/lib/kubelet/pods/293aba3c-f29f-4cd7-a4fe-233b4d111654/volumes/kubernetes.io~projected/clustermesh-secrets
/var/lib/kubelet/pods/293aba3c-f29f-4cd7-a4fe-233b4d111654/volumes/kubernetes.io~projected/hubble-tls
```

## Analyzer: `KubeletMountAnalyzer`

**Trigger:** fires on pods (same trigger as `HostPathAnalyzer` — a pod with mounts in `system.mounts`).

**Relation to `HostPathAnalyzer`:** complementary, not merged. `HostPathAnalyzer` handles "this
pod can see a node". `KubeletMountAnalyzer` handles "which other pods are on that node".

### Path Extraction

For each mount in `system.mounts`:

1. Strip prefix `/var/lib/kubelet/pods/`
2. Split remainder on `/` — expect at least 4 parts: `[pod-uid, "volumes", volume-type, volume-name]`
3. Validate `parts[0]` is a well-formed UUID
4. Take `parts[3]` as the volume name

**On any failure** (wrong structure, too-few segments, non-UUID): log a warning and skip the
mount. Warnings are emitted during early deployment to catch unexpected real-world formats.

### Generic Volume Detection

Volume names starting with `kube-api-access-` are flagged as **generic** (the default projected
SA token). All other names — including non-SA-token projected volumes like `clustermesh-secrets`
or `hubble-tls` — are treated as workload hints. The distinction is on the name pattern only,
not on the volume type (`kubernetes.io~secret` vs `kubernetes.io~projected`).

### Grouping

Mounts are grouped by pod UID. A pod typically appears multiple times (one entry per mounted
volume). All volume names for a given UID are collected before deriving the display name.

### Display Name Algorithm

Given the set of volume names for a pod UID:

1. Filter to non-generic names (exclude `kube-api-access-*`)
2. Extract the first UUID segment (the portion before the first `-` in the UID)
3. If non-generic names remain:
   - Compute the longest common prefix (LCP) character-by-character across all non-generic names
   - If LCP does not end with `-`, append `-`
   - Append the first UUID segment
   - Result: `{lcp}{first-uuid-segment}`, e.g. `argocd-84cc979b`
4. If all names are generic (or no non-generic names):
   - Use only the first UUID segment, e.g. `84cc979b`

Examples:

| Pod UID (short) | Non-generic volumes (after filtering) | Display name |
|---|---|---|
| `84cc979b` | `argocd-dex-server-tls`, `argocd-repo-server-tls` | `argocd-84cc979b` |
| `1a2d455a` | `argocd-dex-server-tls`, `argocd-repo-server-tls` (`kube-api-access-dml5k` filtered out) | `argocd-1a2d455a` |
| `293aba3c` | `clustermesh-secrets`, `hubble-tls` (no common prefix) | `293aba3c` |
| `430772bd` | *(all generic, none remain after filtering)* | `430772bd` |

### Emitted Entities

For each discovered pod UID:

**Pod entity:**
- `meta.uid` = extracted UUID
- `meta.name` = derived display name
- `meta.namespace` = `"?"` (unknown until a SA token for that pod is directly read)
- All other fields at defaults (no containers, no system info, `is_running` unset)

**Relation:**
- `RunsOn(discovered_pod → node)` — node is inferred the same way `HostPathAnalyzer` does:
  use the observing pod's `node_name` if set, otherwise a placeholder node entity keyed on `"?"`

The observing pod itself is unaffected — no new relations are added to it.

### Namespace Resolution (Out of Scope)

Namespace remains `"?"` until a SA token is read from that pod's path. Propagating namespace
across pods via workload-prefix matching is explicitly out of scope for this iteration.

## Testing

Unit tests on the analyzer function with a synthetic `Vec<Mount>` covering:

- Multiple pods with mixed generic/non-generic volumes
- Varying LCP depth (full prefix match, partial match, no match)
- All-generic-volume pods
- Malformed paths (too few segments, non-UUID pod UID) — verify warning is logged and pod is skipped
- Single volume per pod (non-generic and generic)
