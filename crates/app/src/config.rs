use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Top-level Ran configuration, loaded from `ran.yaml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub namespaces: NamespaceFilter,
    pub scoring: ScoringConfig,
    pub plans: PlansConfig,
    #[serde(rename = "seedKnowledge")]
    pub seed_knowledge: Vec<SeedKnowledgeConfig>,
}

impl Config {
    /// Directory to read pre-defined plans from, defaulting to `plans` in the
    /// current working directory when unset.
    pub fn plans_dir(&self) -> PathBuf {
        self.plans
            .dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("plans"))
    }
}

/// Where the web UI and CLI look for pre-defined plan files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlansConfig {
    /// Directory containing `*.plan.yaml` files. Defaults to `plans` in the
    /// current working directory when unset. See [`Config::plans_dir`].
    pub dir: Option<PathBuf>,
}

/// Action-selection (utility AI) configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoringConfig {
    /// How per-consideration scores are combined into a single utility:
    /// `weighted_arithmetic` (default), `weighted_geometric`, or
    /// `iaus_multiplicative`.
    pub combination: utility_ai::CombinationMode,
    /// Feature flag: when `true`, the frontend exposes the live response-curve /
    /// weight tuning flyout for the scoring considerations.
    pub tuning_ui: bool,
}

impl ScoringConfig {
    /// Build the scoring [`utility_ai::Profile`] this config describes.
    pub fn to_profile(&self) -> utility_ai::Profile {
        utility_ai::Profile {
            combination: self.combination,
            ..utility_ai::Profile::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SeedKnowledgeConfig {
    Cluster(SeedClusterConfig),
    Credential(SeedCredentialConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedClusterConfig {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub context_name: Option<String>,
    pub provenance: campaign::KnowledgeProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedCredentialConfig {
    pub credential_type: String,
    pub id: String,
    pub path: PathBuf,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub cluster: Option<String>,
    pub provenance: campaign::KnowledgeProvenance,
}

/// Controls which namespaces are visible during discovery.
///
/// - **Whitelist mode**: if `included` is non-empty, only those namespaces are shown.
/// - **Blacklist mode**: if `included` is empty, namespaces in `excluded` are hidden.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NamespaceFilter {
    /// Namespaces to hide (blacklist mode, active only when `included` is empty).
    pub excluded: Vec<String>,
    /// Namespaces to show (whitelist mode, takes precedence over `excluded`).
    pub included: Vec<String>,
}

impl Default for NamespaceFilter {
    fn default() -> Self {
        Self {
            excluded: vec!["kube-system".to_string(), "local-path-storage".to_string()],
            included: vec![],
        }
    }
}

impl NamespaceFilter {
    /// Returns `true` if `ns` should be included based on the current filter.
    pub fn should_include(&self, ns: &str) -> bool {
        if !self.included.is_empty() {
            return self.included.iter().any(|a| a == ns);
        }
        !self.excluded.iter().any(|e| e == ns)
    }
}

/// Load configuration from `path`, defaulting to `ran.yaml` in the current directory.
/// If the file does not exist the default configuration is returned silently.
pub fn load(path: Option<PathBuf>) -> Result<Config> {
    let path = path.unwrap_or_else(|| PathBuf::from("ran.yaml"));

    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!(path = %path.display(), "ran.yaml not found, using defaults");
            return Ok(Config::default());
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to read config file {}: {}",
                path.display(),
                e
            ));
        }
    };

    let mut cfg: Config = serde_yaml::from_slice(&data)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;

    let base_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    for seed in &mut cfg.seed_knowledge {
        if let SeedKnowledgeConfig::Credential(credential) = seed {
            if credential.credential_type != "kubeconfig" {
                return Err(anyhow::anyhow!(
                    "unsupported credentialType '{}' for seed '{}'",
                    credential.credential_type,
                    credential.id
                ));
            }
            if credential.path.is_relative() {
                credential.path = base_dir.join(&credential.path);
            }
        }
    }

    let mut ids = std::collections::HashSet::new();
    for seed in &cfg.seed_knowledge {
        let id = match seed {
            SeedKnowledgeConfig::Cluster(cluster) => &cluster.id,
            SeedKnowledgeConfig::Credential(credential) => &credential.id,
        };
        if !ids.insert(id.clone()) {
            return Err(anyhow::anyhow!("duplicate seedKnowledge id '{}'", id));
        }
    }

    debug!(path = %path.display(), "config loaded");
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use utility_ai::CombinationMode;

    #[test]
    fn plans_dir_defaults_to_plans() {
        let cfg = Config::default();
        assert_eq!(cfg.plans_dir(), PathBuf::from("plans"));
    }

    #[test]
    fn plans_dir_honors_configured_value() {
        let cfg: Config = serde_yaml::from_str("plans:\n  dir: /custom/plans").unwrap();
        assert_eq!(cfg.plans_dir(), PathBuf::from("/custom/plans"));
    }

    #[test]
    fn scoring_defaults_to_weighted_arithmetic() {
        let cfg: Config = serde_yaml::from_str("namespaces: {}").unwrap();
        assert_eq!(cfg.scoring.combination, CombinationMode::WeightedArithmetic);
    }

    #[test]
    fn scoring_combination_parses_each_mode() {
        for (yaml, mode) in [
            ("weighted_arithmetic", CombinationMode::WeightedArithmetic),
            ("weighted_geometric", CombinationMode::WeightedGeometric),
            ("iaus_multiplicative", CombinationMode::IausMultiplicative),
        ] {
            let cfg: Config =
                serde_yaml::from_str(&format!("scoring:\n  combination: {yaml}")).unwrap();
            assert_eq!(cfg.scoring.combination, mode);
            assert_eq!(cfg.scoring.to_profile().combination, mode);
        }
    }
}
