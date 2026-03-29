use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

use crate::types::{C2Event, ExecTtp, TtpExecuted};

#[derive(Clone)]
pub struct C2Handle {
    cmd_tx: mpsc::Sender<ExecTtp>,
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

    pub fn publish(&self, event: C2Event) -> Result<usize, broadcast::error::SendError<C2Event>> {
        self.tx.send(event)
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
    event_bus: C2EventBus,
}

impl C2Manager {
    pub fn new(buffer_size: usize) -> (C2Handle, C2EventBus, Self) {
        let (cmd_tx, cmd_rx) = mpsc::channel(buffer_size);
        let event_bus = C2EventBus::new(buffer_size);

        (
            C2Handle { cmd_tx },
            event_bus.clone(),
            Self { cmd_rx, event_bus },
        )
    }

    pub async fn run(mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            let event = self.execute_command(&cmd).await;
            if self
                .event_bus
                .publish(C2Event::TtpExecuted { cmd, event })
                .is_err()
            {
                debug!("no c2 event subscribers currently registered");
            }
        }
        warn!("c2 command channel closed; stopping c2 manager loop");
    }

    async fn execute_command(&self, cmd: &ExecTtp) -> TtpExecuted {
        let is_set_target = cmd.procedure.command.trim_start().starts_with("setTarget(");

        if is_set_target {
            return TtpExecuted {
                id: cmd.id.clone(),
                success: true,
                results: vec!["ok".to_string()],
                exit_code: 0,
                fail_reason: String::new(),
            };
        }

        TtpExecuted {
            id: cmd.id.clone(),
            success: true,
            results: vec!["ok".to_string()],
            exit_code: 0,
            fail_reason: String::new(),
        }
    }
}
