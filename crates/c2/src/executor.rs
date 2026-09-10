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
/// The `labctl port-forward` processes started by `c2.port-forward`, keyed by
/// the canonical `<play id>/<remote port>` entry - the same identity the
/// `Redirector` entity carries. Keying on the remote port alone would be wrong:
/// two playgrounds are two hosts, so each can forward the same port, and `RPORT`
/// defaults to the same value for both.
///
/// Holding the `Child` is what makes a redirector stoppable - and, via
/// `kill_on_drop`, what stops the tunnels from outliving Ran.
type Redirectors = Arc<RwLock<HashMap<String, RedirectorProcess>>>;

/// A running `labctl port-forward` and the listener it was pointed at.
struct RedirectorProcess {
    child: tokio::process::Child,
    /// The listener this tunnel forwards into. Part of what the operator asked
    /// for, but *not* part of the redirector's identity, so it has to be
    /// remembered separately: it is what tells "this tunnel is already up" apart
    /// from "re-point this tunnel at a different listener".
    listener_port: u16,
}

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
    /// `labctl` children spawned by `c2.port-forward`, so `c2.stop-port-forward`
    /// can tear their tunnels down.
    redirectors: Redirectors,
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
            "no active Kubernetes client configured - read the local kubeconfig or restart with --kubeconfig",
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
        // consults the backend, so those still work - which is the whole point
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
                    redirectors: Redirectors::default(),
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
                    redirectors: Redirectors::default(),
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
            self.spawn_session_listener(ListenerSpec {
                cmd_id: cmd.id.clone(),
                backend_id,
                target_entity_id,
                port,
                protocol,
            })
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

        if let Some((play_id, remote_port, listener_ref)) = parse_port_forward_command(trimmed) {
            return self
                .start_redirector(cmd, &play_id, remote_port, &listener_ref)
                .await;
        }

        if let Some(redirector_id) = parse_stop_port_forward_command(trimmed) {
            return self.stop_redirector(cmd, &redirector_id).await;
        }

        let mut event = self.select_backend(cmd).await.execute(cmd).await;
        event.session_connected = None;

        // A live session that died surfaces as a session-death fail_reason -
        // either an unexpected close mid-command or sustained unresponsiveness
        // (repeated timeouts). Both are distinct from an ordinary non-zero exit
        // or a single slow-command timeout, which leave the session healthy.
        // Signal it as a SessionLost so the campaign marks the backing
        // exec-channel edge broken. The backend that ran the command -
        // `exec_system_id` - is the session id carried on that edge, so it
        // matches the edge back without extra bookkeeping.
        if !event.success && crate::types::is_session_death_reason(&event.fail_reason) {
            warn!(
                backend_id = %cmd.exec_system_id,
                target_id = %cmd.target_id,
                reason = %event.fail_reason,
                "session died; publishing SessionLost"
            );
            let _ = self.event_bus.publish(C2Event::SessionLost {
                backend_id: cmd.exec_system_id.clone(),
                target_entity_id: cmd.target_id.clone(),
            });
        }

        event
    }

    /// Read the kubeconfig from the machine running Ran and return its contents
    /// as stdout. This is a local filesystem read on the operator host - it does
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

    async fn spawn_session_listener(&self, spec: ListenerSpec) {
        let backends = self.backends.clone();
        let event_bus = self.event_bus.clone();
        let listeners = self.listeners.clone();
        let port = spec.port;
        let handle = tokio::spawn(async move {
            accept_session_loop(backends, event_bus, listeners, spec).await;
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

        let _ = self.event_bus.publish(C2Event::ListenerStopped {
            cmd_id: cmd.id.clone(),
            port,
        });
        TtpExecuted {
            id: cmd.id.clone(),
            success: true,
            results: vec![format!("listener on port {port} stopped")],
            exit_code: 0,
            fail_reason: String::new(),
            session_connected: None,
        }
    }

    /// Stand up a redirector:
    /// `labctl port-forward <play id> -R <remote>:127.0.0.1:<listener>`.
    ///
    /// This runs on the operator host - the same machine as Ran, which is where
    /// `labctl` is configured - so it is a control command rather than something
    /// dispatched to a backend. The child is long-running by design and is kept
    /// in `redirectors` rather than awaited; `kill_on_drop` means Ran going away
    /// takes the tunnel with it instead of leaking a forwarded port.
    ///
    /// Two details of the `-R` spec are easy to get wrong, and both fail quietly:
    ///
    /// - Its full form is `[REMOTE_HOST:]REMOTE_PORT:LOCAL_HOST:LOCAL_PORT`, and
    ///   `REMOTE_HOST` is the *name of a playground machine*, not a bind address.
    ///   Passing `0.0.0.0` there makes labctl exit with
    ///   `machine "0.0.0.0" not found in the playground`. The prefix is omitted
    ///   so labctl uses the playground's default machine.
    /// - The local side is `127.0.0.1` and not `localhost`, because the session
    ///   listener binds IPv4 `0.0.0.0` only (see `accept_session_loop`) while
    ///   `localhost` also resolves to `::1`.
    async fn start_redirector(
        &self,
        cmd: &ExecTtp,
        play_id: &str,
        remote_port: u16,
        listener_ref: &str,
    ) -> TtpExecuted {
        let Some(listener_port) = ran_domain::listener_port(listener_ref) else {
            return failed_result(
                cmd,
                &format!("'{listener_ref}' is not a listener id (expected <protocol>/<port>)"),
            );
        };
        let entry = ran_domain::format_redirector(play_id, remote_port);

        // Asking for a tunnel Ran is already running is not a failure to create
        // one; it is the state the operator asked for. Short-circuit rather than
        // spawning a second labctl, which the playground refuses with a 409
        // anyway - and which would report that refusal as though the redirector
        // did not exist. The existing Redirector entity stands, so no
        // `RedirectorStarted` is published: nothing was discovered, and
        // re-announcing it would surface an already-known hop as a fresh find.
        if self.holds_live_redirector(&entry, listener_port).await {
            tracing::info!(%entry, "redirector already running; reporting the existing tunnel");
            return TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec![format!(
                    "redirector {entry} is already running and already forwards to \
                     127.0.0.1:{listener_port}; nothing to do"
                )],
                exit_code: 0,
                fail_reason: String::new(),
                session_connected: None,
            };
        }

        let forward = redirector_forward_spec(remote_port, listener_port);
        let mut command = tokio::process::Command::new("labctl");
        command
            .args(["port-forward", play_id, "-R", &forward])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // The TTP shows `c2.port-forward(...)`, not the command line it becomes,
        // so report the real one. It is what the operator needs to reproduce the
        // tunnel by hand when Ran and labctl disagree about whether it worked.
        let argv = format!("labctl port-forward {play_id} -R {forward}");
        tracing::info!(%argv, "starting redirector");

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                return failed_result(cmd, &format!("could not run `{argv}`: {error}"));
            }
        };

        // Drain from the moment it starts: labctl's output is both the readiness
        // signal and the only account of why a forward later stops working.
        let mut lines = drain_child_output(&mut child, entry.clone());
        let startup = await_tunnel_ready(&mut child, &mut lines, TUNNEL_READY_TIMEOUT).await;
        let caveat = match startup {
            Ok(TunnelStartup::Ready) => None,
            Ok(TunnelStartup::StillSilent) => {
                warn!(
                    %entry,
                    timeout = ?TUNNEL_READY_TIMEOUT,
                    "labctl has not reported the tunnel up yet; leaving it running"
                );
                Some(format!(
                    " (labctl has not confirmed the tunnel within {TUNNEL_READY_TIMEOUT:?}; \
                     it is still running - check the redirector before relying on it)"
                ))
            }
            Err(reason) => {
                // Ran knows something labctl does not: which tunnels it is
                // already holding open on this playground.
                let held = self.redirectors_on(play_id).await;
                let mut detail = format!("`{argv}` {reason}");
                if let Some(hint) = tunnel_failure_hint(&reason, play_id, &held) {
                    detail.push_str(" - ");
                    detail.push_str(&hint);
                }
                return failed_result(cmd, &detail);
            }
        };

        // Re-forwarding the same port on the same playground replaces the tunnel
        // that held it, mirroring how the campaign keeps one redirector record per
        // entry. The same port on another playground is a different redirector.
        // Reaching here means the existing tunnel was dead or pointed at another
        // listener, since a live matching one short-circuits above.
        let previous = self.redirectors.write().await.insert(
            entry.clone(),
            RedirectorProcess {
                child,
                listener_port,
            },
        );
        if let Some(mut previous) = previous {
            let _ = previous.child.kill().await;
            tracing::info!(%entry, "replaced the redirector already on this entry");
        }

        tracing::info!(
            play_id,
            remote_port,
            listener_port,
            "redirector started; labctl port-forward running"
        );
        let _ = self.event_bus.publish(C2Event::RedirectorStarted {
            cmd_id: cmd.id.clone(),
            via: REDIRECTOR_TOOL.to_string(),
            play_id: play_id.to_string(),
            remote_port,
            listener_port,
        });
        TtpExecuted {
            id: cmd.id.clone(),
            success: true,
            results: vec![format!(
                "`{argv}` - port {remote_port} on playground {play_id} now reaches the local listener on 127.0.0.1:{listener_port}{}",
                caveat.unwrap_or_default()
            )],
            exit_code: 0,
            fail_reason: String::new(),
            session_connected: None,
        }
    }

    /// Whether Ran already holds a live tunnel on `entry` forwarding into
    /// `listener_port`.
    ///
    /// Two things stop this from claiming a success that is not true:
    ///
    /// - A tunnel pointed at a *different* listener is not the one being asked
    ///   for. The operator is re-pointing it, so it has to be rebuilt rather
    ///   than reported as already done.
    /// - `labctl` may have exited underneath us - the process is long-running
    ///   but not immortal, and nothing reaps it until someone looks. A dead
    ///   entry is dropped here so the caller spawns a fresh one instead of
    ///   reporting a tunnel that stopped carrying traffic hours ago.
    async fn holds_live_redirector(&self, entry: &str, listener_port: u16) -> bool {
        let mut redirectors = self.redirectors.write().await;
        let Some(existing) = redirectors.get_mut(entry) else {
            return false;
        };
        if existing.listener_port != listener_port {
            return false;
        }
        match existing.child.try_wait() {
            // Still running: this is the tunnel the operator asked for.
            Ok(None) => true,
            outcome => {
                // Dropping the record kills the child (`kill_on_drop`), which is
                // a no-op for one that already exited and the right move for one
                // whose state we could not read.
                redirectors.remove(entry);
                warn!(
                    %entry,
                    ?outcome,
                    "labctl is no longer running; rebuilding the redirector"
                );
                false
            }
        }
    }

    /// The entries of every redirector currently held open on `play_id`.
    async fn redirectors_on(&self, play_id: &str) -> Vec<String> {
        let prefix = format!("{play_id}/");
        let mut held: Vec<String> = self
            .redirectors
            .read()
            .await
            .keys()
            .filter(|entry| entry.starts_with(&prefix))
            .cloned()
            .collect();
        held.sort();
        held
    }

    /// Tear a redirector's tunnel down by killing its `labctl` child.
    ///
    /// Sessions that already came in through the tunnel are separate backends and
    /// are left running, exactly as with [`C2Executor::stop_listener`].
    async fn stop_redirector(&self, cmd: &ExecTtp, redirector_id: &str) -> TtpExecuted {
        let Some((play_id, remote_port)) = ran_domain::split_redirector(redirector_id) else {
            return failed_result(
                cmd,
                &format!(
                    "'{redirector_id}' is not a redirector id (expected <play id>/<remote port>)"
                ),
            );
        };
        let entry = ran_domain::format_redirector(&play_id, remote_port);

        let Some(mut existing) = self.redirectors.write().await.remove(&entry) else {
            return failed_result(cmd, &format!("no redirector is forwarding {entry}"));
        };
        if let Err(error) = existing.child.kill().await {
            // In practice this only fires when labctl already died on its own, in
            // which case the tunnel is gone anyway. The `Child` is dropped here
            // with `kill_on_drop` set, so the process is reaped either way.
            warn!(%entry, %error, "killing labctl reported an error");
        }
        tracing::info!(%entry, "redirector stopped; tunnel closed");

        let _ = self.event_bus.publish(C2Event::RedirectorStopped {
            cmd_id: cmd.id.clone(),
            play_id,
            remote_port,
        });
        TtpExecuted {
            id: cmd.id.clone(),
            success: true,
            results: vec![format!("redirector {entry} stopped")],
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

/// The tool `c2.port-forward` shells out to, recorded on the redirector it
/// creates. A redirector is only meaningful together with how it was built, and
/// this is the one place that knows.
const REDIRECTOR_TOOL: &str = "labctl";

/// Build the `-R` argument for `labctl port-forward`.
///
/// The spec is `[REMOTE_HOST:]REMOTE_PORT:LOCAL_HOST:LOCAL_PORT`. `REMOTE_HOST`
/// is deliberately omitted: it names a *playground machine*, not a bind address,
/// so anything address-shaped there is rejected with
/// `machine "..." not found in the playground`. Leaving it out uses the
/// playground's default machine.
fn redirector_forward_spec(remote_port: u16, listener_port: u16) -> String {
    format!("{remote_port}:127.0.0.1:{listener_port}")
}

/// The line `labctl port-forward` prints once the tunnel is actually carrying
/// traffic. Waiting for it is the difference between "the process started" and
/// "the port is open": labctl has to reach the Labs API and open a WebSocket
/// first, which takes well over a second on a cold playground.
const TUNNEL_READY_MARKER: &str = "Keeping port forwarding running";

/// How long to wait for [`TUNNEL_READY_MARKER`] before giving up on hearing it.
const TUNNEL_READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// How much of a failed tool's output to quote back to the operator.
const TRANSCRIPT_LINES: usize = 10;

/// What [`await_tunnel_ready`] concluded about a tunnel that is still running.
#[derive(Debug)]
enum TunnelStartup {
    /// The tool said the tunnel is up.
    Ready,
    /// The tool is alive but never said so within the timeout. Not a failure -
    /// the process would have exited had it rejected its arguments - but worth
    /// repeating to the operator rather than reporting a bare success.
    StillSilent,
}

/// Continuously read a child's stdout and stderr, logging every line and
/// forwarding it to `tx`.
///
/// Draining is not optional. `labctl` is verbose for the whole life of a tunnel
/// (it logs the readiness banner, then a line per connection, including
/// `error dialing local target ...` when the local side refuses). An unread pipe
/// fills after ~64KB and then blocks the writer mid-forward, which stops the
/// tunnel for reasons nothing downstream can see. Sending to `tx` is best-effort
/// so that draining continues after the startup receiver is dropped.
fn drain_child_output(
    child: &mut tokio::process::Child,
    entry: String,
) -> mpsc::UnboundedReceiver<String> {
    use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

    let (tx, rx) = mpsc::unbounded_channel();

    fn spawn_drain<R: AsyncRead + Unpin + Send + 'static>(
        reader: Option<R>,
        stream: &'static str,
        entry: String,
        tx: mpsc::UnboundedSender<String>,
    ) {
        let Some(reader) = reader else { return };
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // A tunnel that is up but refusing traffic only says so here -
                // `error dialing local target ...` is per-connection and arrives
                // long after the action reported success, so it has to be visible
                // at the default log level rather than buried at debug.
                if looks_like_an_error(&line) {
                    warn!(%entry, stream, %line, "labctl");
                    let _ = tx.send(line);
                    continue;
                }
                debug!(%entry, stream, %line, "labctl");
                let _ = tx.send(line);
            }
        });
    }

    spawn_drain(child.stdout.take(), "stdout", entry.clone(), tx.clone());
    spawn_drain(child.stderr.take(), "stderr", entry, tx);
    rx
}

/// Wait for a freshly spawned tunnel to report itself up.
///
/// `Err` means the tunnel definitely did not come up - the process exited, which
/// is what a rejected playground id does within milliseconds. The error carries
/// the tool's own last words so a bad `PLAY_ID` is diagnosable rather than a
/// silent no-op.
async fn await_tunnel_ready(
    child: &mut tokio::process::Child,
    lines: &mut mpsc::UnboundedReceiver<String>,
    timeout: std::time::Duration,
) -> Result<TunnelStartup, String> {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);

    let mut transcript: Vec<String> = Vec::new();
    let mut draining = true;
    loop {
        tokio::select! {
            // `Child::wait` is cancel-safe, so losing this branch to another is
            // free of side effects.
            status = child.wait() => {
                let detail = match status {
                    Ok(status) => format!("exited before the tunnel came up ({status})"),
                    Err(error) => format!("could not be waited on: {error}"),
                };
                return Err(format!("{detail}: {}", quote_transcript(&transcript)));
            }
            line = lines.recv(), if draining => match line {
                Some(line) => {
                    if line.contains(TUNNEL_READY_MARKER) {
                        return Ok(TunnelStartup::Ready);
                    }
                    transcript.push(line);
                }
                // Both pipes hit EOF without the marker. The process may still be
                // running, so let the other branches decide its fate.
                None => draining = false,
            },
            _ = &mut deadline => return Ok(TunnelStartup::StillSilent),
        }
    }
}

/// Translate a labctl startup failure into something an operator can act on.
///
/// Only the conflict case is worth translating, and it is worth it because
/// `error dialing reverse control WS (status 409)` says nothing about what to do
/// next. Two things make it self-inflicted often enough to call out: the
/// playground accepts one reverse control channel at a time, and labctl *saves*
/// every `-R` into the playground's config ("port forwards are automatically
/// saved ... for later restoration"), so abandoned attempts linger there.
fn tunnel_failure_hint(reason: &str, play_id: &str, held: &[String]) -> Option<String> {
    let lowered = reason.to_ascii_lowercase();
    if !lowered.contains("409") && !lowered.contains("conflict") {
        return None;
    }

    let mine = if held.is_empty() {
        String::new()
    } else {
        format!(
            " Ran is already holding {} open on this playground ({}); stopping it frees the channel.",
            if held.len() == 1 {
                "a redirector"
            } else {
                "redirectors"
            },
            held.join(", ")
        )
    };
    Some(format!(
        "the playground refused the tunnel (409 Conflict), which usually means one is already \
         attached to {play_id}.{mine} labctl also saves every -R forward into the playground's \
         config, so earlier attempts can still be registered - list them with \
         `labctl port-forward {play_id} --list` and drop one with `--remove <index>`."
    ))
}

/// Whether a line of tool output reads like a complaint rather than progress.
///
/// Deliberately crude: the point is to raise labctl's own problem reports to a
/// visible log level without parsing its format, so a false positive costs one
/// needless warning and a false negative costs only a debug line.
fn looks_like_an_error(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    ["error", "failed", "couldn't", "refused", "timeout"]
        .iter()
        .any(|needle| line.contains(needle))
}

/// Render a tool's output for an operator-facing failure reason, keeping the
/// tail - the last thing a tool says before dying is the part that explains why.
fn quote_transcript(transcript: &[String]) -> String {
    let tail: Vec<&str> = transcript
        .iter()
        .rev()
        .take(TRANSCRIPT_LINES)
        .rev()
        .map(String::as_str)
        .collect();
    if tail.is_empty() {
        "no output".to_string()
    } else {
        tail.join("; ")
    }
}

/// Parse `c2.port-forward(<play id>, <remote port>, <listener id>)` from a
/// procedure command string. The listener id is whatever the TTP parameter
/// carried - canonically `protocol/port`, resolved to a port downstream.
fn parse_port_forward_command(cmd: &str) -> Option<(String, u16, String)> {
    let inner = cmd
        .trim()
        .strip_prefix("c2.port-forward(")?
        .strip_suffix(')')?;
    let mut parts = inner.splitn(3, ',');
    let play_id = parts.next()?.trim().to_string();
    let remote_port: u16 = parts.next()?.trim().parse().ok()?;
    let listener_ref = parts.next()?.trim().to_string();
    if play_id.is_empty() || remote_port == 0 || listener_ref.is_empty() {
        return None;
    }
    Some((play_id, remote_port, listener_ref))
}

/// Parse `c2.stop-port-forward(<redirector id>)` from a procedure command
/// string. The redirector id is canonically `<play id>/<remote port>`.
fn parse_stop_port_forward_command(cmd: &str) -> Option<String> {
    let inner = cmd
        .trim()
        .strip_prefix("c2.stop-port-forward(")?
        .strip_suffix(')')?;
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    Some(inner.to_string())
}

/// Parse `c2.stop-listener(<listener id>)` from a procedure command string.
/// The listener id is whatever the TTP parameter carried - canonically
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
/// execution context - uses the same deterministic scheme as the effect handler.
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

/// Everything the accept loop needs to identify the listener it is running.
struct ListenerSpec {
    /// The execution that asked for this listener, carried so the campaign can
    /// attribute the resulting listener entity to it.
    cmd_id: String,
    backend_id: String,
    target_entity_id: String,
    port: u16,
    protocol: String,
}

async fn accept_session_loop(
    backends: Backends,
    event_bus: C2EventBus,
    listeners: Listeners,
    spec: ListenerSpec,
) {
    use crate::ShellSession;
    use std::net::{Ipv4Addr, SocketAddr};

    let ListenerSpec {
        cmd_id,
        backend_id,
        target_entity_id,
        port,
        protocol,
    } = spec;

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
        cmd_id,
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
        await_tunnel_ready, drain_child_output, looks_like_an_error,
        parse_kubeconfig_permission_command, parse_kubectl_exec_command,
        parse_port_forward_command, parse_read_local_kubeconfig_command,
        parse_stop_listener_command, parse_stop_port_forward_command, quote_transcript,
        redirector_forward_spec, tunnel_failure_hint, C2Executor, RedirectorProcess, Redirectors,
        TunnelStartup,
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
        listen.id = "cmd-listen".to_string();
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
                C2Event::ListenerStarted {
                    cmd_id,
                    port: bound,
                    ..
                } => {
                    assert_eq!(bound, port);
                    // Carried from the command so the campaign can attribute the
                    // listener entity to the action that bound it.
                    assert_eq!(cmd_id, "cmd-listen");
                    break;
                }
                _ => continue,
            }
        }
        // Probe with the same wildcard address the accept loop binds. Tokio sets
        // SO_REUSEADDR, and on BSD-derived stacks that lets a specific address
        // coexist with a wildcard bind - so probing 127.0.0.1 here would succeed
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
                C2Event::ListenerStopped {
                    cmd_id,
                    port: stopped,
                } => {
                    assert_eq!(stopped, port);
                    // The campaign attributes the teardown to this command.
                    assert_eq!(cmd_id, "cmd-stop");
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

    #[test]
    fn parses_port_forward_control_command() {
        assert_eq!(
            parse_port_forward_command("c2.port-forward(zn1kqxk3ykpvxp5x, 1337, tcp/4444)"),
            Some(("zn1kqxk3ykpvxp5x".to_string(), 1337, "tcp/4444".to_string()))
        );
        // The listener parameter may arrive as a full entity id, which is what
        // `${TARGET}` resolves to for a Listener-typed parameter.
        assert_eq!(
            parse_port_forward_command("c2.port-forward( play1 , 1337 , listener/tcp/4444 )"),
            Some(("play1".to_string(), 1337, "listener/tcp/4444".to_string()))
        );
        // Every field is required, and port 0 is not a port an operator can mean.
        assert_eq!(
            parse_port_forward_command("c2.port-forward(play1, 1337)"),
            None
        );
        assert_eq!(
            parse_port_forward_command("c2.port-forward(, 1337, tcp/4444)"),
            None
        );
        assert_eq!(
            parse_port_forward_command("c2.port-forward(play1, 0, tcp/4444)"),
            None
        );
        assert_eq!(
            parse_port_forward_command("c2.port-forward(play1, http, tcp/4444)"),
            None
        );
        assert_eq!(
            parse_port_forward_command("c2.stop-port-forward(play1/1337)"),
            None
        );
    }

    #[test]
    fn parses_stop_port_forward_control_command() {
        assert_eq!(
            parse_stop_port_forward_command("c2.stop-port-forward(play1/1337)"),
            Some("play1/1337".to_string())
        );
        // A full entity id is what `${TARGET}` resolves to for a Redirector param.
        assert_eq!(
            parse_stop_port_forward_command("c2.stop-port-forward( redirector/play1/1337 )"),
            Some("redirector/play1/1337".to_string())
        );
        assert_eq!(
            parse_stop_port_forward_command("c2.stop-port-forward()"),
            None
        );
        assert_eq!(
            parse_stop_port_forward_command("c2.port-forward(play1, 1337, tcp/4444)"),
            None
        );
    }

    /// Spawn `sh -c <script>` with piped output, the way a labctl stand-in needs
    /// to be wired for the startup helpers. Used instead of labctl itself so the
    /// tests hold on a machine that has never installed it.
    fn fake_tunnel(script: &str) -> tokio::process::Child {
        tokio::process::Command::new("sh")
            .args(["-c", script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("sh should be available")
    }

    /// The guard behind gotcha 4: a control command must not report a tunnel as
    /// up just because the process spawned.
    #[tokio::test]
    async fn a_tunnel_that_exits_before_reporting_ready_is_a_failure() {
        let mut child = fake_tunnel("echo 'playground not found' >&2; exit 1");
        let mut lines = drain_child_output(&mut child, "play1/1337".to_string());

        let reason = await_tunnel_ready(&mut child, &mut lines, Duration::from_secs(5))
            .await
            .expect_err("a child that exits at once must not pass startup");
        assert!(
            reason.contains("exited before the tunnel came up"),
            "unexpected reason: {reason}"
        );
        // The tool's own diagnosis has to survive into the failure reason, or a
        // bad PLAY_ID is indistinguishable from any other non-zero exit.
        assert!(
            reason.contains("playground not found"),
            "labctl's output was not surfaced: {reason}"
        );
    }

    /// The readiness line is what separates "process running" from "port open" -
    /// labctl has to reach the Labs API and open a WebSocket before either is true.
    #[tokio::test]
    async fn a_tunnel_is_ready_once_labctl_says_it_is_forwarding() {
        let mut child = fake_tunnel(
            "echo 'time=... level=INFO msg=\"Forwarding 0.0.0.0:1337\"'; \
             echo 'Keeping port forwarding running. Press Ctrl+C to stop.'; \
             sleep 60",
        );
        let mut lines = drain_child_output(&mut child, "play1/1337".to_string());

        let startup = await_tunnel_ready(&mut child, &mut lines, Duration::from_secs(5))
            .await
            .expect("a tunnel that announces itself is up");
        assert!(matches!(startup, TunnelStartup::Ready));

        let _ = child.kill().await;
    }

    /// A tool that is alive but quiet is not a failure - it never rejected its
    /// arguments - but it must not be reported as a plain success either.
    #[tokio::test]
    async fn a_silent_running_tunnel_is_reported_but_left_alone() {
        let mut child = fake_tunnel("sleep 60");
        let mut lines = drain_child_output(&mut child, "play1/1337".to_string());

        let startup = await_tunnel_ready(&mut child, &mut lines, Duration::from_millis(200))
            .await
            .expect("a running tunnel is not a failure");
        assert!(matches!(startup, TunnelStartup::StillSilent));
        assert!(
            child.try_wait().expect("child is pollable").is_none(),
            "a silent tunnel must be left running, not killed"
        );

        let _ = child.kill().await;
    }

    /// Regression guard for the deadlock that made forwards stop working: labctl
    /// logs for the whole life of a tunnel, and an unread pipe blocks the writer
    /// once the ~64KB OS buffer fills.
    #[tokio::test]
    async fn a_chatty_tunnel_is_drained_so_it_cannot_block_on_a_full_pipe() {
        // Well past any pipe buffer, and the readiness line comes last so it can
        // only be seen by a reader that kept up with the noise.
        let mut child = fake_tunnel(
            "i=0; while [ $i -lt 4000 ]; do \
               echo 'connection from 10.0.0.1 dialing local target 127.0.0.1:4444'; \
               i=$((i+1)); \
             done; \
             echo 'Keeping port forwarding running. Press Ctrl+C to stop.'; \
             sleep 60",
        );
        let mut lines = drain_child_output(&mut child, "play1/1337".to_string());

        let startup = await_tunnel_ready(&mut child, &mut lines, Duration::from_secs(10))
            .await
            .expect("draining must let a chatty tunnel reach readiness");
        assert!(matches!(startup, TunnelStartup::Ready));

        let _ = child.kill().await;
    }

    /// Regression guard for the bug that made the redirector silently useless:
    /// labctl's `-R` spec is `[REMOTE_HOST:]REMOTE_PORT:LOCAL_HOST:LOCAL_PORT`,
    /// and `REMOTE_HOST` is the name of a *playground machine*, not a bind
    /// address. A leading `0.0.0.0:` made labctl exit with
    /// `machine "0.0.0.0" not found in the playground`.
    #[test]
    fn the_remote_forward_spec_carries_no_host_prefix() {
        let spec = redirector_forward_spec(9000, 4444);

        assert_eq!(spec, "9000:127.0.0.1:4444");
        // Three parts: remote port, local host, local port. A fourth would mean a
        // REMOTE_HOST crept back in and labctl would read it as a machine name.
        assert_eq!(spec.split(':').count(), 3);
        assert!(
            !spec.starts_with("0.0.0.0"),
            "0.0.0.0 is not a playground machine"
        );
        // The local side must be literal IPv4: the session listener binds
        // 0.0.0.0 on IPv4 only, while `localhost` also resolves to ::1.
        assert!(spec.contains("127.0.0.1"), "{spec}");
        assert!(!spec.contains("localhost"), "{spec}");
    }

    /// `error dialing reverse control WS (status 409)` is a transport fact that
    /// tells the operator nothing about what to do, so it gets translated.
    #[test]
    fn a_conflicting_tunnel_is_explained_rather_than_echoed() {
        let reason = "exited before the tunnel came up (exit status: 1): \
             Tunnel error: error dialing reverse control WS (status 409): websocket: bad handshake";

        let hint =
            tunnel_failure_hint(reason, "play1", &[]).expect("a 409 must come with an explanation");
        assert!(hint.contains("409 Conflict"), "{hint}");
        // The two things the operator can actually check.
        assert!(hint.contains("labctl port-forward play1 --list"), "{hint}");
        assert!(hint.contains("--remove"), "{hint}");
        // Nothing of Ran's is open, so it must not claim otherwise.
        assert!(!hint.contains("Ran is already holding"), "{hint}");
    }

    #[test]
    fn a_conflict_names_the_tunnels_ran_itself_is_holding() {
        let reason = "exited before the tunnel came up (exit status: 1): status 409";

        let hint = tunnel_failure_hint(reason, "play1", &["play1/9000".to_string()])
            .expect("a 409 must come with an explanation");
        assert!(
            hint.contains("Ran is already holding a redirector"),
            "{hint}"
        );
        assert!(hint.contains("play1/9000"), "{hint}");
    }

    #[test]
    fn failures_that_are_not_conflicts_are_left_to_speak_for_themselves() {
        assert_eq!(
            tunnel_failure_hint(
                "exited before the tunnel came up: playground not found",
                "p",
                &[]
            ),
            None
        );
    }

    #[test]
    fn labctl_complaints_are_told_apart_from_progress() {
        // The line that explains "the target can't connect" while the tunnel is
        // otherwise up and reporting success.
        assert!(looks_like_an_error(
            "error dialing local target 127.0.0.1:4444: connection refused"
        ));
        assert!(looks_like_an_error("couldn't connect to play connection"));
        assert!(looks_like_an_error("ERROR: timeout"));
        assert!(!looks_like_an_error(
            "Keeping port forwarding running. Press Ctrl+C to stop."
        ));
        assert!(!looks_like_an_error("Forwarding 0.0.0.0:1337"));
    }

    #[test]
    fn a_failure_transcript_quotes_the_tools_last_words() {
        let transcript: Vec<String> = (0..25).map(|i| format!("line {i}")).collect();
        let quoted = quote_transcript(&transcript);

        // The tail explains the failure; the head is startup noise.
        assert!(quoted.contains("line 24"), "{quoted}");
        assert!(quoted.contains("line 15"), "{quoted}");
        assert!(!quoted.contains("line 14"), "{quoted}");
        assert_eq!(quote_transcript(&[]), "no output");
    }

    /// Stops a redirector whose `labctl` stand-in is a real long-running child,
    /// so the teardown path is exercised end to end without labctl installed.
    #[tokio::test]
    async fn stopping_a_redirector_kills_its_child_and_frees_the_remote_port() {
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();

        let child = tokio::process::Command::new("sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .expect("sleep should be available");
        let pid = child.id().expect("a live child has a pid");
        let redirectors = manager.executor.redirectors.clone();
        redirectors.write().await.insert(
            "play1/1337".to_string(),
            RedirectorProcess {
                child,
                listener_port: 4444,
            },
        );
        tokio::spawn(manager.run());

        let mut stop = exec_cmd("ran");
        stop.id = "cmd-stop-redirector".to_string();
        stop.procedure = Procedure::new("ran", "id");
        stop.procedure.command = "c2.stop-port-forward(redirector/play1/1337)".to_string();
        handle.send(stop).await.expect("stop should queue");

        let mut saw_stopped = false;
        let mut execution: Option<TtpExecuted> = None;
        while execution.is_none() || !saw_stopped {
            match tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("stop should report back")
                .expect("event bus should stay open")
            {
                C2Event::RedirectorStopped {
                    play_id,
                    remote_port,
                    ..
                } => {
                    assert_eq!(play_id, "play1");
                    assert_eq!(remote_port, 1337);
                    saw_stopped = true;
                }
                C2Event::TtpExecuted { event, .. } if event.id == "cmd-stop-redirector" => {
                    execution = Some(event);
                }
                _ => continue,
            }
        }
        let execution = execution.expect("loop only exits with an execution");
        assert!(execution.success, "{}", execution.fail_reason);

        // The record is gone, so a second stop cannot claim to succeed.
        assert!(redirectors.read().await.is_empty());
        // `kill().await` reaps the child, so the pid no longer names a process
        // this shell can signal.
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            // "No such process" on stderr is the expected outcome here; it is the
            // assertion, not a test failure worth printing.
            .stderr(std::process::Stdio::null())
            .status()
            .expect("kill should be available")
            .success();
        assert!(!alive, "labctl child {pid} outlived its redirector");

        drop(handle);
    }

    /// Build a manager whose redirector map already holds `entry`, backed by a
    /// real long-running child, and run `command` through it.
    async fn with_held_redirector(
        entry: &str,
        listener_port: u16,
        script: &str,
        command: &str,
    ) -> (TtpExecuted, Redirectors, Option<u32>) {
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();

        let child = fake_tunnel(script);
        let pid = child.id();
        let redirectors = manager.executor.redirectors.clone();
        redirectors.write().await.insert(
            entry.to_string(),
            RedirectorProcess {
                child,
                listener_port,
            },
        );
        tokio::spawn(manager.run());

        let mut exec = exec_cmd("ran");
        exec.id = "cmd-recreate".to_string();
        exec.procedure = Procedure::new("ran", "id");
        exec.procedure.command = command.to_string();
        handle.send(exec).await.expect("command should queue");

        let event = wait_for_execution(&mut rx).await;
        drop(handle);
        (event, redirectors, pid)
    }

    /// Re-creating a redirector Ran is already running is the state the operator
    /// asked for, not a failure. Spawning a second labctl would earn a 409 and
    /// report the tunnel as missing when it is right there.
    #[tokio::test]
    async fn re_creating_a_running_redirector_succeeds_without_spawning() {
        let (event, redirectors, pid) = with_held_redirector(
            "play1/1337",
            4444,
            "sleep 60",
            "c2.port-forward(play1, 1337, tcp/4444)",
        )
        .await;

        assert!(event.success, "{}", event.fail_reason);
        assert!(
            event.results[0].contains("already running"),
            "the result must say nothing was done: {:?}",
            event.results
        );
        // The original child is still the one on file - nothing was replaced.
        let held = redirectors.read().await;
        assert_eq!(held.len(), 1);
        assert_eq!(held["play1/1337"].child.id(), pid);
    }

    /// An executor with nothing registered, for exercising its own methods
    /// without running the manager loop or spawning anything real.
    fn bare_executor() -> C2Executor {
        let (_handle, _events, manager) = C2Manager::new_with_backends(8, HashMap::new());
        manager.executor
    }

    /// The three answers `holds_live_redirector` has to get right. Exercised
    /// directly rather than through a control command: the other two cases fall
    /// through to a real `labctl` spawn, which would make the test depend on
    /// whether labctl is installed and on the network behind it.
    #[tokio::test]
    async fn a_live_matching_tunnel_is_the_only_thing_reported_as_already_running() {
        let executor = bare_executor();

        // Live and pointed at the listener being asked for.
        executor.redirectors.write().await.insert(
            "play1/1337".to_string(),
            RedirectorProcess {
                child: fake_tunnel("sleep 60"),
                listener_port: 4444,
            },
        );
        assert!(executor.holds_live_redirector("play1/1337", 4444).await);

        // The listener is not part of a redirector's identity, so an entry match
        // alone would silently swallow a request to re-point the tunnel.
        assert!(!executor.holds_live_redirector("play1/1337", 8080).await);
        assert!(
            executor.redirectors.read().await.contains_key("play1/1337"),
            "a re-point must not drop the tunnel before the new one is up"
        );

        // An entry nobody holds.
        assert!(!executor.holds_live_redirector("play2/1337", 4444).await);
    }

    /// A held entry whose labctl died is not a running tunnel, and must not be
    /// reported as one - the process is long-running but not immortal, and
    /// nothing reaps it until someone looks.
    #[tokio::test]
    async fn a_dead_held_redirector_is_dropped_rather_than_reported_as_running() {
        let executor = bare_executor();

        let mut child = fake_tunnel("exit 0");
        // Reap it here so the state is settled rather than racing the check.
        let _ = child.wait().await;
        executor.redirectors.write().await.insert(
            "play1/1337".to_string(),
            RedirectorProcess {
                child,
                listener_port: 4444,
            },
        );

        assert!(!executor.holds_live_redirector("play1/1337", 4444).await);
        assert!(
            !executor.redirectors.read().await.contains_key("play1/1337"),
            "a dead tunnel must be dropped so the next attempt spawns a fresh one"
        );
    }

    #[tokio::test]
    async fn stopping_an_unforwarded_remote_port_fails_with_a_clear_reason() {
        let (event, _events) = run_control_command("c2.stop-port-forward(play1/9999)").await;

        assert!(!event.success);
        assert!(
            event
                .fail_reason
                .contains("no redirector is forwarding play1/9999"),
            "unexpected reason: {}",
            event.fail_reason
        );
    }

    /// A tunnel is identified by playground *and* port, so stopping the same port
    /// on another playground must not reach into this one.
    #[tokio::test]
    async fn stopping_the_same_port_on_another_playground_does_not_touch_this_one() {
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();

        let child = tokio::process::Command::new("sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .expect("sleep should be available");
        let redirectors = manager.executor.redirectors.clone();
        redirectors.write().await.insert(
            "play1/1337".to_string(),
            RedirectorProcess {
                child,
                listener_port: 4444,
            },
        );
        tokio::spawn(manager.run());

        let mut stop = exec_cmd("ran");
        stop.id = "cmd-stop-other".to_string();
        stop.procedure = Procedure::new("ran", "id");
        stop.procedure.command = "c2.stop-port-forward(play2/1337)".to_string();
        handle.send(stop).await.expect("stop should queue");

        let event = wait_for_execution(&mut rx).await;
        assert!(!event.success);
        assert!(
            event
                .fail_reason
                .contains("no redirector is forwarding play2/1337"),
            "unexpected reason: {}",
            event.fail_reason
        );
        assert!(
            redirectors.read().await.contains_key("play1/1337"),
            "the other playground's tunnel on the same port must survive"
        );

        drop(handle);
    }

    #[tokio::test]
    async fn stopping_a_malformed_redirector_id_fails_without_touching_tunnels() {
        let (event, _events) = run_control_command("c2.stop-port-forward(not-a-redirector)").await;

        assert!(!event.success);
        assert!(
            event.fail_reason.contains("is not a redirector id"),
            "unexpected reason: {}",
            event.fail_reason
        );
    }

    #[tokio::test]
    async fn port_forwarding_to_a_malformed_listener_fails_before_spawning() {
        let (event, _events) =
            run_control_command("c2.port-forward(play1, 1337, not-a-listener)").await;

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
