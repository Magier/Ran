use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use k8s::{Client, ExecOutputObserver, ExecOutputStream, PodExecOutput};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, warn};

use crate::builtin::BuiltinC2;
use crate::output::{IncrementalTextDecoder, OutputFragment, OutputSink, OutputStream};
use crate::types::{C2Event, ExecTtp, ExecutionOperation, TtpExecuted};

use crate::types::BUILTIN_C2_ID;

type Backends = Arc<RwLock<HashMap<String, Arc<dyn C2Backend>>>>;
type K8sClients = Arc<RwLock<HashMap<String, Client>>>;
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
/// Command ids whose redirector exists but is owned by an external labctl
/// process. The marker is consumed when the matching execution event is
/// published, so it cannot leak into a later action with the same id.
type PartialExecutions = Arc<RwLock<HashSet<String>>>;

const OUTPUT_FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const OUTPUT_FLUSH_BYTES: usize = 16 * 1024;
const OUTPUT_CAPTURE_LIMIT: usize = 1024 * 1024;

fn append_bounded_bytes(buffer: &mut Vec<u8>, bytes: &[u8]) {
    buffer.extend_from_slice(bytes);
    if buffer.len() > OUTPUT_CAPTURE_LIMIT {
        let remove = buffer.len() - OUTPUT_CAPTURE_LIMIT;
        buffer.drain(..remove);
    }
}

#[derive(Default)]
struct OutputCollector {
    stdout_decoder: IncrementalTextDecoder,
    stderr_decoder: IncrementalTextDecoder,
    pending_stdout: String,
    pending_stderr: String,
    captured_stdout: Vec<u8>,
    captured_stderr: Vec<u8>,
    stdout_bytes: u64,
    stderr_bytes: u64,
    pending_bytes: usize,
    sequence: u64,
}

impl OutputCollector {
    fn push(&mut self, fragment: OutputFragment) {
        self.pending_bytes += fragment.bytes.len();
        match fragment.stream {
            OutputStream::Stdout => {
                self.stdout_bytes += fragment.bytes.len() as u64;
                append_bounded_bytes(&mut self.captured_stdout, &fragment.bytes);
                self.pending_stdout
                    .push_str(&self.stdout_decoder.push(&fragment.bytes));
            }
            OutputStream::Stderr => {
                self.stderr_bytes += fragment.bytes.len() as u64;
                append_bounded_bytes(&mut self.captured_stderr, &fragment.bytes);
                self.pending_stderr
                    .push_str(&self.stderr_decoder.push(&fragment.bytes));
            }
        }
    }

    fn should_flush(&self) -> bool {
        self.pending_bytes >= OUTPUT_FLUSH_BYTES
    }

    fn finish_decoding(&mut self) {
        self.pending_stdout.push_str(&self.stdout_decoder.finish());
        self.pending_stderr.push_str(&self.stderr_decoder.finish());
    }

    fn take_batch(&mut self, cmd_id: &str) -> Option<C2Event> {
        if self.pending_stdout.is_empty() && self.pending_stderr.is_empty() {
            self.pending_bytes = 0;
            return None;
        }
        self.sequence += 1;
        self.pending_bytes = 0;
        Some(C2Event::TtpOutput {
            cmd_id: cmd_id.to_string(),
            sequence: self.sequence,
            stdout: std::mem::take(&mut self.pending_stdout),
            stderr: std::mem::take(&mut self.pending_stderr),
            stdout_bytes: self.stdout_bytes,
            stderr_bytes: self.stderr_bytes,
        })
    }

    fn preserve_partial_failure_output(&self, event: &mut TtpExecuted) {
        if event.success || (self.captured_stdout.is_empty() && self.captured_stderr.is_empty()) {
            return;
        }
        let only_failure_reason = event.results.is_empty()
            || (event.results.len() == 1 && event.results[0] == event.fail_reason);
        if !only_failure_reason {
            return;
        }
        let stdout = String::from_utf8_lossy(&self.captured_stdout)
            .trim_end()
            .to_string();
        let stderr = String::from_utf8_lossy(&self.captured_stderr)
            .trim_end()
            .to_string();
        event.results.clear();
        if !stdout.is_empty() {
            event.results.push(stdout);
        }
        if !stderr.is_empty() {
            if event.results.is_empty() {
                event.results.push(String::new());
            }
            event.results.push(stderr);
        }
    }
}

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
    k8s_clients: K8sClients,
}

impl C2Handle {
    pub async fn register_backend(&self, id: impl Into<String>, backend: Arc<dyn C2Backend>) {
        self.backends.write().await.insert(id.into(), backend);
    }

    pub async fn register_k8s_client(&self, identity_id: String, client: Client) {
        self.k8s_clients.write().await.insert(identity_id, client);
    }

    pub async fn has_k8s_client(&self, identity_id: &str) -> bool {
        self.k8s_clients.read().await.contains_key(identity_id)
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
    k8s_clients: K8sClients,
    /// Accept loops of the listeners bound by `c2.listen`, so `c2.stop-listener`
    /// can release their ports.
    listeners: Listeners,
    /// `labctl` children spawned by `c2.port-forward`, so `c2.stop-port-forward`
    /// can tear their tunnels down.
    redirectors: Redirectors,
    partial_executions: PartialExecutions,
}

impl C2Executor {
    /// Select the exact Kubernetes client requested by an action. An unknown
    /// identity must not silently fall back to Ran's default credentials.
    async fn client_for(&self, auth_identity_id: Option<&str>) -> Option<Client> {
        match auth_identity_id {
            Some(id) => self.k8s_clients.read().await.get(id).cloned(),
            None => self.k8s.clone(),
        }
    }
}

#[async_trait]
pub trait C2Backend: Send + Sync {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted;

    async fn execute_streaming(&self, cmd: &ExecTtp, output: OutputSink) -> TtpExecuted {
        let _ = output;
        self.execute(cmd).await
    }
}

#[async_trait]
impl C2Backend for BuiltinC2 {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
        self.execute(cmd).await
    }

    async fn execute_streaming(&self, cmd: &ExecTtp, output: OutputSink) -> TtpExecuted {
        self.execute_streaming(cmd, output).await
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
        let k8s_clients = Arc::new(RwLock::new(k8s_clients));

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
                k8s_clients: k8s_clients.clone(),
            },
            event_bus.clone(),
            Self {
                cmd_rx,
                executor: C2Executor {
                    event_bus,
                    backends,
                    k8s,
                    k8s_clients,
                    listeners: Listeners::default(),
                    redirectors: Redirectors::default(),
                    partial_executions: PartialExecutions::default(),
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
        let k8s_clients = Arc::new(RwLock::new(HashMap::new()));

        (
            C2Handle {
                cmd_tx,
                backends: backends.clone(),
                k8s_clients: k8s_clients.clone(),
            },
            event_bus.clone(),
            Self {
                cmd_rx,
                executor: C2Executor {
                    event_bus,
                    backends,
                    k8s: None,
                    k8s_clients,
                    listeners: Listeners::default(),
                    redirectors: Redirectors::default(),
                    partial_executions: PartialExecutions::default(),
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
        let (output, mut output_rx) = OutputSink::channel();
        let mut execution = Box::pin(self.execute_command(&cmd, output.clone()));
        let mut collector = OutputCollector::default();
        let mut ticker = tokio::time::interval_at(
            tokio::time::Instant::now() + OUTPUT_FLUSH_INTERVAL,
            OUTPUT_FLUSH_INTERVAL,
        );

        let mut event = loop {
            tokio::select! {
                result = &mut execution => break result,
                Some(fragment) = output_rx.recv() => {
                    collector.push(fragment);
                    if collector.should_flush() {
                        self.publish_output_batch(collector.take_batch(&cmd.id));
                    }
                }
                _ = ticker.tick() => {
                    self.publish_output_batch(collector.take_batch(&cmd.id));
                }
            }
        };
        drop(execution);

        while let Ok(fragment) = output_rx.try_recv() {
            collector.push(fragment);
        }
        collector.finish_decoding();
        self.publish_output_batch(collector.take_batch(&cmd.id));
        collector.preserve_partial_failure_output(&mut event);
        let partial = self.partial_executions.write().await.remove(&cmd.id);
        if self
            .event_bus
            .publish(C2Event::TtpExecuted {
                cmd: Box::new(cmd),
                event,
                partial,
            })
            .is_err()
        {
            debug!("no c2 event subscribers currently registered");
        }
    }

    fn publish_output_batch(&self, event: Option<C2Event>) {
        if let Some(event) = event {
            if self.event_bus.publish(event).is_err() {
                debug!("no c2 event subscribers currently registered");
            }
        }
    }

    async fn execute_command(&self, cmd: &ExecTtp, output: OutputSink) -> TtpExecuted {
        match &cmd.operation {
            ExecutionOperation::ReadLocalKubeconfig { path } => {
                self.read_local_kubeconfig(cmd, path.clone())
            }
            ExecutionOperation::SelfSubjectRulesReview { namespace } => {
                let Some(k8s) = self.client_for(cmd.auth_identity_id.as_deref()).await else {
                    return failed_result(cmd, "no active Kubernetes client configured");
                };
                match k8s.self_subject_rules_review(namespace).await {
                    Ok(response) => TtpExecuted {
                        id: cmd.id.clone(),
                        success: true,
                        results: vec![response],
                        exit_code: 0,
                        fail_reason: String::new(),
                        session_connected: None,
                    },
                    Err(error) => failed_result(cmd, &format!("{error:#}")),
                }
            }
            ExecutionOperation::KubernetesExecSession { container } => {
                let target_entity_id = cmd
                    .args
                    .get("TARGET_ID")
                    .map(String::as_str)
                    .unwrap_or(&cmd.target_id)
                    .to_string();
                let backend_id = kubectl_exec_backend_id(&target_entity_id, container.as_deref());
                let Some(k8s) = self.client_for(cmd.auth_identity_id.as_deref()).await else {
                    return failed_result(cmd, "no active Kubernetes client configured");
                };
                match open_kubectl_exec_session(
                    self.backends.clone(),
                    self.event_bus.clone(),
                    k8s,
                    backend_id,
                    target_entity_id,
                    container.clone(),
                    cmd.id.clone(),
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
                }
            }
            ExecutionOperation::KubernetesRequest { request } => {
                let Some(k8s) = self.client_for(cmd.auth_identity_id.as_deref()).await else {
                    return failed_result(cmd, "no active Kubernetes client configured");
                };
                match k8s.execute_request(request).await {
                    Ok(stdout) => command_output_result(
                        cmd,
                        PodExecOutput {
                            stdout,
                            stderr: String::new(),
                            exit_code: 0,
                        },
                    ),
                    Err(error) => failed_result(cmd, &error.to_string()),
                }
            }
            ExecutionOperation::AuthenticatedHttpRequest { request } => {
                let Some(k8s) = self.client_for(cmd.auth_identity_id.as_deref()).await else {
                    return failed_result(cmd, "no active Kubernetes client configured");
                };
                match k8s.execute_authenticated_http_request(request).await {
                    Ok(stdout) => command_output_result(
                        cmd,
                        PodExecOutput {
                            stdout,
                            stderr: String::new(),
                            exit_code: 0,
                        },
                    ),
                    Err(error) => failed_result(cmd, &error.to_string()),
                }
            }
            ExecutionOperation::KubernetesCommand { command } => {
                let Some(k8s) = self.client_for(cmd.auth_identity_id.as_deref()).await else {
                    return failed_result(cmd, "no active Kubernetes client configured");
                };
                let sink = output.clone();
                let observer: ExecOutputObserver = Arc::new(move |stream, bytes| match stream {
                    ExecOutputStream::Stdout => sink.stdout(bytes.to_vec()),
                    ExecOutputStream::Stderr => sink.stderr(bytes.to_vec()),
                });
                let timeout_seconds = cmd.execution_timeout_seconds.max(1);
                match tokio::time::timeout(
                    Duration::from_secs(timeout_seconds),
                    k8s.execute_kubectl_command_streaming(command, Some(observer)),
                )
                .await
                {
                    Ok(Ok(result)) => command_output_result(cmd, result),
                    Ok(Err(error)) => failed_result(cmd, &error.to_string()),
                    Err(_) => failed_result(
                        cmd,
                        &format!("kubectl command timed out after {timeout_seconds}s"),
                    ),
                }
            }
            ExecutionOperation::Noop => TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec!["ok".to_string()],
                exit_code: 0,
                fail_reason: String::new(),
                session_connected: None,
            },
            ExecutionOperation::StartListener { port, protocol } => {
                let backend_id = session_backend_id_from_cmd(cmd);
                let target_entity_id = cmd
                    .args
                    .get("TARGET_ID")
                    .map(String::as_str)
                    .unwrap_or(&cmd.target_id)
                    .to_string();
                if let Err(error) = self
                    .spawn_session_listener(ListenerSpec {
                        cmd_id: cmd.id.clone(),
                        backend_id,
                        target_entity_id,
                        port: *port,
                        protocol: protocol.clone(),
                    })
                    .await
                {
                    return failed_result(
                        cmd,
                        &format!("failed to bind listener on port {port}: {error}"),
                    );
                }
                TtpExecuted {
                    id: cmd.id.clone(),
                    success: true,
                    results: vec![format!("listener started on port {port}")],
                    exit_code: 0,
                    fail_reason: String::new(),
                    session_connected: None,
                }
            }
            ExecutionOperation::StopListener { listener } => {
                self.stop_listener(cmd, listener).await
            }
            ExecutionOperation::StartRedirector {
                play_id,
                remote_port,
                listener,
            } => {
                self.start_redirector(cmd, play_id, *remote_port, listener)
                    .await
            }
            ExecutionOperation::StopRedirector { redirector } => {
                self.stop_redirector(cmd, redirector).await
            }
            ExecutionOperation::LocalShell { command } => {
                self.run_local_command(cmd, command, output).await
            }
            ExecutionOperation::Shell { .. } => {
                let backend = match self.select_backend(cmd).await {
                    Ok(backend) => backend,
                    Err(reason) => return failed_result(cmd, &reason),
                };
                let mut event = backend.execute_streaming(cmd, output).await;
                event.session_connected = None;
                event
            }
        }
    }

    /// Execute a procedure explicitly marked `isLocal` on the host running Ran.
    ///
    /// This is deliberately handled before C2 backend selection.  Local
    /// procedures are not pod-exec commands with relaxed routing requirements:
    /// their process exit status is the action result the operator must see.
    async fn run_local_command(
        &self,
        cmd: &ExecTtp,
        command: &str,
        output: OutputSink,
    ) -> TtpExecuted {
        let command = command.trim();
        if command.is_empty() {
            return failed_result(cmd, "local procedure has an empty command");
        }

        let timeout_seconds = cmd.execution_timeout_seconds.max(1);
        tracing::info!(
            cmd_id = %cmd.id,
            target_id = %cmd.target_id,
            procedure_id = %cmd.procedure.id,
            timeout_seconds,
            command,
            "executing local procedure on operator host"
        );

        let mut process = tokio::process::Command::new("sh");
        process
            .args(["-c", command])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let output =
            match run_streamed_child(process, Duration::from_secs(timeout_seconds), output).await {
                Err(error) => {
                    let reason = format!("failed to start local command: {error}");
                    tracing::warn!(cmd_id = %cmd.id, target_id = %cmd.target_id, %reason);
                    return failed_result(cmd, &reason);
                }
                Ok(output) => output,
            };

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let exit_code = output.exit_code;
        let mut results = Vec::new();
        if !stdout.is_empty() {
            results.push(stdout.clone());
        }
        if !stderr.is_empty() {
            if results.is_empty() {
                results.push(String::new());
            }
            results.push(stderr.clone());
        }

        if output.timed_out {
            let reason = format!("local command timed out after {timeout_seconds}s");
            tracing::warn!(cmd_id = %cmd.id, target_id = %cmd.target_id, %reason);
            return TtpExecuted {
                id: cmd.id.clone(),
                success: false,
                results,
                exit_code,
                fail_reason: reason,
                session_connected: None,
            };
        }

        if output.success {
            tracing::info!(cmd_id = %cmd.id, target_id = %cmd.target_id, exit_code, "local procedure completed");
            TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results,
                exit_code,
                fail_reason: String::new(),
                session_connected: None,
            }
        } else {
            let reason = stderr
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .or_else(|| stdout.lines().rev().find(|line| !line.trim().is_empty()))
                .map(str::to_string)
                .unwrap_or_else(|| format!("local command exited with code {exit_code}"));
            tracing::warn!(
                cmd_id = %cmd.id,
                target_id = %cmd.target_id,
                exit_code,
                stderr = %stderr,
                fail_reason = %reason,
                "local procedure failed"
            );
            TtpExecuted {
                id: cmd.id.clone(),
                success: false,
                results,
                exit_code,
                fail_reason: reason,
                session_connected: None,
            }
        }
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

    /// Bind the listener socket and hand it to an accept loop.
    ///
    /// The bind happens here, not in the accept loop, so that a port conflict is
    /// a failed action the operator sees rather than a log line hidden behind a
    /// successful-looking result. It also means a port already held by a live
    /// listener keeps its registration: binding second fails before the map is
    /// touched, so the original stays stoppable via `c2.stop-listener`.
    async fn spawn_session_listener(&self, spec: ListenerSpec) -> std::io::Result<()> {
        use std::net::{Ipv4Addr, SocketAddr};

        let port = spec.port;
        let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(port, backend_id = %spec.backend_id, "session listener ready");

        let _ = self.event_bus.publish(C2Event::ListenerStarted {
            cmd_id: spec.cmd_id.clone(),
            port,
            protocol: spec.protocol.clone(),
        });

        let backends = self.backends.clone();
        let event_bus = self.event_bus.clone();
        let listeners = self.listeners.clone();
        let handle = tokio::spawn(async move {
            accept_session_loop(backends, event_bus, listeners, listener, spec).await;
        });
        self.listeners
            .write()
            .await
            .insert(port, handle.abort_handle());
        Ok(())
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
            Ok(TunnelStartup::AlreadyAttached) => {
                // `labctl` exited because the playground already owns this
                // reverse-control channel. There is no child for Ran to retain
                // or stop, but the redirector itself is present and must reach
                // the campaign so the UI reflects the actual playground state.
                warn!(
                    %entry,
                    "labctl found an already-attached redirector outside Ran's control"
                );
                self.partial_executions.write().await.insert(cmd.id.clone());
                let _ = self.event_bus.publish(C2Event::RedirectorStarted {
                    cmd_id: cmd.id.clone(),
                    via: REDIRECTOR_TOOL.to_string(),
                    play_id: play_id.to_string(),
                    remote_port,
                    listener_port,
                });
                return TtpExecuted {
                    id: cmd.id.clone(),
                    success: true,
                    results: vec![format!(
                        "redirector {entry} is already attached on playground {play_id}; \
                         Ran registered it as forwarding to 127.0.0.1:{listener_port}, but \
                         does not control or stop the existing tunnel"
                    )],
                    exit_code: 0,
                    fail_reason: String::new(),
                    session_connected: None,
                };
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

    async fn select_backend(&self, cmd: &ExecTtp) -> Result<Arc<dyn C2Backend>, String> {
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
                .cloned()
                .ok_or_else(|| "builtin c2 backend is not registered".to_string());
        }

        if let Some(backend) = backends.get(&key) {
            debug!(
                cmd_id = %cmd.id,
                target_id = %cmd.target_id,
                exec_system_id = %cmd.exec_system_id,
                exec_chain = ?cmd.exec_chain,
                "select_backend: exact match"
            );
            return Ok(backend.clone());
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
                return Ok(backend.clone());
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
                return Ok(backend.clone());
            }
        }

        warn!(
            cmd_id = %cmd.id,
            target_id = %cmd.target_id,
            exec_system_id = %cmd.exec_system_id,
            exec_chain = ?cmd.exec_chain,
            "select_backend: backend not found"
        );
        Err(format!(
            "execution backend '{}' is not registered",
            cmd.exec_system_id
        ))
    }
}

struct StreamedChildOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: i32,
    success: bool,
    timed_out: bool,
}

async fn drain_child_stream<R>(
    mut reader: R,
    stream: OutputStream,
    output: OutputSink,
) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut captured = Vec::new();
    let mut buffer = vec![0u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let fragment = buffer[..read].to_vec();
        match stream {
            OutputStream::Stdout => output.stdout(fragment.clone()),
            OutputStream::Stderr => output.stderr(fragment.clone()),
        }
        captured.extend_from_slice(&fragment);
    }
    Ok(captured)
}

async fn run_streamed_child(
    mut command: tokio::process::Command,
    timeout: Duration,
    output: OutputSink,
) -> Result<StreamedChildOutput, String> {
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "child stdout was not piped".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "child stderr was not piped".to_string())?;
    let stdout_task = tokio::spawn(drain_child_stream(
        stdout,
        OutputStream::Stdout,
        output.clone(),
    ));
    let stderr_task = tokio::spawn(drain_child_stream(stderr, OutputStream::Stderr, output));

    let (status, timed_out) = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => (status, false),
        Ok(Err(error)) => return Err(format!("failed waiting for child process: {error}")),
        Err(_) => {
            let _ = child.kill().await;
            let status = child
                .wait()
                .await
                .map_err(|error| format!("failed reaping timed-out child process: {error}"))?;
            (status, true)
        }
    };
    let stdout = stdout_task
        .await
        .map_err(|error| format!("stdout reader task failed: {error}"))?
        .map_err(|error| format!("failed reading child stdout: {error}"))?;
    let stderr = stderr_task
        .await
        .map_err(|error| format!("stderr reader task failed: {error}"))?
        .map_err(|error| format!("failed reading child stderr: {error}"))?;

    Ok(StreamedChildOutput {
        stdout,
        stderr,
        exit_code: status.code().unwrap_or(-1),
        success: status.success(),
        timed_out,
    })
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
    event_bus: C2EventBus,
    k8s: Client,
    backend_id: String,
    target_entity_id: String,
    container: Option<String>,
    opened_by_cmd_id: String,
) -> Result<crate::types::SessionConnectedData, String> {
    let (ns, pod) = split_pod_entity_id(&target_entity_id).ok_or_else(|| {
        format!(
            "target '{}' is not a pod entity (expected ns/<ns>/pod/<name>)",
            target_entity_id
        )
    })?;
    let (ns, pod) = (ns.to_string(), pod.to_string());

    let stream = match k8s.open_exec_session(&ns, &pod, container.as_deref()).await {
        Ok(stream) => stream,
        Err(error) => {
            // Kubernetes can return an opaque WebSocket-upgrade 400 when an
            // exec request omitted a container for a multi-container Pod.
            // Check the current Pod before offering that guidance, but retain
            // the original error because the upgrade response alone does not
            // prove why it was rejected.
            let containers = if container.is_none() {
                k8s.pod_container_names(&ns, &pod).await.ok()
            } else {
                None
            };
            return Err(kubectl_exec_open_failure(
                &target_entity_id,
                container.as_deref(),
                containers.as_deref(),
                &format!("{error:#}"),
            ));
        }
    };

    let (rx, tx) = tokio::io::split(stream);
    let session = crate::ShellSession::from_rw(rx, tx, &backend_id);

    session.init().await.map_err(|error| {
        format!(
            "kubectl exec shell initialization failed for {target_entity_id}: {error}; \
             the selected container may not provide /bin/sh"
        )
    })?;

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

    let session = Arc::new(session);
    let health = session.subscribe_health();
    let backend: Arc<dyn C2Backend> = session;
    backends
        .write()
        .await
        .insert(backend_id.clone(), backend.clone());
    let events = event_bus.subscribe();
    tokio::spawn(monitor_session_health_after_execution(
        backends.clone(),
        event_bus,
        events,
        opened_by_cmd_id,
        backend,
        health,
        backend_id.clone(),
        target_entity_id.clone(),
    ));

    Ok(crate::types::SessionConnectedData {
        backend_id,
        target_entity_id,
        hostname,
        user,
        os,
    })
}

fn kubectl_exec_open_failure(
    target_entity_id: &str,
    selected_container: Option<&str>,
    pod_containers: Option<&[String]>,
    error: &str,
) -> String {
    let Some(containers) = pod_containers.filter(|containers| containers.len() > 1) else {
        return format!("kubectl exec open failed for {target_entity_id}: {error}");
    };

    if selected_container.is_some() {
        return format!("kubectl exec open failed for {target_entity_id}: {error}");
    }

    format!(
        "kubectl exec could not open a session for {target_entity_id}. The request did not select a container, and Kubernetes currently reports multiple containers in this Pod: {}. Select a container and try again. Kubernetes reported: {error}",
        containers.join(", ")
    )
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

/// What [`await_tunnel_ready`] concluded about a redirector startup attempt.
#[derive(Debug)]
enum TunnelStartup {
    /// The tool said the tunnel is up.
    Ready,
    /// The tool is alive but never said so within the timeout. Not a failure -
    /// the process would have exited had it rejected its arguments - but worth
    /// repeating to the operator rather than reporting a bare success.
    StillSilent,
    /// A `labctl`-specific reverse-control conflict. The playground already
    /// owns the redirector, but it was not started by this Ran process and
    /// cannot be controlled through Ran.
    AlreadyAttached,
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
/// `Err` means the tunnel definitely did not come up. The one labctl-specific
/// exception, an already-attached reverse-control channel, is represented by
/// [`TunnelStartup::AlreadyAttached`]. Other errors carry the tool's last words
/// so a bad `PLAY_ID` is diagnosable rather than a silent no-op.
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
                // The process can exit before the independent pipe-draining
                // tasks get their first scheduling turn. Give them a bounded
                // chance to forward the final diagnostic before reporting the
                // failure, otherwise a rejected `labctl` invocation looks like
                // it produced no output.
                if draining {
                    while let Ok(Some(line)) = tokio::time::timeout(
                            std::time::Duration::from_millis(250),
                            lines.recv(),
                        )
                        .await
                    {
                        transcript.push(line);
                    }
                }
                let reason = format!("{detail}: {}", quote_transcript(&transcript));
                if is_existing_redirector_conflict(&reason) {
                    return Ok(TunnelStartup::AlreadyAttached);
                }
                return Err(reason);
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

/// Whether labctl's exit means a reverse-control channel is already attached.
///
/// This deliberately recognizes the full labctl diagnostic rather than treating
/// every HTTP conflict as success. Only that exact conflict means the playground
/// has already accepted a redirector which Ran can show but cannot manage.
fn is_existing_redirector_conflict(reason: &str) -> bool {
    let lowered = reason.to_ascii_lowercase();
    lowered.contains("error dialing reverse control ws")
        && (lowered.contains("status 409") || lowered.contains("409 conflict"))
}

/// Translate another labctl startup failure into something an operator can act on.
///
/// The already-attached reverse-control conflict is handled above as a success.
/// Other conflicts retain the existing diagnostic because they cannot safely be
/// assumed to describe an externally managed redirector.
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
    listener: tokio::net::TcpListener,
    spec: ListenerSpec,
) {
    use crate::ShellSession;

    let ListenerSpec {
        backend_id,
        target_entity_id,
        port,
        ..
    } = spec;

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

                let session = Arc::new(session);
                let health = session.subscribe_health();
                let backend: Arc<dyn C2Backend> = session;
                backends
                    .write()
                    .await
                    .insert(backend_id.clone(), backend.clone());
                let publish_result = event_bus.publish(C2Event::SessionConnected {
                    backend_id: backend_id.clone(),
                    target_entity_id: target_entity_id.clone(),
                    hostname,
                    user,
                    os,
                    port: Some(port),
                });
                tracing::info!(%backend_id, receivers = ?publish_result, "SessionConnected published");
                tokio::spawn(monitor_session_health(
                    backends.clone(),
                    event_bus.clone(),
                    backend,
                    health,
                    backend_id.clone(),
                    target_entity_id,
                ));
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

async fn monitor_session_health(
    backends: Backends,
    event_bus: C2EventBus,
    backend: Arc<dyn C2Backend>,
    mut health: tokio::sync::watch::Receiver<crate::shell_session::SessionHealth>,
    backend_id: String,
    target_entity_id: String,
) {
    loop {
        let state = *health.borrow_and_update();
        // A successful idle heartbeat briefly occupies the single shell stream,
        // transitioning through Busy and back to Responsive. Those are normal
        // operation, not useful health diagnostics.
        if matches!(
            state,
            crate::shell_session::SessionHealth::Suspect
                | crate::shell_session::SessionHealth::Lost
        ) {
            tracing::debug!(%backend_id, ?state, "shell session health changed");
        }
        if state == crate::shell_session::SessionHealth::Lost {
            let is_current = backends
                .read()
                .await
                .get(&backend_id)
                .is_some_and(|current| Arc::ptr_eq(current, &backend));
            if is_current {
                let _ = event_bus.publish(C2Event::SessionLost {
                    backend_id,
                    target_entity_id,
                });
            } else {
                tracing::debug!(%backend_id, "ignoring loss from a superseded shell session");
            }
            return;
        }
        if health.changed().await.is_err() {
            return;
        }
    }
}

/// A kubectl-exec session is returned inside the TTP result that created it.
/// Wait until that result is on the event bus before publishing health changes,
/// so the campaign always attaches the session before it can process its loss.
#[allow(clippy::too_many_arguments)]
async fn monitor_session_health_after_execution(
    backends: Backends,
    event_bus: C2EventBus,
    mut events: broadcast::Receiver<C2Event>,
    opened_by_cmd_id: String,
    backend: Arc<dyn C2Backend>,
    health: tokio::sync::watch::Receiver<crate::shell_session::SessionHealth>,
    backend_id: String,
    target_entity_id: String,
) {
    loop {
        match events.recv().await {
            Ok(C2Event::TtpExecuted { cmd, .. }) if cmd.id == opened_by_cmd_id => break,
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
    monitor_session_health(
        backends,
        event_bus,
        backend,
        health,
        backend_id,
        target_entity_id,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use armory::{Procedure, Ttp};
    use tokio::sync::{broadcast, mpsc, watch, Mutex, RwLock, Semaphore};

    use super::{
        await_tunnel_ready, drain_child_output, is_existing_redirector_conflict,
        looks_like_an_error, monitor_session_health, quote_transcript, redirector_forward_spec,
        tunnel_failure_hint, Backends, C2EventBus, C2Executor, RedirectorProcess, Redirectors,
        TunnelStartup,
    };
    use super::{C2Backend, C2Event, C2Manager, ExecTtp, TtpExecuted, BUILTIN_C2_ID};
    use crate::ExecutionOperation;

    static LISTENER_TEST_LOCK: Mutex<()> = Mutex::const_new(());

    struct MockBackend {
        marker: String,
    }

    struct BlockingBackend {
        started: mpsc::UnboundedSender<String>,
        release: Arc<Semaphore>,
    }

    #[test]
    fn exec_open_failure_explains_unselected_multicontainer_pod_without_claiming_cause() {
        let containers = vec!["redis".to_string(), "metric-receiver".to_string()];
        let message = super::kubectl_exec_open_failure(
            "ns/oopservability/pod/redis",
            None,
            Some(&containers),
            "failed to switch protocol: 400 Bad Request",
        );

        assert!(message.contains("did not select a container"));
        assert!(message.contains("redis, metric-receiver"));
        assert!(message.contains("Kubernetes reported: failed to switch protocol"));
        assert!(!message.contains("because Kubernetes requires"));
    }

    #[test]
    fn exec_open_failure_keeps_original_error_when_a_container_was_selected() {
        let containers = vec!["redis".to_string(), "metric-receiver".to_string()];
        let message = super::kubectl_exec_open_failure(
            "ns/oopservability/pod/redis",
            Some("redis"),
            Some(&containers),
            "failed to switch protocol: 400 Bad Request",
        );

        assert_eq!(
            message,
            "kubectl exec open failed for ns/oopservability/pod/redis: failed to switch protocol: 400 Bad Request"
        );
    }

    #[tokio::test]
    async fn current_session_transport_loss_is_published_independently_of_actions() {
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "session".to_string(),
        });
        let backend_id = "session/node-victim-4444".to_string();
        let backends: Backends = Arc::new(RwLock::new(HashMap::from([(
            backend_id.clone(),
            backend.clone(),
        )])));
        let event_bus = C2EventBus::new(4);
        let mut events = event_bus.subscribe();
        let (health_tx, health_rx) =
            watch::channel(crate::shell_session::SessionHealth::Responsive);

        tokio::spawn(monitor_session_health(
            backends,
            event_bus,
            backend,
            health_rx,
            backend_id.clone(),
            "node/victim".to_string(),
        ));
        health_tx
            .send(crate::shell_session::SessionHealth::Lost)
            .expect("monitor is subscribed");

        match tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("session loss should publish")
            .expect("event bus should remain open")
        {
            C2Event::SessionLost {
                backend_id: actual_backend_id,
                target_entity_id,
            } => {
                assert_eq!(actual_backend_id, backend_id);
                assert_eq!(target_entity_id, "node/victim");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn superseded_session_loss_does_not_break_the_reconnected_session() {
        let old_backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "old".to_string(),
        });
        let new_backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "new".to_string(),
        });
        let backend_id = "session/node-victim-4444".to_string();
        let backends: Backends = Arc::new(RwLock::new(HashMap::from([(
            backend_id.clone(),
            new_backend,
        )])));
        let event_bus = C2EventBus::new(4);
        let mut events = event_bus.subscribe();
        let (health_tx, health_rx) =
            watch::channel(crate::shell_session::SessionHealth::Responsive);

        tokio::spawn(monitor_session_health(
            backends,
            event_bus,
            old_backend,
            health_rx,
            backend_id,
            "node/victim".to_string(),
        ));
        health_tx
            .send(crate::shell_session::SessionHealth::Lost)
            .expect("monitor is subscribed");

        match tokio::time::timeout(Duration::from_millis(100), events.recv()).await {
            Err(_) | Ok(Err(broadcast::error::RecvError::Closed)) => {}
            Ok(Ok(event)) => panic!("superseded session unexpectedly published: {event:?}"),
            Ok(Err(error)) => panic!("event bus unexpectedly lagged: {error}"),
        }
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
    async fn local_procedure_runs_on_the_operator_host_and_reports_its_exit_status() {
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "backend should not run".to_string(),
        });
        let mut backends = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();
        let manager_task = tokio::spawn(manager.run());

        let mut cmd = exec_cmd("ran");
        cmd.procedure = Procedure {
            is_local_command: Some(true),
            ..Procedure::new(
                "local-test",
                "printf local-output; printf local-error >&2; exit 7",
            )
        };
        cmd.operation = ExecutionOperation::LocalShell {
            command: cmd.procedure.command.clone(),
        };
        handle.send(cmd).await.expect("command should queue");

        let mut saw_output = false;
        loop {
            match rx.recv().await.expect("execution event should publish") {
                C2Event::TtpOutput { stdout, stderr, .. } => {
                    saw_output = true;
                    assert_eq!(stdout, "local-output");
                    assert_eq!(stderr, "local-error");
                }
                C2Event::TtpExecuted { event, .. } => {
                    assert!(!event.success);
                    assert_eq!(event.exit_code, 7);
                    assert_eq!(event.results, vec!["local-output", "local-error"]);
                    assert_eq!(event.fail_reason, "local-error");
                    break;
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert!(saw_output, "local output should publish before completion");

        drop(handle);
        manager_task
            .await
            .expect("manager should shut down cleanly");
    }

    #[tokio::test]
    async fn local_procedure_publishes_output_while_command_is_still_running() {
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "backend should not run".to_string(),
        });
        let mut backends = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);
        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();
        let manager_task = tokio::spawn(manager.run());

        let mut cmd = exec_cmd("ran");
        cmd.procedure = Procedure {
            is_local_command: Some(true),
            ..Procedure::new("local-stream", "printf first; sleep 1; printf second")
        };
        cmd.operation = ExecutionOperation::LocalShell {
            command: cmd.procedure.command.clone(),
        };
        handle.send(cmd).await.expect("command should queue");

        let first = tokio::time::timeout(Duration::from_millis(800), rx.recv())
            .await
            .expect("output should arrive before the command exits")
            .expect("event bus should remain open");
        match first {
            C2Event::TtpOutput { stdout, .. } => assert_eq!(stdout, "first"),
            other => panic!("expected live output, got {other:?}"),
        }

        loop {
            if let C2Event::TtpExecuted { event, .. } =
                rx.recv().await.expect("completion should publish")
            {
                assert!(event.success);
                assert_eq!(event.results, vec!["firstsecond"]);
                break;
            }
        }

        drop(handle);
        manager_task
            .await
            .expect("manager should shut down cleanly");
    }

    #[tokio::test]
    async fn unknown_exec_system_id_fails_closed() {
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
                assert!(!event.success);
                assert!(event.fail_reason.contains("not registered"));
            }
            other => panic!("unexpected event: {:?}", other),
        }

        drop(handle);
        manager_task
            .await
            .expect("manager should shut down cleanly");
    }

    #[tokio::test]
    async fn control_looking_shell_text_does_not_select_a_control_operation() {
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "shell backend".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();
        let manager_task = tokio::spawn(manager.run());

        let mut cmd = exec_cmd("ran");
        cmd.procedure.command = "c2.stop-listener(tcp/9)".to_string();
        cmd.operation = ExecutionOperation::Shell {
            command: cmd.procedure.command.clone(),
        };
        handle.send(cmd).await.expect("command should queue");

        match rx.recv().await.expect("event should be published") {
            C2Event::TtpExecuted { event, .. } => {
                assert!(event.success);
                assert_eq!(event.results, ["shell backend"]);
            }
            other => panic!("unexpected event: {other:?}"),
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

    /// Run one typed control operation through the manager and return its result.
    async fn run_control_operation(
        operation: ExecutionOperation,
    ) -> (TtpExecuted, broadcast::Receiver<C2Event>) {
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
        cmd.operation = operation;
        handle.send(cmd).await.expect("command should queue");
        (wait_for_execution(&mut { rx }).await, events.subscribe())
    }

    async fn wait_for_execution(rx: &mut broadcast::Receiver<C2Event>) -> TtpExecuted {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), rx.recv())
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
    async fn a_port_conflict_fails_the_listen_action() {
        let _listener_test_guard = LISTENER_TEST_LOCK.lock().await;
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();
        tokio::spawn(manager.run());

        // Hold the port for the whole test, standing in for the listener that
        // outlived a campaign reset. Same skip as the release test: a sandbox
        // that forbids binding cannot exercise this at all.
        let squatter = match tokio::net::TcpListener::bind("0.0.0.0:0").await {
            Ok(squatter) => squatter,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipped: this environment does not permit binding sockets");
                return;
            }
            Err(error) => panic!("squatter bind failed: {error}"),
        };
        let port = squatter
            .local_addr()
            .expect("squatter has an address")
            .port();

        let mut listen = exec_cmd("ran");
        listen.id = "cmd-listen".to_string();
        listen.procedure = Procedure::new("ran", "id");
        listen.operation = ExecutionOperation::StartListener {
            port,
            protocol: "tcp".to_string(),
        };
        handle.send(listen).await.expect("listen should queue");

        let execution = wait_for_execution(&mut rx).await;
        // The operator has to see this. Reporting success and logging the bind
        // error is what left the UI showing a listener that never existed.
        assert!(
            !execution.success,
            "a port conflict must fail the action, got: {:?}",
            execution.results
        );
        assert!(
            execution.fail_reason.contains(&port.to_string()),
            "fail reason should name the port: {}",
            execution.fail_reason
        );

        drop(squatter);
    }

    #[tokio::test]
    async fn stopping_a_listener_releases_its_port() {
        let _listener_test_guard = LISTENER_TEST_LOCK.lock().await;
        let backend: Arc<dyn C2Backend> = Arc::new(MockBackend {
            marker: "builtin".to_string(),
        });
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), backend.clone());
        backends.insert("ran".to_string(), backend);

        let (handle, events, manager) = C2Manager::new_with_backends(8, backends);
        let mut rx = events.subscribe();
        tokio::spawn(manager.run());

        // Releasing a port-0 probe creates an unavoidable race with other
        // processes on the host. Retry a few kernel-selected ports so that race
        // does not masquerade as a listener lifecycle failure.
        let mut bound_port = None;
        for _ in 0..10 {
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
            listen.operation = ExecutionOperation::StartListener {
                port,
                protocol: "tcp".to_string(),
            };
            handle.send(listen).await.expect("listen should queue");

            loop {
                match tokio::time::timeout(Duration::from_secs(5), rx.recv())
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
                        assert_eq!(cmd_id, "cmd-listen");
                        bound_port = Some(port);
                        break;
                    }
                    C2Event::TtpExecuted { event, .. } if event.id == "cmd-listen" => {
                        if event.fail_reason.contains("Address already in use") {
                            break;
                        }
                        panic!("listener action failed before bind: {}", event.fail_reason);
                    }
                    _ => continue,
                }
            }
            if bound_port.is_some() {
                break;
            }
        }
        let port = bound_port.expect("listener should bind one of the probed ports");
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
        stop.operation = ExecutionOperation::StopListener {
            listener: format!("tcp/{port}"),
        };
        handle.send(stop).await.expect("stop should queue");

        let mut saw_stopped = false;
        let mut execution: Option<TtpExecuted> = None;
        while execution.is_none() || !saw_stopped {
            match tokio::time::timeout(Duration::from_secs(5), rx.recv())
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
        let (event, _events) = run_control_operation(ExecutionOperation::StopListener {
            listener: "tcp/9".to_string(),
        })
        .await;

        assert!(!event.success);
        assert!(
            event.fail_reason.contains("no listener is bound on port 9"),
            "unexpected reason: {}",
            event.fail_reason
        );
    }

    #[tokio::test]
    async fn stopping_a_malformed_listener_id_fails_without_touching_ports() {
        let (event, _events) = run_control_operation(ExecutionOperation::StopListener {
            listener: "not-a-listener".to_string(),
        })
        .await;

        assert!(!event.success);
        assert!(
            event.fail_reason.contains("is not a listener id"),
            "unexpected reason: {}",
            event.fail_reason
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

    #[tokio::test]
    async fn an_already_attached_redirector_is_not_reported_as_a_failure() {
        let mut child = fake_tunnel(
            "echo 'Tunnel error: error dialing reverse control WS (status 409): websocket: bad handshake' >&2; exit 1",
        );
        let mut lines = drain_child_output(&mut child, "play1/1337".to_string());

        let startup = await_tunnel_ready(&mut child, &mut lines, Duration::from_secs(5))
            .await
            .expect("the existing external redirector is a success case");
        assert!(matches!(startup, TunnelStartup::AlreadyAttached));
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

    /// Labctl only emits this precise 409 when a playground already owns the
    /// reverse-control channel. It is an externally managed redirector, not a
    /// failed attempt to create one.
    #[test]
    fn an_existing_reverse_control_channel_is_accepted() {
        let reason = "exited before the tunnel came up (exit status: 1): \
             Tunnel error: error dialing reverse control WS (status 409): websocket: bad handshake";

        assert!(is_existing_redirector_conflict(reason));
    }

    #[test]
    fn other_conflicts_remain_failures() {
        assert!(!is_existing_redirector_conflict(
            "exited before the tunnel came up: status 409"
        ));
        assert!(!is_existing_redirector_conflict(
            "exited before the tunnel came up: playground not found"
        ));
    }

    #[test]
    fn another_conflict_keeps_the_operator_diagnostic() {
        let hint = tunnel_failure_hint(
            "exited before the tunnel came up (exit status: 1): status 409",
            "play1",
            &["play1/9000".to_string()],
        )
        .expect("a non-exceptional 409 must retain its explanation");

        assert!(
            hint.contains("Ran is already holding a redirector"),
            "{hint}"
        );
        assert!(hint.contains("play1/9000"), "{hint}");
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
        stop.operation = ExecutionOperation::StopRedirector {
            redirector: "redirector/play1/1337".to_string(),
        };
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
    /// real long-running child, and run `operation` through it.
    async fn with_held_redirector(
        entry: &str,
        listener_port: u16,
        script: &str,
        operation: ExecutionOperation,
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
        exec.operation = operation;
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
            ExecutionOperation::StartRedirector {
                play_id: "play1".to_string(),
                remote_port: 1337,
                listener: "tcp/4444".to_string(),
            },
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
        let (event, _events) = run_control_operation(ExecutionOperation::StopRedirector {
            redirector: "play1/9999".to_string(),
        })
        .await;

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
        stop.operation = ExecutionOperation::StopRedirector {
            redirector: "play2/1337".to_string(),
        };
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
        let (event, _events) = run_control_operation(ExecutionOperation::StopRedirector {
            redirector: "not-a-redirector".to_string(),
        })
        .await;

        assert!(!event.success);
        assert!(
            event.fail_reason.contains("is not a redirector id"),
            "unexpected reason: {}",
            event.fail_reason
        );
    }

    #[tokio::test]
    async fn port_forwarding_to_a_malformed_listener_fails_before_spawning() {
        let (event, _events) = run_control_operation(ExecutionOperation::StartRedirector {
            play_id: "play1".to_string(),
            remote_port: 1337,
            listener: "not-a-listener".to_string(),
        })
        .await;

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
            operation: crate::ExecutionOperation::Shell {
                command: "id".to_string(),
            },
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
