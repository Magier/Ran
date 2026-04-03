use armory::{Armory, Procedure, Ttp};
use c2::{ExecTtp, TtpExecuted};
use ran_domain::{EntityId, RelationSummary};

use crate::effects::{ground_template, parse_effect_with_status};
use crate::failure_analyzers::{classify_failure, FAILURE_ANALYZER_EFFECT_ID};
use crate::output_parsers::{build_no_parser_audit, build_parse_audit, parse_output_effect};
use crate::rules::{default_rules as make_rules, run_rules_fixpoint};
use crate::{FactsUpdate, ParseResult};

use super::{
    Campaign, ExecuteActionError, ExecuteActionRequest, ExecuteActionResult, ExecutedActionEvent,
    TtpExecutionProcessing,
};

impl Campaign {
    pub fn prepare_action(
        &mut self,
        request: ExecuteActionRequest,
        armory: &Armory,
    ) -> Result<ExecTtp, ExecuteActionError> {
        if request.action_id.trim().is_empty() || request.target_id.trim().is_empty() {
            return Err(ExecuteActionError::InvalidInput(
                "actionId and targetId are required".to_string(),
            ));
        }

        let target_exists = self
            .get_entities()
            .into_iter()
            .any(|entity| entity.entity_id().0 == request.target_id);
        if !target_exists {
            return Err(ExecuteActionError::NotFound(format!(
                "failed to get target entity: {}",
                request.target_id
            )));
        }

        let mut ttp = armory
            .get_ttp(&request.action_id)
            .cloned()
            .ok_or_else(|| {
                ExecuteActionError::NotFound(format!(
                    "No TTP with ID '{}' found",
                    request.action_id
                ))
            })?;

        let mut args = request.args;
        for p in &ttp.params {
            if !args.contains_key(&p.name) && !p.default.is_empty() {
                args.insert(p.name.clone(), p.default.clone());
            }
        }

        let mut procedure = self.select_procedure(&ttp, request.procedure_id.as_deref())?;
        procedure.command = ground_template(&procedure.command, &args);

        for effect in &mut ttp.effects {
            *effect = ground_template(effect, &args);
        }

        Ok(ExecTtp {
            id: generate_cmd_id(),
            ttp,
            procedure,
            args,
            target_id: request.target_id,
            exec_system_id: request.exec_system_id.unwrap_or_default(),
        })
    }

    pub fn on_ttp_executed(
        &mut self,
        cmd: &ExecTtp,
        event: &TtpExecuted,
    ) -> Result<TtpExecutionProcessing, ExecuteActionError> {
        let mut updates = FactsUpdate::default();
        let mut parse_audits = Vec::new();

        if !event.success {
            let classified = classify_failure(cmd, event);
            parse_audits.push(build_parse_audit(
                FAILURE_ANALYZER_EFFECT_ID,
                cmd,
                event,
                classified.parse_result,
                &classified.detail,
                0,
            ));

            self.parse_audits.extend(parse_audits.clone());
            return Ok(TtpExecutionProcessing {
                updates,
                parse_audits,
            });
        }

        for effect in &cmd.ttp.effects {
            if let Some(parsed_output) = parse_output_effect(self, effect, cmd, event) {
                updates.merge(parsed_output.updates);
                parse_audits.push(parsed_output.audit);
                continue;
            }

            match parse_effect_with_status(effect, &cmd.args) {
                Ok(parsed_structural) if parsed_structural.handled => {
                    updates.merge(parsed_structural.updates);
                    parse_audits.push(build_parse_audit(
                        effect,
                        cmd,
                        event,
                        ParseResult::Parsed,
                        "parsed by structural effect handler",
                        0,
                    ));
                }
                Ok(_) => {
                    parse_audits.push(build_no_parser_audit(effect, cmd, event));
                }
                Err(err) => {
                    parse_audits.push(build_parse_audit(
                        effect,
                        cmd,
                        event,
                        ParseResult::ParserBug,
                        &err,
                        0,
                    ));
                }
            }
        }

        let rules = make_rules();
        updates = run_rules_fixpoint(self, &rules, updates);

        self.apply_facts(&updates);
        self.parse_audits.extend(parse_audits.clone());

        Ok(TtpExecutionProcessing {
            updates,
            parse_audits,
        })
    }

    pub fn execute_action(
        &mut self,
        request: ExecuteActionRequest,
        armory: &Armory,
    ) -> Result<ExecuteActionResult, ExecuteActionError> {
        let exec = self.prepare_action(request, armory)?;
        Ok(ExecuteActionResult {
            cmd_id: exec.id.clone(),
            event: ExecutedActionEvent {
                id: exec.id.clone(),
                cmd_id: exec.id,
                ttp: exec.ttp,
                args: exec.args,
                exec_system_id: exec.exec_system_id,
                success: true,
                fail_reason: String::new(),
            },
        })
    }

    fn apply_facts(&mut self, updates: &FactsUpdate) {
        for entity in &updates.new_entities {
            self.insert_entity(entity.as_ref());
        }

        for rel in &updates.new_relations {
            let summary = RelationSummary::from_relation(rel.as_ref());

            // Invariant: a pod can run on only one node at a time.
            // If another runs-on arrives, reconcile by preferring a concrete
            // node name over placeholders and rewrite stale node references.
            if summary.name == "runs-on" {
                self.apply_runs_on_with_invariant(summary);
                continue;
            }

            let exists = self.relations.iter().any(|r| {
                r.name == summary.name
                    && r.source_id == summary.source_id
                    && r.target_id == summary.target_id
            });
            if !exists {
                self.relations.push(summary);
            }
        }
    }

    fn apply_runs_on_with_invariant(&mut self, incoming: RelationSummary) {
        let existing_idx = self
            .relations
            .iter()
            .position(|r| r.name == "runs-on" && r.source_id == incoming.source_id);

        let Some(idx) = existing_idx else {
            self.relations.push(incoming);
            return;
        };

        let existing = self.relations[idx].clone();
        if existing.target_id == incoming.target_id {
            return;
        }

        let preferred = choose_preferred_node_id(&existing.target_id, &incoming.target_id);
        let stale = if preferred == existing.target_id {
            incoming.target_id
        } else {
            existing.target_id
        };

        self.relations[idx].target_id = preferred.clone();
        self.rewrite_relation_entity_id(&stale, &preferred);
        self.merge_node_entities(&preferred, &stale);
    }

    fn rewrite_relation_entity_id(&mut self, stale_id: &str, preferred_id: &str) {
        for rel in &mut self.relations {
            if rel.source_id == stale_id {
                rel.source_id = preferred_id.to_string();
            }
            if rel.target_id == stale_id {
                rel.target_id = preferred_id.to_string();
            }
        }

        // Deduplicate summaries that became identical after rewrite.
        let mut deduped = Vec::with_capacity(self.relations.len());
        for rel in self.relations.drain(..) {
            let exists = deduped.iter().any(|r: &RelationSummary| {
                r.name == rel.name && r.source_id == rel.source_id && r.target_id == rel.target_id
            });
            if !exists {
                deduped.push(rel);
            }
        }
        self.relations = deduped;
    }

    fn merge_node_entities(&mut self, preferred_id: &str, stale_id: &str) {
        if preferred_id == stale_id {
            return;
        }

        let preferred = EntityId::new(preferred_id);
        let stale = EntityId::new(stale_id);

        if !self.nodes.contains_key(&preferred) {
            if let Some(stale_node) = self.nodes.get(&stale).cloned() {
                self.nodes.insert(preferred.clone(), stale_node);
            }
        }

        self.nodes.remove(&stale);
    }

    fn select_procedure(
        &self,
        ttp: &Ttp,
        procedure_id: Option<&str>,
    ) -> Result<Procedure, ExecuteActionError> {
        if let Some(proc_id) = procedure_id {
            return ttp
                .procedures
                .iter()
                .find(|p| p.id == proc_id)
                .cloned()
                .ok_or_else(|| {
                    ExecuteActionError::InvalidInput(format!(
                        "procedure '{}' not found for action '{}'",
                        proc_id, ttp.id
                    ))
                });
        }

        ttp.procedures.first().cloned().ok_or_else(|| {
            ExecuteActionError::InvalidInput(format!("No procedure found for action '{}'", ttp.id))
        })
    }
}

fn choose_preferred_node_id(a: &str, b: &str) -> String {
    let a_unknown = is_placeholder_node_id(a);
    let b_unknown = is_placeholder_node_id(b);

    match (a_unknown, b_unknown) {
        (true, false) => b.to_string(),
        (false, true) => a.to_string(),
        _ => a.to_string(),
    }
}

fn is_placeholder_node_id(node_id: &str) -> bool {
    match node_id.strip_prefix("node/") {
        Some(name) => {
            let n = name.trim();
            n.is_empty() || n == "?" || n.eq_ignore_ascii_case("unknown")
        }
        None => node_id.trim().is_empty(),
    }
}

fn generate_cmd_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("cmd-{}", millis)
}
