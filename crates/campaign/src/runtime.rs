use std::sync::{Arc, RwLock};

use armory::Ttp;
use c2::{C2Event, C2EventBus};
use ran_domain::EntityId;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::Campaign;
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

                    let update = {
                        let mut campaign_guard = match campaign.write() {
                            Ok(guard) => guard,
                            Err(_) => {
                                error!("campaign lock poisoned while processing c2 event");
                                continue;
                            }
                        };

                        match campaign_guard.on_ttp_executed(&cmd, &event) {
                            Ok(update) => update,
                            Err(err) => {
                                error!("failed to process c2 ttp result: {:?}", err);
                                continue;
                            }
                        }
                    };

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

                    let _ = campaign_events.publish(CampaignEvent::FactsChanged {
                        cmd_id: cmd.id,
                        new_entities: update
                            .new_entities
                            .iter()
                            .map(|e| EntitySummary {
                                id: e.entity_id(),
                                kind: e.entity_kind().to_string(),
                                name: e.entity_name().to_string(),
                            })
                            .collect(),
                        new_relations: update
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
