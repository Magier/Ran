use std::fmt;

use serde::{Deserialize, Serialize};

/// An RBAC permission that a subject holds on the cluster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RbacPermission {
    pub verb: String,
    pub resource_type: String,
    /// If `None`, applies to all resources of this type.
    pub resource_name: Option<String>,
    pub api_group: Option<String>,
    /// `None` = namespace-local only.
    /// `Some("*")` = cluster-wide (ClusterRole/ClusterRoleBinding).
    /// `Some(ns)` = bound in a specific namespace.
    pub scope: Option<String>,
    /// Which (Cluster)Role granted this permission.
    pub source_role: Option<String>,
}

impl RbacPermission {
    pub fn new(verb: impl Into<String>, resource_type: impl Into<String>) -> Self {
        RbacPermission {
            verb: verb.into(),
            resource_type: resource_type.into(),
            resource_name: None,
            api_group: None,
            scope: None,
            source_role: None,
        }
    }

    pub fn is_cluster_wide(&self) -> bool {
        self.scope.as_deref() == Some("*")
    }

    pub fn is_in_scope(&self, namespace: &str) -> bool {
        match self.scope.as_deref() {
            Some("*") => true,
            Some(s) => s == namespace,
            None => false,
        }
    }

    /// Returns `true` if this permission satisfies the requested verb+resource.
    /// Wildcard verb/resource (`"*"`) matches anything.
    pub fn satisfies(&self, verb: &str, resource: &str) -> bool {
        let verb_ok = self.verb == "*" || self.verb == verb;
        let res_ok = self.resource_type == "*" || self.resource_type == resource;
        verb_ok && res_ok
    }
}

impl fmt::Display for RbacPermission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.verb, self.resource_type)?;
        if let Some(name) = &self.resource_name {
            write!(f, "/{}", name)?;
        }
        Ok(())
    }
}
