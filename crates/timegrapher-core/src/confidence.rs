//! How far each reading can be trusted: a ± from the spread between short
//! windows and a grade a person can read at a glance.
//!
//! The ± is precision only: how much the reading would move on another
//! stretch of the same recording. Accuracy rests on things the recording
//! cannot show (the lift angle, the sound card's clock), so those lower the
//! grade instead of widening the ±.

use crate::amplitude::{standard_error, AmplitudeWindow};
use crate::timing::WindowFit;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Grade {
    Poor,
    Fair,
    Good,
}

impl Grade {
    fn lower(self) -> Grade {
        match self {
            Grade::Good => Grade::Fair,
            _ => Grade::Poor,
        }
    }

    fn from_se(se: Option<f64>, good: f64, fair: f64) -> Grade {
        match se {
            Some(e) if e <= good => Grade::Good,
            Some(e) if e <= fair => Grade::Fair,
            _ => Grade::Poor,
        }
    }
}

/// Upper bounds of the ± for Good and Fair: rate s/d, amplitude degrees,
/// beat error ms. Good is a little above what the regression check counts
/// as a real change (0.3 s/d, 1.5°, 0.03 ms).
pub const RATE_SE: (f64, f64) = (0.5, 2.0);
pub const AMPLITUDE_SE: (f64, f64) = (2.0, 5.0);
pub const BEAT_ERROR_SE: (f64, f64) = (0.05, 0.1);

/// Beats found below this share of the beats expected lower every grade.
pub const MIN_BEAT_FRACTION: f64 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Reading {
    /// Standard error, in the reading's own unit.
    pub se: Option<f64>,
    pub grade: Grade,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Confidence {
    pub rate: Reading,
    pub amplitude: Reading,
    pub beat_error: Reading,
}

/// What lowers a grade beyond its ±.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Doubts {
    /// More than 1% of beats clipped: amplitude and beat error.
    pub clipped: bool,
    /// Fewer than [`MIN_BEAT_FRACTION`] of the beats expected: all three.
    pub beats_missing: bool,
    /// The lift angle is a default, not the calibre's: amplitude.
    pub lift_unknown: bool,
    /// No correction for the sound card's clock: rate.
    pub clock_unknown: bool,
}

impl Confidence {
    pub fn new(
        rate_se: Option<f64>,
        amplitude_se: Option<f64>,
        beat_error_se: Option<f64>,
        d: Doubts,
    ) -> Self {
        Confidence {
            rate: rate(rate_se, d),
            amplitude: amplitude(amplitude_se, d),
            beat_error: beat_error(beat_error_se, d),
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

pub fn rate(se: Option<f64>, d: Doubts) -> Reading {
    let mut g = Grade::from_se(se, RATE_SE.0, RATE_SE.1);
    if d.beats_missing {
        g = g.lower();
    }
    if d.clock_unknown {
        g = g.lower();
    }
    Reading { se, grade: g }
}

pub fn amplitude(se: Option<f64>, d: Doubts) -> Reading {
    let mut g = Grade::from_se(se, AMPLITUDE_SE.0, AMPLITUDE_SE.1);
    for doubt in [d.clipped, d.beats_missing, d.lift_unknown] {
        if doubt {
            g = g.lower();
        }
    }
    Reading { se, grade: g }
}

pub fn beat_error(se: Option<f64>, d: Doubts) -> Reading {
    let mut g = Grade::from_se(se, BEAT_ERROR_SE.0, BEAT_ERROR_SE.1);
    for doubt in [d.clipped, d.beats_missing] {
        if doubt {
            g = g.lower();
        }
    }
    Reading { se, grade: g }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grades_follow_the_se_and_each_doubt_lowers_one_step() {
        let none = Doubts::default();
        assert_eq!(rate(Some(0.2), none).grade, Grade::Good);
        assert_eq!(rate(Some(1.0), none).grade, Grade::Fair);
        assert_eq!(rate(Some(3.0), none).grade, Grade::Poor);
        assert_eq!(rate(None, none).grade, Grade::Poor);
        let clock = Doubts {
            clock_unknown: true,
            ..none
        };
        assert_eq!(rate(Some(0.2), clock).grade, Grade::Fair);
        // Clipping touches amplitude and beat error, not rate.
        let clipped = Doubts {
            clipped: true,
            ..none
        };
        assert_eq!(rate(Some(0.2), clipped).grade, Grade::Good);
        assert_eq!(amplitude(Some(1.0), clipped).grade, Grade::Fair);
        assert_eq!(beat_error(Some(0.01), clipped).grade, Grade::Fair);
        let two = Doubts {
            clipped: true,
            lift_unknown: true,
            ..none
        };
        assert_eq!(amplitude(Some(1.0), two).grade, Grade::Poor);
    }
}
