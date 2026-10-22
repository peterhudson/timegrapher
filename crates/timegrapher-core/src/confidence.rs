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
    let r = if rho.is_finite() {
        rho.clamp(-0.99, 0.99)
    } else {
        0.0
    };
    (n * (1.0 - r) / (1.0 + r)).clamp(1.0_f64.min(n), n)
}

/// Lag-one correlation of values in time order, less their least-squares
/// line, so a steady drift is not counted as memory. `None` with fewer
/// than three values or no scatter about the line.
pub fn lag_one(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }
    let mut x = values.to_vec();
    crate::dsp::detrend_line(&mut x);
    crate::steadiness::acf(&x, 1)
        .get(1)
        .copied()
        .filter(|r| r.is_finite())
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
