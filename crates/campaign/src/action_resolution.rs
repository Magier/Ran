use crate::ttp_applicability::{
    eligible_auth_identities, resolve_target_context, ttp_applicable_for_target,
};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct ActionResolutionInput {
    pub args: HashMap<String, String>,
    pub auth_identity_id: Option<String>,
    pub procedure_id: Option<String>,
    pub exec_system_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionReadinessStatus {
    Inapplicable,
    Blocked,
    NeedsInput,
    NeedsChoice,
    Ready,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArgumentSummary {
    pub total: usize,
    pub resolved: usize,
    pub needs_input: usize,
    pub needs_choice: usize,
    pub blocked: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionState {
    pub status: ActionReadinessStatus,
    pub reasons: Vec<String>,
    pub arguments: ArgumentSummary,
    pub procedures: Vec<ProcedureState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_procedure_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResolution {
    pub action_id: String,
    pub target_id: String,
    pub status: ActionReadinessStatus,
    pub reasons: Vec<String>,
    pub arguments: Vec<ArgumentResolution>,
    pub procedures: Vec<ProcedureState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_procedure_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcedureReadinessStatus {
    Ready,
    Unknown,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcedureState {
    pub procedure_id: String,
    pub status: ProcedureReadinessStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentResolutionStatus {
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
pub struct ArgumentResolution {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub required: bool,
    pub status: ArgumentResolutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub candidates: Vec<ArgumentCandidate>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<BindingSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArgumentCandidate {
    pub value: String,
    pub label: String,
    pub source: BindingSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingSource {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmoryAction {
    #[serde(flatten)]
    pub ttp: armory::Ttp,
    #[serde(rename = "actionState", skip_serializing_if = "Option::is_none")]
    pub action_state: Option<ActionState>,
}

impl ArmoryAction {
    pub fn static_action(ttp: armory::Ttp) -> Self {
        Self {
            ttp,
            action_state: None,
        }
    }
}

pub fn resolve_action(
    ttp: &armory::Ttp,
    campaign: &crate::Campaign,
    target_id: &str,
    input: &ActionResolutionInput,
) -> Option<ActionResolution> {
    let target = campaign
        .get_entities()
        .into_iter()
        .find(|entity| entity.entity_id().0 == target_id)?;
    let target_context = resolve_target_context(campaign, target_id)?;
    let applicable = !ttp.status.eq_ignore_ascii_case("disabled")
        && ttp_applicable_for_target(ttp, campaign, &target_context);

    let mut arguments = ttp
        .params
        .iter()
        .map(|param| resolve_argument(param, ttp, campaign, &target, target_id, input))
        .collect::<Vec<_>>();
    finalize_argument_dependencies(&mut arguments, ttp, campaign, target_id, input);
    let procedures = ttp
        .procedures
        .iter()
        .map(|procedure| {
            resolve_procedure(
                ttp,
                procedure,
                campaign,
                target_id,
                input.exec_system_id.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    let selected_procedure = input.procedure_id.as_deref().and_then(|procedure_id| {
        procedures
            .iter()
            .find(|procedure| procedure.procedure_id == procedure_id)
    });
    let selected_procedure_unavailable = selected_procedure
        .is_some_and(|procedure| procedure.status == ProcedureReadinessStatus::Unavailable);
    let recommended_procedure_id = selected_procedure
        .filter(|procedure| procedure.status != ProcedureReadinessStatus::Unavailable)
        .map(|procedure| procedure.procedure_id.clone())
        .or_else(|| {
            crate::recommended_procedure(ttp, campaign, target_id, input.exec_system_id.as_deref())
                .map(|procedure| procedure.id.clone())
        });

    let mut reasons = Vec::new();
    let status = if !applicable {
        reasons.push("Action prerequisites are not satisfied for this target".to_string());
        ActionReadinessStatus::Inapplicable
    } else if selected_procedure_unavailable {
        reasons.push(
            selected_procedure
                .and_then(|procedure| procedure.reason.clone())
                .unwrap_or_else(|| "Selected procedure is unavailable".to_string()),
        );
        ActionReadinessStatus::Blocked
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

pub fn summarize(resolution: &ActionResolution) -> ActionState {
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
    campaign: &crate::Campaign,
    target_id: &str,
    exec_system_id: Option<&str>,
) -> ProcedureState {
    let required_tool = crate::procedure_required_tool(procedure).map(str::to_string);
    let readiness = crate::procedure_readiness(ttp, procedure, campaign, target_id, exec_system_id);
    let (status, reason) = match readiness {
        crate::ProcedureReadiness::Ready => (ProcedureReadinessStatus::Ready, None),
        crate::ProcedureReadiness::Unknown => (
            ProcedureReadinessStatus::Unknown,
            required_tool.as_ref().map(|tool| {
                format!("required tool '{tool}' has not been observed on the execution system")
            }),
        ),
        crate::ProcedureReadiness::Unavailable => (
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

fn finalize_argument_dependencies(
    arguments: &mut [ArgumentResolution],
    ttp: &armory::Ttp,
    campaign: &crate::Campaign,
    target_id: &str,
    input: &ActionResolutionInput,
) {
    let declared_names = ttp
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect::<Vec<_>>();
    let mut values = arguments
        .iter()
        .filter_map(|argument| {
            argument
                .value
                .as_ref()
                .map(|value| (argument.name.clone(), value.clone()))
        })
        .collect::<HashMap<_, _>>();
    if ttp.tactic.eq_ignore_ascii_case("Lateral Movement") {
        if let Some(source_id) = input.exec_system_id.as_deref().filter(|id| {
            campaign
                .get_entities()
                .iter()
                .any(|entity| entity.entity_id().0 == *id)
        }) {
            values.insert("SRC".to_string(), source_id.to_string());
        }
    } else {
        values.insert("SRC".to_string(), target_id.to_string());
    }
    values.insert("TARGET_ID".to_string(), target_id.to_string());
    crate::grounding::ground_entity_ref_vars(&mut values, campaign);
    crate::grounding::resolve_argument_dependencies(&mut values);

    for argument in arguments {
        let Some(param) = ttp.params.iter().find(|param| param.name == argument.name) else {
            continue;
        };
        let default_vars = crate::grounding::detect_ungrounded_vars(&param.default);
        for dependency in default_vars.iter().filter(|variable| {
            !argument.name.eq_ignore_ascii_case(variable)
                && declared_names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(variable))
        }) {
            if !argument
                .depends_on
                .iter()
                .any(|name| name.eq_ignore_ascii_case(dependency))
            {
                argument.depends_on.push(dependency.clone());
            }
        }

        if argument.status != ArgumentResolutionStatus::GeneratedAtExecution {
            continue;
        }
        let Some(value) = values.get(&argument.name).cloned() else {
            continue;
        };
        let unresolved = crate::grounding::detect_ungrounded_vars(&value);
        argument.value = Some(value);
        if unresolved.is_empty() {
            argument.status = ArgumentResolutionStatus::Resolved;
            argument.reason = None;
            continue;
        }
        let runtime_generated = unresolved.iter().all(|variable| {
            matches!(
                variable.to_ascii_uppercase().as_str(),
                "RANDOM" | "LISTENER" | "LISTENER_PORT"
            )
        });
        if !runtime_generated && argument.required {
            argument.status = ArgumentResolutionStatus::NeedsInput;
            argument.reason = Some(format!(
                "{} needs input because its default has unresolved variables: {}",
                argument.name,
                unresolved.join(", ")
            ));
        }
    }
}

fn resolve_argument(
    param: &armory::TtpParam,
    ttp: &armory::Ttp,
    campaign: &crate::Campaign,
    target: &crate::CampaignEntityRef<'_>,
    target_id: &str,
    input: &ActionResolutionInput,
) -> ArgumentResolution {
    let mut candidates = candidates_for_param(param, ttp, campaign, target_id, input);
    if candidates.is_empty() && !param.options.is_empty() {
        candidates = option_candidates(param);
    }
    let base = || ArgumentResolution {
        name: param.name.clone(),
        param_type: param.param_type.clone(),
        required: param.required,
        status: ArgumentResolutionStatus::Resolved,
        value: None,
        candidates: candidates.clone(),
        depends_on: dependencies_for_param(param, ttp),
        blocks: blocked_params_for_param(param, ttp),
        source: None,
        reason: None,
    };

    let supplied = if param.param_type.eq_ignore_ascii_case("K8sAuth") {
        input
            .auth_identity_id
            .as_ref()
            .or_else(|| input.args.get(&param.name))
    } else {
        input.args.get(&param.name)
    }
    .map(String::as_str)
    .map(str::trim)
    .filter(|value| {
        !value.is_empty() && !value.starts_with("${") && *value != param.default.trim()
    });

    if let Some(value) = supplied {
        let constrained = !candidates.is_empty()
            || !param.options.is_empty()
            || requires_entity_id(&param.param_type);
        if constrained {
            let canonical_value = campaign.canonical_entity_id(value);
            let selected = candidates.iter().find(|candidate| {
                candidate.value == value
                    || candidate.source.entity_id.as_deref() == Some(canonical_value.as_str())
            });
            let Some(candidate) = selected else {
                return ArgumentResolution {
                    status: ArgumentResolutionStatus::Blocked,
                    value: Some(value.to_string()),
                    source: Some(source("operator", None, Some(param.name.clone()), None)),
                    reason: Some(format!(
                        "{} value '{}' is not available for this target",
                        param.name, value
                    )),
                    ..base()
                };
            };
            return ArgumentResolution {
                status: ArgumentResolutionStatus::Resolved,
                value: Some(candidate.value.clone()),
                source: Some(source(
                    "operator",
                    candidate.source.entity_id.clone(),
                    candidate.source.field.clone(),
                    None,
                )),
                ..base()
            };
        }
        return ArgumentResolution {
            status: ArgumentResolutionStatus::Resolved,
            value: Some(value.to_string()),
            source: Some(source("operator", None, Some(param.name.clone()), None)),
            ..base()
        };
    }

    if !param.default.trim().is_empty() {
        if let Some(binding) = crate::grounding::resolve_runtime_argument(
            &param.name,
            &param.default,
            target_id,
            campaign,
        ) {
            return from_runtime_binding(binding, target_id, base());
        }
        return resolve_default(param, ttp, campaign, target, target_id, input, base());
    }

    if listener_selection_not_needed(param, ttp, input) {
        return ArgumentResolution {
            status: ArgumentResolutionStatus::Omitted,
            source: Some(source("explicit_dependency", None, None, None)),
            ..base()
        };
    }

    if let Some(binding) =
        crate::grounding::resolve_runtime_argument(&param.name, "", target_id, campaign)
    {
        return from_runtime_binding(binding, target_id, base());
    }

    if !param.required {
        return ArgumentResolution {
            status: ArgumentResolutionStatus::Omitted,
            source: Some(source("optional", None, None, None)),
            ..base()
        };
    }

    match candidates.as_slice() {
        [candidate] => ArgumentResolution {
            status: ArgumentResolutionStatus::Resolved,
            value: Some(candidate.value.clone()),
            source: Some(candidate.source.clone()),
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
        [] if param.param_type.eq_ignore_ascii_case("Session") => ArgumentResolution {
            status: ArgumentResolutionStatus::Blocked,
            reason: Some(format!(
                "{} requires a live c2.session channel, but none is available",
                param.name
            )),
            ..base()
        },
        [] if param.param_type.eq_ignore_ascii_case("Listener") => ArgumentResolution {
            status: ArgumentResolutionStatus::Blocked,
            reason: Some(format!(
                "{} requires an active Listener, but none exists; run create-listener first",
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
        _ => from_candidates(param, candidates.clone(), base()),
    }
}

fn resolve_default(
    param: &armory::TtpParam,
    ttp: &armory::Ttp,
    campaign: &crate::Campaign,
    target: &crate::CampaignEntityRef<'_>,
    target_id: &str,
    input: &ActionResolutionInput,
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
        if let Some(binding) = crate::grounding::resolve_target_entity_parameter(
            &param.param_type,
            target_id,
            campaign,
        ) {
            return from_runtime_binding(binding, target_id, base);
        }
        if param.param_type.eq_ignore_ascii_case("Pod")
            || param.param_type.eq_ignore_ascii_case("Namespace")
        {
            return ArgumentResolution {
                status: ArgumentResolutionStatus::NeedsInput,
                reason: Some(format!(
                    "{} needs input because the target cannot be projected as {}",
                    param.name, param.param_type
                )),
                ..base
            };
        }
        let use_name = param.param_type.eq_ignore_ascii_case("string")
            || (is_entity_param(&param.param_type) && !requires_entity_id(&param.param_type));
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
        let selected_namespace = selected_namespace(ttp, input)
            .map(|namespace| {
                (
                    namespace,
                    source("operator", None, Some("namespace".to_string()), None),
                )
            })
            .or_else(|| {
                target.namespace().map(|namespace| {
                    (
                        namespace.to_string(),
                        source(
                            "target_fact",
                            Some(target_id.to_string()),
                            Some("namespace".to_string()),
                            Some(default.clone()),
                        ),
                    )
                })
            })
            .or_else(|| {
                selected_identity_namespace(ttp, campaign, target_id, input).map(
                    |(namespace, identity_id)| {
                        (
                            namespace,
                            source(
                                "identity_fact",
                                Some(identity_id),
                                Some("namespace".to_string()),
                                Some(default.clone()),
                            ),
                        )
                    },
                )
            });
        return match selected_namespace {
            Some((namespace, namespace_source)) => ArgumentResolution {
                status: ArgumentResolutionStatus::Resolved,
                value: Some(namespace),
                source: Some(namespace_source),
                ..base
            },
            None if base.candidates.is_empty() => ArgumentResolution {
                status: ArgumentResolutionStatus::NeedsInput,
                reason: Some(format!(
                    "{} needs input because the target namespace is not known",
                    param.name
                )),
                ..base
            },
            None => {
                let candidates = base.candidates.clone();
                from_candidates(param, candidates, base)
            }
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
            status: ArgumentResolutionStatus::GeneratedAtExecution,
            value: Some(default.clone()),
            source: Some(source("argument_dependency", None, None, Some(default))),
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

fn from_runtime_binding(
    binding: crate::grounding::RuntimeArgumentBinding,
    target_id: &str,
    base: ArgumentResolution,
) -> ArgumentResolution {
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
    ArgumentResolution {
        status,
        value: binding.readiness_value().map(str::to_string),
        source: Some(source(
            source_kind,
            (!binding.source().is_default()).then(|| target_id.to_string()),
            Some(binding.source().field().to_string()),
            None,
        )),
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
    campaign: &crate::Campaign,
    target_id: &str,
    input: &ActionResolutionInput,
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

    let target_cluster_id = cluster_id_for_target(campaign, target_id);
    let namespace = selected_namespace(ttp, input)
        .or_else(|| {
            campaign
                .get_entities()
                .into_iter()
                .find(|entity| entity.entity_id().0 == target_id)
                .and_then(|entity| entity.namespace().map(str::to_string))
        })
        .or_else(|| {
            selected_identity_namespace(ttp, campaign, target_id, input)
                .map(|(namespace, _)| namespace)
        });

    let mut candidates = campaign
        .get_entities()
        .into_iter()
        .filter(|entity| entity.entity_kind().eq_ignore_ascii_case(&param.param_type))
        .filter(|entity| {
            entity_in_scope(
                campaign,
                entity,
                target_cluster_id.as_deref(),
                namespace.as_deref(),
                &param.param_type,
            )
        })
        .map(|entity| {
            let field = if requires_entity_id(&param.param_type) {
                "id"
            } else {
                "name"
            };
            ArgumentCandidate {
                value: if field == "name" {
                    entity.entity_name().to_string()
                } else {
                    entity.entity_id().0.clone()
                },
                label: entity.entity_name().to_string(),
                source: source(
                    "entity",
                    Some(entity.entity_id().0),
                    Some(field.to_string()),
                    None,
                ),
            }
        })
        .collect::<Vec<_>>();
    if param.param_type.eq_ignore_ascii_case("Listener")
        && !listener_selection_not_needed(param, ttp, input)
    {
        candidates.retain(|candidate| {
            let mut args = input.args.clone();
            for declared in &ttp.params {
                if !declared.default.is_empty() {
                    args.entry(declared.name.clone())
                        .or_insert_with(|| declared.default.clone());
                }
            }
            args.insert(param.name.clone(), candidate.value.clone());
            crate::campaign::execution::ground_listener_defaults(ttp, &mut args, campaign).is_ok()
        });
    }
    candidates.sort_by(|a, b| a.label.cmp(&b.label).then(a.value.cmp(&b.value)));
    candidates
}

fn dependencies_for_param(param: &armory::TtpParam, ttp: &armory::Ttp) -> Vec<String> {
    if param.param_type.eq_ignore_ascii_case("ServiceAccount") {
        if let Some(namespace) = namespace_param(ttp) {
            return vec![namespace.name.clone()];
        }
    }
    if param.default.contains("${LISTENER") {
        if let Some(listener) = ttp
            .params
            .iter()
            .find(|candidate| candidate.param_type.eq_ignore_ascii_case("Listener"))
        {
            return vec![listener.name.clone()];
        }
    }
    Vec::new()
}

fn blocked_params_for_param(param: &armory::TtpParam, ttp: &armory::Ttp) -> Vec<String> {
    if !param.param_type.eq_ignore_ascii_case("Listener") {
        return Vec::new();
    }
    ttp.params
        .iter()
        .filter(|candidate| candidate.default.contains("${LISTENER"))
        .map(|candidate| candidate.name.clone())
        .collect()
}

fn namespace_param(ttp: &armory::Ttp) -> Option<&armory::TtpParam> {
    ttp.params.iter().find(|param| {
        param.param_type.eq_ignore_ascii_case("Namespace")
            || param.name.eq_ignore_ascii_case("Namespace")
            || param.name.eq_ignore_ascii_case("NS")
    })
}

fn selected_namespace(ttp: &armory::Ttp, input: &ActionResolutionInput) -> Option<String> {
    namespace_param(ttp)
        .and_then(|param| input.args.get(&param.name))
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.starts_with("${"))
        .map(|value| value.strip_prefix("ns/").unwrap_or(value).to_string())
}

fn selected_identity_namespace(
    ttp: &armory::Ttp,
    campaign: &crate::Campaign,
    target_id: &str,
    input: &ActionResolutionInput,
) -> Option<(String, String)> {
    let eligible = eligible_auth_identities(ttp, campaign, target_id);
    let identity_id = input
        .auth_identity_id
        .clone()
        .or_else(|| input.args.get("K8S_AUTH").cloned())
        .or_else(|| {
            eligible
                .iter()
                .find(|identity| identity.id == target_id)
                .map(|identity| identity.id.clone())
        })
        .or_else(|| (eligible.len() == 1).then(|| eligible[0].id.clone()));
    identity_id
        .as_deref()
        .and_then(|id| {
            campaign
                .get_entities()
                .into_iter()
                .find(|entity| entity.entity_id().0 == id)
        })
        .and_then(|entity| {
            let namespace = match entity {
                crate::CampaignEntityRef::K8sCredential(credential) => {
                    credential.default_namespace.clone()
                }
                _ => entity.namespace().map(str::to_string),
            }?;
            Some((namespace, entity.entity_id().0))
        })
}

fn cluster_id_for_target(campaign: &crate::Campaign, target_id: &str) -> Option<String> {
    let target = campaign
        .get_entities()
        .into_iter()
        .find(|entity| entity.entity_id().0 == target_id)?;
    if matches!(target, crate::CampaignEntityRef::Cluster(_)) {
        return Some(target_id.to_string());
    }
    let container_id = target
        .namespace()
        .map(|namespace| format!("ns/{namespace}"));
    let contained_id = container_id.as_deref().unwrap_or(target_id);
    campaign
        .relation_sources(&ran_domain::EntityId::new(contained_id), "contains")
        .into_iter()
        .find(|id| {
            campaign.get_entities().into_iter().any(|entity| {
                entity.entity_id() == *id && matches!(entity, crate::CampaignEntityRef::Cluster(_))
            })
        })
        .map(|id| id.0)
}

fn entity_in_scope(
    campaign: &crate::Campaign,
    entity: &crate::CampaignEntityRef<'_>,
    target_cluster_id: Option<&str>,
    namespace: Option<&str>,
    param_type: &str,
) -> bool {
    if param_type.eq_ignore_ascii_case("ServiceAccount") {
        if let Some(namespace) = namespace {
            return entity.namespace() == Some(namespace);
        }
    }
    if !(param_type.eq_ignore_ascii_case("Namespace") || param_type.eq_ignore_ascii_case("Node")) {
        return true;
    }
    let Some(cluster_id) = target_cluster_id else {
        return true;
    };
    campaign
        .relation_sources(&entity.entity_id(), "contains")
        .into_iter()
        .any(|id| id.0 == cluster_id)
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

fn requires_entity_id(param_type: &str) -> bool {
    matches!(
        param_type.to_ascii_lowercase().as_str(),
        "k8sauth" | "listener" | "redirector" | "session" | "k8scredential"
    )
}

fn listener_selection_not_needed(
    param: &armory::TtpParam,
    ttp: &armory::Ttp,
    input: &ActionResolutionInput,
) -> bool {
    if !param.param_type.eq_ignore_ascii_case("Listener") {
        return false;
    }
    let needs = |placeholder: &str, value_name: &str| {
        let direct_reference = ttp
            .procedures
            .iter()
            .any(|procedure| procedure.command.contains(placeholder))
            || ttp
                .effects
                .iter()
                .any(|effect| effect.contains(placeholder));
        let direct_needs_value = direct_reference
            && input
                .args
                .get(value_name)
                .map(String::as_str)
                .map(str::trim)
                .is_none_or(|value| value.is_empty() || value == placeholder);
        let active_default = ttp.params.iter().any(|candidate| {
            candidate.default.contains(placeholder)
                && input
                    .args
                    .get(&candidate.name)
                    .is_none_or(|value| value == &candidate.default)
                && ttp_references_param(ttp, &candidate.name)
        });
        direct_needs_value || active_default
    };
    !needs("${LISTENER}", "LISTENER") && !needs("${LISTENER_PORT}", "LISTENER_PORT")
}

fn ttp_references_param(ttp: &armory::Ttp, name: &str) -> bool {
    let dollar = format!("${{{name}}}");
    let tera_with_space = format!("{{{{ {name}");
    let tera_without_space = format!("{{{{{name}");
    ttp.procedures.iter().any(|procedure| {
        procedure.command.contains(&dollar)
            || procedure.command.contains(&tera_with_space)
            || procedure.command.contains(&tera_without_space)
    }) || ttp.effects.iter().any(|effect| effect.contains(&dollar))
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
        AccessLevel, BinaryPresence, Contains, Entity, JwToken, K8sCluster, Mount, Namespace, Pod,
        ServiceAccount, ServiceAccountToken, SessionChannel, UnknownSystem,
    };

    use super::*;

    #[test]
    fn target_ip_default_is_resolved_with_provenance() {
        let mut campaign = crate::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut target = UnknownSystem::new("target");
        target.system.access_level = AccessLevel::Exec;
        target.system.ips.push("10.23.4.5".parse().unwrap());
        let target_id = target.entity_id().0;
        campaign.upsert_entity(target, crate::KnowledgeProvenance::Scenario);

        let mut ttp = armory::Ttp::new("scan", "Scan", "Discovery");
        ttp.params.push(armory::TtpParam {
            name: "CIDR".to_string(),
            param_type: "string".to_string(),
            description: String::new(),
            required: true,
            default: "${TARGET.IP}/24".to_string(),
            options: Vec::new(),
        });

        let resolution = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();
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
        let mut campaign = crate::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut target = UnknownSystem::new("target");
        target.system.access_level = AccessLevel::Exec;
        target.system.ips.push("10.23.4.5".parse().unwrap());
        target.system.ips.push("192.0.2.2".parse().unwrap());
        let target_id = target.entity_id().0;
        campaign.upsert_entity(target, crate::KnowledgeProvenance::Scenario);

        let mut ttp = armory::Ttp::new("scan", "Scan", "Discovery");
        ttp.params.push(armory::TtpParam {
            name: "CIDR".to_string(),
            param_type: "string".to_string(),
            description: String::new(),
            required: true,
            default: "${TARGET.IP}/24".to_string(),
            options: Vec::new(),
        });

        let resolution = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();
        assert_eq!(resolution.status, ActionReadinessStatus::NeedsChoice);
        assert_eq!(resolution.arguments[0].candidates.len(), 2);
    }

    #[test]
    fn api_server_runtime_default_matches_execution_grounding() {
        let mut campaign = crate::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut target = UnknownSystem::new("target");
        target.system.access_level = AccessLevel::Exec;
        let target_id = target.entity_id().0;
        campaign.upsert_entity(target, crate::KnowledgeProvenance::Scenario);

        let mut ttp = armory::Ttp::new("permissions", "Permissions", "Discovery");
        ttp.params.push(armory::TtpParam {
            name: "API_SERVER".to_string(),
            param_type: "string".to_string(),
            description: String::new(),
            required: true,
            default: String::new(),
            options: Vec::new(),
        });

        let resolution = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();
        assert_eq!(resolution.status, ActionReadinessStatus::Ready);
        assert_eq!(
            resolution.arguments[0].status,
            ArgumentResolutionStatus::Defaulted
        );
        assert_eq!(
            resolution.arguments[0].value.as_deref(),
            Some(crate::grounding::DEFAULT_API_SERVER)
        );
        assert_eq!(
            resolution.arguments[0]
                .source
                .as_ref()
                .map(|source| source.kind.as_str()),
            Some("runtime_default")
        );

        let mut args = std::collections::HashMap::new();
        crate::grounding::ground_args_from_context(&mut args, &target_id, &campaign);
        assert_eq!(
            args.get("API_SERVER").map(String::as_str),
            resolution.arguments[0].value.as_deref()
        );
    }

    #[test]
    fn target_runtime_bindings_are_ready_without_exposing_tokens() {
        let mut campaign = crate::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut pod = Pod::new("runner", "workloads");
        pod.system.access_level = AccessLevel::Exec;
        pod.node_name = Some("worker-a".to_string());
        pod.host_ip = Some("10.0.0.8".parse().unwrap());
        pod.service_account_name = Some("runner-sa".to_string());
        let target_id = pod.entity_id().0;
        campaign.upsert_entity(pod, crate::KnowledgeProvenance::Scenario);

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
        campaign.upsert_entity(service_account, crate::KnowledgeProvenance::Scenario);

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

        let resolution = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();
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
        crate::grounding::ground_args_from_context(&mut args, &target_id, &campaign);
        assert_eq!(
            args.get("TOKEN").map(String::as_str),
            Some("ey.runtime.secret")
        );
    }

    #[test]
    fn session_parameter_resolves_live_edges_of_the_selected_c2() {
        let mut campaign = crate::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        campaign.upsert_relation(
            &SessionChannel::new("c2/ran", "node/victim", "session/victim-4444"),
            crate::KnowledgeProvenance::Scenario,
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

        let resolution =
            resolve_action(&ttp, &campaign, "c2/ran", &ActionResolutionInput::default())
                .expect("C2 is a target");
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
    fn partial_namespace_selection_recomputes_semantic_service_accounts() {
        let cluster = K8sCluster::new("dev");
        let cluster_id = cluster.entity_id().0;
        let mut campaign = crate::Campaign::bootstrap("Ran", cluster);
        for namespace in ["default", "workloads"] {
            let entity = Namespace::new(namespace);
            let namespace_id = entity.entity_id().0;
            campaign.upsert_entity(entity, crate::KnowledgeProvenance::Scenario);
            campaign.upsert_relation(
                &Contains::new(cluster_id.clone(), namespace_id),
                crate::KnowledgeProvenance::Scenario,
            );
        }
        for (name, namespace) in [("default-sa", "default"), ("runner", "workloads")] {
            campaign.upsert_entity(
                ServiceAccount::new(name, namespace),
                crate::KnowledgeProvenance::Scenario,
            );
        }

        let mut ttp = armory::Ttp::new("deploy", "Deploy", "Execution");
        ttp.params = vec![
            armory::TtpParam {
                name: "Namespace".to_string(),
                param_type: "Namespace".to_string(),
                description: String::new(),
                required: true,
                default: "${NS}".to_string(),
                options: Vec::new(),
            },
            armory::TtpParam {
                name: "ServiceAccount".to_string(),
                param_type: "ServiceAccount".to_string(),
                description: String::new(),
                required: false,
                default: String::new(),
                options: Vec::new(),
            },
        ];

        let initial = resolve_action(
            &ttp,
            &campaign,
            &cluster_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();
        assert_eq!(initial.status, ActionReadinessStatus::NeedsChoice);
        assert_eq!(
            initial.arguments[0].status,
            ArgumentResolutionStatus::NeedsChoice
        );
        assert_eq!(initial.arguments[0].value, None);
        assert_eq!(
            initial.arguments[0]
                .candidates
                .iter()
                .map(|candidate| candidate.value.as_str())
                .collect::<Vec<_>>(),
            vec!["default", "workloads"]
        );
        assert_eq!(
            initial.arguments[1].status,
            ArgumentResolutionStatus::Omitted
        );
        assert_eq!(initial.arguments[1].candidates.len(), 2);
        assert_eq!(initial.arguments[1].depends_on, vec!["Namespace"]);

        let selected = resolve_action(
            &ttp,
            &campaign,
            &cluster_id,
            &ActionResolutionInput {
                args: HashMap::from([("Namespace".to_string(), "workloads".to_string())]),
                ..ActionResolutionInput::default()
            },
        )
        .unwrap();
        assert_eq!(selected.arguments[0].value.as_deref(), Some("workloads"));
        assert_eq!(selected.arguments[1].candidates.len(), 1);
        assert_eq!(selected.arguments[1].candidates[0].value, "runner");
        assert_eq!(
            selected.arguments[1].candidates[0].source.field.as_deref(),
            Some("name")
        );
    }

    #[test]
    fn typed_target_defaults_project_a_pod_and_survive_resolution_feedback() {
        let cluster = K8sCluster::new("dev");
        let cluster_id = cluster.entity_id().0;
        let mut campaign = crate::Campaign::bootstrap("Ran", cluster);
        let namespace = Namespace::new("agent-system");
        let namespace_id = namespace.entity_id().0;
        campaign.upsert_entity(namespace, crate::KnowledgeProvenance::Scenario);
        campaign.upsert_relation(
            &Contains::new(cluster_id, namespace_id),
            crate::KnowledgeProvenance::Scenario,
        );
        let pod = Pod::new("agent-worker-hsv7z", "agent-system");
        let target_id = pod.entity_id().0;
        campaign.upsert_entity(pod, crate::KnowledgeProvenance::Scenario);

        let mut ttp = armory::Ttp::new("pod-context", "Pod context", "Execution");
        ttp.params = vec![
            armory::TtpParam {
                name: "NAMESPACE".to_string(),
                param_type: "Namespace".to_string(),
                description: String::new(),
                required: true,
                default: "${TARGET}".to_string(),
                options: Vec::new(),
            },
            armory::TtpParam {
                name: "POD_NAME".to_string(),
                param_type: "Pod".to_string(),
                description: String::new(),
                required: true,
                default: "${TARGET}".to_string(),
                options: Vec::new(),
            },
        ];

        let initial = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();
        assert_eq!(initial.status, ActionReadinessStatus::Ready);
        assert_eq!(initial.arguments[0].value.as_deref(), Some("agent-system"));
        assert_eq!(
            initial.arguments[1].value.as_deref(),
            Some("agent-worker-hsv7z")
        );

        let feedback_args = initial
            .arguments
            .into_iter()
            .filter_map(|argument| argument.value.map(|value| (argument.name, value)))
            .collect();
        let feedback = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput {
                args: feedback_args,
                ..ActionResolutionInput::default()
            },
        )
        .unwrap();
        assert_eq!(feedback.status, ActionReadinessStatus::Ready);
        assert!(feedback
            .arguments
            .iter()
            .all(|argument| argument.status == ArgumentResolutionStatus::Resolved));
    }

    #[test]
    fn compound_argument_default_resolves_from_other_arguments() {
        let mut campaign = crate::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let target = UnknownSystem::new("target");
        let target_id = target.entity_id().0;
        campaign.upsert_entity(target, crate::KnowledgeProvenance::Scenario);
        let mut ttp = armory::Ttp::new("compound", "Compound", "Execution");
        ttp.params = vec![
            armory::TtpParam {
                name: "SUBJECT".to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: true,
                default: "runner".to_string(),
                options: Vec::new(),
            },
            armory::TtpParam {
                name: "ROLE_NAME".to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: true,
                default: "nsadmin".to_string(),
                options: Vec::new(),
            },
            armory::TtpParam {
                name: "BINDING_NAME".to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: true,
                default: "${SUBJECT}-${ROLE_NAME}".to_string(),
                options: Vec::new(),
            },
        ];

        let resolution = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();

        assert_eq!(resolution.status, ActionReadinessStatus::Ready);
        assert_eq!(
            resolution.arguments[2].value.as_deref(),
            Some("runner-nsadmin")
        );
        assert_eq!(
            resolution.arguments[2].depends_on,
            vec!["SUBJECT", "ROLE_NAME"]
        );
    }

    #[test]
    fn entity_reference_default_uses_the_same_target_facts_as_execution() {
        let mut campaign = crate::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let mut pod = Pod::new("attacker", "default");
        pod.volume_mounts.push(Mount {
            name: "host-proc".to_string(),
            mount_root: "/proc".to_string(),
            mount_point: "/mnt/host-proc".to_string(),
            mount_type: None,
            is_host_path: true,
            read_only: true,
        });
        let target_id = pod.entity_id().0;
        campaign.upsert_entity(pod, crate::KnowledgeProvenance::Scenario);
        let mut ttp = armory::Ttp::new("host-proc", "Host proc", "Privilege Escalation");
        ttp.params.push(armory::TtpParam {
            name: "HOST_PROC".to_string(),
            param_type: "string".to_string(),
            description: String::new(),
            required: true,
            default: "${SRC.HOST_PATH:/proc}".to_string(),
            options: Vec::new(),
        });

        let resolution = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();

        assert_eq!(resolution.status, ActionReadinessStatus::Ready);
        assert_eq!(
            resolution.arguments[0].value.as_deref(),
            Some("/mnt/host-proc")
        );
    }

    #[test]
    fn argument_dependency_cycle_requires_input() {
        let mut campaign = crate::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
        let target = UnknownSystem::new("target");
        let target_id = target.entity_id().0;
        campaign.upsert_entity(target, crate::KnowledgeProvenance::Scenario);
        let mut ttp = armory::Ttp::new("cycle", "Cycle", "Execution");
        ttp.params = [("A", "${B}"), ("B", "${A}")]
            .into_iter()
            .map(|(name, default)| armory::TtpParam {
                name: name.to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: true,
                default: default.to_string(),
                options: Vec::new(),
            })
            .collect();

        let resolution = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();

        assert_eq!(resolution.status, ActionReadinessStatus::NeedsInput);
        assert!(resolution
            .reasons
            .iter()
            .any(|reason| reason.contains("unresolved variables")));
    }

    #[test]
    fn token_permission_namespace_accepts_the_kubernetes_name() {
        let cluster = K8sCluster::new("dev");
        let cluster_id = cluster.entity_id().0;
        let mut campaign = crate::Campaign::bootstrap("Ran", cluster);
        let namespace = Namespace::new("agent-system");
        let namespace_id = namespace.entity_id().0;
        campaign.upsert_entity(namespace, crate::KnowledgeProvenance::Scenario);
        campaign.upsert_relation(
            &Contains::new(cluster_id, namespace_id),
            crate::KnowledgeProvenance::Scenario,
        );

        let mut service_account = ServiceAccount::new("agent-worker", "agent-system");
        service_account.token = Some(ServiceAccountToken {
            jwt: JwToken {
                raw: "ey.test.token".to_string(),
                ..Default::default()
            },
            service_account_name: "agent-worker".to_string(),
            namespace: "agent-system".to_string(),
            pod_name: None,
            pod_uid: None,
            service_account_uid: None,
            is_bound: false,
        });
        let target_id = service_account.entity_id().0;
        campaign.upsert_entity(service_account, crate::KnowledgeProvenance::Scenario);

        let armory_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../armory/TTPs");
        let armory = armory::Armory::load_from_dir(armory_path).expect("repository armory");
        let ttp = armory
            .get_ttp("check-token-permissions")
            .expect("Check Token Permissions action");
        let resolution = resolve_action(
            ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput {
                args: HashMap::from([("NS".to_string(), "agent-system".to_string())]),
                auth_identity_id: Some(target_id.clone()),
                procedure_id: Some("kubectl".to_string()),
                exec_system_id: None,
            },
        )
        .expect("ServiceAccount target");

        let namespace = resolution
            .arguments
            .iter()
            .find(|argument| argument.name == "NS")
            .expect("NS resolution");
        assert_eq!(namespace.status, ArgumentResolutionStatus::Resolved);
        assert_eq!(namespace.value.as_deref(), Some("agent-system"));
        assert_eq!(namespace.candidates[0].value, "agent-system");
        assert_eq!(
            namespace.candidates[0].source.field.as_deref(),
            Some("name")
        );
    }

    #[test]
    fn missing_listener_exposes_the_parameters_it_blocks() {
        let cluster = K8sCluster::new("dev");
        let target_id = cluster.entity_id().0;
        let campaign = crate::Campaign::bootstrap("Ran", cluster);
        let mut ttp = armory::Ttp::new("callback", "Callback", "Execution");
        ttp.params = vec![
            armory::TtpParam {
                name: "LISTENER_REF".to_string(),
                param_type: "Listener".to_string(),
                description: String::new(),
                required: true,
                default: String::new(),
                options: Vec::new(),
            },
            armory::TtpParam {
                name: "LISTENER".to_string(),
                param_type: "string".to_string(),
                description: String::new(),
                required: true,
                default: "${LISTENER}".to_string(),
                options: Vec::new(),
            },
            armory::TtpParam {
                name: "LISTENER_PORT".to_string(),
                param_type: "int".to_string(),
                description: String::new(),
                required: true,
                default: "${LISTENER_PORT}".to_string(),
                options: Vec::new(),
            },
        ];
        ttp.procedures.push(armory::Procedure::new(
            "shell",
            "connect ${LISTENER}:${LISTENER_PORT}",
        ));

        let resolution = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();
        assert_eq!(resolution.status, ActionReadinessStatus::Blocked);
        assert_eq!(
            resolution.arguments[0].blocks,
            vec!["LISTENER", "LISTENER_PORT"]
        );
        assert_eq!(resolution.arguments[1].depends_on, vec!["LISTENER_REF"]);
    }

    #[test]
    fn procedure_readiness_and_recommendation_are_resolved_by_the_backend() {
        let mut campaign = crate::Campaign::bootstrap("Ran", K8sCluster::new("dev"));
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
        campaign.upsert_entity(target, crate::KnowledgeProvenance::Scenario);

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

        let resolution = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput::default(),
        )
        .unwrap();

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

        let selected_unavailable = resolve_action(
            &ttp,
            &campaign,
            &target_id,
            &ActionResolutionInput {
                procedure_id: Some("ip".to_string()),
                ..ActionResolutionInput::default()
            },
        )
        .unwrap();

        assert_eq!(selected_unavailable.status, ActionReadinessStatus::Blocked);
        assert!(selected_unavailable.reasons.iter().any(|reason| {
            reason == "required tool 'ip' is known to be absent from the execution system"
        }));
        assert_eq!(
            selected_unavailable.recommended_procedure_id.as_deref(),
            Some("hostname")
        );
    }
}
