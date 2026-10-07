//! Plan the environment that runs a client independently of the resource it
//! addresses. Inputs are original Armory definitions, never rendered commands.

use std::cell::{Ref, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use armory::{Procedure, Ttp};
use ran_domain::{BinaryPresence, EntityId, K8sCredential, OperatorHost};

use super::execution::{
    http_response_output_transform, procedure_required_tool, ProcedureReadiness,
};
use super::{Campaign, ExecChannel, ExecuteActionError};
use crate::ttp_applicability::{eligible_auth_identities, procedure_uses_k8s_auth};

/// Stable intent captured before grounding or lowering consumes request specs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionPlacement {
    Target,
    RequiredTarget,
    Client,
    AlternativeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcedureExecutionSemantics {
    pub placement: ExecutionPlacement,
    pub uses_k8s_auth: bool,
}

impl ProcedureExecutionSemantics {
    pub fn from_definition(procedure: &Procedure) -> Self {
        let uses_k8s_auth = procedure_uses_k8s_auth(procedure);
        let placement = if procedure.run_on_target == Some(true) {
            ExecutionPlacement::RequiredTarget
        } else if procedure.run_on_target == Some(false) {
            ExecutionPlacement::AlternativeSource
        } else if procedure.k8s_request.is_some()
            || procedure.http_request.is_some()
            || (procedure.operation.is_shell() && procedure.is_local_command == Some(true))
            // Legacy shell clients are classified at the definition boundary.
            // Typed session/control operations keep their own lifecycle.
            || (procedure.operation.is_shell() && uses_k8s_auth)
        {
            ExecutionPlacement::Client
        } else {
            ExecutionPlacement::Target
        };
        Self {
            placement,
            uses_k8s_auth,
        }
    }

    pub fn needs_client_plan(self) -> bool {
        self.placement != ExecutionPlacement::Target
    }
}

/// No credential material or rendered payload belongs in an execution plan.
#[derive(Debug, Clone)]
pub(crate) struct ClientExecutionPlan {
    pub target_id: String,
    pub auth_identity_id: Option<String>,
    /// None means the local/native client environment, not the API resource.
    pub channel: Option<ExecChannel>,
    pub local_system_id: Option<String>,
    pub readiness: ProcedureReadiness,
    pub required_tool: Option<String>,
}

impl ClientExecutionPlan {
    pub fn executor_id(&self) -> Option<&str> {
        self.channel
            .as_ref()
            .and_then(|channel| channel.exec_target_id.as_deref())
            .or(self.local_system_id.as_deref())
    }
}

/// Snapshot-scoped memoization shares graph routes across procedure and
/// credential alternatives. Nothing is cached across campaign mutations.
pub struct ClientExecutionPlanner<'a> {
    campaign: &'a Campaign,
    channels: RefCell<ChannelCache>,
}

type CandidateChannel = (String, f32, ExecChannel);
type ChannelCache = HashMap<(Option<String>, bool), Vec<CandidateChannel>>;

impl<'a> ClientExecutionPlanner<'a> {
    pub(crate) fn belongs_to(&self, campaign: &Campaign) -> bool {
        std::ptr::eq(self.campaign, campaign)
    }

    /// Number of graph searches in this snapshot context, useful for scaling
    /// assertions and endpoint benchmarks. Exclusion sets require separate work.
    pub fn graph_search_count(&self) -> usize {
        self.channels.borrow().len()
    }

    pub fn new(campaign: &'a Campaign) -> Self {
        Self {
            campaign,
            channels: RefCell::new(HashMap::new()),
        }
    }

    fn channels(
        &self,
        excluded: Option<&str>,
        allow_output_transform: bool,
    ) -> Ref<'_, Vec<CandidateChannel>> {
        let key = (excluded.map(str::to_string), allow_output_transform);
        if !self.channels.borrow().contains_key(&key) {
            let campaign = self.campaign;
            let mut sources: BTreeSet<String> = campaign
                .direct_foothold_systems()
                .into_iter()
                .map(|id| id.0)
                .collect();
            // A live session is evidence even when its graph edge has not yet
            // been reconciled or its entity is still a provisional system.
            for entity in campaign.get_entities() {
                let id = entity.entity_id().0;
                if campaign.active_session_backend(&id).is_some() {
                    sources.insert(id);
                }
            }
            let source_channels = sources
                .into_iter()
                .filter(|id| excluded != Some(id.as_str()))
                .filter_map(|id| {
                    campaign.get_system_entity(&id)?;
                    let backend_id = campaign.resolve_source_backend_id(&id);
                    let mut channel = ExecChannel::direct(backend_id);
                    channel.exec_target_id = Some(id.clone());
                    Some((id, channel))
                })
                .collect::<BTreeMap<_, _>>();
            let seeds = source_channels
                .keys()
                .map(EntityId::new)
                .collect::<Vec<_>>();
            let excluded_id = excluded.map(EntityId::new);
            let paths = campaign.executable_paths_from(
                &seeds,
                excluded_id.as_ref(),
                allow_output_transform,
            );
            let mut channels = paths
                .into_iter()
                .filter_map(|(id, path)| {
                    campaign.get_system_entity(&id.0)?;
                    let source = source_channels.get(&path.nodes.first()?.0)?;
                    Some((
                        id.0.clone(),
                        (
                            path.cost,
                            ExecChannel {
                                kubelet_plans: path.kubelet_plans,
                                edges: path.edges,
                                backend_id: source.backend_id.clone(),
                                hops: path.nodes[..path.nodes.len() - 1]
                                    .iter()
                                    .map(|hop| hop.0.clone())
                                    .collect(),
                                exec_target_id: Some(id.0),
                            },
                        ),
                    ))
                })
                .collect::<BTreeMap<_, _>>();
            // A session-only source need not have a graph node yet.
            for (id, channel) in source_channels {
                // A direct foothold is authoritative for its own system. Do
                // not replace it with a derived path from another seed merely
                // because both paths terminate at the same entity.
                channels.insert(id, (0.0, channel));
            }
            let candidates = channels
                .into_iter()
                .map(|(id, (cost, channel))| (id, cost, channel))
                .collect();
            self.channels.borrow_mut().insert(key.clone(), candidates);
        }
        Ref::map(self.channels.borrow(), |channels| &channels[&key])
    }

    pub(crate) fn plan(
        &self,
        ttp: &Ttp,
        procedure: &Procedure,
        target_id: &str,
        auth_identity_id: Option<&str>,
        exec_hint: Option<&str>,
    ) -> Result<ClientExecutionPlan, ExecuteActionError> {
        let semantics = ProcedureExecutionSemantics::from_definition(procedure);
        let ambient_kubeconfig = procedure.is_local_command == Some(true)
            && procedure.command.contains("kubectl ")
            && !procedure.command.contains("${K8S_AUTH}");
        let identities = if semantics.uses_k8s_auth
            && ambient_kubeconfig
            && auth_identity_id.is_none_or(|id| id.trim().is_empty())
        {
            vec![None]
        } else if semantics.uses_k8s_auth {
            let eligible = eligible_auth_identities(ttp, self.campaign, target_id);
            if let Some(identity_id) = auth_identity_id.map(str::trim).filter(|id| !id.is_empty()) {
                if !eligible.iter().any(|identity| identity.id == identity_id) {
                    return Err(ExecuteActionError::InvalidInput(format!(
                        "authentication identity '{}' is not eligible for action '{}'",
                        identity_id, ttp.id
                    )));
                }
                vec![Some(identity_id.to_string())]
            } else if !eligible.is_empty() {
                eligible
                    .into_iter()
                    .map(|identity| Some(identity.id))
                    .collect()
            } else {
                let ambient_kubeconfig = procedure.is_local_command == Some(true)
                    && procedure.command.contains("kubectl ")
                    && !procedure.command.contains("${K8S_AUTH}");
                if !ambient_kubeconfig {
                    return Err(ExecuteActionError::InvalidInput(format!(
                        "action '{}' requires an Authenticate As identity",
                        ttp.id
                    )));
                }
                vec![None]
            }
        } else {
            vec![None]
        };

        let mut fallback = None;
        let mut failure = None;
        for identity_id in identities {
            match self.plan_with_identity(procedure, target_id, identity_id, exec_hint, semantics) {
                Ok(plan) if plan.readiness == ProcedureReadiness::Ready => return Ok(plan),
                Ok(plan) => {
                    fallback.get_or_insert(plan);
                }
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        fallback
            .ok_or_else(|| failure.expect("at least one authentication alternative was planned"))
    }

    fn plan_with_identity(
        &self,
        procedure: &Procedure,
        target_id: &str,
        auth_identity_id: Option<String>,
        exec_hint: Option<&str>,
        semantics: ProcedureExecutionSemantics,
    ) -> Result<ClientExecutionPlan, ExecuteActionError> {
        let native_client = semantics.uses_k8s_auth
            && auth_identity_id.as_deref().is_some_and(|id| {
                self.campaign
                    .entities
                    .contains::<K8sCredential>(&EntityId::new(id))
            });
        let required_tool = if native_client
            && (procedure.k8s_request.is_some() || procedure.http_request.is_some())
        {
            None
        } else {
            procedure_required_tool(procedure)
        };
        let make_plan = |channel, readiness| ClientExecutionPlan {
            target_id: target_id.to_string(),
            auth_identity_id: auth_identity_id.clone(),
            channel,
            local_system_id: self
                .campaign
                .graph
                .sources_of(&EntityId::new(c2::BUILTIN_C2_ID), "contains")
                .into_iter()
                .filter(|id| self.campaign.entities.contains::<OperatorHost>(id))
                .map(|id| id.0.clone())
                .min(),
            readiness,
            required_tool: required_tool.clone(),
        };
        let exclude_target = semantics.placement == ExecutionPlacement::AlternativeSource;
        let canonical_target = self.campaign.canonical_entity_id(target_id);
        let source_hint = exec_hint
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| self.campaign.canonical_entity_id(id));
        let required_target = semantics.placement == ExecutionPlacement::RequiredTarget;
        let local_id = make_plan(None, ProcedureReadiness::Ready).local_system_id;
        if native_client || procedure.is_local_command == Some(true) {
            if exclude_target
                && (local_id.is_none() || local_id.as_deref() == Some(canonical_target.as_str()))
            {
                return Err(ExecuteActionError::InvalidInput(
                    "local client cannot satisfy the excluded-target placement constraint".into(),
                ));
            }
            if required_target && local_id.as_deref() != Some(canonical_target.as_str()) {
                return Err(ExecuteActionError::InvalidInput(format!(
                    "procedure '{}' requires target execution but its client is local",
                    procedure.id
                )));
            }
            if source_hint
                .as_ref()
                .is_some_and(|hint| local_id.as_ref() != Some(hint) && hint != c2::BUILTIN_C2_ID)
            {
                return Err(ExecuteActionError::InvalidInput("selected execution system is incompatible with the local client; select the operator host or a remotely realizable authentication identity".into()));
            }
            let readiness = required_tool.as_deref().map(|tool| {
                local_id
                    .as_deref()
                    .and_then(|id| self.campaign.get_system_entity(id))
                    .map(|system| system.entity().system().has_binary(tool))
                    .unwrap_or(BinaryPresence::Unknown)
            });
            if readiness == Some(BinaryPresence::Absent) {
                return Err(ExecuteActionError::InvalidInput("required client tool is known to be absent from the local execution environment".into()));
            }
            return Ok(make_plan(
                None,
                if readiness == Some(BinaryPresence::Unknown) {
                    ProcedureReadiness::Unknown
                } else {
                    ProcedureReadiness::Ready
                },
            ));
        }

        if exclude_target && source_hint.as_ref() == Some(&canonical_target) {
            return Err(ExecuteActionError::InvalidInput(
                "selected execution system is excluded by runOnTarget: false".into(),
            ));
        }
        if required_target
            && source_hint
                .as_ref()
                .is_some_and(|hint| hint != &canonical_target)
        {
            return Err(ExecuteActionError::InvalidInput(
                "selected execution system conflicts with runOnTarget: true".into(),
            ));
        }
        let source_hint = if required_target {
            Some(canonical_target.clone())
        } else {
            source_hint
        };
        let tool = procedure_required_tool(procedure);
        let readiness_on = |id: &str, channel: &ExecChannel| {
            let system = self
                .campaign
                .get_system_entity(id)
                .expect("executor is a system");
            match tool
                .as_deref()
                .map(|tool| system.entity().system().has_binary(tool))
            {
                None | Some(BinaryPresence::Present(_)) => {
                    if channel.kubelet_plans.iter().all(|plan| plan.is_confirmed()) {
                        ProcedureReadiness::Ready
                    } else {
                        ProcedureReadiness::Unknown
                    }
                }
                Some(BinaryPresence::Unknown) => ProcedureReadiness::Unknown,
                Some(BinaryPresence::Absent) => ProcedureReadiness::Unavailable,
            }
        };
        let allowed = |channel: &ExecChannel| {
            !exclude_target
                || !channel
                    .hops
                    .iter()
                    .chain(channel.exec_target_id.iter())
                    .any(|id| self.campaign.canonical_entity_id(id) == canonical_target)
        };
        let absent_tool = || {
            ExecuteActionError::InvalidInput(format!(
            "procedure '{}' requires tool '{}' which is known to be absent from the execution system",
            procedure.id,
            tool.as_deref().unwrap_or("unknown")
        ))
        };

        let channels = self.channels(
            exclude_target.then_some(canonical_target.as_str()),
            http_response_output_transform(procedure).is_none(),
        );
        if let Some(source_id) = source_hint
            .as_ref()
            .filter(|id| self.campaign.get_system_entity(id).is_some())
        {
            let channel = channels
                .iter()
                .find(|(id, _, _)| id == source_id)
                .map(|(_, _, channel)| channel.clone())
                .ok_or_else(|| {
                    ExecuteActionError::NoExecChannel(
                        if exclude_target && self.campaign.resolve_exec_channel(source_id).is_ok() {
                            format!(
                                "source-side procedure cannot traverse selected target '{}'",
                                target_id
                            )
                        } else {
                            format!(
                                "no viable execution channel to client execution system '{}'",
                                source_id
                            )
                        },
                    )
                })?;
            if !allowed(&channel) {
                return Err(ExecuteActionError::NoExecChannel(format!(
                    "source-side procedure cannot traverse selected target '{}'",
                    target_id
                )));
            }
            let readiness = readiness_on(source_id, &channel);
            if readiness == ProcedureReadiness::Unavailable {
                return Err(absent_tool());
            }
            return Ok(make_plan(Some(channel), readiness));
        }

        let mut candidates = channels
            .iter()
            .filter(|(_, _, channel)| allowed(channel))
            // Legacy backend selectors constrain the environment, rather than
            // making the API resource an implicit physical destination.
            .filter(|(_, _, channel)| {
                source_hint.as_ref().is_none_or(|hint| {
                    channel.backend_id == *hint || channel.backend_id == format!("c2/{hint}")
                })
            })
            .map(|(id, cost, channel)| (readiness_on(id, channel), id, *cost, channel))
            .collect::<Vec<_>>();
        // Confirmed tools precede unknown tools, then lower-cost routes.
        // Entity IDs break ties, independent of graph/hash insertion order.
        let readiness_rank = |readiness| match readiness {
            ProcedureReadiness::Ready => 0,
            ProcedureReadiness::Unknown => 1,
            ProcedureReadiness::Unavailable => 2,
        };
        candidates.sort_by(
            |(left, left_id, left_cost, _), (right, right_id, right_cost, _)| {
                readiness_rank(*left)
                    .cmp(&readiness_rank(*right))
                    .then_with(|| left_cost.total_cmp(right_cost))
                    .then_with(|| left_id.cmp(right_id))
            },
        );
        match candidates.first() {
            Some((ProcedureReadiness::Unavailable, _, _, _)) => Err(absent_tool()),
            Some((readiness, _, _, channel)) => Ok(make_plan(Some((*channel).clone()), *readiness)),
            None => Err(ExecuteActionError::NoExecChannel(if exclude_target {
                format!(
                    "no compromised execution source other than target '{}' is available",
                    target_id
                )
            } else {
                format!(
                    "no reachable client execution system is available for procedure '{}'",
                    procedure.id
                )
            })),
        }
    }
}
