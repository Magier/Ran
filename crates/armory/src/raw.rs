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
    cleanup: Option<RawProcedure>,
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
    http_request: Option<JsonValue>,
    k8s_request: Option<JsonValue>,
    steps: Option<JsonValue>,
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
                if p.command.trim().is_empty()
                    && p.http_request.is_none()
                    && p.k8s_request.is_none()
                    && p.steps.is_none()
                {
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
                    http_request: p.http_request,
                    k8s_request: p.k8s_request,
                    steps: p.steps,
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
                        http_request: None,
                        k8s_request: None,
                        steps: None,
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

        let cleanup = self.cleanup.and_then(|p| {
            if p.command.trim().is_empty()
                && p.http_request.is_none()
                && p.k8s_request.is_none()
                && p.steps.is_none()
            {
                return None;
            }
            let id =
                p.id.or(p.key.clone())
                    .unwrap_or_else(|| "cleanup".to_string());
            Some(Procedure {
                id,
                command: p.command,
                tool: p.tool.or(p.key),
                is_local_command: p.is_local,
                http_request: p.http_request,
                k8s_request: p.k8s_request,
                steps: p.steps,
            })
        });

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
            cleanup,
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
    fn cleanup_with_no_id_defaults_to_cleanup_id() {
        let yaml = r#"
name: Install curl
tactic: Execution
procedures:
  - command: apt install -y curl
cleanup:
  command: apt remove -y curl
"#;
        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw
            .into_ttp(Path::new("Execution/install_curl.yaml"))
            .unwrap();

        let cleanup = ttp.cleanup.expect("cleanup should be present");
        assert_eq!(cleanup.id, "cleanup");
        assert_eq!(cleanup.command, "apt remove -y curl");
        assert!(cleanup.tool.is_none());
    }

    #[test]
    fn cleanup_with_key_sets_id_and_tool() {
        let yaml = r#"
name: Install curl
tactic: Execution
procedures:
  - command: apt install -y curl
cleanup:
  key: ubuntu
  command: apt remove -y curl
"#;
        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw
            .into_ttp(Path::new("Execution/install_curl.yaml"))
            .unwrap();

        let cleanup = ttp.cleanup.expect("cleanup should be present");
        assert_eq!(cleanup.id, "ubuntu");
        assert_eq!(cleanup.tool.as_deref(), Some("ubuntu"));
        assert_eq!(cleanup.command, "apt remove -y curl");
    }

    #[test]
    fn cleanup_with_empty_command_produces_none() {
        let yaml = r#"
name: Install curl
tactic: Execution
procedures:
  - command: apt install -y curl
cleanup:
  command: ""
"#;
        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw
            .into_ttp(Path::new("Execution/install_curl.yaml"))
            .unwrap();

        assert!(
            ttp.cleanup.is_none(),
            "empty cleanup command should produce None"
        );
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

    #[test]
    fn k8s_request_procedure_is_preserved_through_into_ttp() {
        let yaml = r#"
name: Get Pods
tactic: Discovery
procedures:
  - key: k8s-request
    k8s_request:
      api_server: https://10.0.0.1:6443
      api: /api/v1
      resource: pods
      namespace: default
      cluster_scoped: "false"
      query: limit=500
      token: mytoken
      use_ca: false
"#;
        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw.into_ttp(Path::new("Discovery/get_pods.yaml")).unwrap();
        assert_eq!(ttp.procedures.len(), 1);
        let proc = &ttp.procedures[0];
        assert_eq!(proc.id, "k8s-request");
        assert!(
            proc.k8s_request.is_some(),
            "k8s_request should be preserved"
        );
        assert!(proc.http_request.is_none());
        assert!(proc.command.is_empty());
    }

    #[test]
    fn k8s_request_procedure_without_key_gets_positional_id() {
        let yaml = r#"
name: Get Pods
tactic: Discovery
procedures:
  - k8s_request:
      api: /api/v1
      resource: pods
"#;
        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw.into_ttp(Path::new("Discovery/get_pods.yaml")).unwrap();
        assert_eq!(ttp.procedures.len(), 1);
        assert_eq!(ttp.procedures[0].id, "proc-1");
        assert!(ttp.procedures[0].k8s_request.is_some());
    }

    #[test]
    fn procedure_with_only_k8s_request_is_not_filtered_out() {
        // Regression: the empty-check filter must treat k8s_request as non-empty
        let yaml = r#"
name: Test
tactic: Discovery
procedures:
  - k8s_request:
      api: /api/v1
      resource: nodes
"#;
        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw.into_ttp(Path::new("Discovery/test.yaml")).unwrap();
        assert_eq!(ttp.procedures.len(), 1);
    }

    #[test]
    fn cleanup_with_only_k8s_request_is_not_filtered_out() {
        let yaml = r#"
name: Test
tactic: Discovery
procedures:
  - command: kubectl get nodes
cleanup:
  k8s_request:
    api: /api/v1
    resource: nodes
"#;
        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw.into_ttp(Path::new("Discovery/test.yaml")).unwrap();
        assert!(
            ttp.cleanup.is_some(),
            "cleanup with k8s_request should not be filtered out"
        );
        let cleanup = ttp.cleanup.unwrap();
        assert!(cleanup.k8s_request.is_some());
    }

    #[test]
    fn copyfail_ttp_parses_correctly() {
        let yaml = r#"
name: Escape container via CopyFail (CVE-2026-31431)
description: >
  Exploit a Linux kernel page-cache Copy-on-Write race (CVE-2026-31431) to
  escape an unprivileged container.
tactic: "Privilege Escalation"
techniques: ["Escape to Host", "T1611"]
status: draft
effects:
  - container.escape(sys)
parameters:
  KERNEL_VERSION:
    type: string
    required: false
    description: "Kernel version of the target node (vulnerable: <6.6.89 or <6.12.80)"
  PAYLOAD:
    type: string
    required: false
    default: hostname
    description: "Command to run in host context after a privileged binary is corrupted"
preconditions:
  accessLevel: "user-exec"
procedures:
  - key: copyfail-poc
    command: python3
    isLocal: true
  - key: ran-implant
    command: ran-implant --exploit copyfail --payload ${PAYLOAD}
references:
  - https://github.com/Percivalll/Copy-Fail-CVE-2026-31431-Kubernetes-PoC
  - https://attack.mitre.org/techniques/T1611/
"#;

        let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
        let ttp = raw
            .into_ttp(Path::new(
                "Privilege Escalation/Escape to Host/copyfail.yaml",
            ))
            .unwrap();

        assert_eq!(ttp.name, "Escape container via CopyFail (CVE-2026-31431)");
        assert_eq!(ttp.tactic, "Privilege Escalation");
        assert_eq!(ttp.status, "draft");
        assert!(
            ttp.techniques.iter().any(|t| t == "T1611"),
            "should include T1611"
        );
        assert!(
            ttp.techniques.iter().any(|t| t == "Escape to Host"),
            "should include Escape to Host technique"
        );
        assert!(
            ttp.effects.iter().any(|e| e == "container.escape(sys)"),
            "should have escape effect"
        );
        assert!(
            ttp.requires
                .get("accessLevel")
                .and_then(|v| v.as_str())
                == Some("user-exec"),
            "precondition accessLevel should be user-exec"
        );

        let kernel_param = ttp.params.iter().find(|p| p.name == "KERNEL_VERSION");
        assert!(kernel_param.is_some(), "KERNEL_VERSION param should exist");
        assert!(!kernel_param.unwrap().required, "KERNEL_VERSION should be optional");

        let payload_param = ttp.params.iter().find(|p| p.name == "PAYLOAD");
        assert!(payload_param.is_some(), "PAYLOAD param should exist");
        assert_eq!(payload_param.unwrap().default, "hostname");

        assert_eq!(ttp.procedures.len(), 2, "should have two procedures");
        assert_eq!(ttp.procedures[0].id, "copyfail-poc");
        assert_eq!(ttp.procedures[0].command, "python3");
        assert_eq!(ttp.procedures[1].id, "ran-implant");
        assert_eq!(
            ttp.procedures[0].is_local_command,
            Some(true),
            "copyfail-poc should be marked as a local command"
        );
        assert!(
            ttp.procedures[1].is_local_command.is_none(),
            "ran-implant should not be marked as local"
        );
        assert!(
            ttp.procedures[1]
                .command
                .contains("ran-implant --exploit copyfail"),
            "ran-implant procedure should reference the implant binary"
        );

        assert_eq!(ttp.references.len(), 2);
    }
}
