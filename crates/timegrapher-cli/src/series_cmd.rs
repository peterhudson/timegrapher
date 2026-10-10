//! The `series` command: is each reading series of a take steady, or does
//! it carry a cycle, two states, a shifting mean, a drift or wander?

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use timegrapher_core::audio;
use timegrapher_core::clock::{self, ClockFit};
use timegrapher_core::longrun;
use timegrapher_core::longterm::LongConfig;
use timegrapher_core::periodicity::{standard_wheels, Wheel};
use timegrapher_core::steadiness::{self, p_text, Config, CycleSource, Report, SeriesCheck};
use timegrapher_core::stream::{self, StreamConfig};

pub struct Options<'a> {
    pub files: &'a [PathBuf],
    pub clock_log: Option<&'a Path>,
    pub stream: StreamConfig,
    pub escape_teeth: u32,
    pub wheels: &'a [String],
    pub check: Config,
    pub json: bool,
}

pub fn run(o: Options) -> Result<(), String> {
    let extra: Vec<Wheel> = o
        .wheels
        .iter()
        .map(|w| crate::long::parse_wheel(w))
        .collect::<Result<_, _>>()?;
    let files = crate::long::expand_dirs(o.files)?;
    let file = files.first().ok_or("no recording given")?.as_path();
    let mut info = audio::info(file).map_err(|e| format!("{}: {e}", file.display()))?;
    for f in &files[1..] {
        let i = audio::info(f).map_err(|e| format!("{}: {e}", f.display()))?;
        info.frames = info.frames.zip(i.frames).map(|(a, b)| a + b);
    }
    let clock = match o.clock_log {
        Some(p) => {
            let text = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let pairs = clock::parse_log(&text, info.sample_rate, info.bytes_per_frame)
                .map_err(|e| format!("{}: {e}", p.display()))?;
            Some(ClockFit::new(&pairs).map_err(|e| format!("{}: {e}", p.display()))?)
        }
        None => None,
    };

    let total = info.frames.map(|f| f as f64 / info.sample_rate as f64);
    let tty = std::io::stderr().is_terminal();
    let mut last = 0.0;
    let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
    let log = stream::analyze_files(&paths, &o.stream, |done| {
        if done - last >= 600.0 || total.is_some_and(|t| done >= t) {
            last = done;
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
        wheels: standard_wheels(log.bph, o.escape_teeth),
        ..Default::default()
    };
    lc.wheels.extend(extra);
    let long = longrun::analyse(&log, clock.as_ref(), &lc);
    let rep = steadiness::check(&log, clock.as_ref(), &long, &lc, &o.check);

    if o.json {
        let settings = serde_json::json!({
            "clock_log": o.clock_log.map(|p| p.display().to_string()),
            "bph": o.stream.analysis.bph,
            "lift_deg": o.stream.analysis.amplitude.lift_deg,
            "notch_hz": o.stream.analysis.envelope.notch_hz,
            "highpass_hz": o.stream.analysis.envelope.highpass_hz,
            "escape_teeth": o.escape_teeth,
            "wheels": o.wheels,
            "reading_s": o.check.rate_reading_s,
        });
        crate::output::print("series", crate::output::input(&files, settings), &rep)
    } else {
        print_text(&rep, clock.is_some());
        Ok(())
    }
}

fn print_text(r: &Report, calibrated: bool) {
    println!(
        "Recording    {} at {} bph{}",
        crate::long::duration(r.duration_s),
        r.bph,
        if calibrated { ", clock corrected" } else { "" }
    );
    for s in &r.series {
        print_series(s);
    }
    if r.findings.is_empty() {
        println!("\nEvery series is steady: the readings look independent about one level.");
    }
}

fn opt(v: Option<f64>, d: usize) -> String {
    v.map_or("-".into(), |x| format!("{x:.d$}"))
}

fn print_series(s: &SeriesCheck) {
    let name = s.series.name();
    println!();
    println!("{}", s.headline);
    println!(
        "  {:<14} {} readings of {:.0} s ({} outliers left out); median {}, sd {}, reading to reading {} {}",
        name,
        s.readings,
        s.step_s,
        s.outliers,
        opt(s.median, 2),
        opt(s.sd, 2),
        opt(s.short_term_sd, 2),
        s.unit
    );
    if let Some(a) = &s.autocorrelation {
        println!(
            "  {:<14} lag-1 r {:.2}, largest of {} lags {:.2} (band ±{:.2}); Ljung–Box p {}{}",
            "independence",
            a.r.get(1).copied().unwrap_or(f64::NAN),
            a.ljung_box_lags,
            a.max_short_lag_r,
            a.band,
            p_text(a.p_value),
            a.memory_s
                .map_or(String::new(), |m| format!("; memory {m:.0} s"))
        );
    }
    if let Some(a) = &s.allan {
        println!(
            "  {:<14} slope {} (−0.5 independent); up to {:.1}× the independent line; steadiest averaging {} s ({} {})",
            "Allan",
            opt(a.slope_short, 2),
            a.max_excess,
            opt(a.best_tau_s, 0),
            opt(a.best_deviation, 2),
            s.unit
        );
    }
    if let Some(c) = &s.cusum {
        println!(
            "  {:<14} max {:.2} at {:.0} s (p {} if independent)",
            "CUSUM",
            c.max,
            c.at_s,
            p_text(c.p_value)
        );
    }
    if let Some(c) = &s.changes {
        let means: Vec<String> = c
            .segments
            .iter()
            .map(|g| format!("{:.0}–{:.0} s: {:.2}", g.start_s, g.end_s, g.mean))
            .collect();
        println!(
            "  {:<14} {} ({:.0}% explained)",
            "levels",
            means.join("; "),
            c.explained * 100.0
        );
    }
    if let Some(t) = &s.trend {
        println!(
            "  {:<14} {:+.3} {}/h ({:.0}% explained)",
            "trend",
            t.per_hour,
            s.unit,
            t.explained * 100.0
        );
    }
    if let Some(c) = &s.cycle {
        println!(
            "  {:<14} {:.1} s{}, {:.2} {} p-p, explains {:.0}% (found by {})",
            "cycle",
            c.period_s,
            c.wheel
                .as_ref()
                .map_or(String::new(), |w| format!(" ({w})")),
            c.peak_to_peak,
            s.unit,
            c.explained * 100.0,
            match c.source {
                CycleSource::TwoState => "the two-state finder",
                CycleSource::Autocorrelation => "the autocorrelation",
                CycleSource::PeriodSearch => "the period search",
            }
        );
    }
    if let Some(p) = &s.period {
        println!(
            "  {:<14} {:.1} s{}, {:.2} {} p-p, explains {:.0}%, false alarm {}",
            "period search",
            p.period_s,
            p.wheel
                .as_ref()
                .map_or(String::new(), |w| format!(" ({w})")),
            p.size,
            p.size_unit,
            p.explained * 100.0,
            p_text(10f64.powf(-p.significance))
        );
    }
}
