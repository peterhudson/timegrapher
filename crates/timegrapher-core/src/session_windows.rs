//! Which beats and amplitude windows a reading takes, and where a reading
//! too short for its own ± borrows its noise from (split from
//! `session.rs`, which re-exports all of it).

use super::{AmplitudeWindow, Beat, Cuts, WindowFit, RATE_SE_MIN_S};

/// The beats a reading of the stretch from `from_s` to `to_s` (audio
/// seconds) takes, from a time-ordered `beats`: those at `from_s` or after
/// and before `to_s`. Live and saved readings both take them so
/// ([`crate::live::LiveAnalyzer::reading_at`], [`super::measure_masked`]).
pub fn stretch_beats(beats: &[Beat], from_s: f64, to_s: f64) -> std::ops::Range<usize> {
    let lo = beats.partition_point(|b| b.time < from_s);
    let hi = beats.partition_point(|b| b.time < to_s);
    lo.min(hi)..hi
}

/// The amplitude windows a reading of the stretch from `from_s` to `to_s`
/// (audio seconds) takes: those `holes` keeps, clear of every hole on
/// true time, whose midpoint lies in the stretch, from `from_s` up to but
/// not at `to_s`, so each window belongs to exactly one of two stretches
/// that meet (#632). Live and saved readings both take them so, so one
/// stretch reads one way.
pub fn stretch_windows<'w>(
    holes: &Cuts,
    windows: &'w [AmplitudeWindow],
    from_s: f64,
    to_s: f64,
) -> Vec<&'w AmplitudeWindow> {
    holes
        .amplitude_windows(windows)
        .into_iter()
        .filter(|w| window_in(w, from_s, to_s))
        .collect()
}

/// How far either side of a reading its ± looks for the values it
/// borrows its noise from when it holds too few (#1470), s: an hour. Noise
/// from further away describes a different state of the watch
/// (math-standards section 7); it also keeps a figure whose windows
/// rarely give a value (an unlock seldom found) from searching a long
/// take end to end at every reading.
pub const BORROW_LOOK_S: f64 = 3600.0;

/// How far a saved reading's borrowing of the rate's windows first
/// reaches, s: the 300 s its 30 values need ([`RATE_SE_MIN_S`]), since a
/// reading short enough to borrow holds few of them itself. Live starts
/// shorter (`live::RATE_FIRST_REACH_S`), its run counting with the pool.
pub const RATE_FIRST_REACH_S: f64 = RATE_SE_MIN_S;

/// How far borrowing of the amplitude's windows first reaches, saved or
/// live, s: 600 s, ten times the minute their 30 values of 2 s need, so a
/// take whose unlock is often missed is still found in one reach.
pub const AMPLITUDE_FIRST_REACH_S: f64 = crate::confidence::MIN_NOISE_VALUES as f64
    * crate::distribution::STRETCH_S
    * AMPLITUDE_REACH_MARGIN;
/// [`AMPLITUDE_FIRST_REACH_S`] over the time its values need.
const AMPLITUDE_REACH_MARGIN: f64 = 10.0;

/// The windows `fetch(a, b)` gives for the take from `a` to `b` (s), over
/// the least reach either side of a reading from `from_s` to `to_s` that
/// gives `enough`: back first, as far as the take's start, then on past
/// the reading towards `end_s`, the take's end (#1470). Each reach starts
/// at `first_s` and doubles, up to [`BORROW_LOOK_S`].
pub fn nearest_windows<T>(
    (from_s, to_s, end_s): (f64, f64, f64),
    first_s: f64,
    fetch: impl Fn(f64, f64) -> Vec<T>,
    enough: impl Fn(&[T]) -> bool,
) -> Vec<T> {
    let reach = |at: &dyn Fn(f64) -> (f64, f64), done: &dyn Fn(f64) -> bool| {
        let mut look = first_s;
        loop {
            let (a, b) = at(look);
            let w = fetch(a, b);
            if enough(&w) || done(look) || look >= BORROW_LOOK_S {
                return (w, look);
            }
            look *= 2.0;
        }
    };
    let back = |look: f64| ((from_s - look).max(0.0), to_s);
    let (w, look) = reach(&back, &|look| from_s - look <= 0.0);
    if enough(&w) || to_s >= end_s {
        return w;
    }
    let a = back(look).0;
    reach(&|on| (a, (to_s + on).min(end_s)), &|on| to_s + on >= end_s).0
}

/// The parts of the take from `a` to `b` (s) a reading may borrow its
/// noise from: those inside the take's `read` stretches (#1624), in time
/// order, or the whole of it where none are given.
pub fn read_within(read: &[(f64, f64)], a: f64, b: f64) -> Vec<(f64, f64)> {
    if read.is_empty() {
        return vec![(a, b)];
    }
    read.iter()
        .map(|&(x, y)| (x.max(a), y.min(b)))
        .filter(|(x, y)| x < y)
        .collect()
}

/// Whether the 2 s window `w` belongs to the stretch from `from_s` to
/// `to_s` of a take whose test read the stops `stops`: wholly inside it,
/// since a window over a stop's edge holds the handling between stops
/// (Norm on #1624, masks cull, never fill); with no stops (a plain take),
/// by its midpoint ([`window_in`]).
pub fn stop_window_in(w: &AmplitudeWindow, stops: &[(f64, f64)], from_s: f64, to_s: f64) -> bool {
    if stops.is_empty() {
        window_in(w, from_s, to_s)
    } else {
        w.start_s >= from_s && w.end_s <= to_s
    }
}

/// Whether enough of a figure's values are among `windows` to borrow its
/// noise from: [`crate::confidence::MIN_NOISE_VALUES`] of each of the
/// amplitude and the unlock beat error.
pub fn noise_enough(windows: &[&AmplitudeWindow]) -> bool {
    let n = crate::confidence::MIN_NOISE_VALUES;
    windows.iter().filter(|w| w.mean().is_some()).count() >= n
        && windows
            .iter()
            .filter(|w| w.beat_error_unlock_ms.is_some())
            .count()
            >= n
}

/// Whether enough rate windows are among `windows` to borrow the rate's
/// noise from: every other one, as [`crate::confidence::rate_se`] takes
/// them, [`crate::confidence::MIN_NOISE_VALUES`] times.
pub fn rate_noise_enough(windows: &[WindowFit]) -> bool {
    crate::confidence::rate_value_count(windows.len()) >= crate::confidence::MIN_NOISE_VALUES
}

/// Whether the 2 s window `w` belongs to the stretch from `from_s` to
/// `to_s` (seconds on the windows' own clock): its midpoint lies in the
/// stretch, from `from_s` up to but not at `to_s`, so each window belongs
/// to exactly one of two stretches that meet (#632). The one rule for
/// which windows a stretch takes.
pub fn window_in(w: &AmplitudeWindow, from_s: f64, to_s: f64) -> bool {
    window_span_in(w.start_s, w.end_s, from_s, to_s)
}

/// Midpoint membership for measured and nominal amplitude windows alike.
pub(crate) fn window_span_in(start: f64, end: f64, from_s: f64, to_s: f64) -> bool {
    let mid = (start + end) / 2.0;
    mid >= from_s && mid < to_s
}
