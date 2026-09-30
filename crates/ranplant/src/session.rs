use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ranplant_protocol::{
    Execution, Message, OutputStream, MAGIC, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

const TERMINATE_GRACE: Duration = Duration::from_secs(1);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const DETACHED_START_TIMEOUT: Duration = Duration::from_secs(22);
const OS_RELEASE_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_OS_RELEASE_BYTES: u64 = 64 * 1024;
const OUTPUT_CHUNK_BYTES: usize = 16 * 1024;

pub(crate) async fn run_stdio() -> Result<(), String> {
    run_transport(tokio::io::stdin(), tokio::io::stdout(), false).await
}

pub(crate) async fn connect(host: &str, port: u16, announce_ready: bool) -> Result<(), String> {
    let destination = format!("{host}:{port}");
    let stream = timeout(
        CONNECT_TIMEOUT,
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .map_err(|_| format!("connection to {destination} timed out"))?
    .map_err(|error| format!("failed to connect to {destination}: {error}"))?;
    stream
        .set_nodelay(true)
        .map_err(|error| format!("failed to configure connection to {destination}: {error}"))?;
    let (input, output) = stream.into_split();
    run_transport(input, output, announce_ready).await
}

pub(crate) async fn spawn_detached(host: &str, port: u16) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to locate Ranplant executable: {error}"))?;
    let mut child = Command::new(executable)
        .args([
            "connect",
            "--host",
            host,
            "--port",
            &port.to_string(),
            "--detached-child",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(false)
        .spawn()
        .map_err(|error| format!("failed to launch detached Ranplant: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "detached Ranplant readiness channel is unavailable".to_string())?;
    let mut readiness = BufReader::new(stdout);
    let mut line = String::new();
    let ready = timeout(DETACHED_START_TIMEOUT, readiness.read_line(&mut line)).await;
    if matches!(ready, Ok(Ok(_))) && line.trim() == "READY" {
        return Ok(());
    }

    let _ = child.kill().await;
    let _ = child.wait().await;
    match ready {
        Err(_) => Err("detached Ranplant did not connect before its startup deadline".to_string()),
        Ok(Err(error)) => Err(format!(
            "failed to read detached Ranplant readiness: {error}"
        )),
        Ok(Ok(_)) if line.trim().starts_with("ERROR ") => Err(line
            .trim()
            .strip_prefix("ERROR ")
            .unwrap_or(line.trim())
            .to_string()),
        Ok(Ok(_)) => Err("detached Ranplant exited before completing its handshake".to_string()),
    }
}

async fn run_transport(
    mut input: impl AsyncRead + Unpin + Send + 'static,
    mut output: impl AsyncWrite + Unpin + Send + 'static,
    announce_ready: bool,
) -> Result<(), String> {
    output
        .write_all(&MAGIC)
        .await
        .map_err(|error| format!("failed to write protocol preface: {error}"))?;
    write_message(&mut output, &hello().await).await?;
    let acknowledgement = timeout(HANDSHAKE_TIMEOUT, read_message(&mut input))
        .await
        .map_err(|_| "controller handshake timed out".to_string())??;
    match acknowledgement {
        Message::HelloAck { version, .. } if version == PROTOCOL_VERSION => {}
        Message::HelloAck { version, .. } => {
            return Err(format!(
                "controller selected unsupported protocol version {version}"
            ));
        }
        other => return Err(format!("expected hello-ack, received {other:?}")),
    }
    if announce_ready {
        let mut readiness = tokio::io::stdout();
        readiness
            .write_all(b"READY\n")
            .await
            .map_err(|error| format!("failed to report connection readiness: {error}"))?;
        readiness
            .flush()
            .await
            .map_err(|error| format!("failed to flush connection readiness: {error}"))?;
    }

    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<Message>(128);
    let writer = tokio::spawn(async move {
        let mut output_sequence = 0_u64;
        while let Some(mut message) = outgoing_rx.recv().await {
            if let Message::Output { sequence, .. } = &mut message {
                output_sequence = output_sequence.wrapping_add(1);
                *sequence = output_sequence;
            }
            write_message(&mut output, &message).await?;
        }
        Ok::<(), String>(())
    });
    let (incoming_tx, mut incoming_rx) = mpsc::channel::<Result<Message, String>>(32);
    let reader = tokio::spawn(async move {
        loop {
            let message = read_message(&mut input).await;
            let stop = message.is_err();
            if incoming_tx.send(message).await.is_err() || stop {
                return;
            }
        }
    });
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<String>();
    let mut active: Option<(String, Option<oneshot::Sender<()>>)> = None;

    loop {
        tokio::select! {
            incoming = incoming_rx.recv() => {
                let Some(incoming) = incoming else { break };
                let message = incoming?;
                match message {
                    Message::Run { id, execution, timeout_ms, output_limit } => {
                        if active.is_some() {
                            outgoing_tx.send(Message::Error {
                                id: Some(id),
                                message: "Ranplant currently permits one active command".to_string(),
                            }).await.map_err(|_| "protocol writer stopped".to_string())?;
                            continue;
                        }
                        let (cancel_tx, cancel_rx) = oneshot::channel();
                        active = Some((id.clone(), Some(cancel_tx)));
                        tokio::spawn(execute(
                            id,
                            execution,
                            Duration::from_millis(timeout_ms.max(1)),
                            output_limit,
                            outgoing_tx.clone(),
                            done_tx.clone(),
                            cancel_rx,
                        ));
                    }
                    Message::Cancel { id } => {
                        if let Some((active_id, cancel)) = active.as_mut() {
                            if active_id == &id {
                                if let Some(cancel) = cancel.take() {
                                    let _ = cancel.send(());
                                }
                            }
                        }
                    }
                    Message::Ping { nonce } => {
                        outgoing_tx.send(Message::Pong { nonce }).await
                            .map_err(|_| "protocol writer stopped".to_string())?;
                    }
                    Message::Shutdown => {
                        if let Some((_, Some(cancel))) = active.take() {
                            let _ = cancel.send(());
                        }
                        break;
                    }
                    other => {
                        outgoing_tx.send(Message::Error {
                            id: None,
                            message: format!("unexpected controller message: {other:?}"),
                        }).await.map_err(|_| "protocol writer stopped".to_string())?;
                    }
                }
            }
            done = done_rx.recv(), if active.is_some() => {
                let Some(done_id) = done else { break };
                if active.as_ref().is_some_and(|(active_id, _)| active_id == &done_id) {
                    active = None;
                }
            }
        }
    }

    reader.abort();
    drop(outgoing_tx);
    writer
        .await
        .map_err(|error| format!("protocol writer task failed: {error}"))??;
    Ok(())
}

async fn hello() -> Message {
    Message::Hello {
        min_version: PROTOCOL_VERSION,
        max_version: PROTOCOL_VERSION,
        runner_version: env!("CARGO_PKG_VERSION").to_string(),
        hostname: fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "unknown".to_string())
            .trim()
            .to_string(),
        user: std::env::var("USER").unwrap_or_default(),
        os: std::env::consts::OS.to_string(),
        os_release: read_os_release().await,
        arch: std::env::consts::ARCH.to_string(),
        capabilities: vec![
            "exec-shell-v1".to_string(),
            "exec-argv-v1".to_string(),
            "cancel-process-group-v1".to_string(),
            "kubelet-exec-v1".to_string(),
        ],
    }
}

async fn read_os_release() -> BTreeMap<String, String> {
    let (sender, receiver) = oneshot::channel();
    let worker = std::thread::Builder::new()
        .name("ranplant-os-release".to_string())
        .spawn(move || {
            let _ = sender.send(read_os_release_from_disk());
        });
    if worker.is_err() {
        return BTreeMap::new();
    }
    match timeout(OS_RELEASE_TIMEOUT, receiver).await {
        Ok(Ok(values)) => values,
        Ok(Err(_)) | Err(_) => BTreeMap::new(),
    }
}

fn read_os_release_from_disk() -> BTreeMap<String, String> {
    [
        Path::new("/etc/os-release"),
        Path::new("/usr/lib/os-release"),
    ]
    .into_iter()
    .filter_map(read_os_release_file)
    .find(|values| !values.is_empty())
    .unwrap_or_default()
}

fn read_os_release_file(path: &Path) -> Option<BTreeMap<String, String>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_OS_RELEASE_BYTES {
        return None;
    }

    let mut bytes = Vec::with_capacity(metadata.len().min(MAX_OS_RELEASE_BYTES) as usize);
    file.take(MAX_OS_RELEASE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_OS_RELEASE_BYTES {
        return None;
    }
    Some(parse_os_release(&String::from_utf8_lossy(&bytes)))
}

fn parse_os_release(contents: &str) -> BTreeMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, raw_value) = line.split_once('=')?;
            if key.is_empty()
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return None;
            }
            let values = shell_words::split(raw_value).ok()?;
            (values.len() == 1).then(|| (key.to_string(), values[0].clone()))
        })
        .collect()
}

async fn execute(
    id: String,
    execution: Execution,
    timeout: Duration,
    output_limit: u64,
    outgoing: mpsc::Sender<Message>,
    done: mpsc::UnboundedSender<String>,
    cancel: oneshot::Receiver<()>,
) {
    let result = execute_inner(
        &id,
        execution,
        timeout,
        output_limit,
        outgoing.clone(),
        cancel,
    )
    .await;
    if let Err(message) = result {
        let _ = outgoing
            .send(Message::Error {
                id: Some(id.clone()),
                message,
            })
            .await;
    }
    let _ = done.send(id);
}

async fn execute_inner(
    id: &str,
    execution: Execution,
    timeout: Duration,
    output_limit: u64,
    outgoing: mpsc::Sender<Message>,
    mut cancel: oneshot::Receiver<()>,
) -> Result<(), String> {
    let mut command = match execution {
        Execution::Shell { command } => {
            let mut process = Command::new("/bin/sh");
            process.arg("-c").arg(command);
            process
        }
        Execution::Argv {
            program,
            args,
            cwd,
            env,
        } => {
            let mut process = Command::new(program);
            process.args(args).envs(env);
            if let Some(cwd) = cwd {
                process.current_dir(cwd);
            }
            process
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start command: {error}"))?;
    let pid = child
        .id()
        .ok_or_else(|| "spawned command has no process id".to_string())?;
    outgoing
        .send(Message::Started {
            id: id.to_string(),
            pid,
        })
        .await
        .map_err(|_| "protocol writer stopped".to_string())?;

    let emitted = Arc::new(AtomicU64::new(0));
    let truncated = Arc::new(AtomicBool::new(false));
    let stdout = tokio::spawn(pump_output(
        id.to_string(),
        OutputStream::Stdout,
        child.stdout.take().expect("piped stdout"),
        outgoing.clone(),
        emitted.clone(),
        truncated.clone(),
        output_limit,
    ));
    let stderr = tokio::spawn(pump_output(
        id.to_string(),
        OutputStream::Stderr,
        child.stderr.take().expect("piped stderr"),
        outgoing.clone(),
        emitted,
        truncated.clone(),
        output_limit,
    ));

    let mut timed_out = false;
    let mut cancelled = false;
    let status = tokio::select! {
        status = child.wait() => status.map_err(|error| format!("failed waiting for command: {error}"))?,
        _ = tokio::time::sleep(timeout) => {
            timed_out = true;
            terminate_process_group(&mut child, pid).await?
        }
        _ = &mut cancel => {
            cancelled = true;
            terminate_process_group(&mut child, pid).await?
        }
    };
    let _ = stdout.await;
    let _ = stderr.await;

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    outgoing
        .send(Message::Exited {
            id: id.to_string(),
            code: status.code(),
            #[cfg(unix)]
            signal: status.signal(),
            #[cfg(not(unix))]
            signal: None,
            timed_out,
            cancelled,
            output_truncated: truncated.load(Ordering::Relaxed),
        })
        .await
        .map_err(|_| "protocol writer stopped".to_string())?;
    Ok(())
}

async fn terminate_process_group(
    child: &mut Child,
    pid: u32,
) -> Result<std::process::ExitStatus, String> {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
    let grace = tokio::time::sleep(TERMINATE_GRACE);
    tokio::pin!(grace);
    tokio::select! {
        status = child.wait() => status.map_err(|error| format!("failed waiting for terminated command: {error}")),
        _ = &mut grace => {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            child.wait().await.map_err(|error| format!("failed waiting for killed command: {error}"))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn pump_output(
    id: String,
    stream: OutputStream,
    mut reader: impl AsyncRead + Unpin,
    outgoing: mpsc::Sender<Message>,
    emitted: Arc<AtomicU64>,
    truncated: Arc<AtomicBool>,
    output_limit: u64,
) {
    let mut buffer = vec![0_u8; OUTPUT_CHUNK_BYTES];
    loop {
        let Ok(read) = reader.read(&mut buffer).await else {
            return;
        };
        if read == 0 {
            return;
        }
        let before = emitted.fetch_add(read as u64, Ordering::Relaxed);
        let available = output_limit.saturating_sub(before) as usize;
        let retained = read.min(available);
        if retained < read {
            truncated.store(true, Ordering::Relaxed);
        }
        if retained == 0 {
            continue;
        }
        let message = Message::Output {
            id: id.clone(),
            sequence: 0,
            stream,
            bytes: buffer[..retained].to_vec(),
        };
        if outgoing.send(message).await.is_err() {
            return;
        }
    }
}

async fn write_message(
    writer: &mut (impl AsyncWrite + Unpin),
    message: &Message,
) -> Result<(), String> {
    let payload = ranplant_protocol::encode(message)
        .map_err(|error| format!("failed to encode protocol message: {error}"))?;
    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await
        .map_err(|error| format!("failed to write protocol frame length: {error}"))?;
    writer
        .write_all(&payload)
        .await
        .map_err(|error| format!("failed to write protocol frame: {error}"))?;
    writer
        .flush()
        .await
        .map_err(|error| format!("failed to flush protocol frame: {error}"))
}

async fn read_message(reader: &mut (impl AsyncRead + Unpin)) -> Result<Message, String> {
    let length = reader
        .read_u32()
        .await
        .map_err(|error| format!("failed to read protocol frame length: {error}"))?
        as usize;
    if length > MAX_FRAME_BYTES {
        return Err("protocol frame exceeds maximum size".to_string());
    }
    let mut payload = vec![0_u8; length];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|error| format!("failed to read protocol frame: {error}"))?;
    ranplant_protocol::decode(&payload)
        .map_err(|error| format!("failed to decode protocol message: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::process::Stdio;

    use tempfile::{tempdir, NamedTempFile};
    use tokio::process::Command;

    #[tokio::test]
    async fn shell_exit_does_not_exit_ranplant_process() {
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 7")
            .stdin(Stdio::null())
            .status()
            .await
            .expect("run shell child");
        assert_eq!(status.code(), Some(7));
    }

    #[test]
    fn parses_standard_os_release_values_without_running_a_shell() {
        let parsed = super::parse_os_release(
            r#"
                NAME="Ubuntu"
                VERSION_ID="24.04"
                PRETTY_NAME="Ubuntu 24.04.1 LTS"
                ID=ubuntu
                # ignored
                invalid-key=value
            "#,
        );

        assert_eq!(parsed.get("ID").map(String::as_str), Some("ubuntu"));
        assert_eq!(
            parsed.get("PRETTY_NAME").map(String::as_str),
            Some("Ubuntu 24.04.1 LTS")
        );
        assert!(!parsed.contains_key("invalid-key"));
    }

    #[test]
    fn malformed_and_non_utf8_os_release_data_is_non_fatal() {
        let mut file = NamedTempFile::new().expect("temporary os-release");
        file.write_all(b"ID=test\nBROKEN=\"unterminated\nNOTE=bad-utf8-\xff\n")
            .expect("write os-release");

        let parsed = super::read_os_release_file(file.path()).expect("read bounded regular file");
        assert_eq!(parsed.get("ID").map(String::as_str), Some("test"));
        assert!(!parsed.contains_key("BROKEN"));
    }

    #[test]
    fn rejects_oversized_or_non_regular_os_release_sources() {
        let mut oversized = NamedTempFile::new().expect("temporary oversized os-release");
        oversized
            .write_all(&vec![b'x'; super::MAX_OS_RELEASE_BYTES as usize + 1])
            .expect("write oversized os-release");
        assert!(super::read_os_release_file(oversized.path()).is_none());

        let directory = tempdir().expect("temporary directory");
        assert!(super::read_os_release_file(directory.path()).is_none());
        assert!(super::read_os_release_file(&directory.path().join("missing")).is_none());
    }
}
