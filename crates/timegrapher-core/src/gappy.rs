//! Lomb–Scargle on a take's readings, gaps and all.
//!
//! The period search and the FFT read a grid, with each gap filled by the
//! mean. Here each reading goes in at its own time on the tests' time line
//! and nothing is filled in, so a long run with pauses gives one spectrum
//! over the whole of it. The periodogram is the generalised one of
//! Zechmeister and Kürster (2009): a floating mean, and each reading
//! weighted by the scatter of the readings around it, so a noisy stretch
//! counts for less.
//!
//! Beside each peak:
//! - its false-alarm probability, by Baluev's (2008) bound: the chance
//!   that readings with no cycle in them, independent from one to the
//!   next with the same spread, give a peak this tall anywhere in the
//!   band;
//! - the spectral window there: the periodogram of the reading times
//!   alone. Gaps that recur put lines in it (a pause every 30 min puts one
//!   at 30 min), and a peak that sits on one may come from the gaps.

use crate::longterm::Grid;
use crate::periodicity::Wheel;
use serde::Serialize;

/// Frequencies per independent frequency (1 / span).
const OVERSAMPLE: f64 = 5.0;
/// Points kept for drawing, at most.
const MAX_POINTS: usize = 600;
/// Peaks reported, strongest first, at most.
const MAX_PEAKS: usize = 8;
/// Readings either side whose scatter weights a reading.
const SCATTER_HALF: usize = 30;
/// The spectral window's power at which a peak counts as on a window
/// line. Without gaps the window's sidelobes in the band (from three
/// cycles over the span) stay under 0.009; a 5 min pause every 30 min
/// gives 0.037 at 30 min.
pub const WINDOW_LINE: f64 = 0.02;

/// One peak of a [`GappySpectrum`].
#[derive(Debug, Clone, Serialize)]
pub struct Peak {
    pub period_s: f64,
    /// Share of the weighted variance a sine at this period explains.
    pub power: f64,
    /// Baluev's false-alarm probability over the whole band.
    pub false_alarm: f64,
    /// The spectral window's highest power within one resolution width
    /// (one cycle over the span) of this period, 0 to 1.
    pub window_power: f64,
    /// `window_power` is at least [`WINDOW_LINE`]: the gaps alone put a
    /// line here.
    pub on_window_line: bool,
    /// A wheel whose turn is this period within the resolution (or 1%).
    pub wheel: Option<String>,
}

/// The Lomb–Scargle periodogram of one series' readings.
#[derive(Debug, Clone, Serialize)]
pub struct GappySpectrum {
    /// `rate` (s/d), `amplitude` (deg) or `beat_error` (ms).
    pub series: &'static str,
    pub unit: &'static str,
    /// Readings used, and the time from the first to the last.
    pub readings: usize,
    pub span_s: f64,
    /// Periods, seconds, longest first, thinned to at most 600 on a log
    /// axis, keeping the strongest in each slice.
    pub period_s: Vec<f64>,
    /// Share of the weighted variance a sine at each period explains.
    pub power: Vec<f64>,
    /// The spectral window at each period.
    pub window: Vec<f64>,
    pub peaks: Vec<Peak>,
}

/// The generalised Lomb–Scargle power (0 to 1) of `y` at times `t` with
/// weights `w` (summing to 1), at frequencies `f0 + k df`, `k < nf`. The
/// sines and cosines turn one step of `df` at a time.
fn gls(t: &[f64], y: &[f64], w: &[f64], f0: f64, df: f64, nf: usize) -> Vec<f64> {
    let tau = std::f64::consts::TAU;
    let ybar: f64 = w.iter().zip(y).map(|(a, b)| a * b).sum();
    let yy: f64 = w.iter().zip(y).map(|(a, b)| a * (b - ybar).powi(2)).sum();
    let mut c: Vec<f64> = t.iter().map(|&ti| (tau * f0 * ti).cos()).collect();
    let mut s: Vec<f64> = t.iter().map(|&ti| (tau * f0 * ti).sin()).collect();
    let (dc, ds): (Vec<f64>, Vec<f64>) = t
        .iter()
        .map(|&ti| (tau * df * ti).sin_cos())
        .map(|(a, b)| (b, a))
        .unzip();
    let mut out = Vec::with_capacity(nf);
    for _ in 0..nf {
        let (mut sc, mut ss_, mut syc, mut sys, mut scc, mut scs) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for i in 0..t.len() {
            let (wi, ci, si) = (w[i], c[i], s[i]);
            let yi = y[i] - ybar;
            sc += wi * ci;
            ss_ += wi * si;
            syc += wi * yi * ci;
            sys += wi * yi * si;
            scc += wi * ci * ci;
            scs += wi * ci * si;
        }
        let cc = scc - sc * sc;
        let ssv = (1.0 - scc) - ss_ * ss_;
        let cs = scs - sc * ss_;
        let d = cc * ssv - cs * cs;
        let p = if d > 0.0 && yy > 0.0 {
            (ssv * syc * syc + cc * sys * sys - 2.0 * cs * syc * sys) / (yy * d)
        } else {
            0.0
        };
        out.push(p.clamp(0.0, 1.0));
        for i in 0..t.len() {
            let (ci, si) = (c[i], s[i]);
            c[i] = ci * dc[i] - si * ds[i];
            s[i] = si * dc[i] + ci * ds[i];
        }
    }
    out
}

/// The spectral window `|sum exp(2 pi i f t)|^2 / n^2` at the same
/// frequencies as [`gls`].
fn window(t: &[f64], f0: f64, df: f64, nf: usize) -> Vec<f64> {
    let tau = std::f64::consts::TAU;
    let n = t.len() as f64;
    let mut c: Vec<f64> = t.iter().map(|&ti| (tau * f0 * ti).cos()).collect();
    let mut s: Vec<f64> = t.iter().map(|&ti| (tau * f0 * ti).sin()).collect();
    let (dc, ds): (Vec<f64>, Vec<f64>) = t
        .iter()
        .map(|&ti| (tau * df * ti).sin_cos())
        .map(|(a, b)| (b, a))
        .unzip();
    let mut out = Vec::with_capacity(nf);
    for _ in 0..nf {
        let (sc, ss): (f64, f64) = (c.iter().sum(), s.iter().sum());
        out.push((sc * sc + ss * ss) / (n * n));
        for i in 0..t.len() {
            let (ci, si) = (c[i], s[i]);
            c[i] = ci * dc[i] - si * ds[i];
            s[i] = si * dc[i] + ci * ds[i];
        }
    }
    out
}

/// Baluev's (2008) false-alarm probability of a peak of power `z` (the
/// generalised periodogram's 0-to-1 form) over frequencies up to `fmax`,
/// from `n` readings at times `t` with weights `w` (summing to 1).
pub fn false_alarm(z: f64, n: usize, fmax: f64, t: &[f64], w: &[f64]) -> f64 {
    if n < 5 {
        return 1.0;
    }
    let (nh, nk) = (n as f64 - 1.0, n as f64 - 3.0);
    // One frequency: the power of noise is Beta(1, nk / 2).
    let single = (1.0 - z).max(0.0).powf(0.5 * nk);
    let tbar: f64 = w.iter().zip(t).map(|(a, b)| a * b).sum();
    let var_t: f64 = w.iter().zip(t).map(|(a, b)| a * (b - tbar).powi(2)).sum();
    let width = fmax * (4.0 * std::f64::consts::PI * var_t).sqrt();
    let gamma = (2.0 / nh).sqrt()
        * (crate::steadiness::ln_gamma(nh / 2.0) - crate::steadiness::ln_gamma((nh - 1.0) / 2.0))
            .exp();
    let tau = gamma * width * (1.0 - z).max(0.0).powf(0.5 * (nk - 1.0)) * (0.5 * nh * z).sqrt();
    (1.0 - (1.0 - single) * (-tau).exp()).clamp(0.0, 1.0)
}

/// Each reading's weight: one over the square of the scatter from one
/// reading to the next among the readings either side of it (up to
/// [`SCATTER_HALF`] each way, in the same stretch), robustly (the median
/// absolute difference), normalised to sum to 1.
fn weights(y: &[f64], stretch: &[usize]) -> Vec<f64> {
    let n = y.len();
    let mut w = vec![0.0; n];
    for i in 0..n {
        let lo = i.saturating_sub(SCATTER_HALF);
        let hi = (i + SCATTER_HALF + 1).min(n);
        let mut d: Vec<f64> = (lo + 1..hi)
            .filter(|&j| stretch[j] == stretch[i] && stretch[j - 1] == stretch[i])
            .map(|j| (y[j] - y[j - 1]).abs())
            .collect();
        let sd = if d.len() >= 5 {
            crate::dsp::median(&mut d) * 1.4826 / std::f64::consts::SQRT_2
        } else {
            f64::NAN
        };
        w[i] = if sd.is_finite() && sd > 0.0 {
            1.0 / (sd * sd)
        } else {
            f64::NAN
        };
    }
    // A reading whose neighbours are too few takes the median weight.
    let mut fin: Vec<f64> = w.iter().copied().filter(|v| v.is_finite()).collect();
    let med = if fin.is_empty() {
        1.0
    } else {
        crate::dsp::median(&mut fin)
    };
    for v in w.iter_mut() {
        if !v.is_finite() {
            *v = med;
        }
    }
    let sum: f64 = w.iter().sum();
    w.iter_mut().for_each(|v| *v /= sum);
    w
}

/// The Lomb–Scargle periodogram of the finite readings of `g`, from four
/// readings' period to a third of the span, with its peaks named after
/// `wheels`. `None` with fewer than 20 readings or no band.
pub fn spectrum(
    g: &Grid,
    series: &'static str,
    unit: &'static str,
    wheels: &[Wheel],
) -> Option<GappySpectrum> {
    let (mut t, mut y, mut stretch) = (Vec::new(), Vec::new(), Vec::new());
    let mut k = 0;
    let mut prev_finite = false;
    for (i, &v) in g.y.iter().enumerate() {
        if v.is_finite() {
            if !prev_finite && !t.is_empty() {
                k += 1;
            }
            t.push(g.time(i));
            y.push(v);
            stretch.push(k);
        }
        prev_finite = v.is_finite();
    }
    let n = t.len();
    if n < 20 {
        return None;
    }
    let span = t[n - 1] - t[0];
    let (fmin, fmax) = (3.0 / span, 1.0 / (4.0 * g.step));
    if fmax <= fmin {
        return None;
    }
    let df = 1.0 / (OVERSAMPLE * span);
    let nf = ((fmax - fmin) / df).floor() as usize + 1;
    // Times from the first reading, so the phases stay small.
    let t: Vec<f64> = t.iter().map(|v| v - t[0]).collect();
    let w = weights(&y, &stretch);
    let power = gls(&t, &y, &w, fmin, df, nf);
    let win = window(&t, fmin, df, nf);
    let freq = |k: usize| fmin + k as f64 * df;
    // Local maxima, strongest first; a weaker one within two resolution
    // widths of a stronger one is its sidelobe, and is left out.
    let mut maxima: Vec<usize> = (1..nf.saturating_sub(1))
        .filter(|&k| power[k] >= power[k - 1] && power[k] > power[k + 1])
        .collect();
    maxima.sort_by(|&a, &b| power[b].total_cmp(&power[a]));
    let reach = (2.0 * OVERSAMPLE).round() as usize;
    let mut kept_peaks: Vec<usize> = Vec::new();
    for k in maxima {
        if kept_peaks.len() == MAX_PEAKS {
            break;
        }
        if kept_peaks.iter().all(|&j| j.abs_diff(k) > reach) {
            kept_peaks.push(k);
        }
    }
    let width = OVERSAMPLE.round() as usize;
    let peaks: Vec<Peak> = kept_peaks
        .into_iter()
        .map(|k| {
            let p = 1.0 / freq(k);
            let resolution = p * p / span;
            let wp = win[k.saturating_sub(width)..(k + width + 1).min(nf)]
                .iter()
                .copied()
                .fold(0.0, f64::max);
            Peak {
                period_s: p,
                power: power[k],
                false_alarm: false_alarm(power[k], n, fmax, &t, &w),
                window_power: wp,
                on_window_line: wp >= WINDOW_LINE,
                wheel: wheels
                    .iter()
                    .find(|wh| (p - wh.period_s).abs() <= resolution.max(0.01 * wh.period_s))
                    .map(|wh| wh.name.clone()),
            }
        })
        .collect();
    // Thin to MAX_POINTS slices of log period, keeping each slice's
    // strongest point; longest period first.
    let (lmin, lmax) = ((1.0 / fmax).ln(), (1.0 / fmin).ln());
    let mut best: Vec<Option<usize>> = vec![None; MAX_POINTS];
    for k in 0..nf {
        let lp = (1.0 / freq(k)).ln();
        let b = (((lmax - lp) / (lmax - lmin)) * MAX_POINTS as f64) as usize;
        let b = b.min(MAX_POINTS - 1);
        if best[b].is_none_or(|j| power[k] > power[j]) {
            best[b] = Some(k);
        }
    }
    let kept: Vec<usize> = best.into_iter().flatten().collect();
    Some(GappySpectrum {
        series,
        unit,
        readings: n,
        span_s: span,
        period_s: kept.iter().map(|&k| 1.0 / freq(k)).collect(),
        power: kept.iter().map(|&k| power[k]).collect(),
        window: kept.iter().map(|&k| win[k]).collect(),
        peaks,
    })
}

/// The periodograms of a take's readings: the rate, the amplitude and the
/// beat error on the `reading_s` readings [`crate::steadiness::check`]
/// tests, each with its slow change taken off as the period search takes
/// it off.
pub fn for_take(
    log: &crate::stream::BeatLog,
    clock: Option<&crate::clock::ClockFit>,
    long: &crate::longrun::LongReport,
    lc: &crate::longterm::LongConfig,
    reading_s: f64,
) -> Vec<GappySpectrum> {
    let grids = crate::steadiness::reading_grids(log, clock, &long.breaks, reading_s);
    let names = [("rate", "s/d"), ("amplitude", "deg"), ("beat_error", "ms")];
    grids
        .iter()
        .zip(names)
        .filter_map(|(g, (name, unit))| {
            spectrum(
                &crate::longterm::detrend(g, lc.detrend_s),
                name,
                unit,
                &lc.wheels,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::Rng;

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

    #[test]
    fn a_line_is_found_across_gaps_and_named() {
        // 10 h of 10 s readings with a 30 s pause every 20 min and a
        // 2 h hole, a 600 s cycle under noise.
        let mut rng = Rng::new(1);
        let g = grid(10.0, 3600, |t| {
            if t % 1200.0 < 30.0 || (14400.0..21600.0).contains(&t) {
                f64::NAN
            } else {
                0.5 * (std::f64::consts::TAU * t / 600.0).sin() + rng.normal()
            }
        });
        let wheels = vec![Wheel {
            name: "third wheel".into(),
            period_s: 600.0,
        }];
        let s = spectrum(&g, "rate", "s/d", &wheels).unwrap();
        let top = &s.peaks[0];
        assert!(
            (top.period_s / 600.0 - 1.0).abs() < 0.01,
            "{}",
            top.period_s
        );
        assert!(top.false_alarm < 1e-6, "{}", top.false_alarm);
        assert_eq!(top.wheel.as_deref(), Some("third wheel"));
        assert!(!top.on_window_line);
        // The rest are noise.
        assert!(s.peaks[1].false_alarm > 0.01, "{:?}", s.peaks[1]);
    }

    #[test]
    fn noise_alone_gives_no_false_alarm_below_one_percent_most_runs() {
        let mut low = 0;
        for seed in 0..40 {
            let mut rng = Rng::new(100 + seed);
            let g = grid(10.0, 2000, |t| {
                if t % 1800.0 < 60.0 {
                    f64::NAN
                } else {
                    rng.normal()
                }
            });
            let s = spectrum(&g, "rate", "s/d", &[]).unwrap();
            low += (s.peaks[0].false_alarm < 0.01) as usize;
        }
        // About 1 in 100 by chance, and the bound is conservative.
        assert!(low <= 2, "{low} of 40");
    }

    #[test]
    fn recurring_gaps_put_a_line_in_the_window() {
        // A pause of 5 min every 30 min, and a level that differs a
        // little between the stretches: the gaps put a line at 30 min.
        let mut rng = Rng::new(9);
        let mut level = 0.0;
        let g = grid(10.0, 4320, |t| {
            if t % 1800.0 < 10.0 {
                level = rng.normal();
            }
            if t % 1800.0 < 300.0 {
                f64::NAN
            } else {
                level + 0.3 * rng.normal()
            }
        });
        let s = spectrum(&g, "rate", "s/d", &[]).unwrap();
        let near: Vec<&Peak> = s
            .peaks
            .iter()
            .filter(|p| (p.period_s / 1800.0 - 1.0).abs() < 0.02)
            .collect();
        assert!(near.iter().all(|p| p.on_window_line), "{:?}", s.peaks);
        let line = s
            .period_s
            .iter()
            .zip(&s.window)
            .filter(|(p, _)| (*p / 1800.0 - 1.0).abs() < 0.02)
            .map(|(_, w)| *w)
            .fold(0.0, f64::max);
        assert!(line > WINDOW_LINE, "{line}");
    }

    #[test]
    fn a_noisy_stretch_counts_for_less() {
        // A 300 s cycle in a quiet half, and noise ten times larger in the
        // other: weighted, the line stands; unweighted it would not.
        let mut rng = Rng::new(4);
        let g = grid(10.0, 4000, |t| {
            let noise = if t < 20000.0 { 0.5 } else { 5.0 };
            0.3 * (std::f64::consts::TAU * t / 300.0).sin() + noise * rng.normal()
        });
        let s = spectrum(&g, "rate", "s/d", &[]).unwrap();
        assert!(
            (s.peaks[0].period_s / 300.0 - 1.0).abs() < 0.01,
            "{:?}",
            s.peaks[0]
        );
        assert!(s.peaks[0].false_alarm < 1e-3, "{:?}", s.peaks[0]);
    }
}
