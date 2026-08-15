# The Context Model

The campaign maintains a **knowledge graph** — a set of typed entities and the
directed relations between them. This is the "world model" the emulation works
with: what the adversary has found, where they can go, and what they can do.

## Entities

Each entity is a cluster resource with a stable ID. Entity IDs follow a path
convention:

| Entity type    | ID format                           | Example                                  |
| -------------- | ----------------------------------- | ---------------------------------------- |
| Pod            | `ns/<namespace>/pod/<name>`         | `ns/default/pod/nginx-7d5`               |
| ServiceAccount | `ns/<namespace>/sa/<name>`          | `ns/default/sa/ci-deployer`              |
| Node           | `node/<name>`                       | `node/worker-1`                          |
| K8sRole        | `ns/<namespace>/role/<name>`        | `ns/default/role/nsadmin`                |
| K8sRoleBinding | `ns/<namespace>/rolebinding/<name>` | `ns/default/rolebinding/nsadmin-binding` |
| CronJob        | `ns/<namespace>/cronjob/<name>`     | `ns/default/cronjob/cleanup`             |
| C2Server       | `c2/<id>`                           | `c2/ran`                                 |

Entities carry runtime data: a pod knows its node name (if discovered), its
mounted service account (if enumerated), and its current access level. A service
account accumulates RBAC entitlements as they are discovered via
`SelfSubjectRulesReview` or role enumeration.

## Relations

Relations are directed edges in the graph. They record how entities are connected
and, critically, how commands can be routed through the graph.

| Relation           | Direction       | Meaning                                                     |
| ------------------ | --------------- | ----------------------------------------------------------- |
| `runs-on`          | pod → node      | This pod runs on this node                                  |
| `k8s.can-exec`     | actor → pod     | Actor can `kubectl exec` into this pod                      |
| `k8s.can-reach`    | source → target | Source can reach target over the network                    |
| `k8s.kubelet-exec` | pod → node      | Pod can exec commands on the node via the kubelet API       |
| `container.escape` | pod → node      | Pod has a proven container escape path to this node         |
| `rce.can-exec`     | source → target | Source has RCE on target via an exploit chain               |
| `c2.session`       | C2 → target     | An active C2 session exists from this server to this target |

Some relations carry an **envelope** — the command template used to reach the
target. For example, a `container.escape` relation stores the `nsenter` invocation
that breaks out of the container. When you later run techniques "from" the escaped
node, Ran wraps your commands with that envelope automatically.

## The access level

Every entity has an **access level** — a measure of the foothold the adversary
has on it:

| Level  | Meaning                                                        |
| ------ | -------------------------------------------------------------- |
| `None` | Known to exist, but no interactive access                      |
| `Exec` | Can execute arbitrary commands (kubectl exec, RCE, C2 session) |

Access level determines whether techniques that require execution (most techniques
other than pure API calls) are applicable to a given entity.

## How the graph grows

The graph grows in two ways:

1. **Effects** — each TTP declares [effects](effects.md) in its YAML. After a
   successful run, Ran parses the effect strings and applies the resulting entity
   and relation updates.
2. **Output parsers** — Ran parses structured command output (e.g. `kubectl get
pods -o json`) to extract entities even when no explicit effect is declared.

Ran also runs a set of **inference rules** after each update. For example: if a
pod entity and a runs-on relation are both present, and the node has a known
kubelet endpoint, Ran infers a `k8s.kubelet-exec` relation automatically.
