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
        || procedure
            .command
            .trim_start()
            .starts_with("c2.kubectl_exec(")
        || procedure
            .command
            .trim_start()
            .starts_with("k8sSelfSubjectRulesReview(")
}

pub fn ttp_uses_k8s_auth(ttp: &armory::Ttp) -> bool {
    ttp.procedures.iter().any(procedure_uses_k8s_auth)
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
                credential.active || campaign.is_operator_host_credential(&credential.entity_id())
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
    /// `true` when the target implements `SystemEntity` (Pod / Node / UnknownSystem).
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
    ttp_target_scope_satisfied(ttp, tc)
        && ttp_auth_satisfied_for_target(ttp, campaign, tc)
        && ttp_execution_source_satisfied(ttp, campaign)
        && ttp_exists_satisfied(ttp, campaign)
        && ttp_has_listener_satisfied(ttp, campaign)
        && (!tc.is_system || ttp_access_level_satisfied(ttp, tc.access_level))
        && ttp_has_token_satisfied(ttp, tc.has_token)
        && ttp_active_session_satisfied(ttp, tc.active_session)
        && ttp_related_satisfied(ttp, &tc.target_id, &tc.target_kind, campaign)
        && ttp_tool_satisfied(ttp, campaign, tc)
        // Last: the only gate that touches the filesystem. `&&` short-circuits,
        // so it runs only for targets every cheaper gate already accepted.
        && ttp_operator_tool_satisfied(ttp)
}

/// Non-Kubernetes lateral movement must originate from an existing execution
/// foothold. Merely discovering a possible remote target is not enough to make
/// exploit actions runnable.
fn ttp_execution_source_satisfied(ttp: &armory::Ttp, campaign: &Campaign) -> bool {
    let tactic = ttp
        .tactic
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();

    !tactic.eq_ignore_ascii_case("LateralMovement")
        || ttp_uses_k8s_auth(ttp)
        || campaign.resolve_exec_source().is_ok()
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
        return !identities.is_empty()
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
/// - No `exists` in `requires` → satisfied.
/// - `"Listener"` → at least one C2Server must have a non-empty `listeners` list.
/// - Any other item → `false` (unknown entity kind; fail safe).
pub fn ttp_exists_satisfied(ttp: &armory::Ttp, campaign: &Campaign) -> bool {
    let Some(Value::Array(items)) = ttp.requires.get("exists") else {
        return true;
    };

    if items.is_empty() {
        return true;
    }

    items.iter().all(|item| {
        let kind = item.as_str().unwrap_or("").trim().to_ascii_lowercase();
        match kind.as_str() {
            "listener" => campaign.entities.values::<Listener>().next().is_some(),
            _ => false, // unknown entity kind - fail safe
        }
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
    use armory::Ttp;
    use ran_domain::{
        C2Server, K8sCluster, K8sCredential, K8sNode, Listener, RbacPermission, Redirector,
        ServiceAccount, SessionInfo, SessionStatus,
    };
    use serde_json::json;

    use ran_domain::AccessLevel;

    use super::{
        resolve_target_context, ttp_access_level_satisfied, ttp_applicable_for_target,
        ttp_exists_satisfied, ttp_has_listener_satisfied, ttp_operator_tool_satisfied,
        ttp_rbac_satisfied,
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

    fn ttp_with_has_listener(required: bool) -> Ttp {
        let mut ttp = ttp_no_rbac();
        ttp.requires
            .insert("c2.has-listener".to_string(), json!(required));
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

    use ran_domain::{Entity as _, JwToken, Pod, ServiceAccountToken};

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
        ttp.procedures = vec![armory::Procedure::new(
            "inspect",
            "k8sSelfSubjectRulesReview(default)",
        )];

        let tc = resolve_target_context(&c, &id).expect("credential should resolve");
        assert!(!ttp_applicable_for_target(&ttp, &c, &tc));

        credential.active = true;
        c.entities
            .get_mut::<K8sCredential>()
            .insert(ran_domain::EntityId::new(&id), credential);
        let tc = resolve_target_context(&c, &id).expect("credential should resolve");
        assert!(ttp_applicable_for_target(&ttp, &c, &tc));
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
}
