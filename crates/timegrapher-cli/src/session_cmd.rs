//! `timegrapher session`: a watch measured in several positions, read
//! together into one report (Witschi's multi-position test).

use crate::long::{ResolvedCalibre, ResolvedTrain, Train};
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use timegrapher_core::calibre_store::{self, UserCalibres};
use timegrapher_core::calibres;
use timegrapher_core::library::fixed_text;
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::Wheel;
use timegrapher_core::session::{
    self, Finding, Manifest, Position, Reading, RecordingEntry, SessionReport,
};
use timegrapher_core::stream::StreamConfig;
use timegrapher_core::{analysis_take, audio, power_reserve, take, twostate};

pub use timegrapher_core::take::MANIFEST;

pub struct Options {
    pub bph: Option<u32>,
    pub lift: Option<f64>,
    /// A calibre in the table (`calibres`): its beat rate, lift angle and
    /// wheels, where the flags and the session file give none.
    pub calibre: Option<String>,
    pub settle: Option<f64>,
    pub out: Option<PathBuf>,
    pub json: bool,
    pub init: bool,
    pub no_shape: bool,
    pub no_cycles: bool,
}

#[derive(Debug)]
pub(crate) struct SessionCalibre {
    train: ResolvedTrain,
    manifest_name: Option<String>,
}

impl Options {
    /// Resolve all names from one catalogue read. Unknown manifest names are labels.
    pub(crate) fn calibre(&self, m: &Manifest) -> Result<SessionCalibre, String> {
        let user = if self.calibre.is_some() || m.calibre.is_some() {
            match calibre_store::default_path() {
                Some(path) => UserCalibres::load(&path)?,
                None => UserCalibres::default(),
            }
        } else {
            UserCalibres::default()
        };
        self.calibre_with(m, &user)
    }

    pub(crate) fn calibre_with(
        &self,
        m: &Manifest,
        user: &UserCalibres,
    ) -> Result<SessionCalibre, String> {
        let inherited = Train {
            calibre: m.calibre.as_deref(),
            escape_teeth: m.escape_teeth,
        }
        .resolve_label_with(user)?;
        let manifest_name = inherited.calibre.as_ref().map(|c| c.name.clone());
        let train = if self.calibre.is_some() {
            Train {
                calibre: self.calibre.as_deref(),
                escape_teeth: m.escape_teeth,
            }
            .resolve_with(user)
            .map_err(|e| {
                // Retain the session command's existing actionable known-name list.
                format!(
                    "{e}; known: {}",
                    calibres::all()
                        .iter()
                        .map(|c| c.calibre.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?
        } else {
            inherited
        };
        // With no calibre and no `escape_teeth`, no wheel is named.
        let teeth = m.escape_teeth.or(train
            .calibre
            .as_ref()
            .map(|c| c.escape_teeth.unwrap_or(take::ESCAPE_TEETH)));
        Ok(SessionCalibre {
            train: train.with_escape_teeth(teeth),
            manifest_name,
        })
    }

    /// Where the calibre and the values that win over it disagree, with
    /// the value used.
    fn calibre_notes(&self, m: &Manifest, selection: &SessionCalibre) -> Vec<String> {
        let Some(c) = selection.train.calibre.as_ref() else {
            return Vec::new();
        };
        let mut notes = Vec::new();
        if self.calibre.is_some() {
            if let Some(other) = selection.manifest_name.as_deref() {
                if other != c.name {
                    notes.push(format!(
                        "the session file names the {}, --calibre the {}; using the {}",
                        other, c.name, c.name
                    ));
                }
            }
        }
        let from = |flag: bool| if flag { "the flag" } else { "the session file" };
        if let Some((b, cb)) = self.bph.or(m.bph).zip(c.bph).filter(|&(b, cb)| b != cb) {
            notes.push(format!(
                "the {} beats at {cb} bph, {} says {b}; using {b}",
                c.name,
                from(self.bph.is_some())
            ));
        }
        if let (Some(l), Some(cl)) = (self.lift.or(m.lift), c.lift_angle_deg) {
            if (l - cl).abs() > 1e-9 {
                notes.push(format!(
                    "the {}'s lift angle is {cl}°, {} says {l}°; using {l}°",
                    c.name,
                    from(self.lift.is_some())
                ));
            }
        }
        if let (Some(t), Some(ct)) = (m.escape_teeth, c.escape_teeth) {
            if t != ct {
                notes.push(format!(
                    "the {}'s escape wheel has {ct} teeth, the session file says {t}; using {t}",
                    c.name
                ));
            }
        }
        notes
    }

    /// What the flags give before the session file.
    fn given(&self) -> take::Given {
        take::Given {
            bph: self.bph,
            lift_deg: self.lift,
        }
    }

    /// Beat rate: the flag, else the session file, else the calibre's;
    /// none means it is measured ([`take::stream_settings`]).
    fn bph(&self, m: &Manifest, cal: Option<&ResolvedCalibre>) -> Option<u32> {
        take::stream_settings(m, cal, self.given()).analysis.bph
    }

    /// Lift angle: the flag, else the session file, else the calibre's,
    /// else the default ([`take::stream_settings`]).
    fn lift(&self, m: &Manifest, cal: Option<&ResolvedCalibre>) -> f64 {
        take::stream_settings(m, cal, self.given())
            .analysis
            .amplitude
            .lift_deg
    }
}

/// Recordings named on the command line, or found in a folder: each
/// audio file is one recording, and so is each subfolder of segments.
fn scan(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for p in paths {
        if !p.is_dir() {
            out.push(p.clone());
            continue;
        }
        let mut inner: Vec<PathBuf> = fs::read_dir(p)
            .map_err(|e| format!("{}: {e}", p.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|q| take::is_sound(q) || (q.is_dir() && take::has_sound(q)))
            .collect();
        inner.sort();
        if inner.is_empty() {
            return Err(format!("{}: no recordings found", p.display()));
        }
        out.extend(inner);
    }
    Ok(out)
}

fn rel(p: &Path, base: &Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// A session file, the folder holding it, and the folder it came from.
pub(crate) fn load_manifest(paths: &[PathBuf]) -> Result<(Manifest, PathBuf), String> {
    let single = match paths {
        [p] => Some(p),
        _ => None,
    };
    let file = single.and_then(|p| {
        if p.is_dir() && p.join(MANIFEST).is_file() {
            Some(p.join(MANIFEST))
        } else if p.extension().is_some_and(|e| e == "toml") {
            Some(p.clone())
        } else {
            None
        }
    });
    if let Some(f) = file {
        let (m, base, _) = load_manifest_file(&f)?;
        return Ok((m, base));
    }
    // No session file: every recording, with its position from its name.
    let base = match single {
        Some(p) if p.is_dir() => p.clone(),
        _ => PathBuf::from("."),
    };
    let files = scan(paths)?;
    let recordings = files
        .iter()
        .map(|f| RecordingEntry {
            file: rel(f, &base),
            ..Default::default()
        })
        .collect();
    Ok((
        Manifest {
            recordings,
            ..Default::default()
        },
        base,
    ))
}

/// Identity is computed from the exact UTF-8 bytes parsed, not a later reread.
pub(crate) fn load_manifest_file(f: &Path) -> Result<(Manifest, PathBuf, String), String> {
    let text = fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
    let m: Manifest = toml::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
    m.validate_analysis_inputs()
        .map_err(|e| format!("{}: {e}", f.display()))?;
    check_times(&m).map_err(|e| format!("{}: {e}", f.display()))?;
    if m.recordings.is_empty() {
        return Err(format!("{}: no [[recording]] entries", f.display()));
    }
    let base = f
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    Ok((m, base, crate::archive::sha256_hex(text.as_bytes())))
}

/// Seconds that must be a finite number of at least `min` (or above it,
/// when `above`), checked before any audio is read.
fn check_seconds(name: &str, value: Option<f64>, min: f64, above: bool) -> Result<(), String> {
    match value {
        Some(v) if !v.is_finite() || v < min || (above && v <= min) => Err(format!(
            "{name} is {v}; it must be a number of seconds {} {min}",
            if above { "above" } else { "of at least" }
        )),
        _ => Ok(()),
    }
}

/// The session file's settling and measuring times, its own and each
/// reading's, and each reading's start and end.
fn check_times(m: &Manifest) -> Result<(), String> {
    check_seconds("settle_s", m.settle_s, 0.0, false)?;
    check_seconds("measure_s", m.measure_s, 0.0, true)?;
    for e in &m.recordings {
        let at = |name: &str| format!("{}: {name}", e.file);
        check_seconds(&at("settle_s"), e.settle_s, 0.0, false)?;
        check_seconds(&at("measure_s"), e.measure_s, 0.0, true)?;
        check_seconds(&at("start_s"), e.start_s, 0.0, false)?;
        check_seconds(&at("end_s"), e.end_s, 0.0, true)?;
    }
    Ok(())
}

pub(crate) fn position_of(e: &RecordingEntry) -> Result<Position, String> {
    match &e.position {
        Some(p) => Position::parse(p).ok_or_else(|| {
            format!(
                "{}: unknown position '{p}' (use {})",
                e.file,
                Position::accepted_names()
            )
        }),
        None => Position::from_file_name(&e.file).ok_or_else(|| {
            format!(
                "{}: no position in the file name; give one in {MANIFEST} (try `timegrapher session --init`)",
                e.file
            )
        }),
    }
}

fn init(paths: &[PathBuf], o: &Options) -> Result<(), String> {
    let dir = match paths {
        [p] if p.is_dir() => p.clone(),
        _ => return Err("--init takes one folder of recordings".into()),
    };
    let target = dir.join(MANIFEST);
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    let mut files = scan(paths)?;
    // In Witschi's order of positions, files with no position last.
    files.sort_by_key(|f| {
        let p = Position::from_file_name(&rel(f, &dir));
        (p.is_none(), p, f.clone())
    });
    let mut t = String::new();
    t.push_str("# Test session for `timegrapher session`. Edit and run:\n");
    t.push_str(&format!("#   timegrapher session {}\n\n", dir.display()));
    t.push_str("watch = \"\"\ncalibre = \"\"\n");
    let selection = o.calibre(&Manifest::default())?;
    let cal = selection.train.calibre.as_ref();
    t.push_str(&format!(
        "bph = {}\n",
        o.bph.or(cal.and_then(|c| c.bph)).unwrap_or(28800)
    ));
    t.push_str(&format!(
        "lift = {}\n",
        o.lift
            .or(cal.and_then(|c| c.lift_angle_deg))
            .unwrap_or(take::DEFAULT_LIFT_DEG)
    ));
    t.push_str("# seconds skipped at the start of each recording while the watch settles\n");
    t.push_str(&format!(
        "settle_s = {}\n",
        o.settle.unwrap_or(take::SETTLE_S)
    ));
    t.push_str(
        "# known sound-card error, ppm fast (negative: slow), for recordings without a clock log\n",
    );
    t.push_str("# card_ppm_fast = 0.0\n");
    for f in &files {
        let name = rel(f, &dir);
        t.push_str("\n[[recording]]\n");
        t.push_str(&format!("file = \"{name}\"\n"));
        match Position::from_file_name(&name) {
            Some(p) => t.push_str(&format!(
                "position = \"{}\"  # {}, guessed from the name\n",
                p.code(),
                p.name()
            )),
            None => t.push_str(&format!(
                "position = \"\"  # {}\n",
                Position::accepted_names()
            )),
        }
        t.push_str(
            "# wind_h = 0  # hours since the watch was fully wound (0 = just wound); left out, the wind is not given: its own state, never fully wound\n",
        );
        if f.is_dir() {
            if let Some(log) = ["clocklog.txt", "clock.csv", "clock.log"]
                .iter()
                .map(|n| f.join(n))
                .find(|p| p.is_file())
            {
                t.push_str(&format!("clock = \"{}\"\n", rel(&log, &dir)));
            }
        }
        t.push_str("# date = \"\"\n# notes = \"\"\n");
        t.push_str(
            "# reference = { rate = 0.0, amplitude = 0, beat_error = 0.0, source = \"\", clock = \"own\" }  # clock: \"own\" (the instrument's own time base) or \"sound_card\" (read from this recording)\n",
        );
    }
    fs::write(&target, t).map_err(|e| format!("{}: {e}", target.display()))?;
    println!(
        "Wrote {} with {} recording(s); check the positions, then run it",
        target.display(),
        files.len()
    );
    Ok(())
}

/// Up to `len_s` seconds of audio from `from_s`, across segment files.
fn excerpt(files: &[PathBuf], from_s: f64, len_s: f64) -> Result<(Vec<f32>, f64), String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut fs = 0.0;
    for f in files {
        let info = audio::info(f).map_err(|e| format!("{}: {e}", f.display()))?;
        fs = info.sample_rate as f64;
        let a = (from_s * fs) as usize;
        let b = ((from_s + len_s) * fs) as usize;
        if pos >= b {
            break;
        }
        if let Some(n) = info.frames {
            if pos + n as usize <= a {
                pos += n as usize;
                continue;
            }
        }
        audio::stream(f, 1 << 16, |block| {
            let (s, e) = (pos, pos + block.len());
            if e > a && s < b {
                out.extend_from_slice(&block[a.max(s) - s..b.min(e) - s]);
            }
            pos = e;
        })
        .map_err(|e| format!("{}: {e}", f.display()))?;
    }
    Ok((out, fs))
}

pub(crate) struct Ctx<'a> {
    pub m: &'a Manifest,
    pub calibre: &'a SessionCalibre,
    pub base: &'a Path,
    pub o: &'a Options,
    pub tty: bool,
    /// No progress lines (the file names stay out of logs).
    pub quiet: bool,
    /// Rates on the sound card's own clock, uncorrected: for setting
    /// beside references read from the same recordings on that clock
    /// ([`session::Reference::same_clock_as`]), as `regress` does.
    pub card_clock: bool,
}

/// The analysis settings for a session's recordings, and its calibre.
fn stream_config<'a>(c: &Ctx<'a>) -> Result<(StreamConfig, Option<&'a ResolvedCalibre>), String> {
    let cal = c.calibre.train.calibre.as_ref();
    Ok((take::stream_settings(c.m, cal, c.o.given()), cal))
}

/// Whether the lift angle came from `--lift`, the session file or
/// the calibre, rather than the default.
fn lift_known(c: &Ctx, cal: Option<&ResolvedCalibre>) -> bool {
    take::stream_settings(c.m, cal, c.o.given())
        .analysis
        .lift_known
}

/// What `session` reads of the sound file `file` when the session file
/// in its folder lists it: each entry naming it, read as `session` reads
/// it ([`read_one`], with `bph`, `lift` and `calibre` as `session`'s own
/// flags and no cycles), and the entries naming it with nothing to read.
/// None when the folder has no session file or it does not list `file`.
/// A session file that cannot be read, or an entry naming `file` that
/// `session` would refuse, gives `session_file` and the `error`, so the
/// file's own reading still stands.
pub(crate) fn take_of(
    file: &Path,
    bph: Option<u32>,
    lift: Option<f64>,
    calibre: Option<String>,
) -> Option<serde_json::Value> {
    let dir = file.parent()?;
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let session_file = dir.join(take::MANIFEST);
    if !session_file.is_file() {
        return None;
    }
    read_take_of(file, &session_file, bph, lift, calibre).unwrap_or_else(|error| {
        Some(serde_json::json!({
            "session_file": session_file,
            "error": error,
        }))
    })
}

/// The session file in `file`'s folder, if it has one, with only its
/// entries naming the file, and the folder they are read from: none when
/// there is no session file or no entry names the file.
pub(crate) fn listing_of(file: &Path) -> Result<Option<(Manifest, PathBuf)>, String> {
    let dir = match file.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let session_file = dir.join(take::MANIFEST);
    if !session_file.is_file() {
        return Ok(None);
    }
    listing_in(file, &session_file)
}

/// The session file `session_file` with only its entries naming `file`,
/// and the folder they are read from; none when no entry names it. Each
/// kept entry settles as it does in the whole session file.
fn listing_in(file: &Path, session_file: &Path) -> Result<Option<(Manifest, PathBuf)>, String> {
    let (mut m, base, _) = load_manifest_file(session_file)?;
    // An entry names the file as the app and `series` find it: its sound
    // is that file alone, as a path or as a folder (".", a take's own)
    // whose only sound it is (`take_path::lists`, #1573).
    let same = |e: &RecordingEntry| timegrapher_core::take_path::lists(&base, &e.file, file);
    // An entry carrying on the position of the one before it, in another
    // file, settles as session reads it, though that one is not kept here
    // (`take::continues_position`).
    let carried: Vec<bool> = m
        .recordings
        .iter()
        .map(|e| take::continues_position(e, &m))
        .collect();
    // An entry's own settling still comes first.
    for (e, _) in m.recordings.iter_mut().zip(carried).filter(|(_, c)| *c) {
        e.settle_s = e.settle_s.or(Some(0.0));
    }
    m.recordings.retain(same);
    Ok((!m.recordings.is_empty()).then_some((m, base)))
}

fn read_take_of(
    file: &Path,
    session_file: &Path,
    bph: Option<u32>,
    lift: Option<f64>,
    calibre: Option<String>,
) -> Result<Option<serde_json::Value>, String> {
    let Some((m, base)) = listing_in(file, session_file)? else {
        return Ok(None);
    };
    let o = Options {
        bph,
        lift,
        calibre,
        settle: None,
        out: None,
        json: true,
        init: false,
        no_shape: false,
        no_cycles: true,
    };
    let selection = o.calibre(&m)?;
    let ctx = Ctx {
        m: &m,
        calibre: &selection,
        base: &base,
        o: &o,
        tty: false,
        quiet: true,
        card_clock: false,
    };
    let mut readings = Vec::new();
    let mut left_out = Vec::new();
    for e in &m.recordings {
        position_of(e)?;
        match read_one_or_left_out(e, &ctx)? {
            Ok((r, _)) => readings.push(r),
            Err(l) => left_out.push(l),
        }
    }
    Ok(Some(serde_json::json!({
        "session_file": session_file,
        "readings": readings,
        "left_out": left_out,
    })))
}

pub(crate) fn read_one(e: &RecordingEntry, c: &Ctx) -> Result<Reading, String> {
    Ok(read_one_inner(e, c, false)?.0)
}

/// An entry of the session file with nothing to read in its recording,
/// and core's reason ([`take::NoReading`]). It is left out of the
/// readings, and the report says so.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LeftOut {
    pub file: String,
    /// As a reading names it ([`position_of`]): Witschi's code in the
    /// JSON.
    pub position: Option<Position>,
    pub start_s: f64,
    pub why: String,
}

impl LeftOut {
    fn new(e: &RecordingEntry, why: &take::NoReading) -> Self {
        // The file is named beside the reason, so not again inside it.
        let why = why.to_string();
        let why = why
            .strip_prefix(&format!("{}: ", e.file))
            .unwrap_or(&why)
            .to_string();
        LeftOut {
            file: e.file.clone(),
            position: position_of(e).ok(),
            start_s: e.start_s.unwrap_or(0.0),
            why,
        }
    }

    /// The line the summary and the note print.
    pub fn text(&self) -> String {
        format!(
            "{}, {}, at {} s is left out: {}",
            self.file,
            self.position
                .map_or("-".to_string(), Position::display_label),
            fixed_text(self.start_s, 0),
            self.why
        )
    }
}

/// [`read_one`], or the entry left out when it has nothing to read in its
/// recording ([`take::NoReading`]), so the rest are still read.
/// With the reading, a long take's run ([`long_run_of`]).
fn read_one_or_left_out(e: &RecordingEntry, c: &Ctx) -> Result<Result<ReadRun, LeftOut>, String> {
    if take::reads_nothing(e) {
        return Ok(Err(LeftOut::new(e, &take::NoReading::Nothing)));
    }
    match read_one_inner(e, c, false) {
        Ok((reading, _, run)) => Ok(Ok((reading, run))),
        Err(take::EntryError::NoReading(why)) => Ok(Err(LeftOut::new(e, &why))),
        Err(e) => Err(e.into()),
    }
}

/// Baseline writers request context from this same resolution/analysis pass.
pub(crate) fn read_one_with_context(
    e: &RecordingEntry,
    c: &Ctx,
) -> Result<(Reading, crate::regress_provenance::ReadContext), String> {
    let (reading, context, _) = read_one_inner(e, c, true)?;
    let context = context.ok_or_else(|| "missing baseline provenance context".to_string())?;
    Ok((reading, context))
}

/// The cycles found in an entry's reading, from its own windows; none for
/// noise, or where `--no-cycles` or the session file turns them off.
fn cycles_of(c: &Ctx, read: &take::EntryRead) -> Vec<session::Cycle> {
    if !read.measurement.watch_heard || c.o.no_cycles || c.m.cycles == Some(false) {
        Vec::new()
    } else {
        let lc = LongConfig {
            wheels: c
                .calibre
                .train
                .wheels(read.log.bph)
                .into_iter()
                .chain(c.m.wheels.iter().map(|(name, &period_s)| Wheel {
                    name: name.clone(),
                    period_s,
                }))
                .collect(),
            ..Default::default()
        };
        // The reading's own stretch, cut as the reading is.
        let part = read.measured_part();
        let mut long = longrun::analyse_with_breaks(&part.log, part.clock, &part.breaks, &lc);
        // The cycles the Steadiness pane names (#659).
        timegrapher_core::steadiness::family_long(&part.log, part.clock, &mut long, &lc);
        session::cycles(&long, &lc.wheels)
    }
}

/// The beat's shape of an entry's reading: as measured when a take kept
/// as its analysis was kept, else read from its sound; none for noise, or
/// where `--no-shape` or the session file turns it off.
fn shape_of(
    e: &RecordingEntry,
    c: &Ctx,
    read: &take::EntryRead,
) -> Result<Option<session::ShapeSummary>, take::EntryError> {
    let shape = if !read.measurement.watch_heard || c.o.no_shape || c.m.shape == Some(false) {
        None
    } else if let Some(k) = &read.kept {
        // Measured on the sound when the take was kept.
        let i =
            c.m.recordings
                .iter()
                .position(|r| std::ptr::eq(r, e))
                .or_else(|| {
                    c.m.recordings.iter().position(|r| {
                        r.file == e.file
                            && r.start_s == e.start_s
                            && r.end_s == e.end_s
                            && r.position == e.position
                    })
                });
        i.and_then(|i| k.shape_of(i))
            .map(|s| session::ShapeSummary {
                from_s: s.summary.from_s.or(Some(s.from_s)),
                ..s.summary.clone()
            })
    } else if let Some((from, len)) =
        analysis_take::shape_stretch(&read.masked, read.settle_s, read.end_s)
    {
        let (x, fs) = excerpt(&read.files, from, len)?;
        analysis_take::measure_shape(&x, fs, read.log.bph, from)
    } else {
        None
    };
    Ok(shape)
}

fn read_one_inner(
    e: &RecordingEntry,
    c: &Ctx,
    capture: bool,
) -> Result<
    (
        Reading,
        Option<crate::regress_provenance::ReadContext>,
        EntryRun,
    ),
    take::EntryError,
> {
    let position = position_of(e)?;
    let (cfg, cal) = stream_config(c)?;
    if c.quiet {
    } else if c.tty {
        eprint!("\r{:<70}", format!("analysing {} ({})", e.file, position));
    } else {
        eprintln!("analysing {} ({})", e.file, position);
    }
    // The take opened as every surface opens it.
    let settings = take::EntrySettings {
        stream: cfg.clone(),
        card_clock: c.card_clock,
        settle_s: c.o.settle,
    };
    let read = take::measure_entry(c.base, c.m, e, &settings, take::usb, |first| {
        take::correction_for(c.m, first, None, None)
    })?;
    if capture && read.kept.is_some() {
        return Err(format!(
            "{}: kept as its analysis alone, with no sound to record provenance for",
            e.file
        )
        .into());
    }
    let cycles = cycles_of(c, &read);
    let shape = shape_of(e, c, &read)?;
    // A take of one entry is read whole here; one of several is read
    // whole once, after all of them ([`whole_take_run`], #1150).
    let run = if c.m.recordings.len() == 1 {
        long_run_of(c, &read)
    } else {
        None
    };
    let heard_s = read.log.duration_s;
    let take::EntryRead {
        files,
        log,
        clock,
        clock_text: log_text,
        breaks,
        start_s: start,
        stop_s: stop,
        settle_s: settle,
        end_s: end,
        measurement,
        ..
    } = read;
    let clock_input_sha256 = log_text
        .as_ref()
        .map(|t| crate::archive::sha256_hex(t.as_bytes()));

    let label = if e.start_s.is_some() || e.end_s.is_some() {
        format!(
            "{} ({}–{} s)",
            e.file,
            fixed_text(start, 0),
            fixed_text(stop, 0)
        )
    } else {
        e.file.clone()
    };
    let context = if capture {
        Some(crate::regress_provenance::ReadContext::capture(
            &files,
            c.base,
            &cfg,
            serde_json::json!({
                "selected_start_s": start, "selected_end_s": stop,
                "measured_start_s": settle, "measured_end_s": end,
                "audio_duration_s": log.duration_s, "sample_rate": log.sample_rate,
                "detected_bph": log.bph, "resolved_lift_known": lift_known(c, cal),
                "clock_input": e.clock, "clock_input_sha256": clock_input_sha256,
                "card_ppm": c.m.card_ppm,
                "clock_fit_reported_summary": clock,
                "applied_breaks": breaks,
                "no_shape": c.o.no_shape, "no_cycles": c.o.no_cycles,
            }),
        )?)
    } else {
        None
    };
    Ok((
        Reading {
            bph_set: cfg.analysis.bph,
            label,
            position,
            wind_h: e.wind_h,
            date: e.date.clone(),
            notes: e.notes.clone(),
            duration_s: stop - start,
            bph: log.bph,
            measurement,
            cycles,
            shape,
            reference: e.reference.clone(),
        },
        context,
        (run, heard_s),
    ))
}

/// The run of an entry when it is a take of its own and a long one, and
/// the seconds of sound it holds.
type EntryRun = (Option<power_reserve::Results>, f64);

/// A reading, with its entry's [`EntryRun`].
type ReadRun = (Reading, EntryRun);

/// The run of an entry that is a long take in the session's one position,
/// read as a Power Reserve Test's run (core
/// [`power_reserve::from_long_take`]), as the app reads it.
fn long_run_of(c: &Ctx, read: &take::EntryRead) -> Option<power_reserve::Results> {
    power_reserve::from_long_take(
        &read.log,
        read.clock.as_ref(),
        &read.breaks,
        &session::with_holes(&read.masked, &read.holes()),
        &positions_listed(c),
        "",
        read.kept.is_none(),
    )
}

/// Everything the report and `summary.json` show.
#[derive(serde::Serialize)]
pub struct Session<'a> {
    /// Name and version of this layout; bumped when a field changes meaning.
    pub schema: String,
    /// Name, version and platform of the software, as in every `--json`
    /// document (see docs/reference/agent-interface.md).
    pub software: serde_json::Value,
    /// The recordings read and the settings used.
    pub input: serde_json::Value,
    pub watch: Option<&'a str>,
    pub calibre: Option<&'a str>,
    pub owner: Option<&'a str>,
    pub notes: Option<&'a str>,
    pub bph: u32,
    pub lift_deg: f64,
    /// Whether `lift_deg` came from `--lift`, the session file or the
    /// calibre rather than the default; the text and the HTML report say
    /// so when it did not (the readings carry the caveat too).
    #[serde(skip)]
    pub lift_known: bool,
    pub readings: Vec<Reading>,
    /// Entries with nothing to read, and why: they are not in `readings`.
    pub left_out: Vec<LeftOut>,
    pub report: SessionReport,
    /// A Power Reserve Test's run, from the folder's `power-reserve.json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_reserve: Option<timegrapher_core::power_reserve::Results>,
    /// The Dynamic Poise fit of each state of wind with five or more
    /// upright readings ([`timegrapher_core::poise::by_state`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub poise: Vec<timegrapher_core::poise::StateFit>,
}

impl Session<'_> {
    /// The lift angle as the text and the HTML report show it: core's
    /// caveat wording ("lift angle 52°, the default") when it is the
    /// default rather than one set for the watch.
    pub fn lift_text(&self) -> String {
        if self.lift_known {
            format!("lift angle {}°", self.lift_deg)
        } else {
            timegrapher_core::measure::Caveat::LiftUnknown
                .short(self.lift_deg, 0)
                .unwrap_or_default()
        }
    }
}

pub fn run(paths: &[PathBuf], o: &Options) -> Result<(), String> {
    check_seconds("--settle", o.settle, 0.0, false)?;
    if o.init {
        return init(paths, o);
    }
    let (m, base) = load_manifest(paths)?;
    // The take's files in the order a folder of them is joined in, as the
    // app opens it.
    if let Ok(files) = take::sound_files(std::slice::from_ref(&base)) {
        crate::long::note_join_order(&files);
    }
    let selection = o.calibre(&m)?;
    let cal = selection.train.calibre.as_ref();
    let calibre_notes = o.calibre_notes(&m, &selection);
    for n in &calibre_notes {
        eprintln!("note: {n}");
    }
    for e in &m.recordings {
        position_of(e)?;
    }
    let ctx = Ctx {
        m: &m,
        calibre: &selection,
        base: &base,
        o,
        tty: std::io::stderr().is_terminal(),
        quiet: false,
        card_clock: false,
    };
    let (readings, left_out, long_run) = read_all(&m, &ctx)?;
    if readings.is_empty() {
        return Err(format!(
            "{}: no recording listed with anything to read",
            base.join(take::MANIFEST).display()
        ));
    }
    if ctx.tty {
        eprintln!();
    }
    let report = session::evaluate(&readings);
    let bph = readings.first().map_or(0, |r| r.bph);
    let files: Vec<PathBuf> = m.recordings.iter().map(|e| base.join(&e.file)).collect();
    let settings = serde_json::json!({
        "bph": o.bph(&m, cal),
        "lift_deg": o.lift(&m, cal),
        "calibre": cal.map(|c| c.name.as_str()),
        "calibre_notes": calibre_notes,
        "settle_s": o.settle,
        "shape": !o.no_shape,
        "cycles": !o.no_cycles,
        "analysis_only": kept_note(&base, &m),
    });
    let s = Session {
        schema: format!(
            "timegrapher.session/{}",
            crate::output::schema_version("session")
        ),
        software: crate::output::software(),
        input: crate::output::input(&files, settings),
        watch: m.watch.as_deref().filter(|w| !w.is_empty()),
        calibre: m
            .calibre
            .as_deref()
            .filter(|w| !w.is_empty())
            .or(cal.map(|c| c.name.as_str())),
        owner: m.owner.as_deref(),
        notes: m.notes.as_deref(),
        bph,
        lift_deg: o.lift(&m, cal),
        lift_known: lift_known(&ctx, cal),
        poise: timegrapher_core::poise::by_state(&readings),
        readings,
        left_out,
        report,
        // A test's kept run, else a long take's read as one.
        power_reserve: power_reserve_of(&base)?.or(long_run),
    };
    write_out(&s, o, &base)
}

/// Every entry of the session file read: an entry with nothing to read is
/// left out, with a note, and the report lists it.
/// With them, the take's run when it is a long one: a take of one entry
/// from that entry's read ([`long_run_of`]), one of several read whole
/// once ([`whole_take_run`]), as the app opens it (#1150).
fn read_all(m: &Manifest, ctx: &Ctx<'_>) -> Result<ReadAll, String> {
    let mut readings = Vec::new();
    let mut left_out = Vec::new();
    let mut long_run = None;
    let mut heard_s = 0.0;
    for e in &m.recordings {
        match read_one_or_left_out(e, ctx)? {
            Ok((r, (run, s))) => {
                readings.push(r);
                long_run = long_run.or(run);
                heard_s += s;
            }
            Err(l) => {
                if ctx.tty {
                    eprintln!();
                }
                eprintln!("note: {}", l.text());
                left_out.push(l);
            }
        }
    }
    // No run is that long in less sound: the take is read again only when
    // it could hold one.
    if m.recordings.len() > 1 && heard_s >= power_reserve::LONG_TAKE_S {
        long_run = whole_take_run(ctx)?;
    }
    Ok((readings, left_out, long_run))
}

/// The run of the take `c` names read whole, once, as its take context
/// (core [`TakeContext`]: every recording end to end, each entry's masks
/// at its recording's offset), when it is a long take in one position
/// ([`power_reserve::from_long_take`]): a take of several entries is one
/// run however its files are listed, as the app opens a take's folder
/// (#1150), cut where the take's readings are cut. None for a take whose entries name more than one clock log,
/// which core can't read whole.
///
/// [`TakeContext`]: timegrapher_core::take_context::TakeContext
fn whole_take_run(c: &Ctx) -> Result<Option<power_reserve::Results>, String> {
    use timegrapher_core::take_context::{Masks, TakeContext};
    let mut clocks: Vec<&str> =
        c.m.recordings
            .iter()
            .filter_map(|e| e.clock.as_deref())
            .collect();
    clocks.sort_unstable();
    clocks.dedup();
    if clocks.len() > 1 {
        return Ok(None);
    }
    let (cfg, _) = stream_config(c)?;
    let settings = take::EntrySettings {
        stream: cfg,
        card_clock: c.card_clock,
        settle_s: c.o.settle,
    };
    if !c.quiet {
        eprintln!("analysing the take whole for its run");
    }
    let (ctx, log) = TakeContext::open(
        c.base,
        c.m,
        settings,
        Masks::Cut,
        take::usb,
        |first| take::correction_for(c.m, first, None, None),
        |_, _| {},
    )?;
    // Cut where the take's readings are cut, as the app cuts the take it
    // opens.
    let breaks = ctx.reading_breaks(c.m);
    // Its settlings left out as a mask is (#1233); the run still starts at
    // its first beat.
    let masked = session::with_holes(&ctx.masked, &ctx.holes(c.m));
    Ok(power_reserve::from_long_take(
        &log,
        ctx.clock.as_ref(),
        &breaks,
        &masked,
        &positions_listed(c),
        "",
        !ctx.files.is_empty(),
    ))
}

/// The positions the take's entries name, as core names them.
fn positions_listed(c: &Ctx) -> Vec<&'static str> {
    c.m.recordings
        .iter()
        .filter_map(|e| position_of(e).ok())
        .map(Position::common)
        .collect()
}

/// The readings, the entries left out, and a long take's run.
type ReadAll = (Vec<Reading>, Vec<LeftOut>, Option<power_reserve::Results>);

/// The summary, readings and report written to the report folder, and the
/// summary printed (or its JSON with `--json`).
fn write_out(s: &Session, o: &Options, base: &Path) -> Result<(), String> {
    let out = o.out.clone().unwrap_or_else(|| base.join(take::REPORT_DIR));
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let w = |name: &str, text: &str| {
        let p = out.join(name);
        fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
    };
    w("summary.json", &json)?;
    w("readings.csv", &csv(s))?;
    w("report.html", &crate::session_report::html(s))?;
    if o.json {
        println!("{json}");
    } else {
        print_summary(s);
        let report = out.join("report.html");
        println!(
            "Report       {}",
            std::path::absolute(&report).unwrap_or(report).display()
        );
        println!(
            "             One page in English: open it in any web browser, or attach it to an email to send it."
        );
    }
    Ok(())
}

/// The Power Reserve Test's results in the session folder `base`, if it
/// holds them; an unreadable file is an error, not left out.
fn power_reserve_of(
    base: &Path,
) -> Result<Option<timegrapher_core::power_reserve::Results>, String> {
    use timegrapher_core::power_reserve::{Results, FILE};
    match fs::read_to_string(base.join(FILE)) {
        Ok(t) => Results::from_json(&t)
            .map(Some)
            .map_err(|e| format!("{}: {e}", base.join(FILE).display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", base.join(FILE).display())),
    }
}

/// For a session file whose take was kept as its analysis alone: the fact,
/// and the software that measured its sound (the beats, windows and beat
/// shape are as that version found them). Null otherwise.
fn kept_note(base: &Path, m: &Manifest) -> serde_json::Value {
    if m.audio != Some(false) {
        return serde_json::Value::Null;
    }
    match fs::read_to_string(base.join(analysis_take::DIR).join(analysis_take::HEADER))
        .ok()
        .and_then(|t| serde_json::from_str::<analysis_take::Header>(&t).ok())
    {
        Some(h) => serde_json::json!({ "note": h.note, "measured_by": h.software }),
        None => serde_json::json!({ "note": analysis_take::NO_SOUND }),
    }
}

pub fn opt(v: Option<f64>, digits: usize) -> String {
    v.filter(|x| x.is_finite())
        .map_or("-".into(), |x| fixed_text(x, digits))
}

pub fn signed(v: Option<f64>, digits: usize) -> String {
    v.filter(|x| x.is_finite()).map_or("-".into(), |x| {
        timegrapher_core::library::signed_text(x, digits)
    })
}

/// The state of wind in words: `wind_h` is hours since the watch was
/// fully wound. A reading without it is "not given", never fully wound.
pub fn wind(w: Option<f64>) -> String {
    match w {
        None => "not given".into(),
        Some(w) => timegrapher_core::reading_text::wind_words(w),
    }
}

/// A state of wind in a sentence ("at full wind", "24 h after winding",
/// "wind not given").
fn wind_state(w: Option<f64>) -> String {
    let words = timegrapher_core::reading_text::wind_state_words(w);
    if w.is_some_and(|w| w <= 0.0) {
        format!("at {words}")
    } else {
        words
    }
}

fn csv(s: &Session) -> String {
    let mut t = String::from(
        "recording,position,wind_h,measured_s,beats,clean_fraction,calibrated,rate_s_per_day,rate_window_median_s_per_day,amplitude_deg,beat_error_unlock_ms,beat_error_drop_ms,jitter_us,rate_p05,rate_p95,clipped_fraction,ref_rate,ref_amplitude,ref_beat_error\n",
    );
    for r in &s.readings {
        let m = &r.measurement;
        let rf = r.reference.clone().unwrap_or_default();
        t.push_str(&format!(
            "{},{},{},{:.1},{},{:.3},{},{},{},{},{},{},{},{},{},{:.4},{},{},{}\n",
            r.label.replace(',', ";"),
            r.position,
            opt(r.wind_h, 1).replace('-', ""),
            m.measured_s(),
            m.beats,
            m.clean_fraction,
            m.calibrated,
            opt(m.rate_s_per_day, 2),
            opt(m.rate_window_median_s_per_day, 2),
            opt(m.amplitude_deg, 1),
            opt(m.beat_error_unlock_ms.map(f64::abs), 3),
            opt(m.beat_error_ms.map(f64::abs), 3),
            opt(m.jitter_us, 0),
            opt(m.rate_p05, 1),
            opt(m.rate_p95, 1),
            m.clipped_fraction,
            opt(rf.rate_s_per_day, 1),
            opt(rf.amplitude_deg, 0),
            opt(rf.beat_error_ms, 2),
        ));
    }
    t.replace(",-,", ",,")
        .replace(",-,", ",,")
        .replace(",-\n", ",\n")
}

fn finding_lines(f: &Finding) {
    println!("  {}", f.title);
    println!("    {}", f.evidence);
}

fn print_summary(s: &Session) {
    print_watch(s);
    println!();
    print_readings(s);
    print_notes(s);
    print_states(s);
    print_poise(s);
    println!();
    print_findings(&s.report);
}

/// The Dynamic Poise fit of each state of wind that gives one.
fn print_poise(s: &Session) {
    for p in &s.poise {
        println!();
        println!("Poise {}: {}", wind_state(p.wind_h), p.fit.text);
    }
}

/// The watch, its beat rate and lift, and the power reserve run.
fn print_watch(s: &Session) {
    let name = match (s.watch, s.calibre) {
        (Some(w), Some(c)) => format!("{w}, calibre {c}"),
        (Some(w), None) => w.to_string(),
        (None, Some(c)) => format!("calibre {c}"),
        (None, None) => String::new(),
    };
    println!(
        "Watch        {}{} bph, {}",
        if name.is_empty() {
            String::new()
        } else {
            format!("{name}; ")
        },
        s.bph,
        s.lift_text()
    );
    if let Some(p) = &s.power_reserve {
        println!("{}", p.sentence(&p.position));
    }
}

/// One line for each position's reading, under its heading.
fn print_readings(s: &Session) {
    println!(
        "{:<21} {:<18} {:>7} {:>5} {:>5} {:>5} {:>7} {:>15} {:>6} {:>5} {:>17}  Recording",
        "Position",
        "Wind",
        "Rate",
        "Amp",
        "BE",
        "drop",
        "Jitter",
        "10 s rates",
        "Unlock",
        "Clip",
        "Reference"
    );
    println!(
        "{:<21} {:<18} {:>7} {:>5} {:>5} {:>5} {:>7} {:>15} {:>6} {:>5} {:>17}",
        "", "", "s/d", "deg", "ms", "ms", "us", "s/d", "%", "%", "s/d, deg, ms"
    );
    for rd in &s.readings {
        let m = &rd.measurement;
        let rf = rd
            .reference
            .as_ref()
            .map(|f| {
                format!(
                    "{} {} {}",
                    signed(f.rate_s_per_day, 0),
                    opt(f.amplitude_deg, 0),
                    opt(f.beat_error_ms, 1)
                )
            })
            .unwrap_or_default();
        let clock = if m.calibrated { "" } else { " (card clock)" };
        if !m.watch_heard {
            println!(
                "{:<21} {:<18} {:<62} {:>17}  {}{clock}",
                rd.position.display_label(),
                wind(rd.wind_h),
                session::no_regular_beat_in(m, rd.bph_set),
                rf,
                rd.label,
            );
            continue;
        }
        println!(
            "{:<21} {:<18} {:>7} {:>5} {:>5} {:>5} {:>7} {:>15} {:>6} {:>5} {:>17}  {}{clock}",
            rd.position.display_label(),
            wind(rd.wind_h),
            signed(m.rate_s_per_day, 1),
            opt(m.amplitude_deg, 0),
            opt(m.beat_error(), 2),
            opt(m.beat_error_ms.map(f64::abs), 2),
            opt(m.jitter_us, 0),
            format!("{}..{}", signed(m.rate_p05, 0), signed(m.rate_p95, 0)),
            opt(m.unlock_coverage.map(|c| 100.0 * c), 0),
            opt(Some(100.0 * m.clipped_fraction), 1),
            rf,
            rd.label,
        );
    }
}

/// What was left out, and what the columns and readings mean.
fn print_notes(s: &Session) {
    for l in &s.left_out {
        println!("Left out     {}", l.text());
    }
    println!(
        "Unlock is the share of 2 s windows that found the unlock on both sides, on which amplitude and BE rest; Clip is the share of beats with a clipped sample."
    );
    println!(
        "BE is beat error as a size; which beat is longer is not shown, because it depends on the beat the analysis locked on first."
    );
    println!(
        "Readings leave out the settling time at the start of each position, as record's quick check does. That check uses the session file's settle_s (20 s when the file has none, as this summary does) and none of --settle, --bph, --lift, --calibre, a reading's own settle_s, measure_s, start_s, end_s, a clock log, card_ppm_fast or a restart in listening, so a reading that uses one of these can differ from it."
    );
    if s.readings.iter().any(|r| r.measurement.calibrated) {
        println!(
            "The sound card's clock is corrected where a clock log or card_ppm_fast is given."
        );
    }
    if s.readings.iter().any(|r| r.wind_h.is_none()) {
        println!(
            "Wind is the hours since the watch was fully wound (wind_h in the session file); readings where it is not given are their own state, never taken as fully wound, and are left out of the isochronism and Ie."
        );
    }
    println!();
}

/// Each reading's states, the wind states, isochronism and Im, Ie.
fn print_states(s: &Session) {
    let r = &s.report;
    for rd in s.readings.iter().filter(|r| r.measurement.watch_heard) {
        let m = &rd.measurement;
        println!(
            "States       {:<21} {}; {}",
            rd.position.display_label(),
            twostate::describe(&m.amplitude_states, "amplitude"),
            twostate::describe(&m.rate_states, "rate")
        );
    }
    println!();
    for st in &r.states {
        println!(
            "{:<21} X {} s/d (H {}, V {}); D {} s/d, {} deg; DVH {} s/d, {} deg; Di {} s/d",
            {
                let w = wind_state(st.wind_h);
                w[..1].to_uppercase() + &w[1..]
            },
            signed(st.x, 1),
            signed(st.xh, 1),
            signed(st.xv, 1),
            opt(st.d_rate, 1),
            opt(st.d_amplitude, 0),
            signed(st.dvh_rate, 1),
            signed(st.dvh_amplitude, 0),
            signed(st.di, 1)
        );
    }
    for i in &r.isochronism {
        println!(
            "Isochronism  {} {} s/d from {} to {} ({} deg, beat error {} ms)",
            i.position.display_label(),
            signed(Some(i.rate_change), 1),
            wind_state(Some(i.from_wind_h)).trim_start_matches("at "),
            wind_state(Some(i.to_wind_h)).trim_start_matches("at "),
            signed(i.amplitude_change, 0),
            signed(i.beat_error_change, 2)
        );
    }
    if r.im.is_some() || r.ie.is_some() {
        println!(
            "Im {} s/d, Im* {} s/d, Ie {} s/d",
            signed(r.im, 1),
            signed(r.im_all, 1),
            opt(r.ie, 1)
        );
    }
}

fn print_findings(r: &SessionReport) {
    if r.findings.is_empty() {
        println!("Findings     none");
    } else {
        println!("Findings");
        for f in &r.findings {
            finding_lines(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `analyze FILE` reads a file's entry over the stretch `session`
    /// reads it over: one carrying on the position of the file before it
    /// settles no more, and one with its own settling keeps it (#1145).
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a test checking session's reading against core's own window"
    )]
    fn a_files_entry_alone_settles_as_in_the_whole_session_file() {
        let dir = std::env::temp_dir().join(format!("tg-listing-settle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let audio = timegrapher_core::audio::Audio {
            sample_rate: 100,
            samples: vec![0.0; 6000],
        };
        for f in ["a.wav", "b.wav", "g.wav"] {
            timegrapher_core::audio::write_wav(&dir.join(f), &audio).unwrap();
        }
        let text = "[[recording]]\nfile = 'a.wav'\nposition = 'DU'\n\n\
                    [[recording]]\nfile = 'b.wav'\nposition = 'DU'\n\n\
                    [[recording]]\nfile = 'g.wav'\nposition = 'DU'\nsettle_s = 40.0\n";
        let session_file = dir.join(take::MANIFEST);
        fs::write(&session_file, text).unwrap();
        let whole: Manifest = toml::from_str(text).unwrap();
        let from = |e: &RecordingEntry, m: &Manifest| {
            take::reading_window(e, m, None, 60.0).unwrap().from_s
        };
        let mut read = Vec::new();
        for (i, f) in ["a.wav", "b.wav", "g.wav"].iter().enumerate() {
            let (alone, _) = listing_in(&dir.join(f), &session_file).unwrap().unwrap();
            let e = &alone.recordings[0];
            assert_eq!(from(e, &alone), from(&whole.recordings[i], &whole), "{f}");
            read.push(from(e, &alone));
        }
        assert_eq!(read, [take::SETTLE_S, 0.0, 40.0]);
        fs::remove_dir_all(&dir).unwrap();
    }

    fn options(bph: Option<u32>, lift: Option<f64>, calibre: Option<&str>) -> Options {
        Options {
            bph,
            lift,
            calibre: calibre.map(String::from),
            settle: None,
            out: None,
            json: false,
            init: false,
            no_shape: false,
            no_cycles: false,
        }
    }

    #[test]
    fn a_defaulted_lift_angle_is_unknown() {
        let base = Path::new(".");
        let known = |o: &Options, m: &Manifest| {
            let selection = o.calibre(m).unwrap();
            let cal = selection.train.calibre.as_ref();
            let c = Ctx {
                m,
                calibre: &selection,
                base,
                o,
                tty: false,
                quiet: true,
                card_clock: false,
            };
            lift_known(&c, cal)
        };
        let none = Manifest::default();
        assert!(!known(&options(None, None, None), &none));
        assert!(known(&options(None, Some(50.0), None), &none));
        assert!(known(&options(None, None, Some("Unitas 6497-2")), &none));
        let file: Manifest = toml::from_str("lift = 50\n").unwrap();
        assert!(known(&options(None, None, None), &file));
        // A calibre the table does not know leaves the lift angle unknown.
        let label: Manifest = toml::from_str("calibre = \"Bespoke 9999\"\n").unwrap();
        assert!(!known(&options(None, None, None), &label));
    }

    #[test]
    fn persisted_lift_context_follows_the_winning_value() {
        let base = Path::new(".");
        for (text, flag, expected) in [
            ("lift = 52\nlift_known = false", None, false),
            ("lift = 52\nlift_known = true", None, true),
            ("lift = 49\nlift_known = false", None, false),
            ("lift = 52\nlift_known = false", Some(52.0), true),
            ("lift = 52", None, false), // An older file's default: assumed (#685).
            ("lift = 49", None, true),  // Any other value was supplied.
            ("lift_known = true", None, false), // No accompanying value.
            (
                "calibre = \"Unitas 6497-2\"\nlift_known = false",
                None,
                true,
            ),
            ("calibre = \"Bespoke 9999\"", None, false),
        ] {
            let m: Manifest = toml::from_str(text).unwrap();
            let o = options(None, flag, None);
            let selection = o.calibre(&m).unwrap();
            let c = Ctx {
                m: &m,
                calibre: &selection,
                base,
                o: &o,
                tty: false,
                quiet: true,
                card_clock: false,
            };
            let (cfg, cal) = stream_config(&c).unwrap();
            assert_eq!(lift_known(&c, cal), expected, "{text}, {flag:?}");
            assert_eq!(cfg.analysis.lift_known, expected);
            let encoded = toml::to_string(&m).unwrap();
            let again: Manifest = toml::from_str(&encoded).unwrap();
            assert_eq!(again.lift, m.lift);
            assert_eq!(again.lift_known, m.lift_known);
        }
    }

    #[test]
    fn calibre_fills_what_the_flags_and_session_file_leave_out() {
        let none = Manifest::default();
        let o = options(None, None, Some("Unitas 6497-2"));
        let selection = o.calibre(&none).unwrap();
        let cal = selection.train.calibre.as_ref();
        assert_eq!(o.bph(&none, cal), Some(21600));
        assert_eq!(o.lift(&none, cal), 44.0);
        // The session file wins over the calibre, and a flag over both.
        let file: Manifest = toml::from_str("bph = 18000\nlift = 50\n").unwrap();
        assert_eq!(o.bph(&file, cal), Some(18000));
        assert_eq!(o.lift(&file, cal), 50.0);
        let o = options(Some(28800), Some(53.0), Some("Unitas 6497-2"));
        assert_eq!(o.bph(&file, cal), Some(28800));
        assert_eq!(o.lift(&file, cal), 53.0);
        // No calibre: measured beat rate and 52 degrees, as before.
        let o = options(None, None, None);
        assert_eq!(o.bph(&none, None), None);
        assert_eq!(o.lift(&none, None), 52.0);
        // An unknown name lists the known ones.
        let e = options(None, None, Some("no such calibre"))
            .calibre(&Manifest::default())
            .unwrap_err();
        assert!(e.contains("Unitas 6497-2"), "{e}");
    }

    #[test]
    fn the_session_files_calibre_line_counts_when_the_table_knows_it() {
        let o = options(None, None, None);
        let m: Manifest = toml::from_str("calibre = \"2824-2\"\n").unwrap();
        let selection = o.calibre(&m).unwrap();
        let cal = selection.train.calibre.as_ref();
        assert_eq!(cal.map(|c| c.name.as_str()), Some("ETA 2824-2"));
        assert_eq!(o.bph(&m, cal), Some(28800));
        assert_eq!(o.lift(&m, cal), 50.0);
        assert!(o.calibre_notes(&m, &selection).is_empty());
        // A name the table does not know is only a label.
        let m: Manifest = toml::from_str("calibre = \"Bespoke 9999\"\n").unwrap();
        assert!(o.calibre(&m).unwrap().train.calibre.is_none());
        // --calibre wins over the line, and explicit values over both, with
        // a note for each disagreement.
        let m: Manifest =
            toml::from_str("calibre = \"2824-2\"\nbph = 21600\nlift = 52\nescape_teeth = 15\n")
                .unwrap();
        let selection = o.calibre(&m).unwrap();
        let cal = selection.train.calibre.as_ref();
        assert_eq!(o.bph(&m, cal), Some(21600));
        assert_eq!(o.lift(&m, cal), 52.0);
        assert_eq!(
            o.calibre_notes(&m, &selection).len(),
            3,
            "{:?}",
            o.calibre_notes(&m, &selection)
        );
        let o = options(None, Some(55.0), Some("Rolex 3235"));
        let selection = o.calibre(&m).unwrap();
        let cal = selection.train.calibre.as_ref();
        assert_eq!(cal.map(|c| c.name.as_str()), Some("Rolex 3235"));
        assert_eq!(o.lift(&m, cal), 55.0);
        let notes = o.calibre_notes(&m, &selection);
        assert!(notes[0].contains("ETA 2824-2"), "{notes:?}");
        assert_eq!(notes.len(), 3, "{notes:?}");
    }

    /// The user catalogue the custom-calibre tests read: a custom calibre
    /// with an alias and a wheel, and a wheel override for a shipped one.
    fn custom_user() -> UserCalibres {
        serde_json::from_value(serde_json::json!({
            "schema": calibre_store::SCHEMA,
            "custom":[{"name":"Bench 44","aliases":["Workshop-A"],"bph":21600,"lift_angle_deg":44.0,"escape_teeth":20,"wheels":[{"name":"third wheel","period_s":123.0}]}],
            "wheels":[{"calibre":"Rolex 3235","name":"third wheel","period_s":241.0}],
        })).unwrap()
    }

    /// The analysis settings a session file reads with under these options.
    fn analysis_of(
        m: &Manifest,
        calibre: &SessionCalibre,
        o: &Options,
    ) -> timegrapher_core::analysis::AnalysisConfig {
        let ctx = Ctx {
            m,
            calibre,
            base: Path::new("."),
            o,
            tty: false,
            quiet: true,
            card_clock: false,
        };
        stream_config(&ctx).unwrap().0.analysis
    }

    #[test]
    fn custom_session_catalogue_preserves_value_context_precedence() {
        let user = custom_user();
        for (text, flag_bph, flag_lift, expected) in [
            (
                "calibre = \"Workshop-A\"",
                None,
                None,
                (Some(21600), 44.0, true),
            ),
            (
                "calibre = \"Workshop-A\"\nbph = 18000\nlift = 52\nlift_known = false",
                None,
                None,
                (Some(18000), 52.0, false),
            ),
            (
                "calibre = \"Workshop-A\"\nbph = 18000\nlift = 52\nlift_known = false",
                Some(28800),
                Some(52.0),
                (Some(28800), 52.0, true),
            ),
            (
                "calibre = \"Unknown label\"\nlift_known = true",
                None,
                None,
                (None, 52.0, false),
            ),
        ] {
            let m: Manifest = toml::from_str(text).unwrap();
            let o = options(flag_bph, flag_lift, None);
            let selection = o.calibre_with(&m, &user).unwrap();
            let a = analysis_of(&m, &selection, &o);
            assert_eq!((a.bph, a.amplitude.lift_deg, a.lift_known), expected);
        }
    }

    #[test]
    fn custom_session_catalogue_is_a_snapshot() {
        let mut user = custom_user();
        let m: Manifest = toml::from_str("calibre = \"Workshop-A\"").unwrap();
        let o = options(None, None, None);
        let selected = o.calibre_with(&m, &user).unwrap();
        assert_eq!(selected.train.calibre.as_ref().unwrap().name, "Bench 44");
        assert!(selected
            .train
            .wheels(21600)
            .iter()
            .any(|w| w.name == "third wheel" && w.period_s == 123.0));
        user.custom[0].lift_angle_deg = Some(60.0);
        user.custom[0].wheels[0].period_s = 999.0;
        assert_eq!(analysis_of(&m, &selected, &o).amplitude.lift_deg, 44.0);
        assert!(selected
            .train
            .wheels(21600)
            .iter()
            .any(|w| w.period_s == 123.0));
        assert!(options(None, None, Some("Unknown explicit"))
            .calibre_with(&m, &user)
            .is_err());
    }

    #[test]
    fn a_shipped_calibres_wheel_override_is_read_unless_the_catalogue_is_empty() {
        let user = custom_user();
        let o = options(None, None, None);
        let shipped: Manifest = toml::from_str("calibre = \"Rolex 3235\"").unwrap();
        let selected = o.calibre_with(&shipped, &user).unwrap();
        assert!(selected
            .train
            .wheels(28800)
            .iter()
            .any(|w| w.name == "third wheel" && w.period_s == 241.0));
        let corpus = o.calibre_with(&shipped, &UserCalibres::default()).unwrap();
        assert!(
            !corpus
                .train
                .wheels(28800)
                .iter()
                .any(|w| w.name == "third wheel" && w.period_s == 241.0),
            "regression adapter's explicit empty catalogue ignores ambient overrides"
        );
    }

    #[test]
    fn consumed_manifest_toml_validation_keeps_aliases_legacy_fields_and_context() {
        for (text, field) in [
            ("bph=0", "bph"),
            ("lift=nan", "lift"),
            ("lift_angle=0", "lift"),
            ("escape_teeth=0", "escape_teeth"),
            ("card_ppm_fast=1000000", "card_ppm_fast"),
        ] {
            let m: Manifest = toml::from_str(text).unwrap();
            assert!(m
                .validate_analysis_inputs()
                .unwrap_err()
                .starts_with(&format!("{field}:")));
        }
        let text="calibre=\"Unknown label\"\nlift_angle=52\nlift_known=false\ncard_ppm_fast=0\ntolerance=\"old\"\n[limits]\nrate=99\n";
        let m: Manifest = toml::from_str(text).unwrap();
        m.validate_analysis_inputs().unwrap();
        assert_eq!(m.lift, Some(52.0));
        assert_eq!(m.lift_known, Some(false));
        assert_eq!(m.calibre.as_deref(), Some("Unknown label"));
        assert_eq!(m.card_ppm, Some(0.0));
        let encoded = toml::to_string(&m).unwrap();
        let again: Manifest = toml::from_str(&encoded).unwrap();
        again.validate_analysis_inputs().unwrap();
        assert_eq!(again.lift_known, m.lift_known);
        assert!(toml::from_str::<Manifest>("no_such_key=1").is_err());
    }

    #[test]
    fn a_session_file_with_impossible_times_is_refused_before_any_audio() {
        let dir = std::env::temp_dir().join(format!("tg-times-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join(MANIFEST);
        for (text, field) in [
            ("settle_s = 0\nmeasure_s = -20\n", "measure_s"),
            ("settle_s = -20\nmeasure_s = 1\n", "settle_s"),
            ("measure_s = 0\n", "measure_s"),
            ("settle_s = nan\n", "settle_s"),
            (
                "[[recording]]\nfile = \"a.wav\"\nstart_s = nan\n",
                "a.wav: start_s",
            ),
            (
                "[[recording]]\nfile = \"a.wav\"\nstart_s = -1\n",
                "a.wav: start_s",
            ),
            (
                "[[recording]]\nfile = \"a.wav\"\nend_s = 0\n",
                "a.wav: end_s",
            ),
            (
                "[[recording]]\nfile = \"a.wav\"\nsettle_s = inf\n",
                "a.wav: settle_s",
            ),
            (
                "[[recording]]\nfile = \"a.wav\"\nmeasure_s = -5\n",
                "a.wav: measure_s",
            ),
        ] {
            let text = if text.contains("[[recording]]") {
                text.to_string()
            } else {
                format!("{text}[[recording]]\nfile = \"a.wav\"\n")
            };
            fs::write(&f, &text).unwrap();
            let err = load_manifest_file(&f).unwrap_err();
            assert!(err.contains(&format!("{field} is")), "{text}: {err}");
        }
        fs::write(
            &f,
            "settle_s = 0\nmeasure_s = 10\n[[recording]]\nfile = \"a.wav\"\nstart_s = 0\nend_s = 60\n",
        )
        .unwrap();
        load_manifest_file(&f).unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn one_take_lists_each_position_by_time() {
        let m: Manifest = toml::from_str(
            r#"
            [[recording]]
            file = "take"
            position = "DU"
            end_s = 600

            [[recording]]
            file = "take"
            position = "CD"
            start_s = 600
            end_s = 1200

            [[recording]]
            file = "take"
            position = "CL"
            start_s = 1200
            "#,
        )
        .unwrap();
        let spans: Vec<_> = m
            .recordings
            .iter()
            .map(|e| take::span_of(e, 1500.0).unwrap())
            .collect();
        assert_eq!(spans, [(0.0, 600.0), (600.0, 1200.0), (1200.0, 1500.0)]);
    }

    #[test]
    fn a_session_file_with_a_tolerance_and_limits_still_reads() {
        // Written by `session --init` before schema /2; both are ignored.
        let m: Manifest = toml::from_str(
            "tolerance = \"mens\"\n[limits]\ndelta_rate = 12.0\noverbanking = 340.0\n",
        )
        .unwrap();
        assert!(m.recordings.is_empty());
        assert!(toml::from_str::<Manifest>("no_such_key = 1\n").is_err());
    }

    /// A session file with a masked bump: `session` leaves it out of the
    /// rate, says how much was masked, and counts only the seconds read.
    #[test]
    fn a_masked_bump_in_a_session_file_is_left_out() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let dir = std::env::temp_dir().join(format!("tg-session-masked-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let bump = (50.0, 56.0);
        let cfg = SynthConfig {
            duration_s: 120.0,
            rate_s_per_day: 0.0,
            ..Default::default()
        };
        let audio = generate(
            &cfg,
            |_| 280.0,
            |t| {
                if t >= bump.0 && t < bump.1 {
                    4000.0
                } else {
                    3.0
                }
            },
        );
        audio::write_wav(&dir.join("take.wav"), &audio).unwrap();
        let read = |masked: &str| -> serde_json::Value {
            fs::write(
                dir.join("session.toml"),
                format!("bph = 28800\nlift = 52\n\n[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\n{masked}"),
            )
            .unwrap();
            let out = dir.join("out");
            let o = Options {
                settle: Some(10.0),
                out: Some(out.clone()),
                ..options(None, None, None)
            };
            run(std::slice::from_ref(&dir), &o).unwrap();
            let text = fs::read_to_string(out.join("summary.json")).unwrap();
            let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
            v["csv"] = fs::read_to_string(out.join("readings.csv")).unwrap().into();
            v
        };
        let bumped = read("");
        let m = &bumped["readings"][0]["measurement"];
        let rate = m["rate_s_per_day"].as_f64().unwrap();
        assert!((rate - 3.0).abs() > 5.0, "the bump didn't show: {rate}");
        assert!(m.get("masked_s").is_none());

        let v = read("masked = [[49.0, 57.0]]\n");
        let m = &v["readings"][0]["measurement"];
        let rate = m["rate_s_per_day"].as_f64().unwrap();
        assert!((rate - 3.0).abs() < 1.0, "rate {rate}");
        assert_eq!(m["masked_s"].as_f64(), Some(8.0));
        let caveats = m["caveats"].to_string();
        assert!(caveats.contains("masked"), "{caveats}");
        assert!(!caveats.contains("beats_missing"), "{caveats}");
        // 110 s after settling, less the 8 s masked.
        let row = v["csv"]
            .as_str()
            .unwrap()
            .lines()
            .nth(1)
            .unwrap()
            .to_string();
        assert_eq!(row.split(',').nth(3), Some("102.0"), "{row}");
        assert!(v["readings"][0]["shape"].is_object(), "no shape");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Without a clock log, a recording's USB lines set the rate, as in
    /// `series` and `long`; read on the sound card's clock, as `regress`
    /// sets readings beside tg's, they don't.
    #[test]
    fn usb_lines_set_the_rate_without_a_clock_log() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let dir = std::env::temp_dir().join(format!("tg-session-usb-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = SynthConfig {
            duration_s: 120.0,
            rate_s_per_day: 0.0,
            ..Default::default()
        };
        let mut audio = generate(&cfg, |_| 280.0, |_| 3.0);
        // The lines of a card 20 ppm slow: a true 5, 6 and 7 kHz lands
        // 20 ppm high on its clock.
        let fs_hz = audio.sample_rate as f64;
        for (f, a) in [(5000.0, 0.002), (6000.0, 0.003), (7000.0, 0.002)] {
            let f = f * (1.0 + 20e-6);
            for (i, v) in audio.samples.iter_mut().enumerate() {
                *v += (a * (2.0 * std::f64::consts::PI * f * i as f64 / fs_hz).sin()) as f32;
            }
        }
        audio::write_wav(&dir.join("take.wav"), &audio).unwrap();
        fs::write(
            dir.join(MANIFEST),
            "bph = 28800\nlift = 52\n\n[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\n",
        )
        .unwrap();
        let (m, base, _) = load_manifest_file(&dir.join(MANIFEST)).unwrap();
        let o = options(None, None, None);
        let selection = o.calibre(&m).unwrap();
        let read = |card_clock| {
            let c = Ctx {
                m: &m,
                calibre: &selection,
                base: &base,
                o: &o,
                tty: false,
                quiet: true,
                card_clock,
            };
            read_one(&m.recordings[0], &c).unwrap().measurement
        };
        let card = read(true);
        assert!(!card.calibrated);
        let on_card = card.rate_s_per_day.unwrap();
        assert!(
            (on_card - 3.0).abs() < 0.3,
            "rate on the card's clock {on_card}"
        );
        let usb = read(false);
        assert!(usb.calibrated, "the USB lines were not used");
        let corrected = usb.rate_s_per_day.unwrap();
        // 20 ppm is 1.728 s/d.
        assert!(
            (on_card - corrected - 1.728).abs() < 0.1,
            "{on_card} on the card's clock, {corrected} on the USB lines'"
        );
        // The JSON reading says what the clock corrects and where it came
        // from, as the app's clock line does (#714).
        let json = serde_json::to_value(&usb).unwrap();
        let ppm = json["clock_ppm"].as_f64().unwrap();
        // The card runs 20 ppm slow: negative, as every clock ppm is
        // positive when the card runs fast (#657).
        assert!((ppm + 20.0).abs() < 0.5, "{json}");
        let source = json["clock_source"].as_str().unwrap();
        assert!(source.contains("USB lines"), "{source}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Keeps the take at `wav` (session file `toml`, read over `span_s`) as
    /// its analysis alone in `kept`, with the session file saying so and the
    /// other files given.
    fn keep_as_analysis(
        wav: &Path,
        toml: &str,
        span_s: f64,
        kept: &Path,
        others: Vec<(String, Vec<u8>)>,
    ) {
        let m: Manifest = toml::from_str(toml).unwrap();
        let mut cfg = StreamConfig::default();
        cfg.analysis.bph = Some(28800);
        cfg.analysis.amplitude.lift_deg = 52.0;
        let shapes = analysis_take::shape_stretches(&m, span_s);
        let stored = analysis_take::measure(
            &[wav],
            &cfg,
            &shapes,
            serde_json::json!({"name": "timegrapher", "version": "test", "git_hash": "abc"}),
            |_| {},
        )
        .unwrap();
        let mut files = vec![(
            MANIFEST.to_string(),
            format!("audio = false\n{toml}").into_bytes(),
        )];
        files.extend(others);
        analysis_take::save(kept, &files, &stored).unwrap();
    }

    /// `session` on `dir` with these options, writing to `dir/out`: its
    /// summary.json.
    fn summary_of(dir: &Path, o: Options) -> serde_json::Value {
        let out = dir.join("out");
        let o = Options {
            out: Some(out.clone()),
            ..o
        };
        run(&[dir.to_path_buf()], &o).unwrap();
        let v =
            serde_json::from_str(&fs::read_to_string(out.join("summary.json")).unwrap()).unwrap();
        let _ = fs::remove_dir_all(&out);
        v
    }

    /// A clock log for audio heard to `pause`, then after a pause of `gap`,
    /// with the system clock set 2 s forward between 140 s and 150 s.
    fn clock_log_with_a_step(pause: f64, gap: f64) -> String {
        let mut clock = String::from("audio_s,unix_s\n");
        let mut audio_s: Vec<f64> = (0..=9).map(|i| i as f64 * 10.0).collect();
        audio_s.push(pause);
        audio_s.extend((10..=18).map(|i| i as f64 * 10.0));
        for a in audio_s {
            let off = if a >= pause { gap } else { 0.0 } + if a >= 150.0 { 2.0 } else { 0.0 };
            clock.push_str(&format!("{a},{}\n", 1.79e9 + a + off));
        }
        clock
    }

    /// A take kept as its analysis alone has the breaks the take with its
    /// sound has: its listed pause, and a step in its clock log that no
    /// listed pause explains.
    #[test]
    fn a_kept_take_has_the_breaks_its_sound_has() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let base =
            std::env::temp_dir().join(format!("tg-session-kept-breaks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let full = base.join("full");
        fs::create_dir_all(&full).unwrap();
        // 300 s of a watch, heard for the first 90.03 s and from 210.03 s:
        // a pause of 120 s, listed in session.json.
        let (pause, gap) = (90.03, 120.0);
        let watch = generate(
            &SynthConfig {
                duration_s: 300.0,
                rate_s_per_day: 6.0,
                beat_error_ms: 0.6,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let sr = watch.sample_rate;
        let cut = |t: f64| (t * sr as f64) as usize;
        let mut samples = watch.samples[..cut(pause)].to_vec();
        samples.extend_from_slice(&watch.samples[cut(pause + gap)..]);
        let wav = full.join("take_part01-001.wav");
        audio::write_wav(
            &wav,
            &audio::Audio {
                samples,
                sample_rate: sr,
            },
        )
        .unwrap();
        let json = format!(r#"{{"breaks": [{{"audio_s": {pause}, "gap_s": {gap}}}]}}"#);
        fs::write(full.join("session.json"), &json).unwrap();
        // The clock log: the pause's gap from 90.03 s on, and the system
        // clock set 2 s forward between 140 s and 150 s with no pause.
        let clock = clock_log_with_a_step(pause, gap);
        fs::write(full.join("clock.csv"), &clock).unwrap();
        let toml = "bph = 28800\nlift = 52\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n\
                    clock = \"clock.csv\"\n";
        fs::write(full.join(MANIFEST), toml).unwrap();
        let kept = base.join("kept");
        let others = vec![
            ("session.json".to_string(), json.into_bytes()),
            ("clock.csv".to_string(), clock.into_bytes()),
        ];
        keep_as_analysis(&wav, toml, 180.0, &kept, others);
        let read = |dir: &Path| {
            let o = Options {
                settle: Some(10.0),
                ..options(None, None, None)
            };
            summary_of(dir, o)
        };
        let caveat = |m: &serde_json::Value, code: &str| -> Option<u64> {
            m["caveats"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["code"] == code)
                .and_then(|c| c["count"].as_u64())
        };
        let (a, b) = (read(&full), read(&kept));
        let (ma, mb) = (
            &a["readings"][0]["measurement"],
            &b["readings"][0]["measurement"],
        );
        assert_eq!(
            ma["breaks"].as_array().map(Vec::len),
            Some(2),
            "{}",
            ma["breaks"]
        );
        assert_eq!(ma["breaks"], mb["breaks"]);
        assert_eq!(caveat(ma, "breaks"), Some(2), "{}", ma["caveats"]);
        assert_eq!(caveat(mb, "breaks"), Some(2), "{}", mb["caveats"]);
        assert_eq!(caveat(ma, "sides_by_count"), caveat(mb, "sides_by_count"));
        let rate = |m: &serde_json::Value| m["rate_s_per_day"].as_f64().unwrap();
        assert!(
            (rate(ma) - rate(mb)).abs() < 0.001,
            "{} and {}",
            rate(ma),
            rate(mb)
        );
        let _ = fs::remove_dir_all(&base);
    }

    /// A Power Reserve Test's folder gives its run in `power_reserve`; a
    /// folder without one has no such key, and a broken file is an error.
    #[test]
    fn a_power_reserve_take_gives_its_run() {
        use timegrapher_core::power_reserve::{self, Ending, Heard, Results};
        let base = std::env::temp_dir().join(format!("tg-session-reserve-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        assert_eq!(power_reserve_of(&base), Ok(None));
        let heard = Heard {
            first_s: Some(2.0),
            last_s: Some(2.0 + 151_200.0),
            pauses: Vec::new(),
        };
        let r = Results::of(
            &heard,
            Ending::Stopped,
            "2026-10-22T16:15:00Z",
            "CL",
            true,
            |s| s,
        )
        .unwrap();
        power_reserve::write(&base, &r).unwrap();
        assert_eq!(power_reserve_of(&base), Ok(Some(r.clone())));
        let s = Session {
            schema: String::new(),
            software: serde_json::Value::Null,
            input: serde_json::Value::Null,
            watch: None,
            calibre: None,
            owner: None,
            notes: None,
            bph: 28800,
            lift_deg: 52.0,
            lift_known: true,
            readings: Vec::new(),
            left_out: Vec::new(),
            report: session::evaluate(&[]),
            power_reserve: Some(r),
            poise: Vec::new(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["power_reserve"]["run_s"], 151_200.0);
        assert_eq!(v["power_reserve"]["ending"], "stopped");
        let none = Session {
            power_reserve: None,
            ..s
        };
        assert!(serde_json::to_value(&none)
            .unwrap()
            .get("power_reserve")
            .is_none());
        fs::write(base.join(power_reserve::FILE), "{").unwrap();
        assert!(power_reserve_of(&base).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    /// A position the app saved as settling right to its end (a Dial Up
    /// painted over 10 s in, inside the take's own settling) has nothing to
    /// read: `session` leaves it out and reads the rest, and a take with
    /// nothing else is refused, saying so.
    #[test]
    fn a_position_that_settles_to_its_end_is_left_out() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let base = std::env::temp_dir().join(format!("tg-session-settled-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 80.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        audio::write_wav(&base.join("take.wav"), &audio).unwrap();
        let entry = |pos: &str, times: &str| {
            format!("[[recording]]\nfile = \"take.wav\"\nposition = \"{pos}\"\n{times}settle_s = 0.0\n\n")
        };
        let saved = format!(
            "bph = 28800\nlift = 52\n\n{}{}{}",
            entry("DU", "start_s = 10.0\nend_s = 10.0\n"),
            entry("CU", "start_s = 10.0\nend_s = 40.0\n"),
            entry("DU", "start_s = 40.0\n"),
        );
        fs::write(base.join(MANIFEST), &saved).unwrap();
        let out = base.join("out");
        let o = Options {
            out: Some(out.clone()),
            ..options(None, None, None)
        };
        run(std::slice::from_ref(&base), &o).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(out.join("summary.json")).unwrap()).unwrap();
        let read: Vec<&str> = v["readings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["position"].as_str().unwrap())
            .collect();
        assert_eq!(read, ["3H", "CH"], "crown up, then dial up");
        let nothing = format!(
            "bph = 28800\n\n{}",
            entry("DU", "start_s = 10.0\nend_s = 10.0\n")
        );
        fs::write(base.join(MANIFEST), nothing).unwrap();
        let why = run(std::slice::from_ref(&base), &o).unwrap_err();
        assert!(why.contains("with anything to read"), "{why}");
        let _ = fs::remove_dir_all(&base);
    }

    /// A two-position take in `base/full`, with its sound, and the same take
    /// kept as its analysis alone in `base/kept`: the two folders and the
    /// sound file.
    fn a_take_and_its_kept_analysis(base: &Path) -> (PathBuf, PathBuf, PathBuf) {
        use timegrapher_core::synth::{generate, SynthConfig};
        let _ = fs::remove_dir_all(base);
        let full = base.join("full");
        fs::create_dir_all(&full).unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 120.0,
                rate_s_per_day: 6.0,
                beat_error_ms: 0.6,
                odd_gains: Some([0.9, 0.25, 1.0]),
                ..Default::default()
            },
            |t| if t < 60.0 { 280.0 } else { 260.0 },
            |_| 0.0,
        );
        let wav = full.join("take_part01-001.wav");
        audio::write_wav(&wav, &audio).unwrap();
        let toml = "bph = 28800\nlift = 52\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n\
                    end_s = 60.0\nmasked = [[40.0, 43.0]]\n\n[[recording]]\nfile = \".\"\n\
                    position = \"DD\"\nstart_s = 60.0\n";
        fs::write(full.join(MANIFEST), toml).unwrap();
        let kept = base.join("kept");
        keep_as_analysis(&wav, toml, 120.0, &kept, Vec::new());
        (full, kept, wav)
    }

    /// A take kept as its analysis alone reads as the take with its sound
    /// did: the same readings and beat shape for each position, with the
    /// report saying no sound was kept.
    #[test]
    fn a_take_kept_as_its_analysis_reads_as_its_sound() {
        let base = std::env::temp_dir().join(format!("tg-session-kept-{}", std::process::id()));
        let (full, kept, _) = a_take_and_its_kept_analysis(&base);
        let read = |dir: &Path| summary_of(dir, options(None, None, None));
        let (a, b) = (read(&full), read(&kept));
        for i in 0..2 {
            let (ra, rb) = (&a["readings"][i], &b["readings"][i]);
            let f = |r: &serde_json::Value, k: &str| r["measurement"][k].as_f64();
            let (x, y) = (
                f(ra, "rate_s_per_day").unwrap(),
                f(rb, "rate_s_per_day").unwrap(),
            );
            assert!((x - y).abs() < 0.05, "reading {i}: rate {x} and {y}");
            for k in ["amplitude_deg", "beat_error_ms"] {
                let (x, y) = (f(ra, k).unwrap(), f(rb, k).unwrap());
                assert!((x - y).abs() < 0.01, "reading {i}: {k} {x} and {y}");
            }
            assert_eq!(ra["measurement"]["masked_s"], rb["measurement"]["masked_s"]);
            assert!(rb["shape"].is_object(), "reading {i}: no shape");
            assert_eq!(ra["shape"], rb["shape"], "reading {i}");
        }
        assert!(a["input"]["settings"]["analysis_only"].is_null());
        let note = &b["input"]["settings"]["analysis_only"];
        assert_eq!(note["note"], analysis_take::NO_SOUND);
        assert_eq!(note["measured_by"]["version"], "test");
        let s: Session = Session {
            schema: String::new(),
            software: serde_json::Value::Null,
            input: b["input"].clone(),
            watch: None,
            calibre: None,
            owner: None,
            notes: None,
            bph: 28800,
            lift_deg: 52.0,
            lift_known: true,
            readings: Vec::new(),
            left_out: Vec::new(),
            report: session::evaluate(&[]),
            power_reserve: None,
            poise: Vec::new(),
        };
        let html = crate::session_report::html(&s);
        assert!(html.contains("None kept") && html.contains("timegrapher test (abc)"));
        let _ = fs::remove_dir_all(&base);
    }

    /// Read at another lift, the kept amplitudes are those the sound gives
    /// at it.
    #[test]
    fn a_take_kept_as_its_analysis_reads_at_another_lift_as_its_sound() {
        let base =
            std::env::temp_dir().join(format!("tg-session-kept-lift-{}", std::process::id()));
        let (full, kept, _) = a_take_and_its_kept_analysis(&base);
        let read = |dir: &Path| summary_of(dir, options(None, Some(44.0), None));
        let (a, b) = (read(&full), read(&kept));
        for i in 0..2 {
            let f = |v: &serde_json::Value| {
                v["readings"][i]["measurement"]["amplitude_deg"]
                    .as_f64()
                    .unwrap()
            };
            assert!(
                (f(&a) - f(&b)).abs() < 0.01,
                "reading {i} at 44°: {} and {}",
                f(&a),
                f(&b)
            );
        }
        let _ = fs::remove_dir_all(&base);
    }

    /// A take is kept with its sound or as its analysis, never both, and a
    /// session file saying no sound needs the analysis to read.
    #[test]
    fn a_kept_take_has_its_sound_or_its_analysis_not_both_nor_neither() {
        let base =
            std::env::temp_dir().join(format!("tg-session-kept-both-{}", std::process::id()));
        let (_, kept, wav) = a_take_and_its_kept_analysis(&base);
        fs::copy(&wav, kept.join("take_part01-001.wav")).unwrap();
        let o = options(None, None, None);
        let err = run(std::slice::from_ref(&kept), &o).unwrap_err();
        assert!(err.contains("both sound and"), "{err}");
        fs::remove_file(kept.join("take_part01-001.wav")).unwrap();
        fs::remove_dir_all(kept.join(analysis_take::DIR)).unwrap();
        let err = run(std::slice::from_ref(&kept), &o).unwrap_err();
        assert!(err.contains("audio = false, but there is no"), "{err}");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn whole_recording_without_a_span() {
        let e = RecordingEntry::default();
        assert_eq!(take::span_of(&e, 300.0).unwrap(), (0.0, 300.0));
    }

    #[test]
    fn empty_span_is_an_error() {
        let e = RecordingEntry {
            start_s: Some(400.0),
            ..Default::default()
        };
        assert!(take::span_of(&e, 300.0).is_err());
        let e = RecordingEntry {
            start_s: Some(100.0),
            end_s: Some(100.0),
            ..Default::default()
        };
        assert!(take::span_of(&e, 300.0).is_err());
    }
}
