use crate::model::{
    Dependency, PlanDefinition, Require, RetryStrategy, StepDefinition, TargetQuery,
};
use campaign::ExecutionRecord;
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub enum Confidence {
    High,
    Low,
    Stable,
}

#[derive(Debug, Clone)]
pub struct FuzzResult {
    pub original: String,
    pub pattern: String,
    pub confidence: Confidence,
}

pub struct FuzzReport(pub Vec<FuzzResult>);

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub include_failed: bool,
}

pub fn fuzzify_entity_id(entity_id: &str) -> FuzzResult {
    let name = entity_id.rsplit('/').next().unwrap_or(entity_id);
    let kind = entity_kind_from_id(entity_id);

    if kind != "pod" {
        return FuzzResult {
            original: entity_id.to_string(),
            pattern: name.to_string(),
            confidence: Confidence::Stable,
        };
    }

    // Strip trailing k8s-generated suffixes from pod names:
    //   Deployment:   <name>-<rs-hash(10)>-<pod-hash(5)>
    //   DaemonSet:    <name>-<node-hash(5)>
    //   StatefulSet:  <name>-<ordinal>
    static K8S_HASH: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re =
        K8S_HASH.get_or_init(|| Regex::new(r"-([a-z0-9]{5,6}|[a-z0-9]{9,10}|[0-9]+)$").unwrap());

    let mut base = name.to_string();
    let mut stripped = false;

    for _ in 0..2 {
        if let Some(m) = re.find(&base) {
            base = base[..m.start()].to_string();
            stripped = true;
        } else {
            break;
        }
    }

    if stripped {
        FuzzResult {
            original: entity_id.to_string(),
            pattern: format!("{}-.*", base),
            confidence: Confidence::High,
        }
    } else {
        FuzzResult {
            original: entity_id.to_string(),
            pattern: format!("{}.*", base),
            confidence: Confidence::Low,
        }
    }
}

fn entity_kind_from_id(entity_id: &str) -> &str {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["node", ..] => "node",
        ["ns", _, "sa", ..] => "serviceaccount",
        ["ns", _, kind, ..] => kind,
        _ => "unknown",
    }
}

fn entity_namespace_from_id(entity_id: &str) -> Option<&str> {
    let parts: Vec<&str> = entity_id.splitn(4, '/').collect();
    match parts.as_slice() {
        ["ns", ns, ..] => Some(ns),
        _ => None,
    }
}

fn step_id_from_record(ttp_id: &str, index: usize) -> String {
    let slug = ttp_id.replace(['.', '-'], "_");
    format!("step_{}_{}", index, slug)
}

pub fn export_plan(records: &[ExecutionRecord], opts: &ExportOptions) -> PlanDefinition {
    let successful: Vec<&ExecutionRecord> = records
        .iter()
        .filter(|r| r.success && !r.is_cleanup)
        .collect();

    let failed: Vec<&ExecutionRecord> = if opts.include_failed {
        records
            .iter()
            .filter(|r| !r.success && !r.is_cleanup)
            .collect()
    } else {
        vec![]
    };

    let mut steps: Vec<StepDefinition> = Vec::new();
    let mut success_step_ids: Vec<String> = Vec::new();

    // Build success chain (linear, each depends on previous)
    for (i, rec) in successful.iter().enumerate() {
        let step_id = step_id_from_record(&rec.ttp_id, i);
        let fuzz = fuzzify_entity_id(&rec.target_id);

        let depends_on = if i == 0 {
            vec![]
        } else {
            vec![Dependency::Step {
                step: success_step_ids[i - 1].clone(),
                require: Require::Success,
            }]
        };

        let kind = capitalize(entity_kind_from_id(&rec.target_id));
        let namespace = entity_namespace_from_id(&rec.target_id).map(str::to_string);

        steps.push(StepDefinition {
            id: step_id.clone(),
            action: rec.ttp_id.clone(),
            target: TargetQuery {
                kind,
                namespace,
                name: fuzz.pattern,
                select: None,
            },
            args: rec.args.clone(),
            procedure: Some(rec.procedure_id.clone()).filter(|s| !s.is_empty()),
            retry: RetryStrategy::None,
            depends_on,
            note: None,
        });
        success_step_ids.push(step_id);
    }

    // Add failed steps as side branches
    let failed_start = steps.len();
    for (fi, rec) in failed.iter().enumerate() {
        let step_id = step_id_from_record(&rec.ttp_id, failed_start + fi);
        let fuzz = fuzzify_entity_id(&rec.target_id);

        // Find last successful record that appears before this failed one
        let record_pos = records.iter().position(|r| r.id == rec.id).unwrap_or(0);
        let last_success_step_id = records[..record_pos]
            .iter()
            .rev()
            .find(|r| r.success && !r.is_cleanup)
            .and_then(|r| {
                let idx = successful.iter().position(|s| s.id == r.id)?;
                success_step_ids.get(idx).cloned()
            });

        let depends_on = match last_success_step_id {
            Some(sid) => vec![Dependency::Step {
                step: sid,
                require: Require::Success,
            }],
            None => vec![],
        };

        let kind = capitalize(entity_kind_from_id(&rec.target_id));
        let namespace = entity_namespace_from_id(&rec.target_id).map(str::to_string);

        steps.push(StepDefinition {
            id: step_id,
            action: rec.ttp_id.clone(),
            target: TargetQuery {
                kind,
                namespace,
                name: fuzz.pattern,
                select: None,
            },
            args: rec.args.clone(),
            procedure: Some(rec.procedure_id.clone()).filter(|s| !s.is_empty()),
            retry: RetryStrategy::None,
            depends_on,
            note: Some("recorded: failed".into()),
        });
    }

    PlanDefinition {
        id: "exported-plan".into(),
        name: "Exported Plan".into(),
        description: Some("Auto-generated from campaign execution history".into()),
        version: "1.0".into(),
        steps,
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn fuzzifies_deployment_pod() {
        let r = fuzzify_entity_id("ns/default/pod/nginx-7d4b9f-xk2jp");
        assert_eq!(r.pattern, "nginx-.*");
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn fuzzifies_statefulset_pod() {
        let r = fuzzify_entity_id("ns/default/pod/postgres-0");
        assert_eq!(r.pattern, "postgres-.*");
        assert_eq!(r.confidence, Confidence::High);
    }

    #[test]
    fn stable_name_passes_through() {
        let r = fuzzify_entity_id("node/worker-1");
        assert_eq!(r.pattern, "worker-1");
        assert_eq!(r.confidence, Confidence::Stable);
    }

    #[test]
    fn service_account_stable() {
        let r = fuzzify_entity_id("ns/default/sa/nginx-sa");
        assert_eq!(r.pattern, "nginx-sa");
        assert_eq!(r.confidence, Confidence::Stable);
    }

    #[test]
    fn export_success_only_plan() {
        let records = vec![
            make_record(
                "cmd-1",
                "k8s.exec-into-pod",
                "ns/default/pod/nginx-abc-xyz",
                "proc-1",
                true,
            ),
            make_record(
                "cmd-2",
                "container.check-caps",
                "ns/default/pod/nginx-abc-xyz",
                "proc-1",
                false,
            ),
            make_record(
                "cmd-3",
                "container.escape",
                "ns/default/pod/nginx-abc-xyz",
                "proc-1",
                true,
            ),
        ];
        let opts = ExportOptions {
            include_failed: false,
        };
        let plan = export_plan(&records, &opts);
        assert_eq!(plan.steps.len(), 2); // only cmd-1 and cmd-3
        assert_eq!(plan.steps[0].id, "step_0_k8s_exec_into_pod");
        assert!(plan.steps[1].depends_on.iter().any(|d| {
            matches!(d, crate::model::Dependency::Step { step, require: crate::model::Require::Success }
                if step == "step_0_k8s_exec_into_pod")
        }));
    }

    #[test]
    fn export_include_failed_adds_side_branches() {
        let records = vec![
            make_record(
                "cmd-1",
                "k8s.exec-into-pod",
                "ns/default/pod/nginx-abc-xyz",
                "proc-1",
                true,
            ),
            make_record(
                "cmd-2",
                "container.exploit-cve",
                "ns/default/pod/nginx-abc-xyz",
                "proc-1",
                false,
            ),
            make_record(
                "cmd-3",
                "container.escape",
                "ns/default/pod/nginx-abc-xyz",
                "proc-1",
                true,
            ),
        ];
        let opts = ExportOptions {
            include_failed: true,
        };
        let plan = export_plan(&records, &opts);
        assert_eq!(plan.steps.len(), 3);
        // The failed step depends on cmd-1's step (same predecessor as cmd-3), not on cmd-3
        let failed_step = plan
            .steps
            .iter()
            .find(|s| s.note.as_deref() == Some("recorded: failed"))
            .unwrap();
        assert!(failed_step.depends_on.iter().any(|d| {
            matches!(d, crate::model::Dependency::Step { step, .. } if step == "step_0_k8s_exec_into_pod")
        }));
        // Nothing depends on the failed step
        let failed_id = failed_step.id.clone();
        for step in &plan.steps {
            for dep in &step.depends_on {
                if let crate::model::Dependency::Step { step: dep_id, .. } = dep {
                    assert_ne!(dep_id, &failed_id, "something depends on failed step");
                }
            }
        }
    }

    fn make_record(
        id: &str,
        ttp_id: &str,
        target_id: &str,
        procedure_id: &str,
        success: bool,
    ) -> campaign::ExecutionRecord {
        campaign::ExecutionRecord {
            id: id.to_string(),
            ttp_id: ttp_id.to_string(),
            ttp_name: ttp_id.to_string(),
            tactic: "Execution".to_string(),
            target_id: target_id.to_string(),
            exec_system_id: target_id.to_string(),
            procedure_id: procedure_id.to_string(),
            command: "echo test".to_string(),
            args: HashMap::new(),
            success,
            exit_code: if success { 0 } else { 1 },
            results: vec![],
            fail_reason: String::new(),
            started_at_ms: 0,
            completed_at_ms: 0,
            is_cleanup: false,
        }
    }
}
