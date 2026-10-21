//! `timegrapher session`: a watch measured in several positions, read
//! together into one report (Witschi's multi-position test).

use crate::long::expand_dirs;
use crate::long::{ResolvedCalibre, ResolvedTrain, Train};
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use timegrapher_core::calibre_store::{self, UserCalibres};
use timegrapher_core::calibres;
use timegrapher_core::clock::{self, ClockFit};
use timegrapher_core::dsp::{envelope, EnvelopeConfig};
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::{standard_wheels, Wheel};
use timegrapher_core::session::{
    self, Finding, Manifest, Position, Reading, RecordingEntry, SessionReport,
};
use timegrapher_core::shape::{self, ShapeConfig};
use timegrapher_core::stream::{self, BeatLog, StreamConfig};
use timegrapher_core::{analysis_take, audio, beats, breaks, timing, twostate};

pub const MANIFEST: &str = "session.toml";

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
        let teeth = m
            .escape_teeth
            .or(train.calibre.as_ref().and_then(|c| c.escape_teeth))
            .unwrap_or(15);
        Ok(SessionCalibre {
            train: train.with_escape_teeth(Some(teeth)),
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

    /// Beat rate: the flag, else the session file, else the calibre's;
    /// none means it is measured.
    fn bph(&self, m: &Manifest, cal: Option<&ResolvedCalibre>) -> Option<u32> {
        self.bph.or(m.bph).or(cal.and_then(|c| c.bph))
    }

    /// Lift angle: the flag, else the session file, else the calibre's,
    /// else 52 degrees.
    fn lift(&self, m: &Manifest, cal: Option<&ResolvedCalibre>) -> f64 {
        self.lift
            .or(m.lift)
            .or(cal.and_then(|c| c.lift_angle_deg))
            .unwrap_or(52.0)
    }
}

/// Seconds of audio the beat shape is measured on.
const SHAPE_S: f64 = 60.0;

/// Where the beat shape is measured, clear of masked stretches: the first
/// unmasked stretch from `from_s` to `to_s` that is [`SHAPE_S`] long, else
/// the longest one. Start and length, seconds; None when all of it is
/// masked.
fn shape_stretch(masked: &[(f64, f64)], from_s: f64, to_s: f64) -> Option<(f64, f64)> {
    let mut clear = Vec::new();
    let mut at = from_s;
    for &(a, b) in masked {
        if a > at {
            clear.push((at, a.min(to_s)));
        }
        at = at.max(b);
    }
    clear.push((at, to_s));
    let clear: Vec<(f64, f64)> = clear.into_iter().filter(|(a, b)| b > a).collect();
    let pick = clear.iter().find(|(a, b)| b - a >= SHAPE_S).or_else(|| {
        clear
            .iter()
            .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
    })?;
    Some((pick.0, SHAPE_S.min(pick.1 - pick.0)))
}

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
    if m.recordings.is_empty() {
        return Err(format!("{}: no [[recording]] entries", f.display()));
    }
    let base = f
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    Ok((m, base, crate::archive::sha256_hex(text.as_bytes())))
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
            .unwrap_or(52.0)
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
                p.name()
            )),
            None => t.push_str(
                "position = \"\"  # CH, CB, 3H, 6H, 9H, 12H (or DU, DD, CU, CL, CD, CR)\n",
            ),
        }
        t.push_str(
            "# wind_h = 0  # hours since the watch was fully wound (0 = just wound); left out, the reading is taken as fully wound\n",
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

fn read_clock(path: &Path, first: &Path) -> Result<(ClockFit, String), String> {
    let info = audio::info(first).map_err(|e| format!("{}: {e}", first.display()))?;
    read_clock_as(path, info.sample_rate, info.bytes_per_frame)
}

/// [`read_clock`] for a sound at `sample_rate` with `bytes_per_frame`.
fn read_clock_as(
    path: &Path,
    sample_rate: u32,
    bytes_per_frame: u32,
) -> Result<(ClockFit, String), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let pairs = clock::parse_log(&text, sample_rate, bytes_per_frame)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let fit = ClockFit::new(&pairs).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((fit, crate::archive::sha256_hex(text.as_bytes())))
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
    pub calibre: &'a SessionCalibre,
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

/// The analysis settings for a session's recordings, and its calibre.
fn stream_config<'a>(c: &Ctx<'a>) -> Result<(StreamConfig, Option<&'a ResolvedCalibre>), String> {
    let mut cfg = StreamConfig::default();
    let cal = c.calibre.train.calibre.as_ref();
    cfg.analysis.bph = c.o.bph(c.m, cal);
    cfg.analysis.amplitude.lift_deg = c.o.lift(c.m, cal);
    cfg.analysis.lift_known = lift_known(c, cal);
    cfg.analysis.escape_teeth =
        c.m.escape_teeth
            .or(cal.and_then(|k| k.escape_teeth))
            .unwrap_or(15);
    Ok((cfg, cal))
}

/// Whether the lift angle came from `--lift`, the session file or
/// the calibre, rather than the default.
fn lift_known(c: &Ctx, cal: Option<&ResolvedCalibre>) -> bool {
    if c.o.lift.is_some() {
        true
    } else if c.m.lift.is_some() {
        c.m.lift_known.unwrap_or(true)
    } else {
        cal.is_some_and(|k| k.lift_angle_deg.is_some())
    }
}

pub(crate) fn read_one(e: &RecordingEntry, c: &Ctx) -> Result<Reading, String> {
    read_one_inner(e, c, false).map(|(reading, _)| reading)
}

/// Baseline writers request context from this same resolution/analysis pass.
pub(crate) fn read_one_with_context(
    e: &RecordingEntry,
    c: &Ctx,
) -> Result<(Reading, crate::regress_provenance::ReadContext), String> {
    let (reading, context) = read_one_inner(e, c, true)?;
    let context = context.ok_or_else(|| "missing baseline provenance context".to_string())?;
    Ok((reading, context))
}

fn read_one_inner(
    e: &RecordingEntry,
    c: &Ctx,
    capture: bool,
) -> Result<(Reading, Option<crate::regress_provenance::ReadContext>), String> {
    let position = position_of(e)?;
    let path = c.base.join(&e.file);
    // A take kept as its analysis alone: its beats are read, not found.
    let kept = if analysis_take::is_analysis_only(&path) {
        if c.m.audio != Some(false) {
            return Err(format!(
                "{}: it has an {}/ folder, but {MANIFEST} doesn't say audio = false",
                e.file,
                analysis_take::DIR
            ));
        }
        if capture {
            return Err(format!(
                "{}: kept as its analysis alone, with no sound to record provenance for",
                e.file
            ));
        }
        Some(analysis_take::read(&path).map_err(|err| format!("{}: {err}", e.file))?)
    } else {
        None
    };
    let files = match &kept {
        Some(_) => Vec::new(),
        None => expand_dirs(std::slice::from_ref(&path))?,
    };
    let (cfg, cal) = stream_config(c)?;
    let escape_teeth = cfg.analysis.escape_teeth;
    if c.quiet {
    } else if c.tty {
        eprint!("\r{:<70}", format!("analysing {} ({})", e.file, position));
    } else {
        eprintln!("analysing {} ({})", e.file, position);
    }
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let mut log = match &kept {
        Some(k) => k.log.clone(),
        None => stream::analyze_files(&paths, &cfg, |_| {})
            .map_err(|err| format!("{}: {err}", e.file))?,
    };
    let (clock, clock_input_sha256) = match (&e.clock, c.m.card_ppm) {
        (Some(p), _) => {
            let (fit, digest) = match &kept {
                Some(k) => read_clock_as(
                    &c.base.join(p),
                    k.header.sample_rate,
                    k.header.bytes_per_frame,
                )?,
                None => read_clock(&c.base.join(p), &files[0])?,
            };
            (Some(fit), Some(digest))
        }
        (None, Some(ppm)) => (session::fixed_clock(ppm, log.duration_s), None),
        (None, None) => (None, None),
    };
    let listed = match &kept {
        // Every part of the take was in it, so its session.json lists its
        // breaks as for the whole take's sound.
        Some(_) => {
            let json = fs::read_to_string(path.join("session.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            breaks::for_take(json.as_ref(), clock.as_ref(), log.sample_rate)
        }
        None => breaks::of_files(&paths, clock.as_ref(), log.sample_rate),
    };
    let breaks = breaks::with_jumps(listed, &log.jumps);
    breaks::renumber(&mut log.beats, &mut log.amplitude_windows, &breaks, log.bph);
    // Stretches masked by hand are left out of everything read below.
    let masked = session::masked_spans(&e.masked);
    session::mask_log(&mut log, &masked);
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
    let mut measurement =
        session::measure_masked(&log, clock.as_ref(), &breaks, &masked, settle, end);
    if !lift_known(c, cal) {
        measurement.lift_unknown();
    }

    // Cycles and shape of noise would only mislead.
    let heard = measurement.watch_heard;
    let cycles = if !heard || c.o.no_cycles || c.m.cycles == Some(false) {
        Vec::new()
    } else {
        let lc = LongConfig {
            wheels: match cal {
                Some(_) => c.calibre.train.wheels(log.bph),
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
        // The part's times start at `settle`, so its breaks move with
        // it; a clock fit that holds the gaps at their own places is left
        // out (a ppm or so makes no odds to a cycle's period).
        let mut inside: Vec<_> = breaks
            .iter()
            .filter(|b| b.audio_s > settle && b.audio_s < end)
            .map(|b| breaks::Break {
                audio_s: b.audio_s - settle,
                gap_s: b.gap_s,
            })
            // A masked stretch is a missing one, with no time lost.
            .chain(
                masked
                    .iter()
                    .filter(|&&(a, _)| a > settle && a < end)
                    .map(|&(a, _)| breaks::Break {
                        audio_s: a - settle,
                        gap_s: 0.0,
                    }),
            )
            .collect();
        inside.sort_by(|a, b| a.audio_s.total_cmp(&b.audio_s));
        let part_clock = clock.as_ref().filter(|c| c.breaks.is_empty());
        session::cycles(
            &longrun::analyse_with_breaks(&part, part_clock, &inside, &lc),
            &lc.wheels,
        )
    };

    let shape = if !heard || c.o.no_shape || c.m.shape == Some(false) {
        None
    } else if let Some(k) = &kept {
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
        i.and_then(|i| k.shape_of(i)).map(|s| s.summary.clone())
    } else if let Some((from, len)) = shape_stretch(&masked, settle, end) {
        let (x, fs) = excerpt(&files, from, len)?;
        let env = envelope(&x, fs, &EnvelopeConfig::default());
        let (found, _) = beats::detect(&env, fs, log.bph);
        let beat_s = timing::fit(&found, log.bph).map_or(3600.0 / log.bph as f64, |f| f.period_s);
        let r = shape::analyze(&env, fs, &found, beat_s, &ShapeConfig::default());
        Some(session::shape_summary(&r, x.len() as f64 / fs))
    } else {
        None
    };

    let label = if e.start_s.is_some() || e.end_s.is_some() {
        format!("{} ({:.0}–{:.0} s)", e.file, start, stop)
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
    ))
}

/// Everything the report and `summary.json` show.
#[derive(serde::Serialize)]
pub struct Session<'a> {
    /// Name and version of this layout; bumped when a field changes meaning.
    pub schema: String,
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
    };
    let readings: Vec<Reading> = m
        .recordings
        .iter()
        .map(|e| read_one(e, &ctx))
        .collect::<Result<_, _>>()?;
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
        .map_or("-".into(), |x| format!("{x:.digits$}"))
}

pub fn signed(v: Option<f64>, digits: usize) -> String {
    v.filter(|x| x.is_finite())
        .map_or("-".into(), |x| format!("{x:+.digits$}"))
}

/// The state of wind in words: `wind_h` is hours since the watch was
/// fully wound. A reading without it is taken as fully wound.
pub fn wind(w: Option<f64>) -> String {
    match w {
        None => "not given".into(),
        Some(w) if w <= 0.0 => "full wind".into(),
        Some(w) => format!("{w:.0} h after winding"),
    }
}

/// A state of wind in a sentence ("at full wind", "24 h after winding").
fn wind_state(w: f64) -> String {
    if w <= 0.0 {
        "at full wind".into()
    } else {
        format!("{w:.0} h after winding")
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
    println!();
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
                session::no_regular_beat(m.jitter_us, rd.bph),
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
    println!(
        "Unlock is the share of 2 s windows that found the unlock on both sides, on which amplitude and BE rest; Clip is the share of beats with a clipped sample."
    );
    println!(
        "BE is beat error as a size; which beat is longer is not shown, because it depends on the beat the analysis locked on first."
    );
    println!(
        "Readings leave out the settling time at the start of each position, as record's quick check does. That check uses the session file's settle_s (30 s when the file has none, where this summary uses 20 s) and none of --settle, --bph, --lift, --calibre, a reading's own settle_s, measure_s, start_s, end_s, a clock log, card_ppm or a restart in listening, so a reading that uses one of these can differ from it."
    );
    if s.readings.iter().any(|r| r.measurement.calibrated) {
        println!("The sound card's clock is corrected where a clock log or card_ppm is given.");
    }
    if s.readings.iter().any(|r| r.wind_h.is_none()) {
        println!(
            "Wind is the hours since the watch was fully wound (wind_h in the session file); readings where it is not given are taken as fully wound."
        );
    }
    println!();
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
            wind_state(i.from_wind_h).trim_start_matches("at "),
            wind_state(i.to_wind_h).trim_start_matches("at "),
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
            ("lift = 52", None, true),          // Legacy CLI semantics.
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

    #[test]
    fn custom_session_catalogue_preserves_value_context_precedence_and_snapshot() {
        let mut user: UserCalibres = serde_json::from_value(serde_json::json!({
            "schema": calibre_store::SCHEMA,
            "custom":[{"name":"Bench 44","aliases":["Workshop-A"],"bph":21600,"lift_angle_deg":44.0,"escape_teeth":20,"wheels":[{"name":"third wheel","period_s":123.0}]}],
            "wheels":[{"calibre":"Rolex 3235","name":"third wheel","period_s":241.0}],
        })).unwrap();
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
            let ctx = Ctx {
                m: &m,
                calibre: &selection,
                base: Path::new("."),
                o: &o,
                tty: false,
                quiet: true,
            };
            let (cfg, _) = stream_config(&ctx).unwrap();
            assert_eq!(
                (
                    cfg.analysis.bph,
                    cfg.analysis.amplitude.lift_deg,
                    cfg.analysis.lift_known
                ),
                expected
            );
        }
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
        let ctx = Ctx {
            m: &m,
            calibre: &selected,
            base: Path::new("."),
            o: &o,
            tty: false,
            quiet: true,
        };
        assert_eq!(
            stream_config(&ctx).unwrap().0.analysis.amplitude.lift_deg,
            44.0
        );
        assert!(selected
            .train
            .wheels(21600)
            .iter()
            .any(|w| w.period_s == 123.0));
        assert!(options(None, None, Some("Unknown explicit"))
            .calibre_with(&m, &user)
            .is_err());
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
            ("card_ppm=-1000000", "card_ppm"),
        ] {
            let m: Manifest = toml::from_str(text).unwrap();
            assert!(m
                .validate_analysis_inputs()
                .unwrap_err()
                .starts_with(&format!("{field}:")));
        }
        let text="calibre=\"Unknown label\"\nlift_angle=52\nlift_known=false\ncard_ppm=0\ntolerance=\"old\"\n[limits]\nrate=99\n";
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
    fn a_session_file_with_a_tolerance_and_limits_still_reads() {
        // Written by `session --init` before schema /2; both are ignored.
        let m: Manifest = toml::from_str(
            "tolerance = \"mens\"\n[limits]\ndelta_rate = 12.0\noverbanking = 340.0\n",
        )
        .unwrap();
        assert!(m.recordings.is_empty());
        assert!(toml::from_str::<Manifest>("no_such_key = 1\n").is_err());
    }

    #[test]
    fn the_shape_is_measured_clear_of_masked_stretches() {
        assert_eq!(shape_stretch(&[], 30.0, 600.0), Some((30.0, SHAPE_S)));
        // A bump in the first minute: the next clear minute.
        assert_eq!(
            shape_stretch(&[(50.0, 58.0)], 30.0, 600.0),
            Some((58.0, SHAPE_S))
        );
        // No clear minute: the longest clear stretch.
        assert_eq!(
            shape_stretch(&[(40.0, 50.0), (70.0, 75.0)], 30.0, 100.0),
            Some((75.0, 25.0))
        );
        assert_eq!(shape_stretch(&[(0.0, 700.0)], 30.0, 600.0), None);
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

    /// A take kept as its analysis alone reads as the take with its sound
    /// did: the same readings and beat shape for each position, with the
    /// report saying no sound was kept.
    #[test]
    fn a_take_kept_as_its_analysis_reads_as_its_sound() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let base = std::env::temp_dir().join(format!("tg-session-kept-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
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
        let m: Manifest = toml::from_str(toml).unwrap();
        let mut cfg = StreamConfig::default();
        cfg.analysis.bph = Some(28800);
        cfg.analysis.amplitude.lift_deg = 52.0;
        let shapes = analysis_take::shape_stretches(&m, 120.0);
        let stored = analysis_take::measure(
            &[wav.as_path()],
            &cfg,
            &shapes,
            serde_json::json!({"name": "timegrapher", "version": "test", "git_hash": "abc"}),
            |_| {},
        )
        .unwrap();
        let kept = base.join("kept");
        let kept_toml = format!("audio = false\n{toml}");
        analysis_take::save(
            &kept,
            &[(MANIFEST.to_string(), kept_toml.into_bytes())],
            &stored,
        )
        .unwrap();
        let read = |dir: &Path| -> serde_json::Value {
            let out = dir.join("out");
            let o = Options {
                out: Some(out.clone()),
                ..options(None, None, None)
            };
            run(&[dir.to_path_buf()], &o).unwrap();
            let v = serde_json::from_str(&fs::read_to_string(out.join("summary.json")).unwrap())
                .unwrap();
            let _ = fs::remove_dir_all(&out);
            v
        };
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
            readings: Vec::new(),
            report: session::evaluate(&[]),
        };
        let html = crate::session_report::html(&s);
        assert!(html.contains("None kept") && html.contains("timegrapher test (abc)"));
        let _ = fs::remove_dir_all(&base);
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
