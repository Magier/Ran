use std::convert::Infallible;
use std::sync::OnceLock;
use std::time::Duration;

use async_stream::stream;
use axum::response::sse::{Event, Sse};
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::error;

#[derive(Debug, Clone)]
pub(crate) struct SseEnvelope {
    pub(crate) event: String,
    pub(crate) data: String,
}

static SSE_EVENT_BUS: OnceLock<broadcast::Sender<SseEnvelope>> = OnceLock::new();

pub(crate) fn sse_event_bus() -> &'static broadcast::Sender<SseEnvelope> {
    SSE_EVENT_BUS.get_or_init(|| {
        let (tx, _rx) = broadcast::channel(256);
        tx
    })
}

pub trait SsePayload: Serialize {
    const EVENT_TYPE: &'static str;
}

#[derive(Serialize)]
struct SseEvent<'a, P> {
    #[serde(rename = "type")]
    event_type: &'static str,
    data: &'a P,
}

pub fn serialize_sse_event<P: SsePayload>(payload: &P) -> serde_json::Result<String> {
    serde_json::to_string(&SseEvent {
        event_type: P::EVENT_TYPE,
        data: payload,
    })
}

pub fn publish_sse_event<P: SsePayload>(payload: P) {
    let data = match serialize_sse_event(&payload) {
        Ok(data) => data,
        Err(err) => {
            error!(event = P::EVENT_TYPE, %err, "failed to serialize SSE event");
            return;
        }
    };
    let _ = sse_event_bus().send(SseEnvelope {
        event: P::EVENT_TYPE.to_string(),
        data,
    });
}

pub(crate) async fn events_handler(armory: Vec<armory::Ttp>) -> impl axum::response::IntoResponse {
    let initial_payload = serialize_sse_event(&crate::ArmoryLoadedSseData(armory))
        .expect("armory-loaded SSE payload must serialize");

    let mut rx = sse_event_bus().subscribe();

    let event_stream = stream! {
        // Keep compatibility with frontend listener registration and message parser.
        yield Ok::<Event, Infallible>(
            Event::default().event("armory-loaded").data(initial_payload),
        );

        loop {
            tokio::select! {
                received = rx.recv() => {
                    match received {
                        Ok(msg) => {
                            yield Ok::<Event, Infallible>(
                                Event::default().event(msg.event).data(msg.data),
                            );
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(15)) => {
                    let ping = serialize_sse_event(&crate::PingSseData("keepalive".to_string()))
                        .expect("ping SSE payload must serialize");
                    yield Ok::<Event, Infallible>(
                        Event::default().event("ping").data(ping),
                    );
                }
            }
        }
    };

    Sse::new(event_stream)
}

impl From<campaign::ParseAudit> for crate::SseParseAudit {
    fn from(audit: campaign::ParseAudit) -> Self {
        let parse_result = match audit.parse_result {
            campaign::ParseResult::Parsed => "Parsed",
            campaign::ParseResult::KnownFailure => "KnownFailure",
            campaign::ParseResult::UnknownFormat => "UnknownFormat",
            campaign::ParseResult::NoParser => "NoParser",
            campaign::ParseResult::ParserBug => "ParserBug",
        };
        Self {
            cmd_id: audit.cmd_id,
            effect_id: audit.effect_id,
            ttp_id: audit.ttp_id,
            target_id: audit.target_id,
            parser_version: audit.parser_version,
            raw_output_hash: audit.raw_output_hash,
            raw_output_preview: audit.raw_output_preview,
            parse_result: parse_result.to_string(),
            detail: audit.detail,
            inferred_facts_written: audit.inferred_facts_written as i64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_event_and_envelope_type_cannot_drift() {
        let payload = crate::TtpOutputSseData {
            cmd_id: "cmd-1".to_string(),
            sequence: 2,
            stdout: "out".to_string(),
            stderr: String::new(),
            stdout_bytes: 3,
            stderr_bytes: 0,
        };

        let event: serde_json::Value =
            serde_json::from_str(&serialize_sse_event(&payload).unwrap()).unwrap();
        assert_eq!(
            event,
            serde_json::json!({
                "type": "ttp-output",
                "data": {
                    "cmdId": "cmd-1",
                    "sequence": 2,
                    "stdout": "out",
                    "stderr": "",
                    "stdoutBytes": 3,
                    "stderrBytes": 0,
                }
            })
        );
    }
}
