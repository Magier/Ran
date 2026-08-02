//! Utility-AI action selection.
//!
//! Given the current campaign belief state and the armory, the [`Scorer`] ranks
//! applicable grounded `(TTP × target)` candidates by utility. Each candidate is
//! scored by a set of independent [`Consideration`]s; a [`Profile`] supplies the
//! per-consideration weights and response curves that encode an operator
//! preference (stealthy, aggressive, recon-heavy, …).
//!
//! Phase 1 scope: the engine plus structural considerations (novelty,
//! reliability, cost). Effect-derived considerations (privilege/information
//! gain, reachability) land in Phase 2 alongside the canonical effect taxonomy.

pub mod calibration;
mod consideration;
pub mod considerations;
mod curve;
mod profile;
pub mod replay;
mod scorer;

pub use calibration::{fit, Calibration, CandidateSample, DecisionFit, DecisionPoint, FitOptions};
pub use consideration::{Consideration, ConsiderationKind, ScoringContext};
pub use considerations::{consideration_names, utility_consideration_names};
pub use curve::ResponseCurve;
pub use profile::{CombinationMode, ConsiderationConfig, Profile};
pub use replay::{
    candidate_samples, decision_point, replay_trace, ReplayResult, UnseenReason, UnseenStep,
};
pub use scorer::{ConsiderationScore, ScoredCandidate, Scorer};
