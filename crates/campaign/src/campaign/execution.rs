use armory::{Armory, Procedure, Ttp};
use c2::{ExecTtp, TtpExecuted, BUILTIN_C2_ID};
use ran_domain::{BinaryPresence, EntityId, Merge};

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

        // Treat `exec_system_id == target_id` as implicit/unspecified. The UI
        // often sends this value by default, but for remote targets we still
        // need full channel resolution and hop wrapping.
        let normalized_exec_system_id: Option<String> = request
            .exec_system_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != request.target_id.as_str())
            .map(str::to_string);

        // Pre-resolve the lateral movement exec source so that ${SRC}/${src}
        // is available when the effect strings are grounded below.  Lateral
        // Movement TTPs run FROM a compromised pod, so the source pod entity
        // ID is the value that these vars, used in effects like
        // `rce.can-exec(${SRC}, ${TARGET_ID})`, should resolve to.
        let preselected_lateral_src = if is_lateral_movement_tactic(&ttp.tactic) {
            normalized_exec_system_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|s| {
                    if self.get_system_entity(s).is_some() {
                        Some(s.to_string())
                    } else {
                        None
                    }
                })
        } else {
            None
        };

        if let Some(src_id) = &preselected_lateral_src {
            args.insert("SRC".to_string(), src_id.clone());
            args.insert("src".to_string(), src_id.clone());
        }

        let pre_resolved_src: Option<super::ExecChannel> =
            if normalized_exec_system_id
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
                && is_lateral_movement_tactic(&ttp.tactic)
            {
                let ch = self
                    .resolve_exec_source()
                    .map_err(ExecuteActionError::NoExecChannel)?;
                if let Some(ref src_id) = ch.exec_target_id {
                    args.insert("SRC".to_string(), src_id.clone());
                    args.insert("src".to_string(), src_id.clone());
                }
                Some(ch)
            } else {
                None
            };

        // Inject TARGET_ID — the canonical graph entity ID of the target — so
        // that effect strings like `rce.can-exec(${SRC}, ${TARGET_ID})` record
        // the relation with the correct ID even when ${TARGET} holds an IP.
        args.entry("TARGET_ID".to_string())
            .or_insert_with(|| request.target_id.clone());

        let mut procedure = self.select_procedure(&ttp, request.procedure_id.as_deref())?;

        // Compute the wrapping envelope before fully grounding the command.
        // Ground every arg *except* CMD so that the ${CMD} placeholder is
        // preserved as the injection slot for future commands routed over this
        // hop (e.g. rce.can-exec).  Any effect handler that needs it reads
        // PROCEDURE_CMD from the args context.
        {
            let envelope_args: std::collections::HashMap<_, _> = args
                .iter()
                .filter(|(k, _)| k.to_uppercase() != "CMD")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let envelope = ground_template(&procedure.command, &envelope_args);
            args.entry("PROCEDURE_CMD".to_string()).or_insert(envelope);
        }

        procedure.command = ground_template(&procedure.command, &args);

        for effect in &mut ttp.effects {
            *effect = ground_template(effect, &args);
        }

        // Warn about any variables that were not resolved.
        for var in detect_ungrounded_vars(&procedure.command) {
            tracing::warn!(var, "ungrounded variable remaining in command after grounding");
        }

        tracing::debug!(
            "exec_system_id before backend selection: '{}'",
            normalized_exec_system_id.as_deref().unwrap_or("")
        );

        let (exec_system_id, resolved_target_id) = self.resolve_c2_channel(
            request.action_id.as_str(),
            request.target_id.as_str(),
            &ttp,
            &mut procedure,
            normalized_exec_system_id.as_deref(),
            pre_resolved_src,
        )?;

        tracing::debug!("final grounded command: '{}'", procedure.command);

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

    fn resolve_c2_channel(
        &self,
        action_id: &str,
        target_id: &str,
        ttp: &Ttp,
        procedure: &mut Procedure,
        normalized_exec_system_id: Option<&str>,
        pre_resolved_src: Option<super::ExecChannel>,
    ) -> Result<(String, String), ExecuteActionError> {
        match normalized_exec_system_id {
            Some(id) if !id.trim().is_empty() => {
                // API callers may pass an entity ID (where the command should run)
                // or a backend ID. If this matches a known system entity, treat it
                // as an execution source and route through the builtin backend.
                if self.get_system_entity(id).is_some() {
                    tracing::info!(
                        logical_target = %target_id,
                        selected_source = %id,
                        backend_id = %BUILTIN_C2_ID,
                        chain = %format_exec_chain(BUILTIN_C2_ID, &[], id),
                        "using caller-supplied exec source entity"
                    );
                    Ok((BUILTIN_C2_ID.to_string(), id.to_string()))
                } else {
                    tracing::info!(
                        target_id = %target_id,
                        backend_id = %id,
                        chain = %format_exec_chain(id, &[], target_id),
                        "using caller-supplied exec backend"
                    );
                    Ok((id.to_string(), target_id.to_string()))
                }
            }
            _ if is_lateral_movement_tactic(&ttp.tactic) => {
                // Lateral Movement TTPs run FROM an already-compromised pod and
                // CREATE the exec edge to the target — they must not require a
                // pre-existing channel to the victim.  The source was resolved
                // above so ${SRC} is grounded in effects; use the same channel
                // here to target the source pod for kubectl exec.
                let ch = pre_resolved_src
                    .ok_or_else(|| ExecuteActionError::InvariantViolation(
                        "lateral movement exec source should have been resolved before \
                         reaching resolve_c2_channel".to_string(),
                    ))?;
                let exec_target = ch.exec_target_id.unwrap_or(target_id.to_string());

                tracing::warn!("lateral movement tactic detected; targeting exec to source entity {} via channel with backend {}",
                    exec_target, ch.backend_id);
                tracing::info!(
                    target_id = %target_id,
                    selected_source = %exec_target,
                    backend_id = %ch.backend_id,
                    chain = %format_exec_chain(ch.backend_id.as_str(), &ch.hops, exec_target.as_str()),
                    "selected lateral-movement execution chain"
                );

                Ok((ch.backend_id, exec_target))
            }
            _ if needs_remote_channel(procedure, &ttp.tactic) => {
                let ch = self
                    .resolve_exec_channel(target_id)
                    .map_err(ExecuteActionError::NoExecChannel)?;
                let exec_target = ch.exec_target_id.clone().unwrap_or(target_id.to_string());

                tracing::warn!(
                    target_id = %target_id,
                    exec_target = %exec_target,
                    "resolved exec channel for action target",
                );
                tracing::info!("channel backend: {}, hops: {:?}", ch.backend_id, ch.hops);
                tracing::info!(
                    target_id = %target_id,
                    backend_id = %ch.backend_id,
                    chain = %format_exec_chain(ch.backend_id.as_str(), &ch.hops, exec_target.as_str()),
                    "selected remote execution chain"
                );

                if ch.hops.is_empty() {
                    // Direct path: C2 can reach the target without any hop.
                    // Ground the procedure binary against the target pod's binary
                    // map so non-standard install paths (e.g. /tmp/kubectl) are
                    // used correctly.
                    let tgt_id = EntityId::new(exec_target.as_str());
                    if let Some(pod) = self.pods.get(&tgt_id) {
                        procedure.command = ground_binary_in_cmd(&procedure.command, &pod.system.binaries);
                    }
                    Ok((ch.backend_id, exec_target))
                } else {
                    self.wrap_command_for_hops(procedure, &ch.hops, exec_target.as_str());
                    Ok((ch.backend_id, ch.hops[0].clone()))
                }
            }
            _ => {
                // Safety fallback: if the logical target is a pod and no explicit
                // exec system was provided, prefer executing FROM an in-cluster
                // foothold rather than directly from Ran.
                let target_eid = EntityId::new(target_id);
                if self.pods.contains_key(&target_eid) {
                    tracing::warn!(
                        target_id = %target_id,
                        action_id = %action_id,
                        tactic = %ttp.tactic,
                        "no explicit exec channel selected for pod target; falling back to in-cluster source"
                    );
                    let ch = self
                        .resolve_exec_source()
                        .map_err(ExecuteActionError::NoExecChannel)?;
                    let exec_target = ch.exec_target_id.unwrap_or(target_id.to_string());
                    tracing::info!(
                        target_id = %target_id,
                        selected_source = %exec_target,
                        backend_id = %ch.backend_id,
                        chain = %format_exec_chain(ch.backend_id.as_str(), &ch.hops, exec_target.as_str()),
                        "pod fallback source selected"
                    );
                    Ok((ch.backend_id, exec_target))
                } else {
                    Ok((String::new(), target_id.to_string()))
                }
            }
        }
    }

    /// Wrap `procedure.command` through each hop in reverse order so BuiltinC2
    /// can exec into the first hop and the nested command traverses the rest of
    /// the chain to the final execution target.
    fn wrap_command_for_hops(
        &self,
        procedure: &mut Procedure,
        hops: &[String],
        exec_target: &str,
    ) {
        let full_chain: Vec<&str> = hops
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(exec_target))
            .collect();

        // Wrap from innermost (last pair) to outermost (second pair;
        // hops[0] is handled by BuiltinC2 itself).
        for i in (1..full_chain.len()).rev() {
            let src = full_chain[i - 1];
            let tgt = full_chain[i];

            // Ground the inner command's binary against the target system's
            // known paths before embedding it in the envelope.
            let tgt_id = EntityId::new(tgt);
            if let Some(pod) = self.pods.get(&tgt_id) {
                procedure.command = ground_binary_in_cmd(&procedure.command, &pod.system.binaries);
            }

            let src_eid = EntityId::new(src);
            let tgt_eid_inner = EntityId::new(tgt);
            let found = self
                .graph
                .outgoing(&src_eid)
                .into_iter()
                .find(|(t, d)| *t == &tgt_eid_inner && d.is_exec_channel)
                .map(|(t, d)| ran_domain::RelationSummary {
                    name: d.relation_name.clone(),
                    source_id: src.to_string(),
                    target_id: t.0.clone(),
                    is_exec_channel: true,
                    envelope: d.envelope.clone(),
                    weight: d.weight,
                });
            procedure.command = match found {
                Some(ref rel) => rel.wrap_command(&procedure.command),
                None => {
                    // Fallback: try kubectl exec via target entity ID.
                    if let Some((ns, name)) = split_pod_entity_id(tgt) {
                        format!("kubectl exec -n {} {} -- {}", ns, name, procedure.command)
                    } else {
                        procedure.command.clone()
                    }
                }
            };

            // After wrapping, ground the outer tool (first word of the wrapped
            // command) against the source pod's binary map.
            let src_id = EntityId::new(src);
            if let Some(pod) = self.pods.get(&src_id) {
                procedure.command = ground_binary_in_cmd(&procedure.command, &pod.system.binaries);
            }
        }
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
                    // args["TARGET_ID"] = the original request target, which is
                    // the actual execution target even in multi-hop chains (where
                    // cmd.target_id is the first hop, not the final destination).
                    let target_id_arg = cmd.args.get("TARGET_ID").map(String::as_str).unwrap_or("");
                    let system_id = if self.get_system_entity(target_id_arg).is_some() {
                        Some(target_id_arg)
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
            self.complete_open_step(&cmd.id);
            return Ok(TtpExecutionProcessing {
                updates,
                parse_audits,
                effective_success: false,
                effective_fail_reason: event.fail_reason.clone(),
            });
        }

        // Build the effect-parsing context: start with the TTP args and add
        // PROCEDURE_CMD so relation-effect handlers (e.g. rce.can-exec) that
        // need the executed command template can read it without special-casing.
        let mut effect_ctx = cmd.args.clone();
        effect_ctx
            .entry("PROCEDURE_CMD".to_string())
            .or_insert_with(|| cmd.procedure.command.clone());

        for effect in &cmd.ttp.effects {
            if let Some(parsed_output) = parse_output_effect(self, effect, cmd, event) {
                updates.merge(parsed_output.updates);
                parse_audits.push(parsed_output.audit);
                continue;
            }

            match parse_effect_with_status(effect, &effect_ctx) {
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

        // Detect when a TTP ran against an IP-placeholder pod and the output
        // revealed the real pod identity (e.g. via a service-account token).
        // Record the alias so apply_facts can transplant all relations.
        self.detect_pod_identity_merge(cmd, &mut updates);

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

        // If a parser detected a semantic failure inside an otherwise-successful
        // transport response (e.g. Kubernetes API 403 Forbidden returned as HTTP
        // 200 with a Status body), override the recorded success flag so the
        // audit log and /api/flow reflect the real outcome.
        let api_error = parse_audits.iter().find(|a| {
            matches!(a.parse_result, ParseResult::KnownFailure)
                && a.detail.starts_with("K8s API error ")
        });
        let (effective_success, effective_fail_reason) = if let Some(err_audit) = api_error {
            let mut record = ExecutionRecord::from_execution(cmd, event);
            record.success = false;
            record.fail_reason = err_audit.detail.clone();
            self.execution_records.push(record);
            (false, err_audit.detail.clone())
        } else {
            self.execution_records.push(ExecutionRecord::from_execution(cmd, event));
            (event.success, event.fail_reason.clone())
        };
        self.complete_open_step(&cmd.id);

        Ok(TtpExecutionProcessing {
            updates,
            parse_audits,
            effective_success,
            effective_fail_reason,
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

        // Merge entity aliases: transplant graph edges and entity data.
        // Runs after insert_entity so the preferred node already exists.
        for (stale_id, preferred_id) in &updates.entity_aliases {
            // Graph: retarget all edges from stale → preferred.
            self.graph.merge_entities(preferred_id, stale_id);
            // Entity maps: merge runtime data (IPs, access level, etc.).
            self.merge_pod_entities(&preferred_id.0, &stale_id.0);
        }

        for rel in &updates.new_relations {
            // Resolve stale entity IDs before inserting into the graph.
            let (src, tgt) = updates.entity_aliases.iter().fold(
                (rel.source_id().clone(), rel.target_id().clone()),
                |(src, tgt), (stale, preferred)| {
                    let src = if src == *stale { preferred.clone() } else { src };
                    let tgt = if tgt == *stale { preferred.clone() } else { tgt };
                    (src, tgt)
                },
            );

            // runs-on: when the new node differs, pick the preferred one and
            // merge the stale node entity into it.
            if rel.relation_name() == "runs-on" {
                let existing_node = self
                    .graph
                    .targets_of(&src, "runs-on")
                    .first()
                    .cloned()
                    .cloned();

                if let Some(old_node) = existing_node {
                    if old_node != tgt {
                        let preferred_node =
                            EntityId::new(choose_preferred_node_id(&old_node.0, &tgt.0));
                        let stale_node = if preferred_node == old_node {
                            tgt.clone()
                        } else {
                            old_node
                        };
                        self.graph.merge_entities(&preferred_node, &stale_node);
                        self.merge_node_entities(&preferred_node.0, &stale_node.0);
                        // Insert edge to preferred node (graph PodSingleNode
                        // invariant removes the old runs-on automatically).
                        self.insert_relation_with_ids(&src, &preferred_node, rel.as_ref());
                        continue;
                    }
                    // Same node — nothing to do (PodSingleNode invariant will
                    // replace the edge anyway, but we skip the insert).
                    continue;
                }
            }

            // Common path: no alias resolution changed the IDs — use the
            // public `insert_relation` so it gets a live production call site.
            if src == *rel.source_id() && tgt == *rel.target_id() {
                self.insert_relation(rel.as_ref());
            } else {
                self.insert_relation_with_ids(&src, &tgt, rel.as_ref());
            }
        }
    }

    /// Insert a relation into the graph using explicit (possibly alias-resolved)
    /// source and target IDs rather than the relation's own stored IDs.
    pub(super) fn insert_relation_with_ids(
        &mut self,
        src: &EntityId,
        tgt: &EntityId,
        rel: &dyn ran_domain::Relation,
    ) {
        use cortex::edge_data_for;
        use ran_domain::RceCanExec;
        let envelope = rel
            .as_any()
            .downcast_ref::<RceCanExec>()
            .and_then(|r| r.envelope.clone());
        let data = edge_data_for(rel.relation_name(), envelope);
        self.graph.insert_edge(src, tgt, data);

        // When a C2 channel relation is added to a pod, ensure access_level
        // reflects at least UserExec so the field stays consistent with the
        if rel.is_exec_channel() {
            if let Some(pod) = self.pods.get_mut(tgt) {
                if pod.system.access_level == ran_domain::AccessLevel::None {
                    pod.system.access_level = ran_domain::AccessLevel::Exec;
                }
            }
        }
    }

    fn merge_node_entities(&mut self, preferred_id: &str, stale_id: &str) {
        if preferred_id == stale_id {
            return;
        }

        let preferred = EntityId::new(preferred_id);
        let stale = EntityId::new(stale_id);

        let Some(stale_node) = self.nodes.remove(&stale) else {
            return;
        };

        if let Some(preferred_node) = self.nodes.get_mut(&preferred) {
            preferred_node.merge_from(&stale_node);
        } else {
            self.nodes.insert(preferred, stale_node);
        }
    }

    /// Merge a stale (placeholder) pod into the preferred (real-named) pod.
    ///
    /// All facts accumulated on the stale entity are folded into the preferred
    /// one via `Pod::merge_from`.  The stale entity is removed regardless.
    fn merge_pod_entities(&mut self, preferred_id: &str, stale_id: &str) {
        if preferred_id == stale_id {
            return;
        }

        let preferred = EntityId::new(preferred_id);
        let stale = EntityId::new(stale_id);

        let Some(stale_pod) = self.pods.remove(&stale) else {
            return;
        };

        if let Some(preferred_pod) = self.pods.get_mut(&preferred) {
            preferred_pod.merge_from(&stale_pod);
        } else {
            // Preferred entity not yet in the campaign (shouldn't happen in the
            // normal flow, but handle gracefully by keeping the stale data).
            self.pods.insert(preferred, stale_pod);
        }
    }

    /// Detect when a TTP ran against an IP-placeholder pod and the output
    /// revealed the real pod identity (e.g. from a service-account token).
    ///
    /// An "IP-placeholder" pod is one whose name was derived from the pod's IP
    /// address during a network scan (e.g. `backend-service.10-244-1-4` from
    /// reverse DNS).  When a subsequent TTP on that pod produces a
    /// `ServiceAccount` entity whose token carries the real pod name, the
    /// `ServiceAccountTokenAnalyzer` emits a properly-named Pod entity (and/or
    /// a `uses` relation from it).  This function detects that situation and
    /// records an alias `(stale_id, preferred_id)` in `updates` so that
    /// `apply_facts` can transplant all relations to the real entity.
    fn detect_pod_identity_merge(&self, cmd: &ExecTtp, updates: &mut FactsUpdate) {
        // For multi-hop TTPs the C2 sets `cmd.target_id` to the first hop (the
        // pod it kubectl-execs into), NOT the logical target of the TTP.  The
        // original request target is always preserved in args["TARGET_ID"].
        let logical_target = cmd
            .args
            .get("TARGET_ID")
            .map(String::as_str)
            .unwrap_or(&cmd.target_id);

        let stale_id = EntityId::new(logical_target);

        // Only proceed when the execution target is a known IP-derived pod.
        let Some(exec_pod) = self.pods.get(&stale_id) else {
            return;
        };
        if !is_ip_derived_pod_name(&exec_pod.meta.name) {
            return;
        }

        let ns = exec_pod.meta.namespace.as_deref().unwrap_or("");
        if ns.is_empty() {
            return;
        }
        let ns_pod_prefix = format!("ns/{}/pod/", ns);

        // Strategy 1: a new Pod entity with a real name appeared in updates.
        let preferred_from_entity = updates
            .new_entities
            .iter()
            .find(|e| {
                if e.entity_kind() != "Pod" {
                    return false;
                }
                let id = e.entity_id();
                if id == stale_id {
                    return false;
                }
                id.0.starts_with(&ns_pod_prefix)
                    && !is_ip_derived_pod_name(
                        id.0.strip_prefix(&ns_pod_prefix).unwrap_or(""),
                    )
            })
            .map(|e| e.entity_id());

        // Strategy 2: a `uses` relation from a real pod appeared in updates
        // (SA token analysis won't re-emit the pod entity if already known).
        let preferred_from_relation = if preferred_from_entity.is_none() {
            updates
                .new_relations
                .iter()
                .filter(|r| r.relation_name() == "uses")
                .map(|r| r.source_id().clone())
                .find(|id| {
                    *id != stale_id
                        && id.0.starts_with(&ns_pod_prefix)
                        && !is_ip_derived_pod_name(
                            id.0.strip_prefix(&ns_pod_prefix).unwrap_or(""),
                        )
                })
        } else {
            None
        };

        let Some(preferred_id) = preferred_from_entity.or(preferred_from_relation) else {
            return;
        };

        tracing::info!(
            stale = %stale_id.0,
            preferred = %preferred_id.0,
            "merging IP-placeholder pod with discovered real pod identity"
        );
        updates.entity_aliases.push((stale_id, preferred_id));
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

/// Returns `true` when a pod name was derived from its IP address during a
/// network scan.
///
/// The rDNS parser produces names like `backend-service.10-244-1-4` (from
/// `10-244-1-4.backend-service.dev.svc.cluster.local`) or bare `10-244-1-4`
/// (when there is no service component).  In both cases the last dot-separated
/// segment is an IPv4 address in kebab notation — exactly four numeric parts
/// separated by hyphens.
pub(crate) fn is_ip_derived_pod_name(name: &str) -> bool {
    let last = name.rsplit('.').next().unwrap_or(name);
    let parts: Vec<&str> = last.split('-').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
}

/// Parse a pod entity ID in the canonical form `ns/<namespace>/pod/<name>` and
/// return `(namespace, pod_name)`, or `None` if the format doesn't match.
/// Used to build the inner `kubectl exec` when routing via an intermediate pod.
fn split_pod_entity_id(entity_id: &str) -> Option<(&str, &str)> {
    let mut parts = entity_id.splitn(5, '/');
    let kind_a = parts.next()?;
    let namespace = parts.next()?;
    let kind_b = parts.next()?;
    let pod_name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if kind_a != "ns" || kind_b != "pod" || namespace.is_empty() || pod_name.is_empty() {
        return None;
    }
    Some((namespace, pod_name))
}

/// Returns `true` when the tactic creates a new execution edge rather than
/// requiring one to exist.  For these tactics the command is run FROM an
/// already-compromised source, not TO the target.
fn is_lateral_movement_tactic(tactic: &str) -> bool {
    normalize_tactic(tactic) == "lateral movement"
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
    !matches!(normalize_tactic(tactic).as_str(), "reconnaissance" | "resource development")
}

fn normalize_tactic(tactic: &str) -> String {
    tactic
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_exec_chain(backend_id: &str, hops: &[String], exec_target: &str) -> String {
    let mut parts: Vec<String> = vec![backend_id.to_string()];
    parts.extend(hops.iter().cloned());
    if parts.last().map(|p| p.as_str()) != Some(exec_target) {
        parts.push(exec_target.to_string());
    }
    parts.join(" -> ")
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

/// Resolve the first word of `cmd` using a system's binary map.
///
/// If the first word of `cmd` is a known binary with a `Present` path that
/// differs from the bare name (e.g. `kubectl` → `/tmp/kubectl`), the first
/// occurrence of the bare name is replaced with the resolved path.
///
/// Words that already contain `/` are skipped — they are already absolute
/// paths and do not need further resolution.
///
/// Mirrors Go's `groundUsedTool` in `campaign/campaign.go`.
fn ground_binary_in_cmd(
    cmd: &str,
    binaries: &std::collections::HashMap<String, ran_domain::BinaryPresence>,
) -> String {
    use ran_domain::BinaryPresence;

    let first_word = match cmd.split_whitespace().next() {
        Some(w) => w,
        None => return cmd.to_string(),
    };

    // Already an absolute/relative path — nothing to resolve.
    if first_word.contains('/') {
        return cmd.to_string();
    }

    if let Some(BinaryPresence::Present(path)) = binaries.get(first_word) {
        if !path.is_empty() && path.as_str() != first_word {
            // Replace the first occurrence of `first_word` in `cmd`, which is
            // guaranteed to be at a word boundary since it is the first token.
            if let Some(pos) = cmd.find(first_word) {
                let mut result = cmd.to_string();
                result.replace_range(pos..pos + first_word.len(), path.as_str());
                return result;
            }
        }
    }

    cmd.to_string()
}