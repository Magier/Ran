use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};

use api::ApiService;
use armory::{Armory, Ttp};
use c2::{C2Event, C2Handle, C2Manager, ExecTtp, ExecutionOperation};
use campaign::{Campaign, CampaignEvent, CampaignEventBus, ExecuteActionRequest, InitialKnowledge};
use ran_domain::{Entity, Pod, SessionStatus};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::broadcast,
};

struct Harness {
    state: app::AppState,
    campaign: Arc<RwLock<Campaign>>,
    c2: C2Handle,
    old_events: c2::C2EventBus,
    events: broadcast::Receiver<CampaignEvent>,
    manager: tokio::task::JoinHandle<()>,
    processor: tokio::task::JoinHandle<()>,
}

impl Harness {
    fn new(ttps: Vec<Ttp>) -> Self {
        Self::with_limits(
            ttps,
            c2::C2RuntimeLimits {
                command_queue_capacity: 64,
                event_buffer_capacity: 64,
                max_concurrent_executions: c2::DEFAULT_MAX_CONCURRENT_EXECUTIONS,
            },
        )
    }

    fn with_limits(ttps: Vec<Ttp>, limits: c2::C2RuntimeLimits) -> Self {
        let initial = InitialKnowledge::default();
        let campaign = Arc::new(RwLock::new(Campaign::bootstrap_with_knowledge(
            "Ran",
            initial.clone(),
        )));
        let (c2, old_events, manager) = C2Manager::new_with_limits(limits, None, HashMap::new());
        let bus = CampaignEventBus::new(128);
        let events = bus.subscribe();
        let processor =
            campaign::spawn_c2_event_processor(campaign.clone(), old_events.clone(), bus.clone());
        let manager = tokio::spawn(manager.run());
        let state = app::AppState::new(
            None,
            campaign.clone(),
            c2.clone(),
            Armory::from_ttps(ttps),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
            false,
            false,
            "Ran".into(),
            initial,
            bus,
            "plans".into(),
            kubetier::Catalog::embedded(),
        );
        Self {
            state,
            campaign,
            c2,
            old_events,
            events,
            manager,
            processor,
        }
    }

    async fn until(&mut self, predicate: impl Fn(&CampaignEvent) -> bool) -> CampaignEvent {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = self.events.recv().await.unwrap();
                if predicate(&event) {
                    return event;
                }
            }
        })
        .await
        .expect("campaign event")
    }

    async fn run(&mut self, action: &str, args: HashMap<String, String>) -> String {
        let result = self
            .state
            .execute_action(ExecuteActionRequest {
                action_id: action.into(),
                target_id: c2::BUILTIN_C2_ID.into(),
                exec_system_id: None,
                auth_identity_id: None,
                procedure_id: None,
                args,
                execution_timeout_seconds: None,
                reasoning: None,
            })
            .await
            .unwrap();
        let event = self.until(|event| matches!(event, CampaignEvent::TtpExecuted { cmd_id, .. } if cmd_id == &result.cmd_id)).await;
        assert!(
            matches!(event, CampaignEvent::TtpExecuted { success: true, .. }),
            "{event:?}"
        );
        result.cmd_id
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.manager.abort();
        self.processor.abort();
    }
}

fn ttp_from_yaml(yaml: &str) -> Ttp {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ttp.yaml"), yaml).unwrap();
    Armory::load_from_dir(dir.path()).unwrap().ttps()[0].clone()
}

fn listener_ttp(cleanup: bool) -> Ttp {
    let mut ttp = ttp_from_yaml(include_str!(
        "../../../armory/TTPs/Resource Development/create_listener.yaml"
    ));
    if !cleanup {
        ttp.cleanup = None;
    }
    ttp
}

async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    listener.local_addr().unwrap().port()
}

// A benign framed shell peer. Every probe/command reports the same pod name.
async fn shell_peer(port: u16) {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream.write_all(b"shell\n").await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line == "exit" {
            return;
        }
        let Some(rest) = line.strip_prefix("printf '") else {
            continue;
        };
        let marker = rest.split('%').next().unwrap_or("").trim_end_matches(':');
        if !marker.starts_with("__RAN_") {
            continue;
        }
        let reply = if marker.contains("INIT0") {
            format!("{marker}\n")
        } else {
            format!("agent-worker\n{marker}:0\n")
        };
        if writer.write_all(reply.as_bytes()).await.is_err() {
            return;
        }
    }
}

async fn connected(h: &mut Harness) -> String {
    let event = h
        .until(|event| {
            matches!(event, CampaignEvent::FactsChanged { new_relations, .. }
        if new_relations.iter().any(|r| r.name == "c2.session"))
        })
        .await;
    let CampaignEvent::FactsChanged { cmd_id, .. } = event else {
        unreachable!()
    };
    cmd_id
}

fn command(id: &str, backend: &str, target: &str) -> ExecTtp {
    ExecTtp {
        id: id.into(),
        ttp: Ttp::new("test", "Test", "Execution"),
        procedure: armory::Procedure::new("test", "hostname"),
        operation: ExecutionOperation::Shell {
            command: "hostname".into(),
        },
        target_id: target.into(),
        exec_system_id: backend.into(),
        exec_chain: vec![target.into()],
        args: HashMap::new(),
        started_at_ms: 0,
        execution_timeout_seconds: 5,
        execution_environment: None,
        transport_environment: None,
        auth_identity_id: None,
        output_transform: None,
        is_cleanup: false,
        reasoning: String::new(),
    }
}

async fn reset_reconnect_delayed_loss(declared_cleanup: bool) {
    let mut h = Harness::new(vec![listener_ttp(declared_cleanup)]);
    let pod = Pod::new("agent-worker", "default");
    let target = pod.entity_id().0;
    h.campaign
        .write()
        .unwrap()
        .upsert_entity(pod.clone(), campaign::KnowledgeProvenance::Action);
    let port = free_port().await;
    let args = HashMap::from([("PORT".into(), port.to_string())]);
    h.run("create-listener", args.clone()).await;
    let old_peer = tokio::spawn(shell_peer(port));
    let old_id = connected(&mut h).await;

    h.state
        .reset_campaign()
        .await
        .expect("reset with live transports");
    tokio::time::timeout(Duration::from_secs(1), old_peer)
        .await
        .expect("old socket closed")
        .unwrap();
    // Recreating the actual listener on the same port below verifies release
    // through the production bind path, followed by a usable callback.
    while h.events.try_recv().is_ok() {}
    h.campaign
        .write()
        .unwrap()
        .upsert_entity(pod.clone(), campaign::KnowledgeProvenance::Action);
    h.run("create-listener", args).await;
    let new_peer = tokio::spawn(shell_peer(port));
    let new_id = connected(&mut h).await;
    assert_ne!(old_id, new_id);
    while h.events.try_recv().is_ok() {}

    // Simulate both a queued connection and delayed lifecycle events from the
    // retired publisher, including a killed event with a reused backend ID.
    h.old_events
        .publish(C2Event::SessionConnected {
            backend_id: old_id.clone(),
            target_entity_id: "node/agent-worker".into(),
            hostname: "agent-worker".into(),
            user: "root".into(),
            os: "Linux".into(),
            kind: "tcp".into(),
            port: Some(port),
        })
        .await
        .unwrap();
    h.old_events
        .publish(C2Event::SessionLost {
            backend_id: old_id,
            target_entity_id: "node/agent-worker".into(),
        })
        .await
        .unwrap();
    h.old_events
        .publish(C2Event::SessionKilled {
            backend_id: new_id.clone(),
        })
        .await
        .unwrap();
    h.c2.send(command("after-reset", &new_id, &target))
        .await
        .unwrap();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), h.events.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            CampaignEvent::SessionStateChanged { .. } => {
                panic!("stale lifecycle notification: {event:?}")
            }
            CampaignEvent::FactsChanged { cmd_id, .. } => {
                assert_eq!(cmd_id, "after-reset", "stale facts notification")
            }
            CampaignEvent::TtpExecuted {
                cmd_id,
                success,
                results,
                ..
            } if cmd_id == "after-reset" => {
                assert!(success);
                assert_eq!(results, ["agent-worker"]);
                break;
            }
            _ => {}
        }
    }
    let snapshot = h.state.get_campaign().await.unwrap();
    let system = snapshot.get_system_entity(&target).unwrap();
    assert_eq!(system.entity().system().sessions.len(), 1);
    assert_eq!(system.entity().system().sessions[0].backend_id(), new_id);
    assert_eq!(
        system.entity().system().sessions[0].status,
        SessionStatus::Active
    );
    assert!(snapshot
        .get_relations()
        .iter()
        .any(|r| r.session_id.as_deref() == Some(&new_id) && !r.broken));
    // A current loss still produces a notification with the current identity.
    new_peer.abort();
    let _ = new_peer.await;
    h.c2.send(command("detect-loss", &new_id, &target))
        .await
        .unwrap();
    h.until(|event| matches!(event, CampaignEvent::SessionStateChanged { backend_id, state: campaign::runtime::SessionLifecycle::Lost, .. } if backend_id == &new_id)).await;
    h.state.reset_campaign().await.unwrap();
}

#[tokio::test]
async fn reset_closes_live_transports_and_isolates_same_pod_reconnect() {
    reset_reconnect_delayed_loss(true).await;
}

#[tokio::test]
async fn reset_tears_down_transports_without_declared_cleanup() {
    reset_reconnect_delayed_loss(false).await;
}

#[tokio::test]
async fn failed_cleanup_returns_error_retains_diagnostics_and_releases_listener() {
    let failing = ttp_from_yaml(
        r#"
id: failing-cleanup
name: Failing cleanup
tactic: Execution
procedures:
  - id: local
    command: 'true'
    isLocal: true
cleanup:
  id: cleanup
  command: 'echo cleanup-failure; exit 23'
  isLocal: true
"#,
    );
    let mut h = Harness::new(vec![listener_ttp(false), failing]);
    let port = free_port().await;
    h.run(
        "create-listener",
        HashMap::from([("PORT".into(), port.to_string())]),
    )
    .await;
    h.run("failing-cleanup", HashMap::new()).await;
    let error = h
        .state
        .reset_campaign()
        .await
        .expect_err("cleanup failure must surface");
    assert!(error.body.error.contains("cleanup incomplete"), "{error:?}");
    let snapshot = h.state.get_campaign().await.unwrap();
    assert!(snapshot
        .get_execution_records()
        .iter()
        .any(|r| r.is_cleanup && !r.success && r.exit_code == 23));
    assert!(tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .is_ok());
    assert!(!std::iter::from_fn(|| h.events.try_recv().ok())
        .any(|event| matches!(event, CampaignEvent::Reset)));
}

#[tokio::test]
async fn cleanup_preparation_failure_is_reported() {
    let ttp = ttp_from_yaml(
        r#"
id: missing-cleanup-argument
name: Missing cleanup argument
tactic: Execution
procedures:
  - id: local
    command: 'true'
    isLocal: true
cleanup:
  command: 'c2.listen(${MISSING}, tcp)'
  isLocal: true
  operation:
    type: start-listener
    port: ${MISSING}
    protocol: tcp
"#,
    );
    let mut h = Harness::new(vec![ttp]);
    h.run("missing-cleanup-argument", HashMap::new()).await;
    let error = h.state.reset_campaign().await.unwrap_err();
    assert!(
        error.body.error.contains("cleanup preparation failed"),
        "{error:?}"
    );
}

#[tokio::test]
async fn cleanup_timeout_is_reported_and_transport_teardown_still_runs() {
    let ttp = ttp_from_yaml(
        r#"
id: hanging-cleanup
name: Hanging cleanup
tactic: Execution
procedures:
  - id: local
    command: 'true'
    isLocal: true
cleanup:
  command: 'exec sleep 60'
  isLocal: true
"#,
    );
    let mut h = Harness::new(vec![listener_ttp(false), ttp]);
    let port = free_port().await;
    h.run(
        "create-listener",
        HashMap::from([("PORT".into(), port.to_string())]),
    )
    .await;
    h.run("hanging-cleanup", HashMap::new()).await;
    let error = tokio::time::timeout(Duration::from_secs(35), h.state.reset_campaign())
        .await
        .expect("reset must bound cleanup")
        .unwrap_err();
    assert!(error.body.error.contains("cleanup timed out"), "{error:?}");
    assert!(tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .is_ok());
}

#[tokio::test]
async fn reset_after_manual_listener_stop_is_idempotent() {
    let mut h = Harness::new(vec![listener_ttp(true)]);
    let port = free_port().await;
    h.run(
        "create-listener",
        HashMap::from([("PORT".into(), port.to_string())]),
    )
    .await;
    let mut stop = command("manual-stop", c2::BUILTIN_C2_ID, c2::BUILTIN_C2_ID);
    stop.operation = ExecutionOperation::StopListener {
        listener: format!("tcp/{port}"),
    };
    h.c2.send(stop).await.unwrap();
    h.until(|event| matches!(event, CampaignEvent::TtpExecuted { cmd_id, success: true, .. } if cmd_id == "manual-stop")).await;
    h.state.reset_campaign().await.unwrap();
}

#[tokio::test]
async fn reset_retry_skips_each_successful_cleanup_and_retries_failures() {
    let dir = tempfile::tempdir().unwrap();
    let allowed = dir.path().join("allowed");
    let cleaned = dir.path().join("cleaned");
    let once = ttp_from_yaml(&format!(
        r#"
id: clean-once
name: Clean once
tactic: Execution
procedures:
  - id: local
    command: 'true'
    isLocal: true
cleanup:
  command: 'echo cleaned >> {}'
  isLocal: true
"#,
        cleaned.display()
    ));
    let retry = ttp_from_yaml(&format!(
        r#"
id: retry-cleanup
name: Retry cleanup
tactic: Execution
procedures:
  - id: local
    command: 'true'
    isLocal: true
cleanup:
  command: 'test -f {}'
  isLocal: true
"#,
        allowed.display()
    ));
    let mut h = Harness::new(vec![listener_ttp(true), once, retry]);
    let port = free_port().await;
    h.run(
        "create-listener",
        HashMap::from([("PORT".into(), port.to_string())]),
    )
    .await;
    let first = h.run("clean-once", HashMap::new()).await;
    let second = h.run("clean-once", HashMap::new()).await;
    assert_ne!(first, second);
    h.run("retry-cleanup", HashMap::new()).await;
    h.state.reset_campaign().await.unwrap_err();
    assert_eq!(
        std::fs::read_to_string(&cleaned).unwrap().lines().count(),
        2
    );
    std::fs::write(&allowed, "ready").unwrap();
    h.state
        .reset_campaign()
        .await
        .expect("successful cleanup must not be replayed");
    assert_eq!(
        std::fs::read_to_string(&cleaned).unwrap().lines().count(),
        2
    );
}

struct GatedBackend {
    started: tokio::sync::mpsc::UnboundedSender<()>,
    release: tokio::sync::Semaphore,
}

#[async_trait::async_trait]
impl c2::C2Backend for GatedBackend {
    async fn execute(&self, cmd: &ExecTtp) -> c2::TtpExecuted {
        self.started.send(()).unwrap();
        self.release.acquire().await.unwrap().forget();
        c2::TtpExecuted {
            id: cmd.id.clone(),
            success: true,
            exit_code: 0,
            results: vec![],
            fail_reason: String::new(),
            session_connected: None,
        }
    }
}

#[tokio::test]
async fn reset_reaches_quiesce_while_an_api_request_waits_for_queue_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let proof = dir.path().join("cleanup-proof");
    let cleanup = ttp_from_yaml(&format!(
        r#"
id: cleanup-proof
name: Cleanup proof
tactic: Execution
procedures:
  - id: local
    command: 'true'
    isLocal: true
cleanup:
  command: 'touch {}'
  isLocal: true
"#,
        proof.display()
    ));
    let mut h = Harness::with_limits(
        vec![listener_ttp(true), cleanup],
        c2::C2RuntimeLimits {
            command_queue_capacity: 1,
            event_buffer_capacity: 16,
            max_concurrent_executions: 1,
        },
    );
    h.run("cleanup-proof", HashMap::new()).await;
    let (started, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = Arc::new(GatedBackend {
        started,
        release: tokio::sync::Semaphore::new(0),
    });
    h.c2.register_backend("ran", backend.clone()).await;
    h.c2.send(command("running", "ran", c2::BUILTIN_C2_ID))
        .await
        .unwrap();
    started_rx.recv().await.unwrap();
    h.c2.send(command("queued", "ran", c2::BUILTIN_C2_ID))
        .await
        .unwrap();
    let state = h.state.clone();
    let mut waiting = tokio::spawn(async move {
        state
            .execute_action(ExecuteActionRequest {
                action_id: "create-listener".into(),
                target_id: c2::BUILTIN_C2_ID.into(),
                exec_system_id: None,
                auth_identity_id: None,
                procedure_id: None,
                args: HashMap::new(),
                execution_timeout_seconds: None,
                reasoning: None,
            })
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut waiting)
            .await
            .is_err()
    );
    let state = h.state.clone();
    let reset = tokio::spawn(async move { state.reset_campaign().await });
    // Cancellation of the queued action proves reset has acquired the write
    // guard and reached C2 even while the running action remains blocked.
    h.until(|event| matches!(event, CampaignEvent::TtpExecuted { cmd_id, success: false, .. } if cmd_id == "queued")).await;
    backend.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), reset)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let error = waiting.await.unwrap().unwrap_err();
    assert!(
        error.body.error.contains("campaign changed")
            || error.body.error.contains("reset in progress")
    );
    assert!(
        proof.exists(),
        "waiting API request must release capacity for cleanup"
    );
    assert!(h
        .state
        .get_campaign()
        .await
        .unwrap()
        .entities_of::<ran_domain::Listener>()
        .next()
        .is_none());
    assert!(
        started_rx.try_recv().is_err(),
        "queued action must never start"
    );
}

#[tokio::test]
async fn reset_cleans_resource_created_by_an_action_that_finishes_during_quiesce() {
    let dir = tempfile::tempdir().unwrap();
    let resource = dir.path().join("resource");
    let proof = dir.path().join("cleanup-proof");
    let ttp = ttp_from_yaml(&format!(
        r#"
id: slow-create
name: Slow create
tactic: Execution
procedures:
  - id: local
    command: 'printf started; sleep 1; touch {}'
    isLocal: true
cleanup:
  command: 'test -f {} && rm {} && touch {}'
  isLocal: true
"#,
        resource.display(),
        resource.display(),
        resource.display(),
        proof.display()
    ));
    let mut h = Harness::new(vec![ttp]);
    h.state
        .execute_action(ExecuteActionRequest {
            action_id: "slow-create".into(),
            target_id: c2::BUILTIN_C2_ID.into(),
            exec_system_id: None,
            auth_identity_id: None,
            procedure_id: None,
            args: HashMap::new(),
            execution_timeout_seconds: None,
            reasoning: None,
        })
        .await
        .unwrap();
    h.until(
        |event| matches!(event, CampaignEvent::TtpOutput { stdout, .. } if stdout == "started"),
    )
    .await;
    h.state.reset_campaign().await.unwrap();
    assert!(proof.exists(), "cleanup must include the late completion");
    assert!(!resource.exists());
}
