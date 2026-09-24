use campaign::ttp_applicability::{
    eligible_auth_identities, resolve_target_context, ttp_applicable_for_target,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionReadinessStatus {
    Inapplicable,
    Blocked,
    NeedsInput,
    NeedsChoice,
    Ready,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArgumentSummary {
    pub(crate) total: usize,
    pub(crate) resolved: usize,
    pub(crate) needs_input: usize,
    pub(crate) needs_choice: usize,
    pub(crate) blocked: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActionState {
    pub(crate) status: ActionReadinessStatus,
    pub(crate) reasons: Vec<String>,
    pub(crate) arguments: ArgumentSummary,
    pub(crate) procedures: Vec<ProcedureState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recommended_procedure_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActionResolution {
    pub(crate) action_id: String,
    pub(crate) target_id: String,
    pub(crate) status: ActionReadinessStatus,
    pub(crate) reasons: Vec<String>,
    pub(crate) arguments: Vec<ArgumentResolution>,
    pub(crate) procedures: Vec<ProcedureState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recommended_procedure_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProcedureReadinessStatus {
    Ready,
    Unknown,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProcedureState {
    pub(crate) procedure_id: String,
    pub(crate) status: ProcedureReadinessStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) required_tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArgumentResolutionStatus {
    Resolved,
    Defaulted,
    GeneratedAtExecution,
    NeedsInput,
    NeedsChoice,
    Blocked,
    Omitted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArgumentResolution {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) param_type: String,
    pub(crate) required: bool,
    pub(crate) status: ArgumentResolutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) value: Option<String>,
    pub(crate) candidates: Vec<ArgumentCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<BindingSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArgumentCandidate {
    pub(crate) value: String,
    pub(crate) label: String,
    pub(crate) source: BindingSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BindingSource {
    pub(crate) kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) expression: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ArmoryAction {
    #[serde(flatten)]
    pub(crate) ttp: armory::Ttp,
    #[serde(rename = "actionState", skip_serializing_if = "Option::is_none")]
    pub(crate) action_state: Option<ActionState>,
}

impl ArmoryAction {
    pub(crate) fn static_action(ttp: armory::Ttp) -> Self {
        Self {
            ttp,
            action_state: None,
        }
    }
}

pub(crate) fn resolve_action(
    ttp: &armory::Ttp,
    campaign: &campaign::Campaign,
    target_id: &str,
    exec_system_id: Option<&str>,
) -> Option<ActionResolution> {
    let target = campaign
        .get_entities()
        .into_iter()
        .find(|entity| entity.entity_id().0 == target_id)?;
    let target_context = resolve_target_context(campaign, target_id)?;
    let applicable = !ttp.status.eq_ignore_ascii_case("disabled")
        && ttp_applicable_for_target(ttp, campaign, &target_context);

    let arguments = ttp
        .params
        .iter()
        .map(|param| resolve_argument(param, ttp, campaign, &target, target_id))
        .collect::<Vec<_>>();
    let procedures = ttp
        .procedures
        .iter()
        .map(|procedure| resolve_procedure(ttp, procedure, campaign, target_id, exec_system_id))
        .collect::<Vec<_>>();
    let recommended_procedure_id =
        campaign::recommended_procedure(ttp, campaign, target_id, exec_system_id)
            .map(|procedure| procedure.id.clone());

    let mut reasons = Vec::new();
    let status = if !applicable {
        reasons.push("Action prerequisites are not satisfied for this target".to_string());
        ActionReadinessStatus::Inapplicable
    } else if arguments
        .iter()
        .any(|argument| argument.status == ArgumentResolutionStatus::Blocked)
    {
        reasons.extend(argument_reasons(
            &arguments,
            ArgumentResolutionStatus::Blocked,
        ));
        ActionReadinessStatus::Blocked
    } else if arguments
        .iter()
        .any(|argument| argument.status == ArgumentResolutionStatus::NeedsChoice)
    {
        reasons.extend(argument_reasons(
            &arguments,
            ArgumentResolutionStatus::NeedsChoice,
        ));
        ActionReadinessStatus::NeedsChoice
    } else if arguments
        .iter()
        .any(|argument| argument.status == ArgumentResolutionStatus::NeedsInput)
    {
        reasons.extend(argument_reasons(
            &arguments,
            ArgumentResolutionStatus::NeedsInput,
        ));
        ActionReadinessStatus::NeedsInput
    } else {
        ActionReadinessStatus::Ready
    };

    Some(ActionResolution {
        action_id: ttp.id.clone(),
        target_id: target_id.to_string(),
        status,
        reasons,
        arguments,
        procedures,
        recommended_procedure_id,
    })
}

pub(crate) fn summarize(resolution: &ActionResolution) -> ActionState {
    let mut arguments = ArgumentSummary {
        total: resolution.arguments.len(),
        ..ArgumentSummary::default()
    };
    for argument in &resolution.arguments {
        match argument.status {
            ArgumentResolutionStatus::Blocked => arguments.blocked += 1,
            ArgumentResolutionStatus::NeedsChoice => arguments.needs_choice += 1,
            ArgumentResolutionStatus::NeedsInput => arguments.needs_input += 1,
            _ => arguments.resolved += 1,
        }
    }
    ActionState {
        status: resolution.status,
        reasons: resolution.reasons.clone(),
        arguments,
        procedures: resolution.procedures.clone(),
        recommended_procedure_id: resolution.recommended_procedure_id.clone(),
    }
}

fn resolve_procedure(
    ttp: &armory::Ttp,
    procedure: &armory::Procedure,
    campaign: &campaign::Campaign,
    target_id: &str,
    exec_system_id: Option<&str>,
) -> ProcedureState {
    let required_tool = campaign::procedure_required_tool(procedure).map(str::to_string);
    let readiness =
        campaign::procedure_readiness(ttp, procedure, campaign, target_id, exec_system_id);
    let (status, reason) = match readiness {
        campaign::ProcedureReadiness::Ready => (ProcedureReadinessStatus::Ready, None),
        campaign::ProcedureReadiness::Unknown => (
            ProcedureReadinessStatus::Unknown,
            required_tool.as_ref().map(|tool| {
                format!("required tool '{tool}' has not been observed on the execution system")
            }),
        ),
        campaign::ProcedureReadiness::Unavailable => (
            ProcedureReadinessStatus::Unavailable,
            required_tool.as_ref().map(|tool| {
                format!("required tool '{tool}' is known to be absent from the execution system")
            }),
        ),
    };
    ProcedureState {
        procedure_id: procedure.id.clone(),
        status,
        required_tool,
        reason,
    }
}

fn argument_reasons(
    arguments: &[ArgumentResolution],
    status: ArgumentResolutionStatus,
) -> Vec<String> {
    arguments
        .iter()
        .filter(|argument| argument.status == status)
        .filter_map(|argument| argument.reason.clone())
        .collect()
}

fn resolve_argument(
    param: &armory::TtpParam,
    ttp: &armory::Ttp,
    campaign: &campaign::Campaign,
    target: &campaign::CampaignEntityRef<'_>,
    target_id: &str,
) -> ArgumentResolution {
    let base = || ArgumentResolution {
        name: param.name.clone(),
        param_type: param.param_type.clone(),
        required: param.required,
        status: ArgumentResolutionStatus::Resolved,
        value: None,
        candidates: Vec::new(),
        source: None,
        reason: None,
    };

    if !param.default.trim().is_empty() {
        return resolve_default(param, ttp, campaign, target, target_id, base());
    }

    if let Some(binding) =
        campaign::grounding::resolve_runtime_argument(&param.name, "", target_id, campaign)
    {
        let source_kind = if binding.source().is_default() {
            "runtime_default"
        } else {
            "target_fact"
        };
        let status = if binding.source().is_sensitive() {
            ArgumentResolutionStatus::GeneratedAtExecution
        } else if binding.source().is_default() {
            ArgumentResolutionStatus::Defaulted
        } else {
            ArgumentResolutionStatus::Resolved
        };
        return ArgumentResolution {
            status,
            value: binding.readiness_value().map(str::to_string),
            source: Some(source(
                source_kind,
                (!binding.source().is_default()).then(|| target_id.to_string()),
                Some(binding.source().field().to_string()),
                None,
            )),
            ..base()
        };
    }

    let candidates = candidates_for_param(param, ttp, campaign, target_id);
    match candidates.as_slice() {
        [candidate] => ArgumentResolution {
            status: ArgumentResolutionStatus::Resolved,
            value: Some(candidate.value.clone()),
            source: Some(candidate.source.clone()),
            candidates,
            ..base()
        },
        [] if param.param_type.eq_ignore_ascii_case("K8sAuth")
            && can_use_default_kubeconfig(ttp) =>
        {
            ArgumentResolution {
                status: ArgumentResolutionStatus::Defaulted,
                source: Some(source(
                    "runtime_default",
                    None,
                    Some("kubeconfig".to_string()),
                    None,
                )),
                ..base()
            }
        }
        [] if !param.options.is_empty() => from_candidates(param, option_candidates(param), base()),
        [] if !param.required => ArgumentResolution {
            status: ArgumentResolutionStatus::Omitted,
            source: Some(source("optional", None, None, None)),
            ..base()
        },
        [] if param.param_type.eq_ignore_ascii_case("Session") => ArgumentResolution {
            status: ArgumentResolutionStatus::Blocked,
            reason: Some(format!(
                "{} requires a live c2.session channel, but none is available",
                param.name
            )),
            ..base()
        },
        [] if is_entity_param(&param.param_type) => ArgumentResolution {
            status: ArgumentResolutionStatus::Blocked,
            reason: Some(format!(
                "{} requires a {} entity, but none is available",
                param.name, param.param_type
            )),
            ..base()
        },
        [] => ArgumentResolution {
            status: ArgumentResolutionStatus::NeedsInput,
            reason: Some(format!("{} requires operator input", param.name)),
            ..base()
        },
        _ => from_candidates(param, candidates, base()),
    }
}

fn resolve_default(
    param: &armory::TtpParam,
    ttp: &armory::Ttp,
    campaign: &campaign::Campaign,
    target: &campaign::CampaignEntityRef<'_>,
    target_id: &str,
    base: ArgumentResolution,
) -> ArgumentResolution {
    let default = param.default.clone();
    if default.contains("${RANDOM}") {
        return ArgumentResolution {
            status: ArgumentResolutionStatus::GeneratedAtExecution,
            value: Some(default.clone()),
            source: Some(source(
                "generated",
                None,
                None,
                Some("${RANDOM}".to_string()),
            )),
            ..base
        };
    }

    if default == "${TARGET}" {
        let use_name = param.param_type.eq_ignore_ascii_case("string");
        return ArgumentResolution {
            status: ArgumentResolutionStatus::Resolved,
            value: Some(if use_name {
                target.entity_name().to_string()
            } else {
                target_id.to_string()
            }),
            source: Some(source(
                "target",
                Some(target_id.to_string()),
                Some(if use_name { "name" } else { "id" }.to_string()),
                Some(default),
            )),
            ..base
        };
    }

    if default.contains("${TARGET.IP}") {
        let ips = campaign
            .get_system_entity(target_id)
            .map(|system| {
                system
                    .entity()
                    .system()
                    .ips
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let candidates = ips
            .into_iter()
            .map(|ip| ArgumentCandidate {
                value: default.replace("${TARGET.IP}", &ip),
                label: ip,
                source: source(
                    "target_fact",
                    Some(target_id.to_string()),
                    Some("system.ips".to_string()),
                    Some("${TARGET.IP}".to_string()),
                ),
            })
            .collect::<Vec<_>>();
        return match candidates.as_slice() {
            [candidate] => ArgumentResolution {
                status: ArgumentResolutionStatus::Resolved,
                value: Some(candidate.value.clone()),
                source: Some(candidate.source.clone()),
                candidates,
                ..base
            },
            [] => ArgumentResolution {
                status: ArgumentResolutionStatus::NeedsInput,
                reason: Some(format!(
                    "{} needs input because the target has no known IP",
                    param.name
                )),
                ..base
            },
            _ => ArgumentResolution {
                status: ArgumentResolutionStatus::NeedsChoice,
                candidates,
                reason: Some(format!(
                    "{} has multiple values derived from TARGET.IP",
                    param.name
                )),
                ..base
            },
        };
    }

    if default == "${NS}" || default == "${NAMESPACE}" {
        return match target.namespace() {
            Some(namespace) => ArgumentResolution {
                status: ArgumentResolutionStatus::Resolved,
                value: Some(namespace.to_string()),
                source: Some(source(
                    "target_fact",
                    Some(target_id.to_string()),
                    Some("namespace".to_string()),
                    Some(default),
                )),
                ..base
            },
            None => ArgumentResolution {
                status: ArgumentResolutionStatus::NeedsInput,
                reason: Some(format!(
                    "{} needs input because the target namespace is not known",
                    param.name
                )),
                ..base
            },
        };
    }

    if default == "${IXIMIUZ_PLAY_ID}" {
        return match std::env::var("IXIMIUZ_PLAY_ID") {
            Ok(value) if !value.trim().is_empty() => ArgumentResolution {
                status: ArgumentResolutionStatus::Resolved,
                value: Some(value),
                source: Some(source(
                    "environment",
                    None,
                    Some("IXIMIUZ_PLAY_ID".to_string()),
                    Some(default),
                )),
                ..base
            },
            _ => ArgumentResolution {
                status: ArgumentResolutionStatus::NeedsInput,
                reason: Some(format!(
                    "{} needs a value because IXIMIUZ_PLAY_ID is not set",
                    param.name
                )),
                ..base
            },
        };
    }

    if default.contains("${LISTENER")
        && ttp
            .params
            .iter()
            .any(|item| item.param_type.eq_ignore_ascii_case("Listener"))
    {
        return ArgumentResolution {
            status: ArgumentResolutionStatus::GeneratedAtExecution,
            value: Some(default.clone()),
            source: Some(source(
                "argument_dependency",
                None,
                Some("Listener".to_string()),
                Some(default),
            )),
            ..base
        };
    }

    if default.contains("${") {
        return ArgumentResolution {
            status: ArgumentResolutionStatus::NeedsInput,
            reason: Some(format!(
                "{} needs input because its default could not be resolved: {}",
                param.name, default
            )),
            ..base
        };
    }

    ArgumentResolution {
        status: ArgumentResolutionStatus::Defaulted,
        value: Some(default.clone()),
        source: Some(source("yaml_default", None, None, Some(default))),
        ..base
    }
}

fn from_candidates(
    param: &armory::TtpParam,
    candidates: Vec<ArgumentCandidate>,
    base: ArgumentResolution,
) -> ArgumentResolution {
    match candidates.as_slice() {
        [candidate] => ArgumentResolution {
            status: ArgumentResolutionStatus::Resolved,
            value: Some(candidate.value.clone()),
            source: Some(candidate.source.clone()),
            candidates,
            ..base
        },
        _ => ArgumentResolution {
            status: ArgumentResolutionStatus::NeedsChoice,
            candidates,
            reason: Some(format!("{} requires a choice", param.name)),
            ..base
        },
    }
}

fn option_candidates(param: &armory::TtpParam) -> Vec<ArgumentCandidate> {
    param
        .options
        .iter()
        .map(|option| ArgumentCandidate {
            value: option.clone(),
            label: option.clone(),
            source: source("yaml_option", None, Some(param.name.clone()), None),
        })
        .collect()
}

fn candidates_for_param(
    param: &armory::TtpParam,
    ttp: &armory::Ttp,
    campaign: &campaign::Campaign,
    target_id: &str,
) -> Vec<ArgumentCandidate> {
    if param.param_type.eq_ignore_ascii_case("K8sAuth") {
        return eligible_auth_identities(ttp, campaign, target_id)
            .into_iter()
            .map(|identity| ArgumentCandidate {
                value: identity.id.clone(),
                label: identity.name,
                source: source(
                    "entity",
                    Some(identity.id),
                    Some("authentication_identity".to_string()),
                    None,
                ),
            })
            .collect();
    }
    if param.param_type.eq_ignore_ascii_case("Session") {
        let mut candidates = campaign
            .get_relations()
            .into_iter()
            .filter(|relation| {
                relation.name == "c2.session" && relation.source_id == target_id && !relation.broken
            })
            .filter_map(|relation| {
                let session_id = relation.session_id?;
                Some(ArgumentCandidate {
                    value: session_id.clone(),
                    label: format!("{} ({session_id})", relation.target_id),
                    source: source(
                        "relation",
                        Some(session_id),
                        Some("c2.session".to_string()),
                        None,
                    ),
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|a, b| a.label.cmp(&b.label).then(a.value.cmp(&b.value)));
        return candidates;
    }
    if !is_entity_param(&param.param_type) {
        return Vec::new();
    }

    let mut candidates = campaign
        .get_entities()
        .into_iter()
        .filter(|entity| entity.entity_kind().eq_ignore_ascii_case(&param.param_type))
        .map(|entity| ArgumentCandidate {
            value: entity.entity_id().0.clone(),
            label: entity.entity_name().to_string(),
            source: source(
                "entity",
                Some(entity.entity_id().0),
                Some("id".to_string()),
                None,
            ),
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.label.cmp(&b.label).then(a.value.cmp(&b.value)));
    candidates
}

fn is_entity_param(param_type: &str) -> bool {
    matches!(
        param_type.to_ascii_lowercase().as_str(),
        "k8sauth"
            | "listener"
            | "redirector"
            | "pod"
            | "serviceaccount"
            | "namespace"
            | "node"
            | "deployment"
            | "secret"
            | "configmap"
            | "k8scredential"
    )
}

fn can_use_default_kubeconfig(ttp: &armory::Ttp) -> bool {
    ttp.procedures.iter().any(|procedure| {
        procedure.is_local_command == Some(true)
            && procedure.command.contains("kubectl ")
            && !procedure.command.contains("${K8S_AUTH}")
    })
}

fn source(
    kind: &str,
    entity_id: Option<String>,
    field: Option<String>,
    expression: Option<String>,
) -> BindingSource {
    BindingSource {
        kind: kind.to_string(),
        entity_id,
        field,
        expression,
    }
}

#[cfg(test)]
mod tests {
    use ran_domain::{
        AccessLevel, BinaryPresence, Entity, JwToken, K8sCluster, Pod, ServiceAccount,
        ServiceAccountToken, SessionChannel, UnknownSystem,
    };

    use super::*;

    #[test]
    fn target_ip_default_is_resolved_with_provenance() {
        let mut campaign = campaign::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut target = UnknownSystem::new("target");
        target.system.access_level = AccessLevel::Exec;
        target.system.ips.push("10.23.4.5".parse().unwrap());
        let target_id = target.entity_id().0;
        campaign.upsert_entity(target, campaign::KnowledgeProvenance::Scenario);

        let mut ttp = armory::Ttp::new("scan", "Scan", "Discovery");
        ttp.params.push(armory::TtpParam {
            name: "CIDR".to_string(),
            param_type: "string".to_string(),
            description: String::new(),
            required: true,
            default: "${TARGET.IP}/24".to_string(),
            options: Vec::new(),
        });

        let resolution = resolve_action(&ttp, &campaign, &target_id, None).unwrap();
        assert_eq!(resolution.status, ActionReadinessStatus::Ready);
        assert_eq!(
            resolution.arguments[0].value.as_deref(),
            Some("10.23.4.5/24")
        );
        assert_eq!(
            resolution.arguments[0]
                .source
                .as_ref()
                .and_then(|source| source.field.as_deref()),
            Some("system.ips")
        );
    }

    #[test]
    fn multiple_target_ips_require_a_choice() {
        let mut campaign = campaign::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut target = UnknownSystem::new("target");
        target.system.access_level = AccessLevel::Exec;
        target.system.ips.push("10.23.4.5".parse().unwrap());
        target.system.ips.push("192.0.2.2".parse().unwrap());
        let target_id = target.entity_id().0;
        campaign.upsert_entity(target, campaign::KnowledgeProvenance::Scenario);

        let mut ttp = armory::Ttp::new("scan", "Scan", "Discovery");
        ttp.params.push(armory::TtpParam {
            name: "CIDR".to_string(),
            param_type: "string".to_string(),
            description: String::new(),
            required: true,
            default: "${TARGET.IP}/24".to_string(),
            options: Vec::new(),
        });

        let resolution = resolve_action(&ttp, &campaign, &target_id, None).unwrap();
        assert_eq!(resolution.status, ActionReadinessStatus::NeedsChoice);
        assert_eq!(resolution.arguments[0].candidates.len(), 2);
    }

    #[test]
    fn api_server_runtime_default_matches_execution_grounding() {
        let mut campaign = campaign::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut target = UnknownSystem::new("target");
        target.system.access_level = AccessLevel::Exec;
        let target_id = target.entity_id().0;
        campaign.upsert_entity(target, campaign::KnowledgeProvenance::Scenario);

        let mut ttp = armory::Ttp::new("permissions", "Permissions", "Discovery");
        ttp.params.push(armory::TtpParam {
            name: "API_SERVER".to_string(),
            param_type: "string".to_string(),
            description: String::new(),
            required: true,
            default: String::new(),
            options: Vec::new(),
        });

        let resolution = resolve_action(&ttp, &campaign, &target_id, None).unwrap();
        assert_eq!(resolution.status, ActionReadinessStatus::Ready);
        assert_eq!(
            resolution.arguments[0].status,
            ArgumentResolutionStatus::Defaulted
        );
        assert_eq!(
            resolution.arguments[0].value.as_deref(),
            Some(campaign::grounding::DEFAULT_API_SERVER)
        );
        assert_eq!(
            resolution.arguments[0]
                .source
                .as_ref()
                .map(|source| source.kind.as_str()),
            Some("runtime_default")
        );

        let mut args = std::collections::HashMap::new();
        campaign::grounding::ground_args_from_context(&mut args, &target_id, &campaign);
        assert_eq!(
            args.get("API_SERVER").map(String::as_str),
            resolution.arguments[0].value.as_deref()
        );
    }

    #[test]
    fn target_runtime_bindings_are_ready_without_exposing_tokens() {
        let mut campaign = campaign::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut pod = Pod::new("runner", "workloads");
        pod.system.access_level = AccessLevel::Exec;
        pod.node_name = Some("worker-a".to_string());
        pod.host_ip = Some("10.0.0.8".parse().unwrap());
        pod.service_account_name = Some("runner-sa".to_string());
        let target_id = pod.entity_id().0;
        campaign.upsert_entity(pod, campaign::KnowledgeProvenance::Scenario);

        let mut service_account = ServiceAccount::new("runner-sa", "workloads");
        service_account.token = Some(ServiceAccountToken {
            jwt: JwToken {
                raw: "ey.runtime.secret".to_string(),
                ..Default::default()
            },
            service_account_name: "runner-sa".to_string(),
            namespace: "workloads".to_string(),
            pod_name: None,
            pod_uid: None,
            service_account_uid: None,
            is_bound: false,
        });
        campaign.upsert_entity(service_account, campaign::KnowledgeProvenance::Scenario);

        let mut ttp = armory::Ttp::new("context", "Context", "Discovery");
        for name in ["NS", "POD_NAME", "NODE", "NODE.IP", "NODE.NAME", "TOKEN"] {
            ttp.params.push(armory::TtpParam {
                name: name.to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: true,
                default: String::new(),
                options: Vec::new(),
            });
        }

        let resolution = resolve_action(&ttp, &campaign, &target_id, None).unwrap();
        assert_eq!(resolution.status, ActionReadinessStatus::Ready);
        let value = |name: &str| {
            resolution
                .arguments
                .iter()
                .find(|argument| argument.name == name)
                .and_then(|argument| argument.value.as_deref())
        };
        assert_eq!(value("NS"), Some("workloads"));
        assert_eq!(value("POD_NAME"), Some("runner"));
        assert_eq!(value("NODE"), Some("10.0.0.8"));
        assert_eq!(value("NODE.IP"), Some("10.0.0.8"));
        assert_eq!(value("NODE.NAME"), Some("worker-a"));

        let token = resolution
            .arguments
            .iter()
            .find(|argument| argument.name == "TOKEN")
            .expect("TOKEN resolution");
        assert_eq!(token.status, ArgumentResolutionStatus::GeneratedAtExecution);
        assert!(token.value.is_none());
        assert!(!format!("{resolution:?}").contains("ey.runtime.secret"));

        let mut args = std::collections::HashMap::new();
        campaign::grounding::ground_args_from_context(&mut args, &target_id, &campaign);
        assert_eq!(
            args.get("TOKEN").map(String::as_str),
            Some("ey.runtime.secret")
        );
    }

    #[test]
    fn session_parameter_resolves_live_edges_of_the_selected_c2() {
        let mut campaign = campaign::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        campaign.upsert_relation(
            &SessionChannel::new("c2/ran", "node/victim", "session/victim-4444"),
            campaign::KnowledgeProvenance::Scenario,
        );
        let mut ttp = armory::Ttp::new("kill-session", "Kill Session", "Resource Development");
        ttp.params.push(armory::TtpParam {
            name: "SessionID".to_string(),
            param_type: "Session".to_string(),
            description: String::new(),
            required: true,
            default: String::new(),
            options: Vec::new(),
        });

        let resolution = resolve_action(&ttp, &campaign, "c2/ran", None).expect("C2 is a target");
        assert_eq!(resolution.status, ActionReadinessStatus::Ready);
        assert_eq!(
            resolution.arguments[0].value.as_deref(),
            Some("session/victim-4444")
        );
        assert_eq!(
            resolution.arguments[0].candidates[0].label,
            "node/victim (session/victim-4444)"
        );
    }

    #[test]
    fn procedure_readiness_and_recommendation_are_resolved_by_the_backend() {
        let mut campaign = campaign::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut target = UnknownSystem::new("target");
        target.system.access_level = AccessLevel::Exec;
        target
            .system
            .binaries
            .insert("ip".to_string(), BinaryPresence::Absent);
        target.system.binaries.insert(
            "hostname".to_string(),
            BinaryPresence::Present("/bin/hostname".into()),
        );
        let target_id = target.entity_id().0;
        campaign.upsert_entity(target, campaign::KnowledgeProvenance::Scenario);

        let ttp = armory::Ttp {
            procedures: vec![
                armory::Procedure {
                    tool: Some("ip".to_string()),
                    ..armory::Procedure::new("ip", "ip address")
                },
                armory::Procedure {
                    tool: Some("hostname".to_string()),
                    ..armory::Procedure::new("hostname", "hostname -i")
                },
            ],
            ..armory::Ttp::new("local-ip", "Local IP", "Discovery")
        };

        let resolution = resolve_action(&ttp, &campaign, &target_id, None).unwrap();

        assert_eq!(resolution.procedures.len(), 2);
        assert_eq!(
            resolution.procedures[0].status,
            ProcedureReadinessStatus::Unavailable
        );
        assert_eq!(
            resolution.procedures[1].status,
            ProcedureReadinessStatus::Ready
        );
        assert_eq!(
            resolution.recommended_procedure_id.as_deref(),
            Some("hostname")
        );
    }
}
