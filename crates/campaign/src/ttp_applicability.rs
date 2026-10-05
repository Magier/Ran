use armory::ProcedureOperation;
use ran_domain::{
    AccessLevel, Entity as _, K8sCredential, Listener, Pod, ServiceAccount, SessionStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Campaign, CampaignEntityRef};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthIdentitySummary {
    pub id: String,
    pub name: String,
    pub kind: String,
}

pub fn procedure_uses_k8s_auth(procedure: &armory::Procedure) -> bool {
    procedure.k8s_request.is_some()
        || procedure.command.contains("${K8S_AUTH}")
        || procedure
            .http_request
            .as_ref()
            .and_then(|request| request.get("authentication"))
            .is_some()
        || procedure.command.contains("kubectl ")
        || matches!(
            procedure.operation,
            ProcedureOperation::KubernetesExecSession { .. }
                | ProcedureOperation::SelfSubjectRulesReview { .. }
        )
}

pub fn ttp_uses_k8s_auth(ttp: &armory::Ttp) -> bool {
    ttp.procedures.iter().any(procedure_uses_k8s_auth)
}

pub fn credential_has_replayable_auth(credential: &K8sCredential) -> bool {
    credential
        .token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty())
        || (credential
            .cert_data
            .as_deref()
            .is_some_and(|cert| !cert.trim().is_empty())
            && credential
                .key_data
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty()))
}

/// A local kubectl procedure without an authentication marker uses the Ran
/// host's configured/default kubeconfig. It therefore does not need an
/// identity entity to be applicable.
fn ttp_can_use_default_kubeconfig(ttp: &armory::Ttp) -> bool {
    ttp.procedures.iter().any(|procedure| {
        procedure.is_local_command == Some(true)
            && procedure.command.contains("kubectl ")
            && !procedure.command.contains("${K8S_AUTH}")
    })
}

fn entitlements_satisfy(ttp: &armory::Ttp, entitlements: &[ran_domain::RbacPermission]) -> bool {
    let Some(Value::Array(requirements)) = ttp.requires.get("rbacPermissions") else {
        return true;
    };
    requirements.iter().all(|requirement| {
        let Some(requirement) = requirement.as_object() else {
            return true;
        };
        let verb = requirement
            .get("verb")
            .and_then(Value::as_str)
            .unwrap_or("");
        let resource = requirement
            .get("resourceType")
            .and_then(Value::as_str)
            .unwrap_or("");
        (verb.is_empty() || resource.is_empty())
            || entitlements
                .iter()
                .any(|permission| permission.satisfies(verb, resource))
    })
}

/// Return the executable identities that witness this action's existential
/// authentication/RBAC precondition. Identity-inspection actions are pinned to
/// the selected identity target; other Kubernetes actions may use any matching
/// active kubeconfig or captured ServiceAccount token.
pub fn eligible_auth_identities(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    target_id: &str,
) -> Vec<AuthIdentitySummary> {
    if !ttp_uses_k8s_auth(ttp) {
        return Vec::new();
    }

    let identity_target = ttp
        .requires
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| matches!(*kind, "ServiceAccount" | "K8sCredential"));
    let mut identities = Vec::new();
    identities.extend(
        campaign
            .entities
            .values::<ServiceAccount>()
            .filter(|account| account.raw_token().is_some())
            .filter(|account| {
                identity_target != Some("ServiceAccount") || account.entity_id().0 == target_id
            })
            .filter(|account| entitlements_satisfy(ttp, &account.entitlements))
            .map(|account| AuthIdentitySummary {
                id: account.entity_id().0,
                name: account.entity_name().to_string(),
                kind: "ServiceAccount".to_string(),
            }),
    );
    identities.extend(
        campaign
            .entities
            .values::<K8sCredential>()
            .filter(|credential| {
                credential.active
                    || campaign.is_operator_host_credential(&credential.entity_id())
                    || credential_has_replayable_auth(credential)
            })
            .filter(|credential| {
                identity_target != Some("K8sCredential") || credential.entity_id().0 == target_id
            })
            .filter(|credential| entitlements_satisfy(ttp, &credential.entitlements))
            .map(|credential| AuthIdentitySummary {
                id: credential.entity_id().0,
                name: credential.entity_name().to_string(),
                kind: "K8sCredential".to_string(),
            }),
    );
    identities.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    identities
}

/// The per-target facts the applicability predicates need to evaluate a TTP
/// against a concrete entity. Resolved once per target via
/// [`resolve_target_context`] and reused across every candidate TTP.
#[derive(Debug, Clone)]
pub struct TargetContext {
    pub target_id: String,
    /// Entity kind string (e.g. `"Pod"`, `"ServiceAccount"`).
    pub target_kind: String,
    /// `true` when the target is a machine the engagement acts on.
    ///
    /// This is target-ness, *not* the `SystemEntity` capability - see the
    /// comment where it is computed in [`resolve_target_context`]. It is the
    /// single definition site for "in play", and the only input to
    /// `requires.kind: System`.
    pub is_system: bool,
    /// Effective access level. For pods this includes the "reachable pod ⇒ Exec"
    /// inference: a pod with a kubectl-exec channel is treated as Exec even
    /// before a TTP has explicitly raised its `access_level`.
    pub access_level: AccessLevel,
    /// `true` when the target is a ServiceAccount holding a non-empty token.
    pub has_token: bool,
    /// `true` when the target system has at least one live shell session.
    pub active_session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementStatus {
    Supported,
    Uncertain,
    Contradicted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementState {
    pub key: String,
    pub status: RequirementStatus,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<ran_domain::SoftwareFact>,
}

pub fn software_requirement_states(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    target_id: &str,
) -> Vec<RequirementState> {
    let facts = software_facts_for_target(campaign, target_id);
    ttp.requires
        .iter()
        .filter(|(key, _)| key.starts_with("pkg:"))
        .map(|(key, constraint)| grade_software_requirement(key, constraint, &facts))
        .collect()
}

fn software_facts_for_target<'a>(
    campaign: &'a Campaign,
    target_id: &str,
) -> Vec<&'a ran_domain::SoftwareFact> {
    let canonical = campaign.canonical_entity_id(target_id);
    let mut facts = campaign
        .get_entities()
        .into_iter()
        .find(|entity| entity.entity_id().0 == canonical)
        .map(|entity| match entity {
            CampaignEntityRef::Pod(pod) => pod.system.software.iter().collect(),
            CampaignEntityRef::Node(node) => node.system.software.iter().collect(),
            CampaignEntityRef::UnknownSystem(system) => system.system.software.iter().collect(),
            CampaignEntityRef::OperatorHost(host) => host.system.software.iter().collect(),
            CampaignEntityRef::Deployment(deployment) => deployment.software.iter().collect(),
            CampaignEntityRef::AppService(service) => service.software.iter().collect(),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    for service_id in campaign
        .graph
        .targets_of(&ran_domain::EntityId::new(&canonical), "hosts-service")
    {
        if let Some(service) = campaign
            .entities
            .get::<ran_domain::AppService>()
            .get(service_id)
        {
            facts.extend(service.software.iter());
        }
    }
    facts
}

fn grade_software_requirement(
    key: &str,
    constraint: &Value,
    all_facts: &[&ran_domain::SoftwareFact],
) -> RequirementState {
    let matching_identity = all_facts
        .iter()
        .copied()
        .filter(|fact| fact.purl.split('@').next() == Some(key))
        .collect::<Vec<_>>();
    if matching_identity.is_empty() {
        return RequirementState {
            key: key.to_string(),
            status: RequirementStatus::Uncertain,
            reason: "no matching software identity observation for this target".to_string(),
            evidence: Vec::new(),
        };
    }

    let evidence = matching_identity
        .iter()
        .map(|fact| (*fact).clone())
        .collect::<Vec<_>>();
    let authoritative = matching_identity
        .iter()
        .copied()
        .filter(|fact| fact.confidence == ran_domain::NameConfidence::Authoritative)
        .collect::<Vec<_>>();
    if authoritative.is_empty() {
        return RequirementState {
            key: key.to_string(),
            status: RequirementStatus::Uncertain,
            reason: "software identity is supported only by derived evidence".to_string(),
            evidence,
        };
    }

    let comparisons = authoritative
        .iter()
        .map(|fact| software_constraint_matches(fact.version.as_deref(), constraint))
        .collect::<Vec<_>>();
    let matches = comparisons.contains(&Some(true));
    let unknown = comparisons.iter().any(Option::is_none);
    if matches {
        RequirementState {
            key: key.to_string(),
            status: RequirementStatus::Supported,
            reason: "authoritative software identity observation supports the requirement"
                .to_string(),
            evidence,
        }
    } else if unknown {
        RequirementState {
            key: key.to_string(),
            status: RequirementStatus::Uncertain,
            reason:
                "software identity is authoritative but its version is unknown or not comparable"
                    .to_string(),
            evidence,
        }
    } else {
        RequirementState {
            key: key.to_string(),
            status: RequirementStatus::Contradicted,
            reason: "authoritative software version observation contradicts the requirement"
                .to_string(),
            evidence,
        }
    }
}

fn software_constraint_matches(version: Option<&str>, constraint: &Value) -> Option<bool> {
    let alternatives = match constraint {
        Value::Array(values) => values.iter().filter_map(Value::as_str).collect::<Vec<_>>(),
        Value::String(value) => vec![value.as_str()],
        _ => return None,
    };
    if alternatives
        .iter()
        .any(|alternative| alternative.trim() == "*")
    {
        return Some(true);
    }
    let version = parse_software_version(version?)?;
    let mut parsed_any = false;
    let matched = alternatives.into_iter().any(|alternative| {
        let alternative = alternative.trim();
        if alternative.is_empty() {
            return false;
        }
        if alternative
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
        {
            return parse_software_version(alternative)
                .map(|required| {
                    parsed_any = true;
                    version == required
                })
                .unwrap_or(false);
        }
        let normalized = alternative
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(", ");
        semver::VersionReq::parse(&normalized)
            .map(|required| {
                parsed_any = true;
                required.matches(&version)
            })
            .unwrap_or(false)
    });
    parsed_any.then_some(matched)
}

fn parse_software_version(value: &str) -> Option<semver::Version> {
    let value = value.trim_start_matches('v');
    semver::Version::parse(value).ok().or_else(|| {
        (value.matches('.').count() == 1)
            .then(|| semver::Version::parse(&format!("{value}.0")).ok())
            .flatten()
    })
}

/// Resolve the [`TargetContext`] for `target_id` from current campaign state.
///
/// Returns `None` when no entity with that id exists. This centralizes the
/// per-target fact resolution that the applicability predicates depend on, so
/// the API handler, the MCP tool, and the action scorer all agree.
pub fn resolve_target_context(campaign: &Campaign, target_id: &str) -> Option<TargetContext> {
    let entities = campaign.get_entities();
    let entity = entities
        .into_iter()
        .find(|e| e.entity_id().0 == target_id)?;

    let target_kind = entity.entity_kind().to_string();
    // The single definition site for target-ness: which machines the
    // engagement acts on. `requires.kind: System` is a wildcard over this set
    // (see `kind_matches_target_kind`), so adding a variant here makes every
    // host-oriented TTP in the armory applicable to it.
    //
    // `OperatorHost` is deliberately absent even though it *does* implement
    // `SystemEntity`. That asymmetry is the point, not an oversight: the
    // operator host is a real machine (it has binaries and IPs, which
    // `get_system_entity` exposes) but it is the operator's own laptop, not
    // something to run post-exploitation TTPs against. Adding it here lights up
    // Install Package, Drop binary, NSenter and ~20 others against it. If you
    // came here to make this list agree with `Campaign::is_system_entity_id`,
    // read the doc comment on `OperatorHost` first - the two lists answer
    // different questions and are meant to differ.
    let is_system = matches!(
        &entity,
        CampaignEntityRef::Pod(_)
            | CampaignEntityRef::Node(_)
            | CampaignEntityRef::UnknownSystem(_)
    );

    let access_level = match &entity {
        CampaignEntityRef::Pod(p) => {
            // A reachable pod (kubectl-exec channel exists) implies exec access
            // even before a TTP has explicitly updated the access_level field.
            if p.system.access_level == AccessLevel::None
                && campaign.reachable_pods().contains(&entity.entity_id().0)
            {
                AccessLevel::Exec
            } else {
                p.system.access_level
            }
        }
        CampaignEntityRef::Node(n) => n.system.access_level,
        CampaignEntityRef::UnknownSystem(s) => s.system.access_level,
        _ => AccessLevel::None,
    };

    let has_token = match &entity {
        CampaignEntityRef::ServiceAccount(sa) => sa.raw_token().is_some(),
        _ => false,
    };
    let sessions: &[ran_domain::SessionInfo] = match &entity {
        CampaignEntityRef::Pod(pod) => pod.system.sessions.as_slice(),
        CampaignEntityRef::Node(node) => node.system.sessions.as_slice(),
        CampaignEntityRef::UnknownSystem(system) => system.system.sessions.as_slice(),
        _ => &[],
    };
    let active_session = sessions
        .iter()
        .any(|session| session.status == SessionStatus::Active);

    Some(TargetContext {
        target_id: target_id.to_string(),
        target_kind,
        is_system,
        access_level,
        has_token,
        active_session,
    })
}

/// Aggregate applicability gate: `true` when `ttp` can run against the target
/// described by `tc` given current campaign state. This is the single source of
/// truth combining all supported precondition predicates plus the kind match.
pub fn ttp_applicable_for_target(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    tc: &TargetContext,
) -> bool {
    ttp_applicable_with_context(
        ttp,
        campaign,
        tc,
        &crate::ExecutionPlanningContext::new(campaign),
    )
}

pub fn ttp_applicable_with_context(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    tc: &TargetContext,
    planner: &crate::ExecutionPlanningContext<'_>,
) -> bool {
    if !planner.belongs_to(campaign) {
        return false;
    }
    ttp_target_scope_satisfied(ttp, tc)
        && ttp_auth_satisfied_for_target(ttp, campaign, tc)
        && ttp_execution_source_satisfied(ttp, campaign, tc, planner)
        && ttp_exists_satisfied(ttp, campaign)
        && ttp_has_listener_satisfied(ttp, campaign)
        && ttp_has_session_satisfied(ttp, campaign, &tc.target_id)
        && ttp_session_upgrade_satisfied(ttp, campaign, &tc.target_id)
        && (!tc.is_system || ttp_access_level_satisfied(ttp, tc.access_level))
        && ttp_has_token_satisfied(ttp, tc.has_token)
        && ttp_active_session_satisfied(ttp, tc.active_session)
        && ttp_filesystem_access_satisfied(ttp, campaign, tc)
        && ttp_pod_requirements_satisfied(ttp, campaign, &tc.target_id)
        && ttp_namespace_access_satisfied(ttp, campaign, &tc.target_id)
        && ttp_related_satisfied(ttp, &tc.target_id, &tc.target_kind, campaign)
        && software_requirement_states(ttp, campaign, &tc.target_id)
            .iter()
            .all(|requirement| requirement.status != RequirementStatus::Contradicted)
        && crate::campaign::execution::best_tool_readiness_with_context(ttp, campaign, &tc.target_id, planner) > 0.0
        // Last: the only gate that touches the filesystem. `&&` short-circuits,
        // so it runs only for targets every cheaper gate already accepted.
        && ttp_operator_tool_satisfied(ttp)
}

/// A structured Ranplant upgrade is useful only while the selected target does
/// not already have a live Ranplant session. A plain TCP shell remains eligible
/// because it is precisely the transport this operation upgrades.
pub fn ttp_session_upgrade_satisfied(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    target_id: &str,
) -> bool {
    let starts_ranplant = ttp.procedures.iter().any(|procedure| {
        matches!(
            procedure.operation,
            ProcedureOperation::StartRanplantSession { .. }
        )
    });
    if !starts_ranplant {
        return true;
    }

    let canonical_target = campaign.canonical_entity_id(target_id);
    campaign
        .get_system_entity(&canonical_target)
        .is_none_or(|system| {
            !system.entity().system().sessions.iter().any(|session| {
                session.status == SessionStatus::Active
                    && session.kind.eq_ignore_ascii_case("ranplant")
            })
        })
}

/// Require a physical execution source when the procedure cannot run on its
/// semantic target. This covers source-side lateral movement and authenticated
/// Kubernetes actions against API resources or identity entities.
fn ttp_execution_source_satisfied(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    tc: &TargetContext,
    planner: &crate::ExecutionPlanningContext<'_>,
) -> bool {
    use crate::campaign::execution_planning::ProcedureExecutionSemantics;
    let client_procedures = ttp
        .procedures
        .iter()
        .filter(|procedure| {
            ProcedureExecutionSemantics::from_definition(procedure).needs_client_plan()
        })
        .collect::<Vec<_>>();
    if !client_procedures.is_empty() {
        let has_client_source = client_procedures.iter().any(|procedure| {
            planner
                .plan(ttp, procedure, &tc.target_id, None, None)
                .is_ok()
        });
        if has_client_source || client_procedures.len() == ttp.procedures.len() {
            return has_client_source;
        }
        // Mixed actions can still be witnessed by a host/local procedure.
    }
    let tactic = ttp
        .tactic
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();

    if tactic.eq_ignore_ascii_case("LateralMovement") && !ttp_uses_k8s_auth(ttp) {
        return campaign.resolve_exec_source().is_ok();
    }

    // Authenticated Kubernetes procedures against API resources and identity
    // entities run from a controlled system, not on the semantic target. A
    // locally usable kubeconfig is the exception because BuiltinC2 can realize
    // it without a remote execution source.
    if ttp_uses_k8s_auth(ttp) && campaign.get_system_entity(&tc.target_id).is_none() {
        let has_local_identity = eligible_auth_identities(ttp, campaign, &tc.target_id)
            .iter()
            .any(|identity| identity.kind == "K8sCredential");
        return has_local_identity
            || ttp_can_use_default_kubeconfig(ttp)
            || campaign.resolve_exec_source().is_ok();
    }

    true
}

/// Keep the selected entity as the semantic target. Kubernetes actions without
/// an explicit kind belong to cluster/namespace views or to the selected
/// executable identity; they must not leak into Pod and other resource views
/// merely because some unrelated credential can authorize them.
fn ttp_target_scope_satisfied(ttp: &armory::Ttp, tc: &TargetContext) -> bool {
    if ttp.requires.contains_key("kind") || !ttp_uses_k8s_auth(ttp) {
        return ttp_is_applicable_for_target_kind(ttp, &tc.target_kind, tc.is_system);
    }

    matches!(
        tc.target_kind.as_str(),
        "Cluster" | "Namespace" | "K8sCredential" | "ServiceAccount"
    )
}

/// Require a realizable authentication identity for Kubernetes actions. When
/// the semantic target is itself an identity, that exact identity must be the
/// witness; resource targets may use any eligible identity selected through
/// Authenticate As.
fn ttp_auth_satisfied_for_target(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    tc: &TargetContext,
) -> bool {
    if ttp_uses_k8s_auth(ttp) {
        let identities = eligible_auth_identities(ttp, campaign, &tc.target_id);
        let selected_identity =
            matches!(tc.target_kind.as_str(), "K8sCredential" | "ServiceAccount");
        let explicitly_targets_identity = ttp
            .requires
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| matches!(kind, "K8sCredential" | "ServiceAccount"));
        let identity_must_match = selected_identity
            && (!ttp.requires.contains_key("kind") || explicitly_targets_identity);
        return (!identities.is_empty() || ttp_can_use_default_kubeconfig(ttp))
            && (!identity_must_match
                || identities
                    .iter()
                    .any(|identity| identity.id == tc.target_id));
    }

    ttp_rbac_satisfied(ttp, campaign)
}

/// Returns `false` only when the action cannot run because *every* procedure's
/// required tool is **known absent** on the target. Procedures whose tool is
/// present or unknown, operator-side procedures (local / recon / resource-dev),
/// and non-system targets all pass - we can't rule them out.
///
/// Shares [`best_tool_readiness`](crate::campaign::execution::best_tool_readiness)
/// with the `reliability` scoring consideration so the gate and the soft
/// preference agree on which procedures can run.
pub fn ttp_tool_satisfied(ttp: &armory::Ttp, campaign: &Campaign, tc: &TargetContext) -> bool {
    crate::campaign::execution::best_tool_readiness(ttp, campaign, &tc.target_id) > 0.0
}

/// Returns `true` when the TTP's `requires.kind` is satisfied by the target's
/// kind. A disabled TTP is never applicable. Absent `requires.kind` → satisfied.
pub fn ttp_is_applicable_for_target_kind(
    ttp: &armory::Ttp,
    target_kind: &str,
    is_system_target: bool,
) -> bool {
    if ttp.status.eq_ignore_ascii_case("disabled") {
        return false;
    }

    let Some(kind_req) = ttp.requires.get("kind") else {
        return true;
    };

    match kind_req {
        Value::String(kind) => kind_matches_target_kind(kind, target_kind, is_system_target),
        Value::Array(kinds) => kinds.iter().any(|k| {
            k.as_str()
                .map(|s| kind_matches_target_kind(s, target_kind, is_system_target))
                .unwrap_or(true)
        }),
        _ => true,
    }
}

/// Returns `true` if `required_kind` (from a TTP's `requires.kind`) is satisfied
/// by the target entity.
///
/// `required_kind == "System"` is an abstract requirement satisfied by any entity
/// that implements `SystemEntity` - i.e. wherever `is_system_target` is `true`.
/// This is driven by the flag rather than a hardcoded list of kind strings, so
/// future `SystemEntity` implementors (e.g. `UnknownSystem`) are picked up
/// automatically without touching this function.
fn kind_matches_target_kind(
    required_kind: &str,
    target_kind: &str,
    is_system_target: bool,
) -> bool {
    if required_kind.eq_ignore_ascii_case(target_kind) {
        return true;
    }

    required_kind.eq_ignore_ascii_case("System") && is_system_target
}

/// Returns `true` when the TTP's `exists` pre-conditions are met by the
/// current campaign state.
///
/// - No `exists` in `requires` means satisfied.
/// - Each item may be a kind string or an object with `kind` plus optional
///   exact `name` and `namespace` constraints.
/// - Unknown kinds fail safe because no graph entity can match them.
pub fn ttp_exists_satisfied(ttp: &armory::Ttp, campaign: &Campaign) -> bool {
    let Some(Value::Array(items)) = ttp.requires.get("exists") else {
        return true;
    };

    if items.is_empty() {
        return true;
    }

    let entities = campaign.get_entities();
    items.iter().all(|item| {
        let (kind, name, namespace) = match item {
            Value::String(kind) => (kind.as_str(), None, None),
            Value::Object(requirement) => (
                requirement
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                requirement.get("name").and_then(Value::as_str),
                requirement.get("namespace").and_then(Value::as_str),
            ),
            _ => return false,
        };
        let kind = kind.trim();
        !kind.is_empty()
            && entities.iter().any(|entity| {
                entity.entity_kind().eq_ignore_ascii_case(kind)
                    && name.is_none_or(|required| entity.entity_name() == required)
                    && namespace.is_none_or(|required| entity.namespace() == Some(required))
            })
    })
}

/// Returns `true` when the TTP's RBAC requirements are satisfied by at least
/// one captured ServiceAccount or kubeconfig credential in the campaign.
///
/// - No `rbacPermissions` in `requires` → satisfied (no restriction).
/// - `rbacPermissions` is present but no known identity has matching
///   entitlements → not satisfied.
/// - At least one ServiceAccount or K8sCredential must satisfy **all** required
///   permissions.
pub fn ttp_rbac_satisfied(ttp: &armory::Ttp, campaign: &Campaign) -> bool {
    let Some(Value::Array(reqs)) = ttp.requires.get("rbacPermissions") else {
        return true;
    };

    if reqs.is_empty() {
        return true;
    }

    campaign
        .entities
        .values::<ServiceAccount>()
        .filter(|account| account.raw_token().is_some())
        .any(|account| entitlements_satisfy(ttp, &account.entitlements))
        || campaign
            .entities
            .values::<K8sCredential>()
            .filter(|credential| {
                credential.active || campaign.is_operator_host_credential(&credential.entity_id())
            })
            .any(|credential| entitlements_satisfy(ttp, &credential.entitlements))
}

/// Gate actions that require a live shell session on the selected target.
pub fn ttp_active_session_satisfied(ttp: &armory::Ttp, active: bool) -> bool {
    match ttp.requires.get("activeSession").and_then(Value::as_bool) {
        Some(true) => active,
        _ => true,
    }
}

/// Gate actions that require executable access to the selected system's
/// filesystem.
///
/// Direct execution on the target satisfies the requirement. A Node target is
/// also satisfied by an executable Pod scheduled on that node when the Pod has
/// a hostPath mount, because that mount exposes at least part of the node's
/// filesystem to the Pod.
pub fn ttp_filesystem_access_satisfied(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    tc: &TargetContext,
) -> bool {
    let Some(required) = ttp
        .requires
        .get("filesystemAccess")
        .and_then(Value::as_bool)
    else {
        return true;
    };
    if !required {
        return true;
    }

    if tc.access_level >= AccessLevel::Exec || tc.active_session {
        return true;
    }

    if tc.target_kind != "Node" {
        return false;
    }

    let Some(node) = campaign
        .get_entities()
        .into_iter()
        .find(|entity| entity.entity_id().0 == tc.target_id)
        .and_then(|entity| match entity {
            CampaignEntityRef::Node(node) => Some(node),
            _ => None,
        })
    else {
        return false;
    };
    let reachable_pods = campaign.reachable_pods();

    campaign.entities.values::<Pod>().any(|pod| {
        pod.node_name.as_deref() == Some(node.entity_name())
            && pod.has_host_paths()
            && (pod.system.can_exec()
                || pod
                    .system
                    .sessions
                    .iter()
                    .any(|session| session.status == SessionStatus::Active)
                || reachable_pods.contains(&pod.entity_id().0))
    })
}

/// Evaluate Pod-specific runtime requirements against the selected Pod.
/// Security facts are tri-state, so unknown values remain applicable while a
/// known false value blocks the action. HostPath requirements are strict
/// because their mount point is needed to ground the procedure.
pub fn ttp_pod_requirements_satisfied(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    target_id: &str,
) -> bool {
    let privileged = ttp
        .requires
        .get("Pod.securityContext.privileged")
        .and_then(Value::as_bool);
    let host_pid = ttp
        .requires
        .get("Pod.securityContext.hostPID")
        .and_then(Value::as_bool);
    let host_path = ttp.requires.get("Pod.hostPath");
    if privileged.is_none() && host_pid.is_none() && host_path.is_none() {
        return true;
    }

    let Some(pod) = campaign
        .entities
        .values::<Pod>()
        .find(|pod| pod.entity_id().0 == campaign.canonical_entity_id(target_id))
    else {
        return false;
    };

    let confidence_satisfies =
        |actual: ran_domain::Confidence, required: Option<bool>| match required {
            Some(true) => actual != ran_domain::Confidence::No,
            Some(false) => actual != ran_domain::Confidence::Yes,
            None => true,
        };
    confidence_satisfies(pod.privileged, privileged)
        && confidence_satisfies(pod.host_pid, host_pid)
        && match host_path {
            None => true,
            Some(Value::Bool(true)) => pod.has_host_paths(),
            Some(Value::Bool(false)) => !pod.has_host_paths(),
            Some(Value::String(required_root)) => pod.volume_mounts.iter().any(|mount| {
                mount.is_host_path && paths_equivalent(&mount.mount_root, required_root)
            }),
            Some(Value::Array(required_roots)) => required_roots.iter().all(|required_root| {
                required_root.as_str().is_some_and(|required_root| {
                    pod.volume_mounts.iter().any(|mount| {
                        mount.is_host_path && paths_equivalent(&mount.mount_root, required_root)
                    })
                })
            }),
            Some(_) => false,
        }
}

fn paths_equivalent(actual: &str, required: &str) -> bool {
    fn normalize(path: &str) -> &str {
        let trimmed = path.trim();
        if trimmed == "/" {
            "/"
        } else {
            trimmed.trim_end_matches('/')
        }
    }
    normalize(actual) == normalize(required)
}

/// Gate actions on namespace access learned from earlier `nsenter` executions.
/// Unknown access remains applicable. A recognized denial blocks the action
/// until a newer successful execution proves that namespace accessible.
pub fn ttp_namespace_access_satisfied(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    target_id: &str,
) -> bool {
    let Some(Value::Array(required)) = ttp.requires.get("linuxNamespaceAccess") else {
        return true;
    };

    let target_id = campaign.canonical_entity_id(target_id);
    required.iter().all(|value| {
        let Some(namespace) = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return false;
        };

        campaign
            .get_execution_records()
            .iter()
            .rev()
            .filter(|record| {
                let observed_system_id = if record.exec_system_id.trim().is_empty() {
                    &record.target_id
                } else {
                    &record.exec_system_id
                };
                record.ttp_id == ttp.id
                    && campaign.canonical_entity_id(observed_system_id) == target_id
            })
            .find_map(|record| nsenter_namespace_observation(record, namespace))
            .unwrap_or(true)
    })
}

fn nsenter_namespace_observation(
    record: &crate::execution_record::ExecutionRecord,
    namespace: &str,
) -> Option<bool> {
    if !record.command.to_ascii_lowercase().contains("nsenter")
        && !record.procedure_id.eq_ignore_ascii_case("nsenter")
    {
        return None;
    }

    let namespace = namespace.to_ascii_lowercase();
    if !record.success {
        let kernel_namespace = match namespace.as_str() {
            "mount" => "mnt",
            other => other,
        };
        let output =
            format!("{}\n{}", record.fail_reason, record.results.join("\n")).to_ascii_lowercase();
        let denied = [
            format!("namespace 'ns/{kernel_namespace}' failed: operation not permitted"),
            format!("namespace \"ns/{kernel_namespace}\" failed: operation not permitted"),
        ]
        .iter()
        .any(|signature| output.contains(signature));
        return denied.then_some(false);
    }

    let arg_name = namespace.to_ascii_uppercase();
    let enabled_by_arg = record.args.get(&arg_name).is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes"
        )
    });
    let enabled_by_command = record
        .command
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case(&format!("--{namespace}")));
    (enabled_by_arg || enabled_by_command).then_some(true)
}

/// Returns `true` when the TTP's `c2.has-listener` requirement is satisfied.
///
/// - No `c2.has-listener` in `requires` → satisfied (no restriction).
/// - `c2.has-listener: true` → some C2Server must have a bound listener.
/// - `c2.has-listener: false` → no C2Server may have one.
///
/// Unlike `exists: [Listener]` this is about the C2 the action runs on, so it
/// gates actions that operate *on* a listener (stopping one) rather than
/// actions that merely need one to exist somewhere.
pub fn ttp_has_listener_satisfied(ttp: &armory::Ttp, campaign: &Campaign) -> bool {
    let Some(required) = ttp.requires.get("c2.has-listener").and_then(Value::as_bool) else {
        return true;
    };
    let any_listener = campaign.entities.values::<Listener>().next().is_some();
    any_listener == required
}

/// Returns `true` when the selected C2 satisfies its `c2.has-session`
/// requirement.
///
/// A session is an execution-channel relation owned by a specific C2, not a
/// property that can be borrowed from another C2. Broken channels do not count
/// as live sessions.
pub fn ttp_has_session_satisfied(ttp: &armory::Ttp, campaign: &Campaign, target_id: &str) -> bool {
    let Some(required) = ttp.requires.get("c2.has-session").and_then(Value::as_bool) else {
        return true;
    };

    let has_session = campaign.get_relations().iter().any(|relation| {
        relation.name == "c2.session"
            && relation.source_id == target_id
            && relation.session_id.is_some()
            && !relation.broken
    });
    has_session == required
}

/// Returns `true` when the TTP's operator-side tool requirement is met.
///
/// `requires["c2.has-tool"]` names a tool - or an array of them - that must
/// exist on the machine running Ran, as opposed to on the target.
/// [`ttp_tool_satisfied`] cannot answer this: it reads the *target's* binary
/// map, and an operator-side procedure never touches one, so a TTP that shells
/// out locally is otherwise ungated no matter what it needs installed.
///
/// Unlike the target-side gate there is no "unknown" state to be generous
/// about - `PATH` either resolves the tool or it does not - so a missing tool
/// withdraws the action instead of offering one that cannot run.
pub fn ttp_operator_tool_satisfied(ttp: &armory::Ttp) -> bool {
    let Some(required) = ttp.requires.get("c2.has-tool") else {
        return true;
    };
    match required {
        Value::String(tool) => operator_has_tool(tool),
        // Unparseable entries are not requirements we can check, so they pass
        // rather than hiding an action for a malformed line of YAML.
        Value::Array(tools) => tools
            .iter()
            .all(|tool| tool.as_str().map(operator_has_tool).unwrap_or(true)),
        _ => true,
    }
}

/// Whether `tool` resolves to an executable on the operator host.
///
/// This mirrors what `std::process::Command` does when the executor spawns the
/// tool, so it predicts that spawn rather than guessing at it.
fn operator_has_tool(tool: &str) -> bool {
    let tool = tool.trim();
    if tool.is_empty() {
        return false;
    }
    // A path is used verbatim by `Command`, without a `PATH` search.
    if tool.contains(std::path::MAIN_SEPARATOR) {
        return is_executable_file(std::path::Path::new(tool));
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable_file(&dir.join(tool)))
}

fn is_executable_file(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Returns `true` when the TTP's `has-token` requirement is satisfied by the target entity.
///
/// - No `has-token` in `requires` → satisfied (no restriction).
/// - `has-token: true` → the target must have a non-empty token.
/// - `has-token: false` (or any other value) → always satisfied.
pub fn ttp_has_token_satisfied(ttp: &armory::Ttp, target_has_token: bool) -> bool {
    match ttp.requires.get("has-token").and_then(Value::as_bool) {
        Some(true) => target_has_token,
        _ => true,
    }
}

/// Returns `true` when the target entity's access level satisfies the TTP's requirement.
///
/// Access level is **opt-in**: when `requires.accessLevel` is absent the check
/// always passes.  Three tactics are also unconditionally exempt: `Initial Access`,
/// `Lateral Movement`, and `Resource Development`.
///
/// Tactic names are normalised (spaces stripped, ASCII lower-cased) before
/// comparison so `"InitialAccess"` (directory-derived) and `"Initial Access"`
/// (YAML-declared) are treated identically.
pub fn ttp_access_level_satisfied(ttp: &armory::Ttp, target_access_level: AccessLevel) -> bool {
    fn normalise(s: &str) -> String {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase()
    }

    let tactic = normalise(&ttp.tactic);
    if matches!(
        tactic.as_str(),
        "initialaccess" | "lateralmovement" | "resourcedevelopment"
    ) {
        return true;
    }

    // Only enforce an access level when one is explicitly declared.
    let Some(declared) = ttp.requires.get("accessLevel").and_then(Value::as_str) else {
        return true;
    };

    if declared == "none" {
        return true;
    }

    target_access_level >= AccessLevel::Exec
}

/// Returns `true` when the TTP's `related` pre-conditions are met by entities
/// in the campaign that are related to the selected target.
///
/// - No `related` in `requires` → satisfied.
/// - Each entry must declare `kind`; `accessLevel` is optional.
/// - Currently supported relationships:
///   - target `ServiceAccount` + related `Pod`: finds pods that mount the SA
///     and, if `accessLevel` is set, requires at least one to have exec access
///     or be reachable via kubectl-exec.
/// - Unknown `(target_kind, related_kind)` combinations → satisfied (fail open
///   so future relationships can be added to YAMLs before the code lands).
pub fn ttp_related_satisfied(
    ttp: &armory::Ttp,
    target_id: &str,
    target_kind: &str,
    campaign: &Campaign,
) -> bool {
    let Some(Value::Array(related)) = ttp.requires.get("related") else {
        return true;
    };
    if related.is_empty() {
        return true;
    }

    related.iter().all(|entry| {
        let Some(obj) = entry.as_object() else {
            return true;
        };
        let related_kind = obj.get("kind").and_then(Value::as_str).unwrap_or("");
        let requires_access = obj.get("accessLevel").and_then(Value::as_str).unwrap_or("");

        match (target_kind, related_kind) {
            ("ServiceAccount", "Pod") => {
                let Some(sa) = campaign
                    .entities
                    .values::<ServiceAccount>()
                    .find(|sa| sa.entity_id().0 == target_id)
                else {
                    return false;
                };
                let sa_name = sa.entity_name();
                let sa_ns = sa.namespace().unwrap_or("");
                let reachable = campaign.reachable_pods();
                campaign.entities.values::<Pod>().any(|pod| {
                    let mounts_sa = pod.service_account_name.as_deref() == Some(sa_name)
                        && pod.namespace() == Some(sa_ns);
                    if !mounts_sa {
                        return false;
                    }
                    requires_access.is_empty()
                        || pod.system.access_level >= AccessLevel::Exec
                        || reachable.contains(&pod.entity_id().0)
                })
            }
            _ => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use armory::{Procedure, ProcedureOperation, Ttp};
    use ran_domain::{
        C2Server, Confidence, Entity, K8sCluster, K8sCredential, K8sNode, KnowledgeProvenance,
        Listener, Mount, NameConfidence, Pod, RbacPermission, Redirector, ServiceAccount,
        SessionChannel, SessionInfo, SessionStatus, SoftwareFact, Uses,
    };
    use serde_json::json;

    use ran_domain::AccessLevel;

    use super::{
        eligible_auth_identities, resolve_target_context, software_requirement_states,
        ttp_access_level_satisfied, ttp_applicable_for_target, ttp_exists_satisfied,
        ttp_has_listener_satisfied, ttp_has_session_satisfied, ttp_namespace_access_satisfied,
        ttp_operator_tool_satisfied, ttp_pod_requirements_satisfied, ttp_rbac_satisfied,
        ttp_session_upgrade_satisfied, RequirementStatus,
    };

    fn ttp_with_rbac(verb: &str, resource_type: &str) -> Ttp {
        let mut requires = serde_json::Map::new();
        requires.insert(
            "rbacPermissions".to_string(),
            json!([{"verb": verb, "resourceType": resource_type}]),
        );
        Ttp {
            status: "enabled".to_string(),
            requires,
            ..Ttp::new("test", "Test", "Discovery")
        }
    }

    fn ttp_no_rbac() -> Ttp {
        Ttp {
            status: "enabled".to_string(),
            ..Ttp::new("test", "Test", "Discovery")
        }
    }

    fn kubernetes_ttp_with_rbac(verb: &str, resource_type: &str) -> Ttp {
        let mut ttp = ttp_with_rbac(verb, resource_type);
        ttp.procedures = vec![armory::Procedure::new(
            "kubectl",
            "kubectl get pods ${K8S_AUTH}",
        )];
        ttp
    }

    fn empty_campaign() -> crate::Campaign {
        crate::Campaign::bootstrap("test", K8sCluster::new("test"))
    }

    fn campaign_with_sa(verb: &str, resource_type: &str) -> crate::Campaign {
        let mut c = empty_campaign();
        let mut sa = ServiceAccount::new("attacker", "default");
        sa.token = Some(ServiceAccountToken {
            jwt: JwToken {
                raw: "header.payload.signature".to_string(),
                ..Default::default()
            },
            namespace: "default".to_string(),
            service_account_name: "attacker".to_string(),
            ..Default::default()
        });
        sa.entitlements
            .push(RbacPermission::new(verb, resource_type));
        c.entities.insert_typed(sa);
        c
    }

    #[test]
    fn pod_target_requires_a_realizable_auth_identity_without_action_special_case() {
        let mut campaign = empty_campaign();
        let pod = ran_domain::Pod::new("target", "default");
        let pod_id = pod.entity_id().0;
        campaign.entities.insert_typed(pod);
        let mut requires = serde_json::Map::new();
        requires.insert("kind".to_string(), json!("Pod"));
        let ttp = Ttp {
            requires,
            procedures: vec![armory::Procedure::new(
                "kubectl",
                "kubectl ${K8S_AUTH} exec -n default target -- true",
            )],
            ..Ttp::new("pod-exec", "Pod exec", "Initial Access")
        };
        let context = resolve_target_context(&campaign, &pod_id).expect("Pod target context");
        assert!(!ttp_applicable_for_target(&ttp, &campaign, &context));

        let mut credential = K8sCredential::new("https://cluster.example").with_name("operator");
        credential.active = true;
        campaign.entities.insert_typed(credential);
        assert!(ttp_applicable_for_target(&ttp, &campaign, &context));
    }

    #[test]
    fn local_kubectl_procedure_can_use_rans_default_configuration() {
        let campaign = empty_campaign();
        let mut requires = serde_json::Map::new();
        requires.insert("kind".to_string(), json!("C2"));
        let ttp = Ttp {
            requires,
            procedures: vec![armory::Procedure {
                is_local_command: Some(true),
                ..armory::Procedure::new("kubectl", "kubectl get namespaces")
            }],
            ..Ttp::new("local-kubectl", "Local kubectl", "Execution")
        };

        let context = resolve_target_context(&campaign, "c2/test").expect("C2 target context");
        assert!(ttp_applicable_for_target(&ttp, &campaign, &context));
    }

    fn ttp_with_has_listener(required: bool) -> Ttp {
        let mut ttp = ttp_no_rbac();
        ttp.requires
            .insert("c2.has-listener".to_string(), json!(required));
        ttp
    }

    fn ttp_with_has_session(required: bool) -> Ttp {
        let mut ttp = ttp_no_rbac();
        ttp.requires
            .insert("c2.has-session".to_string(), json!(required));
        ttp
    }

    fn ttp_with_exists(kind: &str) -> Ttp {
        let mut requires = serde_json::Map::new();
        requires.insert("exists".to_string(), json!([kind]));
        Ttp {
            status: "enabled".to_string(),
            requires,
            ..Ttp::new("test", "Test", "Resource Development")
        }
    }

    fn ttp_with_tactic_and_access(tactic: &str, access_level: Option<&str>) -> Ttp {
        let mut requires = serde_json::Map::new();
        if let Some(level) = access_level {
            requires.insert("accessLevel".to_string(), json!(level));
        }
        Ttp {
            status: "enabled".to_string(),
            requires,
            ..Ttp::new("test", "Test", tactic)
        }
    }

    #[test]
    fn exempt_tactics_always_satisfied_regardless_of_access_level() {
        for tactic in &[
            "Initial Access",
            "InitialAccess",
            "Lateral Movement",
            "LateralMovement",
            "Resource Development",
            "ResourceDevelopment",
        ] {
            let ttp = ttp_with_tactic_and_access(tactic, Some("root-exec"));
            assert!(
                ttp_access_level_satisfied(&ttp, AccessLevel::None),
                "tactic '{tactic}' should be exempt"
            );
        }
    }

    #[test]
    fn undeclared_access_level_is_always_satisfied() {
        let ttp = ttp_with_tactic_and_access("Discovery", None);
        assert!(ttp_access_level_satisfied(&ttp, AccessLevel::None));
        assert!(ttp_access_level_satisfied(&ttp, AccessLevel::Exec));
    }

    #[test]
    fn declared_exec_requires_exec() {
        for declared in &[
            "user-exec",
            "user-read",
            "user-write",
            "root-exec",
            "root-read",
        ] {
            let ttp = ttp_with_tactic_and_access("Discovery", Some(declared));
            assert!(
                !ttp_access_level_satisfied(&ttp, AccessLevel::None),
                "declared '{declared}' should require Exec"
            );
            assert!(
                ttp_access_level_satisfied(&ttp, AccessLevel::Exec),
                "declared '{declared}' should be satisfied by Exec"
            );
        }
    }

    #[test]
    fn declared_none_is_always_satisfied() {
        let ttp = ttp_with_tactic_and_access("Discovery", Some("none"));
        assert!(ttp_access_level_satisfied(&ttp, AccessLevel::None));
    }

    #[test]
    fn exists_satisfied_when_no_constraint() {
        assert!(ttp_exists_satisfied(&ttp_no_rbac(), &empty_campaign()));
    }

    #[test]
    fn exists_not_satisfied_when_listener_required_and_none_in_campaign() {
        // No listener has been bound, so `C2Server.listeners` is empty.
        assert!(!ttp_exists_satisfied(
            &ttp_with_exists("Listener"),
            &empty_campaign()
        ));
    }

    #[test]
    fn exists_satisfied_when_a_listener_is_bound() {
        let mut c = empty_campaign();
        c.entities.insert_typed(Listener::new(1337, "tcp"));
        assert!(ttp_exists_satisfied(&ttp_with_exists("Listener"), &c));
    }

    #[test]
    fn exists_matches_a_named_custom_resource() {
        let mut c = empty_campaign();
        c.entities.insert_typed(ran_domain::K8sCustomResource::new(
            "monitoring.coreos.com",
            "v1",
            "ServiceMonitor",
            "redis-metrics",
            "monitoring",
        ));
        let mut ttp = ttp_no_rbac();
        ttp.requires.insert(
            "exists".to_string(),
            json!([{
                "kind": "ServiceMonitor",
                "name": "redis-metrics",
                "namespace": "monitoring"
            }]),
        );

        assert!(ttp_exists_satisfied(&ttp, &c));
        ttp.requires.insert(
            "exists".to_string(),
            json!([{"kind": "ServiceMonitor", "name": "other"}]),
        );
        assert!(!ttp_exists_satisfied(&ttp, &c));
    }

    #[test]
    fn has_listener_unconstrained_when_absent() {
        assert!(ttp_has_listener_satisfied(
            &ttp_no_rbac(),
            &empty_campaign()
        ));
    }

    #[test]
    fn has_listener_required_but_none_bound() {
        let mut c = empty_campaign();
        c.entities.insert_typed(C2Server::new("ran"));
        assert!(
            !ttp_has_listener_satisfied(&ttp_with_has_listener(true), &c),
            "stopping a listener must not be offered when none is bound"
        );
    }

    #[test]
    fn has_listener_required_and_one_bound() {
        let mut c = empty_campaign();
        c.entities.insert_typed(Listener::new(4444, "tcp"));
        assert!(ttp_has_listener_satisfied(&ttp_with_has_listener(true), &c));
    }

    #[test]
    fn has_listener_false_excludes_a_c2_that_has_one() {
        let mut c = empty_campaign();
        c.entities.insert_typed(Listener::new(4444, "tcp"));
        assert!(
            !ttp_has_listener_satisfied(&ttp_with_has_listener(false), &c),
            "`c2.has-listener: false` must exclude a C2 that already has one"
        );
    }

    #[test]
    fn has_session_requires_a_live_channel_from_the_selected_c2() {
        let mut c = empty_campaign();
        let target_id = "c2/test";
        let other_c2 = C2Server::new("other");
        let other_c2_id = other_c2.entity_id().0;
        c.entities.insert_typed(other_c2);

        c.insert_relation(&SessionChannel::new(
            other_c2_id,
            "node/victim",
            "session/other",
        ));

        let ttp = ttp_with_has_session(true);
        assert!(
            !ttp_has_session_satisfied(&ttp, &c, target_id),
            "a session belonging to another C2 must not make this action applicable"
        );

        c.insert_relation(&SessionChannel::new(
            target_id,
            "node/target",
            "session/target",
        ));
        assert!(ttp_has_session_satisfied(&ttp, &c, target_id));
    }

    #[test]
    fn c2_action_requiring_a_session_is_inapplicable_without_one() {
        let mut c = empty_campaign();
        let target_id = "c2/test";
        let mut ttp = ttp_with_has_session(true);
        ttp.requires.insert("kind".to_string(), json!("C2"));
        let context = resolve_target_context(&c, target_id).expect("C2 target context");

        assert!(
            !ttp_applicable_for_target(&ttp, &c, &context),
            "the Armory must hide Kill Session until this C2 owns a live session"
        );

        c.insert_relation(&SessionChannel::new(
            target_id,
            "node/victim",
            "session/target",
        ));
        assert!(ttp_applicable_for_target(&ttp, &c, &context));
    }

    #[test]
    fn exists_not_satisfied_for_unknown_entity_kind() {
        // Unknown kinds fail safe so phantom pre-conditions don't silently pass.
        assert!(!ttp_exists_satisfied(
            &ttp_with_exists("UnknownThing"),
            &empty_campaign()
        ));
    }

    #[test]
    fn rbac_satisfied_when_no_requirement() {
        let c = empty_campaign();
        assert!(ttp_rbac_satisfied(&ttp_no_rbac(), &c));
    }

    #[test]
    fn rbac_not_satisfied_when_no_identity_has_permissions() {
        // TTP has RBAC requirements but no identity has been reviewed yet.
        assert!(!ttp_rbac_satisfied(
            &ttp_with_rbac("delete", "events"),
            &empty_campaign()
        ));
    }

    #[test]
    fn rbac_satisfied_when_matching_sa_exists() {
        let c = campaign_with_sa("delete", "events");
        assert!(ttp_rbac_satisfied(&ttp_with_rbac("delete", "events"), &c));
    }

    #[test]
    fn rbac_not_satisfied_when_sa_lacks_required_permission() {
        let c = campaign_with_sa("get", "pods");
        assert!(!ttp_rbac_satisfied(&ttp_with_rbac("delete", "events"), &c));
    }

    #[test]
    fn rbac_satisfied_by_wildcard_sa_entitlement() {
        let c = campaign_with_sa("*", "*");
        assert!(ttp_rbac_satisfied(&ttp_with_rbac("delete", "events"), &c));
    }

    #[test]
    fn rbac_satisfied_when_matching_kubeconfig_credential_exists() {
        let mut c = empty_campaign();
        let mut credential = K8sCredential::new("https://cluster.example");
        credential.active = true;
        credential
            .entitlements
            .push(RbacPermission::new("list", "pods"));
        c.entities.insert_typed(credential);

        assert!(ttp_rbac_satisfied(&ttp_with_rbac("list", "pods"), &c));
        assert!(!ttp_rbac_satisfied(&ttp_with_rbac("delete", "pods"), &c));
    }

    #[test]
    fn eligible_identities_return_the_witnesses_for_global_rbac_applicability() {
        let mut campaign = campaign_with_sa("list", "pods");
        let mut credential =
            K8sCredential::new("https://cluster.example").with_name("operator-kubeconfig");
        credential.active = true;
        credential
            .entitlements
            .push(RbacPermission::new("list", "pods"));
        let credential_id = credential.entity_id().0;
        campaign.entities.insert_typed(credential);

        let mut ttp = ttp_with_rbac("list", "pods");
        ttp.procedures = vec![armory::Procedure::new(
            "kubectl",
            "kubectl get pods --output=json",
        )];
        let identities =
            super::eligible_auth_identities(&ttp, &campaign, "k8s/cluster/test-cluster");

        assert_eq!(identities.len(), 2);
        assert!(identities
            .iter()
            .any(|identity| identity.id == credential_id));
        assert!(identities
            .iter()
            .any(|identity| identity.kind == "ServiceAccount"));
    }

    #[test]
    fn token_execution_requires_a_controlled_execution_source() {
        let mut campaign = campaign_with_sa("list", "pods");
        let service_account_id = campaign
            .entities
            .values::<ServiceAccount>()
            .next()
            .expect("fixture service account")
            .entity_id()
            .0;
        let mut ttp = kubernetes_ttp_with_rbac("list", "pods");
        ttp.requires
            .insert("kind".to_string(), json!("ServiceAccount"));
        let target = resolve_target_context(&campaign, &service_account_id)
            .expect("service account target context");

        assert!(
            !ttp_applicable_for_target(&ttp, &campaign, &target),
            "captured authentication alone cannot execute a client command"
        );

        let source = Pod::new("agent-worker", "agent-system");
        let source_id = source.entity_id().0;
        campaign.entities.insert_typed(source);
        campaign.insert_relation(&ran_domain::PodExec::new(c2::BUILTIN_C2_ID, source_id));

        assert!(ttp_applicable_for_target(&ttp, &campaign, &target));
    }

    use super::kind_matches_target_kind;

    #[test]
    fn system_kind_matches_any_system_entity_target() {
        // is_system_target=true represents anything implementing SystemEntity
        assert!(kind_matches_target_kind("System", "Pod", true));
        assert!(kind_matches_target_kind("System", "Node", true));
        // A hypothetical future type also matches as long as it is a SystemEntity
        assert!(kind_matches_target_kind("System", "UnknownSystem", true));
    }

    #[test]
    fn system_kind_does_not_match_non_system_entities() {
        assert!(!kind_matches_target_kind("System", "ServiceAccount", false));
        assert!(!kind_matches_target_kind("System", "Namespace", false));
    }

    #[test]
    fn exact_kind_matching_still_works() {
        assert!(kind_matches_target_kind("Pod", "Pod", false));
        assert!(!kind_matches_target_kind("Pod", "Node", false));
        // is_system_target flag is irrelevant for non-System requirements
        assert!(!kind_matches_target_kind("Pod", "Node", true));
    }

    use ran_domain::{JwToken, ServiceAccountToken};

    #[test]
    fn target_context_none_for_unknown_entity() {
        let c = empty_campaign();
        assert!(resolve_target_context(&c, "ns/default/pod/ghost").is_none());
    }

    #[test]
    fn target_context_plain_pod_is_system_without_token_or_access() {
        let mut c = empty_campaign();
        let pod = Pod::new("nginx", "default");
        let id = pod.entity_id().0;
        c.entities.insert_typed(pod);

        let tc = resolve_target_context(&c, &id).expect("pod should resolve");
        assert!(tc.is_system);
        assert!(!tc.has_token);
        assert_eq!(tc.access_level, AccessLevel::None);
    }

    #[test]
    fn target_context_reachable_pod_infers_exec_access() {
        let mut c = empty_campaign();
        // seed_pod_for_trigger wires a direct kubectl-exec channel from the C2,
        // making the pod reachable → access level should be inferred as Exec
        // even though no TTP has explicitly raised it.
        let id = c.seed_pod_for_trigger("nginx", "default").0;

        let tc = resolve_target_context(&c, &id).expect("pod should resolve");
        assert_eq!(tc.access_level, AccessLevel::Exec);
    }

    fn campaign_with_pod_session(status: Option<SessionStatus>) -> (crate::Campaign, String) {
        let mut campaign = empty_campaign();
        let mut pod = Pod::new("target", "default");
        if let Some(status) = status {
            pod.system.sessions.push(SessionInfo {
                id: "shell-1".to_string(),
                kind: "tcp".to_string(),
                port: None,
                status,
            });
        }
        let id = pod.entity_id().0;
        campaign.entities.insert_typed(pod);
        (campaign, id)
    }

    fn ttp_requiring_active_pod_session() -> Ttp {
        let mut ttp = Ttp::new("copyfail", "CopyFail", "Privilege Escalation");
        ttp.requires.insert("kind".to_string(), json!("Pod"));
        ttp.requires
            .insert("activeSession".to_string(), json!(true));
        ttp
    }

    #[test]
    fn active_session_requirement_only_accepts_active_status() {
        let ttp = ttp_requiring_active_pod_session();

        for status in [
            None,
            Some(SessionStatus::Connecting),
            Some(SessionStatus::Lost),
        ] {
            let (campaign, id) = campaign_with_pod_session(status);
            let context = resolve_target_context(&campaign, &id).unwrap();
            assert!(!context.active_session);
            assert!(!ttp_applicable_for_target(&ttp, &campaign, &context));
        }

        let (campaign, id) = campaign_with_pod_session(Some(SessionStatus::Active));
        let context = resolve_target_context(&campaign, &id).unwrap();
        assert!(context.active_session);
        assert!(ttp_applicable_for_target(&ttp, &campaign, &context));
    }

    fn ranplant_upgrade_ttp() -> Ttp {
        Ttp {
            procedures: vec![Procedure {
                operation: ProcedureOperation::StartRanplantSession {
                    listener_port: "1337".to_string(),
                },
                ..Procedure::new("native", "ranplant connect")
            }],
            ..Ttp::new(
                "start-ranplant-session",
                "Start Ranplant Session",
                "Execution",
            )
        }
    }

    #[test]
    fn ranplant_upgrade_allows_plain_shell_but_not_active_ranplant() {
        let ttp = ranplant_upgrade_ttp();
        let (plain_shell, target_id) = campaign_with_pod_session(Some(SessionStatus::Active));
        assert!(ttp_session_upgrade_satisfied(
            &ttp,
            &plain_shell,
            &target_id
        ));

        let mut ranplant = empty_campaign();
        let mut pod = Pod::new("target", "default");
        pod.system.sessions.push(SessionInfo {
            id: "c2-ran-1337-ranplant".to_string(),
            kind: "ranplant".to_string(),
            port: Some(1337),
            status: SessionStatus::Active,
        });
        let target_id = pod.entity_id().0;
        ranplant.entities.insert_typed(pod);
        assert!(!ttp_session_upgrade_satisfied(&ttp, &ranplant, &target_id));
    }

    #[test]
    fn active_node_session_does_not_satisfy_pod_kind_requirement() {
        let mut campaign = empty_campaign();
        let mut node = K8sNode::new("worker-1");
        node.system.sessions.push(SessionInfo {
            id: "shell-1".to_string(),
            kind: "mtls".to_string(),
            port: None,
            status: SessionStatus::Active,
        });
        let id = node.entity_id().0;
        campaign.entities.insert_typed(node);

        let context = resolve_target_context(&campaign, &id).unwrap();
        assert!(context.active_session);
        assert!(!ttp_applicable_for_target(
            &ttp_requiring_active_pod_session(),
            &campaign,
            &context
        ));
    }

    fn ttp_requiring_node_filesystem_access() -> Ttp {
        let mut ttp = Ttp::new("node-files", "Node files", "Discovery");
        ttp.requires.insert("kind".to_string(), json!("Node"));
        ttp.requires
            .insert("filesystemAccess".to_string(), json!(true));
        ttp
    }

    fn ttp_requiring_host_pid() -> Ttp {
        let mut ttp = Ttp::new(
            "escape-container-via-nsenter",
            "Escape container via NSenter",
            "Privilege Escalation",
        );
        ttp.requires
            .insert("Pod.securityContext.privileged".to_string(), json!(true));
        ttp.requires
            .insert("Pod.securityContext.hostPID".to_string(), json!(true));
        ttp
    }

    #[test]
    fn node_filesystem_access_requires_a_direct_or_hostpath_execution_capability() {
        let mut campaign = empty_campaign();
        let node = K8sNode::new("worker-1");
        let node_id = node.entity_id().0;
        campaign.entities.insert_typed(node);
        let ttp = ttp_requiring_node_filesystem_access();

        let context = resolve_target_context(&campaign, &node_id).unwrap();
        assert!(!ttp_applicable_for_target(&ttp, &campaign, &context));

        let mut inaccessible_pod = Pod::new("host-reader", "default");
        inaccessible_pod.node_name = Some("worker-1".to_string());
        inaccessible_pod.volume_mounts.push(ran_domain::Mount {
            name: "host".to_string(),
            mount_point: "/host".to_string(),
            mount_root: "/".to_string(),
            is_host_path: true,
            ..ran_domain::Mount::default()
        });
        let pod_id = inaccessible_pod.entity_id().0;
        campaign.entities.insert_typed(inaccessible_pod);
        assert!(!ttp_applicable_for_target(&ttp, &campaign, &context));

        campaign
            .entities
            .get_mut::<Pod>()
            .get_mut(&ran_domain::EntityId::new(&pod_id))
            .expect("hostPath pod")
            .system
            .access_level = AccessLevel::Exec;
        assert!(ttp_applicable_for_target(&ttp, &campaign, &context));
    }

    #[test]
    fn node_filesystem_access_accepts_direct_node_execution() {
        let mut campaign = empty_campaign();
        let mut node = K8sNode::new("worker-1");
        node.system.access_level = AccessLevel::Exec;
        let node_id = node.entity_id().0;
        campaign.entities.insert_typed(node);

        let context = resolve_target_context(&campaign, &node_id).unwrap();
        assert!(ttp_applicable_for_target(
            &ttp_requiring_node_filesystem_access(),
            &campaign,
            &context
        ));
    }

    #[test]
    fn host_pid_requirement_allows_unknown_but_rejects_known_false() {
        let (mut campaign, pod_id) = campaign_with_pod_session(None);
        let ttp = ttp_requiring_host_pid();
        assert!(ttp_pod_requirements_satisfied(&ttp, &campaign, &pod_id));

        campaign
            .entities
            .find_mut::<Pod>(&ran_domain::EntityId::new(&pod_id))
            .expect("pod")
            .host_pid = Confidence::No;
        assert!(!ttp_pod_requirements_satisfied(&ttp, &campaign, &pod_id));

        let pod = campaign
            .entities
            .find_mut::<Pod>(&ran_domain::EntityId::new(&pod_id))
            .expect("pod");
        pod.host_pid = Confidence::Yes;
        pod.privileged = Confidence::No;
        assert!(!ttp_pod_requirements_satisfied(&ttp, &campaign, &pod_id));

        campaign
            .entities
            .find_mut::<Pod>(&ran_domain::EntityId::new(&pod_id))
            .expect("pod")
            .privileged = Confidence::Yes;
        assert!(ttp_pod_requirements_satisfied(&ttp, &campaign, &pod_id));
    }

    #[test]
    fn exact_host_proc_requirement_rejects_other_host_paths() {
        let (mut campaign, pod_id) = campaign_with_pod_session(None);
        let mut ttp = Ttp::new("host-proc", "Host proc", "Privilege Escalation");
        ttp.requires
            .insert("Pod.hostPath".to_string(), json!("/proc"));
        let pod = campaign
            .entities
            .find_mut::<Pod>(&ran_domain::EntityId::new(&pod_id))
            .expect("pod");
        pod.volume_mounts.push(Mount {
            mount_root: "/".to_string(),
            mount_point: "/host".to_string(),
            is_host_path: true,
            ..Default::default()
        });
        assert!(!ttp_pod_requirements_satisfied(&ttp, &campaign, &pod_id));

        campaign
            .entities
            .find_mut::<Pod>(&ran_domain::EntityId::new(&pod_id))
            .expect("pod")
            .volume_mounts
            .push(Mount {
                mount_root: "/proc/".to_string(),
                mount_point: "/host/proc".to_string(),
                is_host_path: true,
                ..Default::default()
            });
        assert!(ttp_pod_requirements_satisfied(&ttp, &campaign, &pod_id));
    }

    fn namespace_execution_record(
        target_id: &str,
        success: bool,
        results: Vec<&str>,
    ) -> crate::ExecutionRecord {
        crate::ExecutionRecord {
            id: format!("execution-{}", success),
            ttp_id: "escape-container-via-nsenter".to_string(),
            ttp_name: "Escape container via NSenter".to_string(),
            tactic: "Privilege Escalation".to_string(),
            target_id: target_id.to_string(),
            exec_system_id: target_id.to_string(),
            execution_environment: None,
            auth_identity_id: None,
            procedure_id: "nsenter".to_string(),
            command: "nsenter --target 1 --mount --uts --ipc --net --pid hostname".to_string(),
            args: HashMap::from([
                ("MOUNT".to_string(), "true".to_string()),
                ("UTS".to_string(), "true".to_string()),
                ("IPC".to_string(), "true".to_string()),
                ("NET".to_string(), "true".to_string()),
                ("PID".to_string(), "true".to_string()),
            ]),
            success,
            partial: false,
            exit_code: if success { 0 } else { 1 },
            results: results.into_iter().map(str::to_string).collect(),
            fail_reason: String::new(),
            started_at_ms: 1,
            completed_at_ms: 2,
            is_cleanup: false,
            reasoning: String::new(),
            discovered_entities: Vec::new(),
            discovered_relations: Vec::new(),
        }
    }

    fn ttp_requiring_namespace_access() -> Ttp {
        let mut ttp = Ttp::new(
            "escape-container-via-nsenter",
            "Escape container via NSenter",
            "Privilege Escalation",
        );
        ttp.requires.insert(
            "linuxNamespaceAccess".to_string(),
            json!(["mount", "uts", "pid"]),
        );
        ttp
    }

    #[test]
    fn namespace_access_unknown_keeps_action_applicable() {
        let (campaign, pod_id) = campaign_with_pod_session(None);

        assert!(ttp_namespace_access_satisfied(
            &ttp_requiring_namespace_access(),
            &campaign,
            &pod_id
        ));
    }

    #[test]
    fn known_required_nsenter_namespace_denial_blocks_only_affected_system() {
        let (mut campaign, pod_id) = campaign_with_pod_session(None);
        let other_pod = Pod::new("other", "default");
        let other_pod_id = other_pod.entity_id().0;
        campaign.entities.insert_typed(other_pod);
        campaign.append_execution_record(namespace_execution_record(
            &pod_id,
            false,
            vec!["nsenter: reassociate to namespace 'ns/pid' failed: Operation not permitted"],
        ));
        let ttp = ttp_requiring_namespace_access();

        assert!(!ttp_namespace_access_satisfied(&ttp, &campaign, &pod_id));
        assert!(ttp_namespace_access_satisfied(
            &ttp,
            &campaign,
            &other_pod_id
        ));
    }

    #[test]
    fn optional_ipc_namespace_denial_keeps_minimal_escape_applicable() {
        let (mut campaign, pod_id) = campaign_with_pod_session(None);
        campaign.append_execution_record(namespace_execution_record(
            &pod_id,
            false,
            vec!["nsenter: reassociate to namespace 'ns/ipc' failed: Operation not permitted"],
        ));

        assert!(ttp_namespace_access_satisfied(
            &ttp_requiring_namespace_access(),
            &campaign,
            &pod_id
        ));
    }

    #[test]
    fn direct_nsenter_denial_does_not_hide_host_proc_alternative() {
        let (mut campaign, pod_id) = campaign_with_pod_session(None);
        campaign.append_execution_record(namespace_execution_record(
            &pod_id,
            false,
            vec!["nsenter: reassociate to namespace 'ns/pid' failed: Operation not permitted"],
        ));
        let mut alternative = ttp_requiring_namespace_access();
        alternative.id = "escape-container-via-nsenter-and-mounted-host-proc".to_string();

        assert!(ttp_namespace_access_satisfied(
            &alternative,
            &campaign,
            &pod_id
        ));
    }

    #[test]
    fn mount_requirement_recognizes_kernel_mnt_namespace_name() {
        let (mut campaign, pod_id) = campaign_with_pod_session(None);
        campaign.append_execution_record(namespace_execution_record(
            &pod_id,
            false,
            vec!["nsenter: reassociate to namespace 'ns/mnt' failed: Operation not permitted"],
        ));

        assert!(!ttp_namespace_access_satisfied(
            &ttp_requiring_namespace_access(),
            &campaign,
            &pod_id
        ));
    }

    #[test]
    fn namespace_denial_is_attributed_to_physical_execution_system() {
        let (mut campaign, pod_id) = campaign_with_pod_session(None);
        let mut record = namespace_execution_record(
            &pod_id,
            false,
            vec!["nsenter: reassociate to namespace 'ns/pid' failed: Operation not permitted"],
        );
        record.target_id = "node/worker-1".to_string();
        campaign.append_execution_record(record);
        let ttp = ttp_requiring_namespace_access();

        assert!(!ttp_namespace_access_satisfied(&ttp, &campaign, &pod_id));
        assert!(ttp_namespace_access_satisfied(
            &ttp,
            &campaign,
            "node/worker-1"
        ));
    }

    #[test]
    fn repository_nsenter_action_declares_namespace_access_requirement() {
        let armory_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../armory/TTPs");
        let armory =
            armory::Armory::load_from_dir(armory_path).expect("repository armory should load");
        let ttp = armory
            .get_ttp("escape-container-via-nsenter")
            .expect("nsenter escape action");

        assert_eq!(
            ttp.requires.get("linuxNamespaceAccess"),
            Some(&json!(["mount", "uts", "pid"]))
        );
        assert_eq!(
            ttp.requires.get("Pod.securityContext.hostPID"),
            Some(&json!(true))
        );
        assert_eq!(
            ttp.params
                .iter()
                .find(|param| param.name == "IPC")
                .map(|param| param.default.as_str()),
            Some("false")
        );

        let host_proc_ttp = armory
            .get_ttp("escape-container-via-nsenter-and-mounted-host-proc")
            .expect("mounted host proc escape action");
        assert_eq!(host_proc_ttp.requires.get("kind"), Some(&json!("Pod")));
        assert_eq!(
            host_proc_ttp.requires.get("Pod.hostPath"),
            Some(&json!("/proc"))
        );
        assert_eq!(
            host_proc_ttp
                .params
                .iter()
                .find(|param| param.name == "HOST_PROC")
                .map(|param| param.default.as_str()),
            Some("${SRC.HOST_PATH:/proc}")
        );
        assert_eq!(
            ttp.params
                .iter()
                .find(|param| param.name == "NET")
                .map(|param| param.default.as_str()),
            Some("false")
        );
    }

    #[test]
    fn mounted_host_proc_action_grounds_exact_mount_and_minimal_namespaces() {
        let armory_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../armory/TTPs");
        let armory =
            armory::Armory::load_from_dir(armory_path).expect("repository armory should load");
        let ttp = armory
            .get_ttp("escape-container-via-nsenter-and-mounted-host-proc")
            .expect("mounted host proc escape action");
        let mut campaign = empty_campaign();
        let mut pod = Pod::new("target", "default");
        pod.volume_mounts.push(Mount {
            mount_root: "/proc".to_string(),
            mount_point: "/host/proc".to_string(),
            is_host_path: true,
            ..Default::default()
        });
        let pod_id = pod.entity_id().0;
        campaign.entities.insert_typed(pod);
        let mut args: HashMap<_, _> = ttp
            .params
            .iter()
            .map(|param| (param.name.clone(), param.default.clone()))
            .collect();
        args.insert("SRC".to_string(), pod_id);

        crate::grounding::ground_entity_ref_vars(&mut args, &campaign);
        let command = crate::effects::ground_template(
            &crate::grounding::resolve_template(&ttp.procedures[0].command, &args),
            &args,
        );

        assert!(
            command.contains("--mount=/host/proc/1/ns/mnt"),
            "rendered command: {command}"
        );
        assert!(
            command.contains("--uts=/host/proc/1/ns/uts"),
            "rendered command: {command}"
        );
        assert!(
            command.contains("--pid=/host/proc/1/ns/pid"),
            "rendered command: {command}"
        );
        assert!(!command.contains("--ipc"));
        assert!(!command.contains("--net"));
        assert!(!command.contains("${"));
    }

    #[test]
    fn newer_successful_nsenter_execution_restores_namespace_access() {
        let (mut campaign, pod_id) = campaign_with_pod_session(None);
        campaign.append_execution_record(namespace_execution_record(
            &pod_id,
            false,
            vec!["nsenter: reassociate to namespace 'ns/pid' failed: Operation not permitted"],
        ));
        campaign.append_execution_record(namespace_execution_record(&pod_id, true, vec![]));

        assert!(ttp_namespace_access_satisfied(
            &ttp_requiring_namespace_access(),
            &campaign,
            &pod_id
        ));
    }

    #[test]
    fn target_context_service_account_with_token_has_token() {
        let mut c = empty_campaign();
        let mut sa = ServiceAccount::new("attacker", "default");
        sa.token = Some(ServiceAccountToken {
            jwt: JwToken {
                raw: "eyJhbGciOiJSUzI1NiJ9.test".to_string(),
                ..Default::default()
            },
            namespace: "default".to_string(),
            service_account_name: "attacker".to_string(),
            ..Default::default()
        });
        let id = sa.entity_id().0;
        c.entities.insert_typed(sa);

        let tc = resolve_target_context(&c, &id).expect("sa should resolve");
        assert!(!tc.is_system);
        assert!(tc.has_token);
    }

    #[test]
    fn authentication_capability_rejects_knowledge_only_credentials() {
        let mut c = empty_campaign();
        let mut credential = K8sCredential::new("https://cluster.example");
        let id = credential.entity_id().0;
        c.entities.insert_typed(credential.clone());

        let mut ttp = Ttp::new("check", "Check", "Discovery");
        ttp.status = "enabled".to_string();
        ttp.requires
            .insert("kind".to_string(), json!("K8sCredential"));
        ttp.procedures = vec![armory::Procedure {
            operation: armory::ProcedureOperation::SelfSubjectRulesReview {
                namespace: "default".to_string(),
            },
            ..armory::Procedure::new("inspect", "k8sSelfSubjectRulesReview(default)")
        }];

        let tc = resolve_target_context(&c, &id).expect("credential should resolve");
        assert!(!ttp_applicable_for_target(&ttp, &c, &tc));

        credential.token = Some("captured-token".to_string());
        credential.has_token = true;
        c.entities
            .get_mut::<K8sCredential>()
            .insert(ran_domain::EntityId::new(&id), credential);
        let tc = resolve_target_context(&c, &id).expect("credential should resolve");
        assert!(ttp_applicable_for_target(&ttp, &c, &tc));
    }

    #[test]
    fn captured_kubeconfig_offers_the_repository_permission_review_action() {
        let mut campaign = empty_campaign();
        let mut source = Pod::new("reader", "default");
        source.system.sessions.push(SessionInfo {
            id: "reader-shell".to_string(),
            kind: "exec".to_string(),
            port: None,
            status: SessionStatus::Active,
        });
        let source_id = source.entity_id();
        campaign.entities.insert_typed(source);

        let mut credential =
            K8sCredential::new("https://cluster.example").with_name("super-admin.conf (default)");
        credential.source_path = Some("/host/etc/kubernetes/super-admin.conf".to_string());
        credential.token = Some("captured-token".to_string());
        credential.has_token = true;
        let credential_id = credential.entity_id().0;
        campaign.entities.insert_typed(credential);
        campaign.insert_relation(&Uses::new(source_id.0, credential_id.clone()));

        let armory_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../armory/TTPs");
        let armory =
            armory::Armory::load_from_dir(armory_path).expect("repository armory should load");
        let ttp = armory
            .get_ttp("check-kubeconfig-permissions")
            .expect("permission review action");
        let target =
            resolve_target_context(&campaign, &credential_id).expect("credential target context");

        assert!(ttp_applicable_for_target(ttp, &campaign, &target));
        assert_eq!(
            eligible_auth_identities(ttp, &campaign, &credential_id)
                .into_iter()
                .map(|identity| identity.id)
                .collect::<Vec<_>>(),
            vec![credential_id]
        );
    }

    #[test]
    fn a_selected_listener_offers_listener_actions_only() {
        // The point of modelling a listener as an entity: selecting one narrows
        // the armory to what can be done *to* it. Creating a listener belongs to
        // the C2 and must not show up here.
        let mut c = empty_campaign();
        c.entities.insert_typed(C2Server::new("ran"));
        let listener = Listener::new(4444, "tcp");
        let listener_id = listener.entity_id().0;
        c.entities.insert_typed(listener);

        let tc = resolve_target_context(&c, &listener_id).expect("a listener is a target");
        assert_eq!(tc.target_kind, "Listener");

        let mut stop = ttp_no_rbac();
        stop.requires.insert("kind".to_string(), json!("Listener"));
        stop.requires
            .insert("c2.has-listener".to_string(), json!(true));
        assert!(
            ttp_applicable_for_target(&stop, &c, &tc),
            "Stop Listener applies to the listener that was selected"
        );

        let mut create = ttp_no_rbac();
        create.requires.insert("kind".to_string(), json!("C2"));
        assert!(
            !ttp_applicable_for_target(&create, &c, &tc),
            "Create Listener targets the C2, not a running listener"
        );
    }

    #[test]
    fn a_selected_redirector_offers_redirector_actions_only() {
        // The reason a redirector is an entity and not a `cleanup:` block:
        // selecting one narrows the armory to what can be done *to* it.
        let mut c = empty_campaign();
        c.entities.insert_typed(C2Server::new("ran"));
        c.entities.insert_typed(Listener::new(4444, "tcp"));
        let redirector = Redirector::new("labctl", "zn1kqxk3ykpvxp5x", 1337, 4444);
        let redirector_id = redirector.entity_id().0;
        c.entities.insert_typed(redirector);

        let tc = resolve_target_context(&c, &redirector_id).expect("a redirector is a target");
        assert_eq!(tc.target_kind, "Redirector");

        let mut stop = ttp_no_rbac();
        stop.requires
            .insert("kind".to_string(), json!("Redirector"));
        assert!(
            ttp_applicable_for_target(&stop, &c, &tc),
            "Stop Redirector applies to the redirector that was selected"
        );

        // Create Redirector targets the listener it will forward into, so it must
        // not also offer itself on a redirector that already exists.
        let mut create = ttp_no_rbac();
        create
            .requires
            .insert("kind".to_string(), json!("Listener"));
        create
            .requires
            .insert("c2.has-listener".to_string(), json!(true));
        assert!(
            !ttp_applicable_for_target(&create, &c, &tc),
            "Create Redirector targets a listener, not a running redirector"
        );
    }

    #[test]
    fn a_stopped_redirector_is_no_longer_a_target() {
        let mut c = empty_campaign();
        let redirector = Redirector::new("labctl", "play1", 1337, 4444);
        let redirector_id = redirector.entity_id();
        c.entities.insert_typed(redirector);

        assert!(c.remove_redirector("play1", 1337));
        assert!(
            resolve_target_context(&c, &redirector_id.0).is_none(),
            "a stopped redirector is no longer a target at all"
        );
    }

    /// The gate that `ttp_tool_satisfied` cannot provide: an operator-side
    /// procedure never consults a target binary map, so a TTP that shells out
    /// locally would otherwise be offered whatever is installed.
    #[test]
    fn an_operator_side_tool_requirement_gates_on_the_operator_host() {
        let mut needs_missing = ttp_no_rbac();
        needs_missing.requires.insert(
            "c2.has-tool".to_string(),
            json!("ran-tool-that-does-not-exist"),
        );
        assert!(!ttp_operator_tool_satisfied(&needs_missing));

        // `sh` is on PATH anywhere this test can run.
        let mut needs_present = ttp_no_rbac();
        needs_present
            .requires
            .insert("c2.has-tool".to_string(), json!("sh"));
        assert!(ttp_operator_tool_satisfied(&needs_present));

        // No requirement is no restriction.
        assert!(ttp_operator_tool_satisfied(&ttp_no_rbac()));
    }

    #[test]
    fn every_named_operator_tool_must_be_present() {
        let mut ttp = ttp_no_rbac();
        ttp.requires.insert(
            "c2.has-tool".to_string(),
            json!(["sh", "ran-tool-that-does-not-exist"]),
        );
        assert!(!ttp_operator_tool_satisfied(&ttp));

        ttp.requires
            .insert("c2.has-tool".to_string(), json!(["sh"]));
        assert!(ttp_operator_tool_satisfied(&ttp));
    }

    #[test]
    fn an_operator_tool_given_as_a_path_is_checked_as_one() {
        let mut ttp = ttp_no_rbac();
        // A path is used verbatim by `Command`, so it is checked directly rather
        // than searched for on PATH.
        ttp.requires
            .insert("c2.has-tool".to_string(), json!("/bin/sh"));
        assert!(ttp_operator_tool_satisfied(&ttp));

        ttp.requires
            .insert("c2.has-tool".to_string(), json!("/bin/nope-not-here"));
        assert!(!ttp_operator_tool_satisfied(&ttp));

        // A directory is not something that can be executed.
        ttp.requires
            .insert("c2.has-tool".to_string(), json!("/bin"));
        assert!(!ttp_operator_tool_satisfied(&ttp));
    }

    #[test]
    fn two_playgrounds_can_forward_the_same_remote_port() {
        // RPORT defaults to 1337, so this is the ordinary case once a second
        // playground is in play - stopping one must not take the other with it.
        let mut c = empty_campaign();
        c.entities
            .insert_typed(Redirector::new("labctl", "play1", 1337, 4444));
        c.entities
            .insert_typed(Redirector::new("labctl", "play2", 1337, 4444));

        assert!(c.remove_redirector("play1", 1337));

        assert!(
            resolve_target_context(&c, "redirector/play1/1337").is_none(),
            "the stopped redirector is gone"
        );
        assert!(
            resolve_target_context(&c, "redirector/play2/1337").is_some(),
            "the other playground's redirector on the same port is untouched"
        );
    }

    #[test]
    fn stopping_is_not_offered_once_the_last_listener_is_gone() {
        let mut c = empty_campaign();
        let listener = Listener::new(4444, "tcp");
        let listener_id = listener.entity_id();
        c.entities.insert_typed(listener);

        let tc = resolve_target_context(&c, &listener_id.0).expect("a listener is a target");
        assert_eq!(c.remove_listeners_on_port(4444), 1);

        assert!(
            resolve_target_context(&c, &listener_id.0).is_none(),
            "a stopped listener is no longer a target at all"
        );
        let mut stop = ttp_no_rbac();
        stop.requires.insert("kind".to_string(), json!("Listener"));
        stop.requires
            .insert("c2.has-listener".to_string(), json!(true));
        assert!(
            !ttp_applicable_for_target(&stop, &c, &tc),
            "with no listener bound, stopping one is not applicable"
        );
    }

    #[test]
    fn applicable_for_target_combines_kind_and_rbac() {
        // Campaign has a SA entitled to `get serviceaccounts`; target is that SA.
        let c = campaign_with_sa("get", "serviceaccounts");
        let sa_id = c
            .entities
            .values::<ServiceAccount>()
            .next()
            .unwrap()
            .entity_id()
            .0;
        let tc = resolve_target_context(&c, &sa_id).expect("sa should resolve");

        // A discovery TTP requiring that exact RBAC permission applies.
        let ttp = ttp_with_rbac("get", "serviceaccounts");
        assert!(ttp_applicable_for_target(&ttp, &c, &tc));

        // A TTP restricted to Node targets does not apply to a ServiceAccount.
        let mut node_ttp = ttp_no_rbac();
        node_ttp.requires.insert("kind".to_string(), json!("Node"));
        assert!(!ttp_applicable_for_target(&node_ttp, &c, &tc));
    }

    #[test]
    fn unrelated_credential_does_not_add_cluster_discovery_to_pod_target() {
        let mut campaign = empty_campaign();
        let pod = ran_domain::Pod::new("target", "default");
        let pod_id = pod.entity_id().0;
        campaign.entities.insert_typed(pod);

        let mut credential = K8sCredential::new("https://cluster.example");
        credential.active = true;
        credential
            .entitlements
            .push(RbacPermission::new("list", "pods"));
        campaign.entities.insert_typed(credential);

        let tc = resolve_target_context(&campaign, &pod_id).expect("pod should resolve");
        assert!(!ttp_applicable_for_target(
            &kubernetes_ttp_with_rbac("list", "pods"),
            &campaign,
            &tc
        ));
    }

    #[test]
    fn non_kubernetes_lateral_action_requires_an_execution_foothold() {
        let mut campaign = empty_campaign();
        let pod = ran_domain::Pod::new("redis", "default");
        let pod_id = pod.entity_id().0;
        campaign.entities.insert_typed(pod);

        let mut requires = serde_json::Map::new();
        requires.insert("kind".to_string(), json!("System"));
        let ttp = Ttp {
            requires,
            procedures: vec![armory::Procedure::new(
                "exec-cmd",
                "redis-cli -h ${TARGET} ping",
            )],
            ..Ttp::new("exploit-redis", "Exploit Redis", "Lateral Movement")
        };

        let tc = resolve_target_context(&campaign, &pod_id).expect("pod should resolve");
        assert!(!ttp_applicable_for_target(&ttp, &campaign, &tc));
    }

    #[test]
    fn cluster_target_keeps_actions_authorized_by_available_identity() {
        let mut campaign = empty_campaign();
        let mut credential = K8sCredential::new("https://cluster.example");
        credential.active = true;
        credential
            .entitlements
            .push(RbacPermission::new("list", "pods"));
        campaign.entities.insert_typed(credential);

        let cluster_id = "k8s/cluster/test";
        let tc = resolve_target_context(&campaign, cluster_id).expect("cluster should resolve");
        assert_eq!(tc.target_kind, "Cluster");
        assert!(ttp_applicable_for_target(
            &kubernetes_ttp_with_rbac("list", "pods"),
            &campaign,
            &tc
        ));
    }

    #[test]
    fn unread_service_account_cannot_borrow_another_identity_for_global_actions() {
        let mut campaign = empty_campaign();
        let sa = ServiceAccount::new("workload", "default");
        let sa_id = sa.entity_id().0;
        campaign.entities.insert_typed(sa);

        let mut credential = K8sCredential::new("https://cluster.example");
        credential.active = true;
        credential
            .entitlements
            .push(RbacPermission::new("list", "pods"));
        campaign.entities.insert_typed(credential);

        let tc = resolve_target_context(&campaign, &sa_id).expect("SA should resolve");
        assert!(!ttp_applicable_for_target(
            &kubernetes_ttp_with_rbac("list", "pods"),
            &campaign,
            &tc
        ));
    }

    #[test]
    fn captured_service_account_and_active_kubeconfig_show_their_own_actions() {
        let mut campaign = campaign_with_sa("list", "pods");
        let sa_id = campaign
            .entities
            .values::<ServiceAccount>()
            .next()
            .unwrap()
            .entity_id()
            .0;

        let mut credential = K8sCredential::new("https://cluster.example");
        credential.active = true;
        credential
            .entitlements
            .push(RbacPermission::new("list", "pods"));
        let credential_id = credential.entity_id().0;
        campaign.entities.insert_typed(credential);

        let ttp = kubernetes_ttp_with_rbac("list", "pods");
        for id in [sa_id, credential_id] {
            let tc = resolve_target_context(&campaign, &id).expect("identity should resolve");
            assert!(ttp_applicable_for_target(&ttp, &campaign, &tc));
        }
    }

    use super::ttp_tool_satisfied;
    use ran_domain::BinaryPresence;

    fn ttp_with_tool(tool: &str) -> Ttp {
        Ttp {
            status: "enabled".to_string(),
            procedures: vec![armory::Procedure {
                tool: Some(tool.to_string()),
                ..armory::Procedure::new("p", format!("{tool} --version"))
            }],
            ..Ttp::new("t", "t", "Discovery")
        }
    }

    fn campaign_with_pod_binary(
        tool: &str,
        presence: Option<BinaryPresence>,
    ) -> (crate::Campaign, String) {
        let mut c = empty_campaign();
        let mut pod = Pod::new("nginx", "default");
        if let Some(p) = presence {
            pod.system.binaries.insert(tool.to_string(), p);
        }
        let id = pod.entity_id().0;
        c.entities.insert_typed(pod);
        (c, id)
    }

    #[test]
    fn tool_satisfied_blocks_only_when_tool_known_absent() {
        let tool = "nmap";

        // Known absent → the action can't run → blocked.
        let (c, id) = campaign_with_pod_binary(tool, Some(BinaryPresence::Absent));
        let tc = resolve_target_context(&c, &id).unwrap();
        assert!(!ttp_tool_satisfied(&ttp_with_tool(tool), &c, &tc));

        // Unknown presence → not ruled out → allowed.
        let (c, id) = campaign_with_pod_binary(tool, None);
        let tc = resolve_target_context(&c, &id).unwrap();
        assert!(ttp_tool_satisfied(&ttp_with_tool(tool), &c, &tc));

        // Confirmed present → allowed.
        let (c, id) =
            campaign_with_pod_binary(tool, Some(BinaryPresence::Present("/usr/bin/nmap".into())));
        let tc = resolve_target_context(&c, &id).unwrap();
        assert!(ttp_tool_satisfied(&ttp_with_tool(tool), &c, &tc));
    }

    #[test]
    fn source_side_tool_is_not_gated_by_target_binary_facts() {
        let tool = "redis-cli";
        let (campaign, target_id) = campaign_with_pod_binary(tool, Some(BinaryPresence::Absent));
        let tc = resolve_target_context(&campaign, &target_id).unwrap();
        let mut ttp = ttp_with_tool(tool);
        ttp.procedures[0].run_on_target = Some(false);

        assert!(ttp_tool_satisfied(&ttp, &campaign, &tc));
    }

    #[test]
    fn fallback_procedure_keeps_ttp_runnable_when_primary_tool_is_absent() {
        // Mirrors get-local-ip-address: `ip` primary, `hostname` fallback.
        let ttp = Ttp {
            status: "enabled".to_string(),
            procedures: vec![
                armory::Procedure {
                    tool: Some("ip".to_string()),
                    ..armory::Procedure::new("ip", "ip -o -4 addr show scope global")
                },
                armory::Procedure {
                    tool: Some("hostname".to_string()),
                    ..armory::Procedure::new("hostname", "hostname -i")
                },
            ],
            ..Ttp::new("get-local-ip-address", "Get local IP address", "Discovery")
        };

        // iproute2 missing but hostname present: readiness is the max over
        // procedures, so the fallback keeps the action on the table.
        let mut c = empty_campaign();
        let mut pod = Pod::new("nginx", "default");
        pod.system
            .binaries
            .insert("ip".to_string(), BinaryPresence::Absent);
        pod.system.binaries.insert(
            "hostname".to_string(),
            BinaryPresence::Present("/bin/hostname".into()),
        );
        let id = pod.entity_id().0;
        c.entities.insert_typed(pod);

        let tc = resolve_target_context(&c, &id).unwrap();
        assert!(ttp_tool_satisfied(&ttp, &c, &tc));

        // Both absent: nothing left to fall back to.
        let mut c = empty_campaign();
        let mut pod = Pod::new("nginx", "default");
        for tool in ["ip", "hostname"] {
            pod.system
                .binaries
                .insert(tool.to_string(), BinaryPresence::Absent);
        }
        let id = pod.entity_id().0;
        c.entities.insert_typed(pod);

        let tc = resolve_target_context(&c, &id).unwrap();
        assert!(!ttp_tool_satisfied(&ttp, &c, &tc));
    }

    #[test]
    fn tool_satisfied_passes_for_non_system_target() {
        // A ServiceAccount has no binary map to assess → never blocked on tools.
        let c = campaign_with_sa("get", "serviceaccounts");
        let sa_id = c
            .entities
            .values::<ServiceAccount>()
            .next()
            .unwrap()
            .entity_id()
            .0;
        let tc = resolve_target_context(&c, &sa_id).unwrap();
        assert!(ttp_tool_satisfied(&ttp_with_tool("nmap"), &c, &tc));
    }

    #[test]
    fn software_requirement_is_uncertain_without_authoritative_evidence() {
        let mut campaign = empty_campaign();
        let pod = Pod::new("target", "default");
        let pod_id = pod.entity_id().0;
        campaign.entities.insert_typed(pod);
        let mut ttp = Ttp::new("redis", "Redis exploit", "Lateral Movement");
        ttp.requires.insert(
            "pkg:generic/redis".to_string(),
            json!(["<6.2.7", ">=7.0.0 <7.0.1"]),
        );

        let states = software_requirement_states(&ttp, &campaign, &pod_id);
        assert_eq!(states[0].status, RequirementStatus::Uncertain);

        let mut update = Pod::new("target", "default");
        update.system.software.push(SoftwareFact::new(
            "pkg:generic/redis@6.2.6",
            Some("6.2.6".to_string()),
            NameConfidence::Derived,
            KnowledgeProvenance::Action,
            "redis:6.2.6",
        ));
        campaign.entities.insert_typed(update);
        let states = software_requirement_states(&ttp, &campaign, &pod_id);
        assert_eq!(states[0].status, RequirementStatus::Uncertain);
        assert_eq!(states[0].evidence[0].confidence, NameConfidence::Derived);
    }

    #[test]
    fn authoritative_software_version_supports_or_contradicts_requirement() {
        let mut campaign = empty_campaign();
        let mut pod = Pod::new("target", "default");
        let pod_id = pod.entity_id().0;
        pod.system.software.push(SoftwareFact::new(
            "pkg:generic/redis@6.2.6",
            Some("6.2.6".to_string()),
            NameConfidence::Authoritative,
            KnowledgeProvenance::Action,
            "INFO server",
        ));
        campaign.entities.insert_typed(pod);
        let mut ttp = Ttp::new("redis", "Redis exploit", "Lateral Movement");
        ttp.requires
            .insert("pkg:generic/redis".to_string(), json!(["<6.2.7"]));
        assert_eq!(
            software_requirement_states(&ttp, &campaign, &pod_id)[0].status,
            RequirementStatus::Supported
        );

        ttp.requires
            .insert("pkg:generic/redis".to_string(), json!([">=6.2.7"]));
        assert_eq!(
            software_requirement_states(&ttp, &campaign, &pod_id)[0].status,
            RequirementStatus::Contradicted
        );
    }
}
