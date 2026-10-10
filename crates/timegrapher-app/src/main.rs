//! Desktop timegrapher: the live trace with rate, amplitude and beat error
//! from a microphone or a recording.
//!
//! All analysis is in `timegrapher-core`; this crate draws it. Two modes
//! without a window, for scripts and agents:
//!
//! - `timegrapher-app --devices` lists the sound input devices as JSON.
//! - `timegrapher-app --headless FILE` runs a recording through the live
//!   engine as fast as it can and prints the readings as JSON lines.

mod app;
mod fields;
mod help;
mod profiles;
mod settings;
mod steady;
mod strip;
mod theme;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use timegrapher_core::audio;
use timegrapher_core::live::{LiveAnalyzer, LiveConfig};

const USAGE: &str = "\
Usage:
  timegrapher-app [FILE]            open the window (FILE: a recording to replay,
                                    or a folder of segments to analyse as one)
  timegrapher-app --analyse FILE    open the window with FILE analysed all at once
  timegrapher-app --devices         list sound input devices as JSON
  timegrapher-app --headless FILE [--bph N] [--lift DEG] [--average S] [--every S]
                                    print live readings for FILE as JSON lines";

struct Args {
    file: Option<PathBuf>,
    headless: bool,
    devices: bool,
    analyse: bool,
    bph: Option<u32>,
    lift: f64,
    average_s: f64,
    every_s: f64,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        file: None,
        headless: false,
        devices: false,
        analyse: false,
        bph: None,
        lift: 52.0,
        average_s: 10.0,
        every_s: 10.0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| -> Result<String, String> {
            it.next().ok_or_else(|| format!("{name} needs a value"))
        };
        let num = |v: String, name: &str| -> Result<f64, String> {
            v.parse::<f64>()
                .ok()
                .filter(|x| x.is_finite() && *x > 0.0)
                .ok_or_else(|| format!("{name}: not a positive number: {v}"))
        };
        match arg.as_str() {
            "--headless" => a.headless = true,
            "--devices" => a.devices = true,
            "--analyse" | "--analyze" => a.analyse = true,
            "--bph" => a.bph = Some(num(value("--bph")?, "--bph")? as u32),
            "--lift" => a.lift = num(value("--lift")?, "--lift")?,
            "--average" => a.average_s = num(value("--average")?, "--average")?,
            "--every" => a.every_s = num(value("--every")?, "--every")?,
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => a.file = Some(PathBuf::from(s)),
        }
    }
    Ok(a)
}

fn headless(a: &Args) -> Result<(), String> {
    let path = a.file.as_ref().ok_or("--headless needs a FILE")?;
    let info = audio::info(path).map_err(|e| e.to_string())?;
    let mut cfg = LiveConfig::default();
    cfg.analysis.bph = a.bph;
    cfg.analysis.amplitude.lift_deg = a.lift;
    let mut live = LiveAnalyzer::new(info.sample_rate, cfg);
    let mut next = a.every_s;
    // Stop printing quietly if the reader goes away (`| head`).
    let mut out = std::io::stdout().lock();
    let mut open = true;
    let mut print = |live: &LiveAnalyzer| {
        if open {
            let r = live.reading(a.average_s);
            open = writeln!(out, "{}", serde_json::to_string(&r).expect("json")).is_ok();
        }
    };
    audio::stream(path, 4800, |block| {
        live.push(block);
        if live.duration_s() >= next {
            print(&live);
            next += a.every_s;
        }
    })
    .map_err(|e| e.to_string())?;
    print(&live);
    Ok(())
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}");
            }
            eprintln!("{USAGE}");
            return if e.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            };
        }
    };
    if args.devices {
        return match timegrapher_core::capture::list() {
            Ok(d) => {
                println!("{}", serde_json::to_string_pretty(&d).expect("json"));
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    if args.headless {
        return match headless(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Timegrapher")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([800.0, 560.0]),
        ..Default::default()
    };
    let file = args.file.clone();
    let analyse = args.analyse;
    match eframe::run_native(
        "Timegrapher",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            let saved = cc.storage.and_then(settings::load);
            if let Some(s) = &saved {
                cc.egui_ctx.set_theme(s.theme);
            }
            Ok(Box::new(app::TimegrapherApp::new(file, analyse, saved)))
        }),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
