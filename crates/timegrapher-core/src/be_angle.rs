//! The beat error as an angle of the balance's swing, "BE Angle" (#1576
//! row 2, Horace's audit, Witschi 2025 p. 8; Peter named it 13 Nov 2026).
//!
//! The beat error in ms is the time between the balance passing its
//! centre and the escapement's mid point, so it grows as the amplitude
//! falls even when nothing in the escapement has moved. As an angle it
//! stays put: angle = amplitude × π × (bph / 7200) × beat error (Norm).
//! The balance swings through its centre at its fastest, amplitude × 2π ×
//! frequency degrees a second, with frequency = bph / 7200 Hz; one half of
//! that factor goes because the swing is measured either side of centre.
//! It rests on the lift angle through the amplitude, so a lift angle 7%
//! off makes the angle 7% off; the beat error in ms does not.
//!
//! It keeps as many significant figures as the fewest of its inputs, the
//! beat error and the amplitude, each as written at its place (math guide
//! section 11; Peter, 13 Nov 2026): 0.46 ms at 302° is 1.7°, 0.47 ms at
//! 129° is 0.76°. Its ± is rounded up at that place.

use crate::figure_text::{base_digits, derived_digits, figure_with_pm, significant_figures};

/// Its name, on every surface that shows it (Peter: "Angle" alone would
/// be ambiguous).
pub const NAME: &str = "BE Angle";

/// The angle, degrees, of a beat error `beat_error_ms` (its size) at
/// `amplitude_deg` on a watch beating `bph`.
/// With the beat period T = 3600 / bph ([`crate::timing::beat_period_s`]),
/// bph / 7200 is 1 / (2T).
pub fn degrees(amplitude_deg: f64, beat_error_ms: f64, bph: u32) -> f64 {
    let period_s = crate::timing::beat_period_s(None, bph);
    amplitude_deg * std::f64::consts::PI * (beat_error_ms.abs() / 1000.0) / (2.0 * period_s)
}

/// A BE Angle with its ± and the decimals it is written to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Angle {
    pub degrees: f64,
    pub se: Option<f64>,
    /// Decimals: as many significant figures as the fewest of its inputs.
    pub digits: usize,
}

impl Angle {
    /// The angle of a beat error and an amplitude, each `(value, its ±)`,
    /// at `bph`. None without a positive amplitude, a beat error or a
    /// beat rate. The ± takes both inputs' relative ±s in quadrature, and
    /// is none unless both have one.
    pub fn of(
        (amplitude_deg, amplitude_se): (f64, Option<f64>),
        (beat_error_ms, beat_error_se): (f64, Option<f64>),
        bph: u32,
    ) -> Option<Angle> {
        let be = beat_error_ms.abs();
        let ok = |v: f64| v.is_finite() && v > 0.0;
        if !(ok(amplitude_deg) && be.is_finite() && bph > 0) {
            return None;
        }
        let d = degrees(amplitude_deg, be, bph);
        let se = match (amplitude_se, beat_error_se) {
            (Some(sa), Some(sb)) if be > 0.0 => {
                Some(d * ((sa / amplitude_deg).powi(2) + (sb / be).powi(2)).sqrt())
            }
            _ => None,
        };
        let sig = significant_figures(amplitude_deg, base_digits::AMPLITUDE)
            .min(significant_figures(be, base_digits::BEAT_ERROR));
        Some(Angle {
            degrees: d,
            se,
            digits: derived_digits(d, sig),
        })
    }

    /// As a table writes it, no unit: "1.7", "0.76".
    pub fn number(&self) -> String {
        figure_with_pm(self.degrees, None, self.digits, false)
    }

    /// With its ± and the degree sign: "1.7 ± 0.2°".
    pub fn text(&self) -> String {
        format!(
            "{}°",
            figure_with_pm(self.degrees, self.se, self.digits, false)
        )
    }
}

/// The Beat Error card's hover on the angle, under the figure the hover
/// already gives: the angle, the amplitude it is at and the lift angle it
/// rests on (Hugh: the figure isn't said twice).
pub fn card_words(angle: &Angle, amplitude_deg: f64, lift_deg: f64) -> String {
    let amp = figure_with_pm(amplitude_deg, None, base_digits::AMPLITUDE, false);
    let lift = figure_with_pm(lift_deg, None, base_digits::AMPLITUDE, false);
    format!(
        "That's a {NAME} of {} of the balance's swing, at {amp}° amplitude. \
         Degrees stay the same as the amplitude falls; ms grow. The degrees rest on the \
         lift angle, {lift}°, through the amplitude.",
        angle.text()
    )
}

/// The Power Reserve table's BE Angle column's hover.
pub const COLUMN_HINT: &str = "The hour's beat error as an angle of the balance's swing: \
     amplitude × π × beat rate / 7200 × beat error. It stays the same as the amplitude falls, \
     where ms grow, and rests on the lift angle through the amplitude. Grey where its beat error \
     or amplitude is grey.";

#[cfg(test)]
mod tests {
    use super::*;

    /// Hugh's worked example: 0.46 ms at 302° on 28,800 bph is 1.7°.
    #[test]
    fn the_worked_examples() {
        let a = Angle::of((302.0, None), (0.46, None), 28_800).unwrap();
        assert!((a.degrees - 1.7457).abs() < 1e-3, "{}", a.degrees);
        assert_eq!(a.number(), "1.7");
        let b = Angle::of((129.0, None), (0.47, None), 28_800).unwrap();
        assert_eq!(b.number(), "0.76");
        // A signed beat error reads as its size.
        let c = Angle::of((129.0, None), (-0.47, None), 28_800).unwrap();
        assert_eq!(c.number(), "0.76");
    }

    /// The ± takes both relative ±s in quadrature, rounded up at the
    /// angle's place: 0.46 ± 0.05 ms at 302 ± 2° is 1.7 ± 0.2°.
    #[test]
    fn the_pm_is_rounded_up_at_the_angles_place() {
        let a = Angle::of((302.0, Some(2.0)), (0.46, Some(0.05)), 28_800).unwrap();
        assert_eq!(a.text(), "1.7 ± 0.2°");
        let no_amp_pm = Angle::of((302.0, None), (0.46, Some(0.05)), 28_800).unwrap();
        assert_eq!(no_amp_pm.se, None);
    }

    /// Fewer significant figures in either input give fewer in the angle.
    #[test]
    fn the_fewest_significant_figures_win() {
        // 0.04 ms has one significant figure.
        let a = Angle::of((302.0, None), (0.04, None), 28_800).unwrap();
        assert_eq!(a.number(), "0.2");
        // 1.98 ms has three, 302° three.
        let b = Angle::of((302.0, None), (1.98, None), 28_800).unwrap();
        assert_eq!(b.number(), "7.51");
    }

    #[test]
    fn no_angle_without_an_amplitude_or_beat_rate() {
        assert!(Angle::of((0.0, None), (0.4, None), 28_800).is_none());
        assert!(Angle::of((f64::NAN, None), (0.4, None), 28_800).is_none());
        assert!(Angle::of((300.0, None), (0.4, None), 0).is_none());
    }

    #[test]
    fn the_card_says_what_it_rests_on() {
        let a = Angle::of((302.0, Some(2.0)), (0.46, Some(0.05)), 28_800).unwrap();
        let w = card_words(&a, 302.0, 52.0);
        assert_eq!(
            w,
            "That's a BE Angle of 1.7 ± 0.2° of the balance's swing, at 302° \
             amplitude. Degrees stay the same as the amplitude falls; ms grow. The degrees \
             rest on the lift angle, 52°, through the amplitude."
        );
    }
}
