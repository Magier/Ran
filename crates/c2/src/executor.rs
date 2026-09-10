use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use k8s::{Client, PodExecOutput};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, warn};

use crate::builtin::BuiltinC2;
use crate::types::{C2Event, ExecTtp, TtpExecuted};

use crate::types::BUILTIN_C2_ID;

type Backends = Arc<RwLock<HashMap<String, Arc<dyn C2Backend>>>>;
/// Abort handles for the accept loops of currently bound listeners, keyed by
/// port. This is what makes a listener stoppable: the `TcpListener` lives
/// inside its task, so releasing the port means dropping that task.
type Listeners = Arc<RwLock<HashMap<u16, tokio::task::AbortHandle>>>;

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
    executor: C2Executor,
}

#[derive(Clone)]
struct C2Executor {
    event_bus: C2EventBus,
    backends: Backends,
    /// Default Kubernetes client (the kubeconfig's current context).
    k8s: Option<Client>,
    /// Per-identity Kubernetes clients keyed by K8sCredential entity id
    /// (`k8s/credential/<slug>`). Populated from every context in the local
    /// kubeconfig so that "Authenticate As" a non-current context actually
    /// authenticates as that identity instead of silently using the default.
    k8s_clients: Arc<HashMap<String, Client>>,
    /// Accept loops of the listeners bound by `c2.listen`, so `c2.stop-listener`
    /// can release their ports.
    listeners: Listeners,
}

impl C2Executor {
    /// Select the Kubernetes client for an action's authentication identity,
    /// falling back to the default (current-context) client when the identity
    /// has no dedicated client (e.g. a discovered credential).
    fn client_for(&self, auth_identity_id: Option<&str>) -> Option<&Client> {
        auth_identity_id
            .and_then(|id| self.k8s_clients.get(id))
            .or(self.k8s.as_ref())
    }
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

/// Fallback backend registered when startup could not build a live Kubernetes
/// client. Every command fails with a clear reason so the operator can tell
/// the difference between "action ran and failed" and "action never had a
/// chance to run because there's no cluster connection".
struct NoClientBackend;

#[async_trait]
impl C2Backend for NoClientBackend {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
        failed_result(
            cmd,
            "no active Kubernetes client configured — read the local kubeconfig or restart with --kubeconfig",
        )
    }
}

impl C2Manager {
    pub fn new(
        buffer_size: usize,
        k8s: Option<Client>,
        k8s_clients: HashMap<String, Client>,
    ) -> (C2Handle, C2EventBus, Self) {
        let (cmd_tx, cmd_rx) = mpsc::channel(buffer_size);
        let event_bus = C2EventBus::new(buffer_size);

        // The builtin C2 backend routes commands through pod-exec, which needs
        // a live Kubernetes client. Without one (e.g. startup could not
        // authenticate to the cluster's current context) we register a stub
        // that fails every command with a clear reason. Control commands like
        // c2.read_local_kubeconfig are dispatched by the executor before it
        // consults the backend, so those still work — which is the whole point
        // of degrading here rather than aborting startup.
        let builtin: Arc<dyn C2Backend> = match k8s.clone() {
            Some(client) => Arc::new(BuiltinC2::new(client)),
            None => Arc::new(NoClientBackend),
        };
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
                executor: C2Executor {
                    event_bus,
                    backends,
                    k8s,
                    k8s_clients: Arc::new(k8s_clients),
                    listeners: Listeners::default(),
                },
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
                executor: C2Executor {
                    event_bus,
                    backends,
                    k8s: None,
                    k8s_clients: Arc::new(HashMap::new()),
                    listeners: Listeners::default(),
                },
            },
        )
    }

    pub async fn run(mut self) {
        let mut executions = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => match cmd {
                    Some(cmd) => {
                        let executor = self.executor.clone();
                        executions.spawn(async move { executor.execute_and_publish(cmd).await });
                    }
                    None => break,
                },
                Some(result) = executions.join_next(), if !executions.is_empty() => {
                    if let Err(error) = result {
                        warn!(%error, "c2 command task failed");
                    }
                }
            }
        }

        // Closing the command channel is a graceful shutdown: allow commands
        // that were already accepted to publish their completion events.
        while let Some(result) = executions.join_next().await {
            if let Err(error) = result {
                warn!(%error, "c2 command task failed");
            }
        }
        warn!("c2 command channel closed; stopping c2 manager loop");
    }
}

impl C2Executor {
    async fn execute_and_publish(&self, cmd: ExecTtp) {
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

    async fn execute_command(&self, cmd: &ExecTtp) -> TtpExecuted {
        let trimmed = cmd.procedure.command.trim_start();

        if let Some(explicit_path) = parse_read_local_kubeconfig_command(trimmed) {
            return self.read_local_kubeconfig(cmd, explicit_path);
        }

        if let Some(namespace) = parse_kubeconfig_permission_command(trimmed) {
            let Some(k8s) = self.client_for(cmd.auth_identity_id.as_deref()) else {
                let reason = "no K8s client configured".to_string();
                return TtpExecuted {
                    id: cmd.id.clone(),
                    success: false,
                    results: vec![reason.clone()],
                    exit_code: 1,
                    fail_reason: reason,
                    session_connected: None,
                };
            };
            return match k8s.self_subject_rules_review(namespace).await {
                Ok(response) => TtpExecuted {
                    id: cmd.id.clone(),
                    success: true,
                    results: vec![response],
                    exit_code: 0,
                    fail_reason: String::new(),
                    session_connected: None,
                },
                Err(error) => {
                    // Alternate formatting retains anyhow's source chain. In particular,
                    // connection, TLS, and API errors would otherwise be hidden behind
                    // the high-level SelfSubjectRulesReview context.
                    let reason = format!("{error:#}");
                    TtpExecuted {
                        id: cmd.id.clone(),
                        success: false,
                        results: vec![reason.clone()],
                        exit_code: 1,
                        fail_reason: reason,
                        session_connected: None,
                    }
                }
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
            let Some(k8s) = self.k8s.clone() else {
                return failed_result(cmd, "no active Kubernetes client configured");
            };
            return match open_kubectl_exec_session(
                self.backends.clone(),
                k8s,
                backend_id,
                target_entity_id,
                container,
            )
            .await
            {
                Ok(session_data) => TtpExecuted {
                    id: cmd.id.clone(),
                    success: true,
                    results: vec!["kubectl exec session ready".to_string()],
                    exit_code: 0,
                    fail_reason: String::new(),
                    session_connected: Some(session_data),
                },
                Err(error) => failed_result(cmd, &error),
            };
        }

        if cmd
            .auth_identity_id
            .as_deref()
            .is_some_and(|identity| identity.starts_with("k8s/credential/"))
        {
            let Some(k8s) = self.client_for(cmd.auth_identity_id.as_deref()) else {
                return failed_result(cmd, "no active Kubernetes client configured");
            };
            let result = if let Some(request) = cmd.procedure.k8s_request.as_ref() {
                k8s.execute_request(request)
                    .await
                    .map(|stdout| PodExecOutput {
                        stdout,
                        stderr: String::new(),
                        exit_code: 0,
                    })
            } else if let Some(request) = cmd.procedure.http_request.as_ref() {
                k8s.execute_authenticated_http_request(request)
                    .await
                    .map(|stdout| PodExecOutput {
                        stdout,
                        stderr: String::new(),
                        exit_code: 0,
                    })
            } else if trimmed.contains("kubectl ") || trimmed.starts_with("kubectl") {
                k8s.execute_kubectl_command(trimmed).await
            } else {
                return failed_result(
                    cmd,
                    "selected procedure does not support kubeconfig authentication",
                );
            };
            return match result {
                Ok(output) => command_output_result(cmd, output),
                Err(error) => failed_result(cmd, &error.to_string()),
            };
        }

        if trimmed == "noop" {
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
            self.spawn_session_listener(backend_id, target_entity_id, port, protocol)
                .await;
            return TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec![format!("listener starting on port {}", port)],
                exit_code: 0,
                fail_reason: String::new(),
                session_connected: None,
            };
        }

        if let Some(listener_id) = parse_stop_listener_command(trimmed) {
            return self.stop_listener(cmd, &listener_id).await;
        }

        let mut event = self.select_backend(cmd).await.execute(cmd).await;
        event.session_connected = None;
        event
    }

    /// Read the kubeconfig from the machine running Ran and return its contents
    /// as stdout. This is a local filesystem read on the operator host — it does
    /// not touch the cluster. The path is, in order of preference: the explicit
    /// `PATH` argument, the path the active client was configured with, then the
    /// default kubeconfig location.
    fn read_local_kubeconfig(&self, cmd: &ExecTtp, explicit_path: Option<String>) -> TtpExecuted {
        let path = explicit_path
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                self.k8s
                    .as_ref()
                    .and_then(|k8s| k8s.kubeconfig_path().map(Path::to_path_buf))
            })
            .unwrap_or_else(k8s::default_kubeconfig_path);

        match std::fs::read_to_string(&path) {
            Ok(contents) => TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec![contents],
                exit_code: 0,
                fail_reason: String::new(),
                session_connected: None,
            },
            Err(error) => failed_result(
                cmd,
                &format!("failed to read kubeconfig at {}: {error}", path.display()),
            ),
        }
    }

    async fn spawn_session_listener(
        &self,
        backend_id: String,
        target_entity_id: String,
        port: u16,
        protocol: String,
    ) {
        let backends = self.backends.clone();
        let event_bus = self.event_bus.clone();
        let listeners = self.listeners.clone();
        let handle = tokio::spawn(async move {
            accept_session_loop(
                backends,
                event_bus,
                listeners,
                backend_id,
                target_entity_id,
                port,
                protocol,
            )
            .await;
        });
        // Re-binding a port replaces the old handle, mirroring how the campaign
        // keeps one listener record per port.
        self.listeners
            .write()
            .await
            .insert(port, handle.abort_handle());
    }

    /// Release the port held by a bound listener.
    ///
    /// Aborting the accept loop drops its `TcpListener`, which is what frees the
    /// port. Sessions accepted earlier are registered backends and are left
    /// untouched, so an operator can stop listening without losing the shells
    /// they already caught.
    async fn stop_listener(&self, cmd: &ExecTtp, listener_id: &str) -> TtpExecuted {
        let Some(port) = ran_domain::listener_port(listener_id) else {
            return failed_result(
                cmd,
                &format!("'{listener_id}' is not a listener id (expected <protocol>/<port>)"),
            );
        };

        let Some(handle) = self.listeners.write().await.remove(&port) else {
            return failed_result(cmd, &format!("no listener is bound on port {port}"));
        };
        handle.abort();
        tracing::info!(port, "listener stopped; port released");

        let _ = self.event_bus.publish(C2Event::ListenerStopped { port });
        TtpExecuted {
            id: cmd.id.clone(),
            success: true,
            results: vec![format!("listener on port {port} stopped")],
            exit_code: 0,
            fail_reason: String::new(),
            session_connected: None,
        }
    }

    async fn select_backend(&self, cmd: &ExecTtp) -> Arc<dyn C2Backend> {
        let key = cmd.exec_system_id.trim().to_ascii_lowercase();
        let backends = self.backends.read().await;

        if key.is_empty() {
            debug!(
                cmd_id = %cmd.id,
                target_id = %cmd.target_id,
                exec_chain = ?cmd.exec_chain,
                "select_backend: empty exec_system_id → builtin c2"
            );
            return backends
                .get(BUILTIN_C2_ID)
                .expect("builtin c2 backend must always be registered")
                .clone();
        }

        if let Some(backend) = backends.get(&key) {
            debug!(
                cmd_id = %cmd.id,
                target_id = %cmd.target_id,
                exec_system_id = %cmd.exec_system_id,
                exec_chain = ?cmd.exec_chain,
                "select_backend: exact match"
            );
            return backend.clone();
        }

        // Accept `c2/<name>` and `<name>` as aliases when looking up backends.
        if let Some(stripped) = key.strip_prefix("c2/") {
            if let Some(backend) = backends.get(stripped) {
                debug!(
                    cmd_id = %cmd.id,
                    target_id = %cmd.target_id,
                    exec_system_id = %cmd.exec_system_id,
                    exec_chain = ?cmd.exec_chain,
                    "select_backend: matched via c2/ strip"
                );
                return backend.clone();
            }
        } else {
            let prefixed = format!("c2/{key}");
            if let Some(backend) = backends.get(&prefixed) {
                debug!(
                    cmd_id = %cmd.id,
                    target_id = %cmd.target_id,
                    exec_system_id = %cmd.exec_system_id,
                    exec_chain = ?cmd.exec_chain,
                    "select_backend: matched via c2/ prefix"
                );
                return backend.clone();
            }
        }

        warn!(
            cmd_id = %cmd.id,
            target_id = %cmd.target_id,
            exec_system_id = %cmd.exec_system_id,
            exec_chain = ?cmd.exec_chain,
            "select_backend: backend not found; falling back to builtin c2"
        );

        backends
            .get(BUILTIN_C2_ID)
            .expect("builtin c2 backend must always be registered")
            .clone()
    }
}

fn parse_kubeconfig_permission_command(command: &str) -> Option<&str> {
    command
        .trim()
        .strip_prefix("k8sSelfSubjectRulesReview(")?
        .strip_suffix(')')
        .map(str::trim)
        .filter(|namespace| !namespace.is_empty())
}

fn failed_result(cmd: &ExecTtp, reason: &str) -> TtpExecuted {
    TtpExecuted {
        id: cmd.id.clone(),
        success: false,
        results: vec![reason.to_string()],
        exit_code: 1,
        fail_reason: reason.to_string(),
        session_connected: None,
    }
}

fn command_output_result(cmd: &ExecTtp, output: k8s::PodExecOutput) -> TtpExecuted {
    let mut results = Vec::new();
    if !output.stdout.trim().is_empty() {
        results.push(output.stdout.trim().to_string());
    }
    if !output.stderr.trim().is_empty() {
        if results.is_empty() {
            results.push(String::new());
        }
        results.push(output.stderr.trim().to_string());
    }
    TtpExecuted {
        id: cmd.id.clone(),
        success: output.exit_code == 0,
        results,
        exit_code: output.exit_code,
        fail_reason: if output.exit_code == 0 {
            String::new()
        } else {
            output
                .stderr
                .lines()
                .last()
                .unwrap_or("kubectl command failed")
                .to_string()
        },
        session_connected: None,
    }
}

/// Open a kubectl exec session and register it as a backend. Returns the probe
/// data (hostname, user, os) for the caller to embed in `TtpExecuted` so the
/// campaign can process it after TTP effects rather than as a separate event.
async fn open_kubectl_exec_session(
    backends: Backends,
    k8s: Client,
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
        // Preserve anyhow's complete source chain. Kubernetes API status,
        // transport, TLS, container-selection, and upgrade errors otherwise
        // collapse into the generic open_exec_session context.
        .map_err(|e| format!("kubectl exec open failed for {target_entity_id}: {e:#}"))?;

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

/// Parse `c2.read_local_kubeconfig()` or `c2.read_local_kubeconfig(path)` from a
/// procedure command string. Returns `Some(None)` for the no-path form (use the
/// configured/default kubeconfig), `Some(Some(path))` when an explicit path is
/// given, and `None` when the command doesn't match.
fn parse_read_local_kubeconfig_command(cmd: &str) -> Option<Option<String>> {
    let inner = cmd
        .trim()
        .strip_prefix("c2.read_local_kubeconfig(")?
        .strip_suffix(')')?;
    let path = if inner.trim().is_empty() {
        None
    } else {
        Some(inner.trim().to_string())
    };
    Some(path)
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

/// Parse `c2.stop-listener(<listener id>)` from a procedure command string.
/// The listener id is whatever the TTP parameter carried — canonically
/// `protocol/port`, though a bare port is accepted downstream.
fn parse_stop_listener_command(cmd: &str) -> Option<String> {
    let inner = cmd.strip_prefix("c2.stop-listener(")?.strip_suffix(')')?;
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    Some(inner.to_string())
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
    listeners: Listeners,
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
            // The port was never held, so drop the registration made at spawn;
            // otherwise `c2.stop-listener` would report success for nothing.
            listeners.write().await.remove(&port);
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

                // Probe the shell for its target identity and operating system.
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
                listeners.write().await.remove(&port);
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
    use std::time::Duration;

    use armory::{Procedure, Ttp};
    use tokio::sync::{broadcast, mpsc, Semaphore};

    use super::{
        parse_kubeconfig_permission_command, parse_kubectl_exec_command,
        parse_read_local_kubeconfig_command, parse_stop_listener_command,
    };
    use super::{C2Backend, C2Event, C2Manager, ExecTtp, TtpExecuted, BUILTIN_C2_ID};

    struct MockBackend {
        marker: String,
    }

    struct BlockingBackend {
        started: mpsc::UnboundedSender<String>,
        release: Arc<Semaphore>,
    }

    #[test]
    fn parses_kubeconfig_permission_control_command() {
        assert_eq!(
            parse_kubeconfig_permission_command("k8sSelfSubjectRulesReview(dungeon)"),
            Some("dungeon")
        );
        assert_eq!(
            parse_kubeconfig_permission_command("k8sSelfSubjectRulesReview()"),
            None
        );
        assert_eq!(
            parse_kubeconfig_permission_command("kubectl get pods"),
            None
        );
    }

    #[test]
    fn parses_synchronous_kubectl_exec_control_command() {
        assert_eq!(parse_kubectl_exec_command("c2.kubectl_exec()"), Some(None));
        assert_eq!(
            parse_kubectl_exec_command("c2.kubectl_exec(debug)"),
            Some(Some("debug".to_string()))
        );
        assert_eq!(parse_kubectl_exec_command("kubectl exec pod -- true"), None);
    }

    #[test]
    fn parses_read_local_kubeconfig_control_command() {
        assert_eq!(
            parse_read_local_kubeconfig_command("c2.read_local_kubeconfig()"),
            Some(None)
        );
        assert_eq!(
            parse_read_local_kubeconfig_command("c2.read_local_kubeconfig(/home/op/.kube/config)"),
            Some(Some("/home/op/.kube/config".to_string()))
        );
        assert_eq!(
            parse_read_local_kubeconfig_command("  c2.read_local_kubeconfig()  "),
            Some(None)
        );
        assert_eq!(
            parse_read_local_kubeconfig_command("cat ~/.kube/config"),
            None
        );
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

    #[async_trait::async_trait]
    impl C2Backend for BlockingBackend {
        async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
            self.started
                .send(cmd.id.clone())
                .expect("test receiver should remain open");
            let permit = self.release.acquire().await.expect("semaphore is open");
            permit.forget();
            TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec![],
                exit_code: 0,
                fail_reason: String::new(),
                session_connected: None,
            }
        }
    }

    #[tokio::test]
    async fn executes_independent_commands_concurrently() {
        let (started_tx, mut started_rx) = mpsc::unbounded_channel();
        let release = Arc::new(Semaphore::new(0));
        let backend: Arc<dyn C2Backend> = Arc::new(BlockingBackend {
            started: started_tx,
            release: release.clone(),
        });
        let mut backends = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut events_rx = events.subscribe();
        let manager_task = tokio::spawn(manager.run());
        let first = exec_cmd("ran");
        let mut second = exec_cmd("ran");
        second.id = "cmd-second".to_string();

        handle
            .send(first)
            .await
            .expect("first command should queue");
        handle
            .send(second)
            .await
            .expect("second command should queue");

        tokio::time::timeout(Duration::from_secs(1), started_rx.recv())
            .await
            .expect("first command should start");
        tokio::time::timeout(Duration::from_secs(1), started_rx.recv())
            .await
            .expect("second command should start before the first completes");

        release.add_permits(2);
        events_rx.recv().await.expect("first result should publish");
        events_rx
            .recv()
            .await
            .expect("second result should publish");
        drop(handle);
        manager_task
            .await
            .expect("manager should shut down cleanly");
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

    #[test]
    fn parses_stop_listener_control_command() {
        assert_eq!(
            parse_stop_listener_command("c2.stop-listener(tcp/4444)"),
            Some("tcp/4444".to_string())
        );
        assert_eq!(
            parse_stop_listener_command("c2.stop-listener( 1337 )"),
            Some("1337".to_string())
        );
        assert_eq!(parse_stop_listener_command("c2.stop-listener()"), None);
        assert_eq!(parse_stop_listener_command("c2.listen(4444, tcp)"), None);
    }

    /// Build a control command whose procedure is `command`, run it through the
    /// manager, and return the resulting `TtpExecuted`.
    async fn run_control_command(command: &str) -> (TtpExecuted, broadcast::Receiver<C2Event>) {
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let rx = events.subscribe();
        tokio::spawn(manager.run());

        let mut cmd = exec_cmd("ran");
        cmd.procedure = Procedure::new("ran", "id");
        cmd.procedure.command = command.to_string();
        handle.send(cmd).await.expect("command should queue");
        (wait_for_execution(&mut { rx }).await, events.subscribe())
    }

    async fn wait_for_execution(rx: &mut broadcast::Receiver<C2Event>) -> TtpExecuted {
        loop {
            match tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("an execution event should arrive")
                .expect("event bus should stay open")
            {
                C2Event::TtpExecuted { event, .. } => return event,
                _ => continue,
            }
        }
    }

    #[tokio::test]
    async fn stopping_a_listener_releases_its_port() {
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();
        tokio::spawn(manager.run());

        // Let the OS pick a free port, then release it so the listener can take it.
        // A sandbox that forbids binding cannot exercise port release at all;
        // skip loudly there rather than reporting a failure it did not test.
        let probe = match tokio::net::TcpListener::bind("0.0.0.0:0").await {
            Ok(probe) => probe,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipped: this environment does not permit binding sockets");
                return;
            }
            Err(error) => panic!("probe bind failed: {error}"),
        };
        let port = probe.local_addr().expect("probe has an address").port();
        drop(probe);

        let mut listen = exec_cmd("ran");
        listen.procedure = Procedure::new("ran", "id");
        listen.procedure.command = format!("c2.listen({port}, tcp)");
        handle.send(listen).await.expect("listen should queue");

        // ListenerStarted only fires once the bind succeeded.
        loop {
            match tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("listener should bind")
                .expect("event bus should stay open")
            {
                C2Event::ListenerStarted { port: bound, .. } => {
                    assert_eq!(bound, port);
                    break;
                }
                _ => continue,
            }
        }
        // Probe with the same wildcard address the accept loop binds. Tokio sets
        // SO_REUSEADDR, and on BSD-derived stacks that lets a specific address
        // coexist with a wildcard bind — so probing 127.0.0.1 here would succeed
        // even while the listener holds the port, and prove nothing.
        assert!(
            tokio::net::TcpListener::bind(("0.0.0.0", port))
                .await
                .is_err(),
            "the port must be held while the listener runs"
        );

        let mut stop = exec_cmd("ran");
        stop.id = "cmd-stop".to_string();
        stop.procedure = Procedure::new("ran", "id");
        stop.procedure.command = format!("c2.stop-listener(tcp/{port})");
        handle.send(stop).await.expect("stop should queue");

        let mut saw_stopped = false;
        let mut execution: Option<TtpExecuted> = None;
        while execution.is_none() || !saw_stopped {
            match tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("stop should report back")
                .expect("event bus should stay open")
            {
                C2Event::ListenerStopped { port: stopped } => {
                    assert_eq!(stopped, port);
                    saw_stopped = true;
                }
                C2Event::TtpExecuted { event, .. } if event.id == "cmd-stop" => {
                    execution = Some(event);
                }
                _ => continue,
            }
        }
        let execution = execution.expect("loop only exits with an execution");
        assert!(execution.success, "{}", execution.fail_reason);

        // Aborting the accept loop drops its TcpListener, so the port is free.
        // Bind may lag the abort by a scheduler tick; retry briefly.
        let mut rebound = false;
        for _ in 0..20 {
            if tokio::net::TcpListener::bind(("0.0.0.0", port))
                .await
                .is_ok()
            {
                rebound = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(rebound, "stopping the listener must release port {port}");

        drop(handle);
    }

    #[tokio::test]
    async fn stopping_an_unbound_port_fails_with_a_clear_reason() {
        let (event, _events) = run_control_command("c2.stop-listener(tcp/9)").await;

        assert!(!event.success);
        assert!(
            event.fail_reason.contains("no listener is bound on port 9"),
            "unexpected reason: {}",
            event.fail_reason
        );
    }

    #[tokio::test]
    async fn stopping_a_malformed_listener_id_fails_without_touching_ports() {
        let (event, _events) = run_control_command("c2.stop-listener(not-a-listener)").await;

        assert!(!event.success);
        assert!(
            event.fail_reason.contains("is not a listener id"),
            "unexpected reason: {}",
            event.fail_reason
        );
    }

    fn exec_cmd(exec_system_id: &str) -> ExecTtp {
        ExecTtp {
            id: "cmd-fallback".to_string(),
            started_at_ms: 0,
            execution_timeout_seconds: crate::DEFAULT_EXECUTION_TIMEOUT_SECONDS,
            ttp: Ttp {
                description: "test".to_string(),
                ..Ttp::new("T0001", "Test TTP", "Execution")
            },
            procedure: Procedure::new("proc-1", "id"),
            args: HashMap::new(),
            target_id: "ns/default/pod/nginx".to_string(),
            exec_chain: vec!["ns/default/pod/nginx".to_string()],
            exec_system_id: exec_system_id.to_string(),
            auth_identity_id: None,
            output_transform: None,
            is_cleanup: false,
            reasoning: String::new(),
        }
    }
}
