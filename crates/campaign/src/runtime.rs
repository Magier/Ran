use std::sync::{Arc, RwLock};

use armory::Ttp;
use c2::{C2Event, C2EventBus};
use ran_domain::EntityId;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::external_parser::{ExternalParseRequest, ExternalParser};
use crate::{Campaign, ParseAudit, ParseResult};
use crate::output_parsers::build_parse_audit;
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
        ttp: Ttp,
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
    ) -> Result<usize, broadcast::error::SendError<CampaignEvent>> {
        self.tx.send(event)
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
                                    match guard.apply_system_update(
                                        &cmd.target_id,
                                        &response.system,
                                    ) {
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
                        ttp: cmd.ttp,
                        args: cmd.args,
                        success: event.success,
                        fail_reason: event.fail_reason,
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
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(skipped, "campaign c2 event processor lagged behind c2 event bus");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("c2 event bus closed; stopping campaign c2 event processor");
                    break;
                }
            }
        }
    })
}
