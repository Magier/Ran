use serde::Serialize;

use crate::ttp_applicability::{resolve_target_context, ttp_applicable_for_target};
use crate::Campaign;

use super::considerations::default_considerations;
use super::{CombinationMode, Consideration, Profile, ScoringContext};

/// Floor applied to value inputs before taking logs in geometric mode, so a
/// legitimately-zero value axis (e.g. a recon TTP with no privilege effect)
/// drags the mean down rather than annihilating the whole score.
const GEOMETRIC_EPS: f32 = 1e-3;

/// One consideration's contribution to a candidate's utility, retained for
/// explainability (UI, tuning, debugging).
#[derive(Debug, Clone, Serialize)]
pub struct ConsiderationScore {
    pub name: &'static str,
    /// Raw measurement in `[0, 1]`.
    pub raw: f32,
    /// After the profile's response curve.
    pub curved: f32,
    /// Weight applied (from the profile).
    pub weight: f32,
    /// Whether this consideration acted as a multiplicative veto.
    pub veto: bool,
}

/// A scored, grounded `(TTP × target)` candidate.
#[derive(Debug, Clone, Serialize)]
pub struct ScoredCandidate {
    pub ttp_id: String,
    pub target_id: String,
    /// Final utility in `[0, 1]`.
    pub utility: f32,
    /// Per-consideration breakdown, in registration order.
    pub breakdown: Vec<ConsiderationScore>,
}

/// Ranks applicable grounded actions by utility for a given [`Profile`].
///
/// The profile's [`CombinationMode`] decides how the per-consideration scores
/// are merged (weighted arithmetic/geometric mean over value axes × gate axes,
/// or faithful IAUS multiplication). A profile is otherwise just a weight/curve
/// vector — swapping it changes preferences without touching scoring code.
pub struct Scorer {
    considerations: Vec<Box<dyn Consideration>>,
    profile: Profile,
}

impl Scorer {
    /// Build a scorer from an explicit consideration set.
    pub fn new(considerations: Vec<Box<dyn Consideration>>, profile: Profile) -> Self {
        Self {
            considerations,
            profile,
        }
    }

    /// Build a scorer with the Phase 1 built-in considerations.
    pub fn with_defaults(profile: Profile) -> Self {
        Self::new(default_considerations(), profile)
    }

    /// Score a single grounded candidate.
    pub fn score_candidate(&self, ctx: &ScoringContext) -> ScoredCandidate {
        let mut breakdown = Vec::with_capacity(self.considerations.len());
        // (weight, curved) for non-veto value axes; curved for veto gate axes.
        let mut value_axes: Vec<(f32, f32)> = Vec::new();
        let mut gate_curves: Vec<f32> = Vec::new();
        let mut all_curves: Vec<f32> = Vec::new();

        for c in &self.considerations {
            let cfg = self.profile.config(c.name());
            if !cfg.enabled {
                continue;
            }
            let raw = c.measure(ctx).clamp(0.0, 1.0);
            let curved = cfg.curve.apply(raw);

            all_curves.push(curved);
            if cfg.veto {
                gate_curves.push(curved);
            } else {
                value_axes.push((cfg.weight, curved));
            }

            breakdown.push(ConsiderationScore {
                name: c.name(),
                raw,
                curved,
                weight: cfg.weight,
                veto: cfg.veto,
            });
        }

        let utility = match self.profile.combination {
            // Faithful IAUS: every axis multiplies, with compensation.
            CombinationMode::IausMultiplicative => iaus_combine(&all_curves),
            // Hybrid: value mean × gate product.
            CombinationMode::WeightedArithmetic => {
                weighted_arithmetic(&value_axes) * product(&gate_curves)
            }
            CombinationMode::WeightedGeometric => {
                weighted_geometric(&value_axes) * product(&gate_curves)
            }
        };

        ScoredCandidate {
            ttp_id: ctx.ttp.id.clone(),
            target_id: ctx.tc.target_id.clone(),
            utility,
            breakdown,
        }
    }

    /// Enumerate every applicable `(TTP × target)` candidate across the campaign
    /// and return them ranked by descending utility.
    ///
    /// Ties break deterministically on `(ttp_id, target_id)` so the ordering is
    /// reproducible. Complexity is `O(entities × ttps)`; fine at current scale.
    pub fn rank(&self, campaign: &Campaign, armory: &[armory::Ttp]) -> Vec<ScoredCandidate> {
        let mut out = Vec::new();

        for entity in campaign.get_entities() {
            let target_id = entity.entity_id().0;
            let Some(tc) = resolve_target_context(campaign, &target_id) else {
                continue;
            };
            for ttp in armory {
                if !ttp_applicable_for_target(ttp, campaign, &tc) {
                    continue;
                }
                let ctx = ScoringContext {
                    campaign,
                    ttp,
                    tc: &tc,
                };
                out.push(self.score_candidate(&ctx));
            }
        }

        out.sort_by(|a, b| {
            b.utility
                .partial_cmp(&a.utility)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.ttp_id.cmp(&b.ttp_id))
                .then_with(|| a.target_id.cmp(&b.target_id))
        });

        out
    }
}

/// Product of curved scores. Empty (no gate axes) → `1.0` (neutral).
fn product(curves: &[f32]) -> f32 {
    curves.iter().copied().product()
}

/// Weighted arithmetic mean. No value axes → `1.0` (neutral, so gates alone
/// decide the score).
fn weighted_arithmetic(axes: &[(f32, f32)]) -> f32 {
    let wsum: f32 = axes.iter().map(|(w, _)| w).sum();
    if wsum <= 0.0 {
        return 1.0;
    }
    axes.iter().map(|(w, c)| w * c).sum::<f32>() / wsum
}

/// Weighted geometric mean, computed in log space with an ε-floor on inputs.
/// No value axes → `1.0` (neutral).
fn weighted_geometric(axes: &[(f32, f32)]) -> f32 {
    let wsum: f32 = axes.iter().map(|(w, _)| w).sum();
    if wsum <= 0.0 {
        return 1.0;
    }
    let log_sum: f32 = axes
        .iter()
        .map(|(w, c)| w * c.max(GEOMETRIC_EPS).ln())
        .sum();
    (log_sum / wsum).exp()
}

/// Faithful IAUS combination: multiply every axis after applying the
/// compensation factor `1 - 1/n`, which counteracts the dilution of multiplying
/// many `[0, 1]` values. No axes → `0.0`.
fn iaus_combine(curves: &[f32]) -> f32 {
    let n = curves.len();
    if n == 0 {
        return 0.0;
    }
    let mod_factor = 1.0 - 1.0 / n as f32;
    curves
        .iter()
        .map(|&c| c + (1.0 - c) * mod_factor * c)
        .product()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExecutionRecord;
    use armory::{Procedure, Ttp};
    use ran_domain::K8sCluster;
    use serde_json::json;
    use std::collections::HashMap;

    fn system_ttp(id: &str) -> Ttp {
        let mut requires = serde_json::Map::new();
        requires.insert("kind".to_string(), json!("System"));
        Ttp {
            status: "enabled".to_string(),
            requires,
            procedures: vec![Procedure::new("shell", "id")],
            ..Ttp::new(id, id, "Discovery")
        }
    }

    fn success_record(ttp_id: &str, target_id: &str) -> ExecutionRecord {
        ExecutionRecord {
            id: format!("{ttp_id}-1"),
            ttp_id: ttp_id.to_string(),
            ttp_name: ttp_id.to_string(),
            tactic: "Discovery".to_string(),
            target_id: target_id.to_string(),
            exec_system_id: target_id.to_string(),
            procedure_id: "shell".to_string(),
            command: "id".to_string(),
            args: HashMap::new(),
            success: true,
            exit_code: 0,
            results: vec![],
            fail_reason: String::new(),
            started_at_ms: 0,
            completed_at_ms: 0,
            is_cleanup: false,
        }
    }

    fn campaign_with_reachable_pod() -> (Campaign, String) {
        let mut c = Campaign::bootstrap("test", K8sCluster::new("test"));
        let pod_id = c.seed_pod_for_trigger("nginx", "default").0;
        (c, pod_id)
    }

    #[test]
    fn rank_only_emits_applicable_system_candidates() {
        let (campaign, pod_id) = campaign_with_reachable_pod();
        let armory = vec![system_ttp("ttp-a")];
        let scorer = Scorer::with_defaults(Profile::default());

        let ranked = scorer.rank(&campaign, &armory);

        // System-only TTP applies to the pod, not to the bootstrap C2/Cluster.
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].ttp_id, "ttp-a");
        assert_eq!(ranked[0].target_id, pod_id);
        assert!(!ranked[0].breakdown.is_empty());
    }

    #[test]
    fn unrun_action_outranks_already_run_action() {
        let (mut campaign, pod_id) = campaign_with_reachable_pod();
        // ttp-b has already been run successfully against the pod once.
        campaign
            .execution_records
            .push(success_record("ttp-b", &pod_id));

        let armory = vec![system_ttp("ttp-a"), system_ttp("ttp-b")];
        let scorer = Scorer::with_defaults(Profile::default());

        let ranked = scorer.rank(&campaign, &armory);
        assert_eq!(ranked.len(), 2);
        // Equal cost; ttp-a is fully novel (freshness 1.0) while ttp-b already
        // succeeded once and is idempotent (no volatile effect) → freshness 0.0.
        assert_eq!(ranked[0].ttp_id, "ttp-a");
        assert!(ranked[0].utility > ranked[1].utility);
    }

    #[test]
    fn disabling_a_consideration_drops_it_from_breakdown() {
        let (campaign, _) = campaign_with_reachable_pod();
        let armory = vec![system_ttp("ttp-a")];

        let mut profile = Profile::default();
        profile.considerations.insert(
            "cost".to_string(),
            super::super::ConsiderationConfig {
                enabled: false,
                ..Default::default()
            },
        );
        let scorer = Scorer::with_defaults(profile);

        let ranked = scorer.rank(&campaign, &armory);
        assert_eq!(ranked.len(), 1);
        assert!(ranked[0].breakdown.iter().all(|b| b.name != "cost"));
        // 7 built-in considerations minus the disabled `cost`.
        assert_eq!(ranked[0].breakdown.len(), 6);
    }

    #[test]
    fn veto_consideration_zeroes_candidate_when_curved_zero() {
        let (campaign, _) = campaign_with_reachable_pod();
        let armory = vec![system_ttp("ttp-a")];

        // Force novelty to act as a veto with a step curve that yields 0 unless
        // the measurement is >= 1.1 (impossible) → veto multiplier is 0.
        let mut profile = Profile::default();
        profile.considerations.insert(
            "novelty".to_string(),
            super::super::ConsiderationConfig {
                veto: true,
                curve: super::super::ResponseCurve::Step { threshold: 1.1 },
                ..Default::default()
            },
        );
        let scorer = Scorer::with_defaults(profile);

        let ranked = scorer.rank(&campaign, &armory);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].utility, 0.0);
    }

    // --- combination helpers ---

    #[test]
    fn product_of_empty_is_one() {
        assert_eq!(product(&[]), 1.0);
        assert_eq!(product(&[0.5, 0.5]), 0.25);
    }

    #[test]
    fn weighted_arithmetic_means_and_weights() {
        assert_eq!(weighted_arithmetic(&[]), 1.0); // neutral when no value axes
        assert!((weighted_arithmetic(&[(1.0, 0.9), (1.0, 0.1)]) - 0.5).abs() < 1e-6);
        // weight 2 on 1.0, weight 1 on 0.0 → 2/3.
        assert!((weighted_arithmetic(&[(2.0, 1.0), (1.0, 0.0)]) - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn weighted_geometric_rewards_balance_and_floors_zero() {
        // (0.9, 0.1): arithmetic 0.50, geometric sqrt(0.09)=0.30 → lower.
        let g = weighted_geometric(&[(1.0, 0.9), (1.0, 0.1)]);
        assert!((g - 0.3).abs() < 1e-3);
        assert!(g < 0.5);
        // A zero axis is floored, not annihilating: result is small but > 0.
        let with_zero = weighted_geometric(&[(1.0, 0.0), (1.0, 0.5)]);
        assert!(with_zero > 0.0 && with_zero < 0.1);
    }

    #[test]
    fn iaus_compensation_lifts_score_above_raw_product() {
        assert_eq!(iaus_combine(&[]), 0.0);
        // n=1 → compensation factor 0 → identity.
        assert!((iaus_combine(&[0.5]) - 0.5).abs() < 1e-6);
        // Two 0.5s: raw product 0.25, compensated 0.390625 (> product).
        let two = iaus_combine(&[0.5, 0.5]);
        assert!((two - 0.390625).abs() < 1e-6);
        assert!(two > 0.5 * 0.5);
    }

    #[test]
    fn geometric_mode_punishes_zero_value_axes_harder_than_arithmetic() {
        // An effectless TTP scores 0 on privilege/info/reachability.
        let (campaign, _) = campaign_with_reachable_pod();
        let armory = vec![system_ttp("ttp-a")];

        let arith = Scorer::with_defaults(Profile {
            combination: CombinationMode::WeightedArithmetic,
            ..Profile::default()
        });
        let geom = Scorer::with_defaults(Profile {
            combination: CombinationMode::WeightedGeometric,
            ..Profile::default()
        });

        let a = arith.rank(&campaign, &armory);
        let g = geom.rank(&campaign, &armory);
        assert_eq!(a.len(), 1);
        assert_eq!(g.len(), 1);
        assert!(
            g[0].utility < a[0].utility,
            "geometric ({}) should punish zero effect axes harder than arithmetic ({})",
            g[0].utility,
            a[0].utility
        );
    }
}
