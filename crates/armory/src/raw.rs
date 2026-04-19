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
                let id =
                    p.id.or(p.key.clone())
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

        let mut requires = match self.preconditions.or(self.requires) {
            Some(JsonValue::Object(map)) => map,
            Some(other) => {
                let mut map = JsonMap::new();
                map.insert("value".to_string(), other);
                map
            }
            None => JsonMap::new(),
        };
        normalize_requires(&mut requires);

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

/// Normalise the `requires` map so it matches the OpenAPI `Requirements` schema
/// the frontend expects.
///
/// - Renames `"rbac"` → `"rbacPermissions"` so the frontend badge renderer fires.
/// - Within each RBAC entry, renames `"resource"` → `"resourceType"` to match the
///   `RBACPermission` schema field used by `formatRbac`.
fn normalize_requires(map: &mut JsonMap<String, JsonValue>) {
    if let Some(rbac) = map.remove("rbac") {
        let normalized = if let JsonValue::Array(entries) = rbac {
            let entries = entries
                .into_iter()
                .map(|entry| {
                    let JsonValue::Object(mut obj) = entry else {
                        return entry;
                    };
                    if let Some(resource) = obj.remove("resource") {
                        obj.insert("resourceType".to_string(), resource);
                    }
                    JsonValue::Object(obj)
                })
                .collect();
            JsonValue::Array(entries)
        } else {
            rbac
        };
        map.insert("rbacPermissions".to_string(), normalized);
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
        let ttp = raw
            .into_ttp(Path::new("Impact/delete_deployment.yaml"))
            .unwrap();

        assert_eq!(ttp.params.len(), 1);
        assert_eq!(ttp.params[0].name, "Namespace");
        assert_eq!(ttp.params[0].param_type, "string");
        assert_eq!(
            ttp.requires.get("kind").and_then(|v| v.as_str()),
            Some("Deployment")
        );
        assert_eq!(ttp.procedures[0].id, "kubectl");
    }

    #[test]
    fn rbac_field_is_normalised_to_rbac_permissions() {
        let yaml = r#"
name: Delete Events
tactic: Defense Evasion
preconditions:
  rbac:
    - verb: delete
      resource: events
procedures:
  - command: kubectl delete events --all
"#;
        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw
            .into_ttp(Path::new("Defense Evasion/delete_events.yaml"))
            .unwrap();

        assert!(
            ttp.requires.get("rbac").is_none(),
            "raw 'rbac' key should be removed"
        );
        let rbac_perms = ttp
            .requires
            .get("rbacPermissions")
            .expect("rbacPermissions should exist")
            .as_array()
            .expect("should be array");
        assert_eq!(rbac_perms.len(), 1);

        let entry = rbac_perms[0].as_object().unwrap();
        assert!(
            entry.contains_key("resourceType"),
            "resource should be renamed to resourceType"
        );
        assert!(
            !entry.contains_key("resource"),
            "original 'resource' key should be gone"
        );
        assert_eq!(entry["verb"].as_str(), Some("delete"));
        assert_eq!(entry["resourceType"].as_str(), Some("events"));
    }
}
