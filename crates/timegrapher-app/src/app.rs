//! The main window: input and settings on the left, the readings across
//! the top, and below them panes (the paper strip and the charts over time)
//! that can be dragged by their tabs into any arrangement.

use crate::fields::{self, Format};
use crate::help;
use crate::profiles;
use crate::settings::{CustomCalibre, CustomWheel, Settings};
use crate::steady::{self, Steadiness};
use crate::strip::{self, Anchor, StripInput, StripView};
use crate::theme;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{GridMark, Legend, Line, LineStyle, Plot, Points};
use egui_tiles::{Linear, LinearDir, SimplificationOptions, TileId, Tiles, Tree, UiResponse};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::Instant;
use timegrapher_core::beats::STANDARD_BPH;
use timegrapher_core::calibres;
use timegrapher_core::capture::{self, Capture, Event, InputDevice, Level};
use timegrapher_core::clockstore;
use timegrapher_core::diagnose::{HOT_PEAK_DBFS, TARGET_PEAK_DBFS};
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading, MAX_PROFILE_S};
use timegrapher_core::mixer::{GainState, InputGain};
use timegrapher_core::periodicity::Wheel;
use timegrapher_core::profile::TickProfile;

/// The A and B sides' sounds.
type Profiles = [Option<TickProfile>; 2];

/// How often the sound is kept for looking back, seconds.
const SOUND_EVERY_S: f64 = 2.0;
use timegrapher_core::recorder::{Recorder, SessionInfo};
use timegrapher_core::stream::{self, BeatLog, StreamConfig};

/// What a question before throwing away unsaved sound is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    NewSession,
    Quit,
}

/// Where a session's sound is kept until it is saved or thrown away.
fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join("timegrapher-unsaved")
}

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
            Position::DialUp => "Dial Up",
            Position::DialDown => "Dial Down",
            Position::CrownUp => "Crown Up",
            Position::CrownDown => "Crown Down",
            Position::CrownLeft => "Crown Left",
            Position::CrownRight => "Crown Right",
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Pane {
    Strip,
    Sound,
    Rate,
    Amplitude,
    BeatError,
    RateHistogram,
    AmplitudeHistogram,
    BeatErrorHistogram,
    Steadiness,
}

impl Pane {
    /// Every pane, in the order the Panes list shows them.
    const ALL: [Pane; 9] = [
        Pane::Strip,
        Pane::Sound,
        Pane::Steadiness,
        Pane::Rate,
        Pane::Amplitude,
        Pane::BeatError,
        Pane::RateHistogram,
        Pane::AmplitudeHistogram,
        Pane::BeatErrorHistogram,
    ];
    const CHARTS: [Pane; 3] = [Pane::Rate, Pane::Amplitude, Pane::BeatError];
    const HISTOGRAMS: [Pane; 3] = [
        Pane::RateHistogram,
        Pane::AmplitudeHistogram,
        Pane::BeatErrorHistogram,
    ];

    fn title(self) -> &'static str {
        match self {
            Pane::Strip => "Paper Strip",
            Pane::Sound => "Tick Tock Profile",
            Pane::Rate => "Rate",
            Pane::Amplitude => "Amplitude",
            Pane::BeatError => "Beat Error",
            Pane::RateHistogram => "Rate Distribution",
            Pane::AmplitudeHistogram => "Amplitude Distribution",
            Pane::BeatErrorHistogram => "Beat Error Distribution",
            Pane::Steadiness => "Steadiness",
        }
    }

    /// The short name in a sidebar card that is already about charts or
    /// histograms.
    fn short(self) -> &'static str {
        match self {
            Pane::Rate | Pane::RateHistogram => "Rate",
            Pane::Amplitude | Pane::AmplitudeHistogram => "Amplitude",
            Pane::BeatError | Pane::BeatErrorHistogram => "Beat Error",
            p => p.title(),
        }
    }

    /// What the pane shows, for its switch in the sidebar.
    fn hint(self) -> &'static str {
        match self {
            Pane::Strip => "The beats as dots on a paper strip, like a printing timegrapher",
            Pane::Sound => "The typical tick and tock sound, with the edges the readings come from",
            Pane::Rate => "The rate over the whole session",
            Pane::Amplitude => "The amplitude over the whole session, from the ticks and tocks",
            Pane::BeatError => "The beat error over the whole session",
            Pane::RateHistogram => {
                "The rate readings on probability paper: one steady rate falls on a straight \
                 line, two states bend it"
            }
            Pane::AmplitudeHistogram => {
                "The amplitude readings on probability paper: a balance swinging between a \
                 high and a low state bends the line"
            }
            Pane::BeatErrorHistogram => {
                "The beat error readings, from the unlock, on probability paper"
            }
            Pane::Steadiness => {
                "Whether the rate, amplitude and beat error each hold steady over the \
                 session, or carry a cycle, two states, a step, a drift or wander"
            }
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
    let charts: Vec<TileId> = Pane::CHARTS.into_iter().map(&mut tabbed).collect();
    let hists: Vec<TileId> = Pane::HISTOGRAMS.into_iter().map(&mut tabbed).collect();
    // The steadiness tests share the profile's place, as a second tab.
    let steadiness = tiles.insert_pane(Pane::Steadiness);
    if let Some(egui_tiles::Tile::Container(c)) = tiles.get_mut(sound) {
        c.add_child(steadiness);
    }
    let dir = if horizontal {
        LinearDir::Horizontal
    } else {
        LinearDir::Vertical
    };
    // The histograms side by side across the charts, hidden until wanted.
    let across = if horizontal {
        LinearDir::Vertical
    } else {
        LinearDir::Horizontal
    };
    let hist_row = tiles.insert_container(Linear::new(across, hists));
    let mut rest = vec![sound];
    rest.extend(&charts);
    rest.push(hist_row);
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
    let mut tree = Tree::new("panes", root, tiles);
    for p in Pane::HISTOGRAMS {
        set_pane_visible(&mut tree.tiles, p, false);
    }
    tree
}

/// A layout saved before the Steadiness pane existed, with it added as a
/// tab beside the profile (or at the top, if the profile is not in tabs).
/// After a drag moves a tab out of a tab group, egui_tiles can leave the
/// group's active tab pointing at the tile that left, so the group shows
/// nothing. Point every group back at one of its own tabs.
fn repair_tabs(tree: &mut Tree<Pane>) {
    for tile in tree.tiles.tiles_mut() {
        if let egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t)) = tile {
            if t.active.is_none_or(|a| !t.children.contains(&a)) {
                t.active = t.children.first().copied();
            }
        }
    }
}

fn add_missing_steadiness(tree: &mut Tree<Pane>) {
    if tree.tiles.find_pane(&Pane::Steadiness).is_some() {
        return;
    }
    let pane = tree.tiles.insert_pane(Pane::Steadiness);
    let host = tree
        .tiles
        .find_pane(&Pane::Sound)
        .and_then(|sound| tree.tiles.parent_of(sound))
        .filter(|&id| {
            matches!(
                tree.tiles.get(id),
                Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(_)))
            )
        })
        .or(tree.root());
    if let Some(Some(egui_tiles::Tile::Container(c))) = host.map(|id| tree.tiles.get_mut(id)) {
        c.add_child(pane);
    }
}

/// Whether a saved layout holds every pane exactly once.
fn layout_is_whole(tree: &Tree<Pane>) -> bool {
    let panes: Vec<Pane> = tree
        .tiles
        .tiles()
        .filter_map(|t| match t {
            egui_tiles::Tile::Pane(p) => Some(*p),
            _ => None,
        })
        .collect();
    panes.len() == Pane::ALL.len() && Pane::ALL.iter().all(|p| panes.contains(p))
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

/// Width of the time panes' left axis, the strip's (lying across) and the
/// charts' alike, so their time axes line up when stacked.
const GUTTER: f32 = 76.0;

/// Shares labelled up the probability plot, percent, as on probability
/// paper.
const PROBABILITY_MARKS: [(f64, &str); 11] = [
    (0.1, "0.1%"),
    (1.0, "1%"),
    (5.0, "5%"),
    (10.0, "10%"),
    (25.0, "25%"),
    (50.0, "50%"),
    (75.0, "75%"),
    (90.0, "90%"),
    (95.0, "95%"),
    (99.0, "99%"),
    (99.9, "99.9%"),
];

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
/// A wheel's period: as many decimals as it has, up to three (3.75 s).
const PERIOD: Format = Format {
    show: |v| {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    },
    parse: fields::parse_number,
};
const DURATION: Format = Format {
    show: fields::duration,
    parse: fields::parse_duration,
};

/// One point of the charts over time.
/// The calibre picked under Watch.
#[derive(Clone, Copy)]
enum Calibre<'a> {
    None,
    Table(&'a calibres::Calibre),
    Custom(usize),
}

/// What the table says about a calibre, for its entry in the menu.
fn calibre_hint(c: &calibres::Calibre) -> String {
    let mut t = format!("{} bph", c.bph);
    if let Some(l) = c.lift_angle_deg {
        t += &format!(", lift angle {}°", fields::plain(l));
    }
    for w in &c.wheels {
        t += &format!("\n{}: {} s", w.name, (PERIOD.show)(w.period_s));
    }
    t
}

/// One line on a reading's chart over the session.
#[derive(Clone)]
struct ChartLine {
    name: String,
    pts: Vec<[f64; 2]>,
    color: Color32,
    style: LineStyle,
    /// The swatch beside its value under the pointer.
    key: Color32,
    /// The reading itself, drawn heavier than the lines beside it.
    main: bool,
}

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
    /// Set to stop the analysis.
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    duration_s: Option<f64>,
    label: String,
    started: Instant,
    /// How many files, and the first and last names, for a folder.
    files: usize,
    first: String,
    last: String,
}

impl Drop for Batch {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Unwound through the analysis to stop it when it is cancelled.
struct Cancelled;

pub struct TimegrapherApp {
    input: Input,
    devices: Vec<InputDevice>,
    /// Whether `devices` has been filled. Listing the inputs asks each one
    /// what it supports, which briefly opens it and can disturb another
    /// program recording from it, so the app lists them only when the
    /// microphone menu opens, Rescan is pressed or listening starts.
    devices_listed: bool,
    device: Option<String>,
    file_path: String,
    bph: Option<u32>,
    lift_deg: f64,
    position: Position,
    /// Where the watch was put in a new position during the session: the
    /// time and the position from then on, oldest first. Empty while it has
    /// stayed in `position` all along.
    stretches: Vec<(f64, Position)>,
    average_s: f64,
    watch: String,
    /// The calibre picked under Watch: a name in the built-in table or one
    /// of `custom_calibres`, or empty when not set.
    calibre: String,
    custom_calibres: Vec<CustomCalibre>,
    /// Where the last Save put the session, if it has been saved.
    saved_to: Option<PathBuf>,
    /// The folder Save offers first.
    save_dir: String,
    /// The session came from the microphone, and when it was paused.
    mic_session: bool,
    paused_at: Option<Instant>,
    /// Asking before something throws away unsaved sound.
    confirm: Option<Confirm>,
    /// The window may close: the user has said what to do with the sound.
    may_close: bool,
    strip: StripView,
    /// Keep the newest beats on the centre line, sliding the rest.
    follow: bool,
    /// Draw the rate reading as a line over the strip.
    rate_line: bool,
    /// Faint parallels to the rate line across the strip.
    rate_guides: bool,
    /// Amplitude, rate and beat error drawn over the strip.
    overlays: [bool; 3],
    /// The charts and histograms cover only the strip's length, ending
    /// where the strip does, instead of the whole session.
    span_of_strip: bool,
    /// Show the cumulative share instead of the probability plot.
    hist_cumulative: bool,
    /// Amplitude and beat error histograms count the readings rather than
    /// each 2 seconds of beats.
    hist_readings: bool,
    /// What the Steadiness pane draws under each verdict.
    steady_view: steady::View,
    /// The steadiness tests' latest answer over the session, and the run
    /// in the background that will replace it.
    steady: Option<Steadiness>,
    steady_job: Option<std::sync::mpsc::Receiver<Steadiness>>,
    /// The time under the pointer on the strip or a chart, drawn as a
    /// cursor on all of them: last frame's, and this frame's so far.
    cursor_t: Option<f64>,
    cursor_next: Option<f64>,
    /// Light, dark or following the system, kept for the next start.
    theme: egui::ThemePreference,
    /// Sidebar cards folded down to their caption.
    folded: std::collections::BTreeSet<String>,
    /// The sidebar is shown.
    sidebar: bool,
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
    /// The stored clock error of the input this session listens to, if one
    /// was measured (`timegrapher clock measure --save`).
    clock: Option<clockstore::DeviceClock>,
    gain_error: Option<String>,
    message: Option<(String, bool)>,
    /// The tick and tock sound: how it is drawn, what it was every few
    /// seconds of this session, and one worked out from the file for a
    /// moment that history doesn't cover.
    sound_opts: profiles::Options,
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
    pub fn new(file: Option<PathBuf>, analyse: bool, saved: Option<Settings>) -> Self {
        // No device is touched at launch: see `devices_listed`.
        let mut app = Self::with_devices(Vec::new(), file, analyse);
        app.devices_listed = false;
        if let Some(s) = saved {
            app.apply_settings(s);
        }
        app
    }

    /// The views and choices kept from the last time the app ran.
    pub fn settings(&self) -> Settings {
        Settings {
            device: self.device.clone(),
            average_s: self.average_s,
            strip: self.strip,
            rate_line: self.rate_line,
            rate_guides: self.rate_guides,
            overlays: self.overlays,
            follow: self.follow,
            span_of_strip: self.span_of_strip,
            hist_cumulative: self.hist_cumulative,
            hist_readings: self.hist_readings,
            sound: self.sound_opts,
            folded: self.folded.iter().cloned().collect(),
            sidebar: self.sidebar,
            theme: self.theme,
            steady_view: self.steady_view,
            panes: Some(self.panes.clone()),
            custom_calibres: self.custom_calibres.clone(),
        }
    }

    /// Put back the views and choices from the last time. A pane layout
    /// from another version of the app, missing a pane or holding one twice,
    /// is set aside for the starting one.
    pub fn apply_settings(&mut self, s: Settings) {
        let Settings {
            device,
            average_s,
            strip,
            rate_line,
            rate_guides,
            overlays,
            follow,
            span_of_strip,
            hist_cumulative,
            hist_readings,
            sound,
            folded,
            sidebar,
            theme,
            steady_view,
            panes,
            custom_calibres,
        } = s;
        self.custom_calibres = custom_calibres;
        // Checked against the inputs when they are listed.
        if let Some(d) = device.filter(|d| {
            !self.devices_listed || capture::choices(&self.devices).iter().any(|c| &c.id == d)
        }) {
            self.device = Some(d);
        }
        if AVERAGES.contains(&average_s) {
            self.set_average(average_s);
        }
        let mut strip = strip;
        strip.set_half_width(strip.half_width_ms);
        strip.set_span(strip.span_s);
        self.strip = strip;
        self.rate_line = rate_line;
        self.rate_guides = rate_guides;
        self.overlays = overlays;
        self.follow = follow;
        self.span_of_strip = span_of_strip;
        self.hist_cumulative = hist_cumulative;
        self.hist_readings = hist_readings;
        self.sound_opts = sound;
        self.folded = folded.into_iter().collect();
        self.sidebar = sidebar;
        self.theme = theme;
        self.steady_view = steady_view;
        self.panes = match panes.map(|mut p| {
            add_missing_steadiness(&mut p);
            p
        }) {
            Some(p) if layout_is_whole(&p) => p,
            _ => default_layout(self.strip.horizontal),
        };
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
            devices_listed: true,
            device,
            file_path: file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            bph: None,
            lift_deg: 52.0,
            position: Position::DialUp,
            stretches: Vec::new(),
            average_s: 10.0,
            watch: String::new(),
            calibre: String::new(),
            custom_calibres: Vec::new(),
            saved_to: None,
            mic_session: false,
            paused_at: None,
            confirm: None,
            may_close: false,
            save_dir: home.join("Timegrapher recordings").display().to_string(),
            strip: StripView {
                half_width_ms: 10.0,
                span_s: 30.0,
                horizontal: false,
            },
            follow: false,
            rate_line: true,
            rate_guides: true,
            overlays: [true, false, false],
            span_of_strip: false,
            hist_cumulative: false,
            steady_view: steady::View::default(),
            steady: None,
            steady_job: None,
            hist_readings: true,
            cursor_t: None,
            cursor_next: None,
            folded: Default::default(),
            theme: egui::ThemePreference::System,
            sidebar: true,
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
            clock: None,
            gain_error: None,
            message: None,
            sound_opts: profiles::Options::default(),
            sound_history: Vec::new(),
            sound_at: None,
            sound_job: None,
            all_inputs: false,
            opened_at: Instant::now(),
            audio_at: None,
        };
        if let Some(f) = file {
            // A folder of segments can only be analysed at once.
            if analyse || f.is_dir() {
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
        self.discard_recording();
        self.mic_session = false;
        self.clock = None;
        self.paused_at = None;
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
        self.steady = None;
        self.steady_job = None;
        self.stretches.clear();
    }

    /// List the inputs, once, and keep the chosen one if it is still there,
    /// else take the first.
    fn ensure_devices(&mut self) {
        if !self.devices_listed {
            self.rescan();
        }
    }

    /// List the inputs again, after one is plugged in.
    fn rescan(&mut self) {
        self.devices = capture::list().unwrap_or_default();
        self.devices_listed = true;
        let there = self
            .device
            .as_ref()
            .is_some_and(|d| self.devices.iter().any(|x| &x.id == d));
        if !there {
            self.device = capture::choices(&self.devices)
                .first()
                .map(|c| c.id.clone());
        }
    }

    /// Listen to the microphone: carry on the paused session if there is
    /// one, else start a new one.
    fn start_microphone(&mut self) {
        self.ensure_devices();
        if self.can_resume() {
            return self.resume_microphone();
        }
        self.stop();
        match capture::open_input(self.device.as_deref(), None) {
            Ok(c) => {
                let label = self.device_label(&c);
                self.reset_session(
                    c.sample_rate,
                    format!("{label} at {} Hz, {}-bit", c.sample_rate, c.bits),
                );
                self.opened_at = Instant::now();
                self.audio_at = None;
                self.mic_session = true;
                self.clock = clockstore::ClockStore::load()
                    .ok()
                    .and_then(|s| s.get(&c.label).cloned());
                // Every session is kept as it goes, so it can be saved at any
                // point; it is thrown away with a new session unless saved.
                let info = self.info(&c.label, c.sample_rate, c.bits);
                match Recorder::start(&scratch_dir(), info) {
                    Ok(r) => self.recorder = Some(r),
                    Err(e) => self.error(format!(
                        "Can't keep the sound for saving later: {e}. The readings still work."
                    )),
                }
                self.capture = Some(c);
            }
            Err(e) => self.error(format!(
                "Can't open the input. {}",
                capture::explain_error(&e)
            )),
        }
    }

    fn device_label(&self, c: &Capture) -> String {
        capture::choices(&self.devices)
            .into_iter()
            .find(|ch| Some(&ch.id) == self.device.as_ref())
            .map_or_else(|| c.label.clone(), |ch| ch.label)
    }

    /// A microphone session that is paused and can carry on.
    fn can_resume(&self) -> bool {
        self.mic_session && self.paused_at.is_some() && self.live.is_some() && !self.running()
    }

    /// Stop listening for now, keeping everything.
    fn pause(&mut self) {
        if self.capture.is_none() {
            return;
        }
        self.capture = None;
        if self.mic_session {
            self.paused_at = Some(Instant::now());
            if let Some(r) = self.recorder.as_mut() {
                let _ = r.pause();
            }
        }
    }

    /// Carry on listening after a pause, in the same session.
    fn resume_microphone(&mut self) {
        let rate = self.live.as_ref().map(|l| l.sample_rate());
        match capture::open_input(self.device.as_deref(), rate) {
            Ok(c) if Some(c.sample_rate) == rate => {
                let paused = self
                    .paused_at
                    .take()
                    .map_or(0.0, |t| t.elapsed().as_secs_f64());
                if let Some(l) = self.live.as_mut() {
                    l.skip(paused);
                }
                self.opened_at = Instant::now();
                self.audio_at = None;
                self.view_end = None;
                self.message = None;
                self.capture = Some(c);
            }
            Ok(c) => self.error(format!(
                "The microphone opened at {} Hz this time, not the {} Hz this session \
                 was listening at, so it can't carry on. Press New session to start again.",
                c.sample_rate,
                rate.unwrap_or_default()
            )),
            Err(e) => self.error(format!(
                "Can't open the input. {}",
                capture::explain_error(&e)
            )),
        }
    }

    /// Forget this session (after asking, if its sound isn't saved), ready
    /// to start another.
    fn new_session(&mut self) {
        self.stop();
        self.discard_recording();
        self.live = None;
        self.mic_session = false;
        self.paused_at = None;
        self.trend.clear();
        self.sound_history.clear();
        self.source_label.clear();
        self.message = None;
    }

    /// Unsaved sound that a new session would throw away, seconds.
    fn unsaved_s(&self) -> f64 {
        match (&self.recorder, &self.saved_to) {
            (Some(r), None) => r.duration_s(),
            _ => 0.0,
        }
    }

    /// Close and delete the sound kept for saving.
    fn discard_recording(&mut self) {
        if let Some(r) = self.recorder.take() {
            if let Ok(dir) = r.finish(None) {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
        self.saved_to = None;
    }

    /// Save a copy of the session's sound so far, with its clock log and
    /// settings, in a folder the user picks. Listening carries on.
    fn save_recording(&mut self) {
        let mut d = rfd::FileDialog::new().set_title("Save the recording in…");
        let start = self
            .saved_to
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from(&self.save_dir));
        if start.is_dir() {
            d = d.set_directory(&start);
        }
        let Some(parent) = d.pick_folder() else {
            return;
        };
        self.save_into(&parent);
    }

    fn save_into(&mut self, parent: &Path) {
        let result = self.reading().and_then(|r| serde_json::to_value(r).ok());
        let Some(r) = self.recorder.as_mut() else {
            return;
        };
        let secs = r.duration_s();
        match r.save_copy(parent, result.as_ref()) {
            Ok(dir) => {
                self.info_msg(format!(
                    "Saved {} of sound in {}",
                    strip::fmt_time(secs),
                    dir.display()
                ));
                self.save_dir = parent.display().to_string();
                self.saved_to = Some(dir);
            }
            Err(e) => self.error(format!("Saving the recording failed: {e}")),
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
        let files = match recordings_in(path) {
            Ok(f) => f,
            Err(e) => return self.error(e),
        };
        let mut info = match timegrapher_core::audio::info(&files[0]) {
            Ok(i) => i,
            Err(e) => return self.error(format!("Can't read {}: {e}", files[0].display())),
        };
        for f in &files[1..] {
            match timegrapher_core::audio::info(f) {
                Ok(i) => info.frames = info.frames.zip(i.frames).map(|(a, b)| a + b),
                Err(e) => return self.error(format!("Can't read {}: {e}", f.display())),
            }
        }
        let label = if path.is_dir() {
            match files.len() {
                1 => format!("{} (1 file)", capture_label(path)),
                n => format!("{} ({n} files)", capture_label(path)),
            }
        } else {
            capture_label(path)
        };
        let mut cfg = StreamConfig::default();
        cfg.analysis.bph = self.bph;
        cfg.analysis.amplitude.lift_deg = self.lift_deg;
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (p2, c2) = (progress.clone(), cancel.clone());
        let name = |p: &PathBuf| capture_label(p);
        let (count, first, last) = (files.len(), name(&files[0]), name(&files[files.len() - 1]));
        std::thread::spawn(move || {
            let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
            // A cancel unwinds out of the analysis from its progress report
            // (resume_unwind runs no panic hook, so nothing is printed).
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                stream::analyze_files(&paths, &cfg, |s| {
                    p2.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
                    if c2.load(std::sync::atomic::Ordering::Relaxed) {
                        std::panic::resume_unwind(Box::new(Cancelled));
                    }
                })
            }));
            match r {
                Ok(r) => {
                    let _ = tx.send(r.map(Some).map_err(|e| e.to_string()));
                }
                Err(e) if e.is::<Cancelled>() => {}
                Err(e) => std::panic::resume_unwind(e),
            }
        });
        self.reset_session(info.sample_rate, label.clone());
        self.live = None;
        self.batch = Some(Batch {
            rx,
            progress,
            cancel,
            duration_s: info.frames.map(|f| f as f64 / info.sample_rate as f64),
            label,
            started: Instant::now(),
            files: count,
            first,
            last,
        });
    }

    /// Stop listening or replaying, and any analysis under way. A
    /// microphone session pauses, so it can carry on or be saved.
    fn stop(&mut self) {
        if self.capture.as_ref().is_some_and(|c| !c.is_file) {
            self.pause();
        }
        self.capture = None;
        self.batch = None;
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
            let label = self
                .batch
                .take()
                .map(|b| b.label.clone())
                .unwrap_or_default();
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
        self.steady = None;
        self.steady_job = None;
        self.stretches.clear();
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
            .set_title("Open a Recording")
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

    /// Pick a folder of segments, which is analysed as one recording.
    fn open_folder_dialog(&mut self) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new().set_title("Open a Folder of Segments");
        let here = Path::new(self.file_path.trim());
        let start = if here.is_dir() {
            Some(here)
        } else {
            here.parent()
        };
        if let Some(dir) = start.filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        let p = d.pick_folder()?;
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
            if theme::sidebar_button(ui, self.sidebar)
                .on_hover_text(if self.sidebar {
                    "Hide the sidebar, to give the panes the whole window (Ctrl+B, or Cmd+B \
                     on a Mac)"
                } else {
                    "Show the sidebar (Ctrl+B, or Cmd+B on a Mac)"
                })
                .clicked()
            {
                self.sidebar = !self.sidebar;
            }
            ui.add_space(2.0);
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
                        "Analyse or replay a WAV or FLAC recording",
                    ),
                ],
            );
            ui.add_space(4.0);
            match self.input {
                Input::Microphone => {
                    let choices = self.input_choices();
                    let listed = self.devices_listed;
                    let sel = choices
                        .iter()
                        .find(|c| Some(&c.id) == self.device.as_ref())
                        .map_or_else(
                            || match (&self.device, listed) {
                                // Not listed yet: name the one kept from last time.
                                (Some(d), false) => d.strip_prefix("alsa:").unwrap_or(d).into(),
                                (None, false) => "System Default".to_string(),
                                _ => "Choose a Microphone".to_string(),
                            },
                            |c| c.label.clone(),
                        );
                    let mut opened = false;
                    egui::ComboBox::from_id_salt("device")
                        .selected_text(short(&sel, 36))
                        .width(270.0)
                        .show_ui(ui, |ui| {
                            opened = true;
                            if !listed {
                                ui.weak("Looking for microphones…");
                            }
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
                        self.rescan();
                    }
                    if opened && !listed {
                        self.ensure_devices();
                        ui.ctx().request_repaint();
                    }
                }
                Input::File => {
                    if ui
                        .button("Open…")
                        .on_hover_text("Choose a WAV or FLAC recording and analyse it all")
                        .clicked()
                    {
                        if let Some(p) = self.open_file_dialog() {
                            self.start_batch(&p);
                        }
                    }
                    if ui
                        .button("Open Folder…")
                        .on_hover_text(FOLDER_RULE)
                        .clicked()
                    {
                        if let Some(p) = self.open_folder_dialog() {
                            self.start_batch(&p);
                        }
                    }
                    let w = (ui.available_width() - 200.0).clamp(120.0, 480.0);
                    let r = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.file_path)
                                .hint_text("or type a file's or folder's path and press Enter")
                                .desired_width(w),
                        )
                        .on_hover_text("A WAV or FLAC file, or a folder; Enter analyses it");
                    let path = PathBuf::from(self.file_path.trim());
                    if r.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !self.file_path.trim().is_empty()
                    {
                        self.start_batch(&path);
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match self.input {
                    Input::Microphone => self.microphone_buttons(ui),
                    Input::File => {
                        if self.running() {
                            if ui
                                .add(theme::destructive(ui, "Stop"))
                                .on_hover_text(
                                    "Stop the replay. What it has shown so far stays, to look \
                                     back through with the time slider.",
                                )
                                .clicked()
                            {
                                self.stop();
                            }
                        } else if self.batch.is_some() {
                            if ui
                                .add(theme::secondary(ui, "Cancel"))
                                .on_hover_text("Stop analysing this recording")
                                .clicked()
                            {
                                self.batch = None;
                            }
                        } else {
                            let path = PathBuf::from(self.file_path.trim());
                            let file = path.is_file();
                            if ui
                                .add_enabled(file, theme::secondary(ui, "Replay"))
                                .on_hover_text(
                                    "Play the recording through at its own speed, as if live, \
                                     from the start",
                                )
                                .on_disabled_hover_text(
                                    "Open a recording first. A folder is analysed all at once.",
                                )
                                .clicked()
                            {
                                self.start_replay(&path);
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
                    } else {
                        ui.label(
                            RichText::new("Microphone Off")
                                .small()
                                .color(theme::pal(ui).text_tertiary),
                        )
                        .on_hover_text(
                            "The app isn't listening and has let go of the microphone, so \
                             another program can use it. Start or Resume opens it again.",
                        );
                    }
                }
            });
        });
    }

    /// Start, Stop and Resume, New session and Save, for the microphone.
    /// Laid out right to left.
    fn microphone_buttons(&mut self, ui: &mut egui::Ui) {
        let mic_data = self.mic_session && self.live.is_some();
        if self.running() {
            if ui
                .add(theme::secondary(ui, "Stop"))
                .on_hover_text(
                    "Stop listening and let go of the microphone. Everything so far stays: \
                     look back through it with the time slider, save it, or Resume to carry \
                     on in the same session.",
                )
                .clicked()
            {
                self.pause();
            }
        } else if self.can_resume() {
            if ui
                .add(theme::primary(ui, "Resume"))
                .on_hover_text(
                    "Carry on listening in the same session. The readings start afresh \
                     after the stop, since the watch may have moved; the strip and charts \
                     keep what came before.",
                )
                .clicked()
            {
                self.resume_microphone();
            }
        } else if ui
            .add(theme::primary(ui, "Start"))
            .on_hover_text("Start listening to the microphone")
            .clicked()
        {
            self.start_microphone();
        }
        if mic_data
            && !self.running()
            && ui
                .add(egui::Button::new("New Session").min_size(Vec2::new(0.0, 26.0)))
                .on_hover_text("Clear everything and start again with the next watch or position")
                .clicked()
        {
            if self.unsaved_s() > 5.0 {
                self.confirm = Some(Confirm::NewSession);
            } else {
                self.new_session();
            }
        }
        if self.recorder.is_some() {
            let saved = self.saved_to.is_some();
            if ui
                .add(
                    egui::Button::new(if saved { "Save Again…" } else { "Save…" })
                        .min_size(Vec2::new(0.0, 26.0)),
                )
                .on_hover_text(
                    "Save the sound of this session so far, with its clock log and \
                     settings, to analyse again later. Listening carries on. Unsaved \
                     sound is thrown away by New session or quitting.",
                )
                .clicked()
            {
                self.save_recording();
            }
        }
    }

    /// The question before throwing away unsaved sound.
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(what) = self.confirm else { return };
        let mut choice = None;
        let unsaved = strip::fmt_time(self.unsaved_s());
        egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                RichText::new(match what {
                    Confirm::NewSession => "Start a new session?",
                    Confirm::Quit => "Quit without saving?",
                })
                .font(theme::semibold(15.0)),
            );
            ui.add_space(4.0);
            ui.label(format!(
                "This session's {unsaved} of sound hasn't been saved and will be thrown away."
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary(ui, "Save…")).clicked() {
                    choice = Some(0);
                }
                let go = match what {
                    Confirm::NewSession => "Don't Save",
                    Confirm::Quit => "Quit Without Saving",
                };
                if ui
                    .add(egui::Button::new(go).min_size(Vec2::new(0.0, 26.0)))
                    .clicked()
                {
                    choice = Some(1);
                }
                if ui
                    .add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 26.0)))
                    .clicked()
                {
                    choice = Some(2);
                }
            });
        });
        match choice {
            Some(0) => {
                self.save_recording();
                if self.saved_to.is_none() {
                    return; // the folder dialog was cancelled
                }
            }
            Some(1) => {}
            Some(_) => {
                self.confirm = None;
                return;
            }
            None => return,
        }
        self.confirm = None;
        match what {
            Confirm::NewSession => self.new_session(),
            Confirm::Quit => {
                self.may_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// The sidebar: the watch and the microphone, then a section for each
    /// pane with the switch that shows it, its options and a "?" that
    /// explains it.
    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 8.0;
        self.section(ui, "Watch", None, help::WATCH, Self::watch_settings);
        if self.input == Input::Microphone {
            self.section(
                ui,
                "Microphone",
                None,
                help::MICROPHONE,
                Self::microphone_settings,
            );
        }
        let mut on = self.show_readings;
        self.section(
            ui,
            "Readings",
            Some(&mut on),
            help::READINGS,
            Self::readings_settings,
        );
        self.show_readings = on;
        self.pane_section(ui, Pane::Strip, help::STRIP, Self::strip_settings);
        self.pane_section(ui, Pane::Sound, help::PROFILE, Self::profile_settings);
        self.pane_section(
            ui,
            Pane::Steadiness,
            help::STEADINESS,
            Self::steady_settings,
        );
        self.section(ui, "Charts", None, help::CHARTS, |app, ui| {
            app.pane_switches(ui, &Pane::CHARTS);
            app.span_setting(ui);
        });
        self.section(ui, "Distributions", None, help::HISTOGRAMS, |app, ui| {
            app.pane_switches(ui, &Pane::HISTOGRAMS);
            app.span_setting(ui);
            app.histogram_settings(ui);
        });
        self.section(ui, "Window", None, help::WINDOW, Self::window_settings);
        ui.add_space(8.0);
    }

    /// A card of settings under its caption, which folds it away; `shown`
    /// is the switch for what the card is about, if it has one. True when
    /// that switch flipped.
    fn section(
        &mut self,
        ui: &mut egui::Ui,
        title: &'static str,
        shown: Option<&mut bool>,
        help: &[&str],
        settings: impl FnOnce(&mut Self, &mut egui::Ui),
    ) -> bool {
        let mut folded = self.folded.contains(title);
        let changed = theme::section_header(ui, title, shown, help, &mut folded);
        if folded {
            self.folded.insert(title.to_string());
        } else {
            self.folded.remove(title);
            theme::card_ui(ui, |ui| settings(self, ui));
        }
        changed
    }

    /// A pane's section: its switch in the caption, its settings below.
    fn pane_section(
        &mut self,
        ui: &mut egui::Ui,
        pane: Pane,
        help: &[&str],
        settings: fn(&mut Self, &mut egui::Ui),
    ) {
        let mut on = pane_visible(&self.panes.tiles, pane);
        if self.section(ui, pane.title(), Some(&mut on), help, settings) {
            set_pane_visible(&mut self.panes.tiles, pane, on);
        }
    }

    /// A switch for each of these panes.
    fn pane_switches(&mut self, ui: &mut egui::Ui, panes: &[Pane]) {
        for &pane in panes {
            let mut on = pane_visible(&self.panes.tiles, pane);
            if theme::switch_row(ui, pane.short(), &mut on, pane.hint()) {
                set_pane_visible(&mut self.panes.tiles, pane, on);
            }
        }
    }

    fn steady_settings(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Tested over the session's beats in the watch's position once there \
                     are 5 minutes of them, and again every minute while they grow. Pick \
                     the view at the top of the pane.",
                )
                .small()
                .color(pal.text_secondary),
            )
            .wrap(),
        );
    }

    /// The Steadiness pane: start the tests when there is enough new, show
    /// the latest answer.
    fn steady_pane(&mut self, ui: &mut egui::Ui) {
        if let Some(rx) = &self.steady_job {
            match rx.try_recv() {
                Ok(s) => {
                    self.steady = Some(s);
                    self.steady_job = None;
                }
                Err(TryRecvError::Disconnected) => self.steady_job = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        // Only the beats in the position the watch is in now, every stretch
        // of it joined.
        let spans = self.current_spans();
        let dur = self
            .live
            .as_ref()
            .map_or(0.0, |l| steady::joined_duration(&spans, l.duration_s()));
        let upto = self.steady.as_ref().map(|s| s.upto_s);
        let due = match upto {
            None => true,
            Some(u) if self.running() => dur - u >= steady::RERUN_S,
            Some(u) => dur - u > 1.0,
        };
        if self.steady_job.is_none() && due && dur >= steady::MIN_S {
            let log = self.live.as_ref().and_then(steady::log_of);
            let joined = !self.stretches.is_empty();
            if let Some(log) = log.map(|l| if joined { steady::join(&l, &spans) } else { l }) {
                let wheels = self.calibre_wheels();
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let _ = tx.send(steady::compute(&log, wheels));
                    ctx.request_repaint();
                });
                self.steady_job = Some(rx);
            }
        }
        let working = self.steady_job.is_some();
        let of = if self.stretches.is_empty() {
            String::new()
        } else {
            format!("{} ", self.position.name())
        };
        let status = match upto {
            Some(u) if working => {
                format!("Tested over {} of {of}beats · updating", strip::fmt_time(u))
            }
            Some(u) => format!("Tested over {} of {of}beats", strip::fmt_time(u)),
            None if working => "Testing the session…".to_string(),
            None => format!(
                "The tests need 5 minutes of beats in one position; {} so far",
                strip::fmt_time(dur)
            ),
        };
        steady::draw(ui, self.steady.as_ref(), &status, &mut self.steady_view);
    }

    fn histogram_settings(&mut self, ui: &mut egui::Ui) {
        theme::row(
            ui,
            "Show",
            Some("The values on probability paper, or their cumulative share"),
            |ui| {
                theme::segmented(
                    ui,
                    &mut self.hist_cumulative,
                    &[
                        (
                            false,
                            "Probability",
                            "Each value against its rank on a scale of standard deviations: \
                             one steady state falls on the dashed straight line, two states \
                             draw two lines joined by a bend",
                        ),
                        (
                            true,
                            "Cumulative",
                            "The share of values at or below each value: one state rises in \
                             one steep stretch, two states rise twice with a flatter stretch \
                             between",
                        ),
                    ],
                );
            },
        );
        theme::row(
            ui,
            "Values",
            Some("What the amplitude and beat error distributions count"),
            |ui| {
                theme::segmented(
                    ui,
                    &mut self.hist_readings,
                    &[
                        (
                            true,
                            "Readings",
                            "The readings, each averaged over Average Over, as on the charts \
                             and the strip, one per Average Over so that no two share beats: \
                             states lasting longer than that stand out clearly",
                        ),
                        (
                            false,
                            "2 s",
                            "Each 2 seconds of beats, the finest the app measures: catches \
                             quicker changes, with more scatter",
                        ),
                    ],
                );
            },
        );
    }

    /// Whether the charts and histograms cover the whole session or the
    /// strip's length.
    fn span_setting(&mut self, ui: &mut egui::Ui) {
        theme::row(
            ui,
            "Time Span",
            Some("How much of the session the charts and distributions cover"),
            |ui| {
                theme::segmented(
                    ui,
                    &mut self.span_of_strip,
                    &[
                        (
                            false,
                            "Session",
                            "Everything since the session started, growing as it goes",
                        ),
                        (
                            true,
                            "Strip",
                            "The same stretch of time as the paper strip, so the charts \
                             line up with it and move with it",
                        ),
                    ],
                );
            },
        );
    }

    fn watch_settings(&mut self, ui: &mut egui::Ui) {
        theme::row(ui, "Name", Some("Noted in a saved recording"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.watch)
                    .hint_text("Make, model or calibre")
                    .desired_width(f32::INFINITY),
            );
        });
        self.calibre_settings(ui);
        let mut bph = self.bph;
        theme::row(
            ui,
            "Beat Rate",
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
            "Lift Angle",
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
                "Noted in a saved recording. Changing it while listening marks a new \
                 stretch on the strip and the charts, and the readings, profile, \
                 distributions and Steadiness then use only this position's beats.",
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
            self.set_position(pos);
            if let Some(r) = self.recorder.as_mut() {
                let _ = r.note(&format!("position {}", pos.code()));
            }
            self.note_settings();
        }
    }

    /// Put the watch in a new position. Listening (or paused), the session
    /// carries on with a new stretch: the strip keeps its beats and marks
    /// the stretch, and the readings, the profile, the distributions and the
    /// steadiness tests take only the beats in the new position.
    fn set_position(&mut self, pos: Position) {
        let old = self.position;
        self.position = pos;
        let going = self.capture.is_some() || self.can_resume();
        if let (Some(l), true) = (self.live.as_mut(), going) {
            if self.stretches.is_empty() {
                self.stretches.push((0.0, old));
            }
            l.new_stretch();
            let at = l.duration_s();
            // A position changed back before any beats came is undone.
            if self.stretches.last().is_some_and(|s| s.0 >= at) {
                self.stretches.pop();
            }
            if self.stretches.last().is_none_or(|s| s.1 != pos) {
                self.stretches.push((at, pos));
            }
            self.steady = None;
            self.steady_job = None;
        }
    }

    /// Each stretch of the session in one position: start, end and
    /// position. One stretch over everything while the watch stayed put.
    fn position_spans(&self) -> Vec<(f64, f64, Position)> {
        if self.stretches.is_empty() {
            return vec![(f64::NEG_INFINITY, f64::INFINITY, self.position)];
        }
        let n = self.stretches.len();
        (0..n)
            .map(|i| {
                let (a, p) = self.stretches[i];
                let b = self.stretches.get(i + 1).map_or(f64::INFINITY, |s| s.0);
                (if i == 0 { f64::NEG_INFINITY } else { a }, b, p)
            })
            .collect()
    }

    /// The stretches in the position the watch is in now, which the
    /// profile, the distributions and the steadiness tests are made over.
    fn current_spans(&self) -> Vec<(f64, f64)> {
        self.position_spans()
            .into_iter()
            .filter(|s| s.2 == self.position)
            .map(|s| (s.0, s.1))
            .collect()
    }

    /// Whether a reading ending at `t` comes from the position the watch is
    /// in now.
    fn in_position(&self, t: f64) -> bool {
        self.stretches.is_empty()
            || self
                .position_spans()
                .iter()
                .any(|s| s.2 == self.position && t > s.0 && t <= s.1)
    }

    /// The stretches to mark on the strip and the charts, once there is
    /// more than one: start, end and the position's name.
    fn position_bands(&self) -> Vec<(f64, f64, &'static str)> {
        if self.stretches.len() < 2 {
            return Vec::new();
        }
        self.position_spans()
            .into_iter()
            .map(|(a, b, p)| (a.max(0.0), b, p.name()))
            .collect()
    }

    fn readings_settings(&mut self, ui: &mut egui::Ui) {
        let mut avg = self.average_s;
        let changed = theme::row(
            ui,
            "Average Over",
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
            "Show Every Input",
            &mut self.all_inputs,
            "List every device the system offers in the microphone menu, with its id",
        );
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
            "Rate Line",
            &mut self.rate_line,
            "Draw the Rate reading as a red line over the beats it was fitted to, to \
             check that it follows the dots",
        );
        ui.add_enabled_ui(self.rate_line, |ui| {
            theme::switch_row(
                ui,
                "Parallel Guides",
                &mut self.rate_guides,
                "Faint lines at the rate line's slope across the whole strip, so it is \
                 easy to see whether the dots run parallel to it",
            );
        });
        ui.label(
            RichText::new("Draw over the strip")
                .small()
                .color(theme::pal(ui).text_secondary),
        );
        for (i, (name, hint)) in [
            (
                "Amplitude",
                "The amplitude readings as a purple line against the same time, on their \
                 own scale",
            ),
            (
                "Rate",
                "The rate readings as a red line against the same time, on their own scale",
            ),
            (
                "Beat Error",
                "The beat error readings, from the unlock, as a yellow line against the same \
                 time, on their own scale",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            theme::switch_row(ui, name, &mut self.overlays[i], hint);
        }
        theme::switch_row(
            ui,
            "Auto-Centre",
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

    fn profile_settings(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.sound_opts;
        theme::row(
            ui,
            "Layout",
            Some("Tick above tock, or side by side as tg shows them"),
            |ui| {
                theme::segmented(
                    ui,
                    &mut o.side_by_side,
                    &[
                        (false, "Stacked", "Tick above tock"),
                        (
                            true,
                            "Beside",
                            "Tick on the left, tock on the right, as tg shows them",
                        ),
                    ],
                );
            },
        );
        theme::row(
            ui,
            "Time From",
            Some("Where each side's time is measured from"),
            |ui| {
                theme::segmented(
                    ui,
                    &mut o.shared_clock,
                    &[
                        (
                            true,
                            "Shared",
                            "Tick and tock on one clock: the drops stand the beat error \
                             from the drop apart, and the unlocks the beat error from the unlock",
                        ),
                        (
                            false,
                            "Own Drop",
                            "Each side from its own drop, so both drops sit at 0 ms",
                        ),
                    ],
                );
            },
        );
        theme::row(ui, "Scale", Some("How the loudness is drawn"), |ui| {
            theme::segmented(
                ui,
                &mut o.scale,
                &[
                    (
                        profiles::Scale::Linear,
                        "Linear",
                        "The sound's loudness as the engine measures it",
                    ),
                    (
                        profiles::Scale::Decibels,
                        "dB",
                        "Decibels below the loudest point, which makes the quiet unlock \
                         easier to see",
                    ),
                ],
            );
        });
        theme::switch_row(
            ui,
            "Edges",
            &mut o.edges,
            "The solid unlock, drop and peak lines: the edges amplitude and the \
             beat error are read from",
        );
        theme::switch_row(
            ui,
            "Sounds 1, 2 and 3",
            &mut o.sounds,
            "The dashed brown lines where the three sounds of each beat rise: unlock, \
             impulse and drop",
        );
    }

    fn window_settings(&mut self, ui: &mut egui::Ui) {
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
            self.theme = t;
        }
        ui.horizontal(|ui| {
            if ui
                .button("Reset Panes")
                .on_hover_text("Show every pane and put them back where they started")
                .clicked()
            {
                self.show_readings = true;
                self.panes = default_layout(self.strip.horizontal);
            }
        });
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
        // Before the inputs are listed, the system default's level control:
        // reading a mixer does not open the input.
        let id = match (&self.device, self.devices_listed) {
            (Some(d), _) => Some(d.as_str()),
            (None, false) => Some("default"),
            (None, true) => None,
        };
        self.gain = id.and_then(InputGain::for_device);
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
                theme::row(ui, "Input Level", Some(hint), |ui| {
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
                // Automatic gain only ever spoils the readings, so there is
                // nothing to choose: it shows only when it is on, with the way
                // to turn it off.
                if st.agc == Some(true) {
                    let pal = theme::pal(ui);
                    theme::card()
                        .fill(pal.warn.gamma_multiply(0.15))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(
                                        "The microphone's automatic gain is on. It turns the \
                                         level up between ticks, so they clip and the \
                                         amplitude and level readings go wrong.",
                                    )
                                    .small(),
                                )
                                .wrap(),
                            );
                            if ui.button("Turn It Off").clicked() {
                                self.gain_error = g.set_agc(false).err();
                                self.read_gain();
                            }
                        });
                }
            }
            _ => {
                theme::row(ui, "Input Level", Some(hint), |ui| {
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
    /// The calibre picked, from the built-in table or typed in.
    fn calibre_choice(&self) -> Calibre<'_> {
        if self.calibre.is_empty() {
            return Calibre::None;
        }
        if let Some(i) = self
            .custom_calibres
            .iter()
            .position(|c| c.name == self.calibre)
        {
            return Calibre::Custom(i);
        }
        calibres::find(&self.calibre).map_or(Calibre::None, Calibre::Table)
    }

    /// The train the Steadiness tests name cycles after, when a calibre is
    /// picked; `None` leaves them to the beat rate's usual wheels.
    fn calibre_wheels(&self) -> Option<Vec<Wheel>> {
        match self.calibre_choice() {
            Calibre::None => None,
            Calibre::Table(c) => Some(c.wheels()),
            Calibre::Custom(i) => Some(
                self.custom_calibres[i]
                    .wheels
                    .iter()
                    .filter(|w| w.period_s > 0.0 && !w.name.trim().is_empty())
                    .map(|w| Wheel {
                        name: w.name.trim().to_lowercase(),
                        period_s: w.period_s,
                    })
                    .collect(),
            ),
        }
    }

    /// Pick a calibre: its lift angle, where the table has one, and its
    /// wheels for naming the cycles Steadiness finds.
    fn set_calibre(&mut self, name: String) {
        if name == self.calibre {
            return;
        }
        self.calibre = name;
        if let Calibre::Table(c) = self.calibre_choice() {
            if let Some(l) = c.lift_angle_deg {
                self.set_lift(l);
            }
        }
        // Named afresh with the new wheels.
        self.steady = None;
        self.steady_job = None;
    }

    /// The Calibre row under Watch, and the editor for a typed-in one.
    fn calibre_settings(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let choice = self.calibre_choice();
        let shown = match choice {
            Calibre::None => "Not Set".to_string(),
            Calibre::Table(c) => c.calibre.clone(),
            Calibre::Custom(i) => self.custom_calibres[i].name.clone(),
        };
        let mut picked: Option<String> = None;
        let mut new_custom = false;
        theme::row(
            ui,
            "Calibre",
            Some(
                "The movement, so that cycles Steadiness finds are named after the right \
                 wheels and the lift angle is filled in. Pick New Custom Calibre for one \
                 that isn't listed, and type its wheels' periods.",
            ),
            |ui| {
                egui::ComboBox::from_id_salt("calibre")
                    .width(ui.available_width())
                    .height(420.0)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(matches!(choice, Calibre::None), "Not Set")
                            .on_hover_text(
                                "Name cycles after the wheels most calibres at this beat rate \
                                 share",
                            )
                            .clicked()
                        {
                            picked = Some(String::new());
                        }
                        let mut table: Vec<&calibres::Calibre> = calibres::all().iter().collect();
                        table.sort_by(|a, b| (&a.maker, &a.calibre).cmp(&(&b.maker, &b.calibre)));
                        let mut maker = "";
                        for c in table {
                            if c.maker != maker {
                                maker = &c.maker;
                                ui.separator();
                                ui.label(RichText::new(maker).small().color(pal.text_tertiary));
                            }
                            let on = matches!(choice, Calibre::Table(x) if x.calibre == c.calibre);
                            if ui
                                .selectable_label(on, &c.calibre)
                                .on_hover_text(calibre_hint(c))
                                .clicked()
                            {
                                picked = Some(c.calibre.clone());
                            }
                        }
                        ui.separator();
                        ui.label(
                            RichText::new("Your Calibres")
                                .small()
                                .color(pal.text_tertiary),
                        );
                        for (i, c) in self.custom_calibres.iter().enumerate() {
                            let on = matches!(choice, Calibre::Custom(x) if x == i);
                            if ui.selectable_label(on, &c.name).clicked() {
                                picked = Some(c.name.clone());
                            }
                        }
                        if ui
                            .selectable_label(false, "New Custom Calibre…")
                            .on_hover_text(
                                "Type the turn period of each wheel of a calibre that isn't \
                                 listed; it is kept for next time",
                            )
                            .clicked()
                        {
                            new_custom = true;
                        }
                    });
            },
        );
        if new_custom {
            let mut n = self.custom_calibres.len() + 1;
            let mut name = format!("My Calibre {n}");
            while self.custom_calibres.iter().any(|c| c.name == name) {
                n += 1;
                name = format!("My Calibre {n}");
            }
            self.custom_calibres.push(CustomCalibre::new(name.clone()));
            picked = Some(name);
        }
        if let Some(p) = picked {
            self.set_calibre(p);
        }
        match self.calibre_choice() {
            Calibre::Table(c) => {
                let heard = self.live.as_ref().and_then(|l| l.bph());
                if let Some(b) = heard.filter(|&b| b != c.bph) {
                    ui.label(
                        RichText::new(format!(
                            "The {} beats at {} bph, but this watch beats at {b}.",
                            c.calibre, c.bph
                        ))
                        .small()
                        .color(pal.warn),
                    );
                }
            }
            Calibre::Custom(i) => self.custom_calibre_editor(ui, i),
            Calibre::None => {}
        }
    }

    /// Name and wheels of a typed-in calibre, edited in place.
    fn custom_calibre_editor(&mut self, ui: &mut egui::Ui, i: usize) {
        let pal = theme::pal(ui);
        let before = self.custom_calibres[i].clone();
        let mut delete = false;
        ui.add_space(2.0);
        let c = &mut self.custom_calibres[i];
        theme::row(
            ui,
            "Calibre Name",
            Some("What the calibre is called in the menu"),
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(f32::INFINITY));
            },
        );
        let mut remove = None;
        for (j, w) in c.wheels.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut w.name)
                        .hint_text("wheel")
                        .desired_width(theme::LABEL_W - 8.0),
                )
                .on_hover_text("The wheel's name, as Steadiness labels it");
                fields::entry(
                    ui,
                    &format!("wheel-{i}-{j}"),
                    &mut w.period_s,
                    0.01..=1.0e6,
                    &PERIOD,
                    60.0,
                );
                ui.label("s");
                if ui
                    .small_button("Remove")
                    .on_hover_text("Remove this wheel")
                    .clicked()
                {
                    remove = Some(j);
                }
            })
            .response
            .on_hover_text("The time the wheel takes to turn once, in seconds");
        }
        if let Some(j) = remove {
            c.wheels.remove(j);
        }
        ui.horizontal(|ui| {
            if ui
                .button("Add Wheel")
                .on_hover_text("Another wheel, such as the third wheel or the barrel")
                .clicked()
            {
                c.wheels.push(CustomWheel {
                    name: String::new(),
                    period_s: 60.0,
                });
            }
            if ui
                .button("Delete Calibre")
                .on_hover_text("Forget this calibre")
                .clicked()
            {
                delete = true;
            }
        });
        ui.label(
            RichText::new(
                "A wheel's period is one full turn: 60 s for a fourth wheel carrying the \
                 seconds hand, 3600 s for the centre wheel.",
            )
            .small()
            .color(pal.text_tertiary),
        );
        let renamed = self.custom_calibres[i].name != before.name;
        if renamed {
            self.calibre = self.custom_calibres[i].name.clone();
        }
        if delete {
            self.custom_calibres.remove(i);
            self.set_calibre(String::new());
        } else if self.custom_calibres[i].wheels != before.wheels {
            self.steady = None;
            self.steady_job = None;
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
        // The cards share one height, the tallest's on the last frame, so a
        // card with an extra line doesn't leave the row ragged.
        let height_id = ui.id().with("readout height");
        let height = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(height_id))
            .unwrap_or(0.0);
        let tallest = std::cell::Cell::new(0f32);
        let card = |ui: &mut egui::Ui, title: &str, help: &[&str], body: &dyn Fn(&mut egui::Ui)| {
            theme::card()
                .fill(pal.card)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    // Before anything is laid out: the minimum is taken from
                    // the cursor down.
                    ui.set_min_height(height);
                    let top = ui.min_rect().top();
                    ui.horizontal(|ui| {
                        ui.label(theme::caption(title).color(pal.text_secondary));
                        theme::help(ui, title, help);
                    });
                    body(ui);
                    let bottom = ui.cursor().top() - ui.spacing().item_spacing.y;
                    tallest.set(tallest.get().max(bottom - top));
                });
        };
        ui.spacing_mut().item_spacing.x = theme::GAP;
        ui.columns(3, |cols| {
            let rate = r.and_then(|r| r.rate_s_per_day);
            card(&mut cols[0], "Rate", help::RATE, &|ui| {
                // Roughly how far the rate could be off from the beats'
                // scatter alone: the standard error of the fitted slope.
                let pm = r.and_then(|r| {
                    let j = r.jitter_us? * 1e-6;
                    let n = r.beats_used as f64;
                    (n >= 6.0 && r.span_s > 1.0)
                        .then(|| j * 12f64.sqrt() / (n.sqrt() * r.span_s) * 86400.0)
                });
                let unit = match pm {
                    Some(e) if e < 0.05 => "seconds per day".to_string(),
                    Some(e) => format!("± {e:.1} seconds per day"),
                    None => "seconds per day".to_string(),
                };
                figure(ui, rate.map(|v| format!("{v:+.1}")), &unit);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let small = |t: String| RichText::new(t).small().color(pal.text_secondary);
                    let Some(r) = r else {
                        ui.label(small("waiting for beats".into()));
                        return;
                    };
                    let short = r.span_s + 1.0 < self.average_s;
                    ui.label(small(if short {
                        format!(
                            "{} beats in {} of {}",
                            r.beats_used,
                            fields::duration(r.span_s.round()),
                            fields::duration(self.average_s)
                        )
                    } else {
                        format!(
                            "{} beats in {}",
                            r.beats_used,
                            fields::duration(self.average_s)
                        )
                    }))
                    .on_hover_text(if short {
                        "The beats the readings are fitted to. There are fewer seconds of \
                             beats than Average over asks for, early in a session or after a \
                             pause, so the readings use what there is."
                    } else {
                        "The beats the readings are fitted to. Set the time with Average \
                             over in the sidebar."
                    });
                    if let Some(j) = r.jitter_us {
                        ui.label(small("·".into()));
                        ui.label(small(format!("jitter {j:.0} µs"))).on_hover_text(
                            "How far single beats land from the rate's straight line: the \
                                 spread of the dots across the strip. The ? beside Rate says more.",
                        );
                    }
                });
                if let (Some(rate), Some(c)) = (rate, &self.clock) {
                    let v = format!("{:+.1}", c.correct_rate(rate)).replace('-', "−");
                    let slow = if c.ppm >= 0.0 { "slow" } else { "fast" };
                    ui.label(
                        RichText::new(format!("True Clock {v} seconds per day"))
                            .small()
                            .color(pal.text_secondary),
                    )
                    .on_hover_text(format!(
                        "The rate corrected for this input's own clock, which runs {:.1} ppm \
                         {slow} ({:+.1} s/d on every rate), measured {} from {}. The big \
                         figure stays on the card's clock, the one tg and an analysis of \
                         the recording read, so the two compare directly.",
                        c.ppm.abs(),
                        c.rate_error_s_per_day(),
                        c.measured_utc,
                        match c.source.as_str() {
                            "log" => "a recording's clock log",
                            "measure" => "listening against the system clock",
                            _ => "a value typed in",
                        }
                    ));
                }
            });
            let amp = r.and_then(|r| r.amplitude_deg);
            let lift = fields::plain(self.lift_deg);
            card(&mut cols[1], "Amplitude", help::AMPLITUDE, &|ui| {
                // Whole degrees: a live reading's ± is about a degree, so a
                // decimal would claim more than the beats can say.
                let unit = match r.and_then(|r| r.amplitude_error_deg) {
                    Some(e) if e < 0.95 => format!("± {e:.1}°"),
                    Some(e) => format!("± {e:.0}°"),
                    None => String::new(),
                };
                figure(ui, amp.map(|v| format!("{v:.0}°")), &unit);
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
            });
            let unlock = r.and_then(|r| r.beat_error_unlock_ms);
            let drop = r.and_then(|r| r.beat_error_ms);
            card(&mut cols[2], "Beat Error", help::BEAT_ERROR, &|ui| {
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
            });
        });
        if tallest.get() != height {
            ui.ctx()
                .data_mut(|d| d.insert_temp(height_id, tallest.get()));
            ui.ctx().request_repaint();
        }
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
                    let (word, color) = if s >= 10.0 {
                        ("good", pal.good)
                    } else if s >= 5.0 {
                        ("fair: rate reliable", pal.warn)
                    } else if s >= 3.0 {
                        ("poor: only the rate", pal.bad)
                    } else {
                        ("no watch heard", pal.bad)
                    };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        theme::dot(ui, color);
                        ui.label(
                            RichText::new(format!("signal {s:.0}×, {word}"))
                                .small()
                                .color(pal.text_secondary),
                        );
                    })
                    .response
                    .on_hover_text(help::SIGNAL);
                }
            }
            if let Some(r) = &self.recorder {
                let (text, color, hint) = match &self.saved_to {
                    Some(p) => (
                        format!("{} of sound, saved", strip::fmt_time(r.duration_s())),
                        pal.text_secondary,
                        format!(
                            "Saved in {}. Sound since then is kept too; Save again to add it.",
                            p.display()
                        ),
                    ),
                    None => (
                        format!("{} of sound, not saved", strip::fmt_time(r.duration_s())),
                        pal.text_secondary,
                        "The session's sound is kept as it goes, so Save can keep it at any \
                         point. New session or quitting throws it away unless saved."
                            .to_string(),
                    ),
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    theme::dot(
                        ui,
                        if self.running() {
                            pal.bad
                        } else {
                            pal.text_tertiary
                        },
                    );
                    ui.label(RichText::new(text).small().color(color));
                })
                .response
                .on_hover_text(hint);
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
        let shown: Vec<(Pane, bool)> = Pane::ALL
            .into_iter()
            .map(|p| (p, pane_visible(&self.panes.tiles, p)))
            .collect();
        self.panes = default_layout(horizontal);
        for (p, on) in shown {
            set_pane_visible(&mut self.panes.tiles, p, on);
        }
    }

    /// What shows before there is anything to show: how to begin, and the
    /// two ways to.
    /// Before anything is open: the three ways in. While a recording is
    /// being analysed, how far it has got.
    fn empty_state(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.add_space((ui.available_height() * 0.16).clamp(16.0, 110.0));
        ui.vertical_centered(|ui| {
            let w = 520.0_f32.min(ui.available_width());
            ui.allocate_ui(Vec2::new(w, 0.0), |ui| {
                theme::card().fill(pal.card).show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        if self.batch.is_some() {
                            self.batch_progress(ui);
                        } else {
                            self.ways_in(ui);
                        }
                    });
                });
            });
        });
    }

    fn ways_in(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.label(RichText::new("Timegrapher").font(theme::semibold(20.0)));
        let row = |ui: &mut egui::Ui, button: egui::Button<'static>, hint: &str, text: &str| {
            ui.horizontal(|ui| {
                let b = ui
                    .add_sized(Vec2::new(178.0, 30.0), button)
                    .on_hover_text(hint);
                ui.add(
                    egui::Label::new(RichText::new(text).small().color(pal.text_secondary)).wrap(),
                );
                b.clicked()
            })
            .inner
        };
        if row(
            ui,
            theme::primary(ui, "Listen to a Watch"),
            "Start listening to the microphone chosen at the top",
            "Put the watch on the microphone, dial up. Choose the microphone at the top \
             and set its level in the sidebar.",
        ) {
            self.input = Input::Microphone;
            self.start_microphone();
        }
        if row(
            ui,
            theme::secondary(ui, "Open a Recording…"),
            "Choose a WAV or FLAC file and analyse it all at once",
            "One WAV or FLAC file, analysed all at once.",
        ) {
            self.input = Input::File;
            if let Some(p) = self.open_file_dialog() {
                self.start_batch(&p);
            }
        }
        if row(
            ui,
            theme::secondary(ui, "Open a Folder…"),
            "Choose a folder saved by this app, or a folder of WAV or FLAC segments of \
             one long take",
            FOLDER_RULE,
        ) {
            self.input = Input::File;
            if let Some(p) = self.open_folder_dialog() {
                self.start_batch(&p);
            }
        }
    }

    /// How far the analysis of a recording has got, and a way to stop it.
    fn batch_progress(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let Some(b) = &self.batch else { return };
        let done = f64::from_bits(b.progress.load(std::sync::atomic::Ordering::Relaxed));
        ui.label(RichText::new(format!("Analysing {}", b.label)).font(theme::semibold(18.0)));
        let frac = b.duration_s.map(|d| (done / d.max(1e-9)).clamp(0.0, 1.0));
        ui.add(
            egui::ProgressBar::new(frac.unwrap_or(0.0) as f32)
                .desired_width(ui.available_width())
                .desired_height(16.0)
                .fill(pal.accent)
                .text(RichText::new(match frac {
                    Some(f) => format!("{:.0}%", 100.0 * f),
                    None => String::new(),
                })),
        );
        let elapsed = b.started.elapsed().as_secs_f64();
        let mut line = match b.duration_s {
            Some(d) => format!(
                "{} of {} of sound analysed",
                strip::fmt_time(done),
                strip::fmt_time(d)
            ),
            None => format!("{} of sound analysed", strip::fmt_time(done)),
        };
        // Time left at the pace so far, once there is a pace to go by.
        if let (Some(d), true) = (b.duration_s, done > 0.0 && elapsed > 3.0) {
            let left = (d - done).max(0.0) * elapsed / done;
            line += &format!(" · about {} to go", strip::fmt_time(left));
        }
        ui.label(RichText::new(line).color(pal.text_secondary));
        if b.files > 1 {
            ui.label(
                RichText::new(format!(
                    "{} files joined in name order, {} to {}.",
                    b.files, b.first, b.last
                ))
                .small()
                .color(pal.text_tertiary),
            );
        }
        if ui
            .add(theme::secondary(ui, "Cancel"))
            .on_hover_text("Stop analysing this recording")
            .clicked()
        {
            self.batch = None;
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }

    /// A problem, where it can't be missed, with a way to put it away.
    fn error_banner(&mut self, ui: &mut egui::Ui) {
        let Some((m, true)) = self.message.clone() else {
            return;
        };
        let pal = theme::pal(ui);
        let mut dismiss = false;
        banner(ui, pal.bad, |ui| {
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
        if dismiss {
            self.message = None;
        }
        ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
    }

    /// A warning over the readings while the sound they come from clips.
    fn clipping_banner(&self, ui: &mut egui::Ui) {
        let Some(r) = self.reading().filter(|r| r.clipping_warns()) else {
            return;
        };
        let pal = theme::pal(ui);
        let pct = r.clipped_fraction.unwrap_or(0.0) * 100.0;
        let what = if self.mic_session {
            "Lower Input Level in the Microphone card."
        } else {
            "Record with the input level lower."
        };
        let resp = banner(ui, pal.warn, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "Clipping on {pct:.0}% of beats. {what} Amplitude and Beat Error read \
                         wrong while the sound clips; Rate is unaffected."
                    ))
                    .color(pal.text),
                )
                .wrap(),
            );
        });
        resp.on_hover_text(help::CLIPPING_BANNER.join("\n\n"));
        ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
    }

    fn main_view(&mut self, ui: &mut egui::Ui) {
        self.error_banner(ui);
        if self.show_readings {
            self.clipping_banner(ui);
        }
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
                    RichText::new(
                        "Every pane is hidden. Turn one on with its switch in the sidebar.",
                    )
                    .size(15.0)
                    .weak(),
                );
            });
            return;
        }
        self.charts_to_live_now = std::mem::take(&mut self.charts_to_live);
        let mut panes = std::mem::replace(&mut self.panes, Tree::empty("panes-swap"));
        repair_tabs(&mut panes);
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
        let rate_line = self
            .rate_line
            .then(|| self.reading())
            .flatten()
            .and_then(|r| {
                Some(strip::RateLine {
                    from_s: r.time_s - r.span_s,
                    to_s: r.time_s,
                    rate_s_per_day: r.rate_s_per_day?,
                })
            });
        let pal = theme::pal(ui);
        let from = end - self.strip.span_s - 5.0;
        let series = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<[f64; 2]> {
            self.trend
                .iter()
                .filter(|p| p.t >= from && p.t <= end)
                .filter_map(|p| f(p).map(|v| [p.t, v]))
                .collect()
        };
        let mut overlays = Vec::new();
        if self.overlays[0] {
            overlays.push(strip::Overlay {
                name: "Amplitude",
                unit: "°",
                decimals: 0,
                color: pal.trace_amplitude,
                min_span: 10.0,
                points: series(|p| p.amplitude),
            });
        }
        if self.overlays[1] {
            overlays.push(strip::Overlay {
                name: "Rate",
                unit: " s/d",
                decimals: 1,
                color: pal.trace_rate,
                min_span: 4.0,
                points: series(|p| p.rate),
            });
        }
        if self.overlays[2] {
            overlays.push(strip::Overlay {
                name: "Beat Error",
                unit: " ms",
                decimals: 2,
                color: pal.trace_beat_error,
                min_span: 0.2,
                points: series(|p| p.beat_error_unlock),
            });
        }
        let bands = self.position_bands();
        let extras = strip::Extras {
            note: note.as_deref(),
            rate_line,
            guides: self.rate_guides,
            overlays: &overlays,
            cursor_t: self.cursor_t,
            gutter: GUTTER,
            bands: &bands,
        };
        let Some(live) = &self.live else { return };
        let input = strip::draw_strip(
            ui,
            live.beats(),
            period,
            self.anchor,
            end,
            &self.strip,
            &extras,
            ui.available_size(),
        );
        if input.hover_t.is_some() {
            self.cursor_next = input.hover_t;
        }
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
        let i = self.trend.partition_point(|p| p.t <= end + 1e-9);
        let drop_be = i.checked_sub(1).and_then(|i| self.trend[i].beat_error_drop);
        profiles::draw(ui, &shown, self.sound_opts, drop_be, note.as_deref());
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

    /// The stretch of time the charts and histograms cover: the strip's
    /// length, ending where the strip does, or `None` for the whole session.
    fn chart_window(&self) -> Option<(f64, f64)> {
        self.span_of_strip.then(|| {
            let end = self.end_s();
            (end - self.strip.span_s, end)
        })
    }

    /// How often each value of a reading came up, over the session or the
    /// strip's length: bars with a smooth density curve over them, or the
    /// cumulative share.
    fn histogram(&mut self, ui: &mut egui::Ui, pane: Pane) {
        use timegrapher_core::histogram as hist;
        let pal = theme::pal(ui);
        let window = self.chart_window();
        // Only the position the watch is in now: values from another
        // position are a different measurement.
        let inside = |t: f64| window.is_none_or(|(a, b)| t >= a && t <= b) && self.in_position(t);
        let windows = self
            .live
            .as_ref()
            .map_or(&[][..], |l| l.amplitude_windows());
        // One reading per averaging time: a new reading comes every half
        // second, and the ones between share most of their beats, so counting
        // them all would stack up repeats and look surer than it is.
        let every = self.average_s;
        let readings = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<f64> {
            let mut last = f64::NEG_INFINITY;
            self.trend
                .iter()
                .filter(|p| inside(p.t))
                .filter_map(|p| Some((p.t, f(p)?)))
                .filter(|&(t, _)| {
                    let keep = t >= last + every - 1e-6;
                    if keep {
                        last = t;
                    }
                    keep
                })
                .map(|(_, v)| v)
                .collect()
        };
        let short = !self.hist_readings;
        let (values, resolution, unit, decimals, color, per): (
            Vec<f64>,
            f64,
            &str,
            usize,
            _,
            &str,
        ) = match pane {
            Pane::RateHistogram => (
                readings(|p| p.rate),
                0.1,
                " s/day",
                1,
                pal.trace_rate,
                "readings",
            ),
            Pane::AmplitudeHistogram if short => (
                windows
                    .iter()
                    .filter(|w| inside(w.end_s) && self.in_position(w.start_s + 1e-6))
                    .filter_map(|w| w.mean())
                    .collect(),
                0.5,
                "°",
                0,
                pal.trace_amplitude,
                "2-second stretches",
            ),
            Pane::AmplitudeHistogram => (
                readings(|p| p.amplitude),
                0.5,
                "°",
                0,
                pal.trace_amplitude,
                "readings",
            ),
            Pane::BeatErrorHistogram if short => (
                windows
                    .iter()
                    .filter(|w| inside(w.end_s) && self.in_position(w.start_s + 1e-6))
                    .filter_map(|w| w.beat_error_unlock_ms)
                    .collect(),
                0.01,
                " ms",
                2,
                pal.trace_beat_error,
                "2-second stretches",
            ),
            Pane::BeatErrorHistogram => (
                readings(|p| p.beat_error_unlock),
                0.01,
                " ms",
                2,
                pal.trace_beat_error,
                "readings",
            ),
            _ => return,
        };
        let opt = hist::Options {
            resolution,
            width_factor: 1.0,
            trim: 0.005,
        };
        let Some(h) = hist::histogram_with(&values, &opt) else {
            ui.label(
                RichText::new("Not enough readings yet")
                    .small()
                    .color(pal.text_secondary),
            );
            return;
        };
        let peaks = h.peaks(0.05);
        let f = |v: f64| format!("{v:.decimals$}{unit}");
        let mut line = format!(
            "{} {per} · median {} · middle 80% {} to {}",
            h.n,
            f(h.median),
            f(h.p10),
            f(h.p90),
        );
        if !self.stretches.is_empty() {
            line += &format!(" · {} only", self.position.name());
        }
        if h.below + h.above > 0 {
            line += &format!(" · {} outliers off the scale", h.below + h.above);
        }
        if peaks.len() > 1 {
            let at: Vec<String> = peaks.iter().map(|&i| f(h.centre(i))).collect();
            line += &format!(" · {} peaks, at {}", peaks.len(), at.join(" and "));
        }
        ui.add(egui::Label::new(RichText::new(&line).small().color(pal.text_secondary)).truncate())
            .on_hover_text(line.clone());
        let (x_lo, x_hi) = (h.start, h.start + h.counts.len() as f64 * h.bin_width);
        let unit_owned = unit.to_string();
        let cumulative = self.hist_cumulative;
        let plot = Plot::new(pane.title())
            .allow_scroll(false)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_double_click_reset(false)
            .show_x(false)
            .show_y(false)
            .x_grid_spacer(|g| theme::even_grid(g, 80.0, &[]))
            .x_axis_formatter(move |m, _| {
                let d = if m.step_size >= 1.0 {
                    0
                } else if m.step_size >= 0.1 {
                    1
                } else {
                    2
                };
                // No "-0" from rounding.
                let v = if m.value.abs() < m.step_size * 1e-6 {
                    0.0
                } else {
                    m.value
                };
                format!("{:.*}{unit_owned}", d, v)
            })
            .y_axis_min_width(44.0)
            .default_x_bounds(x_lo, x_hi);
        if cumulative {
            // The share of values at or below each value, as a step line.
            let mut pts: Vec<[f64; 2]> = Vec::new();
            for [x, q] in hist::ecdf(&values) {
                if x < x_lo || x > x_hi {
                    continue;
                }
                if let Some(&[_, prev]) = pts.last() {
                    pts.push([x, prev]);
                }
                pts.push([x, q * 100.0]);
            }
            plot.y_grid_spacer(|g| theme::even_grid(g, 30.0, &[]))
                .y_axis_formatter(|m, _| {
                    if m.value < -1e-9 || m.value > 100.0 + 1e-9 {
                        String::new()
                    } else {
                        format!("{}%", m.value.abs())
                    }
                })
                .default_y_bounds(0.0, 100.0)
                .show(ui, |p| {
                    p.line(
                        Line::new("share at or below", pts)
                            .color(color)
                            .width(1.8_f32),
                    );
                });
            return;
        }
        // Probability paper: the share at or below each value on a scale of
        // standard deviations, labelled in percent.
        let pts: Vec<[f64; 2]> = hist::probability_plot(&values)
            .into_iter()
            .filter(|p| p[0] >= x_lo && p[0] <= x_hi)
            .collect();
        let z_top = hist::normal_quantile(1.0 - 0.5 / values.len().max(2) as f64).min(3.3) + 0.25;
        let line = hist::normal_line(&values)
            .map(|(mid, sd)| vec![[mid - sd * z_top, -z_top], [mid + sd * z_top, z_top]]);
        plot.y_grid_spacer(move |g| {
            // From 50% outwards, leaving out a share whose label would crowd
            // the one before it.
            let px = 8.0 / g.base_step_size;
            let mut kept: Vec<f64> = Vec::new();
            for &(pc, _) in &PROBABILITY_MARKS[5..] {
                let z = hist::normal_quantile(pc / 100.0);
                if z <= z_top && kept.last().is_none_or(|&k| (z - k) * px >= 16.0) {
                    kept.push(z);
                }
            }
            kept.iter()
                .flat_map(|&z| if z == 0.0 { vec![z] } else { vec![z, -z] })
                .map(|value| GridMark {
                    value,
                    step_size: 1.0,
                })
                .collect()
        })
        .y_axis_formatter(|m, _| {
            PROBABILITY_MARKS
                .iter()
                .find(|&&(pc, _)| (hist::normal_quantile(pc / 100.0) - m.value).abs() < 1e-6)
                .map_or(String::new(), |&(_, label)| label.to_string())
        })
        .default_y_bounds(-z_top, z_top)
        .show(ui, |p| {
            if let Some(l) = line {
                p.line(
                    Line::new("one steady state", l)
                        .color(pal.text_tertiary)
                        .style(LineStyle::dashed_loose())
                        .width(1.0_f32),
                );
            }
            p.points(
                Points::new("values", pts)
                    .color(color)
                    .radius(1.6_f32)
                    .filled(true),
            );
        });
    }

    fn chart(&mut self, ui: &mut egui::Ui, pane: Pane) {
        let pal = theme::pal(ui);
        let weak = pal.text_tertiary;
        let pick = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<[f64; 2]> {
            self.trend
                .iter()
                .filter_map(|p| f(p).map(|v| [p.t, v]))
                .collect()
        };
        // Each line in its reading's colour, as on the strip and in the
        // distributions. Tick's amplitude is purple dashed and Tock's purple
        // dotted, with blue and orange swatches in the box at the pointer;
        // beat error from the drop is a fainter dashed yellow.
        let solid = LineStyle::Solid;
        let (series, unit, decimals): (Vec<ChartLine>, &'static str, usize) = match pane {
            Pane::Rate => (
                vec![ChartLine {
                    name: format!(
                        "Rate in seconds per day, {} average",
                        fields::duration(self.average_s)
                    ),
                    pts: pick(|p| p.rate),
                    color: pal.trace_rate,
                    style: solid,
                    key: pal.trace_rate,
                    main: true,
                }],
                " s/day",
                1,
            ),
            Pane::Amplitude => (
                vec![
                    ChartLine {
                        name: "Average Amplitude".into(),
                        pts: pick(|p| p.amplitude),
                        color: pal.trace_amplitude,
                        style: solid,
                        key: pal.trace_amplitude,
                        main: true,
                    },
                    ChartLine {
                        name: "Amplitude from Tick".into(),
                        pts: pick(|p| p.amplitude_a),
                        color: pal.trace_amplitude,
                        style: LineStyle::dashed_dense(),
                        key: pal.tick,
                        main: false,
                    },
                    ChartLine {
                        name: "Amplitude from Tock".into(),
                        pts: pick(|p| p.amplitude_b),
                        color: pal.trace_amplitude,
                        style: LineStyle::dotted_dense(),
                        key: pal.tock,
                        main: false,
                    },
                ],
                "°",
                0,
            ),
            Pane::BeatError => (
                vec![
                    ChartLine {
                        name: "From the Unlock".into(),
                        pts: pick(|p| p.beat_error_unlock),
                        color: pal.trace_beat_error,
                        style: solid,
                        key: pal.trace_beat_error,
                        main: true,
                    },
                    ChartLine {
                        name: "From the Drop".into(),
                        pts: pick(|p| p.beat_error_drop),
                        color: pal.trace_beat_error.gamma_multiply(0.6),
                        style: LineStyle::dashed_dense(),
                        key: pal.trace_beat_error.gamma_multiply(0.6),
                        main: false,
                    },
                ],
                " ms",
                2,
            ),
            _ => return,
        };
        let mut plot = Plot::new(pane.title())
            .link_axis("trend", [true, false])
            .link_cursor("trend", [true, false])
            .allow_scroll(false)
            .allow_zoom([false, true])
            .x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
            .custom_x_axes(vec![
                egui_plot::AxisHints::new_x().formatter(|m, _| strip::fmt_time(m.value))
            ])
            .y_grid_spacer(|g| theme::even_grid(g, 30.0, &[]))
            .y_axis_min_width(GUTTER)
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
        let window = self.chart_window();
        if window.is_some() && self.strip.horizontal {
            // Leave the room the strip keeps for its overlays' scales, so the
            // time axes end together too.
            let n = self.overlays.iter().filter(|&&o| o).count() as f32;
            let right = 4.0 + 58.0 * n;
            ui.set_max_width((ui.available_width() - right).max(100.0));
        }
        let all: Vec<[f64; 2]> = series
            .iter()
            .flat_map(|s| s.pts.iter().copied())
            .filter(|p| window.is_none_or(|(a, b)| p[0] >= a && p[0] <= b))
            .collect();
        let y_range = percentile_range(&all);
        if let Some((lo, hi)) = y_range {
            plot = plot.default_y_bounds(lo, hi);
        }
        if window.is_some() {
            // The strip sets the time: only the vertical scale moves.
            plot = plot
                .allow_drag([false, true])
                .allow_double_click_reset(false);
        }
        let marker = (self.view_end.is_some() || !self.running()).then(|| self.end_s());
        let lookup = series.clone();
        let to_live = self.charts_to_live_now;
        let resp = plot.show(ui, |p| {
            if let Some((a, b)) = window {
                p.set_plot_bounds_x(a..=b);
                if let (Some((lo, hi)), true) = (y_range, to_live) {
                    p.set_plot_bounds_y(lo..=hi);
                }
            } else if to_live {
                p.set_auto_bounds([true, true]);
            }
            if p.response().hovered() && window.is_none() {
                let s = p.ctx().input(|i| i.smooth_scroll_delta);
                let wheel = s.x + s.y;
                if wheel != 0.0 {
                    p.zoom_bounds_around_hovered(Vec2::new((wheel * 0.003).exp(), 1.0));
                }
            }
            for l in series {
                let w = if l.main { 1.8_f32 } else { 1.2_f32 };
                p.line(
                    Line::new(l.name, l.pts)
                        .color(l.color)
                        .style(l.style)
                        .width(w),
                );
            }
            let (hovered, clicked) = {
                let r = p.response();
                (r.hovered(), r.clicked() && !r.double_clicked())
            };
            let hover = p.pointer_coordinate().filter(|_| hovered);
            let click = clicked.then(|| p.pointer_coordinate()).flatten();
            // Panned or zoomed away from the whole session.
            let moved = window.is_none() && !to_live && !p.auto_bounds().x;
            (click, hover.map(|h| h.x), moved)
        });
        let (click, hover, moved) = resp.inner;
        // The moment shown and the pointer's time, painted over the plot
        // rather than added to it: a plot line counts towards the automatic
        // bounds, so one under the pointer near the edge widened the chart
        // a little more every frame.
        let frame = *resp.transform.frame();
        let painter = ui.painter_at(frame);
        let t = resp.transform;
        strip::paint_bands(
            &painter,
            frame,
            &self.position_bands(),
            |x| egui::pos2(t.position_from_point_x(x), frame.top()),
            true,
            ui.visuals(),
        );
        if hover.is_some() {
            self.cursor_next = hover;
        }
        let cursor = hover.or(self.cursor_t);
        for x in [marker, cursor].into_iter().flatten() {
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
                "Follow Live"
            } else {
                "Show Whole Session"
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
                for ChartLine { name, pts, key, .. } in &lookup {
                    let v = value_at(pts, x);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("■").color(*key));
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
            Pane::Steadiness => self.app.steady_pane(ui),
            p if Pane::HISTOGRAMS.contains(p) => self.app.histogram(ui, *p),
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

/// The recordings to analyse for `path`: the file itself, or a folder's
/// WAV and FLAC files in name order (the segments of one long take).
/// How the files in a folder are put together, said where a folder is
/// opened. The command line's `long` and `series` read folders the same way.
const FOLDER_RULE: &str = "A take this app saved, or a folder of WAV or FLAC segments of one \
     long run, joined end to end in file-name order (audio-001.wav, audio-002.wav, …).";

fn recordings_in(path: &Path) -> Result<Vec<PathBuf>, String> {
    if !path.is_dir() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| format!("Can't read the folder {}: {e}", path.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    e.eq_ignore_ascii_case("wav") || e.eq_ignore_ascii_case("flac")
                })
        })
        .collect();
    if files.is_empty() {
        return Err(format!(
            "There are no WAV or FLAC files in {}.",
            path.display()
        ));
    }
    files.sort();
    Ok(files)
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
        let toggle = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::B);
        if ctx.input_mut(|i| i.consume_shortcut(&toggle)) {
            self.sidebar = !self.sidebar;
        }
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
            .show_animated(ctx, self.sidebar, |ui| {
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
            self.start_batch(&p);
        }

        self.poll();

        // Closing the window with unsaved sound asks first.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.may_close
            && self.unsaved_s() > 5.0
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::Quit);
        }
        self.panels(ctx);
        self.confirm_dialog(ctx);
        // The cursor shared by the strip and the charts follows the pointer;
        // repaint once more when it leaves so the line goes too.
        let next = self.cursor_next.take();
        if next != self.cursor_t {
            self.cursor_t = next;
            ctx.request_repaint();
        }

        if self.running() || self.batch.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        crate::settings::store(storage, &self.settings());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop();
        self.discard_recording();
    }
}

/// A full-width note in a tint of `color`, with a bar of it down the left
/// edge, for something on the screen that needs attention.
fn banner(
    ui: &mut egui::Ui,
    color: Color32,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let resp = theme::card()
        .fill(color.gamma_multiply(0.14))
        .inner_margin(egui::Margin {
            left: 18,
            right: 10,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(contents);
        });
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
        color,
    );
    resp.response
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

    /// Every piece of text a frame painted.
    fn painted_text(out: &egui::FullOutput) -> String {
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Clipped sound puts a warning over the readings, and a stored clock
    /// error adds the corrected rate under the card-clock one.
    #[test]
    fn clipping_and_the_true_clock_show_with_the_readings() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.reset_session(48000, "syn".into());
        let audio = generate(
            &SynthConfig {
                duration_s: 12.0,
                rate_s_per_day: 10.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let peak = audio.samples.iter().fold(0f32, |m, v| m.max(v.abs()));
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            let hot: Vec<f32> = b
                .iter()
                .map(|v| (v * 4.0 / peak).clamp(-1.0, 1.0))
                .collect();
            live.push(&hot);
        }
        app.clock = Some(clockstore::DeviceClock {
            device: "syn".into(),
            ppm: 20.0,
            ppm_sd: None,
            span_s: 600.0,
            measured_utc: "2026-10-10T13:00:00Z".into(),
            source: "measure".into(),
        });
        let ctx = themed();
        let mut text = String::new();
        for w in [400.0, 400.0, 1280.0, 1280.0, 1280.0] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 820.0),
                )),
                ..Default::default()
            };
            text = painted_text(&ctx.run(input, |ctx| app.panels(ctx)));
        }
        assert!(text.contains("Clipping on"), "{text}");
        assert!(text.contains("True Clock"), "{text}");
        // 20 ppm slow takes 1.7 s/d off a rate read on the card.
        let card = app.reading().unwrap().rate_s_per_day.unwrap();
        let shown = format!("{:+.1}", clockstore::correct_rate(card, 20.0)).replace('-', "−");
        assert!(text.contains(&format!("True Clock {shown}")), "{text}");
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

    /// A folder of segments is analysed in name order as one recording,
    /// and its Steadiness tests cover the whole of it.
    #[test]
    fn a_folder_of_segments_is_analysed_as_one_recording() {
        let dir = std::env::temp_dir().join(format!("tg-app-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 360.0,
                rate_s_per_day: 8.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        // Three 2-minute segments, written out of name order, plus a file
        // that isn't a recording.
        let n = audio.samples.len() / 3;
        for (i, name) in [(2, "seg-03.wav"), (0, "seg-01.wav"), (1, "seg-02.wav")] {
            let part = timegrapher_core::audio::Audio {
                samples: audio.samples[i * n..(i + 1) * n].to_vec(),
                sample_rate: audio.sample_rate,
            };
            timegrapher_core::audio::write_wav(&dir.join(name), &part).unwrap();
        }
        std::fs::write(dir.join("notes.txt"), "not audio").unwrap();
        let files = recordings_in(&dir).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["seg-01.wav", "seg-02.wav", "seg-03.wav"]);

        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        // Cancelled, the analysis stops without an answer.
        app.start_batch(&dir);
        let b = app.batch.as_ref().unwrap();
        assert_eq!((b.files, b.first.as_str()), (3, "seg-01.wav"));
        b.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let stopped = b.rx.recv_timeout(std::time::Duration::from_secs(60));
        assert!(
            matches!(
                stopped,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
            ),
            "{stopped:?}"
        );
        app.batch = None;
        app.start_batch(&dir);
        let t = Instant::now();
        while app.batch.is_some() && t.elapsed().as_secs() < 120 {
            app.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let live = app.live.as_ref().expect("analysed");
        assert!(
            (live.duration_s() - 360.0).abs() < 1.0,
            "{}",
            live.duration_s()
        );
        assert!(app.source_label.contains("3 files"), "{}", app.source_label);
        let rate = live.reading(60.0).rate_s_per_day.unwrap();
        assert!((rate - 8.0).abs() < 2.0, "rate {rate}");
        // An empty folder says so instead of failing quietly.
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(recordings_in(&empty).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A microphone session's sound is kept as it goes, can be saved at
    /// any point, and goes with a new session.
    #[test]
    fn sessions_keep_save_and_discard_their_sound() {
        let base = std::env::temp_dir().join(format!("tg-app-save-{}", std::process::id()));
        let mut app = synthetic(12.0, 20.0);
        app.mic_session = true;
        let info = app.info("test", 48000, 16);
        let mut r = Recorder::start(&base.join("kept"), info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 6],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        let kept = r.dir().to_path_buf();
        app.recorder = Some(r);
        assert!(app.unsaved_s() > 5.0, "unsaved sound to ask about");
        app.save_into(&base.join("saved"));
        let saved = app.saved_to.clone().expect("saved");
        assert!(saved.join("audio-001.wav").is_file());
        assert_eq!(app.unsaved_s(), 0.0);
        // Paused, a session can carry on; a new session forgets it.
        app.paused_at = Some(Instant::now());
        assert!(app.can_resume());
        app.new_session();
        assert!(app.live.is_none() && app.recorder.is_none() && !app.can_resume());
        assert!(!kept.exists(), "the unsaved copy is thrown away");
        assert!(
            saved.join("audio-001.wav").is_file(),
            "the saved copy stays"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A new position keeps the strip's beats and marks the stretch; the
    /// readings and the distributions take only the position the watch is
    /// in, and going back to an earlier position joins its stretches for
    /// the steadiness tests.
    #[test]
    fn a_new_position_marks_a_stretch_and_keeps_the_strip() {
        let mut app = synthetic(20.0, 0.0);
        app.mic_session = true;
        app.paused_at = Some(Instant::now());
        let feed = |app: &mut TimegrapherApp, rate: f64| {
            let cfg = SynthConfig {
                duration_s: 20.0,
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
        };
        let first = app.live.as_ref().unwrap().beats().len();
        app.set_position(Position::DialDown);
        assert_eq!(app.live.as_ref().unwrap().beats().len(), first, "kept");
        feed(&mut app, 60.0);
        app.set_position(Position::DialUp);
        feed(&mut app, 0.0);
        let live = app.live.as_ref().unwrap();
        assert!(live.beats().len() > 2 * first);
        assert_eq!(app.stretches.len(), 3);
        let bands = app.position_bands();
        assert_eq!(
            bands.iter().map(|b| b.2).collect::<Vec<_>>(),
            ["Dial Up", "Dial Down", "Dial Up"]
        );
        assert!(app.in_position(10.0) && !app.in_position(30.0) && app.in_position(50.0));
        // The readings since the move are Dial Up's alone.
        let r = app.reading().unwrap();
        assert!(
            r.rate_s_per_day.unwrap().abs() < 5.0,
            "{:?}",
            r.rate_s_per_day
        );
        // Dial Up's two stretches joined, without Dial Down's beats.
        let spans = app.current_spans();
        assert_eq!(spans.len(), 2);
        let log = steady::log_of(live).unwrap();
        let joined = steady::join(&log, &spans);
        let dd = log
            .beats
            .iter()
            .filter(|b| b.time > spans[0].1 && b.time < spans[1].0)
            .count();
        assert!(dd > 100);
        assert!(joined.beats.len() + dd <= log.beats.len());
        assert!(joined.beats.len() + dd + 40 > log.beats.len());
        assert!(joined
            .beats
            .windows(2)
            .all(|w| w[1].time > w[0].time && w[1].index > w[0].index));
        let want = steady::joined_duration(&spans, live.duration_s());
        assert!((joined.duration_s - want).abs() < 1e-9);
        assert!((want - (live.duration_s() - 20.0 + steady::JOIN_S)).abs() < 1.0);
        let fit = timegrapher_core::timing::fit(&joined.beats, joined.bph).unwrap();
        assert!(fit.rate_s_per_day.abs() < 5.0, "{}", fit.rate_s_per_day);
        // And it all draws, bands and all.
        let ctx = themed();
        for p in Pane::CHARTS.into_iter().chain(Pane::HISTOGRAMS) {
            set_pane_visible(&mut app.panes.tiles, p, true);
        }
        frames(&mut app, &ctx, 2);
        app.relayout(false);
        frames(&mut app, &ctx, 2);
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
    fn histograms_start_hidden_and_keep_their_switch_through_a_relayout() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        for p in Pane::HISTOGRAMS {
            assert!(!pane_visible(&app.panes.tiles, p));
        }
        assert!(pane_visible(&app.panes.tiles, Pane::Rate));
        set_pane_visible(&mut app.panes.tiles, Pane::AmplitudeHistogram, true);
        app.relayout(true);
        assert!(pane_visible(&app.panes.tiles, Pane::AmplitudeHistogram));
        assert!(!pane_visible(&app.panes.tiles, Pane::RateHistogram));
    }

    #[test]
    fn launching_and_replaying_touch_no_input() {
        // `new` lists nothing; a replay never needs to.
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.devices_listed = false;
        app.apply_settings(Settings {
            device: Some("alsa:hw:CARD=Device,DEV=0".into()),
            ..Settings::default()
        });
        assert_eq!(app.device.as_deref(), Some("alsa:hw:CARD=Device,DEV=0"));
        let dir = std::env::temp_dir().join("tg-app-no-input");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("take.wav");
        let audio = generate(
            &SynthConfig {
                duration_s: 4.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        timegrapher_core::audio::write_wav(&path, &audio).unwrap();
        app.start_replay(&path);
        let ctx = themed();
        frames(&mut app, &ctx, 3);
        assert!(!app.devices_listed);
        // Nor does the microphone view, until its menu opens or Start.
        app.stop();
        app.input = Input::Microphone;
        frames(&mut app, &ctx, 2);
        assert!(!app.devices_listed);
    }

    #[test]
    fn a_calibre_names_the_wheels_and_a_typed_one_is_kept() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        assert!(app.calibre_wheels().is_none());
        // From the table: its train, and its lift angle where it has one.
        let c = calibres::find("2824-2").expect("ETA 2824-2 in the table");
        app.set_calibre(c.calibre.clone());
        let wheels = app.calibre_wheels().expect("wheels");
        assert_eq!(wheels.len(), c.wheels.len());
        if let Some(l) = c.lift_angle_deg {
            assert_eq!(app.lift_deg, l);
        }
        // Typed in: tidied for the tests, and kept with the settings.
        let mut mine = CustomCalibre::new("Bench Watch".into());
        mine.wheels.push(CustomWheel {
            name: " Third Wheel ".into(),
            period_s: 450.0,
        });
        mine.wheels.push(CustomWheel {
            name: String::new(),
            period_s: 10.0,
        });
        app.custom_calibres.push(mine.clone());
        app.set_calibre("Bench Watch".into());
        let wheels = app.calibre_wheels().expect("wheels");
        assert_eq!(wheels.len(), 4, "the unnamed wheel is left out");
        assert!(wheels.iter().any(|w| w.name == "third wheel"));
        let mut next = TimegrapherApp::with_devices(Vec::new(), None, false);
        next.apply_settings(app.settings());
        assert_eq!(next.custom_calibres, vec![mine]);
        // The pick itself is about the watch on the stand, so it isn't kept.
        assert!(next.calibre.is_empty());
    }

    #[test]
    fn settings_come_back_as_they_were_left() {
        // A fresh app opens with the defaults the settings describe.
        let fresh = TimegrapherApp::with_devices(Vec::new(), None, false);
        let mut first = fresh.settings();
        first.panes = None;
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(Settings::default()).unwrap()
        );
        // Change a few views, keep them, and open a new app with them.
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.strip.horizontal = true;
        app.relayout(true);
        app.sound_opts.scale = profiles::Scale::Decibels;
        app.hist_cumulative = true;
        app.folded.insert("Window".into());
        set_pane_visible(&mut app.panes.tiles, Pane::AmplitudeHistogram, true);
        app.set_average(30.0);
        let json = serde_json::to_string(&app.settings()).unwrap();
        let mut again = TimegrapherApp::with_devices(Vec::new(), None, false);
        again.apply_settings(serde_json::from_str(&json).unwrap());
        assert!(again.strip.horizontal);
        assert_eq!(again.sound_opts.scale, profiles::Scale::Decibels);
        assert!(again.hist_cumulative);
        assert!(again.folded.contains("Window"));
        assert_eq!(again.average_s, 30.0);
        assert!(pane_visible(&again.panes.tiles, Pane::AmplitudeHistogram));
        // Settings from an older app, with fields missing, fill in the rest.
        let old: Settings = serde_json::from_str(r#"{"sidebar": false}"#).unwrap();
        assert!(!old.sidebar && old.rate_line);
        // A layout missing panes is set aside for the starting one.
        let mut broken = app.settings();
        let mut tiles = Tiles::default();
        let only = tiles.insert_pane(Pane::Strip);
        broken.panes = Some(Tree::new("panes", only, tiles));
        again.apply_settings(broken);
        assert!(layout_is_whole(&again.panes));
        // A layout from before the Steadiness pane keeps its arrangement
        // and gains the pane as a tab beside the profile.
        let mut before = app.settings();
        let mut tree = app.panes.clone();
        let id = tree.tiles.find_pane(&Pane::Steadiness).unwrap();
        let tabs = tree.tiles.parent_of(id).unwrap();
        if let Some(egui_tiles::Tile::Container(c)) = tree.tiles.get_mut(tabs) {
            c.remove_child(id);
        }
        tree.tiles.remove(id);
        assert!(!layout_is_whole(&tree));
        before.panes = Some(tree);
        again.apply_settings(before);
        assert!(layout_is_whole(&again.panes));
        assert!(again.strip.horizontal, "the saved arrangement was kept");
        let sound = again.panes.tiles.find_pane(&Pane::Sound).unwrap();
        let steady = again.panes.tiles.find_pane(&Pane::Steadiness).unwrap();
        assert_eq!(
            again.panes.tiles.parent_of(sound),
            again.panes.tiles.parent_of(steady)
        );
    }

    #[test]
    fn moving_steadiness_out_leaves_the_profile_showing() {
        let app = synthetic(40.0, 0.0);
        let mut tree = app.panes.clone();
        let steady = tree.tiles.find_pane(&Pane::Steadiness).unwrap();
        let sound = tree.tiles.find_pane(&Pane::Sound).unwrap();
        let tabs = tree.tiles.parent_of(steady).unwrap();
        // What a drag leaves behind: Steadiness active in the profile's
        // tab group but moved into a tab group of its own.
        if let Some(egui_tiles::Tile::Container(c)) = tree.tiles.get_mut(tabs) {
            c.remove_child(steady);
            if let egui_tiles::Container::Tabs(t) = c {
                t.active = Some(steady);
            }
        }
        let own = tree.tiles.insert_tab_tile(vec![steady]);
        let root = tree.root().unwrap();
        if let Some(egui_tiles::Tile::Container(c)) = tree.tiles.get_mut(root) {
            c.add_child(own);
        }
        repair_tabs(&mut tree);
        let Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t))) =
            tree.tiles.get(tabs)
        else {
            panic!("the profile's tab group is gone");
        };
        assert_eq!(t.active, Some(sound));
    }

    #[test]
    fn charts_can_follow_the_strip() {
        let mut app = synthetic(40.0, 0.0);
        assert_eq!(app.chart_window(), None);
        app.span_of_strip = true;
        let (a, b) = app.chart_window().unwrap();
        assert!((b - app.end_s()).abs() < 1e-9);
        assert!((b - a - app.strip.span_s).abs() < 1e-9);
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
