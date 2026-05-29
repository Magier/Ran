use std::convert::Infallible;
use std::sync::OnceLock;
use std::time::Duration;

use async_stream::stream;
use axum::response::sse::{Event, Sse};
use tokio::sync::broadcast;

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

pub fn publish_sse_event(event: impl Into<String>, data: impl Into<String>) {
    let _ = sse_event_bus().send(SseEnvelope {
        event: event.into(),
        data: data.into(),
    });
}

pub(crate) async fn events_handler(armory: Vec<armory::Ttp>) -> impl axum::response::IntoResponse {
    let initial_payload = serde_json::json!({
        "type": "armory-loaded",
        "data": armory,
    })
    .to_string();

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
                    yield Ok::<Event, Infallible>(
                        Event::default().event("ping").data(r#"{"type":"ping","data":"keepalive"}"#),
                    );
                }
            }
        }
    };

    Sse::new(event_stream)
}
