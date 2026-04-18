use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use k8s::K8sService;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

use crate::builtin::BuiltinC2;
use crate::types::{C2Event, ExecTtp, TtpExecuted};

use crate::types::BUILTIN_C2_ID;

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
    backends: HashMap<String, Arc<dyn C2Backend>>,
}

#[async_trait]
trait C2Backend: Send + Sync {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted;
}

#[async_trait]
impl C2Backend for BuiltinC2 {
    async fn execute(&self, cmd: &ExecTtp) -> TtpExecuted {
        self.execute(cmd).await
    }
}

impl C2Manager {
    pub fn new(buffer_size: usize, k8s: K8sService) -> (C2Handle, C2EventBus, Self) {
        let (cmd_tx, cmd_rx) = mpsc::channel(buffer_size);
        let event_bus = C2EventBus::new(buffer_size);

        let builtin: Arc<dyn C2Backend> = Arc::new(BuiltinC2::new(k8s));
        let mut backends: HashMap<String, Arc<dyn C2Backend>> = HashMap::new();
        backends.insert(BUILTIN_C2_ID.to_string(), builtin.clone());
        backends.insert("ran".to_string(), builtin);

        (
            C2Handle { cmd_tx },
            event_bus.clone(),
            Self {
                cmd_rx,
                event_bus,
                backends,
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

        (
            C2Handle { cmd_tx },
            event_bus.clone(),
            Self {
                cmd_rx,
                event_bus,
                backends,
            },
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

        let backend = self.select_backend(cmd);
        backend.execute(cmd).await
    }

    fn select_backend(&self, cmd: &ExecTtp) -> Arc<dyn C2Backend> {
        let key = cmd.exec_system_id.trim().to_ascii_lowercase();

        if key.is_empty() {
            return self
                .backends
                .get(BUILTIN_C2_ID)
                .expect("builtin c2 backend must always be registered")
                .clone();
        }

        if let Some(backend) = self.backends.get(&key) {
            return backend.clone();
        }

        warn!(
            exec_system_id = %cmd.exec_system_id,
            "c2 backend not found; falling back to builtin c2"
        );

        self.backends
            .get(BUILTIN_C2_ID)
            .expect("builtin c2 backend must always be registered")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use armory::{Procedure, Ttp};

    use super::{C2Backend, C2Event, C2Manager, ExecTtp, TtpExecuted, BUILTIN_C2_ID};

    struct MockBackend {
        marker: String,
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
            }
        }
    }

    #[tokio::test]
    async fn unknown_exec_system_id_falls_back_to_builtin_backend() {
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
                assert_eq!(event.results, vec!["builtin"]);
                assert!(event.success);
            }
        }

        drop(handle);
        manager_task.await.expect("manager should shut down cleanly");
    }

    fn exec_cmd(exec_system_id: &str) -> ExecTtp {
        ExecTtp {
            id: "cmd-fallback".to_string(),
            started_at_ms: 0,
            ttp: Ttp {
                id: "T0001".to_string(),
                name: "Test TTP".to_string(),
                description: "test".to_string(),
                tactic: "Execution".to_string(),
                techniques: vec![],
                status: "stable".to_string(),
                params: vec![],
                requires: Default::default(),
                effects: vec![],
                procedures: vec![],
                references: vec![],
            },
            procedure: Procedure {
                id: "proc-1".to_string(),
                command: "id".to_string(),
                tool: None,
                is_local_command: None,
            },
            args: HashMap::new(),
            target_id: "ns/default/pod/nginx".to_string(),
            exec_entity_id: "ns/default/pod/nginx".to_string(),
            exec_system_id: exec_system_id.to_string(),
        }
    }
}
