//! Each reading's ± from the spread between short windows, and the
//! conditions detected in the recording beside it.
//!
//! The ± estimates precision from successive window differences, allowing
//! for the readings' lag-one correlation through an effective number of
//! readings. It is not calibrated repeat-measurement coverage for every
//! kind of serial correlation, or a measure of true watch variation.
//! Accuracy also rests on the lift angle and sound card's clock; these are
//! stated as conditions ([`Doubts`] and [`crate::measure`]), not folded into
//! the ±. See docs/measurement-precision.md for the model and its limits.

use crate::amplitude::AmplitudeWindow;
use crate::timing::WindowFit;
use serde::Serialize;

/// Beats found below this share of the beats expected is the
/// `beats_missing` condition.
pub const MIN_BEAT_FRACTION: f64 = 0.95;

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
/// held to 0 ..= 0.99. On a seeded test of 360 AR(1) readings this keeps
/// the CUSUM's false alarms at 4-6% from ρ 0 to 0.9, and under 8% at
/// ρ 0.9 with block gaps or scattered dropouts (#647).
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
        Slow::Step => take_out_step(&mut x),
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
    let r = r + 2.0 * (1.0 + 3.0 * r) / n;
    if r.is_finite() {
        r.clamp(0.0, 0.99)
    } else {
        0.0
    }
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

/// Estimated standard error of a median from values in time order, in their
/// own units. The scatter comes from successive differences, so a linear
/// drift cancels exactly: s = 1.4826 MAD(differences) / sqrt(2). With the
/// values' lag-one correlation ρ ([`lag_one`], negative taken as 0), the
/// differences understate the spread by sqrt(1 − ρ) and the values are worth
/// [`effective_readings`] independent ones, so
/// SE = 1.2533 × s / sqrt(1 − ρ) / sqrt(n_eff). With ρ = 0 this is
/// 1.2533 s / sqrt(n), the median's standard error under independent
/// Gaussian noise. None with fewer than three values.
pub fn standard_error(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }
    let d: Vec<f64> = values.windows(2).map(|w| w[1] - w[0]).collect();
    let s = crate::dsp::robust_sd(&d) / std::f64::consts::SQRT_2;
    let rho = lag_one(values).unwrap_or(0.0).clamp(0.0, 0.99);
    let n_eff = effective_readings(values.len() as f64, rho);
    Some(1.2533 * s / (1.0 - rho).sqrt() / n_eff.sqrt())
}

/// Estimated standard error of the rate from the 10 s windows. Taking every
/// other available window removes ordinary half-window overlap; the
/// correlation left between the windows taken is allowed for by
/// [`standard_error`].
pub fn rate_se(windows: &[WindowFit]) -> Option<f64> {
    let v: Vec<f64> = windows
        .iter()
        .step_by(2)
        .map(|w| w.fit.rate_s_per_day)
        .filter(|r| r.is_finite())
        .collect();
    standard_error(&v)
}

/// Standard error of the beat error from the unlock, ms.
pub fn beat_error_se(windows: &[AmplitudeWindow]) -> Option<f64> {
    let v: Vec<f64> = windows
        .iter()
        .filter_map(|w| w.beat_error_unlock_ms)
        .collect();
    standard_error(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::Rng;

    #[test]
    fn beat_error_se_reads_the_unlock_beat_errors() {
        // Every third window has no unlock beat error and is skipped.
        let windows: Vec<AmplitudeWindow> = (0..30)
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
            })
            .collect();
        let kept: Vec<f64> = windows
            .iter()
            .filter_map(|w| w.beat_error_unlock_ms)
            .collect();
        let se = beat_error_se(&windows).unwrap();
        assert!(se > 0.0, "{se}");
        assert_eq!(Some(se), standard_error(&kept));
        assert_eq!(beat_error_se(&windows[..2]), None);
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

    #[test]
    fn standard_error_allows_for_correlated_readings() {
        // One SE either side should hold the true centre about 68% of the
        // time. Under independent readings the old figure held 70% of 2,000
        // series, at ρ 0.5 37.5% and at ρ 0.9 7% (docs/model-probes).
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
