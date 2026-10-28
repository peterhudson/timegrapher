// No console window behind the app on Windows; messages go to the app log.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! Desktop timegrapher: the live trace with rate, amplitude and beat error
//! from a microphone or a recording.
//!
//! All analysis is in `timegrapher-core`; this crate draws it. Two modes
//! without a window, for scripts and agents:
//!
//! - `timegrapher-app --devices` lists the sound input devices as JSON.
//! - `timegrapher-app --headless FILE` runs a recording through the live
//!   engine as fast as it can and prints the readings as JSON lines.
//!
//! On Windows the app opens without a console window, so these print only
//! when their output is redirected or piped; the `timegrapher` CLI is the
//! command-line tool there.

mod app;
mod applog;
mod awake;
mod calibre_ui;
mod desktop;
mod export;
mod fields;
mod guided;
mod help;
mod icons;
mod isochronism;
mod marks;
mod photos;
mod profiles;
mod pump;
mod report;
mod reserve;
mod scope;
mod session;
mod settings;
mod settings_window;
mod span;
mod steady;
mod strip;
mod tfmap;
mod theme;
mod update;
mod watches;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use timegrapher_core::audio;
use timegrapher_core::live::{LiveAnalyzer, LiveConfig};

const USAGE: &str = "\
Usage:
  timegrapher-app [FILE]            open the window with FILE analysed all at once
                                    (a recording, or a folder of segments as one)
  timegrapher-app --devices         list sound input devices as JSON
  timegrapher-app --headless FILE [--bph N] [--lift DEG] [--average S] [--every S]
                                    print live readings for FILE as JSON lines";

struct Args {
    file: Option<PathBuf>,
    headless: bool,
    devices: bool,
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
            // Analysing all at once is all the app does; the old flag still works.
            "--analyse" | "--analyze" => {}
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
    // The log and crash records live beside the settings.
    // The settings folder `timegrapher report` collects logs from.
    applog::init(timegrapher_core::clockstore::config_dir());
    let file = args.file.clone();
    // OpenGL first; where the graphics can't do OpenGL 2.0 (a virtual
    // machine or remote desktop without 3D, an old driver), wgpu, which on
    // Windows can draw with Direct3D 12's software renderer.
    let gl = match open_window(eframe::Renderer::Glow, file.clone()) {
        Closed::Quit => return ExitCode::SUCCESS,
        Closed::Broke(e) => return window_broke(&e),
        Closed::NeverDrew(e) => e,
    };
    applog::warn(&format!(
        "OpenGL didn't open the window ({gl}); trying wgpu"
    ));
    match open_window(eframe::Renderer::Wgpu, file) {
        Closed::Quit => ExitCode::SUCCESS,
        Closed::Broke(e) => window_broke(&e),
        Closed::NeverDrew(wgpu) => {
            window_failed(&format!("With OpenGL: {gl}\nWith wgpu: {wgpu}"));
            ExitCode::FAILURE
        }
    }
}

/// Set once the app has built a frame, so a window that closes before
/// showing anything counts as a failure, not a quit.
static FRAME_BUILT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called each frame by the app.
pub fn frame_built() {
    if !FRAME_BUILT.swap(true, std::sync::atomic::Ordering::Relaxed) {
        applog::info("The window drew its first frame");
    }
}

/// How the window's run ended.
enum Closed {
    /// Quit after drawing.
    Quit,
    /// Couldn't open, or closed before drawing anything: worth another
    /// renderer.
    NeverDrew(String),
    /// Drew, then ended with an error.
    Broke(String),
}

/// Run the window with `renderer` until it closes.
fn open_window(renderer: eframe::Renderer, file: Option<PathBuf>) -> Closed {
    let options = eframe::NativeOptions {
        renderer,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Timegrapher")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(icon())
            // The name a Wayland desktop finds the app's menu entry and
            // icon by (see `desktop`).
            .with_app_id("timegrapher"),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Timegrapher",
        options,
        Box::new(move |cc| {
            applog::info(&format!("Drawing with {}", graphics(cc)));
            theme::install(&cc.egui_ctx);
            let saved = cc.storage.and_then(settings::load);
            if let Some(s) = &saved {
                cc.egui_ctx.set_theme(s.theme);
            }
            Ok(Box::new(pump::Shell::new(app::TimegrapherApp::new(
                file, saved,
            ))))
        }),
    );
    let built = FRAME_BUILT.load(std::sync::atomic::Ordering::Relaxed);
    match result {
        Err(e) if !built => Closed::NeverDrew(e.to_string()),
        Err(e) => Closed::Broke(e.to_string()),
        Ok(()) if !built => Closed::NeverDrew("the window closed before drawing anything".into()),
        Ok(()) => {
            applog::info("The window closed");
            Closed::Quit
        }
    }
}

/// What the window draws with, for the log: the OpenGL version and
/// renderer, or the wgpu adapter.
fn graphics(cc: &eframe::CreationContext) -> String {
    if let Some(gl) = &cc.gl {
        use eframe::glow::HasContext;
        // SAFETY: the context is current while the app is created.
        let (version, renderer) = unsafe {
            (
                gl.get_parameter_string(eframe::glow::VERSION),
                gl.get_parameter_string(eframe::glow::RENDERER),
            )
        };
        return format!("OpenGL {version} on {renderer}");
    }
    if let Some(w) = &cc.wgpu_render_state {
        let i = w.adapter.get_info();
        return format!("wgpu {:?} on {} ({:?})", i.backend, i.name, i.device_type);
    }
    "an unknown renderer".into()
}

/// The window couldn't open with either renderer.
fn window_failed(reason: &str) {
    applog::error(&format!("The window couldn't open: {reason}"));
    tell(
        reason,
        &format!(
            "Timegrapher couldn't open its window.\n\n{reason}\n\nThe window needs the \
             computer's graphics to draw with OpenGL 2.0 or newer, or with Direct3D 12, \
             Metal or Vulkan. A virtual machine or remote desktop without 3D graphics, or \
             a missing graphics driver, may offer neither. The command line tool, \
             timegrapher, works without a window."
        ),
    );
}

/// The window drew, then ended with an error.
fn window_broke(reason: &str) -> ExitCode {
    applog::error(&format!("The window closed with an error: {reason}"));
    tell(
        reason,
        &format!("Timegrapher's window closed with an error.\n\n{reason}"),
    );
    ExitCode::FAILURE
}

/// Say why the app is ending. A Windows app has nowhere to print, so the
/// reason is also in the log and, on Windows and macOS, a message box.
fn tell(reason: &str, text: &str) {
    eprintln!("{reason}");
    let log = applog::dir()
        .map(|d| d.join("logs").join("timegrapher.log"))
        .map(|p| format!("\n\nThis is also written in the log, {}.", p.display()))
        .unwrap_or_default();
    let text = format!("{text}{log}");
    #[cfg(any(windows, target_os = "macos"))]
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Timegrapher")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = text;
}

/// The window's icon, the same artwork the release packages use.
fn icon() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icon.png"))
        .unwrap_or_default()
}
