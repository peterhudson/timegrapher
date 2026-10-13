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
use crate::distribution::{self, Distributions};
use crate::dsp::median;
use crate::longrun::LongReport;
use crate::longterm::{self, Grid, LongConfig};
use crate::periodicity::{polyfit2, Wheel};
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
    /// Hours since the watch was last fully wound when the take started,
    /// for the wind-down comparison.
    pub wind_h: f64,
    /// Length of the stretches the distributions are also counted over,
    /// seconds; 0 counts the whole take only.
    pub stretch_s: f64,
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
            wind_h: 0.0,
            stretch_s: 3600.0,
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

/// Which detected structure leads a series' headline. Internal only: it
/// is not in the JSON (schema /2 removed the one-word verdict), and says
/// nothing about whether the watch is good or bad.
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
    /// A shorter cycle inside this one, when the period is a whole number
    /// of its turns (`turns`) and it explains at least half as much on its
    /// own: the pattern changes from turn to turn and repeats every
    /// `turns` turns.
    pub inner_period_s: Option<f64>,
    pub inner_explained: Option<f64>,
    pub turns: Option<u32>,
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
    /// Which detected structure leads the headline; not serialized.
    #[serde(skip)]
    pub verdict: Verdict,
    /// One plain sentence of what was measured.
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
    /// The cycle found by the two-state finder, the autocorrelation or the
    /// period search, if any.
    pub cycle: Option<Cycle>,
    /// Amplitude and beat error only: one side's unlock edges or 2 s
    /// windows fall in two clusters the other side's do not (the two-state
    /// finder's check).
    pub unlock_hopping: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub series: SeriesKind,
    pub title: String,
    pub evidence: String,
}

/// The rate's median over the readings whose amplitude falls in one bin.
#[derive(Debug, Clone, Serialize)]
pub struct AmplitudeBin {
    /// Middle of the bin, degrees.
    pub amplitude_deg: f64,
    pub readings: usize,
    /// Median rate and its quartiles, s/d.
    pub rate_s_per_day: f64,
    pub rate_q25: f64,
    pub rate_q75: f64,
    /// When in the take the bin's readings fall: the first and the last,
    /// hours since full wind.
    pub from_wind_h: f64,
    pub to_wind_h: f64,
}

/// The isochronism curve: the rate by amplitude over the take, as the
/// mainspring lets down.
#[derive(Debug, Clone, Serialize)]
pub struct IsochronismCurve {
    /// Reading length, seconds, and bin width, degrees.
    pub step_s: f64,
    pub bin_deg: f64,
    /// Bins with enough readings, lowest amplitude first.
    pub bins: Vec<AmplitudeBin>,
    /// Amplitudes the take covers (2nd and 98th percentile), degrees.
    pub amplitude_from_deg: f64,
    pub amplitude_to_deg: f64,
    /// Straight line through the bins (weighted by readings): s/d per
    /// degree, and the rate change over the amplitudes covered, s/d.
    pub s_per_day_per_deg: f64,
    pub rate_change_s_per_day: f64,
    /// A quadratic through the bins: rate = c0 + c1·(A − centre) +
    /// c2·(A − centre)², and the share of the bins' spread it explains.
    pub curve_centre_deg: f64,
    pub curve: [f64; 3],
    pub curve_explained: f64,
    /// The amplitude where the curve turns (the rate changes least with
    /// amplitude), when it falls inside the amplitudes covered.
    pub turn_deg: Option<f64>,
    pub headline: String,
}

/// Rate, amplitude and beat error over one stretch of the take.
#[derive(Debug, Clone, Serialize)]
pub struct Stretch {
    /// Hours since full wind at the stretch's start and end.
    pub from_wind_h: f64,
    pub to_wind_h: f64,
    pub readings: usize,
    /// Medians of the readings.
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    pub beat_error_ms: Option<f64>,
}

/// The first half hour after the take starts against the half hour 24 h
/// later (or the take's last half hour when it is shorter): what the
/// commercial 0 h / 24 h test gives for one position.
#[derive(Debug, Clone, Serialize)]
pub struct WindDown {
    pub start: Stretch,
    pub end: Stretch,
    /// Hours between the two stretches' middles, and whether the end
    /// stretch is the one at 24 h.
    pub hours_apart: f64,
    pub at_24h: bool,
    /// End minus start.
    pub rate_change_s_per_day: Option<f64>,
    pub amplitude_change_deg: Option<f64>,
    pub beat_error_change_ms: Option<f64>,
    pub headline: String,
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
    /// Rate change per degree of amplitude within half an hour, s/d.
    pub s_per_day_per_deg: f64,
    /// The plain correlation and slope over the whole take at no lag,
    /// slow change included (a shared drift counts here).
    pub take_r: f64,
    pub take_s_per_day_per_deg: f64,
    pub headline: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub duration_s: f64,
    pub bph: u32,
    /// Pauses and resumes in the take; times run on with each gap in.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<crate::breaks::Break>,
    pub config: Config,
    pub series: Vec<SeriesCheck>,
    /// The rate against the amplitude, at the rate's reading length.
    pub rate_amplitude: Option<CrossCorrelation>,
    /// The rate by amplitude over the take: the isochronism curve.
    pub isochronism: Option<IsochronismCurve>,
    /// The start of the take against 24 h later, or its end.
    pub wind_down: Option<WindDown>,
    /// How often each value came up, over the take and each stretch: the
    /// app's Distributions pane as data.
    pub distributions: Distributions,
    /// The plain amplitude spectrum of each series: the rate's from the
    /// beat timing (ms), the amplitude's and the beat error's from their
    /// 2 s windows.
    pub spectra: Vec<crate::spectrum::AmplitudeSpectrum>,
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

/// Autocorrelation of readings on a uniform grid (gaps NaN), with the
/// Ljung–Box test, the memory and a repeat.
fn autocorrelation(y: &[f64], step: f64, cfg: &Config) -> Autocorrelation {
    let n = y.iter().filter(|v| v.is_finite()).count();
    let max_lag = ((cfg.max_lag_s / step) as usize)
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
        .map(|k| k as f64 * step);
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
    Autocorrelation {
        lag_s: (0..r.len())
            .step_by(stride)
            .map(|k| k as f64 * step)
            .collect(),
        r: r.iter().step_by(stride).copied().collect(),
        band,
        ljung_box_q: q,
        ljung_box_lags: h,
        p_value: lb_p,
        max_short_lag_r: max_short,
        repeat_lag_s: repeat.map(|k| k as f64 * step),
        repeat_r: repeat.map(|k| r[k]),
        memory_s: memory,
    }
}

/// Test one series. `g` is on a uniform grid of readings with gaps as NaN.
/// `fine`, when given, holds the same series in shorter windows: the
/// verdict is judged on `g`, but cycles are looked for in `fine`, which
/// keeps cycles too short for `g`'s readings.
#[allow(clippy::too_many_arguments)]
pub fn check_series(
    kind: SeriesKind,
    mut g: Grid,
    fine: Option<Grid>,
    period: Option<Period>,
    two_state: Option<TwoState>,
    wheels: &[Wheel],
    cfg: &Config,
) -> SeriesCheck {
    let outliers = drop_outliers(&mut g, cfg.outlier_sigma);
    let fine = fine.map(|mut f| {
        drop_outliers(&mut f, cfg.outlier_sigma);
        f
    });
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
            "{} {:.0} s {} readings over {:.0} s: too few to search for structure (needs 30 readings over 120 s).",
            n,
            g.step,
            kind.name(),
            g.span()
        );
        return out;
    }
    let (mean, sd) = mean_sd(y).unwrap();
    let sst = out.short_term_sd.unwrap_or(sd);

    let nf = n as f64;
    out.autocorrelation = Some(autocorrelation(y, g.step, cfg));

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

    out.cycle = cycle(&out, &g, fine.as_ref(), wheels, cfg);
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
            "It also repeats every {:.0} s{}{}, {} peak to peak.",
            c.period_s,
            c.wheel
                .as_ref()
                .map_or(String::new(), |w| format!(" (the {w})")),
            inner_text(c),
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
/// report. With `fine` given, the folds and the autocorrelation's
/// candidates come from its shorter windows.
fn cycle(
    s: &SeriesCheck,
    g: &Grid,
    fine: Option<&Grid>,
    wheels: &[Wheel],
    cfg: &Config,
) -> Option<Cycle> {
    let fine_ac = fine.map(|f| autocorrelation(&f.y, f.step, cfg));
    let ac = fine_ac.as_ref().or(s.autocorrelation.as_ref());
    let g = fine.unwrap_or(g);
    let d = longterm::detrend(g, 1800.0);
    let mut cands: Vec<(f64, CycleSource)> = Vec::new();
    if let Some(t) = &s.two_state {
        if t.verdict == twostate::Verdict::Regular {
            if let Some(p) = t.period_s {
                cands.push((refine(&d, p, 0.05), CycleSource::TwoState));
            }
        }
    }
    if let Some(l) = ac.and_then(|a| a.repeat_lag_s) {
        // The lag is only as fine as one reading: try around it.
        cands.push((refine(&d, l, 0.2), CycleSource::Autocorrelation));
    }
    // Every later peak of the autocorrelation above three times its 95% band, even one
    // too weak to count as a repeat on its own: the fold below decides.
    if let Some(a) = ac {
        for k in 2..a.r.len().saturating_sub(1) {
            if a.r[k] >= a.r[k - 1] && a.r[k] > a.r[k + 1] && a.r[k] >= 3.0 * a.band {
                cands.push((refine(&d, a.lag_s[k], 0.2), CycleSource::Autocorrelation));
            }
        }
    }
    if let Some(p) = &s.period {
        for &q in std::iter::once(&p.period_s).chain(&p.other_periods_s) {
            cands.push((refine(&d, q, 0.01), CycleSource::PeriodSearch));
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
            inner_period_s: None,
            inner_explained: None,
            turns: None,
        }
    };
    // Folding at five turns of a cycle explains as much as folding at one,
    // and the autocorrelation's lag grid can favour the multiple (a 48 s
    // cycle in 10 s readings lines up best at 240 s). Each candidate is
    // first reduced to its shortest whole fraction that explains at least
    // 85% as much; then the strongest wins. Reducing first keeps a slower,
    // stronger cycle from being swapped for a weaker fraction of another.
    let reduce = |best: Cycle| -> Cycle {
        // A fraction of a fraction too (540 s to 180 s to 60 s).
        let mut chosen = best;
        while let Some(c) = (2..=8u32)
            .rev()
            .map(|k| chosen.period_s / k as f64)
            .filter(|&p| p >= 3.0 * g.step)
            .map(|p| at(refine(&d, p, 0.03), chosen.source))
            .find(|c| c.explained >= 0.85 * chosen.explained)
        {
            chosen = c;
        }
        // When the longer period still wins, the shorter cycle inside it
        // can be the stronger single cycle: its shape changes from turn to
        // turn and repeats every few turns. Name it when it explains at
        // least half as much on its own, fits three or more times (half the
        // period would catch the cycle's own second harmonic) and is at
        // least 30 s (faster lines are the timing search's, in ms).
        if let Some(inner) = (3..=8u32)
            .map(|k| chosen.period_s / k as f64)
            .filter(|&p| p >= 3.0 * g.step && p >= crate::longrun::TIMING_UNIT_BELOW_S)
            .map(|p| at(refine(&d, p, 0.03), chosen.source))
            .max_by(|a, b| a.explained.total_cmp(&b.explained))
        {
            if inner.explained >= 0.5 * chosen.explained {
                chosen.inner_period_s = Some(inner.period_s);
                chosen.inner_explained = Some(inner.explained);
                chosen.turns = Some((chosen.period_s / inner.period_s).round() as u32);
            }
        }
        chosen
    };
    cands
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
        .map(reduce)
        .filter(|c| c.explained >= cfg.periodic_min_explained)
        .max_by(|a, b| a.explained.total_cmp(&b.explained))
}

/// ", N turns of a P s cycle" when the cycle holds a shorter one.
fn inner_text(c: &Cycle) -> String {
    match (c.inner_period_s, c.turns) {
        (Some(p), Some(n)) => {
            let words = [
                "", "one", "two", "three", "four", "five", "six", "seven", "eight",
            ];
            let n = words
                .get(n as usize)
                .map_or(n.to_string(), |w| w.to_string());
            format!(", {n} turns of a {p:.0} s cycle whose shape changes from turn to turn")
        }
        _ => String::new(),
    }
}

/// The period near `p` (within ± `rel` of it) whose fold explains the
/// most. A fold over a long take is sharp: a period off by P²/T drifts a
/// whole turn over a take of T seconds, so after a coarse scan the search
/// narrows until its step is a twentieth of that.
fn refine(d: &Grid, p: f64, rel: f64) -> f64 {
    let fine = (p * p / (20.0 * d.span().max(p))).max(1e-6 * p);
    let (mut lo, mut hi) = (p * (1.0 - rel), p * (1.0 + rel));
    let mut best = p;
    loop {
        let step = (hi - lo) / 40.0;
        best = (0..=40)
            .map(|i| lo + step * i as f64)
            .map(|q| (q, fold(d, q).0))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(best, |b| b.0);
        if step <= fine {
            return best;
        }
        lo = best - step;
        hi = best + step;
    }
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
    let step = every(s.step_s);
    match s.verdict {
        // check_series states the too-short case, with its rule, before
        // the tests run.
        Verdict::TooShort => s.headline.clone(),
        Verdict::Steady => format!(
            "{cap}: {} {step} readings, median {}, ±{} from one reading to the next; no cycle, level change, trend or correlation between readings detected.",
            s.readings,
            k.fmt(s.median.unwrap_or(mean)),
            k.fmt_size(s.short_term_sd.unwrap_or(0.0))
        ),
                Verdict::Periodic => match (&s.cycle, &s.two_state) {
            (Some(p), _) => format!(
                "{cap} repeats every {}{}{}, {} peak to peak.",
                every(p.period_s),
                p.wheel
                    .as_ref()
                    .map_or(String::new(), |w| format!(" (the {w})")),
                inner_text(p),
                k.fmt_size(p.peak_to_peak)
            ),
            (None, Some(t)) => format!(
                "{cap} drops to a low level and back on a cycle{}.",
                t.period_s
                    .map_or(String::new(), |p| format!(" of about {}", every(p)))
            ),
            _ => format!("{cap} repeats on a cycle."),
        },
        Verdict::TwoStates | Verdict::Measurement => {
            let t = s.two_state.as_ref().unwrap();
            let base = format!(
                "{cap} switches between two levels, {} and {}, about every {}",
                k.fmt(t.low),
                k.fmt(t.high),
                every(t.switch_every_s)
            );
            if s.verdict == Verdict::Measurement {
                let why = match lone_side(t) {
                    Some(side) => clusters(side),
                    None if t.unlock_jump => "the beat error from the unlock jumps between the levels; the one from the drop does not".to_string(),
                    None => {
                        let tick = t.tick_change.unwrap_or(0.0).abs();
                        let tock = t.tock_change.unwrap_or(0.0).abs();
                        let (a, b) = if tick > tock { ("Tick", "Tock") } else { ("Tock", "Tick") };
                        format!("the {a} side changes between the levels; the {b} side does not")
                    }
                };
                format!("{base}; {why}.")
            } else {
                format!("{base}.")
            }
        }
        Verdict::ShiftingMean => {
            let ch = s.changes.as_ref().unwrap();
            let n = ch.segments.len() - 1;
            let times = if n == 1 { "" } else { "s" };
            // A long take can shift dozens of times: past a few, name the
            // range and the ends, and leave each level to the table.
            if ch.segments.len() <= 4 {
                let means: Vec<String> = ch.segments.iter().map(|g| k.fmt(g.mean)).collect();
                format!("{cap} shifts level {n} time{times}: {}.", means.join(", then "))
            } else {
                let lo = ch.segments.iter().map(|g| g.mean).fold(f64::INFINITY, f64::min);
                let hi = ch.segments.iter().map(|g| g.mean).fold(f64::NEG_INFINITY, f64::max);
                format!(
                    "{cap} shifts level {n} time{times}, between {} and {}: {} at the start, {} at the end.",
                    k.fmt(lo),
                    k.fmt(hi),
                    k.fmt(ch.segments[0].mean),
                    k.fmt(ch.segments[n].mean)
                )
            }
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
                "{cap} slides from {} to {} ({} per hour).",
                k.fmt(t.start),
                k.fmt(t.end),
                match k {
                    SeriesKind::Rate => format!("{:+.1} s/d", t.per_hour),
                    SeriesKind::Amplitude => format!("{:+.1}°", t.per_hour),
                    SeriesKind::BeatError => format!("{:+.2} ms", t.per_hour),
                }
            )
        }
        Verdict::Wandering => {
            let mut parts = Vec::new();
            if let Some(a) = &s.autocorrelation {
                parts.push(format!(
                    "lag-1 correlation {:.2}, Ljung–Box p {}{}",
                    a.r.get(1).copied().unwrap_or(f64::NAN),
                    p_text(a.p_value),
                    a.memory_s
                        .map_or(String::new(), |m| format!(", correlated for about {}", every(m)))
                ));
            }
            if let Some(a) = s.allan.as_ref().filter(|a| a.max_excess >= 2.0) {
                parts.push(format!(
                    "Allan deviation up to {:.1}× what independent readings give",
                    a.max_excess
                ));
            }
            format!(
                "Successive {step} {name} readings are correlated ({}); no cycle, level change or trend detected.",
                parts.join("; ")
            )
        }
    }
}

fn findings(s: &SeriesCheck) -> Option<Finding> {
    let k = s.series;
    let name = k.name();
    let (code, title) = match s.verdict {
        Verdict::Steady | Verdict::TooShort => return None,
        Verdict::Periodic => ("periodic", format!("The {name} repeats on a cycle")),
        Verdict::TwoStates => (
            "two_states",
            format!("The {name} switches between two levels"),
        ),
        Verdict::Measurement => (
            "measurement_split",
            format!("The {name} switches between two levels; the change shows on one side only, or in the unlock but not the drop"),
        ),
        Verdict::ShiftingMean => (
            "shifting_mean",
            format!("The {name} changes level during the take"),
        ),
        Verdict::Drifting => ("drifting", format!("The {name} slides through the take")),
        Verdict::Wandering => (
            "wandering",
            format!("Successive {name} readings are correlated"),
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
    Some(Finding {
        code,
        series: k,
        title,
        evidence: format!("{} Evidence: {}.", s.headline, ev.join("; ")),
    })
}

/// The side whose unlock edges or 2 s windows fall in two clusters the
/// other side's do not, if one does.
fn lone_side(t: &TwoState) -> Option<&'static str> {
    if t.tick_edge_split_alone == Some(true) || t.tick_split_alone == Some(true) {
        Some("Tick")
    } else if t.tock_edge_split_alone == Some(true) || t.tock_split_alone == Some(true) {
        Some("Tock")
    } else {
        None
    }
}

/// "Tick windows fall in two clusters; Tock windows do not".
fn clusters(side: &str) -> String {
    let other = if side == "Tick" { "Tock" } else { "Tick" };
    format!("{side} windows fall in two clusters; {other} windows do not")
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
    let map = crate::longrun::true_time(clock, &long.breaks);
    let cuts = crate::longrun::cuts(clock, &long.breaks);
    let beats: Vec<Beat> = log
        .beats
        .iter()
        .map(|b| Beat {
            time: map(b.time),
            ..*b
        })
        .collect();

    // Rate readings; none reaches across a break.
    let r = cfg.rate_reading_s;
    let mut wins = timing::windows(&beats, log.bph, r, r);
    wins.retain(|w| !cuts.iter().any(|&c| w.start_s < c && c < w.end_s));
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
        // With breaks, on true time, so each gap is left empty.
        let pos = |t: f64| if long.breaks.is_empty() { t } else { map(t) };
        let n = ((pos(aw.last().unwrap().start_s) - pos(w0.start_s)) / astep).round() as usize + 1;
        let mut y = vec![f64::NAN; n];
        for w in aw {
            let i = ((pos(w.start_s) - pos(w0.start_s)) / astep).round() as usize;
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
    // The verdicts judge every series at the rate's reading length: the
    // amplitude and beat error windows are averaged into the rate's
    // readings, so one reading means the same time for all three. The
    // short windows are kept for finding cycles.
    let (t0, n) = if rate_grid.y.is_empty() {
        (amp_grid.t0, (amp_grid.span() / r).ceil() as usize)
    } else {
        (rate_grid.t0, rate_grid.y.len())
    };
    let amp_readings = rebin(&amp_grid, t0, r, n);
    let be_readings = rebin(&be_grid, t0, r, n);
    let rate_amplitude = cross_correlation(&rate_grid, &amp_readings, cfg);
    let isochronism = isochronism_curve(&rate_grid, &amp_readings, cfg);
    let wind_down = wind_down(&rate_grid, &amp_readings, &be_readings, cfg);
    let distributions = {
        use distribution::{Input, AMPLITUDE, BEAT_ERROR, RATE};
        let grids = [&rate_grid, &amp_grid];
        let start = grids
            .iter()
            .filter(|g| !g.y.is_empty())
            .map(|g| g.t0)
            .fold(f64::INFINITY, f64::min);
        let end = grids
            .iter()
            .filter(|g| !g.y.is_empty())
            .map(|g| g.t0 + g.span())
            .fold(f64::NEG_INFINITY, f64::max);
        let input = |kind, label, grid| Input { kind, label, grid };
        let inputs = [
            input(RATE, "readings", &rate_grid),
            input(AMPLITUDE, "readings", &amp_readings),
            input(AMPLITUDE, "2 s", &amp_grid),
            input(BEAT_ERROR, "readings", &be_readings),
            input(BEAT_ERROR, "2 s", &be_grid),
        ];
        if start.is_finite() {
            distribution::distributions(&inputs, start, end, cfg.stretch_s)
        } else {
            distribution::distributions(&[], 0.0, 0.0, 0.0)
        }
    };
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
    let be_long = {
        let mut lg = be_grid.clone();
        lg.t0 = 0.0;
        longterm::analyse(&lg, long_cfg)
    };
    let be_period = period_of(&be_long, SeriesKind::BeatError.unit());
    // The plain spectra of the series the period searches read, with
    // their slow change taken off as the searches take it off.
    let spectra = [
        (&long.timing.detrended, "rate", "ms", 1000.0),
        (&long.amplitude.detrended, "amplitude", "deg", 1.0),
        (&be_long.detrended, "beat_error", "ms", 1.0),
    ]
    .into_iter()
    .filter_map(|(g, name, unit, scale)| {
        crate::spectrum::amplitude_spectrum(g, name, unit, scale, cfg.max_points)
    })
    .collect();

    let series = vec![
        check_series(
            SeriesKind::Rate,
            rate_grid,
            None,
            rate_period,
            Some(twostate::find(&rate_samples, &p)),
            &long_cfg.wheels,
            cfg,
        ),
        check_series(
            SeriesKind::Amplitude,
            amp_readings,
            Some(amp_grid),
            amp_period,
            Some(twostate::find(&amp_samples, &p)),
            &long_cfg.wheels,
            cfg,
        ),
        check_series(
            SeriesKind::BeatError,
            be_readings,
            Some(be_grid),
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
    let lone = series[1].two_state.as_ref().and_then(lone_side);
    if let Some(side) = lone {
        for s in series.iter_mut().skip(1) {
            if !matches!(
                s.verdict,
                Verdict::Steady | Verdict::TooShort | Verdict::Measurement
            ) {
                s.unlock_hopping = true;
                s.headline = format!("{} {}.", s.headline, {
                    let c = clusters(side);
                    let mut ch = c.chars();
                    ch.next().map_or(String::new(), |f| {
                        f.to_uppercase().chain(ch).collect::<String>()
                    })
                });
            }
        }
    }
    let findings: Vec<Finding> = series.iter().filter_map(findings).collect();
    Report {
        duration_s: long.duration_s,
        bph: log.bph,
        breaks: long.breaks.clone(),
        config: cfg.clone(),
        series,
        rate_amplitude,
        isochronism,
        wind_down,
        distributions,
        spectra,
        findings,
    }
}

/// Average the windows of `fine` into `n` readings of `step` seconds
/// from `t0`. A reading needs at least half its windows.
fn rebin(fine: &Grid, t0: f64, step: f64, n: usize) -> Grid {
    let mut sum = vec![0.0; n];
    let mut count = vec![0usize; n];
    for (i, &v) in fine.y.iter().enumerate() {
        let b = ((fine.time(i) - t0) / step).floor();
        if v.is_finite() && b >= 0.0 && (b as usize) < n {
            sum[b as usize] += v;
            count[b as usize] += 1;
        }
    }
    let need = ((step / fine.step / 2.0).ceil() as usize).max(1);
    Grid {
        t0,
        step,
        y: sum
            .iter()
            .zip(&count)
            .map(|(s, &c)| if c >= need { s / c as f64 } else { f64::NAN })
            .collect(),
    }
}

/// Correlate the rate with the amplitude, each reading against the same
/// reading (no lag: the rate answers the amplitude at once, and a lag
/// search on a shared cycle finds the half-turn where the two
/// anticorrelate). Both grids share `t0` and step.
/// The test is on the changes within half an hour (both series with
/// their slow change taken out): two series that both drift correlate
/// whether or not one drives the other, and their drift leaves too few
/// independent readings to tell. The whole-take slope is given beside it.
fn cross_correlation(rate: &Grid, amp: &Grid, cfg: &Config) -> Option<CrossCorrelation> {
    let (mut x, mut y) = (amp.clone(), rate.clone());
    drop_outliers(&mut x, cfg.outlier_sigma);
    drop_outliers(&mut y, cfg.outlier_sigma);
    let (xd, yd) = (longterm::detrend(&x, 1800.0), longterm::detrend(&y, 1800.0));
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
    let (r, slope) = stats(&p);
    if !r.is_finite() {
        return None;
    }
    let (take_r, take_slope) = stats(&pairs(&x, &y));
    let n = p.len() as f64;
    let rho = |g: &Grid| {
        acf(&g.y, 1)
            .get(1)
            .copied()
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
    };
    let rr = (rho(&xd) * rho(&yd)).clamp(-0.99, 0.99);
    let n_eff = (n * (1.0 - rr) / (1.0 + rr)).clamp(4.0, n);
    let z = r.clamp(-0.999_999, 0.999_999).atanh() * (n_eff - 3.0).sqrt();
    let p_value = chi2_tail(z * z, 1);
    let step = rate.step;
    let headline = format!(
        "Rate against amplitude from one {:.0} s reading to the next: {:+.2} s/d per degree, r {:.2}, p {} over {} pairs (worth about {:.0} independent ones); over the whole take, {:+.2} s/d per degree (r {:.2}).",
        step,
        slope,
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
        s_per_day_per_deg: slope,
        take_r: finite_or_nan(take_r),
        take_s_per_day_per_deg: finite_or_nan(take_slope),
        headline,
    })
}

fn finite_or_nan(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        f64::NAN
    }
}

/// Amplitude bin width for the isochronism curve, degrees, and the
/// readings a bin needs.
const ISO_BIN_DEG: f64 = 2.0;
const ISO_MIN_READINGS: usize = 10;

fn quantile(sorted: &[f64], q: f64) -> f64 {
    let x = q * (sorted.len() - 1) as f64;
    let (i, f) = (x.floor() as usize, x - x.floor());
    let j = (i + 1).min(sorted.len() - 1);
    sorted[i] * (1.0 - f) + sorted[j] * f
}

/// Weighted least squares of y on 1, x, x²; returns the coefficients.
#[allow(clippy::needless_range_loop)]
fn wquad(x: &[f64], y: &[f64], w: &[f64]) -> Option<[f64; 3]> {
    let mut a = [[0.0f64; 3]; 3];
    let mut b = [0.0f64; 3];
    for ((&xi, &yi), &wi) in x.iter().zip(y).zip(w) {
        let r = [1.0, xi, xi * xi];
        for i in 0..3 {
            b[i] += wi * r[i] * yi;
            for j in 0..3 {
                a[i][j] += wi * r[i] * r[j];
            }
        }
    }
    for c in 0..3 {
        let p = (c..3).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        a.swap(c, p);
        b.swap(c, p);
        if a[c][c].abs() < 1e-12 {
            return None;
        }
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

/// The rate binned by amplitude, reading against the same reading. Needs
/// at least three bins with enough readings.
fn isochronism_curve(rate: &Grid, amp: &Grid, cfg: &Config) -> Option<IsochronismCurve> {
    let (mut x, mut y) = (amp.clone(), rate.clone());
    drop_outliers(&mut x, cfg.outlier_sigma);
    drop_outliers(&mut y, cfg.outlier_sigma);
    let pairs: Vec<(f64, f64, f64)> = (0..y.y.len().min(x.y.len()))
        .filter(|&i| x.y[i].is_finite() && y.y[i].is_finite())
        .map(|i| (x.y[i], y.y[i], cfg.wind_h + y.time(i) / 3600.0))
        .collect();
    if pairs.len() < 3 * ISO_MIN_READINGS {
        return None;
    }
    let mut amps: Vec<f64> = pairs.iter().map(|p| p.0).collect();
    amps.sort_by(f64::total_cmp);
    let (lo, hi) = (quantile(&amps, 0.02), quantile(&amps, 0.98));
    let mut bins: Vec<AmplitudeBin> = Vec::new();
    let first = (lo / ISO_BIN_DEG).floor() as i64;
    let last = (hi / ISO_BIN_DEG).floor() as i64;
    for b in first..=last {
        let (a0, a1) = (b as f64 * ISO_BIN_DEG, (b + 1) as f64 * ISO_BIN_DEG);
        let inbin: Vec<&(f64, f64, f64)> = pairs.iter().filter(|p| p.0 >= a0 && p.0 < a1).collect();
        if inbin.len() < ISO_MIN_READINGS {
            continue;
        }
        let mut r: Vec<f64> = inbin.iter().map(|p| p.1).collect();
        r.sort_by(f64::total_cmp);
        let hours = inbin.iter().map(|p| p.2);
        bins.push(AmplitudeBin {
            amplitude_deg: (a0 + a1) / 2.0,
            readings: inbin.len(),
            rate_s_per_day: quantile(&r, 0.5),
            rate_q25: quantile(&r, 0.25),
            rate_q75: quantile(&r, 0.75),
            from_wind_h: hours.clone().fold(f64::INFINITY, f64::min),
            to_wind_h: hours.fold(f64::NEG_INFINITY, f64::max),
        });
    }
    if bins.len() < 3 {
        return None;
    }
    let w: Vec<f64> = bins.iter().map(|b| b.readings as f64).collect();
    let sw: f64 = w.iter().sum();
    let centre = bins
        .iter()
        .zip(&w)
        .map(|(b, w)| b.amplitude_deg * w)
        .sum::<f64>()
        / sw;
    let bx: Vec<f64> = bins.iter().map(|b| b.amplitude_deg - centre).collect();
    let by: Vec<f64> = bins.iter().map(|b| b.rate_s_per_day).collect();
    let my = by.iter().zip(&w).map(|(y, w)| y * w).sum::<f64>() / sw;
    let sxx: f64 = bx.iter().zip(&w).map(|(x, w)| w * x * x).sum();
    let sxy: f64 = bx
        .iter()
        .zip(&by)
        .zip(&w)
        .map(|((x, y), w)| w * x * (y - my))
        .sum();
    let slope = sxy / sxx;
    let curve = wquad(&bx, &by, &w)?;
    let at = |x: f64| curve[0] + curve[1] * x + curve[2] * x * x;
    let sst: f64 = by.iter().zip(&w).map(|(y, w)| w * (y - my).powi(2)).sum();
    let sse: f64 = bx
        .iter()
        .zip(&by)
        .zip(&w)
        .map(|((x, y), w)| w * (y - at(*x)).powi(2))
        .sum();
    let curve_explained = if sst > 0.0 { 1.0 - sse / sst } else { 0.0 };
    let (b0, b1) = (bins[0].amplitude_deg, bins[bins.len() - 1].amplitude_deg);
    let turn = (curve[2] != 0.0)
        .then(|| centre - curve[1] / (2.0 * curve[2]))
        .filter(|t| *t > b0 + 0.1 * (b1 - b0) && *t < b1 - 0.1 * (b1 - b0));
    let change = slope * (b1 - b0);
    let headline = format!(
        "Between {:.0}° and {:.0}° the rate moves {:+.2} s/d per degree of amplitude: {:+.1} s/d at {:.0}° and {:+.1} s/d at {:.0}°{}.",
        b0,
        b1,
        slope,
        by[0],
        b0,
        by[by.len() - 1],
        b1,
        turn.map_or(String::new(), |t| format!(
            "; it changes least near {t:.0}°"
        ))
    );
    Some(IsochronismCurve {
        step_s: rate.step,
        bin_deg: ISO_BIN_DEG,
        bins,
        amplitude_from_deg: lo,
        amplitude_to_deg: hi,
        s_per_day_per_deg: slope,
        rate_change_s_per_day: change,
        curve_centre_deg: centre,
        curve,
        curve_explained,
        turn_deg: turn,
        headline,
    })
}

/// Length of each stretch in the wind-down comparison, seconds.
const WIND_STRETCH_S: f64 = 1800.0;

fn stretch(rate: &Grid, amp: &Grid, be: &Grid, from_s: f64, cfg: &Config) -> Stretch {
    let to_s = from_s + WIND_STRETCH_S;
    let med = |g: &Grid| {
        let mut v: Vec<f64> = (0..g.y.len())
            .filter(|&i| g.time(i) >= from_s && g.time(i) < to_s && g.y[i].is_finite())
            .map(|i| g.y[i])
            .collect();
        (v.len() >= 30).then(|| median(&mut v))
    };
    Stretch {
        from_wind_h: cfg.wind_h + from_s / 3600.0,
        to_wind_h: cfg.wind_h + to_s / 3600.0,
        readings: (0..rate.y.len())
            .filter(|&i| rate.time(i) >= from_s && rate.time(i) < to_s && rate.y[i].is_finite())
            .count(),
        rate_s_per_day: med(rate),
        amplitude_deg: med(amp),
        beat_error_ms: med(be),
    }
}

/// The first half hour against the half hour 24 h after full wind, or the
/// last half hour when the take ends sooner. Needs a take of at least two
/// hours.
fn wind_down(rate: &Grid, amp: &Grid, be: &Grid, cfg: &Config) -> Option<WindDown> {
    if rate.y.is_empty() {
        return None;
    }
    let t_end = rate.t0 + rate.span();
    let t_start = rate.t0;
    if t_end - t_start < 2.0 * 3600.0 {
        return None;
    }
    let at24 = t_start + (24.0 - cfg.wind_h) * 3600.0;
    let (end_from, at_24h) = if at24 > t_start + WIND_STRETCH_S && at24 + WIND_STRETCH_S <= t_end {
        (at24, true)
    } else {
        (t_end - WIND_STRETCH_S, false)
    };
    let start = stretch(rate, amp, be, t_start, cfg);
    let end = stretch(rate, amp, be, end_from, cfg);
    let diff = |a: Option<f64>, b: Option<f64>| Some(b? - a?);
    let rate_change = diff(start.rate_s_per_day, end.rate_s_per_day);
    let amplitude_change = diff(start.amplitude_deg, end.amplitude_deg);
    let beat_error_change = diff(start.beat_error_ms, end.beat_error_ms);
    let hours_apart = (end_from - t_start) / 3600.0;
    let headline = match (rate_change, amplitude_change) {
        (Some(r), Some(a)) => format!(
            "From {:.1} h to {:.1} h after full wind the rate moves {:+.1} s/d ({:+.1} to {:+.1}) and the amplitude {:+.0}° ({:.0}° to {:.0}°){}.",
            start.from_wind_h,
            end.from_wind_h,
            r,
            start.rate_s_per_day.unwrap_or(f64::NAN),
            end.rate_s_per_day.unwrap_or(f64::NAN),
            a,
            start.amplitude_deg.unwrap_or(f64::NAN),
            end.amplitude_deg.unwrap_or(f64::NAN),
            beat_error_change.map_or(String::new(), |b| format!(", beat error {b:+.2} ms"))
        ),
        _ => "Too few readings at the start or the end to compare them.".to_string(),
    };
    Some(WindDown {
        start,
        end,
        hours_apart,
        at_24h,
        rate_change_s_per_day: rate_change,
        amplitude_change_deg: amplitude_change,
        beat_error_change_ms: beat_error_change,
        headline,
    })
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

    #[test]
    fn a_long_take_names_the_cycle_inside_a_slower_one() {
        // 16 h of 10 s readings: a 48.004 s cycle whose shape changes from
        // turn to turn and repeats every five turns. Over 16 h a period
        // off by 0.02 s smears the fold, so the period must be found
        // finely.
        let mut z = Noise(7);
        let tau = std::f64::consts::TAU;
        let y: Vec<f64> = (0..5700)
            .map(|i| {
                let t = (i as f64 + 0.5) * 10.0;
                3.0 * (tau * t / 48.004).sin() + 2.0 * (tau * t / 240.02).sin() + 2.0 * z.next()
            })
            .collect();
        let s = check_series(
            SeriesKind::Rate,
            grid(y, 10.0),
            None,
            None,
            None,
            &[],
            &Config::default(),
        );
        assert_eq!(s.verdict, Verdict::Periodic, "{}", s.headline);
        let c = s.cycle.unwrap();
        assert!((c.period_s - 240.02).abs() < 0.1, "{c:?}");
        assert!((c.inner_period_s.unwrap() - 48.004).abs() < 0.02, "{c:?}");
        assert_eq!(c.turns, Some(5));
        assert!(
            s.headline.contains("five turns of a 48 s cycle"),
            "{}",
            s.headline
        );
    }

    #[test]
    fn isochronism_curve_and_wind_down_follow_the_run_down() {
        // 26 h of 10 s readings: amplitude falls from 270° to 218°, and
        // the rate follows it at 0.5 s/d per degree.
        let mut z = Noise(11);
        let n = 26 * 360;
        let amp: Vec<f64> = (0..n)
            .map(|i| 270.0 - 52.0 * i as f64 / n as f64 + 2.0 * z.next())
            .collect();
        let rate: Vec<f64> = amp
            .iter()
            .map(|a| 0.5 * (a - 250.0) + 1.5 * z.next())
            .collect();
        let be = vec![0.3; n];
        let cfg = Config::default();
        let c =
            isochronism_curve(&grid(rate.clone(), 10.0), &grid(amp.clone(), 10.0), &cfg).unwrap();
        assert!((c.s_per_day_per_deg - 0.5).abs() < 0.05, "{}", c.headline);
        assert!(c.bins.len() >= 20);
        let w = wind_down(&grid(rate, 10.0), &grid(amp, 10.0), &grid(be, 10.0), &cfg).unwrap();
        assert!(w.at_24h, "{}", w.headline);
        // 24 h of 26 at 2° an hour: about −48° and −24 s/d.
        let da = w.amplitude_change_deg.unwrap();
        assert!((da + 48.0).abs() < 3.0, "{}", w.headline);
        assert!(
            (w.rate_change_s_per_day.unwrap() + 24.0).abs() < 2.0,
            "{}",
            w.headline
        );
        assert!(w.beat_error_change_ms.unwrap().abs() < 1e-9);
    }
}
