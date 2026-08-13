use crate::error::ArmoryError;
use crate::model::{Procedure, Ttp};
use crate::raw::RawTtp;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const VALID_ACCOUNTS_KUBECONFIG_ID: &str = "valid-accounts-kubeconfig";
pub const DEPRECATED_INITIAL_ACCESS_POD_EXEC_ID: &str = "initial-access-pod-exec";

/// Resolve compatibility action IDs without advertising duplicate TTPs.
pub fn canonical_ttp_id(id: &str) -> &str {
    match id {
        DEPRECATED_INITIAL_ACCESS_POD_EXEC_ID => VALID_ACCOUNTS_KUBECONFIG_ID,
        _ => id,
    }
}

#[cfg(feature = "bundled-armory")]
#[derive(rust_embed::Embed)]
#[folder = "../../armory/TTPs"]
struct BundledTtps;

#[derive(Debug, Clone)]
pub struct Armory {
    source_dir: PathBuf,
    ttps: Vec<Ttp>,
}

impl Armory {
    /// Primary entry point.
    ///
    /// **Dev builds** (no `bundled-armory` feature): loads TTPs exclusively from
    /// `user_dir`. The caller is responsible for resolving a default path when
    /// `user_dir` is `None`.
    ///
    /// **Release builds** (`bundled-armory` feature): always loads the built-in
    /// TTPs embedded in the binary first, then appends any TTPs found in
    /// `user_dir` (if provided). This mirrors the Go behaviour: built-ins are
    /// the baseline, the user directory extends them.
    pub fn load(user_dir: Option<&Path>) -> Result<Self, ArmoryError> {
        let mut ttps: Vec<Ttp> = Vec::new();

        // --- Phase 1: built-in TTPs (release only) ---------------------------
        #[cfg(feature = "bundled-armory")]
        {
            ttps.extend(Self::ttps_from_bundled()?);
        }

        // --- Phase 2: user-supplied directory --------------------------------
        if let Some(dir) = user_dir {
            ttps.extend(Self::ttps_from_dir(dir)?);
        }

        if ttps.is_empty() {
            let label = user_dir
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "<bundled>".to_string());
            return Err(ArmoryError::NoTtpsLoaded(label));
        }

        // --- Phase 3: expand slot references ---------------------------------
        Self::expand_slot_procedures(&mut ttps);
        Self::validate_k8s_auth(&ttps)?;

        Ok(Self {
            source_dir: user_dir
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("<bundled>")),
            ttps,
        })
    }

    /// Load TTPs from a filesystem directory. Used by both `load()` and tests.
    pub fn load_from_dir(path: impl AsRef<Path>) -> Result<Self, ArmoryError> {
        let path = path.as_ref();
        let mut ttps = Self::ttps_from_dir(path)?;

        if ttps.is_empty() {
            return Err(ArmoryError::NoTtpsLoaded(path.display().to_string()));
        }

        Self::expand_slot_procedures(&mut ttps);
        Self::validate_k8s_auth(&ttps)?;

        Ok(Self {
            source_dir: path.to_path_buf(),
            ttps,
        })
    }

    pub fn source_dir(&self) -> &Path {
        &self.source_dir
    }

    pub fn ttps(&self) -> &[Ttp] {
        &self.ttps
    }

    /// Construct an `Armory` directly from a list of TTPs (useful in tests).
    pub fn from_ttps(ttps: Vec<Ttp>) -> Self {
        Self {
            source_dir: PathBuf::new(),
            ttps,
        }
    }

    pub fn get_ttp(&self, id: &str) -> Option<&Ttp> {
        let id = canonical_ttp_id(id);
        self.ttps.iter().find(|ttp| ttp.id == id)
    }

    /// Find a tool TTP by its ID (e.g. `"curl"`, `"wget"`).
    /// Only returns TTPs that declare a `tool_slot`.
    pub fn get_tool_ttp(&self, id: &str) -> Option<&Ttp> {
        self.ttps
            .iter()
            .find(|t| t.tool_slot.is_some() && t.id == id)
    }

    /// Return all tool TTPs that fill the given slot (e.g. `"http-request"`).
    pub fn get_tools_for_slot(&self, slot: &str) -> Vec<&Ttp> {
        self.ttps
            .iter()
            .filter(|t| t.tool_slot.as_deref() == Some(slot))
            .collect()
    }

    pub fn ttps_for_tactic(&self, tactic: Option<&str>) -> Vec<Ttp> {
        let Some(tactic) = tactic.and_then(|t| {
            let trimmed = t.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }) else {
            return self.ttps.clone();
        };

        self.ttps
            .iter()
            .filter(|ttp| ttp.tactic.eq_ignore_ascii_case(tactic))
            .cloned()
            .collect()
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// For each non-tool TTP, replace any procedure whose `tool` field names a
    /// known slot (e.g. `"http-request"`) with one cloned procedure per
    /// concrete tool that fills that slot.  The clone receives the concrete
    /// tool's ID as both its `id` and `tool` field.
    fn expand_slot_procedures(ttps: &mut [Ttp]) {
        // Build slot → [concrete tool IDs] map from tool TTPs.
        let mut slot_map: HashMap<String, Vec<String>> = HashMap::new();
        for ttp in ttps.iter() {
            if let Some(slot) = &ttp.tool_slot {
                slot_map
                    .entry(slot.clone())
                    .or_default()
                    .push(ttp.id.clone());
            }
        }

        if slot_map.is_empty() {
            return;
        }

        for ttp in ttps.iter_mut() {
            if ttp.tool_slot.is_some() {
                continue; // tool TTPs themselves are never expanded
            }

            let original = std::mem::take(&mut ttp.procedures);
            let mut expanded: Vec<Procedure> = Vec::with_capacity(original.len());

            for proc in original {
                let slot_tools = proc.tool.as_deref().and_then(|t| slot_map.get(t));

                match slot_tools {
                    Some(tool_ids) => {
                        for tool_id in tool_ids {
                            let mut p = proc.clone();
                            p.id = tool_id.clone();
                            p.tool = Some(tool_id.clone());
                            expanded.push(p);
                        }
                    }
                    None => expanded.push(proc),
                }
            }

            ttp.procedures = expanded;
        }
    }

    fn validate_k8s_auth(ttps: &[Ttp]) -> Result<(), ArmoryError> {
        for ttp in ttps {
            let procedures = ttp.procedures.iter().chain(ttp.cleanup.iter());
            let uses_k8s_auth = procedures.clone().any(|procedure| {
                procedure.k8s_request.is_some()
                    || procedure.command.contains("${K8S_AUTH}")
                    || procedure
                        .http_request
                        .as_ref()
                        .and_then(|request| request.get("authentication"))
                        .is_some()
                    || procedure.command.contains("kubectl ")
                    || procedure
                        .command
                        .trim_start()
                        .starts_with("c2.kubectl_exec(")
                    || procedure
                        .command
                        .trim_start()
                        .starts_with("k8sSelfSubjectRulesReview(")
            });
            if !uses_k8s_auth {
                continue;
            }

            let references_k8s_auth = procedures.clone().any(|procedure| {
                procedure.command.contains("${K8S_AUTH}")
                    || procedure
                        .k8s_request
                        .as_ref()
                        .is_some_and(|request| request.to_string().contains("${K8S_AUTH}"))
                    || procedure
                        .http_request
                        .as_ref()
                        .is_some_and(|request| request.to_string().contains("${K8S_AUTH}"))
            });
            if references_k8s_auth {
                let Some(auth_param) = ttp
                    .params
                    .iter()
                    .find(|param| param.name.eq_ignore_ascii_case("K8S_AUTH"))
                else {
                    return Err(ArmoryError::InvalidTtp {
                        ttp_id: ttp.id.clone(),
                        reason: "TTPs that reference ${K8S_AUTH} must explicitly declare a K8S_AUTH parameter of type K8sAuth"
                            .to_string(),
                    });
                };
                if !auth_param.param_type.eq_ignore_ascii_case("K8sAuth") {
                    return Err(ArmoryError::InvalidTtp {
                        ttp_id: ttp.id.clone(),
                        reason: "K8S_AUTH must use parameter type K8sAuth".to_string(),
                    });
                }
            }

            if ttp
                .params
                .iter()
                .any(|param| param.name.eq_ignore_ascii_case("TOKEN"))
            {
                return Err(ArmoryError::InvalidTtp {
                    ttp_id: ttp.id.clone(),
                    reason:
                        "Kubernetes authentication must use Authenticate As, not a TOKEN parameter"
                            .to_string(),
                });
            }

            for procedure in ttp.procedures.iter().chain(ttp.cleanup.iter()) {
                let serialized_k8s_request = procedure
                    .k8s_request
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                let serialized_http_request = procedure
                    .http_request
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                if procedure.command.contains("${TOKEN}")
                    || procedure.command.contains("$TOKEN")
                    || procedure.command.contains("--token")
                    || serialized_k8s_request.contains("${TOKEN}")
                    || serialized_http_request.contains("${TOKEN}")
                    || procedure
                        .k8s_request
                        .as_ref()
                        .and_then(|request| request.get("token"))
                        .is_some()
                {
                    return Err(ArmoryError::InvalidTtp {
                        ttp_id: ttp.id.clone(),
                        reason: format!(
                            "Kubernetes procedure '{}' uses legacy TOKEN authentication; use authentication: ${{K8S_AUTH}} for structured requests or ${{K8S_AUTH}} for kubectl",
                            procedure.id
                        ),
                    });
                }
                if let Some(request) = &procedure.http_request {
                    if request.get("authentication").is_some()
                        && request
                            .get("authentication")
                            .and_then(|value| value.as_str())
                            != Some("${K8S_AUTH}")
                    {
                        return Err(ArmoryError::InvalidTtp {
                            ttp_id: ttp.id.clone(),
                            reason: format!(
                                "authenticated http_request procedure '{}' must declare authentication: ${{K8S_AUTH}}",
                                procedure.id
                            ),
                        });
                    }
                    if request.get("authentication").is_some()
                        && request
                            .get("headers")
                            .and_then(|headers| headers.as_object())
                            .is_some_and(|headers| {
                                headers
                                    .keys()
                                    .any(|name| name.eq_ignore_ascii_case("authorization"))
                            })
                    {
                        return Err(ArmoryError::InvalidTtp {
                            ttp_id: ttp.id.clone(),
                            reason: format!(
                                "authenticated http_request procedure '{}' must not declare an Authorization header; Authenticate As supplies it",
                                procedure.id
                            ),
                        });
                    }
                }
                if let Some(request) = &procedure.k8s_request {
                    if request
                        .get("authentication")
                        .and_then(|value| value.as_str())
                        != Some("${K8S_AUTH}")
                    {
                        return Err(ArmoryError::InvalidTtp {
                            ttp_id: ttp.id.clone(),
                            reason: format!(
                                "k8s_request procedure '{}' must declare authentication: ${{K8S_AUTH}}",
                                procedure.id
                            ),
                        });
                    }
                }
                if procedure.command.contains("kubectl ")
                    && !procedure.command.contains("${K8S_AUTH}")
                {
                    return Err(ArmoryError::InvalidTtp {
                        ttp_id: ttp.id.clone(),
                        reason: format!(
                            "kubectl procedure '{}' must include ${{K8S_AUTH}}",
                            procedure.id
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    fn ttps_from_dir(dir: &Path) -> Result<Vec<Ttp>, ArmoryError> {
        if !dir.exists() {
            return Err(ArmoryError::DirNotFound(dir.display().to_string()));
        }

        let mut ttps = Vec::new();

        for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }

            let file_path = entry.path();
            let is_yaml = file_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"))
                .unwrap_or(false);
            if !is_yaml {
                continue;
            }

            let raw = fs::read_to_string(file_path).map_err(|source| ArmoryError::ReadFile {
                path: file_path.display().to_string(),
                source,
            })?;

            let raw_ttp: RawTtp =
                serde_yaml::from_str(&raw).map_err(|source| ArmoryError::ParseYaml {
                    path: file_path.display().to_string(),
                    source,
                })?;

            if let Some(ttp) = raw_ttp.into_ttp(file_path) {
                ttps.push(ttp);
            }
        }

        Ok(ttps)
    }

    #[cfg(feature = "bundled-armory")]
    fn ttps_from_bundled() -> Result<Vec<Ttp>, ArmoryError> {
        let mut ttps = Vec::new();

        for filename in BundledTtps::iter() {
            let file = BundledTtps::get(filename.as_ref()).expect("file listed but not found");
            let raw = std::str::from_utf8(file.data.as_ref())
                .map_err(|_| ArmoryError::InvalidUtf8(filename.to_string()))?;
            let raw_ttp: RawTtp =
                serde_yaml::from_str(raw).map_err(|source| ArmoryError::ParseYaml {
                    path: filename.to_string(),
                    source,
                })?;
            if let Some(ttp) = raw_ttp.into_ttp(Path::new(filename.as_ref())) {
                ttps.push(ttp);
            }
        }

        Ok(ttps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_legacy_token_authentication_for_kubernetes_procedures() {
        let ttp = Ttp {
            params: vec![
                crate::TtpParam {
                    name: "K8S_AUTH".to_string(),
                    param_type: "K8sAuth".to_string(),
                    description: String::new(),
                    required: true,
                    default: String::new(),
                },
                crate::TtpParam {
                    name: "TOKEN".to_string(),
                    param_type: "ServiceAccount".to_string(),
                    description: String::new(),
                    required: false,
                    default: String::new(),
                },
            ],
            procedures: vec![Procedure::new(
                "kubectl",
                "kubectl get pods --token=${TOKEN}",
            )],
            ..Ttp::new("legacy-auth", "Legacy Auth", "Discovery")
        };

        assert!(matches!(
            Armory::validate_k8s_auth(&[ttp]),
            Err(ArmoryError::InvalidTtp { reason, .. }) if reason.contains("Authenticate As")
        ));
    }

    #[test]
    fn allows_explicit_tokens_for_non_kubernetes_apis() {
        let ttp = Ttp {
            params: vec![crate::TtpParam {
                name: "TOKEN".to_string(),
                param_type: "ServiceAccount".to_string(),
                description: String::new(),
                required: true,
                default: String::new(),
            }],
            procedures: vec![Procedure::new(
                "ran-ws",
                "ran-ws --url wss://node:10250/exec --token ${TOKEN}",
            )],
            ..Ttp::new("kubelet-auth", "Kubelet Auth", "Execution")
        };

        assert!(Armory::validate_k8s_auth(&[ttp]).is_ok());
    }

    #[test]
    fn rejects_k8s_request_without_explicit_authentication_marker() {
        let ttp = Ttp {
            params: vec![crate::TtpParam {
                name: "K8S_AUTH".to_string(),
                param_type: "K8sAuth".to_string(),
                description: String::new(),
                required: true,
                default: String::new(),
            }],
            procedures: vec![Procedure {
                k8s_request: Some(serde_json::json!({
                    "api_server": "https://cluster.example",
                    "api": "/api/v1",
                    "resource": "pods"
                })),
                ..Procedure::new("k8s-request", "")
            }],
            ..Ttp::new("hidden-auth", "Hidden Auth", "Discovery")
        };

        assert!(matches!(
            Armory::validate_k8s_auth(&[ttp]),
            Err(ArmoryError::InvalidTtp { reason, .. })
                if reason.contains("authentication: ${K8S_AUTH}")
        ));
    }

    #[test]
    fn rejects_kubernetes_ttp_without_declared_auth_parameter() {
        let ttp = Ttp {
            procedures: vec![Procedure::new("kubectl", "kubectl ${K8S_AUTH} get pods")],
            ..Ttp::new("implicit-auth", "Implicit Auth", "Discovery")
        };

        assert!(matches!(
            Armory::validate_k8s_auth(&[ttp]),
            Err(ArmoryError::InvalidTtp { reason, .. })
                if reason.contains("explicitly declare a K8S_AUTH parameter")
        ));
    }

    #[test]
    fn allows_local_active_client_procedure_without_unused_auth_parameter() {
        let ttp = Ttp {
            procedures: vec![Procedure {
                is_local_command: Some(true),
                ..Procedure::new("k8s-client", "k8sSelfSubjectRulesReview(default)")
            }],
            ..Ttp::new("native-review", "Native Review", "Discovery")
        };

        assert!(Armory::validate_k8s_auth(&[ttp]).is_ok());
    }

    #[test]
    fn rejects_ambient_kubectl_for_missing_auth_marker_not_parameter() {
        let ttp = Ttp {
            procedures: vec![Procedure::new("kubectl", "kubectl get pods")],
            ..Ttp::new("ambient-kubectl", "Ambient kubectl", "Discovery")
        };

        assert!(matches!(
            Armory::validate_k8s_auth(&[ttp]),
            Err(ArmoryError::InvalidTtp { reason, .. })
                if reason.contains("must include ${K8S_AUTH}")
                    && !reason.contains("parameter")
        ));
    }

    #[test]
    fn preserves_kubernetes_http_request_procedure_variants() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../armory/TTPs");
        let armory = Armory::load_from_dir(path).expect("repository armory should load");

        for ttp_id in ["check-token-permissions", "get-roles-via-api-server"] {
            let ttp = armory.get_ttp(ttp_id).expect("Kubernetes discovery TTP");
            assert_eq!(
                ttp.procedures.len(),
                2,
                "{ttp_id} must retain both kubectl and HTTP procedures"
            );
            assert!(
                ttp.procedures
                    .iter()
                    .any(|procedure| procedure.command.contains("kubectl ")),
                "{ttp_id} must retain its kubectl procedure"
            );
            assert!(
                ttp.procedures.iter().any(|procedure| {
                    procedure
                        .http_request
                        .as_ref()
                        .and_then(|request| request.get("authentication"))
                        .and_then(|value| value.as_str())
                        == Some("${K8S_AUTH}")
                }),
                "{ttp_id} must retain its authenticated HTTP request procedure"
            );
        }

        let node_proxy = armory
            .get_ttp("get-pods-via-node-proxy")
            .expect("node proxy discovery TTP");
        let procedure = node_proxy.procedures.first().expect("node proxy procedure");
        assert_eq!(procedure.id, "proc-1", "positional procedure ID is stable");
        let request = procedure
            .k8s_request
            .as_ref()
            .expect("node proxy remains a structured HTTP operation");
        assert_eq!(
            request.get("use_ca").and_then(|value| value.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn valid_accounts_ttp_is_canonical_and_old_id_is_an_alias() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../armory/TTPs");
        let armory = Armory::load_from_dir(path).expect("repository armory should load");

        let canonical = armory
            .get_ttp(VALID_ACCOUNTS_KUBECONFIG_ID)
            .expect("canonical Valid Accounts TTP");
        assert_eq!(canonical.name, "Execute into pod via Valid Account");
        assert_eq!(canonical.tactic, "Initial Access");
        assert_eq!(canonical.techniques, ["Valid Accounts", "T1078"]);
        assert_eq!(
            canonical.requires.get("kind").and_then(|v| v.as_str()),
            Some("Pod")
        );
        assert_eq!(
            canonical
                .requires
                .get("activeKubeconfig")
                .and_then(|v| v.as_bool()),
            Some(true)
        );

        let alias = armory
            .get_ttp(DEPRECATED_INITIAL_ACCESS_POD_EXEC_ID)
            .expect("deprecated ID should resolve");
        assert_eq!(alias.id, VALID_ACCOUNTS_KUBECONFIG_ID);
        assert_eq!(
            armory
                .ttps()
                .iter()
                .filter(|ttp| ttp.id == VALID_ACCOUNTS_KUBECONFIG_ID)
                .count(),
            1
        );
    }
}
