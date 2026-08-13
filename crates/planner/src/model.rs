use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDefinition {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub steps: Vec<StepDefinition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TargetQuery {
    /// Explicit entity id (exact match). Takes precedence over every other field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Entity kind to match (Pod, ServiceAccount, …). Defaults to "Pod" when
    /// `workload` is set but kind is omitted.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Name regex (wildcard mode). Ignored when `id` or `workload` is set.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Match entities controlled by this workload. When live owner references
    /// aren't available, the controller's generated-name pattern is derived from
    /// the workload name (see `resolver::derive_pod_pattern`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<WorkloadRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<SelectStrategy>,
}

/// Reference to a controlling workload (Deployment, StatefulSet, DaemonSet, Job, …)
/// whose managed pods a step should target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkloadRef {
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectStrategy {
    Random,
    First,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDefinition {
    pub id: String,
    pub action: String,
    pub target: TargetQuery,
    /// Optional execution source override. When set, planner resolves this
    /// query and passes the selected entity as `exec_system_id` while keeping
    /// `target` as the semantic action target.
    #[serde(default)]
    pub exec_target: Option<TargetQuery>,
    /// Optional Kubernetes authentication identity. Resolves to an eligible
    /// ServiceAccount or active K8sCredential and is dispatched as
    /// `auth_identity_id`.
    ///
    /// Backward compatibility: `token` and `token_target` are accepted as aliases.
    #[serde(default, alias = "token", alias = "token_target")]
    pub authenticate_as: Option<TargetQuery>,
    #[serde(default)]
    pub args: HashMap<String, String>,
    #[serde(default)]
    pub procedure: Option<String>,
    #[serde(default)]
    pub retry: RetryStrategy,
    #[serde(default)]
    pub depends_on: Vec<Dependency>,
    #[serde(default)]
    pub expect: Option<StepExpectation>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepExpectation {
    /// Minimum inferred facts count (entities + relations + system-field updates)
    /// required for this step to be considered successful.
    #[serde(default)]
    pub min_facts_written: usize,
}

#[cfg(test)]
mod auth_identity_tests {
    use super::StepDefinition;

    fn parse_selector(field: &str) -> StepDefinition {
        serde_yaml::from_str(&format!(
            r#"
id: step-1
action: get-pods
target:
  id: ns/default/pod/demo
{field}:
  id: ns/default/sa/operator
note: test
"#
        ))
        .expect("plan step should deserialize")
    }

    #[test]
    fn canonical_and_legacy_auth_selectors_deserialize_to_authenticate_as() {
        for field in ["authenticate_as", "token", "token_target"] {
            let step = parse_selector(field);
            assert_eq!(
                step.authenticate_as.and_then(|query| query.id).as_deref(),
                Some("ns/default/sa/operator")
            );
        }
    }

    #[test]
    fn serialization_uses_only_canonical_authenticate_as_name() {
        let yaml = serde_yaml::to_string(&parse_selector("token")).expect("serialize step");
        assert!(yaml.contains("authenticate_as:"));
        assert!(!yaml.contains("\ntoken:"));
        assert!(!yaml.contains("\ntoken_target:"));
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryStrategy {
    /// Default planner behavior: if a step fails, try the next procedure
    /// variant (when available) before declaring step failure.
    #[default]
    NextProcedure,
    /// Retry the same procedure once on failure. Useful for transient
    /// transport failures where switching procedure is unnecessary.
    SameProcedure,
    None,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Require {
    #[default]
    Completion,
    Success,
    AnySuccess,
    AllSuccess,
}

// Helper struct for deserialization only
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawDep {
    Step {
        step: String,
        #[serde(default)]
        require: Require,
    },
    Graph {
        graph: String,
    },
}

// Parsed form of a Graph dependency
#[derive(Debug, Clone)]
pub(crate) struct ParsedGraphDep {
    pub step_ref: String,
    pub relation: String,
    pub all: bool,
}

impl ParsedGraphDep {
    pub fn parse(raw: &str) -> Option<Self> {
        let parts: Vec<&str> = raw.splitn(2, ' ').collect();
        if parts.len() != 2 {
            return None;
        }
        let step_ref = parts[0].strip_prefix("step:")?.to_string();
        let (all, relation) = if let Some(r) = parts[1].strip_prefix("all_have:") {
            (true, r.to_string())
        } else if let Some(r) = parts[1].strip_prefix("has:") {
            (false, r.to_string())
        } else {
            return None;
        };
        Some(Self {
            step_ref,
            relation,
            all,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dependency {
    Step {
        step: String,
        require: Require,
    },
    Graph {
        step_ref: String,
        relation: String,
        all: bool,
    },
}

impl<'de> Deserialize<'de> for Dependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawDep::deserialize(deserializer)?;
        match raw {
            RawDep::Step { step, require } => Ok(Dependency::Step { step, require }),
            RawDep::Graph { graph } => {
                let parsed = ParsedGraphDep::parse(&graph).ok_or_else(|| {
                    serde::de::Error::custom(format!("invalid graph predicate: {graph}"))
                })?;
                Ok(Dependency::Graph {
                    step_ref: parsed.step_ref,
                    relation: parsed.relation,
                    all: parsed.all,
                })
            }
        }
    }
}

impl Serialize for Dependency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        match self {
            Dependency::Step { step, require } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("step", step)?;
                if require != &Require::Completion {
                    map.serialize_entry("require", require)?;
                }
                map.end()
            }
            Dependency::Graph {
                step_ref,
                relation,
                all,
            } => {
                let mut map = serializer.serialize_map(Some(1))?;
                let graph_str = if *all {
                    format!("step:{} all_have:{}", step_ref, relation)
                } else {
                    format!("step:{} has:{}", step_ref, relation)
                };
                map.serialize_entry("graph", &graph_str)?;
                map.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PLAN: &str = r#"
id: test-plan
name: Test Plan
version: "1.0"
steps:
  - id: step_a
    action: k8s.exec-into-pod
    target:
      kind: Pod
      namespace: default
      name: "nginx-.*"
    args:
      cmd: id
    retry: next_procedure
  - id: step_b
    action: container.escape-to-host
    target:
      kind: Pod
      namespace: default
      name: "nginx-.*"
      select: first
    depends_on:
      - step: step_a
        require: success
      - graph: "step:step_a has:rce.can-exec"
"#;

    #[test]
    fn parses_plan_from_yaml() {
        let plan: PlanDefinition = serde_yaml::from_str(SAMPLE_PLAN).unwrap();
        assert_eq!(plan.id, "test-plan");
        assert_eq!(plan.steps.len(), 2);

        let step_a = &plan.steps[0];
        assert_eq!(step_a.id, "step_a");
        assert_eq!(step_a.action, "k8s.exec-into-pod");
        assert_eq!(step_a.target.kind, "Pod");
        assert_eq!(step_a.target.namespace, Some("default".into()));
        assert_eq!(step_a.target.name, "nginx-.*");
        assert_eq!(step_a.target.select, None);
        assert_eq!(step_a.retry, RetryStrategy::NextProcedure);
        assert_eq!(step_a.args.get("cmd"), Some(&"id".to_string()));
        assert!(step_a.depends_on.is_empty());

        let step_b = &plan.steps[1];
        assert_eq!(step_b.target.select, Some(SelectStrategy::First));
        assert_eq!(step_b.depends_on.len(), 2);
        assert!(matches!(
            &step_b.depends_on[0],
            Dependency::Step { step, require: Require::Success } if step == "step_a"
        ));
        assert!(matches!(
            &step_b.depends_on[1],
            Dependency::Graph { step_ref, relation, all: false }
            if step_ref == "step_a" && relation == "rce.can-exec"
        ));
    }

    #[test]
    fn parses_workload_and_id_targets() {
        let yaml = r#"
id: t
name: T
version: "1.0"
steps:
  - id: by_workload
    action: k8s.exec-into-pod
    target:
      kind: Pod
      namespace: web
      workload: { kind: Deployment, name: app }
      select: all
  - id: by_id
    action: container.escape-to-host
    target:
      id: "ns/web/pod/app-7d4b9f-xk2jp"
"#;
        let plan: PlanDefinition = serde_yaml::from_str(yaml).unwrap();
        let w = &plan.steps[0].target;
        assert_eq!(w.kind, "Pod");
        assert_eq!(
            w.workload,
            Some(WorkloadRef {
                kind: "Deployment".into(),
                name: "app".into()
            })
        );
        assert_eq!(w.select, Some(SelectStrategy::All));
        assert!(w.name.is_empty());

        let i = &plan.steps[1].target;
        assert_eq!(i.id.as_deref(), Some("ns/web/pod/app-7d4b9f-xk2jp"));
        assert!(i.kind.is_empty());
    }
}
