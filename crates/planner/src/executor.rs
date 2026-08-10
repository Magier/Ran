use crate::{
    error::PlanError,
    model::{Dependency, PlanDefinition, Require, RetryStrategy, StepExpectation},
    resolver::resolve_target,
    state::{PlanExecutionState, StepStatus},
};
#[allow(unused_imports)]
use armory::Armory;
use campaign::{Campaign, ExecuteActionRequest};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct PlanDispatch {
    pub step_id: String,
    pub request: ExecuteActionRequest,
}

#[derive(Debug, Clone)]
pub enum PlanEvent {
    StepDispatched { step_id: String, exec_count: usize },
    StepCompleted { step_id: String, success: bool },
    StepSkipped { step_id: String, reason: String },
    StepFailed { step_id: String, reason: String },
    PlanComplete,
}

pub struct PlanExecutor {
    plan: PlanDefinition,
    state: PlanExecutionState,
    retry_attempts: HashMap<String, usize>,
}

impl PlanExecutor {
    pub fn new(plan: PlanDefinition) -> Result<Self, PlanError> {
        validate_plan(&plan)?;
        let step_ids = plan.steps.iter().map(|s| s.id.clone()).collect();
        let state = PlanExecutionState::new(step_ids);
        Ok(Self {
            plan,
            state,
            retry_attempts: HashMap::new(),
        })
    }

    pub fn state(&self) -> &PlanExecutionState {
        &self.state
    }

    pub fn is_complete(&self) -> bool {
        self.state.is_complete()
    }

    /// True when a step has been dispatched and its outcome is still pending.
    pub fn has_in_flight(&self) -> bool {
        self.state.has_in_flight()
    }

    /// Called by the API layer after dispatching the requests returned by tick().
    /// Replaces placeholder cmd_ids with the real ones from C2.
    pub fn record_dispatched(&mut self, step_id: &str, real_cmd_ids: Vec<String>) {
        self.state.mark_dispatched(step_id, real_cmd_ids);
    }

    /// Testable inner tick — takes resolved entity IDs and a graph predicate function.
    pub fn tick_inner(
        &mut self,
        entity_ids: &[String],
        graph_check: impl Fn(&str, &str) -> bool,
    ) -> Vec<PlanDispatch> {
        let mut dispatches = Vec::new();

        for step in self.plan.steps.clone().iter() {
            match self.state.get(&step.id) {
                Some(StepStatus::Pending) | Some(StepStatus::PendingRetry { .. }) => {}
                _ => continue,
            }

            if !self.all_deps_satisfied(&step.id, entity_ids, &graph_check) {
                continue;
            }

            let targets = resolve_target(&step.target, entity_ids);
            if targets.is_empty() {
                continue;
            }

            let exec_system_id = if let Some(exec_q) = &step.exec_target {
                let exec_matches = resolve_target(exec_q, entity_ids);
                if exec_matches.is_empty() {
                    // Execution source is explicit and not yet resolvable.
                    continue;
                }
                exec_matches.into_iter().next()
            } else {
                None
            };

            let token_target_id = if let Some(token_q) = &step.token {
                let token_matches = resolve_target(token_q, entity_ids);
                if token_matches.is_empty() {
                    // Token source is explicit and not yet resolvable.
                    continue;
                }
                token_matches.into_iter().next()
            } else {
                None
            };

            self.state.set_targets(&step.id, targets.clone());

            let procedure = match self.state.get(&step.id) {
                Some(StepStatus::PendingRetry { next_procedure, .. }) => next_procedure.clone(),
                _ => step.procedure.clone(),
            };

            let cmd_ids_placeholder: Vec<String> = targets
                .iter()
                .map(|t| format!("pending-{}-{}", step.id, t))
                .collect();
            self.state.mark_dispatched(&step.id, cmd_ids_placeholder);

            for target_id in targets {
                let mut args = step.args.clone();
                if let Some(token_id) = &token_target_id {
                    args.entry("TOKEN".to_string())
                        .or_insert_with(|| token_id.clone());
                }
                dispatches.push(PlanDispatch {
                    step_id: step.id.clone(),
                    request: ExecuteActionRequest {
                        action_id: step.action.clone(),
                        exec_system_id: exec_system_id.clone(),
                        auth_identity_id: token_target_id.clone(),
                        target_id,
                        procedure_id: procedure.clone(),
                        args,
                        reasoning: step
                            .note
                            .clone()
                            .or_else(|| Some(format!("plan step '{}'", step.id))),
                    },
                });
            }
        }

        dispatches
    }

    /// Public tick — takes a Campaign reference. Call this from the API layer.
    pub fn tick(&mut self, campaign: &Campaign) -> Vec<PlanDispatch> {
        let entity_ids = campaign.all_entity_ids();
        self.tick_inner(&entity_ids, |eid, rel| {
            campaign.entity_has_relation(eid, rel)
        })
    }

    fn all_deps_satisfied(
        &self,
        step_id: &str,
        _entity_ids: &[String],
        graph_check: &impl Fn(&str, &str) -> bool,
    ) -> bool {
        let step = self.plan.steps.iter().find(|s| s.id == step_id).unwrap();

        for dep in &step.depends_on {
            match dep {
                Dependency::Step {
                    step: dep_id,
                    require,
                } => match self.state.get(dep_id) {
                    Some(StepStatus::Completed { outcomes }) => {
                        let ok = match require {
                            Require::Completion => true,
                            Require::Success | Require::AnySuccess => outcomes.iter().any(|&o| o),
                            Require::AllSuccess => outcomes.iter().all(|&o| o),
                        };
                        if !ok {
                            return false;
                        }
                    }
                    Some(StepStatus::Skipped { .. }) | Some(StepStatus::Failed { .. }) => {
                        if !matches!(require, Require::Completion) {
                            return false;
                        }
                    }
                    _ => return false,
                },
                Dependency::Graph {
                    step_ref,
                    relation,
                    all,
                } => {
                    let targets = self.state.targets_for(step_ref);
                    if targets.is_empty() {
                        return false;
                    }
                    let satisfied = if *all {
                        targets.iter().all(|t| graph_check(t, relation))
                    } else {
                        targets.iter().any(|t| graph_check(t, relation))
                    };
                    if !satisfied {
                        return false;
                    }
                }
            }
        }
        true
    }
}

impl PlanExecutor {
    /// Inner handler for a completed execution.
    /// Returns plan events to publish. Call tick() afterward to dispatch newly-unblocked steps.
    pub fn on_ttp_executed_inner(
        &mut self,
        cmd_id: &str,
        success: bool,
        _procedure_id: Option<&str>,
        armory: Option<&Armory>,
    ) -> Vec<PlanEvent> {
        let mut events = Vec::new();

        let step_id = match self.state.step_for_cmd(cmd_id) {
            Some(id) => id.to_string(),
            None => return events,
        };

        // Record outcome; if the caller already recorded it the step may already
        // be Completed — in that case use the existing status directly.
        let completed: Option<StepStatus> = match self.state.record_outcome(cmd_id, success) {
            Some(status) => Some(status),
            None => {
                // Outcome was pre-recorded (e.g. in tests); grab current status.
                match self.state.get(&step_id) {
                    Some(StepStatus::Completed { .. }) => self.state.get(&step_id).cloned(),
                    _ => None,
                }
            }
        };

        if let Some(StepStatus::Completed { ref outcomes }) = completed {
            let overall_success = outcomes.iter().any(|&o| o);
            const SAME_PROCEDURE_MAX_RETRIES: usize = 1;

            let step = self
                .plan
                .steps
                .iter()
                .find(|s| s.id == step_id)
                .unwrap()
                .clone();
            if !overall_success && step.retry == RetryStrategy::NextProcedure {
                if let Some(armory) = armory {
                    let attempt = {
                        let a = self.retry_attempts.entry(step_id.clone()).or_insert(0);
                        *a += 1;
                        *a
                    };
                    let next = self.retry_procedure_id_with_armory(&step.action, attempt, armory);
                    if next.is_some() {
                        self.state
                            .mark_pending_retry(&step_id, attempt, next.clone());
                        return events; // tick() will handle re-dispatch
                    }
                    self.state.mark_failed(&step_id, "all procedures exhausted");
                    events.push(PlanEvent::StepFailed {
                        step_id: step_id.clone(),
                        reason: "all procedures exhausted".into(),
                    });
                } else {
                    events.push(PlanEvent::StepCompleted {
                        step_id: step_id.clone(),
                        success: false,
                    });
                }
            } else if !overall_success && step.retry == RetryStrategy::SameProcedure {
                let attempt = {
                    let a = self.retry_attempts.entry(step_id.clone()).or_insert(0);
                    *a += 1;
                    *a
                };

                if attempt <= SAME_PROCEDURE_MAX_RETRIES {
                    self.state
                        .mark_pending_retry(&step_id, attempt, step.procedure.clone());
                    return events; // tick() will handle re-dispatch
                }

                self.state
                    .mark_failed(&step_id, "same-procedure retries exhausted");
                events.push(PlanEvent::StepFailed {
                    step_id: step_id.clone(),
                    reason: "same-procedure retries exhausted".into(),
                });
            } else if overall_success {
                events.push(PlanEvent::StepCompleted {
                    step_id: step_id.clone(),
                    success: true,
                });
            } else {
                events.push(PlanEvent::StepCompleted {
                    step_id: step_id.clone(),
                    success: false,
                });
            }

            events.extend(self.propagate_skips());

            if self.state.is_complete() {
                events.push(PlanEvent::PlanComplete);
            }
        }

        events
    }

    /// Public on_ttp_executed — call from the API layer with the Armory for retry support.
    pub fn on_ttp_executed(
        &mut self,
        cmd_id: &str,
        success: bool,
        procedure_id: Option<&str>,
        armory: &Armory,
    ) -> Vec<PlanEvent> {
        self.on_ttp_executed_inner(cmd_id, success, procedure_id, Some(armory))
    }

    /// Return the step id associated with a command id, if any.
    pub fn step_for_cmd(&self, cmd_id: &str) -> Option<&str> {
        self.state.step_for_cmd(cmd_id)
    }

    /// Return expectation config for the step associated with `cmd_id`.
    pub fn expectation_for_cmd(&self, cmd_id: &str) -> Option<StepExpectation> {
        let step_id = self.state.step_for_cmd(cmd_id)?;
        self.plan
            .steps
            .iter()
            .find(|s| s.id == step_id)
            .and_then(|s| s.expect.clone())
    }

    /// Force every still-pending step to a terminal state so a stalled run can
    /// finish instead of hanging forever. Call this when the run loop observes a
    /// stall: nothing in flight, yet the plan isn't complete (e.g. a step's target
    /// never appeared, or a graph predecessor relation never materialised).
    ///
    /// A pending step whose dependencies *are* satisfied could only be stuck
    /// because its target didn't resolve → `target not found`; otherwise it's
    /// blocked on an unmet dependency → `dependencies unmet`. Skips then propagate.
    pub fn fail_stalled_inner(
        &mut self,
        entity_ids: &[String],
        graph_check: impl Fn(&str, &str) -> bool,
    ) -> Vec<PlanEvent> {
        let mut events = Vec::new();
        let step_ids: Vec<String> = self.plan.steps.iter().map(|s| s.id.clone()).collect();

        let is_pending = |state: &PlanExecutionState, id: &str| {
            matches!(
                state.get(id),
                Some(StepStatus::Pending) | Some(StepStatus::PendingRetry { .. })
            )
        };

        loop {
            // Let dependents of already-failed/skipped hard deps become Skipped
            // first, so they aren't mislabelled as failures below.
            events.extend(self.propagate_skips());

            // Fail steps whose deps are satisfied yet still pending: at a stall
            // that can only mean the target never resolved (a dispatchable step
            // would already be Dispatched, not Pending).
            let mut progressed = false;
            for step_id in &step_ids {
                if is_pending(&self.state, step_id)
                    && self.all_deps_satisfied(step_id, entity_ids, &graph_check)
                {
                    let reason = "target not found: no matching entity (stalled)";
                    self.state.mark_failed(step_id, reason);
                    events.push(PlanEvent::StepFailed {
                        step_id: step_id.clone(),
                        reason: reason.to_string(),
                    });
                    progressed = true;
                }
            }
            if progressed {
                continue; // re-propagate skips, re-evaluate
            }

            // Anything still pending is blocked on a dependency that will never be
            // satisfied (e.g. a graph predicate that never held).
            let remaining: Vec<String> = step_ids
                .iter()
                .filter(|id| is_pending(&self.state, id))
                .cloned()
                .collect();
            if remaining.is_empty() {
                break;
            }
            for step_id in remaining {
                let reason = "dependencies unmet (stalled)";
                self.state.mark_failed(&step_id, reason);
                events.push(PlanEvent::StepFailed {
                    step_id,
                    reason: reason.to_string(),
                });
            }
        }

        if self.state.is_complete() {
            events.push(PlanEvent::PlanComplete);
        }
        events
    }

    /// Public fail_stalled — call from the API layer with a Campaign reference.
    pub fn fail_stalled(&mut self, campaign: &Campaign) -> Vec<PlanEvent> {
        let entity_ids = campaign.all_entity_ids();
        self.fail_stalled_inner(&entity_ids, |eid, rel| {
            campaign.entity_has_relation(eid, rel)
        })
    }

    fn propagate_skips(&mut self) -> Vec<PlanEvent> {
        let mut events = Vec::new();
        let mut changed = true;
        while changed {
            changed = false;
            let step_ids: Vec<String> = self.plan.steps.iter().map(|s| s.id.clone()).collect();
            for step_id in &step_ids {
                if !matches!(self.state.get(step_id), Some(StepStatus::Pending)) {
                    continue;
                }
                if self.should_skip(step_id) {
                    self.state
                        .mark_skipped(step_id, "hard dependency failed or skipped");
                    events.push(PlanEvent::StepSkipped {
                        step_id: step_id.clone(),
                        reason: "hard dependency failed or skipped".into(),
                    });
                    changed = true;
                }
            }
        }
        events
    }

    fn should_skip(&self, step_id: &str) -> bool {
        let step = self.plan.steps.iter().find(|s| s.id == step_id).unwrap();
        for dep in &step.depends_on {
            if let Dependency::Step {
                step: dep_id,
                require,
            } = dep
            {
                if matches!(require, Require::Completion) {
                    continue;
                }
                match self.state.get(dep_id) {
                    Some(StepStatus::Failed { .. }) | Some(StepStatus::Skipped { .. }) => {
                        return true;
                    }
                    Some(StepStatus::Completed { outcomes }) if !outcomes.iter().any(|&o| o) => {
                        return true;
                    }
                    _ => {}
                }
            }
        }
        false
    }

    fn retry_procedure_id_with_armory(
        &self,
        action_id: &str,
        attempt: usize,
        armory: &Armory,
    ) -> Option<String> {
        let ttp = armory.get_ttp(action_id)?;
        ttp.procedures.get(attempt).map(|p| p.id.clone())
    }
}

fn validate_plan(plan: &PlanDefinition) -> Result<(), PlanError> {
    let step_ids: HashSet<&str> = plan.steps.iter().map(|s| s.id.as_str()).collect();

    for step in &plan.steps {
        for dep in &step.depends_on {
            match dep {
                Dependency::Step { step: ref_id, .. } => {
                    if !step_ids.contains(ref_id.as_str()) {
                        return Err(PlanError::UnknownStepRef(ref_id.clone()));
                    }
                }
                Dependency::Graph { step_ref, .. } => {
                    if !step_ids.contains(step_ref.as_str()) {
                        return Err(PlanError::UnknownStepRef(step_ref.clone()));
                    }
                }
            }
        }
    }

    // Cycle detection via DFS
    detect_cycles(plan).map_err(PlanError::CircularDependency)?;

    Ok(())
}

fn detect_cycles(plan: &PlanDefinition) -> Result<(), String> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for step in &plan.steps {
        let deps: Vec<&str> = step
            .depends_on
            .iter()
            .filter_map(|d| match d {
                Dependency::Step { step, .. } => Some(step.as_str()),
                _ => None,
            })
            .collect();
        adj.entry(step.id.as_str()).or_default().extend(deps);
    }

    let mut visited = HashSet::new();
    let mut in_stack = HashSet::new();

    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        visited: &mut HashSet<&'a str>,
        in_stack: &mut HashSet<&'a str>,
    ) -> Option<&'a str> {
        if in_stack.contains(node) {
            return Some(node);
        }
        if visited.contains(node) {
            return None;
        }
        visited.insert(node);
        in_stack.insert(node);
        for &dep in adj.get(node).map(|v| v.as_slice()).unwrap_or(&[]) {
            if let Some(cycle_node) = dfs(dep, adj, visited, in_stack) {
                return Some(cycle_node);
            }
        }
        in_stack.remove(node);
        None
    }

    for step in &plan.steps {
        if let Some(node) = dfs(step.id.as_str(), &adj, &mut visited, &mut in_stack) {
            return Err(node.to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use std::collections::HashMap;

    fn make_step(id: &str, deps: Vec<Dependency>) -> StepDefinition {
        StepDefinition {
            id: id.into(),
            action: "k8s.exec".into(),
            target: TargetQuery {
                kind: "Pod".into(),
                namespace: Some("default".into()),
                name: "nginx-.*".into(),
                select: None,
                ..Default::default()
            },
            exec_target: None,
            token: None,
            args: HashMap::new(),
            procedure: None,
            retry: RetryStrategy::None,
            depends_on: deps,
            expect: None,
            note: None,
        }
    }

    fn make_plan(steps: Vec<StepDefinition>) -> PlanDefinition {
        PlanDefinition {
            id: "test".into(),
            name: "Test".into(),
            description: None,
            version: "1.0".into(),
            steps,
        }
    }

    #[test]
    fn valid_plan_creates_executor() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step(
                "b",
                vec![Dependency::Step {
                    step: "a".into(),
                    require: Require::Success,
                }],
            ),
        ]);
        assert!(PlanExecutor::new(plan).is_ok());
    }

    #[test]
    fn unknown_step_ref_fails_validation() {
        let plan = make_plan(vec![make_step(
            "a",
            vec![Dependency::Step {
                step: "nonexistent".into(),
                require: Require::Success,
            }],
        )]);
        assert!(matches!(
            PlanExecutor::new(plan),
            Err(PlanError::UnknownStepRef(_))
        ));
    }

    #[test]
    fn circular_dependency_fails_validation() {
        let plan = make_plan(vec![
            make_step(
                "a",
                vec![Dependency::Step {
                    step: "b".into(),
                    require: Require::Success,
                }],
            ),
            make_step(
                "b",
                vec![Dependency::Step {
                    step: "a".into(),
                    require: Require::Success,
                }],
            ),
        ]);
        assert!(matches!(
            PlanExecutor::new(plan),
            Err(PlanError::CircularDependency(_))
        ));
    }

    #[test]
    fn graph_dep_with_unknown_step_ref_fails_validation() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step(
                "b",
                vec![Dependency::Graph {
                    step_ref: "nonexistent".into(),
                    relation: "rce.can-exec".into(),
                    all: false,
                }],
            ),
        ]);
        assert!(matches!(
            PlanExecutor::new(plan),
            Err(PlanError::UnknownStepRef(_))
        ));
    }

    fn entity_ids() -> Vec<String> {
        vec!["ns/default/pod/nginx-abc123".to_string()]
    }

    fn no_relations(_: &str, _: &str) -> bool {
        false
    }
    fn has_rce(entity_id: &str, relation: &str) -> bool {
        entity_id.contains("nginx") && relation == "rce.can-exec"
    }

    #[test]
    fn steps_with_no_deps_are_dispatched_on_first_tick() {
        let plan = make_plan(vec![make_step("a", vec![]), make_step("b", vec![])]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let dispatches = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(dispatches.len(), 2);
        let ids: Vec<_> = dispatches.iter().map(|d| d.step_id.as_str()).collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
    }

    #[test]
    fn step_with_dep_stays_pending_until_predecessor_done() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step(
                "b",
                vec![Dependency::Step {
                    step: "a".into(),
                    require: Require::Success,
                }],
            ),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let dispatches = exec.tick_inner(&entity_ids(), no_relations);
        // Only "a" dispatched
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].step_id, "a");
    }

    #[test]
    fn step_dispatched_after_predecessor_succeeds() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step(
                "b",
                vec![Dependency::Step {
                    step: "a".into(),
                    require: Require::Success,
                }],
            ),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let d1 = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(d1.len(), 1);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", true);
        let d2 = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(d2.len(), 1);
        assert_eq!(d2[0].step_id, "b");
    }

    #[test]
    fn soft_dep_unblocks_on_any_outcome() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step(
                "b",
                vec![Dependency::Step {
                    step: "a".into(),
                    require: Require::Completion,
                }],
            ),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", false); // failed
        let d2 = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(d2.len(), 1);
        assert_eq!(d2[0].step_id, "b");
    }

    #[test]
    fn graph_predicate_blocks_until_satisfied() {
        let dep = Dependency::Graph {
            step_ref: "a".into(),
            relation: "rce.can-exec".into(),
            all: false,
        };
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step(
                "b",
                vec![
                    Dependency::Step {
                        step: "a".into(),
                        require: Require::Success,
                    },
                    dep,
                ],
            ),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.set_targets("a", entity_ids());
        exec.state.record_outcome("cmd-1", true);
        // Without the relation, b stays pending
        let d2 = exec.tick_inner(&entity_ids(), no_relations);
        assert!(d2.is_empty());
        // With the relation, b dispatches
        let d3 = exec.tick_inner(&entity_ids(), has_rce);
        assert_eq!(d3.len(), 1);
        assert_eq!(d3[0].step_id, "b");
    }

    #[test]
    fn step_stays_pending_when_no_matching_entities() {
        let plan = make_plan(vec![make_step("a", vec![])]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let d = exec.tick_inner(&[], no_relations);
        assert!(d.is_empty());
    }

    #[test]
    fn hard_dep_on_failed_step_skips_dependent() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step(
                "b",
                vec![Dependency::Step {
                    step: "a".into(),
                    require: Require::Success,
                }],
            ),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", false);

        let events = exec.on_ttp_executed_inner("cmd-1", false, None, None);
        let skipped: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, PlanEvent::StepSkipped { .. }))
            .collect();
        assert_eq!(skipped.len(), 1);
        assert!(matches!(&skipped[0], PlanEvent::StepSkipped { step_id, .. } if step_id == "b"));
    }

    #[test]
    fn skip_propagates_transitively() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step(
                "b",
                vec![Dependency::Step {
                    step: "a".into(),
                    require: Require::Success,
                }],
            ),
            make_step(
                "c",
                vec![Dependency::Step {
                    step: "b".into(),
                    require: Require::Success,
                }],
            ),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", false);

        let events = exec.on_ttp_executed_inner("cmd-1", false, None, None);
        let skipped: Vec<_> = events
            .iter()
            .filter_map(|e| {
                if let PlanEvent::StepSkipped { step_id, .. } = e {
                    Some(step_id.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(skipped.contains(&"b"));
        assert!(skipped.contains(&"c"));
    }

    #[test]
    fn fan_out_select_all_dispatches_multiple() {
        let mut step = make_step("a", vec![]);
        step.target.select = Some(SelectStrategy::All);
        let plan = make_plan(vec![step]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let all_ids = vec![
            "ns/default/pod/nginx-aaa".to_string(),
            "ns/default/pod/nginx-bbb".to_string(),
        ];
        let dispatches = exec.tick_inner(&all_ids, no_relations);
        assert_eq!(dispatches.len(), 2);
    }

    #[test]
    fn plan_complete_event_emitted_when_all_terminal() {
        let plan = make_plan(vec![make_step("a", vec![])]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        exec.state.record_outcome("cmd-1", true);
        let events = exec.on_ttp_executed_inner("cmd-1", true, None, None);
        assert!(events.iter().any(|e| matches!(e, PlanEvent::PlanComplete)));
    }

    #[test]
    fn has_in_flight_tracks_dispatched_steps() {
        let plan = make_plan(vec![make_step("a", vec![])]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        assert!(!exec.has_in_flight());
        exec.tick_inner(&entity_ids(), no_relations);
        exec.state.mark_dispatched("a", vec!["cmd-1".into()]);
        assert!(exec.has_in_flight());
        exec.state.record_outcome("cmd-1", true);
        assert!(!exec.has_in_flight());
    }

    #[test]
    fn fail_stalled_terminates_unresolvable_step() {
        // A step whose target never resolves (no matching entities) stays pending
        // forever; fail_stalled must drive it terminal so the plan can complete.
        let plan = make_plan(vec![make_step("a", vec![])]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        exec.tick_inner(&[], no_relations); // no entities → nothing dispatched
        assert!(!exec.is_complete());

        let events = exec.fail_stalled_inner(&[], no_relations);
        assert!(exec.is_complete());
        assert!(events
            .iter()
            .any(|e| matches!(e, PlanEvent::StepFailed { step_id, .. } if step_id == "a")));
        assert!(events.iter().any(|e| matches!(e, PlanEvent::PlanComplete)));
    }

    #[test]
    fn fail_stalled_skips_dependent_of_unmet_dep() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step(
                "b",
                vec![Dependency::Step {
                    step: "a".into(),
                    require: Require::Success,
                }],
            ),
        ]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        // No entities: "a" can't resolve, "b" is blocked on "a".
        exec.tick_inner(&[], no_relations);
        let events = exec.fail_stalled_inner(&[], no_relations);
        assert!(exec.is_complete());
        // a → failed (target not found), b → skipped (hard dep failed).
        assert!(events
            .iter()
            .any(|e| matches!(e, PlanEvent::StepFailed { step_id, .. } if step_id == "a")));
        assert!(events
            .iter()
            .any(|e| matches!(e, PlanEvent::StepSkipped { step_id, .. } if step_id == "b")));
    }

    #[test]
    fn record_dispatched_replaces_placeholder_cmd_ids() {
        let plan = make_plan(vec![make_step("a", vec![])]);
        let mut exec = PlanExecutor::new(plan).unwrap();
        let dispatches = exec.tick_inner(&entity_ids(), no_relations);
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].step_id, "a");

        // Simulate: API dispatched and got real cmd_id back
        exec.record_dispatched("a", vec!["real-cmd-001".into()]);
        assert_eq!(exec.state().step_for_cmd("real-cmd-001"), Some("a"));
        assert_eq!(
            exec.state()
                .step_for_cmd("pending-a-ns/default/pod/nginx-abc123"),
            None
        );
    }
}
