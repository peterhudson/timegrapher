//! Time-frequency maps of a series: how big each period's line is at each
//! time of the take, so a line that comes and goes, or drifts, shows where
//! it does. Two maps of the same series, on one log period axis:
//!
//! - the **STFT**: the amplitude spectrum of each 20 min stretch, stepped
//!   5 min, the plain view; every period is read over the same window;
//! - the **synchrosqueezed wavelet transform** (SST): a Morlet wavelet
//!   transform, whose window grows with the period (short periods read
//!   over short times, long ones over long), with each value moved to the
//!   period its own phase turns at, which sharpens a line to near its
//!   true period.
//!
//! Both read only unbroken stretches: an STFT window holds no missing
//! reading, and the wavelet transform runs on each stretch alone, with
//! the times too near a stretch's ends for a period's wavelet (its cone
//! of influence) left empty. Nothing is filled in across a gap.
//!
//! Sizes are as in [`crate::spectrum`]: the sine-wave size at each period
//! (half its peak to peak) for a steady line, in the series' units. Under
//! noise the SST spreads a line over neighbouring rows, so one row reads
//! under the line's size: the STFT gives the size, the SST where and when.
//!
//! The **wavelet coherence** of two series read at the same times (the
//! rate and the amplitude, Tick's amplitude and Tock's) uses the same
//! Morlet wavelet: how tightly the two move together at each period and
//! time, from 0 to 1, with the phase between them, beside the level that
//! pairs of unrelated red-noise series stay under 95% of the time.

use crate::longterm::Grid;
use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner};
use serde::Serialize;

/// One map, thinned for drawing.
#[derive(Debug, Clone, Serialize)]
pub struct TimeFrequency {
    /// `rate` (from the beat timing), `amplitude` or `beat_error`.
    pub series: &'static str,
    /// `stft` or `sst`.
    pub method: &'static str,
    /// Unit of the values: `ms` of timing for the rate, `deg`, `ms`.
    pub unit: &'static str,
    /// Spacing of the series the map was read from, seconds (for the SST,
    /// after averaging a long series down, see [`MAX_SAMPLES`]).
    pub step_s: f64,
    /// The STFT's window and the step between windows, seconds; `None`
    /// for the SST.
    pub window_s: Option<f64>,
    pub hop_s: Option<f64>,
    /// Periods, seconds, longest first, evenly spaced in log period.
    pub period_s: Vec<f64>,
    /// Column times, seconds on the series' time line: each STFT window's
    /// middle, or the middle of each SST slice of time.
    pub t_s: Vec<f64>,
    /// One column per `t_s`, each a size per `period_s`; `null` (NaN)
    /// where nothing was read: a window with a missing reading, a gap, or
    /// a period's cone of influence at a stretch's end. An SST column is
    /// the mean over its slice of time of the readings that were read.
    pub values: Vec<Vec<f64>>,
}

/// Rows of the period axis.
pub const ROWS: usize = 48;
/// STFT window and step, seconds.
pub const STFT_WINDOW_S: f64 = 1200.0;
pub const STFT_HOP_S: f64 = 300.0;
/// Longest period the SST reads, seconds, also kept to a quarter of the
/// longest stretch.
pub const SST_MAX_PERIOD_S: f64 = 3600.0;
/// A stretch longer than this many readings is averaged down in whole
/// blocks before the wavelet transform, to keep the work bounded.
pub const MAX_SAMPLES: usize = 1 << 16;
/// Columns of an SST map at most.
pub const MAX_COLUMNS: usize = 400;
/// Morlet wavelet's centre frequency, radians per unit scale.
const OMEGA0: f64 = 6.0;

/// `rows` periods evenly spaced in log period from `hi` down to `lo`.
fn period_axis(lo: f64, hi: f64, rows: usize) -> Vec<f64> {
    let (a, b) = (hi.ln(), lo.ln());
    (0..rows)
        .map(|r| (a + (b - a) * r as f64 / (rows - 1) as f64).exp())
        .collect()
}

/// `y` less its least-squares line, in place.
fn detrend_line(y: &mut [f64]) {
    let n = y.len() as f64;
    let mx = (n - 1.0) / 2.0;
    let my = y.iter().sum::<f64>() / n;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (i, v) in y.iter().enumerate() {
        let dx = i as f64 - mx;
        sxy += dx * (v - my);
        sxx += dx * dx;
    }
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    for (i, v) in y.iter_mut().enumerate() {
        *v -= my + slope * (i as f64 - mx);
    }
}

/// The STFT of `g`: windows of `window_s` stepped `hop_s`, each with
/// every reading present, its line taken off and a Hann window applied,
/// read at `ROWS` periods from 4 readings to a third of the window.
/// `scale` multiplies the values (1000 for seconds to ms). `None` when no
/// window fits.
pub fn stft(
    g: &Grid,
    series: &'static str,
    unit: &'static str,
    scale: f64,
    window_s: f64,
    hop_s: f64,
) -> Option<TimeFrequency> {
    let nw = (window_s / g.step).round() as usize;
    let hop = ((hop_s / g.step).round() as usize).max(1);
    let (lo, hi) = (4.0 * g.step, window_s / 3.0);
    if nw < 16 || g.y.len() < nw || hi <= lo {
        return None;
    }
    let period_s = period_axis(lo, hi, ROWS);
    let w: Vec<f64> = (0..nw)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * (i as f64 + 0.5) / nw as f64).cos())
        .collect();
    let wsum: f64 = w.iter().sum();
    let (mut t_s, mut values) = (Vec::new(), Vec::new());
    let mut x = vec![0.0; nw];
    let mut start = 0;
    while start + nw <= g.y.len() {
        let part = &g.y[start..start + nw];
        if part.iter().all(|v| v.is_finite()) {
            x.copy_from_slice(part);
            detrend_line(&mut x);
            let col = period_s
                .iter()
                .map(|&p| {
                    // A phasor turned one reading at a time.
                    let d = Complex::from_polar(1.0, -std::f64::consts::TAU * g.step / p);
                    let mut e = Complex::new(1.0, 0.0);
                    let mut acc = Complex::new(0.0, 0.0);
                    for (v, wi) in x.iter().zip(&w) {
                        acc += e * (v * wi);
                        e *= d;
                    }
                    2.0 * acc.norm() / wsum * scale
                })
                .collect();
            t_s.push(g.t0 + (start as f64 + nw as f64 / 2.0) * g.step);
            values.push(col);
        }
        start += hop;
    }
    if values.is_empty() {
        return None;
    }
    Some(TimeFrequency {
        series,
        method: "stft",
        unit,
        step_s: g.step,
        window_s: Some(window_s),
        hop_s: Some(hop as f64 * g.step),
        period_s,
        t_s,
        values,
    })
}

/// The unbroken stretches of `y`: index ranges of finite values.
fn stretches(y: &[f64]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < y.len() {
        if y[i].is_finite() {
            let a = i;
            while i < y.len() && y[i].is_finite() {
                i += 1;
            }
            out.push((a, i));
        } else {
            i += 1;
        }
    }
    out
}

/// The Morlet filter's gain at `sw` (scale times angular frequency):
/// 2 at the scale's own frequency, so a sine of size A reads A there.
fn morlet_gain(sw: f64) -> f64 {
    2.0 * (-(sw - OMEGA0).powi(2) / 2.0).exp()
}

/// The first `m` values of the analytic signal whose one-sided spectrum
/// (of a zero-padded length `inv.len()`) is `z`. Real inverse transforms
/// take a one-sided spectrum, so it comes from two of them: the real part
/// from `z`, the imaginary from `-i z`.
fn analytic(
    inv: &dyn ComplexToReal<f64>,
    m: usize,
    z: &dyn Fn(usize) -> Complex<f64>,
) -> Option<Vec<Complex<f64>>> {
    let l = inv.len();
    let mut half = inv.make_input_vec();
    let mut out = vec![0.0; l];
    let last = half.len() - 1;
    let mut part = |rot: Complex<f64>| -> Option<Vec<f64>> {
        for (j, h) in half.iter_mut().enumerate() {
            *h = z(j) * rot;
        }
        half[0] = Complex::new(0.0, 0.0);
        half[last] = Complex::new(0.0, 0.0);
        inv.process(&mut half, &mut out).ok()?;
        Some(out[..m].to_vec())
    };
    let re = part(Complex::new(1.0, 0.0))?;
    let im = part(Complex::new(0.0, -1.0))?;
    Some(
        re.iter()
            .zip(&im)
            .map(|(&r, &i)| Complex::new(r, i) * (0.5 / l as f64))
            .collect(),
    )
}

/// The synchrosqueezed Morlet wavelet transform of `g`, on each unbroken
/// stretch of at least 64 readings alone, at `ROWS` periods from 4
/// readings to [`SST_MAX_PERIOD_S`] (or a quarter of the longest
/// stretch), averaged into [`MAX_COLUMNS`] slices of time. `scale` as for
/// [`stft`]. `None` when no stretch is long enough.
pub fn sst(
    g: &Grid,
    series: &'static str,
    unit: &'static str,
    scale: f64,
) -> Option<TimeFrequency> {
    let runs: Vec<(usize, usize)> = stretches(&g.y)
        .into_iter()
        .filter(|(a, b)| b - a >= 64)
        .collect();
    let longest = runs.iter().map(|(a, b)| b - a).max()?;
    // Average down in whole blocks of `k` readings when a stretch is long.
    let k = longest.div_ceil(MAX_SAMPLES).max(1);
    let step = g.step * k as f64;
    let lo = 4.0 * step;
    let hi = SST_MAX_PERIOD_S.min(longest as f64 * g.step / 4.0);
    if hi <= lo {
        return None;
    }
    let period_s = period_axis(lo, hi, ROWS);
    let du = (period_s[0] / period_s[1]).ln();
    let ln_hi = period_s[0].ln();
    // Columns: even slices of the whole series' time.
    let t_first = g.time(0);
    let span = g.y.len() as f64 * g.step;
    let ncols = MAX_COLUMNS.min(g.y.len() / k).max(1);
    let col_w = span / ncols as f64;
    let mut sums = vec![vec![(0.0, 0usize); ROWS]; ncols];
    let mut planner = RealFftPlanner::<f64>::new();
    for &(a, b) in &runs {
        let x: Vec<f64> = g.y[a..b]
            .chunks_exact(k)
            .map(|c| c.iter().sum::<f64>() / k as f64)
            .collect();
        let m = x.len();
        if m < 64 {
            continue;
        }
        let t0 = g.time(a) + (k as f64 - 1.0) / 2.0 * g.step;
        let mut x = x;
        detrend_line(&mut x);
        let l = (2 * m).next_power_of_two();
        let fwd = planner.plan_fft_forward(l);
        let inv = planner.plan_fft_inverse(l);
        let mut buf = vec![0.0; l];
        buf[..m].copy_from_slice(&x);
        let mut spec = fwd.make_output_vec();
        fwd.process(&mut buf, &mut spec).ok()?;
        // The squeezed transform, accumulated per row at each reading.
        let mut tre = vec![vec![0.0f32; m]; ROWS];
        let mut tim = vec![vec![0.0f32; m]; ROWS];
        let dw = std::f64::consts::TAU / (l as f64 * step);
        for &p in &period_s {
            let sc = OMEGA0 * p / std::f64::consts::TAU;
            let filt = |j: usize| {
                let om = j as f64 * dw;
                spec[j] * morlet_gain(sc * om)
            };
            let Some(w) = analytic(inv.as_ref(), m, &filt) else {
                continue;
            };
            let Some(dwt) = analytic(inv.as_ref(), m, &|j| {
                filt(j) * Complex::new(0.0, j as f64 * dw)
            }) else {
                continue;
            };
            // The wavelet's e-folding time: readings nearer a stretch's
            // end than this are read partly from outside it.
            let coi = (std::f64::consts::SQRT_2 * sc / step).ceil() as usize;
            for i in coi..m.saturating_sub(coi) {
                let wi = w[i];
                let mag2 = wi.norm_sqr();
                if mag2 < 1e-30 {
                    continue;
                }
                // The frequency the transform's phase turns at here.
                let f = (dwt[i] * wi.conj()).im / mag2 / std::f64::consts::TAU;
                if f <= 0.0 {
                    continue;
                }
                let r = ((ln_hi - (1.0 / f).ln()) / du).round();
                if !(0.0..ROWS as f64).contains(&r) {
                    continue;
                }
                let c = wi * (du * OMEGA0 / (std::f64::consts::TAU).sqrt());
                tre[r as usize][i] += c.re as f32;
                tim[r as usize][i] += c.im as f32;
            }
        }
        for r in 0..ROWS {
            // A row sums the scales up to half again its period (longer
            // ones see a line at this period at under 1% of their gain),
            // so it is read only where all of those are clear of the
            // stretch's ends by twice their e-folding time, where what the
            // ends add has fallen to e^-8 of its power.
            let coi = (2.0 * std::f64::consts::SQRT_2 * OMEGA0 * 1.5 * period_s[r]
                / std::f64::consts::TAU
                / step)
                .ceil() as usize;
            for i in coi..m.saturating_sub(coi) {
                let v = (tre[r][i] as f64).hypot(tim[r][i] as f64) * scale;
                let t = t0 + i as f64 * step;
                let c = (((t - t_first + 0.5 * g.step) / col_w) as usize).min(ncols - 1);
                let cell = &mut sums[c][r];
                cell.0 += v;
                cell.1 += 1;
            }
        }
    }
    let values = sums
        .into_iter()
        .map(|col| {
            col.into_iter()
                .map(|(s, n)| if n > 0 { s / n as f64 } else { f64::NAN })
                .collect()
        })
        .collect();
    let t_s = (0..ncols)
        .map(|c| t_first - 0.5 * g.step + (c as f64 + 0.5) * col_w)
        .collect();
    Some(TimeFrequency {
        series,
        method: "sst",
        unit,
        step_s: step,
        window_s: None,
        hop_s: None,
        period_s,
        t_s,
        values,
    })
}

/// The wavelet coherence of two series read at the same times: how
/// tightly they move together at each period and time, and which leads.
#[derive(Debug, Clone, Serialize)]
pub struct Coherence {
    /// The two series, `x` first: `rate` and `amplitude` (the rate
    /// readings), or `tick_amplitude` and `tock_amplitude` (the 2 s
    /// windows).
    pub x: &'static str,
    pub y: &'static str,
    /// Spacing of the series, seconds (after averaging a long series
    /// down, see [`MAX_SAMPLES`]).
    pub step_s: f64,
    /// Periods, seconds, longest first, evenly spaced in log period.
    pub period_s: Vec<f64>,
    /// Column times, seconds: the middle of each slice of time.
    pub t_s: Vec<f64>,
    /// One column per `t_s`, a coherence from 0 to 1 per `period_s` (the
    /// mean over the slice); `null` (NaN) where nothing was read.
    pub coherence: Vec<Vec<f64>>,
    /// The phase of `x` against `y`, degrees from -180 to 180: positive
    /// when `x` leads, by the phase's share of 360 of a period. `null`
    /// where `coherence` is.
    pub phase_deg: Vec<Vec<f64>>,
    /// Per period, the coherence that 95% of the values from pairs of
    /// unrelated red-noise series, with the two series' own lag-one
    /// correlations, stay under.
    pub noise_95: Vec<f64>,
    /// How many red-noise pairs `noise_95` comes from.
    pub noise_pairs: usize,
}

/// Red-noise pairs behind [`Coherence::noise_95`].
pub const NOISE_PAIRS: usize = 40;
/// Length of each red-noise series at most, readings.
const NOISE_MAX: usize = 1 << 14;
/// Readings each red-noise series holds beyond the longest period's
/// cones of influence at its two ends.
const NOISE_INSIDE: usize = 1024;
/// Values kept per period from each red-noise pair.
const NOISE_KEEP: usize = 200;
/// Width of the smoothing across periods, octaves (Torrence and Webster
/// 1999 give 0.6 for the Morlet wavelet).
const SCALE_SMOOTH_OCTAVES: f64 = 0.6;

/// `v[a..b]` smoothed by a Gaussian of `sigma` readings (three passes of
/// a running mean), each value divided by the share of the kernel inside
/// `a..b`, so nothing from outside is read.
fn smooth(v: &mut [f64], sigma: f64) {
    let n = v.len();
    if n == 0 {
        return;
    }
    // Three boxes of width w have the variance of a Gaussian of sigma
    // when w^2 = 4 sigma^2 + 1.
    let half = (((4.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0).round() as usize;
    let mut ones = vec![1.0; n];
    let pass = |x: &mut [f64]| {
        let mut c = vec![0.0; n + 1];
        for i in 0..n {
            c[i + 1] = c[i] + x[i];
        }
        for (i, xi) in x.iter_mut().enumerate() {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(n);
            *xi = (c[hi] - c[lo]) / (2 * half + 1) as f64;
        }
    };
    for _ in 0..3 {
        pass(v);
        pass(&mut ones);
    }
    for (x, o) in v.iter_mut().zip(&ones) {
        *x /= o;
    }
}

/// One reading of a coherence map: the coherence and the cross spectrum
/// divided by the root of the two auto spectra's product.
type Cell = (f64, Complex<f64>);

/// The smoothed cross and auto wavelet spectra of one stretch of `x` and
/// `y` (each with its line taken off), per period: for each reading in
/// the period's cone of influence, `(coherence, normalised cross
/// spectrum)`, NaN outside it.
fn coherence_stretch(
    planner: &mut RealFftPlanner<f64>,
    x: &[f64],
    y: &[f64],
    period_s: &[f64],
    step: f64,
) -> Option<Vec<Vec<Cell>>> {
    let m = x.len();
    let l = (2 * m).next_power_of_two();
    let fwd = planner.plan_fft_forward(l);
    let inv = planner.plan_fft_inverse(l);
    let spectrum = |v: &[f64]| -> Option<Vec<Complex<f64>>> {
        let mut buf = vec![0.0; l];
        buf[..m].copy_from_slice(v);
        detrend_line(&mut buf[..m]);
        let mut spec = fwd.make_output_vec();
        fwd.process(&mut buf, &mut spec).ok()?;
        Some(spec)
    };
    let (sx, sy) = (spectrum(x)?, spectrum(y)?);
    let dw = std::f64::consts::TAU / (l as f64 * step);
    let rows = period_s.len();
    // Per row: the smoothed |Wx|^2, |Wy|^2 and Wx Wy*, each over the
    // scale, inside the row's cone of influence.
    let mut auto_x = vec![vec![f64::NAN; m]; rows];
    let mut auto_y = vec![vec![f64::NAN; m]; rows];
    let mut cross = vec![vec![Complex::new(f64::NAN, f64::NAN); m]; rows];
    for (r, &p) in period_s.iter().enumerate() {
        let sc = OMEGA0 * p / std::f64::consts::TAU;
        let coi = (std::f64::consts::SQRT_2 * sc / step).ceil() as usize;
        if 2 * coi >= m {
            continue;
        }
        let wx = analytic(inv.as_ref(), m, &|j| {
            sx[j] * morlet_gain(sc * j as f64 * dw)
        })?;
        let wy = analytic(inv.as_ref(), m, &|j| {
            sy[j] * morlet_gain(sc * j as f64 * dw)
        })?;
        let (a, b) = (coi, m - coi);
        let sigma = sc / step;
        let mut parts: Vec<Vec<f64>> = vec![
            wx[a..b].iter().map(|w| w.norm_sqr() / sc).collect(),
            wy[a..b].iter().map(|w| w.norm_sqr() / sc).collect(),
            (a..b).map(|i| (wx[i] * wy[i].conj()).re / sc).collect(),
            (a..b).map(|i| (wx[i] * wy[i].conj()).im / sc).collect(),
        ];
        for v in parts.iter_mut() {
            smooth(v, sigma);
        }
        for (k, i) in (a..b).enumerate() {
            auto_x[r][i] = parts[0][k];
            auto_y[r][i] = parts[1][k];
            cross[r][i] = Complex::new(parts[2][k], parts[3][k]);
        }
    }
    // Across periods: a running mean over SCALE_SMOOTH_OCTAVES, of the
    // rows read at each reading.
    let du = if rows > 1 {
        (period_s[0] / period_s[1]).ln()
    } else {
        1.0
    };
    let reach = (SCALE_SMOOTH_OCTAVES * std::f64::consts::LN_2 / du / 2.0).round() as usize;
    let mut out = vec![vec![(f64::NAN, Complex::new(f64::NAN, f64::NAN)); m]; rows];
    for r in 0..rows {
        let (lo, hi) = (r.saturating_sub(reach), (r + reach + 1).min(rows));
        for i in 0..m {
            if !auto_x[r][i].is_finite() {
                continue;
            }
            let (mut ax, mut ay, mut c) = (0.0, 0.0, Complex::new(0.0, 0.0));
            for q in lo..hi {
                if auto_x[q][i].is_finite() {
                    ax += auto_x[q][i];
                    ay += auto_y[q][i];
                    c += cross[q][i];
                }
            }
            let d = (ax * ay).sqrt();
            if d > 0.0 {
                out[r][i] = ((c.norm_sqr() / (ax * ay)).min(1.0), c / d);
            }
        }
    }
    Some(out)
}

/// The wavelet coherence of `x` and `y` (on one grid), on each stretch
/// where both are read, of at least 64 readings, alone, at `ROWS`
/// periods as for [`sst`], averaged into [`MAX_COLUMNS`] slices of time,
/// with the red-noise level each period's coherence is read against.
/// `None` when no stretch is long enough.
pub fn coherence(
    x: &Grid,
    y: &Grid,
    x_name: &'static str,
    y_name: &'static str,
) -> Option<Coherence> {
    let n = x.y.len().min(y.y.len());
    let both: Vec<f64> = (0..n)
        .map(|i| {
            if x.y[i].is_finite() && y.y[i].is_finite() {
                0.0
            } else {
                f64::NAN
            }
        })
        .collect();
    let runs: Vec<(usize, usize)> = stretches(&both)
        .into_iter()
        .filter(|(a, b)| b - a >= 64)
        .collect();
    let longest = runs.iter().map(|(a, b)| b - a).max()?;
    let k = longest.div_ceil(MAX_SAMPLES).max(1);
    let step = x.step * k as f64;
    let lo = 4.0 * step;
    let hi = SST_MAX_PERIOD_S.min(longest as f64 * x.step / 4.0);
    if hi <= lo {
        return None;
    }
    let period_s = period_axis(lo, hi, ROWS);
    let down = |g: &Grid, a: usize, b: usize| -> Vec<f64> {
        g.y[a..b]
            .chunks_exact(k)
            .map(|c| c.iter().sum::<f64>() / k as f64)
            .collect()
    };
    let t_first = x.time(0);
    let span = n as f64 * x.step;
    let ncols = MAX_COLUMNS.min(n / k).max(1);
    let col_w = span / ncols as f64;
    let mut sums = vec![vec![(0.0, Complex::new(0.0, 0.0), 0usize); ROWS]; ncols];
    let mut planner = RealFftPlanner::<f64>::new();
    // Lag-one correlations, over every stretch, for the red noise.
    let (mut lag, mut var) = ([0.0; 2], [0.0; 2]);
    let mut longest_down = 0;
    for &(a, b) in &runs {
        let (mut xs, mut ys) = (down(x, a, b), down(y, a, b));
        let m = xs.len();
        if m < 64 {
            continue;
        }
        longest_down = longest_down.max(m);
        let Some(rows) = coherence_stretch(&mut planner, &xs, &ys, &period_s, step) else {
            continue;
        };
        detrend_line(&mut xs);
        detrend_line(&mut ys);
        for (j, v) in [&xs, &ys].into_iter().enumerate() {
            lag[j] += v.windows(2).map(|w| w[0] * w[1]).sum::<f64>();
            var[j] += v.iter().map(|q| q * q).sum::<f64>();
        }
        let t0 = x.time(a) + (k as f64 - 1.0) / 2.0 * x.step;
        for (r, row) in rows.iter().enumerate() {
            for (i, &(c, z)) in row.iter().enumerate() {
                if !c.is_finite() {
                    continue;
                }
                let t = t0 + i as f64 * step;
                let col = (((t - t_first + 0.5 * x.step) / col_w) as usize).min(ncols - 1);
                let cell = &mut sums[col][r];
                cell.0 += c;
                cell.1 += z;
                cell.2 += 1;
            }
        }
    }
    if longest_down == 0 {
        return None;
    }
    let ar = [0, 1].map(|j| {
        if var[j] > 0.0 {
            (lag[j] / var[j]).clamp(0.0, 0.99)
        } else {
            0.0
        }
    });
    // Long enough for the longest period to be read on NOISE_INSIDE
    // readings, and no longer than the take's longest stretch.
    let coi_hi = std::f64::consts::SQRT_2 * OMEGA0 * period_s[0] / std::f64::consts::TAU / step;
    let noise_len = (2 * coi_hi.ceil() as usize + NOISE_INSIDE)
        .min(NOISE_MAX)
        .min(longest_down);
    let noise_95 = red_noise_level(&mut planner, ar, noise_len, &period_s, step);
    let (mut coherence, mut phase_deg) = (Vec::new(), Vec::new());
    for col in sums {
        coherence.push(
            col.iter()
                .map(|&(c, _, n)| if n > 0 { c / n as f64 } else { f64::NAN })
                .collect(),
        );
        phase_deg.push(
            col.iter()
                .map(|&(_, z, n)| {
                    if n > 0 {
                        z.arg().to_degrees()
                    } else {
                        f64::NAN
                    }
                })
                .collect(),
        );
    }
    let t_s = (0..ncols)
        .map(|c| t_first - 0.5 * x.step + (c as f64 + 0.5) * col_w)
        .collect();
    Some(Coherence {
        x: x_name,
        y: y_name,
        step_s: step,
        period_s,
        t_s,
        coherence,
        phase_deg,
        noise_95,
        noise_pairs: NOISE_PAIRS,
    })
}

/// Per period, the 95th percentile of the coherence of [`NOISE_PAIRS`]
/// pairs of unrelated red-noise series of `len` readings, with lag-one
/// correlations `ar` (Grinsted, Moore and Jevrejeva 2004). The seed is
/// fixed, so a take always gives the same levels.
fn red_noise_level(
    planner: &mut RealFftPlanner<f64>,
    ar: [f64; 2],
    len: usize,
    period_s: &[f64],
    step: f64,
) -> Vec<f64> {
    let mut rng = crate::synth::Rng::new(1);
    let mut red = |a: f64| -> Vec<f64> {
        let s = (1.0 - a * a).sqrt();
        let mut v = Vec::with_capacity(len);
        let mut prev = rng.normal();
        for _ in 0..len {
            prev = a * prev + s * rng.normal();
            v.push(prev);
        }
        v
    };
    let mut kept = vec![Vec::<f64>::new(); period_s.len()];
    for _ in 0..NOISE_PAIRS {
        let (nx, ny) = (red(ar[0]), red(ar[1]));
        let Some(rows) = coherence_stretch(planner, &nx, &ny, period_s, step) else {
            continue;
        };
        for (r, row) in rows.iter().enumerate() {
            let read: Vec<f64> = row.iter().map(|q| q.0).filter(|c| c.is_finite()).collect();
            let every = read.len().div_ceil(NOISE_KEEP).max(1);
            kept[r].extend(read.iter().step_by(every));
        }
    }
    kept.into_iter()
        .map(|mut v| {
            if v.is_empty() {
                return f64::NAN;
            }
            v.sort_by(|a, b| a.total_cmp(b));
            v[((v.len() - 1) as f64 * 0.95).round() as usize]
        })
        .collect()
}

/// Both maps of each series a take's period searches and spectra read:
/// the beat timing (in ms, so the rate's map reaches the escape wheel),
/// the amplitude, and the beat error from the unlock, each with its slow
/// change taken off as the searches take it off. STFT maps first.
pub fn for_take(
    log: &crate::stream::BeatLog,
    clock: Option<&crate::clock::ClockFit>,
    long: &crate::longrun::LongReport,
    lc: &crate::longterm::LongConfig,
) -> Vec<TimeFrequency> {
    let [_, be] = crate::steadiness::window_grids(log, clock, &long.breaks);
    let be = crate::longterm::detrend(&be, lc.detrend_s);
    let inputs = [
        (&long.timing.detrended, "rate", "ms", 1000.0),
        (&long.amplitude.detrended, "amplitude", "deg", 1.0),
        (&be, "beat_error", "ms", 1.0),
    ];
    let stfts = inputs
        .iter()
        .filter_map(|(g, name, unit, k)| stft(g, name, unit, *k, STFT_WINDOW_S, STFT_HOP_S));
    let ssts = inputs
        .iter()
        .filter_map(|(g, name, unit, k)| sst(g, name, unit, *k));
    stfts.chain(ssts).collect()
}

/// The coherence maps of a take: the rate against the amplitude, on the
/// `reading_s` readings [`crate::steadiness::check`] correlates them on,
/// and Tick's amplitude against Tock's, on the 2 s windows, so a cycle
/// both sides share shows in phase.
pub fn coherences_for_take(
    log: &crate::stream::BeatLog,
    clock: Option<&crate::clock::ClockFit>,
    long: &crate::longrun::LongReport,
    reading_s: f64,
) -> Vec<Coherence> {
    let [rate, amp] = crate::steadiness::rate_amplitude_grids(log, clock, &long.breaks, reading_s);
    let [tick, tock] = crate::steadiness::side_grids(log, clock, &long.breaks);
    [
        coherence(&rate, &amp, "rate", "amplitude"),
        coherence(&tick, &tock, "tick_amplitude", "tock_amplitude"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(step: f64, n: usize, mut f: impl FnMut(f64) -> f64) -> Grid {
        let mut g = Grid {
            t0: 0.0,
            step,
            y: vec![0.0; n],
        };
        for i in 0..n {
            g.y[i] = f(g.time(i));
        }
        g
    }

    /// The row nearest `p`.
    fn row(tf: &TimeFrequency, p: f64) -> usize {
        (0..tf.period_s.len())
            .min_by(|&a, &b| {
                (tf.period_s[a] / p)
                    .ln()
                    .abs()
                    .total_cmp(&(tf.period_s[b] / p).ln().abs())
            })
            .unwrap()
    }

    fn peak(col: &[f64]) -> (usize, f64) {
        col.iter()
            .copied()
            .enumerate()
            .filter(|(_, v)| v.is_finite())
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap()
    }

    #[test]
    fn a_steady_line_reads_its_size_at_its_period() {
        // A 60 s sine of size 2 on 2 s readings over 2 h.
        let g = grid(2.0, 3600, |t| {
            2.0 * (std::f64::consts::TAU * t / 60.0).sin()
        });
        for tf in [
            stft(&g, "amplitude", "deg", 1.0, STFT_WINDOW_S, STFT_HOP_S).unwrap(),
            sst(&g, "amplitude", "deg", 1.0).unwrap(),
        ] {
            let r = row(&tf, 60.0);
            let mut read = 0;
            for col in &tf.values {
                // Near a stretch's ends only the short periods are read.
                if col[r].is_nan() {
                    continue;
                }
                let (pr, size) = peak(col);
                assert!(pr.abs_diff(r) <= 1, "{}: row {pr}, want {r}", tf.method);
                assert!((size / 2.0 - 1.0).abs() < 0.1, "{}: size {size}", tf.method);
                read += 1;
            }
            assert!(read > 10, "{}: {read} columns", tf.method);
        }
    }

    #[test]
    fn a_line_that_switches_on_shows_from_then() {
        // A 60 s line of size 1 from 1 h of a 2 h series only.
        let g = grid(2.0, 3600, |t| {
            if t >= 3600.0 {
                (std::f64::consts::TAU * t / 60.0).sin()
            } else {
                0.0
            }
        });
        for tf in [
            stft(&g, "amplitude", "deg", 1.0, STFT_WINDOW_S, STFT_HOP_S).unwrap(),
            sst(&g, "amplitude", "deg", 1.0).unwrap(),
        ] {
            let r = row(&tf, 60.0);
            for (t, col) in tf.t_s.iter().zip(&tf.values) {
                let v = col[r];
                if !v.is_finite() {
                    continue;
                }
                // Clear of the switch by the window (STFT) or the 60 s
                // wavelet's reach (SST).
                let reach = tf.window_s.map_or(300.0, |w| w / 2.0 + 60.0);
                if *t < 3600.0 - reach {
                    assert!(v < 0.05, "{}: {v} at {t}", tf.method);
                } else if *t > 3600.0 + reach {
                    assert!((v - 1.0).abs() < 0.15, "{}: {v} at {t}", tf.method);
                }
            }
        }
    }

    #[test]
    fn a_line_under_noise_stands_above_it() {
        // A 60 s line of size 0.5 under reading noise of 1 (sd): in both
        // maps its row stays well above the other rows' typical level.
        let mut rng = crate::synth::Rng::new(3);
        let g = grid(2.0, 3600, |t| {
            0.5 * (std::f64::consts::TAU * t / 60.0).sin() + rng.normal()
        });
        for tf in [
            stft(&g, "amplitude", "deg", 1.0, STFT_WINDOW_S, STFT_HOP_S).unwrap(),
            sst(&g, "amplitude", "deg", 1.0).unwrap(),
        ] {
            let r = row(&tf, 60.0);
            let (mut line, mut rest) = (Vec::new(), Vec::new());
            for col in &tf.values {
                // The line's size is its row's or a neighbour's: it can
                // fall between two rows.
                let near = col[r - 1].max(col[r]).max(col[r + 1]);
                if near.is_finite() {
                    line.push(near);
                }
                for (i, v) in col.iter().enumerate() {
                    if v.is_finite() && i.abs_diff(r) > 4 {
                        rest.push(*v);
                    }
                }
            }
            let med = |v: &mut Vec<f64>| {
                v.sort_by(f64::total_cmp);
                v[v.len() / 2]
            };
            let (l, n) = (med(&mut line), med(&mut rest));
            // Noise moves each reading's own period a little, so the SST
            // spreads a line over neighbouring rows and any one row reads
            // under its size; the STFT keeps the size.
            let want = if tf.method == "stft" { 0.4 } else { 0.25 };
            assert!(l > want && l > 3.0 * n, "{}: line {l}, rest {n}", tf.method);
        }
    }

    #[test]
    fn nothing_is_read_across_a_gap() {
        // 2 h of a 60 s line with a missing 10 min in the middle: no STFT
        // window holds a missing reading, and the SST leaves the gap and
        // its edges empty.
        let mut g = grid(2.0, 3600, |t| (std::f64::consts::TAU * t / 60.0).sin());
        for v in &mut g.y[1650..1950] {
            *v = f64::NAN;
        }
        let s = stft(&g, "amplitude", "deg", 1.0, STFT_WINDOW_S, STFT_HOP_S).unwrap();
        for t in &s.t_s {
            let (a, b) = (t - 600.0, t + 600.0);
            assert!(b <= 3300.0 || a >= 3900.0, "window {a}..{b}");
        }
        let w = sst(&g, "amplitude", "deg", 1.0).unwrap();
        let r = row(&w, 60.0);
        for (t, col) in w.t_s.iter().zip(&w.values) {
            if *t > 3310.0 && *t < 3890.0 {
                assert!(col[r].is_nan(), "{t}: {}", col[r]);
            }
        }
        // The edges' cone: the 60 s row is empty within its reach of the
        // gap; longer periods reach further.
        let long = row(&w, 1000.0);
        let near = w
            .t_s
            .iter()
            .zip(&w.values)
            .filter(|(t, _)| **t > 3000.0 && **t < 3300.0)
            .collect::<Vec<_>>();
        assert!(near.iter().all(|(_, c)| c[long].is_nan()));
        assert!(near.iter().any(|(_, c)| c[r].is_finite()));
    }

    fn row_of(period_s: &[f64], p: f64) -> usize {
        (0..period_s.len())
            .min_by(|&i, &j| {
                (period_s[i] / p)
                    .ln()
                    .abs()
                    .total_cmp(&(period_s[j] / p).ln().abs())
            })
            .unwrap()
    }

    /// The coherence map's row nearest `p`, and the columns whose middles
    /// lie between `a` and `b` seconds and were read there.
    fn coh_cells(c: &Coherence, p: f64, a: f64, b: f64) -> Vec<(f64, f64)> {
        let r = row_of(&c.period_s, p);
        c.t_s
            .iter()
            .enumerate()
            .filter(|(_, &t)| t >= a && t <= b)
            .map(|(i, _)| (c.coherence[i][r], c.phase_deg[i][r]))
            .filter(|q| q.0.is_finite())
            .collect()
    }

    #[test]
    fn a_shared_cycle_is_coherent_and_its_lead_is_read() {
        // Two series sharing a 120 s cycle, y 30 s behind x (a quarter
        // turn), each with its own noise as large as the cycle.
        let mut rng = crate::synth::Rng::new(3);
        let w = std::f64::consts::TAU / 120.0;
        let x = grid(10.0, 2160, |t| (w * t).sin() + rng.normal());
        let mut rng = crate::synth::Rng::new(4);
        let y = grid(10.0, 2160, |t| (w * (t - 30.0)).sin() + rng.normal());
        let c = coherence(&x, &y, "x", "y").unwrap();
        let cells = coh_cells(&c, 120.0, 0.0, 1e9);
        assert!(cells.len() > 100);
        let mean = cells.iter().map(|q| q.0).sum::<f64>() / cells.len() as f64;
        let r = row_of(&c.period_s, 120.0);
        assert!(
            mean > 0.75 && mean > c.noise_95[r],
            "{mean} {}",
            c.noise_95[r]
        );
        let phase = cells.iter().map(|q| q.1).sum::<f64>() / cells.len() as f64;
        assert!((phase - 90.0).abs() < 15.0, "{phase}");
    }

    #[test]
    fn unrelated_noise_stays_near_the_noise_level() {
        let mut rng = crate::synth::Rng::new(5);
        let x = grid(10.0, 2160, |_| rng.normal());
        let mut rng = crate::synth::Rng::new(6);
        let y = grid(10.0, 2160, |_| rng.normal());
        let c = coherence(&x, &y, "x", "y").unwrap();
        // Over every period and time, about one value in twenty above the
        // 95% level, as the level means.
        let (mut above, mut all) = (0, 0);
        for col in &c.coherence {
            for (r, v) in col.iter().enumerate() {
                if v.is_finite() {
                    all += 1;
                    above += (*v > c.noise_95[r]) as usize;
                }
            }
        }
        let share = above as f64 / all as f64;
        assert!(all > 1000 && share < 0.12, "{share} of {all}");
    }

    #[test]
    fn coherence_that_starts_halfway_shows_from_then() {
        let w = std::f64::consts::TAU / 120.0;
        let mut rng = crate::synth::Rng::new(7);
        let x = grid(10.0, 4320, |t| (w * t).sin() + 0.5 * rng.normal());
        let mut rng = crate::synth::Rng::new(8);
        let y = grid(10.0, 4320, |t| {
            let shared = if t >= 21600.0 { (w * t).sin() } else { 0.0 };
            shared + 0.5 * rng.normal()
        });
        let c = coherence(&x, &y, "x", "y").unwrap();
        let mean = |v: Vec<(f64, f64)>| v.iter().map(|q| q.0).sum::<f64>() / v.len() as f64;
        let before = mean(coh_cells(&c, 120.0, 0.0, 19000.0));
        let after = mean(coh_cells(&c, 120.0, 24000.0, 1e9));
        let r = row_of(&c.period_s, 120.0);
        assert!(before < c.noise_95[r], "{before} {}", c.noise_95[r]);
        assert!(after > 0.8, "{after}");
    }

    #[test]
    fn coherence_is_not_read_across_a_gap() {
        let w = std::f64::consts::TAU / 120.0;
        let gap = |t: f64| (10000.0..12000.0).contains(&t);
        let x = grid(
            10.0,
            2160,
            |t| if gap(t) { f64::NAN } else { (w * t).sin() },
        );
        let y = grid(10.0, 2160, |t| (w * t).cos());
        let c = coherence(&x, &y, "x", "y").unwrap();
        // The period's cone of influence: sqrt 2 scales, a scale being
        // 6 / 2 pi of the period.
        let coi = std::f64::consts::SQRT_2 * 6.0 / std::f64::consts::TAU * 120.0;
        let r = row_of(&c.period_s, 120.0);
        for (i, &t) in c.t_s.iter().enumerate() {
            let half = (c.t_s[1] - c.t_s[0]) / 2.0;
            if t - half >= 10000.0 - coi && t + half <= 12000.0 + coi {
                assert!(c.coherence[i][r].is_nan(), "read at {t}");
            }
        }
        assert!(coh_cells(&c, 120.0, 0.0, 1e9).len() > 50);
    }
}
