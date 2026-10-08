//! Rate and beat error from beat times.
//!
//! Beat `k` is modelled as `t = t0 + k*P + s*e/2`, where `P` is the beat
//! period, `s` is +1 for even beats and -1 for odd ones, and `e` is the
//! beat error. Fitting all three at once gives the rate from tick and toc
//! together while cancelling beat error, as Witschi recommends.

use crate::beats::Beat;
use crate::dsp::robust_sd;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TimingFit {
    /// Daily rate in seconds per day; positive means the watch gains.
    pub rate_s_per_day: f64,
    /// Signed beat error in milliseconds (even beats late is positive).
    pub beat_error_ms: f64,
    /// Fitted beat period, seconds.
    pub period_s: f64,
    /// Robust standard deviation of the residuals, microseconds.
    pub jitter_us: f64,
    pub beats_used: usize,
}

fn side(index: i64) -> f64 {
    if index.rem_euclid(2) == 0 {
        0.5
    } else {
        -0.5
    }
}

/// Least-squares fit of `t = a + b*k + c*s` over the selected beats.
fn solve(beats: &[&Beat]) -> Option<[f64; 3]> {
    if beats.len() < 6 {
        return None;
    }
    // Centre k and t for numerical stability.
    let k0 = beats[0].index as f64;
    let t0 = beats[0].time;
    let mut ata = [[0.0f64; 3]; 3];
    let mut atb = [0.0f64; 3];
    for b in beats {
        let row = [1.0, b.index as f64 - k0, side(b.index)];
        let y = b.time - t0;
        for i in 0..3 {
            atb[i] += row[i] * y;
            for j in 0..3 {
                ata[i][j] += row[i] * row[j];
            }
        }
    }
    let x = solve3(ata, atb)?;
    Some([x[0] + t0 - x[1] * k0, x[1], x[2]])
}

#[allow(clippy::needless_range_loop)]
fn solve3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for c in 0..3 {
        let p = (c..3).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-300 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        for r in 0..3 {
            if r != c {
                let f = a[r][c] / a[c][c];
                for k in 0..3 {
                    a[r][k] -= f * a[c][k];
                }
                b[r] -= f * b[c];
            }
        }
    }
    Some([b[0] / a[0][0], b[1] / a[1][1], b[2] / a[2][2]])
}

/// Robust fit: beats with low template correlation are left out, then
/// beats more than 5 robust SDs from the fit, and the fit is repeated.
fn fit_coef(beats: &[Beat]) -> Option<([f64; 3], Vec<&Beat>)> {
    let mut sel: Vec<&Beat> = beats.iter().filter(|b| b.quality > 0.4).collect();
    let mut coef = solve(&sel)?;
    for _ in 0..2 {
        let res: Vec<f64> = sel.iter().map(|b| residual(b, &coef)).collect();
        let sd = robust_sd(&res).max(1e-6);
        sel = sel
            .iter()
            .zip(&res)
            .filter(|(_, r)| r.abs() < 5.0 * sd)
            .map(|(b, _)| *b)
            .collect();
        coef = solve(&sel)?;
    }
    Some((coef, sel))
}

/// Fit rate and beat error to a run of beats.
pub fn fit(beats: &[Beat], bph: u32) -> Option<TimingFit> {
    let nominal = 3600.0 / bph as f64;
    let (coef, sel) = fit_coef(beats)?;
    let res: Vec<f64> = sel.iter().map(|b| residual(b, &coef)).collect();
    Some(TimingFit {
        rate_s_per_day: (nominal / coef[1] - 1.0) * 86400.0,
        beat_error_ms: coef[2] * 1000.0,
        period_s: coef[1],
        jitter_us: robust_sd(&res) * 1e6,
        beats_used: sel.len(),
    })
}

/// Signed beat error alone, ms, for runs too short to need the rate.
pub fn beat_error_ms(beats: &[Beat]) -> Option<f64> {
    fit_coef(beats).map(|(c, _)| c[2] * 1000.0)
}

fn residual(b: &Beat, c: &[f64; 3]) -> f64 {
    b.time - (c[0] + c[1] * b.index as f64 + c[2] * side(b.index))
}

/// Residual of every beat against one fit for the whole recording:
/// how far each beat is from where a constant rate would put it, seconds.
pub fn residuals(beats: &[Beat]) -> Option<Vec<f64>> {
    let (coef, _) = fit_coef(beats)?;
    Some(beats.iter().map(|b| residual(b, &coef)).collect())
}

/// Rate, beat error and jitter in consecutive windows of `window_s`.
#[derive(Debug, Clone, Serialize)]
pub struct WindowFit {
    pub start_s: f64,
    pub end_s: f64,
    pub fit: TimingFit,
}

pub fn windows(beats: &[Beat], bph: u32, window_s: f64, step_s: f64) -> Vec<WindowFit> {
    let mut out = Vec::new();
    let Some(last) = beats.last() else { return out };
    let mut start = beats[0].time;
    let mut lo = 0usize;
    while start + window_s <= last.time + 1e-9 {
        while lo < beats.len() && beats[lo].time < start {
            lo += 1;
        }
        let mut hi = lo;
        while hi < beats.len() && beats[hi].time < start + window_s {
            hi += 1;
        }
        if let Some(fit) = fit(&beats[lo..hi], bph) {
            out.push(WindowFit {
                start_s: start,
                end_s: start + window_s,
                fit,
            });
        }
        start += step_s;
    }
    out
}
