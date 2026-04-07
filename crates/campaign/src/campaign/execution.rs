use armory::{Armory, Procedure, Ttp};
use c2::{ExecTtp, TtpExecuted};
use ran_domain::{BinaryPresence, EntityId};

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

        // Pre-resolve the lateral movement exec source so that ${SRC} is
        // available when the effect strings are grounded below.  Lateral
        // Movement TTPs run FROM a compromised pod, so the source pod entity
        // ID is the value that ${SRC}, used in effects like
        // `rce.can-exec(${SRC}, ${TARGET_ID})`, should resolve to.
        let pre_resolved_src: Option<super::ExecChannel> =
            if request.exec_system_id.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true)
                && is_lateral_movement_tactic(&ttp.tactic)
            {
                let ch = self
                    .resolve_exec_source()
                    .map_err(ExecuteActionError::NoExecChannel)?;
                if let Some(ref src_id) = ch.exec_target_id {
                    args.insert("SRC".to_string(), src_id.clone());
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

        let (exec_system_id, resolved_target_id) = match request.exec_system_id {
            Some(id) if !id.trim().is_empty() => (id, request.target_id),
            _ if is_lateral_movement_tactic(&ttp.tactic) => {
                // Lateral Movement TTPs run FROM an already-compromised pod and
                // CREATE the exec edge to the target — they must not require a
                // pre-existing channel to the victim.  The source was resolved
                // above so ${SRC} is grounded in effects; use the same channel
                // here to target the source pod for kubectl exec.
                let ch = pre_resolved_src
                    .expect("lateral movement exec source resolved above");
                let exec_target = ch.exec_target_id.unwrap_or(request.target_id);
                (ch.backend_id, exec_target)
            }
            _ if needs_remote_channel(&procedure, &ttp.tactic) => {
                let ch = self
                    .resolve_exec_channel(&request.target_id)
                    .map_err(ExecuteActionError::NoExecChannel)?;
                let exec_target = ch.exec_target_id.clone().unwrap_or(request.target_id.clone());

                if ch.hops.is_empty() {
                    // Direct path: C2 can reach the target without any hop.
                    // Ground the procedure binary against the target pod's binary
                    // map so non-standard install paths (e.g. /tmp/kubectl) are
                    // used correctly.
                    let tgt_id = EntityId::new(exec_target.as_str());
                    if let Some(pod) = self.pods.get(&tgt_id) {
                        procedure.command = ground_binary_in_cmd(&procedure.command, &pod.system.binaries);
                    }
                    (ch.backend_id, exec_target)
                } else {
                    // Multi-hop: build nested command wrappers, one per hop.
                    //
                    // hops[0] is what BuiltinC2 execs into; the inner hops and
                    // the final exec_target are wrapped inside the procedure
                    // command from innermost (exec_target) outward (hops[1]).
                    //
                    // For each consecutive pair in the full chain
                    //   [hops[0], hops[1], ..., exec_target]
                    // we look up the exec-channel relation between them and
                    // delegate wrapping to RelationSummary::wrap_command, which
                    // applies the envelope for rce.can-exec hops and kubectl exec
                    // for pod-exec hops — no per-relation-type special-casing here.
                    let full_chain: Vec<&str> = ch.hops.iter().map(String::as_str)
                        .chain(std::iter::once(exec_target.as_str()))
                        .collect();

                    // Wrap from innermost (last pair) to outermost (second pair;
                    // hops[0] is handled by BuiltinC2 itself).
                    for i in (1..full_chain.len()).rev() {
                        let src = full_chain[i - 1];
                        let tgt = full_chain[i];

                        // Ground the inner command's binary against the target
                        // system's known paths before embedding it in the envelope.
                        // This mirrors Go's buildFinalCommand groundUsedTool call
                        // and ensures tools at non-standard locations (e.g.
                        // /tmp/kubectl) are referenced correctly inside RCE templates.
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
                                // Fallback: try kubectl exec via target entity ID
                                if let Some((ns, name)) = split_pod_entity_id(tgt) {
                                    format!("kubectl exec -n {} {} -- {}", ns, name, procedure.command)
                                } else {
                                    procedure.command.clone()
                                }
                            }
                        };

                        // After wrapping, ground the outer tool (first word of the
                        // wrapped command) against the source pod's binary map.
                        // The wrapped command runs inside src, so paths on src apply
                        // (e.g. redis-cli at /usr/bin/redis-cli on the pivot pod).
                        let src_id = EntityId::new(src);
                        if let Some(pod) = self.pods.get(&src_id) {
                            procedure.command = ground_binary_in_cmd(&procedure.command, &pod.system.binaries);
                        }
                    }
                    (ch.backend_id, ch.hops[0].clone())
                }
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

    /// Merge a stale (placeholder) pod into the preferred (real-named) pod.
    ///
    /// Runtime data accumulated on the stale entity — IPs, access level, env
    /// vars, binaries, processes — is copied into the preferred entity.  Fields
    /// that are already set on the preferred entity are preserved as-is.  The
    /// stale entity is removed from the campaign regardless.
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
            // IPs: add any that the preferred entity doesn't already have.
            for ip in &stale_pod.system.ips {
                if !preferred_pod.system.ips.contains(ip) {
                    preferred_pod.system.ips.push(*ip);
                }
            }
            // Access level: take the higher of the two.
            if stale_pod.system.access_level > preferred_pod.system.access_level {
                preferred_pod.system.access_level = stale_pod.system.access_level;
            }
            // Env vars / binaries / files: add missing entries from the stale pod.
            for (k, v) in stale_pod.system.env_vars {
                preferred_pod.system.env_vars.entry(k).or_insert(v);
            }
            for (k, v) in stale_pod.system.binaries {
                preferred_pod.system.binaries.entry(k).or_insert(v);
            }
            for f in stale_pod.system.files {
                if !preferred_pod.system.files.contains(&f) {
                    preferred_pod.system.files.push(f);
                }
            }
            // Processes: add any not already tracked by pid.
            for proc in stale_pod.system.processes {
                if !preferred_pod.system.processes.iter().any(|p| p.pid == proc.pid) {
                    preferred_pod.system.processes.push(proc);
                }
            }
            // Fill in Kubernetes metadata absent from the preferred entity.
            if preferred_pod.node_name.is_none() {
                preferred_pod.node_name = stale_pod.node_name;
            }
            if preferred_pod.service_account_name.is_none() {
                preferred_pod.service_account_name = stale_pod.service_account_name;
            }
            if preferred_pod.meta.uid.is_none() {
                preferred_pod.meta.uid = stale_pod.meta.uid;
            }
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
    matches!(
        tactic.trim().to_ascii_lowercase().as_str(),
        "lateral movement" | "lateral-movement"
    )
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