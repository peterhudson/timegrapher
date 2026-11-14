//! `timegrapher record`: record one position of a test, for as long as
//! asked, into a session folder, and add it to the folder's session file.
//!
//! Each run is one take: a folder made by [`Recorder`] exactly as the
//! desktop app makes it (`audio-001.flac`, `clock.csv`, `session.json`),
//! with the app's `session.toml` inside it, so the app opens it with its
//! settings. The take is then listed in the session folder's own
//! `session.toml` with its position and state of wind, and `timegrapher
//! session FOLDER` reads them all together. The microphone is open only
//! while the command runs, for the time it says it will.
//!
//! Without a FOLDER, `--watch NAME` names a watch in the app's library
//! (`timegrapher_core::library`): the session folder is made in that
//! watch's `takes/`, named by the local date and time it started (with
//! no position, as more can be added later), and the watch's calibre,
//! beat rate and lift angle fill in what isn't given. Recording into the library holds its lock, so it is refused
//! while the app has the library open, as a second copy of the app
//! would be.
//!
//! With `--power-reserve` the run is a Power Reserve Test: it records in
//! one position until the watch stops (the last beat heard before ten
//! minutes with none, core `power_reserve`) or 80 hours pass, then keeps
//! the run's results, hour by hour, in the session folder's
//! `power-reserve.json`, as the app's Power Reserve pane does. In the
//! library the test is one take, as the app files it (#1732): the
//! recording itself, in a folder named by the date and time and "Power
//! Reserve" ([`library::power_reserve_take_dir`]), with its own session
//! file and `power-reserve.json`.

use crate::long::Train;
use crate::output;
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use timegrapher_core::analysis::Clipping;
use timegrapher_core::audio;
use timegrapher_core::beats;
use timegrapher_core::breaks;
use timegrapher_core::calibre_store::{self, UserCalibres};
#[cfg(test)]
use timegrapher_core::calibres;
use timegrapher_core::capture::Block;
use timegrapher_core::library::{self, Library};
use timegrapher_core::library::{base_digits, fixed_text, signed_text};
use timegrapher_core::live::{LiveAnalyzer, LiveConfig};
use timegrapher_core::power_reserve;
use timegrapher_core::recorder::{self, Recorder, SessionInfo};
use timegrapher_core::session::{self, Manifest, Position};
use timegrapher_core::take::{self, MANIFEST};
use timegrapher_core::timing::Anchor;

#[derive(Clone)]
pub struct Options {
    /// The session folder; `None` to make one in the library watch
    /// `watch` names.
    pub folder: Option<PathBuf>,
    /// The library, for a watch named without a FOLDER (default:
    /// Timegrapher Data in the home folder).
    pub library: Option<PathBuf>,
    pub position: String,
    pub seconds: f64,
    pub device: Option<String>,
    pub file: Option<PathBuf>,
    pub watch: Option<String>,
    pub calibre: Option<String>,
    pub bph: Option<u32>,
    pub lift: Option<f64>,
    /// Hours since full wind; left out of the session file when not given.
    pub wind_h: Option<f64>,
    pub json: bool,
    /// Keep the latest readings here while recording (`timegrapher.live/1`).
    pub live: Option<PathBuf>,
    /// Run until the watch stops: a Power Reserve Test.
    pub power_reserve: bool,
    /// End the recording early once this file exists, as the app's Stop
    /// does: another program's way to stop it.
    pub stop_file: Option<PathBuf>,
}

/// A quick look at the take: the stretch `session` will read, after the
/// settling time, with the facts over the whole take.
#[derive(Serialize)]
struct Check {
    /// The stretch read, seconds from the start of the take.
    from_s: f64,
    to_s: f64,
    /// Whether a regular beat is heard over the stretch read (over the
    /// whole take when nothing is read).
    watch_heard: bool,
    /// The median timing scatter of the beats over the stretch read (over
    /// the whole take when it gives none), and that as a share of the
    /// beat: 2% or more is no regular beat.
    timing_scatter_ms: Option<f64>,
    timing_scatter_fraction: Option<f64>,
    bph: u32,
    bph_set: Option<u32>,
    beats_found: usize,
    beats_expected: usize,
    rate_s_per_day: Option<f64>,
    amplitude_deg: Option<f64>,
    beat_error_ms: Option<f64>,
    clipping: Clipping,
}

#[derive(Serialize)]
struct Report {
    /// The take's folder.
    take: String,
    /// The library watch the session folder was made under, when no
    /// FOLDER was given.
    library_watch: Option<String>,
    /// The session file the take was added to.
    session_file: String,
    position: &'static str,
    wind_h: Option<f64>,
    seconds: f64,
    /// The recording was ended early by `--stop-file`.
    stopped_early: bool,
    source: String,
    sample_rate: u32,
    check: Check,
    /// A Power Reserve Test's run, as `power-reserve.json` keeps it.
    #[serde(skip_serializing_if = "Option::is_none")]
    power_reserve: Option<power_reserve::Results>,
}

/// The session settings: what the folder's session file says, filled in
/// from the options and the calibre table.
pub(crate) struct Settings {
    pub(crate) watch: Option<String>,
    pub(crate) calibre: Option<String>,
    pub(crate) bph: Option<u32>,
    pub(crate) lift: f64,
    pub(crate) lift_known: bool,
    /// Where the lift angle came from (#1648).
    pub(crate) lift_from: take::SettingFrom,
    /// Seconds `session` skips at the start of each take: the session
    /// file's, none when it gives none ([`take::measured`]'s default).
    pub(crate) settle_s: Option<f64>,
}

impl Options {
    /// The session folder, once [`place`] has chosen it.
    fn dir(&self) -> &Path {
        self.folder.as_deref().unwrap_or(Path::new("."))
    }
}

/// Where the session folder is, and the library's lock while it is in a
/// library.
struct Placed {
    /// Held until the command ends.
    _library: Option<Library>,
    /// The watch it was made under, without a FOLDER.
    watch: Option<String>,
    /// A Power Reserve Test filed into a library: the local minute it
    /// started, which names its take. The take is then one folder, as
    /// the app keeps one ([`library::power_reserve_take_dir`], #1732),
    /// and the session folder is the watch's `takes/`.
    reserve_take: Option<String>,
}

/// The library holding `folder` as one of a watch's takes, if it is one:
/// where it really is, so a link to the take, its watch or the library
/// holds the library's lock too (#1641).
fn library_of(folder: &Path) -> Option<PathBuf> {
    let abs = library::resolved(&std::path::absolute(folder).ok()?);
    let takes = abs.parent()?;
    let root = takes.parent()?.parent()?;
    (takes.file_name()? == library::TAKES && Library::is_library(root)).then(|| root.to_path_buf())
}

/// The library at `root`, held: an error saying why where it can only be
/// read (the app has it open).
fn hold(root: &Path) -> Result<Library, String> {
    let lib = Library::open(root)?;
    if let Some(why) = &lib.read_only {
        return Err(format!(
            "{}: {why}; quit the app (or close the library) first, or record into a folder \
             outside the library",
            root.display()
        ));
    }
    Ok(lib)
}

/// Choose the session folder: FOLDER as given (holding the library's lock
/// when it is a take in one), or a new one in the library watch `--watch`
/// names, whose settings fill in the options not given.
fn place(o: &mut Options) -> Result<Placed, String> {
    if let Some(f) = &o.folder {
        let held = library_of(f).map(|root| hold(&root)).transpose()?;
        return Ok(Placed {
            _library: held,
            watch: None,
            reserve_take: None,
        });
    }
    let name = o.watch.clone().ok_or(
        "give the session FOLDER, or --watch NAME to record into a watch in the app's library",
    )?;
    let root = o
        .library
        .clone()
        .or_else(library::default_location)
        .ok_or("there is no home folder: give the library with --library")?;
    if !Library::is_library(&root) {
        return Err(format!(
            "{} is not a Timegrapher library (it has no {}): give the session FOLDER, or the \
             library with --library",
            root.display(),
            library::LIBRARY_FILE
        ));
    }
    let lib = hold(&root)?;
    let e = lib.find(&name).ok_or_else(|| {
        format!(
            "there is no watch called \"{}\" in {}: add it in the app, or give the session \
             FOLDER",
            name.trim(),
            root.display()
        )
    })?;
    let w = &e.watch;
    o.watch = Some(w.name.clone());
    if o.calibre.is_none() {
        o.calibre = w.calibre.clone().filter(|c| !c.trim().is_empty());
    }
    o.bph = o.bph.or(w.bph);
    o.lift = o.lift.or(w.lift);
    let started = chrono::Local::now()
        .format(library::TAKE_NAME_TIME)
        .to_string();
    let takes = e.dir.join(library::TAKES);
    // A Power Reserve Test is recorded straight into the watch's takes and
    // filed as one take, as the app files it; any other test gets a
    // session folder its positions are added to.
    let reserve_take = o.power_reserve.then(|| started.clone());
    o.folder = Some(if o.power_reserve {
        takes
    } else {
        library::unused_dir(&takes, &started)
    });
    Ok(Placed {
        watch: Some(w.name.clone()),
        _library: Some(lib),
        reserve_take,
    })
}

/// Every take is kept and added to the session; the report states what
/// was measured.
pub fn run(o: &Options) -> Result<(), String> {
    let (o, rep, settings) = record(o)?;
    if o.json {
        let files: Vec<PathBuf> = o.file.iter().cloned().collect();
        output::print("record", output::input(&files, settings), &rep)?;
    } else {
        print_report(&rep);
    }
    Ok(())
}

/// Record the take as [`run`] does: the options as used, the report, and
/// the settings the JSON document's input names.
fn record(o: &Options) -> Result<(Options, Report, serde_json::Value), String> {
    let mut o = o.clone();
    if o.power_reserve {
        // As long as the watch runs, up to the limit.
        o.seconds = power_reserve::LIMIT_S;
    }
    // Checked before anything is made.
    Position::parse(&o.position).ok_or_else(|| {
        format!(
            "'{}' is not a position; use {}",
            o.position,
            Position::accepted_names()
        )
    })?;
    // A stop file already there would end the take at once.
    if let Some(p) = o
        .stop_file
        .as_ref()
        .filter(|p| fs::symlink_metadata(p).is_ok())
    {
        return Err(format!(
            "--stop-file {} is already there; remove it first",
            p.display()
        ));
    }
    let placed = place(&mut o)?;
    let (rep, settings) = record_in(&o, placed.watch, placed.reserve_take.as_deref())?;
    Ok((o, rep, settings))
}

fn record_in(
    o: &Options,
    library_watch: Option<String>,
    reserve_take: Option<&str>,
) -> Result<(Report, serde_json::Value), String> {
    let pos = Position::parse(&o.position).ok_or_else(|| {
        format!(
            "'{}' is not a position; use {}",
            o.position,
            Position::accepted_names()
        )
    })?;
    timegrapher_core::capture::checked_duration(o.seconds).map_err(|e| format!("--seconds {e}"))?;
    if o.wind_h.is_some_and(|w| !w.is_finite() || w < 0.0) {
        return Err("--wind-h must be 0 or more".into());
    }
    fs::create_dir_all(o.dir()).map_err(|e| format!("{}: {e}", o.dir().display()))?;
    let set = settings(o)?;

    let started = recorder::utc(SystemTime::now());
    let (rec, source, sample_rate, ending, stopped_early) = capture(o, pos, &set)?;
    let take = rec.dir().to_path_buf();
    // The take ends here: its last hour is kept as FLAC too.
    let dir = rec
        .finish_as_flac(None)
        .map_err(|e| format!("{}: {e}", take.display()))?;
    // Filed into a library, a Power Reserve Test's recording is its take.
    let dir = match reserve_take {
        Some(started) => file_reserve_take(dir, &library::power_reserve_take_dir(o.dir(), started)),
        None => dir,
    };
    write_take_manifest(&dir, pos, &set, Some("clock.csv"))?;
    // The run's results go beside the session file that lists the take:
    // the take's own when it is the whole test.
    let results_dir = if reserve_take.is_some() {
        &dir
    } else {
        o.dir()
    };
    let reserve = if o.power_reserve {
        let ending = ending.unwrap_or(power_reserve::Ending::ByHand);
        let r = reserve_results(&dir, &started, pos, set.bph, ending)?;
        power_reserve::write(results_dir, &r)
            .map_err(|e| format!("{}: {e}", results_dir.join(power_reserve::FILE).display()))?;
        Some(r)
    } else {
        None
    };

    let check = check(&dir, &set)?;
    let session_file = if reserve_take.is_some() {
        dir.join(MANIFEST)
    } else {
        add_to_session(o, pos, &set, &dir, &started, &check)?
    };

    let rep = Report {
        take: dir.display().to_string(),
        library_watch,
        session_file: session_file.display().to_string(),
        position: pos.common(),
        wind_h: o.wind_h,
        seconds: o.seconds,
        stopped_early,
        source,
        sample_rate,
        check,
        power_reserve: reserve,
    };
    let settings = json!({
        "bph": set.bph, "lift_deg": set.lift, "lift_known": set.lift_known,
        "lift_from": set.lift_from, "calibre": set.calibre,
        "device": o.device, "seconds": o.seconds,
    });
    Ok((rep, settings))
}

fn settings(o: &Options) -> Result<Settings, String> {
    let file = o.dir().join(MANIFEST);
    let m: Option<Manifest> = if file.is_file() {
        let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        Some(toml::from_str(&text).map_err(|e| format!("Can't read {}: {e}", file.display()))?)
    } else {
        None
    };
    if let Some(manifest) = &m {
        manifest
            .validate_analysis_inputs()
            .map_err(|e| format!("{}: {e}", file.display()))?;
    }
    let named = o
        .calibre
        .as_deref()
        .or(m.as_ref().and_then(|m| m.calibre.as_deref()));
    let user = if named.is_some() {
        match calibre_store::default_path() {
            Some(path) => UserCalibres::load(&path)?,
            None => UserCalibres::default(),
        }
    } else {
        UserCalibres::default()
    };
    settings_with(o, m, &user)
}

fn settings_with(
    o: &Options,
    m: Option<Manifest>,
    user: &UserCalibres,
) -> Result<Settings, String> {
    let file = o.dir().join(MANIFEST);
    let named = o
        .calibre
        .as_deref()
        .or(m.as_ref().and_then(|m| m.calibre.as_deref()));
    let train = Train {
        calibre: named,
        escape_teeth: None,
    };
    let selected = if o.calibre.is_some() {
        train.resolve_with(user)?
    } else {
        train.resolve_label_with(user)?
    };
    let table = selected.calibre.as_ref();
    let bph = o.bph.or(table.and_then(|c| c.bph));
    let lift = o.lift.or(table.and_then(|c| c.lift_angle_deg));
    let calibre = table.map(|c| c.name.clone());
    let Some(m) = m else {
        return Ok(Settings {
            watch: o.watch.clone(),
            calibre,
            bph,
            lift: lift.unwrap_or(take::DEFAULT_LIFT_DEG),
            lift_known: lift.is_some(),
            lift_from: take::SettingFrom::of(o.lift, None, table.and_then(|c| c.lift_angle_deg)),
            settle_s: None,
        });
    };
    // A session's takes must agree on how they are measured.
    let differs = |what: &str, given: String, there: String| {
        format!(
            "{} says {what} {there}, and this take would be {given}; \
             make them agree (edit the file, or record into another folder)",
            file.display()
        )
    };
    // An inherited catalogue fills holes only. A newly supplied flag/calibre
    // still cannot change a session's persisted numeric/context settings.
    let given_bph = o.bph.or(if o.calibre.is_some() {
        table.and_then(|c| c.bph)
    } else {
        None
    });
    let given_lift = o.lift.or(if o.calibre.is_some() {
        table.and_then(|c| c.lift_angle_deg)
    } else {
        None
    });
    if let (Some(a), Some(b)) = (given_bph, m.bph) {
        if a != b {
            return Err(differs("bph", a.to_string(), b.to_string()));
        }
    }
    if let (Some(a), Some(b)) = (given_lift, m.lift) {
        if (a - b).abs() > 1e-9 {
            return Err(differs("lift", format!("{a}"), format!("{b}")));
        }
    }
    // A persisted numeric value wins, along with its context, read as
    // every reader reads it (#685).
    let lift_known = take::file_lift_known(&m, table).unwrap_or(lift.is_some());
    if given_lift.is_some() && m.lift.is_some() && !lift_known {
        return Err(format!(
            "{} records an assumed lift, but this take supplies one; edit lift_known = true \
             only if that context applies to all its takes, or record into another folder",
            file.display()
        ));
    }
    Ok(Settings {
        watch: m.watch.or(o.watch.clone()),
        calibre: m.calibre.or(calibre),
        bph: m.bph.or(bph),
        lift: m.lift.or(lift).unwrap_or(take::DEFAULT_LIFT_DEG),
        lift_known,
        lift_from: take::SettingFrom::of(o.lift, m.lift, table.and_then(|c| c.lift_angle_deg)),
        settle_s: m.settle_s,
    })
}

/// Record into a new take folder under the session folder.
/// A take recorded: its recorder, where the sound came from, its sample
/// rate, and for a Power Reserve Test how it ended.
/// The take, its source, its sample rate, how a Power Reserve Test ended,
/// and whether `--stop-file` ended it early.
type Captured = (Recorder, String, u32, Option<power_reserve::Ending>, bool);

fn capture(o: &Options, pos: Position, set: &Settings) -> Result<Captured, String> {
    let info = |device: String, sample_rate: u32, bits: u16| SessionInfo {
        watch: set.watch.clone().unwrap_or_default(),
        position: pos.common().to_string(),
        lift_deg: set.lift,
        bph: set.bph,
        device,
        sample_rate,
        bits,
        software: format!("timegrapher {}", env!("CARGO_PKG_VERSION")),
    };
    let start = |i: SessionInfo| {
        Recorder::start(o.dir(), i).map_err(|e| format!("{}: {e}", o.dir().display()))
    };
    if let Some(f) = &o.file {
        // A file stands in for the microphone, read at once rather than
        // in real time: for trying the command, and for tests.
        let a = audio::load(f).map_err(|e| format!("{}: {e}", f.display()))?;
        let fs_hz = a.sample_rate;
        let label = f.display().to_string();
        let want =
            timegrapher_core::capture::checked_sample_count(o.seconds, fs_hz)?.min(a.samples.len());
        let mut rec = start(info(label.clone(), fs_hz, 16))?;
        let t0 = SystemTime::now();
        let block = (fs_hz / 50).max(1) as usize;
        let mut feed = o
            .live
            .as_ref()
            .map(|p| Feed::new(p, pos, o.seconds, fs_hz, set));
        let mut stop = o.power_reserve.then(|| StopWatch::new(fs_hz, set));
        let mut asked = StopFile::new(o.stop_file.as_deref());
        let mut n = 0;
        while n < want {
            if asked.asked() {
                break;
            }
            let m = (want - n).min(block);
            let b = Block {
                samples: a.samples[n..n + m].to_vec(),
                at: t0 + Duration::from_secs_f64((n + m) as f64 / fs_hz as f64),
            };
            rec.write(&b)
                .map_err(|e| format!("{}: {e}", rec.dir().display()))?;
            n += m;
            if stop.as_mut().and_then(|s| s.push(&b.samples)).is_some() {
                break;
            }
            // With a live view, played back as it would have been heard.
            if let Some(f) = feed.as_mut() {
                f.push(&b.samples);
                let ahead =
                    n as f64 / fs_hz as f64 - t0.elapsed().unwrap_or_default().as_secs_f64();
                if ahead > 0.0 {
                    std::thread::sleep(Duration::from_secs_f64(ahead));
                }
            }
        }
        if let Some(f) = feed.as_mut() {
            f.finish();
        }
        return Ok((rec, label, fs_hz, stop.map(|s| s.ended()), asked.seen));
    }
    live(o, pos, set, info, start)
}

/// `--stop-file`: whether the file has appeared, looked for at most four
/// times a second, so another program can end the recording as the app's
/// Stop does.
struct StopFile<'a> {
    path: Option<&'a Path>,
    next: std::time::Instant,
    /// The file was seen and the recording ended for it.
    seen: bool,
}

impl<'a> StopFile<'a> {
    fn new(path: Option<&'a Path>) -> StopFile<'a> {
        StopFile {
            path,
            next: std::time::Instant::now(),
            seen: false,
        }
    }

    /// Whether to stop now.
    fn asked(&mut self) -> bool {
        let Some(p) = self.path else { return false };
        let now = std::time::Instant::now();
        if self.seen || now < self.next {
            return self.seen;
        }
        self.next = now + Duration::from_millis(250);
        self.seen = fs::symlink_metadata(p).is_ok();
        if self.seen {
            eprintln!("stopped: {} appeared", p.display());
        }
        self.seen
    }
}

/// Listening for the watch to stop during a Power Reserve Test: the beats
/// as the app hears them, looked at every couple of seconds of sound.
struct StopWatch {
    live: LiveAnalyzer,
    next_s: f64,
}

impl StopWatch {
    fn new(fs_hz: u32, set: &Settings) -> StopWatch {
        let mut cfg = LiveConfig::default();
        cfg.analysis.bph = set.bph;
        cfg.analysis.amplitude.lift_deg = set.lift;
        StopWatch {
            live: LiveAnalyzer::new(fs_hz, cfg),
            next_s: 2.0,
        }
    }

    /// Take in `samples`: how the test has ended, once it has.
    fn push(&mut self, samples: &[f32]) -> Option<power_reserve::Ending> {
        self.live.push(samples);
        let now = self.live.duration_s();
        if now < self.next_s {
            return None;
        }
        self.next_s = now + 2.0;
        self.ending()
    }

    /// How the test has ended by the sound taken in so far, if it has
    /// ([`power_reserve::Heard::ending`]), as the app decides it.
    fn ending(&self) -> Option<power_reserve::Ending> {
        let beat_s = power_reserve::beat_s(self.live.bph());
        power_reserve::heard(self.live.beats(), beat_s).ending(self.live.duration_s(), 0.0)
    }

    /// How the test ended when the recording did: by hand unless the
    /// watch had stopped or the limit was reached.
    fn ended(&self) -> power_reserve::Ending {
        self.ending().unwrap_or(power_reserve::Ending::ByHand)
    }
}

/// A Power Reserve Test's results from its take `dir`, read from the
/// sound as `session` reads it, on the take's clock log: the stop, the run
/// time and each hour. It ended as `ending` says, as decided while it
/// listened.
fn reserve_results(
    dir: &Path,
    started: &str,
    pos: Position,
    bph: Option<u32>,
    ending: power_reserve::Ending,
) -> Result<power_reserve::Results, String> {
    use power_reserve::Results;
    // Opened as every command opens a take (core's `TakeContext::open` on
    // its own session file): its beats, clock log and breaks. A take with
    // no session file is read as its sound alone.
    let manifest = dir.join(MANIFEST);
    let m = match manifest.is_file() {
        true => crate::session_cmd::load_manifest_file(&manifest)?.0,
        false => Manifest::default(),
    };
    let given = take::Given {
        bph,
        lift_deg: None,
    };
    let settings = take::EntrySettings {
        stream: take::stream_settings(&m, None, given),
        card_clock: false,
        settle_s: None,
    };
    let (ctx, log) = timegrapher_core::take_context::TakeContext::open(
        dir,
        &m,
        settings,
        timegrapher_core::take_context::Masks::Cut,
        take::usb,
        |first| take::correction_for(&m, first, None, None),
        |_, _| {},
    )
    .map_err(|e| format!("{}: {e}", dir.display()))?;
    let (clock, breaks) = (ctx.clock, ctx.breaks);
    let beat_s = power_reserve::beat_s(Some(log.bph));
    let heard = power_reserve::heard(&log.beats, beat_s);
    let mut r = Results::of(&heard, ending, started, pos.common(), true, |s| {
        clock.as_ref().map_or(s, |c| c.map(s))
    })
    .ok_or("no beat was heard in the take")?;
    r.hours = power_reserve::hours(
        &log,
        clock.as_ref(),
        &breaks,
        &[],
        r.first_beat_s,
        r.last_beat_s,
    );
    Ok(r)
}

/// Seconds of beats on the live strip: the app's default.
const STRIP_S: f64 = timegrapher_core::live::DEFAULT_STRIP_S;
/// Averaging time of the live readings, seconds: the app's default.
const AVERAGE_S: f64 = timegrapher_core::live::DEFAULT_AVERAGE_S;
/// Most points kept in the live trend; older ones are thinned.
const MAX_TREND: usize = 300;

/// The latest readings while recording, kept in a file for a live view
/// (`timegrapher.live/1`): heard as the app hears it, written about once
/// a second of audio, each time whole, so a reader never sees half a file.
struct Feed {
    path: PathBuf,
    position: &'static str,
    seconds: f64,
    live: LiveAnalyzer,
    /// The strip's anchor, set once from the first beats as the app does.
    anchor: Option<Anchor>,
    /// Time, rate, amplitude, chosen beat error and its source, once a second.
    trend: Vec<([Option<f64>; 4], Option<session::BeatErrorSource>)>,
    next_s: f64,
}

impl Feed {
    fn new(path: &Path, pos: Position, seconds: f64, fs_hz: u32, set: &Settings) -> Feed {
        let mut cfg = LiveConfig::default();
        cfg.analysis.bph = set.bph;
        cfg.analysis.amplitude.lift_deg = set.lift;
        cfg.analysis.lift_known = set.lift_known;
        let mut f = Feed {
            path: path.to_path_buf(),
            position: pos.common(),
            seconds,
            live: LiveAnalyzer::new(fs_hz, cfg),
            anchor: None,
            trend: Vec::new(),
            next_s: 1.0,
        };
        f.write(false);
        f
    }

    fn push(&mut self, samples: &[f32]) {
        self.live.push(samples);
        if self.live.duration_s() >= self.next_s {
            self.next_s = self.live.duration_s().floor() + 1.0;
            self.write(false);
        }
    }

    fn finish(&mut self) {
        self.write(true);
    }

    fn write(&mut self, done: bool) {
        let r = self.live.reading(AVERAGE_S);
        let t = self.live.duration_s();
        let period = self.live.bph().map(|b| 3600.0 / b as f64);
        let beats = self.live.beats();
        if self.anchor.is_none() && beats.len() >= 16 {
            if let Some(p) = period {
                self.anchor = Anchor::centre_on(&beats[..16], p, 16);
            }
        }
        if r.beats_used > 0 {
            self.trend.push((
                [
                    Some(t),
                    r.rate_s_per_day,
                    r.amplitude_deg,
                    session::beat_error_signed(r.beat_error_unlock_ms, r.beat_error_ms),
                ],
                r.beat_error_source,
            ));
            if self.trend.len() > MAX_TREND {
                // Every other point of the older half goes.
                let half = self.trend.len() / 2;
                let mut k = 0;
                self.trend.retain(|_| {
                    k += 1;
                    k > half || k % 2 == 1
                });
            }
        }
        // Each beat's lead on the anchor, ms (positive: early), as the
        // app's strip draws it; Tick is side 0.
        let strip: Vec<[f64; 3]> = match (self.anchor, period) {
            (Some(anchor), Some(p)) => beats
                .iter()
                .filter(|b| b.time >= t - STRIP_S)
                .map(|b| [b.time, anchor.lead_ms(b, p), b.index.rem_euclid(2) as f64])
                .collect(),
            _ => Vec::new(),
        };
        let col = |i: usize| -> Vec<Option<f64>> { self.trend.iter().map(|p| p.0[i]).collect() };
        let doc = json!({
            "schema": "timegrapher.live/1",
            "position": self.position,
            "seconds": self.seconds,
            "recorded_s": t,
            "done": done,
            "average_s": AVERAGE_S,
            "beats_heard": beats.len(),
            "reading": r,
            "strip": { "span_s": STRIP_S, "period_s": period, "beats": strip },
            "trend": { "t_s": col(0), "rate": col(1), "amplitude": col(2), "beat_error": col(3),
                "beat_error_source": self.trend.iter().map(|p| p.1).collect::<Vec<_>>() },
        });
        let tmp = self.path.with_extension("tmp");
        if fs::write(&tmp, doc.to_string()).is_ok() {
            // Windows refuses the swap while another program (a virus
            // scanner, say) holds either file open; try again briefly
            // rather than lose the second's readings.
            for k in 0..5 {
                if k > 0 {
                    std::thread::sleep(Duration::from_millis(20));
                }
                if fs::rename(&tmp, &self.path).is_ok() {
                    break;
                }
            }
        }
    }
}

#[cfg(feature = "live")]
fn live(
    o: &Options,
    pos: Position,
    set: &Settings,
    info: impl Fn(String, u32, u16) -> SessionInfo,
    start: impl Fn(SessionInfo) -> Result<Recorder, String>,
) -> Result<Captured, String> {
    use timegrapher_core::capture::{self, Event};
    let (dev, device_info) = capture::find(o.device.as_deref())?;
    let (cap, _, want) = capture::open_for_duration(&dev, &device_info, None, o.seconds)?;
    let mut rec = start(info(cap.label.clone(), cap.sample_rate, cap.bits))?;
    let fs_hz = cap.sample_rate;
    say_recording(o, pos, &cap.label, rec.dir());
    let mut feed = o
        .live
        .as_ref()
        .map(|p| Feed::new(p, pos, o.seconds, fs_hz, set));
    let mut stop = o.power_reserve.then(|| StopWatch::new(fs_hz, set));
    let mut asked = StopFile::new(o.stop_file.as_deref());
    let mut got = 0usize;
    let mut next_note = 10.0;
    let mut gaps = breaks::GapWatch::new(fs_hz);
    let mut ended = false;
    'listen: while got < want {
        if asked.asked() {
            break;
        }
        let came = if ended {
            gaps.finish()
        } else {
            match cap.rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Event::Audio(b)) => gaps.push(b),
                Ok(Event::Lost(frames)) => gaps.lost(frames),
                Ok(Event::Glitch) => gaps.glitch(),
                Ok(Event::Error(e)) => return Err(format!("recording failed: {e}")),
                Ok(Event::End) => {
                    ended = true;
                    continue;
                }
                Err(_) => return Err("the input stopped delivering audio".into()),
            }
        };
        if ended && came.is_empty() {
            break;
        }
        for live in came {
            let b = match live {
                breaks::Live::Block(b) => b,
                lost => {
                    // The computer slept, the input stalled or glitched, or
                    // its queue filled: a break, the time lost left out.
                    let gap = lost.lost_s();
                    rec.sound_lost(gap);
                    if let Some(f) = feed.as_mut() {
                        f.live.skip(gap);
                    }
                    say_break(&lost);
                    continue;
                }
            };
            let take = (want - got).min(b.samples.len());
            let b = Block {
                samples: b.samples[..take].to_vec(),
                at: b.at,
            };
            rec.write(&b)
                .map_err(|e| format!("{}: {e}", rec.dir().display()))?;
            if let Some(f) = feed.as_mut() {
                f.push(&b.samples);
            }
            got += take;
            if stop.as_mut().and_then(|s| s.push(&b.samples)).is_some() {
                eprintln!("the watch has stopped");
                break 'listen;
            }
            let s = got as f64 / fs_hz as f64;
            if s >= next_note && got < want {
                if o.power_reserve {
                    eprintln!("  {}", power_reserve::run_text(s));
                    next_note += 600.0;
                } else {
                    eprintln!("  {} of {} s", fixed_text(s, 0), fixed_text(o.seconds, 0));
                    next_note += 10.0;
                }
            }
            if got >= want {
                break 'listen;
            }
        }
    }
    eprintln!("done: the watch can be moved");
    if let Some(f) = feed.as_mut() {
        f.finish();
    }
    Ok((
        rec,
        cap.label.clone(),
        fs_hz,
        stop.map(|s| s.ended()),
        asked.seen,
    ))
}

#[cfg(not(feature = "live"))]
fn live(
    _: &Options,
    _: Position,
    _: &Settings,
    _: impl Fn(String, u32, u16) -> SessionInfo,
    _: impl Fn(SessionInfo) -> Result<Recorder, String>,
) -> Result<Captured, String> {
    Err("this build has no sound input support; pass --file, or build with --features live".into())
}

fn check(dir: &Path, set: &Settings) -> Result<Check, String> {
    // The take saved, opened as every surface opens a take, with its clock
    // log, breaks and masks.
    let path = dir.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let m: Manifest = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let e = m
        .recordings
        .first()
        .ok_or_else(|| format!("{}: no recording listed", path.display()))?;
    let settings = take::EntrySettings {
        stream: take::stream_settings(&m, None, take::Given::default()),
        card_clock: false,
        settle_s: set.settle_s,
    };
    #[expect(
        clippy::disallowed_methods,
        reason = "the check reads the recording just saved as session reads one entry: take::measure_opened and the peak level need the opened entry"
    )]
    let opened = take::open_entry(
        dir,
        &m,
        e,
        &settings,
        take::usb,
        |first| take::correction_for(&m, first, None, None),
        |_, _| {},
    )?;
    // Whether a regular beat was found, and clipping, over the whole take,
    // which a short take still gets.
    let log = &opened.log;
    let whole = session::measure_masked(
        log,
        opened.clock.as_ref(),
        &opened.breaks,
        &opened.masked,
        0.0,
        log.duration_s,
    );
    // A take `record` just saved has its sound: never read as silence.
    let clipping = opened
        .clipping()
        .ok_or_else(|| format!("{}: no sound to read the peak level from", dir.display()))?;
    let (whole_bph, whole_s) = (log.bph, log.duration_s);
    // The readings as `session` will take them: after the same settling
    // time.
    let read = take::measure_opened(opened, &m, e, &settings);
    // Readings only when the stretch read alone hears the watch too; a
    // take not much longer than its settling may leave too little.
    let check = |from_s: f64, to_s: f64, bph: u32, m: Option<&session::Measurement>| {
        let heard = whole.watch_heard && m.is_some_and(|m| m.watch_heard);
        let beats_found = m.map_or(0, |m| m.beats);
        // The scatter of the stretch read, with its breaks and masks, as
        // `session` gives it; over the whole take when that stretch gives
        // none (nothing read once settled, or too short for a window).
        let scatter_us = m.and_then(|m| m.jitter_us).or(whole.jitter_us);
        // Whether a regular beat is heard, from the same stretch.
        let watch_heard = m.map_or(whole.watch_heard, |m| m.watch_heard);
        let m = m.filter(|_| heard);
        Check {
            bph_set: set.bph,
            from_s,
            to_s,
            watch_heard,
            timing_scatter_ms: scatter_us.map(|j| j / 1000.0),
            timing_scatter_fraction: beats::timing_scatter_fraction(scatter_us, bph),
            bph,
            beats_found,
            beats_expected: beats::expected_beats(to_s - from_s, bph).round() as usize,
            rate_s_per_day: m.and_then(|m| m.rate_s_per_day),
            amplitude_deg: m.and_then(|m| m.amplitude_deg),
            beat_error_ms: m.and_then(|m| m.beat_error_signed()),
            clipping,
        }
    };
    match read {
        Ok(r) => {
            let m = &r.measurement;
            Ok(check(m.start_s, m.end_s, r.log.bph, Some(m)))
        }
        // Nothing to read once settled: the take is kept, with no readings.
        // Its one entry names the whole take, so its settling runs to the
        // take's end and the stretch read is empty there, whichever of
        // the reader's or the session file's settling it was.
        Err(take::EntryError::NoReading(_)) => Ok(check(whole_s, whole_s, whole_bph, None)),
        Err(take::EntryError::Failed(why)) => Err(why),
    }
}

/// A TOML string literal.
fn q(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// The settings lines of a session file.
fn header(set: &Settings) -> String {
    let mut t = String::new();
    if let Some(w) = &set.watch {
        t += &format!("watch = {}\n", q(w));
    }
    if let Some(c) = &set.calibre {
        t += &format!("calibre = {}\n", q(c));
    }
    if let Some(b) = set.bph {
        t += &format!("bph = {b}\n");
    }
    t += &format!(
        "lift = {}\nlift_known = {}\n",
        toml::Value::Float(set.lift),
        set.lift_known
    );
    t
}

/// Rename the finished recording `dir` to its Power Reserve take folder
/// `to`, waiting as every save does for a file another program has open
/// ([`recorder::rename_settled`]). Should it still be refused, the
/// recording stays where it is, under its own name, and is the take: its
/// session file and run are written there all the same, so a run of up to
/// 80 h is never lost to a rename (Felix on #1734).
fn file_reserve_take(dir: PathBuf, to: &Path) -> PathBuf {
    match recorder::rename_settled(&dir, to) {
        Ok(()) => to.to_path_buf(),
        Err(e) => {
            eprintln!(
                "warning: the take couldn't be renamed to {} ({e}); it is kept as {}",
                to.display(),
                dir.display()
            );
            dir
        }
    }
}

/// The app's session file inside the take, so the app opens the take with
/// its watch, position and settings, as a take the app saved, and
/// `series`, `long` and `session` read it at its own beat rate and lift.
/// `clock` is the take's clock log, if it has one.
pub(crate) fn write_take_manifest(
    dir: &Path,
    pos: Position,
    set: &Settings,
    clock: Option<&str>,
) -> Result<(), String> {
    let text = format!(
        "{}\n[[recording]]\nfile = \".\"\nposition = {}\n{}",
        header(set),
        q(pos.common()),
        clock.map_or(String::new(), |c| format!("clock = {}\n", q(c)))
    );
    let p = dir.join(MANIFEST);
    fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
}

/// The lock a `record` holds while it adds its take to the session file in
/// `dir`, so two at once each add theirs. It is a file in the system's
/// temporary folder named for the session folder, so the session folder
/// holds only takes and its session file; it is left in place, and the
/// system lets go of the lock when the command ends.
fn session_lock(dir: &Path) -> Result<fs::File, String> {
    use std::hash::{Hash, Hasher};
    let dir = fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    let p = std::env::temp_dir().join(format!("timegrapher-session-{:016x}.lock", h.finish()));
    fs::File::create(&p)
        .and_then(|f| f.lock().map(|()| f))
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// List the take in the session folder's session file, writing the file
/// first when there is none. The entry is appended, so the rest of the
/// file, comments included, is left as it was. The file is read under
/// [`session_lock`] and replaced whole, so another `record` adding a take
/// at the same time loses neither entry, and a reader never sees half of
/// it.
fn add_to_session(
    o: &Options,
    pos: Position,
    set: &Settings,
    take: &Path,
    started: &str,
    c: &Check,
) -> Result<PathBuf, String> {
    let file = o.dir().join(MANIFEST);
    let lock = session_lock(o.dir())?;
    let before = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => format!(
            "# Written by `timegrapher record`; edit freely.\n{}",
            header(set)
        ),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    let name = take
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut text = before;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text += &format!(
        "\n{}[[recording]]\nfile = {}\nposition = {}\n{}date = {}\n",
        facts(c)
            .iter()
            .map(|f| format!("# {f}\n"))
            .collect::<String>(),
        q(&name),
        q(pos.common()),
        o.wind_h.map_or(String::new(), |w| format!(
            "wind_h = {}\n",
            toml::Value::Float(w)
        )),
        q(started),
    );
    toml::from_str::<Manifest>(&text)
        .map_err(|e| format!("adding the take would break {}: {e}", file.display()))?;
    library::write_atomic(&file, text.as_bytes())?;
    drop(lock);
    Ok(file)
}

/// The facts about the whole take worth stating beside its readings:
/// no regular beat, and clipping.
fn facts(c: &Check) -> Vec<String> {
    let mut f = Vec::new();
    if !c.watch_heard {
        f.push(session::no_regular_beat(
            c.timing_scatter_ms.map(|ms| ms * 1000.0),
            c.bph_set,
        ));
    }
    if c.clipping.beats > 0 {
        f.push(format!(
            "{}% of beats clipped",
            fixed_text(100.0 * c.clipping.beat_fraction, 1)
        ));
    }
    f
}

fn print_report(r: &Report) {
    let c = &r.check;
    println!("Take         {}", r.take);
    let wind = r
        .wind_h
        .map_or("time since full wind not given".to_string(), |w| {
            format!("{w} h after full wind")
        });
    println!(
        "Position     {} for {} s, {wind}",
        r.position,
        fixed_text(r.seconds, 0)
    );
    if r.stopped_early {
        println!("             Stopped early: the --stop-file appeared");
    }
    for f in facts(c) {
        println!("             {f}");
    }
    if c.watch_heard {
        let f = |v: Option<f64>, d: usize, u: &str| {
            v.map_or("-".to_string(), |x| format!("{} {u}", signed_text(x, d)))
        };
        println!(
            "Quick look   {} to {} s: {} beats of {} expected at {} bph; rate {}, amplitude {}, beat error {}",
            fixed_text(c.from_s, 0),
            fixed_text(c.to_s, 0),
            c.beats_found,
            c.beats_expected,
            c.bph,
            f(c.rate_s_per_day, 1, "s/d"),
            c.amplitude_deg.map_or("-".into(), |a| {
                format!("{}°", fixed_text(a, base_digits::AMPLITUDE))
            }),
            c.beat_error_ms
                .map_or("-".into(), |b| {
                    format!("{} ms", fixed_text(b.abs(), base_digits::BEAT_ERROR))
                }),
        );
        if c.rate_s_per_day.is_none() {
            println!(
                "             {} s measured after the {} s settle: no readings from so short a stretch.",
                fixed_text((c.to_s - c.from_s).max(0.0), 0),
                fixed_text(c.from_s, 0)
            );
        }
    }
    if let Some(p) = &r.power_reserve {
        println!(
            "Power reserve {}. {}.",
            power_reserve::run_text(p.run_s),
            p.ending.text()
        );
    }
    println!("Added to     {}", r.session_file);
}

/// What `record` says as it starts: the position, the input, the folder,
/// and how long it records.
fn say_recording(o: &Options, pos: Position, label: &str, dir: &Path) {
    if o.power_reserve {
        eprintln!(
            "recording {} ({}) from '{}' into {} until the watch stops (at most {}); \
             keep the watch still and the computer awake",
            pos.common(),
            pos.description(),
            label,
            dir.display(),
            power_reserve::limit_text()
        );
    } else {
        eprintln!(
            "recording {} ({}) for {} s from '{}' into {}; keep the watch still until it says done",
            pos.common(),
            pos.description(),
            fixed_text(o.seconds, 0),
            label,
            dir.display()
        );
    }
}

/// What `record` says about a break: sound the input dropped (#483), or a
/// stop in its sound.
fn say_break(lost: &breaks::Live) {
    match *lost {
        breaks::Live::Dropped(gap) => eprintln!(
            "  {} of sound was lost (the input's queue filled); left out as a break",
            timegrapher_core::breaks::lost_text(gap)
        ),
        breaks::Live::Break(gap) => {
            eprintln!(
                "  the sound stopped for {} s (the computer slept?); left out as a break",
                fixed_text(gap, 1)
            );
        }
        breaks::Live::Glitch => {
            eprintln!("  {}", timegrapher_core::breaks::GLITCH_TEXT);
        }
        // The end of a backlog culled after a break: said with the break.
        breaks::Live::Block(_) | breaks::Live::Cut => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::stream::StreamConfig;

    #[test]
    fn live_trend_sources_stay_aligned_when_older_points_are_thinned() {
        use session::BeatErrorSource::{Drop, Unlock};
        use timegrapher_core::synth::{generate, SynthConfig};
        let dir = std::env::temp_dir().join(format!("tg-live-sources-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("live.json");
        let set = Settings {
            watch: None,
            calibre: None,
            bph: Some(28800),
            lift: 52.0,
            lift_known: true,
            lift_from: take::SettingFrom::Given,
            settle_s: None,
        };
        let mut feed = Feed::new(&path, Position::CH, 20.0, 48000, &set);
        let sound = generate(
            &SynthConfig {
                duration_s: 20.0,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        for chunk in sound.samples.chunks(4800) {
            feed.live.push(chunk);
        }
        feed.trend = (0..MAX_TREND)
            .map(|i| {
                let (value, source) = if i % 2 == 0 {
                    (0.2, Unlock)
                } else {
                    (0.8, Drop)
                };
                (
                    [Some(i as f64 / 100.0), None, None, Some(value)],
                    Some(source),
                )
            })
            .collect();
        feed.finish();
        let doc: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let trend = &doc["trend"];
        let times = trend["t_s"].as_array().unwrap();
        let sources = trend["beat_error_source"].as_array().unwrap();
        let values = trend["beat_error"].as_array().unwrap();
        assert!(times.len() < MAX_TREND);
        assert_eq!(times.len(), sources.len());
        assert_eq!(times.len(), values.len());
        for i in 0..times.len() - 1 {
            let original = (times[i].as_f64().unwrap() * 100.0).round() as usize;
            let (value, source) = if original.is_multiple_of(2) {
                (0.2, "unlock")
            } else {
                (0.8, "drop")
            };
            assert_eq!(sources[i], source);
            assert_eq!(values[i], value);
        }
        assert!(doc["reading"]["beat_error_source"].is_string());
        assert_eq!(
            sources.last().unwrap(),
            &doc["reading"]["beat_error_source"]
        );
        let _ = fs::remove_dir_all(dir);
    }

    /// The take in `dir` checked as `record` checks the take it saved:
    /// listed in its own session file, here with no clock log.
    fn check_saved(dir: &Path, set: &Settings) -> Result<Check, String> {
        let text = format!(
            "{}\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
            header(set)
        );
        fs::write(dir.join(MANIFEST), text).unwrap();
        check(dir, set)
    }

    /// A take that settles to its end, by its session file's own
    /// `settle_s`, is checked over an empty stretch at its end, as
    /// `session` reads nothing of it: not over the reader's default
    /// settling (#294).
    #[test]
    fn a_take_that_settles_to_its_end_is_checked_over_nothing() {
        let dir = std::env::temp_dir().join(format!("tg-record-unsettled-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let watch = timegrapher_core::synth::generate(
            &timegrapher_core::synth::SynthConfig {
                duration_s: 30.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        audio::write_wav(&dir.join("audio-001.wav"), &watch).unwrap();
        let set = Settings {
            watch: None,
            calibre: None,
            bph: Some(28800),
            lift: 52.0,
            lift_known: true,
            lift_from: take::SettingFrom::Given,
            settle_s: None,
        };
        let text = format!(
            "{}\n[[recording]]\nfile = \".\"\nposition = \"DU\"\nsettle_s = 60\n",
            header(&set)
        );
        fs::write(dir.join(MANIFEST), text).unwrap();
        let c = check(&dir, &set).unwrap();
        assert_eq!((c.from_s, c.to_s), (30.0, 30.0));
        assert_eq!(c.rate_s_per_day, None);
        fs::remove_dir_all(dir).unwrap();
    }

    /// A noisy recording gives a percentage only when its beat rate was supplied.
    #[test]
    fn noise_without_a_set_rate_has_no_guessed_percent_in_the_quick_check() {
        let dir =
            std::env::temp_dir().join(format!("tg-record-noise-percent-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut seed = 7u64;
        let samples = (0..48_000 * 30)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed as i64 as f64 / i64::MAX as f64 * 0.1) as f32
            })
            .collect();
        audio::write_wav(
            &dir.join("audio-001.wav"),
            &audio::Audio {
                sample_rate: 48_000,
                samples,
            },
        )
        .unwrap();
        for bph in [None, Some(28800)] {
            let set = Settings {
                watch: None,
                calibre: None,
                bph,
                lift: 52.0,
                lift_known: true,
                lift_from: take::SettingFrom::Given,
                settle_s: Some(0.0),
            };
            let c = check_saved(&dir, &set).unwrap();
            assert!(!c.watch_heard);
            assert_eq!(c.bph_set, bph);
            assert!(
                c.timing_scatter_fraction.is_some(),
                "raw numerical evidence is retained"
            );
            let words = facts(&c).join(" ");
            assert!(words.contains("timing scatter"), "{words}");
            assert_eq!(words.contains("% of a beat"), bph.is_some(), "{words}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    /// A Power Reserve take's hours are read across the pauses its
    /// session.json lists, as `session` reads a take: an hour's span is
    /// true time, the pause's included.
    #[test]
    fn a_power_reserve_hour_spans_the_pauses_its_take_lists() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let dir = std::env::temp_dir().join(format!("tg-record-pause-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // 300 s of a watch, heard for the first 90.03 s and from 210.03 s.
        let (pause, gap) = (90.03, 120.0);
        let watch = generate(
            &SynthConfig {
                duration_s: 300.0,
                rate_s_per_day: 6.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let sr = watch.sample_rate;
        let cut = |t: f64| (t * sr as f64) as usize;
        let mut samples = watch.samples[..cut(pause)].to_vec();
        samples.extend_from_slice(&watch.samples[cut(pause + gap)..]);
        audio::write_wav(
            &dir.join("take_part01-001.wav"),
            &audio::Audio {
                samples,
                sample_rate: sr,
            },
        )
        .unwrap();
        fs::write(
            dir.join("session.json"),
            format!(r#"{{"breaks": [{{"audio_s": {pause}, "gap_s": {gap}}}]}}"#),
        )
        .unwrap();
        let r = reserve_results(
            &dir,
            "2026-10-27T18:00:00Z",
            Position::parse("DU").unwrap(),
            Some(28800),
            power_reserve::Ending::ByHand,
        )
        .unwrap();
        let heard_s = r.last_beat_s - r.first_beat_s;
        assert_eq!(r.hours.len(), 1);
        assert!(
            (r.hours[0].span_s - (heard_s + gap)).abs() < 1.0,
            "an hour of {} s for {heard_s} s heard and a {gap} s pause",
            r.hours[0].span_s
        );
        // Read at the take's own lift, from the session file `record`
        // writes in it: a smaller lift reads a smaller amplitude (#508).
        let amplitude = |r: &power_reserve::Results| r.hours[0].amplitude_deg.unwrap();
        fs::write(
            dir.join(MANIFEST),
            "bph = 28800\nlift = 40\nlift_known = true\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
        )
        .unwrap();
        let at_40 = reserve_results(
            &dir,
            "2026-10-27T18:00:00Z",
            Position::parse("DU").unwrap(),
            Some(28800),
            power_reserve::Ending::ByHand,
        )
        .unwrap();
        assert!(
            amplitude(&at_40) < amplitude(&r) - 10.0,
            "{} deg at a 40 deg lift, {} deg at the default",
            amplitude(&at_40),
            amplitude(&r)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The quick check hears the whole take, every hour of it, not the
    /// first alone (#508): clipping in a later hour is counted, at the
    /// take's peak level, as `analyze` counts it in that hour.
    #[test]
    fn the_quick_check_counts_clipping_in_a_later_hour() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let dir = std::env::temp_dir().join(format!("tg-record-whole-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = SynthConfig {
            duration_s: 60.0,
            ..Default::default()
        };
        let quiet = generate(&cfg, |_| 280.0, |_| 0.0);
        let loud = audio::Audio {
            samples: quiet
                .samples
                .iter()
                .map(|v| (v * 40.0).clamp(-1.0, 1.0))
                .collect(),
            sample_rate: quiet.sample_rate,
        };
        audio::write_wav(&dir.join("audio-001.wav"), &quiet).unwrap();
        audio::write_wav(&dir.join("audio-002.wav"), &loud).unwrap();
        let set = Settings {
            watch: None,
            calibre: None,
            bph: Some(28800),
            lift: 52.0,
            lift_known: true,
            lift_from: take::SettingFrom::Given,
            settle_s: Some(0.0),
        };
        let c = check_saved(&dir, &set).unwrap();
        // The loud hour as read back from its file, alone.
        let read_back = audio::load(&dir.join("audio-002.wav")).unwrap();
        let alone =
            timegrapher_core::analyze(&read_back, &timegrapher_core::AnalysisConfig::default());
        let hour = alone.summary.clipping;
        assert!(hour.beats > 0, "no clipping in the loud hour");
        assert!(
            c.clipping.beats.abs_diff(hour.beats) <= 2,
            "{} clipped beats in the take, {} in its loud hour",
            c.clipping.beats,
            hour.beats
        );
        assert_eq!(c.clipping.peak_dbfs, hour.peak_dbfs);
        assert!(
            c.from_s == 0.0 && c.to_s > 110.0,
            "{} to {} s",
            c.from_s,
            c.to_s
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A take whose unlock is never found is checked with the drop's beat
    /// error, as `session` reads the take.
    #[test]
    fn a_take_whose_unlock_is_never_found_is_checked_with_the_drops_beat_error() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let dir = std::env::temp_dir().join(format!("tg-record-unlock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut cfg = SynthConfig {
            duration_s: 60.0,
            beat_error_ms: 0.8,
            ..Default::default()
        };
        cfg.sounds[0].gain = 0.0;
        let wav = dir.join("audio-001.wav");
        audio::write_wav(&wav, &generate(&cfg, |_| 250.0, |_| 0.0)).unwrap();
        let set = Settings {
            watch: None,
            calibre: None,
            bph: Some(28800),
            lift: 52.0,
            lift_known: true,
            lift_from: take::SettingFrom::Given,
            settle_s: None,
        };
        let c = check_saved(&dir, &set).unwrap();
        let mut scfg = StreamConfig::default();
        scfg.analysis.bph = set.bph;
        let settings = take::EntrySettings {
            stream: scfg,
            card_clock: true,
            settle_s: None,
        };
        let opened = take::open_files(
            std::slice::from_ref(&wav),
            None,
            &settings,
            |_| Ok(None),
            |_| Ok(None),
            |_, _| {},
        )
        .unwrap();
        let m = session::measure(&opened.log, None, c.from_s, c.to_s);
        assert_eq!(m.beat_error_unlock_ms, None, "the unlock is never found");
        assert!(m.beat_error_ms.is_some());
        assert_eq!(c.beat_error_ms, m.beat_error_ms);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Two `record`s adding to one new session folder at the same moment
    /// each keep their entry: the audit of 2026-10-26 (finding ARCH-03)
    /// found the second wrote over the first. Each of six here starts as
    /// if the folder had no session file yet.
    #[test]
    fn records_at_once_each_keep_their_entry() {
        let base = std::env::temp_dir().join(format!("tg-record-at-once-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let folder = base.join("session");
        fs::create_dir_all(&folder).unwrap();
        let set = Settings {
            watch: None,
            calibre: None,
            bph: Some(28800),
            lift: 52.0,
            lift_known: true,
            lift_from: take::SettingFrom::Given,
            settle_s: None,
        };
        let check = || Check {
            from_s: 0.0,
            to_s: 1.0,
            watch_heard: true,
            timing_scatter_ms: None,
            timing_scatter_fraction: None,
            bph: 28800,
            bph_set: Some(28800),
            beats_found: 8,
            beats_expected: 8,
            rate_s_per_day: None,
            amplitude_deg: None,
            beat_error_ms: None,
            clipping: Clipping {
                samples: 0,
                beats: 0,
                beat_fraction: 0.0,
                peak_dbfs: -20.0,
            },
        };
        let positions = ["DU", "DD", "CU", "CD", "CL", "CR"];
        let start = std::sync::Barrier::new(positions.len());
        std::thread::scope(|s| {
            for p in positions {
                let (folder, set, start) = (&folder, &set, &start);
                s.spawn(move || {
                    let mut o = options(folder.clone());
                    o.position = p.into();
                    let pos = Position::parse(p).unwrap();
                    let take = folder.join(format!("take_{p}"));
                    start.wait();
                    add_to_session(&o, pos, set, &take, "2026-10-28T00:00:00Z", &check()).unwrap();
                });
            }
        });
        let m: Manifest =
            toml::from_str(&fs::read_to_string(folder.join(MANIFEST)).unwrap()).unwrap();
        let mut files: Vec<String> = m.recordings.iter().map(|r| r.file.clone()).collect();
        files.sort();
        let mut want: Vec<String> = positions.iter().map(|p| format!("take_{p}")).collect();
        want.sort();
        assert_eq!(files, want);
        // And nothing else is left in the folder.
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&base);
    }

    fn options(folder: PathBuf) -> Options {
        Options {
            folder: Some(folder),
            library: None,
            position: "DU".into(),
            seconds: 1.0,
            device: None,
            file: None,
            watch: None,
            calibre: None,
            bph: None,
            lift: None,
            wind_h: None,
            json: false,
            live: None,
            power_reserve: false,
            stop_file: None,
        }
    }

    /// A Power Reserve run from a file: recording stops ten minutes after
    /// the watch does, and the run and its hour are kept beside the take.
    #[test]
    fn a_power_reserve_run_stops_with_the_watch_and_keeps_its_run() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let base = std::env::temp_dir().join(format!("tg-record-reserve-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let mut a = generate(
            &SynthConfig {
                duration_s: 120.0,
                rate_s_per_day: 5.0,
                ..Default::default()
            },
            |_| 260.0,
            |_| 0.0,
        );
        // The watch stops at 120 s; the file runs on silent to 760 s.
        a.samples.resize(a.sample_rate as usize * 760, 0.0);
        let wav = base.join("run.wav");
        audio::write_wav(&wav, &a).unwrap();
        let folder = base.join("session");
        let mut o = options(folder.clone());
        o.position = "CL".into();
        o.file = Some(wav);
        o.bph = Some(28800);
        o.power_reserve = true;
        // A stop file that never appears changes nothing.
        o.stop_file = Some(base.join("never"));
        let (_, rep, _) = record(&o).unwrap();
        assert!(!rep.stopped_early);
        let r = power_reserve::Results::from_json(
            &fs::read_to_string(folder.join(power_reserve::FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(r.ending, power_reserve::Ending::Stopped);
        assert_eq!(r.position, "CL");
        assert!((r.run_s - 120.0).abs() < 1.0, "{}", r.run_s);
        assert_eq!(r.hours.len(), 1);
        assert!(
            (r.hours[0].rate_s_per_day.unwrap() - 5.0).abs() < 1.0,
            "{:?}",
            r.hours
        );
        // It stopped listening once the watch had been silent ten minutes.
        let take = fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.is_dir())
            .unwrap();
        let files = timegrapher_core::recording_order::recordings_in(&take, &[]).unwrap();
        let info = audio::info(&files[0]).unwrap();
        let secs = info.frames.unwrap() as f64 / f64::from(info.sample_rate);
        assert!(secs > 715.0 && secs < 740.0, "{secs}");
        let _ = fs::remove_dir_all(&base);
    }

    /// `--stop-file` ends a Power Reserve run as Stop Test does: by hand,
    /// with its run and hours kept beside the take, the take shorter than
    /// the sound on offer.
    #[test]
    fn a_stop_file_ends_a_power_reserve_run_by_hand() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let base = std::env::temp_dir().join(format!("tg-record-stop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let a = generate(
            &SynthConfig {
                duration_s: 60.0,
                rate_s_per_day: 5.0,
                ..Default::default()
            },
            |_| 260.0,
            |_| 0.0,
        );
        let wav = base.join("run.wav");
        audio::write_wav(&wav, &a).unwrap();
        let folder = base.join("session");
        let stop = base.join("stop");
        let mut o = options(folder.clone());
        o.file = Some(wav);
        o.bph = Some(28800);
        o.power_reserve = true;
        // Played back in real time, so the stop comes part way through.
        o.live = Some(base.join("live.json"));
        o.stop_file = Some(stop.clone());
        let asker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            fs::write(stop, "").unwrap();
        });
        let (_, rep, _) = record(&o).unwrap();
        asker.join().unwrap();
        assert!(rep.stopped_early);
        let r = power_reserve::Results::from_json(
            &fs::read_to_string(folder.join(power_reserve::FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(r.ending, power_reserve::Ending::ByHand);
        let take = fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.is_dir())
            .unwrap();
        let files = timegrapher_core::recording_order::recordings_in(&take, &[]).unwrap();
        let info = audio::info(&files[0]).unwrap();
        let secs = info.frames.unwrap() as f64 / f64::from(info.sample_rate);
        assert!(secs > 3.0 && secs < 20.0, "{secs}");
        // The take is in the session file, as any other is.
        let listed = fs::read_to_string(folder.join(MANIFEST)).unwrap();
        assert_eq!(listed.matches("[[recording]]").count(), 1, "{listed}");
        let _ = fs::remove_dir_all(&base);
    }

    /// A Power Reserve take that can't be renamed stays a take under its
    /// recording's own name, so its run is still written (#1734).
    #[test]
    fn a_reserve_take_that_cant_be_renamed_is_kept_where_it_is() {
        let base = std::env::temp_dir().join(format!("tg-record-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let rec = base.join("2026-11-14T17-18-10_YM42_CR");
        fs::create_dir_all(&rec).unwrap();
        // A folder that isn't there can't take the recording.
        let to = base.join("missing").join("2026-11-14 17.18 Power Reserve");
        assert_eq!(file_reserve_take(rec.clone(), &to), rec);
        assert!(rec.is_dir());
        let free = base.join("2026-11-14 17.18 Power Reserve");
        assert_eq!(file_reserve_take(rec.clone(), &free), free);
        assert!(free.is_dir() && !rec.exists());
        let _ = fs::remove_dir_all(&base);
    }

    /// A stop file already there is refused before anything is made.
    #[test]
    fn a_stop_file_already_there_is_refused() {
        let base = std::env::temp_dir().join(format!("tg-record-stale-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let stop = base.join("stop");
        fs::write(&stop, "").unwrap();
        let folder = base.join("session");
        let mut o = options(folder.clone());
        o.file = Some(base.join("missing.wav"));
        o.stop_file = Some(stop);
        let e = run(&o).unwrap_err();
        assert!(e.contains("already there"), "{e}");
        assert!(!folder.exists());
        let _ = fs::remove_dir_all(&base);
    }

    /// Where the lift came from, as `record`'s JSON says it (#1648): a
    /// session file's own, given, a calibre's, or the default.
    #[test]
    fn the_lift_says_where_it_came_from() {
        use timegrapher_core::take::SettingFrom;
        let user = UserCalibres::default();
        let mut o = options(PathBuf::from("unused-settings-folder"));
        let kept = || toml::from_str::<Manifest>("lift = 60\nlift_known = true\n").unwrap();
        let from = |o: &Options, m: Option<Manifest>| {
            let s = settings_with(o, m, &user).unwrap();
            (s.lift, s.lift_known, s.lift_from)
        };
        assert_eq!(from(&o, Some(kept())), (60.0, true, SettingFrom::Take));
        assert_eq!(from(&o, None), (52.0, false, SettingFrom::Default));
        o.lift = Some(60.0);
        assert_eq!(from(&o, Some(kept())), (60.0, true, SettingFrom::Given));
        assert_eq!(from(&o, None), (60.0, true, SettingFrom::Given));
        o.lift = None;
        o.calibre = Some("ETA 2824-2".into());
        assert_eq!(from(&o, None).2, SettingFrom::Calibre);
        // A second take of an ETA 2824-2 whose session file keeps its own
        // lift: the take's, though the table gives one too.
        o.calibre = None;
        let eta =
            toml::from_str::<Manifest>("calibre = \"ETA 2824-2\"\nlift = 48\nlift_known = true\n")
                .unwrap();
        assert_eq!(from(&o, Some(eta)), (48.0, true, SettingFrom::Take));
    }

    #[test]
    fn custom_record_settings_inherit_only_missing_manifest_values_and_context() {
        let user: UserCalibres = serde_json::from_value(json!({
            "schema": calibre_store::SCHEMA,
            "custom":[{"name":"Bench 44","aliases":["Workshop-A"],"bph":21600,"lift_angle_deg":44.0,"escape_teeth":20,"wheels":[]}],
            "wheels":[],
        })).unwrap();
        let mut o = options(PathBuf::from("unused-settings-folder"));
        o.calibre = Some("Workshop-A".into());
        let supplied = settings_with(&o, None, &user).unwrap();
        assert_eq!(
            (supplied.bph, supplied.lift, supplied.lift_known),
            (Some(21600), 44.0, true)
        );
        assert_eq!(supplied.calibre.as_deref(), Some("Bench 44"));
        let saved: Manifest = toml::from_str(&header(&supplied)).unwrap();
        assert_eq!(
            (saved.bph, saved.lift, saved.lift_known),
            (Some(21600), Some(44.0), Some(true))
        );
        o.calibre = None;
        let inherited: Manifest = toml::from_str("calibre = \"Workshop-A\"\n").unwrap();
        let set = settings_with(&o, Some(inherited), &user).unwrap();
        assert_eq!(
            (set.bph, set.lift, set.lift_known),
            (Some(21600), 44.0, true)
        );
        assert_eq!(
            set.calibre.as_deref(),
            Some("Workshop-A"),
            "parent label preserved"
        );
        let persisted = || {
            toml::from_str("calibre = \"Workshop-A\"\nbph=18000\nlift=52\nlift_known=false")
                .unwrap()
        };
        let set = settings_with(&o, Some(persisted()), &user).unwrap();
        assert_eq!(
            (set.bph, set.lift, set.lift_known),
            (Some(18000), 52.0, false)
        );
        o.lift = Some(52.0);
        assert!(settings_with(&o, Some(persisted()), &user)
            .err()
            .unwrap()
            .contains("assumed lift"));
        o.lift = None;
        o.bph = Some(28800);
        assert!(settings_with(&o, Some(persisted()), &user).is_err());
        o.bph = None;
        o.calibre = Some("Workshop-A".into());
        assert!(settings_with(&o, Some(persisted()), &user).is_err());
        o.calibre = Some("Unknown explicit".into());
        assert!(settings_with(&o, None, &user).is_err());
        o.calibre = None;
        let label: Manifest = toml::from_str("calibre=\"Unknown label\"\nlift_known=true").unwrap();
        let set = settings_with(&o, Some(label), &user).unwrap();
        assert_eq!((set.bph, set.lift, set.lift_known), (None, 52.0, false));
        assert_eq!(set.calibre.as_deref(), Some("Unknown label"));
    }

    #[test]
    fn recorded_lift_context_roundtrips_and_rejects_changed_session_context() {
        let dir = std::env::temp_dir().join(format!("tg-record-lift-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut o = options(dir.clone());
        let assumed = settings(&o).unwrap();
        assert_eq!(assumed.lift, 52.0);
        let saved: Manifest = toml::from_str(&header(&assumed)).unwrap();
        assert_eq!(saved.lift_known, Some(false));
        fs::write(dir.join(MANIFEST), header(&assumed)).unwrap();
        assert!(!settings(&o).unwrap().lift_known);
        o.lift = Some(52.0);
        let error = settings(&o).err().expect("same number, changed context");
        assert!(error.contains("assumed lift") && error.contains("another folder"));
        fs::remove_file(dir.join(MANIFEST)).unwrap();
        let supplied = settings(&o).unwrap();
        assert!(supplied.lift_known);
        let saved: Manifest = toml::from_str(&header(&supplied)).unwrap();
        assert_eq!(saved.lift_known, Some(true));
        fs::write(dir.join(MANIFEST), header(&supplied)).unwrap();
        o.lift = None;
        assert!(settings(&o).unwrap().lift_known);
        // An older file's default lift, with no marker, was assumed (#685).
        fs::write(dir.join(MANIFEST), "lift = 52\n").unwrap();
        assert!(!settings(&o).unwrap().lift_known, "an older file's default");
        fs::write(dir.join(MANIFEST), "lift = 49\n").unwrap();
        assert!(
            settings(&o).unwrap().lift_known,
            "an older file's own value"
        );
        fs::remove_file(dir.join(MANIFEST)).unwrap();
        o.calibre = Some("Unitas 6497-2".into());
        let calibre = settings(&o).unwrap();
        assert_eq!(calibre.lift, 44.0);
        assert!(calibre.lift_known);
        assert!(calibres::find("Seagull ST36")
            .unwrap()
            .lift_angle_deg
            .is_none());
        o.calibre = Some("Seagull ST36".into());
        let named = settings(&o).unwrap();
        assert_eq!(named.lift, 52.0);
        assert!(!named.lift_known, "a name alone does not supply a lift");
        fs::remove_dir_all(dir).unwrap();
    }
}
