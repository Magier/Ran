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

mod consideration;
pub mod considerations;
mod curve;
mod profile;
mod scorer;

pub use consideration::{Consideration, ScoringContext};
pub use considerations::consideration_names;
pub use curve::ResponseCurve;
pub use profile::{CombinationMode, ConsiderationConfig, Profile};
pub use scorer::{ConsiderationScore, ScoredCandidate, Scorer};
