use ran_domain::{AccessLevel, C2Server, Entity as _, Pod, ServiceAccount};
use serde_json::Value;

use crate::Campaign;

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
}
