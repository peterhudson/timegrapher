//! The CUSUM of a series and how far readings with no change of level
//! take it: the running sum of each reading's distance from the mean, in
//! units of the long-run σ√n, which with no change of level, correlated
//! or not, wanders inside ±[`CUSUM_BAND`] 95% of the time. Every surface
//! draws the band, and marks the corner from here only at a level change
//! the headline names: one rule names a level change (#697).

use serde::Serialize;

/// The CUSUM's 95% band, σ√n: the Kolmogorov distribution's 95th
/// percentile ([`kolmogorov_tail`] of it is 0.05).
pub const CUSUM_BAND: f64 = 1.36;

#[derive(Debug, Clone, Serialize)]
pub struct Cusum {
    /// Time, seconds, and the CUSUM there in units of σ√n.
    pub t_s: Vec<f64>,
    pub s: Vec<f64>,
    /// Largest |s|; where the level changes, or with no change where
    /// |s| is largest, with s there (signed).
    pub max: f64,
    pub at_s: f64,
    pub at: f64,
    /// Chance of a max this large from readings with no change of level
    /// (Kolmogorov), the CUSUM being scaled by their long-run σ.
    pub p_value: f64,
    /// Readings with no change of level stay inside ±`band` 95% of the time
    /// ([`CUSUM_BAND`]); a guide to the eye, not the test. `shift` when
    /// the headline names a level change, so the level changes near `at_s`.
    pub band: f64,
    pub shift: bool,
}

impl Cusum {
    /// The CUSUM `s` at times `t_s` (seconds, one per reading), kept at
    /// every `stride`th point for drawing; its largest |s| is read from
    /// every point. `cuts` are the times a level change the headline
    /// names starts a new level, from the change finder; the corner is the cut nearest the largest |s|.
    pub fn of(t_s: &[f64], s: &[f64], stride: usize, cuts: &[f64]) -> Cusum {
        let (mut k, max) = s
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v.abs()))
            .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
        let corner = cuts
            .iter()
            .copied()
            .min_by(|a, b| (a - t_s[k]).abs().total_cmp(&(b - t_s[k]).abs()));
        if let Some(c) = corner {
            k = t_s.partition_point(|&t| t < c).min(t_s.len() - 1);
        }
        Cusum {
            max,
            at_s: corner.unwrap_or(t_s[k]),
            at: s[k],
            p_value: kolmogorov_tail(max),
            band: CUSUM_BAND,
            shift: corner.is_some(),
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

    /// The corner is marked at the change finder's cut nearest the largest
    /// |s|, with s there, read from every point though only every other
    /// one is kept to draw.
    #[test]
    fn the_corner_is_marked_at_the_nearest_cut() {
        let t: Vec<f64> = (0..5).map(|i| i as f64 * 10.0).collect();
        let s = [0.2, -0.9, -1.6, -0.7, 0.0];
        let c = Cusum::of(&t, &s, 2, &[10.0, 40.0]);
        assert!(c.shift);
        assert_eq!((c.at_s, c.at, c.max), (10.0, -0.9, 1.6));
        assert_eq!(c.t_s, [0.0, 20.0, 40.0]);
        assert_eq!(Cusum::of(&t, &s, 1, &[20.0]).at, -1.6);
    }

    /// Past the band with no cut, no change is named (#697): the band is
    /// a guide, and the headline's rule alone names a level change.
    #[test]
    fn past_the_band_without_a_cut_names_no_change() {
        let t: Vec<f64> = (0..5).map(|i| i as f64 * 10.0).collect();
        let c = Cusum::of(&t, &[0.2, -0.9, -1.6, -0.7, 0.0], 1, &[]);
        assert!(c.max > CUSUM_BAND);
        assert!(!c.shift);
        assert_eq!((c.at_s, c.at), (20.0, -1.6));
    }

    /// The tail at points across its range, against the series summed to
    /// convergence (2 Σ (−1)^(j−1) e^(−2j²x²)); under 0.2 it is 1.
    #[test]
    fn the_kolmogorov_tail_is_the_series_value() {
        for (x, p) in [
            (0.3, 0.999_990_694_198_665_5),
            (0.4, 0.997_192_326_777_298_2),
            (0.5, 0.963_945_243_664_875_1),
            (0.8, 0.544_142_411_574_198_1),
            (1.0, 0.269_999_671_677_354_56),
            (2.0, 0.000_670_925_255_779_695_3),
            (3.0, 3.045_995_948_942_526e-8),
        ] {
            let got = kolmogorov_tail(x);
            assert!((got / p - 1.0).abs() < 1e-9, "{x}: {got} against {p}");
        }
        // From 0.2 up the series is read, which there is just under 1.
        assert!(kolmogorov_tail(0.2) < 1.0);
        assert_eq!(kolmogorov_tail(0.1), 1.0);
        assert_eq!(kolmogorov_tail(0.0), 1.0);
        assert_eq!(kolmogorov_tail(-1.0), 1.0);
    }

    /// The largest |s| is the first of two equal ones; its chance is the
    /// tail of it; the cut nearest it wins whichever side it is on; and a
    /// cut after the last reading marks the last reading.
    #[test]
    fn the_largest_and_the_nearest_cut_are_read_exactly() {
        let t: Vec<f64> = (0..5).map(|i| i as f64 * 10.0).collect();
        let c = Cusum::of(&t, &[0.1, 1.5, 0.3, -1.5, 0.2], 1, &[]);
        assert_eq!((c.at_s, c.at, c.max), (10.0, 1.5, 1.5));
        assert_eq!(c.p_value, kolmogorov_tail(1.5));
        let s = [0.0, 0.4, 1.2, 0.5, 0.1];
        assert_eq!(Cusum::of(&t, &s, 1, &[5.0, 30.0]).at_s, 30.0);
        assert_eq!(Cusum::of(&t, &s, 1, &[5.0, 30.0]).at, 0.5);
        let late = Cusum::of(&t, &s, 1, &[45.0]);
        assert_eq!((late.at_s, late.at), (45.0, 0.1));
    }
}
