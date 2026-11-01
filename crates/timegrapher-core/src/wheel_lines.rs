//! The known wheels' lines, followed through a take.
//!
//! Two views of each wheel the calibre names, both on the watch's own time
//! (the beats counted, as `long` reads its timing and amplitude), so a
//! wheel's turn keeps one length however the rate moves:
//!
//! - **Line sizes against amplitude**: the size of a sine at the wheel's
//!   period, fitted by least squares in windows of ten turns (at least
//!   20 min) stepped half a window, each with its standard error, and
//!   gathered by the amplitude the window was read at. A line that only
//!   shows at low torque reads near zero at high amplitude and rises below
//!   some amplitude. With no line in it, a window reads a size near 1.25
//!   standard errors (the mean of a Rayleigh spread).
//! - **The turn's shape** (synchronous averaging): every whole turn of the
//!   wheel cut out and averaged phase bin by phase bin, over the take and
//!   over the turns read in each 20° of amplitude. Whatever is not locked
//!   to the wheel averages away; what is left is the wheel's own shape,
//!   so a bad tooth shows at one place in the turn.
//!
//! Neither reads across a gap or a mask: a window or a turn with a stretch
//! of more than 5 s missing is left out. Wheels whose turn is longer than
//! half the period search's detrending window are left out, since taking
//! off the slow change takes off most of their line.

use crate::longterm::Grid;
use crate::periodicity::Wheel;
use serde::Serialize;

/// Turns in a size window, and its shortest length, seconds.
const WINDOW_TURNS: f64 = 10.0;
const WINDOW_MIN_S: f64 = 1200.0;
/// Width of an amplitude bin for sizes, and of a band for turn shapes.
pub const SIZE_BIN_DEG: f64 = 10.0;
pub const TURN_BAND_DEG: f64 = 20.0;
/// Longest stretch of missing readings a window or turn may hold, seconds.
const MAX_HOLE_S: f64 = 5.0;
/// Share of readings a window or turn needs.
const MIN_SHARE: f64 = 0.9;
/// Turns a band's shape needs.
const MIN_TURNS: usize = 10;
/// Phase bins of a turn at most.
const MAX_BINS: usize = 60;

/// One window's fitted line.
#[derive(Debug, Clone, Serialize)]
pub struct LineWindow {
    pub start_s: f64,
    pub end_s: f64,
    /// Mean amplitude over the window, degrees.
    pub amplitude_deg: f64,
    /// Size of the sine (half its peak to peak) and its standard error, in
    /// the series' unit.
    pub size: f64,
    pub se: f64,
    /// Phase of the sine at the take's start, degrees: a line locked to
    /// the wheel keeps one phase from window to window.
    pub phase_deg: f64,
}

/// The windows read at amplitudes within one bin, combined.
#[derive(Debug, Clone, Serialize)]
pub struct LineBin {
    /// The bin's lower edge, degrees; the bin runs [`SIZE_BIN_DEG`] up.
    pub from_deg: f64,
    pub windows: usize,
    /// The windows' sizes, weighted by one over their standard error
    /// squared, and the standard error of that mean.
    pub size: f64,
    pub se: f64,
    /// For the rate: the same in s/d (a timing sine of size A ms at
    /// period P is a rate sine of 86.4 x 2 pi A / P s/d).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_s_per_day: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub se_s_per_day: Option<f64>,
}

/// One wheel's line in one series.
#[derive(Debug, Clone, Serialize)]
pub struct WheelLine {
    pub wheel: String,
    pub period_s: f64,
    /// `rate` (the beat timing, ms), `amplitude` (deg) or `beat_error` (ms).
    pub series: &'static str,
    pub unit: &'static str,
    pub window_s: f64,
    /// Lowest amplitude first.
    pub bins: Vec<LineBin>,
    /// Every window, with `--maps`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<LineWindow>>,
}

/// A wheel turn's mean shape over a set of turns.
#[derive(Debug, Clone, Serialize)]
pub struct TurnShape {
    /// The amplitude band, degrees; both null for every turn of the take.
    pub from_deg: Option<f64>,
    pub to_deg: Option<f64>,
    pub turns: usize,
    /// The mean per phase bin over the turns, from the turn's start, and
    /// its standard error (the spread between turns over their root count).
    pub profile: Vec<f64>,
    pub se: Vec<f64>,
    pub peak_to_peak: f64,
}

/// One wheel's turn, averaged, in one series.
#[derive(Debug, Clone, Serialize)]
pub struct WheelTurns {
    pub wheel: String,
    pub period_s: f64,
    /// `rate` (the beat timing, ms) or `amplitude` (deg).
    pub series: &'static str,
    pub unit: &'static str,
    pub bins: usize,
    pub all: TurnShape,
    /// Lowest amplitude first.
    pub by_amplitude: Vec<TurnShape>,
}

/// Whether `g.y[a..b]` is read well enough: at least [`MIN_SHARE`] of it,
/// with no hole longer than [`MAX_HOLE_S`].
fn read_enough(g: &Grid, a: usize, b: usize) -> bool {
    let max_run = (MAX_HOLE_S / g.step).floor() as usize;
    let (mut run, mut finite) = (0, 0);
    for v in &g.y[a..b] {
        if v.is_finite() {
            finite += 1;
            run = 0;
        } else {
            run += 1;
            if run > max_run {
                return false;
            }
        }
    }
    finite as f64 >= MIN_SHARE * (b - a) as f64
}

/// The mean of the finite values of `g` between times `a` and `b`.
fn mean_between(g: &Grid, a: f64, b: f64) -> Option<f64> {
    let i0 = (((a - g.t0) / g.step).floor().max(0.0)) as usize;
    let i1 = (((b - g.t0) / g.step).ceil().max(0.0) as usize).min(g.y.len());
    let v: Vec<f64> = (i0..i1).map(|i| g.y[i]).filter(|v| v.is_finite()).collect();
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

/// Least squares of `y = m + s t + a cos wt + b sin wt` over the finite
/// values of `g.y[i0..i1]`: `(a, b, residual sd, readings, lag-one
/// correlation of the residuals)`, the last pooled over the runs of
/// readings between missing ones ([`crate::confidence::pooled_lag_one`]),
/// negative or unknown taken as 0.
fn fit_line(g: &Grid, i0: usize, i1: usize, w: f64) -> Option<(f64, f64, f64, usize, f64)> {
    let tm = g.time((i0 + i1) / 2);
    let mut ata = [[0.0; 4]; 4];
    let mut aty = [0.0; 4];
    let mut rows = Vec::new();
    for i in i0..i1 {
        let y = g.y[i];
        if !y.is_finite() {
            continue;
        }
        let t = g.time(i);
        let x = [1.0, t - tm, (w * t).cos(), (w * t).sin()];
        for r in 0..4 {
            for c in 0..4 {
                ata[r][c] += x[r] * x[c];
            }
            aty[r] += x[r] * y;
        }
        rows.push((i, x, y));
    }
    let n = rows.len();
    if n < 8 {
        return None;
    }
    let p = solve4(ata, aty)?;
    let res: Vec<(usize, f64)> = rows
        .iter()
        .map(|(i, x, y)| {
            let f: f64 = x.iter().zip(&p).map(|(a, b)| a * b).sum();
            (*i, y - f)
        })
        .collect();
    let rss: f64 = res.iter().map(|(_, r)| r * r).sum();
    let mut runs: Vec<Vec<f64>> = Vec::new();
    for (k, &(i, r)) in res.iter().enumerate() {
        if k == 0 || i != res[k - 1].0 + 1 {
            runs.push(Vec::new());
        }
        runs.last_mut().expect("a run").push(r);
    }
    let rho = crate::confidence::pooled_lag_one(runs.iter().map(Vec::as_slice))
        .unwrap_or(0.0)
        .clamp(0.0, 0.99);
    Some((p[2], p[3], (rss / (n - 4) as f64).sqrt(), n, rho))
}

#[allow(clippy::needless_range_loop)]
fn solve4(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    for c in 0..4 {
        let piv = (c..4).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[piv][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, piv);
        b.swap(c, piv);
        for r in 0..4 {
            if r != c {
                let f = a[r][c] / a[c][c];
                for k in 0..4 {
                    a[r][k] -= f * a[c][k];
                }
                b[r] -= f * b[c];
            }
        }
    }
    Some([0, 1, 2, 3].map(|i| b[i] / a[i][i]))
}

/// The line of `wheel` in `g` (with its slow change taken off), window
/// by window, gathered by the amplitude `amp` read over each window.
/// `None` when the period is under four readings, or fewer than two
/// windows fit.
pub fn line(
    g: &Grid,
    amp: &Grid,
    wheel: &Wheel,
    series: &'static str,
    unit: &'static str,
    keep_windows: bool,
) -> Option<WheelLine> {
    let p = wheel.period_s;
    if p < 4.0 * g.step {
        return None;
    }
    let window_s = (WINDOW_TURNS * p).max(WINDOW_MIN_S);
    let nw = (window_s / g.step).round() as usize;
    if nw * 2 > g.y.len() {
        return None;
    }
    let w = std::f64::consts::TAU / p;
    let hop = (nw / 2).max(1);
    let mut windows = Vec::new();
    let mut start = 0;
    while start + nw <= g.y.len() {
        let (a, b) = (start, start + nw);
        start += hop;
        if !read_enough(g, a, b) {
            continue;
        }
        let (t0, t1) = (g.time(a) - g.step / 2.0, g.time(b - 1) + g.step / 2.0);
        let Some(amplitude_deg) = mean_between(amp, t0, t1) else {
            continue;
        };
        let Some((ca, sb, sd, n, rho)) = fit_line(g, a, b, w) else {
            continue;
        };
        windows.push(LineWindow {
            start_s: t0,
            end_s: t1,
            amplitude_deg,
            size: ca.hypot(sb),
            // Residuals that follow one another hold fewer independent
            // readings, by their strength at the line's own period.
            se: sd
                * (2.0 / crate::confidence::effective_readings_at(n as f64, rho, g.step / p))
                    .sqrt(),
            // a cos wt + b sin wt = R sin(wt + phi).
            phase_deg: ca.atan2(sb).to_degrees(),
        });
    }
    if windows.len() < 2 {
        return None;
    }
    let to_rate = (series == "rate").then(|| 86.4 * std::f64::consts::TAU / p);
    let mut bins: Vec<LineBin> = Vec::new();
    let mut edges: Vec<f64> = windows
        .iter()
        .map(|x| (x.amplitude_deg / SIZE_BIN_DEG).floor() * SIZE_BIN_DEG)
        .collect();
    edges.sort_by(|a, b| a.total_cmp(b));
    edges.dedup();
    for from in edges {
        let inside: Vec<&LineWindow> = windows
            .iter()
            .filter(|x| (x.amplitude_deg / SIZE_BIN_DEG).floor() * SIZE_BIN_DEG == from)
            .filter(|x| x.se > 0.0)
            .collect();
        if inside.is_empty() {
            continue;
        }
        let wsum: f64 = inside.iter().map(|x| 1.0 / (x.se * x.se)).sum();
        let size = inside.iter().map(|x| x.size / (x.se * x.se)).sum::<f64>() / wsum;
        let se = weighted_mean_se(&inside, wsum);
        bins.push(LineBin {
            from_deg: from,
            windows: inside.len(),
            size,
            se,
            size_s_per_day: to_rate.map(|k| k * size),
            se_s_per_day: to_rate.map(|k| k * se),
        });
    }
    Some(WheelLine {
        wheel: wheel.name.clone(),
        period_s: p,
        series,
        unit,
        window_s: nw as f64 * g.step,
        bins,
        windows: keep_windows.then_some(windows),
    })
}

/// The standard error of the windows' weighted mean (weights one over
/// their SE squared, summing to `wsum`). Windows that overlap share
/// readings, so their errors move together: two windows sharing a
/// fraction f of their length are taken to correlate at f, as a fit on
/// noise that does not follow itself would, and that covariance is
/// carried into the mean instead of treating the windows as independent.
fn weighted_mean_se(inside: &[&LineWindow], wsum: f64) -> f64 {
    let mut shared = 0.0;
    for (i, a) in inside.iter().enumerate() {
        for b in &inside[i + 1..] {
            let len = (a.end_s - a.start_s).max(b.end_s - b.start_s);
            let f = ((a.end_s.min(b.end_s) - a.start_s.max(b.start_s)) / len).max(0.0);
            shared += 2.0 * f / (a.se * b.se);
        }
    }
    (wsum + shared).sqrt() / wsum
}

/// The mean turn of `wheel` in `g`, over every whole turn read well
/// enough, and over the turns in each [`TURN_BAND_DEG`] of the amplitude
/// `amp` read over the turn. `None` when the period is under four
/// readings or fewer than [`MIN_TURNS`] turns are read.
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
pub fn turns(
    g: &Grid,
    amp: &Grid,
    wheel: &Wheel,
    series: &'static str,
    unit: &'static str,
) -> Option<WheelTurns> {
    let p = wheel.period_s;
    let per_turn = p / g.step;
    if per_turn < 4.0 {
        return None;
    }
    let bins = (per_turn.floor() as usize).min(MAX_BINS);
    // Turns on the watch's time from zero, so every series and every take
    // of the same watch cut its turns at the same place.
    let first = (g.t0 / p).ceil() as i64;
    let last = ((g.t0 + g.y.len() as f64 * g.step) / p).floor() as i64;
    let mut rows: Vec<(f64, Vec<f64>)> = Vec::new();
    for k in first..last {
        let (ta, tb) = (k as f64 * p, (k + 1) as f64 * p);
        let a = (((ta - g.t0) / g.step).ceil().max(0.0)) as usize;
        let b = ((((tb - g.t0) / g.step).ceil().max(0.0)) as usize).min(g.y.len());
        if b <= a || !read_enough(g, a, b) {
            continue;
        }
        let Some(amplitude) = mean_between(amp, ta, tb) else {
            continue;
        };
        let (mut sum, mut n) = (vec![0.0; bins], vec![0usize; bins]);
        for i in a..b {
            let v = g.y[i];
            if v.is_finite() {
                let phase = (g.time(i) - ta) / p;
                let j = ((phase * bins as f64) as usize).min(bins - 1);
                sum[j] += v;
                n[j] += 1;
            }
        }
        if n.contains(&0) {
            continue;
        }
        let row: Vec<f64> = sum.iter().zip(&n).map(|(s, &c)| s / c as f64).collect();
        rows.push((amplitude, row));
    }
    let shape = |rows: &[&Vec<f64>], from: Option<f64>| -> Option<TurnShape> {
        let k = rows.len();
        if k < MIN_TURNS {
            return None;
        }
        // Each turn less its own mean, so a turn's level does not count.
        let centred: Vec<Vec<f64>> = rows
            .iter()
            .map(|r| {
                let m = r.iter().sum::<f64>() / r.len() as f64;
                r.iter().map(|v| v - m).collect()
            })
            .collect();
        let profile: Vec<f64> = (0..bins)
            .map(|j| centred.iter().map(|r| r[j]).sum::<f64>() / k as f64)
            .collect();
        let se = (0..bins)
            .map(|j| {
                let v = centred
                    .iter()
                    .map(|r| (r[j] - profile[j]).powi(2))
                    .sum::<f64>()
                    / (k - 1) as f64;
                (v / k as f64).sqrt()
            })
            .collect();
        let (lo, hi) = profile
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| {
                (a.min(v), b.max(v))
            });
        Some(TurnShape {
            from_deg: from,
            to_deg: from.map(|f| f + TURN_BAND_DEG),
            turns: k,
            profile,
            se,
            peak_to_peak: hi - lo,
        })
    };
    let all = shape(&rows.iter().map(|r| &r.1).collect::<Vec<_>>(), None)?;
    let mut bands: Vec<f64> = rows
        .iter()
        .map(|r| (r.0 / TURN_BAND_DEG).floor() * TURN_BAND_DEG)
        .collect();
    bands.sort_by(|a, b| a.total_cmp(b));
    bands.dedup();
    let by_amplitude = bands
        .into_iter()
        .filter_map(|from| {
            let inside: Vec<&Vec<f64>> = rows
                .iter()
                .filter(|r| (r.0 / TURN_BAND_DEG).floor() * TURN_BAND_DEG == from)
                .map(|r| &r.1)
                .collect();
            shape(&inside, Some(from))
        })
        .collect();
    Some(WheelTurns {
        wheel: wheel.name.clone(),
        period_s: p,
        series,
        unit,
        bins,
        all,
        by_amplitude,
    })
}

/// The wheels both views follow: those named, with a turn no longer than
/// half `detrend_s`.
fn followed(wheels: &[Wheel], detrend_s: f64) -> impl Iterator<Item = &Wheel> {
    wheels.iter().filter(move |w| w.period_s <= detrend_s / 2.0)
}

/// Every named wheel's line sizes against amplitude, in the beat timing
/// (ms) and the amplitude of `long`, and in the beat error. `keep_windows`
/// keeps each window.
pub fn lines_for_take(
    log: &crate::stream::BeatLog,
    clock: Option<&crate::clock::ClockFit>,
    long: &crate::longrun::LongReport,
    lc: &crate::longterm::LongConfig,
    keep_windows: bool,
) -> Vec<WheelLine> {
    let amp = &long.amplitude.grid;
    let timing = scaled(&long.timing.detrended, 1000.0);
    let [_, be] = crate::steadiness::window_grids(log, clock, &long.breaks);
    let be = crate::longterm::detrend(&be, lc.detrend_s);
    let mut out = Vec::new();
    for w in followed(&lc.wheels, lc.detrend_s) {
        out.extend(line(&timing, amp, w, "rate", "ms", keep_windows));
        out.extend(line(
            &long.amplitude.detrended,
            amp,
            w,
            "amplitude",
            "deg",
            keep_windows,
        ));
        out.extend(line(&be, amp, w, "beat_error", "ms", keep_windows));
    }
    out
}

/// Every named wheel's mean turn, in the beat timing (ms) and the
/// amplitude of `long`.
pub fn turns_for_take(
    long: &crate::longrun::LongReport,
    lc: &crate::longterm::LongConfig,
) -> Vec<WheelTurns> {
    let amp = &long.amplitude.grid;
    let timing = scaled(&long.timing.detrended, 1000.0);
    let mut out = Vec::new();
    for w in followed(&lc.wheels, lc.detrend_s) {
        out.extend(turns(&timing, amp, w, "rate", "ms"));
        out.extend(turns(&long.amplitude.detrended, amp, w, "amplitude", "deg"));
    }
    out
}

fn scaled(g: &Grid, k: f64) -> Grid {
    Grid {
        t0: g.t0,
        step: g.step,
        y: g.y.iter().map(|v| v * k).collect(),
    }
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

    fn fourth() -> Wheel {
        Wheel {
            name: "fourth wheel".into(),
            period_s: 60.0,
        }
    }

    /// 10 h of 2 s amplitude readings, falling from 300° to 200°.
    fn amp() -> Grid {
        grid(2.0, 18000, |t| 300.0 - 100.0 * t / 36000.0)
    }

    #[test]
    fn a_window_se_allows_for_noise_that_follows_itself() {
        // A 60 s line of 2 ms under noise whose neighbours correlate at
        // 0.8 (0.5 ms in size). One SE should be about the scatter of the
        // sizes from window to window, not a third of it.
        let a = amp();
        let mut rng = Rng::new(3);
        let mut e = 0.0;
        let k = (1.0f64 - 0.8 * 0.8).sqrt();
        let g = grid(1.0, 36000, |t| {
            e = 0.8 * e + 0.5 * k * rng.normal();
            2.0 * (std::f64::consts::TAU * t / 60.0).sin() + e
        });
        let l = line(&g, &a, &fourth(), "rate", "ms", true).unwrap();
        // Every other window, so none overlaps the next.
        let w: Vec<&LineWindow> = l.windows.as_ref().unwrap().iter().step_by(2).collect();
        assert!(w.len() >= 20, "{}", w.len());
        let mean = w.iter().map(|x| x.size).sum::<f64>() / w.len() as f64;
        let scatter =
            (w.iter().map(|x| (x.size - mean).powi(2)).sum::<f64>() / (w.len() - 1) as f64).sqrt();
        let se = w.iter().map(|x| x.se).sum::<f64>() / w.len() as f64;
        assert!(
            (0.6..1.7).contains(&(scatter / se)),
            "scatter {scatter} against se {se}"
        );
    }

    #[test]
    fn a_bin_se_allows_for_windows_that_overlap() {
        // A 10 ms line at 60 s under white noise of 1 ms, 2 h of 1 s
        // readings, so all 11 half-overlapping windows fall in one bin.
        // Over 300 takes the bin's size should land within one SE of the
        // mean size about 68% of the time; taking the windows as
        // independent gives about 53%.
        let a = grid(2.0, 3600, |_| 255.0);
        let (mut within, mut sq, mut se_sum) = (0, 0.0, 0.0);
        let runs = 300;
        let mut bins = Vec::new();
        for seed in 0..runs {
            let mut rng = Rng::new(1000 + seed);
            let g = grid(1.0, 7200, |t| {
                10.0 * (std::f64::consts::TAU * t / 60.0).sin() + rng.normal()
            });
            let l = line(&g, &a, &fourth(), "rate", "ms", false).unwrap();
            assert_eq!(l.bins.len(), 1);
            assert_eq!(l.bins[0].windows, 11);
            bins.push((l.bins[0].size, l.bins[0].se));
        }
        let mean = bins.iter().map(|b| b.0).sum::<f64>() / runs as f64;
        for &(size, se) in &bins {
            within += usize::from((size - mean).abs() <= se);
            sq += (size - mean).powi(2);
            se_sum += se;
        }
        let ratio = (sq / (runs - 1) as f64).sqrt() / (se_sum / runs as f64);
        assert!(
            (0.6..0.78).contains(&(within as f64 / runs as f64)),
            "{within} of {runs} within one SE"
        );
        assert!((0.85..1.2).contains(&ratio), "scatter over SE {ratio}");
    }

    #[test]
    fn a_line_that_shows_only_at_low_amplitude_rises_there() {
        // A 60 s line of 0.2 ms below 240° only, under noise of 0.5 ms.
        let a = amp();
        let mut rng = Rng::new(1);
        let g = grid(1.0, 36000, |t| {
            let deg = 300.0 - 100.0 * t / 36000.0;
            let line = if deg < 240.0 { 0.2 } else { 0.0 };
            line * (std::f64::consts::TAU * t / 60.0 + 1.0).sin() + 0.5 * rng.normal()
        });
        let l = line(&g, &a, &fourth(), "rate", "ms", true).unwrap();
        for b in &l.bins {
            if b.from_deg + SIZE_BIN_DEG <= 240.0 {
                assert!((b.size - 0.2).abs() < 3.0 * b.se + 0.01, "{b:?}");
            } else if b.from_deg >= 240.0 {
                // No line: a size of a few standard errors at most.
                assert!(b.size < 0.06, "{b:?}");
            }
        }
        // A locked line keeps its phase: 1 rad in, read as such.
        let low: Vec<&LineWindow> = l
            .windows
            .as_ref()
            .unwrap()
            .iter()
            .filter(|w| w.amplitude_deg < 235.0)
            .collect();
        assert!(low.len() > 5);
        for w in low {
            assert!((w.phase_deg - 1.0f64.to_degrees()).abs() < 20.0, "{w:?}");
        }
        // The rate's size in s/d: 86.4 x 2 pi x 0.2 / 60.
        let b = l.bins.first().unwrap();
        let k = 86.4 * std::f64::consts::TAU / 60.0;
        assert!((b.size_s_per_day.unwrap() - k * b.size).abs() < 1e-12);
    }

    #[test]
    fn a_window_across_a_gap_is_left_out() {
        let a = amp();
        let g = grid(1.0, 36000, |t| {
            if (10000.0..10600.0).contains(&t) {
                f64::NAN
            } else {
                (std::f64::consts::TAU * t / 60.0).sin()
            }
        });
        let l = line(&g, &a, &fourth(), "rate", "ms", true).unwrap();
        for w in l.windows.unwrap() {
            assert!(w.end_s <= 10000.0 || w.start_s >= 10600.0, "{w:?}");
        }
    }

    #[test]
    fn a_bad_tooth_shows_at_one_place_in_the_turn() {
        // Every turn of a 60 s wheel, a 2 s bump of 0.3 ms at 20 s in,
        // under noise of 0.5 ms; below 240° the bump doubles.
        let a = amp();
        let mut rng = Rng::new(2);
        let g = grid(1.0, 36000, |t| {
            let deg = 300.0 - 100.0 * t / 36000.0;
            let into = t % 60.0;
            let bump = if (20.0..22.0).contains(&into) {
                0.3
            } else {
                0.0
            };
            let k = if deg < 240.0 { 2.0 } else { 1.0 };
            k * bump + 0.5 * rng.normal()
        });
        let s = turns(&g, &a, &fourth(), "rate", "ms").unwrap();
        assert_eq!(s.bins, 60);
        let top = (0..60)
            .max_by(|&i, &j| s.all.profile[i].total_cmp(&s.all.profile[j]))
            .unwrap();
        assert!((20..22).contains(&top), "{top}");
        let high = s
            .by_amplitude
            .iter()
            .find(|b| b.from_deg == Some(260.0))
            .unwrap();
        let low = s
            .by_amplitude
            .iter()
            .find(|b| b.from_deg == Some(200.0))
            .unwrap();
        let at = |b: &TurnShape| (b.profile[20] + b.profile[21]) / 2.0;
        assert!((at(high) - 0.3).abs() < 0.1, "{}", at(high));
        assert!((at(low) - 0.6).abs() < 0.1, "{}", at(low));
    }

    #[test]
    fn noise_alone_leaves_a_flat_turn() {
        let a = amp();
        let mut rng = Rng::new(3);
        let g = grid(1.0, 36000, |_| rng.normal());
        let s = turns(&g, &a, &fourth(), "rate", "ms").unwrap();
        // 600 turns: each bin's mean within 4 standard errors of zero.
        for (p, e) in s.all.profile.iter().zip(&s.all.se) {
            assert!(p.abs() < 4.0 * e, "{p} {e}");
        }
    }
}
