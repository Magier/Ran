use crate::error::ArmoryError;
use crate::model::Ttp;
use crate::raw::RawTtp;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct Armory {
    source_dir: PathBuf,
    ttps: Vec<Ttp>,
}

impl Armory {
    pub fn load_from_dir(path: impl AsRef<Path>) -> Result<Self, ArmoryError> {
        let source_dir = path.as_ref().to_path_buf();
        if !source_dir.exists() {
            return Err(ArmoryError::DirNotFound(source_dir.display().to_string()));
        }

        let mut ttps = Vec::new();

        for entry in WalkDir::new(&source_dir).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }

            let file_path = entry.path();
            let is_yaml = file_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"))
                .unwrap_or(false);
            if !is_yaml {
                continue;
            }

            let raw = fs::read_to_string(file_path).map_err(|source| ArmoryError::ReadFile {
                path: file_path.display().to_string(),
                source,
            })?;

            let raw_ttp: RawTtp = serde_yaml::from_str(&raw).map_err(|source| ArmoryError::ParseYaml {
                path: file_path.display().to_string(),
                source,
            })?;

            if let Some(ttp) = raw_ttp.into_ttp(file_path) {
                ttps.push(ttp);
            }
        }

        if ttps.is_empty() {
            return Err(ArmoryError::NoTtpsLoaded(source_dir.display().to_string()));
        }

        Ok(Self { source_dir, ttps })
    }

    pub fn source_dir(&self) -> &Path {
        &self.source_dir
    }

    pub fn ttps(&self) -> &[Ttp] {
        &self.ttps
    }

    pub fn ttps_for_tactic(&self, tactic: Option<&str>) -> Vec<Ttp> {
        let Some(tactic) = tactic.and_then(|t| {
            let trimmed = t.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }) else {
            return self.ttps.clone();
        };

        self.ttps
            .iter()
            .filter(|ttp| ttp.tactic.eq_ignore_ascii_case(tactic))
            .cloned()
            .collect()
    }
}
