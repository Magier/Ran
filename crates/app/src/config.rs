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
}

/// Action-selection (utility AI) configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoringConfig {
    /// How per-consideration scores are combined into a single utility:
    /// `weighted_arithmetic` (default), `weighted_geometric`, or
    /// `iaus_multiplicative`.
    pub combination: campaign::CombinationMode,
}

impl ScoringConfig {
    /// Build the scoring [`campaign::Profile`] this config describes.
    pub fn to_profile(&self) -> campaign::Profile {
        campaign::Profile {
            combination: self.combination,
            ..campaign::Profile::default()
        }
    }
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

    let cfg: Config = serde_yaml::from_slice(&data)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {}", path.display(), e))?;

    debug!(path = %path.display(), "config loaded");
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use campaign::CombinationMode;

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
