# k8s_request Procedure Type - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce a `k8s_request` procedure type that encodes Kubernetes API namespace-scoping as a struct field, eliminating Jinja2 `{% if ALL_NS %}` URL branching from TTP YAML files.

**Architecture:** `k8s_request` is a compile-time sugar layer that sits above `http_request`. After template variable grounding, a new `materialize_k8s_request` pass converts `k8s_request` → `procedure.command` (via the existing `build_http_command`). Tool adapters (curl/wget) never see `k8s_request` - they only see the final shell command. Adding a new HTTP tool only requires changing `build_http_command`; `k8s_request` gets it for free.

**Tech Stack:** Rust (serde, serde_json, serde_yaml), YAML TTP definitions in `armory/ttps/`.

---

## Files

| File | Change |
|---|---|
| `crates/armory/src/model.rs` | Add `k8s_request: Option<JsonValue>` to `Procedure` |
| `crates/armory/src/raw.rs` | Add `k8s_request` to `RawProcedure`; thread through `into_ttp`; extend empty-check filter |
| `crates/campaign/src/campaign/execution.rs` | Add `KubernetesRequestSpec`, `build_k8s_url`, `materialize_k8s_request`; ground `k8s_request` in `ground_procedure_and_effects`; wire into pipeline |
| `armory/ttps/Discovery/get_serviceaccounts.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_pods.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_services.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_deployments.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_ingresses.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_gateways.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_httproutes.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_rolebindings.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_nodes.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_clusterroles.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/Discovery/get_clusterrolebindings.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/CredentialAccess/list_k8s_secrets.yaml` | Replace `http_request` with `k8s_request` |
| `armory/ttps/CredentialAccess/read_configmap.yaml` | Replace `http_request` with `k8s_request` |

**Out of scope** (stay as `http_request`): `get_roles.yaml` (three-way CLUSTER_ROLE branch selects a different resource, not just scope), `check_sa_token_permissions.yaml` (POST with body), `get_pods_via_kubelet.yaml` / `get_pods_via_node_proxy.yaml` (non-API-server endpoints), GCP TTPs, `exploit-oops.yaml`.

---

## k8s_request YAML shape (reference)

```yaml
- key: k8s-request
  k8s_request:
    api_server: ${API_SERVER}       # grounded from context before materialization
    api: /api/v1                    # API group path, no trailing slash
    resource: pods                  # resource name
    namespace: ${NS}                # optional; empty string → cluster-scoped URL
    cluster_scoped: ${ALL_NS}       # bool override; true → cluster-scoped regardless of namespace
    query: limit=500                # optional query string, no leading '?'
    token: ${TOKEN}                 # optional; produces Authorization: Bearer header
    use_ca: false                   # passed through to build_http_command
    ca_path: ""                     # optional CA cert path
    timeout_seconds: 30             # optional
```

URL construction rule (implemented in `build_k8s_url`):
- `cluster_scoped == true` OR `namespace` is blank → `{api_server}{api}/{resource}?{query}`
- otherwise → `{api_server}{api}/namespaces/{namespace}/{resource}?{query}`

---

## Task 1: Add `k8s_request` field to the Procedure model

**Files:**
- Modify: `crates/armory/src/model.rs`
- Modify: `crates/armory/src/raw.rs`
- Test: `crates/armory/src/raw.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)]` block at the bottom of `crates/armory/src/raw.rs`:

```rust
#[test]
fn k8s_request_procedure_is_preserved_through_into_ttp() {
    let yaml = r#"
name: Get Pods
tactic: Discovery
procedures:
  - key: k8s-request
    k8s_request:
      api_server: https://10.0.0.1:6443
      api: /api/v1
      resource: pods
      namespace: default
      cluster_scoped: "false"
      query: limit=500
      token: mytoken
      use_ca: false
"#;
    let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
    let ttp = raw
        .into_ttp(Path::new("Discovery/get_pods.yaml"))
        .unwrap();
    assert_eq!(ttp.procedures.len(), 1);
    let proc = &ttp.procedures[0];
    assert_eq!(proc.id, "k8s-request");
    assert!(proc.k8s_request.is_some(), "k8s_request should be preserved");
    assert!(proc.http_request.is_none());
    assert!(proc.command.is_empty());
}

#[test]
fn k8s_request_procedure_without_key_gets_positional_id() {
    let yaml = r#"
name: Get Pods
tactic: Discovery
procedures:
  - k8s_request:
      api: /api/v1
      resource: pods
"#;
    let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
    let ttp = raw
        .into_ttp(Path::new("Discovery/get_pods.yaml"))
        .unwrap();
    assert_eq!(ttp.procedures.len(), 1);
    assert_eq!(ttp.procedures[0].id, "proc-1");
    assert!(ttp.procedures[0].k8s_request.is_some());
}

#[test]
fn procedure_with_only_k8s_request_is_not_filtered_out() {
    // Regression: the empty-check filter must treat k8s_request as non-empty
    let yaml = r#"
name: Test
tactic: Discovery
procedures:
  - k8s_request:
      api: /api/v1
      resource: nodes
"#;
    let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
    let ttp = raw.into_ttp(Path::new("Discovery/test.yaml")).unwrap();
    assert_eq!(ttp.procedures.len(), 1);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test -p armory k8s_request 2>&1 | tail -20
```

Expected: compile error - field `k8s_request` does not exist on `Procedure` / `RawProcedure`.

- [ ] **Step 3: Add `k8s_request` to `Procedure` in `model.rs`**

In `crates/armory/src/model.rs`, add one field after `http_request`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Procedure {
    pub id: String,
    #[serde(default)]
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(rename = "isLocalCommand", skip_serializing_if = "Option::is_none")]
    pub is_local_command: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_request: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k8s_request: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<JsonValue>,
}
```

- [ ] **Step 4: Add `k8s_request` to `RawProcedure` in `raw.rs`**

In `crates/armory/src/raw.rs`, add one field to `RawProcedure`:

```rust
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawProcedure {
    id: Option<String>,
    key: Option<String>,
    command: String,
    tool: Option<String>,
    #[serde(alias = "isLocal", alias = "isLocalCommand")]
    is_local: Option<bool>,
    http_request: Option<JsonValue>,
    k8s_request: Option<JsonValue>,
    steps: Option<JsonValue>,
}
```

- [ ] **Step 5: Thread `k8s_request` through `into_ttp` in `raw.rs`**

There are two places in `into_ttp` that construct a `Procedure` - one for the `procedures` vec and one for `cleanup`. Both need the same change.

For the **procedures** mapping (find `filter_map(|(idx, p)| {`):

```rust
let mut procedures: Vec<Procedure> = self
    .procedures
    .into_iter()
    .enumerate()
    .filter_map(|(idx, p)| {
        if p.command.trim().is_empty()
            && p.http_request.is_none()
            && p.k8s_request.is_none()
            && p.steps.is_none()
        {
            return None;
        }
        let id = p
            .id
            .or(p.key.clone())
            .unwrap_or_else(|| format!("proc-{}", idx + 1));
        Some(Procedure {
            id,
            command: p.command,
            tool: p.tool.or(p.key),
            is_local_command: p.is_local,
            http_request: p.http_request,
            k8s_request: p.k8s_request,
            steps: p.steps,
        })
    })
    .collect();
```

For the **cleanup** block (find `self.cleanup.and_then(|p| {`):

```rust
let cleanup = self.cleanup.and_then(|p| {
    if p.command.trim().is_empty()
        && p.http_request.is_none()
        && p.k8s_request.is_none()
        && p.steps.is_none()
    {
        return None;
    }
    let id = p
        .id
        .or(p.key.clone())
        .unwrap_or_else(|| "cleanup".to_string());
    Some(Procedure {
        id,
        command: p.command,
        tool: p.tool.or(p.key),
        is_local_command: p.is_local,
        http_request: p.http_request,
        k8s_request: p.k8s_request,
        steps: p.steps,
    })
});
```

Also fix every place in the codebase that constructs a `Procedure` literal - they all need `k8s_request: None` added. Find them:

```bash
grep -rn "Procedure {" crates/ --include="*.rs" | grep -v "target/"
```

Add `k8s_request: None,` to each struct literal that doesn't already have it.

- [ ] **Step 6: Run tests to confirm they pass**

```bash
cargo test -p armory k8s_request 2>&1 | tail -20
```

Expected: all three new tests pass; existing tests unaffected.

- [ ] **Step 7: Confirm full build is clean**

```bash
cargo build 2>&1 | grep -E "^error" | head -20
```

Expected: no errors.

- [ ] **Step 8: Commit**

```bash
git add crates/armory/src/model.rs crates/armory/src/raw.rs
git add $(grep -rl "k8s_request: None" crates/ --include="*.rs" | grep -v target)
git commit -m "feat(armory): add k8s_request field to Procedure model"
```

---

## Task 2: Materialization - `materialize_k8s_request`

**Files:**
- Modify: `crates/campaign/src/campaign/execution.rs`
- Test: `crates/campaign/src/campaign/tests.rs`

- [ ] **Step 1: Write the failing tests**

Add to `crates/campaign/src/campaign/tests.rs` (find the existing test module and add after the last test):

```rust
#[test]
fn materialize_k8s_request_namespaced_url() {
    let mut procedure = Procedure {
        id: "k8s-request".to_string(),
        command: String::new(),
        tool: None,
        is_local_command: None,
        http_request: None,
        k8s_request: Some(serde_json::json!({
            "api_server": "https://10.0.0.1:6443",
            "api": "/api/v1",
            "resource": "pods",
            "namespace": "default",
            "cluster_scoped": "false",
            "query": "limit=500",
            "token": "mytoken",
            "use_ca": false
        })),
        steps: None,
    };
    materialize_k8s_request(&mut procedure).unwrap();
    assert!(
        procedure.command.contains("10.0.0.1:6443/api/v1/namespaces/default/pods?limit=500"),
        "command was: {}",
        procedure.command
    );
    assert!(procedure.command.contains("Bearer mytoken"));
    assert!(procedure.k8s_request.is_none(), "k8s_request should be consumed");
}

#[test]
fn materialize_k8s_request_cluster_scoped_when_flag_true() {
    let mut procedure = Procedure {
        id: "k8s-request".to_string(),
        command: String::new(),
        tool: None,
        is_local_command: None,
        http_request: None,
        k8s_request: Some(serde_json::json!({
            "api_server": "https://10.0.0.1:6443",
            "api": "/api/v1",
            "resource": "pods",
            "namespace": "default",
            "cluster_scoped": "true",
            "query": "limit=500",
            "token": "tok",
            "use_ca": false
        })),
        steps: None,
    };
    materialize_k8s_request(&mut procedure).unwrap();
    assert!(
        procedure.command.contains("10.0.0.1:6443/api/v1/pods?limit=500"),
        "command was: {}",
        procedure.command
    );
    assert!(
        !procedure.command.contains("namespaces"),
        "cluster-scoped URL must not contain 'namespaces', got: {}",
        procedure.command
    );
}

#[test]
fn materialize_k8s_request_cluster_scoped_when_namespace_empty() {
    let mut procedure = Procedure {
        id: "k8s-request".to_string(),
        command: String::new(),
        tool: None,
        is_local_command: None,
        http_request: None,
        k8s_request: Some(serde_json::json!({
            "api_server": "https://10.0.0.1:6443",
            "api": "/apis/rbac.authorization.k8s.io/v1",
            "resource": "clusterroles",
            "token": "tok",
            "use_ca": false
        })),
        steps: None,
    };
    materialize_k8s_request(&mut procedure).unwrap();
    assert!(
        procedure.command.contains(
            "10.0.0.1:6443/apis/rbac.authorization.k8s.io/v1/clusterroles"
        ),
        "command was: {}",
        procedure.command
    );
    assert!(!procedure.command.contains("namespaces"));
}

#[test]
fn materialize_k8s_request_no_token_no_auth_header() {
    let mut procedure = Procedure {
        id: "k8s-request".to_string(),
        command: String::new(),
        tool: None,
        is_local_command: None,
        http_request: None,
        k8s_request: Some(serde_json::json!({
            "api_server": "https://10.0.0.1:6443",
            "api": "/api/v1",
            "resource": "nodes",
            "use_ca": false
        })),
        steps: None,
    };
    materialize_k8s_request(&mut procedure).unwrap();
    assert!(
        !procedure.command.contains("Authorization"),
        "empty token must not produce Authorization header, got: {}",
        procedure.command
    );
}

#[test]
fn materialize_k8s_request_noop_when_field_absent() {
    let mut procedure = Procedure {
        id: "kubectl".to_string(),
        command: "kubectl get pods".to_string(),
        tool: None,
        is_local_command: None,
        http_request: None,
        k8s_request: None,
        steps: None,
    };
    materialize_k8s_request(&mut procedure).unwrap();
    assert_eq!(procedure.command, "kubectl get pods");
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test -p campaign materialize_k8s_request 2>&1 | tail -20
```

Expected: compile error - `materialize_k8s_request` not found; `k8s_request` field not on `Procedure`.

- [ ] **Step 3: Add `KubernetesRequestSpec` and `build_k8s_url` to `execution.rs`**

Add after the existing `HttpRequestSpec` struct (around line 151):

```rust
#[derive(Debug, Deserialize)]
struct KubernetesRequestSpec {
    api_server: String,
    api: String,
    resource: String,
    #[serde(default)]
    namespace: String,
    #[serde(default)]
    cluster_scoped: BoolOrString,
    #[serde(default)]
    query: String,
    #[serde(default)]
    token: String,
    #[serde(default = "default_http_method")]
    method: String,
    #[serde(default)]
    use_ca: BoolOrString,
    #[serde(default)]
    ca_path: String,
    #[serde(default = "default_timeout_seconds")]
    timeout_seconds: u64,
}

fn build_k8s_url(spec: &KubernetesRequestSpec) -> String {
    let api_server = spec.api_server.trim_end_matches('/');
    let api = spec.api.trim_end_matches('/');
    let base = format!("{}{}", api_server, api);

    let resource_path = if spec.cluster_scoped.is_true() || spec.namespace.trim().is_empty() {
        format!("{}/{}", base, spec.resource)
    } else {
        format!("{}/namespaces/{}/{}", base, spec.namespace.trim(), spec.resource)
    };

    if spec.query.trim().is_empty() {
        resource_path
    } else {
        format!("{}?{}", resource_path, spec.query.trim())
    }
}
```

- [ ] **Step 4: Add `materialize_k8s_request` to `execution.rs`**

Add right before the existing `materialize_abstract_http_request` function (around line 347):

```rust
fn materialize_k8s_request(procedure: &mut Procedure) -> Result<(), ExecuteActionError> {
    let k8s_req_val = match procedure.k8s_request.take() {
        Some(v) => v,
        None => return Ok(()),
    };

    let spec: KubernetesRequestSpec = serde_json::from_value(k8s_req_val).map_err(|e| {
        ExecuteActionError::InvalidInput(format!(
            "invalid k8s_request in procedure '{}': {}",
            procedure.id, e
        ))
    })?;

    let url = build_k8s_url(&spec);

    let mut headers = HashMap::new();
    headers.insert("Accept".to_string(), "application/json".to_string());
    if !spec.token.trim().is_empty() {
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", spec.token.trim()),
        );
    }

    let method = if spec.method.trim().is_empty() {
        "GET".to_string()
    } else {
        spec.method.trim().to_string()
    };

    procedure.command = build_http_command(
        &method,
        &url,
        &headers,
        "",
        spec.timeout_seconds,
        &spec.use_ca,
        &spec.ca_path,
        None,
        false,
    );

    Ok(())
}
```

- [ ] **Step 5: Ground `k8s_request` in `ground_procedure_and_effects`**

In `ground_procedure_and_effects` (around line 91), add one block after the `http_request` grounding:

```rust
    procedure.command = ground_template(&procedure.command, args);
    if let Some(http_req) = procedure.http_request.as_mut() {
        ground_json_value(http_req, args);
    }
    if let Some(k8s_req) = procedure.k8s_request.as_mut() {
        ground_json_value(k8s_req, args);
    }
    if let Some(steps) = procedure.steps.as_mut() {
        ground_json_value(steps, args);
    }
```

- [ ] **Step 6: Wire `materialize_k8s_request` into the pipeline**

In `prepare_action_with_ttp` (around line 516), add one line between grounding and `materialize_steps`:

```rust
        ground_procedure_and_effects(&mut procedure, &mut ttp.effects, &mut args, &ttp.id);
        materialize_k8s_request(&mut procedure)?;
        materialize_steps(&mut procedure)?;
        materialize_abstract_http_request(&mut procedure)?;
```

- [ ] **Step 7: Run the new tests**

```bash
cargo test -p campaign materialize_k8s_request 2>&1 | tail -30
```

Expected: all 5 new tests pass.

- [ ] **Step 8: Run the full test suite**

```bash
cargo test 2>&1 | tail -20
```

Expected: no regressions.

- [ ] **Step 9: Commit**

```bash
git add crates/campaign/src/campaign/execution.rs crates/campaign/src/campaign/tests.rs
git commit -m "feat(campaign): add materialize_k8s_request - lowers k8s_request to shell command via build_http_command"
```

---

## Task 3: Migrate TTPs with `ALL_NS` branching (10 files)

These TTPs have `{% if ALL_NS %}` / `{% else %}` in their URL. Replace the `http_request` procedure with `k8s_request`. The `ALL_NS` parameter stays - it is now passed as `cluster_scoped: ${ALL_NS}`.

**Files:**
- Modify: all 10 YAML files listed below

- [ ] **Step 1: Replace `http_request` in `armory/ttps/Discovery/get_serviceaccounts.yaml`**

Replace the entire `- http_request:` procedure block with:

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /api/v1
      resource: serviceaccounts
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 2: Replace `http_request` in `armory/ttps/Discovery/get_pods.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /api/v1
      resource: pods
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      ca_path: /var/run/secrets/kubernetes.io/serviceaccount/ca.crt
```

- [ ] **Step 3: Replace `http_request` in `armory/ttps/Discovery/get_services.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /api/v1
      resource: services
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 4: Replace `http_request` in `armory/ttps/Discovery/get_deployments.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /apis/apps/v1
      resource: deployments
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      ca_path: /var/run/secrets/kubernetes.io/serviceaccount/ca.crt
```

- [ ] **Step 5: Replace `http_request` in `armory/ttps/Discovery/get_ingresses.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /apis/networking.k8s.io/v1
      resource: ingresses
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 6: Replace `http_request` in `armory/ttps/Discovery/get_gateways.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /apis/gateway.networking.k8s.io/v1
      resource: gateways
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 7: Replace `http_request` in `armory/ttps/Discovery/get_httproutes.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /apis/gateway.networking.k8s.io/v1
      resource: httproutes
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 8: Replace `http_request` in `armory/ttps/Discovery/get_rolebindings.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /apis/rbac.authorization.k8s.io/v1
      resource: rolebindings
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 9: Replace `http_request` in `armory/ttps/CredentialAccess/list_k8s_secrets.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /api/v1
      resource: secrets
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 10: Replace `http_request` in `armory/ttps/CredentialAccess/read_configmap.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /api/v1
      resource: configmaps
      namespace: ${NS}
      cluster_scoped: ${ALL_NS}
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 11: Verify armory loads all 10 files without parse errors**

```bash
cargo test -p armory 2>&1 | tail -20
```

Expected: all tests pass; no YAML parse errors in the armory loader.

- [ ] **Step 12: Verify no Jinja2 branching remains in these 10 files**

```bash
grep -l "{% if ALL_NS" \
  armory/ttps/Discovery/get_serviceaccounts.yaml \
  armory/ttps/Discovery/get_pods.yaml \
  armory/ttps/Discovery/get_services.yaml \
  armory/ttps/Discovery/get_deployments.yaml \
  armory/ttps/Discovery/get_ingresses.yaml \
  armory/ttps/Discovery/get_gateways.yaml \
  armory/ttps/Discovery/get_httproutes.yaml \
  armory/ttps/Discovery/get_rolebindings.yaml \
  armory/ttps/CredentialAccess/list_k8s_secrets.yaml \
  armory/ttps/CredentialAccess/read_configmap.yaml
```

Expected: no output (none of those files contain `{% if ALL_NS` anymore).

- [ ] **Step 13: Commit**

```bash
git add armory/ttps/Discovery/get_serviceaccounts.yaml \
        armory/ttps/Discovery/get_pods.yaml \
        armory/ttps/Discovery/get_services.yaml \
        armory/ttps/Discovery/get_deployments.yaml \
        armory/ttps/Discovery/get_ingresses.yaml \
        armory/ttps/Discovery/get_gateways.yaml \
        armory/ttps/Discovery/get_httproutes.yaml \
        armory/ttps/Discovery/get_rolebindings.yaml \
        armory/ttps/CredentialAccess/list_k8s_secrets.yaml \
        armory/ttps/CredentialAccess/read_configmap.yaml
git commit -m "feat(ttps): migrate namespace-scoped Discovery and CredentialAccess TTPs to k8s_request"
```

---

## Task 4: Migrate always-cluster-scoped TTPs (3 files)

These TTPs have no `ALL_NS` parameter - their URL is always cluster-scoped. `namespace` and `cluster_scoped` are omitted from the `k8s_request` block (absent namespace defaults to cluster-scoped in `build_k8s_url`).

**Files:**
- Modify: `get_nodes.yaml`, `get_clusterroles.yaml`, `get_clusterrolebindings.yaml`

- [ ] **Step 1: Replace `http_request` in `armory/ttps/Discovery/get_nodes.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /api/v1
      resource: nodes
      query: limit=500
      token: ${TOKEN}
      ca_path: /var/run/secrets/kubernetes.io/serviceaccount/ca.crt
```

- [ ] **Step 2: Replace `http_request` in `armory/ttps/Discovery/get_clusterroles.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /apis/rbac.authorization.k8s.io/v1
      resource: clusterroles
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 3: Replace `http_request` in `armory/ttps/Discovery/get_clusterrolebindings.yaml`**

```yaml
  - key: k8s-request
    k8s_request:
      api_server: ${API_SERVER}
      api: /apis/rbac.authorization.k8s.io/v1
      resource: clusterrolebindings
      query: limit=500
      token: ${TOKEN}
      use_ca: false
```

- [ ] **Step 4: Run full test suite**

```bash
cargo test 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 5: Verify no remaining Jinja2 branching on the migrated files**

```bash
grep -rn "{% if " armory/ttps/Discovery/ armory/ttps/CredentialAccess/
```

Expected output - only these files still contain Jinja2 (they are out of scope and correct):
- `Discovery/get_roles.yaml` - CLUSTER_ROLE three-way branch
- `Discovery/check_sa_token_permissions.yaml` - none (already no branching)

- [ ] **Step 6: Commit**

```bash
git add armory/ttps/Discovery/get_nodes.yaml \
        armory/ttps/Discovery/get_clusterroles.yaml \
        armory/ttps/Discovery/get_clusterrolebindings.yaml
git commit -m "feat(ttps): migrate always-cluster-scoped Discovery TTPs to k8s_request"
```
