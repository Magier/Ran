use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use k8s::K8sService;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, warn};

use crate::builtin::BuiltinC2;
use crate::types::{C2Event, ExecTtp, TtpExecuted};

use crate::types::BUILTIN_C2_ID;

type Backends = Arc<RwLock<HashMap<String, Arc<dyn C2Backend>>>>;

#[derive(Clone)]
pub struct C2Handle {
    cmd_tx: mpsc::Sender<ExecTtp>,
    backends: Backends,
}

impl C2Handle {
    pub async fn register_backend(&self, id: impl Into<String>, backend: Arc<dyn C2Backend>) {
        self.backends.write().await.insert(id.into(), backend);
    }
}

#[derive(Clone)]
pub struct C2EventBus {
    tx: broadcast::Sender<C2Event>,
}

impl C2EventBus {
    pub fn new(buffer_size: usize) -> Self {
        let (tx, _rx) = broadcast::channel(buffer_size);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<C2Event> {
        self.tx.subscribe()
    }

    pub fn publish(
        &self,
        event: C2Event,
    ) -> Result<usize, Box<broadcast::error::SendError<C2Event>>> {
        self.tx.send(event).map_err(Box::new)
    }
}

impl C2Handle {
    pub async fn send(&self, cmd: ExecTtp) -> Result<(), String> {
        self.cmd_tx
            .send(cmd)
            .await
            .map_err(|_| "failed to send ExecTtp command to c2 runtime".to_string())
    }
}

pub struct C2Manager {
    cmd_rx: mpsc::Receiver<ExecTtp>,
    event_bus: C2EventBus,
    backends: Backends,
    k8s: Option<K8sService>,
}

#[async_trait]
pub trait C2Backend: Send + Sync {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted;
}

#[async_trait]
impl C2Backend for BuiltinC2 {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
        self.execute(cmd).await
    }
}

impl C2Manager {
    pub fn new(buffer_size: usize, k8s: K8sService) -> (C2Handle, C2EventBus, Self) {
        let (cmd_tx, cmd_rx) = mpsc::channel(buffer_size);
        let event_bus = C2EventBus::new(buffer_size);

        let builtin: Arc<dyn C2Backend> = Arc::new(BuiltinC2::new(k8s.clone()));
        let mut map: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        map.insert(BUILTIN_C2_ID.to_string(), builtin.clone());
        map.insert("ran".to_string(), builtin);
        let backends: Backends = Arc::new(RwLock::new(map));

        (
            C2Handle {
                cmd_tx,
                backends: backends.clone(),
            },
            event_bus.clone(),
            Self {
                cmd_rx,
                event_bus,
                backends,
                k8s: Some(k8s),
            },
        )
    }

    #[cfg(test)]
    fn new_with_backends(
        buffer_size: usize,
        backends: HashMap<String, Arc<dyn C2Backend>>,
    ) -> (C2Handle, C2EventBus, Self) {
        let (cmd_tx, cmd_rx) = mpsc::channel(buffer_size);
        let event_bus = C2EventBus::new(buffer_size);
        let backends: Backends = Arc::new(RwLock::new(backends));

        (
            C2Handle {
                cmd_tx,
                backends: backends.clone(),
            },
            event_bus.clone(),
            Self {
                cmd_rx,
                event_bus,
                backends,
                k8s: None,
            },
        )
    }

    pub async fn run(mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            let event = self.execute_command(&cmd).await;
            if self
                .event_bus
                .publish(C2Event::TtpExecuted {
                    cmd: Box::new(cmd),
                    event,
                })
                .is_err()
            {
                debug!("no c2 event subscribers currently registered");
            }
        }
        warn!("c2 command channel closed; stopping c2 manager loop");
    }

    async fn execute_command(&self, cmd: &ExecTtp) -> TtpExecuted {
        let trimmed = cmd.procedure.command.trim_start();

        if trimmed.starts_with("setTarget(") || trimmed == "noop" {
            if args_flag(&cmd.args, "Interactive") {
                if let Some(pod_entity_id) = parse_set_target_pod_entity(trimmed) {
                    let backend_id = kubectl_exec_backend_id(&pod_entity_id, None);
                    let container = args_str(&cmd.args, "Container");
                    let Some(k8s) = self.k8s.clone() else {
                        return TtpExecuted {
                            id: cmd.id.clone(),
                            success: false,
                            results: vec!["no K8s client configured".to_string()],
                            exit_code: 1,
                            fail_reason: "no K8s client configured".to_string(),
                            session_connected: None,
                        };
                    };
                    match open_kubectl_exec_session(
                        self.backends.clone(),
                        k8s,
                        backend_id,
                        pod_entity_id,
                        container,
                    )
                    .await
                    {
                        Ok(session_data) => {
                            return TtpExecuted {
                                id: cmd.id.clone(),
                                success: true,
                                results: vec!["ok".to_string()],
                                exit_code: 0,
                                fail_reason: String::new(),
                                session_connected: Some(session_data),
                            };
                        }
                        Err(e) => {
                            return TtpExecuted {
                                id: cmd.id.clone(),
                                success: false,
                                results: vec![e.clone()],
                                exit_code: 1,
                                fail_reason: e,
                                session_connected: None,
                            };
                        }
                    }
                }
            }
            return TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec!["ok".to_string()],
                exit_code: 0,
                fail_reason: String::new(),
                session_connected: None,
            };
        }

        if let Some((port, protocol)) = parse_session_listen_command(trimmed) {
            let backend_id = session_backend_id_from_cmd(cmd);
            let target_entity_id = cmd
                .args
                .get("TARGET_ID")
                .map(String::as_str)
                .unwrap_or(&cmd.target_id)
                .to_string();
            self.spawn_session_listener(backend_id, target_entity_id, port, protocol);
            return TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec![format!("listener starting on port {}", port)],
                exit_code: 0,
                fail_reason: String::new(),
                session_connected: None,
            };
        }

        if let Some(container) = parse_kubectl_exec_command(trimmed) {
            let target_entity_id = cmd
                .args
                .get("TARGET_ID")
                .map(String::as_str)
                .unwrap_or(&cmd.target_id)
                .to_string();
            let backend_id = kubectl_exec_backend_id(&target_entity_id, container.as_deref());
            self.spawn_kubectl_exec_session(backend_id, target_entity_id, container);
            return TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec!["kubectl exec session opening".to_string()],
                exit_code: 0,
                fail_reason: String::new(),
                session_connected: None,
            };
        }

        // Transparent session upgrade: if the operator root (c2/ran) is the
        // designated backend but we have an open interactive session for the
        // first exec-chain hop, route through that session instead.  This
        // means nsenter-wrapped commands for container escapes travel through
        // the live shell — the session backend_id never surfaces to callers.
        let first_hop = cmd.exec_entity();
        let session_key = kubectl_exec_backend_id(first_hop, None);
        let session_backend = if cmd.exec_system_id.is_empty()
            || cmd.exec_system_id == BUILTIN_C2_ID
        {
            let backends = self.backends.read().await;
            backends.get(&session_key).cloned()
        } else {
            None
        };

        let mut event = if let Some(backend) = session_backend {
            backend.execute(cmd).await
        } else {
            self.select_backend(cmd).await.execute(cmd).await
        };
        event.session_connected = None;
        event
    }

    fn spawn_session_listener(
        &self,
        backend_id: String,
        target_entity_id: String,
        port: u16,
        protocol: String,
    ) {
        let backends = self.backends.clone();
        let event_bus = self.event_bus.clone();
        tokio::spawn(async move {
            accept_session_loop(
                backends,
                event_bus,
                backend_id,
                target_entity_id,
                port,
                protocol,
            )
            .await;
        });
    }

    fn spawn_kubectl_exec_session(
        &self,
        backend_id: String,
        target_entity_id: String,
        container: Option<String>,
    ) {
        let backends = self.backends.clone();
        let event_bus = self.event_bus.clone();
        let Some(k8s) = self.k8s.clone() else {
            tracing::error!("c2.kubectl_exec: no K8s client available");
            return;
        };
        tokio::spawn(async move {
            match open_kubectl_exec_session(backends, k8s, backend_id.clone(), target_entity_id.clone(), container).await {
                Ok(data) => {
                    let _ = event_bus.publish(C2Event::SessionConnected {
                        backend_id: data.backend_id,
                        target_entity_id: data.target_entity_id,
                        hostname: data.hostname,
                        user: data.user,
                        os: data.os,
                        port: None,
                    });
                }
                Err(e) => {
                    tracing::error!(%backend_id, error = %e, "kubectl exec session failed");
                    let _ = event_bus.publish(C2Event::SessionLost { backend_id, target_entity_id });
                }
            }
        });
    }

    async fn select_backend(&self, cmd: &ExecTtp) -> Arc<dyn C2Backend> {
        let key = cmd.exec_system_id.trim().to_ascii_lowercase();
        let backends = self.backends.read().await;

        if key.is_empty() {
            return backends
                .get(BUILTIN_C2_ID)
                .expect("builtin c2 backend must always be registered")
                .clone();
        }

        if let Some(backend) = backends.get(&key) {
            return backend.clone();
        }

        // Accept `c2/<name>` and `<name>` as aliases when looking up backends.
        if let Some(stripped) = key.strip_prefix("c2/") {
            if let Some(backend) = backends.get(stripped) {
                return backend.clone();
            }
        } else {
            let prefixed = format!("c2/{key}");
            if let Some(backend) = backends.get(&prefixed) {
                return backend.clone();
            }
        }

        warn!(
            exec_system_id = %cmd.exec_system_id,
            "c2 backend not found; falling back to builtin c2"
        );

        backends
            .get(BUILTIN_C2_ID)
            .expect("builtin c2 backend must always be registered")
            .clone()
    }
}

/// Look up a boolean arg by name, case-insensitively. Returns true only for
/// the exact string `"true"` (case-insensitive).
fn args_flag(args: &HashMap<String, String>, key: &str) -> bool {
    let key_lower = key.to_ascii_lowercase();
    args.iter()
        .find(|(k, _)| k.to_ascii_lowercase() == key_lower)
        .map(|(_, v)| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Look up a string arg by name, case-insensitively. Returns `None` when
/// the key is absent or the value is empty.
fn args_str(args: &HashMap<String, String>, key: &str) -> Option<String> {
    let key_lower = key.to_ascii_lowercase();
    args.iter()
        .find(|(k, _)| k.to_ascii_lowercase() == key_lower)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

/// Open a kubectl exec session and register it as a backend. Returns the probe
/// data (hostname, user, os) for the caller to embed in `TtpExecuted` so the
/// campaign can process it after TTP effects rather than as a separate event.
async fn open_kubectl_exec_session(
    backends: Backends,
    k8s: K8sService,
    backend_id: String,
    target_entity_id: String,
    container: Option<String>,
) -> Result<crate::types::SessionConnectedData, String> {
    let (ns, pod) = split_pod_entity_id(&target_entity_id).ok_or_else(|| {
        format!(
            "target '{}' is not a pod entity (expected ns/<ns>/pod/<name>)",
            target_entity_id
        )
    })?;
    let (ns, pod) = (ns.to_string(), pod.to_string());

    let stream = k8s
        .open_exec_session(&ns, &pod, container.as_deref())
        .await
        .map_err(|e| format!("kubectl exec open failed for {target_entity_id}: {e}"))?;

    let (rx, tx) = tokio::io::split(stream);
    let session = crate::ShellSession::from_rw(rx, tx, &backend_id);

    if let Err(e) = session.init().await {
        tracing::warn!(%backend_id, error = %e, "kubectl exec session init warning; proceeding");
    }

    let hostname = session.run_raw("hostname").await.unwrap_or_else(|e| {
        tracing::warn!(%backend_id, error = %e, "hostname probe failed");
        pod.clone()
    });
    let user = session.run_raw("whoami").await.unwrap_or_else(|e| {
        tracing::warn!(%backend_id, error = %e, "whoami probe failed");
        String::new()
    });
    let os = session.run_raw("uname").await.unwrap_or_else(|e| {
        tracing::warn!(%backend_id, error = %e, "uname probe failed");
        String::new()
    });

    tracing::info!(%backend_id, %hostname, %user, %os, "kubectl exec session ready");

    backends
        .write()
        .await
        .insert(backend_id.clone(), Arc::new(session));

    Ok(crate::types::SessionConnectedData {
        backend_id,
        target_entity_id,
        hostname,
        user,
        os,
    })
}

/// Parse `setTarget(namespace, pod)` and return the canonical pod entity ID
/// `ns/<namespace>/pod/<pod>`, or `None` when the format doesn't match.
fn parse_set_target_pod_entity(cmd: &str) -> Option<String> {
    let inner = cmd.strip_prefix("setTarget(")?.strip_suffix(')')?;
    let mut parts = inner.splitn(2, ',');
    let namespace = parts.next()?.trim();
    let pod = parts.next()?.trim();
    if namespace.is_empty() || pod.is_empty() {
        return None;
    }
    Some(format!("ns/{}/pod/{}", namespace, pod))
}

/// Parse `c2.kubectl_exec()` or `c2.kubectl_exec(container)` from a procedure
/// command string.  Returns `Some(None)` for no-container form, `Some(Some(name))`
/// when a container name is given, `None` when the command doesn't match.
fn parse_kubectl_exec_command(cmd: &str) -> Option<Option<String>> {
    let inner = cmd.strip_prefix("c2.kubectl_exec(")?.strip_suffix(')')?;
    let container = if inner.trim().is_empty() {
        None
    } else {
        Some(inner.trim().to_string())
    };
    Some(container)
}

/// Derive a deterministic session backend ID for a kubectl exec session.
fn kubectl_exec_backend_id(target_id: &str, container: Option<&str>) -> String {
    let slug = target_id.replace('/', "-");
    match container {
        Some(c) => format!("session/{}-{}", slug, c),
        None => format!("session/{}", slug),
    }
}

/// Parse a pod entity ID in canonical form `ns/<namespace>/pod/<name>` and
/// return `(namespace, pod_name)`, or `None` if the format doesn't match.
fn split_pod_entity_id(entity_id: &str) -> Option<(&str, &str)> {
    let mut parts = entity_id.splitn(5, '/');
    let kind_a = parts.next()?;
    let namespace = parts.next()?;
    let kind_b = parts.next()?;
    let pod_name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if kind_a != "ns" || kind_b != "pod" || namespace.is_empty() || pod_name.is_empty() {
        return None;
    }
    Some((namespace, pod_name))
}

/// Parse `c2.listen(port, protocol)` or `c2.listen(port)` from a procedure
/// command string.  Returns `(port, protocol)` on match.
fn parse_session_listen_command(cmd: &str) -> Option<(u16, String)> {
    let inner = cmd.strip_prefix("c2.listen(")?.strip_suffix(')')?;
    let mut parts = inner.splitn(2, ',');
    let port: u16 = parts.next()?.trim().parse().ok()?;
    let protocol = parts
        .next()
        .map(|p| p.trim().to_string())
        .unwrap_or_else(|| "tcp".to_string());
    Some((port, protocol))
}

/// Derive the session backend ID for a `session.listen` command from the
/// execution context — uses the same deterministic scheme as the effect handler.
fn session_backend_id_from_cmd(cmd: &ExecTtp) -> String {
    let target_id = cmd
        .args
        .get("TARGET_ID")
        .map(String::as_str)
        .unwrap_or(&cmd.target_id);
    let port = cmd
        .args
        .get("PORT")
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(0);
    let slug = target_id.replace('/', "-");
    format!("session/{}-{}", slug, port)
}

async fn accept_session_loop(
    backends: Backends,
    event_bus: C2EventBus,
    backend_id: String,
    target_entity_id: String,
    port: u16,
    protocol: String,
) {
    use crate::ShellSession;
    use std::net::{Ipv4Addr, SocketAddr};

    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(port, error = %e, "failed to bind session listener");
            return;
        }
    };
    tracing::info!(port, %backend_id, "session listener ready");
    let _ = event_bus.publish(C2Event::ListenerStarted {
        port,
        protocol: protocol.clone(),
    });

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tracing::info!(%peer, %backend_id, "incoming shell connection; running init");
                let session = match ShellSession::from_incoming(stream, &backend_id).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(%peer, error = %e, "shell init failed; waiting for next connection");
                        continue;
                    }
                };
                tracing::info!(%peer, %backend_id, "shell init complete; probing hostname/whoami/uname");

                // Probe the shell — mirrors the Go legacy: hostname / whoami / uname.
                let hostname = session.run_raw("hostname").await.unwrap_or_else(|e| {
                    tracing::warn!(%peer, error = %e, "hostname probe failed");
                    "unknown".to_string()
                });
                tracing::info!(%peer, %backend_id, %hostname, "hostname probe done");
                let user = session.run_raw("whoami").await.unwrap_or_else(|e| {
                    tracing::warn!(%peer, error = %e, "whoami probe failed");
                    String::new()
                });
                let os = session.run_raw("uname").await.unwrap_or_else(|e| {
                    tracing::warn!(%peer, error = %e, "uname probe failed");
                    String::new()
                });
                tracing::info!(%peer, %backend_id, %hostname, %user, %os, "probes complete");

                let target_entity_id = format!("node/{}", hostname.to_lowercase());

                backends
                    .write()
                    .await
                    .insert(backend_id.clone(), Arc::new(session));
                let publish_result = event_bus.publish(C2Event::SessionConnected {
                    backend_id: backend_id.clone(),
                    target_entity_id,
                    hostname,
                    user,
                    os,
                    port: Some(port),
                });
                tracing::info!(%backend_id, receivers = ?publish_result, "SessionConnected published");
            }
            Err(e) => {
                tracing::error!(port, error = %e, "accept error on session listener");
                let _ = event_bus.publish(C2Event::SessionLost {
                    backend_id: backend_id.clone(),
                    target_entity_id: target_entity_id.clone(),
                });
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use armory::{Procedure, Ttp};

    use super::{C2Backend, C2Event, C2Manager, ExecTtp, TtpExecuted, BUILTIN_C2_ID};

    struct MockBackend {
        marker: String,
    }

    #[async_trait::async_trait]
    impl C2Backend for MockBackend {
        async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
            TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec![self.marker.clone()],
                exit_code: 0,
                fail_reason: String::new(),
                session_connected: None,
            }
        }
    }

    #[tokio::test]
    async fn unknown_exec_system_id_falls_back_to_builtin_backend() {
        let builtin_backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), builtin_backend.clone());
        backends.insert("ran".to_string(), builtin_backend);
        backends.insert(
            "sliver".to_string(),
            Arc::new(MockBackend {
                marker: "sliver".to_string(),
            }),
        );

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();
        let manager_task = tokio::spawn(manager.run());

        handle
            .send(exec_cmd("c2/does-not-exist"))
            .await
            .expect("send should succeed");

        match rx.recv().await.expect("event should be published") {
            C2Event::TtpExecuted { event, .. } => {
                assert_eq!(event.results, vec!["builtin"]);
                assert!(event.success);
            }
            other => panic!("unexpected event: {:?}", other),
        }

        drop(handle);
        manager_task
            .await
            .expect("manager should shut down cleanly");
    }

    #[tokio::test]
    async fn register_backend_routes_commands_to_it() {
        let builtin_backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), builtin_backend.clone());
        backends.insert("ran".to_string(), builtin_backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();
        let manager_task = tokio::spawn(manager.run());

        // Register a new backend after the manager is already running.
        handle
            .register_backend(
                "session/test-1",
                Arc::new(MockBackend {
                    marker: "shell-session".to_string(),
                }),
            )
            .await;

        handle
            .send(exec_cmd("session/test-1"))
            .await
            .expect("send should succeed");

        match rx.recv().await.expect("event should be published") {
            C2Event::TtpExecuted { event, .. } => {
                assert_eq!(event.results, vec!["shell-session"]);
                assert!(event.success);
            }
            other => panic!("unexpected event: {:?}", other),
        }

        drop(handle);
        manager_task
            .await
            .expect("manager should shut down cleanly");
    }

    #[tokio::test]
    async fn c2_prefixed_backend_key_routes_to_unprefixed_registration() {
        let builtin_backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), builtin_backend.clone());
        backends.insert("ran".to_string(), builtin_backend);
        backends.insert(
            "sliver".to_string(),
            Arc::new(MockBackend {
                marker: "sliver".to_string(),
            }),
        );

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();
        let manager_task = tokio::spawn(manager.run());

        handle
            .send(exec_cmd("c2/sliver"))
            .await
            .expect("send should succeed");

        match rx.recv().await.expect("event should be published") {
            C2Event::TtpExecuted { event, .. } => {
                assert_eq!(event.results, vec!["sliver"]);
                assert!(event.success);
            }
            other => panic!("unexpected event: {:?}", other),
        }

        drop(handle);
        manager_task
            .await
            .expect("manager should shut down cleanly");
    }

    fn exec_cmd(exec_system_id: &str) -> ExecTtp {
        ExecTtp {
            id: "cmd-fallback".to_string(),
            started_at_ms: 0,
            ttp: Ttp {
                id: "T0001".to_string(),
                name: "Test TTP".to_string(),
                description: "test".to_string(),
                tactic: "Execution".to_string(),
                techniques: vec![],
                status: "stable".to_string(),
                params: vec![],
                requires: Default::default(),
                effects: vec![],
                procedures: vec![],
                cleanup: None,
                references: vec![],
            },
            procedure: Procedure {
                id: "proc-1".to_string(),
                command: "id".to_string(),
                tool: None,
                is_local_command: None,
                http_request: None,
                k8s_request: None,
                steps: None,
            },
            args: HashMap::new(),
            target_id: "ns/default/pod/nginx".to_string(),
            exec_chain: vec!["ns/default/pod/nginx".to_string()],
            exec_system_id: exec_system_id.to_string(),
            output_transform: None,
            is_cleanup: false,
        }
    }
}
