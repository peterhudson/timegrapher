//! Each reading's ± from the spread between short windows, and the
//! conditions detected in the recording beside it.
//!
//! The ± estimates precision from successive window differences, allowing
//! for the readings' lag-one correlation through an effective number of
//! readings. It is not calibrated repeat-measurement coverage for every
//! kind of serial correlation, or a measure of true watch variation.
//! Accuracy also rests on the lift angle and sound card's clock; these are
//! stated as conditions ([`Doubts`] and [`crate::measure`]), not folded into
//! the ±. See docs/research/measurement-precision.md for the model and its limits.

use crate::amplitude::AmplitudeWindow;
use crate::timing::WindowFit;
use serde::Serialize;

/// Beats found below this share of the beats expected is the
/// `beats_missing` condition.
pub const MIN_BEAT_FRACTION: f64 = 0.95;

/// Below this share of nominal kept windows, a reading names its limited coverage.
/// The 60% rule is defined in math-standards.md, section 7.
pub const MIN_WINDOW_COVERAGE: f64 = 0.6;

/// Below this share of nominal kept windows a window figure is withheld,
/// not read (#1429): a median of a
/// handful of stray windows in a stretch the watch was barely heard in is
/// not a reading of it. Between this share and [`MIN_WINDOW_COVERAGE`] the
/// figure stands and names its limited coverage. Same denominator as the
/// 60% rule, math-standards.md section 7.
pub const MIN_WINDOW_SHARE: f64 = 0.1;

/// Whether the share `k2` of `n2` differs from the share `k1` of `n1` by
/// more than 2 standard errors of the difference: `Some(true)` when it is
/// higher, `Some(false)` when lower, `None` within. The standard error is
/// from the pooled share as Agresti and Coull adjust it,
/// (k1 + k2 + 2) / (n1 + n2 + 4), so a share near 0 or 1 never has a
/// standard error of 0. One test for every "does one read keep more than
/// the other" choice: a second read's kept beats (#1423), live's rebuilt
/// template's both-sided windows (#1500).
pub fn share_differs(k1: f64, n1: f64, k2: f64, n2: f64) -> Option<bool> {
    if n1 <= 0.0 || n2 <= 0.0 {
        return None;
    }
    let p = (k1 + k2 + 2.0) / (n1 + n2 + 4.0);
    let se = (p * (1.0 - p) * (1.0 / n1 + 1.0 / n2)).sqrt();
    let diff = k2 / n2 - k1 / n1;
    (diff.abs() > 2.0 * se).then_some(diff > 0.0)
}

/// Whether `used` windows of the `in_span` nominal kept windows are too
/// few to give a window figure: fewer than [`MIN_WINDOW_SHARE`] of them.
/// The share alone: [`MIN_VALUES`] is the floor for a ±, not for a
/// figure, so a short reading whose few cells are read keeps its figure.
pub fn too_few_windows(used: usize, in_span: usize) -> bool {
    (used as f64) < MIN_WINDOW_SHARE * in_span as f64
}

/// The most the unlock beat errors' sign agreement may fall short of what
/// their spread predicts before the unlock beat error is withheld
/// ([`unlock_inconsistent`], #1439). Set on the corpus's 319 healthy
/// readings (largest shortfall 0.127) against a run-down's two hours whose
/// faint Tock unlock was found on noise (0.206 and 0.250); math-standards.md
/// section 7.
pub const MAX_SIGN_SHORTFALL: f64 = 0.18;

/// The standard normal distribution function, Φ(x): Abramowitz and Stegun
/// 7.1.26, within 1.5e-7.
pub fn normal_cdf(x: f64) -> f64 {
    let z = x.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = ((((1.061_405_429 * t - 1.453_152_027) * t + 1.421_413_741) * t - 0.284_496_736)
        * t
        + 0.254_829_592)
        * t;
    let erf = 1.0 - poly * (-z * z).exp();
    0.5 * (1.0 + erf.copysign(x))
}

/// How a reading's unlock beat errors `values` agree in sign: the share
/// that the spread predicts on the median's side, Φ(|median| / robust SD),
/// the share observed there, and the line the shortfall must pass to call
/// them inconsistent, `max(MAX_SIGN_SHORTFALL, 3 √(q(1 − q)/n))`, q the
/// predicted or the observed share, whichever varies more, so a few windows
/// never trip it on noise alone. None with fewer than
/// [`MIN_VALUES`] or no spread.
pub fn sign_agreement(values: &[f64]) -> Option<SignAgreement> {
    let v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    let n = v.len();
    if n < MIN_VALUES {
        return None;
    }
    let med = crate::dsp::median(&mut v.clone());
    let sd = crate::dsp::robust_sd(&v);
    if sd <= 0.0 {
        return None;
    }
    let predicted = normal_cdf(med.abs() / sd);
    let side = |x: f64| x.partial_cmp(&0.0) == med.partial_cmp(&0.0);
    let observed = v.iter().filter(|&&x| side(x)).count() as f64 / n as f64;
    // The share's own noise, by the larger of its variance at the
    // predicted share and at the observed: a tight cluster predicts nearly
    // 1, and one stray window of five must not trip it.
    let var = (predicted * (1.0 - predicted)).max(observed * (1.0 - observed));
    let line = MAX_SIGN_SHORTFALL.max(3.0 * (var / n as f64).sqrt());
    Some(SignAgreement {
        predicted,
        observed,
        line,
    })
}

/// [`sign_agreement`]'s figures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignAgreement {
    pub predicted: f64,
    pub observed: f64,
    pub line: f64,
}

impl SignAgreement {
    /// The predicted share less the observed.
    pub fn shortfall(self) -> f64 {
        self.predicted - self.observed
    }
}

/// Whether a reading's unlock beat errors `values` disagree in sign more
/// than their spread allows (#1439): fewer of them sit on the median's
/// side than Φ(|median| / robust SD) predicts, by more than the line
/// ([`sign_agreement`]). A faint unlock found on noise in part of the
/// windows reads so: a cluster far from zero and another on the far side
/// of it. A watch whose beat error is near zero predicts little agreement
/// and is not judged by it.
pub fn unlock_inconsistent(values: &[f64]) -> bool {
    sign_agreement(values).is_some_and(|a| a.shortfall() > a.line)
}

/// Windows supporting one figure, and nominal windows within its kept time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Coverage {
    pub windows_used: usize,
    pub windows_in_span: usize,
}

impl Coverage {
    /// Whether fewer than 60% of the eligible windows produced this figure.
    pub fn is_thin(self) -> bool {
        self.windows_in_span > 0
            && (self.windows_used as f64) < MIN_WINDOW_COVERAGE * self.windows_in_span as f64
    }
}

/// The figure named by a per-reading coverage caveat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Figure {
    Rate,
    Amplitude,
    BeatError,
}

impl std::fmt::Display for Figure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rate => "rate",
            Self::Amplitude => "amplitude",
            Self::BeatError => "beat error",
        })
    }
}

/// Each figure has its own window lengths and missing measurements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FigureCoverage {
    pub rate: Coverage,
    pub amplitude: Coverage,
    pub beat_error: Coverage,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Reading {
    /// Standard error, in the reading's own unit.
    pub se: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Confidence {
    pub rate: Reading,
    pub amplitude: Reading,
    pub beat_error: Reading,
}

/// Conditions detected in the recording, as plain flags; each one set is
/// also named, with its figure, in the caveats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Doubts {
    /// More than 1% of beats have a clipped sample.
    pub clipped: bool,
    /// Fewer than [`MIN_BEAT_FRACTION`] of the beats expected were found.
    pub beats_missing: bool,
    /// The lift angle is a default, not the calibre's.
    pub lift_unknown: bool,
    /// No correction for the sound card's clock.
    pub clock_unknown: bool,
}

impl Confidence {
    pub fn new(
        rate_se: Option<f64>,
        amplitude_se: Option<f64>,
        beat_error_se: Option<f64>,
    ) -> Self {
        Confidence {
            rate: Reading { se: rate_se },
            amplitude: Reading { se: amplitude_se },
            beat_error: Reading { se: beat_error_se },
        }
    }
}

/// Readings worth `n` independent ones when neighbours have lag-one
/// correlation `rho`: n(1 − ρ)/(1 + ρ), the AR(1) effective sample size
/// (Bretherton et al. 1999, J. Climate 12:1990, eq. 31). For the
/// correlation of two series, `rho` is the product of their lag-one
/// correlations. ρ is held to ±0.99 and the result to 1 ..= n.
pub fn effective_readings(n: f64, rho: f64) -> f64 {
    effective_readings_at(n, rho, 0.0)
}

/// [`effective_readings`] for the size of a sine `cycles` cycles per
/// reading fitted to the readings: AR(1) noise holds
/// (1 − ρ²)/(1 − 2ρ cos 2π·cycles + ρ²) of its variance at that
/// frequency against white noise's, so the readings are worth n over
/// that. At 0 cycles it is n(1 − ρ)/(1 + ρ); for a fast line the noise
/// that follows itself is weaker than white, and the count is held to n.
pub fn effective_readings_at(n: f64, rho: f64, cycles: f64) -> f64 {
    let r = if rho.is_finite() {
        rho.clamp(-0.99, 0.99)
    } else {
        0.0
    };
    let c = (std::f64::consts::TAU * cycles).cos();
    (n * (1.0 - 2.0 * r * c + r * r) / (1.0 - r * r)).clamp(1.0_f64.min(n), n)
}

/// Lag-one correlation of values in time order, less their least-squares
/// line, so a steady drift is not counted as memory. `None` with fewer
/// than three values or no scatter about the line.
pub fn lag_one(values: &[f64]) -> Option<f64> {
    pooled_lag_one([values])
}

/// [`lag_one`] over several stretches of one series in time order (the
/// runs between gaps), read as one: each stretch less its own
/// least-squares line, the products of neighbours within each stretch
/// and the squares summed over all of them, each sum over its count of
/// terms, as [`crate::steadiness::acf`] takes a lag. Stretches of fewer
/// than three values are passed over.
pub fn pooled_lag_one<'a>(stretches: impl IntoIterator<Item = &'a [f64]>) -> Option<f64> {
    let (mut s0, mut n0, mut s1, mut n1) = (0.0, 0usize, 0.0, 0usize);
    for values in stretches {
        if values.len() < 3 {
            continue;
        }
        let mut x = values.to_vec();
        crate::dsp::detrend_line(&mut x);
        let m = x.iter().sum::<f64>() / x.len() as f64;
        x.iter_mut().for_each(|v| *v -= m);
        s0 += x.iter().map(|v| v * v).sum::<f64>();
        n0 += x.len();
        s1 += x.windows(2).map(|w| w[0] * w[1]).sum::<f64>();
        n1 += x.len() - 1;
    }
    let c0 = s0 / n0 as f64;
    (n1 > 0 && c0 > 0.0)
        .then(|| (s1 / n1 as f64) / c0)
        .filter(|r| r.is_finite())
}

/// The largest lag-one correlation a ± allows for. Past it the inflation
/// √((1 + ρ)/(1 − ρ)) cannot be read from about [`MIN_NOISE_VALUES`]
/// values, so a ± held here says so by being wide (Norm, #1544). From 30
/// AR(1) values at ρ 0.9 with their line taken out, [`corrected_lag_one`]
/// reaches it about 21% of the time (seeded, 20,000 series).
pub const MAX_RHO: f64 = 0.99;

/// The lag-one correlation every ± allows for (#1544): [`pooled_lag_one`]
/// plus its bias downwards with each stretch's line taken out,
/// 2(1 + 3ρ)/n (Marriott and Pope 1954, as [`long_run_rho`] adds it), n
/// the stretches' mean length, held to 0 ..= [`MAX_RHO`]. Read from 30
/// values, the raw estimate falls 0.07, 0.15 and 0.26 short of a true ρ
/// of 0, 0.5 and 0.9, against the formula's 0.07, 0.17 and 0.25. 0 where
/// no correlation can be read.
pub fn corrected_lag_one(stretches: &[&[f64]]) -> f64 {
    let Some(r) = pooled_lag_one(stretches.iter().copied()) else {
        return 0.0;
    };
    let read: Vec<usize> = stretches
        .iter()
        .map(|s| s.len())
        .filter(|&n| n >= 3)
        .collect();
    let n = read.iter().sum::<usize>() as f64 / read.len() as f64;
    let r = r + line_lag_one_bias(r, n);
    if r.is_finite() {
        r.clamp(0.0, MAX_RHO)
    } else {
        0.0
    }
}

/// What [`long_run_sd`] takes out of the values before it reads their
/// lag-one correlation, so slow change is not counted as memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slow {
    /// Their least-squares line: a steady drift.
    Line,
    /// Their single largest step: each side's mean, split where the
    /// running sum of the values less their mean is furthest from 0 (the
    /// CUSUM's corner), so a real change of level does not read as memory
    /// and widen the band that should find it.
    Step,
    /// The same step, read for the CUSUM's test of one level: ρ also
    /// gets back [`STEP_CHOICE_BIAS`]/n, what choosing the step to fit
    /// takes out of it (#917). The change finder keeps [`Slow::Step`],
    /// whose σ only scales its penalty.
    ChosenStep,
}

/// The long-run σ of finite values in time order (NaN, a gap, is passed
/// over), for statistics that assume independent readings (#647, #649):
/// `short_sd`, their scatter from neighbour differences, which is
/// σ√(1 − ρ) for AR(1) readings, times √(1 + ρ)/(1 − ρ), so that the
/// result is σ√((1 + ρ)/(1 − ρ)), what the mean of n readings scatters as
/// over √n. ρ is [`long_run_rho`]. `short_sd` itself with fewer than
/// three values.
pub fn long_run_sd(values: &[f64], short_sd: f64, slow: Slow) -> f64 {
    let r = long_run_rho(values, slow);
    short_sd * (1.0 + r).sqrt() / (1.0 - r)
}

/// The lag-one correlation [`long_run_sd`] allows for: that of the finite
/// values less `slow`, read only from neighbours that are both finite so
/// that a gap never joins readings that were not next to each other (as
/// [`pooled_lag_one`] reads it), plus 2(1 + 3ρ)/n for its bias downwards
/// with two parameters taken out (Marriott and Pope 1954), n one more than
/// the neighbouring pairs read (the values' count when none is missing),
/// held to 0 ..= [`MAX_RHO`]. [`Slow::ChosenStep`] adds [`STEP_CHOICE_BIAS`]/n
/// too (#917). On a seeded test of 360 AR(1)
/// readings this keeps the CUSUM's false alarms at 4-6% from ρ 0 to 0.9,
/// and under 8% at ρ 0.9 with block gaps or scattered dropouts (#647).
pub fn long_run_rho(values: &[f64], slow: Slow) -> f64 {
    let at: Vec<usize> = (0..values.len())
        .filter(|&i| values[i].is_finite())
        .collect();
    if at.len() < 3 {
        return 0.0;
    }
    let mut x: Vec<f64> = at.iter().map(|&i| values[i]).collect();
    match slow {
        Slow::Line => crate::dsp::detrend_line(&mut x),
        Slow::Step | Slow::ChosenStep => take_out_step(&mut x),
    }
    let m = x.iter().sum::<f64>() / x.len() as f64;
    x.iter_mut().for_each(|v| *v -= m);
    let c0 = x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64;
    let (s1, pairs) = (1..x.len())
        .filter(|&k| at[k] == at[k - 1] + 1)
        .fold((0.0, 0usize), |(s, c), k| (s + x[k] * x[k - 1], c + 1));
    if c0 <= 0.0 || pairs == 0 {
        return 0.0;
    }
    let r = s1 / pairs as f64 / c0;
    let n = (pairs + 1) as f64;
    let chosen = if slow == Slow::ChosenStep {
        STEP_CHOICE_BIAS / n
    } else {
        0.0
    };
    let r = r + line_lag_one_bias(r, n) + chosen;
    if r.is_finite() {
        r.clamp(0.0, MAX_RHO)
    } else {
        0.0
    }
}

/// How much further below the true ρ a lag-one correlation reads, times
/// n, once the best single step is taken out, beyond Marriott and Pope's
/// two parameters: the step is chosen to fit, so it takes out some of
/// the wander too. Set so the mean of [`long_run_rho`] with
/// [`Slow::ChosenStep`] is the true ρ within ±0.005 on seeded AR(1) readings
/// at ρ 0.5 and 0.9 and n 180 to 1440 (#917).
pub const STEP_CHOICE_BIAS: f64 = 3.3;

/// How far a lag-one correlation `r` read from `n` values less their line
/// falls short of the true one (Marriott and Pope 1954): 2(1 + 3r)/n. At
/// r = 0 it is the size of [`fit_lag_bias`] for a two-term fit at lag one,
/// to first order in 1/n.
fn line_lag_one_bias(r: f64, n: f64) -> f64 {
    2.0 * (1.0 + 3.0 * r) / n
}

/// The correlation that independent readings read on average at lags 0
/// to `max_lag` once a least-squares polynomial with `params` terms
/// (1, t, t², …) has been taken out of the readings present on a uniform
/// grid (`present[i]`): each lag read as products of readings `k` apart,
/// both present, over their count, over the mean square, as
/// [`crate::steadiness`] reads its autocorrelation. Under noise of
/// variance σ² the residuals' covariance is σ²(I − H), H the fit's hat
/// matrix, so the mean square is about σ²(n − p)/n and a pair's product
/// −σ²H_ij: the lag's correlation is −n/(n − p) times the mean H_ij over
/// its pairs (to first order, the ratio's expectation). 0 at lag 0, and
/// at a lag with no pairs. Subtracting it leaves a white series reading
/// about 0 at every lag however much of its slow change was taken out.
pub fn fit_lag_bias(present: &[bool], params: usize, max_lag: usize) -> Vec<f64> {
    let at: Vec<usize> = (0..present.len()).filter(|&i| present[i]).collect();
    let (n, len) = (at.len(), present.len());
    let mut out = vec![0.0; max_lag + 1];
    if params == 0 || n <= params {
        return out;
    }
    // The fit's columns made orthonormal over the readings present
    // (modified Gram–Schmidt), so H_ij is the sum over columns of q(i)q(j).
    let t = |i: usize| 2.0 * i as f64 / (len.max(2) - 1) as f64 - 1.0;
    let mut q: Vec<Vec<f64>> = Vec::with_capacity(params);
    for d in 0..params {
        let mut v: Vec<f64> = at.iter().map(|&i| t(i).powi(d as i32)).collect();
        for u in &q {
            let dot: f64 = v.iter().zip(u).map(|(a, b)| a * b).sum();
            v.iter_mut().zip(u).for_each(|(a, b)| *a -= dot * b);
        }
        let norm = v.iter().map(|a| a * a).sum::<f64>().sqrt();
        if norm < 1e-12 {
            return out;
        }
        v.iter_mut().for_each(|a| *a /= norm);
        q.push(v);
    }
    // Where each grid index sits among the readings present.
    let mut slot = vec![usize::MAX; len];
    at.iter().enumerate().for_each(|(s, &i)| slot[i] = s);
    let scale = n as f64 / (n - params) as f64;
    for (k, bias) in out.iter_mut().enumerate().skip(1) {
        let (mut sum, mut pairs) = (0.0, 0usize);
        for (a, &i) in at.iter().enumerate() {
            let Some(&b) = slot.get(i + k).filter(|&&b| b != usize::MAX) else {
                continue;
            };
            sum += q.iter().map(|u| u[a] * u[b]).sum::<f64>();
            pairs += 1;
        }
        if pairs > 0 {
            *bias = -scale * sum / pairs as f64;
        }
    }
    out
}

/// Each side of the values' largest step less its own mean ([`Slow::Step`]).
fn take_out_step(x: &mut [f64]) {
    let m = x.iter().sum::<f64>() / x.len() as f64;
    let (mut run, mut far, mut at) = (0.0, 0.0f64, 1);
    for (i, v) in x[..x.len() - 1].iter().enumerate() {
        run += v - m;
        if run.abs() > far {
            (far, at) = (run.abs(), i + 1);
        }
    }
    let (a, b) = x.split_at_mut(at);
    for side in [a, b] {
        let m = side.iter().sum::<f64>() / side.len() as f64;
        side.iter_mut().for_each(|v| *v -= m);
    }
}

/// How many standard errors clear of zero a figure must stand before a
/// direction is named in words: the guide's 2 SE. The poise wave and the
/// complication test's On less Off both read it.
pub const CLEAR_SE: f64 = 2.0;

/// Whether `value` stands clear of zero by [`CLEAR_SE`] of its ± `se`,
/// either way. A value with no ±, or a ± that isn't finite, never does.
pub fn clear_of_zero(value: f64, se: Option<f64>) -> bool {
    se.is_some_and(|se| se.is_finite() && value.abs() - CLEAR_SE * se > 0.0)
}

/// The fewest values [`standard_error`] gives a figure from.
pub const MIN_VALUES: usize = 3;

/// Estimated standard error of a median from values in time order, in their
/// own units. The scatter comes from successive differences, so a linear
/// drift cancels exactly: s = 1.4826 MAD(differences) / sqrt(2). With the
/// values' lag-one correlation ρ ([`lag_one`], negative taken as 0), the
/// differences understate the spread by sqrt(1 − ρ) and the values are worth
/// [`effective_readings`] independent ones, so
/// SE = 1.2533 × s / sqrt(1 − ρ) / sqrt(n_eff). With ρ = 0 this is
/// 1.2533 s / sqrt(n), the median's standard error under independent
/// Gaussian noise. None with fewer than [`MIN_VALUES`].
pub fn standard_error(values: &[f64]) -> Option<f64> {
    standard_error_in([values])
}

/// [`standard_error`] of one series read in stretches, the runs between
/// its gaps, in time order (#639): the differences are taken within each
/// stretch only, never across a gap, and the lag-one correlation is
/// pooled over the stretches ([`pooled_lag_one`]); `n` counts every
/// value. With one stretch it is [`standard_error`]. None with fewer than
/// [`MIN_VALUES`] values or no difference within a stretch.
pub fn standard_error_in<'a>(stretches: impl IntoIterator<Item = &'a [f64]>) -> Option<f64> {
    let stretches: Vec<&[f64]> = stretches.into_iter().collect();
    let noise = Noise::of(&stretches, MIN_VALUES)?;
    Some(noise.standard_error(count(&stretches)))
}

/// The fewest values a ± takes its noise from (#1470, math-standards
/// §9): a reading with fewer borrows the scatter and the lag-one
/// correlation of the values nearest it in the same take
/// ([`noise_pool`]), and a take with fewer in all gives no ±.
pub const MIN_NOISE_VALUES: usize = 30;

/// The noise of one series read in stretches: its scatter `s` from the
/// successive differences within each stretch, 1.4826 MAD / √2, and its
/// lag-one correlation `rho` pooled over them, corrected for its bias
/// ([`corrected_lag_one`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Noise {
    pub s: f64,
    pub rho: f64,
}

impl Noise {
    /// The noise of `stretches`; None with fewer than `min` values or no
    /// difference within a stretch.
    fn of(stretches: &[&[f64]], min: usize) -> Option<Noise> {
        let d: Vec<f64> = stretches
            .iter()
            .flat_map(|s| s.windows(2).map(|w| w[1] - w[0]))
            .collect();
        if count(stretches) < min || d.is_empty() {
            return None;
        }
        Some(Noise {
            s: crate::dsp::robust_sd(&d) / std::f64::consts::SQRT_2,
            rho: corrected_lag_one(stretches),
        })
    }

    /// The standard error of a median of `n` values with this noise:
    /// 1.2533 × s / √(1 − ρ) / √n_eff ([`effective_readings`]).
    pub fn standard_error(self, n: usize) -> f64 {
        let n_eff = effective_readings(n as f64, self.rho);
        1.2533 * self.s / (1.0 - self.rho).sqrt() / n_eff.sqrt()
    }
}

fn count(stretches: &[&[f64]]) -> usize {
    stretches.iter().map(|s| s.len()).sum()
}

/// [`standard_error_in`] of a reading's own `stretches` with its noise
/// from at least [`MIN_NOISE_VALUES`] values (#1470): its own when it
/// holds that many, else `pool`'s, the values nearest it in the take
/// ([`noise_pool`]). `n` and n_eff are always the reading's own. None
/// with fewer than [`MIN_VALUES`] values of its own, or with neither
/// holding [`MIN_NOISE_VALUES`].
pub fn standard_error_borrowing(stretches: &[Vec<f64>], pool: &[Vec<f64>]) -> Option<f64> {
    let own: Vec<&[f64]> = stretches.iter().map(Vec::as_slice).collect();
    let n = count(&own);
    if n < MIN_VALUES {
        return None;
    }
    let noise = if n >= MIN_NOISE_VALUES {
        Noise::of(&own, MIN_NOISE_VALUES)
    } else {
        let pool: Vec<&[f64]> = pool.iter().map(Vec::as_slice).collect();
        Noise::of(&pool, MIN_NOISE_VALUES)
    }?;
    Some(noise.standard_error(n))
}

/// The [`MIN_NOISE_VALUES`] values of `take` nearest a reading ending at
/// `end_s`, to borrow its noise from (#1470): the latest that end by
/// then, and where the take holds too few before it, the next after it,
/// in their stretches ([`stretches`]), so no difference is taken across
/// a cut, a mask or a change of position. `take` is the take's windows in
/// time order; a window with no value is passed over. Empty when the
/// whole of `take` holds fewer than [`MIN_NOISE_VALUES`] values.
pub fn noise_pool<T>(
    take: &[T],
    end_s: f64,
    span: impl Fn(&T) -> (f64, f64),
    value: impl Fn(&T) -> Option<f64>,
) -> Vec<Vec<f64>> {
    let valued: Vec<usize> = (0..take.len())
        .filter(|&i| value(&take[i]).is_some_and(f64::is_finite))
        .collect();
    let Some(last) = valued.len().checked_sub(MIN_NOISE_VALUES) else {
        return Vec::new();
    };
    let before = valued.partition_point(|&i| span(&take[i]).1 <= end_s + 1e-3);
    let lo = before.saturating_sub(MIN_NOISE_VALUES).min(last);
    let (a, b) = (valued[lo], valued[lo + MIN_NOISE_VALUES - 1]);
    stretches(&take[a..=b], |w| span(w), |w| value(w))
}

/// A series' values in their stretches ([`stretches`]).
type Stretches = Vec<Vec<f64>>;

/// The values of a reading of `own` windows, and the take's `take`
/// nearest it ([`noise_pool`]), read in their stretches.
fn own_and_pool<T>(
    own: &[T],
    take: &[T],
    span: impl Fn(&T) -> (f64, f64),
    value: impl Fn(&T) -> Option<f64>,
) -> Option<(Stretches, Stretches)> {
    let end = span(own.last()?).1;
    Some((
        stretches(own, |w| span(w), |w| value(w)),
        noise_pool(take, end, &span, &value),
    ))
}

/// The values `value` gives of windows `windows` in time order, in
/// stretches: a new one starts where a window starts later than the one
/// before ended (`span`, s), as across a break, a mask or a window left
/// out, and after a window with no value, since the values either side of
/// it are lag two apart, not neighbours. For windows that tile, as every
/// caller's do (amplitude and beat error 2 s windows; the rate's every
/// other 10 s window), a window kept with no value and a window left out
/// give the same stretches, so the ± can't depend on which way the
/// absence is written (#1637). Where windows overlap, `a > end` can't see
/// a window left out, so only a kept empty window cuts there.
pub fn stretches<T>(
    windows: impl IntoIterator<Item = T>,
    span: impl Fn(&T) -> (f64, f64),
    value: impl Fn(&T) -> Option<f64>,
) -> Vec<Vec<f64>> {
    let mut out: Vec<Vec<f64>> = Vec::new();
    let mut end: Option<f64> = None;
    for w in windows {
        let (a, b) = span(&w);
        // The values either side of an empty window are lag two apart,
        // never neighbours, so it cuts the stretch (#1637).
        let Some(v) = value(&w).filter(|v| v.is_finite()) else {
            end = None;
            continue;
        };
        if end.is_none_or(|e| a > e + 1e-3) {
            out.push(Vec::new());
        }
        end = Some(b);
        out.last_mut().expect("a stretch").push(v);
    }
    out
}

/// The ± of a median figure (amplitude, beat error) from `value` of
/// amplitude windows, in their stretches ([`stretches`]): the noise within
/// the stretches ([`standard_error_borrowing`], borrowed from `take`, the
/// take's amplitude windows in time order, where the reading holds fewer
/// than [`MIN_NOISE_VALUES`]) and, where two or more stretches of
/// [`MIN_VALUES`] or more sit at different levels, the spread between
/// them ([`between_stretches`], the reading's own), added in quadrature
/// (#639, #1470).
pub fn amplitude_windows_se<'a>(
    windows: impl IntoIterator<Item = &'a AmplitudeWindow>,
    take: &[&AmplitudeWindow],
    value: impl Fn(&AmplitudeWindow) -> Option<f64>,
) -> Option<f64> {
    let own: Vec<&AmplitudeWindow> = windows.into_iter().collect();
    let (s, pool) = own_and_pool(&own, take, |w| (w.start_s, w.end_s), |w| value(w))?;
    let within = standard_error_borrowing(&s, &pool)?;
    Some(within.hypot(between_stretches(&s)))
}

/// How far a median over stretches at different levels is from known, past
/// their own noise: over the k stretches of [`MIN_VALUES`] or more, with
/// medians `m_i` and standard errors `SE_i`,
/// `τ² = max(0, var(m_i) − mean(SE_i²))` (sample variance) and the term is
/// `1.2533 · τ / √k`. Stretches at one level differ only by their own
/// noise and add nothing on average; with k < 2 it is 0.
fn between_stretches(stretches: &[Vec<f64>]) -> f64 {
    let (m, se2): (Vec<f64>, Vec<f64>) = stretches
        .iter()
        .filter(|s| s.len() >= MIN_VALUES)
        .filter_map(|s| {
            let se = standard_error(s)?;
            Some((crate::dsp::median(&mut s.clone()), se * se))
        })
        .unzip();
    let k = m.len() as f64;
    if m.len() < 2 {
        return 0.0;
    }
    let mean = m.iter().sum::<f64>() / k;
    let var = m.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (k - 1.0);
    let tau2 = (var - se2.iter().sum::<f64>() / k).max(0.0);
    1.2533 * tau2.sqrt() / k.sqrt()
}

/// The rate's independent values among its windows: every other one,
/// since the windows half overlap (`measure::RATE_WINDOW_STEP_S`). The
/// one place that says so; [`rate_value_count`] counts them (#1556).
pub fn rate_values<T>(windows: &[T]) -> impl Iterator<Item = &T> {
    windows.iter().step_by(RATE_WINDOW_STRIDE)
}

/// Rate windows per independent value: a window over its step, 2.
const RATE_WINDOW_STRIDE: usize =
    (crate::measure::RATE_WINDOW_S / crate::measure::RATE_WINDOW_STEP_S) as usize;

/// How many independent values `windows` rate windows hold, as
/// [`rate_values`] takes them.
pub fn rate_value_count(windows: usize) -> usize {
    windows.div_ceil(RATE_WINDOW_STRIDE)
}

/// Estimated standard error of the rate from the 10 s windows. Taking every
/// other available window removes ordinary half-window overlap; the
/// correlation left between the windows taken is allowed for by
/// [`standard_error_borrowing`], within the stretches between gaps (#639),
/// its noise borrowed from every other of `take`'s, the take's rate
/// windows in time order, where the reading holds fewer than
/// [`MIN_NOISE_VALUES`] (#1470).
pub fn rate_se(windows: &[WindowFit], take: &[WindowFit]) -> Option<f64> {
    let own: Vec<&WindowFit> = rate_values(windows).collect();
    let take: Vec<&WindowFit> = rate_values(take).collect();
    let (s, pool) = own_and_pool(
        &own,
        &take,
        |w| (w.start_s, w.end_s),
        |w| Some(w.fit.rate_s_per_day),
    )?;
    standard_error_borrowing(&s, &pool)
}

/// Standard error of the beat error from the unlock, ms: the one rule for
/// it, which the assessment reads (#641). Of the same quantity as the
/// figure beside it ([`crate::timing::median_signed`]): of the windows'
/// sizes where the figure is the median of sizes (`as_size`, its sign
/// turning over the stretch), else of their signed values (#637). Its
/// noise is borrowed from `take` as [`amplitude_windows_se`]'s.
pub fn beat_error_se<'a>(
    windows: impl IntoIterator<Item = &'a AmplitudeWindow>,
    take: &[&AmplitudeWindow],
    as_size: bool,
) -> Option<f64> {
    amplitude_windows_se(windows, take, |w| {
        w.beat_error_unlock_ms
            .map(|x| if as_size { x.abs() } else { x })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::Rng;

    /// Every window, as a take's ± borrows from.
    fn every(w: &[AmplitudeWindow]) -> Vec<&AmplitudeWindow> {
        w.iter().collect()
    }

    #[test]
    fn a_two_term_fit_lag_bias_is_long_run_rho_s_term() {
        // One source of the bias a fit leaves: for a line and lag one it
        // is the Marriott–Pope term long_run_rho adds back at r = 0, to
        // within the 1/n² the latter leaves out.
        for n in [60usize, 360, 2000] {
            let b = fit_lag_bias(&vec![true; n], 2, 1)[1];
            let mp = -line_lag_one_bias(0.0, n as f64);
            let nf = n as f64;
            assert!((b - mp).abs() < 4.0 / (nf * nf), "{n}: {b} against {mp}");
        }
        // A quadratic leaves about −3/n; the mean alone about −1/n.
        let three = fit_lag_bias(&[true; 90], 3, 1)[1];
        let one = fit_lag_bias(&[true; 90], 1, 1)[1];
        assert!(
            (three + 3.0 / 89.0).abs() < 0.001 && (one + 1.0 / 89.0).abs() < 0.001,
            "{three} {one}"
        );
    }

    #[test]
    fn beat_error_se_reads_the_unlock_beat_errors() {
        // Every third window has no unlock beat error, so the values come
        // in pairs, each its own stretch: no difference spans a missing
        // window (#1637). 30 are kept, enough for a ± of their own (#1470).
        let windows: Vec<AmplitudeWindow> = (0..45)
            .map(|i| AmplitudeWindow {
                start_s: i as f64 * 10.0,
                end_s: i as f64 * 10.0 + 10.0,
                even_deg: None,
                odd_deg: None,
                beat_error_ms: Some(9.0),
                beat_error_unlock_ms: (i % 3 != 2).then(|| 0.4 + 0.1 * (i as f64 * 1.7).sin()),
                even_unlock_ms: None,
                odd_unlock_ms: None,
                even_drop_ms: None,
                odd_drop_ms: None,
                culled_ms: None,
            })
            .collect();
        let pairs: Vec<Vec<f64>> = windows
            .split(|w| w.beat_error_unlock_ms.is_none())
            .map(|run| run.iter().filter_map(|w| w.beat_error_unlock_ms).collect())
            .filter(|run: &Vec<f64>| !run.is_empty())
            .collect();
        assert!(pairs.iter().all(|p| p.len() == 2), "{pairs:?}");
        let se = beat_error_se(&windows, &every(&windows), false).unwrap();
        assert!(se > 0.0, "{se}");
        assert_eq!(Some(se), standard_error_in(pairs.iter().map(Vec::as_slice)));
        assert_eq!(
            beat_error_se(&windows[..2], &every(&windows[..2]), false),
            None
        );
    }

    /// 2 s windows whose unlock beat errors are `values`.
    fn unlock_windows(values: &[f64]) -> Vec<AmplitudeWindow> {
        values
            .iter()
            .enumerate()
            .map(|(i, &v)| AmplitudeWindow {
                start_s: i as f64 * 2.0,
                end_s: i as f64 * 2.0 + 2.0,
                even_deg: None,
                odd_deg: None,
                beat_error_ms: None,
                beat_error_unlock_ms: Some(v),
                even_unlock_ms: None,
                odd_unlock_ms: None,
                even_drop_ms: None,
                odd_drop_ms: None,
                culled_ms: None,
            })
            .collect()
    }

    /// Where the beat error is the median of sizes, its ± is of the sizes
    /// too, not of the signed values beside it (#637): a sign that turns
    /// over between windows is no scatter in the size.
    #[test]
    fn a_beat_errors_se_is_of_the_same_quantity_as_its_figure() {
        let signed: Vec<f64> = (0..40)
            .map(|i| {
                let size = 0.4 + 0.02 * (i as f64 * 1.3).sin();
                if i % 3 == 0 {
                    -size
                } else {
                    size
                }
            })
            .collect();
        let sizes: Vec<f64> = signed.iter().map(|x| x.abs()).collect();
        let w = unlock_windows(&signed);
        assert_eq!(beat_error_se(&w, &every(&w), true), standard_error(&sizes));
        assert_eq!(
            beat_error_se(&w, &every(&w), false),
            standard_error(&signed)
        );
        let (size_se, signed_se) = (
            beat_error_se(&w, &every(&w), true).unwrap(),
            beat_error_se(&w, &every(&w), false).unwrap(),
        );
        assert!(size_se < 0.02 && signed_se > 0.1, "{size_se} {signed_se}");
    }

    /// Seeded coverage: a beat error of size 0.4 ms whose sign turns over
    /// at random, read as the median of 60 windows' sizes, has 0.4 ms
    /// within 1.96 of its ± in about 95% of takes, on whole takes and on
    /// takes with a gap of windows left out.
    #[test]
    fn a_beat_error_sizes_se_covers_its_median() {
        let mut rng = Rng::new(20261103);
        for gapped in [false, true] {
            let trials = 600;
            let mut covered = 0;
            for _ in 0..trials {
                let mut v: Vec<f64> = (0..60)
                    .map(|_| {
                        let size = 0.4 + 0.05 * rng.normal();
                        if rng.next_f64() < 0.5 {
                            -size
                        } else {
                            size
                        }
                    })
                    .collect();
                if gapped {
                    v.drain(20..35);
                }
                let w = unlock_windows(&v);
                let median = crate::timing::median_signed(v.iter().copied(), true).unwrap();
                let se = beat_error_se(&w, &every(&w), true).unwrap();
                if (median - 0.4).abs() <= 1.96 * se {
                    covered += 1;
                }
            }
            let rate = f64::from(covered) / f64::from(trials);
            assert!((0.90..=0.985).contains(&rate), "gapped {gapped}: {rate}");
        }
    }

    /// Rate windows of 10 s every 5 s with `rates`, and a pause of
    /// `pause_s` after every `stretch` of them.
    fn rate_windows(rates: &[f64], stretch: usize, pause_s: f64) -> Vec<WindowFit> {
        rates
            .iter()
            .enumerate()
            .map(|(i, &r)| {
                let t = i as f64 * 5.0 + (i / stretch) as f64 * pause_s;
                WindowFit {
                    start_s: t,
                    end_s: t + 10.0,
                    fit: crate::timing::TimingFit {
                        rate_s_per_day: r,
                        beat_error_ms: 0.0,
                        period_s: 0.2,
                        jitter_us: 0.0,
                        beats_used: 50,
                    },
                }
            })
            .collect()
    }

    /// A take's ± takes no difference across a gap (#639): the rate a
    /// watch reads either side of a pause may differ, and that step is no
    /// scatter of the windows, as a slow change within a stretch is none.
    #[test]
    fn a_gapped_takes_se_takes_no_difference_across_the_gap() {
        let mut rng = Rng::new(639);
        let noise: Vec<f64> = (0..240).map(|_| 0.5 * rng.normal()).collect();
        let whole = rate_se(
            &rate_windows(&noise, 120, 0.0),
            &rate_windows(&noise, 120, 0.0),
        )
        .unwrap();
        // The same windows with a pause of an hour, after which the watch
        // reads 5 s/d faster.
        let stepped: Vec<f64> = noise
            .iter()
            .enumerate()
            .map(|(i, &x)| if i >= 120 { x + 5.0 } else { x })
            .collect();
        let gapped = rate_se(
            &rate_windows(&stepped, 120, 3600.0),
            &rate_windows(&stepped, 120, 3600.0),
        )
        .unwrap();
        assert!(
            (gapped / whole - 1.0).abs() < 0.15,
            "gapped {gapped} against {whole}"
        );
        // Without the pause the step is within one stretch and counts.
        let joined = rate_se(
            &rate_windows(&stepped, 120, 0.0),
            &rate_windows(&stepped, 120, 0.0),
        )
        .unwrap();
        assert!(joined > 1.5 * whole, "{joined} against {whole}");
    }

    #[test]
    fn stretches_break_where_a_window_starts_after_the_last_ended() {
        let spans = [
            (0.0, 10.0),
            (5.0, 15.0),
            (15.0, 25.0),
            (40.0, 50.0),
            (45.0, 55.0),
        ];
        let values = [Some(1.0), None, Some(f64::NAN), Some(4.0), Some(5.0)];
        let s = stretches(spans.iter().zip(values), |(s, _)| **s, |(_, v)| *v);
        assert_eq!(s, vec![vec![1.0], vec![4.0, 5.0]]);
        // One value in each stretch leaves no difference to read.
        assert_eq!(standard_error_in([&[1.0][..], &[2.0], &[3.0]]), None);
        let one = [1.0, 3.0, 2.0, 5.0, 4.0];
        assert_eq!(standard_error_in([&one[..]]), standard_error(&one));
    }

    /// 2 s amplitude windows, `values[i]` the even side's amplitude of the
    /// i-th, `None` read nothing.
    fn amplitude_windows(values: &[Option<f64>]) -> Vec<AmplitudeWindow> {
        values
            .iter()
            .enumerate()
            .map(|(i, &v)| AmplitudeWindow {
                start_s: i as f64 * 2.0,
                end_s: i as f64 * 2.0 + 2.0,
                even_deg: v,
                odd_deg: None,
                beat_error_ms: None,
                beat_error_unlock_ms: None,
                even_unlock_ms: None,
                odd_unlock_ms: None,
                even_drop_ms: None,
                odd_drop_ms: None,
                culled_ms: None,
            })
            .collect()
    }

    /// A window kept with no value and a window left out are one absence
    /// (#1637, the audit's case): 30 windows at 270°, 100 that read
    /// nothing, 30 at 250°. Kept, the empty windows used to make the last
    /// 270° and the first 250° neighbours, and the ± read 0.0°; left out,
    /// 12.5°. Both now read the level change, and agree on seeded noise.
    #[test]
    fn a_window_with_no_value_ends_its_stretch_as_one_left_out() {
        let mut rng = Rng::new(1637);
        for noise in [0.0, 0.1, 2.0] {
            let values: Vec<Option<f64>> = (0..160)
                .map(|i| match i {
                    0..30 => Some(270.0 + noise * rng.normal()),
                    30..130 => None,
                    _ => Some(250.0 + noise * rng.normal()),
                })
                .collect();
            let kept = amplitude_windows(&values);
            let left_out: Vec<AmplitudeWindow> = kept
                .iter()
                .filter(|w| w.even_deg.is_some())
                .cloned()
                .collect();
            let se =
                |w: &[AmplitudeWindow]| amplitude_windows_se(w, &every(w), |w| w.even_deg).unwrap();
            let (a, b) = (se(&kept), se(&left_out));
            assert_eq!(a, b, "noise {noise}");
            assert!(a > 10.0, "noise {noise}: the 20° step reads {a}");
        }
        // Scattered single dropouts on one level: still one level, and the
        // two ways of writing them agree.
        let values: Vec<Option<f64>> = (0..300)
            .map(|i| (i % 7 != 3).then(|| 270.0 + 2.0 * rng.normal()))
            .collect();
        let kept = amplitude_windows(&values);
        let left_out: Vec<AmplitudeWindow> = kept
            .iter()
            .filter(|w| w.even_deg.is_some())
            .cloned()
            .collect();
        let a = amplitude_windows_se(&kept, &every(&kept), |w| w.even_deg).unwrap();
        let b = amplitude_windows_se(&left_out, &every(&left_out), |w| w.even_deg).unwrap();
        assert_eq!(a, b);
        assert!(a < 1.0, "{a}");
    }

    /// An empty window cuts its stretch. On windows that tile, keeping it
    /// with no value and leaving it out agree; on overlapping windows the
    /// kept empty window still cuts, since the values either side of it
    /// are lag two apart (#1637).
    #[test]
    fn an_empty_window_cuts_its_stretch() {
        type W = (f64, f64, Option<f64>);
        let kept = |ws: &[W]| stretches(ws.iter(), |w| (w.0, w.1), |w| w.2);
        let left_out = |ws: &[W]| {
            let left: Vec<&W> = ws.iter().filter(|w| w.2.is_some()).collect();
            stretches(left, |w| (w.0, w.1), |w| w.2)
        };
        // Tiling 2 s windows, one empty: both ways cut.
        let tile = [
            (0.0, 2.0, Some(1.0)),
            (2.0, 4.0, None),
            (4.0, 6.0, Some(2.0)),
        ];
        assert_eq!(kept(&tile), vec![vec![1.0], vec![2.0]]);
        assert_eq!(kept(&tile), left_out(&tile));
        // The rate's every other 10 s window, which tile, two empty in a row.
        let rate: Vec<W> = (0..8)
            .map(|i| {
                let a = 10.0 * i as f64;
                (a, a + 10.0, (!(3..5).contains(&i)).then_some(i as f64))
            })
            .collect();
        assert_eq!(kept(&rate), vec![vec![0.0, 1.0, 2.0], vec![5.0, 6.0, 7.0]]);
        assert_eq!(kept(&rate), left_out(&rate));
        // 2 s windows every 1 s, the middle one empty: 1.0 and 2.0 are lag
        // two apart, so the kept form cuts.
        let overlap = [
            (0.0, 2.0, Some(1.0)),
            (1.0, 3.0, None),
            (2.0, 4.0, Some(2.0)),
        ];
        assert_eq!(kept(&overlap), vec![vec![1.0], vec![2.0]]);
    }

    /// Over independent stretches, each AR(1), the ± of the median covers
    /// the level about 95% of the time.
    #[test]
    fn a_gapped_takes_se_covers_its_median() {
        let mut rng = Rng::new(20261104);
        let trials = 600;
        let mut covered = 0;
        for _ in 0..trials {
            let mut rates = Vec::new();
            for _ in 0..3 {
                let mut x = rng.normal() / (1.0f64 - 0.25).sqrt();
                for _ in 0..80 {
                    x = 0.5 * x + rng.normal();
                    rates.push(x);
                    rates.push(f64::NAN); // the window step_by(2) passes over
                }
            }
            let w = rate_windows(&rates, 160, 600.0);
            let kept: Vec<f64> = rates.iter().copied().filter(|r| r.is_finite()).collect();
            let median = crate::timing::median_signed(kept.iter().copied(), false).unwrap();
            let se = rate_se(&w, &w).unwrap();
            if median.abs() <= 1.96 * se {
                covered += 1;
            }
        }
        let rate = f64::from(covered) / f64::from(trials);
        assert!((0.90..=0.985).contains(&rate), "{rate}");
    }

    /// Unlock beat errors in stretches of 2 s windows, a minute apart.
    fn unlock_stretches(stretches: &[Vec<f64>]) -> Vec<AmplitudeWindow> {
        let mut out = Vec::new();
        let mut t = 0.0;
        for s in stretches {
            for mut w in unlock_windows(s) {
                w.start_s += t;
                w.end_s += t;
                out.push(w);
            }
            t = out.last().map_or(0.0, |w| w.end_s) + 60.0;
        }
        out
    }

    /// A median figure over stretches at two levels is not known to their
    /// noise alone (#639, the walkthrough's case): a beat error at 0.6 ms
    /// for one stretch and 0.3 ms for the next reads about ± 0.19 ms.
    #[test]
    fn a_median_over_two_levels_carries_their_spread() {
        let mut rng = Rng::new(6391);
        let mut level = |l: f64| (0..40).map(|_| l + 0.01 * rng.normal()).collect::<Vec<_>>();
        let two = [level(-0.6), level(0.3)];
        let se = beat_error_se(
            &unlock_stretches(&two),
            &every(&unlock_stretches(&two)),
            true,
        )
        .unwrap();
        assert!((0.17..0.21).contains(&se), "{se}");
        // At one level the stretches add little past their noise, on
        // average over many takes: τ² is kept from going below 0, so with
        // two stretches the ± is about a quarter wider than within alone
        // (a variance ratio near 1.6; near 1.24 with six stretches).
        let (mut with, mut alone) = (0.0, 0.0);
        for _ in 0..300 {
            let one = [level(0.6), level(0.6)];
            with += beat_error_se(
                &unlock_stretches(&one),
                &every(&unlock_stretches(&one)),
                true,
            )
            .unwrap()
            .powi(2);
            alone += standard_error_in(one.iter().map(Vec::as_slice))
                .unwrap()
                .powi(2);
        }
        assert!(with < 1.8 * alone, "{with} against {alone}");
        // One stretch is exactly standard_error.
        let one = level(0.6);
        assert_eq!(
            beat_error_se(&unlock_windows(&one), &every(&unlock_windows(&one)), true),
            standard_error(&one)
        );
    }

    /// Amplitude over stretches whose levels scatter about one level, and
    /// over stretches at one level: the median's ± covers it about 95% of
    /// the time either way.
    #[test]
    fn a_median_over_stretches_covers_its_level() {
        let mut rng = Rng::new(20261105);
        for spread in [0.0, 3.0] {
            let trials = 600;
            let mut covered = 0;
            for _ in 0..trials {
                let s: Vec<Vec<f64>> = (0..6)
                    .map(|_| {
                        let level = 280.0 + spread * rng.normal();
                        let mut x = rng.normal() / (1.0f64 - 0.25).sqrt();
                        (0..30)
                            .map(|_| {
                                x = 0.5 * x + rng.normal();
                                level + x
                            })
                            .collect()
                    })
                    .collect();
                let w = unlock_stretches(&s);
                let all: Vec<f64> = s.concat();
                let median = crate::timing::median_signed(all.iter().copied(), false).unwrap();
                let se = beat_error_se(&w, &every(&w), false).unwrap();
                if (median - 280.0).abs() <= 1.96 * se {
                    covered += 1;
                }
            }
            let rate = f64::from(covered) / f64::from(trials);
            assert!((0.90..=0.985).contains(&rate), "spread {spread}: {rate}");
        }
    }

    #[test]
    fn standard_error_ignores_slow_change() {
        // Gaussian noise of SD 2 from a fixed seed, alone and on a steep
        // ramp: the ramp must not count as noise.
        let mut seed = 12345u64;
        let mut uniform = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let noise: Vec<f64> = (0..400)
            .map(|_| {
                let (u, v) = (uniform(), uniform());
                2.0 * (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
            })
            .collect();
        let want = 1.2533 * 2.0 / 20.0;
        let flat = standard_error(&noise).unwrap();
        let ramp: Vec<f64> = noise
            .iter()
            .enumerate()
            .map(|(i, n)| 200.0 + 0.5 * i as f64 + n)
            .collect();
        let sloped = standard_error(&ramp).unwrap();
        assert!((flat / want - 1.0).abs() < 0.2, "{flat} vs {want}");
        assert!((sloped - flat).abs() < 1e-9, "{sloped} vs {flat}");
        assert_eq!(standard_error(&[1.0, 2.0]), None);
    }

    /// Share of `repeats` stationary Gaussian AR(1) series of `n` values
    /// whose median lies within ±SE of the true centre, 0.
    fn coverage(rho: f64, n: usize, repeats: usize) -> f64 {
        let mut rng = Rng::new(20261022);
        let mut within = 0;
        for _ in 0..repeats {
            let mut x = rng.normal();
            let mut values = Vec::with_capacity(n);
            for _ in 0..n {
                x = rho * x + (1.0 - rho * rho).sqrt() * rng.normal();
                values.push(x);
            }
            let se = standard_error(&values).unwrap();
            if crate::dsp::median(&mut values).abs() <= se {
                within += 1;
            }
        }
        within as f64 / repeats as f64
    }

    /// [`coverage`] of a reading of `n` values whose noise is borrowed
    /// from the [`MIN_NOISE_VALUES`] nearest it in its take, the reading's
    /// own last among them (#1470).
    fn borrowed_coverage(rho: f64, n: usize, repeats: usize, seed: u64) -> f64 {
        let mut rng = Rng::new(seed);
        let mut within = 0;
        for _ in 0..repeats {
            let mut x = rng.normal();
            let values: Vec<f64> = (0..MIN_NOISE_VALUES + 10)
                .map(|_| {
                    x = rho * x + (1.0 - rho * rho).sqrt() * rng.normal();
                    x
                })
                .collect();
            let take = unlock_windows(&values);
            let own = &take[take.len() - n..];
            let se = beat_error_se(own, &every(&take), false).unwrap();
            let mut mine: Vec<f64> = values[values.len() - n..].to_vec();
            if crate::dsp::median(&mut mine).abs() <= se {
                within += 1;
            }
        }
        within as f64 / repeats as f64
    }

    #[test]
    fn a_borrowed_plus_minus_holds_its_centre_at_high_correlation() {
        // At ρ 0.9 the lag-one correlation read from 30 values is biased
        // low, so the ± under-covered: 48.4% on seed 20261114 (#1544),
        // kept as a regression seed. The others are fresh, run once, with
        // the bands set before any result: 0.55 ..= 0.90 at ρ 0.9, and
        // 0.55 ..= 0.85 at ρ 0 and 0.5, so the correction does not widen
        // every watch's ± past what it promises.
        for seed in [20261114, ACCEPTANCE_SEED] {
            let c = borrowed_coverage(0.9, 5, 2000, seed);
            assert!((0.55..=0.90).contains(&c), "seed {seed}: {c}");
        }
        for rho in [0.0, 0.5] {
            let c = borrowed_coverage(rho, 5, 2000, ACCEPTANCE_SEED);
            assert!((0.55..=0.85).contains(&c), "ρ {rho}: {c}");
        }
    }

    /// #1544's fresh seed, run once.
    const ACCEPTANCE_SEED: u64 = 20261131;

    #[test]
    fn the_corrected_lag_one_reads_the_true_one_from_30_values() {
        // Development seed, not the acceptance one: over 4,000 series of
        // 30, the raw mean falls short by about MP's 2(1 + 3ρ)/n; the
        // corrected one is near ρ 0.5 and most of the way to 0.9, the
        // first-order term evaluated at the low raw value.
        let mut rng = Rng::new(900_001);
        for (rho, short) in [(0.5, 0.05), (0.9, 0.1)] {
            let (mut raw, mut cor) = (0.0, 0.0);
            for _ in 0..4000 {
                let mut x = rng.normal();
                let v: Vec<f64> = (0..30)
                    .map(|_| {
                        x = rho * x + (1.0 - rho * rho).sqrt() * rng.normal();
                        x
                    })
                    .collect();
                raw += pooled_lag_one([v.as_slice()]).unwrap() / 4000.0;
                cor += corrected_lag_one(&[v.as_slice()]) / 4000.0;
            }
            assert!(raw < rho - 0.1, "ρ {rho}: raw {raw}");
            assert!((cor - rho).abs() < short, "ρ {rho}: corrected {cor}");
        }
    }

    #[test]
    fn the_bias_is_read_at_the_mean_length_of_the_stretches_read() {
        // Stretches of 10, 50 and 2 values (Trevor's review of #1544):
        // the bias is MP's at the mean length of those read, 30, the 2
        // too short to read left out; not at the total, 62 or 60, nor
        // the shortest, 10 or 2.
        let mut rng = Rng::new(900_002);
        let mut x = 0.0;
        let mut series = |n: usize| -> Vec<f64> {
            (0..n)
                .map(|_| {
                    x = 0.5 * x + 0.75f64.sqrt() * rng.normal();
                    x
                })
                .collect()
        };
        let (a, b, c) = (series(10), series(50), series(2));
        let s = [a.as_slice(), b.as_slice(), c.as_slice()];
        let r = pooled_lag_one(s.iter().copied()).unwrap();
        let at = |n: f64| (r + line_lag_one_bias(r, n)).clamp(0.0, MAX_RHO);
        let got = corrected_lag_one(&s);
        assert!((got - at(30.0)).abs() < 1e-12, "{got} against {}", at(30.0));
        for wrong in [62.0, 60.0, 10.0, 2.0, 62.0 / 3.0] {
            assert!((got - at(wrong)).abs() > 0.01, "n {wrong}: {got}");
        }
    }

    #[test]
    fn a_short_readings_borrowed_plus_minus_holds_its_centre() {
        // Five values of their own, their noise from the 30 nearest: one
        // SE either side should still hold the true centre about 68% of
        // the time (#1470).
        for rho in [0.0, 0.5] {
            let c = borrowed_coverage(rho, 5, 2000, 20261113);
            assert!((0.55..=0.85).contains(&c), "ρ {rho}: {c}");
        }
    }

    #[test]
    fn a_reading_borrows_across_a_cut_and_a_position_but_not_their_step() {
        // A take of two positions, 20 values each, a cut between them, the
        // second 20° lower. A reading of the last five borrows the 30
        // nearest: all 20 of its position and the last 10 of the one
        // before, each stretch on its own, so the step between them adds
        // nothing to the noise.
        let mut rng = Rng::new(1470);
        let noise: Vec<f64> = (0..40).map(|_| rng.normal()).collect();
        let take = |step: f64| -> Vec<AmplitudeWindow> {
            let mut w = unlock_windows(&noise);
            for (i, w) in w.iter_mut().enumerate().skip(20) {
                w.start_s += 60.0;
                w.end_s += 60.0;
                w.beat_error_unlock_ms = Some(noise[i] - step);
            }
            w
        };
        let (stepped, level) = (take(20.0), take(0.0));
        let se = |t: &[AmplitudeWindow]| beat_error_se(&t[35..], &every(t), false);
        let a = se(&stepped).expect("a ± from borrowed noise");
        let b = se(&level).unwrap();
        assert!((a - b).abs() < 1e-9 * b, "{a} against {b}");
        // The pool is the 30 nearest, in two stretches.
        let pool = noise_pool(
            &stepped,
            stepped[39].end_s,
            |w| (w.start_s, w.end_s),
            |w| w.beat_error_unlock_ms,
        );
        assert_eq!(pool.iter().map(Vec::len).collect::<Vec<_>>(), [10, 20]);
        // Its own five alone give none.
        let mine = &stepped[35..];
        assert_eq!(beat_error_se(mine, &every(mine), false), None);
    }

    #[test]
    fn a_reading_early_in_its_take_borrows_after_it() {
        // Before first, then after: a reading of the first five of 40
        // takes the first 30.
        let values: Vec<f64> = (0..40).map(|i| i as f64).collect();
        let take = unlock_windows(&values);
        let pool = noise_pool(
            &take,
            take[4].end_s,
            |w| (w.start_s, w.end_s),
            |w| w.beat_error_unlock_ms,
        );
        assert_eq!(pool, [values[..30].to_vec()]);
        // A reading ending at 30 takes the 30 ending there.
        let pool = noise_pool(
            &take,
            take[34].end_s,
            |w| (w.start_s, w.end_s),
            |w| w.beat_error_unlock_ms,
        );
        assert_eq!(pool, [values[5..35].to_vec()]);
        // A take of 29 gives none, and a reading of 30 its own.
        assert!(noise_pool(
            &take[..29],
            1e9,
            |w| (w.start_s, w.end_s),
            |w| { w.beat_error_unlock_ms }
        )
        .is_empty());
        assert_eq!(beat_error_se(&take[..29], &every(&take[..29]), false), None);
        let own = beat_error_se(&take[..30], &[], false);
        assert_eq!(own, standard_error(&values[..30]));
    }

    #[test]
    fn standard_error_allows_for_correlated_readings() {
        // One SE either side should hold the true centre about 68% of the
        // time. Under independent readings the old figure held 70% of 2,000
        // series, at ρ 0.5 37.5% and at ρ 0.9 7% (docs/research/model-probes).
        for rho in [0.0, 0.5, 0.9] {
            let c = coverage(rho, 300, 600);
            assert!((0.55..=0.85).contains(&c), "ρ {rho}: {c}");
        }
    }

    #[test]
    fn effective_readings_follow_the_lag_one_correlation() {
        assert_eq!(effective_readings(100.0, 0.0), 100.0);
        assert!((effective_readings(100.0, 0.5) - 100.0 / 3.0).abs() < 1e-12);
        // Negative correlation is not credited with more than n readings,
        // nor ρ near 1 with fewer than one.
        assert_eq!(effective_readings(100.0, -0.5), 100.0);
        assert!((effective_readings(1000.0, 1.0) - 1000.0 * 0.01 / 1.99).abs() < 1e-9);
        assert_eq!(effective_readings(100.0, 0.99), 1.0);
        assert_eq!(effective_readings(1.0, 0.99), 1.0);
        assert_eq!(effective_readings(100.0, f64::NAN), 100.0);
    }

    #[test]
    fn effective_readings_follow_the_noise_at_the_line() {
        // At zero cycles, the AR(1) count; a line as slow as the readings
        // allow is near it, and a fast one under noise that follows
        // itself is held to n (the noise is weaker there than white).
        for rho in [0.0, 0.5, 0.9] {
            let flat = effective_readings(500.0, rho);
            assert!((effective_readings_at(500.0, rho, 0.0) - flat).abs() < 1e-9);
            assert!((effective_readings_at(500.0, rho, 0.001) - flat).abs() < 0.05 * flat + 1e-9);
            assert_eq!(effective_readings_at(500.0, rho, 0.25), 500.0);
        }
        // Noise that alternates is strongest at the fastest line.
        assert!(effective_readings_at(500.0, -0.5, 0.5) < 500.0);
    }

    #[test]
    fn pooled_lag_one_reads_each_stretch_alone() {
        let wave =
            |k: usize| -> Vec<f64> { (0..200).map(|i| ((i + k) as f64 * 0.3).sin()).collect() };
        let (a, b) = (wave(0), wave(7));
        // One stretch: the same as acf at lag one of the values less their line.
        let mut x = a.clone();
        crate::dsp::detrend_line(&mut x);
        let acf = crate::steadiness::acf(&x, 1)[1];
        assert!((pooled_lag_one([a.as_slice()]).unwrap() - acf).abs() < 1e-12);
        // A step in level and slope between two stretches is not memory:
        // the second raised by 50 and tilted reads as the two alone do.
        let raised: Vec<f64> = b
            .iter()
            .enumerate()
            .map(|(i, v)| v + 50.0 + 0.1 * i as f64)
            .collect();
        let joined: Vec<f64> = a.iter().chain(&raised).copied().collect();
        let pooled = pooled_lag_one([a.as_slice(), raised.as_slice()]).unwrap();
        let alone = pooled_lag_one([a.as_slice(), b.as_slice()]).unwrap();
        assert!((pooled - alone).abs() < 1e-12, "{pooled} vs {alone}");
        assert!((pooled - 0.3f64.cos()).abs() < 0.05, "{pooled}");
        assert!(
            lag_one(&joined).unwrap() > 0.95,
            "read as one series the step is memory"
        );
        // Too short to read is passed over.
        assert_eq!(pooled_lag_one([[1.0, 2.0].as_slice()]), None);
        assert_eq!(
            pooled_lag_one([[1.0, 2.0].as_slice(), a.as_slice()]),
            lag_one(&a)
        );
    }

    /// A step is not memory: independent readings with a level change
    /// read ρ about 0 once [`Slow::Step`] takes the step out, and near 1
    /// with only a line taken out.
    #[test]
    fn long_run_rho_takes_a_step_out() {
        let mut seed = 5u64;
        let y: Vec<f64> = (0..400)
            .map(|i| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let u = (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5;
                u + if i >= 150 { 3.0 } else { 0.0 }
            })
            .collect();
        assert!(
            long_run_rho(&y, Slow::Step) < 0.05,
            "{}",
            long_run_rho(&y, Slow::Step)
        );
        assert!(
            long_run_rho(&y, Slow::Line) > 0.5,
            "{}",
            long_run_rho(&y, Slow::Line)
        );
        assert_eq!(long_run_sd(&y[..2], 1.0, Slow::Step), 1.0);
    }

    /// [`STEP_CHOICE_BIAS`]'s calibration (#917): on seeded AR(1)
    /// readings the mean of [`long_run_rho`] with [`Slow::ChosenStep`]
    /// is the true ρ within ±0.005 at ρ 0.5 and 0.9, n 180 to 1440. At
    /// ρ 0 the hold at 0 is a floor, not a bias: a mean of half-clamped
    /// readings sits above 0 whatever is added, so there it only stays
    /// under 0.04.
    #[test]
    fn long_run_rho_with_a_step_out_is_unbiased_on_ar1_readings() {
        let mut rng = crate::synth::Rng::new(917);
        for n in [180usize, 360, 720, 1440] {
            for rho in [0.0f64, 0.5, 0.9] {
                let reps = 4000;
                let mean = (0..reps)
                    .map(|_| {
                        let mut x = rng.normal() / (1.0 - rho * rho).sqrt();
                        let y: Vec<f64> = (0..n)
                            .map(|_| {
                                let v = x;
                                x = rho * x + rng.normal();
                                v
                            })
                            .collect();
                        long_run_rho(&y, Slow::ChosenStep)
                    })
                    .sum::<f64>()
                    / reps as f64;
                if rho == 0.0 {
                    assert!(mean < 0.04, "n {n}: {mean}");
                } else {
                    assert!((mean - rho).abs() < 0.005, "n {n}, ρ {rho}: {mean}");
                }
            }
        }
    }
    #[test]
    fn lag_one_ignores_a_steady_drift() {
        let wave: Vec<f64> = (0..200).map(|i| (i as f64 * 0.3).sin()).collect();
        let ramp: Vec<f64> = wave
            .iter()
            .enumerate()
            .map(|(i, v)| v + 0.2 * i as f64)
            .collect();
        let (a, b) = (lag_one(&wave).unwrap(), lag_one(&ramp).unwrap());
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
        assert!((a - 0.3f64.cos()).abs() < 0.05, "{a}");
        assert_eq!(lag_one(&[1.0, 2.0]), None);
    }
}

#[cfg(test)]
mod window_floor_tests {
    use super::*;

    #[test]
    fn too_few_windows_is_under_a_tenth() {
        assert!(too_few_windows(2, 1800));
        assert!(too_few_windows(179, 1800));
        assert!(!too_few_windows(180, 1800));
        // A short reading's few cells: the share alone decides.
        assert!(!too_few_windows(2, 3));
        assert!(!too_few_windows(1, 10));
        assert!(too_few_windows(2, 30));
        assert!(!too_few_windows(3, 30));
    }

    /// Run 11 of #1500's walks: the rebuilt template read both sides in 1
    /// window in 11 where the lock's read 7 in 10, lower by 0.61 against
    /// 2 SE of 0.43. Shares a few windows apart are within.
    #[test]
    fn share_differs_by_two_standard_errors() {
        assert_eq!(share_differs(7.0, 10.0, 1.0, 11.0), Some(false));
        assert_eq!(share_differs(1.0, 11.0, 7.0, 10.0), Some(true));
        assert_eq!(share_differs(7.0, 10.0, 5.0, 11.0), None);
        assert_eq!(share_differs(30.0, 30.0, 27.0, 30.0), None);
        // Every one against none, with few: still clear.
        assert_eq!(share_differs(10.0, 10.0, 0.0, 10.0), Some(false));
        assert_eq!(share_differs(5.0, 10.0, 0.0, 0.0), None);
    }
}

#[cfg(test)]
mod sign_agreement_tests {
    use super::*;

    /// `n` values about `centre`, spread evenly over ±`half`.
    fn spread(centre: f64, half: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| centre - half + 2.0 * half * (i as f64 + 0.5) / n as f64)
            .collect()
    }

    #[test]
    fn normal_cdf_matches_its_table() {
        for (x, p) in [
            (0.0, 0.5),
            (1.0, 0.841_344_7),
            (-1.96, 0.024_997_9),
            (3.0, 0.998_650_1),
        ] {
            assert!((normal_cdf(x) - p).abs() < 2e-7, "{x}");
        }
    }

    /// The YM42 run-down's hour 56 (#1439): 79% of its unlock windows near
    /// −5 ms, the rest near the watch's +0.6 ms. The cluster is tight, so
    /// nearly all should share the median's sign, and a fifth do not.
    #[test]
    fn a_far_cluster_with_a_fifth_across_zero_is_inconsistent() {
        let mut v = spread(-4.9, 0.8, 1286);
        v.extend(spread(0.6, 0.1, 342));
        let a = sign_agreement(&v).unwrap();
        assert!(a.predicted > 0.999, "{a:?}");
        assert!((a.observed - 0.79).abs() < 0.01, "{a:?}");
        assert!(unlock_inconsistent(&v));
    }

    /// The line sits at 0.18, above the 0.127 the healthy corpus reaches
    /// (#1439): many windows with 15% across zero, where the spread
    /// predicts almost none, are not judged inconsistent.
    #[test]
    fn a_shortfall_below_the_line_is_consistent() {
        let mut v = spread(-4.9, 0.8, 1700);
        v.extend(spread(0.6, 0.1, 300));
        let a = sign_agreement(&v).unwrap();
        assert!((a.shortfall() - 0.15).abs() < 0.01, "{a:?}");
        assert_eq!(a.line, MAX_SIGN_SHORTFALL, "{a:?}");
        assert!(!unlock_inconsistent(&v), "{a:?}");
    }

    /// A steady watch, and one whose beat error is near zero: its windows
    /// fall either side as their spread predicts, and neither is judged
    /// inconsistent.
    #[test]
    fn a_steady_or_near_zero_beat_error_is_consistent() {
        assert!(!unlock_inconsistent(&spread(0.5, 0.06, 1800)));
        let near_zero = spread(0.01, 0.3, 300);
        let a = sign_agreement(&near_zero).unwrap();
        assert!(a.predicted < 0.6, "{a:?}");
        assert!(!unlock_inconsistent(&near_zero));
    }

    /// The line widens for few windows, to 3 √(q(1 − q)/n), so a short
    /// reading is not judged on noise alone: 10 values with 4 across zero
    /// predict p ≈ 0.69, and their line is ≈ 0.44, where 1,000 of the same
    /// shape have 0.18.
    #[test]
    fn the_line_widens_for_few_windows() {
        let shape = |n: usize| {
            let k = n * 6 / 10;
            let mut v = spread(1.0, 0.6, k);
            v.extend(spread(-0.6, 0.2, n - k));
            sign_agreement(&v).unwrap()
        };
        let (few, many) = (shape(10), shape(1000));
        let var = |p: f64| p * (1.0 - p);
        let wide = 3.0 * (var(few.predicted).max(var(few.observed)) / 10.0).sqrt();
        assert!((few.line - wide).abs() < 1e-12, "{few:?}");
        assert!(few.line > 0.4, "{few:?}");
        assert_eq!(many.line, MAX_SIGN_SHORTFALL, "{many:?}");
        // A tight cluster with one stray of 5 (a live 10 s reading) or 2
        // of 10 is not judged inconsistent: the line follows the observed
        // share's noise too (Norm's HOLD on #1458).
        for (n, stray) in [(5, 1), (10, 2)] {
            let mut v = spread(0.5, 0.02, n - stray);
            v.extend(vec![-0.4; stray]);
            let a = sign_agreement(&v).unwrap();
            assert!(a.shortfall() > MAX_SIGN_SHORTFALL, "{a:?}");
            assert!(!unlock_inconsistent(&v), "{n}: {a:?}");
        }
        // Too few values, or no spread, are not judged.
        assert!(sign_agreement(&[1.0, -1.0]).is_none());
        assert!(sign_agreement(&[0.5; 20]).is_none());
    }
}
