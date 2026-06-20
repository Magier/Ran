//! The built-in considerations.
//!
//! Two families: *structural* signals derived from the TTP's own shape and the
//! campaign's execution history (novelty, reliability, cost), and *effect-derived*
//! signals that value a TTP's declared effects via the canonical
//! [`EffectKind`](crate::effects::EffectKind) taxonomy (privilege gain,
//! information gain, reachability).

use super::{Consideration, ScoringContext};
use crate::effects::{EffectCategory, EffectKind};

/// Prefer actions not already run against this target. Penalizes re-running the
/// same `(TTP, target)` pair so the scorer doesn't loop on one move.
///
/// `1 / (1 + n)` where `n` is the number of prior non-cleanup runs: never run →
/// `1.0`, once → `0.5`, twice → `0.33`, …
pub struct Novelty;

impl Consideration for Novelty {
    fn name(&self) -> &'static str {
        "novelty"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        let n = ctx
            .campaign
            .get_execution_records()
            .iter()
            .filter(|r| !r.is_cleanup && r.ttp_id == ctx.ttp.id && r.target_id == ctx.tc.target_id)
            .count();
        1.0 / (1.0 + n as f32)
    }
}

/// Confidence the action will actually work. Blends a status-derived prior with
/// the observed success rate of this TTP across the campaign (small pseudo-count
/// so a single run doesn't swing it), then multiplies by **tool readiness** —
/// success needs both a working technique and a present tool. A confirmed-present
/// tool leaves the estimate unchanged; an unknown tool discounts it; a TTP whose
/// every tool is known-absent is filtered out upstream by the applicability gate.
pub struct Reliability;

impl Reliability {
    /// Strength of the status prior, in pseudo-observations.
    const PRIOR_STRENGTH: f32 = 2.0;

    fn status_prior(status: &str) -> f32 {
        match status.to_ascii_lowercase().as_str() {
            "stable" => 0.9,
            "enabled" => 0.7,
            // disabled TTPs are filtered out before scoring; treat anything
            // unrecognized as middling.
            _ => 0.5,
        }
    }
}

impl Consideration for Reliability {
    fn name(&self) -> &'static str {
        "reliability"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        let prior = Self::status_prior(&ctx.ttp.status);

        let (total, successes) = ctx
            .campaign
            .get_execution_records()
            .iter()
            .filter(|r| !r.is_cleanup && r.ttp_id == ctx.ttp.id)
            .fold((0u32, 0u32), |(t, s), r| (t + 1, s + u32::from(r.success)));

        // Bayesian-ish blend: prior contributes PRIOR_STRENGTH pseudo-runs.
        let k = Self::PRIOR_STRENGTH;
        let history = (successes as f32 + prior * k) / (total as f32 + k);

        // Multiply by tool readiness (confirmed-present 1.0, unknown < 1.0):
        // P(success) ≈ P(technique works) · P(tool present).
        let readiness = crate::campaign::execution::best_tool_readiness(
            ctx.ttp,
            ctx.campaign,
            &ctx.tc.target_id,
        );
        history * readiness
    }
}

/// Prefer cheaper actions. Scores the *cheapest available procedure variant* of
/// the TTP — a single local command beats a multi-step payload chain.
pub struct Cost;

impl Cost {
    fn procedure_cost(p: &armory::Procedure) -> f32 {
        let mut c = 1.0;
        if p.steps.is_some() {
            c += 2.0;
        }
        if p.http_request.is_some() {
            c += 1.0;
        }
        if p.k8s_request.is_some() {
            c += 1.0;
        }
        // Each chained shell command adds a little cost.
        c += p.command.matches("&&").count() as f32;
        c
    }
}

impl Consideration for Cost {
    fn name(&self) -> &'static str {
        "cost"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        let min_cost = ctx
            .ttp
            .procedures
            .iter()
            .map(Self::procedure_cost)
            .fold(f32::INFINITY, f32::min);

        if min_cost.is_finite() {
            // cost >= 1 → score in (0, 1]; cheaper is higher.
            1.0 / min_cost
        } else {
            // No procedures (shouldn't happen for a real TTP) — neutral.
            0.5
        }
    }
}

/// Saturating score for a count of category-matching effects: `0 → 0.0`,
/// `1 → 0.5`, `2 → 0.75`, `3 → 0.875`, … More effects in the category score
/// higher but with diminishing returns, keeping the result in `[0, 1)`.
fn saturating(count: usize) -> f32 {
    if count == 0 {
        0.0
    } else {
        1.0 - 0.5f32.powi(count as i32)
    }
}

/// Score a TTP by how many of its effects fall in `want`, via the canonical
/// [`EffectKind`] taxonomy. Effects outside the taxonomy contribute nothing
/// (fail-soft) — adding coverage is a single edit in `effects::EffectKind`.
fn category_score(ttp: &armory::Ttp, want: EffectCategory) -> f32 {
    let count = ttp
        .effects
        .iter()
        .filter_map(|e| EffectKind::parse(e))
        .filter(|k| k.categories().contains(&want))
        .count();
    saturating(count)
}

/// Information floor for inherently information-gathering tactics, so every
/// discovery action scores positive even when its specific effects aren't in
/// the taxonomy yet.
const DISCOVERY_BASELINE: f32 = 0.5;

/// Whether a tactic is inherently about gathering information.
fn is_information_tactic(tactic: &str) -> bool {
    let t: String = tactic
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(t.as_str(), "discovery" | "reconnaissance")
}

/// Value of the privilege the action would gain — effects that add execution or
/// escape capability ([`EffectCategory::PrivilegeEdge`]). Usually the dominant
/// signal for an offensive scorer.
pub struct PrivilegeGain;

impl Consideration for PrivilegeGain {
    fn name(&self) -> &'static str {
        "privilege_gain"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        category_score(ctx.ttp, EffectCategory::PrivilegeEdge)
    }
}

/// Value of the information the action would reveal — effects that add entities
/// or facts ([`EffectCategory::Discovery`]). First-class in this POMDP setting:
/// reducing uncertainty has direct utility. Discovery/Reconnaissance TTPs get a
/// baseline floor so every discovery action scores, even if its effects aren't
/// in the taxonomy yet.
pub struct InformationGain;

impl Consideration for InformationGain {
    fn name(&self) -> &'static str {
        "information_gain"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        let effect = category_score(ctx.ttp, EffectCategory::Discovery);
        let baseline = if is_information_tactic(&ctx.ttp.tactic) {
            DISCOVERY_BASELINE
        } else {
            0.0
        };
        effect.max(baseline)
    }
}

/// Value of new operating positions — effects that add a session or network
/// route to further systems ([`EffectCategory::Reachability`]).
pub struct Reachability;

impl Consideration for Reachability {
    fn name(&self) -> &'static str {
        "reachability"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        category_score(ctx.ttp, EffectCategory::Reachability)
    }
}

/// The built-in consideration set: structural signals (novelty, reliability,
/// cost) plus effect-derived signals (privilege/information gain, reachability).
pub fn default_considerations() -> Vec<Box<dyn Consideration>> {
    vec![
        Box::new(Novelty),
        Box::new(Reliability),
        Box::new(Cost),
        Box::new(PrivilegeGain),
        Box::new(InformationGain),
        Box::new(Reachability),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ttp_applicability::TargetContext;
    use crate::Campaign;
    use armory::Ttp;
    use ran_domain::{AccessLevel, K8sCluster};

    fn ctx_for<'a>(
        campaign: &'a Campaign,
        ttp: &'a Ttp,
        tc: &'a TargetContext,
    ) -> ScoringContext<'a> {
        ScoringContext { campaign, ttp, tc }
    }

    fn tc() -> TargetContext {
        TargetContext {
            target_id: "ns/default/pod/x".to_string(),
            target_kind: "Pod".to_string(),
            is_system: true,
            access_level: AccessLevel::Exec,
            has_token: false,
        }
    }

    // Uses a neutral (non-information) tactic so these tests exercise the
    // *effect-based* score, not the Discovery/Reconnaissance baseline.
    fn ttp_with_effects(effects: &[&str]) -> Ttp {
        Ttp {
            effects: effects.iter().map(|s| s.to_string()).collect(),
            ..Ttp::new("t", "t", "Execution")
        }
    }

    #[test]
    fn privilege_gain_scores_escape_effect() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let ttp = ttp_with_effects(&["container.escape(sys)"]);
        assert!(PrivilegeGain.measure(&ctx_for(&c, &ttp, &tc)) > 0.0);
        // No discovery effect → information gain is zero.
        assert_eq!(InformationGain.measure(&ctx_for(&c, &ttp, &tc)), 0.0);
    }

    #[test]
    fn information_gain_scores_discovery_effect() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let ttp = ttp_with_effects(&["k8s.Pod"]);
        assert!(InformationGain.measure(&ctx_for(&c, &ttp, &tc)) > 0.0);
        assert_eq!(PrivilegeGain.measure(&ctx_for(&c, &ttp, &tc)), 0.0);
    }

    #[test]
    fn reachability_scores_session_effect() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let ttp = ttp_with_effects(&["c2.session(sliver, sys)"]);
        assert!(Reachability.measure(&ctx_for(&c, &ttp, &tc)) > 0.0);
    }

    #[test]
    fn every_discovery_tactic_ttp_yields_information_gain() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        // A Discovery TTP with no taxonomy-classified effects still scores,
        // thanks to the tactic baseline.
        let discovery = Ttp::new("t", "t", "Discovery");
        assert!(InformationGain.measure(&ctx_for(&c, &discovery, &tc)) > 0.0);
        // A non-information tactic with no discovery effect scores zero.
        let other = Ttp::new("t", "t", "Execution");
        assert_eq!(InformationGain.measure(&ctx_for(&c, &other, &tc)), 0.0);
    }

    #[test]
    fn network_enumeration_yields_both_reachability_and_information() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        // Service / ingress enumeration explores network & adjacent entities.
        let ttp = ttp_with_effects(&["k8s.servicelist", "k8s.ingresslist"]);
        assert!(Reachability.measure(&ctx_for(&c, &ttp, &tc)) > 0.0);
        assert!(InformationGain.measure(&ctx_for(&c, &ttp, &tc)) > 0.0);
    }

    #[test]
    fn unknown_effect_contributes_nothing() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let ttp = ttp_with_effects(&["totally.unknown.effect"]);
        assert_eq!(PrivilegeGain.measure(&ctx_for(&c, &ttp, &tc)), 0.0);
        assert_eq!(InformationGain.measure(&ctx_for(&c, &ttp, &tc)), 0.0);
        assert_eq!(Reachability.measure(&ctx_for(&c, &ttp, &tc)), 0.0);
    }

    #[test]
    fn more_effects_in_category_saturate_higher() {
        assert_eq!(saturating(0), 0.0);
        assert_eq!(saturating(1), 0.5);
        assert_eq!(saturating(2), 0.75);
        assert!(saturating(3) > saturating(2));
    }

    #[test]
    fn reliability_prefers_present_tool_over_unknown() {
        use crate::ttp_applicability::resolve_target_context;
        use ran_domain::{BinaryPresence, Entity as _, Pod};

        fn tool_ttp(tool: &str) -> Ttp {
            Ttp {
                status: "enabled".to_string(),
                procedures: vec![armory::Procedure {
                    tool: Some(tool.to_string()),
                    ..armory::Procedure::new("p", format!("{tool} x"))
                }],
                ..Ttp::new("t", "t", "Discovery")
            }
        }

        let mut c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let mut pod = Pod::new("nginx", "default");
        pod.system.binaries.insert(
            "nmap".to_string(),
            BinaryPresence::Present("/usr/bin/nmap".to_string()),
        );
        let id = pod.entity_id().0;
        c.entities.insert_typed(pod);
        let target = resolve_target_context(&c, &id).unwrap();

        let present = tool_ttp("nmap"); // confirmed present on the pod
        let unknown = tool_ttp("curl"); // not in the binary map → unknown

        let r_present = Reliability.measure(&ScoringContext {
            campaign: &c,
            ttp: &present,
            tc: &target,
        });
        let r_unknown = Reliability.measure(&ScoringContext {
            campaign: &c,
            ttp: &unknown,
            tc: &target,
        });
        assert!(
            r_present > r_unknown,
            "present-tool reliability ({r_present}) should beat unknown ({r_unknown})"
        );
    }
}
