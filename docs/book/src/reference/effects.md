# Effect Catalog

Complete reference for all built-in effect expressions. Effects are declared in
a TTP's `effects:` list and are evaluated after a successful run.

---

## Simple effects (no arguments)

These effects extract entities from the active parameter context.

### `k8s.pod`

Creates a `Pod` entity.

**Required context keys:** `Namespace`, `PodName` (also accepts `PODNAME`, `POD_NAME`)

**Optional context keys:** `NodeName`, `ServiceAccount` / `ServiceAccountName`, `IsRunning`

```yaml
effects:
  - k8s.pod
```

---

### `k8s.serviceaccount`

Creates a `ServiceAccount` entity.

**Required context keys:** `Namespace`, `ServiceAccountName` (also accepts `SA_NAME`)

**Optional context keys:** `Token` — if present, attaches a JWT to the SA

```yaml
effects:
  - k8s.serviceaccount
```

---

### `k8s.role`

Creates a `K8sRole` entity.

**Required context keys:** `Namespace`, `RoleName` (also accepts `ROLE_NAME`)

**Optional context keys:** `Rules` — JSON array of `{verbs, resources, apiGroups}` objects

```yaml
effects:
  - k8s.role
```

---

### `k8s.rolebinding`

Creates a `K8sRoleBinding` entity.

**Required context keys:** `Namespace`, `BindingName` (also accepts `BINDING_NAME`)

**Optional context keys:** `RoleRef`, `Subjects` — JSON array of `{kind, name, namespace}` objects

```yaml
effects:
  - k8s.rolebinding
```

---

### `k8s.cronjob`

Creates a `CronJob` entity.

**Required context keys:** `Namespace`, `CronJobName` (also accepts `CRONJOB_NAME`)

**Optional context keys:** `Schedule` — cron expression string

```yaml
effects:
  - k8s.cronjob
```

---

### List-form effects (output parsers)

These trigger Ran's output parser pipeline, which reads the TTP's raw command
output and extracts structured entities from it. The command output must be a
Kubernetes JSON response.

| Effect | Parser triggered |
|---|---|
| `k8s.podList` | Extract `Pod` entities from `kubectl get pods -o json` |
| `k8s.serviceAccountList` | Extract `ServiceAccount` entities |
| `k8s.nodeList` | Extract `Node` entities |
| `k8s.secretList` | Extract `Secret` metadata |
| `k8s.deploymentList` | Extract `Deployment` entities |
| `k8s.configMapList` | Extract `ConfigMap` metadata |
| `k8s.roleList` | Extract `K8sRole` entities |
| `k8s.roleBindingList` | Extract `K8sRoleBinding` entities |
| `k8s.clusterRoleList` | Extract `K8sClusterRole` entities |
| `k8s.clusterRoleBindingList` | Extract `K8sClusterRoleBinding` entities |
| `k8s.serviceList` | Extract `Service` entities |
| `k8s.ingressList` | Extract `Ingress` entities |
| `k8s.gatewayList` | Extract `Gateway` entities (Gateway API) |
| `k8s.httpRouteList` | Extract `HTTPRoute` entities (Gateway API) |

---

## Relation effects (with arguments)

These create directed edges in the knowledge graph.

### `k8s.can-exec(src, tgt)`

Records that `src` can execute commands inside `tgt` via `kubectl exec`.

```yaml
effects:
  - k8s.can-exec(pod/default/attacker, pod/default/victim)
```

---

### `k8s.can-reach(src, tgt)`

Records a proven network path from `src` to `tgt`.

```yaml
effects:
  - k8s.can-reach(sys, pod/production/database)
```

---

### `runs-on(pod, node)`

Records that a pod runs on a specific node. Also accepted as `k8s.runs-on`.

```yaml
effects:
  - runs-on(pod/default/my-pod, node/worker-1)
```

---

### `k8s.kubelet-exec(src, tgt)` / `k8s.kubelet-exec-source(src, tgt)`

Records that `src` can execute commands on nodes via the kubelet API.
`tgt` may be a specific node ID or the wildcard `all(k8s.node)`.

When the procedure command contains `${CMD}`, Ran stores it as an envelope so
subsequent commands are routed via this path automatically.

```yaml
procedures:
  - key: ran-ws
    command: ran-ws -- ${CMD}

effects:
  - k8s.kubelet-exec(sys, all(k8s.node))
```

---

### `container.escape(src)`

Records a proven container escape from `src` (a pod) to its host node.

- Creates a `K8sNode` entity (or a placeholder if the node name is not yet known)
- Creates a `RunsOn` relation
- Creates a `ContainerEscape` relation storing the escape command as an envelope

`src` accepts `sys` (current target) or an explicit pod entity ID.

```yaml
procedures:
  - key: nsenter
    command: nsenter -t 1 -m -u -i -n -p -- ${CMD}

effects:
  - container.escape(sys)
```

---

### `rce.can-exec(src, tgt)`

Records a remote code execution path from `src` to `tgt` via an exploit chain.
The grounded procedure command is stored as an envelope for command routing.

```yaml
effects:
  - rce.can-exec(sys, pod/target-ns/victim-pod)
```

---

### `c2.session(backend, tgt)`

Records an active C2 session from `backend` to `tgt`.

**`backend` formats:**
- `sliver` — shorthand; resolves to source `c2/sliver`, session `session/sliver`
- `c2/sliver` — explicit namespacing
- `session/sliver-1` — references a named session; source becomes `c2/sliver-1`

**`tgt`:** entity ID or `sys`

```yaml
effects:
  - c2.session(sliver, sys)
```
