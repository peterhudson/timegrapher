//! A take opened once, the same way for every surface: the clock its
//! readings are put on, its breaks, and the stretch of each recorded
//! position that is read. The app, the command line and the MCP server
//! call these rather than putting the take together from the pieces in
//! [`crate::clock`], [`crate::breaks`] and [`crate::session`], so a take
//! reads the same wherever it is opened.

use crate::breaks::{self, Break};
use crate::clock::{self, ClockFit};
use crate::session::{Manifest, RecordingEntry};
use crate::stream::BeatLog;
use std::path::Path;

/// The clock a take's readings are put on, for a take `duration_s` long.
///
/// - With a clock log (`log_text`, read at `sample_rate` and
///   `bytes_per_frame`), the log fitted for rates
///   ([`ClockFit::for_rates`]); where the recording has USB lines
///   (`usb_ppm`, slow positive), they set the rate and the log keeps only
///   its breaks ([`ClockFit::rate_from_usb`]).
/// - Without one, a correction known for the input (`correction_ppm`:
///   stored, or given), else the USB lines, as one steady error over the
///   take ([`ClockFit::fixed`]).
/// - Otherwise none: the readings stay on the sound card's clock.
pub fn clock(
    log_text: Option<&str>,
    sample_rate: u32,
    bytes_per_frame: u32,
    usb_ppm: Option<f64>,
    correction_ppm: Option<f64>,
    duration_s: f64,
) -> Result<Option<ClockFit>, String> {
    let Some(text) = log_text else {
        return Ok(correction_ppm
            .or(usb_ppm)
            .map(|ppm| ClockFit::fixed(ppm, duration_s)));
    };
    let pairs = clock::parse_log(text, sample_rate, bytes_per_frame)?;
    let mut fit = ClockFit::for_rates(&pairs).map_err(|e| e.to_string())?;
    if let Some(ppm) = usb_ppm {
        fit.rate_from_usb(ppm);
    }
    Ok(Some(fit))
}

/// Where a take lists its breaks.
#[derive(Debug, Clone, Copy)]
pub enum Listed<'a> {
    /// The take's sound files, in order: the `session.json` beside them
    /// when they are that take's segments, else the clock log's steps
    /// ([`breaks::of_files`]).
    Files(&'a [&'a Path]),
    /// A `session.json` already read, for the whole of the take's sound
    /// (a take kept as its analysis alone): its list, else the clock log's
    /// steps ([`breaks::for_take`]).
    Session(Option<&'a serde_json::Value>),
}

/// A take's breaks, as every reading of it uses them, with `log`'s beats
/// renumbered across them.
///
/// The breaks the take lists, placed on its clock with every clock step
/// no listed break explains ([`breaks::on_clock`]); then each jump the
/// beats show that none of those explains, as a break of unknown length
/// ([`breaks::with_jumps`]). The beats after each break are numbered on
/// by the beats its gap held, and `log.by_count` names the breaks whose
/// sides are kept by the count alone ([`breaks::renumber`]). Sorted.
pub fn breaks(log: &mut BeatLog, listed: Listed, mut clock: Option<&mut ClockFit>) -> Vec<Break> {
    let listed = match listed {
        Listed::Files(files) => {
            breaks::of_files_on_clock(files, clock.as_deref_mut(), log.sample_rate)
        }
        Listed::Session(json) => {
            let listed = breaks::for_take(json, clock.as_deref(), log.sample_rate);
            breaks::on_clock(listed, clock)
        }
    };
    let mut all = breaks::with_jumps(listed, &log.jumps);
    all.sort_by(|a, b| a.audio_s.total_cmp(&b.audio_s));
    log.by_count = breaks::renumber(
        &mut log.beats,
        &mut log.amplitude_windows,
        &all,
        &log.sided,
        log.bph,
    );
    all
}

/// Seconds a position settles before it is read, when neither its entry
/// nor its session file says (Witschi's 20 s).
pub const SETTLE_S: f64 = 20.0;

/// The settling and measuring times a session `entry` asks for: its own,
/// else its session file's.
pub fn entry_times(entry: &RecordingEntry, manifest: &Manifest) -> (Option<f64>, Option<f64>) {
    (
        entry.settle_s.or(manifest.settle_s),
        entry.measure_s.or(manifest.measure_s),
    )
}

/// The stretch of one recorded position that is read, from the position's
/// `start_s` to `stop_s`: it starts once the watch has settled, `settle_s`
/// (default [`SETTLE_S`]) after the start but no later than halfway, and
/// runs `measure_s` from there, or to the stop.
pub fn measured(
    start_s: f64,
    stop_s: f64,
    settle_s: Option<f64>,
    measure_s: Option<f64>,
) -> (f64, f64) {
    let from = start_s + settle_s.unwrap_or(SETTLE_S).min((stop_s - start_s) / 2.0);
    let to = measure_s.map_or(stop_s, |m| from + m).min(stop_s);
    (from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock log every 30 s over an hour, the card `ppm` slow.
    fn log_text(ppm: f64) -> String {
        (0..=120)
            .map(|i| {
                let a = i as f64 * 30.0;
                format!("{a},{}\n", 1.79e9 + a * (1.0 + ppm * 1e-6))
            })
            .collect()
    }

    #[test]
    fn a_clock_log_is_fitted_for_rates_and_usb_lines_set_its_rate() {
        let text = log_text(20.0);
        let pairs = clock::parse_log(&text, 48_000, 2).unwrap();
        let by_hand = ClockFit::for_rates(&pairs).unwrap();
        let fit = clock(Some(&text), 48_000, 2, None, Some(5.0), 3600.0)
            .unwrap()
            .unwrap();
        assert_eq!(fit.ppm, by_hand.ppm);
        assert!(fit.corrects());
        // The USB lines set the rate; the log keeps its breaks.
        let mut by_hand = by_hand;
        by_hand.rate_from_usb(12.0);
        let fit = clock(Some(&text), 48_000, 2, Some(12.0), Some(5.0), 3600.0)
            .unwrap()
            .unwrap();
        assert_eq!(fit.ppm, by_hand.ppm);
        assert_eq!(fit.withheld.map(|w| w.reason), Some("usb_lines"));
        assert!(fit.corrects());
    }

    #[test]
    fn without_a_log_a_known_correction_comes_before_the_usb_lines() {
        let fixed = |ppm| ClockFit::fixed(ppm, 600.0).map(300.0);
        let at = |c: Option<ClockFit>| c.map(|c| c.map(300.0));
        assert_eq!(
            at(clock(None, 48_000, 2, Some(12.0), Some(5.0), 600.0).unwrap()),
            Some(fixed(5.0))
        );
        assert_eq!(
            at(clock(None, 48_000, 2, Some(12.0), None, 600.0).unwrap()),
            Some(fixed(12.0))
        );
        assert!(clock(None, 48_000, 2, None, None, 600.0).unwrap().is_none());
    }

    #[test]
    fn an_unreadable_log_is_an_error_not_a_missing_clock() {
        assert!(clock(Some("no numbers here\n"), 48_000, 2, None, None, 60.0).is_err());
    }

    #[test]
    fn a_position_is_read_after_its_settling_for_its_measuring_time() {
        assert_eq!(measured(100.0, 400.0, None, None), (120.0, 400.0));
        assert_eq!(
            measured(100.0, 400.0, Some(30.0), Some(60.0)),
            (130.0, 190.0)
        );
        // Settling never takes more than half the position.
        assert_eq!(measured(0.0, 30.0, Some(30.0), None), (15.0, 30.0));
        // Measuring stops at the position's end.
        assert_eq!(measured(0.0, 100.0, Some(0.0), Some(500.0)), (0.0, 100.0));
    }

    #[test]
    fn an_entrys_own_times_come_before_its_session_files() {
        let m: Manifest =
            toml::from_str("settle_s = 30.0\nmeasure_s = 90.0\n[[recording]]\nfile = \"a.wav\"\n")
                .unwrap();
        let mut e = m.recordings[0].clone();
        assert_eq!(entry_times(&e, &m), (Some(30.0), Some(90.0)));
        e.settle_s = Some(0.0);
        assert_eq!(entry_times(&e, &m), (Some(0.0), Some(90.0)));
    }
}
