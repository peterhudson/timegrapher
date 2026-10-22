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

use crate::longterm::Grid;
use realfft::num_complex::Complex;
use realfft::RealFftPlanner;
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
        let mut half = inv.make_input_vec();
        let mut out = vec![0.0; l];
        // Real inverse transforms of a one-sided spectrum, so the analytic
        // signal comes from two real transforms: the real part from `z`,
        // the imaginary from `-i z`.
        let mut analytic = |z: &dyn Fn(usize) -> Complex<f64>| -> Option<Vec<Complex<f64>>> {
            let mut re = vec![0.0; m];
            for (j, h) in half.iter_mut().enumerate() {
                *h = z(j);
            }
            half[0] = Complex::new(0.0, 0.0);
            let last = half.len() - 1;
            half[last] = Complex::new(0.0, 0.0);
            inv.process(&mut half, &mut out).ok()?;
            re.copy_from_slice(&out[..m]);
            for (j, h) in half.iter_mut().enumerate() {
                *h = z(j) * Complex::new(0.0, -1.0);
            }
            half[0] = Complex::new(0.0, 0.0);
            half[last] = Complex::new(0.0, 0.0);
            inv.process(&mut half, &mut out).ok()?;
            Some(
                re.iter()
                    .zip(&out[..m])
                    .map(|(&r, &i)| Complex::new(r, i) * (0.5 / l as f64))
                    .collect(),
            )
        };
        let dw = std::f64::consts::TAU / (l as f64 * step);
        for &p in &period_s {
            let sc = OMEGA0 * p / std::f64::consts::TAU;
            let filt = |j: usize| {
                let om = j as f64 * dw;
                // Gain 2 at the scale's own frequency, so a sine of size A
                // reads A there.
                spec[j] * (2.0 * (-(sc * om - OMEGA0).powi(2) / 2.0).exp())
            };
            let Some(w) = analytic(&filt) else { continue };
            let Some(dwt) = analytic(&|j| filt(j) * Complex::new(0.0, j as f64 * dw)) else {
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
}
