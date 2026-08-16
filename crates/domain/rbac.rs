use std::fmt;

use serde::{Deserialize, Serialize};

/// How confidently Ran knows the breadth of an RBAC grant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RbacScopeKind {
    Cluster,
    Namespace,
    #[default]
    Unknown,
}

/// Evidence used to determine the RBAC scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RbacScopeSource {
    Binding,
    Role,
    Ssrr,
}

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
    /// Explicit breadth evidence. SSRR results are `Unknown` because they only
    /// describe permissions effective in the namespace being evaluated.
    #[serde(default)]
    pub scope_kind: RbacScopeKind,
    /// Namespace supplied to SSRR; not proof that the underlying grant is local.
    #[serde(default)]
    pub evaluated_namespace: Option<String>,
    #[serde(default)]
    pub scope_source: Option<RbacScopeSource>,
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
            scope_kind: RbacScopeKind::Unknown,
            evaluated_namespace: None,
            scope_source: None,
            source_role: None,
        }
    }

    pub fn is_cluster_wide(&self) -> bool {
        self.scope_kind == RbacScopeKind::Cluster || self.scope.as_deref() == Some("*")
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
