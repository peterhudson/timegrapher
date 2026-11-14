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
/// "24 h after winding", hours to the whole hour, or "wind not given"
/// where the reading carries none (#1665): a reading is never called fully
/// wound unless that was set.
pub fn wind_state_words(wind_h: Option<f64>) -> String {
    wind_h.map_or_else(|| WIND_NOT_GIVEN.to_string(), wind_words)
}

/// The words for a state of wind that was not given.
pub const WIND_NOT_GIVEN: &str = "wind not given";

/// The wind on the Position card, to 0.1 h: "Full wind 2.0 h ago", or
/// "Wind not given" until it is set (#1665).
pub fn wind_since_words(wind_h: Option<f64>) -> String {
    wind_h.map_or_else(
        || capitalised(WIND_NOT_GIVEN),
        |h| format!("Full wind {} h ago", crate::figure_text::fixed_text(h, 1)),
    )
}

/// A saved take's wind, to 0.1 h: "Full wind 2.0 h before it began", or
/// "Wind not given" (#1665).
pub fn wind_before_take_words(wind_h: Option<f64>) -> String {
    wind_h.map_or_else(
        || capitalised(WIND_NOT_GIVEN),
        |h| {
            format!(
                "Full wind {} h before it began",
                crate::figure_text::fixed_text(h, 1)
            )
        },
    )
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// A state of wind that was given, as [`wind_state_words`] names it.
pub fn wind_words(wind_h: f64) -> String {
    if wind_h <= 0.0 {
        "full wind".into()
    } else {
        format!(
            "{} h after winding",
            crate::figure_text::fixed_text(wind_h, 0)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The card and a take's hover say the wind to 0.1 h, or that it was
    /// not given (#1665).
    #[test]
    fn the_wind_since_full_wind_is_said_to_a_tenth() {
        assert_eq!(wind_since_words(Some(2.0)), "Full wind 2.0 h ago");
        assert_eq!(wind_since_words(Some(0.04)), "Full wind 0.0 h ago");
        assert_eq!(wind_since_words(None), "Wind not given");
        assert_eq!(
            wind_before_take_words(Some(2.0)),
            "Full wind 2.0 h before it began"
        );
        assert_eq!(wind_before_take_words(None), "Wind not given");
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
