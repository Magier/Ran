//! Calibrating a scoring [`Profile`] from demonstrations.
//!
//! Given a set of demonstrated decisions — at each step, the applicable
//! `(TTP × target)` candidates and which one the demonstrator actually chose —
//! we fit per-consideration **weights** so the scorer reproduces those choices
//! with high probability.
//!
//! # Model
//!
//! Each candidate carries a raw measurement vector `x ∈ [0,1]^K` (one entry per
//! consideration). Fixed [`ResponseCurve`]s map it to a curved vector `c`
//! (identity by default), exactly as the [`Scorer`](crate::Scorer) would. The
//! utility is linear in the weights, `s = w · c`, and the demonstrator is
//! modeled as a soft-max chooser over the applicable set:
//!
//! ```text
//! P(choose i) = exp(w·c_i) / Σ_j exp(w·c_j)
//! ```
//!
//! Fitting maximizes the log-likelihood of the demonstrated choices plus an L2
//! penalty — a convex problem (conditional-logit / MaxEnt IRL), solved by
//! projected gradient descent with a backtracking line search. The weight
//! magnitude `‖w‖` doubles as the soft-max temperature: a confident fit sharpens
//! the distribution toward "always pick the demonstrated action", bounded by the
//! L2 term so it doesn't overfit.
//!
//! # What a fit can't do
//!
//! With non-negative weights (the default, so the result is a usable [`Profile`])
//! a candidate that is **Pareto-dominated** — no better than some rival on any
//! axis and strictly worse on one — can never be ranked first. Those decisions
//! are surfaced in [`Calibration::infeasible`]; they mean the demonstrator valued
//! something the current considerations don't measure, i.e. a *missing axis*.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{CombinationMode, ConsiderationConfig, Profile, ResponseCurve};

/// One candidate's raw per-consideration measurements at a decision point, in the
/// consideration order passed to [`fit`]. Values are in `[0, 1]` (clamped).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSample {
    pub ttp_id: String,
    pub target_id: String,
    /// Raw measurements, one per consideration (same order/length as `fit`'s `names`).
    pub features: Vec<f32>,
}

impl CandidateSample {
    pub fn new(
        ttp_id: impl Into<String>,
        target_id: impl Into<String>,
        features: Vec<f32>,
    ) -> Self {
        Self {
            ttp_id: ttp_id.into(),
            target_id: target_id.into(),
            features,
        }
    }
}

/// A single demonstrated decision: the applicable candidate set and the index of
/// the one the demonstrator chose. Concatenate the decision points of several
/// traces to calibrate against all of them at once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionPoint {
    pub candidates: Vec<CandidateSample>,
    /// Index into `candidates` of the demonstrated choice.
    pub chosen: usize,
    /// Consideration names, in feature order, this decision's features were
    /// measured against. Lets a persisted log survive a change to the
    /// consideration set: [`fit`] drops any decision whose schema differs from
    /// the current considerations (even at the same width — a swapped axis is
    /// still incomparable). Empty means unknown (a legacy entry written before
    /// this field existed); those are matched on feature width alone.
    #[serde(default)]
    pub considerations: Vec<String>,
}

/// Knobs for [`fit`]. `Default` is a sensible starting point.
#[derive(Debug, Clone)]
pub struct FitOptions {
    /// L2 penalty on the weights. Larger → smoother, more conservative (lower
    /// confidence); smaller → sharper (higher confidence), risks overfitting.
    pub l2: f32,
    /// Constrain weights to `>= 0` so the result is a directly-usable [`Profile`]
    /// (a consideration can only *help*, matching weighted-mean semantics). When
    /// `false`, weights may go negative — more expressive, but a negative weight
    /// has no clean meaning in the scorer's weighted mean.
    pub nonneg: bool,
    /// Response curve per consideration (len must equal `names`, or empty for
    /// all-identity). Applied to raw features before fitting *and* carried into
    /// the materialized profile so train and serve match.
    pub curves: Vec<ResponseCurve>,
    pub max_iters: usize,
    /// Convergence tolerance on the (projected) gradient norm.
    pub tol: f32,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            l2: 1e-3,
            nonneg: true,
            curves: Vec::new(),
            max_iters: 1000,
            tol: 1e-5,
        }
    }
}

/// Per-decision diagnostics from a fit.
#[derive(Debug, Clone)]
pub struct DecisionFit {
    /// Probability the fitted model assigns to the demonstrated choice.
    pub chosen_prob: f32,
    /// Rank of the demonstrated choice under the fitted weights (1 = top).
    pub chosen_rank: usize,
    /// Number of candidates at this decision point.
    pub candidates: usize,
    /// The chosen candidate is Pareto-dominated → unrealizable with non-negative
    /// weights regardless of the fit.
    pub dominated: bool,
}

/// Result of a calibration fit.
#[derive(Debug, Clone)]
pub struct Calibration {
    /// Consideration names, aligned with [`weights`](Self::weights).
    pub names: Vec<String>,
    /// Fitted weight per consideration.
    pub weights: Vec<f32>,
    /// Curves used during the fit (carried into [`into_profile`](Self::into_profile)).
    pub curves: Vec<ResponseCurve>,
    /// Total log-likelihood of the demonstrated choices under the fitted model.
    pub log_likelihood: f32,
    /// Mean probability assigned to the demonstrated choices.
    pub mean_chosen_prob: f32,
    /// Worst (minimum) probability assigned to any demonstrated choice.
    pub min_chosen_prob: f32,
    /// Fraction of decisions where the demonstrated choice ranks first.
    pub top1_accuracy: f32,
    /// Per-decision diagnostics, in input order.
    pub per_decision: Vec<DecisionFit>,
    /// Indices of decisions whose chosen candidate is Pareto-dominated.
    pub infeasible: Vec<usize>,
    pub iters: usize,
    pub converged: bool,
}

impl Calibration {
    /// Materialize the fitted weights into a [`Profile`] (weights + the curves
    /// used during fitting). Weights are rescaled to average 1.0 for readability;
    /// this is ranking-neutral because the weighted mean normalizes by `Σw`.
    /// Considerations that were held at zero weight stay enabled at 0 — set them
    /// disabled upstream if you'd rather drop them.
    pub fn into_profile(&self, name: impl Into<String>, combination: CombinationMode) -> Profile {
        let positive: Vec<f32> = self.weights.iter().map(|w| w.max(0.0)).collect();
        let sum: f32 = positive.iter().sum();
        let scale = if sum > 0.0 {
            positive.len() as f32 / sum
        } else {
            1.0
        };

        let mut considerations = HashMap::new();
        for (i, cname) in self.names.iter().enumerate() {
            let curve = self.curves.get(i).cloned().unwrap_or_default();
            considerations.insert(
                cname.clone(),
                ConsiderationConfig {
                    weight: positive[i] * scale,
                    curve,
                    enabled: true,
                    veto: false,
                },
            );
        }

        Profile {
            name: name.into(),
            combination,
            considerations,
        }
    }
}

/// Fit consideration weights from demonstrated decisions.
///
/// `names` are the considerations in feature order; every [`CandidateSample`]'s
/// `features` must have that length. Returns the fitted weights plus diagnostics.
/// Decision points with fewer than two candidates carry no ranking signal and are
/// skipped (a choice among one option is free).
pub fn fit(names: &[&str], points: &[DecisionPoint], opts: &FitOptions) -> Calibration {
    let k = names.len();
    let curves: Vec<ResponseCurve> = if opts.curves.is_empty() {
        vec![ResponseCurve::default(); k]
    } else {
        assert_eq!(
            opts.curves.len(),
            k,
            "curves length must match considerations"
        );
        opts.curves.clone()
    };

    // Only well-formed decisions participate: a real, in-range choice, and every
    // candidate's feature vector matching the consideration count `k`. Decisions
    // captured under a *different* consideration set — a stale log after the
    // considerations changed — have the wrong feature width; their entries describe
    // different axes and can't be compared here, so they're dropped rather than
    // indexed out of bounds. `per_decision` therefore covers only the used subset.
    let valid: Vec<&DecisionPoint> = points
        .iter()
        .filter(|d| {
            !d.candidates.is_empty()
                && d.chosen < d.candidates.len()
                && d.candidates.iter().all(|c| c.features.len() == k)
                // Schema must match the current considerations. A recorded schema
                // that differs (a changed *or swapped* axis) is incomparable and
                // dropped; an empty schema (legacy entry) falls back to width.
                && (d.considerations.is_empty() || schema_eq(&d.considerations, names))
        })
        .collect();

    // Pre-curve every valid candidate's features once (model input is the curve).
    let curved: Vec<Vec<Vec<f32>>> = valid
        .iter()
        .map(|d| {
            d.candidates
                .iter()
                .map(|c| {
                    c.features
                        .iter()
                        .enumerate()
                        .map(|(j, &x)| curves[j].apply(x))
                        .collect()
                })
                .collect()
        })
        .collect();
    // Index of the chosen candidate per valid decision, aligned with `curved`.
    let chosen: Vec<usize> = valid.iter().map(|d| d.chosen).collect();

    // Only decisions with a real choice (>= 2 candidates) contribute to the fit.
    let active: Vec<usize> = (0..curved.len())
        .filter(|&d| curved[d].len() >= 2)
        .collect();

    let mut w = vec![0.0f32; k];
    let mut iters = 0;
    let mut converged = false;

    if !active.is_empty() {
        let mut lr = 1.0f32;
        let mut cur_nll = nll(&w, &curved, &active, &chosen, opts.l2);

        for it in 0..opts.max_iters {
            iters = it + 1;
            let grad = gradient(&w, &curved, &active, &chosen, opts.l2);

            // Projected-gradient convergence: for a clamped (active) coordinate a
            // positive gradient can't move it further, so ignore that component.
            let gnorm = projected_grad_norm(&w, &grad, opts.nonneg);
            if gnorm < opts.tol {
                converged = true;
                break;
            }

            // Backtracking line search on the step size.
            let mut stepped = false;
            for _ in 0..30 {
                let mut cand = w.clone();
                for j in 0..k {
                    cand[j] -= lr * grad[j];
                    if opts.nonneg && cand[j] < 0.0 {
                        cand[j] = 0.0;
                    }
                }
                let cand_nll = nll(&cand, &curved, &active, &chosen, opts.l2);
                if cand_nll <= cur_nll {
                    // Negligible relative improvement → we're effectively at the
                    // (unique, thanks to L2) minimum, even if the raw gradient is
                    // still non-trivial on near-separable data.
                    if cur_nll - cand_nll < opts.tol * (1.0 + cur_nll.abs()) {
                        converged = true;
                    }
                    w = cand;
                    cur_nll = cand_nll;
                    lr *= 1.5; // grow while it keeps working
                    stepped = true;
                    break;
                }
                lr *= 0.5; // overshot — shrink and retry
            }
            if converged || !stepped {
                converged = true; // reached a minimum (flat improvement or stalled search)
                break;
            }
        }
    }

    let mut report = build_report(names, &curves, &w, &curved, &chosen);
    report.iters = iters;
    report.converged = converged;
    report
}

/// Negative log-likelihood + L2 penalty at `w` over the active decisions.
/// `chosen[d]` is the winning candidate index for decision `d` (guaranteed in
/// range by [`fit`]'s validity filter).
fn nll(w: &[f32], curved: &[Vec<Vec<f32>>], active: &[usize], chosen: &[usize], l2: f32) -> f32 {
    let mut total = 0.0f32;
    for &d in active {
        let cands = &curved[d];
        let scores: Vec<f32> = cands.iter().map(|c| dot(w, c)).collect();
        let logz = log_sum_exp(&scores);
        total += logz - scores[chosen[d]];
    }
    let reg: f32 = l2 * w.iter().map(|v| v * v).sum::<f32>();
    total + reg
}

/// Gradient of [`nll`] w.r.t. `w`.
fn gradient(
    w: &[f32],
    curved: &[Vec<Vec<f32>>],
    active: &[usize],
    chosen: &[usize],
    l2: f32,
) -> Vec<f32> {
    let k = w.len();
    let mut g = vec![0.0f32; k];
    for &d in active {
        let cands = &curved[d];
        let scores: Vec<f32> = cands.iter().map(|c| dot(w, c)).collect();
        let probs = softmax(&scores);
        // Expected feature vector under the model minus the chosen feature vector.
        let chosen_feat = &cands[chosen[d]];
        for j in 0..k {
            let expected: f32 = cands.iter().zip(&probs).map(|(c, p)| p * c[j]).sum();
            g[j] += expected - chosen_feat[j];
        }
    }
    for j in 0..k {
        g[j] += 2.0 * l2 * w[j];
    }
    g
}

/// Norm of the gradient after zeroing components that the non-negativity
/// projection would clamp (coordinate at 0 with a positive gradient).
fn projected_grad_norm(w: &[f32], grad: &[f32], nonneg: bool) -> f32 {
    let mut sum = 0.0f32;
    for j in 0..w.len() {
        let g = if nonneg && w[j] <= 0.0 && grad[j] > 0.0 {
            0.0
        } else {
            grad[j]
        };
        sum += g * g;
    }
    sum.sqrt()
}

fn build_report(
    names: &[&str],
    curves: &[ResponseCurve],
    w: &[f32],
    curved: &[Vec<Vec<f32>>],
    chosen: &[usize],
) -> Calibration {
    let mut per_decision = Vec::with_capacity(curved.len());
    let mut infeasible = Vec::new();
    let mut ll = 0.0f32;
    let mut prob_sum = 0.0f32;
    let mut prob_min = 1.0f32;
    let mut prob_count = 0usize;
    let mut top1 = 0usize;
    let mut ranked_count = 0usize;

    for (d, cands) in curved.iter().enumerate() {
        let chosen = chosen[d];
        let dominated = is_dominated(cands, chosen);
        if dominated {
            infeasible.push(d);
        }

        if cands.len() < 2 {
            per_decision.push(DecisionFit {
                chosen_prob: 1.0,
                chosen_rank: 1,
                candidates: cands.len(),
                dominated,
            });
            continue;
        }

        let scores: Vec<f32> = cands.iter().map(|c| dot(w, c)).collect();
        let probs = softmax(&scores);
        let p = probs[chosen];
        // Rank: 1 + number of candidates scoring strictly higher.
        let rank = 1 + scores
            .iter()
            .filter(|&&s| s > scores[chosen] + f32::EPSILON)
            .count();

        ll += p.max(1e-12).ln();
        prob_sum += p;
        prob_min = prob_min.min(p);
        prob_count += 1;
        ranked_count += 1;
        if rank == 1 {
            top1 += 1;
        }

        per_decision.push(DecisionFit {
            chosen_prob: p,
            chosen_rank: rank,
            candidates: cands.len(),
            dominated,
        });
    }

    Calibration {
        names: names.iter().map(|s| s.to_string()).collect(),
        weights: w.to_vec(),
        curves: curves.to_vec(),
        log_likelihood: ll,
        mean_chosen_prob: if prob_count > 0 {
            prob_sum / prob_count as f32
        } else {
            1.0
        },
        min_chosen_prob: if prob_count > 0 { prob_min } else { 1.0 },
        top1_accuracy: if ranked_count > 0 {
            top1 as f32 / ranked_count as f32
        } else {
            1.0
        },
        per_decision,
        infeasible,
        iters: 0, // filled in by caller
        converged: false,
    }
}

/// Is candidate `chosen` Pareto-dominated by some other candidate — no greater on
/// any axis and strictly less on at least one? Such a choice cannot be ranked
/// first by any non-negative weighting.
fn is_dominated(cands: &[Vec<f32>], chosen: usize) -> bool {
    let c = &cands[chosen];
    cands.iter().enumerate().any(|(i, other)| {
        if i == chosen {
            return false;
        }
        let mut ge_all = true;
        let mut gt_any = false;
        for j in 0..c.len() {
            if other[j] < c[j] - f32::EPSILON {
                ge_all = false;
                break;
            }
            if other[j] > c[j] + f32::EPSILON {
                gt_any = true;
            }
        }
        ge_all && gt_any
    })
}

/// Whether a decision's recorded consideration schema matches the current one
/// (same names, same order).
fn schema_eq(recorded: &[String], names: &[&str]) -> bool {
    recorded.len() == names.len() && recorded.iter().zip(names).all(|(a, b)| a == b)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn log_sum_exp(xs: &[f32]) -> f32 {
    let m = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !m.is_finite() {
        return m;
    }
    m + xs.iter().map(|x| (x - m).exp()).sum::<f32>().ln()
}

fn softmax(xs: &[f32]) -> Vec<f32> {
    let m = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = xs.iter().map(|x| (x - m).exp()).collect();
    let sum: f32 = exps.iter().sum();
    exps.iter().map(|e| e / sum).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dp(candidates: Vec<CandidateSample>, chosen: usize) -> DecisionPoint {
        // No recorded schema → matched on feature width (legacy-entry behavior),
        // which keeps these width-based tests focused on the fit itself.
        DecisionPoint {
            candidates,
            chosen,
            considerations: Vec::new(),
        }
    }

    fn cand(features: Vec<f32>) -> CandidateSample {
        CandidateSample::new("ttp", "tgt", features)
    }

    fn run(names: &[&str], points: &[DecisionPoint], opts: &FitOptions) -> Calibration {
        fit(names, points, opts)
    }

    #[test]
    fn learns_to_prefer_high_first_feature() {
        // Two axes; the demonstrator always picks the candidate with the higher
        // first feature even when its second feature is lower.
        let points = vec![
            dp(vec![cand(vec![0.9, 0.1]), cand(vec![0.2, 0.8])], 0),
            dp(vec![cand(vec![0.8, 0.3]), cand(vec![0.3, 0.9])], 0),
            dp(vec![cand(vec![0.1, 0.7]), cand(vec![0.95, 0.2])], 1),
        ];
        let cal = run(&["a", "b"], &points, &FitOptions::default());

        assert!(
            cal.weights[0] > cal.weights[1],
            "should weight the decisive axis higher: {:?}",
            cal.weights
        );
        assert_eq!(cal.top1_accuracy, 1.0);
        assert!(cal.mean_chosen_prob > 0.8, "prob {}", cal.mean_chosen_prob);
        assert!(cal.infeasible.is_empty());
    }

    #[test]
    fn flags_pareto_dominated_choice_as_infeasible() {
        // Chosen candidate is worse on axis 0 and equal on axis 1 → dominated.
        let points = vec![dp(vec![cand(vec![0.2, 0.5]), cand(vec![0.9, 0.5])], 0)];
        let cal = run(&["a", "b"], &points, &FitOptions::default());

        assert_eq!(cal.infeasible, vec![0]);
        assert!(cal.per_decision[0].dominated);
        // A dominated choice can at best *tie* the dominator (by zeroing the
        // distinguishing axis), so the model can never give it more than half the
        // probability — it cannot be reproduced with confidence.
        assert!(
            cal.per_decision[0].chosen_prob <= 0.5 + 1e-3,
            "prob {}",
            cal.per_decision[0].chosen_prob
        );
    }

    #[test]
    fn reproduces_choices_from_a_known_weighting() {
        // Generate choices as argmax of a known weight vector, then check the fit
        // reproduces every ranking with high confidence. (The exact learned weight
        // *magnitudes* aren't identifiable from few separable points — reproducing
        // the choices is the goal, not recovering the generating weights.)
        let true_w = [2.0f32, 0.5, 1.0];
        let candidate_sets = [
            (vec![
                vec![0.8, 0.1, 0.3],
                vec![0.2, 0.9, 0.4],
                vec![0.5, 0.5, 0.9],
            ]),
            (vec![
                vec![0.1, 0.8, 0.2],
                vec![0.7, 0.2, 0.1],
                vec![0.3, 0.3, 0.8],
            ]),
            (vec![
                vec![0.9, 0.0, 0.0],
                vec![0.4, 0.6, 0.5],
                vec![0.2, 0.2, 0.95],
            ]),
            (vec![
                vec![0.6, 0.6, 0.6],
                vec![0.3, 0.9, 0.1],
                vec![0.85, 0.1, 0.2],
            ]),
        ];
        let points: Vec<DecisionPoint> = candidate_sets
            .iter()
            .map(|set| {
                let scores: Vec<f32> = set
                    .iter()
                    .map(|f| f.iter().zip(true_w).map(|(x, w)| x * w).sum())
                    .collect();
                let chosen = scores
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                    .unwrap()
                    .0;
                dp(set.iter().map(|f| cand(f.clone())).collect(), chosen)
            })
            .collect();

        let opts = FitOptions {
            l2: 1e-4,
            ..Default::default()
        };
        let cal = run(&["x", "y", "z"], &points, &opts);

        assert_eq!(cal.top1_accuracy, 1.0, "weights {:?}", cal.weights);
        assert!(cal.mean_chosen_prob > 0.7, "prob {}", cal.mean_chosen_prob);
        assert!(cal.converged);
    }

    #[test]
    fn into_profile_sets_weights_and_normalizes() {
        let points = vec![
            dp(vec![cand(vec![0.9, 0.1]), cand(vec![0.2, 0.8])], 0),
            dp(vec![cand(vec![0.85, 0.2]), cand(vec![0.25, 0.9])], 0),
        ];
        let cal = run(&["novelty", "cost"], &points, &FitOptions::default());
        let profile = cal.into_profile("fitted", CombinationMode::WeightedArithmetic);

        assert_eq!(profile.name, "fitted");
        assert!(profile.considerations.contains_key("novelty"));
        assert!(profile.considerations.contains_key("cost"));
        // Rescaled to average 1.0 across the two considerations.
        let sum: f32 = profile.considerations.values().map(|c| c.weight).sum();
        assert!((sum - 2.0).abs() < 1e-3, "sum {sum}");
    }

    #[test]
    fn mixed_feature_widths_drop_stale_entries_without_panicking() {
        // A decision log spanning a consideration-set change: some decisions carry
        // 6-wide feature vectors, others 5-wide. Fitting against a 6-name set must
        // drop the 5-wide (stale-schema) decisions rather than index out of bounds.
        let names6 = ["a", "b", "c", "d", "e", "f"];
        let dp6 = dp(
            vec![
                cand(vec![0.9, 0.1, 0.2, 0.3, 0.4, 0.5]),
                cand(vec![0.1, 0.8, 0.2, 0.1, 0.0, 0.3]),
            ],
            0,
        );
        let dp5 = dp(
            vec![
                cand(vec![0.9, 0.1, 0.2, 0.3, 0.4]),
                cand(vec![0.1, 0.8, 0.2, 0.1, 0.0]),
            ],
            1,
        );
        let cal = fit(&names6, &[dp6, dp5], &FitOptions::default());

        // Only the 6-wide decision participates; the 5-wide one is dropped.
        assert_eq!(cal.per_decision.len(), 1);
        assert_eq!(cal.weights.len(), 6);

        // All-stale input yields an empty (but panic-free) report.
        let only_stale = fit(
            &names6,
            std::slice::from_ref(&dp5_clone()),
            &FitOptions::default(),
        );
        assert!(only_stale.per_decision.is_empty());
    }

    #[test]
    fn same_width_but_different_consideration_set_is_dropped() {
        // A decision captured under a *different* 2-consideration set than the
        // one we're fitting for. Same width, so a width-only guard would wrongly
        // fit it; the schema check must drop it (the axes aren't comparable).
        let mut stale = dp(vec![cand(vec![0.9, 0.1]), cand(vec![0.1, 0.9])], 0);
        stale.considerations = vec!["stealth".into(), "cost".into()];

        let mut current = dp(vec![cand(vec![0.9, 0.1]), cand(vec![0.1, 0.9])], 0);
        current.considerations = vec!["novelty".into(), "cost".into()];

        let cal = fit(
            &["novelty", "cost"],
            &[stale, current],
            &FitOptions::default(),
        );
        // Only the schema-matching decision is used.
        assert_eq!(cal.per_decision.len(), 1);
    }

    fn dp5_clone() -> DecisionPoint {
        dp(
            vec![
                cand(vec![0.9, 0.1, 0.2, 0.3, 0.4]),
                cand(vec![0.1, 0.8, 0.2, 0.1, 0.0]),
            ],
            1,
        )
    }

    #[test]
    fn single_candidate_decisions_are_skipped_not_errored() {
        let points = vec![
            dp(vec![cand(vec![0.5, 0.5])], 0), // no choice — skipped
            dp(vec![cand(vec![0.9, 0.1]), cand(vec![0.1, 0.9])], 0),
        ];
        let cal = run(&["a", "b"], &points, &FitOptions::default());
        assert_eq!(cal.per_decision.len(), 2);
        assert_eq!(cal.per_decision[0].chosen_prob, 1.0);
    }
}
