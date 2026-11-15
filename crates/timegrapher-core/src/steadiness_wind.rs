//! A series' hours and the wind: since full wind only when it was given
//! (#1665).

/// Hours since full wind `from_start_h` hours into a take begun
/// `wind_h` hours after full wind; `None` when the wind was not given,
/// which is never taken as full wind (#1665). The one place every
/// series figure's wind is read from.
pub fn hours_since_wind(wind_h: Option<f64>, from_start_h: f64) -> Option<f64> {
    wind_h.map(|w| w + from_start_h)
}

/// Hours `from`–`to` as a series' text gives them: "2.0–3.0 h after full
/// wind" when the wind was given, else "2.0–3.0 h into the take (wind not
/// given)", hours from the take's start that stand in for no state of
/// wind (#1665).
pub fn hours_words(from_h: f64, to_h: f64, wind_h: Option<f64>) -> String {
    use crate::figure_text::fixed_text;
    match (
        hours_since_wind(wind_h, from_h),
        hours_since_wind(wind_h, to_h),
    ) {
        (Some(a), Some(b)) => format!(
            "{}–{} h after full wind",
            fixed_text(a, 1),
            fixed_text(b, 1)
        ),
        _ => format!(
            "{}–{} h into the take ({})",
            fixed_text(from_h, 1),
            fixed_text(to_h, 1),
            crate::reading_text::WIND_NOT_GIVEN
        ),
    }
}
