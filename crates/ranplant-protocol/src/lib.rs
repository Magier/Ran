use std::collections::BTreeMap;
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

pub const MAGIC: [u8; 4] = *b"RANP";
pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Message {
    Hello {
        min_version: u16,
        max_version: u16,
        runner_version: String,
        hostname: String,
        user: String,
        os: String,
        #[serde(default)]
        os_release: BTreeMap<String, String>,
        arch: String,
        capabilities: Vec<String>,
    },
    HelloAck {
        version: u16,
        heartbeat_interval_ms: u64,
        max_frame_bytes: u32,
    },
    Run {
        id: String,
        execution: Execution,
        timeout_ms: u64,
        output_limit: u64,
    },
    Started {
        id: String,
        pid: u32,
    },
    Output {
        id: String,
        sequence: u64,
        stream: OutputStream,
        bytes: Vec<u8>,
    },
    Exited {
        id: String,
        code: Option<i32>,
        signal: Option<i32>,
        timed_out: bool,
        cancelled: bool,
        output_truncated: bool,
    },
    Cancel {
        id: String,
    },
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    Shutdown,
    Error {
        id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum Execution {
    Shell {
        command: String,
    },
    Argv {
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        env: Vec<(String, String)>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

pub fn write_preface(mut writer: impl Write) -> io::Result<()> {
    writer.write_all(&MAGIC)
}

pub fn read_preface(mut reader: impl Read) -> io::Result<()> {
    let mut magic = [0_u8; MAGIC.len()];
    reader.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid Ranplant protocol preface",
        ));
    }
    Ok(())
}

pub fn encode(message: &Message) -> io::Result<Vec<u8>> {
    let mut payload = Vec::new();
    ciborium::into_writer(message, &mut payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Ranplant frame exceeds maximum size",
        ));
    }
    Ok(payload)
}

pub fn decode(payload: &[u8]) -> io::Result<Message> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Ranplant frame exceeds maximum size",
        ));
    }
    ciborium::from_reader(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

pub fn write_frame(mut writer: impl Write, message: &Message) -> io::Result<()> {
    let payload = encode(message)?;
    let length = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Ranplant frame is too large"))?;
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()
}

pub fn read_frame(mut reader: impl Read) -> io::Result<Message> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Ranplant frame exceeds maximum size",
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    decode(&payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    #[serde(tag = "type", rename_all = "kebab-case")]
    enum LegacyMessage {
        Hello {
            min_version: u16,
            max_version: u16,
            runner_version: String,
            hostname: String,
            user: String,
            os: String,
            arch: String,
            capabilities: Vec<String>,
        },
    }

    #[test]
    fn round_trips_binary_output() {
        let message = Message::Output {
            id: "cmd-1".to_string(),
            sequence: 2,
            stream: OutputStream::Stderr,
            bytes: vec![0, 0xff, b'\n'],
        };
        let mut wire = Vec::new();
        write_frame(&mut wire, &message).expect("encode frame");
        assert_eq!(read_frame(wire.as_slice()).expect("decode frame"), message);
    }

    #[test]
    fn rejects_oversized_frame_before_allocating_payload() {
        let mut wire = ((MAX_FRAME_BYTES as u32) + 1).to_be_bytes().to_vec();
        wire.extend_from_slice(b"ignored");
        let error = read_frame(wire.as_slice()).expect_err("oversized frame must fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn hello_without_os_release_remains_compatible() {
        let legacy = LegacyMessage::Hello {
            min_version: 1,
            max_version: 1,
            runner_version: "old".to_string(),
            hostname: "target".to_string(),
            user: "root".to_string(),
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            capabilities: Vec::new(),
        };
        let mut payload = Vec::new();
        ciborium::into_writer(&legacy, &mut payload).expect("encode legacy hello");

        let Message::Hello { os_release, .. } = decode(&payload).expect("decode legacy hello")
        else {
            panic!("expected hello");
        };
        assert!(os_release.is_empty());
    }
}
