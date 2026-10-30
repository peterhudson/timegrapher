//! The CUSUM of a series and how far independent readings take it: the
//! running sum of each reading's distance from the mean, in units of
//! σ√n, which for independent readings wanders inside ±[`CUSUM_BAND`]
//! 95% of the time. Every surface draws the band and marks the corner
//! where the level changes from here.

use serde::Serialize;

/// The CUSUM's 95% band, σ√n: the Kolmogorov distribution's 95th
/// percentile ([`kolmogorov_tail`] of it is 0.05).
pub const CUSUM_BAND: f64 = 1.36;

#[derive(Debug, Clone, Serialize)]
pub struct Cusum {
    /// Time, seconds, and the CUSUM there in units of σ√n.
    pub t_s: Vec<f64>,
    pub s: Vec<f64>,
    /// Largest |s| and where it is, with s there (signed).
    pub max: f64,
    pub at_s: f64,
    pub at: f64,
    /// Chance of a max this large from readings with no change of level
    /// (Kolmogorov), the CUSUM being scaled by their long-run σ.
    pub p_value: f64,
    /// Readings with no change of level stay inside ±`band` 95% of the time
    /// ([`CUSUM_BAND`]); `shift` when `max` is past it, so the level
    /// changes near `at_s`.
    pub band: f64,
    pub shift: bool,
}

impl Cusum {
    /// The CUSUM `s` at times `t_s` (seconds, one per reading), kept at
    /// every `stride`th point for drawing; its largest |s| is read from
    /// every point.
    pub fn of(t_s: &[f64], s: &[f64], stride: usize) -> Cusum {
        let (k, max) = s
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v.abs()))
            .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
        Cusum {
            max,
            at_s: t_s[k],
            at: s[k],
            p_value: kolmogorov_tail(max),
            band: CUSUM_BAND,
            shift: max > CUSUM_BAND,
            t_s: t_s.iter().step_by(stride).copied().collect(),
            s: s.iter().step_by(stride).copied().collect(),
        }
    }
}

/// P(sup |Brownian bridge| > x).
pub fn kolmogorov_tail(x: f64) -> f64 {
    if x < 0.2 {
        return 1.0;
    }
    let mut s = 0.0;
    for j in 1..100 {
        let t = (-2.0 * (j * j) as f64 * x * x).exp();
        s += if j % 2 == 1 { t } else { -t };
        if t < 1e-17 {
            break;
        }
    }
    (2.0 * s).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The band is where the chance of independent readings reaching it
    /// is 5%, as the panes and the view say.
    #[test]
    fn the_band_is_the_kolmogorov_95th_percentile() {
        assert!((kolmogorov_tail(CUSUM_BAND) - 0.05).abs() < 0.001);
    }

    /// The corner is marked at the largest |s| with its sign, from every
    /// point though only every other one is kept to draw; a CUSUM inside
    /// the band marks none.
    #[test]
    fn the_corner_is_marked_past_the_band() {
        let t: Vec<f64> = (0..5).map(|i| i as f64 * 10.0).collect();
        let c = Cusum::of(&t, &[0.2, -0.9, -1.6, -0.7, 0.0], 2);
        assert!(c.shift);
        assert_eq!((c.at_s, c.at, c.max), (20.0, -1.6, 1.6));
        assert_eq!(c.t_s, [0.0, 20.0, 40.0]);
        assert!(!Cusum::of(&t, &[0.2, 0.9, 1.3, 0.7, 0.0], 1).shift);
    }
}
