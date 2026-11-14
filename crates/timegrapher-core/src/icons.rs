//! The icons the app and the MCP view both draw (the three series' hare,
//! swing and seesaw, and the "i" of an info panel), as SVG on a 24 by 24
//! grid with `currentColor` strokes [`WIDTH`] wide: one drawing for both,
//! so the two never differ (Hugh on #1747).

use crate::steadiness::SeriesKind;

/// A leaping hare: gaining or losing, how fast the watch runs.
pub const RATE: &str = r#"<path d="M4.2 14.2c.4-3.4 3.8-5.6 7.8-5.2 2.4.2 4.2 1.3 5 2.8"/><path d="M16.8 13.4c-.9 1.6-2.6 2.3-4.6 2.3"/><path d="M4.2 14.2c.2 1.8 1.8 3 3.8 3 1.6 0 3-.6 3.8-1.6"/><path d="M7.6 13.6c1.4.4 2.4 1.6 2.4 3.2l-.6 2.2H4.4"/><circle cx="18.4" cy="11.4" r="2.4"/><circle cx="19.2" cy="11" r=".6" fill="currentColor" stroke="none"/><path d="M17.2 9.4c-1.8-2.2-2.6-4.8-2.2-7.2 1.6 1.4 2.8 4 3 6.8"/><path d="M19.2 9.2c.4-2.6 1.4-4.8 3-6 .2 2.2-.4 4.6-1.8 6.6"/><path d="M15.6 15l2.6 2.6M14 15.6l1.8 3"/><circle cx="3.6" cy="12.4" r="1.3" fill="currentColor" stroke="none"/>"#;
/// A swing: how far the balance swings, like a child on a swing.
pub const AMPLITUDE: &str = r#"<path d="M3 3.5h18"/><path d="M10 3.5L6 15M14 3.5L10 16.2"/><path d="M4.6 15.4l6.6 1.4"/><path d="M4.5 19.5a11 11 0 0 0 15 0" stroke-dasharray="1.5 2.4"/>"#;
/// Two children on a seesaw: tick and tock out of balance, one up, one down.
pub const BEAT_ERROR: &str = r#"<path d="M3 13.4l18 5.2"/><path d="M12 16l-2.4 4.6h4.8z"/><circle cx="5" cy="8.8" r="1.6"/><path d="M5 10.4v3.4M5 11.6l1.8.6M5 13.8l1.6 2.6"/><circle cx="19" cy="12.4" r="1.6"/><path d="M19 14v3.8M19 15.2l-1.8.4M19 17.8l-1.4 2.8"/>"#;

/// The standard "i" in a circle: info, the words in full.
pub const INFO: &str = r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6"/><circle cx="12" cy="7.5" r="1.1" fill="currentColor"/>"#;

/// The line width the drawings are designed with, in grid units.
pub const WIDTH: f32 = 1.7;

/// The icon of a series.
pub fn of(kind: SeriesKind) -> &'static str {
    match kind {
        SeriesKind::Rate => RATE,
        SeriesKind::Amplitude => AMPLITUDE,
        SeriesKind::BeatError => BEAT_ERROR,
    }
}
