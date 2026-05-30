use crate::error::ArmoryError;
use crate::model::{Procedure, Ttp};
use crate::raw::RawTtp;
use std::collections::HashMap;
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

        // --- Phase 3: expand slot references ---------------------------------
        Self::expand_slot_procedures(&mut ttps);

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
        let mut ttps = Self::ttps_from_dir(path)?;

        if ttps.is_empty() {
            return Err(ArmoryError::NoTtpsLoaded(path.display().to_string()));
        }

        Self::expand_slot_procedures(&mut ttps);

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

    /// Find a tool TTP by its ID (e.g. `"curl"`, `"wget"`).
    /// Only returns TTPs that declare a `tool_slot`.
    pub fn get_tool_ttp(&self, id: &str) -> Option<&Ttp> {
        self.ttps
            .iter()
            .find(|t| t.tool_slot.is_some() && t.id == id)
    }

    /// Return all tool TTPs that fill the given slot (e.g. `"http-request"`).
    pub fn get_tools_for_slot(&self, slot: &str) -> Vec<&Ttp> {
        self.ttps
            .iter()
            .filter(|t| t.tool_slot.as_deref() == Some(slot))
            .collect()
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

    /// For each non-tool TTP, replace any procedure whose `tool` field names a
    /// known slot (e.g. `"http-request"`) with one cloned procedure per
    /// concrete tool that fills that slot.  The clone receives the concrete
    /// tool's ID as both its `id` and `tool` field.
    fn expand_slot_procedures(ttps: &mut [Ttp]) {
        // Build slot → [concrete tool IDs] map from tool TTPs.
        let mut slot_map: HashMap<String, Vec<String>> = HashMap::new();
        for ttp in ttps.iter() {
            if let Some(slot) = &ttp.tool_slot {
                slot_map
                    .entry(slot.clone())
                    .or_default()
                    .push(ttp.id.clone());
            }
        }

        if slot_map.is_empty() {
            return;
        }

        for ttp in ttps.iter_mut() {
            if ttp.tool_slot.is_some() {
                continue; // tool TTPs themselves are never expanded
            }

            let original = std::mem::take(&mut ttp.procedures);
            let mut expanded: Vec<Procedure> = Vec::with_capacity(original.len());

            for proc in original {
                let slot_tools = proc.tool.as_deref().and_then(|t| slot_map.get(t));

                match slot_tools {
                    Some(tool_ids) => {
                        for tool_id in tool_ids {
                            let mut p = proc.clone();
                            p.id = tool_id.clone();
                            p.tool = Some(tool_id.clone());
                            expanded.push(p);
                        }
                    }
                    None => expanded.push(proc),
                }
            }

            ttp.procedures = expanded;
        }
    }

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
