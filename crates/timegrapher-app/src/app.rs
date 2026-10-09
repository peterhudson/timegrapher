//! The main window: input and settings on the left, the readings across
//! the top, and below them panes (the paper strip and the charts over time)
//! that can be dragged by their tabs into any arrangement.

use crate::fields::{self, Format};
use crate::profiles;
use crate::strip::{self, Anchor, StripInput, StripView};
use crate::theme;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{Legend, Line, Plot};
use egui_tiles::{Linear, LinearDir, SimplificationOptions, TileId, Tiles, Tree, UiResponse};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::Instant;
use timegrapher_core::beats::STANDARD_BPH;
use timegrapher_core::capture::{self, Capture, Event, InputDevice, Level};
use timegrapher_core::diagnose::{HOT_PEAK_DBFS, TARGET_PEAK_DBFS};
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading, MAX_PROFILE_S};
use timegrapher_core::mixer::{GainState, InputGain};
use timegrapher_core::profile::TickProfile;

/// The A and B sides' sounds.
type Profiles = [Option<TickProfile>; 2];

/// How often the sound is kept for looking back, seconds.
const SOUND_EVERY_S: f64 = 2.0;
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
    Sound,
    Rate,
    Amplitude,
    BeatError,
}

impl Pane {
    /// Every pane, in the order the Panes list shows them.
    const ALL: [Pane; 5] = [
        Pane::Strip,
        Pane::Sound,
        Pane::Rate,
        Pane::Amplitude,
        Pane::BeatError,
    ];

    fn title(self) -> &'static str {
        match self {
            Pane::Strip => "Paper strip",
            Pane::Sound => "Tick tock profile",
            Pane::Rate => "Rate",
            Pane::Amplitude => "Amplitude",
            Pane::BeatError => "Beat error",
        }
    }

    /// What the pane shows, for its switch under View.
    fn hint(self) -> &'static str {
        match self {
            Pane::Strip => "The beats as dots on a paper strip, like a printing timegrapher",
            Pane::Sound => "The typical tick and tock sound, with the edges the readings come from",
            Pane::Rate => "The rate over the whole session",
            Pane::Amplitude => "The amplitude over the whole session, from the ticks and tocks",
            Pane::BeatError => "The beat error over the whole session",
        }
    }
}

/// The starting arrangement: the strip beside the sound and the three
/// charts when it runs down the window, above them when it lies on its
/// side.
fn default_layout(horizontal: bool) -> Tree<Pane> {
    let mut tiles = Tiles::default();
    // Each pane in its own tab bar from the start. Left to the tree, the
    // tab bar would take over the pane's id and the pane get a new one,
    // which loses a pane hidden before the first frame.
    let mut tabbed = |p: Pane| {
        let pane = tiles.insert_pane(p);
        tiles.insert_tab_tile(vec![pane])
    };
    let strip = tabbed(Pane::Strip);
    let sound = tabbed(Pane::Sound);
    let charts: Vec<TileId> = [Pane::Rate, Pane::Amplitude, Pane::BeatError]
        .into_iter()
        .map(&mut tabbed)
        .collect();
    let mut rest = vec![sound];
    rest.extend(&charts);
    let dir = if horizontal {
        LinearDir::Horizontal
    } else {
        LinearDir::Vertical
    };
    let mut side = Linear::new(dir, rest);
    side.shares.set_share(sound, 2.5);
    let side = tiles.insert_container(side);
    let root = if horizontal {
        tiles.insert_container(Linear::new_binary(LinearDir::Vertical, [strip, side], 0.45))
    } else {
        tiles.insert_container(Linear::new_binary(
            LinearDir::Horizontal,
            [strip, side],
            0.45,
        ))
    };
    Tree::new("panes", root, tiles)
}

/// Whether a pane is shown.
fn pane_visible(tiles: &Tiles<Pane>, pane: Pane) -> bool {
    tiles
        .find_pane(&pane)
        .is_some_and(|id| tiles.is_visible(id))
}

/// Show or hide a pane. A hidden pane keeps its place, so it comes back
/// where it was.
fn set_pane_visible(tiles: &mut Tiles<Pane>, pane: Pane, visible: bool) {
    if let Some(id) = tiles.find_pane(&pane) {
        tiles.set_visible(id, visible);
        sync_containers(tiles);
    }
}

/// Hide every container with nothing visible in it, and show the rest, so
/// hidden panes leave no empty frame or tab bar behind.
fn sync_containers(tiles: &mut Tiles<Pane>) {
    loop {
        let mut changed = false;
        let ids: Vec<TileId> = tiles.tile_ids().collect();
        for id in ids {
            let Some(c) = tiles.get_container(id) else {
                continue;
            };
            let any = c.children().any(|&c| tiles.is_visible(c));
            if any != tiles.is_visible(id) {
                tiles.set_visible(id, any);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
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
    /// The rate, amplitude and beat error figures above the panes.
    show_readings: bool,
    /// Asked to put the charts back on the whole session, following new
    /// beats: set by the button, applied by every chart on the next frame.
    charts_to_live: bool,
    charts_to_live_now: bool,
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
    /// The tick and tock sound: how it is drawn, what it was every few
    /// seconds of this session, and one worked out from the file for a
    /// moment that history doesn't cover.
    sound_scale: profiles::Scale,
    sound_history: Vec<(f64, Profiles)>,
    sound_at: Option<(f64, Profiles)>,
    sound_job: Option<(f64, std::sync::mpsc::Receiver<Profiles>)>,
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
            show_readings: true,
            charts_to_live: false,
            charts_to_live_now: false,
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
            sound_scale: profiles::Scale::Linear,
            sound_history: Vec::new(),
            sound_at: None,
            sound_job: None,
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
        c.profile_s = self.sound_span();
        c
    }

    /// Seconds of beats the tick and tock sound covers: the averaging time,
    /// so it describes the same beats as the readings, up to the engine's
    /// limit.
    fn sound_span(&self) -> f64 {
        self.average_s.clamp(0.5, MAX_PROFILE_S)
    }

    /// A new averaging time: the readings over the whole session are worked
    /// out again, and the tick and tock sound follows it.
    fn set_average(&mut self, seconds: f64) {
        self.average_s = seconds;
        let span = self.sound_span();
        if let Some(l) = self.live.as_mut() {
            l.set_profile_span(span);
        }
        // The sound kept so far covered the old span.
        self.sound_at = None;
        self.rebuild_trend();
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
        self.sound_history.clear();
        self.sound_at = None;
        self.sound_job = None;
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
                            let p = live.tick_profiles();
                            let due = self
                                .sound_history
                                .last()
                                .is_none_or(|h| r.time_s - h.0 >= SOUND_EVERY_S);
                            if due && p.iter().any(Option::is_some) {
                                self.sound_history.push((r.time_s, p.clone()));
                            }
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

    /// The inputs the device menu offers.
    fn input_choices(&self) -> Vec<capture::InputChoice> {
        if self.all_inputs {
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
        }
    }

    /// The bar across the top: where the sound comes from, and the button
    /// that starts it.
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(28.0);
            theme::segmented(
                ui,
                &mut self.input,
                &[
                    (
                        Input::Microphone,
                        "Microphone",
                        "Listen to a watch on the microphone",
                    ),
                    (
                        Input::File,
                        "Recording",
                        "Replay or analyse a WAV or FLAC recording",
                    ),
                ],
            );
            ui.add_space(4.0);
            match self.input {
                Input::Microphone => {
                    let choices = self.input_choices();
                    let sel = choices
                        .iter()
                        .find(|c| Some(&c.id) == self.device.as_ref())
                        .map_or_else(|| "Choose a microphone".to_string(), |c| c.label.clone());
                    egui::ComboBox::from_id_salt("device")
                        .selected_text(short(&sel, 36))
                        .width(270.0)
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
                        })
                        .response
                        .on_hover_text(
                            "The microphone to listen to. \"Show every input\" in the \
                             sidebar lists every device the system offers.",
                        );
                    if ui
                        .button("Rescan")
                        .on_hover_text("Look for microphones again, after plugging one in")
                        .clicked()
                    {
                        self.devices = capture::list().unwrap_or_default();
                        if self.device.is_none() {
                            self.device = capture::choices(&self.devices)
                                .first()
                                .map(|c| c.id.clone());
                        }
                    }
                }
                Input::File => {
                    if ui
                        .button("Open…")
                        .on_hover_text("Choose a WAV or FLAC recording")
                        .clicked()
                    {
                        self.open_file_dialog();
                    }
                    let w = (ui.available_width() - 260.0).clamp(120.0, 420.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.file_path)
                            .hint_text("or type a file's path, or drop it on the window")
                            .desired_width(w),
                    );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.running() {
                    let what = if self.input == Input::File {
                        "Stop the replay"
                    } else {
                        "Stop listening"
                    };
                    if ui
                        .add(theme::destructive(ui, "Stop"))
                        .on_hover_text(what)
                        .clicked()
                    {
                        self.stop();
                    }
                } else {
                    match self.input {
                        Input::Microphone => {
                            if ui
                                .add(theme::primary(ui, "Start"))
                                .on_hover_text("Start listening to the microphone")
                                .clicked()
                            {
                                self.start_microphone();
                            }
                        }
                        Input::File => {
                            let path = PathBuf::from(self.file_path.trim());
                            let ok = !self.file_path.trim().is_empty();
                            if ui
                                .add_enabled(ok, theme::primary(ui, "Replay"))
                                .on_hover_text("Play it through at its own speed, as if live")
                                .on_disabled_hover_text("Open a recording first")
                                .clicked()
                            {
                                self.start_replay(&path);
                            }
                            if ui
                                .add_enabled(
                                    ok && self.batch.is_none(),
                                    egui::Button::new("Analyse all").min_size(Vec2::new(0.0, 26.0)),
                                )
                                .on_hover_text("Analyse the whole file now and look through it")
                                .clicked()
                            {
                                self.start_batch(&path);
                            }
                        }
                    }
                }
                if self.input == Input::Microphone {
                    ui.add_space(8.0);
                    self.meter(ui, 120.0, 6.0).on_hover_text(
                        "Input level: the loudest sample in the last half second. Aim for \
                         the ticks to reach the light mark (-10 dBFS) and stay short of the \
                         red one (-6 dBFS). Set it with Input level in the sidebar.",
                    );
                    let (txt, color) = self.level_text(ui);
                    if self.running() {
                        ui.label(RichText::new(txt).small().color(color));
                    }
                }
            });
        });
    }

    /// The sidebar: the watch, the microphone, the strip and the view, each
    /// in a card of its own.
    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 8.0;
        theme::section(ui, "Watch");
        theme::card_ui(ui, |ui| self.watch_settings(ui));
        if self.input == Input::Microphone {
            theme::section(ui, "Microphone");
            theme::card_ui(ui, |ui| self.microphone_settings(ui));
        }
        theme::section(ui, "Paper strip");
        theme::card_ui(ui, |ui| self.strip_settings(ui));
        theme::section(ui, "View");
        theme::card_ui(ui, |ui| self.view_settings(ui));
        theme::section(ui, "Mouse");
        theme::card_ui(ui, |ui| {
            let pal = theme::pal(ui);
            for (what, how) in [
                (
                    "Strip",
                    "wheel for length, Ctrl+wheel for width, drag to move, \
                     double-click for the newest beats",
                ),
                (
                    "Charts",
                    "wheel to zoom time, Ctrl+wheel for the scale, drag to pan, \
                     double-click to fit, click to show that moment on the strip",
                ),
                (
                    "Panes",
                    "drag one by its tab to rearrange, close it with its ×, \
                     bring it back under View",
                ),
            ] {
                ui.label(
                    RichText::new(format!("{what}: {how}."))
                        .small()
                        .color(pal.text_secondary),
                );
            }
        });
        ui.add_space(8.0);
    }

    fn watch_settings(&mut self, ui: &mut egui::Ui) {
        theme::row(ui, "Name", Some("Noted in a saved recording"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.watch)
                    .hint_text("Make, model or calibre")
                    .desired_width(f32::INFINITY),
            );
        });
        let mut bph = self.bph;
        theme::row(
            ui,
            "Beat rate",
            Some("Beats per hour. Auto finds it from the first few seconds of beats."),
            |ui| {
                egui::ComboBox::from_id_salt("bph")
                    .width(ui.available_width())
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
            },
        );
        if bph != self.bph {
            self.bph = bph;
            if let Some(l) = self.live.as_mut() {
                l.set_bph(bph);
            }
            self.anchor = None;
            self.trend.clear();
        }

        let mut lift = self.lift_deg;
        let changed = theme::row(
            ui,
            "Lift angle",
            Some(
                "The calibre's lift angle in degrees: type it and press Enter, or pick \
                 a common one from the menu. Amplitude depends on it.",
            ),
            |ui| {
                let a = fields::entry(ui, "lift", &mut lift, 10.0..=90.0, &MS, 44.0);
                ui.label("°");
                let b = fields::presets(ui, "lift-presets", &mut lift, &LIFTS, |v| {
                    format!("{}°", fields::plain(v))
                });
                a || b
            },
        );
        if changed {
            self.set_lift(lift);
        }

        let mut pos = self.position;
        theme::row(
            ui,
            "Position",
            Some(
                "Noted in a saved recording, and changing it starts the readings again. \
                 Guided runs through the positions will use it.",
            ),
            |ui| {
                egui::ComboBox::from_id_salt("position")
                    .width(ui.available_width())
                    .selected_text(format!("{} ({})", pos.name(), pos.code()))
                    .show_ui(ui, |ui| {
                        for p in Position::ALL {
                            ui.selectable_value(
                                &mut pos,
                                p,
                                format!("{} ({})", p.name(), p.code()),
                            );
                        }
                    });
            },
        );
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

        let mut avg = self.average_s;
        let changed = theme::row(
            ui,
            "Average over",
            Some(
                "Each reading is fitted over this much of the latest beats, and the \
                 tick and tock profile is the typical beat over the same time (up to 60 s)",
            ),
            |ui| {
                let a = fields::entry(ui, "average", &mut avg, 1.0..=3600.0, &DURATION, 56.0);
                let b =
                    fields::presets(ui, "average-presets", &mut avg, &AVERAGES, fields::duration);
                a || b
            },
        );
        if changed {
            self.set_average(avg);
        }
    }

    fn microphone_settings(&mut self, ui: &mut egui::Ui) {
        if self.gain_device != self.device {
            self.read_gain();
        }
        self.gain_controls(ui);
        ui.add_space(2.0);
        theme::switch_row(
            ui,
            "Show every input",
            &mut self.all_inputs,
            "List every device the system offers in the microphone menu, with its id",
        );
        ui.add_enabled_ui(!self.running(), |ui| {
            theme::switch_row(
                ui,
                "Save the recording",
                &mut self.save,
                "Keep the sound as a FLAC file with the watch's details, in the folder \
                 below. Set it before pressing Start.",
            );
            if self.save {
                theme::row(ui, "Folder", Some("Where recordings are saved"), |ui| {
                    if ui.button("Choose…").clicked() {
                        let mut d = rfd::FileDialog::new().set_title("Folder for recordings");
                        if Path::new(&self.save_dir).is_dir() {
                            d = d.set_directory(&self.save_dir);
                        }
                        if let Some(p) = d.pick_folder() {
                            self.save_dir = p.display().to_string();
                        }
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.save_dir).desired_width(f32::INFINITY));
            }
        });
    }

    fn strip_settings(&mut self, ui: &mut egui::Ui) {
        theme::row(
            ui,
            "Width",
            Some(
                "Milliseconds either side of the centre line. Ctrl and the mouse wheel \
                 on the strip change it too.",
            ),
            |ui| {
                ui.label("±");
                let mut w = self.strip.half_width_ms;
                let a = fields::entry(
                    ui,
                    "width",
                    &mut w,
                    StripView::MIN_HALF_WIDTH_MS..=StripView::MAX_HALF_WIDTH_MS,
                    &MS,
                    44.0,
                );
                ui.label("ms");
                let b = fields::presets(ui, "width-presets", &mut w, &WIDTHS, |v| {
                    format!("±{} ms", fields::plain(v))
                });
                if a || b {
                    self.strip.set_half_width(w);
                }
            },
        );
        theme::row(
            ui,
            "Length",
            Some(
                "Time shown along the strip, such as 30 s, 5 min or 2 h. \
                 The mouse wheel on the strip changes it too.",
            ),
            |ui| {
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
            },
        );
        let mut h = self.strip.horizontal;
        theme::row(
            ui,
            "Direction",
            Some("Which way time runs along the strip"),
            |ui| {
                theme::segmented(
                    ui,
                    &mut h,
                    &[
                        (
                            false,
                            "Down",
                            "Newest beats at the top, the strip beside the charts",
                        ),
                        (
                            true,
                            "Across",
                            "Newest beats at the right, the strip above the charts",
                        ),
                    ],
                );
            },
        );
        if h != self.strip.horizontal {
            self.strip.horizontal = h;
            self.relayout(h);
        }
        theme::switch_row(
            ui,
            "Auto-centre",
            &mut self.follow,
            "Keep the newest beats on the centre line and slide the rest",
        );
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
                .on_disabled_hover_text("Starts the readings again while listening")
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

    fn view_settings(&mut self, ui: &mut egui::Ui) {
        theme::switch_row(
            ui,
            "Readings",
            &mut self.show_readings,
            "The rate, amplitude and beat error figures across the top",
        );
        for pane in Pane::ALL {
            let mut on = pane_visible(&self.panes.tiles, pane);
            if theme::switch_row(ui, pane.title(), &mut on, pane.hint()) {
                set_pane_visible(&mut self.panes.tiles, pane, on);
            }
        }
        ui.horizontal(|ui| {
            if ui
                .button("Reset panes")
                .on_hover_text("Show every pane and put them back where they started")
                .clicked()
            {
                self.show_readings = true;
                self.panes = default_layout(self.strip.horizontal);
            }
        });
        ui.add_space(2.0);
        let mut t = ui.ctx().options(|o| o.theme_preference);
        theme::row(
            ui,
            "Appearance",
            Some("Light or dark, or follow the system"),
            |ui| {
                theme::segmented(
                    ui,
                    &mut t,
                    &[
                        (egui::ThemePreference::System, "Auto", "Follow the system"),
                        (egui::ThemePreference::Light, "Light", ""),
                        (egui::ThemePreference::Dark, "Dark", ""),
                    ],
                );
            },
        );
        if t != ui.ctx().options(|o| o.theme_preference) {
            ui.ctx().set_theme(t);
        }
    }

    fn clipping(&self) -> bool {
        self.clipped_at
            .is_some_and(|t| t.elapsed().as_secs_f64() < 3.0)
    }

    /// The input level in words and the colour to show it in.
    fn level_text(&self, ui: &egui::Ui) -> (String, Color32) {
        let pal = theme::pal(ui);
        let peak = self.level.peak_dbfs.max(-99.0);
        if self.clipping() {
            (format!("peak {peak:.0} dBFS, clipping"), pal.bad)
        } else if peak > HOT_PEAK_DBFS {
            (format!("peak {peak:.0} dBFS, too hot"), pal.warn)
        } else if peak < -40.0 {
            (format!("peak {peak:.0} dBFS, very quiet"), pal.warn)
        } else {
            (format!("peak {peak:.0} dBFS"), pal.text_secondary)
        }
    }

    /// The level meter: a bar to the latest peak, in green, amber or red,
    /// with marks at the target and at the hot limit.
    fn meter(&self, ui: &mut egui::Ui, width: f32, height: f32) -> egui::Response {
        let pal = theme::pal(ui);
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
        let r = height / 2.0;
        let p = ui.painter();
        p.rect_filled(rect, r, pal.control);
        if self.running() {
            let peak = self.level.peak_dbfs.max(-60.0);
            let frac = ((peak + 60.0) / 60.0).clamp(0.0, 1.0) as f32;
            let color = if self.clipping() {
                pal.bad
            } else if !(-40.0..=HOT_PEAK_DBFS).contains(&peak) {
                pal.warn
            } else {
                pal.good
            };
            let mut fill = rect;
            fill.set_width((rect.width() * frac).max(height));
            p.rect_filled(fill, r, color);
        }
        for (db, c) in [
            (TARGET_PEAK_DBFS, pal.text_secondary),
            (HOT_PEAK_DBFS, pal.bad),
        ] {
            let x = rect.left() + rect.width() * ((db + 60.0) / 60.0) as f32;
            p.line_segment(
                [
                    egui::pos2(x, rect.top() - 2.0),
                    egui::pos2(x, rect.bottom() + 2.0),
                ],
                egui::Stroke::new(1.5_f32, c),
            );
        }
        resp
    }

    /// Find the selected input's level control and read it.
    fn read_gain(&mut self) {
        self.gain_device = self.device.clone();
        self.gain = self.device.as_deref().and_then(InputGain::for_device);
        self.gain_state = self.gain.as_ref().and_then(|g| g.read());
        self.gain_read_at = Instant::now();
    }

    /// The input level control and meter, in the Microphone card.
    fn gain_controls(&mut self, ui: &mut egui::Ui) {
        if self.gain_read_at.elapsed().as_secs_f64() > if self.running() { 2.0 } else { 10.0 } {
            self.read_gain();
        }
        let pal = theme::pal(ui);
        let hint = "The microphone's gain. For the system default this is the sound server's \
                    input volume, which it puts back on the microphone each time it opens it; \
                    for a direct device it is the card's capture level. Aim for ticks peaking \
                    around -10 dBFS, and never clipping.";
        match (self.gain.clone(), self.gain_state.clone()) {
            (Some(g), Some(st)) => {
                theme::row(ui, "Input level", Some(hint), |ui| {
                    let mut pct = st.level * 100.0;
                    ui.spacing_mut().slider_width = (ui.available_width() - 44.0).max(60.0);
                    let r = ui.add(
                        egui::Slider::new(&mut pct, 0.0..=100.0)
                            .show_value(false)
                            .step_by(1.0),
                    );
                    ui.label(RichText::new(&st.text).small().color(pal.text_secondary));
                    // Set it once the drag ends (or on a click or key), not
                    // on every pixel.
                    if (r.changed() && !r.dragged()) || r.drag_stopped() {
                        self.gain_error = g.set_level(pct / 100.0).err();
                        self.read_gain();
                    } else if r.changed() {
                        if let Some(s) = self.gain_state.as_mut() {
                            s.level = pct / 100.0;
                        }
                    }
                });
                if let Some(on) = st.agc {
                    let mut agc = on;
                    if theme::switch_row(
                        ui,
                        if on {
                            "Auto gain (turn it off)"
                        } else {
                            "Auto gain"
                        },
                        &mut agc,
                        "The microphone's automatic gain changes the level as it \
                         listens, which spoils amplitude and level readings. Keep it off.",
                    ) {
                        self.gain_error = g.set_agc(agc).err();
                        self.read_gain();
                    }
                }
            }
            _ => {
                theme::row(ui, "Input level", Some(hint), |ui| {
                    ui.label(
                        RichText::new(if cfg!(target_os = "linux") {
                            "not adjustable here"
                        } else {
                            "set it in the system's sound settings"
                        })
                        .color(pal.text_secondary),
                    );
                });
            }
        }
        let level_hint = "Loudest sample in the last half second; the light mark is the \
                          -10 dBFS target, the red one -6 dBFS, the most that leaves room \
                          for a louder watch.";
        theme::row(ui, "Peak", Some(level_hint), |ui| {
            let w = ui.available_width();
            self.meter(ui, w, 8.0).on_hover_text(level_hint);
        });
        let (txt, color) = if self.running() {
            self.level_text(ui)
        } else {
            ("shows once the input starts".into(), pal.text_tertiary)
        };
        ui.horizontal(|ui| {
            ui.add_space(theme::LABEL_W + ui.spacing().item_spacing.x);
            ui.label(RichText::new(txt).small().color(color));
        });
        if let Some(e) = &self.gain_error {
            ui.label(RichText::new(e).small().color(pal.bad));
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

    /// The three readings, each in a card: a caption, the figure large with
    /// its unit beside it, and a line of detail.
    fn readouts(&self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let r = self.reading();
        let figure = |ui: &mut egui::Ui, value: Option<String>, unit: &str| {
            let mut job = egui::text::LayoutJob::default();
            let (v, c) = match value {
                // A true minus sign, as wide as the plus.
                Some(v) => (v.replace('-', "−"), pal.text),
                None => ("—".to_string(), pal.text_tertiary),
            };
            job.append(&v, 0.0, egui::TextFormat::simple(theme::display(36.0), c));
            if !unit.is_empty() {
                job.append(
                    unit,
                    6.0,
                    egui::TextFormat::simple(egui::FontId::proportional(15.0), pal.text_secondary),
                );
            }
            ui.label(job);
        };
        let card = |ui: &mut egui::Ui, title: &str, hint: &str, body: &dyn Fn(&mut egui::Ui)| {
            theme::card()
                .fill(pal.card)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(theme::caption(title).color(pal.text_secondary));
                    body(ui);
                })
                .response
                .on_hover_text(hint);
        };
        ui.spacing_mut().item_spacing.x = theme::GAP;
        ui.columns(3, |cols| {
            let rate = r.and_then(|r| r.rate_s_per_day);
            card(
                &mut cols[0],
                "Rate",
                "+ the watch gains that many seconds a day, − it loses them",
                &|ui| {
                    figure(ui, rate.map(|v| format!("{v:+.1}")), "seconds per day");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let small = |t: String| RichText::new(t).small().color(pal.text_secondary);
                        let Some(r) = r else {
                            ui.label(small("waiting for beats".into()));
                            return;
                        };
                        ui.label(small(format!(
                            "{} beats in {}",
                            r.beats_used,
                            fields::duration(self.average_s)
                        )))
                        .on_hover_text(
                            "The beats the readings are fitted to. Set the time with \
                             Average over in the sidebar.",
                        );
                        if let Some(j) = r.jitter_us {
                            ui.label(small("·".into()));
                            ui.label(small(format!("jitter {j:.0} µs"))).on_hover_text(
                                "How far single beats land from the steady line that the rate \
                                 and beat error are fitted to: the spread of the dots across the \
                                 strip, as a robust standard deviation in microseconds \
                                 (millionths of a second). Lower is steadier. It rises with \
                                 background noise or a muffled sound as well as with a watch \
                                 that runs unevenly (a rubbing part, a worn tooth, low \
                                 amplitude), so compare it on the same stand and microphone.",
                            );
                        }
                    });
                },
            );
            let amp = r.and_then(|r| r.amplitude_deg);
            let lift = fields::plain(self.lift_deg);
            card(
                &mut cols[1],
                "Amplitude",
                "The big figure is the average amplitude. Below it are the amplitude \
                 measured from the ticks (blue on the strip and charts) and from the tocks \
                 (orange). The sound can't tell which beat is which pallet, so the first beat \
                 heard is called the tick. A big difference between them usually means one \
                 beat's sounds were misread, not a fault in the watch.",
                &|ui| {
                    figure(ui, amp.map(|v| format!("{v:.0}°")), "");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if let Some(LiveReading {
                            amplitude_even_deg: Some(a),
                            amplitude_odd_deg: Some(b),
                            ..
                        }) = r
                        {
                            for (name, v, c) in [("Tick", a, pal.tick), ("Tock", b, pal.tock)] {
                                theme::dot(ui, c);
                                ui.label(
                                    RichText::new(format!("{name} {v:.0}°"))
                                        .small()
                                        .color(pal.text_secondary),
                                );
                                ui.add_space(6.0);
                            }
                        }
                        ui.label(
                            RichText::new(format!("lift angle {lift}°"))
                                .small()
                                .color(pal.text_secondary),
                        );
                    });
                },
            );
            let unlock = r.and_then(|r| r.beat_error_unlock_ms);
            let drop = r.and_then(|r| r.beat_error_ms);
            card(
                &mut cols[2],
                "Beat error",
                "The big figure is timed from the unlock, as tg and commercial \
                 timegraphers measure it. The smaller one is timed from the beat \
                 as a whole, nearer the drop: it is the gap between the two lines \
                 on the strip.",
                &|ui| {
                    figure(
                        ui,
                        unlock.or(drop).map(|v| format!("{v:+.2}")),
                        "milliseconds",
                    );
                    ui.label(
                        RichText::new(match (unlock, drop) {
                            (Some(_), Some(d)) => {
                                format!("from the unlock · from the drop {d:+.2} ms")
                            }
                            (None, Some(_)) => "from the drop".into(),
                            _ => "from the unlock".into(),
                        })
                        .small()
                        .color(pal.text_secondary),
                    );
                },
            );
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            if let Some(b) = &self.batch {
                let done = f64::from_bits(b.progress.load(std::sync::atomic::Ordering::Relaxed));
                let frac = b.duration_s.map_or(0.0, |d| (done / d).clamp(0.0, 1.0));
                ui.add(
                    egui::ProgressBar::new(frac as f32)
                        .desired_width(240.0)
                        .desired_height(14.0)
                        .fill(pal.accent)
                        .text(RichText::new(format!("Analysing {}", b.label)).small()),
                );
            } else if !self.source_label.is_empty() {
                let what = if self.running() {
                    "Listening to"
                } else {
                    "Showing"
                };
                ui.label(
                    RichText::new(format!("{what} {}", self.source_label))
                        .small()
                        .color(pal.text_secondary),
                );
            } else {
                ui.label(
                    RichText::new("Not listening")
                        .small()
                        .color(pal.text_tertiary),
                );
            }
            if self.running() {
                if let Some(s) = self.reading().and_then(|r| r.snr) {
                    ui.label(
                        RichText::new(if s < 4.0 {
                            "no watch heard".to_string()
                        } else {
                            format!("signal {:.0}×", s)
                        })
                        .small()
                        .color(if s < 4.0 {
                            pal.warn
                        } else {
                            pal.text_secondary
                        }),
                    )
                    .on_hover_text("How far the beats stand above the background noise");
                }
            }
            if let Some(r) = &self.recorder {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    theme::dot(ui, pal.bad);
                    ui.label(
                        RichText::new(format!("Saving {}", strip::fmt_time(r.duration_s())))
                            .small()
                            .color(pal.bad),
                    );
                })
                .response
                .on_hover_text(r.dir().display().to_string());
            }
            // Errors show above the readings instead.
            if let Some((m, false)) = &self.message {
                ui.label(RichText::new(m).small().color(pal.text_secondary));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        self.position.code(),
                        self.position.name().to_lowercase()
                    ))
                    .small()
                    .color(pal.text_secondary),
                )
                .on_hover_text("The position, set under Watch in the sidebar");
                if let Some(b) = self.reading().and_then(|r| r.bph) {
                    ui.label(
                        RichText::new(format!("{b} beats per hour"))
                            .small()
                            .color(pal.text_secondary),
                    )
                    .on_hover_text("Found from the beats, or set under Watch in the sidebar");
                }
            });
        });
    }

    /// The starting arrangement for a strip direction, keeping which panes
    /// are hidden.
    fn relayout(&mut self, horizontal: bool) {
        let hidden: Vec<Pane> = Pane::ALL
            .into_iter()
            .filter(|&p| !pane_visible(&self.panes.tiles, p))
            .collect();
        self.panes = default_layout(horizontal);
        for p in hidden {
            set_pane_visible(&mut self.panes.tiles, p, false);
        }
    }

    /// What shows before there is anything to show: how to begin, and the
    /// two ways to.
    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.add_space((ui.available_height() * 0.18).clamp(16.0, 120.0));
        ui.vertical_centered(|ui| {
            let w = 440.0_f32.min(ui.available_width());
            ui.allocate_ui(Vec2::new(w, 0.0), |ui| {
                theme::card().fill(pal.card).show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        ui.label(RichText::new("Ready to listen").font(theme::semibold(20.0)));
                        for (n, step) in [
                            "Put the watch on the microphone, dial up.",
                            "Choose the microphone at the top and set its level in the sidebar.",
                            "Press Start. The readings settle once the first beats are in.",
                        ]
                        .iter()
                        .enumerate()
                        {
                            ui.horizontal(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::hover());
                                ui.painter().circle_filled(rect.center(), 10.0, pal.control);
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    (n + 1).to_string(),
                                    theme::semibold(11.0),
                                    pal.text_secondary,
                                );
                                ui.add(egui::Label::new(*step).wrap());
                            });
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(theme::primary(ui, "Start listening"))
                                .on_hover_text("Listen to the microphone chosen at the top")
                                .clicked()
                            {
                                self.input = Input::Microphone;
                                self.start_microphone();
                            }
                            if ui
                                .add(
                                    egui::Button::new("Open a recording…")
                                        .min_size(Vec2::new(0.0, 26.0)),
                                )
                                .on_hover_text("Replay a WAV or FLAC file as if live")
                                .clicked()
                            {
                                self.input = Input::File;
                                if let Some(p) = self.open_file_dialog() {
                                    self.start_replay(&p);
                                }
                            }
                        });
                        ui.label(
                            RichText::new("You can also drop a WAV or FLAC file on the window.")
                                .small()
                                .color(pal.text_tertiary),
                        );
                    });
                });
            });
        });
    }

    /// A problem, where it can't be missed, with a way to put it away.
    fn error_banner(&mut self, ui: &mut egui::Ui) {
        let Some((m, true)) = self.message.clone() else {
            return;
        };
        let pal = theme::pal(ui);
        let mut dismiss = false;
        let resp = theme::card()
            .fill(pal.bad.gamma_multiply(0.14))
            .inner_margin(egui::Margin {
                left: 18,
                right: 10,
                top: 10,
                bottom: 10,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        dismiss = ui
                            .add(egui::Button::new("Dismiss").frame_when_inactive(false))
                            .on_hover_text("Hide this message")
                            .clicked();
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(&m).color(pal.text)).wrap());
                        });
                    });
                });
            });
        // A bar down the left edge in red.
        let r = resp.response.rect;
        let bar = egui::Rect::from_min_size(r.left_top(), Vec2::new(4.0, r.height()));
        ui.painter().rect_filled(
            bar,
            egui::CornerRadius {
                nw: theme::CARD_RADIUS,
                sw: theme::CARD_RADIUS,
                ne: 0,
                se: 0,
            },
            pal.bad,
        );
        if dismiss {
            self.message = None;
        }
        ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
    }

    fn main_view(&mut self, ui: &mut egui::Ui) {
        self.error_banner(ui);
        if self.live.is_none() {
            self.empty_state(ui);
            return;
        }
        if self.show_readings {
            self.readouts(ui);
            ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
        }

        // Looking back through a finished or analysed recording.
        let total = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        if !self.running() && total > self.strip.span_s.min(self.average_s) {
            let mut end = self.end_s();
            let pal = theme::pal(ui);
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(theme::caption("Time").color(pal.text_secondary))
                    .on_hover_text("The moment the readings, strip and profile show");
                ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(100.0);
                if ui
                    .add(
                        egui::Slider::new(&mut end, 0.0..=total)
                            .custom_formatter(|v, _| strip::fmt_time(v)),
                    )
                    .on_hover_text("Drag to look back through the recording")
                    .changed()
                {
                    self.view_end = Some(end);
                }
            });
            ui.add_space(4.0);
        }

        // A drag can leave a container holding only hidden panes.
        sync_containers(&mut self.panes.tiles);
        if Pane::ALL
            .iter()
            .all(|&p| !pane_visible(&self.panes.tiles, p))
        {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Every pane is hidden. Turn one on under View in the sidebar.")
                        .size(15.0)
                        .weak(),
                );
            });
            return;
        }
        self.charts_to_live_now = std::mem::take(&mut self.charts_to_live);
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

    fn sound_pane(&mut self, ui: &mut egui::Ui) {
        let end = self.end_s();
        let latest = self.view_end.is_none() && self.running();
        let mut note = None;
        let shown: Profiles = if latest {
            self.live
                .as_ref()
                .map_or([None, None], |l| l.tick_profiles().clone())
        } else {
            // Looking back: what the session kept nearest before the
            // moment, else work it out from the file.
            let i = self.sound_history.partition_point(|h| h.0 <= end + 1e-9);
            match i.checked_sub(1).map(|i| &self.sound_history[i]) {
                Some((t, p)) if end - t <= SOUND_EVERY_S + 0.5 => {
                    note = Some(format!("at {}", strip::fmt_time(*t)));
                    p.clone()
                }
                _ => {
                    self.poll_sound_job();
                    match &self.sound_at {
                        Some((t, p)) if (t - end).abs() < 0.5 => {
                            note = Some(format!("at {}", strip::fmt_time(*t)));
                            p.clone()
                        }
                        _ => {
                            note = Some(if self.start_sound_job(end) {
                                "working it out from the recording...".to_string()
                            } else {
                                "not kept for this moment".to_string()
                            });
                            [None, None]
                        }
                    }
                }
            }
        };
        profiles::draw(ui, &shown, &mut self.sound_scale, note.as_deref());
    }

    /// Start working out the sound at `end` from the file, unless one is
    /// already under way. False when there is no file to read.
    fn start_sound_job(&mut self, end: f64) -> bool {
        // One at a time: while the strip is dragged, the next starts when
        // this one is done.
        if self.sound_job.is_some() {
            return true;
        }
        let path = PathBuf::from(self.file_path.trim());
        let Some(live) = &self.live else { return false };
        let Some(bph) = live.bph() else { return false };
        if self.input != Input::File || !path.is_file() {
            return false;
        }
        let span = live.config().profile_s.max(0.5);
        let from = (end - span).max(0.0);
        // A little audio either side of the beats for the template windows.
        let (a0, a1) = (from - 0.05, end + 0.05);
        let beats: Vec<timegrapher_core::beats::Beat> = live
            .beats()
            .iter()
            .filter(|b| b.time >= from && b.time < end)
            .map(|b| timegrapher_core::beats::Beat {
                time: b.time - a0.max(0.0),
                ..*b
            })
            .collect();
        let cfg = live.config().analysis.clone();
        let period = 7200.0 / bph as f64;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Ok(info) = timegrapher_core::audio::info(&path) else {
                return;
            };
            let fs = info.sample_rate as f64;
            let (i0, i1) = ((a0.max(0.0) * fs) as usize, (a1 * fs) as usize);
            let mut x = Vec::with_capacity(i1.saturating_sub(i0));
            let mut pos = 0usize;
            let _ = timegrapher_core::audio::stream(&path, 48000, |b| {
                let (s, e) = (pos, pos + b.len());
                if e > i0 && s < i1 {
                    x.extend_from_slice(&b[i0.saturating_sub(s)..(i1 - s).min(b.len())]);
                }
                pos = e;
            });
            let p = timegrapher_core::profile::profiles_from_audio(&x, fs, &beats, period, &cfg);
            let _ = tx.send(p);
        });
        self.sound_job = Some((end, rx));
        true
    }

    fn poll_sound_job(&mut self) {
        if let Some((t, rx)) = &self.sound_job {
            match rx.try_recv() {
                Ok(p) => {
                    self.sound_at = Some((*t, p));
                    self.sound_job = None;
                }
                Err(TryRecvError::Disconnected) => self.sound_job = None,
                Err(TryRecvError::Empty) => {}
            }
        }
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
        let pal = theme::pal(ui);
        let colors = [pal.tick, pal.tock];
        let main = pal.text;
        let weak = pal.text_tertiary;
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
                    main,
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
                        pal.text_secondary,
                    ),
                ],
                " ms",
                2,
            ),
            Pane::Strip | Pane::Sound => return,
        };
        let mut plot = Plot::new(pane.title())
            .link_axis("trend", [true, false])
            .link_cursor("trend", [true, false])
            .allow_scroll(false)
            .allow_zoom([false, true])
            .x_grid_spacer(time_grid)
            .custom_x_axes(vec![egui_plot::AxisHints::new_x()
                .formatter(|m, _| strip::fmt_time(m.value))
                .label_spacing(30.0..=40.0)])
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
            plot = plot.legend(
                Legend::default()
                    .text_style(egui::TextStyle::Small)
                    .background_alpha(0.85),
            );
        }
        // Scale to the 2nd to 98th percentile of the lines, so that one
        // glitch doesn't flatten them.
        let all: Vec<[f64; 2]> = series.iter().flat_map(|s| s.1.iter().copied()).collect();
        if let Some((lo, hi)) = percentile_range(&all) {
            plot = plot.default_y_bounds(lo, hi);
        }
        let marker = (self.view_end.is_some() || !self.running()).then(|| self.end_s());
        let lookup: Vec<(String, Vec<[f64; 2]>, Color32)> = series.clone();
        let to_live = self.charts_to_live_now;
        let resp = plot.show(ui, |p| {
            if to_live {
                p.set_auto_bounds([true, true]);
            }
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
            let (hovered, clicked) = {
                let r = p.response();
                (r.hovered(), r.clicked() && !r.double_clicked())
            };
            let hover = p.pointer_coordinate().filter(|_| hovered);
            let click = clicked.then(|| p.pointer_coordinate()).flatten();
            // Panned or zoomed away from the whole session.
            let moved = !to_live && !p.auto_bounds().x;
            (click, hover.map(|h| h.x), moved)
        });
        let (click, hover, moved) = resp.inner;
        // The moment shown and the pointer's time, painted over the plot
        // rather than added to it: a plot line counts towards the automatic
        // bounds, so one under the pointer near the edge widened the chart
        // a little more every frame.
        let frame = *resp.transform.frame();
        let painter = ui.painter_at(frame);
        for x in [marker, hover].into_iter().flatten() {
            let px = resp.transform.position_from_point_x(x);
            if px >= frame.left() && px <= frame.right() {
                painter.vline(px, frame.y_range(), egui::Stroke::new(1.0_f32, weak));
            }
        }
        let running = self.running();
        if moved {
            // Say how to get back, on the chart itself.
            let at = resp.response.rect.left_top() + Vec2::new(60.0, 4.0);
            let label = if running {
                "Follow live"
            } else {
                "Show whole session"
            };
            let b = ui.put(
                egui::Rect::from_min_size(at, Vec2::new(150.0, 24.0)),
                theme::primary(ui, label).min_size(Vec2::new(0.0, 24.0)),
            );
            if b.on_hover_text(if running {
                "Back to the whole session, following new beats as they come in. \
                 Double-clicking a chart does the same."
            } else {
                "Back to the whole session. Double-clicking a chart does the same."
            })
            .clicked()
            {
                self.charts_to_live = true;
                ui.ctx().request_repaint();
            }
        }
        if let Some(x) = hover {
            resp.response.on_hover_ui_at_pointer(|ui| {
                ui.set_max_width(300.0);
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
                ui.label(
                    RichText::new(if running {
                        "Wheel zooms time, Ctrl+wheel the scale, drag pans, \
                         click shows that moment on the strip, \
                         double-click follows live again."
                    } else {
                        "Wheel zooms time, Ctrl+wheel the scale, drag pans, \
                         click shows that moment on the strip, \
                         double-click shows the whole session."
                    })
                    .small()
                    .weak(),
                );
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
    /// Each pane is a card under its tab.
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile: TileId, pane: &mut Pane) -> UiResponse {
        let pal = theme::pal(ui);
        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::CARD_RADIUS), pal.card);
        let inner = rect.shrink2(Vec2::new(8.0, 6.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| match pane {
            Pane::Strip => self.app.strip_pane(ui),
            Pane::Sound => self.app.sound_pane(ui),
            p => self.app.chart(ui, *p),
        });
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        RichText::new(pane.title())
            .font(theme::semibold(12.5))
            .into()
    }

    fn is_tab_closable(&self, tiles: &Tiles<Pane>, tile_id: TileId) -> bool {
        matches!(tiles.get(tile_id), Some(egui_tiles::Tile::Pane(_)))
    }

    /// Closing a tab hides the pane instead of removing it, so the Panes
    /// list can bring it back in the same place.
    fn on_tab_close(&mut self, tiles: &mut Tiles<Pane>, tile_id: TileId) -> bool {
        if let Some(egui_tiles::Tile::Pane(p)) = tiles.get(tile_id) {
            let p = *p;
            set_pane_visible(tiles, p, false);
        }
        false
    }

    fn simplification_options(&self) -> SimplificationOptions {
        SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }

    // The tabs sit on the window like captions over their cards, with no
    // bar or outline of their own.
    fn tab_bar_color(&self, visuals: &egui::Visuals) -> Color32 {
        theme::palette(visuals.dark_mode).window
    }

    fn tab_bg_color(
        &self,
        visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        _state: &egui_tiles::TabState,
    ) -> Color32 {
        theme::palette(visuals.dark_mode).window
    }

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        _state: &egui_tiles::TabState,
    ) -> egui::Stroke {
        egui::Stroke::NONE
    }

    fn tab_bar_hline_stroke(&self, _visuals: &egui::Visuals) -> egui::Stroke {
        egui::Stroke::NONE
    }

    fn tab_text_color(
        &self,
        visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Color32 {
        let p = theme::palette(visuals.dark_mode);
        if state.active {
            p.text
        } else {
            p.text_tertiary
        }
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        24.0
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        8.0
    }

    fn resize_stroke(
        &self,
        style: &egui::Style,
        resize_state: egui_tiles::ResizeState,
    ) -> egui::Stroke {
        let accent = theme::palette(style.visuals.dark_mode).accent;
        match resize_state {
            egui_tiles::ResizeState::Idle => egui::Stroke::NONE,
            egui_tiles::ResizeState::Hovering => {
                egui::Stroke::new(2.0_f32, accent.gamma_multiply(0.6))
            }
            egui_tiles::ResizeState::Dragging => egui::Stroke::new(2.0_f32, accent),
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

impl TimegrapherApp {
    /// The toolbar across the top, the status line along the bottom, the
    /// sidebar of settings on the left and the readings and panes in the
    /// rest, all on the window colour with the content in cards.
    fn panels(&mut self, ctx: &egui::Context) {
        let pal = theme::palette(ctx.style().visuals.dark_mode);
        let bar = egui::Frame::new().fill(pal.window);
        egui::TopBottomPanel::top("toolbar")
            .frame(bar.inner_margin(egui::Margin::symmetric(14, 10)))
            .show(ctx, |ui| self.toolbar(ui));
        egui::TopBottomPanel::bottom("status")
            .frame(bar.inner_margin(egui::Margin::symmetric(14, 5)))
            .show(ctx, |ui| self.status_bar(ui));
        egui::SidePanel::left("controls")
            .resizable(false)
            .exact_width(300.0)
            .show_separator_line(false)
            .frame(bar.inner_margin(egui::Margin {
                left: 12,
                right: 0,
                top: 8,
                bottom: 0,
            }))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(ui.max_rect().with_max_x(ui.max_rect().right() - 6.0)),
                        |ui| self.controls(ui),
                    );
                });
            });
        egui::CentralPanel::default()
            .frame(bar.inner_margin(egui::Margin {
                left: 6,
                right: 12,
                top: 12,
                bottom: 12,
            }))
            .show(ctx, |ui| self.main_view(ui));
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

        self.panels(ctx);

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

        let ctx = themed();
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
                let out = ctx.run(input, |ctx| app.panels(ctx));
                assert!(!out.shapes.is_empty());
            }
        }
    }

    /// A context with the app's fonts and styles, as the window has.
    fn themed() -> egui::Context {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx
    }

    /// Draw `n` frames of the whole window.
    fn frames(app: &mut TimegrapherApp, ctx: &egui::Context, n: usize) {
        for _ in 0..n {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1280.0, 820.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.panels(ctx));
        }
    }

    /// The window before anything is open, with a problem showing, in both
    /// themes.
    #[test]
    fn draws_the_empty_and_error_states() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.error("Can't read take.flac".into());
        let ctx = themed();
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.set_theme(theme);
            frames(&mut app, &ctx, 2);
        }
        app.message = None;
        app.input = Input::File;
        frames(&mut app, &ctx, 1);
    }

    #[test]
    fn hidden_panes_stay_hidden_through_a_new_direction() {
        let mut app = synthetic(12.0, 20.0);
        let ctx = themed();
        frames(&mut app, &ctx, 2);
        for p in [Pane::Rate, Pane::Amplitude, Pane::Strip] {
            set_pane_visible(&mut app.panes.tiles, p, false);
        }
        frames(&mut app, &ctx, 2);
        app.strip.horizontal = true;
        app.relayout(true);
        frames(&mut app, &ctx, 3);
        for p in [Pane::Rate, Pane::Amplitude, Pane::Strip] {
            assert!(!pane_visible(&app.panes.tiles, p), "{p:?} came back");
        }
        assert!(pane_visible(&app.panes.tiles, Pane::Sound));
    }

    #[test]
    fn a_new_averaging_time_redraws_the_whole_chart() {
        let mut app = synthetic(30.0, 20.0);
        let before = app.trend.len();
        assert!(before > 20);
        assert_eq!(app.live.as_ref().unwrap().config().profile_s, 10.0);
        app.set_average(4.0);
        // The tick and tock sound covers the same beats as the readings.
        assert_eq!(app.live.as_ref().unwrap().config().profile_s, 4.0);
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
    fn hidden_panes_leave_no_empty_frames_and_come_back() {
        let mut tree = default_layout(false);
        // Wrap each pane in its tab bar, as drawing does.
        tree.simplify(&SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        });
        let tiles = &mut tree.tiles;
        let visible_containers = |t: &Tiles<Pane>| {
            t.tile_ids()
                .filter(|&id| t.get_container(id).is_some() && t.is_visible(id))
                .count()
        };
        let all = visible_containers(tiles);
        set_pane_visible(tiles, Pane::Rate, false);
        assert!(!pane_visible(tiles, Pane::Rate));
        // Its tab container goes too, nothing else.
        assert_eq!(visible_containers(tiles), all - 1);
        for p in Pane::ALL {
            set_pane_visible(tiles, p, false);
        }
        assert_eq!(visible_containers(tiles), 0, "root still shown");
        set_pane_visible(tiles, Pane::Amplitude, true);
        assert!(pane_visible(tiles, Pane::Amplitude));
        assert!(tiles.is_visible(tree.root.unwrap()));
        assert!(!pane_visible(tiles, Pane::Rate));

        // A new strip direction keeps the choice.
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        set_pane_visible(&mut app.panes.tiles, Pane::Sound, false);
        app.relayout(true);
        assert!(!pane_visible(&app.panes.tiles, Pane::Sound));
        assert!(pane_visible(&app.panes.tiles, Pane::Strip));
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
