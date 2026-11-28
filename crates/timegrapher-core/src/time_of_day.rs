//! The time of day along a take: where each of its seconds falls on this
//! computer's clock, from when its recording began and the breaks in its
//! sound. With the Paper Strip's Time of Day on, each of its time labels
//! has the time of day under it.

use crate::breaks::{self, Break};
use chrono::{DateTime, Local, TimeZone};

/// A take's seconds on the wall clock.
#[derive(Debug, Clone, PartialEq)]
pub struct WallClock {
    /// Unix seconds at the take's second 0: when its recording began.
    pub start_unix: f64,
    /// The breaks in the take's sound, each moving the clock on by its
    /// gap ([`breaks::wall`]).
    pub breaks: Vec<Break>,
}

/// From this step between labelled lines, seconds, a label names the day
/// too: lines six hours or more apart cross midnight.
const DAY_LABEL_STEP_S: f64 = 21_600.0;

impl WallClock {
    /// A take begun at `started_utc` (as `2026-10-20T21:32:00Z`) with
    /// `breaks` (the take's seconds before each, its length in true
    /// seconds); `None` when the start can't be read.
    pub fn of_take(started_utc: &str, breaks: &[Break]) -> Option<Self> {
        let start_unix = crate::full_wind::unix_of_utc(started_utc)?;
        Some(Self::new(start_unix, breaks))
    }

    /// A take begun at Unix second `start_unix`, with `breaks`.
    pub fn new(start_unix: f64, breaks: &[Break]) -> Self {
        WallClock {
            start_unix,
            breaks: breaks.to_vec(),
        }
    }

    /// Unix seconds at the take's second `t`: its start, and `t` on true
    /// time ([`breaks::wall`]: every break before `t` added).
    pub fn unix_at(&self, t: f64) -> f64 {
        self.start_unix + breaks::wall(&self.breaks, t)
    }

    /// The time of day at the take's second `t`, for a line on a time
    /// axis whose lines are `step` seconds apart and drawn after its
    /// second `from`, on a take `end` seconds long so far: "14:35:20", or
    /// "Thu 06:00" six hours or more apart. Closer, the first line of each
    /// new day names it too ("Fri 00:04:27"), and so does the take's first
    /// line drawn, the axis reaching its start, when the take falls on
    /// more than one day ([`begins_day_in`], Hugh on #2174).
    pub fn label(&self, t: f64, step: f64, (from, end): (f64, f64)) -> String {
        label_on(self, t, step, (from, end), &Local)
    }

    /// The time of day at the take's second `t`, to the hundredth of a
    /// second, for the cursor: "14:35:20.45"; with the day, "Fri
    /// 00:04:27.31", when the take, `end` seconds long so far, falls on
    /// more than one day (Hugh on #2174).
    pub fn label_fine(&self, t: f64, end: f64) -> String {
        fine_on(self, t, end, &Local)
    }

    /// On a take `end` seconds long so far that falls on more than one
    /// day, a label naming each of its days as a line under six hours
    /// apart would ("Fri 14:32:18", "Sat 14:32:18"): a week's at most, and
    /// the take's last second; none on a take inside one day. So a strip can make
    /// room for the widest wherever it looks (Hugh on #2174).
    pub fn day_labels(&self, end: f64) -> Vec<String> {
        day_labels_in(self, end, &Local)
    }
}

/// [`WallClock::day_labels`] in `tz`.
fn day_labels_in<Tz: TimeZone>(w: &WallClock, end: f64, tz: &Tz) -> Vec<String>
where
    Tz::Offset: std::fmt::Display,
{
    if !spans_in(w, end, tz) {
        return Vec::new();
    }
    let (first, last) = (w.unix_at(0.0), w.unix_at(end.max(0.0)));
    // The take's first second, the midnight of each day after it (a
    // week's at most, as a week names every weekday), and its last.
    let days = local_in(first, tz).map(|d| d.date_naive()).into_iter();
    let days = days.flat_map(|d| d.iter_days().skip(1).take(6));
    let mut labels: Vec<String> = std::iter::once(local_in(first, tz))
        .chain(days.map(|d| {
            let midnight = d.and_hms_opt(0, 0, 0)?;
            tz.from_local_datetime(&midnight).earliest()
        }))
        .flatten()
        .filter(|d| d.timestamp() as f64 <= last)
        .chain(local_in(last, tz))
        .map(|d| d.format(WITH_DAY).to_string())
        .collect();
    labels.dedup();
    labels
}

/// The times of the labelled lines on a time axis, `step` apart, from
/// the newest at or before `end` back to the oldest drawn
/// ([`line_drawn`]).
pub fn time_lines(end: f64, oldest: f64, step: f64) -> Vec<f64> {
    let mut t = (end / step).floor() * step;
    let mut lines = Vec::new();
    while line_drawn(t, oldest) {
        lines.push(t);
        t -= step;
    }
    lines
}

/// Whether a time axis whose oldest end is at the take's second
/// `oldest` draws the line at second `t`: after that end, and none
/// before the session started.
fn line_drawn(t: f64, oldest: f64) -> bool {
    t > oldest && t >= -1e-9
}

/// `unix` in `tz`, rounded to the second.
fn local_in<Tz: TimeZone>(unix: f64, tz: &Tz) -> Option<DateTime<Tz>> {
    let utc = DateTime::from_timestamp(unix.round() as i64, 0)?;
    Some(utc.with_timezone(tz))
}

/// The day in `tz` that Unix second `unix` is labelled on.
fn day_in<Tz: TimeZone>(unix: f64, tz: &Tz) -> Option<chrono::NaiveDate> {
    local_in(unix, tz).map(|d| d.date_naive())
}

/// Whether the take's seconds 0 to `end` fall on more than one day in
/// `tz`.
fn spans_in<Tz: TimeZone>(w: &WallClock, end: f64, tz: &Tz) -> bool {
    day_in(w.unix_at(0.0), tz) != day_in(w.unix_at(end.max(0.0)), tz)
}

/// Whether the line at the take's second `t`, on lines `step` seconds
/// apart drawn after second `from`, is the first of a new day in `tz`:
/// the line before it fell on another day; or, the take's first line
/// drawn with the axis reaching its start, when the take, `end` seconds
/// long so far, falls on more than one day.
fn begins_day_in<Tz: TimeZone>(
    w: &WallClock,
    t: f64,
    step: f64,
    (from, end): (f64, f64),
    tz: &Tz,
) -> bool {
    // The take's first line drawn: the line a step before isn't drawn
    // ([`line_drawn`], the one rule `time_lines` keeps), and that line is
    // second 0 or before, as on a strip fitted to the take, where the
    // line at 0 isn't drawn (Walter and Trevor on #2556). Lines fall on
    // whole steps from second 0, so half a step tells line 0 from line 1.
    let before = t - step;
    if !line_drawn(before, from) && before < step / 2.0 {
        return spans_in(w, end, tz);
    }
    day_in(w.unix_at(t), tz) != day_in(w.unix_at(t - step), tz)
}

/// [`WallClock::label`] in `tz`.
fn label_on<Tz: TimeZone>(w: &WallClock, t: f64, step: f64, span: (f64, f64), tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let unix = w.unix_at(t);
    if step >= DAY_LABEL_STEP_S || !begins_day_in(w, t, step, span, tz) {
        return label_in(unix, step, tz);
    }
    local_in(unix, tz).map_or_else(String::new, |d| d.format(WITH_DAY).to_string())
}

/// A label or cursor time of day that names its day.
const WITH_DAY: &str = "%a %H:%M:%S";

/// The plain label for Unix second `unix` in `tz`, on lines `step`
/// seconds apart, naming no new day.
fn label_in<Tz: TimeZone>(unix: f64, step: f64, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(d) = local_in(unix, tz) else {
        return String::new();
    };
    let shown = if step >= DAY_LABEL_STEP_S {
        "%a %H:%M"
    } else {
        "%H:%M:%S"
    };
    d.format(shown).to_string()
}

/// [`WallClock::label_fine`] in `tz`.
fn fine_on<Tz: TimeZone>(w: &WallClock, t: f64, end: f64, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let unix = w.unix_at(t);
    if spans_in(w, end, tz) {
        fine_as(unix, tz, WITH_DAY)
    } else {
        fine_in(unix, tz)
    }
}

/// The cursor's time of day for Unix second `unix` in `tz`, without the
/// day.
fn fine_in<Tz: TimeZone>(unix: f64, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    fine_as(unix, tz, "%H:%M:%S")
}

/// Unix second `unix` in `tz` to the hundredth, its whole seconds
/// written as `shown`.
fn fine_as<Tz: TimeZone>(unix: f64, tz: &Tz, shown: &str) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let cs = (unix * 100.0).round();
    let whole = (cs / 100.0).floor();
    let frac = (cs - whole * 100.0) as u32;
    match local_in(whole, tz) {
        Some(d) => format!("{}.{frac:02}", d.format(shown)),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    /// 2026-11-20 14:32:17.5 UTC, a Friday.
    const START: f64 = 1_795_185_137.5;

    #[test]
    fn a_take_reads_its_start() {
        let w = WallClock::of_take("2026-11-20T14:32:17.5Z", &[]).unwrap();
        assert!((w.start_unix - START).abs() < 1e-6);
        assert_eq!(label_in(w.unix_at(0.0), 1.0, &utc()), "14:32:18");
        assert_eq!(label_in(w.unix_at(42.5), 60.0, &utc()), "14:33:00");
        assert!(WallClock::of_take("not a time", &[]).is_none());
    }

    /// A break moves every second after it on by its length; one with a
    /// negative gap moves nothing.
    #[test]
    fn breaks_move_the_clock_on() {
        let at = |audio_s, gap_s| Break { audio_s, gap_s };
        let w = WallClock::new(1000.0, &[at(10.0, 5.0), at(30.0, -3.0), at(60.0, 30.0)]);
        assert_eq!(w.unix_at(5.0), 1005.0);
        assert_eq!(w.unix_at(20.0), 1025.0);
        assert_eq!(w.unix_at(70.0), 1105.0);
    }

    #[test]
    fn labels_suit_the_step() {
        let tz = utc();
        assert_eq!(label_in(START, 10.0, &tz), "14:32:18");
        assert_eq!(label_in(START, 300.0, &tz), "14:32:18");
        assert_eq!(label_in(START, 21_600.0, &tz), "Fri 14:32");
        let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap();
        assert_eq!(label_in(START, 60.0, &ist), "20:02:18");
        assert_eq!(fine_in(START + 0.004, &tz), "14:32:17.50");
        assert_eq!(fine_in(START + 0.497, &tz), "14:32:18.00");
    }

    /// 2026-11-19 23:34:27 UTC, a Thursday: 25 min 33 s before Friday.
    const LATE_THU: f64 = 1_795_131_267.0;

    /// The labels of lines `step` apart over a take `end` seconds long, on
    /// an axis reaching back before its start, so the line at 0 is drawn.
    fn labels(w: &WallClock, step: f64, end: f64, tz: &FixedOffset) -> Vec<String> {
        let n = (end / step).floor() as usize;
        (0..=n)
            .map(|k| label_on(w, k as f64 * step, step, (-step / 2.0, end), tz))
            .collect()
    }

    /// Crossing midnight at a 10 min step, the first label after it names
    /// the new day, and so does the take's first, the take falling on two
    /// days; the rest are plain times (Hugh on #2174).
    #[test]
    fn the_first_label_of_a_new_day_names_it() {
        let w = WallClock::new(LATE_THU, &[]);
        assert_eq!(
            labels(&w, 600.0, 3600.0, &utc()),
            [
                "Thu 23:34:27",
                "23:44:27",
                "23:54:27",
                "Fri 00:04:27",
                "00:14:27",
                "00:24:27",
                "00:34:27",
            ]
        );
        // Walter's walk on #2170 at UTC+3: the same, three hours on.
        let east = FixedOffset::east_opt(3 * 3600).unwrap();
        let w = WallClock::new(LATE_THU - 3.0 * 3600.0, &[]);
        assert_eq!(labels(&w, 600.0, 3600.0, &east)[3], "Fri 00:04:27");
        // Six hours apart and more, every label names its day, as before.
        assert_eq!(
            label_on(&w, 0.0, 21_600.0, (-1.0, 3600.0), &east),
            "Thu 23:34"
        );
    }

    /// A take inside one day names no day on any label at any step under
    /// six hours, its first included.
    #[test]
    fn a_take_inside_one_day_names_no_day() {
        let w = WallClock::new(START, &[]);
        let end = 8.0 * 3600.0;
        for step in [1.0, 10.0, 60.0, 600.0, 3600.0, 7200.0] {
            for label in labels(&w, step, end, &utc()) {
                assert!(
                    label.starts_with(|c: char| c.is_ascii_digit()),
                    "{label} at {step} s"
                );
            }
        }
        assert_eq!(fine_on(&w, 0.0, end, &utc()), "14:32:17.50");
    }

    /// On a two-day take at a 1 h step, each midnight's first label names
    /// its day, whichever lines are on show; the cursor names the day at
    /// every second.
    #[test]
    fn a_two_day_take_names_each_new_day() {
        let w = WallClock::new(START, &[]);
        let end = 2.0 * 86_400.0;
        let named: Vec<String> = labels(&w, 3600.0, end, &utc())
            .into_iter()
            .filter(|l| !l.starts_with(|c: char| c.is_ascii_digit()))
            .collect();
        assert_eq!(named, ["Fri 14:32:18", "Sat 00:32:18", "Sun 00:32:18"]);
        assert_eq!(fine_on(&w, 0.0, end, &utc()), "Fri 14:32:17.50");
        assert_eq!(fine_on(&w, 36_000.0, end, &utc()), "Sat 00:32:17.50");
    }

    /// A break that carries the clock over midnight starts the new day at
    /// the first line after it.
    #[test]
    fn a_break_over_midnight_starts_the_new_day() {
        // Ten minutes of sound, then a two-hour gap from 23:44:27 Thursday:
        // the line on the break is the first after it.
        let w = WallClock::new(
            LATE_THU,
            &[Break {
                audio_s: 600.0,
                gap_s: 7200.0,
            }],
        );
        assert_eq!(
            labels(&w, 300.0, 1200.0, &utc()),
            [
                "Thu 23:34:27",
                "23:39:27",
                "Fri 01:44:27",
                "01:49:27",
                "01:54:27"
            ]
        );
    }

    /// A take past midnight lists a day-naming label for each of its days,
    /// its last second included, a week's at most; one inside a day, none.
    #[test]
    fn a_take_of_days_lists_a_label_for_each_day() {
        let w = WallClock::new(LATE_THU, &[]);
        assert_eq!(
            day_labels_in(&w, 3600.0, &utc()),
            ["Thu 23:34:27", "Fri 00:00:00", "Fri 00:34:27"]
        );
        assert_eq!(
            day_labels_in(&w, 2.0 * 86_400.0, &utc()),
            [
                "Thu 23:34:27",
                "Fri 00:00:00",
                "Sat 00:00:00",
                "Sat 23:34:27"
            ]
        );
        assert_eq!(day_labels_in(&w, 30.0 * 86_400.0, &utc()).len(), 8);
        assert!(day_labels_in(&WallClock::new(START, &[]), 3600.0, &utc()).is_empty());
    }

    /// On an axis that starts exactly at the take's second 0 (the take
    /// fitted, or dragged to its start), the line at 0 isn't drawn, so the
    /// first line drawn names the day on a take past midnight; further
    /// on, with the start out of view, the first line on show doesn't
    /// (Walter on #2556).
    #[test]
    fn the_first_line_drawn_at_the_takes_start_names_its_day() {
        let w = WallClock::new(LATE_THU, &[]);
        let at = |t: f64, from: f64| label_on(&w, t, 600.0, (from, 3600.0), &utc());
        assert_eq!(at(600.0, 0.0), "Thu 23:44:27");
        assert_eq!(at(1200.0, 0.0), "23:54:27");
        assert_eq!(at(0.0, -300.0), "Thu 23:34:27");
        assert_eq!(at(600.0, -300.0), "23:44:27");
        assert_eq!(at(0.0, -900.0), "Thu 23:34:27");
        assert_eq!(at(1200.0, 700.0), "23:54:27");
        // Inside one day, the first line drawn stays plain.
        let w = WallClock::new(START, &[]);
        assert_eq!(
            label_on(&w, 600.0, 600.0, (0.0, 3600.0), &utc()),
            "14:42:18"
        );
    }

    /// With the strip's oldest end a hair below 0, the line at 0 is drawn
    /// and names the day, and the line after it doesn't: one day name, as
    /// at exactly 0, where only the line a step in names it (Trevor on
    /// #2556).
    #[test]
    fn one_line_names_the_day_wherever_the_axis_starts_near_zero() {
        let w = WallClock::new(LATE_THU, &[]);
        for from in [0.0, -1e-10, -1e-6, -300.0, 1e-10, 300.0] {
            let named: Vec<String> = time_lines(1800.0, from, 600.0)
                .into_iter()
                .map(|t| label_on(&w, t, 600.0, (from, 3600.0), &utc()))
                .filter(|l| l.starts_with("Thu"))
                .collect();
            assert_eq!(named.len(), 1, "from {from}: {named:?}");
        }
    }
}
