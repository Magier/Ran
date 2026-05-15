use std::collections::{HashMap, HashSet};
use campaign::ExecuteActionRequest;
#[allow(unused_imports)]
use armory::Armory;
use crate::{
    error::PlanError,
    model::{Dependency, PlanDefinition},
    state::PlanExecutionState,
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
}
