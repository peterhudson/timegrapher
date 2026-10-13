//! Each reading's ± from the spread between short windows, and the
//! conditions detected in the recording beside it.
//!
//! The ± is precision only: how much the reading would move on another
//! stretch of the same recording. Accuracy rests on things the recording
//! cannot show (the lift angle, the sound card's clock); those are stated
//! as conditions ([`Doubts`], and the caveats of [`crate::measure`]), not
//! folded into the ±.

use crate::amplitude::{standard_error, AmplitudeWindow};
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

/// Standard error of the rate from the 10 s windows. The windows overlap
/// by half, so every other one is used to keep them independent.
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
