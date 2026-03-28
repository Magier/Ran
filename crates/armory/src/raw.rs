use crate::model::{Procedure, Ttp, TtpParam};
use crate::util::{json_to_string, slugify};
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct RawTtp {
    id: Option<String>,
    name: String,
    description: String,
    tactic: Option<String>,
    techniques: Vec<String>,
    status: Option<String>,
    parameters: BTreeMap<String, RawParam>,
    procedures: Vec<RawProcedure>,
    preconditions: Option<JsonValue>,
    #[serde(alias = "requires")]
    requires: Option<JsonValue>,
    effects: Vec<String>,
    references: Vec<String>,
    #[serde(alias = "refernces")]
    references_typo: Vec<String>,
    command: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawParam {
    #[serde(rename = "type")]
    param_type: String,
    default: JsonValue,
    description: String,
    required: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawProcedure {
    id: Option<String>,
    key: Option<String>,
    command: String,
    tool: Option<String>,
    #[serde(alias = "isLocal", alias = "isLocalCommand")]
    is_local: Option<bool>,
}

impl RawTtp {
    pub(crate) fn into_ttp(self, file_path: &Path) -> Option<Ttp> {
        if self.name.trim().is_empty() {
            return None;
        }

        let params = self
            .parameters
            .into_iter()
            .map(|(name, p)| TtpParam {
                name,
                param_type: if p.param_type.trim().is_empty() {
                    "string".to_string()
                } else {
                    p.param_type
                },
                description: p.description,
                required: p.required.unwrap_or(true),
                default: json_to_string(p.default),
            })
            .collect();

        let mut procedures: Vec<Procedure> = self
            .procedures
            .into_iter()
            .enumerate()
            .filter_map(|(idx, p)| {
                if p.command.trim().is_empty() {
                    return None;
                }
                let id = p
                    .id
                    .or(p.key.clone())
                    .unwrap_or_else(|| format!("proc-{}", idx + 1));
                Some(Procedure {
                    id,
                    command: p.command,
                    tool: p.tool.or(p.key),
                    is_local_command: p.is_local,
                })
            })
            .collect();

        if procedures.is_empty() {
            if let Some(command) = self.command {
                if !command.trim().is_empty() {
                    procedures.push(Procedure {
                        id: "default".to_string(),
                        command,
                        tool: None,
                        is_local_command: None,
                    });
                }
            }
        }

        let references = if !self.references.is_empty() {
            self.references
        } else {
            self.references_typo
        };

        let requires = match self.preconditions.or(self.requires) {
            Some(JsonValue::Object(map)) => map,
            Some(other) => {
                let mut map = JsonMap::new();
                map.insert("value".to_string(), other);
                map
            }
            None => JsonMap::new(),
        };

        let tactic = self
            .tactic
            .filter(|t| !t.trim().is_empty())
            .or_else(|| {
                file_path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "Other".to_string());

        let id = self.id.unwrap_or_else(|| slugify(&self.name));

        Some(Ttp {
            id,
            name: self.name,
            description: self.description,
            tactic,
            techniques: self.techniques,
            status: self.status.unwrap_or_else(|| "enabled".to_string()),
            params,
            requires,
            effects: self.effects,
            procedures,
            references,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_parameters_dict_to_params_array() {
        let yaml = r#"
name: Delete Deployment
description: test
tactic: Impact
techniques: [T1489]
parameters:
  Namespace:
    type: string
    default: kube-system
    description: ns
preconditions:
  kind: Deployment
procedures:
  - key: kubectl
    command: kubectl delete deployment foo
"#;

        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw.into_ttp(Path::new("Impact/delete_deployment.yaml")).unwrap();

        assert_eq!(ttp.params.len(), 1);
        assert_eq!(ttp.params[0].name, "Namespace");
        assert_eq!(ttp.params[0].param_type, "string");
        assert_eq!(ttp.requires.get("kind").and_then(|v| v.as_str()), Some("Deployment"));
        assert_eq!(ttp.procedures[0].id, "kubectl");
    }
}
