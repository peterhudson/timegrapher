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

/// Where a take's clock comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockFrom {
    /// Its clock log, fitted for rates.
    Log,
    /// Its USB lines set the rate; its clock log gives the breaks.
    LogRatedByUsb,
    /// A correction known for the input (stored, or given), as one steady
    /// error over the take.
    Correction,
    /// Its USB lines, as one steady error over the take.
    Usb,
}

/// Where the clock comes from for a take with a clock log (`log`), USB
/// lines (`usb_ppm`) and a known correction (`correction_ppm`), as
/// [`clock`] picks it; `None` when the readings stay on the sound card's
/// clock. USB lines set the rate whenever there is a log; without one, a
/// known correction comes before them.
pub fn clock_from(
    log: bool,
    usb_ppm: Option<f64>,
    correction_ppm: Option<f64>,
) -> Option<ClockFrom> {
    match (log, usb_ppm, correction_ppm) {
        (true, Some(_), _) => Some(ClockFrom::LogRatedByUsb),
        (true, None, _) => Some(ClockFrom::Log),
        (false, _, Some(_)) => Some(ClockFrom::Correction),
        (false, Some(_), None) => Some(ClockFrom::Usb),
        (false, None, None) => None,
    }
}

/// The clock a take's readings are put on, for a take `duration_s` long,
/// from where [`clock_from`] says.
///
/// - A clock log (`log_text`, read at `sample_rate` and
///   `bytes_per_frame`) is fitted for rates ([`ClockFit::for_rates`]); where
///   the recording has USB lines (`usb_ppm`, slow positive), they set the
///   rate and the log keeps only its breaks ([`ClockFit::rate_from_usb`]).
/// - A correction or the USB lines alone are one steady error over the
///   take ([`ClockFit::fixed`]).
pub fn clock(
    log_text: Option<&str>,
    sample_rate: u32,
    bytes_per_frame: u32,
    usb_ppm: Option<f64>,
    correction_ppm: Option<f64>,
    duration_s: f64,
) -> Result<Option<ClockFit>, String> {
    let fitted = |text: &str| -> Result<ClockFit, String> {
        let pairs = clock::parse_log(text, sample_rate, bytes_per_frame)?;
        ClockFit::for_rates(&pairs).map_err(|e| e.to_string())
    };
    Ok(
        match (
            clock_from(log_text.is_some(), usb_ppm, correction_ppm),
            log_text,
        ) {
            (Some(ClockFrom::Log), Some(text)) => Some(fitted(text)?),
            (Some(ClockFrom::LogRatedByUsb), Some(text)) => {
                let mut fit = fitted(text)?;
                fit.rate_from_usb(usb_ppm.unwrap_or_default());
                Some(fit)
            }
            (Some(ClockFrom::Correction), _) => {
                correction_ppm.map(|p| ClockFit::fixed(p, duration_s))
            }
            (Some(ClockFrom::Usb), _) => usb_ppm.map(|p| ClockFit::fixed(p, duration_s)),
            _ => None,
        },
    )
}

/// The clock of a take in the folder `dir` whose sound files are `files`,
/// in order, for a reader with no beats to hand (a library listing a
/// take's figures): [`clock`] from the take's clock log (`log`, a file in
/// `dir`), USB lines (`usb_ppm`, as [`clock::usb_lines`] reads them) and
/// known correction, with the breaks the files list placed on it
/// ([`breaks::of_files_on_clock`]). A reader with the beats passes the
/// clock to [`breaks`] instead.
pub fn clock_of_files(
    dir: &Path,
    log: Option<&str>,
    files: &[std::path::PathBuf],
    usb_ppm: Option<f64>,
    correction_ppm: Option<f64>,
) -> Result<Option<ClockFit>, String> {
    let first = files.first().ok_or("no sound files")?;
    let named = |f: &Path, e: &dyn std::fmt::Display| format!("{}: {e}", f.display());
    let info = crate::audio::info(first).map_err(|e| named(first, &e))?;
    let text = match log {
        Some(log) => {
            let file = dir.join(log);
            Some(std::fs::read_to_string(&file).map_err(|e| named(&file, &e))?)
        }
        None => None,
    };
    let mut frames = 0u64;
    for f in files {
        frames += crate::audio::info(f)
            .map_err(|e| named(f, &e))?
            .frames
            .unwrap_or(0);
    }
    let duration_s = frames as f64 / info.sample_rate as f64;
    let mut fit = clock(
        text.as_deref(),
        info.sample_rate,
        info.bytes_per_frame,
        usb_ppm,
        correction_ppm,
        duration_s,
    )
    .map_err(|e| match log {
        Some(log) => named(&dir.join(log), &e),
        None => e,
    })?;
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    breaks::of_files_on_clock(&paths, fit.as_mut(), info.sample_rate);
    Ok(fit)
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
/// sides are kept by the count alone ([`breaks::renumber`]). In time order.
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
    let all = breaks::with_jumps(listed, &log.jumps);
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
    fn the_clock_comes_from_the_log_and_usb_lines_else_a_correction_else_usb() {
        use ClockFrom::*;
        assert_eq!(clock_from(true, Some(1.0), Some(2.0)), Some(LogRatedByUsb));
        assert_eq!(clock_from(true, None, Some(2.0)), Some(Log));
        assert_eq!(clock_from(false, Some(1.0), Some(2.0)), Some(Correction));
        assert_eq!(clock_from(false, Some(1.0), None), Some(Usb));
        assert_eq!(clock_from(false, None, None), None);
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
