//! The Dynamic Poise picture (#1474, #1729): the rate by the hour at the
//! top, drawn along a line (Plot) or round a watch face (Dial), with the
//! same points, wave, mean and words on every surface. The app's pane,
//! the MCP view and any other surface draw this; none works out its own
//! scale or writes its own words.

use crate::figure_text::{base_digits, signed_text, true_minus};
use crate::poise::{self, PoiseFit};
use crate::session::{Position, Reading};
use serde::Serialize;

/// The test's name, as its pane is headed on every surface.
pub const TITLE: &str = "Dynamic Poise";
/// The plot's across axis: the hour at the top.
pub const HOUR_AXIS: &str = "Hour at the top";
/// What the dial's top right says.
pub const DIAL_KEY: &str = "Further out = faster, s/d";
/// The key's row for the fitted wave.
pub const WAVE_KEY: &str = "Fitted wave";
/// The steps the wave is cut into for the scale, and for a surface that
/// draws it from [`Picture::wave`].
pub const WAVE_STEPS: usize = 120;

/// The key's row for the measured points: "Upright positions (8)".
pub fn points_key(n: usize) -> String {
    format!("Upright positions ({n})")
}

/// The fastest hour's name over its line: "Fastest 3H".
pub fn fastest_name(deg: f64) -> String {
    format!("Fastest {}", poise::hour_code(deg))
}

/// The table's Mean row's name, its rate in the cell beside it.
pub const MEAN: &str = "Mean";

/// The mean's name on the picture, with its rate: "Mean +1.2".
pub fn mean_name(mean: f64) -> String {
    format!(
        "{MEAN} {}",
        true_minus(&signed_text(mean, base_digits::RATE))
    )
}

/// Each measured upright position of `readings`: (hour at the top in
/// degrees, rate s/d, its ±), the points a picture draws.
pub fn points(readings: &[Reading]) -> Vec<(f64, f64, Option<f64>)> {
    readings
        .iter()
        .filter_map(|r| poise::point(r.position, &r.measurement))
        .map(|p| (p.angle_deg, p.rate_s_per_day, p.rate_se))
        .collect()
}

/// The table's Mean over the upright positions of `readings`, s/d (core
/// `sourced_indices_over`, repeat readings of a position averaged first).
pub fn mean(readings: &[Reading]) -> Option<f64> {
    let figures = crate::text_report::figures(readings);
    crate::session::sourced_indices_over(None, &figures, &Position::UPRIGHT).x
}

/// The rates a picture must hold: each point with its ±, and the wave.
pub fn range(points: &[(f64, f64, Option<f64>)], fit: Option<&PoiseFit>) -> Option<(f64, f64)> {
    let mut v: Vec<f64> = Vec::new();
    for &(_, r, se) in points {
        let e = se.unwrap_or(0.0);
        v.extend([r - e, r + e]);
    }
    if let Some(f) = fit {
        v.extend(f.curve(WAVE_STEPS).into_iter().map(|(_, r)| r));
    }
    crate::chart_scale::data_range(&v)
}

/// The rate scale, [lo, hi, step]: [`range`] rounded to whole steps, or
/// ±1 s/d before any rate.
pub fn scale(points: &[(f64, f64, Option<f64>)], fit: Option<&PoiseFit>) -> (f64, f64, f64) {
    let (lo, hi) = range(points, fit).unwrap_or((-1.0, 1.0));
    crate::chart_scale::round_ends(lo, hi)
}

/// One measured upright position.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dot {
    /// The hour at the top, degrees from 12H.
    pub angle_deg: f64,
    /// Its code: "12H", "1H30", ...
    pub code: &'static str,
    pub rate_s_per_day: f64,
    pub rate_se: Option<f64>,
    /// Its rate with its ± and unit, for a hover: "+2.0 ± 0.3 s/d".
    pub text: String,
    /// The same bare, for a table cell under the Rate's unit: "+2.0 ± 0.3".
    pub figure: String,
}

/// A named line: the mean, or the fastest hour.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Named {
    /// The rate (the mean) or the hour at the top (the fastest), degrees.
    pub at: f64,
    /// The fastest hour's rate, where the line ends; the mean's rate.
    pub rate_s_per_day: f64,
    pub name: String,
    /// The rate bare, for a table cell under the Rate's unit: "−1.9".
    pub figure: String,
}

/// What a surface draws for one state of wind's upright readings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Picture {
    pub points: Vec<Dot>,
    /// Core's fitted wave at [`WAVE_STEPS`] + 1 angles, [deg, rate]; empty
    /// until it is fitted.
    pub wave: Vec<[f64; 2]>,
    /// The fastest hour, named once the wave is clear of the noise.
    pub fastest: Option<Named>,
    /// Where the labels sit clear of the wave: the slowest hour, degrees.
    pub slowest_deg: Option<f64>,
    /// The table's Mean over the upright positions, drawn with the wave.
    pub mean: Option<Named>,
    /// The rate scale: [lo, hi, step], in `unit`.
    pub axis: [f64; 3],
    pub unit: &'static str,
    /// The key's rows: the points, then the wave once fitted.
    pub key: Vec<String>,
    /// The heavy spot's row for the facts box once there is a fit.
    pub heavy: Option<(String, String)>,
    /// The upright stops' hours round the face, [deg, code], 45° apart
    /// from 12H.
    pub hours: Vec<(f64, &'static str)>,
    /// The pane's title ([`TITLE`]), the across axis's name, the dial's
    /// key and the table's Mean row's name ([`MEAN`]).
    pub title: &'static str,
    pub hour_axis: &'static str,
    pub dial_key: &'static str,
    pub mean_title: &'static str,
}

/// The picture of `readings`' upright positions and their `fit`.
pub fn picture(readings: &[Reading], fit: Option<&PoiseFit>) -> Picture {
    let raw = points(readings);
    let (lo, hi, step) = scale(&raw, fit);
    let dots = raw
        .iter()
        .map(|&(angle_deg, rate, se)| Dot {
            angle_deg,
            code: poise::hour_code(angle_deg),
            rate_s_per_day: rate,
            rate_se: se,
            text: dot_text(rate, se),
            figure: dot_figure(rate, se),
        })
        .collect();
    let mean = fit.and_then(|_| {
        let x = mean(readings)?;
        Some(Named {
            at: x,
            rate_s_per_day: x,
            name: mean_name(x),
            figure: true_minus(&signed_text(x, base_digits::RATE)),
        })
    });
    let mut key = Vec::new();
    if !raw.is_empty() {
        key.push(points_key(raw.len()));
        if fit.is_some() {
            key.push(WAVE_KEY.to_string());
        }
    }
    Picture {
        points: dots,
        wave: fit.map_or_else(Vec::new, |f| {
            f.curve(WAVE_STEPS)
                .into_iter()
                .map(|(d, r)| [d, r])
                .collect()
        }),
        fastest: fit.filter(|f| f.wave_clear).map(|f| {
            let (deg, rate) = f.fastest();
            Named {
                at: deg,
                rate_s_per_day: rate,
                name: fastest_name(deg),
                figure: true_minus(&signed_text(rate, base_digits::RATE)),
            }
        }),
        slowest_deg: fit.map(|f| f.slowest().0),
        mean,
        axis: [lo, hi, step],
        unit: crate::position_line::unit_of(crate::steadiness::SeriesKind::Rate),
        key,
        heavy: fit.map(PoiseFit::heavy_spot_row),
        hours: hours(),
        title: TITLE,
        hour_axis: HOUR_AXIS,
        dial_key: DIAL_KEY,
        mean_title: MEAN,
    }
}

/// The upright stops round the face, 12H first.
pub fn hours() -> Vec<(f64, &'static str)> {
    let step = 360.0 / Position::UPRIGHT.len() as f64;
    (0..Position::UPRIGHT.len())
        .map(|i| {
            let deg = i as f64 * step;
            (deg, poise::hour_code(deg))
        })
        .collect()
}

/// A point's rate with its ± where it has one, and its unit.
fn dot_text(rate: f64, se: Option<f64>) -> String {
    format!("{} s/d", dot_figure(rate, se))
}

/// A point's rate bare, with its ± where it has one: "+2.0 ± 0.3".
fn dot_figure(rate: f64, se: Option<f64>) -> String {
    let r = true_minus(&signed_text(rate, base_digits::RATE));
    match se {
        Some(e) => format!(
            "{r} ± {}",
            crate::figure_text::fixed_text(e, base_digits::RATE)
        ),
        None => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_the_apps() {
        assert_eq!(points_key(8), "Upright positions (8)");
        assert_eq!(fastest_name(90.0), "Fastest 3H");
        assert_eq!(mean_name(-1.24), "Mean \u{2212}1.2");
        assert_eq!(dot_text(2.0, Some(0.3)), "+2.0 ± 0.3 s/d");
        let codes: Vec<&str> = hours().into_iter().map(|h| h.1).collect();
        assert_eq!(
            codes,
            ["12H", "1H30", "3H", "4H30", "6H", "7H30", "9H", "10H30"]
        );
    }

    #[test]
    fn the_scale_holds_each_point_and_its_spread() {
        let (lo, hi, _) = scale(&[(0.0, 5.0, Some(2.0)), (45.0, -4.0, None)], None);
        assert!(lo <= -4.0 && hi >= 7.0, "{lo} {hi}");
        assert_eq!(scale(&[], None), crate::chart_scale::round_ends(-1.0, 1.0));
    }

    #[test]
    fn nothing_measured_draws_an_empty_picture() {
        let p = picture(&[], None);
        assert!(p.points.is_empty() && p.wave.is_empty() && p.key.is_empty());
        assert!(p.mean.is_none() && p.fastest.is_none() && p.heavy.is_none());
    }
}
