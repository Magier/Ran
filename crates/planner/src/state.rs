use indexmap::IndexMap;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum StepStatus {
    Pending,
    Dispatched { exec_ids: Vec<String> },
    PendingRetry { attempt: usize, next_procedure: Option<String> },
    Completed { outcomes: Vec<bool> },
    Failed { reason: String },
    Skipped { reason: String },
}

impl StepStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. } | Self::Skipped { .. })
    }

    pub fn succeeded(&self) -> Option<bool> {
        match self {
            Self::Completed { outcomes } => Some(outcomes.iter().any(|&o| o)),
            _ => None,
        }
    }
}

pub struct PlanExecutionState {
    steps: IndexMap<String, StepStatus>,
    cmd_to_step: HashMap<String, String>,
    step_targets: HashMap<String, Vec<String>>,
    pending_outcomes: HashMap<String, Vec<Option<bool>>>,
}

impl PlanExecutionState {
    pub fn new(step_ids: Vec<String>) -> Self {
        let steps = step_ids.into_iter().map(|id| (id, StepStatus::Pending)).collect();
        Self {
            steps,
            cmd_to_step: HashMap::new(),
            step_targets: HashMap::new(),
            pending_outcomes: HashMap::new(),
        }
    }

    pub fn get(&self, step_id: &str) -> Option<&StepStatus> {
        self.steps.get(step_id)
    }

    pub fn set(&mut self, step_id: &str, status: StepStatus) {
        if let Some(s) = self.steps.get_mut(step_id) {
            *s = status;
        }
    }

    pub fn mark_dispatched(&mut self, step_id: &str, cmd_ids: Vec<String>) {
        self.cmd_to_step.retain(|_, sid| sid != step_id);
        let n = cmd_ids.len();
        for cmd_id in &cmd_ids {
            self.cmd_to_step.insert(cmd_id.clone(), step_id.to_string());
        }
        self.pending_outcomes.insert(step_id.to_string(), vec![None; n]);
        self.set(step_id, StepStatus::Dispatched { exec_ids: cmd_ids });
    }

    pub fn step_for_cmd(&self, cmd_id: &str) -> Option<&str> {
        self.cmd_to_step.get(cmd_id).map(String::as_str)
    }

    /// Record outcome of one cmd_id. Returns final StepStatus when all cmds for
    /// this step have completed, None if still waiting.
    pub fn record_outcome(&mut self, cmd_id: &str, success: bool) -> Option<StepStatus> {
        let step_id = self.cmd_to_step.get(cmd_id)?.clone();
        let outcomes = self.pending_outcomes.get_mut(&step_id)?;

        let idx = match self.steps.get(&step_id) {
            Some(StepStatus::Dispatched { exec_ids }) => {
                exec_ids.iter().position(|id| id == cmd_id)?
            }
            _ => return None,
        };
        outcomes[idx] = Some(success);

        if outcomes.iter().all(|o| o.is_some()) {
            let final_outcomes: Vec<bool> = outcomes.iter().map(|o| o.unwrap()).collect();
            let status = StepStatus::Completed { outcomes: final_outcomes };
            self.set(&step_id, status.clone());
            self.pending_outcomes.remove(&step_id);
            Some(status)
        } else {
            None
        }
    }

    pub fn mark_skipped(&mut self, step_id: &str, reason: &str) {
        self.set(step_id, StepStatus::Skipped { reason: reason.to_string() });
    }

    pub fn mark_failed(&mut self, step_id: &str, reason: &str) {
        self.set(step_id, StepStatus::Failed { reason: reason.to_string() });
    }

    pub fn mark_pending_retry(&mut self, step_id: &str, attempt: usize, next_procedure: Option<String>) {
        self.set(step_id, StepStatus::PendingRetry { attempt, next_procedure });
        self.pending_outcomes.remove(step_id);
    }

    pub fn set_targets(&mut self, step_id: &str, targets: Vec<String>) {
        self.step_targets.insert(step_id.to_string(), targets);
    }

    pub fn targets_for(&self, step_id: &str) -> &[String] {
        self.step_targets.get(step_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn is_complete(&self) -> bool {
        self.steps.values().all(|s| s.is_terminal())
    }

    pub fn pending_steps(&self) -> impl Iterator<Item = &str> {
        self.steps.iter().filter_map(|(id, s)| {
            matches!(s, StepStatus::Pending | StepStatus::PendingRetry { .. })
                .then_some(id.as_str())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_all_steps_pending() {
        let ids = vec!["a".into(), "b".into(), "c".into()];
        let state = PlanExecutionState::new(ids);
        assert!(matches!(state.get("a"), Some(StepStatus::Pending)));
        assert!(matches!(state.get("b"), Some(StepStatus::Pending)));
        assert!(matches!(state.get("c"), Some(StepStatus::Pending)));
    }

    #[test]
    fn mark_dispatched_and_lookup_by_cmd_id() {
        let mut state = PlanExecutionState::new(vec!["step_a".into()]);
        state.mark_dispatched("step_a", vec!["cmd-001".into(), "cmd-002".into()]);
        assert_eq!(state.step_for_cmd("cmd-001"), Some("step_a"));
        assert_eq!(state.step_for_cmd("cmd-002"), Some("step_a"));
        assert!(matches!(
            state.get("step_a"),
            Some(StepStatus::Dispatched { exec_ids }) if exec_ids.len() == 2
        ));
    }

    #[test]
    fn complete_all_cmds_marks_step_completed() {
        let mut state = PlanExecutionState::new(vec!["step_a".into()]);
        state.mark_dispatched("step_a", vec!["cmd-1".into(), "cmd-2".into()]);
        assert!(state.record_outcome("cmd-1", true).is_none());
        let status = state.record_outcome("cmd-2", false).unwrap();
        assert!(matches!(status, StepStatus::Completed { outcomes } if outcomes == vec![true, false]));
    }

    #[test]
    fn step_targets_stored_and_retrieved() {
        let mut state = PlanExecutionState::new(vec!["step_a".into()]);
        state.set_targets("step_a", vec!["ns/default/pod/nginx-abc".into()]);
        assert_eq!(
            state.targets_for("step_a"),
            &["ns/default/pod/nginx-abc"]
        );
    }

    #[test]
    fn is_complete_when_all_terminal() {
        let mut state = PlanExecutionState::new(vec!["a".into(), "b".into()]);
        state.mark_dispatched("a", vec!["cmd-1".into()]);
        state.record_outcome("cmd-1", true);
        state.mark_skipped("b", "hard dep failed");
        assert!(state.is_complete());
    }
}
