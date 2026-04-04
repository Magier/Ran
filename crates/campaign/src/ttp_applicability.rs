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
                .c2_servers
                .values()
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
    if campaign.service_accounts.is_empty() {
        return false;
    }

    campaign.service_accounts.values().any(|sa| {
        reqs.iter().all(|req| {
            let Some(obj) = req.as_object() else {
                return true; // under-specified — assume satisfied
            };
            let verb = obj.get("verb").and_then(Value::as_str).unwrap_or("");
            let resource = obj.get("resourceType").and_then(Value::as_str).unwrap_or("");

            if verb.is_empty() || resource.is_empty() {
                return true; // under-specified — assume satisfied
            }

            sa.entitlements.iter().any(|perm| perm.satisfies(verb, resource))
        })
    })
}

#[cfg(test)]
mod tests {
    use armory::Ttp;
    use ran_domain::{C2Server, Entity, K8sCluster, RbacPermission, ServiceAccount};
    use serde_json::json;

    use super::{ttp_exists_satisfied, ttp_rbac_satisfied};

    fn ttp_with_rbac(verb: &str, resource_type: &str) -> Ttp {
        let mut requires = serde_json::Map::new();
        requires.insert(
            "rbacPermissions".to_string(),
            json!([{"verb": verb, "resourceType": resource_type}]),
        );
        Ttp {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: String::new(),
            tactic: "Discovery".to_string(),
            techniques: vec![],
            status: "enabled".to_string(),
            params: vec![],
            requires,
            effects: vec![],
            procedures: vec![],
            references: vec![],
        }
    }

    fn ttp_no_rbac() -> Ttp {
        Ttp {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: String::new(),
            tactic: "Discovery".to_string(),
            techniques: vec![],
            status: "enabled".to_string(),
            params: vec![],
            requires: serde_json::Map::new(),
            effects: vec![],
            procedures: vec![],
            references: vec![],
        }
    }

    fn empty_campaign() -> crate::Campaign {
        crate::Campaign::bootstrap("test", K8sCluster::new("test"))
    }

    fn campaign_with_sa(verb: &str, resource_type: &str) -> crate::Campaign {
        let mut c = empty_campaign();
        let mut sa = ServiceAccount::new("attacker", "default");
        sa.entitlements.push(RbacPermission::new(verb, resource_type));
        c.service_accounts.insert(sa.entity_id(), sa);
        c
    }

    fn ttp_with_exists(kind: &str) -> Ttp {
        let mut requires = serde_json::Map::new();
        requires.insert("exists".to_string(), json!([kind]));
        Ttp {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: String::new(),
            tactic: "Resource Development".to_string(),
            techniques: vec![],
            status: "enabled".to_string(),
            params: vec![],
            requires,
            effects: vec![],
            procedures: vec![],
            references: vec![],
        }
    }

    #[test]
    fn exists_satisfied_when_no_constraint() {
        assert!(ttp_exists_satisfied(&ttp_no_rbac(), &empty_campaign()));
    }

    #[test]
    fn exists_not_satisfied_when_listener_required_and_none_in_campaign() {
        // Listener mechanics not yet ported: C2Server.listeners is always empty.
        assert!(!ttp_exists_satisfied(&ttp_with_exists("Listener"), &empty_campaign()));
    }

    #[test]
    fn exists_satisfied_when_c2_has_listener() {
        let mut c = empty_campaign();
        let mut c2 = C2Server::new("ran");
        c2.listeners.push("tcp-1337".to_string());
        c.c2_servers.insert(c2.entity_id(), c2);
        assert!(ttp_exists_satisfied(&ttp_with_exists("Listener"), &c));
    }

    #[test]
    fn exists_not_satisfied_for_unknown_entity_kind() {
        // Unknown kinds fail safe so phantom pre-conditions don't silently pass.
        assert!(!ttp_exists_satisfied(&ttp_with_exists("UnknownThing"), &empty_campaign()));
    }

    #[test]
    fn rbac_satisfied_when_no_requirement() {
        let c = empty_campaign();
        assert!(ttp_rbac_satisfied(&ttp_no_rbac(), &c));
    }

    #[test]
    fn rbac_not_satisfied_when_no_service_accounts_in_campaign() {
        // TTP has RBAC requirements but no SA has been captured yet → must fail.
        assert!(!ttp_rbac_satisfied(&ttp_with_rbac("delete", "events"), &empty_campaign()));
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
