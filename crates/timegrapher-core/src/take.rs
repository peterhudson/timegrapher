//! A take opened once, the same way for every surface: the clock its
//! readings are put on, its breaks, and the stretch of each recorded
//! position that is read. The app, the command line and the MCP server
//! call these rather than putting the take together from the pieces in
//! [`crate::clock`], [`crate::breaks`] and [`crate::session`], so a take
//! reads the same wherever it is opened.

use crate::analysis_take;
use crate::breaks::{self, Break};
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

/// Where a take's clock came from, in words, as `from` ([`clock_from`])
/// picks it: the USB lines' text (`usb`, as
/// [`crate::usbclock::UsbClock::correction`] words it), the correction's
/// (`correction`, such as [`crate::clockstore::DeviceClock::source_text`])
/// and the clock log's path (`log`).
pub fn clock_source_text(
    from: ClockFrom,
    usb: Option<&str>,
    correction: Option<&str>,
    log: Option<&Path>,
) -> String {
    let log = || log.map(|p| p.display().to_string()).unwrap_or_default();
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
/// the order they were recorded ([`crate::recording_order::recordings_in`]).
pub fn sound_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for path in paths {
        let listed = if path.is_dir() {
            fs::read_to_string(path.join(MANIFEST))
                .ok()
                .and_then(|text| toml::from_str::<Manifest>(&text).ok())
                .map(|manifest| {
                    manifest
                        .recordings
                        .into_iter()
                        .map(|entry| entry.file)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        out.extend(crate::recording_order::recordings_in(path, &listed)?);
    }
    Ok(out)
}

/// A kept take's beat log with its amplitudes at `lift_deg`, the lift this
/// reading uses, where the analysis was kept at another: amplitude scales
/// with the lift, as the app's lift change scales it.
pub fn kept_at_lift(log: &BeatLog, lift_deg: f64) -> BeatLog {
    let mut log = log.clone();
    if lift_deg > 0.0 && log.lift_deg > 0.0 && lift_deg != log.lift_deg {
        let k = lift_deg / log.lift_deg;
        for w in &mut log.amplitude_windows {
            w.even_deg = w.even_deg.map(|a| a * k);
            w.odd_deg = w.odd_deg.map(|a| a * k);
        }
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
    pub escape_teeth: Option<u32>,
    pub wheels: Vec<crate::periodicity::Wheel>,
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
        escape_teeth: entry.escape_teeth(),
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
///   gives it (unless it says `lift_known = false`), or when the calibre
///   does;
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
    } else if m.lift.is_some() {
        m.lift_known.unwrap_or(true)
    } else {
        cal.is_some_and(|c| c.lift_angle_deg.is_some())
    };
    cfg.analysis.escape_teeth = m
        .escape_teeth
        .or(cal.and_then(|c| c.escape_teeth))
        .unwrap_or(ESCAPE_TEETH);
    cfg
}

/// The correction stored for the input `file` was recorded on
/// ([`crate::clockstore::recorded_device`]), from the user's store; `None`
/// when the recording names no input or none is stored for it.
pub fn stored_correction(file: &Path) -> Result<Option<crate::clockstore::DeviceClock>, String> {
    let store = crate::clockstore::ClockStore::load()?;
    Ok(crate::clockstore::recorded_device(file).and_then(|d| store.get(&d).cloned()))
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
    /// The clock log's text, when the entry has one.
    pub clock_text: Option<String>,
    pub breaks: Vec<Break>,
    /// The stretches masked by hand, start and end.
    pub masked: Vec<(f64, f64)>,
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
    /// The clock log's text, when the entry has one.
    pub clock_text: Option<String>,
    pub breaks: Vec<Break>,
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

/// Open entry `e` of the take described by `m` in the folder `base`, as
/// every surface reads a take: its beats (found in its sound, or kept as
/// its analysis alone), its clock and breaks ([`clock`], [`breaks`]) and
/// its masks. `usb` and `correction` are as [`measure_entry`] takes them.
/// `progress` is told how far through the sound the beats have been
/// found, from 0 to 1; a take kept as its analysis alone has no sound to
/// go through.
pub fn open_entry(
    base: &Path,
    m: &Manifest,
    e: &RecordingEntry,
    s: &EntrySettings,
    usb: impl FnOnce(&[PathBuf]) -> Result<Option<f64>, String>,
    correction: impl FnOnce(&Path) -> Result<Option<f64>, String>,
    progress: impl FnMut(f64),
) -> Result<OpenedEntry, String> {
    let path = base.join(&e.file);
    // A take kept as its analysis alone: its beats are read, not found.
    let kept = if analysis_take::is_analysis_only(&path) {
        if m.audio != Some(false) {
            return Err(format!(
                "{}: it has an {}/ folder, but {MANIFEST} doesn't say audio = false",
                e.file,
                analysis_take::DIR
            ));
        }
        if has_sound(&path) {
            return Err(format!(
                "{}: it holds both sound and an {}/ folder: a take is kept either with its \
                 sound or as its analysis alone",
                e.file,
                analysis_take::DIR
            ));
        }
        Some(analysis_take::read(&path).map_err(|err| format!("{}: {err}", e.file))?)
    } else if m.audio == Some(false) {
        return Err(format!(
            "{}: {MANIFEST} says audio = false, but there is no {}/ folder to read",
            e.file,
            analysis_take::DIR
        ));
    } else {
        None
    };
    let files = match &kept {
        Some(_) => Vec::new(),
        None => sound_files(std::slice::from_ref(&path))?,
    };
    let cfg = &s.stream;
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let mut log = match &kept {
        Some(k) => kept_at_lift(&k.log, cfg.analysis.amplitude.lift_deg),
        None => stream::analyze_files(&paths, cfg, progress)
            .map_err(|err| format!("{}: {err}", e.file))?,
    };
    let clock_text = match &e.clock {
        Some(p) => {
            let f = base.join(p);
            Some(fs::read_to_string(&f).map_err(|err| format!("{}: {err}", f.display()))?)
        }
        None => None,
    };
    let named = |err: String| match &e.clock {
        Some(p) => format!("{}: {err}", base.join(p).display()),
        None => err,
    };
    let has_log = clock_text.is_some();
    let (mut fit, clock_from) = match &kept {
        _ if s.card_clock => (None, None),
        // Read as it was recorded: its sound is not here.
        Some(k) => {
            let usb_ppm = k.header.usb_clock.map(|u| u.ppm);
            let fit = clock(
                clock_text.as_deref(),
                k.header.sample_rate,
                k.header.bytes_per_frame,
                usb_ppm,
                m.card_ppm,
                log.duration_s,
            )
            .map_err(named)?;
            (fit, clock_from(has_log, usb_ppm, m.card_ppm))
        }
        None => {
            let info = crate::audio::info(&files[0])
                .map_err(|err| format!("{}: {err}", files[0].display()))?;
            let correction = match &clock_text {
                Some(_) => None,
                None => correction(&files[0])?,
            };
            let usb_ppm = usb(&files)?;
            let fit = clock(
                clock_text.as_deref(),
                info.sample_rate,
                info.bytes_per_frame,
                usb_ppm,
                correction,
                log.duration_s,
            )
            .map_err(named)?;
            (fit, clock_from(has_log, usb_ppm, correction))
        }
    };
    let clock_from = clock_from.filter(|_| fit.is_some());
    // A take kept as its analysis alone: every part of the take was in
    // it, so its session.json lists its breaks for the whole take's sound.
    let json = kept.as_ref().and_then(|_| {
        fs::read_to_string(path.join(RECORD))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    });
    let listed = match &kept {
        Some(_) => Listed::Session(json.as_ref()),
        None => Listed::Files(&paths),
    };
    let breaks = breaks(&mut log, listed, fit.as_mut());
    // Stretches masked by hand are left out of everything read below.
    let masked = session::masked_spans(&e.masked);
    session::mask_log(&mut log, &masked);
    Ok(OpenedEntry {
        files,
        kept,
        log,
        clock: fit,
        clock_from,
        clock_text,
        breaks,
        masked,
    })
}

/// Read entry `e` of the take described by `m` in the folder `base`, as
/// every surface reads a take: its beats (found in its sound, or kept as
/// its analysis alone), its clock and breaks ([`clock`], [`breaks`]),
/// its masks, and the stretch measured once the watch has settled
/// ([`measured`]). `usb` gives the sound card's error from the USB lines
/// in the files, and `correction` the correction known for the input of
/// the first file, each ppm slow: they only fetch, and the rule that
/// picks between them and the clock log is this function's ([`clock`]).
/// `correction` is asked only when the entry has no clock log. An error
/// either gives is passed through as its text, which is the caller's.
pub fn measure_entry(
    base: &Path,
    m: &Manifest,
    e: &RecordingEntry,
    s: &EntrySettings,
    usb: impl FnOnce(&[PathBuf]) -> Result<Option<f64>, String>,
    correction: impl FnOnce(&Path) -> Result<Option<f64>, String>,
) -> Result<EntryRead, String> {
    let OpenedEntry {
        files,
        kept,
        log,
        clock: fit,
        clock_from,
        clock_text,
        breaks,
        masked,
    } = open_entry(base, m, e, s, usb, correction, |_| {})?;
    let cfg = &s.stream;
    let (start_s, stop_s) = span_of(e, log.duration_s)?;
    let (settle, measure) = entry_times(e, m);
    let (settle_s, end_s) = measured(
        start_s,
        stop_s,
        e.settle_s.or(s.settle_s).or(settle),
        measure,
    );
    let mut measurement =
        session::measure_masked(&log, fit.as_ref(), &breaks, &masked, settle_s, end_s);
    if !cfg.analysis.lift_known {
        measurement.lift_unknown();
    }
    Ok(EntryRead {
        files,
        kept,
        log,
        clock: fit,
        clock_from,
        clock_text,
        breaks,
        masked,
        start_s,
        stop_s,
        settle_s,
        end_s,
        measurement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn settings_come_from_the_reader_then_the_session_file_then_the_calibre() {
        let cal = Calibre {
            name: "Test".into(),
            bph: Some(21_600),
            lift_angle_deg: Some(44.0),
            escape_teeth: Some(20),
            wheels: Vec::new(),
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
            |_| Ok(Some(20.0)),
            |f| told.push(f),
        )
        .unwrap();
        let read = measure_entry(&dir, &m, e, &s, |_| Ok(None), |_| Ok(Some(20.0))).unwrap();
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
        assert_eq!(opened.clock_text, read.clock_text);
        assert_eq!(opened.masked, read.masked);
        assert_eq!(opened.masked, vec![(30.0, 32.0)]);
        assert!(opened.kept.is_none() && opened.clock.is_some());
        assert!(!told.is_empty() && told.windows(2).all(|w| w[0] <= w[1]));
        assert!(*told.last().unwrap() > 0.9, "{told:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
