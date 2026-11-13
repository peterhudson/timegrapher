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
//! 2. Take the power spectrum and divide it by the noise it should hold.
//!    Under the running median (window W) that is a known shape: taking a
//!    moving average off leaves (1 − sinc(πfW))² of each frequency, and a
//!    median's extra scatter adds (π/2 − 1)·sinc² to the noise. It lifts
//!    noise about 1.5 times near a period of 0.7W, where the search read
//!    the detrend as cycles until #723; above a period of W it is left
//!    flat. An AR(1) noise's shape, with ρ read as everywhere else, goes
//!    on top, on short takes too (#753). What is left is levelled by a median over ±50% in
//!    frequency (±25% under the quadratic a short take is detrended with,
//!    without its lowest three bins).
//!    The score uses an exponential, mean-1 reference for
//!    normalized noise power; whitening does not guarantee that model for
//!    arbitrary noise colour, gaps or nonstationarity.
//! 3. For each trial fundamental, sum the whitened power at its first 1, 2,
//!    4 and 8 harmonics. Independent exponential bins would sum to Gamma(K),
//!    the reference used to score each sum; the best of
//!    the four is the period's score. A pure sinusoid wins with K = 1, a
//!    short dip with K = 8, and a subharmonic (twice the true period) loses
//!    because half its harmonics are noise.
//!    Those sums are not Gamma in practice (local levels, zero padding,
//!    the detrend's own shape), so the whitened spectrum is also divided
//!    by a simulated null's mean, and the best score is read against that
//!    null's maximum ([`null`], #1195): the threshold is where noise alone
//!    reaches it with the configured false-alarm chance.
//! 4. Fold the series at the strongest period to get the shape of one
//!    cycle and a raster (one row per turn of the wheel), subtract that
//!    shape, and search again. The periods of a watch's wheels are integer
//!    ratios of each other, so without the subtraction one fault would show
//!    up at several periods.

use crate::dsp::median;
use crate::periodicity::{polyfit2, Wheel};
use realfft::RealFftPlanner;
use serde::Serialize;

pub mod null;
pub use null::{NullFit, NullModel};

/// The shortest run detrended with a running median; shorter ones take a
/// quadratic.
pub const MEDIAN_FROM_S: f64 = 1200.0;

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

/// The slow change [`detrend`] takes out by default: a 30-minute running
/// median, longer than any cycle searched for.
pub const DETREND_S: f64 = 1800.0;

impl Default for LongConfig {
    fn default() -> Self {
        LongConfig {
            detrend_s: DETREND_S,
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

/// How [`detrend_as`] takes the slow change out under its running median.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slow {
    /// The window's median: for readings that wander about a level
    /// (amplitude, beat error).
    Level,
    /// For the beats' timing, which climbs or falls as the rate sits off
    /// the take's mean. On such a ramp a window holds its values in order,
    /// its median is its middle value, and the plain median took the
    /// cycles out with the ramp: a 5 s/d cycle on a timing that moves 1 ms
    /// each 4 s step (a rate 20 s/d off) never turns the ramp back. So
    /// each window's own line comes off first: its slope the median over
    /// the window of the rise across half of it, per step, and its level
    /// the median of what is left. That leaves the ramp's bend (a rate
    /// that changes within the window, tens of ms on a 2 h take), which
    /// wanders about a level, and the plain running median then takes it
    /// off: so what is left of a cycle is the same with the ramp as
    /// without.
    Sloped,
}

/// The longest period, in windows, that the sloped running median
/// ([`Slow::Sloped`]) leaves at least half of, so the timing's search ends
/// there (Norm: a band ends where its detrend's response falls to one
/// half; past it a line is mostly the detrend's own shape). Measured on a
/// sine of 6 h at 0.5 s, with and without a ramp and white noise of 0.3
/// of the sine, the same with the ramp as without: 1.00 of it is left up
/// to half a window, 1.4 to 1.7 near 0.7 windows (the plain median's
/// 1.3 to 1.4 there), 0.92 to 0.93 at one, 0.53 to 0.56 at 1.2 and 0.36
/// to 0.39 at 1.33.
pub const SLOPED_REACH: f64 = 1.2;

/// Remove the slow trend. Runs shorter than 20 minutes get a quadratic
/// fit; longer ones a running median of `window_s` (at most a quarter of
/// the run), evaluated every 1/40 of the window and interpolated.
pub fn detrend(g: &Grid, window_s: f64) -> Grid {
    detrend_as(g, window_s, Slow::Level)
}

/// [`detrend`], its running median as `slow` has it.
pub fn detrend_as(g: &Grid, window_s: f64, slow: Slow) -> Grid {
    let n = g.y.len();
    let finite: Vec<usize> = (0..n).filter(|&i| g.y[i].is_finite()).collect();
    let plan = slow_out(g, window_s);
    if matches!(plan, SlowOut::Nothing) {
        return g.clone();
    }
    let y = if let SlowOut::Median(w) = plan {
        running_median_off(&g.y, w, slow)
    } else {
        let t: Vec<f64> = finite.iter().map(|&i| g.time(i)).collect();
        let v: Vec<f64> = finite.iter().map(|&i| g.y[i]).collect();
        let c = polyfit2(&t, &v);
        (0..n)
            .map(|i| {
                let t = g.time(i);
                g.y[i] - (c[0] + c[1] * t + c[2] * t * t)
            })
            .collect()
    };
    Grid { y, ..g.clone() }
}

/// [`detrend_as`] on each piece of `g` on its own, a piece running from
/// each of `starts` (grid indices, ascending) to the next: no window
/// reaches across a break, where the beats' timing starts again from its
/// own fit. Every piece takes the whole take's window, so the search's
/// noise model holds on each; a piece shorter than one window is left
/// out (NaN). With no `starts` it is [`detrend_as`].
pub fn detrend_pieces(g: &Grid, window_s: f64, slow: Slow, starts: &[usize]) -> Grid {
    let n = g.y.len();
    let starts: Vec<usize> = starts.iter().copied().filter(|&i| i > 0 && i < n).collect();
    let (true, Some(w)) = (!starts.is_empty(), detrend_window(g, window_s)) else {
        return detrend_as(g, window_s, slow);
    };
    let mut y = vec![f64::NAN; n];
    let bounds: Vec<usize> = std::iter::once(0)
        .chain(starts)
        .chain(std::iter::once(n))
        .collect();
    for p in bounds.windows(2) {
        let (a, b) = (p[0], p[1]);
        let finite = g.y[a..b].iter().filter(|v| v.is_finite()).count();
        if b - a < w || finite < 4 {
            continue;
        }
        y[a..b].copy_from_slice(&running_median_off(&g.y[a..b], w, slow));
    }
    Grid { y, ..g.clone() }
}

/// `y` less its running median over `w` steps ([`Slow`]).
fn running_median_off(y: &[f64], w: usize, slow: Slow) -> Vec<f64> {
    match slow {
        Slow::Level => median_off(y, w, false),
        Slow::Sloped => median_off(&median_off(y, w, true), w, false),
    }
}

/// `y` less its running median over `w` steps, read every 1/40 of the
/// window and interpolated between. `sloped`: each window's own line
/// (its slope the median rise across half the window, per step) comes off
/// before its median, and between two reads the base is the cubic through
/// both reads with their slopes: straight lines between reads 1/40 of a
/// window apart left a bending timing scalloped at that spacing, a 45 s
/// line of 4.4 µs on a rate drifting ±40 s/d under a 30 min window. The
/// plain pass keeps straight lines: the scallop shows only where the bend
/// over one read spacing stands above the noise, which an amplitude's or
/// a beat error's bend never does (a 300° to 250° fall names nothing with
/// 0.05° of noise).
fn median_off(y: &[f64], w: usize, sloped: bool) -> Vec<f64> {
    let n = y.len();
    let half = w / 2;
    let stride = (w / 40).max(1);
    let mut knots: Vec<(f64, f64, f64)> = Vec::new();
    let mut i = 0usize;
    loop {
        let a = i.saturating_sub(half);
        let b = (i + half + 1).min(n);
        let slope = match sloped {
            false => 0.0,
            true => {
                let lag = ((b - a) / 2).max(1);
                let lagf = lag as f64;
                let mut d: Vec<f64> = (a..b.saturating_sub(lag))
                    .map(|k| (y[k + lag] - y[k]) / lagf)
                    .filter(|x| x.is_finite())
                    .collect();
                if d.is_empty() {
                    0.0
                } else {
                    median(&mut d)
                }
            }
        };
        let mut v: Vec<f64> = y[a..b]
            .iter()
            .enumerate()
            .map(|(k, &x)| x - slope * ((a + k) as f64 - i as f64))
            .filter(|x| x.is_finite())
            .collect();
        if !v.is_empty() {
            knots.push((i as f64, median(&mut v), slope));
        }
        if i >= n - 1 {
            break;
        }
        i = (i + stride).min(n - 1);
    }
    if knots.is_empty() {
        return vec![f64::NAN; n];
    }
    let mut k = 0;
    (0..n)
        .map(|i| {
            let x = i as f64;
            while k + 2 < knots.len() && knots[k + 1].0 < x {
                k += 1;
            }
            let line = |(x0, y0, s0): (f64, f64, f64)| y0 + s0 * (x - x0);
            let base = if knots.len() == 1 || x <= knots[0].0 {
                line(knots[0])
            } else if x >= knots[knots.len() - 1].0 {
                line(knots[knots.len() - 1])
            } else {
                let (a, b) = (knots[k], knots[k + 1]);
                let (d, t) = (b.0 - a.0, (x - a.0) / (b.0 - a.0));
                if sloped {
                    // Cubic Hermite through each read and its slope.
                    let (t2, t3) = (t * t, t * t * t);
                    (2.0 * t3 - 3.0 * t2 + 1.0) * a.1
                        + (t3 - 2.0 * t2 + t) * d * a.2
                        + (3.0 * t2 - 2.0 * t3) * b.1
                        + (t3 - t2) * d * b.2
                } else {
                    a.1 + (b.1 - a.1) * t
                }
            };
            y[i] - base
        })
        .collect()
}

/// The share of a noise's power at frequency `f` (Hz) left by a running
/// median of `w` seconds. A moving average of `w` passes sinc(πfw) of each
/// frequency, so taking it off leaves (1 − sinc)². A median of Gaussian
/// noise also scatters more than a mean, by π/2 − 1 of the mean's variance
/// (asymptotically), with the mean's low-pass shape: noise keeps that much
/// of sinc² on top. At f → 0 that leaves 0.57; the least, 0.36, is near
/// a period of 2w; near 0.7w it lifts noise about 1.5 times (#723).
/// For periods longer than w it is left at 1, so nothing is divided out
/// there: dividing by the model's 0.36 to 0.57 lifted slow bins above
/// their level and read a false 654 s line on a 30 min take (#755).
fn noise_pass(f: f64, w: f64) -> f64 {
    if f * w <= 1.0 {
        return 1.0;
    }
    let x = std::f64::consts::PI * f * w;
    let h = x.sin() / x;
    (1.0 - h).powi(2) + (std::f64::consts::FRAC_PI_2 - 1.0) * h * h
}

/// An AR(1) noise's power at `cycles` per step, relative to white.
fn ar1_shape(cycles: f64, rho: f64) -> f64 {
    1.0 / (1.0 - 2.0 * rho * (std::f64::consts::TAU * cycles).cos() + rho * rho)
}

/// Median of the power over bins `a..b` that `keep` keeps, as a mean
/// (an exponential's median is ln 2 of its mean); 1 if none is kept.
fn level(power: &[f64], a: usize, b: usize, keep: &dyn Fn(usize) -> bool) -> f64 {
    let mut w: Vec<f64> = (a..b).filter(|&k| keep(k)).map(|k| power[k]).collect();
    if w.is_empty() {
        return 1.0;
    }
    median(&mut w) / std::f64::consts::LN_2
}

/// What [`detrend`] takes out of `g`: one rule for it and for
/// [`detrend_fit_terms`].
enum SlowOut {
    /// Too few readings: nothing.
    Nothing,
    /// A least-squares quadratic, on a run under 20 minutes.
    Quadratic,
    /// A running median over this many steps.
    Median(usize),
}

fn slow_out(g: &Grid, window_s: f64) -> SlowOut {
    if g.y.iter().filter(|v| v.is_finite()).count() < 4 {
        return SlowOut::Nothing;
    }
    detrend_window(g, window_s).map_or(SlowOut::Quadratic, SlowOut::Median)
}

/// The terms of the polynomial [`detrend`] takes out of `g`: 3 (a
/// quadratic) on a run under 20 minutes, 0 with too few readings to take
/// any out, `None` where it takes a running median instead.
pub fn detrend_fit_terms(g: &Grid, window_s: f64) -> Option<usize> {
    match slow_out(g, window_s) {
        SlowOut::Nothing => Some(0),
        SlowOut::Quadratic => Some(3),
        SlowOut::Median(_) => None,
    }
}

/// The running median's window [`detrend`] uses on `g`, in whole steps (at
/// least 3): `window_s` or a quarter of the run, whichever is shorter;
/// `None` under 20 min, where it fits a quadratic instead.
pub(crate) fn detrend_window(g: &Grid, window_s: f64) -> Option<usize> {
    let span = g.span();
    (span >= MEDIAN_FROM_S).then(|| ((window_s.min(span / 4.0) / g.step).round() as usize).max(3))
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
    /// The plain power, before whitening.
    power: Vec<f64>,
    /// The lag-one ρ of the AR(1) noise it was whitened by.
    rho: f64,
}

fn spectrum(g: &Grid, detrend_w: Option<f64>) -> Spectrum {
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
    let df = 1.0 / (l as f64 * g.step);
    let pad = l / g.y.len().max(1);
    let power: Vec<f64> = spec.iter().map(|c| c.norm_sqr()).collect();
    let m = power.len();
    // The noise the search expects at each bin, up to a constant: an AR(1)
    // noise's shape, ρ read as everywhere else (one noise model), and
    // under a running median the detrend's own shape (below). Short takes
    // detrended with a quadratic take the AR(1) shape too: without it, 1
    // min of red noise at ρ 0.9 named a cycle in a quarter to a third of
    // takes (#753).
    let rho = crate::confidence::long_run_rho(&g.y, crate::confidence::Slow::Line);
    let shape: Vec<f64> = (0..m)
        .map(|j| {
            let f = j as f64 * df;
            ar1_shape(f * g.step, rho) * detrend_w.map_or(1.0, |w| noise_pass(f, w))
        })
        .collect();
    let flat: Vec<f64> = power.iter().zip(&shape).map(|(p, s)| p / s).collect();
    // What the flattened spectrum still leaves, by its median on log-spaced
    // knots, interpolated. Under a running median: over +-50% in frequency
    // (at least LEVEL_MIN bins each side). Leaving out the bins beside each
    // knot drew the level from farther out where it is lowest, and read
    // false slow lines on real takes (#755). Under a quadratic, which takes
    // power out of the lowest 3 independent bins: over +-25% (at least
    // SHORT_MIN bins), without those bins.
    let (share, least, floor) = match detrend_w {
        Some(_) => (0.5, LEVEL_MIN, 1),
        None => (0.25, SHORT_MIN, 3 * pad),
    };
    // Under a running median, a knot above the detrend's corner (f·w = 1)
    // levels only from bins above it: windows reaching below the corner,
    // where noise_pass is held at 1 while the detrend still takes power,
    // set the level too low and named false cycles at f·w 0.8 to 1.7
    // (#1030).
    let corner = detrend_w.map(|w| (1.0 / (w * df)) as usize);
    let level_at = |j: usize, a: usize, b: usize| {
        let a = corner.filter(|&c| j > c).map_or(a, |c| a.max(c + 1));
        level(&flat, a, b, &|k| k >= floor)
    };
    let mut knots: Vec<(usize, f64)> = Vec::new();
    let mut j = 1usize;
    while j < m {
        let half = ((j as f64 * share) as usize).max(least);
        let (a, b) = (j.saturating_sub(half).max(1), (j + half + 1).min(m));
        knots.push((j, level_at(j, a, b)));
        j = (j + 1).max((j as f64 * 1.05) as usize).max(j + 8);
    }
    if knots.last().map(|k| k.0) != Some(m - 1) && m > 1 {
        knots.push((m - 1, level_at(m - 1, m.saturating_sub(least).max(1), m)));
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
        *w = flat[j] / level.max(1e-300);
    }
    Spectrum {
        df,
        pad,
        white,
        power,
        rho,
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
    /// The null the search was levelled and counted against (#1195).
    pub null: NullFit,
    /// The trials count: independent frequencies times harmonic counts.
    pub trials: f64,
    /// How noise's maximum score falls: the table's, or the trials
    /// count's where the table doesn't reach.
    pub tail: null::Tail,
    /// The lag-one ρ the spectrum was whitened by.
    #[serde(skip)]
    pub rho: f64,
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
/// The fewest bins either side the noise level's median reads under a
/// running median, and under a quadratic.
const LEVEL_MIN: usize = 32;
const SHORT_MIN: usize = 32;
/// Whitened power the fundamental needs before its harmonics are summed
/// (a mean-1 exponential reference exceeds it about 5% of the time).
const FUNDAMENTAL_MIN: f64 = 3.0;

/// The band a search covers and the null it is held to.
struct Band<'a> {
    min_p: f64,
    max_p: f64,
    false_alarm: f64,
    detrend_w: Option<f64>,
    model: NullModel,
    /// A levelling to use instead of the table's: the table's own
    /// simulation.
    given: Option<&'a null::Levelling>,
}

/// f·w of spectrum bin `j`: the detrend's window, or the span under the
/// quadratic.
fn x_scale(g: &Grid, detrend_w: Option<f64>) -> f64 {
    detrend_w.unwrap_or_else(|| g.span())
}

/// Timing's mix and misfit (None where too few bands) from the spectrum `sp` of `g`, read from well
/// above the detrend's corner, where it takes no power.
/// `removed` and `thinned`: the lines already taken out of `g`, Hz, as
/// [`null::timing_fit_culled`] takes them.
fn timing_fit_of(
    g: &Grid,
    sp: &Spectrum,
    detrend_w: Option<f64>,
    removed: &[f64],
    thinned: &[f64],
) -> (f64, Option<f64>) {
    let scale = x_scale(g, detrend_w);
    let from_x = if detrend_w.is_some() { 2.0 } else { 6.0 };
    let from = (from_x / (sp.df * scale)).ceil() as usize;
    // Three independent frequencies either side of each line.
    let half = 3.0 * sp.pad as f64;
    let f = null::timing_fit_culled(
        &sp.power,
        sp.df,
        g.step,
        from,
        scale,
        &null::Cull {
            removed,
            thinned,
            half,
        },
    );
    (f.mix, f.misfit.is_finite().then_some(f.misfit))
}

/// The take's mix and the table's levelling for it, when the table holds
/// every bin the search reads.
fn null_for(
    g: &Grid,
    sp: &Spectrum,
    band: &Band,
    jmax: usize,
) -> (NullFit, Option<null::Levelling>) {
    let scale = x_scale(g, band.detrend_w);
    let (mix, misfit) = match band.model {
        NullModel::Readings => (sp.rho, None),
        NullModel::Timing => timing_fit_of(g, sp, band.detrend_w, &[], &[]),
    };
    let lev = match band.given {
        Some(l) => Some(l.clone()),
        None => null::levelling(band.model, g.span(), g.step, mix),
    };
    let jmin = ((1.0 / band.max_p) / sp.df).ceil().max(1.0) as usize;
    let lev = lev.filter(|l| (jmin..=jmax).all(|j| l.at(j as f64 * sp.df * scale).is_some()));
    let fit = NullFit {
        model: band.model,
        mix,
        calibrated: lev.is_some(),
        misfit,
        misfit_limit: lev.as_ref().and_then(|l| l.misfit_p99),
    };
    (fit, lev)
}

fn search(g: &Grid, band: &Band, exclude: &[f64]) -> Search {
    let mut sp = spectrum(g, band.detrend_w);
    let jmin = ((1.0 / band.max_p) / sp.df).ceil().max(1.0) as usize;
    let jmax = ((1.0 / band.min_p) / sp.df).floor() as usize;
    let jmax = jmax.min(sp.white.len() - 1);
    let (fit, lev) = null_for(g, &sp, band, jmax);
    if let Some(l) = &lev {
        let scale = x_scale(g, band.detrend_w);
        for j in jmin..=jmax {
            let x = j as f64 * sp.df * scale;
            if let Some(m) = l.at(x) {
                // Levelled, then read against the bin's own spread.
                sp.white[j] = l.as_exponential(x, sp.white[j] / m.max(1e-9));
            }
        }
    }
    let mut out = Search {
        period_s: Vec::new(),
        score: Vec::new(),
        harmonics: Vec::new(),
        threshold: 0.0,
        null: fit,
        trials: 1.0,
        tail: null::Tail::trials(1.0),
        rho: sp.rho,
        first_j: jmin,
    };
    for j in jmin..=jmax {
        let period = 1.0 / (j as f64 * sp.df);
        // Harmonics only count when the fundamental itself shows: otherwise
        // a line just below the shortest period searched would be reported
        // at twice its period.
        let fundamental_seen = sp.white[j] >= FUNDAMENTAL_MIN;
        let mut sum = 0.0;
        let mut best = (0.0f64, 1u8);
        for h in 1..=HARMONICS[HARMONICS.len() - 1] {
            // Only harmonics inside the searched band count: a strong line
            // below the shortest period (a 3.75 s escape wheel on a long
            // run's 4 s floor) would otherwise lift every period it divides.
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
    // Independent frequencies tried, times the harmonic counts; the table
    // turns that into the maximum's tail.
    let trials =
        ((jmax.saturating_sub(jmin) + 1) as f64 / sp.pad as f64 * HARMONICS.len() as f64).max(1.0);
    out.trials = trials;
    out.tail = lev.map_or(null::Tail::trials(trials), |l| l.tail(trials));
    out.threshold = out.tail.score_at(band.false_alarm);
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

/// A raster thinned for drawing ([`thin`]): its cells, and the rows and
/// columns of the raster each one averages, as `[first, end)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Thinned {
    pub cells: Vec<Vec<f64>>,
    pub rows: Vec<[usize; 2]>,
    pub columns: Vec<[usize; 2]>,
}

/// `[first, end)` of each of at most `max` near-equal, contiguous chunks
/// of `n`: chunk `i` starts at ⌊i·n/max⌋. One a chunk when `n` ≤ `max`.
pub(crate) fn chunks(n: usize, max: usize) -> Vec<[usize; 2]> {
    if n <= max || max == 0 {
        return (0..n).map(|i| [i, i + 1]).collect();
    }
    (0..max).map(|i| [i * n / max, (i + 1) * n / max]).collect()
}

/// `raster` (rows of cells, NaN where empty) thinned to at most
/// `max_rows` rows of at most `max_cols` columns, the one rule every
/// surface thins a fold's cycles by (#512): each cell is the mean of the
/// finite cells in its chunk of rows and columns, NaN when it has none.
/// Averaging keeps every cycle in the picture, where picking rows would
/// drop some and alias a cycle that comes every few. A raster within both
/// limits comes back as it is; it is never stretched.
pub fn thin(raster: &[Vec<f64>], max_rows: usize, max_cols: usize) -> Thinned {
    let width = raster.iter().map(Vec::len).max().unwrap_or(0);
    let rows = chunks(raster.len(), max_rows);
    let columns = chunks(width, max_cols);
    let cells = rows
        .iter()
        .map(|&[r0, r1]| {
            columns
                .iter()
                .map(|&[c0, c1]| {
                    crate::dsp::mean_of(
                        raster[r0..r1]
                            .iter()
                            .flat_map(|r| r.get(c0..c1.min(r.len())).unwrap_or(&[]))
                            .copied(),
                    )
                    .unwrap_or(f64::NAN)
                })
                .collect()
        })
        .collect();
    Thinned {
        cells,
        rows,
        columns,
    }
}

impl Fold {
    /// The fold's cycles thinned for drawing ([`thin`]).
    pub fn thinned(&self, max_rows: usize, max_cols: usize) -> Thinned {
        thin(&self.raster, max_rows, max_cols)
    }
}

/// A folded profile kept to its first `harmonics` Fourier harmonics, which
/// takes most of the noise out of a profile averaged over few cycles.
pub fn smooth_shape(profile: &[f64], harmonics: usize) -> Vec<f64> {
    sized_shape(profile, harmonics, &|_, _| true)
}

/// [`smooth_shape`], keeping beyond the fundamental only the harmonics
/// `keep(h, power)` passes, `power` the sum of the squares of harmonic
/// `h`'s two coefficients.
fn sized_shape(profile: &[f64], harmonics: usize, keep: &dyn Fn(usize, f64) -> bool) -> Vec<f64> {
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
        if h > 1 && !keep(h, scale * scale * (c * c + s * s)) {
            continue;
        }
        for (i, o) in out.iter_mut().enumerate() {
            let (a, b) = (w * h as f64 * i as f64).sin_cos();
            *o += scale * (c * b + s * a);
        }
    }
    out
}

/// Whether a harmonic of a fold at `period` stands out of the noise left
/// in `resid` (the series with the fold taken off): its power must pass
/// the 1% bar of the χ² with 2 degrees of freedom its two coefficients
/// would make in noise alone. That noise is the AR(1) noise's power at the
/// harmonic's frequency, ρ and σ read from `resid` (one noise model), each
/// coefficient's share 2/n of it, times π/2, the share a median's
/// coefficient would carry: kept since the shape became least-squares
/// sines, which makes the bar conservative. The
/// search can sum 8 harmonics to find a period while the shape drawn from
/// them is mostly noise: a 225 s rate cycle folded over 2.6 cycles swung
/// 29 s/d with 8 harmonics against 11 with the fundamental alone (#761).
/// [`analyse`] also keeps no more harmonics than whole cycles folded.
fn harmonic_shows(resid: &Grid, period: f64) -> impl Fn(usize, f64) -> bool {
    let rho = crate::confidence::long_run_rho(&resid.y, crate::confidence::Slow::Line);
    let n = resid.y.iter().filter(|v| v.is_finite()).count().max(1) as f64;
    let var = variance(&resid.y);
    let step = resid.step;
    move |h, power| {
        let noise = var * (1.0 - rho * rho) * ar1_shape(h as f64 * step / period, rho);
        power > 9.21 * 2.0 / n * std::f64::consts::FRAC_PI_2 * noise
    }
}

/// Phase bins for a fold: one per grid step, between 8 and 240, so a
/// sharp event keeps its shape in the profile and raster.
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
    /// Whether `significance` lies past [`SIMULATED_P`] on a search held
    /// to the simulated null: the number is then the fitted tail's
    /// extrapolation, and it is shown as beyond the simulated range
    /// ([`false_alarm_text`], #1243).
    pub beyond_simulated: bool,
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
    /// A longer cycle the search also found whose frequency times a whole
    /// number from 2 to 8 is within one resolution (1/span) of this one's.
    pub harmonic_of: Option<Harmonic>,
}

/// The smallest false-alarm chance the period search's simulated null was
/// checked to on fresh seeds (`fresh_seeds_hold_the_tail`: every model
/// and mix held its 99% bound at p 0.01, 0.001 and 0.0003). A smaller one
/// is the fitted extreme-value tail carried past its draws (400 a cell),
/// and is said as beyond the simulated range, not quoted (#1243).
pub const SIMULATED_P: f64 = 0.0003;

/// Whether a search's `significance` lies past [`SIMULATED_P`], when the
/// search was held to the simulated null (`calibrated`); the plain trials
/// count's tail is not a simulation, and keeps its number.
pub fn beyond_simulated(significance: f64, calibrated: bool) -> bool {
    calibrated && significance > -SIMULATED_P.log10()
}

/// A false-alarm chance as shown: `shown` of its −log10 within the
/// simulated range, and past it that it lies beyond (#1243).
pub fn false_alarm_text(significance: f64, beyond: bool, shown: impl Fn(f64) -> String) -> String {
    if beyond {
        format!("under {SIMULATED_P}, beyond the simulated range")
    } else {
        shown(significance)
    }
}

/// A found cycle's period is `period_s / n`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Harmonic {
    pub period_s: f64,
    pub n: u8,
}

/// How far a line of `period_s`, read at `resolution_s`, sits from the
/// `n`th harmonic of a longer period `of_s`, in resolutions (1/span) in
/// frequency. Under 1 means the line is that harmonic: the one test of a
/// whole fraction, for a found cycle and a wheel's turn alike (#775).
///
/// Within one resolution in frequency of n times the longer period's: the
/// refined periods are read far finer than that, so no n-fold allowance.
/// Before (n resolutions in period plus the fundamental's), 22.4 s
/// amplitude lines on 10 min were marked the 60 s fourth wheel's 3rd, 3.3
/// resolutions off (#762).
pub fn harmonic_offset(period_s: f64, resolution_s: f64, n: u8, of_s: f64) -> f64 {
    let f = 1.0 / period_s;
    (f - f64::from(n) / of_s).abs() / (resolution_s * f * f)
}

/// For each component not at a wheel's period, the found cycle it is a
/// harmonic of: the one n (2 to 8) and found cycle q for which n times q's
/// frequency is within one resolution (1/span) of its own ([`harmonic_offset`] under 1).
/// Neighbouring n are 1/q apart in frequency, more than the two
/// resolutions (2/span) the test spans when q < span/2, so then at most
/// one fits. A q nearer the span can fit two, and then no n is told, as
/// for a wheel's fraction (#789): over 580 s, 224.84 s is 0.43
/// resolutions from 2 turns of 540 s and 0.64 from 3.
fn label_harmonics(components: &mut [Component]) {
    let found: Vec<f64> = components.iter().map(|c| c.period_s).collect();
    for c in components.iter_mut() {
        // A line at a train wheel's period is that wheel's line, never a
        // harmonic of another: the 5 s escape wheel is a 30 s cycle's 6th
        // on DD3235, and labelled so it left the pane (Norm).
        if c.wheel.is_some() {
            c.harmonic_of = None;
            continue;
        }
        let (p, res) = (c.period_s, c.resolution_s);
        let mut fits = (2..=8u8).flat_map(|n| {
            found
                .iter()
                .copied()
                .filter(move |&q| harmonic_offset(p, res, n, q) < 1.0)
                .map(move |q| Harmonic { period_s: q, n })
        });
        let first = fits.next();
        c.harmonic_of = first.filter(|_| fits.next().is_none());
    }
}

/// Everything found in one series.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesReport {
    pub grid: Grid,
    pub detrended: Grid,
    /// Where each piece of `grid` after a break starts (grid indices):
    /// no detrend window reaches across one ([`detrend_pieces`]).
    #[serde(skip)]
    pub starts: Vec<usize>,
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

/// The period search over a series of readings (amplitude, beat error):
/// AR(1) noise is its null.
pub fn analyse(g: &Grid, cfg: &LongConfig) -> SeriesReport {
    analyse_as(g, cfg, NullModel::Readings, None, &[])
}

/// The period search over the beats' timing: random walk plus white noise
/// is its null.
pub fn analyse_timing(g: &Grid, cfg: &LongConfig) -> SeriesReport {
    analyse_timing_pieces(g, &[], cfg)
}

/// [`analyse_timing`] of a take whose timing starts again from its own fit
/// at each of `starts` (grid indices): each piece is detrended on its own
/// ([`detrend_pieces`]).
pub fn analyse_timing_pieces(g: &Grid, starts: &[usize], cfg: &LongConfig) -> SeriesReport {
    analyse_as(g, cfg, NullModel::Timing, None, starts)
}

/// The longest period `model`'s search reads, from `max_p`: under the
/// sloped running median of `detrend_w` seconds, no more than
/// [`SLOPED_REACH`] of its window.
pub(crate) fn reach(model: NullModel, max_p: f64, detrend_w: Option<f64>) -> f64 {
    match (slow_for(model), detrend_w) {
        (Slow::Sloped, Some(w)) => max_p.min(SLOPED_REACH * w),
        _ => max_p,
    }
}

/// [`analyse`] of a series whose pieces run from each of `starts` (grid
/// indices): no detrend window reaches across a break ([`detrend_pieces`]).
pub fn analyse_pieces(g: &Grid, starts: &[usize], cfg: &LongConfig) -> SeriesReport {
    analyse_as(g, cfg, NullModel::Readings, None, starts)
}

/// The first step of `g` at or after each of `cuts` (seconds on the grid's
/// clock): where each piece after a break starts.
pub fn piece_starts(g: &Grid, cuts: &[f64]) -> Vec<usize> {
    let mut out: Vec<usize> = cuts
        .iter()
        .map(|&c| ((c - g.t0) / g.step).ceil())
        .filter(|&i| i > 0.0 && i < g.y.len() as f64)
        .map(|i| i as usize)
        .collect();
    out.dedup();
    out
}

/// How each null model's series has its slow change taken out: the
/// timing climbs with the rate's offset, readings wander about a level.
pub fn slow_for(model: NullModel) -> Slow {
    match model {
        NullModel::Timing => Slow::Sloped,
        NullModel::Readings => Slow::Level,
    }
}

fn analyse_as(
    g: &Grid,
    cfg: &LongConfig,
    model: NullModel,
    given: Option<&null::Levelling>,
    starts: &[usize],
) -> SeriesReport {
    let detrended = detrend_pieces(g, cfg.detrend_s, slow_for(model), starts);
    let span = g.span();
    // At least one reading for each of the fold's 8 bins. With fewer, a
    // period that is a whole number of steps folds onto a few fixed phases,
    // and the slow wander the detrend leaves behind shows up as a cycle: a
    // 5 s escape wheel on a 1 s grid read 10.8 ms where the beats show
    // 0.5 ms.
    let min_p = cfg.min_period_s.max(8.0 * g.step);
    let max_p = cfg.max_period_s.unwrap_or(span / 3.0).min(span / 2.0);
    let detrend_w = detrend_window(g, cfg.detrend_s).map(|w| w as f64 * g.step);
    let max_p = reach(model, max_p, detrend_w);
    let total_var = variance(&detrended.y);
    let mut work = detrended.clone();
    let mut components = Vec::new();
    let mut first = None;
    let mut found: Vec<f64> = Vec::new();
    // Every line after the first is scored on the levelling the first
    // search read, at the mix (or ρ) fitted before anything was taken out.
    // The null is drawn without a subtraction, so a mix refitted on what a
    // strong line leaves reads whiter than the noise is, and the short
    // periods beside it score as lines: on a steady AR(1) rate a 306 s
    // wander's subtraction lifted an 11.8 s line to 16 (Norm).
    let mut kept: Option<null::Levelling> = None;
    if max_p > min_p && total_var > 0.0 {
        for _ in 0..cfg.max_components {
            let band = Band {
                min_p,
                max_p,
                false_alarm: cfg.false_alarm,
                detrend_w,
                model,
                given: given.or(kept.as_ref()),
            };
            let s = search(&work, &band, &found);
            let threshold = s.threshold;
            let s_tail = s.tail;
            let calibrated = s.null.calibrated;
            let pick = pick_period(&s, &work, g.step);
            if first.is_none() {
                if given.is_none() && s.null.calibrated {
                    kept = null::levelling(model, span, g.step, s.null.mix);
                }
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
            let period = refined(&work, period, bins, span);
            let explained = explained_at(&work, period, bins) / total_var;
            let shape;
            (work, shape) = take_out(&work, period, bins, harmonics, &cfg.wheels, detrend_w);
            if model == NullModel::Readings && explained < cfg.min_explained {
                continue;
            }
            let explained = explained.min(1.0);
            if model == NullModel::Timing && rate_swing(&shape, period) < MIN_RATE_SWING_S_PER_DAY {
                continue;
            }
            let (lo, hi) = shape
                .iter()
                .filter(|v| v.is_finite())
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &v| {
                    (a.min(v), b.max(v))
                });
            // The shape seen in the raster is from the series before subtraction.
            let f = fold(&detrended, period, bins);
            let resolution = period * period / span;
            let (wheel, nearest) = wheel_of(&cfg.wheels, period, resolution);
            components.push(Component {
                period_s: period,
                resolution_s: resolution,
                significance: s_tail.significance(score),
                beyond_simulated: beyond_simulated(s_tail.significance(score), calibrated),
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
    let mut search = first.unwrap_or_else(Search::none);
    // The misfit is read once the cycles found are taken out, so that a
    // strong line, which lifts its bands, is not read as the noise's shape.
    // Its limit is read at the mix fitted there too: the cycle's own line
    // lifts the mix the search fitted.
    if model == NullModel::Timing && !components.is_empty() {
        let (mix, misfit) = misfit_without(&detrended, &components, cfg);
        search.null.misfit = misfit;
        if given.is_none() && search.null.misfit_limit.is_some() {
            search.null.misfit_limit =
                null::levelling(model, span, g.step, mix).and_then(|l| l.misfit_p99);
        }
    }
    SeriesReport {
        grid: g.clone(),
        detrended,
        starts: starts.to_vec(),
        search,
        components,
    }
}

/// The timing mix and misfit of `detrended` with the cycles found taken
/// out as least-squares sinusoids, and the bands at and beside those lines
/// culled. The fold the search subtracts is a median over every phase
/// bin, which on a short take adds its own noise at every harmonic up to
/// the Nyquist frequency. Each cycle's kept harmonics are taken out, then
/// any further harmonic whose line still stands past what noise gives
/// once in [`EXTRA_LINE_P`]⁻¹ over the harmonics tried: a sharp cycle
/// (a few seconds in each minute) has more harmonics than the search
/// keeps, and left in they read as misfit.
fn misfit_without(
    detrended: &Grid,
    components: &[Component],
    cfg: &LongConfig,
) -> (f64, Option<f64>) {
    let span = detrended.span();
    let kept = |c: &Component| kept_harmonics(c.period_s, c.harmonics, span);
    let removed: Vec<f64> = components
        .iter()
        .flat_map(|c| cycle_lines(c.period_s, kept(c)))
        .collect();
    let w = detrend_window(detrended, cfg.detrend_s).map(|w| w as f64 * detrended.step);
    let removed = distinct_lines(removed, span);
    let rest = without_lines(detrended, &removed);
    let cycles: Vec<(f64, usize)> = components.iter().map(|c| (c.period_s, kept(c))).collect();
    let more = standing_harmonics(&spectrum(&rest, w), &cycles, detrended.step);
    let more = extra_lines(more, &removed, span);
    let rest = if more.is_empty() {
        rest
    } else {
        without_lines(detrended, &[removed.as_slice(), &more].concat())
    };
    timing_fit_of(&rest, &spectrum(&rest, w), w, &removed, &more)
}

/// A found cycle at `period` taken out of `work`, and its shape: its
/// kept harmonics ([`kept_harmonics`]) less any that is a wheel of the
/// train ([`wheel_harmonic`]), each beyond the first shown only where it
/// stands out of the noise. That noise is read with every harmonic the
/// search can sum taken out, so another line at one of them (a 40 s line
/// at a 240 s cycle's 6th) is not read as noise (Norm). The shape is the
/// least-squares sines the removal takes off, read at each phase bin's
/// middle, so one fit gives both the size and the removal. The removal
/// also takes the cycle's further harmonics that stand out of the
/// spectrum, as the misfit does ([`standing_harmonics`]), short of a
/// wheel's: a pulse of 8 s every minute left its 9th harmonic, 6.7 s, to
/// be named as a cycle of its own, where the fold's staircase took it.
fn take_out(
    work: &Grid,
    period: f64,
    bins: usize,
    harmonics: u8,
    wheels: &[Wheel],
    detrend_w: Option<f64>,
) -> (Grid, Vec<f64>) {
    let span = work.span();
    let kept = kept_harmonics(period, harmonics, span);
    let wheel_h = |h: usize| wheel_harmonic(wheels, period, h, span);
    let own: Vec<usize> = (1..=kept).filter(|&h| !wheel_h(h)).collect();
    let all: Vec<usize> = (1..=MAX_HARMONICS).collect();
    let shows = harmonic_shows(&without_cycle(work, period, &all), period);
    let periods: Vec<f64> = own.iter().map(|&h| period / h as f64).collect();
    let mean = crate::dsp::mean_of(work.y.iter().copied().filter(|v| v.is_finite()));
    let mut shape = vec![mean.unwrap_or(f64::NAN); bins];
    if let Some(b) = sines_fit(work, &periods) {
        for (j, &h) in own.iter().enumerate() {
            let (c, s) = (b[1 + 2 * j], b[2 + 2 * j]);
            if h > 1 && !shows(h, c * c + s * s) {
                continue;
            }
            for (i, o) in shape.iter_mut().enumerate() {
                let t = (i as f64 + 0.5) * period / bins as f64;
                let (sin, cos) = (std::f64::consts::TAU * t / periods[j]).sin_cos();
                *o += c * cos + s * sin;
            }
        }
    }
    let lines: Vec<f64> = own.iter().map(|&h| h as f64 / period).collect();
    let rest = without_lines(work, &lines);
    let more = standing_harmonics(&spectrum(&rest, detrend_w), &[(period, kept)], work.step);
    let more: Vec<f64> = extra_lines(more, &lines, span)
        .into_iter()
        .filter(|&f| !on_wheel(wheels, 1.0 / f, span))
        .collect();
    if more.is_empty() {
        (rest, shape)
    } else {
        (
            without_lines(work, &[lines.as_slice(), &more].concat()),
            shape,
        )
    }
}

/// The harmonics of a cycle at `period` the search sums (`harmonics`),
/// but no more than whole cycles folded: with fewer, a line near a
/// harmonic (the fourth wheel's 60 s by a 225 s cycle's 4th, on 10 min)
/// cannot be told from it.
fn kept_harmonics(period: f64, harmonics: u8, span: f64) -> usize {
    (harmonics as usize).min(((span / period) as usize).max(1))
}

/// The lines, Hz, of a cycle at `period` with its first `kept` harmonics.
fn cycle_lines(period: f64, kept: usize) -> impl Iterator<Item = f64> {
    (1..=kept).map(move |h| h as f64 / period)
}

/// `g` less a found cycle: its `harmonics` as least-squares sines
/// ([`without_lines`]), the one removal the search and the timing misfit
/// share. The fold is for the shape and raster only. Taken off as
/// a fold, a staircase of median per phase bin, a wander that slopes the
/// timing steeply within each bin leaves a sawtooth at the bin rate,
/// periodic at the cycle, whose sharp harmonics the next search named: on
/// a steady AR(1) rate (30 min, ρ 0.7, seed 23) a 306.6 s wander's fold
/// left 11.0 s at significance 19.1 and 11.8 s at 16.1; with sines the
/// best was 0.9 (Norm).
fn without_cycle(g: &Grid, period: f64, harmonics: &[usize]) -> Grid {
    let lines: Vec<f64> = harmonics.iter().map(|&h| h as f64 / period).collect();
    without_lines(g, &lines)
}

/// The most harmonics a search sums.
const MAX_HARMONICS: usize = HARMONICS[HARMONICS.len() - 1];

/// Whether harmonic `h` (above the first) of a cycle at `period` is a
/// wheel of the take's train ([`wheel_of`], its tolerance): then it is
/// that wheel's line, not part of the cycle's shape or size. A 30 s cycle
/// on DD3235's 30 min take carried the escape wheel's 5 s as its 6th
/// harmonic and read 20 s/d, where its own harmonics give 9 (Norm). With
/// no train, nothing is a wheel.
fn wheel_harmonic(wheels: &[Wheel], period: f64, h: usize, span: f64) -> bool {
    h > 1 && on_wheel(wheels, period / h as f64, span)
}

/// Whether `period` is a wheel of the train at a take of `span`
/// seconds: within [`wheel_of`]'s tolerance, one resolution (period²/span).
fn on_wheel(wheels: &[Wheel], period: f64, span: f64) -> bool {
    wheel_of(wheels, period, period * period / span).0.is_some()
}

/// `lines`, Hz, sorted, keeping one of any two within half a resolution
/// (1/`span`) of each other, which a joint fit can't tell apart: a 15 s
/// cycle's 4th harmonic and a 60 s cycle's 16th, say.
fn distinct_lines(mut lines: Vec<f64>, span: f64) -> Vec<f64> {
    lines.sort_by(f64::total_cmp);
    lines.dedup_by(|a, b| (*a - *b).abs() * span < 0.5);
    lines
}

/// The further lines `more`, Hz, kept distinct ([`distinct_lines`]) and
/// less any within half a resolution of a line in `removed`: fitted twice,
/// a line leaves the joint fit singular.
fn extra_lines(more: Vec<f64>, removed: &[f64], span: f64) -> Vec<f64> {
    distinct_lines(more, span)
        .into_iter()
        .filter(|&f| removed.iter().all(|&r| (f - r).abs() * span >= 0.5))
        .collect()
}

/// `g` less the least-squares fit of sines at `lines`, Hz, fitted jointly
/// with a mean ([`sines_fit`]); `g` itself where they can't be fitted.
fn without_lines(g: &Grid, lines: &[f64]) -> Grid {
    let periods: Vec<f64> = lines.iter().map(|f| 1.0 / f).collect();
    let mut out = g.clone();
    if let Some(b) = sines_fit(g, &periods) {
        for i in 0..out.y.len() {
            out.y[i] -= sines_at(&periods, &b, g.time(i) - g.t0) - b[0];
        }
    }
    out
}

/// The family-wise chance that noise alone lifts one of a cycle's
/// further harmonics past the bar [`standing_harmonics`] sets.
const EXTRA_LINE_P: f64 = 0.01;

/// The harmonics of each cycle `(period, kept)` past its `kept` ones, up
/// to the Nyquist frequency, whose power in `sp` stands past the level noise
/// passes with the chance [`EXTRA_LINE_P`] over all those tried: an
/// exponential's tail over the mean, read as the median over ±16
/// independent bins (the median of an exponential is its mean times ln 2).
fn standing_harmonics(sp: &Spectrum, cycles: &[(f64, usize)], step: f64) -> Vec<f64> {
    let span = 16 * sp.pad;
    let nyquist = 0.5 / step;
    let tried: Vec<f64> = cycles
        .iter()
        .flat_map(|&(p, kept)| {
            let top = (nyquist * p) as usize;
            (kept + 1..=top).map(move |h| h as f64 / p)
        })
        .collect();
    let bar = (tried.len().max(1) as f64 / EXTRA_LINE_P).ln();
    tried
        .into_iter()
        .filter(|&f| {
            let j = (f / sp.df).round() as usize;
            let (a, b) = (j.saturating_sub(span), (j + span + 1).min(sp.power.len()));
            if j >= sp.power.len() || b <= a {
                return false;
            }
            let mut near = sp.power[a..b].to_vec();
            near.sort_by(f64::total_cmp);
            let mean = near[near.len() / 2] / std::f64::consts::LN_2;
            sp.power[j] > bar * mean
        })
        .collect()
}

/// The terms a mean and a sine at each of `periods` take at time `t`:
/// 1, then the cosine and sine of each.
fn sines_row(periods: &[f64], t: f64) -> Vec<f64> {
    let mut r = vec![1.0; 1 + 2 * periods.len()];
    for (j, &q) in periods.iter().enumerate() {
        let (sin, cos) = (std::f64::consts::TAU * t / q).sin_cos();
        r[1 + 2 * j] = cos;
        r[2 + 2 * j] = sin;
    }
    r
}

/// The fit [`sines_fit`] gave, at time `t` from the grid's start.
pub fn sines_at(periods: &[f64], b: &[f64], t: f64) -> f64 {
    sines_row(periods, t)
        .iter()
        .zip(b)
        .map(|(r, b)| r * b)
        .sum()
}

/// Least squares fit of a mean and a sine at each of `periods`, jointly,
/// to the finite readings of `g` (time from its start): the mean, then the
/// cosine and sine coefficients of each. None with fewer than two
/// readings a term, or where the terms can't be told apart. The one sine
/// fit, for the steady pane's sizes and the misfit's lines alike.
pub fn sines_fit(g: &Grid, periods: &[f64]) -> Option<Vec<f64>> {
    let m = 1 + 2 * periods.len();
    let pts: Vec<(f64, f64)> =
        g.y.iter()
            .enumerate()
            .filter(|(_, v)| v.is_finite())
            .map(|(i, &v)| (g.time(i) - g.t0, v))
            .collect();
    let n = pts.len();
    if n < 2 * m {
        return None;
    }
    // Normal equations, solved by elimination with partial pivoting.
    let mut a = vec![vec![0.0; m + 1]; m];
    for &(t, v) in &pts {
        let r = sines_row(periods, t);
        for i in 0..m {
            for j in 0..m {
                a[i][j] += r[i] * r[j];
            }
            a[i][m] += r[i] * v;
        }
    }
    for col in 0..m {
        let piv = (col..m)
            .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))
            .unwrap_or(col);
        a.swap(col, piv);
        if a[col][col].abs() <= 1e-9 * n as f64 {
            return None;
        }
        let pivot = a[col].clone();
        for (i, r) in a.iter_mut().enumerate() {
            if i != col {
                let k = r[col] / pivot[col];
                for (x, p) in r.iter_mut().zip(&pivot).skip(col) {
                    *x -= k * p;
                }
            }
        }
    }
    Some((0..m).map(|i| a[i][m] / a[i][i]).collect())
}

/// A sinusoid at 60 s also lights up the harmonics of 120 s and
/// 180 s, which can sum to a higher score when another component
/// adds power at one of them. Take the shortest submultiple whose
/// fold explains nearly as much as the strongest period's.
/// Gives that period, its score and its harmonics; none without a best one.
fn pick_period(s: &Search, work: &Grid, step: f64) -> Option<(f64, f64, u8)> {
    s.best().map(|(i, subs)| {
        let var = |p: f64| explained_at(work, p, fold_bins(p, step));
        let top = var(s.period_s[i]);
        let k = subs
            .into_iter()
            .find(|&k| var(s.period_s[k]) >= 0.9 * top)
            .unwrap_or(i);
        (s.period_s[k], s.score[k], s.harmonics[k])
    })
}

/// The period near `period` whose fold in `bins` explains the most of
/// `work`'s variance.
fn refined(work: &Grid, period: f64, bins: usize, span: f64) -> f64 {
    let res = period * period / span;
    // Two passes, each over 41 trial periods across +-1 resolution
    // and then +-1/20 of it.
    let mut period = period;
    for width in [res, res / 20.0] {
        period = (-20..=20)
            .map(|k| period + width * k as f64 / 20.0)
            .max_by(|&a, &b| explained_at(work, a, bins).total_cmp(&explained_at(work, b, bins)))
            .expect("non-empty");
    }
    period
}

/// The wheel of `wheels` that `period` is, within `resolution`, and the
/// nearest one with how far off it is in percent.
fn wheel_of(
    wheels: &[Wheel],
    period: f64,
    resolution: f64,
) -> (Option<String>, Option<(String, f64)>) {
    let nearest = wheels
        .iter()
        .min_by(|a, b| {
            (period / a.period_s - 1.0)
                .abs()
                .total_cmp(&(period / b.period_s - 1.0).abs())
        })
        .map(|w| (w.name.clone(), (period / w.period_s - 1.0) * 100.0));
    let wheel = wheels
        .iter()
        .find(|w| (period - w.period_s).abs() <= resolution.max(0.01 * w.period_s))
        .map(|w| w.name.clone());
    (wheel, nearest)
}

impl Search {
    /// No search made: a series too short or too flat to search.
    fn none() -> Search {
        Search {
            period_s: Vec::new(),
            score: Vec::new(),
            harmonics: Vec::new(),
            threshold: 0.0,
            null: NullFit {
                model: NullModel::Readings,
                mix: f64::NAN,
                calibrated: false,
                misfit: None,
                misfit_limit: None,
            },
            trials: 1.0,
            tail: null::Tail::trials(1.0),
            rho: 0.0,
            first_j: 0,
        }
    }
}

/// One series' search crossed its threshold about 1 time in this many on
/// seeded white and AR(1) noise, the noise its model fits: 4, 4 and 5 of
/// 400 at 30 min for ρ 0, 0.5 and 0.9, and 3, 5 and 6 of 400 at 2 h (27
/// of 2400 in all, 24 expected), its design rate since the search is
/// held to its simulated null (#1195; 3 to 3.5% before, #916). Re-measured
/// by `crossing_counts_on_400_seeds`. Said in [`period_may_be_noise`]'s words and the Periods
/// threshold's hover.
pub const MEASURED_CROSSING_IN: u32 = 100;
/// …and up to 1 time in this many on 30 min takes with a slow wander under
/// white noise, lag one 0.8, the case [`period_may_be_noise`] names: 15 of
/// 400 at φ 0.98 a window, 10 at 0.95 (#1195; 9 to 10% before, #916).
pub const SHORT_CORRELATED_CROSSING_IN: u32 = 27;

/// Stretches shorter than this get [`period_may_be_noise`].
pub const NOISE_CAVEAT_SPAN_S: f64 = 7200.0;
/// Lag-one ρ from which [`period_may_be_noise`] is given.
pub const NOISE_CAVEAT_RHO: f64 = 0.7;

/// The caveat on a cycle the search named in `r`, an amplitude or beat
/// error series, when the stretch is shorter than 2 h and the ρ the search
/// whitened by is 0.7 or more. There a slow wander under white
/// measurement noise is not the AR(1) the search's null simulates. On
/// seeded 30 min nulls of 2 s windows (400 seeds a cell), a wander of φ
/// 0.98 per window under white noise named a cycle in 4.25% of takes, φ
/// 0.95 in 2.75%, against 1% on white and AR(1) noise; at 2 h, 0.5%
/// (#1195; 9 to 10% and 3 to 3.5% before the search was calibrated,
/// #916). The rate's searches are held to their own null, so its series
/// takes none. No figure, score or threshold changes.
pub fn period_may_be_noise(r: &SeriesReport) -> Option<crate::measure::Caveat> {
    let span_s = r.grid.span();
    let rho = r.search.rho;
    (!r.components.is_empty() && span_s < NOISE_CAVEAT_SPAN_S && rho >= NOISE_CAVEAT_RHO)
        .then_some(crate::measure::Caveat::PeriodMayBeNoise { rho, span_s })
}

/// The caveat on a cycle the search named in `r` when its simulated null
/// did not cover the take ([`null`], #1195).
pub fn period_uncalibrated(r: &SeriesReport) -> Option<crate::measure::Caveat> {
    let n = r.search.null;
    (!r.components.is_empty() && !n.calibrated).then_some(
        crate::measure::Caveat::PeriodSearchUncalibrated {
            model: n.model,
            mix: n.mix,
            span_s: r.grid.span(),
        },
    )
}

/// The caveat on a cycle the search named in `r` when the take's timing
/// sits further from the walk plus white fitted to it than the null's
/// 99th percentile for its cell ([`null::TimingFit::misfit`], #1218).
pub fn timing_misfit(r: &SeriesReport) -> Option<crate::measure::Caveat> {
    let n = r.search.null;
    let (misfit, limit) = (n.misfit?, n.misfit_limit?);
    (!r.components.is_empty() && misfit > limit)
        .then_some(crate::measure::Caveat::TimingModelMisfit { misfit, limit })
}

/// The rate (s/d) at a bin of timing offsets (seconds) `dt` apart: minus
/// the slope from the bin before to the bin after, as a watch that gains
/// runs early; with one of them missing, from the other and the bin
/// itself. A gap beside the bin is a gap in its rate.
fn rate_at(before: Option<f64>, here: f64, after: Option<f64>, dt: f64) -> f64 {
    let slope = match (before, after) {
        (Some(a), Some(b)) => (b - a) / (2.0 * dt),
        (Some(a), None) => (here - a) / dt,
        (None, Some(b)) => (b - here) / dt,
        (None, None) => f64::NAN,
    };
    -slope * 86400.0
}

/// The smallest rate swing (s/d, highest to lowest of the cycle) a line
/// on the timing must have to be named: half a unit of the rate's place
/// ([`crate::figure_text::base_digits::RATE`], 0.1 s/d), so a line named
/// never reads 0.0 s/d (Norm). It stands for the readings' share of
/// variance ([`LongConfig::min_explained`]): the timing is a walk, whose
/// power fills the band as 1/f², so a share of it is no measure of a
/// line (a 5 s escape wheel line of 0.5 s/d explains 0.006% of 2 h).
pub const MIN_RATE_SWING_S_PER_DAY: f64 = 0.05;

/// The rate swing (s/d) of a timing cycle's `shape` over `period_s`.
pub fn rate_swing(shape: &[f64], period_s: f64) -> f64 {
    let r = rate_profile(shape, period_s);
    let lo = r
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f64::INFINITY, f64::min);
    let hi = r
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);
    hi - lo
}

/// The rate (s/d) implied by a folded timing profile (seconds of offset
/// per phase bin) ([`rate_at`]): the average cycle, so its last bin's
/// neighbour after it is its first.
pub fn rate_profile(profile: &[f64], period_s: f64) -> Vec<f64> {
    let n = profile.len();
    let dt = period_s / n as f64;
    (0..n)
        .map(|i| {
            rate_at(
                Some(profile[(i + n - 1) % n]),
                profile[i],
                Some(profile[(i + 1) % n]),
                dt,
            )
        })
        .collect()
}

/// The rate (s/d) each cycle of a fold ran at through its period
/// ([`rate_at`]): `rows` are the cycles in time order, each one's timing
/// offsets, seconds per phase bin, as [`Fold::raster`] keeps them (not
/// rows averaged from them, whose ends are cycles apart). A cycle's last
/// bin is followed by the next cycle's first, and its first preceded by
/// the last cycle's last; the take's first and last bins have one
/// neighbour.
pub fn rate_rows(rows: &[Vec<f64>], period_s: f64) -> Vec<Vec<f64>> {
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let n = r.len();
            let dt = period_s / n as f64;
            let before = i.checked_sub(1).and_then(|k| rows[k].last().copied());
            let after = rows.get(i + 1).and_then(|r| r.first().copied());
            (0..n)
                .map(|j| {
                    let a = if j > 0 { Some(r[j - 1]) } else { before };
                    let b = if j + 1 < n { Some(r[j + 1]) } else { after };
                    rate_at(a, r[j], b, dt)
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
#[path = "longterm/pin_tests.rs"]
mod pin_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines within half a resolution merge before the joint fit: a 15 s
    /// cycle's 4th harmonic and a 60 s cycle's 16th are one line; a line a
    /// whole resolution off is not (#1299).
    #[test]
    fn lines_within_half_a_resolution_are_one() {
        let span = 600.0;
        assert_eq!(
            distinct_lines(vec![4.0 / 15.0, 16.0 / 60.0, 0.1], span).len(),
            2
        );
        let near = vec![0.2, 0.2 + 0.4 / span, 0.2 + 1.0 / span];
        assert_eq!(distinct_lines(near, span), vec![0.2, 0.2 + 1.0 / span]);
    }

    /// A further harmonic on a line already taken out is not fitted twice;
    /// one more than half a resolution off is (#1299).
    #[test]
    fn a_further_line_on_a_kept_one_is_dropped() {
        let (span, kept) = (600.0, 16.0 / 60.0);
        let more = extra_lines(vec![4.0 / 15.0, 0.5], &[kept + 0.1 / span], span);
        assert_eq!(more, vec![0.5]);
        assert_eq!(extra_lines(vec![kept + 0.6 / span], &[kept], span).len(), 1);
    }

    /// The bar a further harmonic must pass is the one noise passes once in
    /// [`EXTRA_LINE_P`]⁻¹ over those tried: on a flat spectrum a line at 10
    /// times the mean stands (the bar is ln(29/0.01), about 8), one at 6
    /// times does not, and none stands without a line (#1299).
    #[test]
    fn a_further_harmonic_stands_past_its_bar() {
        let tau = std::f64::consts::TAU;
        let g = series(600, 1.0, |t| (tau * t / 60.0).sin(), 0.1, 13);
        let c = Component {
            period_s: 60.0,
            harmonics: 1,
            ..analyse(&g, &LongConfig::default()).components.remove(0)
        };
        let mean = 1.0 / std::f64::consts::LN_2;
        let sp = |lines: &[(usize, f64)]| {
            let mut power = vec![1.0; 400];
            for &(h, x) in lines {
                power[10 * h] = x * mean;
            }
            Spectrum {
                df: 1.0 / 600.0,
                pad: 1,
                white: Vec::new(),
                power,
                rho: 0.0,
            }
        };
        let cs = [(c.period_s, c.harmonics as usize)];
        let found = standing_harmonics(&sp(&[(3, 10.0), (5, 6.0)]), &cs, 1.0);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!((found[0] - 3.0 / 60.0).abs() < 1e-12, "{found:?}");
        assert!(standing_harmonics(&sp(&[]), &cs, 1.0).is_empty());
    }

    /// A timing take with a cycle reads its misfit limit at the mix fitted
    /// once the cycle is out, not at the search's, which the cycle's line
    /// shifts: here 1.4 searched, 4.1 with the cycle out, whose limits
    /// differ (#1299).
    #[test]
    fn the_misfit_limit_is_read_at_the_residual_mix() {
        let (step, n) = (0.25, 21600);
        let mut z = crate::synth::Rng::new(37);
        let (mut walk, cfg) = (0.0, LongConfig::default());
        let y = (0..n)
            .map(|i| {
                walk += 5e-7 * z.normal();
                let t = i as f64 * step;
                walk + 1e-4 * z.normal() + if t % 60.0 < 8.0 { 6e-4 } else { 0.0 }
            })
            .collect();
        let g = Grid { t0: 0.0, step, y };
        let r = analyse_timing(&g, &cfg);
        assert!(!r.components.is_empty());
        let (mix, _) = misfit_without(&r.detrended, &r.components, &cfg);
        let at = |m: f64| {
            null::levelling(NullModel::Timing, g.span(), step, m).and_then(|l| l.misfit_p99)
        };
        let (searched, residual) = (at(r.search.null.mix), at(mix));
        assert!(residual.is_some());
        assert_ne!(searched, residual, "mixes {} and {mix}", r.search.null.mix);
        assert_eq!(r.search.null.misfit_limit, residual);
    }

    /// A strong line is flagged beyond the simulated range on a search held
    /// to the simulated null (2 s readings, which the table holds), and not
    /// on one read against the plain trials count (3 s readings, which it
    /// doesn't), however strong (#1243).
    #[test]
    fn only_a_calibrated_search_flags_a_line_beyond_its_range() {
        let tau = std::f64::consts::TAU;
        for (step, calibrated) in [(2.0, true), (3.0, false)] {
            let n = (1800.0 / step) as usize;
            let g = series(n, step, |t| 3.0 * (tau * t / 60.0).sin(), 0.5, 21);
            let r = analyse(&g, &LongConfig::default());
            assert_eq!(r.search.null.calibrated, calibrated, "step {step}");
            let c = &r.components[0];
            assert!(c.significance > 4.0, "step {step}: {}", c.significance);
            assert_eq!(c.beyond_simulated, calibrated, "step {step}");
        }
    }

    /// Past p 0.0003 a calibrated search's false alarm is said to lie
    /// beyond the simulated range; within it, or from the plain trials
    /// count, its number is shown (#1243).
    #[test]
    fn a_false_alarm_past_the_simulated_range_is_not_quoted() {
        assert!(!beyond_simulated(3.5, true));
        assert!(beyond_simulated(3.53, true));
        assert!(!beyond_simulated(87.0, false));
        let shown = |s: f64| format!("{:.0e}", 10f64.powf(-s));
        assert_eq!(false_alarm_text(3.0, false, shown), "1e-3");
        assert_eq!(
            false_alarm_text(87.0, true, shown),
            "under 0.0003, beyond the simulated range"
        );
    }

    /// Thinning averages the finite cells of near-equal chunks, leaves an
    /// empty chunk empty, and leaves a small raster as it is (#512).
    #[test]
    fn a_raster_is_thinned_by_averaging_every_cycle() {
        let nan = f64::NAN;
        // 5 cycles of 3 bins thinned to 2 rows of 2 columns: rows 0..2 and
        // 2..5, columns 0..1 and 1..3.
        let raster = vec![
            vec![1.0, 10.0, nan],
            vec![3.0, 20.0, nan],
            vec![5.0, nan, nan],
            vec![7.0, nan, nan],
            vec![9.0, nan, nan],
        ];
        let t = thin(&raster, 2, 2);
        assert_eq!(t.rows, vec![[0, 2], [2, 5]]);
        assert_eq!(t.columns, vec![[0, 1], [1, 3]]);
        assert_eq!(t.cells[0], vec![2.0, 15.0]);
        assert_eq!(t.cells[1][0], 7.0);
        assert!(t.cells[1][1].is_nan(), "an empty chunk stays empty");
        // Within the limits: the raster as it is, never stretched.
        let same = thin(&raster, 10, 3);
        assert_eq!(same.rows.len(), 5);
        assert_eq!(same.cells[1][..2], raster[1][..2]);
        assert!(same.cells[1][2].is_nan());
        // A cycle that comes every few rows is in every row's average.
        let every_fifth: Vec<Vec<f64>> = (0..1000)
            .map(|r| vec![if r % 5 == 0 { 5.0 } else { 0.0 }])
            .collect();
        let t = thin(&every_fifth, 200, 1);
        assert!(t.cells.iter().all(|c| c[0] == 1.0), "{:?}", &t.cells[..3]);
    }

    /// A take that gains at a steady 8.64 s/d (1e-4 of time) reads 8.64
    /// in every bin of every cycle, its first and last from their one
    /// neighbour; a gap stays a gap beside it.
    #[test]
    fn each_cycle_reads_its_own_rate() {
        let (period, n) = (100.0, 10);
        let dt = period / n as f64;
        let take = |f: &dyn Fn(f64) -> f64| -> Vec<Vec<f64>> {
            (0..3)
                .map(|k| (0..n).map(|j| f((k * n + j) as f64 * dt)).collect())
                .collect()
        };
        let rows = rate_rows(&take(&|t| -1e-4 * t), period);
        assert!(
            rows.iter().flatten().all(|v| (v - 8.64).abs() < 1e-9),
            "{rows:?}"
        );
        let rows = rate_rows(&take(&|t| 1e-4 * t), period);
        assert!(rows.iter().flatten().all(|v| (v + 8.64).abs() < 1e-9));
        let mut gap = take(&|t| -1e-4 * t);
        gap[1][4] = f64::NAN;
        let rows = rate_rows(&gap, period);
        assert!(rows[1][3].is_nan() && rows[1][5].is_nan());
        assert!((rows[1][4] - 8.64).abs() < 1e-9);
        // A gap at a cycle's start is beside the last cycle's last bin.
        gap[1][0] = f64::NAN;
        assert!(rate_rows(&gap, period)[0][n - 1].is_nan());
    }

    /// A cycle's ends read across to the cycles beside it, as its middle
    /// bins read across their neighbours: on a sine of the timing a cycle's
    /// last bin reads what the same bin of a longer row would.
    #[test]
    fn a_cycles_ends_read_across_to_the_next_cycle() {
        let (period, n) = (100.0, 10);
        let dt = period / n as f64;
        let f = |t: f64| 0.01 * (t / 37.0).sin();
        let rows: Vec<Vec<f64>> = (0..3)
            .map(|k| (0..n).map(|j| f((k * n + j) as f64 * dt)).collect())
            .collect();
        let whole: Vec<f64> = rows.iter().flatten().copied().collect();
        let as_one = rate_rows(&[whole], 3.0 * period);
        let rates: Vec<f64> = rate_rows(&rows, period).into_iter().flatten().collect();
        for (a, b) in rates.iter().zip(&as_one[0]) {
            assert!((a - b).abs() < 1e-9, "{a} vs {b}");
        }
    }

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

    /// Outside the null table (a 1 s step, which it holds no cells for)
    /// the search counts trials exactly as before #1195: significance is
    /// the score less log10 of the independent frequencies times the
    /// harmonic counts, and the report says it is uncalibrated.
    #[test]
    fn outside_the_table_the_search_counts_trials_as_before() {
        let tau = std::f64::consts::TAU;
        let g = series(600, 1.0, |t| (tau * t / 60.0).sin(), 0.5, 21);
        let r = analyse(&g, &LongConfig::default());
        let s = &r.search;
        assert!(!s.null.calibrated);
        let w = detrend_window(&g, DETREND_S).map(|w| w as f64 * g.step);
        let pad = spectrum(&r.detrended, w).pad;
        let trials = (s.score.len() as f64 / pad as f64 * HARMONICS.len() as f64).max(1.0);
        assert_eq!(s.trials, trials);
        let best = s.score.iter().copied().fold(0.0, f64::max);
        let top = &r.components[0];
        assert!(
            (top.significance - (best - trials.log10())).abs() < 1e-9,
            "{} against {}",
            top.significance,
            best - trials.log10()
        );
        assert!(period_uncalibrated(&r).is_some());
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

    /// A line 0.03% from the 3rd harmonic of a longer one is labelled so;
    /// one 3.3 resolutions (3.3/span in frequency) from it is not (#762).
    /// On 10 min: 56.60 s with 18.87 s (as ym42_CR entry 10), and 60 s
    /// with 22.44 s, which the n-fold tolerance marked the 60 s line's 3rd.
    #[test]
    fn a_harmonic_is_labelled_only_within_one_resolution() {
        let tau = std::f64::consts::TAU;
        let g = series(600, 1.0, |t| (tau * t / 60.0).sin(), 0.1, 13);
        let one = analyse(&g, &LongConfig::default()).components.remove(0);
        let at = |p: f64| Component {
            period_s: p,
            resolution_s: p * p / 600.0,
            harmonic_of: None,
            ..one.clone()
        };
        for (p0, p1, n) in [(56.60, 18.87, Some(3)), (60.0, 22.44, None)] {
            let mut cs = vec![at(p0), at(p1)];
            label_harmonics(&mut cs);
            assert_eq!(
                cs[1].harmonic_of.as_ref().map(|h| h.n),
                n,
                "{p1} s against {p0} s"
            );
            assert!(cs[0].harmonic_of.is_none());
        }
    }

    /// When the found cycle is near the span, two whole numbers can both
    /// fit within one resolution, and then neither is told (#789): over
    /// 580 s, 224.84 s is 0.43 resolutions from 2 turns of 540 s and 0.64
    /// from 3. Taking the first fit would call it the 2nd.
    #[test]
    fn a_harmonic_that_two_whole_numbers_fit_is_not_labelled() {
        let tau = std::f64::consts::TAU;
        let g = series(600, 1.0, |t| (tau * t / 60.0).sin(), 0.1, 13);
        let one = analyse(&g, &LongConfig::default()).components.remove(0);
        let at = |p: f64| Component {
            period_s: p,
            resolution_s: p * p / 580.0,
            harmonic_of: None,
            ..one.clone()
        };
        let off = |n| harmonic_offset(224.84, 224.84 * 224.84 / 580.0, n, 540.0);
        assert!(off(2) < 1.0 && off(3) < 1.0, "{} {}", off(2), off(3));
        let mut cs = vec![at(540.0), at(224.84)];
        label_harmonics(&mut cs);
        assert!(cs[1].harmonic_of.is_none(), "{:?}", cs[1].harmonic_of);
        // A single fit on the same span is still told: 270 s is 2 turns'
        // exactly and 1.07 resolutions from 3.
        let mut cs = vec![at(540.0), at(270.0)];
        label_harmonics(&mut cs);
        assert_eq!(cs[1].harmonic_of.map(|h| h.n), Some(2));
    }

    /// A line is at a whole fraction of a wheel's turn by the same
    /// one-resolution test as a harmonic of a found cycle, not by a 1%
    /// rule of its own (#775). On 10 min, 30.6 s is half of 60 s (0.4
    /// resolutions off, though 2% off in period); over 2 h, 20.1 s is not
    /// a third of 60 s (1.8 resolutions off, though 0.5% off in period).
    #[test]
    fn a_wheel_fraction_uses_the_one_resolution_test() {
        use crate::periodicity::Wheel;
        let tau = std::f64::consts::TAU;
        let g = series(600, 1.0, |t| (tau * t / 60.0).sin(), 0.1, 13);
        let one = analyse(&g, &LongConfig::default()).components.remove(0);
        let at = |p: f64, span: f64| Component {
            period_s: p,
            resolution_s: p * p / span,
            wheel: None,
            harmonic_of: None,
            ..one.clone()
        };
        let wheels = [Wheel {
            name: "fourth wheel".into(),
            period_s: 60.0,
        }];
        let n = |c: &Component| crate::session::wheel_fraction(c, &wheels).map(|(_, n)| n);
        assert_eq!(n(&at(30.6, 600.0)), Some(2));
        assert_eq!(n(&at(20.1, 600.0)), Some(3));
        assert_eq!(n(&at(20.1, 7200.0)), None);
        assert_eq!(n(&at(22.44, 600.0)), None);
        let tied = Component {
            wheel: Some("third wheel".into()),
            ..at(30.0, 600.0)
        };
        assert_eq!(n(&tied), None);
        // Over 580 s, a 224.84 s line is within one resolution of both 2
        // and 3 turns of a 540 s wheel (0.43 and 0.64 resolutions off, as
        // on ym42_DD_30min), so no fraction is told (#787).
        let third = [Wheel {
            name: "third wheel".into(),
            period_s: 540.0,
        }];
        let c = at(224.84, 580.0);
        assert!(harmonic_offset(c.period_s, c.resolution_s, 2, 540.0) < 1.0);
        assert!(harmonic_offset(c.period_s, c.resolution_s, 3, 540.0) < 1.0);
        assert_eq!(crate::session::wheel_fraction(&c, &third), None);
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

    /// Seeded AR(1) noise with no cycle in it (#723), cell by cell: the
    /// search is set for a 1% chance of naming a cycle in noise, so about
    /// 3 in 100 takes across the three series. Each cell's cap is the count
    /// measured on the fix, the higher of two seed sets of 100 (from 9000
    /// and from 9500). Across the first ten cells #723 names one in 46 and 33
    /// of those 1000 takes, main in 64 and 80 (up to 18 of 100 on half an
    /// hour at ρ 0.9, most near 0.75 of the median's window). Takes under
    /// 20 min take the AR(1) shape too (#753): before, 1 min at ρ 0.9 named
    /// one in 35 and 26 of 100, and 3 min at ρ 0.9 in 7 and 6.
    #[test]
    fn noise_with_no_cycle_rarely_names_one() {
        let cells = [
            (180.0, 1.0, 0.0, 5),
            (180.0, 1.0, 0.9, 2),
            (600.0, 1.0, 0.0, 4),
            (600.0, 1.0, 0.9, 3),
            (1800.0, 10.0, 0.0, 7),
            (1800.0, 10.0, 0.5, 5),
            (1800.0, 10.0, 0.9, 5),
            (7200.0, 10.0, 0.0, 3),
            (7200.0, 10.0, 0.5, 2),
            (7200.0, 10.0, 0.9, 11),
            (60.0, 1.0, 0.5, 5),
            (60.0, 1.0, 0.9, 2),
        ];
        let mut over = Vec::new();
        for (i, &(span, step, rho, cap)) in cells.iter().enumerate() {
            let n = (span / step) as usize;
            let mut z = Rng::new(9_000 + i as u64);
            let named = (0..100)
                .filter(|_| {
                    let mut e = z.normal();
                    let k = (1.0f64 - rho * rho).sqrt();
                    let y: Vec<f64> = (0..n)
                        .map(|_| {
                            e = rho * e + k * z.normal();
                            e
                        })
                        .collect();
                    let g = Grid { t0: 0.0, step, y };
                    !analyse(&g, &LongConfig::default()).components.is_empty()
                })
                .count();
            if named > cap {
                over.push(format!("{span} s at ρ {rho}: {named} of 100 (cap {cap})"));
            }
        }
        assert!(over.is_empty(), "{over:#?}");
    }

    /// A 225 s cycle on 10 min (2.7 cycles) beside a 60 s line of the
    /// same size, near its 4th harmonic, in white noise (#761): its swing
    /// is the cycle's own on every seed, not the two lines' together. With
    /// the search's 4 harmonics in the shape it read 1.55 to 2.07 times
    /// the true swing on these 40 seeds; with the cap, 0.69 to 1.05.
    #[test]
    fn a_cycle_over_few_turns_is_sized_without_a_line_near_its_harmonic() {
        let tau = std::f64::consts::TAU;
        let cfg = LongConfig {
            max_period_s: Some(300.0),
            ..Default::default()
        };
        for seed in 0..40u64 {
            let mut z = Rng::new(20_000 + seed);
            let y: Vec<f64> = (0..600)
                .map(|i| {
                    let t = i as f64;
                    (tau * t / 225.0).sin() + (tau * t / 60.0 + 1.0).sin() + z.normal()
                })
                .collect();
            let r = analyse(
                &Grid {
                    t0: 0.0,
                    step: 1.0,
                    y,
                },
                &cfg,
            );
            let c = r
                .components
                .iter()
                .find(|c| (c.period_s / 225.0 - 1.0).abs() < 0.15)
                .expect("the 225 s cycle is found");
            let size = c.peak_to_peak / 2.0;
            assert!((0.6..1.2).contains(&size), "seed {seed}: {size}");
        }
    }

    /// A 60 s sine in AR(1) noise (ρ 0.8, 1.5 times its size) over 30 min,
    /// 30 cycles, so the cap allows every harmonic the search sums (#761).
    /// On the 27 of 400 seeds where the search sums more than one, the
    /// harmonics past the fundamental are noise: the median rate swing is
    /// the sine's own when only the harmonics that stand out are kept
    /// (1.06 measured), against 1.39 when every one is.
    #[test]
    fn harmonics_that_are_noise_do_not_size_a_cycle() {
        let tau = std::f64::consts::TAU;
        let truth = 2.0 * tau / 60.0 * 86_400.0;
        let mut ratios: Vec<f64> = (0..400u64)
            .filter_map(|seed| {
                let mut z = Rng::new(22_000 + seed);
                let k = (1.0f64 - 0.64).sqrt();
                let mut e = z.normal();
                let y: Vec<f64> = (0..1800)
                    .map(|i| {
                        e = 0.8 * e + k * z.normal();
                        (tau * i as f64 / 60.0).sin() + 1.5 * e
                    })
                    .collect();
                let g = Grid {
                    t0: 0.0,
                    step: 1.0,
                    y,
                };
                let r = analyse(&g, &LongConfig::default());
                let c = r
                    .components
                    .into_iter()
                    .find(|c| (c.period_s / 60.0 - 1.0).abs() < 0.02 && c.harmonics > 1)?;
                let rate = rate_profile(&c.shape, c.period_s);
                let hi = rate.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let lo = rate.iter().copied().fold(f64::INFINITY, f64::min);
                Some((hi - lo) / truth)
            })
            .collect();
        assert_eq!(ratios.len(), 27);
        let mid = median(&mut ratios);
        assert!((0.85..1.2).contains(&mid), "{mid}");
    }

    /// A sinusoid in white noise of the same size, at two periods inside
    /// the band the search reads, near the running median's window where
    /// the fix matters (#723), is found on most takes. The 300 s cycle in
    /// 30 min had a bar of 75 of 100; leveling only from above the corner
    /// (#1030) finds 49, and on 4 Nov the bar was lowered to 49 by the
    /// owner's decision, to stop the search naming cycles in noise there.
    #[test]
    fn a_cycle_near_the_detrend_window_is_still_found() {
        for (i, &(span, period, at_least)) in [(1800.0, 300.0, 49), (7200.0, 1400.0, 95)]
            .iter()
            .enumerate()
        {
            let n = (span / 10.0) as usize;
            let mut z = Rng::new(7_700 + i as u64);
            let found = (0..100)
                .filter(|_| {
                    let y: Vec<f64> = (0..n)
                        .map(|k| {
                            0.6 * (std::f64::consts::TAU * k as f64 * 10.0 / period).sin()
                                + z.normal()
                        })
                        .collect();
                    analyse(
                        &Grid {
                            t0: 0.0,
                            step: 10.0,
                            y,
                        },
                        &LongConfig::default(),
                    )
                    .components
                    .iter()
                    .any(|c| (c.period_s / period - 1.0).abs() < 0.1)
                })
                .count();
            assert!(
                found >= at_least,
                "{period} s in {span} s: {found} of 100 (floor {at_least})"
            );
        }
    }

    /// A slow wander (φ 0.98 a 2 s window) under white measurement noise,
    /// lag one 0.8, the amplitude windows #916 found the search misreads.
    /// The 400-seed counts [`MEASURED_CROSSING_IN`] and
    /// [`SHORT_CORRELATED_CROSSING_IN`] quote: how often one series'
    /// search names a cycle in AR(1) noise at 30 min and 2 h (2 s
    /// windows), and in a slow wander under white noise at 30 min. Run by
    /// hand: it takes minutes.
    #[test]
    #[ignore = "re-measures the crossing constants; run by hand"]
    fn crossing_counts_on_400_seeds() {
        let ar1 = |rho: f64, n: usize, seed: u64| -> Vec<f64> {
            let mut z = Rng::new(seed);
            let inn = (1.0 - rho * rho).sqrt();
            let mut x = z.normal();
            (0..n)
                .map(|_| {
                    let v = x;
                    x = rho * x + inn * z.normal();
                    v
                })
                .collect()
        };
        let wander95 = |n: usize, seed: u64| -> Vec<f64> {
            let (phi, s) = (0.95f64, 0.8 / 0.95);
            let mut z = Rng::new(seed);
            let innov = (s * (1.0 - phi * phi)).sqrt();
            let mut x = s.sqrt() * z.normal();
            (0..n)
                .map(|_| {
                    x = phi * x + innov * z.normal();
                    x + (1.0 - s).sqrt() * z.normal()
                })
                .collect()
        };
        type Gen<'a> = Box<dyn Fn(u64) -> Vec<f64> + Sync + 'a>;
        // (case, series, the counts the constants' docs quote)
        let cases: Vec<(&str, Gen, std::ops::RangeInclusive<usize>)> = vec![
            ("ρ 0, 30 min", Box::new(|s| ar1(0.0, 900, s)), 4..=4),
            ("ρ 0.5, 30 min", Box::new(|s| ar1(0.5, 900, s)), 4..=4),
            ("ρ 0.9, 30 min", Box::new(|s| ar1(0.9, 900, s)), 5..=5),
            ("ρ 0, 2 h", Box::new(|s| ar1(0.0, 3600, s)), 3..=3),
            ("ρ 0.5, 2 h", Box::new(|s| ar1(0.5, 3600, s)), 5..=5),
            ("ρ 0.9, 2 h", Box::new(|s| ar1(0.9, 3600, s)), 6..=6),
            (
                "wander φ 0.98, 30 min",
                Box::new(|s| wander_under_white(900, s)),
                15..=15,
            ),
            (
                "wander φ 0.95, 30 min",
                Box::new(|s| wander95(900, s)),
                10..=10,
            ),
        ];
        let counts: Vec<usize> = std::thread::scope(|sc| {
            let jobs: Vec<_> = cases
                .iter()
                .map(|(_, gen, _)| {
                    sc.spawn(move || {
                        (1..=400u64)
                            .filter(|&seed| {
                                let y = gen(seed * 31 + 5);
                                let g = Grid {
                                    t0: 0.0,
                                    step: 2.0,
                                    y,
                                };
                                !analyse(&g, &LongConfig::default()).components.is_empty()
                            })
                            .count()
                    })
                })
                .collect();
            jobs.into_iter().map(|j| j.join().unwrap()).collect()
        });
        for ((name, _, _), got) in cases.iter().zip(&counts) {
            eprintln!("{name}: {got} of 400");
        }
        for ((name, _, want), got) in cases.iter().zip(&counts) {
            assert!(
                want.contains(got),
                "{name}: {got} of 400, the docs say {want:?}"
            );
        }
    }

    fn wander_under_white(n: usize, seed: u64) -> Vec<f64> {
        let (phi, s) = (0.98f64, 0.8 / 0.98);
        let mut z = Rng::new(seed);
        let innov = (s * (1.0 - phi * phi)).sqrt();
        let mut x = s.sqrt() * z.normal();
        (0..n)
            .map(|_| {
                x = phi * x + innov * z.normal();
                x + (1.0 - s).sqrt() * z.normal()
            })
            .collect()
    }

    /// A cycle the search names in 30 min of that noise alone carries the
    /// caveat (#916); the same noise over 8 h, with a cycle there that it
    /// names, does not: the caveat is for stretches under 2 h. Nor does a
    /// cycle named in 30 min of white noise, read at a low ρ.
    #[test]
    fn a_cycle_named_in_short_correlated_noise_may_be_noise() {
        let g = Grid {
            t0: 0.0,
            step: 2.0,
            y: wander_under_white(900, 7),
        };
        let r = analyse(&g, &LongConfig::default());
        assert!(!r.components.is_empty(), "seed 7 names a cycle in noise");
        match period_may_be_noise(&r) {
            Some(crate::measure::Caveat::PeriodMayBeNoise { rho, span_s }) => {
                assert!(rho >= NOISE_CAVEAT_RHO && rho == r.search.rho, "{rho}");
                assert_eq!(span_s, g.span());
            }
            other => panic!("{other:?}"),
        }
        let tau = std::f64::consts::TAU;
        let y = wander_under_white(14_400, 7)
            .into_iter()
            .enumerate()
            .map(|(i, v)| v + 0.5 * (tau * i as f64 * 2.0 / 300.0).sin())
            .collect();
        let r = analyse(
            &Grid {
                t0: 0.0,
                step: 2.0,
                y,
            },
            &LongConfig::default(),
        );
        assert!(!r.components.is_empty() && r.search.rho >= NOISE_CAVEAT_RHO);
        assert_eq!(period_may_be_noise(&r), None);
        // A cycle named in 30 min of white noise, whose ρ is low: none.
        let mut z = Rng::new(7);
        let y = (0..900)
            .map(|i| z.normal() + 0.5 * (tau * i as f64 * 2.0 / 300.0).sin())
            .collect();
        let r = analyse(
            &Grid {
                t0: 0.0,
                step: 2.0,
                y,
            },
            &LongConfig::default(),
        );
        assert!(!r.components.is_empty() && r.search.rho < NOISE_CAVEAT_RHO);
        assert_eq!(period_may_be_noise(&r), None);
    }
}

#[cfg(test)]
mod slow_tests {
    use super::*;

    /// The beats' timing of a 6 h take whose rate drifts from −40 to
    /// +40 s/d, with a 5 s/d cycle every 270 s, 20 µs of jitter and a walk.
    fn drifting(cycle_s_per_day: f64) -> Grid {
        let (step, span) = (0.25, 6.0 * 3600.0);
        let mut z = crate::synth::Rng::new(5);
        let mut walk = 0.0;
        let (slow, tau) = (12.0 * 3600.0, std::f64::consts::TAU);
        let y = (0..(span / step) as usize)
            .map(|i| {
                let t = i as f64 * step;
                walk += 2e-7 * z.normal();
                // The integral of a rate of 40 sin(2π(t − 3 h)/12 h) s/d...
                let drift = -40.0 * slow / tau * ((tau * (t - 3.0 * 3600.0) / slow).cos());
                // ...and a 5 s/d sine every 270 s.
                let cycle = -cycle_s_per_day * 270.0 / tau * (tau * t / 270.0).cos();
                (drift + cycle) / 86_400.0 + walk + 2e-5 * z.normal()
            })
            .collect();
        Grid { t0: 0.0, step, y }
    }

    /// How much of a sine at `period` the series holds, by projection.
    fn swing_at(g: &Grid, period: f64) -> f64 {
        let (mut s, mut c, mut n) = (0.0, 0.0, 0.0);
        for (i, v) in g.y.iter().enumerate().filter(|(_, v)| v.is_finite()) {
            let x = std::f64::consts::TAU * i as f64 * g.step / period;
            s += v * x.sin();
            c += v * x.cos();
            n += 1.0;
        }
        2.0 * (s * s + c * c).sqrt() / n
    }

    /// A 5 s/d cycle every 270 s on a rate drifting ±40 s/d is found by
    /// the timing's search at its size (2.5 ms each way), where the plain
    /// running median left a sixth of it: the drift's ramp holds each
    /// window's values in order, so its median is its middle value, cycle
    /// and all, and the cycle went out with the trend.
    #[test]
    fn a_cycle_on_a_drifting_rate_is_found() {
        let g = drifting(5.0);
        let want = 5.0 / 86_400.0 * 270.0 / std::f64::consts::TAU;
        let plain = swing_at(&detrend_as(&g, DETREND_S, Slow::Level), 270.0);
        let sloped = swing_at(&detrend_as(&g, DETREND_S, Slow::Sloped), 270.0);
        assert!(plain < 0.2 * want, "plain {plain} of {want}");
        assert!(
            (sloped / want - 1.0).abs() < 0.15,
            "sloped {sloped} of {want}"
        );
        let r = analyse_timing(&g, &LongConfig::default());
        let c = r.components.first().expect("a cycle named");
        assert!((c.period_s - 270.0).abs() < 1.0, "{}", c.period_s);
        assert!(
            (c.peak_to_peak / (2.0 * want) - 1.0).abs() < 0.2,
            "{}",
            c.peak_to_peak
        );
        // Without the cycle, nothing.
        assert!(analyse_timing(&drifting(0.0), &LongConfig::default())
            .components
            .is_empty());
    }

    /// Each piece is detrended on its own: the timing's jump where a piece
    /// starts again from its own fit reads nothing, and a piece shorter
    /// than one window is left out.
    #[test]
    fn pieces_are_detrended_on_their_own() {
        let step = 1.0;
        let starts = [7200, 14400];
        let y: Vec<f64> = (0..15000)
            .map(|i| {
                let t = i as f64 * step;
                let piece = starts.iter().filter(|&&s| i >= s).count() as f64;
                3e-4 * t - 2.0 * piece + 1e-5 * ((i * 7919) % 101) as f64
            })
            .collect();
        let g = Grid { t0: 0.0, step, y };
        let d = detrend_pieces(&g, DETREND_S, Slow::Sloped, &starts);
        let worst = d.y[..14400].iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(worst < 1e-3, "{worst}");
        assert!(d.y[14400..].iter().all(|v| v.is_nan()));
        let across = detrend_as(&g, DETREND_S, Slow::Sloped);
        let jump = across.y.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(jump > 0.5, "{jump}");
    }

    /// An amplitude falling smoothly from 300° to 250° over the take, with
    /// AR(1) noise of 2° (ρ 0.7) on 2 s windows and no cycle, names no
    /// line, in particular none at the spacing of the running median's
    /// reads (1/40 of its window) or its harmonics, on takes whose window
    /// is 15 min (1 h), 30 min (2 h, 12 h) and 30 min over 60 h (Norm).
    #[test]
    fn a_bending_amplitude_names_no_line_at_the_reads_spacing() {
        let step = 2.0;
        for (hours, seed) in [(1.0, 1), (2.0, 2), (12.0, 3), (60.0, 4)] {
            let span = hours * 3600.0;
            let n = (span / step) as usize;
            let mut z = crate::synth::Rng::new(seed);
            let mut x = 0.0;
            let y: Vec<f64> = (0..n)
                .map(|i| {
                    let t = i as f64 * step / span;
                    x = 0.7 * x + (1.0f64 - 0.49).sqrt() * 2.0 * z.normal();
                    300.0 - 50.0 * (1.0 - (-3.0 * t).exp()) / (1.0 - (-3.0f64).exp()) + x
                })
                .collect();
            let g = Grid { t0: 0.0, step, y };
            let r = analyse(&g, &LongConfig::default());
            let w = detrend_window(&g, DETREND_S).unwrap() as f64 * step;
            let named: Vec<f64> = r.components.iter().map(|c| c.period_s).collect();
            assert!(
                named.is_empty(),
                "{hours} h (reads every {} s): {named:?}",
                w / 40.0
            );
        }
    }

    /// The timing's floor is half a unit of the rate's written place.
    #[test]
    fn the_rate_swing_floor_is_half_the_rates_place() {
        let place = 10f64.powi(-(crate::figure_text::base_digits::RATE as i32));
        assert!((MIN_RATE_SWING_S_PER_DAY - 0.5 * place).abs() < 1e-12);
    }

    /// The timing's search ends at 1.2 detrend windows ([`SLOPED_REACH`]); the readings'
    /// at a third of the take.
    #[test]
    fn the_timing_search_ends_where_its_detrend_keeps_half() {
        let r = analyse_timing(&drifting(0.0), &LongConfig::default());
        let longest = r.search.period_s.iter().fold(0.0f64, |m, &p| m.max(p));
        assert!(longest <= SLOPED_REACH * DETREND_S + 1e-6, "{longest}");
        assert!(longest > 0.9 * SLOPED_REACH * DETREND_S, "{longest}");
    }
}
