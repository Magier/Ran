//! Turning a demonstration **trace** into calibration [`DecisionPoint`]s.
//!
//! A trace is the ordered [`ExecutionRecord`]s a campaign accumulated. To learn
//! from it we need, at each step, the belief state *as it was before that action
//! ran* — so we replay the trace from a fresh campaign, and at every step:
//!
//! 1. rank the applicable `(TTP × target)` candidates on the current state (via
//!    the real [`Scorer`], reusing its exact applicability + measurement path),
//! 2. locate the demonstrated action among them → one [`DecisionPoint`],
//! 3. advance the state by feeding the record back through
//!    [`Campaign::on_ttp_executed`], exactly as the live pipeline did.
//!
//! # Fidelity
//!
//! Discovery- and fact-derived effects reconstruct faithfully: they come from
//! re-parsing the record's stored `results` with the TTP's own parsers, which
//! need no extra context. The execution-history axes (epistemic freshness,
//! reliability, privilege/reachability pragmatic-freshness, cost) are therefore
//! exact. The one gap: an `ExecutionRecord` doesn't persist the physical
//! `exec_chain` or `session_connected` probe, so effects that build exec-channel
//! edges or activate sessions are not replayed — later reachability/applicability
//! can drift from the original run. Calibration exports only utility-axis
//! features; belief factors still shape candidate ranking during capture/replay
//! but are not fitted as operator preferences. Steps whose demonstrated action
//! isn't in the reconstructed applicable set are reported in
//! [`ReplayResult::unseen`] rather than silently dropped.

use armory::Ttp;
use c2::{ExecTtp, TtpExecuted};
use campaign::{Campaign, ExecutionRecord};

use crate::{utility_consideration_names, CandidateSample, DecisionPoint, Profile, Scorer};

/// Why a demonstrated step couldn't be turned into a decision point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnseenReason {
    /// The `(TTP × target)` wasn't in the applicable candidate set at that state —
    /// the reconstructed belief state didn't consider the demonstrated action
    /// runnable (often a symptom of the exec-chain/session replay gap).
    NotApplicable,
    /// The record's `ttp_id` isn't present in the supplied armory.
    TtpNotInArmory,
}

/// A demonstrated step that produced no decision point.
#[derive(Debug, Clone)]
pub struct UnseenStep {
    /// Index of the record in the input trace.
    pub index: usize,
    pub ttp_id: String,
    pub target_id: String,
    pub reason: UnseenReason,
}

/// Decision points extracted from a trace, plus what couldn't be extracted.
#[derive(Debug, Clone)]
pub struct ReplayResult {
    /// One entry per demonstrated (non-cleanup) step that was found among the
    /// applicable candidates — ready to hand to [`fit`](crate::fit).
    pub points: Vec<DecisionPoint>,
    /// Steps that couldn't be located as candidates (see [`UnseenReason`]).
    pub unseen: Vec<UnseenStep>,
    /// Utility consideration names the [`points`](Self::points) features are aligned to;
    /// pass straight to [`fit`](crate::fit).
    pub names: Vec<String>,
}

impl ReplayResult {
    /// The consideration names as `&str`, ready for [`fit`](crate::fit).
    pub fn name_refs(&self) -> Vec<&str> {
        self.names.iter().map(String::as_str).collect()
    }
}

/// Every applicable `(TTP × target)` candidate in `campaign` with its raw utility
/// feature vector, aligned to [`utility_consideration_names`]. Uses the real
/// [`Scorer`] with a neutral profile, so applicability, belief factors, and
/// measurements match production exactly while calibration remains preference-only.
pub fn candidate_samples(campaign: &Campaign, armory: &[Ttp]) -> Vec<CandidateSample> {
    let names = utility_consideration_names();
    let scorer = Scorer::with_defaults(Profile::default());
    scorer
        .rank(campaign, armory)
        .iter()
        .map(|sc| CandidateSample {
            ttp_id: sc.ttp_id.clone(),
            target_id: sc.target_id.clone(),
            features: names
                .iter()
                .map(|n| {
                    sc.breakdown
                        .iter()
                        .find(|b| b.name == *n)
                        .map(|b| b.raw)
                        .unwrap_or(0.0)
                })
                .collect(),
        })
        .collect()
}

/// Capture one [`DecisionPoint`] from the *current* belief state: the applicable
/// candidates plus the index of the chosen `(chosen_ttp_id, chosen_target_id)`.
///
/// This is the zero-reconstruction, exact-conditions capture — call it at the
/// moment an action is committed, over the pre-action state. Returns `None` if the
/// chosen action isn't in the applicable set (an operator override the scorer
/// didn't consider runnable), so the caller can count it rather than mislabel it.
pub fn decision_point(
    campaign: &Campaign,
    armory: &[Ttp],
    chosen_ttp_id: &str,
    chosen_target_id: &str,
) -> Option<DecisionPoint> {
    let candidates = candidate_samples(campaign, armory);
    let chosen = candidates
        .iter()
        .position(|c| c.ttp_id == chosen_ttp_id && c.target_id == chosen_target_id)?;
    // Stamp the consideration set the features were measured against, so a fit
    // later drops this decision if the considerations have since changed.
    let considerations = utility_consideration_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    Some(DecisionPoint {
        candidates,
        chosen,
        considerations,
    })
}

/// Replay `trace` from `initial` (a campaign bootstrapped the same way the trace
/// was) and extract decision points. `armory` must contain every TTP the trace
/// references. Cleanup records advance state but are not treated as decisions.
///
/// The initial campaign is consumed — replay mutates it into the trace's end
/// state, which the caller can inspect afterward if needed via the returned
/// campaign.
pub fn replay_trace(
    mut initial: Campaign,
    armory: &[Ttp],
    trace: &[ExecutionRecord],
) -> (ReplayResult, Campaign) {
    let names = utility_consideration_names();

    let mut points = Vec::new();
    let mut unseen = Vec::new();

    for (index, rec) in trace.iter().enumerate() {
        let cmd = reconstruct_cmd(rec, armory);

        if !rec.is_cleanup {
            // Feature extraction against the *pre-action* state.
            match decision_point(&initial, armory, &rec.ttp_id, &rec.target_id) {
                Some(dp) => points.push(dp),
                None => unseen.push(UnseenStep {
                    index,
                    ttp_id: rec.ttp_id.clone(),
                    target_id: rec.target_id.clone(),
                    reason: if cmd.is_none() {
                        UnseenReason::TtpNotInArmory
                    } else {
                        UnseenReason::NotApplicable
                    },
                }),
            }
        }

        // Advance state regardless (cleanup and unseen steps still shaped the
        // world the later decisions were made in).
        if let Some(cmd) = cmd {
            let event = reconstruct_event(rec);
            let _ = initial.on_ttp_executed(&cmd, &event);
        }
    }

    (
        ReplayResult {
            points,
            unseen,
            names: names.iter().map(|s| s.to_string()).collect(),
        },
        initial,
    )
}

/// Rebuild the command object the live pipeline would have dispatched. `None` if
/// the TTP isn't in the armory. `exec_chain`/`output_transform`/`session` are not
/// persisted on the record — see the module-level fidelity note.
fn reconstruct_cmd(rec: &ExecutionRecord, armory: &[Ttp]) -> Option<ExecTtp> {
    let ttp = armory.iter().find(|t| t.id == rec.ttp_id)?.clone();
    let procedure = ttp
        .procedures
        .iter()
        .find(|p| p.id == rec.procedure_id)
        .or_else(|| ttp.procedures.first())
        .cloned()?;
    Some(ExecTtp {
        id: rec.id.clone(),
        ttp,
        procedure,
        args: rec.args.clone(),
        target_id: rec.target_id.clone(),
        // Not persisted on the record; effects keyed on exec-channel hops won't
        // reconstruct. Semantic (target_id-keyed) effects still apply.
        exec_chain: Vec::new(),
        exec_system_id: rec.exec_system_id.clone(),
        auth_identity_id: rec.auth_identity_id.clone(),
        started_at_ms: rec.started_at_ms,
        execution_timeout_seconds: c2::DEFAULT_EXECUTION_TIMEOUT_SECONDS,
        // Stored `results` are already post-unwrap, so no transform on replay.
        output_transform: None,
        is_cleanup: rec.is_cleanup,
        reasoning: rec.reasoning.clone(),
    })
}

fn reconstruct_event(rec: &ExecutionRecord) -> TtpExecuted {
    TtpExecuted {
        id: rec.id.clone(),
        success: rec.success,
        results: rec.results.clone(),
        exit_code: rec.exit_code,
        fail_reason: rec.fail_reason.clone(),
        session_connected: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fit, FitOptions};
    use armory::{Procedure, Ttp};
    use ran_domain::K8sCluster;
    use serde_json::json;
    use std::collections::HashMap;

    fn system_ttp(id: &str, tactic: &str) -> Ttp {
        let mut requires = serde_json::Map::new();
        requires.insert("kind".to_string(), json!("System"));
        Ttp {
            status: "enabled".to_string(),
            requires,
            procedures: vec![Procedure::new("shell", "id")],
            ..Ttp::new(id, id, tactic)
        }
    }

    fn record(ttp_id: &str, target_id: &str, success: bool) -> ExecutionRecord {
        ExecutionRecord {
            id: format!("{ttp_id}-{target_id}"),
            ttp_id: ttp_id.to_string(),
            ttp_name: ttp_id.to_string(),
            tactic: "Discovery".to_string(),
            target_id: target_id.to_string(),
            exec_system_id: target_id.to_string(),
            auth_identity_id: None,
            procedure_id: "shell".to_string(),
            command: "id".to_string(),
            args: HashMap::new(),
            success,
            exit_code: 0,
            results: vec![],
            fail_reason: String::new(),
            started_at_ms: 0,
            completed_at_ms: 0,
            is_cleanup: false,
            reasoning: String::new(),
            discovered_entities: vec![],
        }
    }

    fn campaign_with_pod() -> (Campaign, String) {
        let mut c = Campaign::bootstrap("test", K8sCluster::new("test"));
        let pod = c.seed_pod_for_trigger("nginx", "default").0;
        (c, pod)
    }

    #[test]
    fn extracts_a_decision_point_per_step_with_aligned_features() {
        let (campaign, pod) = campaign_with_pod();
        let armory = vec![
            system_ttp("ttp-a", "Discovery"),
            system_ttp("ttp-b", "Discovery"),
        ];
        // Demonstrator chose ttp-a against the pod.
        let trace = vec![record("ttp-a", &pod, true)];

        let (res, _) = replay_trace(campaign, &armory, &trace);

        assert_eq!(res.points.len(), 1);
        assert!(res.unseen.is_empty());
        // Feature vector has one entry per utility consideration, in canonical order.
        let dp = &res.points[0];
        assert_eq!(
            dp.candidates[0].features.len(),
            utility_consideration_names().len()
        );
        // The chosen candidate is the demonstrated (ttp-a, pod).
        assert_eq!(dp.candidates[dp.chosen].ttp_id, "ttp-a");
        assert_eq!(dp.candidates[dp.chosen].target_id, pod);
    }

    #[test]
    fn advancing_state_makes_the_repeated_action_stale_next_step() {
        // Two steps of the same idempotent action: the second time it has already
        // succeeded, so epistemic freshness (hence the raw feature) should drop.
        let (campaign, pod) = campaign_with_pod();
        let armory = vec![system_ttp("ttp-a", "Discovery")];
        let trace = vec![record("ttp-a", &pod, true), record("ttp-a", &pod, true)];

        let (res, _) = replay_trace(campaign, &armory, &trace);
        assert_eq!(res.points.len(), 2);

        let epi = utility_consideration_names()
            .iter()
            .position(|n| *n == "epistemic_value")
            .unwrap();
        let first = res.points[0].candidates[res.points[0].chosen].features[epi];
        let second = res.points[1].candidates[res.points[1].chosen].features[epi];
        assert!(
            second < first,
            "epistemic value should decay after success: {first} -> {second}"
        );
    }

    #[test]
    fn unknown_ttp_is_reported_unseen_not_panicked() {
        let (campaign, pod) = campaign_with_pod();
        let armory = vec![system_ttp("ttp-a", "Discovery")];
        let trace = vec![record("ghost-ttp", &pod, true)];

        let (res, _) = replay_trace(campaign, &armory, &trace);
        assert!(res.points.is_empty());
        assert_eq!(res.unseen.len(), 1);
        assert_eq!(res.unseen[0].reason, UnseenReason::TtpNotInArmory);
    }

    #[test]
    fn end_to_end_fit_reproduces_the_demonstrated_choice() {
        // A pod with two applicable actions; the demonstrator always picks ttp-a.
        // Calibrating on the replayed decision points should reproduce that.
        let (campaign, pod) = campaign_with_pod();
        let armory = vec![
            system_ttp("ttp-a", "Discovery"),
            system_ttp("ttp-b", "Execution"),
        ];
        let trace = vec![record("ttp-a", &pod, true)];

        let (res, _) = replay_trace(campaign, &armory, &trace);
        let cal = fit(&res.name_refs(), &res.points, &FitOptions::default());

        assert_eq!(cal.top1_accuracy, 1.0);
        // The fitted profile round-trips into a usable Profile.
        let profile = cal.into_profile("fitted", crate::CombinationMode::WeightedArithmetic);
        assert_eq!(
            profile.considerations.len(),
            utility_consideration_names().len()
        );
    }
}
