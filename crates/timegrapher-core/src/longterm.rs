//! Long-horizon analysis: periodic changes in rate and amplitude over
//! hours or days, the signature of a gear-train fault.
//!
//! A damaged tooth or an eccentric wheel repeats once per turn of its
//! wheel, often as a short event (a dip for a few minutes every hour)
//! rather than a sinusoid. The search therefore scores each trial period
//! together with its harmonics:
//!
//! 1. Bin the series on a uniform grid and remove the slow trend
//!    (mainspring let-down, temperature) with a running median.
//! 2. Take the power spectrum and divide it by a local-median estimate of
//!    its noise level. The score uses an exponential, mean-1 reference for
//!    normalized noise power; whitening does not guarantee that model for
//!    arbitrary noise colour, gaps or nonstationarity.
//! 3. For each trial fundamental, sum the whitened power at its first 1, 2,
//!    4 and 8 harmonics. Independent exponential bins would sum to Gamma(K),
//!    the reference used to score each sum; the best of
//!    the four is the period's score. A pure sinusoid wins with K = 1, a
//!    short dip with K = 8, and a subharmonic (twice the true period) loses
//!    because half its harmonics are noise.
//! 4. Fold the series at the strongest period to get the shape of one
//!    cycle and a raster (one row per turn of the wheel), subtract that
//!    shape, and search again. The periods of a watch's wheels are integer
//!    ratios of each other, so without the subtraction one fault would show
//!    up at several periods.

use crate::dsp::median;
use crate::periodicity::{polyfit2, Wheel};
use realfft::RealFftPlanner;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct LongConfig {
    /// Running-median window for the trend, seconds. Periods much longer
    /// than this are attenuated.
    pub detrend_s: f64,
    /// Shortest and longest trial period, seconds. The longest defaults to
    /// a third of the run, so at least three cycles are seen.
    pub min_period_s: f64,
    pub max_period_s: Option<f64>,
    /// Report at most this many components per series.
    pub max_components: usize,
    /// Threshold for the model-based whole-search false-alarm approximation;
    /// not a calibrated false-positive rate for arbitrary input noise.
    pub false_alarm: f64,
    /// ...and when it explains at least this fraction of the detrended
    /// variance. On a very clean recording a sub-microsecond ripple can be
    /// statistically certain and still not worth reporting.
    pub min_explained: f64,
    pub wheels: Vec<Wheel>,
}

impl Default for LongConfig {
    fn default() -> Self {
        LongConfig {
            detrend_s: 1800.0,
            min_period_s: 3.0,
            max_period_s: None,
            max_components: 4,
            false_alarm: 0.01,
            min_explained: 0.005,
            wheels: Vec::new(),
        }
    }
}

/// A series on a uniform time grid; missing bins are NaN.
#[derive(Debug, Clone, Serialize)]
pub struct Grid {
    pub t0: f64,
    pub step: f64,
    pub y: Vec<f64>,
}

impl Grid {
    pub fn time(&self, i: usize) -> f64 {
        self.t0 + (i as f64 + 0.5) * self.step
    }
    pub fn span(&self) -> f64 {
        self.y.len() as f64 * self.step
    }
}

/// Median of `values` in bins of `step` seconds from `t0`, `n` bins.
pub fn grid_median(times: &[f64], values: &[f64], t0: f64, step: f64, n: usize) -> Grid {
    let mut bins: Vec<Vec<f64>> = vec![Vec::new(); n];
    for (&t, &v) in times.iter().zip(values) {
        let b = ((t - t0) / step).floor();
        if v.is_finite() && b >= 0.0 && (b as usize) < n {
            bins[b as usize].push(v);
        }
    }
    let y = bins
        .iter_mut()
        .map(|b| if b.is_empty() { f64::NAN } else { median(b) })
        .collect();
    Grid { t0, step, y }
}

/// Remove the slow trend. Runs shorter than 20 minutes get a quadratic
/// fit; longer ones a running median of `window_s` (at most a quarter of
/// the run), evaluated every 1/40 of the window and interpolated.
pub fn detrend(g: &Grid, window_s: f64) -> Grid {
    let n = g.y.len();
    let finite: Vec<usize> = (0..n).filter(|&i| g.y[i].is_finite()).collect();
    if finite.len() < 4 {
        return g.clone();
    }
    let span = g.span();
    let y = if span < 1200.0 {
        let t: Vec<f64> = finite.iter().map(|&i| g.time(i)).collect();
        let v: Vec<f64> = finite.iter().map(|&i| g.y[i]).collect();
        let c = polyfit2(&t, &v);
        (0..n)
            .map(|i| {
                let t = g.time(i);
                g.y[i] - (c[0] + c[1] * t + c[2] * t * t)
            })
            .collect()
    } else {
        let w = ((window_s.min(span / 4.0) / g.step).round() as usize).max(3);
        let half = w / 2;
        let stride = (w / 40).max(1);
        let mut knots: Vec<(f64, f64)> = Vec::new();
        let mut i = 0usize;
        loop {
            let a = i.saturating_sub(half);
            let b = (i + half + 1).min(n);
            let mut v: Vec<f64> = g.y[a..b]
                .iter()
                .copied()
                .filter(|x| x.is_finite())
                .collect();
            if !v.is_empty() {
                knots.push((i as f64, median(&mut v)));
            }
            if i >= n - 1 {
                break;
            }
            i = (i + stride).min(n - 1);
        }
        let mut k = 0;
        (0..n)
            .map(|i| {
                let x = i as f64;
                while k + 2 < knots.len() && knots[k + 1].0 < x {
                    k += 1;
                }
                let base = if knots.len() == 1 || x <= knots[0].0 {
                    knots[0].1
                } else if x >= knots[knots.len() - 1].0 {
                    knots[knots.len() - 1].1
                } else {
                    let (x0, y0) = knots[k];
                    let (x1, y1) = knots[k + 1];
                    y0 + (y1 - y0) * (x - x0) / (x1 - x0)
                };
                g.y[i] - base
            })
            .collect()
    };
    Grid { y, ..g.clone() }
}

/// log10 of the chance that a Gamma(k, 1) variable exceeds `x`:
/// `P = exp(-x) * sum_{i<k} x^i / i!`.
fn log10_gamma_tail(k: usize, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    // Sum the series in log space, largest term last.
    let mut terms = Vec::with_capacity(k);
    let mut lt = 0.0f64; // log(x^i / i!)
    for i in 0..k {
        if i > 0 {
            lt += x.ln() - (i as f64).ln();
        }
        terms.push(lt);
    }
    let m = terms.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let s: f64 = terms.iter().map(|t| (t - m).exp()).sum();
    ((m + s.ln() - x) / std::f64::consts::LN_10).min(0.0)
}

/// Whitened power spectrum of a grid (missing bins count as the mean).
struct Spectrum {
    /// Frequency step, Hz.
    df: f64,
    /// Zero-padding factor: `pad` bins per independent frequency.
    pad: usize,
    white: Vec<f64>,
}

fn spectrum(g: &Grid) -> Spectrum {
    let fin: Vec<f64> = g.y.iter().copied().filter(|v| v.is_finite()).collect();
    let mean = fin.iter().sum::<f64>() / fin.len().max(1) as f64;
    let pad = 4;
    let l = (g.y.len() * pad).next_power_of_two().max(16);
    let mut planner = RealFftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(l);
    let mut buf = vec![0.0; l];
    for (b, &v) in buf.iter_mut().zip(&g.y) {
        *b = if v.is_finite() { v - mean } else { 0.0 };
    }
    let mut spec = fwd.make_output_vec();
    fwd.process(&mut buf, &mut spec).expect("fft");
    let power: Vec<f64> = spec.iter().map(|c| c.norm_sqr()).collect();
    let m = power.len();

    // Local median over +-25% in frequency (at least 32 bins each side),
    // on log-spaced knots, interpolated. Exp(1) has median ln 2.
    let mut knots: Vec<(usize, f64)> = Vec::new();
    let mut j = 1usize;
    while j < m {
        let half = ((j as f64 * 0.25) as usize).max(32);
        let a = j.saturating_sub(half).max(1);
        let b = (j + half + 1).min(m);
        let mut w = power[a..b].to_vec();
        knots.push((j, median(&mut w) / std::f64::consts::LN_2));
        j = (j + 1).max((j as f64 * 1.05) as usize).max(j + 8);
    }
    if knots.last().map(|k| k.0) != Some(m - 1) && m > 1 {
        let a = (m - 32).max(1);
        let mut w = power[a..].to_vec();
        knots.push((m - 1, median(&mut w) / std::f64::consts::LN_2));
    }
    let mut white = vec![0.0; m];
    let mut k = 0;
    for (j, w) in white.iter_mut().enumerate().skip(1) {
        while k + 2 < knots.len() && knots[k + 1].0 < j {
            k += 1;
        }
        let level = if knots.len() < 2 || j <= knots[0].0 {
            knots.first().map_or(1.0, |k| k.1)
        } else {
            let (j0, y0) = knots[k];
            let (j1, y1) = knots[(k + 1).min(knots.len() - 1)];
            if j1 == j0 {
                y0
            } else {
                y0 + (y1 - y0) * (j - j0) as f64 / (j1 - j0) as f64
            }
        };
        *w = power[j] / level.max(1e-300);
    }
    Spectrum {
        df: 1.0 / (l as f64 * g.step),
        pad: l / g.y.len().max(1),
        white,
    }
}

/// The period search: false-alarm score of every trial period.
#[derive(Debug, Clone, Serialize)]
pub struct Search {
    pub period_s: Vec<f64>,
    /// -log10 of the chance of a score this high from noise at that one
    /// period (not corrected for the number of periods tried).
    pub score: Vec<f64>,
    /// Harmonics summed for the best score at each period.
    pub harmonics: Vec<u8>,
    /// Score a period needs to be reported (includes the trials correction).
    pub threshold: f64,
    /// Spectrum index of the first trial period.
    #[serde(skip)]
    first_j: usize,
}

impl Search {
    /// Index of the trial period at `m` times the frequency of trial `i`.
    fn multiple(&self, i: usize, m: usize) -> Option<usize> {
        let j = (self.first_j + i) * m;
        let k = j.checked_sub(self.first_j)?;
        (k < self.score.len()).then_some(k)
    }

    /// The strongest period, and its submultiples that also pass the
    /// threshold, shortest first.
    fn best(&self) -> Option<(usize, Vec<usize>)> {
        let i = (0..self.score.len()).max_by(|&a, &b| self.score[a].total_cmp(&self.score[b]))?;
        let subs = (2..=8)
            .rev()
            .filter_map(|m| self.multiple(i, m))
            .filter(|&k| self.score[k] >= self.threshold)
            .collect();
        Some((i, subs))
    }
}

const HARMONICS: [usize; 4] = [1, 2, 4, 8];
/// Whitened power the fundamental needs before its harmonics are summed
/// (a mean-1 exponential reference exceeds it about 5% of the time).
const FUNDAMENTAL_MIN: f64 = 3.0;

fn search(g: &Grid, min_p: f64, max_p: f64, false_alarm: f64, exclude: &[f64]) -> Search {
    let sp = spectrum(g);
    let jmin = ((1.0 / max_p) / sp.df).ceil().max(1.0) as usize;
    let jmax = ((1.0 / min_p) / sp.df).floor() as usize;
    let mut out = Search {
        period_s: Vec::new(),
        score: Vec::new(),
        harmonics: Vec::new(),
        threshold: 0.0,
        first_j: jmin,
    };
    for j in jmin..=jmax.min(sp.white.len() - 1) {
        let period = 1.0 / (j as f64 * sp.df);
        // Harmonics only count when the fundamental itself shows: otherwise
        // a line just below the shortest period searched would be reported
        // at twice its period.
        let fundamental_seen = sp.white[j] >= FUNDAMENTAL_MIN;
        let mut sum = 0.0;
        let mut best = (0.0f64, 1u8);
        for h in 1..=HARMONICS[HARMONICS.len() - 1] {
            // Only harmonics inside the searched band count: a strong line
            // below the shortest period (a 5 s escape wheel on a long run's
            // 8 s floor) would otherwise lift every period it divides.
            if h * j > jmax {
                break;
            }
            let Some(&w) = sp.white.get(h * j) else { break };
            sum += w;
            if HARMONICS.contains(&h) && (h == 1 || fundamental_seen) {
                let s = -log10_gamma_tail(h, sum);
                if s > best.0 {
                    best = (s, h as u8);
                }
            }
        }
        let excluded = exclude
            .iter()
            .any(|&p| (period - p).abs() < 3.0 * p * p / g.span());
        out.period_s.push(period);
        out.score.push(if excluded { 0.0 } else { best.0 });
        out.harmonics.push(best.1);
    }
    // Independent frequencies tried, times the harmonic counts.
    let trials =
        ((jmax.saturating_sub(jmin) + 1) as f64 / sp.pad as f64 * HARMONICS.len() as f64).max(1.0);
    out.threshold = trials.log10() - false_alarm.log10();
    out
}

/// The average shape of one cycle, and every cycle as a raster row.
#[derive(Debug, Clone, Serialize)]
pub struct Fold {
    pub period_s: f64,
    /// Median of the detrended series in each phase bin.
    pub profile: Vec<f64>,
    pub counts: Vec<usize>,
    /// One row per cycle, one column per phase bin; NaN where empty.
    pub raster: Vec<Vec<f64>>,
    /// Start time of the first row, seconds.
    pub t0: f64,
}

pub fn fold(g: &Grid, period: f64, bins: usize) -> Fold {
    let mut vals: Vec<Vec<f64>> = vec![Vec::new(); bins];
    let rows = (g.span() / period).ceil() as usize;
    let mut rs = vec![vec![0.0; bins]; rows.max(1)];
    let mut rc = vec![vec![0usize; bins]; rows.max(1)];
    for (i, &v) in g.y.iter().enumerate() {
        if !v.is_finite() {
            continue;
        }
        let (row, b) = phase_bin(g.time(i) - g.t0, period, bins);
        let row = row.min(rs.len() - 1);
        vals[b].push(v);
        rs[row][b] += v;
        rc[row][b] += 1;
    }
    let counts = vals.iter().map(|v| v.len()).collect();
    // Median, so an occasional bad reading cannot pose as part of the shape.
    let profile = vals.iter_mut().map(|v| median(v)).collect();
    let raster = rs
        .iter()
        .zip(&rc)
        .map(|(r, c)| {
            r.iter()
                .zip(c)
                .map(|(&s, &n)| if n > 0 { s / n as f64 } else { f64::NAN })
                .collect()
        })
        .collect();
    Fold {
        period_s: period,
        profile,
        counts,
        raster,
        t0: g.t0,
    }
}

/// A folded profile kept to its first `harmonics` Fourier harmonics, which
/// takes most of the noise out of a profile averaged over few cycles.
pub fn smooth_shape(profile: &[f64], harmonics: usize) -> Vec<f64> {
    let n = profile.len();
    let fin: Vec<(usize, f64)> = profile
        .iter()
        .copied()
        .enumerate()
        .filter(|p| p.1.is_finite())
        .collect();
    if fin.is_empty() {
        return profile.to_vec();
    }
    let mean = fin.iter().map(|p| p.1).sum::<f64>() / fin.len() as f64;
    let w = 2.0 * std::f64::consts::PI / n as f64;
    let mut out = vec![mean; n];
    for h in 1..=harmonics.min(n / 2) {
        let (mut c, mut s) = (0.0, 0.0);
        for &(i, v) in &fin {
            let (a, b) = (w * h as f64 * i as f64).sin_cos();
            c += (v - mean) * b;
            s += (v - mean) * a;
        }
        let scale = 2.0 / fin.len() as f64 / if 2 * h == n { 2.0 } else { 1.0 };
        for (i, o) in out.iter_mut().enumerate() {
            let (a, b) = (w * h as f64 * i as f64).sin_cos();
            *o += scale * (c * b + s * a);
        }
    }
    out
}

/// Phase bins for a fold: one per grid step, between 8 and 240. Fine
/// bins let the subtraction remove a sharp event cleanly, so its
/// harmonics don't come back as components of their own.
fn fold_bins(period: f64, step: f64) -> usize {
    ((period / step).round() as usize).clamp(8, 240)
}

/// A periodic component found in a series.
#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub period_s: f64,
    /// Nominal frequency-resolution scale expressed as a period difference
    /// (period squared over the run length), seconds; not a confidence interval.
    pub resolution_s: f64,
    /// -log10 of the model-based whole-search false-alarm approximation.
    /// A value of 2 corresponds to 1% under that reference model, not a
    /// calibrated probability for arbitrary watch noise or a fault diagnosis.
    pub significance: f64,
    /// Harmonics that gave the best score (1 = a sinusoid; more = a
    /// sharper, shorter event within each cycle).
    pub harmonics: u8,
    /// Fraction of the detrended series' variance the folded shape explains.
    pub explained: f64,
    /// Peak-to-peak size of the folded shape, in the series' units.
    pub peak_to_peak: f64,
    /// A wheel whose turn matches this period within the resolution (or 1%).
    pub wheel: Option<String>,
    /// The nearest wheel and how far off it is, percent.
    pub nearest_wheel: Option<(String, f64)>,
    /// The folded profile kept to the harmonics the search used.
    pub shape: Vec<f64>,
    pub fold: Fold,
    /// A longer cycle the search also found whose period is this one's
    /// times a whole number from 2 to 8, within both resolutions (or 1%),
    /// when that leaves only one whole number.
    pub harmonic_of: Option<Harmonic>,
}

/// A found cycle's period is `period_s / n`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Harmonic {
    pub period_s: f64,
    pub n: u8,
}

/// For each component, the found cycle it is a harmonic of: the smallest
/// n (2 to 8) whose n times its period lands on a longer component within
/// n of its resolutions plus the longer one's, or 1%. When that tolerance
/// reaches half this period, n and n + 1 could both fit, so none is given.
fn label_harmonics(components: &mut [Component]) {
    let found: Vec<(f64, f64)> = components
        .iter()
        .map(|c| (c.period_s, c.resolution_s))
        .collect();
    for c in components.iter_mut() {
        c.harmonic_of = (2..=8u8).find_map(|n| {
            let p = c.period_s * n as f64;
            found
                .iter()
                .filter(|&&(q, r)| {
                    let tol = (n as f64 * c.resolution_s + r).max(0.01 * q);
                    tol < c.period_s / 2.0 && (p - q).abs() <= tol
                })
                .min_by(|a, b| (p - a.0).abs().total_cmp(&(p - b.0).abs()))
                .map(|&(q, _)| Harmonic { period_s: q, n })
        });
    }
}

/// Everything found in one series.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesReport {
    pub grid: Grid,
    pub detrended: Grid,
    /// The first search, before any component was subtracted.
    pub search: Search,
    pub components: Vec<Component>,
}

fn variance(y: &[f64]) -> f64 {
    let v: Vec<f64> = y.iter().copied().filter(|x| x.is_finite()).collect();
    if v.len() < 2 {
        return 0.0;
    }
    let m = v.iter().sum::<f64>() / v.len() as f64;
    v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / v.len() as f64
}

/// Subtract a fold's profile from the series it was folded from.
fn subtract(g: &mut Grid, f: &Fold) {
    let bins = f.profile.len();
    for i in 0..g.y.len() {
        let (_, b) = phase_bin(g.time(i) - g.t0, f.period_s, bins);
        if g.y[i].is_finite() && f.profile[b].is_finite() {
            g.y[i] -= f.profile[b];
        }
    }
}

/// The cycle and phase bin of time `t` from the grid's start. A reading
/// whose phase falls on a bin edge (a 45 s period read every 10 s, in 9
/// bins) goes to the bin above it every time, rather than to either side
/// as rounding in the division happens to fall.
fn phase_bin(t: f64, period: f64, bins: usize) -> (usize, usize) {
    let q = (t / period * bins as f64 + 1e-6).floor().max(0.0) as usize;
    (q / bins, q % bins)
}

/// Variance explained by folding at `period` with the mean in each phase
/// bin (per point of the series): the least-squares measure, used to
/// compare trial periods.
fn explained_at(g: &Grid, period: f64, bins: usize) -> f64 {
    let mut sum = vec![0.0; bins];
    let mut n = vec![0usize; bins];
    for (i, &v) in g.y.iter().enumerate() {
        if v.is_finite() {
            let (_, b) = phase_bin(g.time(i) - g.t0, period, bins);
            sum[b] += v;
            n[b] += 1;
        }
    }
    let total: usize = n.iter().sum();
    let mean = sum.iter().sum::<f64>() / total.max(1) as f64;
    sum.iter()
        .zip(&n)
        .filter(|(_, &c)| c > 0)
        .map(|(&s, &c)| {
            let d = s / c as f64 - mean;
            c as f64 * d * d
        })
        .sum::<f64>()
        / total.max(1) as f64
}

pub fn analyse(g: &Grid, cfg: &LongConfig) -> SeriesReport {
    let detrended = detrend(g, cfg.detrend_s);
    let span = g.span();
    // At least one reading for each of the fold's 8 bins. With fewer, a
    // period that is a whole number of steps folds onto a few fixed phases,
    // and the slow wander the detrend leaves behind shows up as a cycle: a
    // 5 s escape wheel on a 1 s grid read 10.8 ms where the beats show
    // 0.5 ms.
    let min_p = cfg.min_period_s.max(8.0 * g.step);
    let max_p = cfg.max_period_s.unwrap_or(span / 3.0).min(span / 2.0);
    let total_var = variance(&detrended.y);
    let mut work = detrended.clone();
    let mut components = Vec::new();
    let mut first = None;
    let mut found: Vec<f64> = Vec::new();
    if max_p > min_p && total_var > 0.0 {
        for _ in 0..cfg.max_components {
            let s = search(&work, min_p, max_p, cfg.false_alarm, &found);
            let threshold = s.threshold;
            // A sinusoid at 60 s also lights up the harmonics of 120 s and
            // 180 s, which can sum to a higher score when another component
            // adds power at one of them. Take the shortest submultiple whose
            // fold explains nearly as much as the strongest period's.
            let pick = s.best().map(|(i, subs)| {
                let var = |p: f64| explained_at(&work, p, fold_bins(p, g.step));
                let top = var(s.period_s[i]);
                let k = subs
                    .into_iter()
                    .find(|&k| var(s.period_s[k]) >= 0.9 * top)
                    .unwrap_or(i);
                (s.period_s[k], s.score[k], s.harmonics[k])
            });
            if first.is_none() {
                first = Some(s);
            }
            let Some((period, score, harmonics)) = pick else {
                break;
            };
            if score < threshold {
                break;
            }
            found.push(period);
            let bins = fold_bins(period, g.step);
            // Refine the period to the fold that explains the most variance.
            let res = period * period / span;
            // Two passes, each over 41 trial periods across +-1 resolution
            // and then +-1/20 of it.
            let mut period = period;
            for width in [res, res / 20.0] {
                period = (-20..=20)
                    .map(|k| period + width * k as f64 / 20.0)
                    .max_by(|&a, &b| {
                        explained_at(&work, a, bins).total_cmp(&explained_at(&work, b, bins))
                    })
                    .expect("non-empty");
            }
            let explained = explained_at(&work, period, bins) / total_var;
            let f = fold(&work, period, bins);
            subtract(&mut work, &f);
            if explained < cfg.min_explained {
                continue;
            }
            let explained = explained.min(1.0);
            let shape = smooth_shape(&f.profile, harmonics as usize);
            let (lo, hi) = shape
                .iter()
                .filter(|v| v.is_finite())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| {
                    (a.min(v), b.max(v))
                });
            // The shape seen in the raster is from the series before subtraction.
            let f = fold(&detrended, period, bins);
            let resolution = period * period / span;
            let nearest = cfg
                .wheels
                .iter()
                .min_by(|a, b| {
                    (period / a.period_s - 1.0)
                        .abs()
                        .total_cmp(&(period / b.period_s - 1.0).abs())
                })
                .map(|w| (w.name.clone(), (period / w.period_s - 1.0) * 100.0));
            let wheel = cfg
                .wheels
                .iter()
                .find(|w| (period - w.period_s).abs() <= resolution.max(0.01 * w.period_s))
                .map(|w| w.name.clone());
            components.push(Component {
                period_s: period,
                resolution_s: resolution,
                significance: score - threshold + (1.0 / cfg.false_alarm).log10(),
                harmonics,
                explained,
                peak_to_peak: hi - lo,
                wheel,
                nearest_wheel: nearest,
                shape,
                fold: f,
                harmonic_of: None,
            });
        }
    }
    label_harmonics(&mut components);
    SeriesReport {
        grid: g.clone(),
        detrended,
        search: first.unwrap_or(Search {
            period_s: Vec::new(),
            score: Vec::new(),
            harmonics: Vec::new(),
            threshold: 0.0,
            first_j: 0,
        }),
        components,
    }
}

/// The rate (s/d) implied by a folded timing profile (seconds of offset
/// per phase bin): minus its slope, as a watch that gains runs early.
pub fn rate_profile(profile: &[f64], period_s: f64) -> Vec<f64> {
    let n = profile.len();
    let dt = period_s / n as f64;
    (0..n)
        .map(|i| {
            let a = profile[(i + n - 1) % n];
            let b = profile[(i + 1) % n];
            -(b - a) / (2.0 * dt) * 86400.0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phase_on_a_bin_edge_stays_in_one_bin() {
        // A 45 s cycle read every 10 s, in 9 bins: each reading's phase
        // (its time is the middle of its 10 s) is a whole number of ninths,
        // a bin edge. Each phase's readings all go to one bin, so every
        // bin holds one phase and its value.
        let g = Grid {
            t0: 0.0,
            step: 10.0,
            y: (0..360).map(|i| ((i * 10 + 5) % 45) as f64 / 5.0).collect(),
        };
        let f = fold(&g, 45.0, 9);
        assert_eq!(f.counts, vec![40; 9]);
        assert_eq!(f.profile, (0..9).map(|b| b as f64).collect::<Vec<_>>());
        assert!((explained_at(&g, 45.0, 9) - variance(&g.y)).abs() < 1e-9);
    }
    use crate::synth::Rng;

    fn series(n: usize, step: f64, f: impl Fn(f64) -> f64, noise: f64, seed: u64) -> Grid {
        let mut rng = Rng::new(seed);
        Grid {
            t0: 0.0,
            step,
            y: (0..n)
                .map(|i| f((i as f64 + 0.5) * step) + noise * rng.normal())
                .collect(),
        }
    }

    #[test]
    fn gamma_tail() {
        // k = 1: exp(-x).
        assert!((log10_gamma_tail(1, 2.0) - (-2.0 / std::f64::consts::LN_10)).abs() < 1e-9);
        // k = 2 at x = 1: 2/e.
        assert!((log10_gamma_tail(2, 1.0) - (2.0f64 / std::f64::consts::E).log10()).abs() < 1e-9);
    }

    #[test]
    fn noise_alone_finds_nothing() {
        let mut false_alarms = 0;
        for seed in 0..20 {
            let g = series(7200, 1.0, |_| 0.0, 1.0, seed);
            false_alarms += analyse(&g, &LongConfig::default()).components.len();
        }
        assert!(false_alarms <= 1, "{false_alarms} false alarms in 20 runs");
    }

    #[test]
    fn no_cycle_shorter_than_eight_readings() {
        // An escape wheel's 5 s on the 1 s grid of a long run folds onto
        // five fixed phases, too few for the fold's 8 bins: not searched.
        let g = series(
            40_000,
            1.0,
            |t| (2.0 * std::f64::consts::PI * t / 5.00003).sin(),
            0.5,
            7,
        );
        let r = analyse(&g, &LongConfig::default());
        assert!(r.components.iter().all(|c| c.period_s >= 8.0));
    }

    #[test]
    fn hourly_dip_in_a_day() {
        // A 4-minute dip of 1 unit once an hour; noise of SD 0.45 per 10 s,
        // which is 4 per beat at 28,800 bph.
        let dip = |t: f64| {
            if (t % 3600.0 - 1800.0).abs() < 120.0 {
                -1.0
            } else {
                0.0
            }
        };
        let g = series(86400 / 10, 10.0, dip, 0.45, 3);
        let r = analyse(
            &g,
            &LongConfig {
                wheels: crate::periodicity::standard_wheels(28800, 15),
                ..Default::default()
            },
        );
        let c = &r.components[0];
        assert!((c.period_s - 3600.0).abs() < 60.0, "{}", c.period_s);
        assert!(c.harmonics > 1);
        assert_eq!(c.wheel.as_deref(), Some("centre wheel"));
    }

    #[test]
    fn sinusoid_is_found_at_its_period_not_a_multiple() {
        let g = series(
            7200,
            1.0,
            |t| (2.0 * std::f64::consts::PI * t / 57.0).sin(),
            2.0,
            5,
        );
        let r = analyse(&g, &LongConfig::default());
        let c = &r.components[0];
        assert!((c.period_s - 57.0).abs() < 0.3, "{}", c.period_s);
        assert!((c.peak_to_peak - 2.0).abs() < 0.3, "{}", c.peak_to_peak);
    }

    #[test]
    fn a_line_at_a_whole_fraction_of_a_found_cycle_is_labelled() {
        // A strong 150 s line, found and taken off first, then a weaker
        // 600 s one: the 150 s line is its 4th harmonic.
        let tau = std::f64::consts::TAU;
        let g = series(
            14_400,
            1.0,
            |t| 2.0 * (tau * t / 150.0).sin() + 0.8 * (tau * t / 600.0).sin(),
            1.0,
            11,
        );
        let r = analyse(&g, &LongConfig::default());
        let near = |p: f64| {
            r.components
                .iter()
                .find(|c| (c.period_s / p - 1.0).abs() < 0.01)
                .unwrap_or_else(|| {
                    panic!(
                        "no {p} s in {:?}",
                        r.components.iter().map(|c| c.period_s).collect::<Vec<_>>()
                    )
                })
        };
        let h = near(150.0).harmonic_of.expect("labelled");
        assert_eq!(h.n, 4);
        assert!((h.period_s / 600.0 - 1.0).abs() < 0.01, "{}", h.period_s);
        assert!(near(600.0).harmonic_of.is_none());
    }

    #[test]
    fn two_wheels_are_separated() {
        // A 60 s sinusoid and a short dip every 450 s.
        let f = |t: f64| {
            (2.0 * std::f64::consts::PI * t / 60.0).sin()
                + if (t % 450.0) < 20.0 { -3.0 } else { 0.0 }
        };
        let g = series(7200, 1.0, f, 1.5, 9);
        let r = analyse(&g, &LongConfig::default());
        let mut periods: Vec<f64> = r.components.iter().map(|c| c.period_s).collect();
        periods.truncate(2);
        periods.sort_by(f64::total_cmp);
        assert!((periods[0] - 60.0).abs() < 0.5, "{periods:?}");
        assert!((periods[1] - 450.0).abs() < 20.0, "{periods:?}");
        // 450 s is 7.5 turns of 60 s, and over 2 h its resolution (28 s)
        // would also reach 7 turns: no whole number is clear, so neither
        // is labelled.
        assert!(r.components[..2].iter().all(|c| c.harmonic_of.is_none()));
    }

    #[test]
    fn rate_profile_of_a_ramp() {
        // Offset falling 0.1 ms per 10 s bin: gaining 0.864 s/d.
        let p: Vec<f64> = (0..10).map(|i| -(i as f64) * 1e-4).collect();
        let r = rate_profile(&p, 100.0);
        assert!((r[5] - 1e-4 / 10.0 * 86400.0).abs() < 1e-9);
    }
}
