//! The built-in considerations.
//!
//! Four utility families plus a belief layer:
//! - *belief-state* factors — reliability and input readiness;
//! - *structural utility* signals from the TTP's shape — cost;
//! - the *epistemic* axis — an active-inference style value = information
//!   magnitude × freshness (uncertainty), driving epistemic foraging;
//! - *pragmatic* effect-derived signals via the canonical
//!   [`EffectKind`](campaign::effects::EffectKind) taxonomy — privilege gain,
//!   reachability;
//! - *operational-risk* signals — stealth, the inverse of detection risk.

use super::{Consideration, ConsiderationKind, ScoringContext};
use campaign::effects::{EffectCategory, EffectKind};

/// Whether running an action against a TTP's effects is *volatile* — i.e. any of
/// its declared effects can go stale. An action with no volatile effect is
/// **idempotent**: once it has succeeded, re-running reveals nothing new.
fn action_is_volatile(ttp: &armory::Ttp) -> bool {
    ttp.effects
        .iter()
        .filter_map(|e| EffectKind::parse(e))
        .any(|k| k.is_volatile())
}

/// **Epistemic freshness** — how much *new* knowledge running this action would
/// yield right now, in `[0, 1]`. This is the active-inference flavored notion of
/// novelty: value comes from resolving uncertainty, not from the act itself.
///
/// - Never succeeded here → `1.0` (fully novel).
/// - Succeeded and the action is **idempotent** (no volatile effect) → `0.0`,
///   forever: the facts can't go stale, so there is nothing left to learn.
/// - Succeeded but **volatile** → decays to `0.0` immediately after the run,
///   then *recovers* toward `1.0` as later actions change the world and the
///   prior reading becomes potentially stale.
///
/// Recovery is an over-approximation: it counts *any* successful action since the
/// last read, not only ones that touch this action's specific facts. Safe
/// direction (assume volatile info may be stale once the world moves); scoping
/// to overlapping effects is a precise upgrade for later.
///
/// Supplies the uncertainty half of [`EpistemicValue`] (the other half being
/// [`discovery_magnitude`]).
pub fn epistemic_freshness(
    ttp: &armory::Ttp,
    target_id: &str,
    campaign: &campaign::Campaign,
) -> f32 {
    let records = campaign.get_execution_records();

    // Index of the last *successful* non-cleanup run of this (TTP, target).
    let last_success = records.iter().rposition(|r| {
        !r.is_cleanup && r.success && r.ttp_id == ttp.id && r.target_id == target_id
    });

    let Some(idx) = last_success else {
        return 1.0; // never learned this
    };

    if !action_is_volatile(ttp) {
        return 0.0; // idempotent — knowledge can't go stale
    }

    // Volatile: recover as state-changing (successful) actions happen since.
    let changes = records[idx + 1..]
        .iter()
        .filter(|r| !r.is_cleanup && r.success)
        .count();
    1.0 - 1.0 / (1.0 + changes as f32)
}

/// **Pragmatic freshness** — `1.0` if the capability this action grants is not
/// yet held, `0.0` once it is. The pragmatic mirror of [`epistemic_freshness`]:
/// where knowledge can go stale, a *capability* (exec, escape, a session) is
/// held permanently once achieved, so this never recovers — re-running an
/// already-succeeded privilege/reachability action accomplishes nothing.
///
/// Held is proxied by "this `(TTP, target)` already succeeded" — the same
/// history signal used elsewhere. (A graph-state check on the specific
/// capability edge would also catch a *lost* session; that's the precise
/// upgrade, mirroring the epistemic over-approximation note.)
fn pragmatic_freshness(ttp: &armory::Ttp, target_id: &str, campaign: &campaign::Campaign) -> f32 {
    let achieved = campaign
        .get_execution_records()
        .iter()
        .any(|r| !r.is_cleanup && r.success && r.ttp_id == ttp.id && r.target_id == target_id);
    if achieved {
        0.0
    } else {
        1.0
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

    fn kind(&self) -> ConsiderationKind {
        ConsiderationKind::Belief
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
        let readiness = campaign::best_tool_readiness(ctx.ttp, ctx.campaign, &ctx.tc.target_id);
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

fn json_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

fn normalized_resource(resource: &str) -> String {
    resource
        .trim_matches('/')
        .split('/')
        .next()
        .unwrap_or(resource)
        .trim()
        .to_ascii_lowercase()
}

/// Detection risk inferred from structured Kubernetes API usage. Kubernetes API
/// calls are audit-visible, but common read-only enumeration can blend into
/// normal cluster noise; sensitive reads and mutating actions stand out.
fn k8s_request_detection_risk(req: &serde_json::Value) -> f32 {
    let verb = json_str(req, "verb")
        .unwrap_or("get")
        .trim()
        .to_ascii_lowercase();
    let resource = normalized_resource(json_str(req, "resource").unwrap_or(""));

    let read_only = matches!(verb.as_str(), "get" | "list" | "watch");
    if read_only {
        return match resource.as_str() {
            // Audit-visible but common inventory reads.
            "pods" | "services" | "deployments" | "configmaps" | "ingresses" | "httproutes"
            | "gateways" => 0.25,
            // Cluster/RBAC inventory is still read-only, but more security-relevant.
            "nodes"
            | "serviceaccounts"
            | "roles"
            | "rolebindings"
            | "clusterroles"
            | "clusterrolebindings" => 0.45,
            // Credential-bearing resources are high-salience even when read-only.
            "secrets" => 0.75,
            _ => 0.35,
        };
    }

    match resource.as_str() {
        "secrets" | "roles" | "rolebindings" | "clusterroles" | "clusterrolebindings" => 0.90,
        "pods" | "deployments" | "serviceaccounts" | "cronjobs" => 0.80,
        _ => 0.70,
    }
}

fn command_detection_risk(command: &str) -> f32 {
    let c = command.to_ascii_lowercase();
    if c.contains("/var/run/secrets/kubernetes.io/serviceaccount")
        || c.contains("kubectl get secrets")
    {
        0.70
    } else if c.contains("kubectl")
        && (c.contains(" create ") || c.contains(" patch ") || c.contains(" delete "))
    {
        0.75
    } else if c.contains("kubectl get pods") || c.contains("kubectl get services") {
        0.30
    } else {
        0.15
    }
}

fn procedure_detection_risk(p: &armory::Procedure) -> f32 {
    let mut risk = command_detection_risk(&p.command);
    if let Some(req) = &p.k8s_request {
        risk = risk.max(k8s_request_detection_risk(req));
    }
    if p.http_request.is_some() {
        risk = risk.max(0.45);
    }
    if p.steps.is_some() {
        risk = risk.max(0.50);
    }
    risk
}

fn effect_detection_risk(kind: EffectKind) -> f32 {
    match kind {
        EffectKind::PodList
        | EffectKind::ServiceList
        | EffectKind::DeploymentList
        | EffectKind::IngressList
        | EffectKind::HttpRouteList
        | EffectKind::GatewayList => 0.25,
        EffectKind::NodeList
        | EffectKind::ServiceAccountList
        | EffectKind::RoleList
        | EffectKind::RoleBindingList
        | EffectKind::ClusterRoleList
        | EffectKind::ClusterRoleBindingList
        | EffectKind::ConfigMapList
        | EffectKind::SelfSubjectRulesReview => 0.45,
        EffectKind::SecretList
        | EffectKind::RawServiceAccountToken
        | EffectKind::FileKubeconfig => 0.75,
        EffectKind::K8sCanExec
        | EffectKind::KubeletExecSource
        | EffectKind::C2Session
        | EffectKind::RceCanExec => 0.80,
        EffectKind::ContainerEscape | EffectKind::CreateRole | EffectKind::CreateRoleBinding => {
            0.90
        }
        EffectKind::K8sPod
        | EffectKind::K8sServiceAccount
        | EffectKind::K8sRole
        | EffectKind::K8sRoleBinding
        | EffectKind::K8sCronJob
        | EffectKind::PodName
        | EffectKind::ServiceAccountName
        | EffectKind::NamespaceName
        | EffectKind::SysFiles
        | EffectKind::SysProcesses
        | EffectKind::SysIp
        | EffectKind::SysHasFile
        | EffectKind::SysHasBinary
        | EffectKind::LinuxMounts
        | EffectKind::ReverseDns
        | EffectKind::FileContent
        | EffectKind::K8sCanReach
        | EffectKind::RunsOn => 0.20,
    }
}

fn detection_risk(ttp: &armory::Ttp) -> f32 {
    let procedure_risk = ttp
        .procedures
        .iter()
        .map(procedure_detection_risk)
        .fold(0.0f32, f32::max);
    let effect_risk = ttp
        .effects
        .iter()
        .filter_map(|e| EffectKind::parse(e))
        .map(effect_detection_risk)
        .fold(0.0f32, f32::max);
    procedure_risk.max(effect_risk)
}

/// How quietly this action is expected to blend into the environment. This is
/// the positive utility-AI form of detection risk: `1.0` means low exposure /
/// low salience, `0.0` means very noisy or security-relevant. The initial model
/// is an inferred prior from procedure shape and effect salience; explicit
/// armory annotations can override or refine it later without changing the
/// scoring surface.
pub struct Stealth;

impl Consideration for Stealth {
    fn name(&self) -> &'static str {
        "stealth"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        1.0 - detection_risk(ctx.ttp).clamp(0.0, 1.0)
    }
}

/// Saturating map from an effect "amount" to `[0, 1)`: `0 → 0.0`, `1 → 0.5`,
/// `2 → 0.75`, … Diminishing returns as the amount grows. Accepts a fractional
/// amount so generality-weighted sums (not just integer counts) work.
fn saturating(amount: f32) -> f32 {
    if amount <= 0.0 {
        0.0
    } else {
        1.0 - 0.5f32.powf(amount)
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
    saturating(count as f32)
}

/// Discovery value, weighting each discovered fact by its
/// [generality](EffectKind::generality): a foundational fact (an IP, a token)
/// counts for more than a specialized one (a capability check) because it
/// enables more downstream actions. The weighted sum is then saturated.
fn discovery_score(ttp: &armory::Ttp) -> f32 {
    let weighted: f32 = ttp
        .effects
        .iter()
        .filter_map(|e| EffectKind::parse(e))
        .filter(|k| k.categories().contains(&EffectCategory::Discovery))
        .map(|k| k.generality())
        .sum();
    saturating(weighted)
}

/// Information floor for inherently information-gathering tactics, so every
/// discovery action scores positive even when its specific effects aren't in
/// the taxonomy yet. Kept low (a floor, not a target) so it doesn't mask the
/// generality signal for actions whose effects *are* classified.
const DISCOVERY_BASELINE: f32 = 0.3;

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
/// signal for an offensive scorer. State-aware: gated by
/// [`pragmatic_freshness`], so a capability already held (the action already
/// succeeded here) scores `0` — no point re-escaping a host you're already on.
pub struct PrivilegeGain;

impl Consideration for PrivilegeGain {
    fn name(&self) -> &'static str {
        "privilege_gain"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        category_score(ctx.ttp, EffectCategory::PrivilegeEdge)
            * pragmatic_freshness(ctx.ttp, &ctx.tc.target_id, ctx.campaign)
    }
}

/// Magnitude of the knowledge an action would reveal if run now — *ignoring*
/// whether we already know it. Generality-weighted discovery effects (see
/// [`discovery_score`]) with a baseline floor for inherently information-
/// gathering tactics. This is the "how much could I learn" half of epistemic
/// value; freshness supplies the "how much of it is still unknown" half.
fn discovery_magnitude(ttp: &armory::Ttp) -> f32 {
    let effect = discovery_score(ttp);
    let baseline = if is_information_tactic(&ttp.tactic) {
        DISCOVERY_BASELINE
    } else {
        0.0
    };
    effect.max(baseline)
}

/// **Epistemic value** — expected information gain from running this action now,
/// in `[0, 1]`. Active-inference framing: value = how much the action would
/// reveal ([`discovery_magnitude`]) × how uncertain we currently are about it
/// ([`epistemic_freshness`]). A foundational fact you already hold has *zero*
/// epistemic value (high magnitude × zero freshness); a never-seen one has full
/// value; a volatile one regains value as the world drifts. This is the engine
/// of **epistemic foraging** — seek what resolves uncertainty, ignore the known.
///
/// Consolidates the former `information_gain` (magnitude) and `novelty`
/// (freshness) axes into one. Covers *epistemic* satiation; the symmetric
/// pragmatic anti-loop (already-achieved capabilities) lives in
/// [`PrivilegeGain`]/[`Reachability`] via [`pragmatic_freshness`].
pub struct EpistemicValue;

impl Consideration for EpistemicValue {
    fn name(&self) -> &'static str {
        "epistemic_value"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        discovery_magnitude(ctx.ttp) * epistemic_freshness(ctx.ttp, &ctx.tc.target_id, ctx.campaign)
    }
}

/// Value of new operating positions — effects that add a session or network
/// route to further systems ([`EffectCategory::Reachability`]). State-aware:
/// gated by [`pragmatic_freshness`], so a route already established scores `0`.
pub struct Reachability;

impl Consideration for Reachability {
    fn name(&self) -> &'static str {
        "reachability"
    }

    fn measure(&self, ctx: &ScoringContext) -> f32 {
        category_score(ctx.ttp, EffectCategory::Reachability)
            * pragmatic_freshness(ctx.ttp, &ctx.tc.target_id, ctx.campaign)
    }
}

/// The built-in consideration set: utility axes (epistemic value, cost,
/// stealth, privilege gain, reachability) plus belief-state factors
/// (reliability). The scorer keeps those layers separate:
/// final ranking score = utility × success probability.
pub fn default_considerations() -> Vec<Box<dyn Consideration>> {
    vec![
        Box::new(EpistemicValue),
        Box::new(Reliability),
        Box::new(Cost),
        Box::new(Stealth),
        Box::new(PrivilegeGain),
        Box::new(Reachability),
    ]
}

/// Names of the built-in considerations, in scoring order. Used by the tuning UI
/// to enumerate what can be configured.
pub fn consideration_names() -> Vec<&'static str> {
    default_considerations().iter().map(|c| c.name()).collect()
}

/// Names of built-in utility axes only. Used for calibration so operator
/// preference fitting does not learn weights for belief-state factors.
pub fn utility_consideration_names() -> Vec<&'static str> {
    default_considerations()
        .iter()
        .filter(|c| c.kind() == ConsiderationKind::Utility)
        .map(|c| c.name())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use armory::Ttp;
    use campaign::ttp_applicability::TargetContext;
    use campaign::Campaign;
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
            active_session: false,
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
        // No discovery effect → epistemic magnitude is zero.
        assert_eq!(discovery_magnitude(&ttp), 0.0);
    }

    #[test]
    fn epistemic_magnitude_scores_discovery_effect() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let ttp = ttp_with_effects(&["k8s.Pod"]);
        assert!(discovery_magnitude(&ttp) > 0.0);
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
    fn every_discovery_tactic_ttp_yields_epistemic_magnitude() {
        // A Discovery TTP with no taxonomy-classified effects still scores,
        // thanks to the tactic baseline.
        let discovery = Ttp::new("t", "t", "Discovery");
        assert!(discovery_magnitude(&discovery) > 0.0);
        // A non-information tactic with no discovery effect scores zero.
        let other = Ttp::new("t", "t", "Execution");
        assert_eq!(discovery_magnitude(&other), 0.0);
    }

    #[test]
    fn network_enumeration_yields_both_reachability_and_information() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        // Service / ingress enumeration explores network & adjacent entities.
        let ttp = ttp_with_effects(&["k8s.servicelist", "k8s.ingresslist"]);
        assert!(Reachability.measure(&ctx_for(&c, &ttp, &tc)) > 0.0);
        assert!(discovery_magnitude(&ttp) > 0.0);
    }

    #[test]
    fn stealth_treats_common_k8s_reads_as_quieter_than_secret_reads() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let pods = Ttp {
            effects: vec!["k8s.podList".to_string()],
            procedures: vec![armory::Procedure {
                k8s_request: Some(serde_json::json!({ "verb": "list", "resource": "pods" })),
                ..armory::Procedure::new("p", "")
            }],
            ..Ttp::new("pods", "List Pods", "Discovery")
        };
        let secrets = Ttp {
            effects: vec!["k8s.secretList".to_string()],
            procedures: vec![armory::Procedure {
                k8s_request: Some(serde_json::json!({ "verb": "list", "resource": "secrets" })),
                ..armory::Procedure::new("s", "")
            }],
            ..Ttp::new("secrets", "List Secrets", "Credential Access")
        };

        let pod_stealth = Stealth.measure(&ctx_for(&c, &pods, &tc));
        let secret_stealth = Stealth.measure(&ctx_for(&c, &secrets, &tc));
        assert!(
            pod_stealth > secret_stealth,
            "common pod read stealth ({pod_stealth}) should beat secret read ({secret_stealth})"
        );
    }

    #[test]
    fn stealth_penalizes_mutating_privilege_actions() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let read_services = Ttp {
            effects: vec!["k8s.serviceList".to_string()],
            procedures: vec![armory::Procedure {
                k8s_request: Some(serde_json::json!({ "verb": "list", "resource": "services" })),
                ..armory::Procedure::new("read", "")
            }],
            ..Ttp::new("read-services", "List Services", "Discovery")
        };
        let bind_role = Ttp {
            effects: vec!["create k8s.rolebinding".to_string()],
            procedures: vec![armory::Procedure {
                k8s_request: Some(
                    serde_json::json!({ "verb": "create", "resource": "rolebindings" }),
                ),
                ..armory::Procedure::new("bind", "")
            }],
            ..Ttp::new("bind", "Create RoleBinding", "Privilege Escalation")
        };

        assert!(
            Stealth.measure(&ctx_for(&c, &read_services, &tc))
                > Stealth.measure(&ctx_for(&c, &bind_role, &tc))
        );
    }

    #[test]
    fn unknown_effect_contributes_nothing() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let ttp = ttp_with_effects(&["totally.unknown.effect"]);
        assert_eq!(PrivilegeGain.measure(&ctx_for(&c, &ttp, &tc)), 0.0);
        assert_eq!(discovery_magnitude(&ttp), 0.0);
        assert_eq!(Reachability.measure(&ctx_for(&c, &ttp, &tc)), 0.0);
    }

    #[test]
    fn more_effects_in_category_saturate_higher() {
        assert_eq!(saturating(0.0), 0.0);
        assert_eq!(saturating(1.0), 0.5);
        assert_eq!(saturating(2.0), 0.75);
        assert!(saturating(3.0) > saturating(2.0));
    }

    #[test]
    fn epistemic_magnitude_weights_foundational_above_specialized() {
        // Both Execution tactic → no baseline floor, so the generality weight
        // is what differentiates them.
        let foundational = ttp_with_effects(&["sys.ip"]); // generality 1.0
        let specialized = ttp_with_effects(&["linux.mounts"]); // generality 0.3
        let f = discovery_magnitude(&foundational);
        let s = discovery_magnitude(&specialized);
        assert!(
            f > s,
            "foundational ({f}) should outscore specialized ({s})"
        );
        assert!(s > 0.0);
    }

    #[test]
    fn reliability_prefers_present_tool_over_unknown() {
        use campaign::ttp_applicability::resolve_target_context;
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

    // --- epistemic freshness (novelty) ---

    fn record(ttp_id: &str, target_id: &str, success: bool) -> campaign::ExecutionRecord {
        campaign::ExecutionRecord {
            id: format!("{ttp_id}-rec"),
            ttp_id: ttp_id.to_string(),
            ttp_name: ttp_id.to_string(),
            tactic: "Discovery".to_string(),
            target_id: target_id.to_string(),
            exec_system_id: target_id.to_string(),
            auth_identity_id: None,
            procedure_id: "p".to_string(),
            command: "x".to_string(),
            args: std::collections::HashMap::new(),
            success,
            exit_code: 0,
            results: vec![],
            fail_reason: String::new(),
            started_at_ms: 0,
            completed_at_ms: 0,
            is_cleanup: false,
            reasoning: String::new(),
        }
    }

    #[test]
    fn freshness_full_when_never_run() {
        let c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let ttp = ttp_with_effects(&["sys.ip"]);
        assert_eq!(epistemic_freshness(&ttp, &tc.target_id, &c), 1.0);
    }

    #[test]
    fn idempotent_action_collapses_to_zero_after_success() {
        let mut c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        // sys.ip is stable → idempotent.
        let ttp = Ttp {
            effects: vec!["sys.ip".to_string()],
            ..Ttp::new("get-ip", "Get IP", "Discovery")
        };
        c.execution_records
            .push(record("get-ip", &tc.target_id, true));
        // Even after other actions happen, an idempotent fact stays at 0.
        c.execution_records
            .push(record("other", &tc.target_id, true));
        assert_eq!(epistemic_freshness(&ttp, &tc.target_id, &c), 0.0);
    }

    #[test]
    fn failed_run_does_not_reduce_freshness() {
        let mut c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let ttp = Ttp {
            effects: vec!["sys.ip".to_string()],
            ..Ttp::new("get-ip", "Get IP", "Discovery")
        };
        c.execution_records
            .push(record("get-ip", &tc.target_id, false)); // failed
        assert_eq!(epistemic_freshness(&ttp, &tc.target_id, &c), 1.0);
    }

    #[test]
    fn volatile_action_recovers_as_world_changes() {
        let mut c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        // k8s.podList is volatile → freshness recovers after later actions.
        let ttp = Ttp {
            effects: vec!["k8s.podList".to_string()],
            ..Ttp::new("list-pods", "List Pods", "Discovery")
        };
        c.execution_records
            .push(record("list-pods", &tc.target_id, true));
        // Immediately after, nothing has changed → 0.
        assert_eq!(epistemic_freshness(&ttp, &tc.target_id, &c), 0.0);
        // After two state-changing actions, freshness recovers above 0.
        c.execution_records.push(record("a", &tc.target_id, true));
        c.execution_records.push(record("b", &tc.target_id, true));
        let recovered = epistemic_freshness(&ttp, &tc.target_id, &c);
        assert!(
            recovered > 0.0,
            "volatile freshness should recover, got {recovered}"
        );
    }

    #[test]
    fn epistemic_value_is_magnitude_times_freshness() {
        // An idempotent discovery (get-IP) has full epistemic value the first
        // time and zero once learned — magnitude high, freshness collapses.
        let mut c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let ttp = Ttp {
            effects: vec!["sys.ip".to_string()],
            ..Ttp::new("get-ip", "Get IP", "Discovery")
        };

        let before = EpistemicValue.measure(&ctx_for(&c, &ttp, &tc));
        assert!(before > 0.0);
        // Equals magnitude × freshness(=1.0) before any run.
        assert!((before - discovery_magnitude(&ttp)).abs() < 1e-6);

        c.execution_records
            .push(record("get-ip", &tc.target_id, true));
        let after = EpistemicValue.measure(&ctx_for(&c, &ttp, &tc));
        assert_eq!(after, 0.0, "known idempotent fact has no epistemic value");
    }

    #[test]
    fn privilege_gain_drops_to_zero_once_capability_held() {
        let mut c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let escape = Ttp {
            effects: vec!["container.escape(sys)".to_string()],
            ..Ttp::new("escape", "Escape to Host", "Privilege Escalation")
        };
        // Before escaping: full privilege value.
        let before = PrivilegeGain.measure(&ctx_for(&c, &escape, &tc));
        assert!(before > 0.0);
        // After a successful escape, re-escaping the same target is worthless.
        c.execution_records
            .push(record("escape", &tc.target_id, true));
        assert_eq!(PrivilegeGain.measure(&ctx_for(&c, &escape, &tc)), 0.0);
    }

    #[test]
    fn reachability_drops_to_zero_once_route_established() {
        let mut c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let session = Ttp {
            effects: vec!["c2.session(sliver, sys)".to_string()],
            ..Ttp::new("sess", "Open Session", "Lateral Movement")
        };
        assert!(Reachability.measure(&ctx_for(&c, &session, &tc)) > 0.0);
        c.execution_records
            .push(record("sess", &tc.target_id, true));
        assert_eq!(Reachability.measure(&ctx_for(&c, &session, &tc)), 0.0);
    }

    #[test]
    fn failed_capability_attempt_keeps_pragmatic_value() {
        let mut c = Campaign::bootstrap("t", K8sCluster::new("t"));
        let tc = tc();
        let escape = Ttp {
            effects: vec!["container.escape(sys)".to_string()],
            ..Ttp::new("escape", "Escape to Host", "Privilege Escalation")
        };
        // A failed attempt does not count as "held" → still worth retrying.
        c.execution_records
            .push(record("escape", &tc.target_id, false));
        assert!(PrivilegeGain.measure(&ctx_for(&c, &escape, &tc)) > 0.0);
    }
}
