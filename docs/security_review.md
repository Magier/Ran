# Security Review - `crates/` (Rust)

**Date**: 2026-04-28  
**Scope**: ~27K LOC across 9 crates (`api`, `app`, `armory`, `c2`, `campaign`, `cli`, `domain`, `graph`, `k8s`)  
**Reviewers**: 3 parallel agents covering API/C2 attack surface, command-execution / data-handling, and crypto/secrets/deps

## Summary

The biggest issue is one trivially-chainable path: **anyone who can `curl 127.0.0.1` can dump every captured credential, write a Python file the server then executes, and drive the kill chain** - because there is no authentication anywhere on the API, MCP, or C2 listener, and credential structs serialize their secrets verbatim. Most of the rest is hygiene around dependencies, logging, and a few injection vectors in `campaign/execution.rs`.

There are **no `unsafe` blocks**, no FFI, no SQL, no archive extraction, and no insecure kubeconfig writes - those classes are clean.

## Effort key

| Code | Duration |
|------|----------|
| S | < 1 day |
| M | 1–3 days |
| L | 1–2 weeks |
| XL | > 2 weeks / needs redesign |

## Critical

### 1 - Zero authentication on REST API and MCP `/mcp`
- **Category**: systemic
- **Effort**: M
- **Refs**: `app/src/lib.rs:659,676`; `api/src/lib.rs:13-76`; `api/src/mcp.rs:805-820`

Loopback bind is the only boundary; bypassable by any local browser tab (DNS rebinding, fetch from any page) or any local user/process. Every state-mutating route is exposed (`execute_action`, `reset_campaign`, `add_parser`, pod-watch).

### 2 - Credential types serialize secrets verbatim
- **Category**: technical
- **Effort**: S
- **Refs**: `domain/identity.rs:7`; `domain/entities.rs:1359-1417`; `api/src/state_conversions.rs:182,195`; `api/src/mcp.rs:124-127,368-378`

`JwToken.raw`, `K8sCredential.{token,cert_data,key_data}`, `GcpAccessToken.access_token` serialize as plaintext through `/api/graph`, `/api/campaign-state`, MCP `get_graph` / `get_campaign_state` / `wait_for_result`. One unauthenticated GET exfiltrates every harvested SA JWT, kubeconfig bearer, client cert/key, and GCP token.

### 3 - `add_parser` MCP tool → operator-host RCE
- **Category**: technical
- **Effort**: S
- **Refs**: `api/src/mcp.rs:486-523`; `app/src/lib.rs:358-468`

Writes attacker-supplied Python to `armory/parsers/{effect_id}.py`; `python3 <script>` runs on the operator's box on the next matching parse. Filename is sanitized, but the body isn't, and existing parsers can be overwritten (no `create_new`). Combined with #1, this is unauthenticated local RCE.

### 4 - C2 listener binds `0.0.0.0` and trusts the first peer that connects
- **Category**: systemic
- **Effort**: L
- **Refs**: `c2/src/executor.rs:268,283`; `c2/src/shell_session.rs:57-83`

`accept_session_loop` registers any TCP peer as the live `ShellSession` with no PSK / TLS / source-IP allow-list. Anyone on the LAN (or via SSRF) can pose as the implant; subsequent commands containing SA tokens flow to the imposter, who can return crafted output that triggers parser-gap RCE.

---

## High

### 5 - SA JWT interpolated into a shell string; token also in `argv`
- **Category**: technical
- **Effort**: M
- **Refs**: `campaign/src/campaign/execution.rs:1289-1292`

```rust
format!(
    r#"ran-ws --url "wss://{}:10250/exec/{}/{}/{}?..." --token {}"#,
    node_host, namespace, pod.meta.name, container, encoded_cmd, token
)
```

Pod name / namespace / node host are also concatenated raw into the shell command, all from parser-extracted (attacker-controlled) cluster output. Token is visible in `/proc/<pid>/cmdline`.

### 6 - `kubectl exec` fallback string-formats `ns`/`name` into a shell command
- **Category**: technical
- **Effort**: S
- **Refs**: `campaign/src/campaign/execution.rs:537-538`

```rust
format!("kubectl exec -n {} {} -- {}", ns, name, procedure.command)
```

No validation of ns/name. A pod whose name contains `--privileged`, backticks, or `--kubeconfig=/tmp/evil` can inject.

### 7 - Parser scripts auto-discovered from `armory_dir.parent()/parsers/` with no signature/hash/allow-list
- **Category**: systemic
- **Effort**: M
- **Refs**: `app/src/lib.rs:358-468,600,651`

Anyone who can drop a file (malicious TTP bundle, over-eager `git pull`, parser-generator webhook) gets RCE on the operator. There is no manifest check, no hash, and no user prompt before execution.

### 8 - Backends keyed by attacker-controllable args
- **Category**: technical
- **Effort**: M
- **Refs**: `c2/src/executor.rs:200-255,313-334`

`backend_id` derives from `cmd.args.TARGET_ID` / `PORT` (caller-supplied). An unauthenticated caller can seed a backend and have it overwritten by the real implant - or vice-versa, hijacking. Backends are never tombstoned on disconnect.

### 9 - Backend fallback masks impostor sessions
- **Category**: technical
- **Effort**: S
- **Refs**: `c2/src/executor.rs:200-224`

If `exec_system_id` doesn't match a registered backend, execution silently falls back to builtin `kubectl exec`. Should be a hard error when an explicit `exec_system_id` was provided.

### 10 - Swagger UI loaded from `cdn.jsdelivr.net` with no SRI hash
- **Category**: technical
- **Effort**: S
- **Refs**: `api/src/api_handlers.rs:51,60-61`

CDN compromise / DNS spoof → script execution in a page running on `127.0.0.1` with full same-origin access to the unauthenticated API.

### 11 - Dev-mode frontend proxy reads body with `to_bytes(body, usize::MAX)`
- **Category**: technical
- **Effort**: S
- **Refs**: `api/src/api_handlers.rs:617-672`

OOM via streamed body. Debug-only, but `cargo run` is the common dev path.

### 12 - Credential structs derive `Debug`
- **Category**: technical
- **Effort**: S
- **Refs**: `domain/identity.rs:7,29`; `domain/entities.rs:1358,1417`; `campaign/src/grounding.rs:150`

Any `tracing::error!(?sa, …)` or panic backtrace will spill JWTs and kubeconfig bearers. `grounding.rs:150` already logs `token_ref = trimmed` unredacted.

---

## Medium

### 13 - TTP output (incl. raw JWTs) emitted to `tracing::info!` and re-broadcast over SSE
- **Category**: systemic
- **Effort**: S–M
- **Refs**: `c2/src/builtin.rs:115-120`; `app/src/lib.rs:957-963`; `campaign/src/output_parsers/iam.rs`

Screenshots or shared terminal sessions leak every captured token. Structured-log exporters would send them to SIEM.

### 14 - `serde_yaml` (RUSTSEC-2024-0320 unmaintained) parses attacker-influenced YAML with no size/depth caps
- **Category**: technical
- **Effort**: S–M
- **Refs**: `armory/src/armory.rs:139-148`; `app/src/config.rs:51-67`; `campaign/src/output_parsers/file.rs:88`

Includes kubeconfig content captured verbatim from target pod stdout. Billion-laughs YAML DoS on the operator.

### 15 - No length cap on pod-exec stdout/stderr reads
- **Category**: technical
- **Effort**: S
- **Refs**: `k8s/src/lib.rs:220,228`; `app/src/lib.rs:438`

`read_to_string` until EOF. A hostile pod can stream gigabytes and OOM the operator. Same for parser-script `wait_with_output`.

### 16 - Kubelet token selection returns *any* matching SA token, not the pod's own
- **Category**: systemic
- **Effort**: M
- **Refs**: `campaign/src/campaign/execution.rs:1272-1340`

A malicious TTP can engineer a `Uses` relation that tricks the runtime into using a high-priv SA token for an unrelated hop. No logging of which SA was selected, no consent prompt.

### 17 - `std::sync::RwLock<Campaign>` held across `.await` in axum handlers
- **Category**: systemic
- **Effort**: M
- **Refs**: `app/src/lib.rs:6,614,763`; `campaign/src/runtime.rs:312`

Risk of deadlock under load and stale RBAC/credential state on poison. The campaign mutex is the security-critical store; `runtime.rs:312` already explicitly handles the `lock poisoned` path.

### 18 - No body-size limit (`DefaultBodyLimit`) on the router
- **Category**: technical
- **Effort**: S
- **Refs**: `api/src/sse.rs:19`; router setup in `api/src/lib.rs`

axum default is 2 MiB for `Json`, but `StreamableHttpService` (MCP) has no documented cap; SSE broadcast buffer is 256 with no per-client backpressure.

### 19 - No rate limit on `execute_action_handler` / MCP tools
- **Category**: technical
- **Effort**: S
- **Refs**: `api/src/api_handlers.rs:239-258`

Abusable as a `kubectl exec` amplifier (cluster pressure, audit-log noise that masks other activity, possible cloud-API IP block).

### 20 - `c2.listen` accepts any u16, including privileged ports
- **Category**: technical
- **Effort**: S
- **Refs**: `c2/src/executor.rs:229-238`

If the operator runs as root or with `CAP_NET_BIND_SERVICE`, a TTP can hijack ports < 1024.

### 21 - Mixed TLS stacks: `native-tls` (OpenSSL) and `rustls` both pulled in
- **Category**: systemic
- **Effort**: S
- **Refs**: `app/Cargo.toml:19`; `api/Cargo.toml:15`; `Cargo.lock:2413`

`reqwest 0.13.2` uses default `native-tls`; `kube`/`cli` use `rustls`. Doubles attack surface and binary size.

### 22 - `Mutex<Backends>` first-write-wins on `backend_id`
- **Category**: technical
- **Effort**: M
- **Refs**: `c2/src/executor.rs:14,313`

Second TCP connection at the same `backend_id` silently replaces a healthy session; no liveness check before overwrite.

### 23 - `start_pod_watch` namespace not cross-checked against `cfg.namespace_filter`
- **Category**: technical
- **Effort**: S
- **Refs**: `api/src/api_handlers.rs:506-523`; `app/src/lib.rs:585`

Operator's intended scope is bypassable - any namespace the kubeconfig can see is watchable.

### 24 - C2 hostname/user/os from peer logged and SSE-broadcast unescaped
- **Category**: technical
- **Effort**: S
- **Refs**: `c2/src/executor.rs:285,300,309,317-324`

If the frontend ever interpolates these into `innerHTML`, this is XSS into the operator's UI. Bytes come from a raw TCP peer with no character-class validation.

---

## Low

### 25 - No constant-time comparison anywhere (`subtle` crate absent)
- **Category**: technical / **Effort**: S

Preemptive flag: when auth is added (#1), the new token check must use `subtle::ConstantTimeEq`, not `==`.

### 26 - Two `sha2` major versions coexisting (`0.10.9` and `0.11.0`)
- **Category**: systemic / **Effort**: S
- **Refs**: `Cargo.lock:2861,2872`; `campaign/Cargo.toml:16`

### 27 - `kube = "3.1"` / `k8s-openapi = "0.27"` lag the latest line; 3.x receives no security fixes
- **Category**: technical / **Effort**: S
- **Refs**: `k8s/Cargo.toml`

### 28 - Workspace `Cargo.toml` shares almost no dependency pins
- **Category**: systemic / **Effort**: S
- **Refs**: `/Cargo.toml`

Only `serde` and `chrono` are shared; each crate independently picks `tokio`, `axum`, `reqwest`, `tracing`. One forgotten bump = silent split on security-critical deps.

### 29 - `RAN_PARSER_GENERATOR_WEBHOOK` response has no body-size cap; URL query/fragment logged unredacted
- **Category**: technical / **Effort**: S
- **Refs**: `app/src/lib.rs:340-342,502-516`

### 30 - `is_loopback_url` is host-string-only
- **Category**: technical / **Effort**: S
- **Refs**: `app/src/lib.rs:565-570`

`0.0.0.0`, `[::]`, `127.0.0.2`, `localhost.<attacker>.com`, and `/etc/hosts` overrides all bypass the check. Switch to `IpAddr::is_loopback()` after parsing.

### 31 - Panic paths on `Armory::load` and `current_dir()`
- **Category**: technical / **Effort**: S
- **Refs**: `armory/src/armory.rs:163`; `app/src/lib.rs:702-708`

`expect("file listed but not found")` panics during armory load; `unwrap_or_default` on `getcwd()` silently sets the armory path to CWD-relative root.

### 32 - `tool_wait_for_result` polls every 500 ms with no concurrent-wait cap
- **Category**: technical / **Effort**: S
- **Refs**: `api/src/mcp.rs:344-389`

Combined with #1, cheap operator-side DoS. Should be driven off `CampaignEventBus` instead of polling.

### 33 - `connect_bind` shell client is `pub` with no allow-list
- **Category**: technical / **Effort**: S
- **Refs**: `c2/src/shell_session.rs:42-53`

Full SSRF + shell handshake if ever exposed via an API/MCP tool. Tighten to `pub(crate)` until intentionally exposed with auth.

### 34 - Parser `detail` fields include captured-stdout fragments that propagate to SSE → frontend
- **Category**: technical / **Effort**: S
- **Refs**: `campaign/src/output_parsers/{iam,gcp,file}.rs`

Needs an audit pass on every `ParserOutput::*Failure(detail)` and `SuccessWithFacts(_, detail)` site.

### 35 - SSE `armory-loaded` payload sent on connect with no redaction step
- **Category**: technical / **Effort**: S
- **Refs**: `api/src/sse.rs:42`

Current TTP catalog contains no secrets, but the contract isn't enforced. Any future credential snapshot pushed via `publish_sse_event` would broadcast unconditionally.

---

## Items confirmed clean

- No `unsafe` blocks, no FFI in any target crate.
- `kube-rs` context is read-only; nothing writes or switches `KUBECONFIG`.
- No SQL / NoSQL (state lives in memory + filesystem).
- No archive (tar/zip) extraction anywhere.
- No `git = "..."` dependencies without rev pinning (no supply-chain gap).
- No Sentry / OTLP exporter (only `tracing-subscriber` fmt).
- `add_parser` filename sanitization correctly blocks path traversal (alphanumeric + `._-`, leading-dot rejected, `/` → `_`).
- `file_content_handler` reads from an in-memory `HashMap`, not the filesystem.
- `is_loopback_url` is applied to the parser-gap webhook.
- No insecure credential writes; `kube-rs` respects file mode on kubeconfig reads.
- `yash-syntax` AST grounding handles literal first-words structurally without injection.
- No tempfile creation in any reviewed crate.

---

## Recommended fix order

### Week 1 - Plug the unauthenticated chain (Critical #1 + #2 + #3, total ~1–2 days)

1. **Auth middleware**: generate a random token at startup, write to a 0600 file, require as `Authorization: Bearer <token>` on every request. Compare with `subtle::ConstantTimeEq`.
2. **Credential redaction**: implement custom `Serialize` for `JwToken` / `K8sCredential` / `GcpAccessToken` that emits `"<redacted>"` (or a fingerprint). Opaque `Debug` on the same types.
3. **`add_parser` hardening**: refuse overwrite (`OpenOptions::create_new(true)`), sandbox the interpreter (seccomp / separate uid), or remove from MCP toolset entirely (the parser-gap webhook already provides a safer path).

### Week 1–2 - Close the C2 hijack window (Critical #4 + High #8 + Medium #22, ~M)

Bind to a configurable address (default loopback); require a PSK handshake before registering a backend; derive `backend_id` server-side from a UUID at spawn; tombstone on disconnect.

### Week 2 - Stop token bleeding (High #12 + Medium #13, ~S–M)

Opaque `Debug` on credential structs; `redact_secrets()` helper at every TTP-output log site; audit parser `detail` fields (#34).

### Week 2–3 - Fix shell injection vectors (High #5 + #6, ~M)

Pass token via env var or stdin to `ran-ws`; validate ns/pod/node names against a strict DNS-label regex before string formatting.

### Ongoing hygiene (~S each)

- Cap YAML input size before `serde_yaml::from_str`; migrate to `serde_yml` (#14)
- Bound stdout/stderr reads with `take(MAX_BYTES)` (#15)
- `default-features = false, features = ["json","rustls-tls"]` on `reqwest` to drop OpenSSL (#21)
- Bump `kube` to 4.x (#27)
- Promote security-critical deps to `[workspace.dependencies]` (#28)
- Switch `is_loopback_url` to `IpAddr::is_loopback()` (#30)
- Add `subtle` crate, use `ConstantTimeEq` in auth check (#25)

**Total estimated remediation effort - Critical + High**: ~2–3 weeks for one engineer. Systemic items (#7 parser execution model, #16 kubelet-token scoping, #17 lock model) warrant a short design review before patching.
