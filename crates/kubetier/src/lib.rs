//! Offline KubeTier catalog loading and matching data.
//!
//! Ran never contacts KubeTier at runtime. The public metadata snapshot is
//! embedded in the binary and may be overlaid by a locally generated full
//! catalog configured in `ran.yaml`.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const SOURCE_URL: &str = "https://kubetier.com/llms.txt";
pub const ATTRIBUTION: &str = "Assessment data by KubeTier (https://kubetier.com/)";
const EMBEDDED_CATALOG: &str = include_str!("../data/catalog.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema_version: u32,
    pub attribution: String,
    pub source_url: String,
    pub fetched_at: String,
    pub source_etag: Option<String>,
    pub source_sha256: String,
    pub validated_kubernetes_version: Option<String>,
    pub full: bool,
    pub permissions: Vec<PermissionAssessment>,
    pub roles: Vec<RoleAssessment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    T0,
    T1,
    T2,
    T3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Cluster,
    Namespaced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionAssessment {
    pub id: String,
    pub verb: String,
    pub resource: String,
    pub api_group: String,
    pub scope: Scope,
    pub tier: Tier,
    pub escalation_count: usize,
    pub source_url: String,
    pub kubernetes_doc_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub escalation_paths: Vec<EscalationPath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EscalationPath {
    pub name: String,
    pub tier: Tier,
    pub source_url: String,
    #[serde(default)]
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleAssessment {
    pub id: String,
    pub name: String,
    pub scope: Scope,
    pub tier: Tier,
    pub source_url: String,
    pub kubernetes_doc_url: Option<String>,
    #[serde(default)]
    pub rules: Vec<RoleRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleRule {
    #[serde(default)]
    pub api_groups: Vec<String>,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub non_resource_urls: Vec<String>,
    pub verbs: Vec<String>,
}

impl Catalog {
    pub fn embedded() -> Self {
        serde_json::from_str(EMBEDDED_CATALOG)
            .expect("embedded KubeTier catalog must be valid JSON")
    }

    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::embedded());
        };
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read KubeTier catalog {}", path.display()))?;
        let catalog: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse KubeTier catalog {}", path.display()))?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported KubeTier catalog schema"
        );
        anyhow::ensure!(
            self.attribution == ATTRIBUTION,
            "invalid KubeTier attribution"
        );
        anyhow::ensure!(
            self.source_url.starts_with("https://kubetier.com/"),
            "unexpected KubeTier source URL"
        );
        anyhow::ensure!(!self.fetched_at.is_empty(), "missing retrieval date");
        anyhow::ensure!(
            self.source_sha256.len() == 64
                && self
                    .source_sha256
                    .chars()
                    .all(|character| character.is_ascii_hexdigit()),
            "invalid KubeTier source checksum"
        );
        anyhow::ensure!(
            !self.permissions.is_empty(),
            "KubeTier catalog has no permissions"
        );
        let mut ids = std::collections::HashSet::new();
        for permission in &self.permissions {
            validate_id(&permission.id)?;
            anyhow::ensure!(
                ids.insert(&permission.id),
                "duplicate permission id {}",
                permission.id
            );
            validate_source_url(&permission.source_url)?;
            validate_documentation_url(permission.kubernetes_doc_url.as_deref())?;
            anyhow::ensure!(
                permission.source_url == format!("https://kubetier.com/{}", permission.id),
                "permission source URL does not match id {}",
                permission.id
            );
            if self.full {
                anyhow::ensure!(
                    permission.escalation_paths.len() == permission.escalation_count,
                    "permission {} has an invalid escalation count",
                    permission.id
                );
                for path in &permission.escalation_paths {
                    validate_source_url(&path.source_url)?;
                }
            } else {
                anyhow::ensure!(
                    permission.description.is_none() && permission.escalation_paths.is_empty(),
                    "public catalog contains copied prose for {}",
                    permission.id
                );
            }
        }
        anyhow::ensure!(
            self.roles.len() == 15,
            "expected 15 built-in roles, found {}",
            self.roles.len()
        );
        ids.clear();
        for role in &self.roles {
            validate_id(&role.id)?;
            anyhow::ensure!(ids.insert(&role.id), "duplicate role id {}", role.id);
            validate_source_url(&role.source_url)?;
            validate_documentation_url(role.kubernetes_doc_url.as_deref())?;
            anyhow::ensure!(
                role.source_url == format!("https://kubetier.com/{}", role.id),
                "role source URL does not match id {}",
                role.id
            );
            anyhow::ensure!(!role.rules.is_empty(), "role {} has no rules", role.id);
            if !self.full {
                anyhow::ensure!(
                    role.description.is_none() && role.notes.is_empty(),
                    "public catalog contains copied role prose for {}",
                    role.id
                );
            }
        }
        Ok(())
    }
}

fn validate_id(id: &str) -> Result<()> {
    anyhow::ensure!(
        !id.is_empty()
            && id.chars().all(
                |character| character.is_ascii_alphanumeric() || matches!(character, '-' | ':')
            ),
        "unsafe catalog id: {id}"
    );
    Ok(())
}

fn validate_source_url(url: &str) -> Result<()> {
    anyhow::ensure!(
        url.starts_with("https://kubetier.com/") && !url.contains(".."),
        "catalog contains non-KubeTier URL: {url}"
    );
    Ok(())
}

fn validate_documentation_url(url: Option<&str>) -> Result<()> {
    if let Some(url) = url {
        anyhow::ensure!(
            url.starts_with("https://") && !url.contains(char::is_whitespace),
            "invalid documentation URL: {url}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_valid_metadata_only_snapshot() {
        let catalog = Catalog::embedded();
        catalog.validate().unwrap();
        assert!(!catalog.full);
        assert_eq!(catalog.roles.len(), 15);
        assert!(catalog.permissions.len() >= 150);
        assert!(catalog.permissions.iter().all(|p| p.description.is_none()));
    }

    #[test]
    fn rejects_foreign_urls_and_duplicate_ids() {
        let mut catalog = Catalog::embedded();
        catalog.permissions[0].source_url = "https://example.com/stolen".into();
        assert!(catalog.validate().is_err());

        let mut catalog = Catalog::embedded();
        catalog.permissions[1].id = catalog.permissions[0].id.clone();
        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_schema_drift_and_prose_in_public_catalog() {
        let mut catalog = Catalog::embedded();
        catalog.schema_version = 2;
        assert!(catalog.validate().is_err());

        let mut catalog = Catalog::embedded();
        catalog.permissions[0].description = Some("copied text".into());
        assert!(catalog.validate().is_err());
    }
}
