//! The `long` command: a long recording in, a report folder out.

use std::fs::{self, File};
use std::io::{BufWriter, IsTerminal, Write};
use std::path::{Path, PathBuf};
use timegrapher_core::clock::{self, ClockFit};
use timegrapher_core::longrun::{self, LongReport};
use timegrapher_core::longterm::{Component, LongConfig};
use timegrapher_core::periodicity::{standard_wheels, Wheel};
use timegrapher_core::stream::{self, BeatLog, StreamConfig};
use timegrapher_core::{audio, timing};

fn parse_wheel(s: &str) -> Result<Wheel, String> {
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

/// Replace each folder with the WAV and FLAC files in it, sorted by name.
pub(crate) fn expand_dirs(files: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for f in files {
        if !f.is_dir() {
            out.push(f.clone());
            continue;
        }
        let mut inner: Vec<PathBuf> = fs::read_dir(f)
            .map_err(|e| format!("{}: {e}", f.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    e.eq_ignore_ascii_case("wav") || e.eq_ignore_ascii_case("flac")
                })
            })
            .collect();
        if inner.is_empty() {
            return Err(format!("{}: no WAV or FLAC files", f.display()));
        }
        inner.sort();
        out.extend(inner);
    }
    Ok(out)
}

pub fn run(
    files: &[PathBuf],
    clock_log: Option<&Path>,
    cfg: &StreamConfig,
    escape_teeth: u32,
    wheels: &[String],
    out: Option<PathBuf>,
    json: bool,
) -> Result<(), String> {
    let extra: Vec<Wheel> = wheels
        .iter()
        .map(|w| parse_wheel(w))
        .collect::<Result<_, _>>()?;
    let files = expand_dirs(files)?;
    let file = files.first().ok_or("no recording given")?.as_path();
    let mut info = audio::info(file).map_err(|e| format!("{}: {e}", file.display()))?;
    for f in &files[1..] {
        let i = audio::info(f).map_err(|e| format!("{}: {e}", f.display()))?;
        info.frames = info.frames.zip(i.frames).map(|(a, b)| a + b);
    }
    let clock = match clock_log {
        Some(p) => {
            let text = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let pairs = clock::parse_log(&text, info.sample_rate, info.bytes_per_frame)
                .map_err(|e| format!("{}: {e}", p.display()))?;
            Some(ClockFit::new(&pairs).map_err(|e| format!("{}: {e}", p.display()))?)
        }
        None => None,
    };
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
        wheels: standard_wheels(log.bph, escape_teeth),
        ..Default::default()
    };
    lc.wheels.extend(extra);
    let rep = longrun::analyse(&log, clock.as_ref(), &lc);

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
        "bph": cfg.analysis.bph,
        "lift_deg": cfg.analysis.amplitude.lift_deg,
        "notch_hz": cfg.analysis.envelope.notch_hz,
        "highpass_hz": cfg.analysis.envelope.highpass_hz,
        "escape_teeth": escape_teeth,
        "wheels": wheels,
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
            crate::report::html(&title, &rep, &lc.wheels),
        ),
    )?;

    if json {
        println!("{summary}");
    } else {
        print_summary(&rep);
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
    wheel: Option<String>,
    nearest_wheel: Option<(String, f64)>,
}

fn slim(c: &Component, ptp: f64) -> SlimComponent {
    SlimComponent {
        period_s: c.period_s,
        resolution_s: c.resolution_s,
        significance: c.significance,
        harmonics: c.harmonics,
        explained: c.explained,
        peak_to_peak: ptp,
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
                .map(|c| slim(&c.component, c.rate_swing_s_per_day))
                .collect(),
            amplitude_periods: r
                .amplitude
                .components
                .iter()
                .map(|c| slim(c, c.peak_to_peak))
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
    match (&c.wheel, &c.nearest_wheel) {
        (Some(w), _) => format!("matches the {w}"),
        (None, Some((w, off))) => format!("not a wheel listed; nearest is the {w}, {off:+.1}% off"),
        (None, None) => String::new(),
    }
}

fn opt(v: Option<f64>, digits: usize) -> String {
    v.map_or("-".into(), |x| format!("{x:.digits$}"))
}

fn print_summary(r: &LongReport) {
    println!(
        "Recording    {} at {} Hz, {} bph, {} beats ({:.1}% clean)",
        duration(r.duration_s),
        r.sample_rate,
        r.bph,
        r.beats_found,
        r.clean_fraction * 100.0
    );
    match &r.clock {
        Some(c) => println!(
            "Clock        sound card {:.2} ppm {} than NTP time, so uncorrected rates read {:.2} s/d {}; corrected from {} entries, {:.1} ms rms{}",
            c.ppm.abs(),
            if c.ppm >= 0.0 { "slower" } else { "faster" },
            c.rate_error_s_per_day.abs(),
            if c.ppm >= 0.0 { "fast" } else { "slow" },
            c.points,
            c.residual_ms,
            if c.tracks_drift { ", drift followed" } else { "" }
        ),
        None => println!("Clock        not calibrated: absolute rate is only as good as the sound card (use --clock)"),
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
        line(
            &c.component,
            format!("swing {:.1} s/d p-p", c.rate_swing_s_per_day),
        );
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
