//! A take opened once, the same way for every surface: the clock its
//! readings are put on, its breaks, and the stretch of each recorded
//! position that is read. The app, the command line and the MCP server
//! call these rather than putting the take together from the pieces in
//! [`crate::clock`], [`crate::breaks`] and [`crate::session`], so a take
//! reads the same wherever it is opened.

use crate::analysis_take;
use crate::breaks::{self, Break};
use crate::clean::Cuts;
use crate::clock::{self, ClockFit};
use crate::session::{self, Manifest, Measurement, RecordingEntry};
use crate::stream::{self, BeatLog, StreamConfig};
use std::fs;
use std::path::{Path, PathBuf};

/// The file a take folder describes itself in.
pub const MANIFEST: &str = "session.toml";

/// The file the recorder writes beside a take's sound: the input it was
/// made on, its breaks, and, for a take kept as its analysis alone, its
/// parts.
pub const RECORD: &str = "session.json";

/// The folder `timegrapher session` writes its report into, inside the
/// take it read: not part of the take.
pub const REPORT_DIR: &str = "session_report";

/// The file the recorder logs the sound card against the computer's clock
/// in.
pub const CLOCK_LOG: &str = "clock.csv";

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
/// clock. USB lines set the rate whenever the take has them, as they time
/// its card on this take; without them, a log comes before a known
/// correction (#635).
pub fn clock_from(
    log: bool,
    usb_ppm: Option<f64>,
    correction_ppm: Option<f64>,
) -> Option<ClockFrom> {
    match (log, usb_ppm, correction_ppm) {
        (true, Some(_), _) => Some(ClockFrom::LogRatedByUsb),
        (true, None, _) => Some(ClockFrom::Log),
        (false, Some(_), _) => Some(ClockFrom::Usb),
        (false, None, Some(_)) => Some(ClockFrom::Correction),
        (false, None, None) => None,
    }
}

/// The clock a take's readings are put on, for a take `duration_s` long,
/// from where [`clock_from`] says.
///
/// - A clock log (`log_text`, read at `sample_rate` and
///   `bytes_per_frame`) is fitted for rates ([`ClockFit::for_rates`]); where
///   the recording has USB lines (`usb_ppm`, fast positive), they set the
///   rate and the log keeps only its breaks ([`ClockFit::rate_from_usb`]).
/// - A correction or the USB lines alone are one steady error over the
///   take ([`ClockFit::fixed`]); USB lines over a correction keep the
///   correction beside them, to say when the two disagree
///   ([`ClockFit::usb_over_correction`]).
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
            (Some(ClockFrom::Usb), _) => usb_ppm.map(|p| match correction_ppm {
                Some(c) => ClockFit::usb_over_correction(p, c, duration_s),
                None => ClockFit::fixed(p, duration_s),
            }),
            _ => None,
        },
    )
}

/// The clock a live reading is put on (#1721): [`clock`] as opening the
/// take saved now puts it, with no clock log, the recording's USB lines as
/// `usb` has measured them (on the first sound, as a saved take reads
/// them) and the correction kept for the input (`correction_ppm`). Before
/// the lines are measured it is the kept correction alone, or none, and
/// [`crate::usbclock::LiveClock::not_yet`] says so beside the rate.
pub fn live_clock(
    usb: &crate::usbclock::LiveClock,
    correction_ppm: Option<f64>,
    duration_s: f64,
) -> Option<ClockFit> {
    clock(None, 0, 0, usb.usb_ppm(), correction_ppm, duration_s)
        .ok()
        .flatten()
}

/// Where a take's clock came from, in words, as `from` ([`clock_from`])
/// picks it: the USB lines' text (`usb`, as
/// [`crate::usbclock::UsbClock::correction`] words it), the correction's
/// (`correction`, such as [`crate::clockstore::DeviceClock::source_text`])
/// and the clock log's path (`log`), named by its file name alone, as a
/// session file names it: the folder above it, often a home folder, is
/// left out.
pub fn clock_source_text(
    from: ClockFrom,
    usb: Option<&str>,
    correction: Option<&str>,
    log: Option<&Path>,
) -> String {
    let log = || {
        log.and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    match from {
        ClockFrom::LogRatedByUsb => format!(
            "{}; clock log {} for its breaks",
            usb.unwrap_or_default(),
            log()
        ),
        ClockFrom::Log => format!("from clock log {}", log()),
        ClockFrom::Correction => correction.unwrap_or_default().to_string(),
        ClockFrom::Usb => usb.unwrap_or_default().to_string(),
    }
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
    /// A `session.json` already read ([`breaks::read_session`]), for the
    /// whole of the take's sound (a take kept as its analysis alone): its
    /// list, else the clock log's steps ([`breaks::for_take`]); one that
    /// couldn't be read leaves them to the clock log, and says so.
    Session(Result<Option<&'a serde_json::Value>, breaks::Unlisted>),
}

/// A take's breaks, as every reading of it uses them, with `log`'s beats
/// renumbered across them.
///
/// The breaks the take lists, placed on its clock with every clock step
/// no listed break explains ([`breaks::on_clock`]); then each jump the
/// beats show that none of those explains, as a break of unknown length
/// ([`breaks::with_jumps`]). The beats after each break are numbered on
/// by the beats its gap held, as true time keeps it ([`breaks::as_kept`]),
/// and `log.by_count` names the breaks whose
/// sides are kept by the count alone ([`breaks::renumber`]). In time order,
/// with why they are not all the take's own list's, when they are not
/// ([`breaks::Unlisted`]).
pub fn breaks(
    log: &mut BeatLog,
    listed: Listed,
    mut clock: Option<&mut ClockFit>,
) -> breaks::Found {
    let found = match listed {
        Listed::Files(files) => {
            breaks::of_files_on_clock(files, clock.as_deref_mut(), log.sample_rate)
        }
        Listed::Session(json) => {
            let found = match json {
                Ok(json) => breaks::for_take(json, clock.as_deref(), log.sample_rate),
                Err(why) => breaks::of_clock(clock.as_deref(), why),
            };
            breaks::Found {
                breaks: breaks::on_clock(found.breaks, clock.as_deref_mut()),
                ..found
            }
        }
    };
    let all = breaks::with_jumps(found.breaks, &log.jumps);
    // Numbered on by the gaps true time keeps (#646).
    log.by_count = breaks::renumber(
        &mut log.beats,
        &mut log.amplitude_windows,
        &breaks::as_kept(&all, clock.as_deref()),
        &log.sided,
        log.bph,
    );
    breaks::Found {
        breaks: all,
        unlisted: found.unlisted,
    }
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
/// `start_s` to `stop_s`: it starts once the watch has settled and runs
/// `measure_s` from there, or to the stop. A `settle_s` given is kept as
/// it is, up to the stop; with none, the watch settles for [`SETTLE_S`]
/// but no later than halfway, so a short position still has something to
/// read.
pub fn measured(
    start_s: f64,
    stop_s: f64,
    settle_s: Option<f64>,
    measure_s: Option<f64>,
) -> (f64, f64) {
    let settle = settle_s.unwrap_or(SETTLE_S.min((stop_s - start_s) / 2.0));
    let from = (start_s + settle).min(stop_s);
    let to = measure_s.map_or(stop_s, |m| from + m).min(stop_s);
    (from, to)
}

/// The stretch of a recording one session entry reads: the stretch it
/// names ([`span_of`]) and, within it, the stretch measured once the watch
/// has settled ([`measured`]). Seconds into the entry's sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub start_s: f64,
    pub stop_s: f64,
    pub from_s: f64,
    pub to_s: f64,
}

/// Why a session entry gives no reading.
#[derive(Debug, Clone, PartialEq)]
pub enum NoReading {
    /// It settles right to its end ([`reads_nothing`]).
    Nothing,
    /// Its `start_s` and `end_s` leave nothing of the recording
    /// ([`span_of`]'s text).
    Outside(String),
    /// The settling time it was given reaches the end of its stretch.
    Unsettled {
        file: String,
        settle_s: f64,
        length_s: f64,
    },
}

impl std::fmt::Display for NoReading {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoReading::Nothing => write!(f, "nothing to read: it settles right to its end"),
            NoReading::Outside(why) => write!(f, "{why}"),
            NoReading::Unsettled {
                file,
                settle_s,
                length_s,
            } => write!(
                f,
                "{file}: a settle_s of {settle_s} s leaves nothing of its {length_s:.0} s \
                 stretch to read; give it a shorter settle_s"
            ),
        }
    }
}

/// Why [`measure_entry`] gives no reading: the entry has nothing to read
/// in its recording ([`NoReading`]), which a session notes and reads on
/// past, or the take could not be read at all.
#[derive(Debug, Clone, PartialEq)]
pub enum EntryError {
    NoReading(NoReading),
    Failed(String),
}

impl std::fmt::Display for EntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryError::NoReading(w) => write!(f, "{w}"),
            EntryError::Failed(why) => write!(f, "{why}"),
        }
    }
}

impl From<String> for EntryError {
    fn from(why: String) -> Self {
        EntryError::Failed(why)
    }
}

impl From<EntryError> for String {
    fn from(e: EntryError) -> Self {
        e.to_string()
    }
}

/// Whether a session entry's `file` names the take's whole folder
/// (".", "./" or nothing) rather than one of its sound files: the one
/// spelling check for every reader.
pub fn names_whole_folder(file: &str) -> bool {
    matches!(file, "." | "./" | "")
}

/// Whether session entry `e` of the take `m` carries on the entry listed
/// just before it: the next file of one recording in the same position,
/// read from its start after the one before was read to its end. The
/// watch was not handled between them, so no settling begins there: one
/// rule for `session`, `series` and the app (#1145).
pub fn continues_position(e: &RecordingEntry, m: &Manifest) -> bool {
    let Some(i) = m
        .recordings
        .iter()
        .position(|r| r.file == e.file && r.start_s == e.start_s && r.end_s == e.end_s)
    else {
        return false;
    };
    let Some(before) = i.checked_sub(1).map(|j| &m.recordings[j]) else {
        return false;
    };
    let position = |r: &RecordingEntry| r.position.as_deref().and_then(session::Position::parse);
    // An entry for the take's whole folder is not a file the next one
    // carries on from: the first file of the folder still settles.
    before.file != e.file
        && !names_whole_folder(&e.file)
        && !names_whole_folder(&before.file)
        && position(e).is_some()
        && position(before) == position(e)
        && e.start_s.is_none()
        && before.end_s.is_none()
        // An entry that gives its own settling settles: the watch was
        // handled there (ym42_CR_long's third file, #1184).
        && e.settle_s.is_none_or(|s| s == 0.0)
}

/// The stretch entry `e` of the take `m` reads in a recording of
/// `duration_s`, with `given_settle_s` the settling time the reader gives
/// (after the entry's own and the carrying on of a position
/// ([`continues_position`]), before the session file's), or why it reads
/// nothing. Every surface reads an entry from this window.
pub fn reading_window(
    e: &RecordingEntry,
    m: &Manifest,
    given_settle_s: Option<f64>,
    duration_s: f64,
) -> Result<Window, NoReading> {
    if reads_nothing(e) {
        return Err(NoReading::Nothing);
    }
    let (start_s, stop_s) = span_of(e, duration_s).map_err(NoReading::Outside)?;
    let (settle, measure) = entry_times(e, m);
    // A file that carries on the one before in the same position was
    // never handled at its start, so it does not settle again (#1145),
    // whatever settling the reader gives; only its own entry's stands.
    let carried_on = continues_position(e, m).then_some(0.0);
    let settle = e.settle_s.or(carried_on).or(given_settle_s).or(settle);
    let (from_s, to_s) = measured(start_s, stop_s, settle, measure);
    if from_s >= stop_s {
        return Err(NoReading::Unsettled {
            file: e.file.clone(),
            settle_s: settle.unwrap_or(SETTLE_S),
            length_s: stop_s - start_s,
        });
    }
    Ok(Window {
        start_s,
        stop_s,
        from_s,
        to_s,
    })
}

/// The stretches a test take `m` reads in its recording `file` of
/// `duration_s`: each of its entries there from its settling's end
/// ([`reading_window`], `given_settle_s` as that takes it), in time
/// order: what [`session::measure_reading`] keeps a test's stops to
/// (#1624). It reads one stretch alone as a plain take.
pub fn read_stretches(
    m: &Manifest,
    file: &str,
    given_settle_s: Option<f64>,
    duration_s: f64,
) -> Vec<(f64, f64)> {
    let mut read: Vec<(f64, f64)> = m
        .recordings
        .iter()
        .filter(|e| e.file == file)
        .filter_map(|e| reading_window(e, m, given_settle_s, duration_s).ok())
        .map(|w| (w.from_s, w.to_s))
        .collect();
    read.sort_by(|a, b| a.0.total_cmp(&b.0));
    read
}

/// What is opened to read the recording at `path` (a recording or a
/// take's folder) whole, as `analyze --trend` reads it: the folder its
/// session file is read from, the session file (an empty one without)
/// and one entry for the whole recording. With a session file `m`
/// (holding only its entries for the recording), that entry is
/// [`whole_take`]'s: one clock log, every entry's masks.
pub fn whole_entry(
    path: &Path,
    manifest: Option<&Manifest>,
) -> Result<(PathBuf, Manifest, RecordingEntry), String> {
    let (base, file) = if path.is_dir() {
        (path.to_path_buf(), ".".to_string())
    } else {
        (
            path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    };
    Ok(match manifest {
        Some(m) if !m.recordings.is_empty() => (base, m.clone(), whole_take(m)?),
        _ => (
            base,
            Manifest::default(),
            RecordingEntry {
                file,
                ..Default::default()
            },
        ),
    })
}

/// Each of `entries` of the take `m` that names a position: the
/// stretch's start, the start of what is read once the watch has settled
/// and the end of that ([`reading_window`], an entry that reads nothing
/// settling to its end), the position, and whether it carries on the
/// entry before it ([`continues_position`]). `length_s` is the
/// recording's length, infinite where it is not known.
pub fn listed_positions(
    m: &Manifest,
    entries: &[RecordingEntry],
    length_s: f64,
) -> Vec<ListedPosition<session::Position>> {
    entries
        .iter()
        .filter_map(|e| listed_position(e, m, reading_window(e, m, None, length_s).ok(), 0.0))
        .collect()
}

/// A listed position: the stretch's start, the start of what is read,
/// the end of that, the position, and whether the entry carries on the
/// one before it ([`continues_position`]), so that it is no change.
pub type ListedPosition<P> = (f64, f64, Option<f64>, P, bool);

/// Entry `e`'s listed position, as [`listed_positions`] gives it, in take
/// seconds: `offset_s` is where its recording starts in the take and
/// `window` its reading window already moved there (none where it reads
/// nothing); the entry's own `start_s` and `end_s` are moved by
/// `offset_s`. None when it names no position.
pub fn listed_position(
    e: &RecordingEntry,
    m: &Manifest,
    window: Option<Window>,
    offset_s: f64,
) -> Option<ListedPosition<session::Position>> {
    let p = session::Position::parse(e.position.as_deref()?)?;
    let start = e.start_s.unwrap_or(0.0) + offset_s;
    let (from, to) = match window {
        Some(w) => (w.from_s, w.to_s),
        None => {
            let end = e.end_s.map_or(start, |t| t + offset_s);
            (end, end)
        }
    };
    Some((
        start,
        from,
        to.is_finite().then_some(to),
        p,
        continues_position(e, m),
    ))
}

/// A take's changes of position (where each began, and the position)
/// and the settling after each (start, end).
pub type Positions<P> = (Vec<(f64, P)>, Vec<(f64, f64)>);

/// The positions listed (start, start of what is read, end, position,
/// carried on), as changes oldest first and the settling before each:
/// from the end of the one before (or the take's start, for the first) to
/// where its reading starts. An entry that carries on the one before it
/// ([`continues_position`]: the next file of one recording, the watch not
/// handled) is no change, so the take is one stretch across it and
/// nothing is cut there (#1166).
pub fn positions_of<P: Copy>(listed: impl Iterator<Item = ListedPosition<P>>) -> Positions<P> {
    let mut listed: Vec<ListedPosition<P>> = listed.collect();
    listed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut settles = Vec::new();
    for (i, &(start, from, _, p, carried)) in listed.iter().enumerate() {
        if carried && i > 0 {
            continue;
        }
        // The position began where the one before it ended, the first at
        // the take's start: the app saves each entry from the end of its
        // settling, the first's too. Where the one before is read on past
        // this one's start (two entries of one file, each with its own
        // window, as ym42_CR_long's third file), what it reads stays its
        // own: this one begins where that reading ends, or where its own
        // starts if sooner, as `session` reads each window (#1156).
        let began = match i.checked_sub(1).map(|j| listed[j].2) {
            None => 0.0,
            Some(Some(end)) if end < start => end,
            Some(Some(end)) if end > start => end.min(from),
            Some(_) => start,
        };
        if from > began {
            settles.push((began, from));
        }
        out.push((began, p));
    }
    (out, settles)
}

/// Where no reading over a take may reach across: its breaks, each change
/// of position after the first (`changes`, oldest first) and the end of
/// each settling, in order.
pub fn reading_cuts(breaks: &[Break], changes: &[f64], settles: &[(f64, f64)]) -> Vec<f64> {
    let mut cuts: Vec<f64> = breaks.iter().map(|b| b.audio_s).collect();
    cuts.extend(changes.iter().skip(1).copied());
    cuts.extend(settles.iter().map(|s| s.1));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    cuts
}

/// Whether `p` is a sound file a take keeps: WAV or FLAC.
pub fn is_sound(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav") || e.eq_ignore_ascii_case("flac"))
}

/// Whether the folder `dir` holds a sound file.
pub fn has_sound(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|d| d.filter_map(|e| e.ok()).any(|e| is_sound(&e.path())))
}

/// The sound files `paths` name, in order: a file as it is, a folder as
/// its recordings in the order its `session.toml` lists them, else in
/// the order they were recorded ([`crate::take_path::recordings_in`]).
pub fn sound_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for path in paths {
        out.extend(crate::take_path::recordings_in(path)?);
    }
    Ok(out)
}

/// A kept take's beat log with its amplitudes at `lift_deg`, the lift this
/// reading uses, where the analysis was kept at another: each window's
/// sides are read again from their edges at the new lift and gated there
/// ([`crate::amplitude::at_lift`]), so a side the kept lift culled can
/// come back (#1333), then screened for lone wrong-sound sides once over
/// the log ([`crate::amplitude::reject_side_outliers`]), as a read at
/// that lift is.
pub fn kept_at_lift(log: &BeatLog, lift_deg: f64) -> BeatLog {
    let mut log = log.clone();
    if lift_deg > 0.0 && log.lift_deg > 0.0 && lift_deg != log.lift_deg && log.bph > 0 {
        let osc = 7200.0 / log.bph as f64;
        crate::amplitude::relift(&mut log.amplitude_windows, osc, log.lift_deg, lift_deg);
        log.lift_deg = lift_deg;
    }
    log
}

/// The stretch of a recording an entry reads, seconds from its start:
/// `start_s` to `end_s`, or the whole recording when they are left out.
pub fn span_of(e: &RecordingEntry, duration_s: f64) -> Result<(f64, f64), String> {
    let start = e.start_s.unwrap_or(0.0);
    let stop = e.end_s.unwrap_or(duration_s).min(duration_s);
    if start < 0.0 || start >= stop {
        return Err(format!(
            "{}: start_s {} and end_s {} leave nothing of a {:.0} s recording",
            e.file,
            start,
            e.end_s.map_or("(end)".to_string(), |t| t.to_string()),
            duration_s
        ));
    }
    Ok((start, stop))
}

/// Whether entry `e` has nothing to read: its stretch settles right to
/// its end, which the app saves as `start_s` equal to `end_s` (the watch
/// was put in a position and moved again, or painted over, before it
/// settled). Every surface leaves such an entry out of a take's readings
/// rather than failing on it as [`span_of`] would: `session` and
/// `regress` with a note, the Watches list by reading the last entry with
/// something to read. An entry whose settling runs past its `end_s` is not
/// one of these.
pub fn reads_nothing(e: &RecordingEntry) -> bool {
    matches!((e.start_s, e.end_s), (Some(a), Some(b)) if a == b)
}

/// The lift angle amplitudes are worked out at when neither the reader,
/// the session file nor the calibre gives one, degrees.
pub const DEFAULT_LIFT_DEG: f64 = 52.0;

/// Escape wheel teeth when neither the session file nor the calibre
/// gives them.
pub const ESCAPE_TEETH: u32 = 15;

/// A calibre found in the table or the user's own calibres, with what it
/// gives a reading: its beat rate, lift angle, escape wheel teeth and
/// wheels.
#[derive(Debug, Clone)]
pub struct Calibre {
    pub name: String,
    pub bph: Option<u32>,
    pub lift_angle_deg: Option<f64>,
    /// A lift angle it holds outside the range, set aside.
    pub lift_set_aside_deg: Option<f64>,
    pub escape_teeth: Option<u32>,
    pub wheels: Vec<crate::periodicity::Wheel>,
    /// Its meshes, for marking beside the wheels ([`Catalog::meshes`]).
    ///
    /// [`Catalog::meshes`]: crate::calibre_store::Catalog::meshes
    pub meshes: Vec<crate::train::MeshLine>,
}

/// Why a reading's lift angle is the default when no lift was given and
/// `calibre` (if any) gave none either, or `None` when it gave one.
pub fn lift_default_why(calibre: Option<&Calibre>) -> Option<String> {
    let range = &crate::input_domain::LIFT_RANGE_DEG;
    match calibre {
        None => Some("calibre not set".to_string()),
        Some(c) if c.lift_angle_deg.is_some() => None,
        Some(Calibre {
            lift_set_aside_deg: Some(l),
            ..
        }) => Some(format!(
            "the calibre's lift angle of {l}° is set aside, outside {} to {}°",
            range.start(),
            range.end()
        )),
        Some(_) => Some("the calibre's lift angle is not in the table".to_string()),
    }
}

/// The calibre `name` names in the table or `user`'s calibres, or `None`
/// when neither has it.
pub fn calibre_named(name: &str, user: &crate::calibre_store::UserCalibres) -> Option<Calibre> {
    use crate::calibre_store::{Catalog, Entry};
    let cat = Catalog::new(user);
    let entry = cat.find(name)?;
    let mut wheels = cat.wheels(entry);
    if let Entry::Table(c) = entry {
        // Preserve shipped first-match naming order; Catalogue supplies
        // the effective values, overrides and additional wheel names.
        let mut ordered = Vec::new();
        for wheel in &c.wheels {
            if let Some(i) = wheels.iter().position(|w| w.name == wheel.name) {
                ordered.push(wheels.remove(i));
            }
        }
        ordered.extend(wheels);
        wheels = ordered;
    }
    Some(Calibre {
        name: entry.name().to_string(),
        bph: entry.bph(),
        lift_angle_deg: entry.lift_angle_deg(),
        lift_set_aside_deg: entry.lift_set_aside_deg(),
        escape_teeth: entry.escape_teeth(),
        meshes: cat.meshes(entry),
        wheels,
    })
}

/// What a reader gives that comes before the session file: a beat rate
/// and a lift angle.
#[derive(Debug, Clone, Copy, Default)]
pub struct Given {
    pub bph: Option<u32>,
    pub lift_deg: Option<f64>,
}

/// A take's analysis settings, from what the reader gives, else the
/// session file `m`, else its calibre `cal`:
///
/// - beat rate: given, else the session file's, else the calibre's;
///   none means it is measured;
/// - lift angle: given, else the session file's, else the calibre's,
///   else [`DEFAULT_LIFT_DEG`]; known when given, when the session file
///   gives it and [`file_lift_known`] says so, or when the calibre does;
/// - escape wheel teeth: the session file's, else the calibre's, else
///   [`ESCAPE_TEETH`].
pub fn stream_settings(m: &Manifest, cal: Option<&Calibre>, given: Given) -> StreamConfig {
    let mut cfg = StreamConfig::default();
    cfg.analysis.bph = given.bph.or(m.bph).or(cal.and_then(|c| c.bph));
    cfg.analysis.amplitude.lift_deg = given
        .lift_deg
        .or(m.lift)
        .or(cal.and_then(|c| c.lift_angle_deg))
        .unwrap_or(DEFAULT_LIFT_DEG);
    cfg.analysis.lift_known = if given.lift_deg.is_some() {
        true
    } else if let Some(known) = file_lift_known(m, cal) {
        known
    } else {
        cal.is_some_and(|c| c.lift_angle_deg.is_some())
    };
    cfg.analysis.escape_teeth = m
        .escape_teeth
        .or(cal.and_then(|c| c.escape_teeth))
        .unwrap_or(ESCAPE_TEETH);
    cfg
}

/// Whether the lift a session file `m` gives was supplied, or `None`
/// when it gives none: its `lift_known` where it says, else (an older
/// file, which did not record it) supplied unless it is the default
/// [`DEFAULT_LIFT_DEG`] and its calibre `cal` gives no lift (#685). Core,
/// the CLI and the desktop take it from here.
pub fn file_lift_known(m: &Manifest, cal: Option<&Calibre>) -> Option<bool> {
    let lift = m.lift?;
    Some(
        m.lift_known
            .unwrap_or(lift != DEFAULT_LIFT_DEG || cal.is_some_and(|c| c.lift_angle_deg.is_some())),
    )
}

/// Where a setting such as the lift angle came from, so a surface can say
/// so without guessing from its own arguments (#1648): given for this
/// command, the take's session file, the calibre's table entry, or the
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingFrom {
    Given,
    Take,
    Calibre,
    Default,
}

impl SettingFrom {
    /// Where a value came from: `given` for this command, else what a
    /// session file `kept`, else a calibre's `table` figure, else the
    /// default. A given value a session file also keeps is said as given
    /// (the two must agree).
    pub fn of<T>(given: Option<T>, kept: Option<T>, table: Option<T>) -> Self {
        if given.is_some() {
            Self::Given
        } else if kept.is_some() {
            Self::Take
        } else if table.is_some() {
            Self::Calibre
        } else {
            Self::Default
        }
    }
}

/// The correction stored for the input `file` was recorded on
/// ([`crate::clockstore::recorded_device`]), from the user's store; `None`
/// when the recording names no input or none is stored for it.
pub fn stored_correction(file: &Path) -> Result<Option<crate::clockstore::DeviceClock>, String> {
    let store = crate::clockstore::ClockStore::load()?;
    Ok(crate::clockstore::recorded_device(file).and_then(|d| store.get(&d).cloned()))
}

/// The correction to apply to a recording: `card_ppm` when given, else
/// the one stored for `device`, else the one stored for the input the
/// recording `file` says it was made on ([`stored_correction`]). The text
/// says where it came from.
pub fn correction(
    file: &Path,
    device: Option<&str>,
    card_ppm: Option<f64>,
) -> Result<Option<(f64, String)>, String> {
    if let Some(p) = card_ppm {
        return Ok(Some((p, "given with --card-ppm".into())));
    }
    match device {
        Some(d) => match crate::clockstore::ClockStore::load()?.get(d) {
            Some(e) => Ok(Some((e.ppm, e.source_text()))),
            None => Err(format!(
                "no clock correction stored for '{d}'; see `timegrapher clock list`"
            )),
        },
        None => Ok(stored_correction(file)?.map(|e| (e.ppm, e.source_text()))),
    }
}

/// The sound card's error read from the USB lines in the first of
/// `files` ([`clock::usb_lines`]), with its source text; `None` when the
/// recording has none.
pub fn usb(files: &[PathBuf]) -> Result<Option<(f64, String)>, String> {
    Ok(clock::usb_lines(files)?.map(|c| c.correction()))
}

/// The correction known for a take with session file `m` whose first
/// recording is `first`, as [`open_entry`] and [`measure_entry`] take it,
/// with where it came from in words: `card_ppm` when the reader gives it,
/// else the one stored for the input `device` the reader names, else the
/// session file's `card_ppm_fast`, else the one stored for the input the
/// recording says it was made on ([`correction`]).
pub fn correction_for(
    m: &Manifest,
    first: &Path,
    card_ppm: Option<f64>,
    device: Option<&str>,
) -> Result<Option<(f64, String)>, String> {
    match (card_ppm, device, m.card_ppm) {
        (None, None, Some(p)) => Ok(Some((p, format!("card_ppm_fast {p} in {MANIFEST}")))),
        (card_ppm, device, _) => correction(first, device, card_ppm),
    }
}

/// How an entry of a take is read, as the caller has resolved it.
pub struct EntrySettings {
    /// The analysis settings: beat rate, lift angle, whether the lift is
    /// known, escape teeth.
    pub stream: StreamConfig,
    /// Rates on the sound card's own clock, uncorrected: for setting
    /// beside references read from the same recordings on that clock
    /// ([`session::Reference::same_clock_as`]).
    pub card_clock: bool,
    /// A settling time given to the reader: after the entry's own, before
    /// the one the session file gives every entry.
    pub settle_s: Option<f64>,
}

/// One entry of a take, opened: its beats, clock, breaks and masks, read
/// as [`measure_entry`] reads them, before any stretch is measured.
pub struct OpenedEntry {
    /// The sound files read, in order; empty for a take kept as its
    /// analysis alone.
    pub files: Vec<PathBuf>,
    /// The kept analysis, for a take kept as its analysis alone.
    pub kept: Option<analysis_take::Stored>,
    /// The beats, numbered across the breaks, with the masked stretches
    /// left out.
    pub log: BeatLog,
    /// The clock the readings are on, and where it came from; both `None`
    /// on the sound card's clock.
    pub clock: Option<ClockFit>,
    pub clock_from: Option<ClockFrom>,
    /// The steady error, ppm fast, the clock puts on the take: a USB or
    /// known correction's; `None` for a clock log's fit or the card's.
    pub clock_ppm: Option<f64>,
    /// Where the clock came from, in words ([`clock_source_text`]).
    pub clock_source: Option<String>,
    /// The clock log's text, when the entry has one.
    pub clock_text: Option<String>,
    pub breaks: Vec<Break>,
    /// Why the breaks are not all the take's own list's, when they are not.
    pub unlisted: Option<breaks::Unlisted>,
    /// The stretches masked by hand, start and end.
    pub masked: Vec<(f64, f64)>,
    /// The loudest sample's size in the sound read (full scale 1), for
    /// its peak level ([`crate::analysis::Clipping::of`]); `None` for a
    /// take kept as its analysis alone.
    pub peak: Option<f32>,
}

impl OpenedEntry {
    /// The clipping over everything read: the clipped samples and beats
    /// the stream counted, and the peak level. `None` for a take kept as
    /// its analysis alone, which keeps no peak to read.
    pub fn clipping(&self) -> Option<crate::analysis::Clipping> {
        let log = &self.log;
        self.peak.map(|peak| {
            crate::analysis::Clipping::of(
                log.clipped_samples,
                log.clipped_beats.len(),
                log.beats.len(),
                peak,
            )
        })
    }
}

/// One entry of a take, read: what was read, and its measurement.
pub struct EntryRead {
    /// The sound files read, in order; empty for a take kept as its
    /// analysis alone.
    pub files: Vec<PathBuf>,
    /// The kept analysis, for a take kept as its analysis alone.
    pub kept: Option<analysis_take::Stored>,
    /// The beats, numbered across the breaks, with the masked stretches
    /// left out.
    /// The beat rate and lift angle read at are `log.bph` and
    /// `log.lift_deg`.
    pub log: BeatLog,
    /// The clock the readings are on, and where it came from, as
    /// [`clock_from`] picks it; both `None` on the sound card's clock.
    pub clock: Option<ClockFit>,
    pub clock_from: Option<ClockFrom>,
    /// The clock's steady error and its source in words, as
    /// [`OpenedEntry::clock_ppm`] and [`OpenedEntry::clock_source`] give them.
    pub clock_ppm: Option<f64>,
    pub clock_source: Option<String>,
    /// The clock log's text, when the entry has one.
    pub clock_text: Option<String>,
    pub breaks: Vec<Break>,
    /// Why the breaks are not all the take's own list's, when they are not.
    pub unlisted: Option<breaks::Unlisted>,
    /// The stretches masked by hand, start and end.
    pub masked: Vec<(f64, f64)>,
    /// The stretch the entry names ([`span_of`]).
    pub start_s: f64,
    pub stop_s: f64,
    /// The stretch measured, once the watch has settled ([`measured`]).
    pub settle_s: f64,
    pub end_s: f64,
    pub measurement: Measurement,
}

/// The stretch an entry's reading measured, on its own: for reading the
/// changes over it that repeat (`session`'s cycles) as the reading itself
/// is read ([`EntryRead::measured_part`]).
pub struct MeasuredPart<'a> {
    /// The reading's beats, windows and sounds, timed from its start.
    pub log: BeatLog,
    /// The take's clock, when it holds no gap of its own to keep in place
    /// (a ppm or so makes no odds to a cycle's period).
    pub clock: Option<&'a ClockFit>,
    /// The breaks inside it, and each masked stretch cut at both ends with
    /// no time lost, timed from its start, in order.
    pub breaks: Vec<Break>,
}

impl EntryRead {
    /// The entry's settling, from the stretch it names to where its
    /// reading starts: what a reading over the entry whole leaves out as a
    /// mask, as [`crate::take_context::TakeContext::holes`] does for a
    /// take (#1233). None when it settles for no time.
    pub fn holes(&self) -> Vec<(f64, f64)> {
        (self.settle_s > self.start_s)
            .then_some((self.start_s, self.settle_s))
            .into_iter()
            .collect()
    }

    /// The stretch this reading measured, from its settling to its end,
    /// cut as [`session::measure_masked`] cuts it.
    pub fn measured_part(&self) -> MeasuredPart<'_> {
        let holes = Cuts::new(self.clock.as_ref(), &self.breaks, &self.masked);
        let log = trimmed(&self.log, &holes, self.settle_s, self.end_s);
        let inside = |b: &Break| b.audio_s > self.settle_s && b.audio_s < self.end_s;
        let mut breaks: Vec<Break> = self
            .breaks
            .iter()
            .filter(|b| inside(b))
            .map(|b| Break {
                audio_s: b.audio_s - self.settle_s,
                // A pause the clock log did not see has no gap (#646).
                gap_s: if self.clock.as_ref().is_some_and(|c| !c.keeps_gap(b)) {
                    0.0
                } else {
                    b.gap_s
                },
            })
            .chain(
                crate::clean::mask_breaks(&self.masked)
                    .into_iter()
                    .filter(inside)
                    .map(|b| Break {
                        audio_s: b.audio_s - self.settle_s,
                        ..b
                    }),
            )
            .collect();
        breaks.sort_by(|a, b| a.audio_s.total_cmp(&b.audio_s));
        MeasuredPart {
            log,
            clock: self.clock.as_ref().filter(|c| c.breaks.is_empty()),
            breaks,
        }
    }
}

/// The log from `from_s` to `to_s`, with times shifted to start at zero.
/// Its amplitude windows are the reading's: those
/// [`session::stretch_windows`] keeps in the stretch with the reading's
/// cuts `holes`, so none reaches into a mask or a pause (#636).
fn trimmed(log: &BeatLog, holes: &Cuts, from_s: f64, to_s: f64) -> BeatLog {
    let mut l = log.clone();
    l.beats.retain(|b| b.time >= from_s && b.time < to_s);
    for b in &mut l.beats {
        b.time -= from_s;
    }
    l.amplitude_windows = session::stretch_windows(holes, &log.amplitude_windows, from_s, to_s)
        .into_iter()
        .cloned()
        .collect();
    for w in &mut l.amplitude_windows {
        w.start_s -= from_s;
        w.end_s -= from_s;
    }
    l.snippets.retain(|s| s.0 >= from_s && s.0 < to_s);
    for s in &mut l.snippets {
        s.0 -= from_s;
    }
    l.duration_s = to_s.min(log.duration_s) - from_s;
    l
}

/// Open entry `e` of the take described by `m` in the folder `base`, as
/// every surface reads a take: its beats (found in its sound, or kept as
/// its analysis alone), its clock and breaks ([`clock`], [`breaks`]) and
/// its masks. `usb` and `correction` are as [`measure_entry`] takes them.
/// `progress` is told how far through the sound the beats have been
/// found: the seconds of sound read, and the seconds of sound there are
/// in all (`done_s` reaches `total_s` at the end); a take kept as its
/// analysis alone has no sound to go through.
pub fn open_entry(
    base: &Path,
    m: &Manifest,
    e: &RecordingEntry,
    s: &EntrySettings,
    usb: impl FnOnce(&[PathBuf]) -> Result<Option<(f64, String)>, String>,
    correction: impl FnOnce(&Path) -> Result<Option<(f64, String)>, String>,
    progress: impl FnMut(f64, f64),
) -> Result<OpenedEntry, String> {
    let path = base.join(&e.file);
    let kept = kept_take(&path, m, e)?;
    let files = match &kept {
        Some(_) => Vec::new(),
        None => sound_files(std::slice::from_ref(&path))?,
    };
    // A take kept as its analysis alone: every part of the take was in
    // it, so its session.json lists its breaks for the whole take's sound.
    // One that is there but can't be read is said, as for a take with its
    // sound ([`breaks::Unlisted::Unreadable`]), never taken for none.
    let json = kept
        .as_ref()
        .map(|_| breaks::read_session(&path.join(RECORD)));
    let sound = Sound {
        name: &e.file,
        kept,
        json,
        files,
        clock_path: e.clock.as_ref().map(|p| base.join(p)),
        masked: &e.masked,
    };
    open_sound(sound, m, s, usb, correction, progress)
}

/// Open the sound `files` as one recording with no session file, as
/// [`open_entry`] opens a take: each a sound file or a folder of them,
/// read in order ([`sound_files`]), on the clock log `clock_log` when one
/// is given, with no masks. `usb`, `correction` and `progress` are as
/// [`open_entry`] takes them.
pub fn open_files(
    files: &[PathBuf],
    clock_log: Option<&Path>,
    s: &EntrySettings,
    usb: impl FnOnce(&[PathBuf]) -> Result<Option<(f64, String)>, String>,
    correction: impl FnOnce(&Path) -> Result<Option<(f64, String)>, String>,
    progress: impl FnMut(f64, f64),
) -> Result<OpenedEntry, String> {
    let files = sound_files(files)?;
    let name = files
        .first()
        .ok_or("no recording given")?
        .display()
        .to_string();
    let sound = Sound {
        name: &name,
        kept: None,
        json: None,
        files,
        clock_path: clock_log.map(Path::to_path_buf),
        masked: &[],
    };
    open_sound(sound, &Manifest::default(), s, usb, correction, progress)
}

/// What [`open_sound`] opens: a take's entry, or loose sound files.
struct Sound<'a> {
    /// The name its errors are given under.
    name: &'a str,
    /// The take kept as its analysis alone, when it is one.
    kept: Option<analysis_take::Stored>,
    /// That take's `session.json`, read.
    json: Option<Result<Option<serde_json::Value>, breaks::Unlisted>>,
    /// The sound files, in order; empty for a kept take.
    files: Vec<PathBuf>,
    clock_path: Option<PathBuf>,
    /// The stretches masked by hand, as the session file lists them.
    masked: &'a [[f64; 2]],
}

/// Open `sound` with session file `m` (none for loose files), as
/// [`open_entry`] says.
fn open_sound(
    sound: Sound,
    m: &Manifest,
    s: &EntrySettings,
    usb: impl FnOnce(&[PathBuf]) -> Result<Option<(f64, String)>, String>,
    correction: impl FnOnce(&Path) -> Result<Option<(f64, String)>, String>,
    progress: impl FnMut(f64, f64),
) -> Result<OpenedEntry, String> {
    let Sound {
        name,
        kept,
        json,
        files,
        clock_path,
        masked,
    } = sound;
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let (mut log, peak) = entry_log(kept.as_ref(), &paths, &s.stream, name, progress)?;
    let clock_text = match &clock_path {
        Some(f) => Some(fs::read_to_string(f).map_err(|err| format!("{}: {err}", f.display()))?),
        None => None,
    };
    let ReadClock {
        mut fit,
        from: clock_from,
        ppm: clock_ppm,
        source: clock_source,
    } = entry_clock(
        kept.as_ref(),
        &files,
        m,
        s.card_clock,
        (clock_text.as_deref(), clock_path.as_deref()),
        log.duration_s,
        usb,
        correction,
    )?;
    let listed = match &json {
        Some(json) => Listed::Session(json.as_ref().map(Option::as_ref).map_err(|w| *w)),
        None => Listed::Files(&paths),
    };
    let breaks::Found { breaks, unlisted } = breaks(&mut log, listed, fit.as_mut());
    // Stretches masked by hand are left out of everything read below.
    let masked = session::masked_spans(masked);
    session::mask_log(&mut log, &masked);
    Ok(OpenedEntry {
        files,
        kept,
        log,
        clock: fit,
        clock_from,
        clock_ppm,
        clock_source,
        clock_text,
        breaks,
        unlisted,
        masked,
        peak,
    })
}

/// The take at `path` kept as its analysis alone, read; `None` for a
/// take with its sound. Refused when the folder and `audio = false` in
/// the manifest disagree, or when it holds both.
fn kept_take(
    path: &Path,
    m: &Manifest,
    e: &RecordingEntry,
) -> Result<Option<analysis_take::Stored>, String> {
    if analysis_take::is_analysis_only(path) {
        if m.audio != Some(false) {
            return Err(format!(
                "{}: it has an {}/ folder, but {MANIFEST} doesn't say audio = false",
                e.file,
                analysis_take::DIR
            ));
        }
        if has_sound(path) {
            return Err(format!(
                "{}: it holds both sound and an {}/ folder: a take is kept either with its \
                 sound or as its analysis alone",
                e.file,
                analysis_take::DIR
            ));
        }
        Ok(Some(
            analysis_take::read(path).map_err(|err| format!("{}: {err}", e.file))?,
        ))
    } else if m.audio == Some(false) {
        Err(format!(
            "{}: {MANIFEST} says audio = false, but there is no {}/ folder to read",
            e.file,
            analysis_take::DIR
        ))
    } else {
        Ok(None)
    }
}

/// The entry's beats: read from the take `kept` as its analysis alone,
/// or found in its sound `paths`, telling `progress` how far through;
/// with the loudest sample's size in the sound read.
fn entry_log(
    kept: Option<&analysis_take::Stored>,
    paths: &[&Path],
    cfg: &StreamConfig,
    file: &str,
    progress: impl FnMut(f64, f64),
) -> Result<(BeatLog, Option<f32>), String> {
    match kept {
        Some(k) => Ok((kept_at_lift(&k.log, cfg.analysis.amplitude.lift_deg), None)),
        None => sound_log(
            crate::beat_cache::dir().as_deref(),
            paths,
            cfg,
            file,
            progress,
        )
        .map(|(log, peak)| (log, Some(peak))),
    }
}

/// The beats found in the sound `paths` read with `cfg`, as an opened
/// take's are found: from the copy [`crate::beat_cache`] keeps when the
/// surface keeps one and it matches, else from the sound. For a problem
/// report's analysis of the take on show (#400), which then costs no
/// second reading of a long take the app has just opened.
pub fn sound_beats(paths: &[&Path], cfg: &StreamConfig) -> Result<BeatLog, String> {
    let name = paths
        .first()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    sound_log(
        crate::beat_cache::dir().as_deref(),
        paths,
        cfg,
        &name,
        |_, _| {},
    )
    .map(|(log, _)| log)
}

/// The beats found in the sound `paths`, as [`entry_log`] finds them:
/// from the copy kept in `cache` when it has one that matches
/// ([`crate::beat_cache`]), else from the sound, then kept there.
fn sound_log(
    cache: Option<&Path>,
    paths: &[&Path],
    cfg: &StreamConfig,
    file: &str,
    mut progress: impl FnMut(f64, f64),
) -> Result<(BeatLog, f32), String> {
    let mut total_s = 0.0;
    for p in paths {
        let i = crate::audio::info(p).map_err(|err| format!("{}: {err}", p.display()))?;
        total_s += i.frames.unwrap_or(0) as f64 / i.sample_rate as f64;
    }
    if let Some(got) = cache.and_then(|d| crate::beat_cache::read(d, paths, cfg)) {
        progress(total_s, total_s);
        return Ok(got);
    }
    let (log, peak) =
        stream::analyze_files_peak(paths, cfg, |done_s| progress(done_s, total_s.max(done_s)))
            .map_err(|err| format!("{file}: {err}"))?;
    if let Some(d) = cache {
        // A copy that can't be kept only costs the next open its time.
        let _ = crate::beat_cache::write(d, paths, cfg, &log, peak);
    }
    Ok((log, peak))
}

/// A take's clock, read as [`read_clock`] reads it.
#[derive(Debug, Clone, Default)]
pub struct ReadClock {
    /// The clock the readings are on, and where it came from; both `None`
    /// on the sound card's clock.
    pub fit: Option<ClockFit>,
    pub from: Option<ClockFrom>,
    /// The steady error, ppm fast, the clock puts on the take: a USB or
    /// known correction's; `None` for a clock log's fit or the card's.
    pub ppm: Option<f64>,
    /// Where the clock came from, in words ([`clock_source_text`]).
    pub source: Option<String>,
}

/// A take's clock, as every surface reads it: [`clock`] from its clock
/// log (`log`: its text, and the file it was read from), the USB lines
/// `usb` gives and the correction `correction` gives (each ppm fast, with
/// its words), on sound at `rate` Hz with `bytes_per_frame`, `duration_s`
/// long; with its steady error and where it came from in words, said
/// once for every surface. A clock log makes a correction unneeded, so
/// `correction` is asked only without one, and before `usb`.
pub fn read_clock(
    (text, path): (Option<&str>, Option<&Path>),
    (rate, bytes_per_frame): (u32, u32),
    duration_s: f64,
    usb: impl FnOnce() -> Result<Option<(f64, String)>, String>,
    correction: impl FnOnce() -> Result<Option<(f64, String)>, String>,
) -> Result<ReadClock, String> {
    let correction_known = match text {
        Some(_) => None,
        None => correction()?,
    };
    let usb_known = usb()?;
    let usb_ppm = usb_known.as_ref().map(|(p, _)| *p);
    let correction_ppm = correction_known.as_ref().map(|(p, _)| *p);
    let fit = clock(
        text,
        rate,
        bytes_per_frame,
        usb_ppm,
        correction_ppm,
        duration_s,
    )
    .map_err(|err| match path {
        Some(p) => format!("{}: {err}", p.display()),
        None => err,
    })?;
    let from = clock_from(text.is_some(), usb_ppm, correction_ppm).filter(|_| fit.is_some());
    let ppm = from.and_then(|from| match from {
        ClockFrom::Log => None,
        ClockFrom::LogRatedByUsb | ClockFrom::Usb => usb_ppm,
        ClockFrom::Correction => correction_ppm,
    });
    let source = from.map(|from| {
        clock_source_text(
            from,
            usb_known.as_ref().map(|(_, t)| t.as_str()),
            correction_known.as_ref().map(|(_, t)| t.as_str()),
            path,
        )
    });
    Ok(ReadClock {
        fit,
        from,
        ppm,
        source,
    })
}

/// The clock of an entry ([`read_clock`]), from its clock `log` (its
/// text and file), the USB clock and the card correction: as it was
/// recorded for a take `kept` as its analysis alone, or from its sound
/// `files`. None of it when the rates stay on the card's own clock.
#[expect(
    clippy::too_many_arguments,
    reason = "the parts of one entry open_entry has already read"
)]
fn entry_clock(
    kept: Option<&analysis_take::Stored>,
    files: &[PathBuf],
    m: &Manifest,
    card_clock: bool,
    log: (Option<&str>, Option<&Path>),
    duration_s: f64,
    usb: impl FnOnce(&[PathBuf]) -> Result<Option<(f64, String)>, String>,
    correction: impl FnOnce(&Path) -> Result<Option<(f64, String)>, String>,
) -> Result<ReadClock, String> {
    match kept {
        _ if card_clock => Ok(ReadClock::default()),
        // Read as it was recorded: its sound is not here.
        Some(k) => {
            let usb_known = k.header.usb_clock.map(|u| {
                crate::usbclock::UsbClock {
                    ppm: u.ppm,
                    lines: u.lines,
                    spread_ppm: u.spread_ppm,
                }
                .correction()
            });
            let h = &k.header;
            read_clock(
                log,
                (h.sample_rate, h.bytes_per_frame),
                duration_s,
                || Ok(usb_known),
                || {
                    Ok(m.card_ppm
                        .map(|p| (p, format!("card_ppm_fast {p} in {MANIFEST}"))))
                },
            )
        }
        None => {
            let info = crate::audio::info(&files[0])
                .map_err(|err| format!("{}: {err}", files[0].display()))?;
            read_clock(
                log,
                (info.sample_rate, info.bytes_per_frame),
                duration_s,
                || usb(files),
                || correction(&files[0]),
            )
        }
    }
}

/// Read entry `e` of the take described by `m` in the folder `base`, as
/// every surface reads a take: its beats (found in its sound, or kept as
/// its analysis alone), its clock and breaks ([`clock`], [`breaks`]),
/// its masks, and the stretch measured once the watch has settled
/// ([`measured`]). `usb` gives the sound card's error from the USB lines
/// in the files, and `correction` the correction known for the input of
/// the first file, each ppm fast: they only fetch, and the rule that
/// picks between them and the clock log is this function's ([`clock`]).
/// `correction` is asked only when the entry has no clock log. An error
/// either gives is passed through as its text, which is the caller's. An
/// entry with nothing to read in its recording is [`EntryError::NoReading`].
pub fn measure_entry(
    base: &Path,
    m: &Manifest,
    e: &RecordingEntry,
    s: &EntrySettings,
    usb: impl FnOnce(&[PathBuf]) -> Result<Option<(f64, String)>, String>,
    correction: impl FnOnce(&Path) -> Result<Option<(f64, String)>, String>,
) -> Result<EntryRead, EntryError> {
    let opened = open_entry(base, m, e, s, usb, correction, |_, _| {})?;
    measure_opened(opened, m, e, s)
}

/// Entry `e` of the take described by `m`, already opened with
/// [`open_entry`], measured as [`measure_entry`] measures it: for a
/// reader that also reads the opened entry whole.
pub fn measure_opened(
    opened: OpenedEntry,
    m: &Manifest,
    e: &RecordingEntry,
    s: &EntrySettings,
) -> Result<EntryRead, EntryError> {
    let OpenedEntry {
        files,
        kept,
        log,
        clock: fit,
        clock_from,
        clock_ppm,
        clock_source,
        clock_text,
        breaks,
        unlisted,
        masked,
        peak: _,
    } = opened;
    let cfg = &s.stream;
    let Window {
        start_s,
        stop_s,
        from_s: settle_s,
        to_s: end_s,
    } = reading_window(e, m, s.settle_s, log.duration_s).map_err(EntryError::NoReading)?;
    // A lead of noise before the watch is left out of the reading (#844).
    let lead = session::lead_s(&log, fit.as_ref(), &breaks, &masked, settle_s, end_s);
    let read = read_stretches(m, &e.file, s.settle_s, log.duration_s);
    let mut measurement = session::measure_reading(
        &log,
        fit.as_ref(),
        &breaks,
        &masked,
        (lead.unwrap_or(settle_s), end_s),
        &read,
    );
    if let Some(t) = lead {
        let seconds = t - start_s;
        crate::measure::add(
            &mut measurement.caveats,
            crate::measure::Caveat::LeadLeftOut { seconds },
        );
    }
    measurement.take_doubts(cfg.analysis.lift_known, unlisted);
    measurement.clock_ppm = clock_ppm;
    measurement.clock_source = clock_source.clone();
    Ok(EntryRead {
        files,
        kept,
        log,
        clock: fit,
        clock_from,
        clock_ppm,
        clock_source,
        clock_text,
        breaks,
        unlisted,
        masked,
        start_s,
        stop_s,
        settle_s,
        end_s,
        measurement,
    })
}

/// The take described by `m` as one entry over the whole of its sound,
/// for a reader of the whole take (`series`, `long`) to open with
/// [`open_entry`]: the recording its entries name when they all name the
/// same one (a take the app saved, one entry per position), with every
/// entry's masks; else the take's folder, read as one recording. The
/// clock log is the one the entries name. Positions, stretches and
/// settling are left out: the whole take is read.
///
/// An error when the entries name different clock logs, or when they
/// name different recordings and mask any of them, as a mask is placed in
/// seconds into its own recording.
pub fn whole_take(m: &Manifest) -> Result<RecordingEntry, String> {
    let mut files: Vec<&str> = m.recordings.iter().map(|e| e.file.as_str()).collect();
    files.sort_unstable();
    files.dedup();
    let mut clocks: Vec<&str> = m
        .recordings
        .iter()
        .filter_map(|e| e.clock.as_deref())
        .collect();
    clocks.sort_unstable();
    clocks.dedup();
    if clocks.len() > 1 {
        return Err(format!(
            "{MANIFEST} names {} clock logs ({}); a whole take is read on one",
            clocks.len(),
            clocks.join(", ")
        ));
    }
    let masked: Vec<[f64; 2]> = m
        .recordings
        .iter()
        .flat_map(|e| e.masked.iter().copied())
        .collect();
    let file = match files.as_slice() {
        [one] => one.to_string(),
        _ if !masked.is_empty() => {
            return Err(format!(
                "{MANIFEST} lists {} recordings and masks stretches of them, in seconds into \
                 each; read them one at a time with `timegrapher session`",
                files.len()
            ))
        }
        _ => ".".to_string(),
    };
    Ok(RecordingEntry {
        file,
        clock: clocks.first().map(|c| c.to_string()),
        masked,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {

    /// A live reading's clock is the one opening the take saved at that
    /// moment gives: the measured USB lines over a kept correction, and
    /// the kept correction alone until the lines are measured (#1721).
    #[test]
    fn a_live_reading_takes_the_clock_a_reopened_take_takes() {
        use crate::usbclock::{LiveClock, UsbClock};
        let lines = UsbClock {
            ppm: -19.3,
            lines: 8,
            spread_ppm: 0.2,
        };
        let measured = LiveClock::Measured {
            clock: lines,
            over_s: 60.0,
        };
        for kept in [None, Some(-18.0)] {
            let same =
                |a: Option<ClockFit>, b: Option<ClockFit>| format!("{a:?}") == format!("{b:?}");
            assert!(same(
                live_clock(&measured, kept, 300.0),
                clock(None, 0, 0, Some(-19.3), kept, 300.0).unwrap()
            ));
            for open in [
                LiveClock::NotMeasured { heard_s: 12.0 },
                LiveClock::NoLines { over_s: 30.0 },
            ] {
                assert!(same(
                    live_clock(&open, kept, 300.0),
                    clock(None, 0, 0, None, kept, 300.0).unwrap()
                ));
            }
        }
        assert!(live_clock(&measured, None, 300.0).is_some());
        assert!(live_clock(&LiveClock::NotMeasured { heard_s: 1.0 }, None, 300.0).is_none());
    }
    use super::*;

    /// A part of a log takes the reading's amplitude windows: those whose
    /// midpoint is in it and that reach into no cut (#636).
    #[test]
    fn a_part_of_a_log_takes_the_windows_whose_midpoint_is_in_it() {
        let w = |start_s: f64| crate::amplitude::AmplitudeWindow {
            start_s,
            end_s: start_s + 2.0,
            even_deg: Some(250.0),
            odd_deg: Some(250.0),
            beat_error_ms: None,
            beat_error_unlock_ms: None,
            even_unlock_ms: None,
            odd_unlock_ms: None,
            even_drop_ms: None,
            odd_drop_ms: None,
            culled_ms: None,
        };
        let log = BeatLog {
            sample_rate: 48000,
            duration_s: 40.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: Vec::new(),
            amplitude_windows: [8.5, 9.5, 10.5, 28.5, 29.5].map(w).to_vec(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let none = Cuts::new(None, &[], &[]);
        let part = trimmed(&log, &none, 10.0, 30.0);
        let starts: Vec<f64> = part.amplitude_windows.iter().map(|w| w.start_s).collect();
        // Midpoints 10.5, 11.5 and 29.5 are in [10, 30); 9.5 and 30.5 not.
        assert_eq!(starts, vec![-0.5, 0.5, 18.5]);
        // A window that reaches into a mask is not the reading's, so not
        // the part's either.
        let masked = Cuts::new(None, &[], &[(11.0, 12.0)]);
        let part = trimmed(&log, &masked, 10.0, 30.0);
        let starts: Vec<f64> = part.amplitude_windows.iter().map(|w| w.start_s).collect();
        assert_eq!(starts, vec![18.5]);
    }

    /// A take's clock from a known correction runs over all its sound
    /// files, and so does what its reading says there is to read: their
    /// frames added up, at the first file's rate (#756).
    #[test]
    fn a_takes_clock_and_reading_span_all_its_files() {
        let dir = std::env::temp_dir().join(format!("tg-clock-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let files = [dir.join("a.wav"), dir.join("b.wav")];
        for (f, s) in files.iter().zip([10, 20]) {
            let audio = crate::audio::Audio {
                samples: vec![0.0; 8000 * s],
                sample_rate: 8000,
            };
            crate::audio::write_wav(f, &audio).unwrap();
        }
        let fit = clock_of_files(&dir, None, &files, None, Some(10.0))
            .unwrap()
            .unwrap();
        assert_eq!((fit.ppm, fit.span_s), (10.0, 30.0));
        // The sound to read is all of it, known before any is read.
        let cfg = StreamConfig {
            chunk_s: 5.0,
            ..StreamConfig::default()
        };
        let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
        let mut told = Vec::new();
        entry_log(None, &paths, &cfg, "a.wav", |done, total| {
            told.push((done, total))
        })
        .unwrap();
        assert!(told.len() > 1 && told[0].0 < 30.0, "{told:?}");
        assert!(told.iter().all(|t| t.1 == 30.0), "{told:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// With a folder of copies, the first open finds the beats in the
    /// sound and keeps them; the next reads them from the copy, telling
    /// `progress` once that all the sound is done, and gives the same
    /// beats to the bit (#726).
    #[test]
    fn a_take_opened_again_reads_its_kept_beats() {
        use std::time::{Duration, SystemTime};
        let dir = std::env::temp_dir().join(format!("tg-sound-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.wav");
        let audio = crate::synth::generate(
            &crate::synth::SynthConfig {
                duration_s: 60.0,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        crate::audio::write_wav(&file, &audio).unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
        let cache = dir.join("cache");
        let cfg = StreamConfig::default();
        let paths = [file.as_path()];
        let mut first = 0;
        let found = sound_log(Some(&cache), &paths, &cfg, "a.wav", |_, _| first += 1).unwrap();
        let mut told = Vec::new();
        let kept = sound_log(Some(&cache), &paths, &cfg, "a.wav", |d, t| {
            told.push((d, t))
        })
        .unwrap();
        assert!(first > 1);
        assert_eq!(told, [(60.0, 60.0)]);
        assert_eq!(
            crate::beat_cache::encode(&kept.0, kept.1),
            crate::beat_cache::encode(&found.0, found.1)
        );
        // With no folder, nothing is kept or read.
        let mut none = 0;
        sound_log(None, &paths, &cfg, "a.wav", |_, _| none += 1).unwrap();
        assert!(none > 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A log of `duration_s` seconds holding `beats` at those times and
    /// nothing else.
    fn bare_log(duration_s: f64, beats: &[f64]) -> BeatLog {
        BeatLog {
            sample_rate: 48000,
            duration_s,
            bph: 28800,
            lift_deg: 52.0,
            beats: beats
                .iter()
                .enumerate()
                .map(|(i, &time)| crate::beats::Beat {
                    index: i as i64,
                    time,
                    quality: 1.0,
                })
                .collect(),
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        }
    }

    /// The measured part holds what lies from the settling up to the end,
    /// timed from the settling: beats from its first instant, and only
    /// the breaks strictly inside it. A listed pause keeps its gap; each
    /// end of a mask is a break with none (#756).
    #[test]
    fn the_measured_part_is_what_lies_inside_it_timed_from_its_settling() {
        let mut log = bare_log(200.0, &[9.5, 10.0, 50.0, 99.5, 100.0]);
        log.amplitude_windows = vec![crate::amplitude::AmplitudeWindow {
            start_s: 50.0,
            end_s: 52.0,
            even_deg: Some(250.0),
            odd_deg: Some(250.0),
            beat_error_ms: None,
            beat_error_unlock_ms: None,
            even_unlock_ms: None,
            odd_unlock_ms: None,
            even_drop_ms: None,
            odd_drop_ms: None,
            culled_ms: None,
        }];
        log.snippets = [9.5, 10.0, 50.0, 100.0].map(|t| (t, Vec::new())).to_vec();
        let pause = |audio_s| Break {
            audio_s,
            gap_s: 3.0,
        };
        let read = EntryRead {
            files: Vec::new(),
            kept: None,
            log: log.clone(),
            // A clock with no log of its own keeps every listed gap.
            clock: Some(ClockFit::fixed(0.0, 200.0)),
            clock_from: None,
            clock_ppm: None,
            clock_source: None,
            clock_text: None,
            breaks: vec![pause(5.0), pause(10.0), pause(60.0), pause(100.0)],
            unlisted: None,
            masked: vec![(30.0, 40.0)],
            start_s: 0.0,
            stop_s: 200.0,
            settle_s: 10.0,
            end_s: 100.0,
            measurement: session::measure(&log, None, 10.0, 100.0),
        };
        let part = read.measured_part();
        let times: Vec<f64> = part.log.beats.iter().map(|b| b.time).collect();
        assert_eq!(times, vec![0.0, 40.0, 89.5]);
        let w = &part.log.amplitude_windows;
        assert_eq!((w.len(), w[0].start_s, w[0].end_s), (1, 40.0, 42.0));
        let sounds: Vec<f64> = part.log.snippets.iter().map(|s| s.0).collect();
        assert_eq!(sounds, vec![0.0, 40.0]);
        assert_eq!(part.log.duration_s, 90.0);
        let breaks: Vec<(f64, f64)> = part.breaks.iter().map(|b| (b.audio_s, b.gap_s)).collect();
        assert_eq!(breaks, vec![(20.0, 0.0), (30.0, 0.0), (50.0, 3.0)]);
        assert!(part.clock.is_some());
    }

    /// A take's whole entry: a folder with a take's entries is the take
    /// read whole ([`whole_take`]); a file, or a take that lists nothing,
    /// is that sound on its own, in the folder that holds it (#756).
    #[test]
    fn a_whole_entry_is_the_take_or_else_the_sound_named() {
        let dir = std::env::temp_dir();
        let m = Manifest {
            recordings: vec![entry("take.flac", Some("clock.csv"), &[])],
            ..Default::default()
        };
        let (base, read, e) = whole_entry(&dir, Some(&m)).unwrap();
        assert_eq!((base, read.recordings.len()), (dir.clone(), 1));
        assert_eq!(
            (e.file.as_str(), e.clock.as_deref()),
            ("take.flac", Some("clock.csv"))
        );
        let file = dir.join("one.wav");
        for listed in [None, Some(&Manifest::default())] {
            let (base, read, e) = whole_entry(&file, listed).unwrap();
            assert_eq!((base, read.recordings.len()), (dir.clone(), 0));
            assert_eq!((e.file.as_str(), e.clock), ("one.wav", None));
        }
    }

    /// Each entry that names a position is listed with its start, where
    /// its reading starts and ends (no end when it runs to the end of a
    /// recording of unknown length) and its position; an entry that reads
    /// nothing, at its end (#756).
    #[test]
    fn the_listed_positions_are_the_entries_that_name_one() {
        let m: Manifest = toml::from_str(
            "settle_s = 30.0\n\
             [[recording]]\nfile = \"a.flac\"\nposition = \"DU\"\nend_s = 600.0\n\
             [[recording]]\nfile = \"a.flac\"\nstart_s = 600.0\nend_s = 650.0\n\
             [[recording]]\nfile = \"a.flac\"\nposition = \"CD\"\nstart_s = 650.0\nend_s = 660.0\n\
             [[recording]]\nfile = \"a.flac\"\nposition = \"CU\"\nstart_s = 700.0\n",
        )
        .unwrap();
        let listed = listed_positions(&m, &m.recordings, f64::INFINITY);
        let p = |s: &str| session::Position::parse(s).unwrap();
        assert_eq!(
            listed,
            vec![
                (0.0, 30.0, Some(600.0), p("DU"), false),
                (650.0, 660.0, Some(660.0), p("CD"), false),
                (700.0, 730.0, None, p("CU"), false),
            ]
        );
    }

    /// Each position begins where the one before it ended, also when that
    /// one ran on past its start; the first at the take's start.
    /// Its settling runs from there to where its reading starts, and is
    /// none when the reading starts there (#756).
    #[test]
    fn a_position_begins_where_the_one_before_ended() {
        let listed = [
            (290.0, 320.0, None, 'C', false),
            (0.0, 0.0, Some(100.0), 'A', false),
            (120.0, 150.0, Some(300.0), 'B', false),
        ];
        let (changes, settles) = positions_of(listed.into_iter());
        // B is read to 300, past C's start: C begins and settles from where
        // B stopped being read, so no sound is both read and settling
        // (#1156).
        assert_eq!(changes, vec![(0.0, 'A'), (100.0, 'B'), (300.0, 'C')]);
        assert_eq!(settles, vec![(100.0, 150.0), (300.0, 320.0)]);
        // ym42_CR_long's third file: one entry read to 337 s, the next of
        // the same file settling to 377 s. The second begins at 337, so
        // the take reads both windows, as session does. One whose reading
        // starts before the one before stops (readings overlapping) begins
        // at its reading, with no settling.
        let listed = [
            (0.0, 0.0, Some(337.0), 'R', false),
            (0.0, 377.0, None, 'R', false),
        ];
        assert_eq!(
            positions_of(listed.into_iter()),
            (vec![(0.0, 'R'), (337.0, 'R')], vec![(337.0, 377.0)])
        );
        let listed = [
            (0.0, 0.0, Some(337.0), 'R', false),
            (0.0, 300.0, None, 'R', false),
        ];
        assert_eq!(
            positions_of(listed.into_iter()),
            (vec![(0.0, 'R'), (300.0, 'R')], vec![])
        );
    }

    /// An entry that carries on the one before it ([`continues_position`])
    /// is no change: the take is one stretch across it (#1166). Two entries
    /// of one file split at 600 s with `settle_s = 0` do not carry on, so
    /// they stay two, as `series` cuts them.
    #[test]
    fn a_carried_on_entry_is_no_change_of_position() {
        let m: Manifest = toml::from_str(
            "[[recording]]\nfile = \"a.flac\"\nposition = \"DD\"\nsettle_s = 20.0\n\
             [[recording]]\nfile = \"b.flac\"\nposition = \"DD\"\n\
             [[recording]]\nfile = \"c.flac\"\nposition = \"DD\"\n",
        )
        .unwrap();
        let carried: Vec<bool> = m
            .recordings
            .iter()
            .map(|e| continues_position(e, &m))
            .collect();
        assert_eq!(carried, [false, true, true]);
        let listed = [
            (0.0, 20.0, Some(600.0), 'D', false),
            (600.0, 600.0, Some(1200.0), 'D', true),
            (1200.0, 1200.0, None, 'D', true),
        ];
        let (changes, settles) = positions_of(listed.into_iter());
        assert_eq!(changes, vec![(0.0, 'D')]);
        assert_eq!(settles, vec![(0.0, 20.0)]);
        let split: Manifest = toml::from_str(
            "settle_s = 0.0\n\
             [[recording]]\nfile = \"a.flac\"\nposition = \"DD\"\nend_s = 600.0\n\
             [[recording]]\nfile = \"a.flac\"\nposition = \"DD\"\nstart_s = 600.0\n",
        )
        .unwrap();
        let listed = listed_positions(&split, &split.recordings, 1200.0);
        assert!(listed.iter().all(|l| !l.4), "{listed:?}");
        // ym42_CR_long's third file: a carried-on entry with its own
        // measure_s, then one of the same file with its own settle_s. The
        // second settles, so it is a change, however the first is read.
        let own: Manifest = toml::from_str(
            "[[recording]]\nfile = \"b.flac\"\nposition = \"CR\"\n\
             [[recording]]\nfile = \"c.flac\"\nposition = \"CR\"\nmeasure_s = 337.0\n\
             [[recording]]\nfile = \"c.flac\"\nposition = \"CR\"\nsettle_s = 377.0\n",
        )
        .unwrap();
        assert!(continues_position(&own.recordings[1], &own));
        assert!(!continues_position(&own.recordings[2], &own));
        let listed = listed_positions(&own, &own.recordings[1..], 600.0);
        assert_eq!(
            listed.iter().map(|l| l.4).collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(listed[1].1, 377.0);
        let (changes, _) = positions_of(listed.into_iter());
        assert_eq!(changes.len(), 2);
    }

    /// A reading reaches across no break, no change of position after the
    /// first and no end of a settling; each place is said once, in order.
    #[test]
    fn a_reading_is_cut_at_breaks_changes_and_settled_ends() {
        let b = Break {
            audio_s: 130.0,
            gap_s: 2.0,
        };
        let cuts = reading_cuts(&[b], &[0.0, 100.0, 200.0], &[(0.0, 30.0), (100.0, 130.0)]);
        assert_eq!(cuts, vec![30.0, 100.0, 130.0, 200.0]);
    }

    /// A take keeps WAV and FLAC files, in any case, and a folder holds
    /// sound when it holds one of them.
    #[test]
    fn a_take_keeps_wav_and_flac_sound() {
        for (name, kept) in [
            ("a.WAV", true),
            ("b.flac", true),
            ("c.mp3", false),
            ("wav", false),
        ] {
            assert_eq!(is_sound(Path::new(name)), kept, "{name}");
        }
        let dir = std::env::temp_dir().join(format!("tg-has-sound-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();
        assert!(!has_sound(&dir));
        fs::write(dir.join("take.flac"), "").unwrap();
        assert!(has_sound(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    fn entry(file: &str, clock: Option<&str>, masked: &[[f64; 2]]) -> RecordingEntry {
        RecordingEntry {
            file: file.into(),
            position: Some("DU".into()),
            clock: clock.map(Into::into),
            start_s: Some(10.0),
            settle_s: Some(0.0),
            masked: masked.to_vec(),
            ..Default::default()
        }
    }

    /// A take the app saved, one entry per position over one recording,
    /// is read whole: its recording, its clock log and every entry's
    /// masks, with no stretch or settling.
    #[test]
    fn a_whole_take_keeps_its_recording_clock_log_and_masks() {
        let m = Manifest {
            recordings: vec![
                entry("take.flac", Some("clock.csv"), &[[60.0, 70.0]]),
                entry("take.flac", Some("clock.csv"), &[[600.0, 612.5]]),
            ],
            ..Default::default()
        };
        let e = whole_take(&m).unwrap();
        assert_eq!(e.file, "take.flac");
        assert_eq!(e.clock.as_deref(), Some("clock.csv"));
        assert_eq!(e.masked, vec![[60.0, 70.0], [600.0, 612.5]]);
        assert_eq!((e.start_s, e.end_s, e.settle_s), (None, None, None));
    }

    /// Several recordings are read as the take's folder; masks in them,
    /// or two clock logs, are refused rather than placed wrong.
    #[test]
    fn a_whole_take_of_several_recordings_is_its_folder() {
        let mut m = Manifest {
            recordings: vec![entry("a.flac", None, &[]), entry("b.flac", None, &[])],
            ..Default::default()
        };
        assert_eq!(whole_take(&m).unwrap().file, ".");
        m.recordings[1].masked = vec![[5.0, 6.0]];
        assert!(whole_take(&m).unwrap_err().contains("masks"));
        m.recordings[1].masked.clear();
        m.recordings[0].clock = Some("a.csv".into());
        m.recordings[1].clock = Some("b.csv".into());
        assert!(whole_take(&m).unwrap_err().contains("2 clock logs"));
    }

    /// A kept take read at another lift has its amplitudes in proportion
    /// to the lift (58° on a take kept at 52° reads 58/52 as high), its
    /// lift said as the new one; read at its own lift, or with no lift on
    /// either side, it is as kept.
    #[test]
    fn a_kept_take_read_at_another_lift_scales_its_amplitudes() {
        let s = crate::scenarios::find("control").unwrap();
        let kept = crate::scenarios::beat_log(&s, 1);
        let at = kept.lift_deg + 6.0;
        let read = kept_at_lift(&kept, at);
        assert_eq!(read.lift_deg, at);
        let k = at / kept.lift_deg;
        assert!(k > 1.0);
        for (r, w) in read.amplitude_windows.iter().zip(&kept.amplitude_windows) {
            assert!((r.even_deg.unwrap() - w.even_deg.unwrap() * k).abs() < 1e-9);
            assert!((r.odd_deg.unwrap() - w.odd_deg.unwrap() * k).abs() < 1e-9);
        }
        // No lift to scale from or to: as kept.
        for (log, at) in [
            (kept.clone(), 0.0),
            (
                BeatLog {
                    lift_deg: 0.0,
                    ..kept.clone()
                },
                58.0,
            ),
        ] {
            let read = kept_at_lift(&log, at);
            assert_eq!(read.lift_deg, log.lift_deg);
            assert_eq!(
                read.amplitude_windows[0].even_deg,
                log.amplitude_windows[0].even_deg
            );
        }
        let same = kept_at_lift(&kept, kept.lift_deg);
        assert_eq!(same.lift_deg, kept.lift_deg);
        assert_eq!(
            same.amplitude_windows[0].even_deg,
            kept.amplitude_windows[0].even_deg
        );
    }

    /// A side culled at the kept lift that passes at the new one is
    /// screened with the rest, as a read at that lift screens it: a lone
    /// wrong-sound side stays out (#1333, Norm).
    #[test]
    fn a_side_read_again_at_a_new_lift_is_screened_for_outliers() {
        let w = |i: usize| {
            let mut w = crate::amplitude::AmplitudeWindow {
                start_s: 2.0 * i as f64,
                end_s: 2.0 * i as f64 + 2.0,
                even_deg: Some(318.7),
                odd_deg: Some(345.2),
                beat_error_ms: Some(0.1),
                beat_error_unlock_ms: None,
                even_unlock_ms: Some(-6.5),
                odd_unlock_ms: Some(-6.0),
                even_drop_ms: Some(0.0),
                odd_drop_ms: Some(0.0),
                culled_ms: None,
            };
            if i == 10 {
                // 398° at 52°, culled; 345° at 45° against its
                // neighbours' 276°.
                (w.even_deg, w.even_unlock_ms, w.even_drop_ms) = (None, None, None);
                w.set_culled([Some([-5.2, 0.0]), None]);
            }
            w
        };
        let kept = BeatLog {
            sample_rate: 48000,
            duration_s: 40.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: Vec::new(),
            amplitude_windows: (0..20).map(w).collect(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let read = kept_at_lift(&kept, 45.0);
        let at10 = &read.amplitude_windows[10];
        assert_eq!(at10.even_deg, None, "screened as a lone wrong sound");
        assert_eq!(at10.even_unlock_ms, Some(-5.2), "its edges kept");
        assert!(read.amplitude_windows[9].even_deg.is_some());
    }

    /// A take's own `card_ppm_fast` is its correction, said as given, before
    /// any stored one is looked for.
    #[test]
    fn a_takes_card_ppm_is_its_correction() {
        let m = Manifest {
            card_ppm: Some(-12.5),
            ..Default::default()
        };
        let first = Path::new("no-such-take/audio-001.wav");
        assert_eq!(
            correction_for(&m, first, None, None).unwrap(),
            Some((-12.5, "card_ppm_fast -12.5 in session.toml".to_string()))
        );
        // The reader's own, given or named by its input, comes first.
        assert_eq!(
            correction_for(&m, first, Some(3.0), None).unwrap(),
            Some((3.0, "given with --card-ppm".to_string()))
        );
        assert!(correction_for(&m, first, None, Some("no such device")).is_err());
        let (ppm, from) = correction(first, Some("no such device"), Some(3.0))
            .unwrap()
            .unwrap();
        assert_eq!(ppm, 3.0);
        assert_eq!(from, "given with --card-ppm");
    }

    /// An entry whose stretch starts where it ends has nothing to read;
    /// one with a stretch, or no times at all, has.
    #[test]
    fn an_entry_that_starts_where_it_ends_reads_nothing() {
        let e = |start_s, end_s| RecordingEntry {
            file: ".".into(),
            start_s,
            end_s,
            ..Default::default()
        };
        assert!(reads_nothing(&e(Some(10.0), Some(10.0))));
        assert!(!reads_nothing(&e(Some(10.0), Some(40.0))));
        assert!(!reads_nothing(&e(Some(40.0), None)));
        assert!(!reads_nothing(&e(None, None)));
    }

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
    fn without_a_log_the_usb_lines_come_before_a_known_correction() {
        let fixed = |ppm| ClockFit::fixed(ppm, 600.0).map(300.0);
        let at = |c: &Option<ClockFit>| c.as_ref().map(|c| c.map(300.0));
        // The USB lines set the rate; the correction is kept beside them,
        // and said when they are more than 5 ppm apart (#635).
        let over = clock(None, 48_000, 2, Some(12.0), Some(5.0), 600.0).unwrap();
        assert_eq!(at(&over), Some(fixed(12.0)));
        let over = over.unwrap();
        assert!(over.corrects() && over.withheld.is_none());
        assert_eq!(
            over.sources_disagree(),
            Some(crate::clock::SourcesDisagree {
                usb_ppm: 12.0,
                log_ppm: 5.0,
                log_ppm_sd: 0.0,
                against: crate::clock::Against::Correction,
            })
        );
        let near = clock(None, 48_000, 2, Some(12.0), Some(8.0), 600.0).unwrap();
        assert_eq!(at(&near), Some(fixed(12.0)));
        assert_eq!(near.unwrap().sources_disagree(), None);
        let alone = clock(None, 48_000, 2, Some(12.0), None, 600.0).unwrap();
        assert_eq!(at(&alone), Some(fixed(12.0)));
        assert_eq!(alone.unwrap().sources_disagree(), None);
        let correction = clock(None, 48_000, 2, None, Some(5.0), 600.0).unwrap();
        assert_eq!(at(&correction), Some(fixed(5.0)));
        assert!(clock(None, 48_000, 2, None, None, 600.0).unwrap().is_none());
    }

    #[test]
    fn the_clock_comes_from_the_log_and_usb_lines_else_usb_else_a_correction() {
        use ClockFrom::*;
        assert_eq!(clock_from(true, Some(1.0), Some(2.0)), Some(LogRatedByUsb));
        assert_eq!(clock_from(true, None, Some(2.0)), Some(Log));
        assert_eq!(clock_from(false, Some(1.0), Some(2.0)), Some(Usb));
        assert_eq!(clock_from(false, Some(1.0), None), Some(Usb));
        assert_eq!(clock_from(false, None, Some(2.0)), Some(Correction));
        assert_eq!(clock_from(false, None, None), None);
    }

    /// A clock log is named by its file name, as a session file names it:
    /// never the folders above it, which hold the user's home path.
    #[test]
    fn a_clock_log_is_named_by_its_file_name_alone() {
        let log = Path::new("/Users/someone/Timegrapher Data/take/clock.csv");
        assert_eq!(
            clock_source_text(ClockFrom::Log, None, None, Some(log)),
            "from clock log clock.csv"
        );
        assert_eq!(
            clock_source_text(
                ClockFrom::LogRatedByUsb,
                Some("from 3 USB lines"),
                None,
                Some(log)
            ),
            "from 3 USB lines; clock log clock.csv for its breaks"
        );
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
        // The default settling never takes more than half the position;
        // a settling given is kept, up to the position's end.
        assert_eq!(measured(0.0, 30.0, None, None), (15.0, 30.0));
        assert_eq!(measured(0.0, 600.0, Some(377.0), None), (377.0, 600.0));
        assert_eq!(measured(0.0, 30.0, Some(40.0), None), (30.0, 30.0));
        // Measuring stops at the position's end.
        assert_eq!(measured(0.0, 100.0, Some(0.0), Some(500.0)), (0.0, 100.0));
    }

    /// The window an entry reads: a settling given is kept even past
    /// halfway (a take split at a jump in its sound, read from 20 s past
    /// the jump), and an entry with nothing to read says why.
    #[test]
    fn an_entry_is_read_in_its_window_or_says_why_not() {
        let m: Manifest = toml::from_str(
            "[[recording]]\nfile = \"a.flac\"\nmeasure_s = 337\n\n\
             [[recording]]\nfile = \"a.flac\"\nsettle_s = 377\n\n\
             [[recording]]\nfile = \"b.flac\"\n\n\
             [[recording]]\nfile = \"c.flac\"\nstart_s = 50\nend_s = 50\n\n\
             [[recording]]\nfile = \"d.flac\"\nstart_s = 700\n",
        )
        .unwrap();
        let w =
            |i: usize, given: Option<f64>, d: f64| reading_window(&m.recordings[i], &m, given, d);
        let at = |start_s, stop_s, from_s, to_s| {
            Ok(Window {
                start_s,
                stop_s,
                from_s,
                to_s,
            })
        };
        assert_eq!(w(0, None, 600.0), at(0.0, 600.0, 20.0, 357.0));
        assert_eq!(w(1, None, 600.0), at(0.0, 600.0, 377.0, 600.0));
        // The entry's own settling comes before the reader's.
        assert_eq!(w(1, Some(5.0), 600.0), at(0.0, 600.0, 377.0, 600.0));
        assert_eq!(w(2, Some(5.0), 600.0), at(0.0, 600.0, 5.0, 600.0));
        // A short recording: the default settling stops halfway.
        assert_eq!(w(2, None, 26.0), at(0.0, 26.0, 13.0, 26.0));
        assert_eq!(
            w(1, None, 300.0),
            Err(NoReading::Unsettled {
                file: "a.flac".into(),
                settle_s: 377.0,
                length_s: 300.0
            })
        );
        assert_eq!(w(3, None, 600.0), Err(NoReading::Nothing));
        assert!(matches!(w(4, None, 600.0), Err(NoReading::Outside(_))));
    }

    /// The next file of a recording in the same position carries on the
    /// one before and does not settle again; a new position, a file read
    /// from partway in, one after a file stopped early, one with its own
    /// settling, or the first, does (#1145, #1184).
    #[test]
    fn a_file_carrying_on_its_position_does_not_settle_again() {
        let m: Manifest = toml::from_str(
            "[[recording]]\nfile = \"a.flac\"\nposition = \"DU\"\n\
             [[recording]]\nfile = \"b.flac\"\nposition = \"dial up\"\n\
             [[recording]]\nfile = \"c.flac\"\nposition = \"CU\"\n\
             [[recording]]\nfile = \"d.flac\"\nposition = \"CU\"\nstart_s = 30\n\
             [[recording]]\nfile = \"e.flac\"\nposition = \"CU\"\nend_s = 500\n\
             [[recording]]\nfile = \"f.flac\"\nposition = \"CU\"\n\
             [[recording]]\nfile = \"g.flac\"\nposition = \"CU\"\nsettle_s = 40\n\
             [[recording]]\nfile = \"h.flac\"\nposition = \"CU\"\nsettle_s = 0\n",
        )
        .unwrap();
        let carries: Vec<bool> = m
            .recordings
            .iter()
            .map(|e| continues_position(e, &m))
            .collect();
        // g gives its own settling, so the watch was handled there: it does
        // not carry f on; h's settling of none still carries g on (#1184).
        assert_eq!(
            carries,
            [false, true, false, false, true, false, false, true]
        );
        let from = |i: usize| {
            reading_window(&m.recordings[i], &m, None, 600.0)
                .unwrap()
                .from_s
        };
        assert_eq!(from(0), SETTLE_S);
        assert_eq!(from(1), 0.0);
        assert_eq!(from(2), SETTLE_S);
        // Its own settling still stands.
        assert_eq!(from(6), 40.0);
        // A settling the reader gives (`session --settle`) settles each
        // new position, not a file that carries one on.
        let given = |i: usize| {
            reading_window(&m.recordings[i], &m, Some(30.0), 600.0)
                .unwrap()
                .from_s
        };
        assert_eq!(
            (given(0), given(1), given(2), given(5)),
            (30.0, 0.0, 30.0, 30.0)
        );
        assert_eq!(given(6), 40.0);
        // A take whose session file lists its whole folder first, then its
        // first file (as dd3235_DU_30min does): that file still settles.
        let m: Manifest = toml::from_str(
            "[[recording]]\nfile = \".\"\nposition = \"DU\"\n\
             [[recording]]\nfile = \"001.flac\"\nposition = \"DU\"\n\
             [[recording]]\nfile = \"002.flac\"\nposition = \"DU\"\n",
        )
        .unwrap();
        let carries: Vec<bool> = m
            .recordings
            .iter()
            .map(|e| continues_position(e, &m))
            .collect();
        assert_eq!(carries, [false, false, true]);
        let from = |i: usize| {
            reading_window(&m.recordings[i], &m, None, 600.0)
                .unwrap()
                .from_s
        };
        assert_eq!((from(1), from(2)), (SETTLE_S, 0.0));
    }

    /// A stretch that starts partway into its recording settles halfway
    /// through its own length, not the recording's, and an unsettled
    /// entry gives its stretch's length; each reason for no reading says
    /// itself in words (#694).
    #[test]
    fn a_stretch_partway_in_is_measured_on_its_own_length() {
        assert_eq!(measured(100.0, 110.0, None, None), (105.0, 110.0));
        let m: Manifest =
            toml::from_str("[[recording]]\nfile = \"a.flac\"\nstart_s = 100\nsettle_s = 50\n")
                .unwrap();
        let no = reading_window(&m.recordings[0], &m, None, 130.0).unwrap_err();
        assert_eq!(
            no,
            NoReading::Unsettled {
                file: "a.flac".into(),
                settle_s: 50.0,
                length_s: 30.0
            }
        );
        assert_eq!(
            no.to_string(),
            "a.flac: a settle_s of 50 s leaves nothing of its 30 s stretch to read; \
             give it a shorter settle_s"
        );
        assert_eq!(
            NoReading::Nothing.to_string(),
            "nothing to read: it settles right to its end"
        );
        assert_eq!(NoReading::Outside("out".into()).to_string(), "out");
        assert_eq!(
            EntryError::NoReading(NoReading::Nothing).to_string(),
            NoReading::Nothing.to_string()
        );
        assert_eq!(EntryError::Failed("bad".into()).to_string(), "bad");
        assert_eq!(String::from(EntryError::from("bad".to_string())), "bad");
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

    /// Why the lift is the default: no calibre, a calibre without one, or
    /// a calibre whose lift is outside the range; nothing when it has one.
    #[test]
    fn the_lift_defaults_with_its_reason() {
        let cal = |lift, aside| Calibre {
            name: "Test".into(),
            bph: None,
            lift_angle_deg: lift,
            lift_set_aside_deg: aside,
            escape_teeth: None,
            wheels: Vec::new(),
            meshes: Vec::new(),
        };
        assert_eq!(lift_default_why(None).unwrap(), "calibre not set");
        assert_eq!(
            lift_default_why(Some(&cal(None, None))).unwrap(),
            "the calibre's lift angle is not in the table"
        );
        assert_eq!(
            lift_default_why(Some(&cal(None, Some(120.0)))).unwrap(),
            "the calibre's lift angle of 120° is set aside, outside 10 to 90°"
        );
        assert_eq!(lift_default_why(Some(&cal(Some(44.0), None))), None);
    }

    #[test]
    fn settings_come_from_the_reader_then_the_session_file_then_the_calibre() {
        let cal = Calibre {
            name: "Test".into(),
            bph: Some(21_600),
            lift_angle_deg: Some(44.0),
            lift_set_aside_deg: None,
            escape_teeth: Some(20),
            wheels: Vec::new(),
            meshes: Vec::new(),
        };
        let bare: Manifest = toml::from_str("").unwrap();
        let none = stream_settings(&bare, None, Given::default());
        assert_eq!(none.analysis.bph, None);
        assert_eq!(none.analysis.amplitude.lift_deg, DEFAULT_LIFT_DEG);
        assert!(!none.analysis.lift_known);
        assert_eq!(none.analysis.escape_teeth, ESCAPE_TEETH);

        let from_cal = stream_settings(&bare, Some(&cal), Given::default());
        assert_eq!(from_cal.analysis.bph, Some(21_600));
        assert_eq!(from_cal.analysis.amplitude.lift_deg, 44.0);
        assert!(from_cal.analysis.lift_known);
        assert_eq!(from_cal.analysis.escape_teeth, 20);

        let file: Manifest =
            toml::from_str("bph = 18000\nlift = 50\nlift_known = false\nescape_teeth = 15")
                .unwrap();
        let from_file = stream_settings(&file, Some(&cal), Given::default());
        assert_eq!(from_file.analysis.bph, Some(18_000));
        assert_eq!(from_file.analysis.amplitude.lift_deg, 50.0);
        assert!(!from_file.analysis.lift_known);
        assert_eq!(from_file.analysis.escape_teeth, 15);

        let given = Given {
            bph: Some(28_800),
            lift_deg: Some(53.0),
        };
        let from_reader = stream_settings(&file, Some(&cal), given);
        assert_eq!(from_reader.analysis.bph, Some(28_800));
        assert_eq!(from_reader.analysis.amplitude.lift_deg, 53.0);
        assert!(from_reader.analysis.lift_known);
    }

    /// An older session file, with no `lift_known`, gave its lift; it was
    /// assumed when it is the default and the calibre gives none (#685).
    #[test]
    fn an_older_files_default_lift_is_assumed_unless_its_calibre_gives_one() {
        let cal = |lift| Calibre {
            name: "Test".into(),
            bph: None,
            lift_angle_deg: lift,
            lift_set_aside_deg: None,
            escape_teeth: None,
            wheels: Vec::new(),
            meshes: Vec::new(),
        };
        let file = |text: &str| -> Manifest { toml::from_str(text).unwrap() };
        let known = |m: &Manifest, c: Option<&Calibre>| {
            stream_settings(m, c, Given::default()).analysis.lift_known
        };
        let bare_52 = file("lift = 52");
        assert_eq!(file_lift_known(&bare_52, None), Some(false));
        assert!(!known(&bare_52, None));
        assert!(!known(&bare_52, Some(&cal(None))));
        assert!(known(&bare_52, Some(&cal(Some(52.0)))));
        let bare_50 = file("lift = 50");
        assert!(known(&bare_50, None));
        // A marker says it, whatever the value.
        assert!(known(&file("lift = 52\nlift_known = true"), None));
        assert!(!known(&file("lift = 50\nlift_known = false"), None));
        assert_eq!(file_lift_known(&file(""), None), None);
    }

    /// Opening an entry reads it as measuring it does, and tells how far
    /// through its sound the beats have been found.
    #[test]
    fn an_opened_entry_is_the_entry_measure_entry_reads() {
        use crate::synth::{generate, SynthConfig};
        let dir = std::env::temp_dir().join(format!("tg-open-entry-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = SynthConfig {
            duration_s: 40.0,
            ..SynthConfig::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 5.0);
        crate::audio::write_wav(&dir.join("take.wav"), &audio).unwrap();
        let m: Manifest = toml::from_str(
            "bph = 28800\nlift = 52\n\n[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\nmasked = [[30.0, 32.0]]\n",
        )
        .unwrap();
        let e = &m.recordings[0];
        let s = EntrySettings {
            stream: stream_settings(&m, None, Given::default()),
            card_clock: false,
            settle_s: Some(5.0),
        };
        let mut told = Vec::new();
        let opened = open_entry(
            &dir,
            &m,
            e,
            &s,
            |_| Ok(None),
            |_| Ok(Some((20.0, "stored for 'test'".to_string()))),
            |done, total| told.push((done, total)),
        )
        .unwrap();
        let read = measure_entry(
            &dir,
            &m,
            e,
            &s,
            |_| Ok(None),
            |_| Ok(Some((20.0, "stored for 'test'".to_string()))),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&opened.log).unwrap(),
            serde_json::to_value(&read.log).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&opened.breaks).unwrap(),
            serde_json::to_value(&read.breaks).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&opened.clock).unwrap(),
            serde_json::to_value(&read.clock).unwrap()
        );
        assert_eq!(opened.files, read.files);
        assert_eq!(opened.clock_from, read.clock_from);
        assert_eq!(opened.clock_ppm, read.clock_ppm);
        assert_eq!(opened.clock_source, read.clock_source);
        // With no clock log, the known correction is the clock, said once.
        assert_eq!(opened.clock_from, Some(ClockFrom::Correction));
        assert_eq!(opened.clock_ppm, Some(20.0));
        assert_eq!(opened.clock_source.as_deref(), Some("stored for 'test'"));
        assert_eq!(opened.clock_text, read.clock_text);
        assert_eq!(opened.masked, read.masked);
        assert_eq!(opened.masked, vec![(30.0, 32.0)]);
        assert!(opened.kept.is_none() && opened.clock.is_some());
        // Seconds read, rising to all the sound there is.
        assert!(!told.is_empty() && told.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(told.iter().all(|(done, total)| done <= total), "{told:?}");
        let (done, total) = *told.last().unwrap();
        assert_eq!(done, total);
        assert!((total - read.log.duration_s).abs() < 1e-6, "{told:?}");
        // The whole length is known from the first word on.
        assert!(
            told.iter()
                .all(|&(_, total)| (total - read.log.duration_s).abs() < 1e-6),
            "{told:?}"
        );
        // Read on the sound card's own clock, the correction is not used.
        let on_card = EntrySettings {
            card_clock: true,
            ..s
        };
        let card = open_entry(
            &dir,
            &m,
            e,
            &on_card,
            |_| Ok(None),
            |_| Ok(Some((20.0, "stored for 'test'".to_string()))),
            |_, _| {},
        )
        .unwrap();
        assert!(
            card.clock.is_none() && card.clock_from.is_none(),
            "{:?}",
            card.clock_from
        );
        assert_eq!(card.clock_ppm, None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Loose sound files open as a take of them opens (#508): the same
    /// beats, clock and breaks, on the clock log given, which makes a
    /// correction unneeded.
    #[test]
    fn loose_files_open_as_the_take_of_them_opens() {
        use crate::synth::{generate, SynthConfig};
        let dir = std::env::temp_dir().join(format!("tg-open-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = SynthConfig {
            duration_s: 40.0,
            ..SynthConfig::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 5.0);
        let half = audio.samples.len() / 2;
        for (name, part) in [
            ("audio-001.wav", &audio.samples[..half]),
            ("audio-002.wav", &audio.samples[half..]),
        ] {
            let a = crate::audio::Audio {
                samples: part.to_vec(),
                sample_rate: audio.sample_rate,
            };
            crate::audio::write_wav(&dir.join(name), &a).unwrap();
        }
        // A line every 4 s, the card 20 ppm slow.
        let mut text = String::from("unix_s,frames\n");
        for k in 0..=10 {
            let audio_s = k as f64 * 4.0;
            let frames = (audio_s * f64::from(audio.sample_rate)) as u64;
            text.push_str(&format!("{},{frames}\n", 1.7e9 + audio_s * (1.0 + 20e-6)));
        }
        let log = dir.join("clock.csv");
        fs::write(&log, &text).unwrap();
        let m: Manifest =
            toml::from_str("bph = 28800\n\n[[recording]]\nfile = \".\"\nclock = \"clock.csv\"\n")
                .unwrap();
        let s = EntrySettings {
            stream: stream_settings(&m, None, Given::default()),
            card_clock: false,
            settle_s: None,
        };
        let no_correction = |_: &Path| -> Result<Option<(f64, String)>, String> {
            panic!("a clock log makes a correction unneeded")
        };
        let take = open_entry(
            &dir,
            &m,
            &m.recordings[0],
            &s,
            |_| Ok(None),
            no_correction,
            |_, _| {},
        )
        .unwrap();
        let files = [dir.join("audio-001.wav"), dir.join("audio-002.wav")];
        let loose = open_files(
            &files,
            Some(&log),
            &s,
            |_| Ok(None),
            no_correction,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&loose.log).unwrap(),
            serde_json::to_value(&take.log).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&loose.breaks).unwrap(),
            serde_json::to_value(&take.breaks).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&loose.clock).unwrap(),
            serde_json::to_value(&take.clock).unwrap()
        );
        assert_eq!(loose.files, take.files);
        assert_eq!(loose.clock_from, Some(ClockFrom::Log));
        assert_eq!(loose.clock_source, take.clock_source);
        assert_eq!(
            loose.clock_source.as_deref(),
            Some("from clock log clock.csv")
        );
        assert!(loose
            .clock
            .as_ref()
            .is_some_and(|c| (c.ppm + 20.0).abs() < 0.5));
        assert!(loose.masked.is_empty() && loose.kept.is_none());
        // With no files there is nothing to open.
        assert!(open_files(&[], None, &s, |_| Ok(None), |_| Ok(None), |_, _| {}).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A take kept as its analysis alone lists its breaks in its session
    /// file; a step in its clock log that no listed break explains is a
    /// break too, as it is for a take read from its sound.
    #[test]
    fn a_kept_takes_clock_step_is_a_break_beside_its_listed_ones() {
        // A line every 10 s, true time 10 s ahead from 60 s of sound on.
        let mut text = String::from("unix_s,frames\n");
        for k in 0..=12 {
            let audio_s = k as f64 * 10.0;
            let unix_s = 1.7e9 + audio_s + if audio_s >= 60.0 { 10.0 } else { 0.0 };
            text.push_str(&format!("{unix_s},{}\n", (audio_s * 48_000.0) as u64));
        }
        let mut fit = clock(Some(&text), 48_000, 4, None, None, 120.0)
            .unwrap()
            .unwrap();
        assert_eq!(fit.breaks.len(), 1, "{:?}", fit.breaks);
        let json = serde_json::json!({ "breaks": [{ "audio_s": 30.0, "gap_s": 0.0 }] });
        let mut log = BeatLog {
            sample_rate: 48_000,
            duration_s: 120.0,
            bph: 28_800,
            lift_deg: 52.0,
            beats: Vec::new(),
            amplitude_windows: Vec::new(),
            clipped_beats: Vec::new(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        };
        let found = breaks(&mut log, Listed::Session(Ok(Some(&json))), Some(&mut fit));
        let at: Vec<f64> = found.breaks.iter().map(|b| b.audio_s).collect();
        assert_eq!(at.len(), 2, "{at:?}");
        assert!(
            (at[0] - 30.0).abs() < 1e-9 && (at[1] - 60.0).abs() < 1e-9,
            "{at:?}"
        );
    }

    /// A pause the take lists that its clock log spans and shows no gap
    /// at does not number the beats on: on true time they run on along
    /// one line, as the readings and the Paper Strip read them (#646).
    #[test]
    fn a_pause_the_clock_log_did_not_see_does_not_number_the_beats_on() {
        let mut text = String::from("unix_s,frames\n");
        for k in 0..=12 {
            let audio_s = k as f64 * 10.0;
            text.push_str(&format!(
                "{},{}\n",
                1.7e9 + audio_s,
                (audio_s * 48_000.0) as u64
            ));
        }
        let p = 0.125;
        for (listed_on, gap_s) in [(true, 30.0), (false, 0.0)] {
            let mut fit = clock(Some(&text), 48_000, 4, None, None, 120.0)
                .unwrap()
                .unwrap();
            let json = serde_json::json!({ "breaks": [{ "audio_s": 60.0, "gap_s": gap_s }] });
            let mut log = BeatLog {
                sample_rate: 48_000,
                duration_s: 120.0,
                bph: 28_800,
                lift_deg: 52.0,
                beats: (0..960)
                    .map(|index| crate::beats::Beat {
                        index,
                        time: index as f64 * p + 0.01,
                        quality: 1.0,
                    })
                    .collect(),
                amplitude_windows: Vec::new(),
                clipped_beats: Vec::new(),
                clipped_samples: 0,
                jumps: Vec::new(),
                sided: Vec::new(),
                by_count: Vec::new(),
                snippets: Vec::new(),
            };
            let found = breaks(&mut log, Listed::Session(Ok(Some(&json))), Some(&mut fit));
            let lead = |b: &crate::beats::Beat| {
                crate::clean::true_time(Some(&fit), &found.breaks, b.time) - b.index as f64 * p
            };
            let first = lead(&log.beats[0]);
            for b in &log.beats {
                assert!(
                    (lead(b) - first).abs() < 1e-3,
                    "listed {listed_on}: beat {} at {} leads {}",
                    b.index,
                    b.time,
                    lead(b) - first
                );
            }
        }
    }

    /// The words for where a take's clock came from, for each source; a
    /// clock log is named by its file name alone (#669).
    #[test]
    fn clock_source_text_words_each_source() {
        let usb = Some("USB lines, +12.0 ppm");
        let stored = Some("stored for 'USB', 2026-10-10T03:00:00Z");
        let log = Some(Path::new("take/clock.csv"));
        let text = |from| clock_source_text(from, usb, stored, log);
        assert_eq!(
            text(ClockFrom::LogRatedByUsb),
            "USB lines, +12.0 ppm; clock log clock.csv for its breaks"
        );
        assert_eq!(text(ClockFrom::Log), "from clock log clock.csv");
        assert_eq!(
            text(ClockFrom::Correction),
            "stored for 'USB', 2026-10-10T03:00:00Z"
        );
        assert_eq!(text(ClockFrom::Usb), "USB lines, +12.0 ppm");
    }

    /// A calibre named from the table keeps its figures and the table's
    /// order of wheels; a name in neither the table nor the user's
    /// calibres finds nothing.
    #[test]
    fn a_table_calibre_keeps_its_figures_and_wheel_order() {
        let user = crate::calibre_store::UserCalibres::default();
        let c = calibre_named("ETA 2824-2", &user).unwrap();
        assert_eq!(c.name, "ETA 2824-2");
        assert_eq!(
            (c.bph, c.lift_angle_deg, c.escape_teeth),
            (Some(28800), Some(50.0), Some(20))
        );
        let table = crate::calibres::find("ETA 2824-2").unwrap();
        let named: Vec<&str> = c.wheels.iter().map(|w| w.name.as_str()).collect();
        let listed: Vec<&str> = table.wheels.iter().map(|w| w.name.as_str()).collect();
        assert!(listed.len() >= 2, "{listed:?}");
        assert_eq!(&named[..listed.len()], &listed[..]);
        assert_eq!(c.wheels[0].name, "barrel");
        assert!((c.wheels[0].period_s - 21114.027).abs() < 0.01);
        assert!(calibre_named("No Such Calibre 9999", &user).is_none());
    }
}
