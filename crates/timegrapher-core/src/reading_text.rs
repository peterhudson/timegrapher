//! Shared words for a reading in the CLI and MCP JSON documents.
//! Surfaces choose the reading; core chooses its figures, precision and units.

use serde_json::Value;

/// Rate (already on the chosen clock), amplitude and unsigned beat error.
/// The unlock wins over the drop, as in every other core reading.
pub fn values(m: &Value, rate: Option<f64>, drop_ms: Option<f64>) -> [Option<f64>; 3] {
    [
        rate,
        m["amplitude_deg"].as_f64(),
        crate::session::beat_error_size(m["beat_error_unlock_ms"].as_f64(), drop_ms),
    ]
}

/// One reading's figures, each with its own standard error. Units follow
/// the math and design guides: s/d, degrees as °, and ms. Each figure is
/// written at its place, its ± rounded up there
/// ([`crate::figure_text::figure_digits`], #1470). Missing figures are
/// omitted; an entirely empty reading gives no line.
pub fn figures(m: &Value, [rate, amplitude, beat_error]: [Option<f64>; 3]) -> Option<String> {
    use crate::figure_text::{base_digits as base, figure_digits, pm_text, signed_text};
    let se = |key: &str| m["confidence"][key]["se"].as_f64();
    let pm = |key: &str, digits| {
        se(key)
            .and_then(|se| pm_text(se, digits))
            .map_or_else(String::new, |s| format!(" ± {s}"))
    };
    let mut parts = Vec::new();
    if let Some(r) = rate {
        let d = figure_digits(se("rate"), base::RATE);
        parts.push(format!(
            "rate {}{} s/d",
            signed_text(r, d),
            pm("rate", base::RATE)
        ));
    }
    if let Some(a) = amplitude {
        let d = figure_digits(se("amplitude"), base::AMPLITUDE);
        parts.push(format!(
            "amplitude {a:.d$}{}°",
            pm("amplitude", base::AMPLITUDE)
        ));
    }
    if let Some(b) = beat_error {
        let d = figure_digits(se("beat_error"), base::BEAT_ERROR);
        parts.push(format!(
            "beat error {b:.d$}{} ms",
            pm("beat_error", base::BEAT_ERROR)
        ));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// A session/take measurement's figures, or the fact that no beat was found.
pub fn measurement(m: &Value) -> String {
    if m["watch_heard"] == false {
        "no regular beat found".into()
    } else {
        figures(
            m,
            values(m, m["rate_s_per_day"].as_f64(), m["beat_error_ms"].as_f64()),
        )
        .unwrap_or_default()
    }
}

/// A known position's full name, preserving an unknown code as supplied.
pub fn position_name(p: &str) -> String {
    crate::session::Position::parse(p).map_or_else(|| p.to_string(), |p| p.display_label())
}

/// A state of wind as every surface names it: "full wind" at 0 (or less),
/// else "24 h after winding", hours to the whole hour.
pub fn wind_words(wind_h: f64) -> String {
    wind_words_to(wind_h, 0)
}

/// [`wind_words`] with the hours to `digits` decimals: see [`WindHours`].
fn wind_words_to(wind_h: f64, digits: usize) -> String {
    if wind_h <= 0.0 {
        "full wind".into()
    } else {
        format!(
            "{} h after winding",
            crate::figure_text::fixed_text(wind_h, digits)
        )
    }
}

/// Hours after full wind with the decimals they are known to, so every
/// surface writes them at the precision of their source (Norm on #1713):
/// hours a guided test measured, to a tenth ("24.2 h"); hours a person
/// gave, as given ("24 h" for 24, "24.5 h" for 24.5), never more digits
/// than were typed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindHours {
    pub hours: f64,
    pub digits: usize,
}

/// Decimals of hours a guided test measured: it times them to a tenth.
const MEASURED_DIGITS: usize = 1;

/// The most decimals hours given by hand are read to.
const GIVEN_MAX_DIGITS: usize = 3;

impl WindHours {
    /// Hours a guided test measured, to a tenth.
    pub fn measured(hours: f64) -> Self {
        Self {
            hours,
            digits: MEASURED_DIGITS,
        }
    }

    /// Hours `to` less hours `from`, each given by hand: the difference
    /// keeps the decimals of the less precise of the two (math guide,
    /// #1576). Full wind, a `from` of 0, is exact and doesn't limit it. A
    /// typed "24.0" reads as whole, as 24: the number keeps no trailing
    /// zero.
    pub fn given(from: f64, to: f64) -> Self {
        let digits = if from == 0.0 {
            given_digits(to)
        } else {
            given_digits(from).min(given_digits(to))
        };
        Self {
            hours: to - from,
            digits,
        }
    }

    /// "full wind", or "24 h after winding".
    pub fn words(self) -> String {
        wind_words_to(self.hours, self.digits)
    }

    /// The hours alone, "24 h", for a table's head.
    pub fn short(self) -> String {
        format!(
            "{} h",
            crate::figure_text::fixed_text(self.hours, self.digits)
        )
    }
}

/// How many decimals `x` was given to: 0 for 24, 1 for 24.5.
fn given_digits(x: f64) -> usize {
    (0..GIVEN_MAX_DIGITS)
        .find(|&d| {
            let scaled = x * 10f64.powi(d as i32);
            (scaled - scaled.round()).abs() < 1e-6
        })
        .unwrap_or(GIVEN_MAX_DIGITS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hours are written at the precision of their source (Norm on
    /// #1713): a given whole hour stays whole, measured hours keep their
    /// tenth even when it is 0.
    #[test]
    fn wind_hours_keep_the_precision_of_their_source() {
        assert_eq!(WindHours::given(0.0, 24.0).words(), "24 h after winding");
        assert_eq!(WindHours::given(0.0, 24.0).short(), "24 h");
        // 24 is known only to the hour, so the difference is too.
        assert_eq!(WindHours::given(0.5, 24.0).words(), "24 h after winding");
        assert_eq!(WindHours::given(0.1, 24.3).short(), "24.2 h");
        // Full wind is exact: it doesn't take the tenth from 24.5.
        assert_eq!(WindHours::given(0.0, 24.5).short(), "24.5 h");
        assert_eq!(WindHours::measured(24.0).words(), "24.0 h after winding");
        assert_eq!(WindHours::measured(24.2).short(), "24.2 h");
        assert_eq!(WindHours::measured(0.0).words(), "full wind");
    }

    #[test]
    fn reading_text_matches_the_cross_surface_cases() {
        let cases: Value =
            serde_json::from_str(include_str!("../tests/fixtures/reading_text.json")).unwrap();
        for c in cases.as_array().unwrap() {
            assert_eq!(measurement(&c["measurement"]), c["text"].as_str().unwrap());
            assert_eq!(
                position_name(c["position"].as_str().unwrap()),
                c["position_text"].as_str().unwrap()
            );
        }
    }
}
