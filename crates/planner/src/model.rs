use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDefinition {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: String,
    pub steps: Vec<StepDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetQuery {
    pub kind: String,
    pub namespace: Option<String>,
    pub name: String,
    pub select: Option<SelectStrategy>,
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
    #[serde(default)]
    pub args: HashMap<String, String>,
    #[serde(default)]
    pub procedure: Option<String>,
    #[serde(default)]
    pub retry: RetryStrategy,
    #[serde(default)]
    pub depends_on: Vec<Dependency>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryStrategy {
    #[default]
    None,
    NextProcedure,
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
    Step { step: String, require: Require },
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
                let parsed = ParsedGraphDep::parse(&graph)
                    .ok_or_else(|| serde::de::Error::custom(format!("invalid graph predicate: {graph}")))?;
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
}
