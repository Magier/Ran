use std::io::Read;
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use ranplant_protocol::{
    read_frame, read_preface, write_frame, Execution, Message, OutputStream, PROTOCOL_VERSION,
};

struct Harness {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: std::process::ChildStdout,
}

impl Harness {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ranplant"))
            .args(["session", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start Ranplant");
        let mut output = child.stdout.take().expect("Ranplant stdout");
        read_preface(&mut output).expect("protocol preface");
        let hello = read_frame(&mut output).expect("hello frame");
        assert!(matches!(hello, Message::Hello { .. }));
        let mut input = child.stdin.take().expect("Ranplant stdin");
        write_frame(
            &mut input,
            &Message::HelloAck {
                version: PROTOCOL_VERSION,
                heartbeat_interval_ms: 30_000,
                max_frame_bytes: ranplant_protocol::MAX_FRAME_BYTES as u32,
            },
        )
        .expect("hello acknowledgement");
        Self {
            child,
            input,
            output,
        }
    }

    fn run_shell(&mut self, id: &str, command: &str, timeout_ms: u64) -> Transcript {
        write_frame(
            &mut self.input,
            &Message::Run {
                id: id.to_string(),
                execution: Execution::Shell {
                    command: command.to_string(),
                },
                timeout_ms,
                output_limit: 1024 * 1024,
            },
        )
        .expect("run frame");
        let mut transcript = Transcript::default();
        loop {
            match read_frame(&mut self.output).expect("command response") {
                Message::Started { id: response, .. } => assert_eq!(response, id),
                Message::Output {
                    id: response,
                    stream,
                    bytes,
                    ..
                } => {
                    assert_eq!(response, id);
                    match stream {
                        OutputStream::Stdout => transcript.stdout.extend(bytes),
                        OutputStream::Stderr => transcript.stderr.extend(bytes),
                    }
                }
                Message::Exited {
                    id: response,
                    code,
                    signal,
                    timed_out,
                    cancelled,
                    ..
                } => {
                    assert_eq!(response, id);
                    transcript.code = code;
                    transcript.signal = signal;
                    transcript.timed_out = timed_out;
                    transcript.cancelled = cancelled;
                    return transcript;
                }
                other => panic!("unexpected command response: {other:?}"),
            }
        }
    }

    fn shutdown(mut self) {
        write_frame(&mut self.input, &Message::Shutdown).expect("shutdown frame");
        drop(self.input);
        let mut remainder = Vec::new();
        let _ = self.output.read_to_end(&mut remainder);
        let status = self.child.wait().expect("wait for Ranplant");
        assert!(status.success(), "Ranplant exited with {status}");
    }
}

#[derive(Default)]
struct Transcript {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    code: Option<i32>,
    signal: Option<i32>,
    timed_out: bool,
    cancelled: bool,
}

#[test]
fn failed_shell_cannot_poison_the_next_command() {
    let mut runner = Harness::start();
    let failed = runner.run_shell(
        "cmd-failed",
        "printf 'before\\n'; printf 'diagnostic\\n' >&2; exit 7",
        5_000,
    );
    assert_eq!(failed.code, Some(7));
    assert_eq!(failed.stdout, b"before\n");
    assert_eq!(failed.stderr, b"diagnostic\n");

    let next = runner.run_shell("cmd-next", "printf 'alive\\n'", 5_000);
    assert_eq!(next.code, Some(0));
    assert_eq!(next.stdout, b"alive\n");
    runner.shutdown();
}

#[test]
fn timeout_kills_the_command_without_losing_the_session() {
    let mut runner = Harness::start();
    let timed_out = runner.run_shell("cmd-timeout", "sleep 30", 50);
    assert!(timed_out.timed_out);
    assert_ne!(timed_out.code, Some(0));

    let next = runner.run_shell("cmd-next", "printf 'recovered\\n'", 5_000);
    assert_eq!(next.code, Some(0));
    assert_eq!(next.stdout, b"recovered\n");
    runner.shutdown();
}

#[test]
fn native_connect_negotiates_the_same_protocol() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
    let address = listener.local_addr().expect("listener address");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ranplant"))
        .args([
            "connect",
            "--host",
            &address.ip().to_string(),
            "--port",
            &address.port().to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start native Ranplant connection");

    let (mut stream, _) = listener.accept().expect("accept Ranplant");
    read_preface(&mut stream).expect("protocol preface");
    assert!(matches!(
        read_frame(&mut stream).expect("hello"),
        Message::Hello { .. }
    ));
    write_frame(
        &mut stream,
        &Message::HelloAck {
            version: PROTOCOL_VERSION,
            heartbeat_interval_ms: 30_000,
            max_frame_bytes: ranplant_protocol::MAX_FRAME_BYTES as u32,
        },
    )
    .expect("hello acknowledgement");
    write_frame(&mut stream, &Message::Shutdown).expect("shutdown");

    let status = child.wait().expect("wait for native connection");
    assert!(status.success(), "Ranplant exited with {status}");
}

#[test]
fn detached_connect_returns_only_after_the_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
    let address = listener.local_addr().expect("listener address");
    let controller = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept Ranplant");
        read_preface(&mut stream).expect("protocol preface");
        assert!(matches!(
            read_frame(&mut stream).expect("hello"),
            Message::Hello { .. }
        ));
        write_frame(
            &mut stream,
            &Message::HelloAck {
                version: PROTOCOL_VERSION,
                heartbeat_interval_ms: 30_000,
                max_frame_bytes: ranplant_protocol::MAX_FRAME_BYTES as u32,
            },
        )
        .expect("hello acknowledgement");
        thread::sleep(Duration::from_millis(100));
        write_frame(&mut stream, &Message::Shutdown).expect("shutdown");
    });

    let output = Command::new(env!("CARGO_BIN_EXE_ranplant"))
        .args([
            "connect",
            "--host",
            &address.ip().to_string(),
            "--port",
            &address.port().to_string(),
            "--detach",
        ])
        .output()
        .expect("start detached Ranplant connection");

    assert!(
        output.status.success(),
        "detached launch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    controller.join().expect("controller thread");
}

#[test]
fn detached_connect_reports_connection_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve test port");
    let address = listener.local_addr().expect("listener address");
    drop(listener);

    let output = Command::new(env!("CARGO_BIN_EXE_ranplant"))
        .args([
            "connect",
            "--host",
            &address.ip().to_string(),
            "--port",
            &address.port().to_string(),
            "--detach",
        ])
        .output()
        .expect("start detached Ranplant connection");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to connect"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
