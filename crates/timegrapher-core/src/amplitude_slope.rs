//! The rate's slope against a noisy amplitude (#651): each reading's
//! amplitude noise from its own 2 s windows, and a least-squares slope
//! corrected for it, with its standard error. The isochronism curve and
//! the rate against the amplitude in [`crate::steadiness`] read their
//! slopes here.

use crate::confidence;
use crate::longterm::{self, Grid};
use crate::steadiness::{acf, chi2_tail, drop_outliers, finite_or_nan, p_text, Config};
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

/// [`EivSlope`] of `(x, y, noise variance of x)` in time order. The
/// standard error allows for x's and the residuals' lag-one correlation
/// ([`confidence::effective_readings`] of their product) and for λ, but
/// not for λ's own uncertainty. `None` with under 30 pairs, no noise
/// known, or no spread in x.
pub(crate) fn eiv_slope(p: &[(f64, f64, f64)]) -> Option<EivSlope> {
    let noise: Vec<f64> = p.iter().map(|q| q.2).filter(|v| v.is_finite()).collect();
    if p.len() < 30 || noise.len() < p.len() / 2 {
        return None;
    }
    let n = p.len() as f64;
    let mx = p.iter().map(|q| q.0).sum::<f64>() / n;
    let my = p.iter().map(|q| q.1).sum::<f64>() / n;
    let sxx: f64 = p.iter().map(|q| (q.0 - mx).powi(2)).sum();
    let sxy: f64 = p.iter().map(|q| (q.0 - mx) * (q.1 - my)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let b = sxy / sxx;
    let reliability = 1.0 - noise.iter().sum::<f64>() / noise.len() as f64 / (sxx / (n - 1.0));
    let res: Vec<f64> = p.iter().map(|q| q.1 - my - b * (q.0 - mx)).collect();
    let xs: Vec<f64> = p.iter().map(|q| q.0).collect();
    let rho = confidence::lag_one(&xs).unwrap_or(0.0) * confidence::lag_one(&res).unwrap_or(0.0);
    let n_eff = confidence::effective_readings(n, rho.max(0.0)).max(3.0);
    let se = (res.iter().map(|e| e * e).sum::<f64>() / (n - 2.0) / sxx * n / n_eff).sqrt();
    let lam = reliability.max(f64::MIN_POSITIVE);
    Some(EivSlope {
        slope: b / lam,
        se: se / lam,
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
            .filter(|&i| xd.y[i].is_finite() && yd.y[i].is_finite())
            .map(|i| (xd.y[i], yd.y[i], noise.get(i).copied().unwrap_or(f64::NAN)))
            .collect::<Vec<_>>(),
    );
    let shown = eiv.filter(|e| e.reliability >= MIN_RELIABILITY);
    let (take_r, take_slope) = stats(&pairs(&x, &y));
    let n = p.len() as f64;
    let rho = |g: &Grid| {
        acf(&g.y, 1)
            .get(1)
            .copied()
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
    };
    let n_eff = crate::confidence::effective_readings(n, rho(&xd) * rho(&yd)).max(4.0);
    let z = r.clamp(-0.999_999, 0.999_999).atanh() * (n_eff - 3.0).sqrt();
    let p_value = chi2_tail(z * z, 1);
    let step = rate.step;
    let slope_text = match (shown, eiv) {
        (Some(e), _) => format!("{:+.2} ± {:.2} s/d per degree", e.slope, e.se),
        (None, Some(e)) => format!(
            "no slope (the amplitude's noise is too much of its spread, reliability {:.2})",
            e.reliability
        ),
        (None, None) => "no slope (the amplitude's noise is not known)".into(),
    };
    let headline = format!(
        "Rate against amplitude from one {:.0} s reading to the next: {}, r {:.2}, p {} over {} pairs (worth about {:.0} independent ones); over the whole take, {:+.2} s/d per degree (r {:.2}).",
        step,
        slope_text,
        r,
        p_text(p_value),
        p.len(),
        n_eff,
        take_slope,
        take_r
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
