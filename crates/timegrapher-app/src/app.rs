//! The main window: input and settings on the left, readings and the strip
//! on the right.

use crate::strip::{self, Anchor, StripView};
use eframe::egui::{self, Color32, RichText, Vec2};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::Instant;
use timegrapher_core::beats::STANDARD_BPH;
use timegrapher_core::capture::{self, Capture, Event, InputDevice, Level};
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading};
use timegrapher_core::recorder::{Recorder, SessionInfo};
use timegrapher_core::stream::{self, BeatLog, StreamConfig};

/// The six standard test positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    DialUp,
    DialDown,
    CrownUp,
    CrownDown,
    CrownLeft,
    CrownRight,
}

impl Position {
    pub const ALL: [Position; 6] = [
        Position::DialUp,
        Position::DialDown,
        Position::CrownUp,
        Position::CrownDown,
        Position::CrownLeft,
        Position::CrownRight,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Position::DialUp => "DU",
            Position::DialDown => "DD",
            Position::CrownUp => "CU",
            Position::CrownDown => "CD",
            Position::CrownLeft => "CL",
            Position::CrownRight => "CR",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Position::DialUp => "Dial up",
            Position::DialDown => "Dial down",
            Position::CrownUp => "Crown up",
            Position::CrownDown => "Crown down",
            Position::CrownLeft => "Crown left",
            Position::CrownRight => "Crown right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Input {
    Microphone,
    File,
}

/// Views of the session. The tick-shape and long-run views will join these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Timegrapher,
    Trend,
}

/// Averaging times offered for the readings, seconds (Witschi's choices).
const AVERAGES: [f64; 6] = [2.0, 4.0, 10.0, 20.0, 30.0, 60.0];
/// Strip widths offered, ms either side of the centre.
const WIDTHS: [f64; 7] = [1.0, 2.5, 5.0, 10.0, 20.0, 50.0, 62.5];
/// Strip lengths offered, seconds.
const SPANS: [f64; 6] = [10.0, 30.0, 60.0, 300.0, 1800.0, 7200.0];

/// One point of the trend charts.
#[derive(Clone, Copy)]
struct TrendPoint {
    t: f64,
    rate: Option<f64>,
    amplitude: Option<f64>,
}

struct Batch {
    rx: std::sync::mpsc::Receiver<Result<Option<BeatLog>, String>>,
    progress: std::sync::Arc<std::sync::atomic::AtomicU64>,
    duration_s: Option<f64>,
    label: String,
}

pub struct TimegrapherApp {
    input: Input,
    devices: Vec<InputDevice>,
    device: Option<String>,
    file_path: String,
    bph: Option<u32>,
    lift_deg: f64,
    position: Position,
    average_s: f64,
    watch: String,
    save: bool,
    save_dir: String,
    view: View,
    strip: StripView,
    /// `None` follows the newest beat; otherwise the time at the top of the strip.
    view_end: Option<f64>,

    capture: Option<Capture>,
    live: Option<LiveAnalyzer>,
    source_label: String,
    recorder: Option<Recorder>,
    batch: Option<Batch>,
    anchor: Option<Anchor>,
    trend: Vec<TrendPoint>,
    level: Level,
    level_at: Instant,
    clipped_recently: bool,
    message: Option<(String, bool)>,
}

impl TimegrapherApp {
    pub fn new(file: Option<PathBuf>, analyse: bool) -> Self {
        let devices = capture::list().unwrap_or_default();
        let device = devices.first().map(|d| d.id.clone());
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut app = TimegrapherApp {
            input: if file.is_some() {
                Input::File
            } else {
                Input::Microphone
            },
            devices,
            device,
            file_path: file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            bph: None,
            lift_deg: 52.0,
            position: Position::DialUp,
            average_s: 10.0,
            watch: String::new(),
            save: false,
            save_dir: home.join("Timegrapher recordings").display().to_string(),
            view: View::Timegrapher,
            strip: StripView {
                half_width_ms: 10.0,
                span_s: 30.0,
            },
            view_end: None,
            capture: None,
            live: None,
            source_label: String::new(),
            recorder: None,
            batch: None,
            anchor: None,
            trend: Vec::new(),
            level: Level::default(),
            level_at: Instant::now(),
            clipped_recently: false,
            message: None,
        };
        if let Some(f) = file {
            if analyse {
                app.start_batch(&f);
            } else {
                app.start_replay(&f);
            }
        }
        app
    }

    fn live_config(&self) -> LiveConfig {
        let mut c = LiveConfig::default();
        c.analysis.bph = self.bph;
        c.analysis.amplitude.lift_deg = self.lift_deg;
        c
    }

    fn info(&self, device: &str, sample_rate: u32, bits: u16) -> SessionInfo {
        SessionInfo {
            watch: self.watch.clone(),
            position: self.position.code().into(),
            lift_deg: self.lift_deg,
            bph: self.bph,
            device: device.into(),
            sample_rate,
            bits,
            software: format!("timegrapher-app {}", env!("CARGO_PKG_VERSION")),
        }
    }

    fn running(&self) -> bool {
        self.capture.is_some()
    }

    fn reset_session(&mut self, sample_rate: u32, label: String) {
        self.live = Some(LiveAnalyzer::new(sample_rate, self.live_config()));
        self.source_label = label;
        self.anchor = None;
        self.trend.clear();
        self.view_end = None;
        self.level = Level::default();
        self.clipped_recently = false;
        self.message = None;
    }

    fn start_microphone(&mut self) {
        self.stop();
        match capture::open_input(self.device.as_deref(), None) {
            Ok(c) => {
                self.reset_session(c.sample_rate, c.label.clone());
                if self.save {
                    let info = self.info(&c.label, c.sample_rate, c.bits);
                    match Recorder::start(Path::new(&self.save_dir), info) {
                        Ok(r) => self.recorder = Some(r),
                        Err(e) => self.error(format!("Can't save the recording: {e}")),
                    }
                }
                self.capture = Some(c);
            }
            Err(e) => self.error(format!("Can't open the input: {e}")),
        }
    }

    fn start_replay(&mut self, path: &Path) {
        self.stop();
        match capture::replay_file(path) {
            Ok(c) => {
                self.reset_session(c.sample_rate, c.label.clone());
                self.capture = Some(c);
            }
            Err(e) => self.error(format!("Can't read {}: {e}", path.display())),
        }
    }

    fn start_batch(&mut self, path: &Path) {
        self.stop();
        let info = match timegrapher_core::audio::info(path) {
            Ok(i) => i,
            Err(e) => return self.error(format!("Can't read {}: {e}", path.display())),
        };
        let mut cfg = StreamConfig::default();
        cfg.analysis.bph = self.bph;
        cfg.analysis.amplitude.lift_deg = self.lift_deg;
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let p2 = progress.clone();
        let p = path.to_path_buf();
        std::thread::spawn(move || {
            let r = stream::analyze_file(&p, &cfg, |s| {
                p2.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
            });
            let _ = tx.send(r.map(Some).map_err(|e| e.to_string()));
        });
        self.reset_session(info.sample_rate, capture_label(path));
        self.live = None;
        self.batch = Some(Batch {
            rx,
            progress,
            duration_s: info.frames.map(|f| f as f64 / info.sample_rate as f64),
            label: capture_label(path),
        });
    }

    fn stop(&mut self) {
        self.capture = None;
        self.batch = None;
        if let Some(r) = self.recorder.take() {
            let result = self
                .live
                .as_ref()
                .and_then(|l| serde_json::to_value(l.reading(self.average_s)).ok());
            match r.finish(result) {
                Ok(dir) => self.info_msg(format!("Recording saved in {}", dir.display())),
                Err(e) => self.error(format!("Saving the recording failed: {e}")),
            }
        }
    }

    fn error(&mut self, m: String) {
        self.message = Some((m, true));
    }

    fn info_msg(&mut self, m: String) {
        self.message = Some((m, false));
    }

    /// Take in whatever audio has arrived.
    fn poll(&mut self) {
        let mut ended = false;
        let mut failure = None;
        if let (Some(cap), Some(live)) = (self.capture.as_ref(), self.live.as_mut()) {
            loop {
                match cap.rx.try_recv() {
                    Ok(Event::Audio(b)) => {
                        let l = Level::of(&b.samples);
                        if self.level_at.elapsed().as_secs_f64() > 0.5
                            || l.peak_dbfs > self.level.peak_dbfs
                        {
                            self.level = l;
                            self.level_at = Instant::now();
                        }
                        if l.clipped_fraction > 0.0 {
                            self.clipped_recently = true;
                        }
                        if let Some(r) = self.recorder.as_mut() {
                            if let Err(e) = r.write(&b) {
                                failure = Some(format!("Saving stopped: {e}"));
                            }
                        }
                        if live.push(&b.samples) {
                            let r = live.reading(self.average_s);
                            self.trend.push(TrendPoint {
                                t: r.time_s,
                                rate: r.rate_s_per_day,
                                amplitude: r.amplitude_deg,
                            });
                        }
                    }
                    Ok(Event::End) => {
                        ended = true;
                        break;
                    }
                    Ok(Event::Error(e)) => {
                        failure = Some(format!("Input: {e}"));
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        ended = true;
                        break;
                    }
                }
            }
        }
        if let Some(f) = failure {
            if f.starts_with("Saving") {
                self.recorder = None;
            }
            self.error(f);
        }
        if ended {
            self.capture = None;
            self.info_msg("End of the recording.".into());
        }

        let mut done = None;
        if let Some(b) = &self.batch {
            match b.rx.try_recv() {
                Ok(r) => done = Some(r),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => done = Some(Ok(None)),
            }
        }
        if let Some(r) = done {
            let label = self.batch.take().map(|b| b.label).unwrap_or_default();
            match r {
                Ok(Some(log)) => self.show_log(log, label),
                Ok(None) => {}
                Err(e) => self.error(format!("Analysis failed: {e}")),
            }
        }

        // Keep the strip anchored: centre on the first beats, then hold.
        if self.anchor.is_none() {
            if let Some(live) = &self.live {
                if let Some(bph) = live.bph() {
                    let beats = live.beats();
                    if beats.len() >= 16 {
                        self.anchor = Anchor::centre_on(&beats[..16], 3600.0 / bph as f64, 16);
                    }
                }
            }
        }
    }

    fn show_log(&mut self, log: BeatLog, label: String) {
        let live = LiveAnalyzer::from_log(log, self.live_config());
        let end = live.duration_s();
        let step = (self.average_s / 2.0).max(1.0);
        let mut t = self.average_s.min(end);
        self.trend.clear();
        while t <= end {
            let r = live.reading_at(t, self.average_s);
            self.trend.push(TrendPoint {
                t,
                rate: r.rate_s_per_day,
                amplitude: r.amplitude_deg,
            });
            t += step;
        }
        self.source_label = label;
        self.live = Some(live);
        self.anchor = None;
        self.view_end = None;
        self.info_msg("Analysed the whole recording. Drag the slider to look back.".into());
    }

    fn end_s(&self) -> f64 {
        let latest = self
            .live
            .as_ref()
            .map_or(0.0, |l| l.beats().last().map_or(l.duration_s(), |b| b.time));
        self.view_end.unwrap_or(latest).min(latest)
    }

    fn reading(&self) -> Option<LiveReading> {
        let live = self.live.as_ref()?;
        Some(if self.view_end.is_some() || !self.running() {
            live.reading_at(self.end_s(), self.average_s)
        } else {
            live.reading(self.average_s)
        })
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Input");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.input, Input::Microphone, "Microphone");
            ui.selectable_value(&mut self.input, Input::File, "Recording");
        });
        match self.input {
            Input::Microphone => {
                ui.horizontal(|ui| {
                    let sel = self
                        .devices
                        .iter()
                        .find(|d| Some(&d.id) == self.device.as_ref())
                        .map_or_else(|| "(none found)".to_string(), |d| d.name.clone());
                    egui::ComboBox::from_id_salt("device")
                        .selected_text(short(&sel, 28))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for d in &self.devices {
                                let label = if d.is_default {
                                    format!("{} (default)", d.name)
                                } else {
                                    d.name.clone()
                                };
                                ui.selectable_value(&mut self.device, Some(d.id.clone()), label);
                            }
                        });
                    if ui
                        .button("Rescan")
                        .on_hover_text("Look for devices again")
                        .clicked()
                    {
                        self.devices = capture::list().unwrap_or_default();
                        if self.device.is_none() {
                            self.device = self.devices.first().map(|d| d.id.clone());
                        }
                    }
                });
                ui.add_enabled_ui(!self.running(), |ui| {
                    ui.checkbox(&mut self.save, "Save the recording");
                    if self.save {
                        ui.label("Folder");
                        ui.text_edit_singleline(&mut self.save_dir);
                    }
                });
                ui.horizontal(|ui| {
                    if self.running() {
                        if ui.button("Stop").clicked() {
                            self.stop();
                        }
                    } else if ui.button("Start").clicked() {
                        self.start_microphone();
                    }
                });
            }
            Input::File => {
                ui.label("WAV or FLAC file (or drop one on the window)");
                ui.text_edit_singleline(&mut self.file_path);
                ui.horizontal(|ui| {
                    let path = PathBuf::from(self.file_path.trim());
                    let ok = !self.file_path.trim().is_empty();
                    if self.running() {
                        if ui.button("Stop").clicked() {
                            self.stop();
                        }
                    } else if ui
                        .add_enabled(ok, egui::Button::new("Replay"))
                        .on_hover_text("Play it through at its own speed, as if live")
                        .clicked()
                    {
                        self.start_replay(&path);
                    }
                    if ui
                        .add_enabled(ok && self.batch.is_none(), egui::Button::new("Analyse all"))
                        .on_hover_text("Analyse the whole file now and look through it")
                        .clicked()
                    {
                        self.start_batch(&path);
                    }
                });
            }
        }

        ui.separator();
        ui.heading("Watch");
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut self.watch);
        });
        egui::Grid::new("settings").num_columns(2).show(ui, |ui| {
            ui.label("Beat rate");
            let mut bph = self.bph;
            egui::ComboBox::from_id_salt("bph")
                .selected_text(match bph {
                    None => match self.live.as_ref().and_then(|l| l.bph()) {
                        Some(b) => format!("Auto ({b})"),
                        None => "Auto".into(),
                    },
                    Some(b) => format!("{b} bph"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut bph, None, "Auto");
                    for b in STANDARD_BPH {
                        ui.selectable_value(&mut bph, Some(b), format!("{b} bph"));
                    }
                });
            ui.end_row();
            if bph != self.bph {
                self.bph = bph;
                if let Some(l) = self.live.as_mut() {
                    l.set_bph(bph);
                }
                self.anchor = None;
                self.trend.clear();
            }

            ui.label("Lift angle");
            if ui
                .add(
                    egui::DragValue::new(&mut self.lift_deg)
                        .range(20.0..=80.0)
                        .speed(0.2)
                        .fixed_decimals(1)
                        .suffix("°"),
                )
                .changed()
            {
                if let Some(l) = self.live.as_mut() {
                    let k = self.lift_deg / l.config().analysis.amplitude.lift_deg;
                    l.set_lift(self.lift_deg);
                    for p in &mut self.trend {
                        p.amplitude = p.amplitude.map(|a| a * k);
                    }
                }
                self.note_settings();
            }
            ui.end_row();

            ui.label("Position");
            let mut pos = self.position;
            egui::ComboBox::from_id_salt("position")
                .selected_text(format!("{} ({})", pos.name(), pos.code()))
                .show_ui(ui, |ui| {
                    for p in Position::ALL {
                        ui.selectable_value(&mut pos, p, format!("{} ({})", p.name(), p.code()));
                    }
                });
            ui.end_row();
            if pos != self.position {
                self.position = pos;
                // A new position is a new measurement: start the readings again.
                if let Some(l) = self.live.as_mut() {
                    if self.capture.is_some() {
                        l.restart();
                        self.anchor = None;
                    }
                }
                if let Some(r) = self.recorder.as_mut() {
                    let _ = r.note(&format!("position {}", pos.code()));
                }
                self.note_settings();
            }

            ui.label("Average over");
            egui::ComboBox::from_id_salt("average")
                .selected_text(format!("{} s", self.average_s))
                .show_ui(ui, |ui| {
                    for a in AVERAGES {
                        ui.selectable_value(&mut self.average_s, a, format!("{a} s"));
                    }
                });
            ui.end_row();
        });

        ui.separator();
        ui.heading("Strip");
        egui::Grid::new("strip").num_columns(2).show(ui, |ui| {
            ui.label("Width");
            egui::ComboBox::from_id_salt("width")
                .selected_text(format!("±{} ms", self.strip.half_width_ms))
                .show_ui(ui, |ui| {
                    for w in WIDTHS {
                        ui.selectable_value(&mut self.strip.half_width_ms, w, format!("±{w} ms"));
                    }
                });
            ui.end_row();
            ui.label("Length");
            egui::ComboBox::from_id_salt("span")
                .selected_text(strip::fmt_time(self.strip.span_s))
                .show_ui(ui, |ui| {
                    for s in SPANS {
                        ui.selectable_value(&mut self.strip.span_s, s, strip::fmt_time(s));
                    }
                });
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui
                .button("Centre")
                .on_hover_text("Put the latest beats on the centre line")
                .clicked()
            {
                if let Some(l) = &self.live {
                    if let Some(bph) = l.bph() {
                        let hi = l.beats().partition_point(|b| b.time <= self.end_s());
                        self.anchor = Anchor::centre_on(&l.beats()[..hi], 3600.0 / bph as f64, 16);
                    }
                }
            }
            if ui
                .add_enabled(self.capture.is_some(), egui::Button::new("Clear"))
                .on_hover_text("Start the readings and the strip again")
                .clicked()
            {
                if let Some(l) = self.live.as_mut() {
                    l.restart();
                }
                self.anchor = None;
                self.trend.clear();
            }
        });
    }

    fn note_settings(&mut self) {
        if let Some(r) = self.recorder.as_mut() {
            let info = SessionInfo {
                watch: self.watch.clone(),
                position: self.position.code().into(),
                lift_deg: self.lift_deg,
                bph: self.bph,
                device: String::new(),
                sample_rate: 0,
                bits: 0,
                software: String::new(),
            };
            let _ = r.set_info(info);
        }
    }

    fn readouts(&self, ui: &mut egui::Ui) {
        let r = self.reading();
        let big = |v: String| RichText::new(v).size(44.0).monospace().strong();
        let dash = "—".to_string();
        ui.columns(3, |cols| {
            let rate = r.and_then(|r| r.rate_s_per_day);
            cols[0].label(RichText::new("Rate").size(14.0));
            cols[0].label(big(rate.map_or(dash.clone(), |v| format!("{v:+.1}"))));
            cols[0].label("s/d");
            let amp = r.and_then(|r| r.amplitude_deg);
            cols[1].label(RichText::new("Amplitude").size(14.0));
            cols[1].label(big(amp.map_or(dash.clone(), |v| format!("{v:.0}°"))));
            cols[1].label(match r {
                Some(LiveReading {
                    amplitude_even_deg: Some(a),
                    amplitude_odd_deg: Some(b),
                    ..
                }) => format!("A {a:.0}°  B {b:.0}°   lift {:.1}°", self.lift_deg),
                _ => format!("lift {:.1}°", self.lift_deg),
            });
            let be = r.and_then(|r| r.beat_error_ms);
            cols[2].label(RichText::new("Beat error").size(14.0));
            cols[2].label(big(be.map_or(dash.clone(), |v| format!("{v:+.2}"))));
            cols[2].label("ms");
        });
        ui.horizontal(|ui| {
            let mut parts = Vec::new();
            if let Some(b) = r.and_then(|r| r.bph) {
                parts.push(format!("{b} bph"));
            }
            if let Some(j) = r.and_then(|r| r.jitter_us) {
                parts.push(format!("jitter {j:.0} µs"));
            }
            if let Some(r) = r {
                parts.push(format!("{} beats in {} s", r.beats_used, self.average_s));
            }
            parts.push(format!(
                "{} {}",
                self.position.code(),
                self.position.name().to_lowercase()
            ));
            ui.label(RichText::new(parts.join("   ·   ")).weak());
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(b) = &self.batch {
                let done = f64::from_bits(b.progress.load(std::sync::atomic::Ordering::Relaxed));
                let frac = b.duration_s.map_or(0.0, |d| (done / d).clamp(0.0, 1.0));
                ui.add(
                    egui::ProgressBar::new(frac as f32)
                        .desired_width(240.0)
                        .text(format!("Analysing {}", b.label)),
                );
            } else if !self.source_label.is_empty() {
                let what = if self.running() { "Listening to" } else { "Showing" };
                ui.label(format!("{what} {}", self.source_label));
            }
            if self.running() {
                let warn = self.clipped_recently || self.level.peak_dbfs > -1.0;
                let low = self.level.peak_dbfs < -40.0;
                let txt = format!("peak {:.0} dBFS", self.level.peak_dbfs.max(-99.0));
                let color = if warn {
                    Color32::from_rgb(0xe0, 0x40, 0x40)
                } else if low {
                    Color32::from_rgb(0xd0, 0x90, 0x20)
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.label(RichText::new(txt).color(color)).on_hover_text(
                    "Loudest sample in the last half second. Aim for ticks peaking around -12 dBFS; \
                     red means the input is clipping, so turn the gain down.",
                );
                if self.clipped_recently {
                    ui.label(RichText::new("clipping").color(Color32::from_rgb(0xe0, 0x40, 0x40)));
                }
                if let Some(s) = self.reading().and_then(|r| r.snr) {
                    ui.label(
                        RichText::new(if s < 4.0 {
                            "no watch heard".to_string()
                        } else {
                            format!("signal {:.0}×", s)
                        })
                        .weak(),
                    )
                    .on_hover_text("How far the beats stand above the background noise");
                }
            }
            if let Some(r) = &self.recorder {
                ui.label(
                    RichText::new(format!("Saving {}", strip::fmt_time(r.duration_s())))
                        .color(Color32::from_rgb(0xe0, 0x40, 0x40)),
                )
                .on_hover_text(r.dir().display().to_string());
            }
            if let Some((m, err)) = &self.message {
                let c = if *err {
                    Color32::from_rgb(0xe0, 0x40, 0x40)
                } else {
                    ui.visuals().text_color()
                };
                ui.label(RichText::new(m).color(c));
            }
        });
    }

    fn main_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.view, View::Timegrapher, "Timegrapher");
            ui.selectable_value(&mut self.view, View::Trend, "Rate and amplitude over time");
        });
        ui.separator();
        self.readouts(ui);
        ui.add_space(6.0);

        // Looking back through a finished or analysed recording.
        let total = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        if !self.running() && total > self.strip.span_s.min(self.average_s) {
            let mut end = self.view_end.unwrap_or(total);
            ui.horizontal(|ui| {
                ui.label("Time");
                ui.spacing_mut().slider_width = (ui.available_width() - 120.0).max(100.0);
                if ui
                    .add(
                        egui::Slider::new(&mut end, 0.0..=total)
                            .custom_formatter(|v, _| strip::fmt_time(v)),
                    )
                    .changed()
                {
                    self.view_end = Some(end);
                }
            });
        }

        let Some(live) = &self.live else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(
                        "Choose an input and press Start, or open a recording.\n\
                         Put the watch on the microphone dial up to begin.",
                    )
                    .size(16.0)
                    .weak(),
                );
            });
            return;
        };
        let period = live.bph().map_or(0.125, |b| 3600.0 / b as f64);
        let end = self.end_s();
        match self.view {
            View::Timegrapher => {
                let size = ui.available_size();
                let (strip_w, side_w) = if size.x > 900.0 {
                    (size.x * 0.62, size.x * 0.38 - 8.0)
                } else {
                    (size.x, 0.0)
                };
                ui.horizontal_top(|ui| {
                    strip::draw_strip(
                        ui,
                        live.beats(),
                        period,
                        self.anchor,
                        end,
                        &self.strip,
                        Vec2::new(strip_w, size.y),
                    );
                    if side_w > 0.0 {
                        ui.vertical(|ui| self.trend_charts(ui, Vec2::new(side_w, size.y)));
                    }
                });
            }
            View::Trend => {
                let size = ui.available_size();
                self.trend_charts(ui, size);
            }
        }
    }

    fn trend_charts(&self, ui: &mut egui::Ui, size: Vec2) {
        let h = (size.y - 8.0) / 2.0;
        let (t0, t1) = match (self.trend.first(), self.trend.last()) {
            (Some(a), Some(b)) => (a.t, b.t),
            _ => (0.0, 1.0),
        };
        let colors = strip::side_colors(ui.visuals().dark_mode);
        let rate: Vec<(f64, f64)> = self
            .trend
            .iter()
            .filter_map(|p| p.rate.map(|r| (p.t, r)))
            .collect();
        let amp: Vec<(f64, f64)> = self
            .trend
            .iter()
            .filter_map(|p| p.amplitude.map(|a| (p.t, a)))
            .collect();
        strip::draw_series(
            ui,
            &format!("Rate, s/d ({} s average)", self.average_s),
            "",
            &rate,
            (t0, t1),
            colors[0],
            Vec2::new(size.x, h),
        );
        ui.add_space(8.0);
        strip::draw_series(
            ui,
            "Amplitude, degrees",
            "°",
            &amp,
            (t0, t1),
            colors[1],
            Vec2::new(size.x, h),
        );
    }
}

fn capture_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

impl eframe::App for TimegrapherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // A file dropped on the window is replayed.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(p) = dropped.into_iter().next() {
            self.input = Input::File;
            self.file_path = p.display().to_string();
            self.start_replay(&p);
        }

        self.poll();

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.status_bar(ui));
        egui::SidePanel::left("controls")
            .resizable(false)
            .exact_width(270.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });
        egui::CentralPanel::default().show(ctx, |ui| self.main_view(ui));

        if self.running() || self.batch.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::synth::{generate, SynthConfig};

    /// Lay out and paint frames without a window, with a synthetic watch
    /// replayed through the live engine, and check the readings arrive.
    #[test]
    fn draws_a_synthetic_watch_headless() {
        let dir = std::env::temp_dir().join(format!("tg-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("syn.wav");
        let cfg = SynthConfig {
            duration_s: 12.0,
            rate_s_per_day: 20.0,
            ..Default::default()
        };
        timegrapher_core::audio::write_wav(&wav, &generate(&cfg, |_| 280.0, |_| 0.0)).unwrap();

        let mut app = TimegrapherApp::new(None, false);
        app.input = Input::File;
        app.file_path = wav.display().to_string();
        // Feed the whole file straight into the engine instead of in real time.
        app.reset_session(48000, "syn.wav".into());
        let audio = timegrapher_core::load(&wav).unwrap();
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            live.push(b);
        }
        app.poll();
        assert!(app.anchor.is_some(), "strip not anchored");
        let r = app.reading().unwrap();
        let rate = r.rate_s_per_day.unwrap();
        assert!((rate - 20.0).abs() < 2.0, "rate {rate}");

        let ctx = egui::Context::default();
        for view in [View::Timegrapher, View::Trend] {
            app.view = view;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1280.0, 820.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::TopBottomPanel::bottom("status").show(ctx, |ui| app.status_bar(ui));
                egui::SidePanel::left("controls").show(ctx, |ui| app.controls(ui));
                egui::CentralPanel::default().show(ctx, |ui| app.main_view(ui));
            });
            assert!(!out.shapes.is_empty());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
