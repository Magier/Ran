use std::sync::{Arc, RwLock};

use armory::Ttp;
use c2::{C2Event, C2EventBus};
use ran_domain::{
    AccessLevel, C2Server, Entity, EntityId, SessionChannel, SessionInfo, SessionStatus,
    UnknownSystem,
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::external_parser::{ExternalParseRequest, ExternalParser};
use crate::output_parsers::build_parse_audit;
use crate::{Campaign, ParseAudit, ParseResult};
use ran_domain::RelationSummary;

/// Lightweight, serialisable snapshot of a domain entity for use in events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySummary {
    pub id: EntityId,
    pub kind: String,
    pub name: String,
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

                    let processing = {
                        let mut campaign_guard = match campaign.write() {
                            Ok(guard) => guard,
                            Err(_) => {
                                error!("campaign lock poisoned while processing c2 event");
                                continue;
                            }
                        };

                        match campaign_guard.on_ttp_executed(&cmd, &event) {
                            Ok(processing) => processing,
                            Err(err) => {
                                error!("failed to process c2 ttp result: {:?}", err);
                                continue;
                            }
                        }
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
                                exec_system_id: cmd.exec_system_id.clone(),
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
                        exec_system_id: cmd.exec_system_id,
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
                        cmd_id: cmd.id,
                        new_entities: processing
                            .updates
                            .new_entities
                            .iter()
                            .map(|e| EntitySummary {
                                id: e.entity_id(),
                                kind: e.entity_kind().to_string(),
                                name: e.entity_name().to_string(),
                            })
                            .collect(),
                        new_relations: processing
                            .updates
                            .new_relations
                            .iter()
                            .map(|r| RelationSummary::from_relation(r.as_ref()))
                            .collect(),
                    });
                }
                Ok(C2Event::ListenerStarted { port, protocol: _ }) => {
                    let mut guard = match campaign.write() {
                        Ok(g) => g,
                        Err(_) => {
                            error!("campaign lock poisoned on ListenerStarted");
                            continue;
                        }
                    };
                    let c2_id = EntityId::new(c2::BUILTIN_C2_ID);
                    if let Some(c2) = guard.entities.find_mut::<C2Server>(&c2_id) {
                        let entry = port.to_string();
                        if !c2.listeners.contains(&entry) {
                            c2.listeners.push(entry);
                        }
                    }
                    info!(port, "listener started; c2.listeners updated");
                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id: format!("listener-{port}"),
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
                    info!(%backend_id, %target_entity_id, %hostname, %user, %os, "SessionConnected received by campaign processor");
                    let mut guard = match campaign.write() {
                        Ok(g) => g,
                        Err(_) => {
                            error!("campaign lock poisoned on SessionConnected");
                            continue;
                        }
                    };

                    info!(%backend_id, %target_entity_id, %hostname, %user, %os, port, "session connected event received");

                    // Build a K8sNode carrying the session and the probed system info.
                    // If the node already exists (by name) the normal entity merge will
                    // fold these fields in; if not, this creates it for the first time.
                    let session_id = backend_id
                        .strip_prefix("session/")
                        .unwrap_or(&backend_id)
                        .to_string();
                    let sys_name = hostname.to_lowercase();
                    let mut sys = UnknownSystem::new(&sys_name);
                    sys.system.os = if os.is_empty() { None } else { Some(os) };
                    sys.system.username = if user.is_empty() { None } else { Some(user) };
                    sys.system.access_level = AccessLevel::Exec;
                    sys.system.sessions.push(SessionInfo {
                        id: session_id,
                        kind: "tcp".to_string(),
                        port,
                        status: SessionStatus::Active,
                    });

                    guard.insert_entity(&sys);

                    // C2Server → SessionChannel → UnknownSystem: live exec channel
                    // routed through the active session backend.
                    let c2_id = EntityId::new(c2::BUILTIN_C2_ID);
                    let channel = SessionChannel::new(
                        c2_id.0.clone(),
                        sys.entity_id().0.clone(),
                        &backend_id,
                    );
                    guard.insert_relation(&channel);
                    info!(%backend_id, %hostname, "session connected; system entity created/updated");

                    let sys_summary = EntitySummary {
                        id: sys.entity_id(),
                        kind: sys.entity_kind().to_string(),
                        name: sys.entity_name().to_string(),
                    };
                    let relation_summary = ran_domain::RelationSummary::from_relation(&channel);
                    // Publish entity first so the frontend node exists before the edge is added.
                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id: backend_id.clone(),
                        new_entities: vec![sys_summary],
                        new_relations: vec![],
                    });
                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id: backend_id,
                        new_entities: vec![],
                        new_relations: vec![relation_summary],
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
