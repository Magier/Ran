use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;
use tracing::warn;

use crate::executor::C2Backend;
use crate::output::OutputSink;
use crate::types::{ExecTtp, TtpExecuted};

static NONCE: AtomicU64 = AtomicU64::new(1);

/// A live shell session - bind shell, reverse shell, or any other async stream.
///
/// Commands are sent to the shell's stdin and output is framed with a
/// per-command sentinel so discrete stdout/exit_code results can be extracted
/// from the continuous byte stream.
///
/// Framing protocol (written to stdin for each command):
/// ```text
/// {
/// {cmd}
/// } 2>&1
/// __ran_status=$?
/// printf '\n'
/// printf '__RAN_{nonce}__:%d\n' "$__ran_status"
/// ```
/// Lines are read until the sentinel `__RAN_{nonce}__:{exit_code}` appears.
/// Everything before it is stdout (stderr merged via `2>&1`).
pub struct ShellSession {
    requests: mpsc::Sender<ShellRequest>,
    health: watch::Receiver<SessionHealth>,
    close_health: watch::Sender<SessionHealth>,
    actor: tokio::task::AbortHandle,
    /// Entity ID this session currently exits into (for logging/debugging).
    pub entity_id: String,
}

struct ShellInner {
    tx: Box<dyn AsyncWrite + Unpin + Send>,
    rx: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionHealth {
    Responsive,
    Busy,
    Suspect,
    Lost,
}

#[derive(Clone, Copy)]
struct SessionTiming {
    heartbeat_interval: Duration,
    heartbeat_timeout: Duration,
}

impl Default for SessionTiming {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(30),
            heartbeat_timeout: Duration::from_secs(5),
        }
    }
}

enum ShellRequest {
    Raw {
        command: String,
        deadline: Instant,
        reply: oneshot::Sender<Result<String, String>>,
    },
    Execute {
        command: Box<ExecTtp>,
        deadline: Instant,
        output_sink: OutputSink,
        reply: oneshot::Sender<TtpExecuted>,
    },
    Close {
        reply: oneshot::Sender<Result<(), String>>,
    },
}

/// Time to let an interactive shell process a graceful `exit` before local
/// teardown wins. A stuck frame must never leave the kill action blocked.
const GRACEFUL_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

enum PendingReply {
    Raw(Option<oneshot::Sender<Result<String, String>>>),
    Execute {
        cmd_id: String,
        output_sink: OutputSink,
        reply: Option<oneshot::Sender<TtpExecuted>>,
    },
    Heartbeat,
}

/// Frame a shell command with a sentinel on a line of its own. The explicit
/// newline matters for files such as Kubernetes ServiceAccount JWTs, which do
/// not necessarily end with one. The redirection is attached to a command
/// group, rather than appended to the command: an appended `2>&1` only affects
/// the final stage of a pipeline, leaving errors from earlier stages on a
/// socat process's local stderr instead of returning them through the session.
fn framed_command(command: &str, marker: &str) -> String {
    format!(
        "{{\n{command}\n}} 2>&1\n__ran_status=$?\nprintf '\\n'\nprintf '{marker}:%d\\n' \"$__ran_status\"\n"
    )
}

impl ShellSession {
    /// Dial a bind shell at `addr` and return an initialised session.
    pub async fn connect_bind(
        addr: impl AsRef<str>,
        entity_id: impl Into<String>,
    ) -> Result<Self, String> {
        let addr = addr.as_ref();
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| format!("bind shell connect to {addr} failed: {e}"))?;
        let session = Self::from_tcp(stream, entity_id);
        session.init().await?;
        Ok(session)
    }

    /// Wrap an already-established incoming TCP stream (reverse shell).
    /// Calls `init()` to drain the banner and configure the shell environment.
    pub async fn from_incoming(
        stream: TcpStream,
        entity_id: impl Into<String>,
    ) -> Result<Self, String> {
        let session = Self::from_tcp(stream, entity_id);
        session.init().await?;
        Ok(session)
    }

    fn from_tcp(stream: TcpStream, entity_id: impl Into<String>) -> Self {
        configure_tcp_keepalive(&stream);
        let (rx, tx) = tokio::io::split(stream);
        Self::from_rw(rx, tx, entity_id)
    }

    pub(crate) fn from_rw<R, W>(reader: R, writer: W, entity_id: impl Into<String>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::from_rw_with_timing(reader, writer, entity_id, SessionTiming::default())
    }

    fn from_rw_with_timing<R, W>(
        reader: R,
        writer: W,
        entity_id: impl Into<String>,
        timing: SessionTiming,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let entity_id = entity_id.into();
        let (request_tx, request_rx) = mpsc::channel(32);
        let (health_tx, health_rx) = watch::channel(SessionHealth::Responsive);
        let close_health = health_tx.clone();
        let inner = ShellInner {
            tx: Box::new(writer),
            rx: BufReader::new(Box::new(reader)),
        };
        let actor = tokio::spawn(run_session_actor(
            inner,
            request_rx,
            health_tx,
            timing,
            entity_id.clone(),
        ));
        Self {
            requests: request_tx,
            health: health_rx,
            close_health,
            actor: actor.abort_handle(),
            entity_id,
        }
    }

    pub(crate) fn subscribe_health(&self) -> watch::Receiver<SessionHealth> {
        self.health.clone()
    }

    /// Drain any shell banner and configure a clean execution environment.
    ///
    /// If a live shell does not echo the init marker within 5 s, log a warning
    /// and proceed because sentinel framing tolerates leftover prompt output.
    /// A transport that reaches EOF is rejected because it cannot become an
    /// executable session.
    pub async fn init(&self) -> Result<(), String> {
        let init_marker = "__RAN_INIT0__";
        let init_cmd = format!(
            "stty -echo 2>/dev/null; unset PROMPT_COMMAND PS1 PS2 HISTFILE 2>/dev/null\nprintf '{init_marker}\\n'\n"
        );

        match self.run_raw(&init_cmd).await {
            Ok(_) => {}
            Err(error) if *self.health.borrow() == SessionHealth::Lost => {
                return Err(error);
            }
            Err(error) => {
                warn!(entity_id = %self.entity_id, %error, "shell init failed; proceeding without clean init");
            }
        }
        Ok(())
    }

    /// Run a single command and return trimmed stdout.  Used for probing
    /// (hostname, whoami, uname) before the session is fully registered.
    /// Times out after 5 s - returns an error if the shell doesn't respond.
    pub async fn run_raw(&self, cmd: &str) -> Result<String, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let timeout = Duration::from_secs(5);
        let deadline = Instant::now() + timeout;
        let request = ShellRequest::Raw {
            command: cmd.to_string(),
            deadline,
            reply: reply_tx,
        };
        match tokio::time::timeout_at(deadline, self.requests.send(request)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => return Err(crate::types::SESSION_CLOSED_UNEXPECTEDLY.to_string()),
            Err(_) => return Err(format!("run_raw timed out waiting for response to '{cmd}'")),
        }
        match tokio::time::timeout_at(deadline, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(crate::types::SESSION_CLOSED_UNEXPECTEDLY.to_string()),
            Err(_) => Err(format!("run_raw timed out waiting for response to '{cmd}'")),
        }
    }

    /// Ask an interactive shell to exit, then stop the session actor.
    ///
    /// A normal `socat ... EXEC:/bin/sh` peer exits after its shell receives
    /// this unframed `exit`. The bounded fallback is essential because the
    /// actor can be draining a timed-out frame and cannot safely accept a new
    /// command until that stream recovers.
    pub(crate) async fn close(&self) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if matches!(
            tokio::time::timeout(
                GRACEFUL_CLOSE_TIMEOUT,
                self.requests.send(ShellRequest::Close { reply: reply_tx })
            )
            .await,
            Ok(Ok(()))
        ) {
            let _ = tokio::time::timeout(GRACEFUL_CLOSE_TIMEOUT, reply_rx).await;
        }

        // The actor owns both TCP halves. Aborting it drops those halves even
        // if an in-flight frame did not return, which closes the local socket
        // and unblocks callers waiting on a queued shell request.
        self.close_health.send_replace(SessionHealth::Lost);
        self.actor.abort();
        Ok(())
    }
}

#[async_trait]
impl C2Backend for ShellSession {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
        self.execute_streaming(cmd, OutputSink::discard()).await
    }

    async fn execute_streaming(&self, cmd: &ExecTtp, output_sink: OutputSink) -> TtpExecuted {
        let (reply_tx, reply_rx) = oneshot::channel();
        let timeout = Duration::from_secs(cmd.execution_timeout_seconds.max(1));
        let deadline = Instant::now() + timeout;
        let request = ShellRequest::Execute {
            command: Box::new(cmd.clone()),
            deadline,
            output_sink,
            reply: reply_tx,
        };
        match tokio::time::timeout_at(deadline, self.requests.send(request)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                return exec_error(
                    &cmd.id,
                    crate::types::SESSION_CLOSED_UNEXPECTEDLY.to_string(),
                );
            }
            Err(_) => return timeout_error(&cmd.id, timeout),
        }
        match tokio::time::timeout_at(deadline, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => exec_error(
                &cmd.id,
                crate::types::SESSION_CLOSED_UNEXPECTEDLY.to_string(),
            ),
            Err(_) => timeout_error(&cmd.id, timeout),
        }
    }

    async fn close(&self) -> Result<(), String> {
        ShellSession::close(self).await
    }
}

fn configure_tcp_keepalive(stream: &TcpStream) {
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(30))
        .with_interval(Duration::from_secs(10));
    if let Err(error) = socket2::SockRef::from(stream).set_tcp_keepalive(&keepalive) {
        warn!(%error, "failed to configure TCP keepalive for shell session");
    }
}

async fn run_session_actor(
    mut inner: ShellInner,
    mut requests: mpsc::Receiver<ShellRequest>,
    health: watch::Sender<SessionHealth>,
    timing: SessionTiming,
    entity_id: String,
) {
    let mut idle_bytes = Vec::new();
    loop {
        let heartbeat_wait = tokio::time::sleep(timing.heartbeat_interval);
        tokio::pin!(heartbeat_wait);
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { return };
                if let ShellRequest::Close { reply } = request {
                    let result = gracefully_close_shell(&mut inner).await;
                    health.send_replace(SessionHealth::Lost);
                    let _ = reply.send(result);
                    return;
                }
                if !run_request(&mut inner, request, &health, &entity_id).await {
                    return;
                }
            }
            read = inner.rx.read_until(b'\n', &mut idle_bytes) => {
                match read {
                    Ok(0) => {
                        warn!(%entity_id, "shell session EOF while idle");
                        health.send_replace(SessionHealth::Lost);
                        return;
                    }
                    Ok(_) => {
                        warn!(%entity_id, "discarding unexpected shell output while idle");
                        idle_bytes.clear();
                    }
                    Err(error) => {
                        warn!(%entity_id, %error, "shell session read failed while idle");
                        health.send_replace(SessionHealth::Lost);
                        return;
                    }
                }
            }
            _ = &mut heartbeat_wait => {
                if !run_heartbeat(&mut inner, &health, timing.heartbeat_timeout, &entity_id).await {
                    return;
                }
            }
        }
    }
}

async fn gracefully_close_shell(inner: &mut ShellInner) -> Result<(), String> {
    inner
        .tx
        .write_all(b"exit\n")
        .await
        .map_err(|error| format!("shell exit write failed: {error}"))?;
    inner
        .tx
        .flush()
        .await
        .map_err(|error| format!("shell exit flush failed: {error}"))?;

    // The session is being discarded, so any output from `exit` is irrelevant.
    // Wait only for EOF; otherwise force a local TCP shutdown after the grace
    // window and let dropping the actor release the read half too.
    let mut output = Vec::new();
    let _ = tokio::time::timeout(GRACEFUL_CLOSE_TIMEOUT, async {
        loop {
            match inner.rx.read_until(b'\n', &mut output).await {
                Ok(0) => return Ok::<(), String>(()),
                Ok(_) => output.clear(),
                Err(error) => return Err(format!("shell exit read failed: {error}")),
            }
        }
    })
    .await;
    let _ = inner.tx.shutdown().await;
    Ok(())
}

async fn run_request(
    inner: &mut ShellInner,
    request: ShellRequest,
    health: &watch::Sender<SessionHealth>,
    entity_id: &str,
) -> bool {
    let (command, timeout, deadline, mut reply) = match request {
        ShellRequest::Raw {
            command,
            deadline,
            reply,
        } => (
            command,
            Duration::from_secs(5),
            deadline,
            PendingReply::Raw(Some(reply)),
        ),
        ShellRequest::Execute {
            command,
            deadline,
            output_sink,
            reply,
        } => {
            let timeout = Duration::from_secs(command.execution_timeout_seconds.max(1));
            let cmd_id = command.id.clone();
            let Some(shell_command) = command.operation.command().map(str::to_string) else {
                let _ = reply.send(exec_error(
                    &cmd_id,
                    "shell session received a non-shell execution operation".to_string(),
                ));
                return true;
            };
            (
                shell_command,
                timeout,
                deadline,
                PendingReply::Execute {
                    cmd_id,
                    output_sink,
                    reply: Some(reply),
                },
            )
        }
        ShellRequest::Close { .. } => {
            unreachable!("close requests exit the session actor directly")
        }
    };
    if deadline <= Instant::now() {
        finish_with_timeout(&mut reply, timeout, &command);
        return true;
    }
    run_frame(inner, &command, timeout, deadline, reply, health, entity_id).await
}

async fn run_heartbeat(
    inner: &mut ShellInner,
    health: &watch::Sender<SessionHealth>,
    timeout: Duration,
    entity_id: &str,
) -> bool {
    run_frame(
        inner,
        ":",
        timeout,
        Instant::now() + timeout,
        PendingReply::Heartbeat,
        health,
        entity_id,
    )
    .await
}

async fn run_frame(
    inner: &mut ShellInner,
    command: &str,
    timeout: Duration,
    deadline_at: Instant,
    mut reply: PendingReply,
    health: &watch::Sender<SessionHealth>,
    entity_id: &str,
) -> bool {
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let marker = format!("__RAN_{nonce}__");
    let payload = framed_command(command, &marker);

    if let Err(error) = inner.tx.write_all(payload.as_bytes()).await {
        finish_with_error(&mut reply, format!("shell write failed: {error}"));
        health.send_replace(SessionHealth::Lost);
        return false;
    }
    if let Err(error) = inner.tx.flush().await {
        finish_with_error(&mut reply, format!("shell flush failed: {error}"));
        health.send_replace(SessionHealth::Lost);
        return false;
    }

    health.send_replace(SessionHealth::Busy);
    let deadline = tokio::time::sleep_until(deadline_at);
    tokio::pin!(deadline);
    let mut timed_out = false;
    let mut output = String::new();
    let mut line = Vec::new();

    loop {
        tokio::select! {
            read = inner.rx.read_until(b'\n', &mut line) => {
                match read {
                    Ok(0) => {
                        warn!(%entity_id, "shell session EOF");
                        finish_with_error(
                            &mut reply,
                            crate::types::SESSION_CLOSED_UNEXPECTEDLY.to_string(),
                        );
                        health.send_replace(SessionHealth::Lost);
                        return false;
                    }
                    Err(error) => {
                        finish_with_error(&mut reply, format!("shell read failed: {error}"));
                        health.send_replace(SessionHealth::Lost);
                        return false;
                    }
                    Ok(_) => {}
                }

                let text = String::from_utf8_lossy(&line);
                let trimmed = text.trim_end_matches(['\r', '\n']);
                if let Some(code) = trimmed.strip_prefix(&format!("{marker}:")) {
                    if !timed_out {
                        finish_with_success(&mut reply, code.parse().unwrap_or(1), output);
                    }
                    health.send_replace(SessionHealth::Responsive);
                    return true;
                }
                if !timed_out {
                    if let PendingReply::Execute { output_sink, .. } = &reply {
                        output_sink.stdout(line.clone());
                    }
                    output.push_str(&text);
                }
                line.clear();
            }
            _ = &mut deadline, if !timed_out => {
                finish_with_timeout(&mut reply, timeout, command);
                timed_out = true;
                output.clear();
                health.send_replace(SessionHealth::Suspect);
                warn!(%entity_id, "shell command or heartbeat timed out; continuing to drain its response");
            }
        }
    }
}

fn finish_with_success(reply: &mut PendingReply, exit_code: i32, output: String) {
    match reply {
        PendingReply::Raw(sender) => {
            let value = output
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim()
                .to_string();
            if let Some(sender) = sender.take() {
                let _ = sender.send(Ok(value));
            }
        }
        PendingReply::Execute { cmd_id, reply, .. } => {
            let stdout = output.trim_end().to_string();
            let success = exit_code == 0;
            let fail_reason = if success {
                String::new()
            } else if stdout.is_empty() {
                format!("exit code {exit_code}")
            } else {
                stdout.lines().last().unwrap_or("").to_string()
            };
            if let Some(reply) = reply.take() {
                let _ = reply.send(TtpExecuted {
                    id: cmd_id.clone(),
                    success,
                    results: if stdout.is_empty() {
                        vec![]
                    } else {
                        vec![stdout]
                    },
                    exit_code,
                    fail_reason,
                    session_connected: None,
                });
            }
        }
        PendingReply::Heartbeat => {}
    }
}

fn finish_with_timeout(reply: &mut PendingReply, timeout: Duration, command: &str) {
    match reply {
        PendingReply::Raw(sender) => {
            if let Some(sender) = sender.take() {
                let _ = sender.send(Err(format!(
                    "run_raw timed out waiting for response to '{command}'"
                )));
            }
        }
        PendingReply::Execute { cmd_id, reply, .. } => {
            if let Some(reply) = reply.take() {
                let _ = reply.send(timeout_error(cmd_id, timeout));
            }
        }
        PendingReply::Heartbeat => {}
    }
}

fn timeout_error(cmd_id: &str, timeout: Duration) -> TtpExecuted {
    exec_error(
        cmd_id,
        format!("shell command timed out after {}s", timeout.as_secs()),
    )
}

fn finish_with_error(reply: &mut PendingReply, reason: String) {
    match reply {
        PendingReply::Raw(sender) => {
            if let Some(sender) = sender.take() {
                let _ = sender.send(Err(reason));
            }
        }
        PendingReply::Execute { cmd_id, reply, .. } => {
            if let Some(reply) = reply.take() {
                let _ = reply.send(exec_error(cmd_id, reason));
            }
        }
        PendingReply::Heartbeat => {}
    }
}

fn exec_error(cmd_id: &str, reason: String) -> TtpExecuted {
    TtpExecuted {
        id: cmd_id.to_string(),
        success: false,
        results: vec![reason.clone()],
        exit_code: 1,
        fail_reason: reason,
        session_connected: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use armory::{Procedure, Ttp};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    use tokio::process::Command;
    use tokio::sync::oneshot;

    use super::{SessionHealth, SessionTiming, ShellSession};
    use crate::executor::C2Backend;
    use crate::types::ExecTtp;

    /// Spawn a fake shell over a duplex pair. Responds to each framed command
    /// by echoing `hello world\n{marker}:0\n` and to the init sentinel by
    /// echoing it back so `init()` unblocks.
    fn fake_shell_session(entity_id: &str) -> ShellSession {
        // duplex gives two DuplexStreams connected to each other.
        let (client, server) = tokio::io::duplex(4096);
        let (server_rx, mut server_tx) = tokio::io::split(server);
        let (client_rx, client_tx) = tokio::io::split(client);

        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(server_rx);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                // When we see a `printf '...'` line, extract the marker and reply.
                if let Some(rest) = line.trim_end().strip_prefix("printf '") {
                    let marker = rest.split('%').next().unwrap_or("").trim_end_matches(':');
                    if !marker.starts_with("__RAN_") {
                        continue;
                    }
                    let reply = if marker.contains("INIT0") {
                        format!("{marker}\n")
                    } else {
                        format!("hello world\n{marker}:0\n")
                    };
                    if server_tx.write_all(reply.as_bytes()).await.is_err() {
                        break;
                    }
                    let _ = server_tx.flush().await;
                }
                // Other lines (the actual command) are silently consumed.
            }
        });

        ShellSession::from_rw(client_rx, client_tx, entity_id)
    }

    #[tokio::test]
    async fn init_drains_banner_and_unblocks() {
        let session = fake_shell_session("node/test");
        // init() should complete without error - the fake server echoes the
        // init sentinel back so the drain loop terminates.
        session.init().await.expect("init should succeed");
    }

    #[tokio::test]
    async fn init_rejects_a_transport_that_reaches_eof() {
        let session = ShellSession::from_rw(tokio::io::empty(), tokio::io::sink(), "closed");

        let error = session
            .init()
            .await
            .expect_err("an already-closed shell cannot become a session");

        assert_eq!(error, crate::types::SESSION_CLOSED_UNEXPECTEDLY);
        assert_eq!(*session.health.borrow(), SessionHealth::Lost);
    }

    #[tokio::test]
    async fn close_sends_an_unframed_exit_before_closing_the_socket() {
        let (client, server) = tokio::io::duplex(4096);
        let (server_rx, server_tx) = tokio::io::split(server);
        let (client_rx, client_tx) = tokio::io::split(client);
        let (exit_seen_tx, exit_seen_rx) = oneshot::channel();

        tokio::spawn(async move {
            // Keep the peer write half alive until it receives the exit
            // request, so this proves the EOF is caused by close rather than
            // an already-closed test transport.
            let _keep_open = server_tx;
            let mut reader = tokio::io::BufReader::new(server_rx);
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) != 0 {
                if line == "exit\n" {
                    let _ = exit_seen_tx.send(());
                    return;
                }
                line.clear();
            }
        });

        let session = ShellSession::from_rw(client_rx, client_tx, "node/test");
        session.close().await.expect("close should succeed");
        tokio::time::timeout(Duration::from_secs(1), exit_seen_rx)
            .await
            .expect("close should write exit to the peer")
            .expect("peer should observe exit");
        assert_eq!(*session.health.borrow(), SessionHealth::Lost);
    }

    #[tokio::test]
    async fn close_aborts_a_stuck_frame_after_the_grace_window() {
        let (client, server) = tokio::io::duplex(4096);
        let (server_rx, server_tx) = tokio::io::split(server);
        let (client_rx, client_tx) = tokio::io::split(client);
        let (frame_started_tx, frame_started_rx) = oneshot::channel();

        tokio::spawn(async move {
            let _keep_open = server_tx;
            let mut reader = tokio::io::BufReader::new(server_rx);
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) != 0 {
                if line.trim_end().starts_with("printf '__RAN_") {
                    let _ = frame_started_tx.send(());
                    return;
                }
                line.clear();
            }
        });

        let session = Arc::new(ShellSession::from_rw(client_rx, client_tx, "node/test"));
        let execution_session = session.clone();
        let command = make_cmd("blocks forever", "session/test");
        let execution = tokio::spawn(async move { execution_session.execute(&command).await });
        frame_started_rx.await.expect("command frame should start");

        tokio::time::timeout(Duration::from_secs(3), session.close())
            .await
            .expect("close must not wait for the stuck frame")
            .expect("close should succeed");
        let result = tokio::time::timeout(Duration::from_secs(1), execution)
            .await
            .expect("aborting the actor should release the execution")
            .expect("execution task should complete");
        assert!(!result.success);
        assert_eq!(
            result.fail_reason,
            crate::types::SESSION_CLOSED_UNEXPECTEDLY
        );
        assert_eq!(*session.health.borrow(), SessionHealth::Lost);
    }

    #[tokio::test]
    async fn execute_returns_command_output_and_exit_code() {
        let session = fake_shell_session("node/test");
        session.init().await.expect("init");

        let cmd = make_cmd("echo hello", "session/test");
        let result = session.execute(&cmd).await;

        assert!(
            result.success,
            "expected success, got: {:?}",
            result.fail_reason
        );
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.results, vec!["hello world"]);
    }

    #[tokio::test]
    async fn execute_streams_output_without_completion_marker() {
        let session = fake_shell_session("node/test");
        session.init().await.expect("init");
        let cmd = make_cmd("echo hello", "session/test");
        let (output, mut rx) = crate::OutputSink::channel();

        let result = session.execute_streaming(&cmd, output).await;
        assert!(result.success);
        let fragment = rx.try_recv().expect("output fragment");
        assert_eq!(fragment.stream, crate::OutputStream::Stdout);
        let text = String::from_utf8(fragment.bytes).expect("utf8 output");
        assert_eq!(text, "hello world\n");
        assert!(!text.contains("__RAN_"));
    }

    #[tokio::test]
    async fn execute_non_zero_exit_reports_failure() {
        // Return exit code 127 by patching the fake server's reply.
        let (client, server) = tokio::io::duplex(4096);
        let (server_rx, mut server_tx) = tokio::io::split(server);
        let (client_rx, client_tx) = tokio::io::split(client);

        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(server_rx);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if let Some(rest) = line.trim_end().strip_prefix("printf '") {
                    let marker = rest.split('%').next().unwrap_or("").trim_end_matches(':');
                    if !marker.starts_with("__RAN_") {
                        continue;
                    }
                    let reply = if marker.contains("INIT0") {
                        format!("{marker}\n")
                    } else {
                        // Command not found - exit 127
                        format!("bash: nonexistent: command not found\n{marker}:127\n")
                    };
                    if server_tx.write_all(reply.as_bytes()).await.is_err() {
                        break;
                    }
                    let _ = server_tx.flush().await;
                }
            }
        });

        let session = ShellSession::from_rw(client_rx, client_tx, "node/test");
        session.init().await.expect("init");

        let cmd = make_cmd("nonexistent", "session/test");
        let result = session.execute(&cmd).await;

        assert!(!result.success);
        assert_eq!(result.exit_code, 127);
        assert!(!result.results.is_empty());
    }

    #[tokio::test]
    async fn execute_captures_earlier_pipeline_stage_stderr() {
        let mut shell = Command::new("sh")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start POSIX shell");
        let stdin = shell.stdin.take().expect("shell stdin");
        let stdout = shell.stdout.take().expect("shell stdout");
        let session = ShellSession::from_rw(stdout, stdin, "node/test");
        session.init().await.expect("init");

        // Without grouping the command before `2>&1`, this shell error is
        // written to the process's stderr because `cat` is the pipeline's last
        // stage. Socat exposes that stream locally, while Ran used to receive
        // an empty result and the successful exit status from `cat`.
        let cmd = make_cmd("definitely-not-a-command | cat", "session/test");
        let result = session.execute(&cmd).await;

        assert!(result.success, "cat is still the pipeline's final status");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.results.len(), 1);
        assert!(result.results[0].contains("definitely-not-a-command"));
        assert!(result.results[0].contains("not found"));

        drop(session);
        let status = tokio::time::timeout(Duration::from_secs(1), shell.wait())
            .await
            .expect("shell exits after session closes")
            .expect("wait for shell");
        assert!(status.success());
    }

    #[tokio::test]
    async fn a_timed_out_command_is_drained_before_the_next_command_runs() {
        let (client, server) = tokio::io::duplex(4096);
        let (server_rx, mut server_tx) = tokio::io::split(server);
        let (client_rx, client_tx) = tokio::io::split(client);

        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(server_rx);
            let mut line = String::new();
            let mut command_count = 0;
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if let Some(rest) = line.trim_end().strip_prefix("printf '") {
                    let marker = rest.split('%').next().unwrap_or("").trim_end_matches(':');
                    if !marker.starts_with("__RAN_") {
                        continue;
                    }
                    command_count += 1;
                    if command_count == 1 {
                        tokio::time::sleep(Duration::from_millis(1200)).await;
                    }
                    let output = if command_count == 1 {
                        "late output"
                    } else {
                        "fresh output"
                    };
                    let _ = server_tx
                        .write_all(format!("{output}\n{marker}:0\n").as_bytes())
                        .await;
                    let _ = server_tx.flush().await;
                }
            }
        });

        let session = ShellSession::from_rw(client_rx, client_tx, "node/test");
        let mut first_cmd = make_cmd("slow command", "session/test");
        first_cmd.execution_timeout_seconds = 1;

        let first = session.execute(&first_cmd).await;
        assert!(!first.success);
        assert_eq!(first.fail_reason, "shell command timed out after 1s");
        assert_ne!(*session.health.borrow(), SessionHealth::Lost);

        let mut second_cmd = make_cmd("next command", "session/test");
        second_cmd.execution_timeout_seconds = 2;
        let second = session.execute(&second_cmd).await;

        assert!(second.success, "{}", second.fail_reason);
        assert_eq!(second.results, vec!["fresh output"]);
        assert_eq!(*session.health.borrow(), SessionHealth::Responsive);
    }

    #[tokio::test]
    async fn queued_actions_time_out_from_submission_and_are_not_sent_late() {
        let (client, server) = tokio::io::duplex(4096);
        let (server_rx, mut server_tx) = tokio::io::split(server);
        let (client_rx, client_tx) = tokio::io::split(client);
        let command_count = Arc::new(AtomicUsize::new(0));
        let server_count = command_count.clone();

        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(server_rx);
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                    break;
                }
                let Some(rest) = line.trim_end().strip_prefix("printf '") else {
                    continue;
                };
                let marker = rest.split('%').next().unwrap_or("").trim_end_matches(':');
                if !marker.starts_with("__RAN_") {
                    continue;
                }
                if server_count.fetch_add(1, Ordering::SeqCst) == 0 {
                    // The first remote command outlives both 1-second action
                    // deadlines. The second command must never be sent once
                    // its caller has already timed out in the actor queue.
                    tokio::time::sleep(Duration::from_millis(2200)).await;
                    let _ = server_tx
                        .write_all(format!("first done\n{marker}:0\n").as_bytes())
                        .await;
                    let _ = server_tx.flush().await;
                }
            }
        });

        let session = ShellSession::from_rw(client_rx, client_tx, "node/test");
        let mut first_cmd = make_cmd("first", "session/test");
        first_cmd.execution_timeout_seconds = 1;
        let first = session.execute(&first_cmd).await;
        assert_eq!(first.fail_reason, "shell command timed out after 1s");

        let mut second_cmd = make_cmd("second", "session/test");
        second_cmd.execution_timeout_seconds = 1;
        let second = session.execute(&second_cmd).await;
        assert_eq!(second.fail_reason, "shell command timed out after 1s");

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(command_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_idle_eof_marks_the_session_lost_without_an_action() {
        let (client, server) = tokio::io::duplex(4096);
        let (client_rx, client_tx) = tokio::io::split(client);
        let session = ShellSession::from_rw(client_rx, client_tx, "node/test");
        let mut health = session.subscribe_health();

        drop(server);
        tokio::time::timeout(Duration::from_secs(1), async {
            while *health.borrow_and_update() != SessionHealth::Lost {
                health
                    .changed()
                    .await
                    .expect("session actor should stay alive");
            }
        })
        .await
        .expect("idle EOF should be detected promptly");
    }

    #[tokio::test]
    async fn a_missed_idle_heartbeat_is_suspect_not_lost() {
        let (client, server) = tokio::io::duplex(4096);
        let (server_rx, _server_tx) = tokio::io::split(server);
        let (client_rx, client_tx) = tokio::io::split(client);
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(server_rx);
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) != 0 {
                line.clear();
            }
        });

        let session = ShellSession::from_rw_with_timing(
            client_rx,
            client_tx,
            "node/test",
            SessionTiming {
                heartbeat_interval: Duration::from_millis(20),
                heartbeat_timeout: Duration::from_millis(30),
            },
        );
        let mut health = session.subscribe_health();

        tokio::time::timeout(Duration::from_secs(1), async {
            while *health.borrow_and_update() != SessionHealth::Suspect {
                health
                    .changed()
                    .await
                    .expect("session actor should stay alive");
            }
        })
        .await
        .expect("missed heartbeat should update health");
        assert_ne!(*health.borrow(), SessionHealth::Lost);
    }

    #[test]
    fn command_framing_separates_a_marker_from_output_without_a_newline() {
        assert_eq!(
            super::framed_command("cat /token", "__RAN_42__"),
            "{\ncat /token\n} 2>&1\n__ran_status=$?\nprintf '\\n'\nprintf '__RAN_42__:%d\\n' \"$__ran_status\"\n"
        );
    }

    fn make_cmd(command: &str, exec_system_id: &str) -> ExecTtp {
        ExecTtp {
            id: "test-cmd-1".to_string(),
            started_at_ms: 0,
            execution_timeout_seconds: crate::DEFAULT_EXECUTION_TIMEOUT_SECONDS,
            ttp: Ttp::new("T0001", "Test", "Execution"),
            procedure: Procedure::new("proc-1", command),
            operation: crate::ExecutionOperation::Shell {
                command: command.to_string(),
            },
            args: HashMap::new(),
            target_id: "node/test-node".to_string(),
            exec_chain: vec!["node/test-node".to_string()],
            exec_system_id: exec_system_id.to_string(),
            auth_identity_id: None,
            output_transform: None,
            is_cleanup: false,
            reasoning: String::new(),
        }
    }
}
