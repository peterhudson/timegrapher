//! The rate's slope against a noisy amplitude (#651): each reading's
//! amplitude noise from its own 2 s windows, and a least-squares slope
//! corrected for it, with its standard error. The isochronism curve and
//! the rate against the amplitude in [`crate::steadiness`] read their
//! slopes here.

use crate::confidence::{self, Slow};
use crate::library::signed_text;
use crate::longterm::{self, Grid};
use crate::steadiness::{chi2_tail, drop_outliers, finite_or_nan, p_text, Config};
use serde::Serialize;

/// The noise in each reading `steadiness` averages `fine`'s windows into: the variance of
/// its windows' mean, from the scatter between neighbouring windows in it
/// (½·mean of squared differences, so a slow change across the reading
/// is not counted), over how many it holds. The windows' noise is taken
/// as independent from one to the next. NaN for a reading of under three
/// windows.
pub(crate) fn reading_noise(fine: &Grid, t0: f64, step: f64, n: usize) -> Vec<f64> {
    let mut sq = vec![0.0; n];
    let mut diffs = vec![0usize; n];
    let mut count = vec![0usize; n];
    let bin = |i: usize| ((fine.time(i) - t0) / step).floor();
    for (i, &v) in fine.y.iter().enumerate() {
        let b = bin(i);
        if !(v.is_finite() && b >= 0.0 && (b as usize) < n) {
            continue;
        }
        let b = b as usize;
        count[b] += 1;
        if let Some(&u) = i.checked_sub(1).and_then(|j| fine.y.get(j)) {
            if u.is_finite() && bin(i - 1) == b as f64 {
                sq[b] += (v - u).powi(2);
                diffs[b] += 1;
            }
        }
    }
    (0..n)
        .map(|b| {
            if count[b] >= 3 && diffs[b] >= 2 {
                0.5 * sq[b] / diffs[b] as f64 / count[b] as f64
            } else {
                f64::NAN
            }
        })
        .collect()
}

/// A slope of y on x read from noisy x: the least-squares slope divided
/// by its reliability ratio λ = 1 − (mean noise variance of x) / (x's
/// variance), which undoes its shrinking towards 0 by the noise in x
/// (errors in variables, Fuller 1987), with its standard error.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EivSlope {
    pub slope: f64,
    pub se: f64,
    pub reliability: f64,
}

/// Below this reliability ratio the noise in the amplitude is too much
/// of its spread to read a slope from it (#651).
pub(crate) const MIN_RELIABILITY: f64 = 0.5;

/// Why no slope is read when the reliability is under [`MIN_RELIABILITY`],
/// in a watchmaker's words.
pub(crate) const TOO_NOISY: &str =
    "the amplitude readings' own noise is half or more of their spread, so no slope is read";

/// A slope and its ± in s/d per degree to `digits` decimals, a slope
/// that rounds to zero given no sign: the one text every surface prints
/// for the slopes against the amplitude (#734).
pub fn slope_pm_text(slope: f64, se: f64, digits: usize) -> String {
    format!(
        "{} ± {se:.digits$} s/d per degree",
        signed_text(slope, digits)
    )
}

/// [`slope_pm_text`] of an [`EivSlope`], to two decimals.
pub(crate) fn slope_text(e: &EivSlope) -> String {
    slope_pm_text(e.slope, e.se, 2)
}

/// [`EivSlope`] of `(x, y, noise variance of x)`, one per reading in time
/// order, a missing reading's x or y NaN. The standard error is Fuller's
/// (1987, §1.2) for a slope with the noise variance σuu known (here the
/// mean of the readings'), from the residuals of the corrected slope and
/// the amplitude's own variance m̂XX = mxx − σuu. Its equation-error term
/// σee/m̂XX is widened by Bartlett's factor (1 + ρX·ρe)/(1 − ρX·ρe) for
/// the readings' lag-one correlation, ρX = ρx/λ of the amplitude and
/// ρe = ρv·σvv/σee of the equation error, each ρ read by
/// [`confidence::long_run_rho`] (its line out, only across neighbouring
/// readings, bias-corrected) and each held at 0.99 at most; the noise in
/// x is taken as independent from one reading to the next. `None` with
/// under 30 pairs, no noise known, or no spread in x.
pub(crate) fn eiv_slope(p: &[(f64, f64, f64)]) -> Option<EivSlope> {
    let present = |q: &&(f64, f64, f64)| q.0.is_finite() && q.1.is_finite();
    let kept: Vec<(f64, f64, f64)> = p.iter().filter(present).copied().collect();
    let noise: Vec<f64> = kept.iter().map(|q| q.2).filter(|v| v.is_finite()).collect();
    if kept.len() < 30 || noise.len() < kept.len() / 2 {
        return None;
    }
    let n = kept.len() as f64;
    let mx = kept.iter().map(|q| q.0).sum::<f64>() / n;
    let my = kept.iter().map(|q| q.1).sum::<f64>() / n;
    let sxx: f64 = kept.iter().map(|q| (q.0 - mx).powi(2)).sum();
    let sxy: f64 = kept.iter().map(|q| (q.0 - mx) * (q.1 - my)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let b = sxy / sxx;
    let suu = noise.iter().sum::<f64>() / noise.len() as f64;
    let mxx = sxx / (n - 1.0);
    let reliability = 1.0 - suu / mxx;
    let lam = reliability.max(f64::MIN_POSITIVE);
    let slope = b / lam;
    let mxx_true = (mxx - suu).max(f64::MIN_POSITIVE);
    // x and the residuals of the corrected slope in place, NaN where a
    // reading is missing, so ρ never pairs readings a gap separates.
    let (xs, v): (Vec<f64>, Vec<f64>) = p
        .iter()
        .map(|q| {
            if present(&q) {
                (q.0, q.1 - my - slope * (q.0 - mx))
            } else {
                (f64::NAN, f64::NAN)
            }
        })
        .unzip();
    let svv = v
        .iter()
        .filter(|e| e.is_finite())
        .map(|e| e * e)
        .sum::<f64>()
        / (n - 2.0);
    // The residual is the equation error less slope × the noise in x.
    let see = (svv - slope * slope * suu).max(0.0);
    let rho_x = (confidence::long_run_rho(&xs, Slow::Line) / lam).min(0.99);
    let rho_e = if see > 0.0 {
        (confidence::long_run_rho(&v, Slow::Line) * svv / see).min(0.99)
    } else {
        0.0
    };
    let r = rho_x * rho_e;
    let bartlett = (1.0 + r) / (1.0 - r);
    let var = (see * bartlett / mxx_true
        + slope * slope * suu / mxx_true
        + (suu * svv + (slope * suu).powi(2)) / mxx_true.powi(2))
        / (n - 1.0);
    Some(EivSlope {
        slope,
        se: var.sqrt(),
        reliability,
    })
}

/// How the rate moves with the amplitude, reading by reading.
#[derive(Debug, Clone, Serialize)]
pub struct CrossCorrelation {
    /// Length of each reading, seconds, and the pairs of readings used.
    pub step_s: f64,
    pub readings: usize,
    /// Correlation of the two, reading against the same reading.
    pub r: f64,
    /// Readings that are worth as much as independent ones, allowing for
    /// each series' memory.
    pub effective_readings: f64,
    /// Chance of an r this large from unrelated series.
    pub p_value: f64,
    /// Rate change per degree of amplitude within half an hour, s/d,
    /// corrected for the noise in the amplitude readings, and its standard
    /// error; `None` when [`Self::reliability`] is under 0.5.
    pub s_per_day_per_deg: Option<f64>,
    pub s_per_day_per_deg_se: Option<f64>,
    /// The share of the amplitude readings' spread that is the amplitude
    /// and not their noise (from each reading's own 2 s windows).
    pub reliability: Option<f64>,
    /// The plain correlation and slope over the whole take at no lag,
    /// slow change included (a shared drift counts here).
    pub take_r: f64,
    pub take_s_per_day_per_deg: f64,
    pub headline: String,
}

/// Correlate the rate with the amplitude, each reading against the same
/// reading (no lag: the rate answers the amplitude at once, and a lag
/// search on a shared cycle finds the half-turn where the two
/// anticorrelate). Both grids share `t0` and step.
/// The test is on the changes within half an hour (both series with
/// their slow change taken out): two series that both drift correlate
/// whether or not one drives the other, and their drift leaves too few
/// independent readings to tell. The whole-take slope is given beside it.
pub(crate) fn cross_correlation(
    rate: &Grid,
    amp: &Grid,
    noise: &[f64],
    cfg: &Config,
) -> Option<CrossCorrelation> {
    let (mut x, mut y) = (amp.clone(), rate.clone());
    drop_outliers(&mut x, cfg.outlier_sigma);
    drop_outliers(&mut y, cfg.outlier_sigma);
    let (xd, yd) = (
        longterm::detrend(&x, longterm::DETREND_S),
        longterm::detrend(&y, longterm::DETREND_S),
    );
    let pairs = |x: &Grid, y: &Grid| -> Vec<(f64, f64)> {
        (0..y.y.len().min(x.y.len()))
            .filter_map(|i| {
                let (a, b) = (x.y[i], y.y[i]);
                (a.is_finite() && b.is_finite()).then_some((a, b))
            })
            .collect()
    };
    let stats = |p: &[(f64, f64)]| {
        let n = p.len() as f64;
        let (ma, mb) = (
            p.iter().map(|q| q.0).sum::<f64>() / n,
            p.iter().map(|q| q.1).sum::<f64>() / n,
        );
        let sab: f64 = p.iter().map(|q| (q.0 - ma) * (q.1 - mb)).sum();
        let saa: f64 = p.iter().map(|q| (q.0 - ma).powi(2)).sum();
        let sbb: f64 = p.iter().map(|q| (q.1 - mb).powi(2)).sum();
        (sab / (saa * sbb).sqrt(), sab / saa)
    };
    let p = pairs(&xd, &yd);
    if p.len() < 30 {
        return None;
    }
    let (r, _) = stats(&p);
    if !r.is_finite() {
        return None;
    }
    // The slope read off the amplitude's noise, as the isochronism's is.
    let eiv = eiv_slope(
        &(0..yd.y.len().min(xd.y.len()))
            .map(|i| (xd.y[i], yd.y[i], noise.get(i).copied().unwrap_or(f64::NAN)))
            .collect::<Vec<_>>(),
    );
    let shown = eiv.filter(|e| e.reliability >= MIN_RELIABILITY);
    let (take_r, take_slope) = stats(&pairs(&x, &y));
    let n = p.len() as f64;
    // Bartlett's factor for a correlation, each ρ as the slope's is read
    // (#651): held at 0 or above, which drops the rare case of both
    // negative.
    let rho = |g: &Grid| confidence::long_run_rho(&g.y, Slow::Line);
    let n_eff = confidence::effective_readings(n, rho(&xd) * rho(&yd)).max(4.0);
    let z = r.clamp(-0.999_999, 0.999_999).atanh() * (n_eff - 3.0).sqrt();
    let p_value = chi2_tail(z * z, 1);
    let step = rate.step;
    let slope_text = match (shown, eiv) {
        (Some(e), _) => slope_text(&e),
        (None, Some(_)) => format!("no slope ({TOO_NOISY})"),
        (None, None) => "no slope (the amplitude's noise is not known)".into(),
    };
    let headline = format!(
        "Rate against amplitude from one {:.0} s reading to the next: {}, r {}, p {} over {} pairs (worth about {:.0} independent ones); over the whole take, {} s/d per degree (r {}).",
        step,
        slope_text,
        signed_text(r, 2),
        p_text(p_value),
        p.len(),
        n_eff,
        signed_text(take_slope, 2),
        signed_text(take_r, 2)
    );
    Some(CrossCorrelation {
        step_s: step,
        readings: p.len(),
        r,
        effective_readings: n_eff,
        p_value,
        s_per_day_per_deg: shown.map(|e| e.slope),
        s_per_day_per_deg_se: shown.map(|e| e.se),
        reliability: eiv.map(|e| e.reliability),
        take_r: finite_or_nan(take_r),
        take_s_per_day_per_deg: finite_or_nan(take_slope),
        headline,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::Rng;

    /// Of `runs` seeded takes of `n` readings, amplitude (spread 5°) and
    /// the rate's residual each AR(1) at `rho`, the rate −0.5 s/d per
    /// degree, each amplitude reading off by independent noise of SD
    /// `noise`, how many the slope's ±1.96 SE holds the true slope in.
    /// `drop(i, z)` says whether reading i is missing.
    fn covered(
        rho: f64,
        noise: f64,
        n: usize,
        runs: usize,
        drop: impl Fn(usize, &mut Rng) -> bool,
    ) -> usize {
        let mut z = Rng::new(651);
        let mut hits = 0;
        for _ in 0..runs {
            let (mut a, mut e) = (z.normal(), z.normal());
            let k = (1.0 - rho * rho).sqrt();
            let p: Vec<(f64, f64, f64)> = (0..n)
                .map(|i| {
                    a = rho * a + k * z.normal();
                    e = rho * e + k * z.normal();
                    let x = 270.0 + 5.0 * a;
                    let seen = x + noise * z.normal();
                    if drop(i, &mut z) {
                        (f64::NAN, f64::NAN, f64::NAN)
                    } else {
                        (seen, -0.5 * (x - 270.0) + e, (noise * noise).max(1e-9))
                    }
                })
                .collect();
            let s = eiv_slope(&p).unwrap();
            hits += usize::from((s.slope + 0.5).abs() <= 1.96 * s.se);
        }
        hits
    }

    /// The ± holds the true slope about 95 times in 100 with real noise
    /// in the amplitude and memory in both series, gaps or none (#651,
    /// #728): over ρ 0, 0.5 and 0.9, amplitude noise of SD 0 to 3.5° on a
    /// 5° spread (λ down to about 0.67), and no gaps, blocks or half
    /// missing, 929 to 991 of 1000 (half missing at ρ 0.9 leaves fewer
    /// neighbours to carry the correlation, so it errs wide). With the
    /// least-squares SE over λ and Bartlett's factor on all of it, noise of
    /// 3.5° at ρ 0 held 869, and ρ 0.9 with no gaps 816 to 844.
    #[test]
    fn the_slopes_pm_holds_the_true_slope_in_noisy_correlated_readings_with_gaps() {
        let n = 720;
        let blocks = move |i: usize, z: &mut Rng| {
            (n / 6..n / 3).contains(&i) || (n / 2..n / 2 + n / 9).contains(&i) || z.next_f64() < 0.1
        };
        for rho in [0.0, 0.5, 0.9] {
            for noise in [0.0, 1.5, 2.5, 3.5] {
                for (gaps, hits) in [
                    ("no", covered(rho, noise, n, 1000, |_, _| false)),
                    ("block", covered(rho, noise, n, 1000, blocks)),
                    (
                        "half",
                        covered(rho, noise, n, 1000, |_, z| z.next_f64() < 0.5),
                    ),
                ] {
                    assert!(
                        (925..=995).contains(&hits),
                        "{hits} of 1000 at ρ {rho}, noise {noise}° and {gaps} gaps"
                    );
                }
            }
        }
    }

    /// A slope that rounds to zero takes no sign (#734).
    #[test]
    fn a_slope_of_about_zero_reads_without_a_sign() {
        let e = |slope| EivSlope {
            slope,
            se: 0.01,
            reliability: 0.97,
        };
        assert_eq!(slope_text(&e(-0.004)), "0.00 ± 0.01 s/d per degree");
        assert_eq!(slope_text(&e(-0.02)), "-0.02 ± 0.01 s/d per degree");
        assert_eq!(slope_text(&e(0.5)), "+0.50 ± 0.01 s/d per degree");
    }

    /// A figure in the rate-against-amplitude headline that rounds to zero
    /// reads without a sign (#734), the whole take's slope here.
    #[test]
    fn the_cross_correlations_figures_at_zero_read_without_a_sign() {
        let mut z = Rng::new(734);
        let n = 720;
        let grid = |y: Vec<f64>| Grid {
            t0: 0.0,
            step: 10.0,
            y,
        };
        // A rate that barely moves: both slopes round to 0.
        let amp: Vec<f64> = (0..n).map(|_| 270.0 + 5.0 * z.normal()).collect();
        let rate: Vec<f64> = (0..n).map(|_| 1e-4 * z.normal()).collect();
        let x =
            cross_correlation(&grid(rate), &grid(amp), &vec![0.01; n], &Config::default()).unwrap();
        assert!(!x.headline.contains("-0.00"), "{}", x.headline);
        assert!(
            x.headline
                .contains("over the whole take, 0.00 s/d per degree"),
            "{}",
            x.headline
        );
    }
}
