use armory::{Armory, Procedure, Ttp};
use c2::{ExecTtp, TtpExecuted};
use ran_domain::{BinaryPresence, EntityId, RelationSummary};

use crate::effects::{ground_template, parse_effect_with_status};
use crate::external_parser::SystemFieldUpdates;
use crate::failure_analyzers::{classify_failure, CommandNotFoundFailureAnalyzer, FailureAnalyzer, FAILURE_ANALYZER_EFFECT_ID};
use crate::grounding::{detect_ungrounded_vars, ground_args_from_context};
use crate::output_parsers::{build_no_parser_audit, build_parse_audit, parse_output_effect};
use crate::rules::{default_rules as make_rules, run_rules_fixpoint};
use crate::{FactsUpdate, ParseResult};

use crate::execution_record::ExecutionRecord;

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

        // Resolve context-aware variables (NS, POD_NAME, NODE, RANDOM) from
        // the target entity before template substitution so that cross-param
        // references like `${NS}` in other arg defaults resolve correctly.
        ground_args_from_context(&mut args, &request.target_id, self);

        let mut procedure = self.select_procedure(&ttp, request.procedure_id.as_deref())?;
        procedure.command = ground_template(&procedure.command, &args);

        for effect in &mut ttp.effects {
            *effect = ground_template(effect, &args);
        }

        // Warn about any variables that were not resolved.
        for var in detect_ungrounded_vars(&procedure.command) {
            tracing::warn!(var, "ungrounded variable remaining in command after grounding");
        }

        let (exec_system_id, resolved_target_id) = match request.exec_system_id {
            Some(id) if !id.trim().is_empty() => (id, request.target_id),
            _ if needs_remote_channel(&procedure, &ttp.tactic) => {
                let ch = self
                    .resolve_exec_channel(&request.target_id)
                    .map_err(ExecuteActionError::NoExecChannel)?;
                let exec_target = ch.exec_target_id.unwrap_or(request.target_id);
                (ch.backend_id, exec_target)
            }
            _ => (String::new(), request.target_id),
        };

        Ok(ExecTtp {
            id: generate_cmd_id(),
            ttp,
            procedure,
            args,
            target_id: resolved_target_id,
            exec_system_id,
            started_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
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

            // If the binary was not found, record it as Absent in the system's
            // binary map so the procedure selector can automatically fall back to
            // an alternative procedure next time.
            if CommandNotFoundFailureAnalyzer.analyze(cmd, event).is_some() {
                if let Some(binary) = procedure_binary_name(&cmd.procedure) {
                    let system_id = if self.get_system_entity(&cmd.exec_system_id).is_some() {
                        Some(cmd.exec_system_id.as_str())
                    } else if self.get_system_entity(&cmd.target_id).is_some() {
                        Some(cmd.target_id.as_str())
                    } else {
                        None
                    };
                    if let Some(id) = system_id {
                        // Empty path → BinaryPresence::Absent; only written when
                        // currently Unknown (apply_system_update's existing guard).
                        let absent_update = SystemFieldUpdates {
                            binaries: std::collections::HashMap::from([
                                (binary.to_string(), String::new()),
                            ]),
                            ..Default::default()
                        };
                        let _ = self.apply_system_update(id, &absent_update);
                    }
                }
            }

            self.parse_audits.extend(parse_audits.clone());
            self.execution_records.push(ExecutionRecord::from_execution(cmd, event));
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

        // Infer binary presence from the tool used in this procedure.
        // Only records if currently Unknown — preserves more precise paths set by
        // sys.has-binary(${OUTPUT}) or from a real parser.
        if let Some(tool) = procedure_tool(&cmd.procedure) {
            let system_id = if self.get_system_entity(&cmd.exec_system_id).is_some() {
                Some(cmd.exec_system_id.as_str())
            } else if self.get_system_entity(&cmd.target_id).is_some() {
                Some(cmd.target_id.as_str())
            } else {
                None
            };

            if let Some(id) = system_id {
                let already_known = self
                    .get_system_entity(id)
                    .map(|e| e.entity().system().has_binary(tool) != BinaryPresence::Unknown)
                    .unwrap_or(false);

                if !already_known {
                    let binary_updates = SystemFieldUpdates {
                        binaries: std::collections::HashMap::from([
                            (tool.to_string(), tool.to_string()),
                        ]),
                        ..Default::default()
                    };
                    let _ = self.apply_system_update(id, &binary_updates);
                }
            }
        }

        self.apply_facts(&updates);
        self.parse_audits.extend(parse_audits.clone());
        self.execution_records.push(ExecutionRecord::from_execution(cmd, event));

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

/// Returns `true` when the procedure requires a remote execution channel.
///
/// Local commands (`is_local_command = true`) and operator-side tactics
/// (Reconnaissance, Resource Development) run on the C2 side and do not
/// need a channel to a target system.
fn needs_remote_channel(procedure: &Procedure, tactic: &str) -> bool {
    if procedure.is_local_command == Some(true) {
        return false;
    }
    !matches!(
        tactic.trim().to_ascii_lowercase().as_str(),
        "reconnaissance" | "resource development" | "resource-development"
    )
}

fn generate_cmd_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    format!("cmd-{}", millis)
}

/// Return the tool name for a procedure, if one is set and non-empty.
/// Matches Go's `Procedure.GetTool()`.
fn procedure_tool(procedure: &Procedure) -> Option<&str> {
    procedure
        .tool
        .as_deref()
        .filter(|t| !t.trim().is_empty())
}

/// Return the name of the binary a procedure invokes, for use when recording
/// binary presence/absence.
///
/// Resolution order:
/// 1. `procedure.tool` — explicit annotation (e.g. `tool: cat`)
/// 2. `procedure.id` — when it is a single bare word (e.g. key `nmap`, `curl`)
/// 3. First word of `procedure.command` — final fallback
fn procedure_binary_name(procedure: &Procedure) -> Option<&str> {
    if let Some(tool) = procedure_tool(procedure) {
        return Some(tool);
    }

    // Use the procedure ID only when it looks like a bare binary name
    // (no spaces, no path separators).
    let id = procedure.id.trim();
    if !id.is_empty() && !id.contains(' ') && !id.contains('/') {
        return Some(id);
    }

    // Fall back to the first word of the command.
    procedure.command.split_whitespace().next()
}
