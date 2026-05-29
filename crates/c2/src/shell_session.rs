use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::warn;

use crate::executor::C2Backend;
use crate::types::{ExecTtp, TtpExecuted};

static NONCE: AtomicU64 = AtomicU64::new(1);

/// A live shell session — bind shell, reverse shell, or any other async stream.
///
/// Commands are sent to the shell's stdin and output is framed with a
/// per-command sentinel so discrete stdout/exit_code results can be extracted
/// from the continuous byte stream.
///
/// Framing protocol (written to stdin for each command):
/// ```text
/// {cmd} 2>&1
/// printf '__RAN_{nonce}__:%d\n' $?
/// ```
/// Lines are read until the sentinel `__RAN_{nonce}__:{exit_code}` appears.
/// Everything before it is stdout (stderr merged via `2>&1`).
pub struct ShellSession {
    inner: Arc<Mutex<ShellInner>>,
    /// Entity ID this session currently exits into (for logging/debugging).
    pub entity_id: String,
}

struct ShellInner {
    tx: Box<dyn AsyncWrite + Unpin + Send>,
    rx: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
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
        let (rx, tx) = tokio::io::split(stream);
        Self::from_rw(rx, tx, entity_id)
    }

    pub(crate) fn from_rw<R, W>(reader: R, writer: W, entity_id: impl Into<String>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self {
            inner: Arc::new(Mutex::new(ShellInner {
                tx: Box::new(writer),
                rx: BufReader::new(Box::new(reader)),
            })),
            entity_id: entity_id.into(),
        }
    }

    /// Drain any shell banner and configure a clean execution environment.
    ///
    /// Best-effort: if the shell doesn't echo the init marker within 5 s
    /// (e.g. no PTY, wrong shell, slow start) we log a warning and proceed.
    /// The sentinel-based framing in `run_raw` / `execute` is robust to any
    /// extra prefix output (prompts, echoed commands) that may remain.
    pub async fn init(&self) -> Result<(), String> {
        let init_marker = "__RAN_INIT0__";
        let init_cmd = format!(
            "stty -echo 2>/dev/null; unset PROMPT_COMMAND PS1 PS2 HISTFILE 2>/dev/null\nprintf '{init_marker}\\n'\n"
        );

        let mut guard = self.inner.lock().await;
        guard
            .tx
            .write_all(init_cmd.as_bytes())
            .await
            .map_err(|e| format!("shell init write failed: {e}"))?;
        guard
            .tx
            .flush()
            .await
            .map_err(|e| format!("shell init flush failed: {e}"))?;

        let drain = async {
            let mut buf = String::new();
            let mut lines = 0usize;
            loop {
                buf.clear();
                guard
                    .rx
                    .read_line(&mut buf)
                    .await
                    .map_err(|e| format!("shell init drain failed: {e}"))?;
                if buf.trim_end_matches(['\r', '\n']).contains(init_marker) {
                    break;
                }
                lines += 1;
                if lines > 200 {
                    return Err("shell init timed out draining banner (>200 lines)".to_string());
                }
            }
            Ok::<_, String>(())
        };

        match tokio::time::timeout(Duration::from_secs(5), drain).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                warn!(entity_id = %self.entity_id, error = %e, "shell init drain failed; proceeding");
            }
            Err(_) => {
                warn!(entity_id = %self.entity_id, "shell init timed out waiting for marker; proceeding without clean init");
            }
        }
        Ok(())
    }

    /// Run a single command and return trimmed stdout.  Used for probing
    /// (hostname, whoami, uname) before the session is fully registered.
    /// Times out after 5 s — returns an error if the shell doesn't respond.
    pub async fn run_raw(&self, cmd: &str) -> Result<String, String> {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let marker = format!("__RAN_{nonce}__");
        let payload = format!("{cmd} 2>&1\nprintf '{marker}:%d\\n' $?\n");

        let mut guard = self.inner.lock().await;
        guard
            .tx
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("run_raw write failed: {e}"))?;
        guard
            .tx
            .flush()
            .await
            .map_err(|e| format!("run_raw flush failed: {e}"))?;

        let read_fut = async {
            let mut output = String::new();
            let mut line = String::new();
            loop {
                line.clear();
                match guard.rx.read_line(&mut line).await {
                    Ok(0) => return Err("shell closed unexpectedly".to_string()),
                    Err(e) => return Err(format!("run_raw read failed: {e}")),
                    Ok(_) => {}
                }
                let trimmed = line.trim_end_matches(['\r', '\n']);
                if trimmed.starts_with(&format!("{marker}:")) {
                    break;
                }
                output.push_str(&line);
            }
            // Take the last non-empty line so that echoed commands or shell
            // prompts before the actual output don't contaminate the result.
            let last = output
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
                .to_string();
            Ok::<_, String>(last)
        };

        match tokio::time::timeout(Duration::from_secs(5), read_fut).await {
            Ok(result) => result,
            Err(_) => Err(format!("run_raw timed out waiting for response to '{cmd}'")),
        }
    }
}

#[async_trait]
impl C2Backend for ShellSession {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let marker = format!("__RAN_{nonce}__");

        // Two-line payload: run the command with merged stderr, then print
        // the sentinel on its own line so it's never mixed with command output.
        let command = &cmd.procedure.command;
        let payload = format!("{command} 2>&1\nprintf '{marker}:%d\\n' $?\n");

        let mut guard = self.inner.lock().await;

        if let Err(e) = guard.tx.write_all(payload.as_bytes()).await {
            return exec_error(&cmd.id, format!("shell write failed: {e}"));
        }
        if let Err(e) = guard.tx.flush().await {
            return exec_error(&cmd.id, format!("shell flush failed: {e}"));
        }

        let mut output = String::new();
        let exit_code;
        let mut line = String::new();

        let read_fut = async {
            loop {
                line.clear();
                match guard.rx.read_line(&mut line).await {
                    Ok(0) => {
                        warn!(entity_id = %self.entity_id, "shell session EOF");
                        return Err("shell session closed unexpectedly".to_string());
                    }
                    Err(e) => return Err(format!("shell read failed: {e}")),
                    Ok(_) => {}
                }

                let trimmed = line.trim_end_matches(['\r', '\n']);
                if let Some(code_str) = trimmed.strip_prefix(&format!("{marker}:")) {
                    return Ok((code_str.parse().unwrap_or(1i32), output.clone()));
                }
                output.push_str(&line);
            }
        };

        match tokio::time::timeout(Duration::from_secs(60), read_fut).await {
            Ok(Ok((code, out))) => {
                exit_code = code;
                output = out;
            }
            Ok(Err(e)) => return exec_error(&cmd.id, e),
            Err(_) => return exec_error(&cmd.id, "shell command timed out after 60s".to_string()),
        }

        let stdout = output.trim_end().to_string();
        let success = exit_code == 0;
        let fail_reason = if success {
            String::new()
        } else if stdout.is_empty() {
            format!("exit code {exit_code}")
        } else {
            stdout.lines().last().unwrap_or("").to_string()
        };

        TtpExecuted {
            id: cmd.id.clone(),
            success,
            results: if stdout.is_empty() {
                vec![]
            } else {
                vec![stdout]
            },
            exit_code,
            fail_reason,
            session_connected: None,
        }
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

    use armory::{Procedure, Ttp};
    use tokio::io::AsyncWriteExt;

    use super::ShellSession;
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
        // init() should complete without error — the fake server echoes the
        // init sentinel back so the drain loop terminates.
        session.init().await.expect("init should succeed");
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
                    let reply = if marker.contains("INIT0") {
                        format!("{marker}\n")
                    } else {
                        // Command not found — exit 127
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

    fn make_cmd(command: &str, exec_system_id: &str) -> ExecTtp {
        ExecTtp {
            id: "test-cmd-1".to_string(),
            started_at_ms: 0,
            ttp: Ttp::new("T0001", "Test", "Execution"),
            procedure: Procedure::new("proc-1", command),
            args: HashMap::new(),
            target_id: "node/test-node".to_string(),
            exec_chain: vec!["node/test-node".to_string()],
            exec_system_id: exec_system_id.to_string(),
            output_transform: None,
            is_cleanup: false,
        }
    }
}
