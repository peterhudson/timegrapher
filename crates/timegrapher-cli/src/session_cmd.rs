//! `timegrapher session`: a watch measured in several positions, read
//! together into one report (Witschi's multi-position test).

use crate::long::expand_dirs;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use timegrapher_core::calibres::{self, Calibre};
use timegrapher_core::clock::{self, ClockFit};
use timegrapher_core::dsp::{envelope, EnvelopeConfig};
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::{standard_wheels, Wheel};
use timegrapher_core::session::{
    self, Finding, Manifest, Mark, Position, Reading, RecordingEntry, SessionReport, Severity,
    Tolerance,
};
use timegrapher_core::shape::{self, ShapeConfig};
use timegrapher_core::stream::{self, BeatLog, StreamConfig};
use timegrapher_core::{audio, beats, timing, twostate};

pub const MANIFEST: &str = "session.toml";
/// The layout of `summary.json` and `--json`.
pub const SCHEMA: &str = "timegrapher.session/1";

pub struct Options {
    pub bph: Option<u32>,
    pub lift: Option<f64>,
    /// A calibre in the table (`calibres`): its beat rate, lift angle and
    /// wheels, where the flags and the session file give none.
    pub calibre: Option<String>,
    pub tolerance: Option<String>,
    pub settle: Option<f64>,
    pub out: Option<PathBuf>,
    pub json: bool,
    pub init: bool,
    pub no_shape: bool,
    pub no_cycles: bool,
}

impl Options {
    /// The calibre named with `--calibre` (checked before any audio is
    /// read), else the session file's `calibre` line when the table knows
    /// it; a name the table does not know is only a label.
    fn calibre(&self, m: &Manifest) -> Result<Option<&'static Calibre>, String> {
        if self.calibre.is_some() {
            return crate::long::Train {
                calibre: self.calibre.as_deref(),
                escape_teeth: None,
            }
            .calibre();
        }
        Ok(m.calibre.as_deref().and_then(calibres::find))
    }

    /// Where the calibre and the values that win over it disagree, with
    /// the value used.
    fn calibre_notes(&self, m: &Manifest, cal: Option<&Calibre>) -> Vec<String> {
        let Some(c) = cal else {
            return Vec::new();
        };
        let mut notes = Vec::new();
        if self.calibre.is_some() {
            if let Some(other) = m.calibre.as_deref().and_then(calibres::find) {
                if other.calibre != c.calibre {
                    notes.push(format!(
                        "the session file names the {}, --calibre the {}; using the {}",
                        other.calibre, c.calibre, c.calibre
                    ));
                }
            }
        }
        let from = |flag: bool| if flag { "the flag" } else { "the session file" };
        if let Some(b) = self.bph.or(m.bph).filter(|&b| b != c.bph) {
            notes.push(format!(
                "the {} beats at {} bph, {} says {b}; using {b}",
                c.calibre,
                c.bph,
                from(self.bph.is_some())
            ));
        }
        if let (Some(l), Some(cl)) = (self.lift.or(m.lift), c.lift_angle_deg) {
            if (l - cl).abs() > 1e-9 {
                notes.push(format!(
                    "the {}'s lift angle is {cl}°, {} says {l}°; using {l}°",
                    c.calibre,
                    from(self.lift.is_some())
                ));
            }
        }
        if let (Some(t), Some(ct)) = (m.escape_teeth, c.escape_teeth) {
            if t != ct {
                notes.push(format!(
                    "the {}'s escape wheel has {ct} teeth, the session file says {t}; using {t}",
                    c.calibre
                ));
            }
        }
        notes
    }

    /// Beat rate: the flag, else the session file, else the calibre's;
    /// none means it is measured.
    fn bph(&self, m: &Manifest, cal: Option<&Calibre>) -> Option<u32> {
        self.bph.or(m.bph).or(cal.map(|c| c.bph))
    }

    /// Lift angle: the flag, else the session file, else the calibre's,
    /// else 52 degrees.
    fn lift(&self, m: &Manifest, cal: Option<&Calibre>) -> f64 {
        self.lift
            .or(m.lift)
            .or(cal.and_then(|c| c.lift_angle_deg))
            .unwrap_or(52.0)
    }
}

/// Seconds of audio the beat shape is measured on.
const SHAPE_S: f64 = 60.0;

fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav") || e.eq_ignore_ascii_case("flac"))
}

fn has_audio(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|d| d.filter_map(|e| e.ok()).any(|e| is_audio(&e.path())))
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
            .filter(|q| is_audio(q) || (q.is_dir() && has_audio(q)))
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
        let text = fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let m: Manifest = toml::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
        if m.recordings.is_empty() {
            return Err(format!("{}: no [[recording]] entries", f.display()));
        }
        let base = f
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
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

pub(crate) fn position_of(e: &RecordingEntry) -> Result<Position, String> {
    match &e.position {
        Some(p) => Position::parse(p).ok_or_else(|| {
            format!(
                "{}: unknown position '{p}' (use CH, CB, 3H, 6H, 9H, 12H or DU, DD, CU, CL, CD, CR)",
                e.file
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
    let cal = o.calibre(&Manifest::default())?;
    t.push_str(&format!(
        "bph = {}\n",
        o.bph.or(cal.map(|c| c.bph)).unwrap_or(28800)
    ));
    t.push_str(&format!(
        "lift = {}\n",
        o.lift
            .or(cal.and_then(|c| c.lift_angle_deg))
            .unwrap_or(52.0)
    ));
    t.push_str("# ladies, mens, cosc-small, cosc or metas\n");
    t.push_str(&format!(
        "tolerance = \"{}\"\n",
        o.tolerance.as_deref().unwrap_or("mens")
    ));
    t.push_str("# seconds skipped at the start of each recording while the watch settles\n");
    t.push_str(&format!("settle_s = {}\n", o.settle.unwrap_or(20.0)));
    t.push_str("# known sound-card error, ppm slow, for recordings without a clock log\n");
    t.push_str("# card_ppm = 0.0\n");
    for f in &files {
        let name = rel(f, &dir);
        t.push_str("\n[[recording]]\n");
        t.push_str(&format!("file = \"{name}\"\n"));
        match Position::from_file_name(&name) {
            Some(p) => t.push_str(&format!(
                "position = \"{}\"  # {}, guessed from the name\n",
                p.code(),
                p.description()
            )),
            None => t.push_str(
                "position = \"\"  # CH, CB, 3H, 6H, 9H, 12H (or DU, DD, CU, CL, CD, CR)\n",
            ),
        }
        t.push_str("wind_h = 0  # hours since full wind\n");
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
            "# reference = { rate = 0.0, amplitude = 0, beat_error = 0.0, source = \"\" }\n",
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

fn read_clock(path: &Path, first: &Path) -> Result<ClockFit, String> {
    let info = audio::info(first).map_err(|e| format!("{}: {e}", first.display()))?;
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let pairs = clock::parse_log(&text, info.sample_rate, info.bytes_per_frame)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    ClockFit::new(&pairs).map_err(|e| format!("{}: {e}", path.display()))
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

/// The log from `from_s` on, with times shifted to start at zero.
fn trimmed(log: &BeatLog, from_s: f64, to_s: f64) -> BeatLog {
    let mut l = log.clone();
    l.beats.retain(|b| b.time >= from_s && b.time < to_s);
    for b in &mut l.beats {
        b.time -= from_s;
    }
    l.amplitude_windows
        .retain(|w| w.start_s >= from_s && w.end_s <= to_s);
    for w in &mut l.amplitude_windows {
        w.start_s -= from_s;
        w.end_s -= from_s;
    }
    l.duration_s = to_s.min(log.duration_s) - from_s;
    l
}

pub(crate) struct Ctx<'a> {
    pub m: &'a Manifest,
    pub base: &'a Path,
    pub o: &'a Options,
    pub tty: bool,
    /// No progress lines (the file names stay out of logs).
    pub quiet: bool,
}

/// The stretch of a recording an entry reads, seconds from its start:
/// `start_s` to `end_s`, or the whole recording when they are left out.
fn span_of(e: &RecordingEntry, duration_s: f64) -> Result<(f64, f64), String> {
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

pub(crate) fn read_one(e: &RecordingEntry, c: &Ctx) -> Result<Reading, String> {
    let position = position_of(e)?;
    let path = c.base.join(&e.file);
    let files = expand_dirs(std::slice::from_ref(&path))?;
    let mut cfg = StreamConfig::default();
    let cal = c.o.calibre(c.m)?;
    cfg.analysis.bph = c.o.bph(c.m, cal);
    cfg.analysis.amplitude.lift_deg = c.o.lift(c.m, cal);
    let escape_teeth =
        c.m.escape_teeth
            .or(cal.and_then(|k| k.escape_teeth))
            .unwrap_or(15);
    cfg.analysis.escape_teeth = escape_teeth;
    if c.quiet {
    } else if c.tty {
        eprint!("\r{:<70}", format!("analysing {} ({})", e.file, position));
    } else {
        eprintln!("analysing {} ({})", e.file, position);
    }
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let log =
        stream::analyze_files(&paths, &cfg, |_| {}).map_err(|err| format!("{}: {err}", e.file))?;
    let clock = match (&e.clock, c.m.card_ppm) {
        (Some(p), _) => Some(read_clock(&c.base.join(p), &files[0])?),
        (None, Some(ppm)) => session::fixed_clock(ppm, log.duration_s),
        (None, None) => None,
    };
    let (start, stop) = span_of(e, log.duration_s)?;
    let settle = start
        + e.settle_s
            .or(c.o.settle)
            .or(c.m.settle_s)
            .unwrap_or(20.0)
            .min((stop - start) / 2.0);
    let end = e
        .measure_s
        .or(c.m.measure_s)
        .map_or(stop, |m| settle + m)
        .min(stop);
    let measurement = session::measure(&log, clock.as_ref(), settle, end);

    // Cycles and shape of noise would only mislead.
    let heard = measurement.watch_heard;
    let cycles = if !heard || c.o.no_cycles || c.m.cycles == Some(false) {
        Vec::new()
    } else {
        let lc = LongConfig {
            wheels: match cal {
                Some(k) => crate::long::Train {
                    calibre: Some(k.calibre.as_str()),
                    escape_teeth: Some(escape_teeth),
                }
                .wheels(log.bph)?,
                None => standard_wheels(log.bph, escape_teeth),
            }
            .into_iter()
            .chain(c.m.wheels.iter().map(|(name, &period_s)| Wheel {
                name: name.clone(),
                period_s,
            }))
            .collect(),
            ..Default::default()
        };
        let part = trimmed(&log, settle, end);
        session::cycles(&longrun::analyse(&part, clock.as_ref(), &lc), &lc.wheels)
    };

    let shape = if !heard || c.o.no_shape || c.m.shape == Some(false) {
        None
    } else {
        let len = SHAPE_S.min(end - settle);
        let (x, fs) = excerpt(&files, settle, len)?;
        let env = envelope(&x, fs, &EnvelopeConfig::default());
        let (found, _) = beats::detect(&env, fs, log.bph);
        let beat_s = timing::fit(&found, log.bph).map_or(3600.0 / log.bph as f64, |f| f.period_s);
        let r = shape::analyze(&env, fs, &found, beat_s, &ShapeConfig::default());
        Some(session::shape_summary(&r, x.len() as f64 / fs))
    };

    let label = if e.start_s.is_some() || e.end_s.is_some() {
        format!("{} ({:.0}–{:.0} s)", e.file, start, stop)
    } else {
        e.file.clone()
    };
    Ok(Reading {
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
    })
}

/// Everything the report and `summary.json` show.
#[derive(serde::Serialize)]
pub struct Session<'a> {
    /// Name and version of this layout; bumped when a field changes meaning.
    pub schema: &'static str,
    /// Name, version and platform of the software, as in every `--json`
    /// document (see docs/agent-interface.md).
    pub software: serde_json::Value,
    /// The recordings read and the settings used.
    pub input: serde_json::Value,
    pub watch: Option<&'a str>,
    pub calibre: Option<&'a str>,
    pub owner: Option<&'a str>,
    pub notes: Option<&'a str>,
    pub bph: u32,
    pub lift_deg: f64,
    pub readings: Vec<Reading>,
    pub report: SessionReport,
}

pub fn run(paths: &[PathBuf], o: &Options) -> Result<(), String> {
    if o.init {
        return init(paths, o);
    }
    let (m, base) = load_manifest(paths)?;
    let cal = o.calibre(&m)?;
    let calibre_notes = o.calibre_notes(&m, cal);
    for n in &calibre_notes {
        eprintln!("note: {n}");
    }
    for e in &m.recordings {
        position_of(e)?;
    }
    let tol_name = o
        .tolerance
        .as_deref()
        .or(m.tolerance.as_deref())
        .unwrap_or("mens");
    let tol = Tolerance::named(tol_name).ok_or_else(|| {
        format!("unknown tolerance '{tol_name}' (ladies, mens, cosc-small, cosc, metas)")
    })?;
    let limits = m.limits.clone().unwrap_or_default();
    let ctx = Ctx {
        m: &m,
        base: &base,
        o,
        tty: std::io::stderr().is_terminal(),
        quiet: false,
    };
    let readings: Vec<Reading> = m
        .recordings
        .iter()
        .map(|e| read_one(e, &ctx))
        .collect::<Result<_, _>>()?;
    if ctx.tty {
        eprintln!();
    }
    let report = session::evaluate(&readings, &tol, &limits);
    let bph = readings.first().map_or(0, |r| r.bph);
    let files: Vec<PathBuf> = m.recordings.iter().map(|e| base.join(&e.file)).collect();
    let settings = serde_json::json!({
        "bph": o.bph(&m, cal),
        "lift_deg": o.lift(&m, cal),
        "calibre": cal.map(|c| c.calibre.as_str()),
        "calibre_notes": calibre_notes,
        "tolerance": tol_name,
        "settle_s": o.settle,
        "shape": !o.no_shape,
        "cycles": !o.no_cycles,
    });
    let s = Session {
        schema: SCHEMA,
        software: crate::output::software(),
        input: crate::output::input(&files, settings),
        watch: m.watch.as_deref().filter(|w| !w.is_empty()),
        calibre: m
            .calibre
            .as_deref()
            .filter(|w| !w.is_empty())
            .or(cal.map(|c| c.calibre.as_str())),
        owner: m.owner.as_deref(),
        notes: m.notes.as_deref(),
        bph,
        lift_deg: o.lift(&m, cal),
        readings,
        report,
    };

    let out = o.out.clone().unwrap_or_else(|| base.join("session_report"));
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let json = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
    let w = |name: &str, text: &str| {
        let p = out.join(name);
        fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
    };
    w("summary.json", &json)?;
    w("readings.csv", &csv(&s))?;
    w("report.html", &crate::session_report::html(&s))?;
    if o.json {
        println!("{json}");
    } else {
        print_summary(&s);
        println!("Report       {}", out.join("report.html").display());
    }
    Ok(())
}

pub fn opt(v: Option<f64>, digits: usize) -> String {
    v.filter(|x| x.is_finite())
        .map_or("-".into(), |x| format!("{x:.digits$}"))
}

pub fn signed(v: Option<f64>, digits: usize) -> String {
    v.filter(|x| x.is_finite())
        .map_or("-".into(), |x| format!("{x:+.digits$}"))
}

pub fn wind(w: Option<f64>) -> String {
    match w {
        None => "full?".into(),
        Some(w) if w <= 0.0 => "full".into(),
        Some(w) => format!("{w:.0} h"),
    }
}

pub fn severity(s: Severity) -> &'static str {
    match s {
        Severity::Fault => "FAULT",
        Severity::Warning => "CHECK",
        Severity::Note => "NOTE",
    }
}

pub fn mark(m: Mark) -> &'static str {
    match m {
        Mark::Within => "",
        Mark::Outside => "*",
        Mark::NotJudged => "",
        Mark::Unreliable => "?",
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
            m.end_s - m.start_s,
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
    println!("  {:<6} {}", severity(f.severity), f.title);
    println!("         {}", f.evidence);
    println!("         {}", f.advice);
}

fn print_summary(s: &Session) {
    let r = &s.report;
    let name = match (s.watch, s.calibre) {
        (Some(w), Some(c)) => format!("{w}, calibre {c}"),
        (Some(w), None) => w.to_string(),
        (None, Some(c)) => format!("calibre {c}"),
        (None, None) => String::new(),
    };
    println!(
        "Watch        {}{} bph, lift angle {} deg",
        if name.is_empty() {
            String::new()
        } else {
            format!("{name}; ")
        },
        s.bph,
        s.lift_deg
    );
    let t = &r.tolerance;
    println!(
        "Tolerance    {}: {:+.0} to {:+.0} s/d, amplitude {:.0}-{:.0} deg horizontal and {:.0}-{:.0} vertical, beat error under {} ms",
        t.name, t.rate_min, t.rate_max, t.amplitude_h.0, t.amplitude_h.1, t.amplitude_v.0, t.amplitude_v.1, t.beat_error_ms
    );
    println!();
    println!(
        "{:<5} {:<6} {:>8} {:>6} {:>6} {:>5} {:>7} {:>15} {:>17}  Recording",
        "Pos", "Wind", "Rate", "Amp", "BE", "drop", "Jitter", "10 s rates", "Reference"
    );
    println!(
        "{:<5} {:<6} {:>8} {:>6} {:>6} {:>5} {:>7} {:>15} {:>17}",
        "", "", "s/d", "deg", "ms", "ms", "us", "s/d", "s/d, deg, ms"
    );
    for (rd, v) in s.readings.iter().zip(&r.verdicts) {
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
        println!(
            "{:<5} {:<6} {:>7}{:1} {:>5}{:1} {:>5}{:1} {:>5} {:>7} {:>15} {:>17}  {}{}",
            rd.position.code(),
            wind(rd.wind_h),
            signed(m.rate_s_per_day, 1),
            mark(v.rate),
            opt(m.amplitude_deg, 0),
            mark(v.amplitude),
            opt(m.beat_error(), 2),
            mark(v.beat_error),
            opt(m.beat_error_ms.map(f64::abs), 2),
            opt(m.jitter_us, 0),
            format!("{}..{}", signed(m.rate_p05, 0), signed(m.rate_p95, 0)),
            rf,
            rd.label,
            if m.calibrated { "" } else { " (card clock)" }
        );
    }
    println!("* outside tolerance, ? not measured reliably");
    println!();
    for rd in s.readings.iter().filter(|r| r.measurement.watch_heard) {
        let m = &rd.measurement;
        println!(
            "States       {:<4} {}; {}",
            rd.position.code(),
            twostate::describe(&m.amplitude_states, "amplitude"),
            twostate::describe(&m.rate_states, "rate")
        );
    }
    println!();
    for st in &r.states {
        println!(
            "At {:<10} X {} s/d (H {}, V {}); D {} s/d, {} deg; DVH {} s/d, {} deg; Di {} s/d",
            if st.wind_h <= 0.0 {
                "full wind".to_string()
            } else {
                format!("{:.0} h", st.wind_h)
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
            "Isochronism  {} {} s/d from {:.0} h to {:.0} h ({} deg, beat error {} ms)",
            i.position,
            signed(Some(i.rate_change), 1),
            i.from_wind_h,
            i.to_wind_h,
            signed(i.amplitude_change, 0),
            signed(i.beat_error_change, 2)
        );
    }
    if r.im.is_some() || r.ie.is_some() {
        println!(
            "Im {} s/d, Im* {} s/d, Ie {} s/d, N {}",
            signed(r.im, 1),
            signed(r.im_all, 1),
            opt(r.ie, 1),
            opt(r.n, 1)
        );
    }
    println!();
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

    fn options(bph: Option<u32>, lift: Option<f64>, calibre: Option<&str>) -> Options {
        Options {
            bph,
            lift,
            calibre: calibre.map(String::from),
            tolerance: None,
            settle: None,
            out: None,
            json: false,
            init: false,
            no_shape: false,
            no_cycles: false,
        }
    }

    #[test]
    fn calibre_fills_what_the_flags_and_session_file_leave_out() {
        let none = Manifest::default();
        let o = options(None, None, Some("Unitas 6497-2"));
        let cal = o.calibre(&none).unwrap();
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
        let cal = o.calibre(&m).unwrap();
        assert_eq!(cal.map(|c| c.calibre.as_str()), Some("ETA 2824-2"));
        assert_eq!(o.bph(&m, cal), Some(28800));
        assert_eq!(o.lift(&m, cal), 50.0);
        assert!(o.calibre_notes(&m, cal).is_empty());
        // A name the table does not know is only a label.
        let m: Manifest = toml::from_str("calibre = \"Bespoke 9999\"\n").unwrap();
        assert!(o.calibre(&m).unwrap().is_none());
        // --calibre wins over the line, and explicit values over both, with
        // a note for each disagreement.
        let m: Manifest =
            toml::from_str("calibre = \"2824-2\"\nbph = 21600\nlift = 52\nescape_teeth = 15\n")
                .unwrap();
        let cal = o.calibre(&m).unwrap();
        assert_eq!(o.bph(&m, cal), Some(21600));
        assert_eq!(o.lift(&m, cal), 52.0);
        assert_eq!(
            o.calibre_notes(&m, cal).len(),
            3,
            "{:?}",
            o.calibre_notes(&m, cal)
        );
        let o = options(None, Some(55.0), Some("Rolex 3235"));
        let cal = o.calibre(&m).unwrap();
        assert_eq!(cal.map(|c| c.calibre.as_str()), Some("Rolex 3235"));
        assert_eq!(o.lift(&m, cal), 55.0);
        let notes = o.calibre_notes(&m, cal);
        assert!(notes[0].contains("ETA 2824-2"), "{notes:?}");
        assert_eq!(notes.len(), 3, "{notes:?}");
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
            .map(|e| span_of(e, 1500.0).unwrap())
            .collect();
        assert_eq!(spans, [(0.0, 600.0), (600.0, 1200.0), (1200.0, 1500.0)]);
    }

    #[test]
    fn whole_recording_without_a_span() {
        let e = RecordingEntry::default();
        assert_eq!(span_of(&e, 300.0).unwrap(), (0.0, 300.0));
    }

    #[test]
    fn empty_span_is_an_error() {
        let e = RecordingEntry {
            start_s: Some(400.0),
            ..Default::default()
        };
        assert!(span_of(&e, 300.0).is_err());
        let e = RecordingEntry {
            start_s: Some(100.0),
            end_s: Some(100.0),
            ..Default::default()
        };
        assert!(span_of(&e, 300.0).is_err());
    }
}
