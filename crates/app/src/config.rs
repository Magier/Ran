use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Top-level Ran configuration, loaded from `ran.yaml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub namespaces: NamespaceFilter,
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
