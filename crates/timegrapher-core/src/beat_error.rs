//! Beat error shown to the user is a magnitude. Keep the signed short-window
//! measurements for timing and profile alignment, and take their magnitudes
//! before combining windows: a change of Tick/Tock labels must not cancel BE.

use crate::amplitude::AmplitudeWindow;
use serde::Serialize;

/// Medians of the magnitudes of local BE estimates, in milliseconds.
/// These are separate from the signed diagnostic fields in a reading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Magnitudes {
    pub unlock_ms: Option<f64>,
    pub drop_ms: Option<f64>,
}

impl Magnitudes {
    /// Call with only the windows allowed by the take's masks and breaks.
    /// Each input is an already fitted local estimate, not a raw interval.
    pub fn of<'a>(windows: impl IntoIterator<Item = &'a AmplitudeWindow>) -> Self {
        let windows: Vec<_> = windows.into_iter().collect();
        Self {
            unlock_ms: median(windows.iter().filter_map(|w| w.beat_error_unlock_ms)),
            drop_ms: median(windows.iter().filter_map(|w| w.beat_error_ms)),
        }
    }

    /// Prefer the unlock; the drop remains available when no unlock was found.
    pub fn primary(self) -> Option<f64> {
        self.unlock_ms.or(self.drop_ms)
    }
}

/// Median of finite magnitudes. Never take the magnitude after averaging
/// signed windows: even two equally good opposite signs would cancel.
pub fn median(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values: Vec<_> = values.filter(|v| v.is_finite()).map(f64::abs).collect();
    (!values.is_empty()).then(|| crate::dsp::median(&mut values))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnitudes_precede_the_median_even_without_a_known_break() {
        assert_eq!(median([-0.5, 0.5].into_iter()), Some(0.5));
        assert_eq!(median([-0.4, 0.6].into_iter()), Some(0.5));
        assert_eq!(
            median([f64::NAN, f64::INFINITY, -0.4].into_iter()),
            Some(0.4)
        );
        assert_eq!(median([f64::NAN].into_iter()), None);
    }
}
