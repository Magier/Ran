use std::sync::{Arc, RwLock};

use armory::Ttp;
use c2::{C2Event, C2EventBus, SessionConnectedData};
use ran_domain::{
    AccessLevel, Entity, EntityId, HostsListener, Listener, SessionChannel, SessionInfo,
    SessionStatus, UnknownSystem,
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::external_parser::{ExternalParseRequest, ExternalParser};
use crate::output_parsers::build_parse_audit;
use crate::{Campaign, FactOutcome, ParseAudit, ParseResult};
use ran_domain::RelationSummary;

/// Lightweight, serialisable snapshot of a domain entity for use in events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySummary {
    pub id: EntityId,
    pub kind: String,
    pub name: String,
    /// Whether this fact was observed, created by the action, or an update to an
    /// entity already known. Consumers that report "discovered" must check it.
    #[serde(default)]
    pub outcome: FactOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CampaignEvent {
    TtpExecuted {
        cmd_id: String,
        action_id: String,
        target_id: String,
        exec_system_id: String,
        ttp: Box<Ttp>,
        args: std::collections::HashMap<String, String>,
        success: bool,
        fail_reason: String,
        results: Vec<String>,
        exit_code: i32,
    },
    FactsChanged {
        cmd_id: String,
        new_entities: Vec<EntitySummary>,
        new_relations: Vec<RelationSummary>,
    },
    ParseAudited {
        cmd_id: String,
        audits: Vec<ParseAudit>,
    },
    Reset,
    PlanStepDispatched {
        plan_id: String,
        step_id: String,
        exec_count: usize,
    },
    PlanStepCompleted {
        plan_id: String,
        step_id: String,
        success: bool,
    },
    PlanStepSkipped {
        plan_id: String,
        step_id: String,
        reason: String,
    },
    PlanStepFailed {
        plan_id: String,
        step_id: String,
        reason: String,
    },
    PlanComplete {
        plan_id: String,
    },
}

#[derive(Clone)]
pub struct CampaignEventBus {
    tx: broadcast::Sender<CampaignEvent>,
}

impl CampaignEventBus {
    pub fn new(buffer_size: usize) -> Self {
        let (tx, _rx) = broadcast::channel(buffer_size);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CampaignEvent> {
        self.tx.subscribe()
    }

    pub fn publish(
        &self,
        event: CampaignEvent,
    ) -> Result<usize, Box<broadcast::error::SendError<CampaignEvent>>> {
        self.tx.send(event).map_err(Box::new)
    }
}

pub fn spawn_c2_event_processor(
    campaign: Arc<RwLock<Campaign>>,
    c2_events: C2EventBus,
    campaign_events: CampaignEventBus,
) -> JoinHandle<()> {
    spawn_c2_event_processor_with_external_parser(campaign, c2_events, campaign_events, None)
}

pub fn spawn_c2_event_processor_with_external_parser(
    campaign: Arc<RwLock<Campaign>>,
    c2_events: C2EventBus,
    campaign_events: CampaignEventBus,
    external_parser: Option<Arc<dyn ExternalParser>>,
) -> JoinHandle<()> {
    let mut c2_rx = c2_events.subscribe();

    tokio::spawn(async move {
        loop {
            match c2_rx.recv().await {
                Ok(C2Event::TtpExecuted { cmd, event }) => {
                    let action_id = cmd.ttp.id.clone();
                    let target_id = cmd.target_id.clone();
                    let result_preview = event
                        .results
                        .first()
                        .map(|r| {
                            if r.len() > 200 {
                                format!("{}...", &r[..200])
                            } else {
                                r.clone()
                            }
                        })
                        .unwrap_or_default();

                    info!(
                        cmd_id = %event.id,
                        action_id = %action_id,
                        target_id = %target_id,
                        success = event.success,
                        exit_code = event.exit_code,
                        fail_reason = %event.fail_reason,
                        results_count = event.results.len(),
                        result_preview = %result_preview,
                        "Action result"
                    );

                    let (processing, session_entity_summary) = {
                        let mut campaign_guard = match campaign.write() {
                            Ok(guard) => guard,
                            Err(_) => {
                                error!("campaign lock poisoned while processing c2 event");
                                continue;
                            }
                        };

                        let processing = match campaign_guard.on_ttp_executed(&cmd, &event) {
                            Ok(processing) => processing,
                            Err(err) => {
                                error!("failed to process c2 ttp result: {:?}", err);
                                continue;
                            }
                        };

                        // After effects are applied, activate any synchronous session
                        // that was opened during this TTP execution. The exec-channel
                        // edge (e.g. k8s.can-exec) now exists so activation will find it.
                        let session_summary = event.session_connected.as_ref().map(|s| {
                            let summary = apply_session_connected(&mut campaign_guard, s);
                            // Record the hop path that established this session so
                            // later commands tunneling over it can display the same
                            // traversal (the session itself routes opaquely).
                            record_session_path(&mut campaign_guard, &cmd.id, &s.backend_id);
                            summary
                        });

                        (processing, session_summary)
                    };

                    if processing.parse_audits.is_empty() {
                        warn!(
                            cmd_id = %cmd.id,
                            action_id = %action_id,
                            target_id = %target_id,
                            "Execution produced no parse audits; parser coverage may be missing"
                        );
                    } else {
                        for audit in &processing.parse_audits {
                            match audit.parse_result {
                                crate::ParseResult::Parsed => {
                                    info!(
                                        cmd_id = %cmd.id,
                                        effect_id = %audit.effect_id,
                                        parse_result = ?audit.parse_result,
                                        inferred_facts_written = audit.inferred_facts_written,
                                        detail = %audit.detail,
                                        "Parse audit"
                                    );
                                }
                                _ => {
                                    warn!(
                                        cmd_id = %cmd.id,
                                        effect_id = %audit.effect_id,
                                        parse_result = ?audit.parse_result,
                                        inferred_facts_written = audit.inferred_facts_written,
                                        detail = %audit.detail,
                                        "Parse audit indicates parser gap or known failure"
                                    );
                                }
                            }
                        }
                    }

                    // --- External parser fallback for NoParser gaps -----------
                    let mut final_audits = processing.parse_audits.clone();
                    let mut external_facts_changed = false;

                    if let Some(ref parser) = external_parser {
                        let no_parser_indices: Vec<usize> = final_audits
                            .iter()
                            .enumerate()
                            .filter(|(_, a)| matches!(a.parse_result, ParseResult::NoParser))
                            .map(|(i, _)| i)
                            .collect();

                        for idx in no_parser_indices {
                            let audit = &final_audits[idx];
                            let request = ExternalParseRequest {
                                effect_id: audit.effect_id.clone(),
                                ttp_id: audit.ttp_id.clone(),
                                target_id: cmd.target_id.clone(),
                                exec_system_id: cmd.exec_target().to_string(),
                                args: cmd.args.clone(),
                                results: event.results.clone(),
                                exit_code: event.exit_code,
                                success: event.success,
                            };

                            if let Some(response) = parser.try_parse(request).await {
                                let facts_written = {
                                    let mut guard = match campaign.write() {
                                        Ok(g) => g,
                                        Err(_) => {
                                            error!("campaign lock poisoned in external parser");
                                            continue;
                                        }
                                    };
                                    match guard
                                        .apply_system_update(&cmd.target_id, &response.system)
                                    {
                                        Ok(n) => n,
                                        Err(e) => {
                                            warn!(
                                                effect_id = %audit.effect_id,
                                                error = %e,
                                                "External parser produced result but \
                                                 target update failed"
                                            );
                                            0
                                        }
                                    }
                                };

                                if facts_written > 0 {
                                    external_facts_changed = true;
                                }

                                let detail = if response.detail.is_empty() {
                                    format!(
                                        "parsed by external script ({} facts written)",
                                        facts_written
                                    )
                                } else {
                                    response.detail.clone()
                                };

                                // Replace the NoParser audit with a successful one
                                final_audits[idx] = build_parse_audit(
                                    &audit.effect_id,
                                    &cmd,
                                    &event,
                                    ParseResult::Parsed,
                                    &detail,
                                    facts_written,
                                );

                                info!(
                                    cmd_id = %cmd.id,
                                    effect_id = %final_audits[idx].effect_id,
                                    facts_written,
                                    "External parser handled effect"
                                );
                            }
                        }
                    }

                    let _ = campaign_events.publish(CampaignEvent::TtpExecuted {
                        cmd_id: cmd.id.clone(),
                        action_id,
                        target_id,
                        exec_system_id: cmd.exec_entity().to_string(),
                        ttp: Box::new(cmd.ttp),
                        args: cmd.args,
                        // Use the effective success/fail_reason derived by the parser,
                        // which may override the raw transport-level success when a
                        // semantic error (e.g. k8s 403 Forbidden) was detected.
                        success: processing.effective_success,
                        fail_reason: processing.effective_fail_reason.clone(),
                        results: event.results,
                        exit_code: event.exit_code,
                    });

                    let _ = campaign_events.publish(CampaignEvent::ParseAudited {
                        cmd_id: cmd.id.clone(),
                        audits: final_audits,
                    });

                    if external_facts_changed {
                        // Notify frontend that entity data changed due to
                        // external parser.
                        let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                            cmd_id: cmd.id.clone(),
                            new_entities: Vec::new(),
                            new_relations: Vec::new(),
                        });
                    }

                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id: cmd.id.clone(),
                        new_entities: processing
                            .updates
                            .new_entities
                            .iter()
                            .map(|e| EntitySummary {
                                id: e.entity_id(),
                                kind: e.entity_kind().to_string(),
                                name: e.entity_name().to_string(),
                                outcome: processing.updates.outcome_of(&e.entity_id()),
                            })
                            .collect(),
                        new_relations: processing
                            .updates
                            .new_relations
                            .iter()
                            .map(|r| RelationSummary::from_relation(r.as_ref()))
                            .collect(),
                    });

                    // Notify frontend of session activation on the exec-channel edge.
                    if let Some(entity_summary) = session_entity_summary {
                        let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                            cmd_id: cmd.id,
                            new_entities: entity_summary.into_iter().collect(),
                            new_relations: vec![],
                        });
                    }
                }
                Ok(C2Event::ListenerStarted {
                    cmd_id,
                    port,
                    protocol,
                }) => {
                    let mut guard = match campaign.write() {
                        Ok(g) => g,
                        Err(_) => {
                            error!("campaign lock poisoned on ListenerStarted");
                            continue;
                        }
                    };
                    let c2_id = EntityId::new(c2::BUILTIN_C2_ID);
                    let listener = Listener::new(port, &protocol);
                    let listener_id = listener.entity_id();
                    let relation = HostsListener::new(c2_id.0.clone(), listener_id.0.clone());
                    guard.insert_entity(&listener);
                    guard.insert_relation(&relation);
                    info!(port, %protocol, %listener_id, "listener started; listener entity created");
                    // Attributed to the command that bound it, so the timeline
                    // folds the listener into that action instead of showing it
                    // as an unrelated event that happened to arrive next.
                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id,
                        new_entities: vec![EntitySummary {
                            id: listener_id,
                            kind: listener.entity_kind().to_string(),
                            name: listener.entry().to_string(),
                            // Binding a port is the action; a listener is never
                            // something the campaign stumbles upon.
                            outcome: FactOutcome::Created,
                        }],
                        new_relations: vec![ran_domain::RelationSummary::from_relation(&relation)],
                    });
                }
                Ok(C2Event::ListenerStopped { cmd_id, port }) => {
                    let mut guard = match campaign.write() {
                        Ok(g) => g,
                        Err(_) => {
                            error!("campaign lock poisoned on ListenerStopped");
                            continue;
                        }
                    };
                    // The protocol is not on the event, so drop whichever listener
                    // holds this port — a port can only be bound once.
                    let removed = guard.remove_listeners_on_port(port);
                    // Sessions caught through this listener are separate backends
                    // and keep running; only the binding is gone.
                    info!(port, removed, "listener stopped; listener entity removed");
                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id,
                        new_entities: vec![],
                        new_relations: vec![],
                    });
                }
                Ok(C2Event::SessionConnected {
                    backend_id,
                    target_entity_id,
                    hostname,
                    user,
                    os,
                    port,
                }) => {
                    info!(%backend_id, %target_entity_id, %hostname, %user, %os, port, "session connected");
                    let mut guard = match campaign.write() {
                        Ok(g) => g,
                        Err(_) => {
                            error!("campaign lock poisoned on SessionConnected");
                            continue;
                        }
                    };

                    let session_kind = if port.is_some() {
                        "tcp"
                    } else {
                        "kubectl-exec"
                    };
                    let session_short_id = backend_id
                        .strip_prefix("session/")
                        .unwrap_or(&backend_id)
                        .to_string();

                    // Resolve or create the target system entity.
                    let host_was_known = guard.get_system_entity(&target_entity_id).is_some();
                    let channel_entity_id = if host_was_known {
                        if let Some(mut sys) = guard.get_system_entity_mut(&target_entity_id) {
                            let system = sys.entity_mut().system_mut();
                            if !os.is_empty() {
                                system.os = Some(os.clone());
                            }
                            if !user.is_empty() {
                                system.username = Some(user.clone());
                            }
                            system.access_level = AccessLevel::Exec;
                            system.sessions.push(SessionInfo {
                                id: session_short_id,
                                kind: session_kind.to_string(),
                                port,
                                status: SessionStatus::Active,
                            });
                        }
                        target_entity_id.clone()
                    } else {
                        let sys_name = hostname.to_lowercase();
                        let mut sys = UnknownSystem::new(&sys_name);
                        sys.system.os = if os.is_empty() { None } else { Some(os) };
                        sys.system.username = if user.is_empty() { None } else { Some(user) };
                        sys.system.access_level = AccessLevel::Exec;
                        sys.system.sessions.push(SessionInfo {
                            id: session_short_id,
                            kind: session_kind.to_string(),
                            port,
                            status: SessionStatus::Active,
                        });
                        let entity_id = sys.entity_id().0.clone();
                        guard.insert_entity(&sys);
                        entity_id
                    };

                    // If the target already has an exec-channel edge, mark it as
                    // active so the session state lives on the existing relation.
                    // Only create a SessionChannel when no exec path exists yet
                    // (e.g. a reverse shell from a completely unknown host).
                    let has_existing_channel =
                        guard.activate_session_on_exec_channel(&channel_entity_id, &backend_id);

                    let new_relation_summary = if !has_existing_channel {
                        let c2_id = EntityId::new(c2::BUILTIN_C2_ID);
                        let channel = SessionChannel::new(
                            c2_id.0.clone(),
                            channel_entity_id.clone(),
                            &backend_id,
                        );
                        guard.insert_relation(&channel);
                        info!(%backend_id, %channel_entity_id, "no prior exec channel; SessionChannel created");
                        Some(ran_domain::RelationSummary::from_relation(&channel))
                    } else {
                        info!(%backend_id, %channel_entity_id, "session activated on existing exec-channel edge");
                        None
                    };

                    let entity_summary =
                        guard
                            .get_system_entity(&channel_entity_id)
                            .map(|e| EntitySummary {
                                id: e.entity().entity_id(),
                                kind: e.entity().entity_kind().to_string(),
                                name: e.entity().entity_name().to_string(),
                                outcome: if host_was_known {
                                    FactOutcome::Updated
                                } else {
                                    FactOutcome::Observed
                                },
                            });

                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id: backend_id.clone(),
                        new_entities: entity_summary.into_iter().collect(),
                        new_relations: new_relation_summary.into_iter().collect(),
                    });
                }
                Ok(C2Event::SessionLost {
                    backend_id,
                    target_entity_id,
                }) => {
                    let mut guard = match campaign.write() {
                        Ok(g) => g,
                        Err(_) => {
                            error!("campaign lock poisoned on SessionLost");
                            continue;
                        }
                    };
                    update_session_status(
                        &mut guard,
                        &target_entity_id,
                        &backend_id,
                        SessionStatus::Lost,
                    );
                    // Clear session_id from any exec-channel edge that carried
                    // this session, regardless of edge type or transport.
                    guard.deactivate_session(&backend_id);
                    info!(%backend_id, %target_entity_id, "session lost");
                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id: backend_id,
                        new_entities: vec![],
                        new_relations: vec![],
                    });
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(
                        skipped,
                        "campaign c2 event processor lagged behind c2 event bus"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("c2 event bus closed; stopping campaign c2 event processor");
                    break;
                }
            }
        }
    })
}

/// Apply a synchronous session connection to the campaign graph after TTP effects
/// have been processed. Updates the target system entity and activates the
/// session on any existing exec-channel edge. Returns an entity summary if the
/// frontend should be notified of entity changes.
/// Carry the multi-hop traversal of the command that opened a session over to
/// that session's backend id, so every later command routed over the session
/// (which resolves to empty graph hops) can replay the same path. No-op when
/// the establishing command was direct/single-hop.
fn record_session_path(campaign: &mut Campaign, establishing_cmd_id: &str, backend_id: &str) {
    let session_backend = if backend_id.starts_with("session/") {
        backend_id.to_string()
    } else {
        format!("session/{}", backend_id)
    };
    if let Some(hops) = campaign
        .command_traversals
        .get(establishing_cmd_id)
        .map(|ct| ct.hops.clone())
    {
        campaign.session_traversals.insert(session_backend, hops);
    }
}

fn apply_session_connected(
    campaign: &mut Campaign,
    data: &SessionConnectedData,
) -> Option<EntitySummary> {
    let session_short_id = data
        .backend_id
        .strip_prefix("session/")
        .unwrap_or(&data.backend_id)
        .to_string();

    if let Some(mut sys) = campaign.get_system_entity_mut(&data.target_entity_id) {
        let system = sys.entity_mut().system_mut();
        if !data.os.is_empty() {
            system.os = Some(data.os.clone());
        }
        if !data.user.is_empty() {
            system.username = Some(data.user.clone());
        }
        system.access_level = AccessLevel::Exec;
        system.sessions.push(SessionInfo {
            id: session_short_id,
            kind: "kubectl-exec".to_string(),
            port: None,
            status: SessionStatus::Active,
        });
    }

    campaign.activate_session_on_exec_channel(&data.target_entity_id, &data.backend_id);

    campaign
        .get_system_entity(&data.target_entity_id)
        .map(|e| EntitySummary {
            id: e.entity().entity_id(),
            kind: e.entity().entity_kind().to_string(),
            name: e.entity().entity_name().to_string(),
            // This path resolves an existing system entity and attaches a
            // session to it, so nothing here is new knowledge.
            outcome: FactOutcome::Updated,
        })
}

fn update_session_status(
    campaign: &mut Campaign,
    target_entity_id: &str,
    backend_id: &str,
    status: SessionStatus,
) {
    let Some(mut sys) = campaign.get_system_entity_mut(target_entity_id) else {
        return;
    };
    let sessions = &mut sys.entity_mut().system_mut().sessions;

    if let Some(s) = sessions.iter_mut().find(|s| s.backend_id() == backend_id) {
        // Forward-only status transition.
        use SessionStatus::*;
        match (&s.status, &status) {
            (Connecting, Active) | (Connecting, Lost) | (Active, Lost) => s.status = status,
            _ => {}
        }
    } else if status == SessionStatus::Active {
        // First time we hear about this session — the shell connected without a
        // prior listener TTP (e.g. a manually triggered reverse shell).
        let session_id = backend_id
            .strip_prefix("session/")
            .unwrap_or(backend_id)
            .to_string();
        sessions.push(SessionInfo {
            id: session_id,
            kind: "tcp".to_string(),
            port: None,
            status: SessionStatus::Active,
        });
    }
}

#[cfg(test)]
mod listener_event_tests {
    use super::*;
    use c2::C2EventBus;
    use std::time::Duration;

    /// Drain campaign events until a `FactsChanged` shows up, or give up.
    async fn next_facts_changed(
        rx: &mut broadcast::Receiver<CampaignEvent>,
    ) -> (String, Vec<EntitySummary>) {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("a FactsChanged should be published")
                .expect("campaign event bus should stay open");
            if let CampaignEvent::FactsChanged {
                cmd_id,
                new_entities,
                ..
            } = event
            {
                return (cmd_id, new_entities);
            }
        }
    }

    #[tokio::test]
    async fn binding_a_listener_reports_creation_attributed_to_its_command() {
        let campaign = Arc::new(RwLock::new(Campaign::bootstrap(
            "Ran",
            ran_domain::K8sCluster::new("dev"),
        )));
        let c2_events = C2EventBus::new(16);
        let campaign_events = CampaignEventBus::new(16);
        let mut rx = campaign_events.subscribe();

        spawn_c2_event_processor(campaign.clone(), c2_events.clone(), campaign_events);

        c2_events
            .publish(C2Event::ListenerStarted {
                cmd_id: "cmd-42".to_string(),
                port: 1337,
                protocol: "tcp".to_string(),
            })
            .expect("c2 event bus should accept the event");

        let (cmd_id, entities) = next_facts_changed(&mut rx).await;

        // Attributed to the action that bound it, so the timeline folds the
        // listener into that action instead of showing a row beside it.
        assert_eq!(cmd_id, "cmd-42");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].kind, "Listener");
        assert_eq!(entities[0].name, "tcp/1337");
        // The operator bound this port; the campaign did not come across it.
        assert_eq!(entities[0].outcome, FactOutcome::Created);
    }

    #[tokio::test]
    async fn stopping_a_listener_is_attributed_to_its_command_too() {
        let campaign = Arc::new(RwLock::new(Campaign::bootstrap(
            "Ran",
            ran_domain::K8sCluster::new("dev"),
        )));
        let c2_events = C2EventBus::new(16);
        let campaign_events = CampaignEventBus::new(16);
        let mut rx = campaign_events.subscribe();

        spawn_c2_event_processor(campaign.clone(), c2_events.clone(), campaign_events);

        c2_events
            .publish(C2Event::ListenerStopped {
                cmd_id: "cmd-stop".to_string(),
                port: 1337,
            })
            .expect("c2 event bus should accept the event");

        let (cmd_id, entities) = next_facts_changed(&mut rx).await;
        assert_eq!(cmd_id, "cmd-stop");
        assert!(entities.is_empty());
    }
}
