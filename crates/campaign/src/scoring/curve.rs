use serde::{Deserialize, Serialize};

/// Maps a raw, normalized measurement in `[0, 1]` to a tuned score in `[0, 1]`.
///
/// Response curves let a consideration's *measurement* (e.g. "fraction of past
/// runs that succeeded") be defined independently of *how much we care* as that
/// measurement changes. The output is always clamped to `[0, 1]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseCurve {
    /// `slope * x + intercept`. The identity curve is `slope = 1, intercept = 0`.
    Linear { slope: f32, intercept: f32 },
    /// `slope * x^exponent + intercept`. `exponent > 1` is convex (caring more at
    /// the high end), `exponent < 1` is concave.
    Polynomial {
        exponent: f32,
        slope: f32,
        intercept: f32,
    },
    /// Logistic S-curve `1 / (1 + e^(-steepness * (x - midpoint)))`. Good for
    /// "threshold-ish" considerations where mid-range values matter most.
    Logistic { steepness: f32, midpoint: f32 },
    /// Hard step: `1.0` when `x >= threshold`, else `0.0`.
    Step { threshold: f32 },
}

impl ResponseCurve {
    /// Apply the curve to a raw measurement, clamping input and output to `[0, 1]`.
    pub fn apply(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let y = match *self {
            ResponseCurve::Linear { slope, intercept } => slope * x + intercept,
            ResponseCurve::Polynomial {
                exponent,
                slope,
                intercept,
            } => slope * x.powf(exponent) + intercept,
            ResponseCurve::Logistic {
                steepness,
                midpoint,
            } => 1.0 / (1.0 + (-steepness * (x - midpoint)).exp()),
            ResponseCurve::Step { threshold } => {
                if x >= threshold {
                    1.0
                } else {
                    0.0
                }
            }
        };
        y.clamp(0.0, 1.0)
    }
}

impl Default for ResponseCurve {
    /// The identity curve — passes the raw measurement through unchanged.
    fn default() -> Self {
        ResponseCurve::Linear {
            slope: 1.0,
            intercept: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_curve_passes_through() {
        let c = ResponseCurve::default();
        assert_eq!(c.apply(0.0), 0.0);
        assert_eq!(c.apply(0.5), 0.5);
        assert_eq!(c.apply(1.0), 1.0);
    }

    #[test]
    fn output_is_clamped() {
        let c = ResponseCurve::Linear {
            slope: 10.0,
            intercept: 0.0,
        };
        assert_eq!(c.apply(0.5), 1.0); // 5.0 clamped to 1.0
        let c = ResponseCurve::Linear {
            slope: 1.0,
            intercept: -2.0,
        };
        assert_eq!(c.apply(0.5), 0.0); // -1.5 clamped to 0.0
    }

    #[test]
    fn step_curve_thresholds() {
        let c = ResponseCurve::Step { threshold: 0.6 };
        assert_eq!(c.apply(0.59), 0.0);
        assert_eq!(c.apply(0.6), 1.0);
        assert_eq!(c.apply(1.0), 1.0);
    }

    #[test]
    fn logistic_is_monotonic_around_midpoint() {
        let c = ResponseCurve::Logistic {
            steepness: 10.0,
            midpoint: 0.5,
        };
        assert!(c.apply(0.3) < c.apply(0.5));
        assert!(c.apply(0.5) < c.apply(0.7));
        assert!((c.apply(0.5) - 0.5).abs() < 1e-6);
    }
}
