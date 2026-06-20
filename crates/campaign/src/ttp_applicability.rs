use ran_domain::{AccessLevel, C2Server, Entity as _, Pod, ServiceAccount};
use serde_json::Value;

use crate::{Campaign, CampaignEntityRef};

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

    Some(TargetContext {
        target_id: target_id.to_string(),
        target_kind,
        is_system,
        access_level,
        has_token,
    })
}

/// Aggregate applicability gate: `true` when `ttp` can run against the target
/// described by `tc` given current campaign state. This is the single source of
/// truth combining all five precondition predicates plus the kind match.
pub fn ttp_applicable_for_target(
    ttp: &armory::Ttp,
    campaign: &Campaign,
    tc: &TargetContext,
) -> bool {
    ttp_is_applicable_for_target_kind(ttp, &tc.target_kind, tc.is_system)
        && ttp_rbac_satisfied(ttp, campaign)
        && ttp_exists_satisfied(ttp, campaign)
        && (!tc.is_system || ttp_access_level_satisfied(ttp, tc.access_level))
        && ttp_has_token_satisfied(ttp, tc.has_token)
        && ttp_related_satisfied(ttp, &tc.target_id, &tc.target_kind, campaign)
        && ttp_tool_satisfied(ttp, campaign, tc)
}

/// Returns `false` only when the action cannot run because *every* procedure's
/// required tool is **known absent** on the target. Procedures whose tool is
/// present or unknown, operator-side procedures (local / recon / resource-dev),
/// and non-system targets all pass — we can't rule them out.
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
/// that implements `SystemEntity` — i.e. wherever `is_system_target` is `true`.
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
            "listener" => campaign
                .entities
                .values::<C2Server>()
                .any(|c2| !c2.listeners.is_empty()),
            _ => false, // unknown entity kind — fail safe
        }
    })
}

/// Returns `true` when the TTP's RBAC requirements are satisfied by at least
/// one captured ServiceAccount in the campaign.
///
/// - No `rbacPermissions` in `requires` → satisfied (no restriction).
/// - `rbacPermissions` is present but the campaign has **no** ServiceAccounts
///   → not satisfied (we have no identity to act with).
/// - At least one SA must satisfy **all** required permissions.
pub fn ttp_rbac_satisfied(ttp: &armory::Ttp, campaign: &Campaign) -> bool {
    let Some(Value::Array(reqs)) = ttp.requires.get("rbacPermissions") else {
        return true;
    };

    if reqs.is_empty() {
        return true;
    }

    // RBAC requirement present: we need at least one known SA that covers all of them.
    if campaign.entities.get::<ServiceAccount>().is_empty() {
        return false;
    }

    campaign.entities.values::<ServiceAccount>().any(|sa| {
        reqs.iter().all(|req| {
            let Some(obj) = req.as_object() else {
                return true; // under-specified — assume satisfied
            };
            let verb = obj.get("verb").and_then(Value::as_str).unwrap_or("");
            let resource = obj
                .get("resourceType")
                .and_then(Value::as_str)
                .unwrap_or("");

            if verb.is_empty() || resource.is_empty() {
                return true; // under-specified — assume satisfied
            }

            sa.entitlements
                .iter()
                .any(|perm| perm.satisfies(verb, resource))
        })
    })
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
    use ran_domain::{C2Server, K8sCluster, RbacPermission, ServiceAccount};
    use serde_json::json;

    use ran_domain::AccessLevel;

    use super::{ttp_access_level_satisfied, ttp_exists_satisfied, ttp_rbac_satisfied};

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

    fn empty_campaign() -> crate::Campaign {
        crate::Campaign::bootstrap("test", K8sCluster::new("test"))
    }

    fn campaign_with_sa(verb: &str, resource_type: &str) -> crate::Campaign {
        let mut c = empty_campaign();
        let mut sa = ServiceAccount::new("attacker", "default");
        sa.entitlements
            .push(RbacPermission::new(verb, resource_type));
        c.entities.insert_typed(sa);
        c
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
        // Listener mechanics not yet ported: C2Server.listeners is always empty.
        assert!(!ttp_exists_satisfied(
            &ttp_with_exists("Listener"),
            &empty_campaign()
        ));
    }

    #[test]
    fn exists_satisfied_when_c2_has_listener() {
        let mut c = empty_campaign();
        let mut c2 = C2Server::new("ran");
        c2.listeners.push("tcp-1337".to_string());
        c.entities.insert_typed(c2);
        assert!(ttp_exists_satisfied(&ttp_with_exists("Listener"), &c));
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
    fn rbac_not_satisfied_when_no_service_accounts_in_campaign() {
        // TTP has RBAC requirements but no SA has been captured yet → must fail.
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

    use super::{resolve_target_context, ttp_applicable_for_target};
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
