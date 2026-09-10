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
                Ok(C2Event::ListenerStarted { port, protocol }) => {
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
                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id: format!("listener-{port}"),
                        new_entities: vec![EntitySummary {
                            id: listener_id,
                            kind: listener.entity_kind().to_string(),
                            name: listener.entry().to_string(),
                        }],
                        new_relations: vec![ran_domain::RelationSummary::from_relation(&relation)],
                    });
                }
                Ok(C2Event::ListenerStopped { port }) => {
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
                        cmd_id: format!("listener-{port}-stopped"),
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

                    let channel_entity_id = attach_connected_session(
                        &mut guard,
                        &backend_id,
                        &target_entity_id,
                        &hostname,
                        &user,
                        &os,
                        port,
                    );

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
                    // Mark any exec-channel edge that carried this session as
                    // broken rather than clearing it: the edge stays in the
                    // graph (styled as broken) and keeps its session_id so a
                    // reconnecting session can recover it, but path-finding
                    // treats it as non-traversable in the meantime.
                    let marked = guard.mark_session_broken(&backend_id);
                    info!(%backend_id, %target_entity_id, marked, "session lost; exec-channel edge(s) marked broken");
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

/// Attach a freshly connected C2 session to the system entity it exits into,
/// returning that entity's id.
///
/// `target_entity_id` is whatever the C2 believed it was connecting to. For a
/// reverse shell that is derived from the probed hostname and usually names no
/// entity at all, so an `UnknownSystem` is created for it — and the C2's id is
/// recorded as an alias of that system, so a later reconnect or status change
/// still keyed by the C2's id lands on the right entity even after an analyzer
/// has merged that system into the Pod it turned out to be.
fn attach_connected_session(
    campaign: &mut Campaign,
    backend_id: &str,
    target_entity_id: &str,
    hostname: &str,
    user: &str,
    os: &str,
    port: Option<u16>,
) -> String {
    let session_kind = if port.is_some() {
        "tcp"
    } else {
        "kubectl-exec"
    };
    let session_short_id = backend_id
        .strip_prefix("session/")
        .unwrap_or(backend_id)
        .to_string();

    // Resolve the target system entity, following any merge it went through.
    let resolved_id = campaign.canonical_entity_id(target_entity_id);
    if let Some(mut sys) = campaign.get_system_entity_mut(&resolved_id) {
        let system = sys.entity_mut().system_mut();
        if !os.is_empty() {
            system.os = Some(os.to_string());
        }
        if !user.is_empty() {
            system.username = Some(user.to_string());
        }
        system.access_level = AccessLevel::Exec;
        // A reconnect comes back on the listener's own backend id: revive that
        // session instead of pushing a duplicate next to the dead one.
        match system
            .sessions
            .iter_mut()
            .find(|s| s.id == session_short_id)
        {
            Some(existing) => {
                existing.kind = session_kind.to_string();
                existing.port = port;
                existing.status = SessionStatus::Active;
            }
            None => system.sessions.push(SessionInfo {
                id: session_short_id,
                kind: session_kind.to_string(),
                port,
                status: SessionStatus::Active,
            }),
        }
        return resolved_id;
    }

    // Nothing answers to that id — a shell from a host we have never seen.
    // Create a system for it, named after the hostname it reported.
    let mut sys = UnknownSystem::new(hostname.to_lowercase());
    sys.system.os = if os.is_empty() {
        None
    } else {
        Some(os.to_string())
    };
    sys.system.username = if user.is_empty() {
        None
    } else {
        Some(user.to_string())
    };
    sys.system.access_level = AccessLevel::Exec;
    sys.system.sessions.push(SessionInfo {
        id: session_short_id,
        kind: session_kind.to_string(),
        port,
        status: SessionStatus::Active,
    });
    let entity_id = sys.entity_id();
    campaign.insert_entity(&sys);
    campaign.record_entity_alias(&EntityId::new(target_entity_id), &entity_id);
    entity_id.0
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
mod tests {
    use ran_domain::{K8sCluster, Pod, UnknownSystem};

    use super::*;

    /// The variables kubelet injects into every container it starts — enough of
    /// them for `InClusterPodAnalyzer` to promote the system carrying them.
    fn kubelet_env() -> Vec<(String, String)> {
        [
            ("KUBERNETES_SERVICE_HOST", "10.96.0.1"),
            ("KUBERNETES_SERVICE_PORT", "443"),
            ("KUBERNETES_PORT_443_TCP", "tcp://10.96.0.1:443"),
            ("HOSTNAME", "netshoot"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// A campaign whose only foothold is a reverse shell on `netshoot`, after
    /// reading the container's environment promoted it from an `UnknownSystem`
    /// to `ns/?/pod/netshoot`. The entity id therefore changed under the C2,
    /// which still knows that shell by the target id it connected with.
    fn promoted_foothold() -> Campaign {
        let mut campaign = Campaign::bootstrap("ran", K8sCluster::new("test-cluster"));

        // The C2 names a reverse-shell target after the hostname it probed, so
        // this id belongs to no entity in the graph.
        let system_id = attach_connected_session(
            &mut campaign,
            "session/s1",
            "node/netshoot",
            "netshoot",
            "root",
            "Linux",
            Some(4444),
        );
        assert_eq!(system_id, "system/netshoot");

        // Reading /proc/1/environ over that shell hands us kubelet's variables.
        campaign
            .get_system_entity_mut(&system_id)
            .expect("foothold system")
            .entity_mut()
            .system_mut()
            .env_vars
            .extend(kubelet_env());

        let facts = crate::rules::run_rules_fixpoint(
            &campaign,
            &crate::analyzers::default_rules(),
            crate::FactsUpdate::default(),
        );
        campaign.apply_facts(&facts);

        assert!(
            campaign
                .entities
                .contains::<Pod>(&EntityId::new("ns/?/pod/netshoot")),
            "fixture expects the foothold to be promoted to a pod"
        );
        campaign
    }

    fn pod_sessions(campaign: &Campaign) -> Vec<SessionInfo> {
        campaign
            .entities
            .find::<Pod>(&EntityId::new("ns/?/pod/netshoot"))
            .expect("promoted pod")
            .system
            .sessions
            .clone()
    }

    #[test]
    fn a_session_status_change_keyed_by_the_pre_merge_id_reaches_the_pod() {
        let mut campaign = promoted_foothold();

        update_session_status(
            &mut campaign,
            "node/netshoot",
            "session/s1",
            SessionStatus::Lost,
        );

        let sessions = pod_sessions(&campaign);
        assert_eq!(sessions.len(), 1, "sessions were: {:?}", sessions);
        assert_eq!(
            sessions[0].status,
            SessionStatus::Lost,
            "a status change addressed to the pre-merge id must reach the pod"
        );
        assert!(
            campaign.entities.get::<UnknownSystem>().is_empty(),
            "the promoted system must not be resurrected"
        );
    }

    #[test]
    fn a_reconnect_keyed_by_the_pre_merge_id_keeps_the_pod_executable() {
        let mut campaign = promoted_foothold();
        update_session_status(
            &mut campaign,
            "node/netshoot",
            "session/s1",
            SessionStatus::Lost,
        );

        // The shell comes back on the listener's own backend id, still
        // announcing itself with the target id the C2 captured on first connect.
        let channel_id = attach_connected_session(
            &mut campaign,
            "session/s1",
            "node/netshoot",
            "netshoot",
            "root",
            "Linux",
            Some(4444),
        );

        assert_eq!(
            channel_id, "ns/?/pod/netshoot",
            "the reconnect must attach to the pod, not to a fresh system"
        );
        assert!(
            campaign.entities.get::<UnknownSystem>().is_empty(),
            "the reconnect must not resurrect the pre-merge system"
        );

        let sessions = pod_sessions(&campaign);
        assert_eq!(
            sessions.len(),
            1,
            "the reconnect must revive the session, not duplicate it; got: {:?}",
            sessions
        );
        assert_eq!(sessions[0].status, SessionStatus::Active);

        let channel = campaign
            .resolve_exec_channel("ns/?/pod/netshoot")
            .expect("the promoted foothold must stay executable");
        assert_eq!(channel.backend_id, "session/s1");
    }

    #[test]
    fn a_connect_to_a_target_that_resolves_records_no_alias() {
        let mut campaign = Campaign::bootstrap("ran", K8sCluster::new("test-cluster"));
        campaign.insert_entity(&Pod::new("api", "default"));

        let channel_id = attach_connected_session(
            &mut campaign,
            "session/s2",
            "ns/default/pod/api",
            "api",
            "root",
            "Linux",
            None,
        );

        assert_eq!(channel_id, "ns/default/pod/api");
        assert!(
            campaign.entity_aliases.is_empty(),
            "a target that resolves on its own must not record an alias"
        );
    }
}
