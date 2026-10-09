//! The main window: input and settings on the left, the readings across
//! the top, and below them panes (the paper strip and the charts over time)
//! that can be dragged by their tabs into any arrangement.

use crate::fields::{self, Format};
use crate::strip::{self, Anchor, StripInput, StripView};
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{Legend, Line, Plot, VLine};
use egui_tiles::{Linear, LinearDir, SimplificationOptions, TileId, Tiles, Tree, UiResponse};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::Instant;
use timegrapher_core::beats::STANDARD_BPH;
use timegrapher_core::capture::{self, Capture, Event, InputDevice, Level};
use timegrapher_core::diagnose::{HOT_PEAK_DBFS, TARGET_PEAK_DBFS};
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading};
use timegrapher_core::mixer::{GainState, InputGain};
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

/// The panes below the readings. Tick shape and the periodicity (FFT)
/// views will join these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Strip,
    Rate,
    Amplitude,
    BeatError,
}

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Pane::Strip => "Paper strip",
            Pane::Rate => "Rate",
            Pane::Amplitude => "Amplitude",
            Pane::BeatError => "Beat error",
        }
    }
}

/// The starting arrangement: strip beside the three charts when it runs
/// down the window, above them when it lies on its side.
fn default_layout(horizontal: bool) -> Tree<Pane> {
    let mut tiles = Tiles::default();
    let strip = tiles.insert_pane(Pane::Strip);
    let charts: Vec<TileId> = [Pane::Rate, Pane::Amplitude, Pane::BeatError]
        .into_iter()
        .map(|p| tiles.insert_pane(p))
        .collect();
    let root = if horizontal {
        let row = tiles.insert_horizontal_tile(charts);
        tiles.insert_container(Linear::new_binary(LinearDir::Vertical, [strip, row], 0.55))
    } else {
        let col = tiles.insert_vertical_tile(charts);
        tiles.insert_container(Linear::new_binary(LinearDir::Horizontal, [strip, col], 0.6))
    };
    Tree::new("panes", root, tiles)
}

/// Averaging times offered for the readings, seconds (Witschi's choices).
const AVERAGES: [f64; 6] = [2.0, 4.0, 10.0, 20.0, 30.0, 60.0];
/// Strip widths offered, ms either side of the centre. 62.5 ms is half a
/// beat at 28,800 bph, the widest view in which tick and tock can't wrap
/// onto each other.
const WIDTHS: [f64; 7] = [1.0, 2.5, 5.0, 10.0, 20.0, 50.0, 62.5];
/// Strip lengths offered, seconds: from a few seconds of beats to a two-hour
/// run.
const SPANS: [f64; 7] = [10.0, 30.0, 60.0, 300.0, 1800.0, 3600.0, 7200.0];
/// Lift angles offered, degrees.
const LIFTS: [f64; 6] = [44.0, 49.0, 50.0, 52.0, 53.0, 55.0];
/// Most points kept on a chart when it is rebuilt from the whole session.
const TREND_POINTS: f64 = 4000.0;

const MS: Format = Format {
    show: fields::plain,
    parse: fields::parse_number,
};
const DURATION: Format = Format {
    show: fields::duration,
    parse: fields::parse_duration,
};

/// One point of the charts over time.
#[derive(Debug, Clone, Copy)]
struct TrendPoint {
    t: f64,
    rate: Option<f64>,
    amplitude: Option<f64>,
    amplitude_a: Option<f64>,
    amplitude_b: Option<f64>,
    beat_error_unlock: Option<f64>,
    beat_error_drop: Option<f64>,
}

impl TrendPoint {
    fn of(t: f64, r: &LiveReading) -> Self {
        TrendPoint {
            t,
            rate: r.rate_s_per_day,
            amplitude: r.amplitude_deg,
            amplitude_a: r.amplitude_even_deg,
            amplitude_b: r.amplitude_odd_deg,
            beat_error_unlock: r.beat_error_unlock_ms,
            beat_error_drop: r.beat_error_ms,
        }
    }
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
    strip: StripView,
    /// Keep the newest beats on the centre line, sliding the rest.
    follow: bool,
    /// `None` follows the newest beat; otherwise the time at the newest
    /// edge of the strip.
    view_end: Option<f64>,
    panes: Tree<Pane>,

    capture: Option<Capture>,
    live: Option<LiveAnalyzer>,
    source_label: String,
    recorder: Option<Recorder>,
    batch: Option<Batch>,
    anchor: Option<Anchor>,
    trend: Vec<TrendPoint>,
    level: Level,
    level_at: Instant,
    /// When the input last clipped.
    clipped_at: Option<Instant>,
    /// The selected input's level control, and its state when last read.
    gain: Option<InputGain>,
    gain_state: Option<GainState>,
    gain_read_at: Instant,
    /// The device the level control was read for.
    gain_device: Option<String>,
    gain_error: Option<String>,
    message: Option<(String, bool)>,
    /// Offer every input the system lists, not just one per microphone.
    all_inputs: bool,
    /// When the microphone was opened and when it last sent audio.
    opened_at: Instant,
    audio_at: Option<Instant>,
}

impl TimegrapherApp {
    pub fn new(file: Option<PathBuf>, analyse: bool) -> Self {
        Self::with_devices(capture::list().unwrap_or_default(), file, analyse)
    }

    /// The app with a given device list. Tests pass an empty one: listing
    /// devices from several test threads at once crashes on Windows.
    fn with_devices(devices: Vec<InputDevice>, file: Option<PathBuf>, analyse: bool) -> Self {
        let device = capture::choices(&devices).first().map(|c| c.id.clone());
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
            strip: StripView {
                half_width_ms: 10.0,
                span_s: 30.0,
                horizontal: false,
            },
            follow: false,
            view_end: None,
            panes: default_layout(false),
            capture: None,
            live: None,
            source_label: String::new(),
            recorder: None,
            batch: None,
            anchor: None,
            trend: Vec::new(),
            level: Level::default(),
            level_at: Instant::now(),
            clipped_at: None,
            gain: None,
            gain_state: None,
            gain_read_at: Instant::now(),
            gain_device: None,
            gain_error: None,
            message: None,
            all_inputs: false,
            opened_at: Instant::now(),
            audio_at: None,
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
        self.clipped_at = None;
        self.message = None;
    }

    fn start_microphone(&mut self) {
        self.stop();
        match capture::open_input(self.device.as_deref(), None) {
            Ok(c) => {
                let label = capture::choices(&self.devices)
                    .into_iter()
                    .find(|ch| Some(&ch.id) == self.device.as_ref())
                    .map_or_else(|| c.label.clone(), |ch| ch.label);
                self.reset_session(
                    c.sample_rate,
                    format!("{label} at {} Hz, {}-bit", c.sample_rate, c.bits),
                );
                self.opened_at = Instant::now();
                self.audio_at = None;
                if self.save {
                    let info = self.info(&c.label, c.sample_rate, c.bits);
                    match Recorder::start(Path::new(&self.save_dir), info) {
                        Ok(r) => self.recorder = Some(r),
                        Err(e) => self.error(format!("Can't save the recording: {e}")),
                    }
                }
                self.capture = Some(c);
            }
            Err(e) => self.error(format!(
                "Can't open the input. {}",
                capture::explain_error(&e)
            )),
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
                        self.audio_at = Some(Instant::now());
                        let l = Level::of(&b.samples);
                        if self.level_at.elapsed().as_secs_f64() > 0.5
                            || l.peak_dbfs > self.level.peak_dbfs
                        {
                            self.level = l;
                            self.level_at = Instant::now();
                        }
                        if l.clipped_fraction > 0.0 {
                            self.clipped_at = Some(Instant::now());
                        }
                        if let Some(r) = self.recorder.as_mut() {
                            if let Err(e) = r.write(&b) {
                                failure = Some(format!("Saving stopped: {e}"));
                            }
                        }
                        if live.push(&b.samples) {
                            let r = live.reading(self.average_s);
                            self.trend.push(TrendPoint::of(r.time_s, &r));
                        }
                    }
                    Ok(Event::End) => {
                        ended = true;
                        break;
                    }
                    Ok(Event::Error(e)) => {
                        failure = Some(format!("Input stopped. {}", capture::explain_error(&e)));
                        if !cap.is_file {
                            ended = true;
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        ended = true;
                        break;
                    }
                }
            }
        }
        // A microphone that opened but sends nothing.
        if let Some(cap) = &self.capture {
            let quiet = self.audio_at.unwrap_or(self.opened_at).elapsed();
            if !cap.is_file && quiet.as_secs_f64() > 3.0 && failure.is_none() {
                failure = Some(
                    "No sound is arriving from this input. Choose another one, or the \
                     system default."
                        .to_string(),
                );
                ended = true;
            }
        }
        let was_file = self.capture.as_ref().is_some_and(|c| c.is_file);
        if ended {
            self.stop();
            if was_file {
                self.info_msg("End of the recording.".into());
            }
        }
        if let Some(f) = failure {
            if f.starts_with("Saving") {
                self.recorder = None;
            }
            self.error(f);
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
        self.live = Some(LiveAnalyzer::from_log(log, self.live_config()));
        self.rebuild_trend();
        self.source_label = label;
        self.anchor = None;
        self.view_end = None;
        self.info_msg(
            "Analysed the whole recording. Drag the strip or the time slider to look back.".into(),
        );
    }

    /// Work the charts out again over the whole session, for a new
    /// averaging time or lift angle.
    fn rebuild_trend(&mut self) {
        self.trend.clear();
        let Some(live) = &self.live else { return };
        let end = live.duration_s();
        let step = (end / TREND_POINTS).max(0.5);
        let mut t = step;
        while t <= end + 1e-9 {
            self.trend
                .push(TrendPoint::of(t, &live.reading_at(t, self.average_s)));
            t += step;
        }
    }

    fn latest_s(&self) -> f64 {
        self.live
            .as_ref()
            .map_or(0.0, |l| l.beats().last().map_or(l.duration_s(), |b| b.time))
    }

    fn end_s(&self) -> f64 {
        let latest = self.latest_s();
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

    /// Put the beats just before the strip's newest edge on the centre line.
    fn centre(&mut self) {
        let end = self.end_s();
        if let Some(l) = &self.live {
            if let Some(bph) = l.bph() {
                let hi = l.beats().partition_point(|b| b.time <= end);
                if let Some(a) = Anchor::centre_on(&l.beats()[..hi], 3600.0 / bph as f64, 8) {
                    self.anchor = Some(a);
                }
            }
        }
    }

    fn open_file_dialog(&mut self) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new()
            .set_title("Open a recording")
            .add_filter("Recordings", &["wav", "WAV", "flac", "FLAC"])
            .add_filter("All files", &["*"]);
        if let Some(dir) = Path::new(self.file_path.trim()).parent() {
            if dir.is_dir() {
                d = d.set_directory(dir);
            }
        }
        let p = d.pick_file()?;
        self.file_path = p.display().to_string();
        Some(p)
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Input");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.input, Input::Microphone, "Microphone");
            ui.selectable_value(&mut self.input, Input::File, "Recording");
        });
        match self.input {
            Input::Microphone => {
                let choices: Vec<capture::InputChoice> = if self.all_inputs {
                    self.devices
                        .iter()
                        .map(|d| capture::InputChoice {
                            id: d.id.clone(),
                            label: format!("{} ({})", d.name, d.id),
                            detail: String::new(),
                            is_default: d.is_default,
                        })
                        .collect()
                } else {
                    capture::choices(&self.devices)
                };
                ui.horizontal(|ui| {
                    let sel = choices
                        .iter()
                        .find(|c| Some(&c.id) == self.device.as_ref())
                        .map_or_else(|| "(choose an input)".to_string(), |c| c.label.clone());
                    egui::ComboBox::from_id_salt("device")
                        .selected_text(short(&sel, 30))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for c in &choices {
                                let r = ui.selectable_value(
                                    &mut self.device,
                                    Some(c.id.clone()),
                                    &c.label,
                                );
                                if !c.detail.is_empty() {
                                    r.on_hover_text(&c.detail);
                                }
                            }
                        });
                    if ui
                        .button("Rescan")
                        .on_hover_text("Look for devices again")
                        .clicked()
                    {
                        self.devices = capture::list().unwrap_or_default();
                        if self.device.is_none() {
                            self.device = capture::choices(&self.devices)
                                .first()
                                .map(|c| c.id.clone());
                        }
                    }
                });
                ui.checkbox(&mut self.all_inputs, "Show every input")
                    .on_hover_text("List every device the system offers, with its id");
                if self.gain_device != self.device {
                    self.read_gain();
                }
                self.gain_controls(ui);
                ui.add_enabled_ui(!self.running(), |ui| {
                    ui.checkbox(&mut self.save, "Save the recording");
                    if self.save {
                        ui.horizontal(|ui| {
                            ui.label("Folder");
                            if ui.button("Choose...").clicked() {
                                let mut d =
                                    rfd::FileDialog::new().set_title("Folder for recordings");
                                if Path::new(&self.save_dir).is_dir() {
                                    d = d.set_directory(&self.save_dir);
                                }
                                if let Some(p) = d.pick_folder() {
                                    self.save_dir = p.display().to_string();
                                }
                            }
                        });
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
                ui.horizontal(|ui| {
                    ui.label("WAV or FLAC file");
                    if ui
                        .button("Open...")
                        .on_hover_text("Choose a recording")
                        .clicked()
                    {
                        self.open_file_dialog();
                    }
                });
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

            ui.label("Lift angle")
                .on_hover_text("Type the calibre's lift angle in degrees and press Enter");
            let mut lift = self.lift_deg;
            let changed = ui
                .horizontal(|ui| {
                    let a = fields::entry(ui, "lift", &mut lift, 10.0..=90.0, &MS, 50.0);
                    ui.label("°");
                    let b = fields::presets(ui, "lift-presets", &mut lift, &LIFTS, |v| {
                        format!("{}°", fields::plain(v))
                    });
                    a || b
                })
                .inner;
            ui.end_row();
            if changed {
                self.set_lift(lift);
            }

            ui.label("Position").on_hover_text(
                "Noted in a saved recording, and changing it starts the readings again. \
                 Guided runs through the positions will use it.",
            );
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

            ui.label("Average over")
                .on_hover_text("Each reading is fitted over this much of the latest beats");
            let mut avg = self.average_s;
            let changed = ui
                .horizontal(|ui| {
                    let a = fields::entry(ui, "average", &mut avg, 1.0..=3600.0, &DURATION, 50.0);
                    let b = fields::presets(
                        ui,
                        "average-presets",
                        &mut avg,
                        &AVERAGES,
                        fields::duration,
                    );
                    a || b
                })
                .inner;
            ui.end_row();
            if changed {
                self.average_s = avg;
                self.rebuild_trend();
            }
        });

        ui.separator();
        ui.heading("Strip");
        egui::Grid::new("strip").num_columns(2).show(ui, |ui| {
            ui.label("Width").on_hover_text(
                "Milliseconds either side of the centre line. Ctrl and the mouse wheel \
                 on the strip change it too.",
            );
            ui.horizontal(|ui| {
                ui.label("±");
                let mut w = self.strip.half_width_ms;
                let a = fields::entry(
                    ui,
                    "width",
                    &mut w,
                    StripView::MIN_HALF_WIDTH_MS..=StripView::MAX_HALF_WIDTH_MS,
                    &MS,
                    50.0,
                );
                ui.label("ms");
                let b = fields::presets(ui, "width-presets", &mut w, &WIDTHS, |v| {
                    format!("±{} ms", fields::plain(v))
                });
                if a || b {
                    self.strip.set_half_width(w);
                }
            });
            ui.end_row();
            ui.label("Length").on_hover_text(
                "Time shown along the strip, such as 30 s, 5 min or 2 h. \
                 The mouse wheel on the strip changes it too.",
            );
            ui.horizontal(|ui| {
                let mut s = self.strip.span_s;
                let a = fields::entry(
                    ui,
                    "span",
                    &mut s,
                    StripView::MIN_SPAN_S..=StripView::MAX_SPAN_S,
                    &DURATION,
                    66.0,
                );
                let b = fields::presets(ui, "span-presets", &mut s, &SPANS, fields::duration);
                if a || b {
                    self.strip.set_span(s);
                }
            });
            ui.end_row();
            ui.label("Direction");
            ui.horizontal(|ui| {
                let mut h = self.strip.horizontal;
                ui.selectable_value(&mut h, false, "Down");
                ui.selectable_value(&mut h, true, "Across");
                if h != self.strip.horizontal {
                    self.strip.horizontal = h;
                    self.panes = default_layout(h);
                }
            });
            ui.end_row();
            ui.label("Theme");
            ui.horizontal(|ui| {
                let mut t = ui.ctx().options(|o| o.theme_preference);
                ui.selectable_value(&mut t, egui::ThemePreference::System, "System");
                ui.selectable_value(&mut t, egui::ThemePreference::Light, "Light");
                ui.selectable_value(&mut t, egui::ThemePreference::Dark, "Dark");
                if t != ui.ctx().options(|o| o.theme_preference) {
                    ui.ctx().set_theme(t);
                }
            });
            ui.end_row();
        });
        ui.checkbox(&mut self.follow, "Auto-centre")
            .on_hover_text("Keep the newest beats on the centre line and slide the rest");
        ui.horizontal(|ui| {
            if ui
                .button("Centre")
                .on_hover_text("Put the latest beats on the centre line")
                .clicked()
            {
                self.centre();
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
            if ui
                .button("Reset panes")
                .on_hover_text("Put the strip and charts back where they started")
                .clicked()
            {
                self.panes = default_layout(self.strip.horizontal);
            }
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Strip: wheel for length, Ctrl+wheel for width, drag to move, \
                 double-click for the newest beats.\n\
                 Charts: wheel to zoom time, Ctrl+wheel for the scale, drag to pan, \
                 double-click to fit, click to show that moment on the strip.\n\
                 Drag a pane by its tab to rearrange.",
            )
            .small()
            .weak(),
        );
    }

    fn clipping(&self) -> bool {
        self.clipped_at
            .is_some_and(|t| t.elapsed().as_secs_f64() < 3.0)
    }

    /// The input level in words and the colour to show it in.
    fn level_text(&self, ui: &egui::Ui) -> (String, Color32) {
        let peak = self.level.peak_dbfs.max(-99.0);
        let red = Color32::from_rgb(0xe0, 0x40, 0x40);
        let amber = Color32::from_rgb(0xd0, 0x90, 0x20);
        if self.clipping() {
            (format!("peak {peak:.0} dBFS, clipping"), red)
        } else if peak > HOT_PEAK_DBFS {
            (format!("peak {peak:.0} dBFS, too hot"), amber)
        } else if peak < -40.0 {
            (format!("peak {peak:.0} dBFS, very quiet"), amber)
        } else {
            (
                format!("peak {peak:.0} dBFS"),
                ui.visuals().weak_text_color(),
            )
        }
    }

    /// Find the selected input's level control and read it.
    fn read_gain(&mut self) {
        self.gain_device = self.device.clone();
        self.gain = self.device.as_deref().and_then(InputGain::for_device);
        self.gain_state = self.gain.as_ref().and_then(|g| g.read());
        self.gain_read_at = Instant::now();
    }

    /// The input level control and meter, under the device menu.
    fn gain_controls(&mut self, ui: &mut egui::Ui) {
        if self.gain_read_at.elapsed().as_secs_f64() > if self.running() { 2.0 } else { 10.0 } {
            self.read_gain();
        }
        egui::Grid::new("gain").num_columns(2).show(ui, |ui| {
            ui.label("Input level").on_hover_text(
                "The microphone's gain. For the system default this is the sound server's \
                 input volume, which it puts back on the microphone each time it opens it; \
                 for a direct device it is the card's capture level. Aim for ticks peaking \
                 around -10 dBFS, and never clipping.",
            );
            match (self.gain.clone(), self.gain_state.clone()) {
                (Some(g), Some(st)) => {
                    ui.horizontal(|ui| {
                        let mut pct = st.level * 100.0;
                        let r = ui.add(
                            egui::Slider::new(&mut pct, 0.0..=100.0)
                                .show_value(false)
                                .step_by(1.0),
                        );
                        ui.label(&st.text);
                        // Set it once the drag ends (or on a click or key),
                        // not on every pixel.
                        if (r.changed() && !r.dragged()) || r.drag_stopped() {
                            self.gain_error = g.set_level(pct / 100.0).err();
                            self.read_gain();
                        } else if r.changed() {
                            if let Some(s) = self.gain_state.as_mut() {
                                s.level = pct / 100.0;
                            }
                        }
                    });
                    ui.end_row();
                    if let Some(on) = st.agc {
                        ui.label("Auto gain");
                        let mut agc = on;
                        if ui
                            .checkbox(&mut agc, if on { "on (turn it off)" } else { "off" })
                            .on_hover_text(
                                "The microphone's automatic gain changes the level as it \
                                 listens, which spoils amplitude and level readings. Keep it off.",
                            )
                            .changed()
                        {
                            self.gain_error = g.set_agc(agc).err();
                            self.read_gain();
                        }
                        ui.end_row();
                    }
                }
                _ => {
                    ui.label(
                        RichText::new(if cfg!(target_os = "linux") {
                            "not adjustable here"
                        } else {
                            "set it in the system's sound settings"
                        })
                        .weak(),
                    );
                    ui.end_row();
                }
            }
            ui.label("Peak");
            let peak = self.level.peak_dbfs.max(-60.0);
            let (txt, color) = self.level_text(ui);
            let frac = ((peak + 60.0) / 60.0).clamp(0.0, 1.0) as f32;
            let (rect, _) = ui.allocate_exact_size(Vec2::new(150.0, 12.0), egui::Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
            p.rect_stroke(
                rect,
                2.0,
                ui.visuals().widgets.noninteractive.bg_stroke,
                egui::StrokeKind::Inside,
            );
            if self.running() {
                let mut fill = rect;
                fill.set_width(rect.width() * frac);
                p.rect_filled(fill, 2.0, color.gamma_multiply(0.9));
            }
            // Marks at the target and at the hot limit.
            for (db, c) in [
                (TARGET_PEAK_DBFS, ui.visuals().text_color()),
                (HOT_PEAK_DBFS, Color32::from_rgb(0xe0, 0x40, 0x40)),
            ] {
                let x = rect.left() + rect.width() * ((db + 60.0) / 60.0) as f32;
                p.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(1.0_f32, c),
                );
            }
            ui.end_row();
            ui.label("");
            let (txt, color) = if self.running() {
                (txt, color)
            } else {
                ("shows once the input starts".into(), ui.visuals().weak_text_color())
            };
            ui.label(RichText::new(txt).small().color(color))
                .on_hover_text("Loudest sample in the last half second; the white mark is the -10 dBFS target, the red one -6 dBFS, the most that leaves room for a louder watch.");
            ui.end_row();
        });
        if let Some(e) = &self.gain_error {
            ui.label(
                RichText::new(e)
                    .small()
                    .color(Color32::from_rgb(0xe0, 0x40, 0x40)),
            );
        }
    }

    fn set_lift(&mut self, lift: f64) {
        self.lift_deg = lift;
        if let Some(l) = self.live.as_mut() {
            l.set_lift(lift);
        }
        self.rebuild_trend();
        self.note_settings();
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
            cols[0]
                .label("seconds per day")
                .on_hover_text("+ the watch gains that many seconds a day, − it loses them");
            let amp = r.and_then(|r| r.amplitude_deg);
            cols[1].label(RichText::new("Amplitude").size(14.0));
            cols[1].label(big(amp.map_or(dash.clone(), |v| format!("{v:.0}°"))));
            let lift = fields::plain(self.lift_deg);
            cols[1]
                .label(match r {
                    Some(LiveReading {
                        amplitude_even_deg: Some(a),
                        amplitude_odd_deg: Some(b),
                        ..
                    }) => format!("tick {a:.0}°   tock {b:.0}°   lift angle {lift}°"),
                    _ => format!("lift angle {lift}°"),
                })
                .on_hover_text(
                    "The big figure is the average amplitude. Below it are the amplitude \
                 measured from the ticks (blue on the strip and charts) and from the tocks \
                 (orange). The sound can't tell which beat is which pallet, so the first beat \
                 heard is called the tick. A big difference between them usually means one \
                 beat's sounds were misread, not a fault in the watch.",
                );
            let unlock = r.and_then(|r| r.beat_error_unlock_ms);
            let drop = r.and_then(|r| r.beat_error_ms);
            cols[2].label(RichText::new("Beat error").size(14.0));
            cols[2].label(big(unlock
                .or(drop)
                .map_or(dash.clone(), |v| format!("{v:+.2}"))));
            cols[2]
                .label(match (unlock, drop) {
                    (Some(_), Some(d)) => format!("ms   from the drop {d:+.2}"),
                    (None, Some(_)) => "ms, from the drop".into(),
                    _ => "ms".into(),
                })
                .on_hover_text(
                    "The big figure is timed from the unlock, as tg and commercial \
                     timegraphers measure it. The smaller one is timed from the beat \
                     as a whole, nearer the drop: it is the gap between the two lines \
                     on the strip.",
                );
        });
        ui.horizontal(|ui| {
            let mut parts = Vec::new();
            if let Some(b) = r.and_then(|r| r.bph) {
                parts.push(format!("{b} bph"));
            }
            if let Some(r) = r {
                parts.push(format!(
                    "{} beats in {}",
                    r.beats_used,
                    fields::duration(self.average_s)
                ));
            }
            parts.push(format!(
                "{} {}",
                self.position.code(),
                self.position.name().to_lowercase()
            ));
            ui.label(RichText::new(parts.join("   ·   ")).weak());
            if let Some(j) = r.and_then(|r| r.jitter_us) {
                ui.label(RichText::new(format!("·   jitter {j:.0} µs")).weak())
                    .on_hover_text(
                        "How far single beats land from the steady line that the rate and \
                         beat error are fitted to: the spread of the dots across the strip, \
                         as a robust standard deviation in microseconds (millionths of a \
                         second). Lower is steadier. It rises with background noise or a \
                         muffled sound as well as with a watch that runs unevenly (a rubbing \
                         part, a worn tooth, low amplitude), so compare it on the same stand \
                         and microphone.",
                    );
            }
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
                let what = if self.running() {
                    "Listening to"
                } else {
                    "Showing"
                };
                ui.label(format!("{what} {}", self.source_label));
            }
            if self.running() {
                let (txt, color) = self.level_text(ui);
                ui.label(RichText::new(txt).color(color));
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
            // Errors show above the readings instead.
            if let Some((m, false)) = &self.message {
                ui.label(m);
            }
        });
    }

    fn main_view(&mut self, ui: &mut egui::Ui) {
        // Problems with the input go where they can't be missed.
        if let Some((m, true)) = &self.message {
            egui::Frame::new()
                .fill(Color32::from_rgb(0x5a, 0x1a, 0x1a))
                .inner_margin(8.0)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(m).color(Color32::WHITE).size(15.0));
                });
            ui.add_space(4.0);
        }
        self.readouts(ui);
        ui.add_space(4.0);

        // Looking back through a finished or analysed recording.
        let total = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        if !self.running() && total > self.strip.span_s.min(self.average_s) {
            let mut end = self.end_s();
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

        if self.live.is_none() {
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
        }
        let mut panes = std::mem::replace(&mut self.panes, Tree::empty("panes-swap"));
        panes.ui(&mut PaneBehavior { app: self }, ui);
        self.panes = panes;
    }

    fn strip_pane(&mut self, ui: &mut egui::Ui) {
        if self.follow {
            self.centre();
        }
        let end = self.end_s();
        let looking_back = self.view_end.is_some() && end < self.latest_s() - 0.5;
        let note = looking_back.then(|| {
            format!(
                "at {}, double-click for the newest beats",
                strip::fmt_time(end)
            )
        });
        let Some(live) = &self.live else { return };
        let period = live.bph().map_or(0.125, |b| 3600.0 / b as f64);
        let input = strip::draw_strip(
            ui,
            live.beats(),
            period,
            self.anchor,
            end,
            &self.strip,
            note.as_deref(),
            ui.available_size(),
        );
        self.apply_strip_input(input, end);
    }

    fn apply_strip_input(&mut self, input: StripInput, end: f64) {
        if input.span_factor != 1.0 {
            self.strip.set_span(self.strip.span_s * input.span_factor);
        }
        if input.width_factor != 1.0 {
            self.strip
                .set_half_width(self.strip.half_width_ms * input.width_factor);
        }
        if input.time_shift_s != 0.0 {
            let latest = self.latest_s();
            let to = end + input.time_shift_s;
            self.view_end = if to >= latest {
                None
            } else {
                Some(to.max(self.strip.span_s.min(latest)))
            };
        }
        if input.lead_shift_ms != 0.0 {
            if let Some(a) = self.anchor.as_mut() {
                a.time += input.lead_shift_ms / 1000.0;
                // Moving the trace by hand ends auto-centring.
                self.follow = false;
            }
        }
        if input.reset {
            self.view_end = None;
            self.centre();
        }
    }

    fn chart(&mut self, ui: &mut egui::Ui, pane: Pane) {
        let colors = strip::side_colors(ui.visuals().dark_mode);
        let main = ui.visuals().strong_text_color();
        let weak = ui.visuals().weak_text_color();
        let pick = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<[f64; 2]> {
            self.trend
                .iter()
                .filter_map(|p| f(p).map(|v| [p.t, v]))
                .collect()
        };
        type Series = Vec<(String, Vec<[f64; 2]>, Color32)>;
        let (series, unit, decimals): (Series, &'static str, usize) = match pane {
            Pane::Rate => (
                vec![(
                    format!(
                        "Rate in seconds per day, {} average",
                        fields::duration(self.average_s)
                    ),
                    pick(|p| p.rate),
                    colors[0],
                )],
                " s/day",
                1,
            ),
            Pane::Amplitude => (
                vec![
                    ("Average amplitude".into(), pick(|p| p.amplitude), main),
                    (
                        "Amplitude from tick".into(),
                        pick(|p| p.amplitude_a),
                        colors[0],
                    ),
                    (
                        "Amplitude from tock".into(),
                        pick(|p| p.amplitude_b),
                        colors[1],
                    ),
                ],
                "°",
                0,
            ),
            Pane::BeatError => (
                vec![
                    (
                        "From the unlock".into(),
                        pick(|p| p.beat_error_unlock),
                        main,
                    ),
                    (
                        "From the drop".into(),
                        pick(|p| p.beat_error_drop),
                        colors[1],
                    ),
                ],
                " ms",
                2,
            ),
            Pane::Strip => return,
        };
        let mut plot = Plot::new(pane.title())
            .link_axis("trend", [true, false])
            .link_cursor("trend", [true, false])
            .allow_scroll(false)
            .allow_zoom([false, true])
            .x_grid_spacer(time_grid)
            .custom_x_axes(vec![egui_plot::AxisHints::new_x()
                .formatter(|m, _| strip::fmt_time(m.value))
                .label_spacing(36.0..=48.0)])
            .y_grid_spacer(nice_grid)
            .y_axis_min_width(52.0)
            .y_axis_formatter(move |m, _| {
                let d = if m.step_size >= 1.0 {
                    0
                } else if m.step_size >= 0.1 {
                    1
                } else {
                    2
                };
                // No "-0.0" from rounding.
                let v = if m.value.abs() < m.step_size * 1e-6 {
                    0.0
                } else {
                    m.value
                };
                format!("{:.*}{unit}", d, v)
            })
            // The values at the pointer are shown for every line at once
            // (below), instead of only when the pointer is on a line.
            .show_x(false)
            .show_y(false);
        if series.len() > 1 {
            plot = plot.legend(Legend::default());
        }
        // Scale to the 2nd to 98th percentile of the lines, so that one
        // glitch doesn't flatten them.
        let all: Vec<[f64; 2]> = series.iter().flat_map(|s| s.1.iter().copied()).collect();
        if let Some((lo, hi)) = percentile_range(&all) {
            plot = plot.default_y_bounds(lo, hi);
        }
        let marker = (self.view_end.is_some() || !self.running()).then(|| self.end_s());
        let lookup: Vec<(String, Vec<[f64; 2]>, Color32)> = series.clone();
        let resp = plot.show(ui, |p| {
            if p.response().hovered() {
                let s = p.ctx().input(|i| i.smooth_scroll_delta);
                let wheel = s.x + s.y;
                if wheel != 0.0 {
                    p.zoom_bounds_around_hovered(Vec2::new((wheel * 0.003).exp(), 1.0));
                }
            }
            for (name, pts, c) in series {
                let w = if c == main || pane == Pane::Rate {
                    1.8_f32
                } else {
                    1.0_f32
                };
                p.line(Line::new(name, pts).color(c).width(w));
            }
            if let Some(t) = marker {
                p.vline(VLine::new("", t).color(weak).width(1.0_f32));
            }
            let (hovered, clicked) = {
                let r = p.response();
                (r.hovered(), r.clicked() && !r.double_clicked())
            };
            let hover = p.pointer_coordinate().filter(|_| hovered);
            if let Some(h) = hover {
                p.vline(VLine::new("", h.x).color(weak).width(1.0_f32));
            }
            let click = clicked.then(|| p.pointer_coordinate()).flatten();
            (click, hover.map(|h| h.x))
        });
        let (click, hover) = resp.inner;
        if let Some(x) = hover {
            resp.response.on_hover_ui_at_pointer(|ui| {
                ui.label(RichText::new(strip::fmt_time(x)).strong());
                for (name, pts, c) in &lookup {
                    let v = value_at(pts, x);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("■").color(*c));
                        ui.label(match v {
                            Some(v) => format!("{name}: {v:.decimals$}{unit}"),
                            None => format!("{name}: none"),
                        });
                    });
                }
            });
        }
        if let Some(at) = click {
            if at.x > 0.0 {
                self.view_end = Some(at.x);
            }
        }
    }
}

/// Grid steps for a time axis in seconds: whole seconds, minutes and hours.
fn time_grid(input: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    const LADDER: [f64; 21] = [
        0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0,
        3600.0, 7200.0, 14400.0, 21600.0, 43200.0, 86400.0,
    ];
    grid_marks(input, &LADDER)
}

/// Grid steps of 1, 2 and 5 times a power of ten.
fn nice_grid(input: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    let p = 10f64.powf(input.base_step_size.max(1e-9).log10().floor());
    let ladder: Vec<f64> = (0..4)
        .flat_map(|k| [1.0, 2.0, 5.0].map(|m| m * p * 10f64.powi(k)))
        .collect();
    grid_marks(input, &ladder)
}

/// Marks at up to three levels of `ladder`, starting at the first step no
/// finer than the plot asks for.
fn grid_marks(input: egui_plot::GridInput, ladder: &[f64]) -> Vec<egui_plot::GridMark> {
    let (lo, hi) = input.bounds;
    let first = ladder
        .iter()
        .position(|&s| s >= input.base_step_size)
        .unwrap_or(ladder.len() - 1);
    // Each level a whole multiple of the one below, so labels of different
    // levels never crowd each other.
    let mut levels = vec![ladder[first]];
    for &s in &ladder[first + 1..] {
        let below = *levels.last().unwrap();
        if levels.len() < 3 && ((s / below).round() * below - s).abs() < below * 1e-6 {
            levels.push(s);
        }
    }
    let mut marks: Vec<egui_plot::GridMark> = Vec::new();
    let mut coarser: Vec<f64> = Vec::new();
    // Coarsest first, so each time keeps the largest step it falls on.
    for &step in levels.iter().rev() {
        let mut k = (lo / step).ceil();
        while k * step <= hi && marks.len() < 5000 {
            let v = k * step;
            if !coarser
                .iter()
                .any(|&c| ((v / c).round() * c - v).abs() < step * 1e-6)
            {
                marks.push(egui_plot::GridMark {
                    value: v,
                    step_size: step,
                });
            }
            k += 1.0;
        }
        coarser.push(step);
    }
    marks
}

/// The value of the point nearest `x` in points sorted by x, unless the
/// nearest is far off (a gap in the line).
fn value_at(points: &[[f64; 2]], x: f64) -> Option<f64> {
    let i = points.partition_point(|p| p[0] < x);
    let near = [i.checked_sub(1), (i < points.len()).then_some(i)]
        .into_iter()
        .flatten()
        .min_by(|&a, &b| {
            (points[a][0] - x)
                .abs()
                .total_cmp(&(points[b][0] - x).abs())
        })?;
    // The usual spacing of the points: the median gap.
    let mut gaps: Vec<f64> = points.windows(2).map(|w| w[1][0] - w[0][0]).collect();
    gaps.sort_by(f64::total_cmp);
    let spacing = gaps.get(gaps.len() / 2).copied().unwrap_or(f64::INFINITY);
    ((points[near][0] - x).abs() <= 3.0 * spacing).then_some(points[near][1])
}

/// The 2nd to 98th percentile of the values, padded a little.
fn percentile_range(points: &[[f64; 2]]) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let mut ys: Vec<f64> = points.iter().map(|p| p[1]).collect();
    ys.sort_by(f64::total_cmp);
    let q = |f: f64| ys[((ys.len() - 1) as f64 * f).round() as usize];
    let (lo, hi) = (q(0.02), q(0.98));
    let pad = ((hi - lo) * 0.15).max(0.05);
    Some((lo - pad, hi + pad))
}

struct PaneBehavior<'a> {
    app: &'a mut TimegrapherApp,
}

impl egui_tiles::Behavior<Pane> for PaneBehavior<'_> {
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile: TileId, pane: &mut Pane) -> UiResponse {
        match pane {
            Pane::Strip => self.app.strip_pane(ui),
            p => self.app.chart(ui, *p),
        }
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        pane.title().into()
    }

    fn simplification_options(&self) -> SimplificationOptions {
        SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
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
        // A file dropped on the window is replayed, where the desktop passes
        // drops on (not yet on Wayland).
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
            .exact_width(280.0)
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

    fn synthetic(seconds: f64, rate: f64) -> TimegrapherApp {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.reset_session(48000, "syn".into());
        let cfg = SynthConfig {
            duration_s: seconds,
            rate_s_per_day: rate,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            if live.push(b) {
                let r = live.reading(app.average_s);
                app.trend.push(TrendPoint::of(r.time_s, &r));
            }
        }
        app.poll();
        app
    }

    /// Lay out and paint frames without a window, in both strip directions,
    /// with a synthetic watch run through the live engine.
    #[test]
    fn draws_a_synthetic_watch_headless() {
        let mut app = synthetic(12.0, 20.0);
        assert!(app.anchor.is_some(), "strip not anchored");
        let r = app.reading().unwrap();
        let rate = r.rate_s_per_day.unwrap();
        assert!((rate - 20.0).abs() < 2.0, "rate {rate}");

        let ctx = egui::Context::default();
        for horizontal in [false, true] {
            app.strip.horizontal = horizontal;
            app.panes = default_layout(horizontal);
            app.follow = horizontal;
            for _ in 0..2 {
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
        }
    }

    #[test]
    fn a_new_averaging_time_redraws_the_whole_chart() {
        let mut app = synthetic(30.0, 20.0);
        let before = app.trend.len();
        assert!(before > 20);
        app.average_s = 4.0;
        app.rebuild_trend();
        // Every point now uses 4 s of beats, back to the start.
        let early = app.trend.iter().find(|p| p.t >= 8.0).unwrap();
        let r = app.live.as_ref().unwrap().reading_at(early.t, 4.0);
        assert_eq!(early.rate, r.rate_s_per_day);
        assert!(
            (app.trend.last().unwrap().t - app.live.as_ref().unwrap().duration_s()).abs() <= 0.5
        );
    }

    #[test]
    fn values_under_the_pointer() {
        let pts = vec![[0.0, 1.0], [1.0, 2.0], [2.0, 3.0], [10.0, 4.0]];
        assert_eq!(value_at(&pts, 0.9), Some(2.0));
        assert_eq!(value_at(&pts, 1.4), Some(2.0));
        assert_eq!(value_at(&pts, -0.5), Some(1.0));
        // In the gap between 2 s and 10 s, nothing near enough.
        assert_eq!(
            value_at(
                &[[0.0, 1.0], [1.0, 2.0], [2.0, 3.0], [3.0, 3.0], [40.0, 4.0]],
                25.0
            ),
            None
        );
        assert_eq!(value_at(&[], 1.0), None);
    }

    #[test]
    fn dragging_the_strip_looks_back_and_returns() {
        let mut app = synthetic(40.0, 0.0);
        let latest = app.latest_s();
        let back = StripInput {
            time_shift_s: -5.0,
            ..Default::default()
        };
        app.apply_strip_input(back, app.end_s());
        assert!((app.end_s() - (latest - 5.0)).abs() < 1e-6);
        let forward = StripInput {
            time_shift_s: 20.0,
            ..Default::default()
        };
        app.apply_strip_input(forward, app.end_s());
        assert_eq!(
            app.view_end, None,
            "past the newest beat follows live again"
        );

        app.follow = true;
        let slide = StripInput {
            lead_shift_ms: 1.0,
            ..Default::default()
        };
        let before = app.anchor.unwrap().time;
        app.apply_strip_input(slide, app.end_s());
        assert!((app.anchor.unwrap().time - before - 0.001).abs() < 1e-12);
        assert!(!app.follow, "sliding by hand ends auto-centring");

        let zoom = StripInput {
            span_factor: 2.0,
            width_factor: 0.5,
            ..Default::default()
        };
        app.apply_strip_input(zoom, app.end_s());
        assert_eq!(app.strip.span_s, 60.0);
        assert_eq!(app.strip.half_width_ms, 5.0);
    }
}
