use crate::error::ArmoryError;
use crate::model::Ttp;
use crate::raw::RawTtp;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[cfg(feature = "bundled-armory")]
#[derive(rust_embed::Embed)]
#[folder = "../../armory/TTPs"]
struct BundledTtps;

#[derive(Debug, Clone)]
pub struct Armory {
    source_dir: PathBuf,
    ttps: Vec<Ttp>,
}

impl Armory {
    /// Primary entry point.
    ///
    /// **Dev builds** (no `bundled-armory` feature): loads TTPs exclusively from
    /// `user_dir`. The caller is responsible for resolving a default path when
    /// `user_dir` is `None`.
    ///
    /// **Release builds** (`bundled-armory` feature): always loads the built-in
    /// TTPs embedded in the binary first, then appends any TTPs found in
    /// `user_dir` (if provided). This mirrors the Go behaviour: built-ins are
    /// the baseline, the user directory extends them.
    pub fn load(user_dir: Option<&Path>) -> Result<Self, ArmoryError> {
        let mut ttps: Vec<Ttp> = Vec::new();

        // --- Phase 1: built-in TTPs (release only) ---------------------------
        #[cfg(feature = "bundled-armory")]
        {
            ttps.extend(Self::ttps_from_bundled()?);
        }

        // --- Phase 2: user-supplied directory --------------------------------
        if let Some(dir) = user_dir {
            ttps.extend(Self::ttps_from_dir(dir)?);
        }

        if ttps.is_empty() {
            let label = user_dir
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "<bundled>".to_string());
            return Err(ArmoryError::NoTtpsLoaded(label));
        }

        Ok(Self {
            source_dir: user_dir
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("<bundled>")),
            ttps,
        })
    }

    /// Load TTPs from a filesystem directory. Used by both `load()` and tests.
    pub fn load_from_dir(path: impl AsRef<Path>) -> Result<Self, ArmoryError> {
        let path = path.as_ref();
        let ttps = Self::ttps_from_dir(path)?;

        if ttps.is_empty() {
            return Err(ArmoryError::NoTtpsLoaded(path.display().to_string()));
        }

        Ok(Self {
            source_dir: path.to_path_buf(),
            ttps,
        })
    }

    pub fn source_dir(&self) -> &Path {
        &self.source_dir
    }

    pub fn ttps(&self) -> &[Ttp] {
        &self.ttps
    }

    /// Construct an `Armory` directly from a list of TTPs (useful in tests).
    pub fn from_ttps(ttps: Vec<Ttp>) -> Self {
        Self {
            source_dir: PathBuf::new(),
            ttps,
        }
    }

    pub fn get_ttp(&self, id: &str) -> Option<&Ttp> {
        self.ttps.iter().find(|ttp| ttp.id == id)
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

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    fn ttps_from_dir(dir: &Path) -> Result<Vec<Ttp>, ArmoryError> {
        if !dir.exists() {
            return Err(ArmoryError::DirNotFound(dir.display().to_string()));
        }

        let mut ttps = Vec::new();

        for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
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

            let raw_ttp: RawTtp =
                serde_yaml::from_str(&raw).map_err(|source| ArmoryError::ParseYaml {
                    path: file_path.display().to_string(),
                    source,
                })?;

            if let Some(ttp) = raw_ttp.into_ttp(file_path) {
                ttps.push(ttp);
            }
        }

        Ok(ttps)
    }

    #[cfg(feature = "bundled-armory")]
    fn ttps_from_bundled() -> Result<Vec<Ttp>, ArmoryError> {
        let mut ttps = Vec::new();

        for filename in BundledTtps::iter() {
            let file = BundledTtps::get(filename.as_ref()).expect("file listed but not found");
            let raw = std::str::from_utf8(file.data.as_ref())
                .map_err(|_| ArmoryError::InvalidUtf8(filename.to_string()))?;
            let raw_ttp: RawTtp =
                serde_yaml::from_str(raw).map_err(|source| ArmoryError::ParseYaml {
                    path: filename.to_string(),
                    source,
                })?;
            if let Some(ttp) = raw_ttp.into_ttp(Path::new(filename.as_ref())) {
                ttps.push(ttp);
            }
        }

        Ok(ttps)
    }
}
