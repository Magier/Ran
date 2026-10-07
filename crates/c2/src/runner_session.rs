use std::time::Duration;

use async_trait::async_trait;
use ranplant_protocol::{
    Execution, Message, OutputStream as RunnerOutputStream, MAGIC, MAX_FRAME_BYTES,
    PROTOCOL_VERSION,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::executor::C2Backend;
use crate::output::OutputSink;
use crate::shell_session::SessionHealth;
use crate::types::{ExecTtp, TtpExecuted};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROLLER_GUARD_GRACE: Duration = Duration::from_secs(3);
const COMMAND_OUTPUT_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct RunnerIdentity {
    pub(crate) hostname: String,
    pub(crate) user: String,
    pub(crate) os: String,
    pub(crate) os_release: std::collections::BTreeMap<String, String>,
    pub(crate) arch: String,
    pub(crate) runner_version: String,
    pub(crate) capabilities: Vec<String>,
}

impl RunnerIdentity {
    pub(crate) fn display_os(&self) -> String {
        if let Some(pretty_name) = self.os_release.get("PRETTY_NAME") {
            return pretty_name.clone();
        }
        let name = self
            .os_release
            .get("NAME")
            .or_else(|| self.os_release.get("ID"));
        match (name, self.os_release.get("VERSION_ID")) {
            (Some(name), Some(version)) => format!("{name} {version}"),
            (Some(name), None) => name.clone(),
            _ => self.os.clone(),
        }
    }
}

pub struct RunnerSession {
    requests: mpsc::Sender<RunnerRequest>,
    health: watch::Receiver<SessionHealth>,
    close_health: watch::Sender<SessionHealth>,
    actor: tokio::task::AbortHandle,
    pub entity_id: String,
}

enum RunnerRequest {
    Execute {
        command: Box<ExecTtp>,
        output_sink: OutputSink,
        reply: oneshot::Sender<TtpExecuted>,
    },
    Close {
        reply: oneshot::Sender<Result<(), String>>,
    },
}

impl RunnerSession {
    pub(crate) async fn from_incoming(
        mut stream: TcpStream,
        entity_id: impl Into<String>,
    ) -> Result<(Self, RunnerIdentity), String> {
        let mut preface = [0_u8; MAGIC.len()];
        tokio::time::timeout(HEARTBEAT_TIMEOUT, stream.read_exact(&mut preface))
            .await
            .map_err(|_| "Ranplant handshake timed out".to_string())?
            .map_err(|error| format!("failed to read Ranplant preface: {error}"))?;
        if preface != MAGIC {
            return Err("invalid Ranplant protocol preface".to_string());
        }
        let hello = tokio::time::timeout(HEARTBEAT_TIMEOUT, read_message(&mut stream))
            .await
            .map_err(|_| "Ranplant hello timed out".to_string())??;
        let Message::Hello {
            min_version,
            max_version,
            runner_version,
            hostname,
            user,
            os,
            os_release,
            arch,
            capabilities,
        } = hello
        else {
            return Err("Ranplant did not begin with a hello message".to_string());
        };
        if !(min_version..=max_version).contains(&PROTOCOL_VERSION) {
            return Err(format!(
                "Ranplant protocol versions {min_version}..={max_version} do not include controller version {PROTOCOL_VERSION}"
            ));
        }
        write_message(
            &mut stream,
            &Message::HelloAck {
                version: PROTOCOL_VERSION,
                heartbeat_interval_ms: HEARTBEAT_INTERVAL.as_millis() as u64,
                max_frame_bytes: MAX_FRAME_BYTES as u32,
            },
        )
        .await?;

        let identity = RunnerIdentity {
            hostname,
            user,
            os,
            os_release,
            arch,
            runner_version,
            capabilities,
        };
        let entity_id = entity_id.into();
        let (reader, writer) = tokio::io::split(stream);
        let (request_tx, request_rx) = mpsc::channel(32);
        let (health_tx, health_rx) = watch::channel(SessionHealth::Responsive);
        let close_health = health_tx.clone();
        let actor = tokio::spawn(run_actor(
            reader,
            writer,
            request_rx,
            health_tx,
            entity_id.clone(),
        ));
        Ok((
            Self {
                requests: request_tx,
                health: health_rx,
                close_health,
                actor: actor.abort_handle(),
                entity_id,
            },
            identity,
        ))
    }

    pub(crate) fn subscribe_health(&self) -> watch::Receiver<SessionHealth> {
        self.health.clone()
    }

    pub(crate) async fn close(&self) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let result = match self
            .requests
            .send(RunnerRequest::Close { reply: reply_tx })
            .await
        {
            Ok(()) => reply_rx
                .await
                .unwrap_or_else(|_| Err("Ranplant session closed unexpectedly".to_string())),
            Err(_) => Err("Ranplant session closed unexpectedly".to_string()),
        };
        self.close_health.send_replace(SessionHealth::Lost);
        self.actor.abort();
        result
    }
}

#[async_trait]
impl C2Backend for RunnerSession {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
        self.execute_streaming(cmd, OutputSink::discard()).await
    }

    async fn execute_streaming(&self, cmd: &ExecTtp, output_sink: OutputSink) -> TtpExecuted {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .requests
            .send(RunnerRequest::Execute {
                command: Box::new(cmd.clone()),
                output_sink,
                reply: reply_tx,
            })
            .await
            .is_err()
        {
            return execution_error(&cmd.id, "Ranplant session closed unexpectedly");
        }
        reply_rx
            .await
            .unwrap_or_else(|_| execution_error(&cmd.id, "Ranplant session closed unexpectedly"))
    }

    async fn close(&self) -> Result<(), String> {
        RunnerSession::close(self).await
    }
}

async fn run_actor<R, W>(
    mut reader: R,
    mut writer: W,
    mut requests: mpsc::Receiver<RunnerRequest>,
    health: watch::Sender<SessionHealth>,
    entity_id: String,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut nonce = 0_u64;
    loop {
        let heartbeat = tokio::time::sleep(HEARTBEAT_INTERVAL);
        tokio::pin!(heartbeat);
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { return };
                match request {
                    RunnerRequest::Execute { command, output_sink, reply } => {
                        health.send_replace(SessionHealth::Busy);
                        let event = execute_command(
                            &mut reader,
                            &mut writer,
                            &command,
                            output_sink,
                        ).await;
                        match event {
                            Ok(event) => {
                                health.send_replace(SessionHealth::Responsive);
                                let _ = reply.send(event);
                            }
                            Err(error) => {
                                health.send_replace(SessionHealth::Lost);
                                let _ = reply.send(execution_error(&command.id, &error));
                                tracing::warn!(%entity_id, %error, "Ranplant session lost during command");
                                return;
                            }
                        }
                    }
                    RunnerRequest::Close { reply } => {
                        let result = write_message(&mut writer, &Message::Shutdown).await;
                        let _ = writer.shutdown().await;
                        health.send_replace(SessionHealth::Lost);
                        let _ = reply.send(result);
                        return;
                    }
                }
            }
            _ = &mut heartbeat => {
                nonce = nonce.wrapping_add(1);
                let heartbeat_result = async {
                    write_message(&mut writer, &Message::Ping { nonce }).await?;
                    match read_message(&mut reader).await? {
                        Message::Pong { nonce: response } if response == nonce => Ok(()),
                        other => Err(format!("unexpected Ranplant heartbeat response: {other:?}")),
                    }
                };
                match tokio::time::timeout(HEARTBEAT_TIMEOUT, heartbeat_result).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(%entity_id, %error, "Ranplant heartbeat failed");
                        health.send_replace(SessionHealth::Lost);
                        return;
                    }
                    Err(_) => {
                        tracing::warn!(%entity_id, "Ranplant heartbeat timed out");
                        health.send_replace(SessionHealth::Lost);
                        return;
                    }
                }
            }
        }
    }
}

async fn execute_command(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    command: &ExecTtp,
    output_sink: OutputSink,
) -> Result<TtpExecuted, String> {
    let shell_command = command
        .operation
        .command()
        .ok_or_else(|| "Ranplant received a non-shell execution operation".to_string())?;
    let timeout = Duration::from_secs(command.execution_timeout_seconds.max(1));
    write_message(
        writer,
        &Message::Run {
            id: command.id.clone(),
            execution: Execution::Shell {
                command: shell_command.to_string(),
            },
            timeout_ms: timeout.as_millis() as u64,
            output_limit: COMMAND_OUTPUT_LIMIT,
        },
    )
    .await?;

    let deadline = Instant::now() + timeout + CONTROLLER_GUARD_GRACE;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut last_sequence = 0_u64;
    loop {
        let message = match tokio::time::timeout_at(deadline, read_message(reader)).await {
            Ok(message) => message?,
            Err(_) => {
                let _ = write_message(
                    writer,
                    &Message::Cancel {
                        id: command.id.clone(),
                    },
                )
                .await;
                return Err(
                    "Ranplant did not report command completion after its deadline".to_string(),
                );
            }
        };
        match message {
            Message::Started { id, .. } if id == command.id => {}
            Message::Output {
                id,
                sequence,
                stream,
                bytes,
            } if id == command.id => {
                if sequence <= last_sequence {
                    return Err("Ranplant output sequence moved backwards".to_string());
                }
                last_sequence = sequence;
                match stream {
                    RunnerOutputStream::Stdout => {
                        output_sink.stdout(bytes.clone());
                        append_bounded(&mut stdout, &bytes);
                    }
                    RunnerOutputStream::Stderr => {
                        output_sink.stderr(bytes.clone());
                        append_bounded(&mut stderr, &bytes);
                    }
                }
            }
            Message::Exited {
                id,
                code,
                signal,
                timed_out,
                cancelled,
                output_truncated,
            } if id == command.id => {
                return Ok(completed_event(
                    &command.id,
                    code,
                    signal,
                    timed_out,
                    cancelled,
                    output_truncated,
                    stdout,
                    stderr,
                ));
            }
            Message::Error { id, message }
                if id.as_deref().is_none() || id.as_deref() == Some(command.id.as_str()) =>
            {
                return Ok(execution_error(&command.id, &message));
            }
            Message::Ping { nonce } => {
                write_message(writer, &Message::Pong { nonce }).await?;
            }
            other => {
                return Err(format!(
                    "unexpected Ranplant message while executing {}: {other:?}",
                    command.id
                ));
            }
        }
    }
}

fn append_bounded(buffer: &mut Vec<u8>, bytes: &[u8]) {
    let available = (COMMAND_OUTPUT_LIMIT as usize).saturating_sub(buffer.len());
    buffer.extend_from_slice(&bytes[..bytes.len().min(available)]);
}

#[allow(clippy::too_many_arguments)]
fn completed_event(
    id: &str,
    code: Option<i32>,
    signal: Option<i32>,
    timed_out: bool,
    cancelled: bool,
    output_truncated: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
) -> TtpExecuted {
    let stdout = String::from_utf8_lossy(&stdout).trim_end().to_string();
    let stderr = String::from_utf8_lossy(&stderr).trim_end().to_string();
    let success = code == Some(0) && !timed_out && !cancelled;
    let fail_reason = if success {
        String::new()
    } else if timed_out {
        "Ranplant command timed out".to_string()
    } else if cancelled {
        "Ranplant command was cancelled".to_string()
    } else if let Some(line) = stderr.lines().last().filter(|line| !line.is_empty()) {
        line.to_string()
    } else if let Some(line) = stdout.lines().last().filter(|line| !line.is_empty()) {
        line.to_string()
    } else if let Some(signal) = signal {
        format!("command terminated by signal {signal}")
    } else {
        format!("command exited with code {}", code.unwrap_or(1))
    };
    let mut results = Vec::new();
    if !stdout.is_empty() {
        results.push(stdout);
    }
    if !stderr.is_empty() {
        if results.is_empty() {
            results.push(String::new());
        }
        results.push(stderr);
    }
    if output_truncated {
        results.push("[Ranplant output truncated]".to_string());
    }
    TtpExecuted {
        id: id.to_string(),
        success,
        results,
        exit_code: code.unwrap_or_else(|| signal.map_or(1, |value| 128 + value)),
        fail_reason,
        session_connected: None,
    }
}

fn execution_error(id: &str, reason: &str) -> TtpExecuted {
    TtpExecuted {
        id: id.to_string(),
        success: false,
        results: vec![reason.to_string()],
        exit_code: 1,
        fail_reason: reason.to_string(),
        session_connected: None,
    }
}

async fn write_message(
    writer: &mut (impl AsyncWrite + Unpin),
    message: &Message,
) -> Result<(), String> {
    let payload = ranplant_protocol::encode(message)
        .map_err(|error| format!("failed to encode Ranplant message: {error}"))?;
    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await
        .map_err(|error| format!("failed to write Ranplant frame length: {error}"))?;
    writer
        .write_all(&payload)
        .await
        .map_err(|error| format!("failed to write Ranplant frame: {error}"))?;
    writer
        .flush()
        .await
        .map_err(|error| format!("failed to flush Ranplant frame: {error}"))
}

async fn read_message(reader: &mut (impl AsyncRead + Unpin)) -> Result<Message, String> {
    let length = reader
        .read_u32()
        .await
        .map_err(|error| format!("failed to read Ranplant frame length: {error}"))?
        as usize;
    if length > MAX_FRAME_BYTES {
        return Err("Ranplant frame exceeds maximum size".to_string());
    }
    let mut payload = vec![0_u8; length];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|error| format!("failed to read Ranplant frame: {error}"))?;
    ranplant_protocol::decode(&payload)
        .map_err(|error| format!("failed to decode Ranplant frame: {error}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::HashMap;

    use armory::{Procedure, Ttp};
    use tokio::io::AsyncWriteExt;
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::types::ExecutionOperation;

    #[test]
    fn runner_identity_prefers_os_release_distribution_and_version() {
        let identity = RunnerIdentity {
            hostname: "target".to_string(),
            user: "root".to_string(),
            os: "linux".to_string(),
            os_release: BTreeMap::from([
                ("NAME".to_string(), "Alpine Linux".to_string()),
                ("VERSION_ID".to_string(), "3.22".to_string()),
            ]),
            arch: "x86_64".to_string(),
            runner_version: "test".to_string(),
            capabilities: Vec::new(),
        };

        assert_eq!(identity.display_os(), "Alpine Linux 3.22");
    }

    #[tokio::test]
    async fn runner_session_preserves_streams_and_survives_command_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("listener address");
        let peer = tokio::spawn(async move {
            let mut stream = TcpStream::connect(address).await.expect("connect");
            stream.write_all(&MAGIC).await.expect("preface");
            write_message(
                &mut stream,
                &Message::Hello {
                    min_version: PROTOCOL_VERSION,
                    max_version: PROTOCOL_VERSION,
                    runner_version: "test".to_string(),
                    hostname: "runner-host".to_string(),
                    user: "root".to_string(),
                    os: "linux".to_string(),
                    os_release: Default::default(),
                    arch: "x86_64".to_string(),
                    capabilities: vec!["exec-shell-v1".to_string()],
                },
            )
            .await
            .expect("hello");
            assert!(matches!(
                read_message(&mut stream).await.expect("hello ack"),
                Message::HelloAck { .. }
            ));
            for (expected_id, code) in [("cmd-fail", 7), ("cmd-next", 0)] {
                let Message::Run { id, .. } = read_message(&mut stream).await.expect("run request")
                else {
                    panic!("expected run request");
                };
                assert_eq!(id, expected_id);
                write_message(
                    &mut stream,
                    &Message::Started {
                        id: id.clone(),
                        pid: 42,
                    },
                )
                .await
                .expect("started");
                write_message(
                    &mut stream,
                    &Message::Output {
                        id: id.clone(),
                        sequence: 1,
                        stream: RunnerOutputStream::Stdout,
                        bytes: format!("stdout-{id}\n").into_bytes(),
                    },
                )
                .await
                .expect("stdout");
                if code != 0 {
                    write_message(
                        &mut stream,
                        &Message::Output {
                            id: id.clone(),
                            sequence: 2,
                            stream: RunnerOutputStream::Stderr,
                            bytes: b"intentional failure\n".to_vec(),
                        },
                    )
                    .await
                    .expect("stderr");
                }
                write_message(
                    &mut stream,
                    &Message::Exited {
                        id,
                        code: Some(code),
                        signal: None,
                        timed_out: false,
                        cancelled: false,
                        output_truncated: false,
                    },
                )
                .await
                .expect("exited");
            }
            assert!(matches!(
                read_message(&mut stream).await.expect("shutdown"),
                Message::Shutdown
            ));
        });

        let (stream, _) = listener.accept().await.expect("accept");
        let (session, identity) = RunnerSession::from_incoming(stream, "session/test")
            .await
            .expect("negotiate");
        assert_eq!(identity.hostname, "runner-host");

        let failed = session.execute(&make_cmd("cmd-fail")).await;
        assert!(!failed.success);
        assert_eq!(failed.exit_code, 7);
        assert_eq!(failed.results[0], "stdout-cmd-fail");
        assert_eq!(failed.results[1], "intentional failure");

        let next = session.execute(&make_cmd("cmd-next")).await;
        assert!(next.success, "{}", next.fail_reason);
        assert_eq!(next.results, vec!["stdout-cmd-next"]);
        session.close().await.expect("close");
        peer.await.expect("peer task");
    }

    fn make_cmd(id: &str) -> ExecTtp {
        ExecTtp {
            id: id.to_string(),
            ttp: Ttp::new("test", "Test", "Execution"),
            procedure: Procedure::new("shell", "true"),
            operation: ExecutionOperation::Shell {
                command: "true".to_string(),
            },
            args: HashMap::new(),
            target_id: "node/test".to_string(),
            exec_chain: vec!["node/test".to_string()],
            exec_system_id: "session/test".to_string(),
            execution_environment: None,
            transport_environment: None,
            auth_identity_id: None,
            started_at_ms: 0,
            execution_timeout_seconds: 5,
            output_transform: None,
            is_cleanup: false,
            reasoning: String::new(),
        }
    }
}
