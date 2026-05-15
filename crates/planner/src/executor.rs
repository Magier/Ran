use std::collections::{HashMap, HashSet};
use campaign::{Campaign, ExecuteActionRequest};
#[allow(unused_imports)]
use armory::Armory;
use crate::{
    error::PlanError,
    model::{Dependency, PlanDefinition, Require},
    resolver::resolve_target,
    state::{PlanExecutionState, StepStatus},
};

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
                dispatches.push(PlanDispatch {
                    step_id: step.id.clone(),
                    request: ExecuteActionRequest {
                        action_id: step.action.clone(),
                        exec_system_id: None,
                        target_id,
                        procedure_id: procedure.clone(),
                        args: step.args.clone(),
                    },
                });
            }
        }

        dispatches
    }

    /// Public tick — takes a Campaign reference. Call this from the API layer.
    pub fn tick(&mut self, campaign: &Campaign) -> Vec<PlanDispatch> {
        let entity_ids = campaign.all_entity_ids();
        self.tick_inner(&entity_ids, |eid, rel| campaign.entity_has_relation(eid, rel))
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
                Dependency::Step { step: dep_id, require } => {
                    match self.state.get(dep_id) {
                        Some(StepStatus::Completed { outcomes }) => {
                            let ok = match require {
                                Require::Completion => true,
                                Require::Success | Require::AnySuccess => outcomes.iter().any(|&o| o),
                                Require::AllSuccess => outcomes.iter().all(|&o| o),
                            };
                            if !ok { return false; }
                        }
                        Some(StepStatus::Skipped { .. }) | Some(StepStatus::Failed { .. }) => {
                            if !matches!(require, Require::Completion) {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
                Dependency::Graph { step_ref, relation, all } => {
                    let targets = self.state.targets_for(step_ref);
                    if targets.is_empty() { return false; }
                    let satisfied = if *all {
                        targets.iter().all(|t| graph_check(t, relation))
                    } else {
                        targets.iter().any(|t| graph_check(t, relation))
                    };
                    if !satisfied { return false; }
                }
            }
        }
        true
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
        let deps: Vec<&str> = step.depends_on.iter().filter_map(|d| match d {
            Dependency::Step { step, .. } => Some(step.as_str()),
            _ => None,
        }).collect();
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
        if in_stack.contains(node) { return Some(node); }
        if visited.contains(node) { return None; }
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
            },
            args: HashMap::new(),
            procedure: None,
            retry: RetryStrategy::None,
            depends_on: deps,
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
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
        ]);
        assert!(PlanExecutor::new(plan).is_ok());
    }

    #[test]
    fn unknown_step_ref_fails_validation() {
        let plan = make_plan(vec![
            make_step("a", vec![Dependency::Step { step: "nonexistent".into(), require: Require::Success }]),
        ]);
        assert!(matches!(PlanExecutor::new(plan), Err(PlanError::UnknownStepRef(_))));
    }

    #[test]
    fn circular_dependency_fails_validation() {
        let plan = make_plan(vec![
            make_step("a", vec![Dependency::Step { step: "b".into(), require: Require::Success }]),
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
        ]);
        assert!(matches!(PlanExecutor::new(plan), Err(PlanError::CircularDependency(_))));
    }

    #[test]
    fn graph_dep_with_unknown_step_ref_fails_validation() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![Dependency::Graph {
                step_ref: "nonexistent".into(),
                relation: "rce.can-exec".into(),
                all: false,
            }]),
        ]);
        assert!(matches!(PlanExecutor::new(plan), Err(PlanError::UnknownStepRef(_))));
    }

    fn entity_ids() -> Vec<String> {
        vec!["ns/default/pod/nginx-abc123".to_string()]
    }

    fn no_relations(_: &str, _: &str) -> bool { false }
    fn has_rce(entity_id: &str, relation: &str) -> bool {
        entity_id.contains("nginx") && relation == "rce.can-exec"
    }

    #[test]
    fn steps_with_no_deps_are_dispatched_on_first_tick() {
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![]),
        ]);
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
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
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
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Success }]),
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
            make_step("b", vec![Dependency::Step { step: "a".into(), require: Require::Completion }]),
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
        let dep = Dependency::Graph { step_ref: "a".into(), relation: "rce.can-exec".into(), all: false };
        let plan = make_plan(vec![
            make_step("a", vec![]),
            make_step("b", vec![
                Dependency::Step { step: "a".into(), require: Require::Success },
                dep,
            ]),
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
}
