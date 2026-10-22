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
//!   of period P puts a bump near 0.37 P.
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
//! the two-state finder, and one headline is drawn from all of them.

use crate::clean::Cuts;
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
    /// A period is what the headline names when it explains this share of the
    /// detrended variance.
    pub periodic_min_explained: f64,
    /// Block length for the change points, seconds, and the shortest
    /// stretch of constant mean.
    pub change_block_s: f64,
    pub change_min_segment_s: f64,
    /// A trend or a set of steps is what the headline names when it
    /// explains this share of the variance.
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

/// Which detected structure leads a series' headline and names its
/// finding. Private: it is in no output and says nothing about whether
/// the watch is good or bad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Lead {
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
    /// Too few readings to search.
    #[default]
    TooShort,
}

#[derive(Debug, Clone, Serialize)]
pub struct Autocorrelation {
    /// Lags, seconds, and the correlation at each.
    pub lag_s: Vec<f64>,
    pub r: Vec<f64>,
    /// Half-width of the 95% band for independent readings, 1.96/√n.
    pub band: f64,
    /// The same band at each of `lag_s`, from the pairs that lag was read
    /// from, 1.96/√pairs: wider than `band` where gaps leave fewer pairs.
    pub band_by_lag: Vec<f64>,
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

/// The cycle behind a `periodic` headline.
#[derive(Debug, Clone, Serialize)]
pub struct Cycle {
    pub period_s: f64,
    /// Peak to peak of the readings folded at the period, the series'
    /// units.
    pub peak_to_peak: f64,
    /// Share of the detrended variance the cycle's shape explains, above
    /// what noise alone would give.
    pub explained: f64,
    /// Where the period was found; for a cycle moved to half the period
    /// found (an odd number of readings a turn), where that period was.
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
    /// The longer found cycle this one's period is a whole fraction of,
    /// and the same for each of `other_periods_s`.
    pub harmonic_of: Option<longterm::Harmonic>,
    pub other_harmonic_of: Vec<Option<longterm::Harmonic>>,
}

/// Everything about one series.
#[derive(Debug, Clone, Serialize)]
pub struct SeriesCheck {
    pub series: SeriesKind,
    pub unit: &'static str,
    #[serde(skip)]
    lead: Lead,
    /// One plain sentence of what was measured.
    pub headline: String,
    /// Length of each reading, seconds; readings used; readings left out
    /// as outliers.
    pub step_s: f64,
    /// Length of the readings cycles are folded in: `step_s`, or the
    /// shorter windows (2 s) where the amplitude and beat error have them.
    #[serde(skip)]
    cycle_step_s: f64,
    pub readings: usize,
    pub outliers: usize,
    pub median: Option<f64>,
    pub sd: Option<f64>,
    /// Scatter from one reading to the next, √(½·mean of squared
    /// differences): the noise without the slow changes.
    pub short_term_sd: Option<f64>,
    /// Null, like the other tests, when there are too few readings to
    /// search (the headline says how many).
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
    /// The long-run analysis's conditions detected ([`LongReport`]'s).
    pub caveats: Vec<crate::measure::Caveat>,
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

pub(crate) fn ln_gamma(x: f64) -> f64 {
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
    acf_pairs(y, max_lag).0
}

/// [`acf`], and the pairs each lag was read from.
fn acf_pairs(y: &[f64], max_lag: usize) -> (Vec<f64>, Vec<usize>) {
    let Some((m, _)) = mean_sd(y) else {
        return (Vec::new(), Vec::new());
    };
    let x: Vec<f64> = y.iter().map(|v| v - m).collect();
    let mut out = Vec::with_capacity(max_lag + 1);
    let mut pairs = Vec::with_capacity(max_lag + 1);
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
        pairs.push(n);
    }
    (out, pairs)
}

/// Overlapping Allan deviation of a series of averages (frequency-type
/// data) at octave multiples of one reading. A block counts only when
/// every one of its readings exists: one at the edge of a gap is left
/// out, not averaged over what is left.
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
        (c == m).then(|| (sum[a + m] - sum[a]) / c as f64)
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
        harmonic_of: c.harmonic_of,
        other_harmonic_of: r.components[1..].iter().map(|c| c.harmonic_of).collect(),
    })
}

/// Autocorrelation of readings on a uniform grid (gaps NaN), with the
/// Ljung–Box test, the memory and a repeat.
fn autocorrelation(y: &[f64], step: f64, cfg: &Config) -> Autocorrelation {
    let n = y.iter().filter(|v| v.is_finite()).count();
    let max_lag = ((cfg.max_lag_s / step) as usize)
        .min(y.len() / 3)
        .max(cfg.ljung_box_lags.min(y.len() / 3));
    let (r, pairs) = acf_pairs(y, max_lag);
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
        band_by_lag: pairs
            .iter()
            .step_by(stride)
            .map(|&k| {
                if k > 0 {
                    1.96 / (k as f64).sqrt()
                } else {
                    f64::NAN
                }
            })
            .collect(),
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
/// headline is read from `g`, but cycles are looked for in `fine`, which
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
        lead: Lead::TooShort,
        headline: String::new(),
        step_s: g.step,
        cycle_step_s: fine.as_ref().map_or(g.step, |f| f.step),
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
    out.lead = lead(&out, cfg);
    out.headline = lead_text(&out, mean);
    if let Some(more) = also(&out, cfg) {
        out.headline = format!("{} {more}", out.headline);
    }
    if let Some(fast) = fast_sentence(&out) {
        out.headline = format!("{} {fast}", out.headline);
    }
    if let Some(more) = search_also(&out) {
        out.headline = format!("{} {more}", out.headline);
    }
    out
}

/// The period search's lines the headline does not name: not the cycle
/// (or the shorter cycle inside it), and not the fast line, within the resolution (period²
/// over the take, at least 1%). The search lists only lines that pass
/// its own significance test.
fn search_also(s: &SeriesCheck) -> Option<String> {
    let p = s.period.as_ref()?;
    let span = s.readings as f64 * s.step_s;
    let near = |a: f64, b: f64| (a - b).abs() <= (b * b / span).max(0.01 * b);
    let named: Vec<f64> = s
        .cycle
        .iter()
        .flat_map(|c| [Some(c.period_s), c.inner_period_s])
        .flatten()
        .collect();
    let fast = s.faster_than_readings().is_some();
    // Seconds throughout: minutes rounded would make 225 s and 263 s
    // both "4 min".
    let period = |q: f64| {
        if q < 30.0 {
            format!("{q:.1} s")
        } else {
            format!("{q:.0} s")
        }
    };
    let mut lines = Vec::new();
    if !fast && !named.iter().any(|&n| near(p.period_s, n)) {
        let size = match p.size_unit {
            "deg" => format!("{:.2}°", p.size),
            u => format!("{:.2} {u}", p.size),
        };
        let wheel = p
            .wheel
            .as_ref()
            .map_or(String::new(), |w| format!(", the {w}'s period"));
        lines.push(format!(
            "{} ({size} peak to peak{wheel})",
            period(p.period_s)
        ));
    }
    lines.extend(
        p.other_periods_s
            .iter()
            .filter(|&&q| !named.iter().any(|&n| near(q, n)))
            .map(|&q| period(q)),
    );
    let list = match lines.as_slice() {
        [] => return None,
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    Some(format!(
        "The {}'s period search also finds {list}.",
        s.series.name()
    ))
}

/// A second sentence for structure the headline does not name: a cycle
/// under steps or a slide, or steps or a slide under a cycle.
fn also(s: &SeriesCheck, cfg: &Config) -> Option<String> {
    let k = s.series;
    // Named, not "it": after a one-sided lead "it" would read as the side.
    let the = format!("The {}", k.name());
    let mut out = Vec::new();
    if let Some(c) = s.cycle.as_ref().filter(|_| s.lead != Lead::Periodic) {
        out.push(format!(
            "{the} also repeats every {:.0} s{}{}, {} peak to peak.",
            c.period_s,
            c.wheel
                .as_ref()
                .map_or(String::new(), |w| format!(" (the {w})")),
            inner_text(c),
            k.fmt_size(c.peak_to_peak)
        ));
    }
    let moves = !matches!(s.lead, Lead::ShiftingMean | Lead::Drifting);
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
                "{the}'s level also moves over the take, between {} and {}.",
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

/// The cycle a `periodic` headline would rest on. Candidates come from
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
        if t.repeating_levels() {
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
    let at = |p: f64, src: CycleSource| cycle_at(&d, p, src, wheels);
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
        .map(|c| {
            // A line the period search lists stays itself when reducing
            // would move it onto a fast line's alias or its turns: the
            // fraction then holds the alias, not the line.
            let r = reduce(c.clone());
            let listed = |p| slow_search_line(p, g.step, d.span(), s.period.as_ref());
            if listed(c.period_s)
                && !listed(r.period_s)
                && alias_of_fast_line(r.period_s, g.step, d.span(), s.period.as_ref()).is_some()
            {
                c
            } else {
                r
            }
        })
        .filter(|c| c.explained >= cfg.periodic_min_explained)
        .filter(|c| clears_alias(c.period_s, c.explained, &d, s.period.as_ref()))
        .map(|mut c| {
            // At a turn and a half or more of an alias, the fold holds the
            // alias too: its share and size are given net of the alias's own
            // fold. On the alias itself the two cannot be parted.
            if let Some(a) = alias_of_fast_line(c.period_s, g.step, d.span(), s.period.as_ref()) {
                if c.period_s / a >= 1.25 {
                    let (rest, kept) = without_fold(&d, refine(&d, a, 0.03));
                    let (e, ptp) = fold(&rest, c.period_s);
                    c.explained = e * kept;
                    c.peak_to_peak = ptp;
                }
            }
            c
        })
        .filter(|c| c.explained >= cfg.periodic_min_explained)
        .max_by(|a, b| a.explained.total_cmp(&b.explained))
        .map(|c| half_cycle(&c, &d, s.period.as_ref(), wheels, cfg).unwrap_or(c))
}

/// Whether a cycle of `period` stands apart from a fast line's alias.
/// The alias itself is set aside, and so is a whole number of its turns
/// unless the cycle explains clearly more than folding at the alias does
/// (a real slow cycle at that period does). A cycle the period search
/// also finds in the beat timing, where it is slow enough to need no
/// readings, is real however it aliases.
fn clears_alias(period: f64, explained: f64, d: &Grid, search: Option<&Period>) -> bool {
    slow_search_line(period, d.step, d.span(), search)
        || alias_of_fast_line(period, d.step, d.span(), search).is_none_or(|a| {
            (period / a - 1.0).abs() >= 0.05 && explained > 1.25 * fold(d, refine(d, a, 0.03)).0
        })
}

/// The cycle at half of `c`'s period, when the readings repeat there.
///
/// Folded at a period of an odd number of readings, n, the readings fall
/// in the same groups as at half of it: reading k is at phase k/n of the
/// one and 2k/n of the other, and with n odd both visit the same n
/// phases, in another order (a 45 s cycle in 10 s readings can be found
/// at 90 s, nine readings a turn). Which of the two the readings follow
/// shows in the shape's harmonics: a shape that repeats at half the
/// period has its variance in the even ones. The cycle moves to half its
/// period when sines at the half and the quarter explain 85% or more of
/// what all n phases explain (sines at every harmonic up to n/2; at seven
/// readings, where the quarter is under two readings and cannot be told
/// from the third, the half alone), and the
/// moved cycle then explains 10% or more on its own and clears a fast
/// line's alias as any cycle must. A line the period search lists at an
/// odd harmonic, or a listed fast line's alias there, is a line of its
/// own sharing those phases (a 30 s line, or the 30 s alias of a 3.75 s
/// escape wheel, a third of 90 s) and is left out of the n phases first.
/// A cycle the search lists at its own period stays.
///
/// The moved cycle's period is half the period found, which the fold
/// pinned to under half a reading; its share and size are those of the
/// fitted sines (one at seven readings a turn, else two), since a fold
/// at 4.5 readings a turn puts readings of different phases in one bin
/// and reads low. Net of a fast line's alias
/// at a turn and a half or more of it, as for any cycle.
fn half_cycle(
    c: &Cycle,
    d: &Grid,
    search: Option<&Period>,
    wheels: &[Wheel],
    cfg: &Config,
) -> Option<Cycle> {
    let p = c.period_s;
    let n = p / d.step;
    // Only a period known to under half a reading has one whole number of
    // readings to be odd.
    let tol = (p * p / d.span()).max(0.01 * p) / d.step;
    let odd = tol < 0.5 && (n - n.round()).abs() <= tol && n.round() as i64 % 2 == 1;
    let listed = |q| slow_search_line(q, d.step, d.span(), search);
    if !odd || n.round() < 7.0 || listed(p) {
        return None;
    }
    let own = |q: f64| {
        listed(q)
            || alias_of_fast_line(q, d.step, d.span(), search)
                .is_some_and(|a| (q / a - 1.0).abs() < 0.05)
    };
    let h = n.round() as usize / 2;
    let phases: Vec<f64> = (1..=h)
        .filter(|&k| k % 2 == 0 || k == 1 || !own(p / k as f64))
        .map(|k| p / k as f64)
        .collect();
    let t = p / 2.0;
    // At seven readings a turn the quarter (1.75 readings) cannot be told
    // from the third in the readings, so the half is fitted alone.
    let half_sines: &[f64] = if t / 2.0 < 2.0 * d.step {
        &[t]
    } else {
        &[t, t / 2.0]
    };
    let (all, _) = sine_fit(d, &phases, p);
    let (half, mut ptp) = sine_fit(d, half_sines, t);
    if all <= 0.0 || half < 0.85 * all || !clears_alias(t, half, d, search) {
        return None;
    }
    let mut explained = half;
    if let Some(a) = alias_of_fast_line(t, d.step, d.span(), search) {
        if t / a >= 1.25 {
            let (rest, kept) = without_fold(d, refine(d, a, 0.03));
            let (e, size) = sine_fit(&rest, half_sines, t);
            explained = e * kept;
            ptp = size;
        }
    }
    (explained >= cfg.periodic_min_explained).then(|| Cycle {
        period_s: t,
        peak_to_peak: ptp,
        explained,
        source: c.source,
        wheel: wheel_for(t, wheels),
        inner_period_s: None,
        inner_explained: None,
        turns: None,
    })
}

/// Least squares fit of a mean and a sine at each of `periods`: the share
/// of the readings' variance it explains, less what noise alone would give
/// (two per sine over the readings), and its peak to peak over one `turn`.
fn sine_fit(d: &Grid, periods: &[f64], turn: f64) -> (f64, f64) {
    let tau = 2.0 * std::f64::consts::PI;
    let m = 1 + 2 * periods.len();
    let row = |t: f64| {
        let mut r = vec![1.0; m];
        for (j, &q) in periods.iter().enumerate() {
            let x = tau * t / q;
            r[1 + 2 * j] = x.cos();
            r[2 + 2 * j] = x.sin();
        }
        r
    };
    let pts: Vec<(f64, f64)> =
        d.y.iter()
            .enumerate()
            .filter(|(_, v)| v.is_finite())
            .map(|(i, &v)| (d.time(i) - d.t0, v))
            .collect();
    let n = pts.len();
    if n < 2 * m {
        return (0.0, 0.0);
    }
    // Normal equations, solved by elimination with partial pivoting.
    let mut a = vec![vec![0.0; m + 1]; m];
    for &(t, v) in &pts {
        let r = row(t);
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
            return (0.0, 0.0);
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
    let b: Vec<f64> = (0..m).map(|i| a[i][m] / a[i][i]).collect();
    let fit = |t: f64| row(t).iter().zip(&b).map(|(r, b)| r * b).sum::<f64>();
    let mean = pts.iter().map(|p| p.1).sum::<f64>() / n as f64;
    let total: f64 = pts.iter().map(|p| (p.1 - mean).powi(2)).sum();
    let between: f64 = pts.iter().map(|p| (fit(p.0) - mean).powi(2)).sum();
    if total <= 0.0 {
        return (0.0, 0.0);
    }
    let curve: Vec<f64> = (0..200).map(|i| fit(turn * i as f64 / 200.0)).collect();
    let hi = curve.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let lo = curve.iter().copied().fold(f64::INFINITY, f64::min);
    (between / total - (m - 1) as f64 / n as f64, hi - lo)
}

/// The cycle folded at `period`.
fn cycle_at(d: &Grid, period: f64, source: CycleSource, wheels: &[Wheel]) -> Cycle {
    let (explained, peak_to_peak) = fold(d, period);
    Cycle {
        period_s: period,
        peak_to_peak,
        explained,
        source,
        wheel: wheel_for(period, wheels),
        inner_period_s: None,
        inner_explained: None,
        turns: None,
    }
}

/// The readings less their fold at `period`, and the share of their
/// variance left.
fn without_fold(d: &Grid, period: f64) -> (Grid, f64) {
    let bins = ((period / d.step).round() as usize).clamp(2, 30);
    let bin = |i: usize| {
        let x = (d.time(i) - d.t0) / period;
        (((x - x.floor()) * bins as f64) as usize).min(bins - 1)
    };
    let mut sum = vec![0.0; bins];
    let mut n = vec![0usize; bins];
    for (i, &v) in d.y.iter().enumerate() {
        if v.is_finite() {
            sum[bin(i)] += v;
            n[bin(i)] += 1;
        }
    }
    let mut rest = d.clone();
    for (i, v) in rest.y.iter_mut().enumerate() {
        if v.is_finite() && n[bin(i)] > 0 {
            *v -= sum[bin(i)] / n[bin(i)] as f64;
        }
    }
    let var = |y: &[f64]| {
        let f: Vec<f64> = y.iter().copied().filter(|v| v.is_finite()).collect();
        let m = f.iter().sum::<f64>() / f.len().max(1) as f64;
        f.iter().map(|v| (v - m).powi(2)).sum::<f64>()
    };
    let total = var(&d.y);
    let kept = if total > 0.0 {
        var(&rest.y) / total
    } else {
        0.0
    };
    (rest, kept)
}

/// Whether the period search has a line of three readings or longer
/// within its resolution (period² over the span, at least 1%) of
/// `period`: the search's own rule for matching a wheel.
fn slow_search_line(period: f64, step: f64, span: f64, search: Option<&Period>) -> bool {
    search.is_some_and(|p| {
        std::iter::once(p.period_s)
            .chain(p.other_periods_s.iter().copied())
            .any(|q| q >= 3.0 * step && (period - q).abs() <= (q * q / span).max(0.01 * q))
    })
}

/// The alias period, when a cycle of `period` in readings `step` apart
/// is a whole number of turns of the alias of a period-search line too
/// fast for the readings, or a half number that is also a whole number
/// of the line's own turns, within the resolution (period² over
/// `span`, at least 1%, and at least 5% of the alias). Readings averaged
/// over `step` sample a line of frequency ν at 1/`step`, so it shows at
/// |ν − m/`step`| for the nearest whole m: a 15-tooth escape wheel at
/// 28,800 bph (3.75 s) reads as a 30 s cycle in 10 s readings, and folds
/// as well at 60 s, 120 s or 330 s, and partly at 75 s. The readings
/// alone cannot tell such a cycle from a slow one, so the caller sets it
/// aside unless folding at its period explains clearly more than at the
/// alias: the same watch's fourth wheel turns in 60 s, two turns of that
/// alias, and is real.
fn alias_of_fast_line(period: f64, step: f64, span: f64, search: Option<&Period>) -> Option<f64> {
    let p = search?;
    let fs = 1.0 / step;
    std::iter::once(p.period_s)
        .chain(p.other_periods_s.iter().copied())
        .filter(|&q| q < 3.0 * step)
        .find_map(|q| {
            let nu = 1.0 / q;
            let alias = (nu - (nu / fs).round() * fs).abs();
            if alias <= 0.0 {
                return None;
            }
            let a = 1.0 / alias;
            let m = (2.0 * period / a).round() / 2.0;
            let tol = (period * period / span).max(0.01 * period).max(0.05 * a);
            // A half turn holds the alias only where it is also a whole
            // number of the line's own turns (75 s is 20 of 3.75 s).
            let holds = m.fract() == 0.0 || (m * a - (m * a / q).round() * q).abs() <= tol;
            (m >= 1.0 && holds && (period - m * a).abs() <= tol).then_some(a)
        })
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

fn lead(s: &SeriesCheck, cfg: &Config) -> Lead {
    if let Some(t) = &s.two_state {
        if t.switching_levels() {
            return Lead::TwoStates;
        }
        if t.one_side_or_unlock() {
            return Lead::Measurement;
        }
    }
    if s.cycle.is_some() {
        return Lead::Periodic;
    }
    let trend = s
        .trend
        .as_ref()
        .map_or(0.0, |t| t.explained.max(t.curve_explained));
    if let Some(ch) = &s.changes {
        if ch.segments.len() > 1 && ch.explained >= cfg.min_explained {
            // A slide cut into steps is still a slide.
            return if trend >= 0.8 * ch.explained {
                Lead::Drifting
            } else {
                Lead::ShiftingMean
            };
        }
    }
    if trend >= cfg.min_explained {
        return Lead::Drifting;
    }
    let ac = s.autocorrelation.as_ref();
    let memory =
        ac.is_some_and(|a| a.p_value < cfg.alpha && a.max_short_lag_r >= cfg.min_correlation);
    let excess = s.allan.as_ref().is_some_and(|a| a.max_excess >= 2.0);
    if memory || excess {
        Lead::Wandering
    } else {
        Lead::Steady
    }
}

/// The fast line as a sentence of its own, last in the headline, for the
/// leads whose text does not carry it in place of "no cycle"; beside a
/// slower cycle it is one the series "also" repeats.
fn fast_sentence(s: &SeriesCheck) -> Option<String> {
    match s.lead {
        Lead::TooShort | Lead::Steady | Lead::Wandering => None,
        _ => fast_line(s).map(|line| {
            let mut c = line.chars();
            let line: String = c
                .next()
                .map_or(String::new(), |f| f.to_uppercase().chain(c).collect());
            format!("{line}.")
        }),
    }
}

fn every_text(sec: f64) -> String {
    if sec >= 5400.0 {
        format!("{:.1} h", sec / 3600.0)
    } else if sec >= 120.0 {
        format!("{:.0} min", sec / 60.0)
    } else {
        format!("{sec:.0} s")
    }
}

fn lead_text(s: &SeriesCheck, mean: f64) -> String {
    let k = s.series;
    let name = k.name();
    let cap = {
        let mut c = name.chars();
        c.next()
            .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    };
    let every = every_text;
    let step = every(s.step_s);
    match s.lead {
        // check_series states the too-short case, with its rule, before
        // the tests run.
        Lead::TooShort => s.headline.clone(),
        Lead::Steady => match fast_line(s) {
            // A period-search line the headline adds is a cycle: say
            // nothing of cycles here.
            None if search_also(s).is_some() => format!(
                "{cap}: {} {step} readings, median {}, ±{} from one reading to the next; no level change, trend or correlation between readings detected.",
                s.readings,
                k.fmt(s.median.unwrap_or(mean)),
                k.fmt_size(s.short_term_sd.unwrap_or(0.0))
            ),
            None => format!(
                "{cap}: {} {step} readings, median {}, ±{} from one reading to the next; no cycle, level change, trend or correlation between readings detected.",
                s.readings,
                k.fmt(s.median.unwrap_or(mean)),
                k.fmt_size(s.short_term_sd.unwrap_or(0.0))
            ),
            Some(line) => format!(
                "{cap}: {} {step} readings, median {}, ±{} from one reading to the next; no level change, trend or correlation between readings detected; {line}.",
                s.readings,
                k.fmt(s.median.unwrap_or(mean)),
                k.fmt_size(s.short_term_sd.unwrap_or(0.0))
            ),
        },
                Lead::Periodic => match (&s.cycle, &s.two_state) {
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
        Lead::TwoStates | Lead::Measurement => {
            let t = s.two_state.as_ref().unwrap();
            let base = format!(
                "{cap} switches between two levels, {} and {}, about every {}",
                k.fmt(t.low),
                k.fmt(t.high),
                every(t.switch_every_s)
            );
            if s.lead == Lead::Measurement {
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
        Lead::ShiftingMean => {
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
                Lead::Drifting => {
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
        Lead::Wandering => {
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
            match fast_line(s) {
                None if search_also(s).is_some() => format!(
                    "Successive {step} {name} readings are correlated ({}); no level change or trend detected.",
                    parts.join("; ")
                ),
                None => format!(
                    "Successive {step} {name} readings are correlated ({}); no cycle, level change or trend detected.",
                    parts.join("; ")
                ),
                Some(line) => format!(
                    "Successive {step} {name} readings are correlated ({}); no level change or trend detected; {line}.",
                    parts.join("; ")
                ),
            }
        }
    }
}

impl SeriesCheck {
    /// The period search's line when it turns in under three of the
    /// readings cycles are folded in (the 2 s windows for the amplitude and
    /// beat error), so they cannot show it, whether or not they show a
    /// slower cycle.
    pub fn faster_than_readings(&self) -> Option<&Period> {
        self.period
            .as_ref()
            .filter(|p| p.period_s < 3.0 * self.cycle_step_s)
    }
}

/// The period search's line when it is too fast for the readings to show
/// as a cycle (under three readings a turn): without it, a headline that
/// says "no cycle" would contradict the period search on the same series.
/// For the rate the search reads the beat timing: a line under 30 s is
/// sized as the timing swing in ms, a slower one as the rate swing in s/d
/// (`longrun::TIMING_UNIT_BELOW_S`), and named after what it measures.
fn fast_line(s: &SeriesCheck) -> Option<String> {
    let p = s.faster_than_readings()?;
    let what = match (s.series, p.size_unit) {
        (SeriesKind::Rate, "ms") => "the beat timing".to_string(),
        (k, _) => format!("the {}", k.name()),
    };
    let wheel = p
        .wheel
        .as_ref()
        .map_or(String::new(), |w| format!(", the {w}'s period"));
    // Two decimals, as the period search's own line in the report.
    let size = match p.size_unit {
        "deg" => format!("{:.2}°", p.size),
        u => format!("{:.2} {u}", p.size),
    };
    let also = if s.cycle.is_some() { " also" } else { "" };
    Some(format!(
        "{what}{also} repeats every {:.1} s{wheel}, {size} peak to peak; {} readings are too coarse to show it",
        p.period_s,
        every_text(s.cycle_step_s)
    ))
}

fn findings(s: &SeriesCheck) -> Option<Finding> {
    let k = s.series;
    let name = k.name();
    let (code, title) = match s.lead {
        Lead::Steady | Lead::TooShort => return None,
        Lead::Periodic => ("periodic", format!("The {name} repeats on a cycle")),
        Lead::TwoStates => (
            "two_states",
            format!("The {name} switches between two levels"),
        ),
        Lead::Measurement => (
            "measurement_split",
            format!("The {name} switches between two levels; the change shows on one side only, or in the unlock but not the drop"),
        ),
        Lead::ShiftingMean => (
            "shifting_mean",
            format!("The {name} changes level during the take"),
        ),
        Lead::Drifting => ("drifting", format!("The {name} slides through the take")),
        Lead::Wandering => (
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
    let holes = Cuts::new(clock, &long.breaks, &[]);
    let map = |t: f64| holes.true_time(t);
    let r = cfg.rate_reading_s;
    let wins = rate_windows(log, clock, &long.breaks, r);
    let rate_grid = rate_grid(&wins, r);
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
    let [amp_grid, be_grid] = window_grids(log, clock, &long.breaks);
    // The tests read every series at the rate's reading length: the
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
    let amp_samples: Vec<twostate::Sample> = holes
        .amplitude_windows(aw)
        .into_iter()
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
            harmonic_of: c.component.harmonic_of,
            other_harmonic_of: long.rate_components[1..]
                .iter()
                .map(|c| c.component.harmonic_of)
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
            if !matches!(s.lead, Lead::Steady | Lead::TooShort | Lead::Measurement) {
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
        caveats: long.caveats.clone(),
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

/// The rate readings of [`rate_windows`] on a grid of `r` seconds,
/// NaN where none was kept. `wins` must be in time order.
pub(crate) fn rate_grid(wins: &[timing::WindowFit], r: f64) -> Grid {
    match wins.first() {
        Some(w0) => {
            let t0 = w0.start_s;
            let last = wins.last().unwrap();
            let n = ((last.start_s - t0) / r).round() as usize + 1;
            let mut y = vec![f64::NAN; n];
            for w in wins {
                y[((w.start_s - t0) / r).round() as usize] = w.fit.rate_s_per_day;
            }
            Grid { t0, step: r, y }
        }
        None => Grid {
            t0: 0.0,
            step: r,
            y: Vec::new(),
        },
    }
}

/// One value per amplitude window on true time, one grid point every
/// window length, so each gap is left empty; a window that reaches into
/// a break's gap is left out.
fn windows_grid(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[crate::breaks::Break],
    f: &dyn Fn(&crate::amplitude::AmplitudeWindow) -> Option<f64>,
) -> Grid {
    let holes = Cuts::new(clock, breaks, &[]);
    let map = |t: f64| holes.true_time(t);
    let aw = &log.amplitude_windows;
    // One window's length on true time, so the grid keeps step with the
    // windows when the clock fit stretches time (2.9 s over 16 h at
    // 50 ppm on the audio's own spacing).
    let astep = aw.first().map_or(2.0, |w| map(w.end_s) - map(w.start_s));
    let Some(w0) = aw.first() else {
        return Grid {
            t0: 0.0,
            step: astep,
            y: Vec::new(),
        };
    };
    // On true time, so each gap is left empty.
    let n = ((map(aw.last().unwrap().start_s) - map(w0.start_s)) / astep).round() as usize + 1;
    let mut y = vec![f64::NAN; n];
    for w in holes.amplitude_windows(aw) {
        let i = ((map(w.start_s) - map(w0.start_s)) / astep).round() as usize;
        if i < n {
            y[i] = f(w).unwrap_or(f64::NAN);
        }
    }
    Grid {
        t0: map(w0.start_s),
        step: astep,
        y,
    }
}

/// The amplitude and beat error windows on true time, one grid point
/// every window length, so each gap is left empty. A window that reaches
/// into a break's gap is left out, as the rate readings are. The beat
/// errors are sizes, given as positive, when the sign turns over across a
/// break ([`crate::longrun::beat_error_turns`]).
pub(crate) fn window_grids(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[crate::breaks::Break],
) -> [Grid; 2] {
    let grid_of = |f: &dyn Fn(&crate::amplitude::AmplitudeWindow) -> Option<f64>| {
        windows_grid(log, clock, breaks, f)
    };
    let amp_grid = grid_of(&|w| w.mean());
    // As sizes when the sign turns over across a break, as the take's
    // beat errors are.
    let turned = crate::longrun::beat_error_turns(log, clock, breaks);
    let be_grid = grid_of(&|w| {
        w.beat_error_unlock_ms
            .map(|v| if turned { v.abs() } else { v })
    });
    [amp_grid, be_grid]
}

/// Tick's and Tock's amplitudes, each on the amplitude windows' grid as
/// [`window_grids`] puts them.
pub(crate) fn side_grids(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[crate::breaks::Break],
) -> [Grid; 2] {
    [
        windows_grid(log, clock, breaks, &|w| w.even_deg),
        windows_grid(log, clock, breaks, &|w| w.odd_deg),
    ]
}

/// The rate readings, and the amplitude and the beat error averaged into
/// the same readings, as [`check`] tests them: `r` seconds each on true
/// time, one grid.
pub(crate) fn reading_grids(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[crate::breaks::Break],
    r: f64,
) -> [Grid; 3] {
    let rate = rate_grid(&rate_windows(log, clock, breaks, r), r);
    let [amp, be] = window_grids(log, clock, breaks);
    let amp = rebin(&amp, rate.t0, r, rate.y.len());
    let be = rebin(&be, rate.t0, r, rate.y.len());
    [rate, amp, be]
}

/// The rate readings [`check`] tests, `r` seconds each, on true time.
/// None reaches into a break's gap, and none is taken from part of its
/// span: each of its 2 s stretches (the amplitude windows' length) must
/// hold beats, so a reading at the edge of a gap or a masked stretch is
/// left out, not averaged over what is left.
pub(crate) fn rate_windows(
    log: &BeatLog,
    clock: Option<&ClockFit>,
    breaks: &[crate::breaks::Break],
    r: f64,
) -> Vec<timing::WindowFit> {
    let holes = Cuts::new(clock, breaks, &[]);
    let beats = holes.beats(&log.beats);
    let astep = log
        .amplitude_windows
        .first()
        .map_or(2.0, |w| w.end_s - w.start_s);
    let mut wins = timing::windows(&beats, log.bph, r, r);
    wins.retain(|w| holes.whole(&beats, w.start_s, w.end_s, astep));
    wins
}

/// Average the windows of `fine` into `n` readings of `step` seconds
/// from `t0`. A reading needs every one of its windows: one at the edge
/// of a gap or a masked stretch, or with a window missing, is left out
/// rather than taken from part of its span.
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
    // The windows whose middles fall in each reading: all of them must be
    // there. At a reading length that isn't a whole number of windows,
    // readings hold one window more or fewer in turn.
    let need = |b: usize| {
        let lo = (t0 + b as f64 * step - fine.t0) / fine.step;
        let hi = lo + step / fine.step;
        ((hi - 0.5).ceil() - (lo - 0.5).ceil()).max(1.0) as usize
    };
    Grid {
        t0,
        step,
        y: sum
            .iter()
            .zip(&count)
            .enumerate()
            .map(|(b, (s, &c))| if c >= need(b) { s / c as f64 } else { f64::NAN })
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
    let n_eff = crate::confidence::effective_readings(n, rho(&xd) * rho(&yd)).max(4.0);
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

    #[test]
    fn the_band_widens_where_gaps_leave_fewer_pairs() {
        // Readings every 10 s with every other block of 50 missing: at a
        // lag of 50 readings no pair is left; at 60, ten in each of the
        // first 19 blocks.
        let mut rng = crate::synth::Rng::new(1);
        let y: Vec<f64> = (0..2000)
            .map(|i| {
                if (i / 50) % 2 == 1 {
                    f64::NAN
                } else {
                    rng.normal()
                }
            })
            .collect();
        let cfg = Config::default();
        let a = autocorrelation(&y, 10.0, &cfg);
        assert_eq!(a.band_by_lag.len(), a.lag_s.len());
        let at = |lag: f64| {
            let k = a.lag_s.iter().position(|&l| l == lag).unwrap();
            a.band_by_lag[k]
        };
        // Lag 0: every reading pairs with itself, as for `band`.
        assert!((at(0.0) - a.band).abs() < 1e-12);
        assert!(at(500.0).is_nan());
        assert!(
            (at(600.0) - 1.96 / 190.0f64.sqrt()).abs() < 1e-9,
            "{}",
            at(600.0)
        );
        assert!(at(600.0) > 2.0 * a.band);
    }

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
        assert_eq!(s.lead, Lead::Steady, "{}", s.headline);
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
        assert_eq!(s.lead, Lead::ShiftingMean, "{}", s.headline);
        let ch = s.changes.unwrap();
        assert_eq!(ch.segments.len(), 2);
        assert!((ch.segments[1].start_s - 2000.0).abs() < 90.0);
        assert!((s.cusum.unwrap().at_s - 2000.0).abs() < 90.0);
    }

    fn search_line(
        period_s: f64,
        size: f64,
        size_unit: &'static str,
        wheel: Option<&str>,
    ) -> Period {
        Period {
            period_s,
            significance: 10.0,
            explained: 0.5,
            peak_to_peak: size,
            timing_peak_to_peak_ms: None,
            size,
            size_unit,
            wheel: wheel.map(str::to_string),
            other_periods_s: vec![],
            harmonic_of: None,
            other_harmonic_of: vec![],
        }
    }

    fn noise(seed: u64, n: usize, level: f64, sd: f64) -> Vec<f64> {
        let mut z = Noise(seed);
        (0..n).map(|_| level + sd * z.next()).collect()
    }

    #[test]
    fn a_fast_rate_line_is_stated_as_the_beat_timing_in_ms() {
        let s = check_series(
            SeriesKind::Rate,
            grid(noise(11, 360, 5.0, 2.0), 10.0),
            None,
            Some(search_line(5.0, 0.0432, "ms", Some("escape wheel"))),
            None,
            &[],
            &Config::default(),
        );
        assert_eq!(s.lead, Lead::Steady, "{}", s.headline);
        assert!(
            s.headline.ends_with("; the beat timing repeats every 5.0 s, the escape wheel's period, 0.04 ms peak to peak; 10 s readings are too coarse to show it."),
            "{}",
            s.headline
        );
    }

    #[test]
    fn a_rate_line_of_30_s_or_more_is_stated_as_the_rate_in_s_per_day() {
        // `series --reading 30`: the fourth wheel turns in two readings.
        let s = check_series(
            SeriesKind::Rate,
            grid(noise(12, 400, 5.0, 2.0), 30.0),
            None,
            Some(search_line(60.0, 1.2, "s/d", Some("fourth wheel"))),
            None,
            &[],
            &Config::default(),
        );
        assert!(
            s.headline.contains("; the rate repeats every 60.0 s, the fourth wheel's period, 1.20 s/d peak to peak; 30 s readings are too coarse to show it."),
            "{}",
            s.headline
        );
        assert!(!s.headline.contains("beat timing"), "{}", s.headline);
    }

    #[test]
    fn an_amplitude_line_is_judged_against_the_2_s_windows() {
        let fine = || grid(noise(13, 1800, 270.0, 3.0), 2.0);
        let readings = || grid(noise(14, 360, 270.0, 1.5), 10.0);
        let s = check_series(
            SeriesKind::Amplitude,
            readings(),
            Some(fine()),
            Some(search_line(4.0, 0.8, "deg", None)),
            None,
            &[],
            &Config::default(),
        );
        assert!(
            s.headline.ends_with("; the amplitude repeats every 4.0 s, 0.80° peak to peak; 2 s readings are too coarse to show it."),
            "{}",
            s.headline
        );
        // 20 s is under three 10 s readings but over three 2 s windows,
        // which can show it: no fast line.
        let s = check_series(
            SeriesKind::Amplitude,
            readings(),
            Some(fine()),
            Some(search_line(20.0, 0.8, "deg", None)),
            None,
            &[],
            &Config::default(),
        );
        assert!(s.faster_than_readings().is_none(), "{}", s.headline);
        assert!(!s.headline.contains("too coarse"), "{}", s.headline);
        // The search's line is named, and the lead says nothing of cycles.
        assert!(
            s.headline.ends_with(
                "; no level change or trend detected. The amplitude's period search also finds 20.0 s (0.80° peak to peak)."
            ),
            "{}",
            s.headline
        );
    }

    #[test]
    fn a_fast_line_beside_a_level_change_is_its_own_sentence() {
        let mut z = Noise(2);
        let y: Vec<f64> = (0..1800)
            .map(|i| if i < 1000 { 5.0 } else { 9.0 } + 2.0 * z.next())
            .collect();
        let s = check_series(
            SeriesKind::Rate,
            grid(y, 2.0),
            None,
            Some(search_line(4.0, 0.0432, "ms", Some("escape wheel"))),
            None,
            &[],
            &Config::default(),
        );
        assert_eq!(s.lead, Lead::ShiftingMean, "{}", s.headline);
        assert!(
            s.headline.ends_with(". The beat timing repeats every 4.0 s, the escape wheel's period, 0.04 ms peak to peak; 2 s readings are too coarse to show it."),
            "{}",
            s.headline
        );
    }

    #[test]
    fn a_fast_lines_alias_is_no_cycle_at_any_number_of_its_turns() {
        // One to seven hours of 10 s rate readings carrying the 30 s alias
        // of a 3.75 s escape-wheel line, a fifth to a half of their
        // variance: the autocorrelation offers every 30 s lag, and 330 s
        // or 390 s (11 and 13 turns of the alias) do not reduce to it.
        let tau = 2.0 * std::f64::consts::PI;
        let runs = [360, 720, 1440, 2520].into_iter().flat_map(|n| {
            [(0.7, 21), (1.0, 22), (1.4, 23)].map(|(amp, seed)| (n, amp, seed + n as u64))
        });
        for (n, amp, seed) in runs {
            let mut z = Noise(seed);
            let y: Vec<f64> = (0..n)
                .map(|i| 5.0 + amp * (tau * i as f64 / 3.0 + 0.4).sin() + z.next())
                .collect();
            let s = check_series(
                SeriesKind::Rate,
                grid(y, 10.0),
                None,
                Some(search_line(3.75, 0.3, "ms", Some("escape wheel"))),
                None,
                &[],
                &Config::default(),
            );
            assert!(s.cycle.is_none(), "seed {seed}: {}", s.headline);
            assert!(
                s.faster_than_readings().is_some(),
                "seed {seed}: {}",
                s.headline
            );
        }
    }

    /// Fast lines too quick for 10 s readings and the alias each shows at.
    const FAST_LINES: [(f64, f64); 3] = [(3.75, 30.0), (1.0 / 0.15, 20.0), (60.0 / 11.0, 60.0)];

    /// One to seven hours of 10 s rate readings: noise, the alias of a fast
    /// line at `alias_amp`, and slow sines at `slow` (period, amplitude).
    fn aliased_readings(
        n: usize,
        seed: u64,
        alias: f64,
        alias_amp: f64,
        slow: &[(f64, f64)],
    ) -> Vec<f64> {
        let tau = 2.0 * std::f64::consts::PI;
        let mut z = Noise(seed);
        (0..n)
            .map(|i| {
                let t = 10.0 * i as f64;
                // A phase near a quarter turn keeps the 20 s alias, two
                // readings a turn, at nearly its full size.
                5.0 + alias_amp * (tau * t / alias + 1.4).sin()
                    + slow
                        .iter()
                        .map(|&(p, a)| a * (tau * t / p + 1.1).sin())
                        .sum::<f64>()
                    + z.next()
            })
            .collect()
    }

    fn rate_check(y: Vec<f64>, fast: f64, listed: &[f64]) -> SeriesCheck {
        let mut line = search_line(fast, 0.3, "ms", Some("escape wheel"));
        line.other_periods_s = listed.to_vec();
        check_series(
            SeriesKind::Rate,
            grid(y, 10.0),
            None,
            Some(line),
            None,
            &[],
            &Config::default(),
        )
    }

    const HOURS: [usize; 4] = [360, 720, 1440, 2520];

    /// Whether a found cycle is `p`, within the take's resolution.
    fn near(c: &Cycle, p: f64, n: usize) -> bool {
        (c.period_s - p).abs() <= (p * p / (10.0 * n as f64)).max(0.01 * p)
    }

    #[test]
    fn a_weak_fourth_wheel_beside_a_fast_line_is_sized_on_its_own() {
        // The docs' watch: a 3.75 s escape-wheel line (a 30 s alias in 10 s
        // readings) and a sinusoidal 60 s fourth wheel under a third its
        // size, both lines the period search lists. Folding at 60 s, two
        // turns of the alias, holds the alias too; net of it, the wheel's
        // own swing is 0.6 s/d and too small a share to be the cycle, so
        // the headline names it from the period search.
        for n in HOURS {
            for seed in 0..4 {
                let y = aliased_readings(n, seed + n as u64, 30.0, 1.0, &[(60.0, 0.3)]);
                let s = rate_check(y, 3.75, &[60.0]);
                match &s.cycle {
                    Some(c) => assert!(
                        near(c, 60.0, n) && c.peak_to_peak < 1.2,
                        "{n} readings, seed {seed}: {}",
                        s.headline
                    ),
                    None => assert!(
                        s.headline.contains("period search also finds 60 s"),
                        "{n} readings, seed {seed}: {}",
                        s.headline
                    ),
                }
            }
        }
    }

    #[test]
    fn a_listed_line_on_alias_turns_with_nothing_of_its_own_is_no_cycle() {
        // The period search lists 60 s, two turns of the 30 s alias, but
        // the readings hold only the alias: net of it, nothing is left.
        for n in HOURS {
            for seed in 0..4 {
                let y = aliased_readings(n, seed + 5 * n as u64, 30.0, 1.0, &[]);
                let s = rate_check(y, 3.75, &[60.0]);
                assert!(
                    s.cycle.is_none(),
                    "{n} readings, seed {seed}: {}",
                    s.headline
                );
                assert!(
                    s.headline
                        .ends_with(" The rate's period search also finds 60 s."),
                    "{}",
                    s.headline
                );
            }
        }
    }

    #[test]
    fn a_cycle_on_alias_turns_is_sized_net_of_the_alias() {
        // A 60 s fourth wheel of 2 s/d peak to peak beside a 3.75 s line's
        // 30 s alias of 2 s/d: the fold at 60 s holds both, about 3 s/d;
        // net of the alias it is the wheel's own, about 2 s/d.
        for n in HOURS {
            for seed in 0..4 {
                let y = aliased_readings(n, seed + 11 * n as u64, 30.0, 1.0, &[(60.0, 1.0)]);
                let s = rate_check(y, 3.75, &[60.0]);
                let c = s.cycle.as_ref().unwrap();
                assert!(
                    near(c, 60.0, n),
                    "{n} readings, seed {seed}: {}",
                    s.headline
                );
                assert!(
                    (1.4..2.6).contains(&c.peak_to_peak),
                    "{n} readings, seed {seed}: {} ({:.2})",
                    s.headline,
                    c.peak_to_peak
                );
            }
        }
    }

    #[test]
    fn a_search_line_the_headline_does_not_name_is_added() {
        // A 30 min amplitude swing leads; the period search also lists a
        // 45 s line the readings' cycle does not name.
        let tau = 2.0 * std::f64::consts::PI;
        let mut z = Noise(41);
        let y: Vec<f64> = (0..3600)
            .map(|i| 270.0 + 6.0 * (tau * 2.0 * i as f64 / 1800.0).sin() + z.next())
            .collect();
        let mut line = search_line(45.0, 0.8, "deg", None);
        line.other_periods_s = vec![1800.0, 600.0];
        let s = check_series(
            SeriesKind::Amplitude,
            grid(y, 2.0),
            None,
            Some(line),
            None,
            &[],
            &Config::default(),
        );
        assert!(
            s.headline.ends_with(
                " The amplitude's period search also finds 45 s (0.80° peak to peak) and 600 s."
            ),
            "{}",
            s.headline
        );
    }

    #[test]
    fn a_real_slow_line_beside_an_alias_is_the_only_cycle_found() {
        // A 450 s line (a third wheel) beside the 30 s, 20 s and 60 s
        // aliases. Listed by the period search, it is the cycle, or, when
        // net of the alias it is too small a share of the readings, the
        // headline names it from the search; unlisted, it may be lost when
        // it does not clearly outfold the alias. No other cycle is ever
        // listed in its place.
        for n in HOURS {
            for seed in 0..4 {
                for (q, a) in FAST_LINES {
                    let y = aliased_readings(n, seed + 7 * n as u64, a, 1.0, &[(450.0, 0.7)]);
                    let s = rate_check(y.clone(), q, &[450.0]);
                    assert!(
                        match &s.cycle {
                            Some(c) => near(c, 450.0, n),
                            None => s.headline.contains("period search also finds 450 s"),
                        },
                        "{a} s alias, listed, {n} readings, seed {seed}: {}",
                        s.headline
                    );
                    let s = rate_check(y, q, &[]);
                    assert!(
                        s.cycle.as_ref().is_none_or(|c| near(c, 450.0, n)),
                        "{a} s alias, unlisted, {n} readings, seed {seed}: {}",
                        s.headline
                    );
                }
            }
        }
    }

    #[test]
    fn a_pure_alias_is_no_cycle_at_whole_half_or_third_turns() {
        // The 30 s, 20 s and 60 s aliases alone, at a fifth to two thirds
        // of the readings' variance, over one to seven hours: the
        // autocorrelation offers whole, half and third turns of each.
        for n in HOURS {
            for seed in 0..4 {
                for (q, a) in FAST_LINES {
                    for amp in [0.7, 1.0, 1.4] {
                        let y = aliased_readings(n, seed + 3 * n as u64, a, amp, &[]);
                        let s = rate_check(y, q, &[]);
                        assert!(
                            s.cycle.is_none(),
                            "{a} s alias, {n} readings, seed {seed}: {}",
                            s.headline
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_half_turn_of_an_alias_counts_only_where_it_holds_the_line() {
        let line = |q| Some(search_line(q, 0.3, "ms", None));
        // 75 s is 2.5 turns of the 30 s alias of 3.75 s, and 20 of its own.
        let p = line(3.75);
        let a = alias_of_fast_line(75.0, 10.0, 7200.0, p.as_ref()).unwrap();
        assert!((a - 30.0).abs() < 1e-9);
        // 30 s is 1.5 turns of the 20 s alias of 6.67 s, and 4.5 of its own.
        let p = line(1.0 / 0.15);
        let a = alias_of_fast_line(40.0, 10.0, 7200.0, p.as_ref()).unwrap();
        assert!((a - 20.0).abs() < 1e-9);
        assert_eq!(alias_of_fast_line(30.0, 10.0, 7200.0, p.as_ref()), None);
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
        assert_eq!(s.lead, Lead::Drifting, "{}", s.headline);
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
        assert_eq!(s.lead, Lead::Wandering, "{}", s.headline);
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
        assert_eq!(s.lead, Lead::Periodic, "{}", s.headline);
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

    /// Two hours of 10 s rate readings, detrended: unit noise and sines
    /// of (period, amplitude), each with its own phase.
    fn sines(seed: u64, parts: &[(f64, f64)]) -> Grid {
        let tau = 2.0 * std::f64::consts::PI;
        let mut z = Noise(seed);
        let y: Vec<f64> = (0..720)
            .map(|i| {
                let t = 10.0 * (i as f64 + 0.5);
                5.0 + z.next()
                    + parts
                        .iter()
                        .enumerate()
                        .map(|(j, &(p, a))| a * (tau * t / p + 0.3 + 0.9 * j as f64).sin())
                        .sum::<f64>()
            })
            .collect();
        longterm::detrend(&grid(y, 10.0), 1800.0)
    }

    /// `half_cycle` of the cycle found at 90 s, nine readings a turn.
    fn from_90(d: &Grid, search: Option<&Period>) -> Option<Cycle> {
        from_at(d, 90.0, search)
    }

    fn from_at(d: &Grid, p: f64, search: Option<&Period>) -> Option<Cycle> {
        let c = cycle_at(d, refine(d, p, 0.01), CycleSource::Autocorrelation, &[]);
        half_cycle(&c, d, search, &[], &Config::default())
    }

    #[test]
    fn a_cycle_found_at_twice_its_period_moves_to_its_own() {
        // 90 s is nine 10 s readings, so folding at 90 s and at 45 s puts
        // the readings in the same groups. A 45 s cycle, a sine or one with
        // a 22.5 s harmonic, moves to 45 s with the size of its two sines;
        // a 90 s sine, and an even mix of 90 s and 45 s, stay.
        for seed in 0..8 {
            for (parts, ptp) in [
                (vec![(45.0, 0.8)], 1.6),
                (vec![(45.0, 0.8), (22.5, 0.4)], 0.0),
            ] {
                let d = sines(100 + seed, &parts);
                let c = from_90(&d, None).unwrap_or_else(|| panic!("seed {seed}: {parts:?}"));
                assert!((c.period_s - 45.0).abs() < 0.5, "seed {seed}: {c:?}");
                let tau = 2.0 * std::f64::consts::PI;
                let curve: Vec<f64> = (0..450)
                    .map(|i| {
                        let t = i as f64 / 10.0;
                        parts
                            .iter()
                            .enumerate()
                            .map(|(j, &(p, a))| a * (tau * t / p + 0.3 + 0.9 * j as f64).sin())
                            .sum::<f64>()
                    })
                    .collect();
                let truth = if ptp > 0.0 {
                    ptp
                } else {
                    curve.iter().copied().fold(f64::MIN, f64::max)
                        - curve.iter().copied().fold(f64::MAX, f64::min)
                };
                assert!(
                    (c.peak_to_peak / truth - 1.0).abs() < 0.1,
                    "seed {seed}: {:.2} against {truth:.2}",
                    c.peak_to_peak
                );
            }
            for parts in [vec![(90.0, 0.8)], vec![(90.0, 0.8), (45.0, 0.8)]] {
                let d = sines(200 + seed, &parts);
                assert!(from_90(&d, None).is_none(), "seed {seed}: {parts:?}");
            }
        }
    }

    #[test]
    fn a_listed_line_sharing_the_phases_is_left_out_of_them() {
        // A 45 s cycle beside a 30 s line, a third of 90 s: the 30 s line
        // shares the nine phases, so unlisted it keeps the cycle at 90 s;
        // listed by the period search, it is a line of its own.
        for seed in 0..8 {
            let d = sines(300 + seed, &[(45.0, 0.8), (30.0, 0.8)]);
            assert!(from_90(&d, None).is_none(), "seed {seed}");
            let mut line = search_line(5.0, 0.3, "ms", Some("escape wheel"));
            line.other_periods_s = vec![30.0];
            let c = from_90(&d, Some(&line)).unwrap_or_else(|| panic!("seed {seed}"));
            assert!((c.period_s - 45.0).abs() < 0.5, "seed {seed}: {c:?}");
        }
    }

    #[test]
    fn at_seven_readings_a_turn_the_half_is_fitted_alone() {
        // 70 s is seven readings, and its quarter (17.5 s) cannot be told
        // from its third (23.33 s) in them. A 70 s cycle beside a listed
        // 23.33 s line stays at 70 s; a 35 s cycle beside it moves, sized
        // by its own sine.
        let mut line = search_line(5.0, 0.3, "ms", Some("escape wheel"));
        line.other_periods_s = vec![70.0 / 3.0];
        for seed in 0..16 {
            for size in [0.8, 1.2, 1.6] {
                let d = sines(800 + seed, &[(70.0, 0.8), (70.0 / 3.0, size)]);
                assert!(
                    from_at(&d, 70.0, Some(&line)).is_none(),
                    "seed {seed}, {size}"
                );
            }
            let d = sines(900 + seed, &[(35.0, 0.8), (70.0 / 3.0, 0.8)]);
            let c = from_at(&d, 70.0, Some(&line)).unwrap_or_else(|| panic!("seed {seed}"));
            assert!((c.period_s - 35.0).abs() < 0.5, "seed {seed}: {c:?}");
            assert!(
                (c.peak_to_peak / 1.6 - 1.0).abs() < 0.3,
                "seed {seed}: {c:?}"
            );
        }
    }

    #[test]
    fn a_cycle_does_not_move_onto_a_fast_lines_alias() {
        // An 8.18 s line reads as a 45 s alias in 10 s readings: a 45 s
        // cycle found at 90 s stays there when the search lists that line.
        let fast = 1.0 / (0.1 + 1.0 / 45.0);
        let line = search_line(fast, 0.3, "ms", None);
        for seed in 0..8 {
            let d = sines(400 + seed, &[(45.0, 0.8)]);
            assert!(from_90(&d, None).is_some(), "seed {seed}");
            assert!(from_90(&d, Some(&line)).is_none(), "seed {seed}");
        }
    }

    #[test]
    fn a_fast_lines_alias_sharing_the_phases_is_left_out_of_them() {
        // A 45 s cycle beside a listed 3.75 s escape-wheel line, whose 30 s
        // alias (a third of 90 s) is half the cycle's size and not listed
        // itself: the alias is left out of the nine phases and the cycle
        // moves; a 90 s and 45 s mix beside the same line stays.
        let line = search_line(3.75, 0.3, "ms", Some("escape wheel"));
        for seed in 0..8 {
            let d = sines(600 + seed, &[(45.0, 0.8), (30.0, 0.4)]);
            let c = from_90(&d, Some(&line)).unwrap_or_else(|| panic!("seed {seed}"));
            assert!((c.period_s - 45.0).abs() < 0.5, "seed {seed}: {c:?}");
            let d = sines(700 + seed, &[(90.0, 0.8), (45.0, 0.8), (30.0, 0.4)]);
            assert!(from_90(&d, Some(&line)).is_none(), "seed {seed}");
        }
    }

    #[test]
    fn the_headline_names_a_cycle_found_at_twice_its_period_at_its_own() {
        // Through the whole check: a 45 s cycle beside a listed 3.75 s
        // line's 30 s alias half its size reads as 45 s; a 90 s cycle
        // beside the same alias as 90 s.
        let tau = 2.0 * std::f64::consts::PI;
        for seed in 0..8 {
            for (p, name) in [(45.0, "every 45 s"), (90.0, "every 90 s")] {
                let mut z = Noise(500 + seed);
                let y: Vec<f64> = (0..720)
                    .map(|i| {
                        let t = 10.0 * (i as f64 + 0.5);
                        5.0 + 0.8 * (tau * t / p + 0.3).sin()
                            + 0.4 * (tau * t / 30.0 + 1.2).sin()
                            + z.next()
                    })
                    .collect();
                let s = rate_check(y, 3.75, &[]);
                assert!(
                    s.headline.starts_with(&format!("Rate repeats {name}")),
                    "{p} s, seed {seed}: {}",
                    s.headline
                );
            }
        }
    }

    #[test]
    fn no_reading_is_taken_from_part_of_its_span() {
        // Five 2 s windows a 10 s reading: one missing (the edge of a gap
        // or a mask) leaves the reading out.
        let mut fine = grid(vec![1.0; 15], 2.0);
        fine.y[7] = f64::NAN;
        let r = rebin(&fine, 0.0, 10.0, 3);
        assert_eq!(r.y[0], 1.0);
        assert!(r.y[1].is_nan(), "{:?}", r.y);
        assert_eq!(r.y[2], 1.0);
        // At 15 s, readings hold seven and eight windows in turn: an hour
        // of whole windows keeps every reading, and one missing window
        // loses only its own.
        let mut fine = grid(vec![1.0; 1800], 2.0);
        let r = rebin(&fine, 0.0, 15.0, 240);
        assert!(r.y.iter().all(|v| *v == 1.0), "{:?}", r.y);
        fine.y[100] = f64::NAN;
        let r = rebin(&fine, 0.0, 15.0, 240);
        let lost: Vec<usize> = (0..240).filter(|&b| r.y[b].is_nan()).collect();
        assert_eq!(lost, vec![13]);
        // Rate readings: beats in each 2 s of the 10 s, or none.
        let beats: Vec<crate::beats::Beat> = (0..160)
            .filter(|&i| !(80..96).contains(&i))
            .map(|index| crate::beats::Beat {
                time: index as f64 * 0.125,
                index,
                quality: 1.0,
            })
            .collect();
        assert!(crate::clean::covered(&beats, 0.0, 10.0, 2.0));
        assert!(!crate::clean::covered(&beats, 10.0, 20.0, 2.0));
        // Allan deviation: a block with a reading missing is left out.
        let mut y: Vec<f64> = (0..64).map(|i| (i % 2) as f64).collect();
        y[5] = f64::NAN;
        let whole = allan(&y, 1.0);
        let four = whole.iter().find(|a| a.0 == 4.0).unwrap();
        // 57 pairs of 4-reading blocks, less the 6 with reading 5 in
        // either block: 51, or 12 independent (3 of 4 readings used to
        // be enough, keeping all 57).
        assert_eq!(four.2, 12, "{whole:?}");
        // A break with 1 s of no sound after 300 s of audio: a window
        // reaching 0.3 s into the gap misses the cut halfway through it
        // but is left out; one ending or starting at an edge is whole.
        let breaks = [crate::breaks::Break {
            audio_s: 300.0,
            gap_s: 1.0,
        }];
        let holes = Cuts::new(None, &breaks, &[]);
        let gap = holes.holes()[0];
        assert!(
            (gap.start_s - 300.0).abs() < 1e-5 && gap.end_s == 301.0,
            "{gap:?}"
        );
        let cut = crate::longrun::cuts(None, &breaks)[0];
        assert!(!(290.3 < cut && cut < 300.3));
        assert!(!holes.clear(290.3, 300.3));
        assert!(!holes.clear(300.8, 310.8));
        assert!(holes.clear(290.0, 300.0));
        assert!(holes.clear(301.0, 311.0));
    }
}

#[cfg(test)]
mod window_grid_tests {
    use super::*;
    use crate::amplitude::AmplitudeWindow;
    use crate::breaks::Break;

    fn log_of(windows: usize) -> BeatLog {
        BeatLog {
            sample_rate: 48000,
            duration_s: windows as f64 * 2.0,
            bph: 18000,
            lift_deg: 52.0,
            beats: vec![],
            amplitude_windows: (0..windows)
                .map(|k| {
                    let start_s = k as f64 * 2.0;
                    AmplitudeWindow {
                        start_s,
                        end_s: start_s + 2.0,
                        even_deg: Some(k as f64),
                        odd_deg: Some(k as f64),
                        beat_error_ms: None,
                        beat_error_unlock_ms: Some(0.3),
                        even_unlock_ms: None,
                        odd_unlock_ms: None,
                        even_drop_ms: None,
                        odd_drop_ms: None,
                    }
                })
                .collect(),
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![],
            sided: vec![],
            by_count: vec![],
        }
    }

    #[test]
    fn a_window_across_a_break_is_left_out() {
        // A pause at 301 s of audio, inside the window from 300 to 302 s:
        // that window holds sound from both sides, so it gives no reading.
        let log = log_of(300);
        let breaks = [Break {
            audio_s: 301.0,
            gap_s: 30.0,
        }];
        let [amp, _] = window_grids(&log, None, &breaks);
        let at = |t: f64| amp.y[((t - amp.t0) / amp.step).round() as usize];
        assert_eq!(at(298.0), 149.0);
        assert!(at(300.0).is_nan());
        assert_eq!(at(302.0 + 30.0), 151.0);
        // 15 empty points for the gap, and the window across it.
        assert_eq!(amp.y.iter().filter(|v| v.is_nan()).count(), 16);
        assert_eq!(amp.y.iter().filter(|v| v.is_finite()).count(), 299);
    }

    #[test]
    fn windows_keep_step_with_a_stretched_clock() {
        // 16 h of 2 s windows on a sound card 50 ppm fast and no breaks:
        // every window keeps its own grid point, and the grid's time for
        // it is the window's true time.
        let n = 28800;
        let log = log_of(n);
        let k = 1.0 + 50e-6;
        let pairs: Vec<(f64, f64)> = (0..=576)
            .map(|i| {
                let t = i as f64 * 100.0;
                (t, 1.79e9 + t * k)
            })
            .collect();
        let clock = ClockFit::new(&pairs).unwrap();
        let map = crate::longrun::true_time(Some(&clock), &[]);
        let [amp, _] = window_grids(&log, Some(&clock), &[]);
        assert_eq!(amp.y.len(), n);
        for (i, w) in log.amplitude_windows.iter().enumerate() {
            assert_eq!(amp.y[i], w.even_deg.unwrap(), "{i}");
            let t = amp.t0 + i as f64 * amp.step;
            assert!((t - map(w.start_s)).abs() < 0.01, "{i}: {t}");
        }
    }
}
