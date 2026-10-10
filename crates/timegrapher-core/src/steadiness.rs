//! Is a series steady? The rate, the amplitude and the beat error of a
//! take are each read as a series in time order and tested for
//! independence: a healthy watch on a quiet bench gives readings that
//! scatter about one level with no memory of the last one, independent
//! and identically distributed. A distribution plot cannot show the
//! alternatives, because it throws the time order away. These views keep
//! it:
//!
//! - **Autocorrelation**: how much a reading resembles the one a lag
//!   later. Independent readings sit inside a band of ±1.96/√n at every
//!   lag; the Ljung–Box test sums the first lags into one p-value.
//! - **Allan deviation**: the scatter of averages over τ, against what
//!   independent readings would give (the scatter at one reading over
//!   √(τ / reading)). The two lines stay together for independent
//!   readings; wander and drift lift the curve at long τ, and a cycle
//!   puts a bump near half its period.
//! - **CUSUM**: the running sum of each reading's distance from the
//!   mean, in units of σ√n. For independent readings it wanders inside
//!   ±1.36 (95%); a shift of the mean bends it into a V or a tent with
//!   the corner at the shift.
//! - **Change points**: binary segmentation of block medians into
//!   stretches of constant mean, each change kept only when it is
//!   strong evidence against one level.
//!
//! These sit beside the period search of the long-run analysis (which
//! scores each period with a false-alarm chance and names the wheel) and
//! the two-state finder, and one verdict is drawn from all of them.

use crate::beats::Beat;
use crate::clock::ClockFit;
use crate::dsp::median;
use crate::longrun::LongReport;
use crate::longterm::{self, Grid, LongConfig};
use crate::periodicity::{polyfit2, Wheel};
use crate::session::Severity;
use crate::stream::BeatLog;
use crate::timing;
use crate::twostate::{self, TwoState};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Config {
    /// Length of each rate reading, seconds.
    pub rate_reading_s: f64,
    /// A test result counts below this p-value (or false-alarm chance).
    pub alpha: f64,
    /// Correlation at a short lag that is worth calling memory, however
    /// many readings make it significant.
    pub min_correlation: f64,
    /// Longest lag of the autocorrelation, seconds (also capped at a
    /// third of the take).
    pub max_lag_s: f64,
    /// Lags summed by the Ljung–Box test.
    pub ljung_box_lags: usize,
    /// A period is the verdict when it explains this share of the
    /// detrended variance.
    pub periodic_min_explained: f64,
    /// Block length for the change points, seconds, and the shortest
    /// stretch of constant mean.
    pub change_block_s: f64,
    pub change_min_segment_s: f64,
    /// A trend or a set of steps is the verdict when it explains this
    /// share of the variance.
    pub min_explained: f64,
    /// Readings further than this many short-term σ from the running
    /// median are left out as outliers.
    pub outlier_sigma: f64,
    /// Curves in the output are thinned to about this many points.
    pub max_points: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            rate_reading_s: 10.0,
            alpha: 0.01,
            min_correlation: 0.15,
            max_lag_s: 600.0,
            ljung_box_lags: 10,
            periodic_min_explained: 0.1,
            change_block_s: 30.0,
            change_min_segment_s: 300.0,
            min_explained: 0.2,
            outlier_sigma: 6.0,
            max_points: 600,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeriesKind {
    Rate,
    Amplitude,
    BeatError,
}

impl SeriesKind {
    pub fn name(self) -> &'static str {
        match self {
            SeriesKind::Rate => "rate",
            SeriesKind::Amplitude => "amplitude",
            SeriesKind::BeatError => "beat error",
        }
    }
    pub fn unit(self) -> &'static str {
        match self {
            SeriesKind::Rate => "s/d",
            SeriesKind::Amplitude => "deg",
            SeriesKind::BeatError => "ms",
        }
    }

    fn fmt(self, v: f64) -> String {
        match self {
            SeriesKind::Rate => format!("{v:+.1} s/d"),
            SeriesKind::Amplitude => format!("{v:.0}°"),
            SeriesKind::BeatError => format!("{v:.2} ms"),
        }
    }
    fn fmt_size(self, v: f64) -> String {
        match self {
            SeriesKind::Rate => format!("{v:.1} s/d"),
            SeriesKind::Amplitude => format!("{v:.1}°"),
            SeriesKind::BeatError => format!("{v:.2} ms"),
        }
    }
}

/// The one-word answer for a series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Independent readings about one level.
    Steady,
    /// A repeating change, found by the period search (with the wheel
    /// whose turn it matches, if any).
    Periodic,
    /// Two levels the watch switches between.
    TwoStates,
    /// Two levels that come from the measurement (the unlock mark
    /// hopping), not from the watch.
    Measurement,
    /// One or more lasting steps of the mean.
    ShiftingMean,
    /// A steady slide, such as amplitude falling as the mainspring lets
    /// down.
    Drifting,
    /// Readings that remember the last ones without steps, a cycle or a
    /// trend: slow, irregular wander.
    Wandering,
    /// Too few readings to say.
    TooShort,
}

#[derive(Debug, Clone, Serialize)]
pub struct Autocorrelation {
    /// Lags, seconds, and the correlation at each.
    pub lag_s: Vec<f64>,
    pub r: Vec<f64>,
    /// Half-width of the 95% band for independent readings, 1.96/√n.
    pub band: f64,
    pub ljung_box_q: f64,
    pub ljung_box_lags: usize,
    /// Chance of a Q this large from independent readings.
    pub p_value: f64,
    /// Largest |r| over the Ljung–Box lags.
    pub max_short_lag_r: f64,
    /// The first lag where the correlation climbs back to a peak after
    /// falling (the readings repeat), and the correlation there.
    pub repeat_lag_s: Option<f64>,
    pub repeat_r: Option<f64>,
    /// First lag where the correlation falls below 1/e, seconds; None
    /// when it never does within the lags computed.
    pub memory_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Allan {
    /// Averaging time, seconds; the Allan deviation there (the series'
    /// units); what independent readings would give; differences used.
    pub tau_s: Vec<f64>,
    pub deviation: Vec<f64>,
    pub white: Vec<f64>,
    pub pairs: Vec<usize>,
    /// Log-log slope over the short end (−0.5 for independent readings,
    /// 0 for flicker, +0.5 for a random walk, +1 for drift).
    pub slope_short: Option<f64>,
    /// The averaging time with the smallest deviation, and that value:
    /// how long to average for the most repeatable reading.
    pub best_tau_s: Option<f64>,
    pub best_deviation: Option<f64>,
    /// Largest deviation over the white line among points with at least
    /// 8 differences: 1 for independent readings.
    pub max_excess: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cusum {
    /// Time, seconds, and the CUSUM there in units of σ√n.
    pub t_s: Vec<f64>,
    pub s: Vec<f64>,
    /// Largest |s| and where it is.
    pub max: f64,
    pub at_s: f64,
    /// Chance of a max this large from independent readings
    /// (Kolmogorov).
    pub p_value: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub start_s: f64,
    pub end_s: f64,
    pub mean: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Changes {
    pub block_s: f64,
    /// Noise of one block median, from the differences of neighbours.
    pub block_sigma: f64,
    pub segments: Vec<Segment>,
    /// Share of the variance the segment means explain.
    pub explained: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trend {
    /// Slope, units per hour, and the share of the variance a straight
    /// line explains.
    pub per_hour: f64,
    pub explained: f64,
    /// The share a smooth curve (a quadratic) explains, which also covers
    /// a rise and fall.
    pub curve_explained: f64,
    /// The curve at the start and the end, and its turning point when
    /// that falls inside the take (time, value).
    pub start: f64,
    pub end: f64,
    pub turn: Option<(f64, f64)>,
}

/// Where a cycle was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CycleSource {
    /// The two-state finder: a low level that comes back on a period.
    TwoState,
    /// A repeated peak of the readings' autocorrelation.
    Autocorrelation,
    /// The period search of the long-run analysis.
    PeriodSearch,
}

/// The cycle behind a `periodic` verdict.
#[derive(Debug, Clone, Serialize)]
pub struct Cycle {
    pub period_s: f64,
    /// Peak to peak of the readings folded at the period, the series'
    /// units.
    pub peak_to_peak: f64,
    /// Share of the detrended variance the cycle's shape explains, above
    /// what noise alone would give.
    pub explained: f64,
    pub source: CycleSource,
    /// A wheel whose turn is within 6% of the period.
    pub wheel: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Period {
    pub period_s: f64,
    /// −log10 of the false-alarm chance.
    pub significance: f64,
    /// Share of the detrended variance the cycle explains.
    pub explained: f64,
    /// Peak-to-peak over one cycle, the series' units.
    pub peak_to_peak: f64,
    /// Rate only: peak-to-peak of the folded timing, ms.
    pub timing_peak_to_peak_ms: Option<f64>,
    /// The size to show people and its unit: the same as `peak_to_peak`,
    /// except a rate cycle shorter than 30 s, which is stated as its timing
    /// swing in ms (see `longrun::TIMING_UNIT_BELOW_S`).
    pub size: f64,
    pub size_unit: &'static str,
    pub wheel: Option<String>,
    /// The search's weaker components, seconds. Each is also tried as a
    /// cycle, so a slow cycle behind a stronger fast one (an escape wheel's)
    /// is still found.
    pub other_periods_s: Vec<f64>,
}

/// Everything about one series.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesCheck {
    pub series: SeriesKind,
    pub unit: &'static str,
    pub verdict: Verdict,
    /// One plain sentence for people.
    pub headline: String,
    /// Length of each reading, seconds; readings used; readings left out
    /// as outliers.
    pub step_s: f64,
    pub readings: usize,
    pub outliers: usize,
    pub median: Option<f64>,
    pub sd: Option<f64>,
    /// Scatter from one reading to the next, √(½·mean of squared
    /// differences): the noise without the slow changes.
    pub short_term_sd: Option<f64>,
    pub autocorrelation: Option<Autocorrelation>,
    pub allan: Option<Allan>,
    pub cusum: Option<Cusum>,
    pub changes: Option<Changes>,
    pub trend: Option<Trend>,
    /// The strongest periodic component, when the period search found one.
    pub period: Option<Period>,
    pub two_state: Option<TwoState>,
    /// The cycle the verdict rests on, when it is `periodic`, or one
    /// found under another verdict.
    pub cycle: Option<Cycle>,
    /// Amplitude and beat error only: one side's unlock edges split in
    /// two (the two-state finder's check), so what this series shows may
    /// be the measurement.
    pub unlock_hopping: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub series: SeriesKind,
    pub severity: Severity,
    pub title: String,
    pub evidence: String,
    pub advice: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub duration_s: f64,
    pub bph: u32,
    pub config: Config,
    pub series: Vec<SeriesCheck>,
    pub findings: Vec<Finding>,
}

// ---------------------------------------------------------------- statistics

fn finite(y: &[f64]) -> Vec<f64> {
    y.iter().copied().filter(|v| v.is_finite()).collect()
}

fn mean_sd(y: &[f64]) -> Option<(f64, f64)> {
    let v = finite(y);
    if v.len() < 2 {
        return None;
    }
    let m = v.iter().sum::<f64>() / v.len() as f64;
    let var = v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (v.len() - 1) as f64;
    Some((m, var.sqrt()))
}

/// √(½·mean of squared differences of neighbours): the Allan deviation at
/// one reading.
fn short_term_sd(y: &[f64]) -> Option<f64> {
    let d: Vec<f64> = y
        .windows(2)
        .filter(|w| w[0].is_finite() && w[1].is_finite())
        .map(|w| (w[1] - w[0]).powi(2))
        .collect();
    (d.len() >= 2).then(|| (0.5 * d.iter().sum::<f64>() / d.len() as f64).sqrt())
}

/// Robust σ of neighbour differences: 1.4826·MAD/√2.
fn robust_diff_sigma(y: &[f64]) -> Option<f64> {
    let mut d: Vec<f64> = y
        .windows(2)
        .filter(|w| w[0].is_finite() && w[1].is_finite())
        .map(|w| w[1] - w[0])
        .collect();
    if d.len() < 4 {
        return None;
    }
    let m = median(&mut d);
    let mut a: Vec<f64> = d.iter().map(|x| (x - m).abs()).collect();
    Some(1.4826 * median(&mut a) / std::f64::consts::SQRT_2)
}

fn ln_gamma(x: f64) -> f64 {
    // Lanczos approximation (g = 7, n = 9).
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        let pi = std::f64::consts::PI;
        return (pi / (pi * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + 7.5;
    for (i, &c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// Regularised upper incomplete gamma Q(a, x) = P(Gamma(a) > x).
fn gamma_q(a: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    let lead = (-x + a * x.ln() - ln_gamma(a)).exp();
    if x < a + 1.0 {
        // Series for P.
        let (mut sum, mut term, mut ap) = (1.0 / a, 1.0 / a, a);
        for _ in 0..1000 {
            ap += 1.0;
            term *= x / ap;
            sum += term;
            if term.abs() < sum.abs() * 1e-15 {
                break;
            }
        }
        (1.0 - sum * lead).clamp(0.0, 1.0)
    } else {
        // Continued fraction for Q (modified Lentz).
        let tiny = 1e-300;
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / tiny;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..1000 {
            let an = -(i as f64) * (i as f64 - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < tiny {
                d = tiny;
            }
            c = b + an / c;
            if c.abs() < tiny {
                c = tiny;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-15 {
                break;
            }
        }
        (lead * h).clamp(0.0, 1.0)
    }
}

/// P(χ²_k > q).
pub fn chi2_tail(q: f64, k: usize) -> f64 {
    gamma_q(k as f64 / 2.0, q / 2.0)
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

/// Autocorrelation at lags 0..=max_lag of a series with gaps (NaN): each
/// lag uses the pairs where both readings exist.
pub fn acf(y: &[f64], max_lag: usize) -> Vec<f64> {
    let Some((m, _)) = mean_sd(y) else {
        return Vec::new();
    };
    let x: Vec<f64> = y.iter().map(|v| v - m).collect();
    let mut out = Vec::with_capacity(max_lag + 1);
    let mut c0 = 0.0;
    for k in 0..=max_lag.min(y.len().saturating_sub(1)) {
        let (mut s, mut n) = (0.0, 0usize);
        for i in 0..x.len() - k {
            if x[i].is_finite() && x[i + k].is_finite() {
                s += x[i] * x[i + k];
                n += 1;
            }
        }
        let c = if n > 0 { s / n as f64 } else { f64::NAN };
        if k == 0 {
            c0 = c;
        }
        out.push(if c0 > 0.0 { c / c0 } else { f64::NAN });
    }
    out
}

/// Overlapping Allan deviation of a series of averages (frequency-type
/// data) at octave multiples of one reading. A block counts when at
/// least three quarters of its readings exist.
pub fn allan(y: &[f64], step: f64) -> Vec<(f64, f64, usize)> {
    let n = y.len();
    let mut sum = vec![0.0; n + 1];
    let mut cnt = vec![0usize; n + 1];
    for i in 0..n {
        let ok = y[i].is_finite();
        sum[i + 1] = sum[i] + if ok { y[i] } else { 0.0 };
        cnt[i + 1] = cnt[i] + ok as usize;
    }
    let block = |a: usize, m: usize| {
        let c = cnt[a + m] - cnt[a];
        (4 * c >= 3 * m).then(|| (sum[a + m] - sum[a]) / c as f64)
    };
    let mut out = Vec::new();
    let mut m = 1;
    while 2 * m < n {
        let (mut s, mut k) = (0.0, 0usize);
        for a in 0..=(n - 2 * m) {
            if let (Some(p), Some(q)) = (block(a, m), block(a + m, m)) {
                s += (q - p).powi(2);
                k += 1;
            }
        }
        // Independent differences, not overlapping ones, set how many
        // there really are.
        let independent = k / m;
        if independent < 3 {
            break;
        }
        out.push((m as f64 * step, (0.5 * s / k as f64).sqrt(), independent));
        m *= 2;
    }
    out
}

/// Split block medians into stretches of constant mean by binary
/// segmentation. Returns the start index of each stretch.
fn segment(y: &[f64], sigma: f64, min_len: usize, penalty: f64) -> Vec<usize> {
    let mut cuts = vec![0usize, y.len()];
    let mut work = vec![(0usize, y.len())];
    while let Some((a, b)) = work.pop() {
        if b - a < 2 * min_len {
            continue;
        }
        let total: f64 = y[a..b].iter().sum();
        let n = (b - a) as f64;
        let mut left = 0.0;
        let mut best = (0.0, 0usize);
        for (k, &v) in y.iter().enumerate().take(b - 1).skip(a) {
            left += v;
            let n1 = (k + 1 - a) as f64;
            let i = k + 1;
            if i - a < min_len || b - i < min_len {
                continue;
            }
            let n2 = n - n1;
            let d = left / n1 - (total - left) / n2;
            let gain = n1 * n2 / n * d * d / (sigma * sigma);
            if gain > best.0 {
                best = (gain, i);
            }
        }
        if best.0 > penalty {
            cuts.push(best.1);
            work.push((a, best.1));
            work.push((best.1, b));
        }
    }
    cuts.sort();
    cuts.pop();
    cuts
}

fn thin(n: usize, max: usize) -> usize {
    n.div_ceil(max.max(1)).max(1)
}

// ------------------------------------------------------------------ series

/// The share of variance explained when `fitted` replaces the values.
fn explained(y: &[f64], fitted: &[f64]) -> f64 {
    let Some((m, _)) = mean_sd(y) else { return 0.0 };
    let (mut ss, mut rs) = (0.0, 0.0);
    for (&v, &f) in y.iter().zip(fitted) {
        if v.is_finite() && f.is_finite() {
            ss += (v - m).powi(2);
            rs += (v - f).powi(2);
        }
    }
    if ss > 0.0 {
        (1.0 - rs / ss).max(0.0)
    } else {
        0.0
    }
}

/// Set lone wild readings (a knock, a dropped beat) to NaN; returns how
/// many. A reading counts as wild when it sits more than `k` σ from the
/// slow trend, σ being the larger of the scatter between neighbours and
/// the spread about the trend. When more than 3% of the readings would
/// go, they are part of what the watch does (a cycle, a state), not
/// outliers, and all are kept.
fn drop_outliers(g: &mut Grid, k: f64) -> usize {
    let base = longterm::detrend(g, 300.0);
    let mut dev: Vec<f64> = base
        .y
        .iter()
        .map(|d| d.abs())
        .filter(|d| d.is_finite())
        .collect();
    if dev.len() < 10 {
        return 0;
    }
    let spread = 1.4826 * median(&mut dev);
    let sigma = robust_diff_sigma(&g.y).unwrap_or(0.0).max(spread);
    if sigma.is_nan() || sigma <= 0.0 {
        return 0;
    }
    let wild: Vec<usize> = (0..g.y.len())
        .filter(|&i| g.y[i].is_finite() && base.y[i].abs() > k * sigma)
        .collect();
    if wild.len() as f64 > 0.03 * dev.len() as f64 {
        return 0;
    }
    for &i in &wild {
        g.y[i] = f64::NAN;
    }
    wild.len()
}

fn period_of(r: &longterm::SeriesReport, unit: &'static str) -> Option<Period> {
    r.components.first().map(|c| Period {
        period_s: c.period_s,
        significance: c.significance,
        explained: c.explained,
        peak_to_peak: c.peak_to_peak,
        timing_peak_to_peak_ms: None,
        size: c.peak_to_peak,
        size_unit: unit,
        wheel: c.wheel.clone(),
        other_periods_s: r.components[1..].iter().map(|c| c.period_s).collect(),
    })
}

/// Test one series. `g` is on a uniform grid of readings with gaps as NaN.
pub fn check_series(
    kind: SeriesKind,
    mut g: Grid,
    period: Option<Period>,
    two_state: Option<TwoState>,
    wheels: &[Wheel],
    cfg: &Config,
) -> SeriesCheck {
    let outliers = drop_outliers(&mut g, cfg.outlier_sigma);
    let y = &g.y;
    let n = y.iter().filter(|v| v.is_finite()).count();
    let mut out = SeriesCheck {
        series: kind,
        unit: kind.unit(),
        verdict: Verdict::TooShort,
        headline: String::new(),
        step_s: g.step,
        readings: n,
        outliers,
        median: {
            let mut v = finite(y);
            (!v.is_empty()).then(|| median(&mut v))
        },
        sd: mean_sd(y).map(|m| m.1),
        short_term_sd: short_term_sd(y),
        autocorrelation: None,
        allan: None,
        cusum: None,
        changes: None,
        trend: None,
        period,
        two_state,
        cycle: None,
        unlock_hopping: false,
    };
    if n < 30 || g.span() < 120.0 {
        out.headline = format!(
            "Too few {} readings to say whether they are steady.",
            kind.name()
        );
        return out;
    }
    let (mean, sd) = mean_sd(y).unwrap();
    let sst = out.short_term_sd.unwrap_or(sd);

    // Autocorrelation and Ljung–Box.
    let max_lag = ((cfg.max_lag_s / g.step) as usize)
        .min(y.len() / 3)
        .max(cfg.ljung_box_lags.min(y.len() / 3));
    let r = acf(y, max_lag);
    let h = cfg.ljung_box_lags.min(r.len().saturating_sub(1)).max(1);
    let nf = n as f64;
    let q: f64 = (1..=h)
        .filter(|&k| r[k].is_finite())
        .map(|k| r[k] * r[k] / (nf - k as f64).max(1.0))
        .sum::<f64>()
        * nf
        * (nf + 2.0);
    let max_short = (1..=h)
        .filter_map(|k| r.get(k).copied())
        .filter(|v| v.is_finite())
        .fold(0.0f64, |a, v| a.max(v.abs()));
    let memory = r
        .iter()
        .position(|&v| v < (-1.0f64).exp())
        .map(|k| k as f64 * g.step);
    let band = 1.96 / nf.sqrt();
    // A repeat: a local peak, strong, after the correlation has fallen
    // well below it, and that comes back at twice the lag (a cycle
    // repeats; a chance bump does not).
    let repeat = (2..r.len().saturating_sub(1)).find(|&k| {
        let lo = r[1..k].iter().copied().fold(f64::INFINITY, f64::min);
        let again = (2 * k * 9 / 10..=(2 * k * 11 / 10).min(r.len() - 1))
            .map(|j| r[j])
            .fold(f64::NEG_INFINITY, f64::max);
        r[k] >= r[k - 1]
            && r[k] > r[k + 1]
            && r[k] >= 0.3f64.max(4.0 * band)
            && lo <= r[k] - 0.3
            && again >= 0.5 * r[k]
    });
    let stride = thin(r.len(), cfg.max_points);
    let lb_p = chi2_tail(q, h);
    out.autocorrelation = Some(Autocorrelation {
        lag_s: (0..r.len())
            .step_by(stride)
            .map(|k| k as f64 * g.step)
            .collect(),
        r: r.iter().step_by(stride).copied().collect(),
        band,
        ljung_box_q: q,
        ljung_box_lags: h,
        p_value: lb_p,
        max_short_lag_r: max_short,
        repeat_lag_s: repeat.map(|k| k as f64 * g.step),
        repeat_r: repeat.map(|k| r[k]),
        memory_s: memory,
    });

    // Allan deviation with the line independent readings would follow.
    let ad = allan(y, g.step);
    if !ad.is_empty() {
        let a0 = ad[0].1;
        let white: Vec<f64> = ad.iter().map(|p| a0 / (p.0 / g.step).sqrt()).collect();
        let pts: Vec<(f64, f64)> = ad
            .iter()
            .take(4)
            .filter(|p| p.1 > 0.0)
            .map(|p| (p.0.ln(), p.1.ln()))
            .collect();
        let slope = (pts.len() >= 3).then(|| {
            let mx = pts.iter().map(|p| p.0).sum::<f64>() / pts.len() as f64;
            let my = pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64;
            let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
            let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
            sxy / sxx
        });
        let best = ad
            .iter()
            .filter(|p| p.2 >= 8)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let max_excess = ad
            .iter()
            .zip(&white)
            .filter(|(p, _)| p.2 >= 8)
            .map(|(p, w)| p.1 / w)
            .fold(1.0f64, f64::max);
        out.allan = Some(Allan {
            tau_s: ad.iter().map(|p| p.0).collect(),
            deviation: ad.iter().map(|p| p.1).collect(),
            white,
            pairs: ad.iter().map(|p| p.2).collect(),
            slope_short: slope,
            best_tau_s: best.map(|p| p.0),
            best_deviation: best.map(|p| p.1),
            max_excess,
        });
    }

    // CUSUM against the mean, scaled by the short-term σ so that slow
    // change cannot hide itself by inflating the scale.
    if sst > 0.0 {
        let scale = sst * nf.sqrt();
        let mut s = 0.0;
        let mut ts = Vec::with_capacity(n);
        let mut ss = Vec::with_capacity(n);
        for (i, &v) in y.iter().enumerate() {
            if v.is_finite() {
                s += (v - mean) / scale;
                ts.push(g.time(i));
                ss.push(s);
            }
        }
        let (k, m) = ss
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v.abs()))
            .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
        let stride = thin(ss.len(), cfg.max_points);
        out.cusum = Some(Cusum {
            max: m,
            at_s: ts[k],
            p_value: kolmogorov_tail(m),
            t_s: ts.iter().step_by(stride).copied().collect(),
            s: ss.iter().step_by(stride).copied().collect(),
        });
    }

    // Straight-line trend.
    let (mut sx, mut sy, mut sxx, mut sxy, mut c) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &v) in y.iter().enumerate() {
        if v.is_finite() {
            let t = g.time(i);
            sx += t;
            sy += v;
            sxx += t * t;
            sxy += t * v;
            c += 1.0;
        }
    }
    let slope = (c * sxy - sx * sy) / (c * sxx - sx * sx).max(1e-300);
    let icpt = (sy - slope * sx) / c;
    let line: Vec<f64> = (0..y.len()).map(|i| icpt + slope * g.time(i)).collect();
    let (ft, fy): (Vec<f64>, Vec<f64>) = (0..y.len())
        .filter(|&i| y[i].is_finite())
        .map(|i| (g.time(i), y[i]))
        .unzip();
    let qc = polyfit2(&ft, &fy);
    let curve_at = |t: f64| qc[0] + qc[1] * t + qc[2] * t * t;
    let curve: Vec<f64> = (0..y.len()).map(|i| curve_at(g.time(i))).collect();
    let (t_a, t_b) = (ft[0], ft[ft.len() - 1]);
    let vertex = -qc[1] / (2.0 * qc[2]);
    let trend = Trend {
        per_hour: slope * 3600.0,
        explained: explained(y, &line),
        curve_explained: explained(y, &curve),
        start: curve_at(t_a),
        end: curve_at(t_b),
        turn: (qc[2] != 0.0
            && vertex > t_a + 0.1 * (t_b - t_a)
            && vertex < t_b - 0.1 * (t_b - t_a))
            .then(|| (vertex, curve_at(vertex))),
    };

    // Change points on block medians.
    let per = ((cfg.change_block_s / g.step).round() as usize).max(1);
    let blocks: Vec<(usize, f64)> = (0..y.len().div_ceil(per))
        .filter_map(|b| {
            let mut v = finite(&y[b * per..((b + 1) * per).min(y.len())]);
            (2 * v.len() >= per.min(y.len() - b * per)).then(|| (b, median(&mut v)))
        })
        .collect();
    let bv: Vec<f64> = blocks.iter().map(|b| b.1).collect();
    if let Some(bs) = robust_diff_sigma(&bv).filter(|s| *s > 0.0) {
        let min_len = ((cfg.change_min_segment_s / (per as f64 * g.step)).ceil() as usize).max(3);
        // Strong evidence: well past a BIC penalty for each extra mean.
        let penalty = (3.0 * (bv.len() as f64).ln()).max(15.0);
        let cuts = segment(&bv, bs, min_len, penalty);
        let mut segs = Vec::new();
        let mut fitted = vec![f64::NAN; y.len()];
        for (j, &a) in cuts.iter().enumerate() {
            let b = cuts.get(j + 1).copied().unwrap_or(bv.len());
            let m = bv[a..b].iter().sum::<f64>() / (b - a) as f64;
            let i0 = blocks[a].0 * per;
            let i1 = ((blocks[b - 1].0 + 1) * per).min(y.len());
            for f in &mut fitted[i0..i1] {
                *f = m;
            }
            segs.push(Segment {
                start_s: g.t0 + i0 as f64 * g.step,
                end_s: g.t0 + i1 as f64 * g.step,
                mean: m,
            });
        }
        out.changes = Some(Changes {
            block_s: per as f64 * g.step,
            block_sigma: bs,
            explained: explained(y, &fitted),
            segments: segs,
        });
    }
    out.trend = Some(trend);

    out.cycle = cycle(&out, &g, wheels, cfg);
    out.verdict = verdict(&out, cfg);
    out.headline = headline(&out, mean);
    if let Some(more) = also(&out, cfg) {
        out.headline = format!("{} {more}", out.headline);
    }
    out
}

/// A second sentence for structure the verdict does not name: a cycle
/// under steps or a slide, or steps or a slide under a cycle.
fn also(s: &SeriesCheck, cfg: &Config) -> Option<String> {
    let k = s.series;
    let mut out = Vec::new();
    if let Some(c) = s.cycle.as_ref().filter(|_| s.verdict != Verdict::Periodic) {
        out.push(format!(
            "It also repeats every {:.0} s{}, {} peak to peak.",
            c.period_s,
            c.wheel
                .as_ref()
                .map_or(String::new(), |w| format!(" (the {w})")),
            k.fmt_size(c.peak_to_peak)
        ));
    }
    let moves = !matches!(s.verdict, Verdict::ShiftingMean | Verdict::Drifting);
    if let Some(ch) = s.changes.as_ref().filter(|_| moves) {
        if ch.segments.len() >= 2 && ch.explained >= cfg.min_explained {
            let lo = ch
                .segments
                .iter()
                .map(|g| g.mean)
                .fold(f64::INFINITY, f64::min);
            let hi = ch
                .segments
                .iter()
                .map(|g| g.mean)
                .fold(f64::NEG_INFINITY, f64::max);
            out.push(format!(
                "Its level also moves over the take, between {} and {}.",
                k.fmt(lo),
                k.fmt(hi)
            ));
        }
    }
    (!out.is_empty()).then(|| out.join(" "))
}

/// Fold the detrended readings at `period`: the share of their variance
/// the mean shape of one cycle explains, less what noise alone would give
/// (bins − 1 over the readings), and the shape's peak to peak.
fn fold(d: &Grid, period: f64) -> (f64, f64) {
    let bins = ((period / d.step).round() as usize).clamp(2, 30);
    let mut sum = vec![0.0; bins];
    let mut n = vec![0usize; bins];
    for (i, &v) in d.y.iter().enumerate() {
        if v.is_finite() {
            let x = (d.time(i) - d.t0) / period;
            let b = (((x - x.floor()) * bins as f64) as usize).min(bins - 1);
            sum[b] += v;
            n[b] += 1;
        }
    }
    let total: usize = n.iter().sum();
    if total < 2 * bins {
        return (0.0, 0.0);
    }
    let mean = sum.iter().sum::<f64>() / total as f64;
    let var =
        d.y.iter()
            .filter(|v| v.is_finite())
            .map(|v| (v - mean).powi(2))
            .sum::<f64>();
    let m: Vec<(f64, usize)> = sum
        .iter()
        .zip(&n)
        .filter(|(_, &c)| c > 0)
        .map(|(s, &c)| (s / c as f64, c))
        .collect();
    let between: f64 = m.iter().map(|(v, c)| *c as f64 * (v - mean).powi(2)).sum();
    let hi = m.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let lo = m.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let share = if var > 0.0 { between / var } else { 0.0 };
    (share - (bins - 1) as f64 / total as f64, hi - lo)
}

fn wheel_for(period: f64, wheels: &[Wheel]) -> Option<String> {
    wheels
        .iter()
        .find(|w| (period / w.period_s - 1.0).abs() < 0.06)
        .map(|w| w.name.clone())
}

/// The cycle a `periodic` verdict would rest on. Candidates come from
/// the two-state finder (a low level that comes back on a period), from
/// a repeat in the autocorrelation and from the period search; each is
/// tested by folding the readings at it, and the one whose cycle explains
/// the most is kept when that is enough. A period shorter than three
/// readings cannot show in the readings and is left to the period search
/// report.
fn cycle(s: &SeriesCheck, g: &Grid, wheels: &[Wheel], cfg: &Config) -> Option<Cycle> {
    let d = longterm::detrend(g, 1800.0);
    let mut cands: Vec<(f64, CycleSource)> = Vec::new();
    if let Some(t) = &s.two_state {
        if t.verdict == twostate::Verdict::Regular {
            if let Some(p) = t.period_s {
                cands.push((p, CycleSource::TwoState));
            }
        }
    }
    if let Some(l) = s.autocorrelation.as_ref().and_then(|a| a.repeat_lag_s) {
        // The lag is only as fine as one reading: try around it.
        let best = (0..=40)
            .map(|i| l * (0.8 + 0.01 * i as f64))
            .map(|p| (p, fold(&d, p).0))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        cands.push((best.0, CycleSource::Autocorrelation));
    }
    // Every later peak of the autocorrelation above three times its 95% band, even one
    // too weak to count as a repeat on its own: the fold below decides.
    if let Some(a) = &s.autocorrelation {
        for k in 2..a.r.len().saturating_sub(1) {
            if a.r[k] >= a.r[k - 1] && a.r[k] > a.r[k + 1] && a.r[k] >= 3.0 * a.band {
                let l = a.lag_s[k];
                let best = (0..=40)
                    .map(|i| l * (0.8 + 0.01 * i as f64))
                    .map(|p| (p, fold(&d, p).0))
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap();
                cands.push((best.0, CycleSource::Autocorrelation));
            }
        }
    }
    if let Some(p) = &s.period {
        cands.push((p.period_s, CycleSource::PeriodSearch));
        for &q in &p.other_periods_s {
            cands.push((q, CycleSource::PeriodSearch));
        }
    }
    let at = |p: f64, src: CycleSource| {
        let (explained, ptp) = fold(&d, p);
        Cycle {
            period_s: p,
            peak_to_peak: ptp,
            explained,
            source: src,
            wheel: wheel_for(p, wheels),
        }
    };
    let best = cands
        .into_iter()
        // At least three turns for the period search, which tests its own
        // significance; five for the others.
        .filter(|(p, src)| {
            let turns = if *src == CycleSource::PeriodSearch {
                3.0
            } else {
                5.0
            };
            *p >= 3.0 * g.step && *p <= g.span() / turns
        })
        .map(|(p, src)| at(p, src))
        .filter(|c| c.explained >= cfg.periodic_min_explained)
        .max_by(|a, b| a.explained.total_cmp(&b.explained))?;
    // Folding at five turns of a cycle explains as much as folding at one,
    // and the autocorrelation's lag grid can favour the multiple (a 48 s
    // cycle in 10 s readings lines up best at 240 s). Keep the shortest
    // whole fraction that explains at least 85% as much.
    let shortest = (2..=8)
        .rev()
        .map(|k| best.period_s / k as f64)
        .filter(|&p| p >= 3.0 * g.step)
        .find_map(|p| {
            let c = (-5..=5)
                .map(|i| at(p * (1.0 + 0.006 * i as f64), best.source))
                .max_by(|a, b| a.explained.total_cmp(&b.explained))?;
            (c.explained >= 0.85 * best.explained).then_some(c)
        });
    Some(shortest.unwrap_or(best))
}

fn verdict(s: &SeriesCheck, cfg: &Config) -> Verdict {
    use twostate::Verdict as T;
    match s.two_state.as_ref().map(|t| t.verdict) {
        Some(T::TwoStates) => return Verdict::TwoStates,
        Some(T::Measurement) => return Verdict::Measurement,
        _ => {}
    }
    if s.cycle.is_some() {
        return Verdict::Periodic;
    }
    let trend = s
        .trend
        .as_ref()
        .map_or(0.0, |t| t.explained.max(t.curve_explained));
    if let Some(ch) = &s.changes {
        if ch.segments.len() > 1 && ch.explained >= cfg.min_explained {
            // A slide cut into steps is still a slide.
            return if trend >= 0.8 * ch.explained {
                Verdict::Drifting
            } else {
                Verdict::ShiftingMean
            };
        }
    }
    if trend >= cfg.min_explained {
        return Verdict::Drifting;
    }
    let ac = s.autocorrelation.as_ref();
    let memory =
        ac.is_some_and(|a| a.p_value < cfg.alpha && a.max_short_lag_r >= cfg.min_correlation);
    let excess = s.allan.as_ref().is_some_and(|a| a.max_excess >= 2.0);
    if memory || excess {
        Verdict::Wandering
    } else {
        Verdict::Steady
    }
}

fn headline(s: &SeriesCheck, mean: f64) -> String {
    let k = s.series;
    let name = k.name();
    let cap = {
        let mut c = name.chars();
        c.next()
            .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    };
    let every = |sec: f64| {
        if sec >= 5400.0 {
            format!("{:.1} h", sec / 3600.0)
        } else if sec >= 120.0 {
            format!("{:.0} min", sec / 60.0)
        } else {
            format!("{sec:.0} s")
        }
    };
    match s.verdict {
        Verdict::TooShort => format!("Too few {name} readings to say whether they are steady."),
        Verdict::Steady => format!(
            "{cap} is steady: readings scatter independently about {} (±{} from one reading to the next).",
            k.fmt(s.median.unwrap_or(mean)),
            k.fmt_size(s.short_term_sd.unwrap_or(0.0))
        ),
                Verdict::Periodic => match (&s.cycle, &s.two_state) {
            (Some(p), _) => format!(
                "{cap} repeats every {}{}, {} peak to peak.",
                every(p.period_s),
                p.wheel
                    .as_ref()
                    .map_or(String::new(), |w| format!(" (the {w})")),
                k.fmt_size(p.peak_to_peak)
            ),
            (None, Some(t)) => format!(
                "{cap} drops to a low level and back on a regular cycle{}.",
                t.period_s
                    .map_or(String::new(), |p| format!(" of about {}", every(p)))
            ),
            _ => format!("{cap} repeats on a cycle."),
        },
        Verdict::TwoStates | Verdict::Measurement => {
            let t = s.two_state.as_ref().unwrap();
            let base = twostate::describe(t, if k == SeriesKind::Rate { "rate" } else { name });
            let mut c = base.chars();
            let base: String = c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect());
            if s.verdict == Verdict::Measurement {
                format!("{base}; this looks like the measurement, not the watch.")
            } else {
                format!("{base}.")
            }
        }
        Verdict::ShiftingMean => {
            let ch = s.changes.as_ref().unwrap();
            let means: Vec<String> = ch.segments.iter().map(|g| k.fmt(g.mean)).collect();
            format!(
                "{cap} shifts level {} time{}: {}.",
                ch.segments.len() - 1,
                if ch.segments.len() == 2 { "" } else { "s" },
                means.join(", then ")
            )
        }
                Verdict::Drifting => {
            let t = s.trend.as_ref().unwrap();
            if let Some((at, v)) = t.turn.filter(|_| t.curve_explained > t.explained + 0.05) {
                return format!(
                    "{cap} changes slowly: from {} to {} at {}, then to {} at the end.",
                    k.fmt(t.start),
                    k.fmt(v),
                    every(at),
                    k.fmt(t.end)
                );
            }
            format!(
                "{cap} slides slowly from {} to {} ({} per hour) rather than scattering about one level.",
                k.fmt(t.start),
                k.fmt(t.end),
                match k {
                    SeriesKind::Rate => format!("{:+.1} s/d", t.per_hour),
                    SeriesKind::Amplitude => format!("{:+.1}°", t.per_hour),
                    SeriesKind::BeatError => format!("{:+.2} ms", t.per_hour),
                }
            )
        }
        Verdict::Wandering => format!(
            "{cap} wanders: each reading remembers the last ones{}, with no cycle, steps or trend to account for it.",
            s.autocorrelation
                .as_ref()
                .and_then(|a| a.memory_s)
                .map_or(String::new(), |m| format!(" for about {}", every(m)))
        ),
    }
}

fn findings(s: &SeriesCheck) -> Option<Finding> {
    let k = s.series;
    let name = k.name();
    let (code, severity, title, advice) = match s.verdict {
        Verdict::Steady | Verdict::TooShort => return None,
        Verdict::Periodic => (
            "periodic",
            Severity::Warning,
            format!("The {name} repeats on a cycle"),
            "A change that comes back once per turn points at that wheel: a damaged tooth, an eccentric or bent wheel, or a pivot binding once a turn. Check the wheel the period matches.".to_string(),
        ),
        Verdict::TwoStates => (
            "two_states",
            Severity::Warning,
            format!("The {name} switches between two levels"),
            "A watch that flips between two rates or amplitudes is often touching something part of the time: a hairspring coil rubbing, a balance rubbing the cock or the pallet bridge, or a hand or the rotor catching. Look for the rub in this position.".to_string(),
        ),
        Verdict::Measurement => (
            "measurement_split",
            Severity::Note,
            format!("The {name} splits in two, but in the measurement"),
            "The unlock mark is hopping between two points of the sound, so the two levels are not the watch. Read the beat error from the drop and the amplitude with care on this take.".to_string(),
        ),
        Verdict::ShiftingMean => (
            "shifting_mean",
            Severity::Warning,
            format!("The {name} changed level during the take"),
            "A lasting step usually has a cause at that moment: the watch moved or was knocked, the room got noisier, or something in the movement settled. Check what happened at the times given.".to_string(),
        ),
        Verdict::Drifting => (
            "drifting",
            Severity::Note,
            format!("The {name} drifts through the take"),
            if k == SeriesKind::Amplitude {
                "Amplitude falls as the mainspring lets down; a fall of tens of degrees over a day is normal, a fast one in a few hours suggests a weak or dirty mainspring or barrel."
            } else {
                "A slow slide follows the mainspring letting down or the temperature changing. Compare readings taken at the same state of wind."
            }
            .to_string(),
        ),
        Verdict::Wandering => (
            "wandering",
            Severity::Note,
            format!("The {name} wanders"),
            "Readings that remember the last ones are not independent, so a short reading can mislead: average for longer (see the Allan deviation for how long) before judging.".to_string(),
        ),
    };
    let mut ev = Vec::new();
    if let Some(a) = &s.autocorrelation {
        ev.push(format!(
            "Ljung–Box p {} over {} lags, largest short-lag correlation {:.2}",
            p_text(a.p_value),
            a.ljung_box_lags,
            a.max_short_lag_r
        ));
    }
    if let Some(c) = &s.cycle {
        ev.push(format!(
            "cycle of {:.1} s found by the {}",
            c.period_s,
            match c.source {
                CycleSource::TwoState => "two-state finder",
                CycleSource::Autocorrelation => "autocorrelation",
                CycleSource::PeriodSearch => "period search",
            }
        ));
    }
    if let Some(p) = &s.period {
        ev.push(format!(
            "period {:.1} s explains {:.0}% of the detrended variance, false alarm {}",
            p.period_s,
            p.explained * 100.0,
            p_text(10f64.powf(-p.significance))
        ));
    }
    if let Some(c) = &s.changes {
        if c.segments.len() > 1 {
            let at: Vec<String> = c.segments[1..]
                .iter()
                .map(|g| format!("{:.0} s", g.start_s))
                .collect();
            ev.push(format!(
                "{} level change{} at {} explain {:.0}%",
                at.len(),
                if at.len() == 1 { "" } else { "s" },
                at.join(", "),
                c.explained * 100.0
            ));
        }
    }
    if let Some(t) = &s.trend {
        ev.push(format!(
            "a straight line explains {:.0}%, a curve {:.0}%",
            t.explained * 100.0,
            t.curve_explained * 100.0
        ));
    }
    if let Some(a) = &s.allan {
        ev.push(format!(
            "Allan deviation up to {:.1}× what independent readings give",
            a.max_excess
        ));
    }
    let (severity, advice) = if s.unlock_hopping {
        (
            Severity::Note,
            format!("{advice} First check the unlock mark with `timegrapher profile`: one side's unlock edges split in two, so this may be the measurement."),
        )
    } else {
        (severity, advice)
    };
    Some(Finding {
        code,
        series: k,
        severity,
        title,
        evidence: format!("{} Evidence: {}.", s.headline, ev.join("; ")),
        advice,
    })
}

pub fn p_text(p: f64) -> String {
    if p < 1e-300 {
        "<1e-300".into()
    } else if p < 0.001 {
        format!("{p:.0e}")
    } else {
        format!("{p:.3}")
    }
}

/// Check the rate, the amplitude and the beat error of a long-run
/// analysis. `log` and `clock` are what `long` was run on, and `long`
/// its report (whose period search is reused).
pub fn check(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    long: &LongReport,
    long_cfg: &LongConfig,
    cfg: &Config,
) -> Report {
    let map = |t: f64| clock.map_or(t, |c| c.map(t));
    let beats: Vec<Beat> = log
        .beats
        .iter()
        .map(|b| Beat {
            time: map(b.time),
            ..*b
        })
        .collect();

    // Rate readings.
    let r = cfg.rate_reading_s;
    let wins = timing::windows(&beats, log.bph, r, r);
    let rate_grid = match wins.first() {
        Some(w0) => {
            let t0 = w0.start_s;
            let last = wins.last().unwrap();
            let n = ((last.start_s - t0) / r).round() as usize + 1;
            let mut y = vec![f64::NAN; n];
            for w in &wins {
                y[((w.start_s - t0) / r).round() as usize] = w.fit.rate_s_per_day;
            }
            Grid { t0, step: r, y }
        }
        None => Grid {
            t0: 0.0,
            step: r,
            y: Vec::new(),
        },
    };
    let rate_samples: Vec<twostate::Sample> = wins
        .iter()
        .map(|w| twostate::Sample {
            start_s: w.start_s,
            end_s: w.end_s,
            value: w.fit.rate_s_per_day,
            ..Default::default()
        })
        .collect();

    // Amplitude and beat error windows.
    let aw = &log.amplitude_windows;
    let astep = aw.first().map_or(2.0, |w| w.end_s - w.start_s);
    let grid_of = |f: &dyn Fn(&crate::amplitude::AmplitudeWindow) -> Option<f64>| {
        let Some(w0) = aw.first() else {
            return Grid {
                t0: 0.0,
                step: astep,
                y: Vec::new(),
            };
        };
        let n = ((aw.last().unwrap().start_s - w0.start_s) / astep).round() as usize + 1;
        let mut y = vec![f64::NAN; n];
        for w in aw {
            let i = ((w.start_s - w0.start_s) / astep).round() as usize;
            if i < n {
                y[i] = f(w).unwrap_or(f64::NAN);
            }
        }
        Grid {
            t0: map(w0.start_s),
            step: astep,
            y,
        }
    };
    let amp_grid = grid_of(&|w| w.mean());
    let be_grid = grid_of(&|w| w.beat_error_unlock_ms);
    let amp_samples: Vec<twostate::Sample> = aw
        .iter()
        .filter_map(|w| {
            Some(twostate::Sample {
                start_s: map(w.start_s),
                end_s: map(w.end_s),
                value: w.mean()?,
                tick: w.even_deg,
                tock: w.odd_deg,
                beat_error_unlock_ms: w.beat_error_unlock_ms,
                beat_error_drop_ms: w.beat_error_ms,
                tick_unlock_ms: w.even_unlock_ms,
                tock_unlock_ms: w.odd_unlock_ms,
            })
        })
        .collect();

    let p = twostate::Params::default();
    let rate_period = long.rate_components.first().map(|c| {
        let (size, size_unit) = c.size();
        Period {
            period_s: c.component.period_s,
            significance: c.component.significance,
            explained: c.component.explained,
            peak_to_peak: c.rate_swing_s_per_day,
            timing_peak_to_peak_ms: Some(c.timing_swing_ms),
            size,
            size_unit,
            wheel: c.component.wheel.clone(),
            other_periods_s: long.rate_components[1..]
                .iter()
                .map(|c| c.component.period_s)
                .collect(),
        }
    });
    let amp_period = period_of(&long.amplitude, SeriesKind::Amplitude.unit());
    let be_period = {
        let mut lg = be_grid.clone();
        lg.t0 = 0.0;
        period_of(
            &longterm::analyse(&lg, long_cfg),
            SeriesKind::BeatError.unit(),
        )
    };

    let series = vec![
        check_series(
            SeriesKind::Rate,
            rate_grid,
            rate_period,
            Some(twostate::find(&rate_samples, &p)),
            &long_cfg.wheels,
            cfg,
        ),
        check_series(
            SeriesKind::Amplitude,
            amp_grid,
            amp_period,
            Some(twostate::find(&amp_samples, &p)),
            &long_cfg.wheels,
            cfg,
        ),
        check_series(
            SeriesKind::BeatError,
            be_grid,
            be_period,
            None,
            &long_cfg.wheels,
            cfg,
        ),
    ];
    let mut series = series;
    // An amplitude split one side carries alone is the unlock mark
    // hopping (the two-state finder's direct check): whatever the
    // amplitude and the beat error from the unlock seem to do may be the
    // measurement.
    let lone = series[1].two_state.as_ref().and_then(|t| {
        if t.tick_edge_split_alone == Some(true) || t.tick_split_alone == Some(true) {
            Some("Tick")
        } else if t.tock_edge_split_alone == Some(true) || t.tock_split_alone == Some(true) {
            Some("Tock")
        } else {
            None
        }
    });
    if let Some(side) = lone {
        for s in series.iter_mut().skip(1) {
            if !matches!(
                s.verdict,
                Verdict::Steady | Verdict::TooShort | Verdict::Measurement
            ) {
                s.unlock_hopping = true;
                s.headline = format!(
                    "{} The {side} windows fall in two clusters the other side does not follow, so the unlock mark may be hopping and this may be the measurement rather than the watch.",
                    s.headline
                );
            }
        }
    }
    let findings = series.iter().filter_map(findings).collect();
    Report {
        duration_s: long.duration_s,
        bph: log.bph,
        config: cfg.clone(),
        series,
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic noise, roughly Gaussian (sum of uniforms).
    struct Noise(u64);
    impl Noise {
        fn next(&mut self) -> f64 {
            let mut s = 0.0;
            for _ in 0..12 {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                s += (self.0 >> 11) as f64 / (1u64 << 53) as f64;
            }
            s - 6.0
        }
    }

    fn grid(y: Vec<f64>, step: f64) -> Grid {
        Grid { t0: 0.0, step, y }
    }

    #[test]
    fn tails() {
        assert!((chi2_tail(18.307, 10) - 0.05).abs() < 1e-3);
        assert!((chi2_tail(3.841, 1) - 0.05).abs() < 1e-3);
        assert!((kolmogorov_tail(1.358) - 0.05).abs() < 2e-3);
    }

    #[test]
    fn white_noise_is_steady() {
        let mut z = Noise(1);
        let y: Vec<f64> = (0..2000).map(|_| 250.0 + 3.0 * z.next()).collect();
        let s = check_series(
            SeriesKind::Amplitude,
            grid(y, 2.0),
            None,
            None,
            &[],
            &Config::default(),
        );
        assert_eq!(s.verdict, Verdict::Steady, "{}", s.headline);
        let a = s.allan.unwrap();
        assert!(a.max_excess < 1.5, "{}", a.max_excess);
        assert!(a.slope_short.unwrap() < -0.4);
    }

    #[test]
    fn a_step_is_a_shifting_mean() {
        let mut z = Noise(2);
        let y: Vec<f64> = (0..1800)
            .map(|i| if i < 1000 { 5.0 } else { 9.0 } + 2.0 * z.next())
            .collect();
        let s = check_series(
            SeriesKind::Rate,
            grid(y, 2.0),
            None,
            None,
            &[],
            &Config::default(),
        );
        assert_eq!(s.verdict, Verdict::ShiftingMean, "{}", s.headline);
        let ch = s.changes.unwrap();
        assert_eq!(ch.segments.len(), 2);
        assert!((ch.segments[1].start_s - 2000.0).abs() < 90.0);
        assert!((s.cusum.unwrap().at_s - 2000.0).abs() < 90.0);
    }

    #[test]
    fn a_slide_is_drifting() {
        let mut z = Noise(3);
        let y: Vec<f64> = (0..3600)
            .map(|i| 280.0 - 20.0 * i as f64 / 3600.0 + 2.0 * z.next())
            .collect();
        let s = check_series(
            SeriesKind::Amplitude,
            grid(y, 2.0),
            None,
            None,
            &[],
            &Config::default(),
        );
        assert_eq!(s.verdict, Verdict::Drifting, "{}", s.headline);
        assert!((s.trend.unwrap().per_hour + 10.0).abs() < 1.0);
    }

    #[test]
    fn a_random_walk_wanders() {
        let mut z = Noise(4);
        let mut ar = 0.0;
        let y: Vec<f64> = (0..3000)
            .map(|_| {
                // AR(1) with a 20-reading memory, no trend or steps.
                ar = 0.95 * ar + 0.3 * z.next();
                ar + z.next()
            })
            .collect();
        let s = check_series(
            SeriesKind::Rate,
            grid(y, 10.0),
            None,
            None,
            &[],
            &Config::default(),
        );
        assert_eq!(s.verdict, Verdict::Wandering, "{}", s.headline);
    }

    #[test]
    fn allan_of_white_noise_falls_as_root_tau() {
        let mut z = Noise(5);
        let y: Vec<f64> = (0..8192).map(|_| z.next()).collect();
        let a = allan(&y, 1.0);
        let (t, d, _) = a[4];
        assert!((d * t.sqrt() - a[0].1).abs() < 0.15 * a[0].1, "{a:?}");
    }
}
