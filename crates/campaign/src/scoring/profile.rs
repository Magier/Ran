use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::ResponseCurve;

fn default_weight() -> f32 {
    1.0
}

fn default_enabled() -> bool {
    true
}

/// How a candidate's per-consideration scores are combined into one utility.
///
/// Two of the three modes use a **value/gate split**: `veto` considerations are
/// *gates* (necessary conditions) and always multiply in (a gate near 0 vetoes
/// the candidate), while non-veto considerations are *value drivers* combined by
/// the chosen mean. [`IausMultiplicative`] ignores the split and treats every
/// axis uniformly, in the classic Infinite Axis Utility System style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CombinationMode {
    /// `value = weighted arithmetic mean(value axes)`, then `× ∏(gate axes)`.
    /// A strong axis can offset a weak one. This is the baseline behavior.
    #[default]
    WeightedArithmetic,
    /// `value = weighted geometric mean(value axes)` (ε-floored), then
    /// `× ∏(gate axes)`. Rewards balance — lopsided actions score lower.
    WeightedGeometric,
    /// Faithful IAUS: multiply *all* enabled axes (value and gate alike) after
    /// applying the per-axis compensation factor `1 - 1/n`. Weights are ignored
    /// (importance is expressed through response curves).
    IausMultiplicative,
}

/// Per-consideration tuning within a [`Profile`]. Every field has a sensible
/// default, so a profile only needs to name the considerations it wants to
/// deviate from the baseline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConsiderationConfig {
    /// Relative importance in the weighted average. Ignored for `veto`
    /// considerations (they multiply instead of averaging).
    #[serde(default = "default_weight")]
    pub weight: f32,
    /// Response curve applied to the raw measurement. Defaults to identity.
    #[serde(default)]
    pub curve: ResponseCurve,
    /// When `false`, this consideration is skipped entirely for the profile.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// When `true`, this consideration acts as a multiplicative gate rather than
    /// a weighted term: a curved score of `0` zeroes the whole candidate.
    #[serde(default)]
    pub veto: bool,
}

impl Default for ConsiderationConfig {
    fn default() -> Self {
        Self {
            weight: default_weight(),
            curve: ResponseCurve::default(),
            enabled: default_enabled(),
            veto: false,
        }
    }
}

/// A named operator preference: a weight/curve vector over the registered
/// considerations. Swapping the profile changes *what the scorer values*
/// without touching any scoring code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// How per-consideration scores are combined into one utility.
    #[serde(default)]
    pub combination: CombinationMode,
    /// Overrides keyed by [`Consideration::name`](super::Consideration::name).
    /// Considerations absent from this map use [`ConsiderationConfig::default`].
    #[serde(default)]
    pub considerations: HashMap<String, ConsiderationConfig>,
}

impl Profile {
    /// The configuration for `name`, falling back to defaults (enabled, weight
    /// 1, identity curve, no veto) when the profile doesn't mention it.
    pub fn config(&self, name: &str) -> ConsiderationConfig {
        self.considerations.get(name).cloned().unwrap_or_default()
    }
}

impl Default for Profile {
    /// The baseline profile: every consideration enabled at weight 1 with an
    /// identity curve.
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            combination: CombinationMode::default(),
            considerations: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_consideration_falls_back_to_default() {
        let p = Profile::default();
        let cfg = p.config("anything");
        assert_eq!(cfg, ConsiderationConfig::default());
        assert!(cfg.enabled);
        assert_eq!(cfg.weight, 1.0);
        assert!(!cfg.veto);
    }

    #[test]
    fn named_override_is_returned() {
        let mut considerations = HashMap::new();
        considerations.insert(
            "cost".to_string(),
            ConsiderationConfig {
                weight: 2.5,
                ..Default::default()
            },
        );
        let p = Profile {
            name: "test".to_string(),
            considerations,
            ..Profile::default()
        };
        assert_eq!(p.config("cost").weight, 2.5);
        assert_eq!(p.config("novelty").weight, 1.0);
    }
}
