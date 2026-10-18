//! The `long` command: a long recording in, a report folder out.

use std::fs::{self, File};
use std::io::{BufWriter, IsTerminal, Write};
use std::path::{Path, PathBuf};
use timegrapher_core::calibre_store::{self, Catalog, Entry, UserCalibres};
#[cfg(test)]
use timegrapher_core::calibres::{self, Calibre};
use timegrapher_core::clock::{self, ClockFit};
use timegrapher_core::longrun::{self, LongReport};
use timegrapher_core::longterm::{Component, LongConfig};
use timegrapher_core::periodicity::{default_wheels, standard_wheels, Wheel};
use timegrapher_core::stream::{self, BeatLog, StreamConfig};
use timegrapher_core::{audio, breaks, session, timing};

/// Which wheels to name: a calibre's train, the escape wheel at a given
/// number of teeth, or both common escape wheels when neither is given.
pub(crate) struct Train<'a> {
    pub calibre: Option<&'a str>,
    pub escape_teeth: Option<u32>,
}

#[cfg(test)]
impl Train<'_> {
    /// The calibre named, checked before any audio is read.
    pub(crate) fn calibre(&self) -> Result<Option<&'static Calibre>, String> {
        self.calibre
            .map(|n| {
                calibres::find(n).ok_or_else(|| {
                    let known: Vec<&str> =
                        calibres::all().iter().map(|c| c.calibre.as_str()).collect();
                    format!(
                        "--calibre '{n}' is not in the table; known: {}",
                        known.join(", ")
                    )
                })
            })
            .transpose()
    }

    /// The wheels for a recording at `bph`. A calibre whose table gives no
    /// escape wheel gets one from its teeth, or `--escape-teeth`, or both
    /// common ones.
    pub(crate) fn wheels(&self, bph: u32) -> Result<Vec<Wheel>, String> {
        Ok(self.wheels_for(self.calibre()?, bph))
    }

    fn wheels_for(&self, calibre: Option<&Calibre>, bph: u32) -> Vec<Wheel> {
        let escape = |teeth: Option<u32>| match teeth {
            Some(t) => standard_wheels(bph, t),
            None => default_wheels(bph),
        };
        let Some(c) = calibre else {
            return escape(self.escape_teeth);
        };
        if let Some(cb) = c.bph.filter(|&cb| cb != bph) {
            eprintln!(
                "note: the {} beats at {cb} bph, the recording at {bph}",
                c.calibre
            );
        }
        let mut w = c.wheels();
        if !w.iter().any(|w| w.name == "escape wheel") {
            let teeth = self.escape_teeth.or(c.escape_teeth);
            w.extend(
                escape(teeth)
                    .into_iter()
                    .filter(|w| w.name == "escape wheel"),
            );
        }
        w
    }
}

/// Owned catalogue selection shared by CLI analysis and recording commands.
/// Regression callers deliberately supply an empty user catalogue.
#[derive(Debug)]
pub(crate) struct ResolvedTrain {
    pub calibre: Option<ResolvedCalibre>,
    escape_teeth: Option<u32>,
}

#[derive(Debug)]
pub(crate) struct ResolvedCalibre {
    pub name: String,
    pub bph: Option<u32>,
    pub lift_angle_deg: Option<f64>,
    pub escape_teeth: Option<u32>,
    wheels: Vec<Wheel>,
}

impl Train<'_> {
    pub(crate) fn resolve(&self) -> Result<ResolvedTrain, String> {
        // Defaults must not depend on a catalogue that was not requested.
        if self.calibre.is_none() {
            return self.resolve_with(&UserCalibres::default());
        }
        self.resolve_at(calibre_store::default_path().as_deref())
    }

    pub(crate) fn resolve_at(&self, path: Option<&Path>) -> Result<ResolvedTrain, String> {
        if self.calibre.is_none() {
            return self.resolve_with(&UserCalibres::default());
        }
        let user = match path {
            Some(path) => UserCalibres::load(path)?,
            None => UserCalibres::default(),
        };
        self.resolve_with(&user)
    }

    /// Manifest names that are unknown remain descriptive labels.
    pub(crate) fn resolve_label_with(&self, user: &UserCalibres) -> Result<ResolvedTrain, String> {
        if self
            .calibre
            .and_then(|name| Catalog::new(user).find(name))
            .is_none()
        {
            return Train {
                calibre: None,
                escape_teeth: self.escape_teeth,
            }
            .resolve_with(user);
        }
        self.resolve_with(user)
    }

    pub(crate) fn resolve_with(&self, user: &UserCalibres) -> Result<ResolvedTrain, String> {
        let cat = Catalog::new(user);
        let calibre = self.calibre.map(|name| {
            let entry = cat.find(name).ok_or_else(|| format!(
                "--calibre '{name}' is not in the catalogue; use `timegrapher calibres --search {name}`"
            ))?;
            Ok::<_, String>(resolved_calibre(&cat, entry))
        }).transpose()?;
        Ok(ResolvedTrain {
            calibre,
            escape_teeth: self.escape_teeth,
        })
    }
}

fn resolved_calibre(cat: &Catalog<'_>, entry: Entry<'_>) -> ResolvedCalibre {
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
    ResolvedCalibre {
        name: entry.name().to_string(),
        bph: entry.bph(),
        lift_angle_deg: entry.lift_angle_deg(),
        escape_teeth: entry.escape_teeth(),
        wheels,
    }
}

impl ResolvedTrain {
    pub(crate) fn with_escape_teeth(mut self, teeth: Option<u32>) -> Self {
        self.escape_teeth = teeth;
        self
    }

    /// Preserve the direct long/series train behavior: fill a missing escape
    /// wheel only. Their --calibre still does not set analysis BPH.
    pub(crate) fn wheels(&self, bph: u32) -> Vec<Wheel> {
        let escape = |teeth| match teeth {
            Some(t) => standard_wheels(bph, t),
            None => default_wheels(bph),
        };
        let Some(c) = &self.calibre else {
            return escape(self.escape_teeth);
        };
        if let Some(cb) = c.bph.filter(|&cb| cb != bph) {
            eprintln!(
                "note: the {} beats at {cb} bph, the recording at {bph}",
                c.name
            );
        }
        let mut wheels = c.wheels.clone();
        if !wheels.iter().any(|w| w.name == "escape wheel") {
            wheels.extend(
                escape(self.escape_teeth.or(c.escape_teeth))
                    .into_iter()
                    .filter(|w| w.name == "escape wheel"),
            );
        }
        wheels
    }

    /// The lift angle for long and series: `--lift`, else the calibre's,
    /// else the default.
    pub(crate) fn lift_deg(&self, flag: Option<f64>) -> f64 {
        flag.or(self.calibre.as_ref().and_then(|c| c.lift_angle_deg))
            .unwrap_or(crate::calibres_cmd::DEFAULT_LIFT_DEG)
    }

    /// What `lift_deg` fell back on, said as `analyze` says it, when
    /// neither `--lift` nor the calibre gave a lift angle.
    pub(crate) fn lift_default_note(&self, flag: Option<f64>) -> Option<String> {
        let why = match (flag, &self.calibre) {
            (Some(_), _) => return None,
            (None, Some(c)) if c.lift_angle_deg.is_some() => return None,
            (None, Some(_)) => "the calibre's lift angle is not in the table",
            (None, None) => "calibre not set",
        };
        Some(format!(
            "note: lift {}°, the default; {why}",
            crate::calibres_cmd::DEFAULT_LIFT_DEG
        ))
    }

    /// Analyze's existing explicit escape-teeth override wins over named wheels.
    pub(crate) fn analysis_wheels(&self, bph: u32) -> Vec<Wheel> {
        let mut wheels = self.wheels(bph);
        if let Some(teeth) = self.escape_teeth {
            wheels.retain(|w| w.name != "escape wheel");
            wheels.extend(
                standard_wheels(bph, teeth)
                    .into_iter()
                    .filter(|w| w.name == "escape wheel"),
            );
        }
        wheels
    }
}

pub(crate) fn parse_wheel(s: &str) -> Result<Wheel, String> {
    let (name, secs) = s
        .rsplit_once('=')
        .ok_or_else(|| format!("--wheel '{s}': expected NAME=SECONDS"))?;
    let period_s: f64 = secs
        .trim()
        .parse()
        .map_err(|_| format!("--wheel '{s}': '{secs}' is not a number of seconds"))?;
    if period_s <= 0.0 {
        return Err(format!("--wheel '{s}': period must be positive"));
    }
    Ok(Wheel {
        name: name.trim().to_string(),
        period_s,
    })
}

/// Expand each directory in place, in the desktop's manifest-first/natural
/// order. Explicit paths and repetitions keep their argument positions.
pub(crate) fn expand_dirs(files: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for path in files {
        let listed = if path.is_dir() {
            fs::read_to_string(path.join("session.toml"))
                .ok()
                .and_then(|text| toml::from_str::<session::Manifest>(&text).ok())
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
        out.extend(timegrapher_core::recording_order::recordings_in(
            path, &listed,
        )?);
    }
    Ok(out)
}

/// Where the sound card's clock error comes from: a clock log recorded
/// with the take, else `--card-ppm`, else the stored error for `device` or
/// for the input the recording names.
pub struct ClockArgs<'a> {
    pub log: Option<&'a Path>,
    pub device: Option<&'a str>,
    pub card_ppm: Option<f64>,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    files: &[PathBuf],
    clock_args: ClockArgs,
    cfg: &StreamConfig,
    lift: Option<f64>,
    train: Train,
    wheels: &[String],
    out: Option<PathBuf>,
    json: bool,
) -> Result<(), String> {
    let extra: Vec<Wheel> = wheels
        .iter()
        .map(|w| parse_wheel(w))
        .collect::<Result<_, _>>()?;
    let resolved_train = train.resolve()?;
    let mut cfg = cfg.clone();
    cfg.analysis.amplitude.lift_deg = resolved_train.lift_deg(lift);
    if let Some(note) = resolved_train.lift_default_note(lift) {
        eprintln!("{note}");
    }
    let cfg = &cfg;
    let files = expand_dirs(files)?;
    let file = files.first().ok_or("no recording given")?.as_path();
    let mut info = audio::info(file).map_err(|e| format!("{}: {e}", file.display()))?;
    for f in &files[1..] {
        let i = audio::info(f).map_err(|e| format!("{}: {e}", f.display()))?;
        info.frames = info.frames.zip(i.frames).map(|(a, b)| a + b);
    }
    let clock_log = clock_args.log;
    let clock = match clock_log {
        Some(p) => {
            let text = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let pairs = clock::parse_log(&text, info.sample_rate, info.bytes_per_frame)
                .map_err(|e| format!("{}: {e}", p.display()))?;
            Some(ClockFit::new(&pairs).map_err(|e| format!("{}: {e}", p.display()))?)
        }
        None => None,
    };
    // Without a log, a stored (or given) correction for the input: one
    // steady error over the run, which cannot follow drift.
    let fixed = match clock_log {
        Some(_) => None,
        None => {
            crate::clock_cmd::correction_or_usb(&files, clock_args.device, clock_args.card_ppm)?
        }
    };
    let total_s = info
        .frames
        .map_or(0.0, |f| f as f64 / info.sample_rate as f64);
    let clock = match &fixed {
        Some((ppm, _)) => session::fixed_clock(*ppm, total_s),
        None => clock,
    };
    let clock_note = fixed.as_ref().map(|(_, from)| from.as_str());
    let out = out.unwrap_or_else(|| {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("run");
        file.with_file_name(format!("{stem}_long"))
    });

    let total = info.frames.map(|f| f as f64 / info.sample_rate as f64);
    let tty = std::io::stderr().is_terminal();
    let mut last_report = 0.0;
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let log = stream::analyze_files(&paths, cfg, |done| {
        if done - last_report >= 600.0 || total.is_some_and(|t| done >= t) {
            last_report = done;
            let msg = match total {
                Some(t) => format!("analysed {:.0} of {:.0} min", done / 60.0, t / 60.0),
                None => format!("analysed {:.0} min", done / 60.0),
            };
            if tty {
                eprint!("\r{msg}   ");
            } else {
                eprintln!("{msg}");
            }
        }
    })
    .map_err(|e| e.to_string())?;
    if tty {
        eprintln!();
    }

    let mut lc = LongConfig {
        wheels: resolved_train.wheels(log.bph),
        ..Default::default()
    };
    lc.wheels.extend(extra);
    let mut log = log;
    let breaks = breaks::with_jumps(
        breaks::of_files(&paths, clock.as_ref(), log.sample_rate),
        &log.jumps,
    );
    breaks::renumber(&mut log.beats, &mut log.amplitude_windows, &breaks, log.bph);
    let rep = longrun::analyse_with_breaks(&log, clock.as_ref(), &breaks, &lc);

    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let io = |p: PathBuf, r: std::io::Result<()>| r.map_err(|e| format!("{}: {e}", p.display()));
    io(
        out.join("beats.csv"),
        write_beats(&out.join("beats.csv"), &log, clock.as_ref()),
    )?;
    io(
        out.join("amplitude.csv"),
        write_amplitude(&out.join("amplitude.csv"), &log, clock.as_ref()),
    )?;
    io(
        out.join("slices.csv"),
        write_slices(&out.join("slices.csv"), &rep),
    )?;
    io(
        out.join("folds.csv"),
        write_folds(&out.join("folds.csv"), &rep),
    )?;
    let settings = serde_json::json!({
        "clock_log": clock_log.map(|p| p.display().to_string()),
        "clock_correction": fixed.as_ref().map(|(ppm, from)| serde_json::json!({ "ppm": ppm, "from": from })),
        "bph": cfg.analysis.bph,
        "lift_deg": cfg.analysis.amplitude.lift_deg,
        "notch_hz": cfg.analysis.envelope.notch_hz,
        "highpass_hz": cfg.analysis.envelope.highpass_hz,
        "calibre": train.calibre,
        "escape_teeth": train.escape_teeth,
        "wheels": wheels,
        "named_wheels": lc.wheels,
        "out": out.display().to_string(),
    });
    let summary = crate::output::to_json(
        "long",
        crate::output::input(&files, settings),
        &Summary::from(&rep),
    )?;
    io(
        out.join("summary.json"),
        fs::write(out.join("summary.json"), &summary),
    )?;
    let name = file
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("recording");
    let title = match files.len() {
        1 => name.to_string(),
        n => format!("{name} and {} more", n - 1),
    };
    io(
        out.join("report.html"),
        fs::write(
            out.join("report.html"),
            crate::report::html(&title, &rep, &lc.wheels, clock_note),
        ),
    )?;

    if json {
        println!("{summary}");
    } else {
        print_summary(&rep, clock_note);
        println!("Report       {}", out.join("report.html").display());
    }
    Ok(())
}

/// The report without the long arrays, for `summary.json` and `--json`.
#[derive(serde::Serialize)]
struct Summary<'a> {
    duration_s: f64,
    sample_rate: u32,
    bph: u32,
    lift_deg: f64,
    beats_found: usize,
    clean_fraction: f64,
    clock: &'a Option<ClockFit>,
    overall: &'a Option<timing::TimingFit>,
    beat_error_unlock_ms: Option<f64>,
    rate_p05: Option<f64>,
    rate_p95: Option<f64>,
    amplitude_deg: Option<f64>,
    amplitude_p05: Option<f64>,
    amplitude_p95: Option<f64>,
    rate_periods: Vec<SlimComponent>,
    amplitude_periods: Vec<SlimComponent>,
}

#[derive(serde::Serialize)]
struct SlimComponent {
    period_s: f64,
    resolution_s: f64,
    significance: f64,
    harmonics: u8,
    explained: f64,
    /// s/d for rate, degrees for amplitude.
    peak_to_peak: f64,
    /// Rate only: peak-to-peak of the folded timing, ms.
    timing_peak_to_peak_ms: Option<f64>,
    /// The size to show and its unit: `peak_to_peak`, except that a rate
    /// cycle shorter than 30 s is stated as its timing swing in ms.
    size: f64,
    size_unit: &'static str,
    wheel: Option<String>,
    nearest_wheel: Option<(String, f64)>,
}

fn slim(
    c: &Component,
    ptp: f64,
    timing_ms: Option<f64>,
    (size, size_unit): (f64, &'static str),
) -> SlimComponent {
    SlimComponent {
        period_s: c.period_s,
        resolution_s: c.resolution_s,
        significance: c.significance,
        harmonics: c.harmonics,
        explained: c.explained,
        peak_to_peak: ptp,
        timing_peak_to_peak_ms: timing_ms,
        size,
        size_unit,
        wheel: c.wheel.clone(),
        nearest_wheel: c.nearest_wheel.clone(),
    }
}

impl<'a> From<&'a LongReport> for Summary<'a> {
    fn from(r: &'a LongReport) -> Self {
        Summary {
            duration_s: r.duration_s,
            sample_rate: r.sample_rate,
            bph: r.bph,
            lift_deg: r.lift_deg,
            beats_found: r.beats_found,
            clean_fraction: r.clean_fraction,
            clock: &r.clock,
            overall: &r.overall,
            beat_error_unlock_ms: r.beat_error_unlock_ms,
            rate_p05: r.rate_p05,
            rate_p95: r.rate_p95,
            amplitude_deg: r.amplitude_deg,
            amplitude_p05: r.amplitude_p05,
            amplitude_p95: r.amplitude_p95,
            rate_periods: r
                .rate_components
                .iter()
                .map(|c| {
                    slim(
                        &c.component,
                        c.rate_swing_s_per_day,
                        Some(c.timing_swing_ms),
                        c.size(),
                    )
                })
                .collect(),
            amplitude_periods: r
                .amplitude
                .components
                .iter()
                .map(|c| slim(c, c.peak_to_peak, None, (c.peak_to_peak, "deg")))
                .collect(),
        }
    }
}

pub fn duration(s: f64) -> String {
    let s = s.round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub fn false_alarm(sig: f64) -> String {
    if sig > 300.0 {
        "<1e-300".into()
    } else {
        format!("{:.0e}", 10f64.powf(-sig))
    }
}

pub fn wheel_note(c: &Component) -> String {
    // The match rule is longterm's: within the search's resolution or 1%
    // of the wheel's turn, whichever is wider.
    let rule = format!("±{:.1} s or 1%", c.resolution_s);
    let pct = |off: f64| format!("{:+.1}%", (off * 10.0).round() / 10.0 + 0.0);
    match (&c.wheel, &c.nearest_wheel) {
        (Some(w), Some((n, off))) if n == w => {
            format!(
                "matches the {w} ({} off; a match is within {rule})",
                pct(*off)
            )
        }
        (Some(w), _) => format!("matches the {w} (a match is within {rule})"),
        (None, Some((w, off))) => format!(
            "no wheel listed within {rule}; nearest is the {w}, {} off",
            pct(*off)
        ),
        (None, None) => String::new(),
    }
}

fn opt(v: Option<f64>, digits: usize) -> String {
    v.map_or("-".into(), |x| format!("{x:.digits$}"))
}

fn print_summary(r: &LongReport, clock_note: Option<&str>) {
    println!(
        "Recording    {} at {} Hz, {} bph, {} beats ({:.1}% clean)",
        duration(r.duration_s),
        r.sample_rate,
        r.bph,
        r.beats_found,
        r.clean_fraction * 100.0
    );
    match (&r.clock, clock_note) {
        (Some(c), Some(note)) => println!(
            "Clock        sound card {:.2} ppm {} than true time, so uncorrected rates read {:.2} s/d {}; corrected with the steady error {note}",
            c.ppm.abs(),
            if c.ppm >= 0.0 { "slower" } else { "faster" },
            c.rate_error_s_per_day.abs(),
            if c.ppm >= 0.0 { "fast" } else { "slow" },
        ),
        (Some(c), None) => println!(
            "Clock        sound card {:.2} ppm {} than NTP time, so uncorrected rates read {:.2} s/d {}; corrected from {} entries, {:.1} ms rms{}",
            c.ppm.abs(),
            if c.ppm >= 0.0 { "slower" } else { "faster" },
            c.rate_error_s_per_day.abs(),
            if c.ppm >= 0.0 { "fast" } else { "slow" },
            c.points,
            c.residual_ms,
            if c.tracks_drift { ", drift followed" } else { "" }
        ),
        (None, _) => println!("Clock        not calibrated: rates are read on the sound card's clock, whose error is not measured here (--clock, or a card error stored with `timegrapher clock`, corrects it)"),
    }
    if r.clock.is_some() {
        println!("             tg, and a session's reference rates, read on the sound card's clock: add the error above to compare");
    }
    match &r.overall {
        Some(f) => println!(
            "Rate         {:+.2} s/d over the run; {} slices from {} to {} s/d (5th-95th pct)",
            f.rate_s_per_day,
            crate::report::slice_name(r.slice_s),
            opt(r.rate_p05, 1),
            opt(r.rate_p95, 1)
        ),
        None => println!("Rate         not enough clean beats to fit"),
    }
    if let Some(f) = &r.overall {
        println!(
            "Beat error   {}",
            crate::beat_error_text(r.beat_error_unlock_ms, f.beat_error_ms)
        );
    }
    println!(
        "Amplitude    {} deg median; slices from {} to {} deg (lift angle {} deg)",
        opt(r.amplitude_deg, 0),
        opt(r.amplitude_p05, 0),
        opt(r.amplitude_p95, 0),
        r.lift_deg
    );
    let line = |c: &Component, size: String| {
        println!(
            "  {:>9.2} s ±{:<6.2} {:<22} explains {:>3.0}%  false alarm {:<7}  {}",
            c.period_s,
            c.resolution_s,
            size,
            c.explained * 100.0,
            false_alarm(c.significance),
            wheel_note(c)
        );
    };
    println!("Periodic changes in rate:");
    if r.rate_components.is_empty() {
        println!("  none above the 1% false-alarm level");
    }
    for c in &r.rate_components {
        let swing = match c.size() {
            (ms, "ms") => format!("timing swing {ms:.3} ms p-p"),
            (sd, unit) => format!("swing {sd:.1} {unit} p-p"),
        };
        line(&c.component, swing);
    }
    println!("Periodic changes in amplitude:");
    if r.amplitude.components.is_empty() {
        println!("  none above the 1% false-alarm level");
    }
    for c in &r.amplitude.components {
        line(c, format!("swing {:.1} deg p-p", c.peak_to_peak));
    }
}

fn write_beats(p: &Path, log: &BeatLog, clock: Option<&ClockFit>) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(p)?);
    writeln!(w, "index,time_s,true_time_s,quality")?;
    for b in &log.beats {
        writeln!(
            w,
            "{},{:.6},{:.6},{:.3}",
            b.index,
            b.time,
            clock.map_or(b.time, |c| c.map(b.time)),
            b.quality
        )?;
    }
    w.flush()
}

fn write_amplitude(p: &Path, log: &BeatLog, clock: Option<&ClockFit>) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(p)?);
    writeln!(
        w,
        "start_s,end_s,true_start_s,even_deg,odd_deg,beat_error_ms,beat_error_unlock_ms"
    )?;
    for a in &log.amplitude_windows {
        writeln!(
            w,
            "{:.3},{:.3},{:.3},{},{},{},{}",
            a.start_s,
            a.end_s,
            clock.map_or(a.start_s, |c| c.map(a.start_s)),
            a.even_deg.map_or(String::new(), |v| format!("{v:.1}")),
            a.odd_deg.map_or(String::new(), |v| format!("{v:.1}")),
            a.beat_error_ms.map_or(String::new(), |v| format!("{v:.3}")),
            a.beat_error_unlock_ms
                .map_or(String::new(), |v| format!("{v:.3}"))
        )?;
    }
    w.flush()
}

fn write_slices(p: &Path, r: &LongReport) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(p)?);
    writeln!(
        w,
        "start_s,end_s,rate_s_per_day,beat_error_ms,beat_error_unlock_ms,amplitude_deg"
    )?;
    for s in &r.slices {
        writeln!(
            w,
            "{:.1},{:.1},{},{},{},{}",
            s.start_s,
            s.end_s,
            opt(s.rate_s_per_day, 2),
            opt(s.beat_error_ms, 3),
            opt(s.beat_error_unlock_ms, 3),
            opt(s.amplitude_deg, 1)
        )?;
    }
    w.flush()
}

fn write_folds(p: &Path, r: &LongReport) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(p)?);
    writeln!(w, "series,period_s,phase,mean,shape,unit")?;
    for c in &r.rate_components {
        let n = c.rate_shape.len();
        for (i, v) in c.rate_shape.iter().enumerate() {
            writeln!(
                w,
                "rate,{:.3},{:.4},,{:.3},s/d",
                c.component.period_s,
                (i as f64 + 0.5) / n as f64,
                v
            )?;
        }
    }
    for c in &r.amplitude.components {
        let n = c.shape.len();
        for (i, (m, s)) in c.fold.profile.iter().zip(&c.shape).enumerate() {
            writeln!(
                w,
                "amplitude,{:.3},{:.4},{:.3},{:.3},deg",
                c.period_s,
                (i as f64 + 0.5) / n as f64,
                m,
                s
            )?;
        }
    }
    w.flush()
}

#[cfg(test)]
mod recording_order_tests {
    use super::*;

    struct Folder(PathBuf);
    impl Folder {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("tg-cli-order-{}-{n}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            for name in ["part10.wav", "part2.FLAC", "part1.wav", "notes.txt"] {
                fs::write(path.join(name), []).unwrap();
            }
            fs::create_dir(path.join("directory.wav")).unwrap();
            Self(path)
        }
        fn manifest(&self, text: &str) {
            fs::write(self.0.join("session.toml"), text).unwrap();
        }
        fn names(&self) -> Vec<String> {
            expand_dirs(std::slice::from_ref(&self.0))
                .unwrap()
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn directory_expansion_uses_typed_manifest_then_natural_order() {
        let folder = Folder::new();
        assert_eq!(folder.names(), ["part1.wav", "part2.FLAC", "part10.wav"]);
        folder.manifest("[[recording]]\nfile='part10.wav'\nposition='DU'\nstart_s=0\nend_s=1\n\n[[recording]]\nfile='part10.wav'\nposition='DD'\nstart_s=1\nend_s=2\n\n[[recording]]\nfile='part2.FLAC'\n");
        assert_eq!(folder.names(), ["part10.wav", "part2.FLAC", "part1.wav"]);
    }

    #[test]
    fn invalid_manifest_falls_back_without_changing_session_validation() {
        let folder = Folder::new();
        for text in [
            "[[recording]",
            "unknown_field=true\n[[recording]]\nfile='part10.wav'\n",
            "[[recording]]\nposition='DU'\n",
        ] {
            folder.manifest(text);
            assert_eq!(
                folder.names(),
                ["part1.wav", "part2.FLAC", "part10.wav"],
                "{text}"
            );
        }
    }

    #[test]
    fn missing_nested_and_whole_folder_entries_do_not_hide_direct_files() {
        let folder = Folder::new();
        fs::create_dir(folder.0.join("nested")).unwrap();
        fs::write(folder.0.join("nested/part2.FLAC"), []).unwrap();
        folder.manifest("[[recording]]\nfile='.'\n\n[[recording]]\nfile='missing.wav'\n\n[[recording]]\nfile='nested/part2.FLAC'\n\n[[recording]]\nfile='part10.wav'\n");
        assert_eq!(folder.names(), ["part10.wav", "part1.wav", "part2.FLAC"]);
        // read_one also expands base.join(".") for a whole-folder entry.
        assert_eq!(
            expand_dirs(&[folder.0.join(".")]).unwrap(),
            ["part10.wav", "part1.wav", "part2.FLAC"].map(|name| folder.0.join(name))
        );
    }

    #[test]
    fn explicit_argument_order_and_repetitions_survive_folder_expansion() {
        let folder = Folder::new();
        let first = folder.0.join("part10.wav");
        let last = folder.0.join("not-present.anything");
        let input = [first.clone(), folder.0.clone(), first.clone(), last.clone()];
        assert_eq!(
            expand_dirs(&input).unwrap(),
            [
                first.clone(),
                folder.0.join("part1.wav"),
                folder.0.join("part2.FLAC"),
                first.clone(),
                first,
                last
            ]
        );
    }

    #[test]
    fn causal_natural_order_without_audio_named_directories() {
        let folder = Folder::new();
        fs::remove_dir(folder.0.join("directory.wav")).unwrap();
        // No manifest or audio-named directory: only numeric order differs.
        assert_eq!(folder.names(), ["part1.wav", "part2.FLAC", "part10.wav"]);
    }

    #[test]
    fn causal_manifest_precedence_with_matching_lexical_and_natural_names() {
        let folder = Folder::new();
        fs::remove_dir(folder.0.join("directory.wav")).unwrap();
        for name in ["part1.wav", "part2.FLAC", "part10.wav"] {
            fs::remove_file(folder.0.join(name)).unwrap();
        }
        for name in ["alpha.wav", "beta.wav", "gamma.wav"] {
            fs::write(folder.0.join(name), []).unwrap();
        }
        folder.manifest("[[recording]]\nfile='gamma.wav'\n");
        // Lexical and natural order agree; only manifest precedence differs.
        assert_eq!(folder.names(), ["gamma.wav", "alpha.wav", "beta.wav"]);
    }
}

#[cfg(test)]
mod catalogue_tests {
    use super::*;
    use serde_json::json;

    fn user() -> UserCalibres {
        serde_json::from_value(json!({
            "schema": calibre_store::SCHEMA,
            "custom": [{"name": "Bench 44", "aliases": ["Workshop-A"], "bph": 21600,
                "lift_angle_deg": 44.0, "escape_teeth": 20,
                "wheels": [{"name":"escape wheel","period_s":8.0},{"name":"third wheel","period_s":123.0}]}],
            "wheels": [{"calibre":"Rolex 3235","name":"third wheel","period_s":241.0}],
        })).unwrap()
    }

    fn pairs(wheels: Vec<Wheel>) -> Vec<(String, f64)> {
        wheels.into_iter().map(|w| (w.name, w.period_s)).collect()
    }

    #[test]
    fn catalogue_custom_alias_resolves_discovered_values_and_wheels() {
        let user = user();
        let cat = Catalog::new(&user);
        let discovered = cat.find("workshop a").unwrap();
        let train = Train {
            calibre: Some("workshop a"),
            escape_teeth: None,
        }
        .resolve_with(&user)
        .unwrap();
        let c = train.calibre.as_ref().unwrap();
        assert_eq!(c.name, discovered.name());
        assert_eq!(c.bph, Some(21600));
        assert_eq!(c.lift_angle_deg, Some(44.0));
        assert_eq!(c.escape_teeth, Some(20));
        assert_eq!(pairs(train.wheels(21600)), pairs(cat.wheels(discovered)));
        assert!(
            Train {
                calibre: Some("workshop a"),
                escape_teeth: None
            }
            .calibre()
            .is_err(),
            "legacy session adapter remains deliberately separate"
        );
    }

    #[test]
    fn lift_comes_from_the_flag_then_the_calibre_then_the_default() {
        let user = user();
        let named = Train {
            calibre: Some("workshop a"),
            escape_teeth: None,
        }
        .resolve_with(&user)
        .unwrap();
        assert_eq!(named.lift_deg(None), 44.0);
        assert_eq!(named.lift_deg(Some(50.0)), 50.0);
        let none = Train {
            calibre: None,
            escape_teeth: None,
        }
        .resolve_with(&user)
        .unwrap();
        assert_eq!(none.lift_deg(None), crate::calibres_cmd::DEFAULT_LIFT_DEG);
        assert_eq!(
            none.lift_default_note(None).as_deref(),
            Some("note: lift 52°, the default; calibre not set")
        );
        assert_eq!(named.lift_default_note(None), None);
        assert_eq!(none.lift_default_note(Some(50.0)), None);
    }

    #[test]
    fn a_calibre_without_a_lift_figure_falls_back_and_says_so() {
        let mut user = user();
        user.custom[0].lift_angle_deg = None;
        let train = Train {
            calibre: Some("workshop a"),
            escape_teeth: None,
        }
        .resolve_with(&user)
        .unwrap();
        assert_eq!(train.lift_deg(None), crate::calibres_cmd::DEFAULT_LIFT_DEG);
        assert_eq!(train.lift_deg(Some(48.0)), 48.0);
        assert_eq!(
            train.lift_default_note(None).as_deref(),
            Some("note: lift 52°, the default; the calibre's lift angle is not in the table")
        );
    }

    #[test]
    fn catalogue_custom_name_shadow_uses_existing_catalogue_precedence() {
        let mut user = user();
        user.custom[0].aliases.push("ETA 2824-2".into());
        let entry = Catalog::new(&user).find("ETA 2824-2").unwrap();
        assert!(entry.is_custom());
        let resolved = Train {
            calibre: Some("ETA 2824-2"),
            escape_teeth: None,
        }
        .resolve_with(&user)
        .unwrap();
        let c = resolved.calibre.as_ref().unwrap();
        assert_eq!(c.name, entry.name());
        assert_eq!(c.lift_angle_deg, Some(44.0));
        assert_eq!(
            pairs(resolved.wheels(21600)),
            pairs(Catalog::new(&user).wheels(entry))
        );
    }

    #[test]
    fn catalogue_snapshot_is_owned_and_shipped_overrides_match_discovery() {
        let mut user = user();
        let train = Train {
            calibre: Some("Rolex 3235"),
            escape_teeth: None,
        }
        .resolve_with(&user)
        .unwrap();
        let discovery = Catalog::new(&user).wheels(Catalog::new(&user).find("Rolex 3235").unwrap());
        let effective = pairs(train.wheels(28800));
        assert!(effective.contains(&("third wheel".into(), 241.0)));
        for wheel in discovery {
            assert!(effective.contains(&(wheel.name, wheel.period_s)));
        }
        user.wheels[0].period_s = 999.0;
        assert_eq!(
            pairs(train.wheels(28800)),
            effective,
            "no store reread during analysis"
        );
        let custom = Train {
            calibre: Some("Workshop-A"),
            escape_teeth: Some(15),
        }
        .resolve_with(&user)
        .unwrap();
        assert!(
            pairs(custom.wheels(21600)).contains(&("escape wheel".into(), 8.0)),
            "long/series retain their named-wheel precedence"
        );
        let overridden = pairs(custom.analysis_wheels(21600));
        assert!(!overridden.contains(&("escape wheel".into(), 8.0)));
        let expected = standard_wheels(21600, 15)
            .into_iter()
            .find(|w| w.name == "escape wheel")
            .unwrap();
        assert!(overridden.contains(&(expected.name, expected.period_s)));
        assert!(overridden.contains(&("third wheel".into(), 123.0)));
    }

    #[test]
    fn catalogue_empty_store_preserves_every_shipped_train_and_defaults() {
        let empty = UserCalibres::default();
        for c in calibres::all() {
            let bph = c.bph.unwrap_or(28800);
            for teeth in [None, Some(15), Some(20)] {
                let train = Train {
                    calibre: Some(&c.calibre),
                    escape_teeth: teeth,
                };
                let resolved = ResolvedTrain {
                    calibre: Some(resolved_calibre(&Catalog::new(&empty), Entry::Table(c))),
                    escape_teeth: teeth,
                };
                assert_eq!(
                    pairs(resolved.wheels(bph)),
                    pairs(train.wheels_for(Some(c), bph)),
                    "{} {teeth:?}",
                    c.calibre
                );
            }
        }
        for teeth in [None, Some(15), Some(20)] {
            let train = Train {
                calibre: None,
                escape_teeth: teeth,
            };
            assert_eq!(
                pairs(train.resolve_with(&empty).unwrap().wheels(28800)),
                pairs(train.wheels(28800).unwrap())
            );
        }
        assert!(Train {
            calibre: Some("not a calibre"),
            escape_teeth: None
        }
        .resolve_with(&empty)
        .is_err());
        let mut partial = empty;
        partial
            .custom
            .push(serde_json::from_value(json!({"name":"Partial","wheels":[]})).unwrap());
        let train = Train {
            calibre: Some("Partial"),
            escape_teeth: None,
        }
        .resolve_with(&partial)
        .unwrap();
        assert!(train.calibre.as_ref().unwrap().bph.is_none());
        assert_eq!(
            pairs(train.wheels(28800)),
            pairs(
                default_wheels(28800)
                    .into_iter()
                    .filter(|w| w.name == "escape wheel")
                    .collect()
            )
        );
    }
}
