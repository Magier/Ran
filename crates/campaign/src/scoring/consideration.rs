use crate::ttp_applicability::TargetContext;
use crate::Campaign;

/// Everything a [`Consideration`] needs to score one grounded candidate: the
/// current belief state, the candidate TTP, and the resolved facts about its
/// target. Cheap to construct (all borrows) — one per `(TTP × target)` pair.
pub struct ScoringContext<'a> {
    pub campaign: &'a Campaign,
    pub ttp: &'a armory::Ttp,
    pub tc: &'a TargetContext,
}

/// A single, independent scoring axis. Each consideration produces a raw,
/// curve-free measurement normalized to `[0, 1]`; the [`Scorer`](super::Scorer)
/// applies the profile's response curve and weight on top.
///
/// Keep `measure` pure and side-effect free — it is called once per candidate
/// and must be deterministic (no clocks, no RNG) so rankings are reproducible.
pub trait Consideration: Send + Sync {
    /// Stable identifier used to look up this consideration's weight/curve in a
    /// [`Profile`](super::Profile). Must be unique across registered considerations.
    fn name(&self) -> &'static str;

    /// Raw measurement in `[0, 1]`. Values outside the range are clamped by the
    /// scorer, but returning in-range keeps curves predictable.
    fn measure(&self, ctx: &ScoringContext) -> f32;
}
