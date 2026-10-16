//! The main window: input and settings on the left, the readings across
//! the top, and below them panes (the paper strip and the charts over time)
//! that can be dragged by their tabs into any arrangement.

use crate::applog;
use crate::calibre_ui;
use crate::fields::{self, Format};
use crate::guided;
use crate::help;
use crate::isochronism;
use crate::profiles;
use crate::report::{self, ReportDialog};
use crate::scope;
use crate::settings::Settings;
use crate::steady::{self, Steadiness};
use crate::strip::{self, Anchor, StripInput, StripView};
use crate::theme;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{GridMark, Line, LineStyle, Plot, Points};
use egui_tiles::{Linear, LinearDir, SimplificationOptions, TileId, Tiles, Tree, UiResponse};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::Instant;
use timegrapher_core::beats::STANDARD_BPH;
use timegrapher_core::capture::{self, Capture, Event, InputDevice, Level};
use timegrapher_core::clockstore;
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading, MAX_PROFILE_S};
use timegrapher_core::measure::Caveat;
use timegrapher_core::mixer::{GainState, InputGain};
use timegrapher_core::periodicity::Wheel;
use timegrapher_core::profile::TickProfile;

/// The A and B sides' sounds.
type Profiles = [Option<TickProfile>; 2];

/// How often the sound is kept for looking back, seconds.
const SOUND_EVERY_S: f64 = 2.0;
/// The hints on Time Span's two choices.
const SPAN_SESSION_HINT: &str = "Everything since the session started, growing as it goes";
const SPAN_STRIP_HINT: &str = "Only the paper strip's length, ending where the strip does: the \
     newest readings, moving along with the strip";
/// What the reading cards asked for.
struct Readouts {
    /// A new Average Over.
    average: Option<f64>,
    /// Keep the input's clock as timed.
    keep_clock: bool,
}

/// Seconds of listening before the input's clock, timed against the
/// computer's, is offered to keep: the twenty minutes `timegrapher clock
/// measure` takes by default. It is timed again each minute after.
const CLOCK_MEASURE_S: f64 = 1200.0;

/// The microphone's clock, timed against the computer's while listening.
struct ClockTrack {
    /// The input's name, as the clock file keeps it.
    device: String,
    tracker: timegrapher_core::clock::ClockTracker,
    /// Audio seconds at which to fit again.
    next_s: f64,
    /// The latest fit, from [`CLOCK_MEASURE_S`] of audio on.
    fit: Option<timegrapher_core::clock::ClockFit>,
}

impl ClockTrack {
    /// Count a block of audio that arrived at `at`, fitting once a minute.
    fn push(&mut self, samples: usize, at: std::time::SystemTime) {
        self.tracker.push(samples, at);
        if self.tracker.audio_s() >= self.next_s {
            self.next_s = self.tracker.audio_s() + 60.0;
            self.fit = self.tracker.fit();
        }
    }
}

/// The Rate caveat for an input whose clock hasn't been measured.
const CLOCK_UNKNOWN: &str = "input clock not measured";
/// Plain marks on the level meter, for reference only.
const REFERENCE_DBFS: [f64; 2] = [-10.0, -6.0];
use crate::session::{self, Action, Session};
use timegrapher_core::recorder::{Recorder, SessionInfo};
use timegrapher_core::session::{Manifest, RecordingEntry};
use timegrapher_core::stream::{self, BeatLog, StreamConfig};

/// What a question before throwing away unsaved sound is about.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Confirm {
    NewSession,
    Quit,
    /// Open a recording (analysed whole, or replayed when `replay`).
    Open {
        path: PathBuf,
        replay: bool,
    },
}

/// Where a session's sound is kept until it is saved or thrown away.
fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join("timegrapher-unsaved")
}

/// Keep acquisition and recording failures distinct from their display text.
enum CaptureFailure {
    Recording(String),
    Input(String),
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
    /// A position by any name the session files take: DU, CH, "dial up"…
    fn from_name(s: &str) -> Option<Position> {
        let common = timegrapher_core::session::Position::parse(s)?.common();
        Position::ALL.into_iter().find(|p| p.code() == common)
    }

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

    /// The name with its code, and for a crown position the Swiss
    /// laboratory name beside it (the hour that points up), as the menus
    /// and the strip's position ticker show it.
    pub fn label(self) -> &'static str {
        match self {
            Position::DialUp => "Dial Up (DU)",
            Position::DialDown => "Dial Down (DD)",
            Position::CrownUp => "Crown Up (CU, 3H)",
            Position::CrownDown => "Crown Down (CD, 9H)",
            Position::CrownLeft => "Crown Left (CL, 6H)",
            Position::CrownRight => "Crown Right (CR, 12H)",
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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Pane {
    Watch,
    Strip,
    Sound,
    Rate,
    Amplitude,
    BeatError,
    RateHistogram,
    AmplitudeHistogram,
    BeatErrorHistogram,
    /// The Steadiness views, each a pane of its own. Layouts saved when
    /// they were one pane name it Steadiness.
    #[serde(alias = "Steadiness")]
    SteadyReadings,
    SteadyPeriods,
    SteadyCycle,
    SteadyAutocorrelation,
    SteadyAllan,
    SteadyCusum,
    SteadyFft,
    Scope,
    Isochronism,
    PositionsTest,
}

impl Pane {
    /// Every pane, in the order the Panes list shows them.
    const ALL: [Pane; 19] = [
        Pane::Watch,
        Pane::PositionsTest,
        Pane::Strip,
        Pane::Sound,
        Pane::Scope,
        Pane::Isochronism,
        Pane::Rate,
        Pane::Amplitude,
        Pane::BeatError,
        Pane::RateHistogram,
        Pane::AmplitudeHistogram,
        Pane::BeatErrorHistogram,
        Pane::SteadyReadings,
        Pane::SteadyPeriods,
        Pane::SteadyCycle,
        Pane::SteadyAutocorrelation,
        Pane::SteadyAllan,
        Pane::SteadyCusum,
        Pane::SteadyFft,
    ];
    /// The Steadiness views, in the order the icon bar groups them.
    const STEADY: [Pane; 7] = [
        Pane::SteadyReadings,
        Pane::SteadyPeriods,
        Pane::SteadyCycle,
        Pane::SteadyAutocorrelation,
        Pane::SteadyAllan,
        Pane::SteadyCusum,
        Pane::SteadyFft,
    ];
    const CHARTS: [Pane; 3] = [Pane::Rate, Pane::Amplitude, Pane::BeatError];
    /// The panes that average the beats, each over Readings' Average Over
    /// or a time of its own.
    const AVERAGING: [Pane; 8] = [
        Pane::Strip,
        Pane::Sound,
        Pane::Rate,
        Pane::Amplitude,
        Pane::BeatError,
        Pane::RateHistogram,
        Pane::AmplitudeHistogram,
        Pane::BeatErrorHistogram,
    ];
    const HISTOGRAMS: [Pane; 3] = [
        Pane::RateHistogram,
        Pane::AmplitudeHistogram,
        Pane::BeatErrorHistogram,
    ];

    fn title(self) -> &'static str {
        match self {
            Pane::Watch => "Watch",
            Pane::Strip => "Paper Strip",
            Pane::Sound => "Tick Tock Profile",
            Pane::Rate => "Rate",
            Pane::Amplitude => "Amplitude",
            Pane::BeatError => "Beat Error",
            Pane::RateHistogram => "Rate Distribution",
            Pane::AmplitudeHistogram => "Amplitude Distribution",
            Pane::BeatErrorHistogram => "Beat Error Distribution",
            Pane::SteadyReadings => "Readings Over Time",
            Pane::SteadyPeriods => "Periods",
            Pane::SteadyCycle => "Cycle",
            Pane::SteadyAutocorrelation => "Autocorrelation",
            Pane::SteadyAllan => "Allan Deviation",
            Pane::SteadyCusum => "CUSUM",
            Pane::SteadyFft => "FFT",
            Pane::Scope => "Scope",
            Pane::Isochronism => "Isochronism",
            Pane::PositionsTest => "Positions Test",
        }
    }

    /// What the pane shows, for its icon and its line in the View menu.
    fn hint(self) -> &'static str {
        match self {
            Pane::Watch => {
                "The microphone or recording, listening and saving, and the watch: its \
                 beat rate, lift angle and position"
            }
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
            Pane::SteadyReadings => {
                "Whether the rate, amplitude and beat error each hold steady over the \
                 session, or carry a cycle, two states, a step, a drift or wander, with every \
                 reading in time order"
            }
            Pane::SteadyPeriods
            | Pane::SteadyCycle
            | Pane::SteadyAutocorrelation
            | Pane::SteadyAllan
            | Pane::SteadyCusum
            | Pane::SteadyFft => self.steady_view().map_or("", steady::View::hint),
            Pane::Scope => {
                "The sound of the latest beats as it came in, at 20, 200 or 400 ms, the \
                 latest few drawn over each other"
            }
            Pane::Isochronism => {
                "How the rate moves with the amplitude as the mainspring lets down, and the \
                 first half hour against 24 hours later"
            }
            Pane::PositionsTest => {
                "The six positions one after another, settled and measured in turn, with \
                 their table filling in as it goes, and the test 24 hours after winding \
                 against the one just after"
            }
        }
    }
}

impl Pane {
    /// What a Steadiness pane draws under each series.
    fn steady_view(self) -> Option<steady::View> {
        use steady::View;
        Some(match self {
            Pane::SteadyReadings => View::Readings,
            Pane::SteadyPeriods => View::Periods,
            Pane::SteadyCycle => View::Cycle,
            Pane::SteadyAutocorrelation => View::Autocorrelation,
            Pane::SteadyAllan => View::Allan,
            Pane::SteadyCusum => View::Cusum,
            Pane::SteadyFft => View::Fft,
            _ => return None,
        })
    }
}

/// The starting arrangement: the strip beside the sound and the three
/// charts when it runs down the window, above them when it lies on its
/// side.
/// A pane's icon and the colour it is drawn in: the series' colour for a
/// chart or a distribution, grey otherwise.
fn pane_icon(pane: Pane, pal: &theme::Palette) -> (theme::Icon, Color32) {
    use theme::Icon;
    match pane {
        Pane::Watch => (Icon::Watch, pal.text_secondary),
        Pane::Strip => (Icon::Strip, pal.text_secondary),
        Pane::Sound => (Icon::Profile, pal.text_secondary),
        Pane::Scope => (Icon::Scope, pal.text_secondary),
        Pane::SteadyReadings => (Icon::Steadiness, pal.text_secondary),
        Pane::SteadyPeriods => (Icon::Periods, pal.text_secondary),
        Pane::SteadyCycle => (Icon::Cycle, pal.text_secondary),
        Pane::SteadyAutocorrelation => (Icon::Autocorrelation, pal.text_secondary),
        Pane::SteadyAllan => (Icon::Allan, pal.text_secondary),
        Pane::SteadyCusum => (Icon::Cusum, pal.text_secondary),
        Pane::SteadyFft => (Icon::Fft, pal.text_secondary),
        Pane::Isochronism => (Icon::Isochronism, pal.text_secondary),
        Pane::PositionsTest => (Icon::Positions, pal.text_secondary),
        Pane::Rate => (Icon::Chart, pal.trace_rate),
        Pane::Amplitude => (Icon::Chart, pal.trace_amplitude),
        Pane::BeatError => (Icon::Chart, pal.trace_beat_error),
        Pane::RateHistogram => (Icon::Distribution, pal.trace_rate),
        Pane::AmplitudeHistogram => (Icon::Distribution, pal.trace_amplitude),
        Pane::BeatErrorHistogram => (Icon::Distribution, pal.trace_beat_error),
    }
}

/// Save As: Ctrl+Shift+S, or Cmd+Shift+S on a Mac.
fn save_as_shortcut() -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::S,
    )
}

const SAVE_HINT: &str = "Save the sound of this session so far, with its clock log and \
     settings, to analyse again later. The first time it asks where: the take goes in a \
     folder of its own, named for its time and watch, inside the folder you choose. After \
     that, Save Again adds the sound since into the same take without asking. Listening \
     carries on. Unsaved sound is thrown away by New Session or quitting.";

const SAVE_AS_HINT: &str = "Save the session so far as a new take, in a folder you choose; \
     Save Again then goes there";

fn default_layout(horizontal: bool) -> Tree<Pane> {
    let mut tiles = Tiles::default();
    // Each pane in its own tab bar from the start. Left to the tree, the
    // tab bar would take over the pane's id and the pane get a new one,
    // which loses a pane hidden before the first frame.
    let mut tabbed = |p: Pane| {
        let pane = tiles.insert_pane(p);
        tiles.insert_tab_tile(vec![pane])
    };
    let watch = tabbed(Pane::Watch);
    let strip = tabbed(Pane::Strip);
    let sound = tabbed(Pane::Sound);
    let charts: Vec<TileId> = Pane::CHARTS.into_iter().map(&mut tabbed).collect();
    let hists: Vec<TileId> = Pane::HISTOGRAMS.into_iter().map(&mut tabbed).collect();
    // The steadiness views share the profile's place, as more tabs.
    let scope = tiles.insert_pane(Pane::Scope);
    let steady: Vec<TileId> = Pane::STEADY.map(|p| tiles.insert_pane(p)).to_vec();
    let isochronism = tiles.insert_pane(Pane::Isochronism);
    let positions = tiles.insert_pane(Pane::PositionsTest);
    if let Some(egui_tiles::Tile::Container(c)) = tiles.get_mut(sound) {
        c.add_child(scope);
        for v in steady {
            c.add_child(v);
        }
        c.add_child(isochronism);
        c.add_child(positions);
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
    let root = tiles.insert_container(Linear::new_binary(
        LinearDir::Horizontal,
        [watch, root],
        WATCH_SHARE,
    ));
    let mut tree = Tree::new("panes", root, tiles);
    for p in Pane::HISTOGRAMS
        .into_iter()
        .chain(Pane::STEADY.into_iter().skip(1))
    {
        set_pane_visible(&mut tree.tiles, p, false);
    }
    tree
}

/// How much of the window's width the Watch pane starts with, at the left.
const WATCH_SHARE: f32 = 0.22;

/// A layout saved before the Watch pane, with it added in its own column
/// at the left, as it starts.
fn add_watch(tree: &mut Tree<Pane>) {
    if tree.tiles.find_pane(&Pane::Watch).is_some() {
        return;
    }
    let Some(old_root) = tree.root() else { return };
    let pane = tree.tiles.insert_pane(Pane::Watch);
    let tabs = tree.tiles.insert_tab_tile(vec![pane]);
    let root = tree.tiles.insert_container(Linear::new_binary(
        LinearDir::Horizontal,
        [tabs, old_root],
        WATCH_SHARE,
    ));
    tree.root = Some(root);
}

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

/// A layout saved before a pane existed (Steadiness, Scope, Positions
/// Test), with it added
/// as a tab beside the profile (or at the top, if the profile is not in
/// tabs).
fn add_missing(tree: &mut Tree<Pane>, pane: Pane) {
    if tree.tiles.find_pane(&pane).is_some() {
        return;
    }
    let pane = tree.tiles.insert_pane(pane);
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

/// Averaging times offered for the readings, seconds.
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
const DURATION: Format = Format {
    show: fields::duration,
    parse: fields::parse_duration,
};

/// One point of the charts over time.
/// One line on a reading's chart over the session.
#[derive(Clone)]
struct ChartLine {
    name: String,
    pts: Vec<[f64; 2]>,
    color: Color32,
    style: LineStyle,
    /// The swatch beside its value under the pointer.
    key: Color32,
    /// A colour drawn solid under the dashed line, showing in its gaps:
    /// Tick's and Tock's amplitude dash purple with blue or orange.
    under: Option<Color32>,
    /// The reading itself, drawn heavier than the lines beside it.
    main: bool,
}

/// The key a run of readings is kept under: its averaging time in ms.
fn trend_key(seconds: f64) -> u64 {
    (seconds * 1000.0).round() as u64
}

/// The readings over the whole of what `live` holds, averaged over
/// `seconds`, at up to [`TREND_POINTS`] times.
fn trend_over(live: &LiveAnalyzer, seconds: f64) -> Vec<TrendPoint> {
    let end = live.duration_s();
    let step = (end / TREND_POINTS).max(0.5);
    let mut out = Vec::new();
    let mut t = step;
    while t <= end + 1e-9 {
        out.push(TrendPoint::of(t, &live.reading_at(t, seconds)));
        t += step;
    }
    out
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
    /// The beats were spaced like a watch's, not scattered like noise.
    heard: bool,
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
            heard: r.beats_used > 0 && r.watch_heard,
        }
    }
}

/// Where each position began, seconds, and the position, oldest first.
type Stretches = Vec<(f64, Position)>;

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
    /// The positions the take's session file gives, to mark once it is in.
    stretches: Vec<(f64, Position)>,
    /// The settling before each position's readings, from the same file.
    settles: Vec<(f64, f64)>,
    /// Where the files' order disagrees with when they were written.
    order_note: Option<String>,
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
    /// (value, supplied): restored/resolved context applies at this value;
    /// numeric edits away from it retain the existing default-angle inference.
    lift_context: (f64, bool),
    position: Position,
    /// The beat rate the live engine had locked on to, to log a change.
    locked_bph: Option<u32>,
    /// A crash record from an earlier run not yet looked at, and whether
    /// its text is showing.
    crash: Option<PathBuf>,
    crash_shown: bool,
    /// Report a Problem.
    report: ReportDialog,
    updates: crate::update::Updates,
    /// The Positions Test, while it runs and once finished, until a new
    /// test or a new session.
    guided: Option<guided::Test>,
    /// The time the Positions Test didn't use, marked on the strip; kept
    /// after the test ends, until a new session.
    guided_unused: Vec<(f64, f64, &'static str)>,
    /// The stretches the Positions Test measured, the one under way
    /// included, kept like `guided_unused`.
    guided_measured: Vec<(f64, f64)>,
    /// The full-wind test's results, loaded for the next test to run as
    /// its 24 hour half.
    guided_earlier: Option<guided::FullWind>,
    /// Seconds of settling and of measuring in each position, for the
    /// next test.
    guided_settle_s: f64,
    guided_measure_s: f64,
    /// Look for a newer release each time the app starts.
    check_at_start: bool,
    /// The check at start is still to be made (on the first frame, which
    /// has the context it repaints).
    check_pending: bool,
    /// Listening, paused, the positions and settling, the kept sound and
    /// what of it is saved.
    session: Session,
    average_s: f64,
    watch: String,
    /// The calibre picked under Watch: a name in the table or one of the
    /// user's own, or empty when not set.
    calibre: String,
    /// The user's calibres, the short list and the recent picks.
    cal: calibre_ui::Calibres,
    /// Panes that average over their own time rather than Readings'
    /// Average Over, and that time in seconds.
    pane_average: std::collections::BTreeMap<Pane, f64>,
    /// The readings over the session at each averaging time a pane uses
    /// other than Readings' (whose are `trend`), by milliseconds.
    other_trends: std::collections::BTreeMap<u64, Vec<TrendPoint>>,
    /// Where the last Save put the session, if it has been saved.
    /// The folder Save offers first.
    save_dir: String,
    /// Asking before something throws away unsaved sound.
    confirm: Option<Confirm>,
    /// The recording last opened, which the path box goes back to when
    /// another can't be opened.
    opened_path: String,
    /// The watch set up for the microphone, kept while a recording with
    /// its own (or no) settings is open, and put back on listening again.
    mic_setup: Option<WatchSetup>,
    /// The newest beat looked at for a break in the sound, session s.
    last_beat_seen: f64,
    /// A break in the sound while listening, from and to (session s), that
    /// may have been the watch being moved: the banner asks.
    move_prompt: Option<(f64, f64)>,
    /// Said with the result once the recording opening is analysed.
    open_note: Option<String>,
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
    /// The charts cover only the strip's length, ending where the strip
    /// does, instead of the whole session.
    span_of_strip: bool,
    /// The same for the distributions, set on their own.
    hist_span_of_strip: bool,
    /// Show the cumulative share instead of the probability plot.
    hist_cumulative: bool,
    /// Amplitude and beat error histograms count the readings rather than
    /// each 2 seconds of beats.
    hist_readings: bool,
    /// The series each Steadiness pane shows: rate, amplitude, beat error.
    steady_series: std::collections::BTreeMap<steady::View, [bool; 3]>,
    /// The Periods pane's scores on a log scale.
    periods_log: bool,
    /// The FFT pane's rate as the beat timing's spectrum, in ms.
    fft_timing: bool,
    scope_view: scope::View,
    /// Sound read from the recording for the Scope when looking back: the
    /// stretch asked for (seconds) and what was read of it.
    scope_audio: Option<ScopeCut>,
    /// Each beat's trace once drawn, so a beat stays on the Scope (fading
    /// as newer ones come) after the live engine has let its sound go.
    scope_traces: std::collections::VecDeque<(ScopeKey, Vec<f32>)>,
    scope_job: Option<std::sync::mpsc::Receiver<ScopeCut>>,
    /// The steadiness tests' latest answer over the session, and the run
    /// in the background that will replace it.
    steady: Option<Steadiness>,
    steady_job: Option<std::sync::mpsc::Receiver<Steadiness>>,
    /// The step the steadiness run under way is on, and when it started.
    steady_work: (
        std::sync::Arc<std::sync::atomic::AtomicU8>,
        std::time::Instant,
    ),
    /// The time under the pointer on the strip or a chart, drawn as a
    /// cursor on all of them: last frame's, and this frame's so far.
    cursor_t: Option<f64>,
    cursor_next: Option<f64>,
    /// Light, dark or following the system, kept for the next start.
    theme: egui::ThemePreference,
    /// Watch pane groups folded down to their caption.
    folded: std::collections::BTreeSet<String>,
    /// The rate, amplitude and beat error figures above the panes.
    show_readings: bool,
    /// Asked to put the charts back on the whole session, following new
    /// beats: set by the button, applied by every chart on the next frame.
    charts_to_live: bool,
    charts_to_live_now: bool,
    /// The averaging time each chart last drew with: a new one rescales
    /// its values, which spread wider or narrower with it.
    chart_average_drawn: std::collections::HashMap<Pane, f64>,
    /// `None` follows the newest beat; otherwise the time at the newest
    /// edge of the strip.
    view_end: Option<f64>,
    panes: Tree<Pane>,

    capture: Option<Capture>,
    /// How fast Replay plays a recording, times its own speed.
    replay_speed: f32,
    live: Option<LiveAnalyzer>,
    source_label: String,
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
    /// The microphone's clock timed against the computer's as its audio
    /// arrives: offered to keep once it has run long enough.
    clock_track: Option<ClockTrack>,
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
        app.cal = calibre_ui::Calibres::open(timegrapher_core::calibre_store::default_path());
        if let Some(s) = saved {
            app.apply_settings(s);
        }
        app.crash = applog::dir().and_then(|d| applog::unseen_crashes(&d).pop());
        app.check_pending = app.check_at_start;
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
            hist_span_of_strip: self.hist_span_of_strip,
            hist_cumulative: self.hist_cumulative,
            hist_readings: self.hist_readings,
            sound: self.sound_opts,
            folded: self.folded.iter().cloned().collect(),
            theme: self.theme,
            steady_series: [true; 3],
            steady_shown: self.steady_series.iter().map(|(v, s)| (*v, *s)).collect(),
            periods_log: self.periods_log,
            fft_timing: self.fft_timing,
            positions_settle_s: self.guided_settle_s,
            positions_measure_s: self.guided_measure_s,
            scope: self.scope_view,
            panes: Some(self.panes.clone()),
            // Kept in the user calibre file now.
            custom_calibres: Vec::new(),
            recent_calibres: self.cal.recent.clone(),
            short_added: self.cal.short_added.clone(),
            short_removed: self.cal.short_removed.clone(),
            pane_average: self.pane_average.iter().map(|(&p, &s)| (p, s)).collect(),
            check_updates: self.check_at_start,
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
            hist_span_of_strip,
            hist_cumulative,
            hist_readings,
            sound,
            folded,
            theme,
            steady_series,
            steady_shown,
            periods_log,
            fft_timing,
            positions_settle_s,
            positions_measure_s,
            scope,
            panes,
            custom_calibres,
            recent_calibres,
            short_added,
            short_removed,
            pane_average,
            check_updates,
        } = s;
        self.check_at_start = check_updates;
        // Readings' time first: setting it drops every pane's own.
        if AVERAGES.contains(&average_s) {
            self.set_average(average_s);
        }
        self.pane_average = pane_average
            .into_iter()
            .filter(|(p, s)| Pane::AVERAGING.contains(p) && AVERAGES.contains(s))
            .filter(|&(_, s)| s != self.average_s)
            .collect();
        self.sync_trends();
        self.cal.migrate(custom_calibres);
        self.cal.recent = recent_calibres;
        self.cal.recent.truncate(calibre_ui::RECENT);
        self.cal.short_added = short_added;
        self.cal.short_removed = short_removed;
        // Checked against the inputs when they are listed.
        if let Some(d) = device.filter(|d| {
            !self.devices_listed || capture::choices(&self.devices).iter().any(|c| &c.id == d)
        }) {
            self.device = Some(d);
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
        self.hist_span_of_strip = hist_span_of_strip;
        self.hist_cumulative = hist_cumulative;
        self.hist_readings = hist_readings;
        self.sound_opts = sound;
        self.folded = folded.into_iter().collect();
        self.theme = theme;
        self.periods_log = periods_log;
        self.fft_timing = fft_timing;
        self.guided_settle_s =
            positions_settle_s.clamp(*guided::SETTLE_RANGE.start(), *guided::SETTLE_RANGE.end());
        self.guided_measure_s =
            positions_measure_s.clamp(*guided::MEASURE_RANGE.start(), *guided::MEASURE_RANGE.end());
        // Each pane's series, at least one shown; a pane not set yet takes
        // the choice the one Steadiness pane had.
        let every = steady_series;
        self.steady_series = steady_shown
            .into_iter()
            .chain(
                Pane::STEADY
                    .iter()
                    .filter_map(|p| Some((p.steady_view()?, every))),
            )
            .filter(|(_, s)| s.contains(&true))
            .fold(Default::default(), |mut m, (v, s)| {
                m.entry(v).or_insert(s);
                m
            });
        self.scope_view = scope;
        self.panes = match panes.map(|mut p| {
            add_missing(&mut p, Pane::SteadyReadings);
            // The views that were choices in the one Steadiness pane join
            // it hidden, each to be shown from its icon.
            for v in Pane::STEADY.into_iter().skip(1) {
                if p.tiles.find_pane(&v).is_none() {
                    add_missing(&mut p, v);
                    set_pane_visible(&mut p.tiles, v, false);
                }
            }
            add_missing(&mut p, Pane::Scope);
            add_missing(&mut p, Pane::Isochronism);
            add_missing(&mut p, Pane::PositionsTest);
            add_watch(&mut p);
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
            lift_deg: DEFAULT_LIFT_DEG,
            lift_context: (DEFAULT_LIFT_DEG, false),
            position: Position::DialUp,
            session: Session::default(),
            locked_bph: None,
            crash: None,
            crash_shown: false,
            report: ReportDialog::default(),
            updates: Default::default(),
            guided: None,
            guided_unused: Vec::new(),
            guided_measured: Vec::new(),
            guided_earlier: None,
            guided_settle_s: guided::SETTLE_S,
            guided_measure_s: guided::MEASURE_S,
            check_at_start: true,
            check_pending: false,
            average_s: 10.0,
            watch: String::new(),
            calibre: String::new(),
            cal: calibre_ui::Calibres::in_memory(),
            pane_average: Default::default(),
            other_trends: Default::default(),
            confirm: None,
            opened_path: String::new(),
            mic_setup: None,
            last_beat_seen: 0.0,
            move_prompt: None,
            open_note: None,
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
            hist_span_of_strip: false,
            hist_cumulative: false,
            steady_series: Default::default(),
            periods_log: false,
            fft_timing: false,
            scope_view: scope::View::default(),
            scope_audio: None,
            scope_traces: std::collections::VecDeque::new(),
            scope_job: None,
            steady: None,
            steady_job: None,
            steady_work: (Default::default(), std::time::Instant::now()),
            hist_readings: true,
            cursor_t: None,
            cursor_next: None,
            folded: Default::default(),
            theme: egui::ThemePreference::System,
            view_end: None,
            replay_speed: 1.0,
            panes: default_layout(false),
            show_readings: true,
            charts_to_live: false,
            charts_to_live_now: false,
            chart_average_drawn: Default::default(),
            capture: None,
            live: None,
            source_label: String::new(),
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
            clock_track: None,
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
        c.analysis.lift_known = self.lift_known();
        c.profile_s = self.sound_span();
        c
    }

    /// Seconds of beats the tick and tock sound covers: the averaging time,
    /// so it describes the same beats as the readings, up to the engine's
    /// limit.
    fn sound_span(&self) -> f64 {
        self.average_for(Pane::Sound).clamp(0.5, MAX_PROFILE_S)
    }

    /// A new averaging time: the readings over the whole session are worked
    /// out again, and the tick and tock sound follows it.
    /// A new Average Over for Readings, which every pane follows again:
    /// a pane's own time is dropped.
    fn set_average(&mut self, seconds: f64) {
        self.average_s = seconds;
        self.pane_average.clear();
        let span = self.sound_span();
        if let Some(l) = self.live.as_mut() {
            l.set_profile_span(span);
        }
        // The sound kept so far covered the old span.
        self.sound_at = None;
        self.rebuild_trend();
    }

    /// The time a pane averages over: its own, or Readings' Average Over.
    fn average_for(&self, pane: Pane) -> f64 {
        self.pane_average
            .get(&pane)
            .copied()
            .unwrap_or(self.average_s)
    }

    /// Give a pane its own averaging time, or `None` to follow Readings'.
    fn set_pane_average(&mut self, pane: Pane, seconds: Option<f64>) {
        match seconds {
            Some(s) if s != self.average_s => self.pane_average.insert(pane, s),
            _ => self.pane_average.remove(&pane),
        };
        if pane == Pane::Sound {
            let span = self.sound_span();
            if let Some(l) = self.live.as_mut() {
                l.set_profile_span(span);
            }
            self.sound_at = None;
        }
        self.sync_trends();
    }

    /// The readings over the session at an averaging time.
    fn trend_for(&self, seconds: f64) -> &[TrendPoint] {
        if seconds == self.average_s {
            return &self.trend;
        }
        self.other_trends
            .get(&trend_key(seconds))
            .map_or(&[][..], Vec::as_slice)
    }

    /// Keep a run of readings for each averaging time a pane uses, and no
    /// others: a new one is worked out over the session so far.
    fn sync_trends(&mut self) {
        let wanted: std::collections::BTreeSet<u64> = self
            .pane_average
            .values()
            .filter(|&&s| s != self.average_s)
            .map(|&s| trend_key(s))
            .collect();
        self.other_trends.retain(|k, _| wanted.contains(k));
        for k in wanted {
            if !self.other_trends.contains_key(&k) {
                let pts = self.context_trend(k as f64 / 1000.0);
                self.other_trends.insert(k, pts);
            }
        }
    }

    /// Clear every run of readings, for a new session or a change that
    /// makes them all out of date; new readings fill them again.
    fn clear_trends(&mut self) {
        self.trend.clear();
        for t in self.other_trends.values_mut() {
            t.clear();
        }
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
        self.session.discard_recording();
        self.session.clear();
        self.clock = None;
        self.live = Some(LiveAnalyzer::new(sample_rate, self.live_config()));
        self.source_label = label;
        self.anchor = None;
        self.clear_trends();
        self.view_end = None;
        self.level = Level::default();
        self.clipped_at = None;
        self.message = None;
        self.sound_history.clear();
        self.guided = None;
        self.guided_unused.clear();
        self.guided_measured.clear();
        self.sound_at = None;
        self.sound_job = None;
        self.steady = None;
        self.steady_job = None;
        self.last_beat_seen = 0.0;
        self.move_prompt = None;
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
        if self.refuse_faulted_recording() {
            return;
        }
        self.ensure_devices();
        if self.can_resume() {
            return self.resume_microphone();
        }
        self.stop();
        if let Some(s) = self.mic_setup.take() {
            self.set_watch_setup(s);
        }
        match capture::open_input(self.device.as_deref(), None) {
            Ok(c) => {
                let label = self.device_label(&c);
                applog::info(&format!(
                    "microphone opened: {label}, {} Hz, {}-bit",
                    c.sample_rate, c.bits
                ));
                self.reset_session(
                    c.sample_rate,
                    format!("{label} at {} Hz, {}-bit", c.sample_rate, c.bits),
                );
                self.opened_at = Instant::now();
                self.audio_at = None;
                self.session.listen();
                self.clock = clockstore::ClockStore::load()
                    .ok()
                    .and_then(|s| s.get(&c.label).cloned());
                self.clock_track = Some(ClockTrack {
                    device: c.label.clone(),
                    tracker: timegrapher_core::clock::ClockTracker::new(c.sample_rate),
                    next_s: CLOCK_MEASURE_S,
                    fit: None,
                });
                // Every session is kept as it goes, so it can be saved at any
                // point; it is thrown away with a new session unless saved.
                let info = self.info(&c.label, c.sample_rate, c.bits);
                match Recorder::start(&scratch_dir(), info) {
                    Ok(r) => self.session.recorder = Some(r),
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
        self.session.allows(Action::Resume) && self.live.is_some() && !self.running()
    }

    /// Stop listening for now, keeping everything.
    fn pause(&mut self) {
        if self.capture.is_none() {
            return;
        }
        self.capture = None;
        if self.session.mic() {
            applog::info("microphone stopped and released");
            let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
            if let Ok(Err(e)) = self.session.pause(now) {
                self.recording_failed(format!("Couldn't close the kept sound at the pause: {e}"));
            }
        }
    }

    /// Carry on listening after a pause, in the same session.
    fn resume_microphone(&mut self) {
        if self.refuse_faulted_recording() {
            return;
        }
        let rate = self.live.as_ref().map(|l| l.sample_rate());
        match capture::open_input(self.device.as_deref(), rate) {
            Ok(c) if Some(c.sample_rate) == rate => {
                let paused = match self.session.resume(&c.label) {
                    Ok(p) => p,
                    Err(e) => return self.error(e.to_string()),
                };
                if let Some(l) = self.live.as_mut() {
                    l.skip(paused);
                    self.session.resumed_at(l.duration_s());
                }
                self.opened_at = Instant::now();
                self.audio_at = None;
                self.view_end = None;
                self.message = None;
                // A pause is no break in the sound to ask about.
                self.last_beat_seen = 0.0;
                self.move_prompt = None;
                applog::info(&format!("microphone resumed at {} Hz", c.sample_rate));
                self.capture = Some(c);
            }
            Ok(c) => self.error(format!(
                "The microphone opened at {} Hz this time, not the {} Hz this session \
                 was listening at, so it can't carry on. Press New Session to start again.",
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
        self.session.discard_recording();
        self.session.clear();
        self.live = None;
        self.clear_trends();
        self.sound_history.clear();
        self.guided = None;
        self.guided_unused.clear();
        self.guided_measured.clear();
        self.source_label.clear();
        self.message = None;
    }

    /// Unsaved sound that a new session would throw away, seconds.
    fn unsaved_s(&self) -> f64 {
        self.session.unsaved_s()
    }

    /// Memory-only saved-edit inputs. Continuing audio must not make a short
    /// tail look like a settings edit by extending a clipped position range.
    fn save_metadata(&self) -> session::Metadata {
        let guided_ranges = self
            .guided
            .as_ref()
            .map_or_else(Vec::new, |t| t.stretches().to_vec());
        let guided_wind_h = (!guided_ranges.is_empty())
            .then(|| self.guided.as_ref().and_then(|t| t.wind_h()))
            .flatten();
        let mut about = self.guided_about();
        // Auto detection may evolve with sound. Track the selected setting,
        // while save_into still writes the effective BPH as before.
        about.bph = self.bph;
        session::Metadata {
            // session.json retains the raw name; manifest/results trim theirs.
            watch: self.watch.clone(),
            calibre: self.calibre.clone(),
            position: self.position.code().into(),
            bph: self.bph,
            lift_deg: self.lift_deg,
            lift_known: self.lift_known(),
            wheels: self
                .cal
                .custom_wheels(&self.calibre)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            guided_ranges,
            guided_wind_h,
            full_wind: self.guided.as_ref().and_then(|t| t.kept(&about)),
        }
    }

    fn metadata_unsaved(&self) -> bool {
        self.session.saved_to().is_some() && self.session.metadata_changed(&self.save_metadata())
    }

    /// Keep the existing sound threshold; metadata edits have no duration.
    fn needs_save_confirmation(&self) -> bool {
        self.session.recording_fault().is_some()
            || self.unsaved_s() > 5.0
            || self.metadata_unsaved()
    }

    /// Save Again: the sound so far into the take saved before, with no
    /// question; the first time, Save… asks where.
    fn save_again(&mut self) {
        if self.refuse_faulted_recording() {
            return;
        }
        match self.session.saved_to().and_then(|p| p.parent()) {
            Some(parent) => {
                let parent = parent.to_path_buf();
                self.save_into(&parent);
            }
            None => self.save_recording(),
        }
    }

    /// Save a copy of the session's sound so far, with its clock log and
    /// settings, in a folder the user picks. Listening carries on.
    fn save_recording(&mut self) {
        if self.refuse_faulted_recording() {
            return;
        }
        let mut d = rfd::FileDialog::new()
            .set_title("Choose Where to Save (the Take Gets Its Own Folder Inside)");
        let start = self
            .session
            .saved_to()
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
        if self.refuse_faulted_recording() {
            return;
        }
        let metadata = self.session.metadata_snapshot(self.save_metadata());
        // Prepare all required metadata before starting the saved-copy update.
        let prepared = (|| -> Result<_, String> {
            let result = self
                .reading()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| format!("Couldn't prepare the saved reading: {e}"))?;
            let manifest = toml::to_string_pretty(&self.take_manifest())
                .map_err(|e| format!("Couldn't prepare {MANIFEST}: {e}"))?;
            let about = self.guided_about();
            let kept = self
                .guided
                .as_ref()
                .and_then(|t| t.kept(&about))
                .map(|k| serde_json::to_string_pretty(&k).map(|s| s + "\n"))
                .transpose()
                .map_err(|e| format!("Couldn't prepare {}: {e}", guided::RESULTS_FILE))?;
            Ok((result, manifest, kept))
        })();
        let (result, manifest, kept) = match prepared {
            Ok(p) => p,
            Err(e) => return self.error(e),
        };
        // The watch's name as it is now, for session.json and the folder.
        self.note_settings();
        let Some(r) = self.session.recorder.as_mut() else {
            return;
        };
        let secs = r.duration_s();
        let mut files = vec![(MANIFEST, manifest.as_bytes())];
        if let Some(k) = &kept {
            files.push((guided::RESULTS_FILE, k.as_bytes()));
        }
        // With no new full-wind result (including a 24-hour half), preserve
        // the earlier result file for comparison, as Save Again always has.
        match r.save_copy_with_files(parent, &self.calibre, result.as_ref(), &files) {
            Ok(dir) => {
                self.info_msg(format!(
                    "Saved {} of sound in {}",
                    strip::fmt_time(secs),
                    dir.display()
                ));
                self.save_dir = parent.display().to_string();
                self.session.mark_saved(secs, dir, metadata);
            }
            Err(e) => self.error(format!("Saving the recording failed: {e}")),
        }
    }

    /// Open Report a Problem with the sound the app has: the live
    /// session's, or the recording open.
    fn open_report(&mut self) {
        let shown = report::Shown {
            bph: self.bph,
            lift_deg: self.lift_deg,
            app: serde_json::json!({
                "reading": self.reading(),
                "showing": self.source_label,
                "watch": self.watch.trim(),
                "calibre": self.calibre,
                "position": self.position.code(),
                "bph_set": self.bph,
                "lift_deg": self.lift_deg,
                "averaging_s": self.average_s,
                "at_s": self.end_s(),
            }),
        };
        let source = self.report_source();
        self.report.show(report::Kind::Bug, source, shown);
    }

    fn report_source(&mut self) -> Option<report::Source> {
        if self.session.mic() {
            let r = self.session.recorder.as_mut()?;
            let _ = r.flush();
            let total = r.duration_s();
            let about = std::fs::read_to_string(r.dir().join("session.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
            return (total >= 1.0).then(|| report::Source {
                files: r.segment_paths(),
                total_s: total,
                name: "this session".into(),
                live: true,
                end_s: total,
                about,
            });
        }
        let path = PathBuf::from(self.file_path.trim());
        if self.file_path.trim().is_empty() || !path.exists() {
            return None;
        }
        let files = recordings_in(&path).ok()?;
        let mut total = 0.0;
        for f in &files {
            let i = timegrapher_core::audio::info(f).ok()?;
            total += i.frames? as f64 / i.sample_rate as f64;
        }
        // The five minutes up to the time on show, when the time slider
        // has been moved back; else the start.
        let end = match self.view_end {
            Some(_) if self.live.is_some() => self.end_s(),
            _ => report::SPAN_S,
        }
        .clamp(report::SPAN_S.min(total), total);
        let about = std::fs::read_to_string(
            if path.is_dir() {
                path.clone()
            } else {
                path.parent()?.to_path_buf()
            }
            .join(MANIFEST),
        )
        .map(serde_json::Value::String)
        .unwrap_or_default();
        Some(report::Source {
            files,
            total_s: total,
            name: capture_label(&path),
            live: false,
            end_s: end,
            about,
        })
    }

    /// The session file for the take being kept: the watch, its calibre
    /// (with the wheels of a custom one), the lift angle and beat rate, and
    /// one entry per stretch in one position, in seconds into the sound
    /// saved, which `timegrapher session` reads as well.
    fn take_manifest(&self) -> Manifest {
        let mut m = Manifest {
            watch: (!self.watch.trim().is_empty()).then(|| self.watch.trim().to_string()),
            calibre: (!self.calibre.is_empty()).then(|| self.calibre.clone()),
            bph: self.bph.or(self.live.as_ref().and_then(|l| l.bph())),
            lift: Some(self.lift_deg),
            lift_known: Some(self.lift_known()),
            ..Default::default()
        };
        if let Some(w) = self.cal.custom_wheels(&self.calibre) {
            m.wheels = w.into_iter().collect();
        }
        // The positions in the saved sound's own time (the sound leaves
        // out the pauses the session's clock counts).
        m.recordings = self
            .session
            .take_ranges()
            .into_iter()
            .map(|(start_s, end_s, code)| RecordingEntry {
                file: ".".into(),
                position: Some(code),
                clock: Some("clock.csv".into()),
                start_s,
                end_s,
                ..Default::default()
            })
            .collect();
        // A Positions Test's own stretches (after its settling, for its
        // measuring time), so `timegrapher session` reads the seconds its
        // table showed.
        let wind_h = self.guided.as_ref().and_then(|t| t.wind_h());
        let test: Vec<RecordingEntry> = self
            .guided
            .as_ref()
            .map_or(&[][..], |t| t.stretches())
            .iter()
            .filter_map(|&(pos, from, to)| {
                Some(RecordingEntry {
                    file: ".".into(),
                    position: Some(pos.code().to_string()),
                    wind_h,
                    clock: Some("clock.csv".into()),
                    start_s: Some(self.session.audio_of(from)?),
                    end_s: Some(self.session.audio_of(to)?),
                    ..Default::default()
                })
            })
            .collect();
        if !test.is_empty() {
            m.recordings = test;
        }
        m
    }

    /// Take what a session file says about a recording: the watch, its
    /// calibre (a custom one is added to Your Calibres from its wheels),
    /// the lift angle and beat rate. Returns the positions it lists through
    /// the recording, oldest first, and the settling before each: a gap
    /// between one position's end and the next one's start.
    fn apply_manifest(
        &mut self,
        m: &Manifest,
        entries: &[RecordingEntry],
    ) -> (Stretches, Vec<(f64, f64)>) {
        if let Some(w) = &m.watch {
            self.watch = w.clone();
        }
        if let Some(c) = &m.calibre {
            let wheels: Vec<(String, f64)> =
                m.wheels.iter().map(|(n, &p)| (n.clone(), p)).collect();
            self.cal.adopt(c, &wheels);
            self.set_calibre(c.clone());
        }
        if let Some(l) = m.lift {
            self.set_lift(l);
            // Old app manifests did not record provenance. Preserve their
            // numeric/default inference, refined to actual calibre lift data.
            let legacy = l != DEFAULT_LIFT_DEG || self.cal.lift(&self.calibre).is_some();
            self.lift_context = (l, m.lift_known.unwrap_or(legacy));
        }
        if m.bph.is_some() {
            self.bph = m.bph;
        }
        let mut listed: Vec<(f64, Option<f64>, Position)> = entries
            .iter()
            .filter_map(|e| {
                let p = Position::from_name(e.position.as_deref()?)?;
                Some((e.start_s.unwrap_or(0.0), e.end_s, p))
            })
            .collect();
        listed.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out = Vec::new();
        let mut settles = Vec::new();
        for (i, &(start, _, p)) in listed.iter().enumerate() {
            // The position began where the one before it ended.
            let began = match i.checked_sub(1).and_then(|j| listed[j].1) {
                Some(end) if end < start => {
                    settles.push((end, start));
                    end
                }
                _ => start,
            };
            out.push((began, p));
        }
        if let Some(&(_, p)) = out.first() {
            self.position = p;
        }
        (out, settles)
    }

    /// Open a recording, analysed whole or replayed. With `ask`, unsaved
    /// sound from a microphone session is not thrown away without asking
    /// first.
    fn open_recording(&mut self, path: &Path, replay: bool, ask: bool) {
        if ask && self.session.mic() && self.needs_save_confirmation() {
            self.confirm = Some(Confirm::Open {
                path: path.to_path_buf(),
                replay,
            });
        } else if replay {
            self.start_replay(path);
        } else {
            self.start_batch(path);
        }
    }

    /// Play a recording through as if live. A folder plays its files one
    /// after the other in the order it is analysed in, with the watch from
    /// its session file when it has one.
    fn start_replay(&mut self, path: &Path) {
        self.stop();
        let inner = match take_folder(path) {
            Ok(p) => p,
            Err(e) => return self.open_failed(e),
        };
        let path = inner.as_path();
        // Reject a relevant invalid manifest before opening audio or a worker.
        let manifest = match read_manifest(path) {
            Ok(manifest) => manifest,
            Err(e) => return self.open_failed(e),
        };
        let files = match recordings_in(path) {
            Ok(f) => f,
            Err(e) => return self.open_failed(e),
        };
        let started = if path.is_dir() {
            let label = match files.len() {
                1 => format!("{} (1 file)", capture_label(path)),
                n => format!("{} ({n} files)", capture_label(path)),
            };
            capture::replay_files(&files, label)
        } else {
            capture::replay_file(path)
        };
        match started {
            Ok(c) => {
                let (stretches, settles) = match manifest {
                    Some((m, entries)) => {
                        self.keep_mic_setup();
                        self.set_watch_setup(WatchSetup::default());
                        self.open_note = None;
                        self.apply_manifest(&m, &entries)
                    }
                    None => {
                        self.fresh_setup(path, &files);
                        Default::default()
                    }
                };
                if let Some(n) = order_warning(&files) {
                    self.open_note = Some(match self.open_note.take() {
                        Some(o) => format!("{o} {n}"),
                        None => n,
                    });
                }
                if self.file_path.trim() != path.display().to_string() {
                    self.file_path = path.display().to_string();
                }
                self.reset_session(c.sample_rate, c.label.clone());
                self.session.replay();
                self.session.load(&stretches, &settles);
                if let (Some(live), Some(breaks)) =
                    (self.live.as_mut(), self.session.replay_breaks())
                {
                    live.set_breaks(breaks);
                }
                self.opened_path = path.display().to_string();
                c.set_speed(self.replay_speed);
                self.capture = Some(c);
                if let Some(n) = self.open_note.take() {
                    self.info_msg(n);
                }
            }
            Err(e) => self.open_failed(format!("Can't read {}: {e}", path.display())),
        }
    }

    /// Offered while a recording replays or after: the whole of it
    /// analysed, without opening it again.
    fn analyse_all_button(&self, ui: &mut egui::Ui) -> bool {
        ui.add(theme::secondary(ui, "Analyse All"))
            .on_hover_text(
                "Analyse the whole recording at once, as when it was opened, keeping the \
                 settings chosen under Watch",
            )
            .clicked()
    }

    /// A recording couldn't be opened: say why, and put the path box back
    /// to the one still shown.
    fn open_failed(&mut self, e: String) {
        self.error(e);
        if !self.opened_path.is_empty() {
            self.file_path = self.opened_path.clone();
        }
    }

    /// The microphone's watch set aside, the first time a recording is
    /// opened over it.
    fn keep_mic_setup(&mut self) {
        if self.mic_setup.is_none() {
            self.mic_setup = Some(self.watch_setup());
        }
    }

    /// A recording with no session file starts from the defaults, never
    /// the last take's watch, with its position from the file's name when
    /// it carries one (reverso_DU_…), and a note saying so.
    fn fresh_setup(&mut self, path: &Path, files: &[PathBuf]) {
        self.keep_mic_setup();
        self.set_watch_setup(WatchSetup::default());
        let from_name =
            position_in_name(path).or_else(|| files.first().and_then(|f| position_in_name(f)));
        if let Some(p) = from_name {
            self.position = p;
        }
        self.open_note = Some(match from_name {
            Some(p) => format!(
                "Position {} from the file name. The file has no watch settings, so the \
                 calibre is Not Set and the lift angle {DEFAULT_LIFT_DEG}°: set them under Watch.",
                p.label()
            ),
            None => format!(
                "The file has no watch settings, so the calibre is Not Set, the lift angle \
                 {DEFAULT_LIFT_DEG}° and the position Dial Up: set them under Watch."
            ),
        });
    }

    fn watch_setup(&self) -> WatchSetup {
        WatchSetup {
            watch: self.watch.clone(),
            calibre: self.calibre.clone(),
            lift_deg: self.lift_deg,
            lift_context: self.lift_context,
            bph: self.bph,
            position: self.position,
        }
    }

    fn set_watch_setup(&mut self, s: WatchSetup) {
        self.watch = s.watch;
        self.set_calibre(s.calibre);
        self.lift_deg = s.lift_deg;
        self.lift_context = s.lift_context;
        self.bph = s.bph;
        self.position = s.position;
    }

    fn start_batch(&mut self, path: &Path) {
        self.analyse_whole(path, None);
    }

    /// The recording on show, replayed in part, analysed whole: the
    /// settings chosen during the replay carry over.
    fn analyse_replayed(&mut self) {
        let path = PathBuf::from(&self.opened_path);
        let setup = self.watch_setup();
        self.analyse_whole(&path, Some(setup));
    }

    /// Analyse `path` whole. Without a session file it starts from
    /// `carried`, else from the defaults.
    fn analyse_whole(&mut self, path: &Path, carried: Option<WatchSetup>) {
        self.stop();
        // A folder a take was saved into holds the take's own folder.
        let inner = match take_folder(path) {
            Ok(p) => p,
            Err(e) => return self.open_failed(e),
        };
        if inner != path {
            self.info_msg(format!(
                "Opened the take saved inside {}: {}",
                capture_label(path),
                capture_label(&inner)
            ));
        }
        let path = inner.as_path();
        // Reject a relevant invalid manifest before opening audio or a worker.
        let manifest = match read_manifest(path) {
            Ok(manifest) => manifest,
            Err(e) => return self.open_failed(e),
        };
        if self.file_path.trim() != path.display().to_string() {
            self.file_path = path.display().to_string();
        }
        let files = match recordings_in(path) {
            Ok(f) => f,
            Err(e) => return self.open_failed(e),
        };
        let order_note = order_warning(&files);
        let mut info = match timegrapher_core::audio::info(&files[0]) {
            Ok(i) => i,
            Err(e) => return self.open_failed(format!("Can't read {}: {e}", files[0].display())),
        };
        for f in &files[1..] {
            match timegrapher_core::audio::info(f) {
                Ok(i) => info.frames = info.frames.zip(i.frames).map(|(a, b)| a + b),
                Err(e) => return self.open_failed(format!("Can't read {}: {e}", f.display())),
            }
        }
        // What the take's session file says: the watch, calibre, lift
        // angle, beat rate and positions, before the analysis uses them.
        // Nothing carries over from the take before.
        if manifest.is_none() {
            match carried {
                Some(s) => {
                    self.keep_mic_setup();
                    self.set_watch_setup(s);
                    self.open_note = None;
                }
                None => self.fresh_setup(path, &files),
            }
        } else {
            self.keep_mic_setup();
            self.set_watch_setup(WatchSetup::default());
            self.open_note = None;
        }
        let (stretches, settles) = match manifest {
            Some((m, entries)) => self.apply_manifest(&m, &entries),
            None => Default::default(),
        };
        if let Some(n) = &order_note {
            self.open_note = Some(match self.open_note.take() {
                Some(o) => format!("{o} {n}"),
                None => n.clone(),
            });
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
        cfg.analysis.lift_known = self.lift_known();
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
        self.opened_path = path.display().to_string();
        self.reset_session(info.sample_rate, label.clone());
        self.session.analyse();
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
            stretches,
            settles,
            order_note,
        });
    }

    /// Stop listening or replaying, and any analysis under way. A
    /// microphone session pauses, so it can carry on or be saved.
    fn stop(&mut self) {
        if self.capture.as_ref().is_some_and(|c| !c.is_file) {
            self.pause();
        } else if matches!(
            self.session.state(),
            session::State::Replaying | session::State::Analysing
        ) {
            let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
            let _ = self.session.stop(now);
        }
        self.capture = None;
        self.batch = None;
    }

    fn error(&mut self, m: String) {
        applog::error(&m);
        self.message = Some((plain_error(&m), true));
    }

    fn info_msg(&mut self, m: String) {
        applog::info(&m);
        self.message = Some((m, false));
    }

    fn refuse_faulted_recording(&mut self) -> bool {
        if let Some(fault) = self.session.recording_fault() {
            self.error(fault.description());
            true
        } else {
            false
        }
    }

    fn recording_failed(&mut self, mut reason: String) {
        // Release the input before attempting any remaining close/flush work.
        self.capture = None;
        let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        if self.session.state() == session::State::Listening {
            if let Ok(Err(e)) = self.session.pause(now) {
                reason.push_str(&format!("; closing the kept sound also failed: {e}"));
            }
        }
        self.session.recording_failed(now, reason);
        if let Some(fault) = self.session.recording_fault() {
            self.error(fault.description());
        }
    }

    /// Take in whatever audio has arrived.
    fn poll(&mut self) {
        let mut ended = false;
        let mut failure = None;
        if let (Some(cap), Some(live)) = (self.capture.as_ref(), self.live.as_mut()) {
            loop {
                match cap.rx.try_recv() {
                    Ok(Event::Audio(b)) => {
                        if let Some(r) = self.session.recorder.as_mut() {
                            if let Err(e) = r.write(&b) {
                                failure = Some(CaptureFailure::Recording(e.to_string()));
                                // Do not process more queued sound or let a later
                                // input error replace this primary recording error.
                                break;
                            }
                        }
                        self.audio_at = Some(Instant::now());
                        if let Some(t) = self.clock_track.as_mut() {
                            t.push(b.samples.len(), b.at);
                        }
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
                        if live.push(&b.samples) {
                            // While the watch settles after a move nothing is
                            // read; then the readings start afresh.
                            match self.session.settling_now(live.duration_s()) {
                                Some(true) => continue,
                                Some(false) => {
                                    live.new_stretch();
                                    continue;
                                }
                                None => {}
                            }
                            let r = live.reading(self.average_s);
                            if !self.session.replay_reading_allowed(r.time_s) {
                                continue;
                            }
                            self.trend.push(TrendPoint::of(r.time_s, &r));
                            for (&k, t) in self.other_trends.iter_mut() {
                                let o = live.reading(k as f64 / 1000.0);
                                t.push(TrendPoint::of(o.time_s, &o));
                            }
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
                        failure = Some(CaptureFailure::Input(format!(
                            "Input stopped. {}",
                            capture::explain_error(&e)
                        )));
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
        self.watch_for_moves();
        // A microphone that opened but sends nothing.
        if let Some(cap) = &self.capture {
            let quiet = self.audio_at.unwrap_or(self.opened_at).elapsed();
            if !cap.is_file && quiet.as_secs_f64() > 3.0 && failure.is_none() {
                failure = Some(CaptureFailure::Input(
                    "No sound is arriving from this input.".to_string(),
                ));
                ended = true;
            }
        }
        if let Some(CaptureFailure::Recording(error)) = failure {
            self.recording_failed(error);
        } else {
            let was_file = self.capture.as_ref().is_some_and(|c| c.is_file);
            if ended {
                self.stop();
                if was_file {
                    self.info_msg("End of the recording.".into());
                }
            }
            if let Some(CaptureFailure::Input(error)) = failure {
                // A close error during stop is a recording fault in its own
                // right, even when the input failure triggered the stop.
                let message = match self.session.recording_fault() {
                    Some(fault) => format!("{}\n{error}", fault.description()),
                    None => error,
                };
                self.error(message);
            }
        }

        // Beat lock gained or lost, for the log.
        let bph = self.live.as_ref().and_then(|l| l.bph());
        if bph != self.locked_bph && self.capture.is_some() {
            match bph {
                Some(b) => applog::info(&format!("locked on to the beat at {b} beats per hour")),
                None => applog::warn("lost the beat"),
            }
        }
        self.locked_bph = bph;

        let mut done = None;
        if let Some(b) = &self.batch {
            match b.rx.try_recv() {
                Ok(r) => done = Some(r),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => done = Some(Ok(None)),
            }
        }
        if let Some(r) = done {
            let (label, stretches, settles) = self
                .batch
                .take()
                .map(|b| (b.label.clone(), b.stretches.clone(), b.settles.clone()))
                .unwrap_or_default();
            match r {
                Ok(Some(log)) => self.show_log(log, label, stretches, settles),
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

    fn show_log(
        &mut self,
        log: BeatLog,
        label: String,
        stretches: Vec<(f64, Position)>,
        settles: Vec<(f64, f64)>,
    ) {
        let mut live = LiveAnalyzer::from_log(log, self.live_config());
        self.session.load(&[], &[]);
        self.session.analysed();
        if stretches.len() > 1 {
            // No reading fits across a move, nor across a settling's end.
            let mut breaks: Vec<f64> = stretches[1..]
                .iter()
                .map(|s| s.0)
                .chain(settles.iter().map(|s| s.1))
                .collect();
            breaks.sort_by(f64::total_cmp);
            breaks.dedup();
            live.set_breaks(breaks);
            self.session.load(&stretches, &settles);
        }
        if let Some(&(_, p)) = self.session.stretches().last() {
            self.position = p;
        }
        self.live = Some(live);
        self.steady = None;
        self.steady_job = None;
        self.rebuild_trend();
        self.source_label = label;
        self.anchor = None;
        self.view_end = None;
        let done = "Analysed the whole recording. Drag the strip or the time slider to look back.";
        let m = match self.open_note.take() {
            Some(n) => format!("{done} {n}"),
            None => done.to_string(),
        };
        self.info_msg(m);
    }

    /// Work the charts out again over the whole session, for a new
    /// averaging time or lift angle.
    fn rebuild_trend(&mut self) {
        self.other_trends.clear();
        self.trend = self.context_trend(self.average_s);
        self.sync_trends();
    }

    fn context_trend(&self, seconds: f64) -> Vec<TrendPoint> {
        let mut points = self
            .live
            .as_ref()
            .map_or_else(Vec::new, |l| trend_over(l, seconds));
        points.retain(|p| self.session.replay_reading_allowed(p.t));
        points
    }

    /// Clear the readings, retaining a replay's saved boundaries. Restart
    /// forgets all live breaks, but cannot erase the recording's history.
    fn clear_readings(&mut self) {
        if let Some(live) = self.live.as_mut() {
            live.restart();
            if let Some(breaks) = self.session.replay_breaks() {
                live.set_breaks(breaks);
            }
        }
        self.anchor = None;
        self.clear_trends();
    }

    fn latest_s(&self) -> f64 {
        self.live
            .as_ref()
            .map_or(0.0, |l| l.beats().last().map_or(l.duration_s(), |b| b.time))
    }

    /// The end of the last reading with a watch heard; the end when none
    /// was.
    fn last_heard_s(&self) -> f64 {
        self.trend
            .iter()
            .rev()
            .find(|p| p.heard)
            .map_or(f64::INFINITY, |p| p.t)
    }

    /// Over the whole take in the position the watch is in now, once it is
    /// stopped and longer than a few readings: the mean of each reading's
    /// points with a watch heard, their spread (standard deviation) and the
    /// time they cover.
    fn whole_take(&self, f: impl Fn(&TrendPoint) -> Option<f64>) -> Option<(f64, f64, f64)> {
        if self.running() || self.view_end.is_some() {
            return None;
        }
        let pts: Vec<(f64, f64)> = self
            .trend
            .iter()
            .filter(|p| p.heard && self.in_position(p.t))
            .filter_map(|p| Some((p.t, f(p)?)))
            .collect();
        let span = pts.last()?.0 - pts.first()?.0;
        if pts.len() < 6 || span < 3.0 * self.average_s {
            return None;
        }
        let n = pts.len() as f64;
        let mean = pts.iter().map(|p| p.1).sum::<f64>() / n;
        let sd = (pts.iter().map(|p| (p.1 - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
        Some((mean, sd, span))
    }

    fn end_s(&self) -> f64 {
        let latest = self.latest_s();
        self.view_end.unwrap_or(latest).min(latest)
    }

    fn reading(&self) -> Option<LiveReading> {
        self.reading_over(self.average_s)
    }

    /// The reading at the moment shown, averaged over `average` seconds.
    fn reading_over(&self, average: f64) -> Option<LiveReading> {
        let live = self.live.as_ref()?;
        // Stopped inside a settle: the last readings of the position before
        // the move, rather than nothing.
        if let Some(at) = self.stopped_in_settle() {
            return Some(live.reading_at(at, average));
        }
        // Nothing is read while the watch settles after a move.
        if self.settling_left().is_some() {
            return None;
        }
        let reading = if self.view_end.is_some() {
            live.reading_at(self.end_s(), average)
        } else if !self.running() {
            // Stopped: the last window with a watch heard, so the silence
            // after it is taken off the stand doesn't change the readings.
            live.reading_at(self.end_s().min(self.last_heard_s()), average)
        } else {
            live.reading(average)
        };
        self.session
            .replay_reading_allowed(reading.time_s)
            .then_some(reading)
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
        let mut d = rfd::FileDialog::new().set_title("Open a Saved Take or a Folder of Segments");
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

    /// Ask for the full-wind test's saved results and read them; None if
    /// the dialog was cancelled or the file can't be read (which says so).
    fn load_full_wind(&mut self) -> Option<guided::FullWind> {
        let mut d = rfd::FileDialog::new()
            .set_title("Load the Full-Wind Positions Test's Results")
            .add_filter("Positions Test Results", &["json"]);
        let start = self
            .session
            .saved_to()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from(&self.save_dir));
        if start.is_dir() {
            d = d.set_directory(&start);
        }
        let path = d.pick_file()?;
        let read = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| guided::FullWind::from_json(&t));
        match read {
            Ok(f) => Some(f),
            Err(e) => {
                self.error(format!("Couldn't load {}: {e}", path.display()));
                None
            }
        }
    }

    /// The question before throwing away unsaved sound.
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(what) = self.confirm.clone() else {
            return;
        };
        let mut choice = None;
        let unsaved = strip::fmt_time(self.unsaved_s());
        let metadata = self.metadata_unsaved();
        let fault = self.session.recording_fault().map(|f| f.description());
        egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                RichText::new(match what {
                    Confirm::NewSession => "Start a new session?",
                    Confirm::Quit => "Quit without saving?",
                    Confirm::Open { .. } => "Open the recording without saving?",
                })
                .font(theme::semibold(15.0)),
            );
            ui.add_space(4.0);
            ui.label(if let Some(fault) = &fault {
                fault.clone()
            } else if metadata && self.unsaved_s() <= 0.0 {
                "Changes to this session's settings or position history haven't been saved and will be thrown away.".to_string()
            } else if metadata {
                format!("This session's {unsaved} of sound and changes to its settings or position history haven't been saved and will be thrown away.")
            } else {
                format!("This session's {unsaved} of sound hasn't been saved and will be thrown away.")
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if fault.is_none() && ui.add(theme::primary(ui, "Save…")).clicked() {
                    choice = Some(0);
                }
                let go = match (&fault, &what) {
                    (Some(_), Confirm::Quit) => "Leave Files and Quit",
                    (Some(_), _) => "Leave Files and Continue",
                    (None, Confirm::NewSession | Confirm::Open { .. }) => "Don't Save",
                    (None, Confirm::Quit) => "Quit Without Saving",
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
                // Saved before: the rest goes to the same copy.
                self.save_again();
                if self.needs_save_confirmation() {
                    return; // the folder dialog was cancelled, or saving failed
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
            Confirm::Open { path, replay } => self.open_recording(&path, replay, false),
            Confirm::Quit => {
                self.may_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// The Watch pane: where the sound comes from and what to do with it,
    /// then the watch itself. Before anything is open it stands beside
    /// the ways in, so the microphone and the watch can be set first.
    fn watch_pane(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("watch pane")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Clear of the scroll bar.
                ui.set_max_width(ui.available_width() - 8.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                match self.input {
                    Input::Microphone => {
                        self.section(ui, "Microphone", help::MICROPHONE, Self::microphone_card)
                    }
                    Input::File => {
                        self.section(ui, "Recording", help::RECORDING, Self::recording_card)
                    }
                };
                ui.separator();
                ui.separator();
                self.section(ui, "Watch", help::WATCH, Self::watch_settings);
            });
    }

    /// Microphone or Recording, the choice at the top of the Watch pane.
    fn source_choice(&mut self, ui: &mut egui::Ui) {
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
    }

    /// The microphone menu, the level, listening and saving.
    fn microphone_card(&mut self, ui: &mut egui::Ui) {
        self.source_choice(ui);
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
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("device")
                .selected_text(short(&sel, 36))
                .width((ui.available_width() - 70.0).max(120.0))
                .show_ui(ui, |ui| {
                    opened = true;
                    if !listed {
                        ui.weak("Looking for microphones…");
                    }
                    for c in &choices {
                        let r = ui.selectable_value(&mut self.device, Some(c.id.clone()), &c.label);
                        if !c.detail.is_empty() {
                            r.on_hover_text(&c.detail);
                        }
                    }
                })
                .response
                .on_hover_text(
                    "The microphone to listen to. Show Every Input, below, lists every device \
                     the system offers.",
                );
            if ui
                .button("Rescan")
                .on_hover_text("Look for microphones again, after plugging one in")
                .clicked()
            {
                self.rescan();
            }
        });
        if opened && !listed {
            self.ensure_devices();
            ui.ctx().request_repaint();
        }
        ui.horizontal_wrapped(|ui| {
            if self.running() {
                if ui
                    .add(theme::secondary(ui, "Stop"))
                    .on_hover_text(
                        "Stop listening and let go of the microphone. Everything so far stays: \
                         look back through it with the time slider, save it, or Resume to \
                         carry on in the same session.",
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
                .add_enabled(
                    self.session.recording_fault().is_none(),
                    theme::primary(ui, "Start"),
                )
                .on_hover_text("Start listening to the microphone")
                .clicked()
            {
                self.start_microphone();
            }
            let mic_data = self.session.mic() && self.live.is_some();
            if mic_data
                && !self.running()
                && ui
                    .add(egui::Button::new("New Session").min_size(Vec2::new(0.0, 26.0)))
                    .on_hover_text(
                        "Clear everything and start again with the next watch or position",
                    )
                    .clicked()
            {
                if self.needs_save_confirmation() {
                    self.confirm = Some(Confirm::NewSession);
                } else {
                    self.new_session();
                }
            }
        });
        if self.session.allows(Action::Save) {
            let saved = self.session.saved_to().is_some();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(
                        egui::Button::new(if saved { "Save Again" } else { "Save…" })
                            .min_size(Vec2::new(0.0, 26.0)),
                    )
                    .on_hover_text(SAVE_HINT)
                    .clicked()
                {
                    self.save_again();
                }
                if saved
                    && ui
                        .add(egui::Button::new("Save As…").min_size(Vec2::new(0.0, 26.0)))
                        .on_hover_text(SAVE_AS_HINT)
                        .clicked()
                {
                    self.save_recording();
                }
            });
        }
        ui.add_space(2.0);
        self.microphone_settings(ui);
    }

    /// Opening a recording or a folder, replaying it and its speed.
    fn recording_card(&mut self, ui: &mut egui::Ui) {
        self.source_choice(ui);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Open…")
                .on_hover_text("Choose a WAV or FLAC recording and analyse it all")
                .clicked()
            {
                self.open_from_menu(false);
            }
            if ui
                .button("Open Folder…")
                .on_hover_text(FOLDER_RULE)
                .clicked()
            {
                self.open_from_menu(true);
            }
        });
        let r = ui
            .add(
                egui::TextEdit::singleline(&mut self.file_path)
                    .hint_text("or type a file's or folder's path and press Enter")
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text("A WAV or FLAC file, or a folder; Enter analyses it");
        let path = PathBuf::from(self.file_path.trim());
        if r.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !self.file_path.trim().is_empty()
        {
            self.open_recording(&path, false, true);
        }
        let replayed = matches!(
            self.session.state(),
            session::State::Replaying | session::State::ReplayStopped
        );
        ui.horizontal_wrapped(|ui| {
            if self.running() {
                if ui
                    .add(theme::destructive(ui, "Stop"))
                    .on_hover_text(
                        "Stop the replay. What it has shown so far stays, to look back \
                         through with the time slider.",
                    )
                    .clicked()
                {
                    self.stop();
                }
                if self.analyse_all_button(ui) {
                    self.analyse_replayed();
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
                if ui
                    .add_enabled(path.exists(), theme::secondary(ui, "Replay"))
                    .on_hover_text(
                        "Play the recording through as if live, from the start, at the \
                         speed set beside it. A folder plays its files one after the other.",
                    )
                    .on_disabled_hover_text("Open a recording or a folder first")
                    .clicked()
                {
                    self.open_recording(&path, true, true);
                }
                if replayed && !self.opened_path.is_empty() && self.analyse_all_button(ui) {
                    self.analyse_replayed();
                }
            }
        });
        let mut speed = self.replay_speed;
        theme::row(
            ui,
            "Replay Speed",
            Some(
                "How fast Replay plays the recording, times its own speed. Faster gets \
                 through a long take sooner; the readings are the same. It can be changed \
                 while a replay runs.",
            ),
            |ui| {
                egui::ComboBox::from_id_salt("replay speed")
                    .width(ui.available_width())
                    .selected_text(speed_text(speed))
                    .show_ui(ui, |ui| {
                        for x in REPLAY_SPEEDS {
                            ui.selectable_value(&mut speed, x, speed_text(x));
                        }
                    });
            },
        );
        if speed != self.replay_speed {
            self.replay_speed = speed;
            if let Some(c) = self.capture.as_ref().filter(|c| c.is_file) {
                c.set_speed(speed);
            }
        }
    }

    /// A group of settings in the Watch pane under its caption, which
    /// folds it away.
    fn section(
        &mut self,
        ui: &mut egui::Ui,
        title: &'static str,
        help: &[&str],
        settings: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        let mut folded = self.folded.contains(title);
        theme::section_header(ui, title, None, help, &mut folded);
        if folded {
            self.folded.insert(title.to_string());
        } else {
            self.folded.remove(title);
            ui.indent(title, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                settings(self, ui);
            });
        }
    }

    /// A pane's toolbar: its settings in a row and its "?" at the right.
    /// The charts keep theirs in the tab bar (see `top_bar_right_ui`), so
    /// a short chart loses no height; false for them.
    fn pane_tools(&mut self, ui: &mut egui::Ui, pane: Pane) -> bool {
        let title = pane.title();
        match pane {
            Pane::Strip => theme::pane_toolbar(ui, title, help::STRIP, |ui| {
                self.strip_settings(ui);
                self.pane_average_setting(ui, Pane::Strip);
            }),
            Pane::Sound => theme::pane_toolbar(ui, title, help::PROFILE, |ui| {
                self.profile_settings(ui);
                self.pane_average_setting(ui, Pane::Sound);
            }),
            Pane::Scope => theme::pane_toolbar(ui, title, help::SCOPE, |ui| {
                scope::settings(ui, &mut self.scope_view)
            }),
            p if p.steady_view().is_some() => {
                theme::pane_toolbar(ui, title, help::STEADINESS, |ui| {
                    self.steady_settings(ui, p)
                })
            }
            Pane::Isochronism => theme::pane_toolbar(ui, title, help::ISOCHRONISM, |_| {}),
            Pane::PositionsTest => theme::pane_toolbar(ui, title, help::POSITIONS_TEST, |ui| {
                self.positions_settings(ui)
            }),
            p if Pane::HISTOGRAMS.contains(&p) => {
                theme::pane_toolbar(ui, title, help::HISTOGRAMS, |ui| {
                    self.span_setting(ui);
                    self.histogram_settings(ui);
                    self.pane_average_setting(ui, p);
                })
            }
            _ => return false,
        }
        true
    }

    /// Average Over for one pane: Readings' time, or one of its own.
    fn pane_average_setting(&mut self, ui: &mut egui::Ui, pane: Pane) {
        theme::row(ui, "Average Over", Some(PANE_AVERAGE_HINT), |ui| {
            self.pane_average_choice(ui, pane);
        });
    }

    /// The list behind a pane's Average Over: As Readings, or a time of
    /// its own.
    fn pane_average_choice(&mut self, ui: &mut egui::Ui, pane: Pane) {
        let own = self.pane_average.get(&pane).copied();
        let readings = fields::duration(self.average_s);
        let mut choice = own;
        egui::ComboBox::from_id_salt(("pane average", pane))
            .selected_text(match own {
                Some(s) => fields::duration(s),
                None => format!("As Readings ({readings})"),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut choice, None, format!("As Readings ({readings})"));
                for a in AVERAGES {
                    ui.selectable_value(&mut choice, Some(a), fields::duration(a));
                }
            })
            .response
            .on_hover_text(PANE_AVERAGE_HINT);
        if choice != own {
            self.set_pane_average(pane, choice);
        }
    }

    /// A Steadiness pane's Rate, Amplitude and Beat Error switches.
    fn steady_settings(&mut self, ui: &mut egui::Ui, pane: Pane) {
        let Some(view) = pane.steady_view() else {
            return;
        };
        steady::series_switches(ui, self.steady_series.entry(view).or_insert([true; 3]));
        if view == steady::View::Periods {
            ui.separator();
            theme::toggle(ui, "Log Scale", &mut self.periods_log).on_hover_text(
                "Draw the scores on a log scale, so a small peak beside a tall one still shows",
            );
        }
        if view == steady::View::Fft {
            ui.separator();
            theme::toggle(ui, "Timing", &mut self.fft_timing).on_hover_text(
                "Draw rate as the beat timing's spectrum, in ms, at every period, rather than \
                 the swing it makes in 10 s rate readings, in s/d",
            );
        }
    }

    fn update_steady(&mut self, ctx: &egui::Context) -> f64 {
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
            let joined = !self.session.stretches().is_empty();
            if let Some(log) = log.map(|l| if joined { steady::join(&l, &spans) } else { l }) {
                let wheels = self.calibre_wheels();
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx = ctx.clone();
                let stage = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
                self.steady_work = (stage.clone(), std::time::Instant::now());
                std::thread::spawn(move || {
                    let _ = tx.send(steady::compute_with(&log, wheels, &stage));
                    ctx.request_repaint();
                });
                self.steady_job = Some(rx);
            }
        }
        dur
    }

    /// While the steadiness tests run: a spinner, the step they're on and
    /// how long they've taken, for the Steadiness and Isochronism panes.
    /// Nothing when they aren't running.
    fn steady_working(&self, ui: &mut egui::Ui) {
        if self.steady_job.is_none() {
            return;
        }
        let pal = theme::pal(ui);
        let (stage, since) = &self.steady_work;
        let stage = stage.load(std::sync::atomic::Ordering::Relaxed);
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(12.0).color(pal.text_secondary));
            ui.label(
                RichText::new(format!(
                    "{}… {}",
                    steady::stage_text(stage),
                    strip::fmt_time(since.elapsed().as_secs_f64())
                ))
                .small()
                .color(pal.text_secondary),
            );
        })
        .response
        .on_hover_text(
            "The tests run in the background over every reading of the session, so a \
             session of many hours takes a while; what is shown meanwhile is the last \
             answer, if there is one.",
        );
    }

    fn isochronism_pane(&mut self, ui: &mut egui::Ui) {
        let dur = self.update_steady(ui.ctx());
        let pal = theme::pal(ui);
        self.steady_working(ui);
        match &self.steady {
            // Scrolling when the pane is too short for the chart and the
            // whole table.
            Some(s) => {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                        isochronism::ui(
                            ui,
                            s.report.isochronism.as_ref(),
                            s.report.wind_down.as_ref(),
                        )
                    });
            }
            None if self.steady_job.is_some() => {}
            None => {
                ui.label(
                    RichText::new(
                        self.steady_wait(dur)
                            + " The full-wind comparison needs 2 hours, and 24 for the full test.",
                    )
                    .small()
                    .color(pal.text_secondary),
                );
            }
        }
    }

    /// What the steadiness tests wait for, in the terms of what is being
    /// done: a Positions Test never stays long enough in one position.
    fn steady_wait(&self, dur: f64) -> String {
        if self.guided.is_some() {
            "Needs 5 minutes of beats in one position, longer than a Positions Test \
             stays in each. Record one position for 5 minutes or more to see it."
                .to_string()
        } else {
            format!(
                "Needs 5 minutes of beats in one position; {} so far.",
                strip::fmt_time(dur)
            )
        }
    }

    fn steady_pane(&mut self, ui: &mut egui::Ui, view: steady::View) {
        let dur = self.update_steady(ui.ctx());
        let upto = self.steady.as_ref().map(|s| s.upto_s);
        let working = self.steady_job.is_some();
        let of = if self.session.stretches().is_empty() {
            String::new()
        } else {
            format!("{} ", self.position.name())
        };
        self.steady_working(ui);
        let status = match upto {
            Some(u) => format!("Tested over {} of {of}beats", strip::fmt_time(u)),
            None if working => String::new(),
            None => self.steady_wait(dur),
        };
        steady::draw(
            ui,
            self.steady.as_ref(),
            &status,
            view,
            &self.steady_series.get(&view).copied().unwrap_or([true; 3]),
            steady::Opts {
                periods_log: self.periods_log,
                fft_timing: self.fft_timing,
            },
        );
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

    /// Whether the distributions cover the whole session or the strip's
    /// length.
    fn span_setting(&mut self, ui: &mut egui::Ui) {
        theme::row(
            ui,
            "Time Span",
            Some("How much of the session the distributions cover"),
            |ui| {
                theme::segmented(
                    ui,
                    &mut self.hist_span_of_strip,
                    &[
                        (false, "Session", SPAN_SESSION_HINT),
                        (true, "Strip", SPAN_STRIP_HINT),
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
            self.clear_trends();
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
                    .selected_text(pos.label())
                    .show_ui(ui, |ui| {
                        for p in Position::ALL {
                            ui.selectable_value(&mut pos, p, p.label());
                        }
                    });
            },
        );
        if pos != self.position {
            self.change_position(pos);
        }
    }

    /// While listening, a break of more than [`MOVE_GAP_S`] in the beats
    /// with no position change in it may be the watch being moved: ask.
    fn watch_for_moves(&mut self) {
        if !(self.session.mic() && self.running()) {
            self.last_beat_seen = 0.0;
            return;
        }
        let Some(live) = &self.live else { return };
        let beats = live.beats();
        let Some(last) = beats.last().map(|b| b.time) else {
            return;
        };
        let seen = self.last_beat_seen;
        if last <= seen {
            return;
        }
        self.last_beat_seen = last;
        if let Some(gap) = sound_break(beats, seen, &self.session.stretches()) {
            self.move_prompt = Some(gap);
        }
    }

    /// The position picked: a new stretch, noted in the recording.
    fn change_position(&mut self, pos: Position) {
        if self.refuse_faulted_recording() {
            return;
        }
        self.move_prompt = None;
        self.set_position(pos);
        if let Some(r) = self.session.recorder.as_mut() {
            // note appends in memory before writing scratch metadata, so the
            // edit remains unsaved even if that scratch write fails.
            let _ = r.note(&format!("position {}", pos.code()));
            self.session.note_metadata_change();
        }
        self.note_settings();
    }

    /// Put the watch in a new position. Listening (or paused), the session
    /// carries on with a new stretch: the strip keeps its beats and marks
    /// the stretch, and the readings, the profile, the distributions and the
    /// steadiness tests take only the beats in the new position.
    fn set_position(&mut self, pos: Position) {
        let old = self.position;
        applog::info(&format!("position {} to {}", old.code(), pos.code()));
        self.position = pos;
        let going = self.session.allows(Action::Move);
        if let (Some(l), true) = (self.live.as_mut(), going) {
            l.new_stretch();
            let at = l.duration_s();
            let _ = self.session.move_to(at, old, pos);
            self.steady = None;
            self.steady_job = None;
        }
    }

    /// Seconds of settling still to come at the time on show.
    fn settling_left(&self) -> Option<f64> {
        self.session.settling_left(self.end_s())
    }

    /// The stretches the strip draws faintly, each with what the ticker
    /// says over it: the settling after each move, and during and after a
    /// Positions Test the time it didn't use, in place of the moves'
    /// settling it covers.
    fn strip_settles(&self) -> Vec<(f64, f64, &'static str)> {
        let test = &self.guided_unused;
        let mut out: Vec<(f64, f64, &'static str)> = self
            .session
            .settles()
            .into_iter()
            .filter(|s| !test.iter().any(|t| s.0 < t.1 && t.0 < s.1))
            .map(|(a, b)| (a, b, "Settling"))
            .chain(test.iter().copied())
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    /// When stopped (and not looking back) inside the settle after a
    /// move: when that move was, the end of the position before it.
    fn stopped_in_settle(&self) -> Option<f64> {
        if self.running() || self.view_end.is_some() {
            return None;
        }
        let t = self.end_s();
        self.session
            .settles()
            .into_iter()
            .find(|s| t >= s.0 && t < s.1)
            .map(|s| s.0)
    }

    /// The stretches in the position the watch is in now, which the
    /// profile, the distributions and the steadiness tests are made over.
    fn current_spans(&self) -> Vec<(f64, f64)> {
        self.session.current_spans(self.position)
    }

    /// Whether a reading ending at `t` comes from the position the watch is
    /// in now.
    fn in_position(&self, t: f64) -> bool {
        self.session.in_position(t, self.position)
    }

    /// The stretches to mark on the strip and the charts, once there is
    /// more than one or a Positions Test has begun: start, end and the
    /// position's name.
    fn position_bands(&self) -> Vec<(f64, f64, &'static str)> {
        let bands = self.session.position_bands(self.position);
        if bands.is_empty() && self.in_test() {
            return self
                .session
                .position_spans(self.position)
                .into_iter()
                .map(|(a, b, p)| (a.max(0.0), b, p.label()))
                .collect();
        }
        bands
    }

    /// Whether the strip and charts mark a Positions Test's phases: from
    /// its start until the next test or a new session.
    fn in_test(&self) -> bool {
        !self.guided_unused.is_empty() || !self.guided_measured.is_empty()
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
                self.clear_readings();
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

    /// The menus across the very top, File, View and Help, and beside
    /// them an icon for each pane that shows or hides it.
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| self.file_menu(ui));
            ui.menu_button("View", |ui| self.view_menu(ui));
            ui.menu_button("Help", |ui| self.help_menu(ui));
            ui.add_space(16.0);
            self.pane_icons(ui);
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        let command = |key| egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key);
        if ui
            .add(
                egui::Button::new("Open Recording…")
                    .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::O))),
            )
            .on_hover_text("Choose a WAV or FLAC recording and analyse it all")
            .clicked()
        {
            ui.close();
            self.open_from_menu(false);
        }
        if ui
            .button("Open Folder…")
            .on_hover_text(FOLDER_RULE)
            .clicked()
        {
            ui.close();
            self.open_from_menu(true);
        }
        ui.separator();
        let saved = self.session.saved_to().is_some();
        let can_save = self.session.allows(Action::Save);
        if ui
            .add_enabled(
                can_save,
                egui::Button::new(if saved { "Save Again" } else { "Save…" })
                    .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::S))),
            )
            .on_hover_text(SAVE_HINT)
            .on_disabled_hover_text("Nothing to save yet: listen to a watch first")
            .clicked()
        {
            ui.close();
            self.save_again();
        }
        if ui
            .add_enabled(
                can_save && saved,
                egui::Button::new("Save As…")
                    .shortcut_text(ui.ctx().format_shortcut(&save_as_shortcut())),
            )
            .on_hover_text(SAVE_AS_HINT)
            .on_disabled_hover_text("Save once first; Save… asks where")
            .clicked()
        {
            ui.close();
            self.save_recording();
        }
        // Anything shown, from the microphone or a recording, can be
        // cleared once nothing is running.
        if ui
            .add_enabled(
                self.live.is_some() && !self.running(),
                egui::Button::new("New Session"),
            )
            .on_hover_text("Clear everything and start again with the next watch or position")
            .on_disabled_hover_text(if self.running() {
                "Stop listening or replaying first"
            } else {
                "Nothing to clear yet: listen to a watch or open a recording first"
            })
            .clicked()
        {
            ui.close();
            if self.needs_save_confirmation() {
                self.confirm = Some(Confirm::NewSession);
            } else {
                self.new_session();
            }
        }
        ui.separator();
        if ui
            .add(
                egui::Button::new("Quit")
                    .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::Q))),
            )
            .clicked()
        {
            ui.close();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.show_readings, "Readings")
            .on_hover_text("The rate, amplitude and beat error cards");
        let mut pane_box = |ui: &mut egui::Ui, pane: Pane| {
            let mut on = pane_visible(&self.panes.tiles, pane);
            if ui
                .checkbox(&mut on, pane.title())
                .on_hover_text(pane.hint())
                .changed()
            {
                set_pane_visible(&mut self.panes.tiles, pane, on);
            }
        };
        for pane in Pane::ALL {
            if !Pane::STEADY.contains(&pane) {
                pane_box(ui, pane);
            }
        }
        ui.menu_button("Steadiness", |ui| {
            for pane in Pane::STEADY {
                pane_box(ui, pane);
            }
        });
        ui.separator();
        if ui
            .button("Reset Panes")
            .on_hover_text("Show every pane and put them back where they started")
            .clicked()
        {
            ui.close();
            self.show_readings = true;
            self.panes = default_layout(self.strip.horizontal);
        }
        ui.separator();
        ui.menu_button("Appearance", |ui| {
            let mut t = ui.ctx().options(|o| o.theme_preference);
            for (v, name, hint) in [
                (
                    egui::ThemePreference::System,
                    "Auto",
                    "Follow the system's light or dark setting",
                ),
                (egui::ThemePreference::Light, "Light", "Always light"),
                (egui::ThemePreference::Dark, "Dark", "Always dark"),
            ] {
                ui.radio_value(&mut t, v, name).on_hover_text(hint);
            }
            if t != ui.ctx().options(|o| o.theme_preference) {
                ui.ctx().set_theme(t);
                self.theme = t;
            }
        });
    }

    fn help_menu(&mut self, ui: &mut egui::Ui) {
        let checking = self.updates.checking();
        if ui
            .add_enabled(
                !checking,
                egui::Button::new(if checking {
                    "Checking for Updates…"
                } else {
                    "Check for Updates…"
                }),
            )
            .on_hover_text(
                "Ask GitHub whether a newer Timegrapher is published. Nothing downloads or \
                 installs itself.",
            )
            .clicked()
        {
            ui.close();
            self.updates.start(ui.ctx(), true);
        }
        ui.checkbox(&mut self.check_at_start, "Check for Updates at Start")
            .on_hover_text(
                "Ask GitHub whether a newer Timegrapher is published each time the app \
                 starts, and say so if there is. It sends no account or identifier, and \
                 nothing installs itself.",
            );
        ui.separator();
        if ui
            .button("Report a Problem…")
            .on_hover_text(
                "Make a zip of what went wrong, with up to five minutes of the sound, to \
                 attach to a GitHub issue. The app sends nothing itself.",
            )
            .clicked()
        {
            ui.close();
            self.open_report();
        }
        if cfg!(target_os = "linux") {
            ui.separator();
            if ui
                .button("Add to Applications Menu")
                .on_hover_text(
                    "Add Timegrapher to this computer's applications menu, with its icon, \
                     so the desktop shows the icon on the window and in the task bar. It \
                     writes one entry and one icon in your home folder.",
                )
                .clicked()
            {
                ui.close();
                match crate::desktop::install() {
                    Ok(p) => self.info_msg(format!(
                        "Added to the applications menu ({}). The icon shows from the next \
                         start.",
                        p.display()
                    )),
                    Err(e) => self.error(format!("Couldn't add it to the menu: {e}")),
                }
            }
        }
    }

    /// Open a recording, or a folder of segments, chosen in a dialog.
    fn open_from_menu(&mut self, folder: bool) {
        let picked = if folder {
            self.open_folder_dialog()
        } else {
            self.open_file_dialog()
        };
        if let Some(p) = picked {
            self.input = Input::File;
            self.open_recording(&p, false, true);
        }
    }

    /// A button for each pane, its icon pressed while the pane shows.
    fn pane_icons(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.x = 2.0;
        let pal = theme::pal(ui);
        if theme::icon_toggle(
            ui,
            theme::Icon::Readings,
            self.show_readings,
            pal.text_secondary,
        )
        .on_hover_text("Readings: the rate, amplitude and beat error cards")
        .clicked()
        {
            self.show_readings = !self.show_readings;
        }
        // The panes that stand alone, then the charts, the distributions and
        // the Steadiness views, each group under its name.
        let alone: Vec<Pane> = Pane::ALL
            .into_iter()
            .filter(|p| {
                !Pane::CHARTS.contains(p)
                    && !Pane::HISTOGRAMS.contains(p)
                    && !Pane::STEADY.contains(p)
            })
            .collect();
        let groups: [(&str, &str, &[Pane]); 4] = [
            ("", "", &alone),
            (
                "Charts",
                "Each reading over the session, as a line",
                &Pane::CHARTS,
            ),
            (
                "Distributions",
                "How often each value of a reading came up",
                &Pane::HISTOGRAMS,
            ),
            (
                "Steadiness",
                "How the rate, amplitude and beat error each change over the session, from \
                 the same tests as `timegrapher series`: each view a pane of its own, so two \
                 can sit side by side",
                &Pane::STEADY,
            ),
        ];
        for (name, hint, panes) in groups {
            if !name.is_empty() {
                ui.add_space(6.0);
                ui.separator();
                ui.label(RichText::new(name).small().color(pal.text_tertiary))
                    .on_hover_text(hint);
            }
            for &pane in panes {
                let on = pane_visible(&self.panes.tiles, pane);
                let (icon, color) = pane_icon(pane, pal);
                if theme::icon_toggle(ui, icon, on, color)
                    .on_hover_text(format!("{}: {}", pane.title(), pane.hint()))
                    .clicked()
                {
                    set_pane_visible(&mut self.panes.tiles, pane, !on);
                }
            }
        }
    }

    fn clipping(&self) -> bool {
        self.clipped_at
            .is_some_and(|t| t.elapsed().as_secs_f64() < 3.0)
    }

    /// The input level in words and the colour to show it in: red only
    /// while the sound clips.
    fn level_text(&self, ui: &egui::Ui) -> (String, Color32) {
        let pal = theme::pal(ui);
        let peak = self.level.peak_dbfs.max(-99.0);
        if self.clipping() {
            (format!("peak {peak:.0} dBFS, clipping"), pal.bad)
        } else {
            (format!("peak {peak:.0} dBFS"), pal.text_secondary)
        }
    }

    /// The level meter: a bar to the latest peak, red while the sound
    /// clips, with plain reference marks at -10 and -6 dBFS.
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
            } else {
                pal.text_secondary
            };
            let mut fill = rect;
            fill.set_width((rect.width() * frac).max(height));
            p.rect_filled(fill, r, color);
        }
        for db in REFERENCE_DBFS {
            let x = rect.left() + rect.width() * ((db + 60.0) / 60.0) as f32;
            p.line_segment(
                [
                    egui::pos2(x, rect.top() - 2.0),
                    egui::pos2(x, rect.bottom() + 2.0),
                ],
                egui::Stroke::new(1.5_f32, pal.text_tertiary),
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

    /// The input level control and meter, under Microphone in the Watch pane.
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
                        applog::info(&format!("input level set to {pct:.0}%"));
                        self.read_gain();
                    } else if r.changed() {
                        if let Some(s) = self.gain_state.as_mut() {
                            s.level = pct / 100.0;
                        }
                    }
                });
                // Automatic gain shows only when it is on, with the way to
                // turn it off.
                if st.agc == Some(true) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Automatic gain: on").small().color(pal.text))
                            .on_hover_text(
                                "The microphone sets its own level as the sound changes, \
                                 raising it between ticks.",
                            );
                        if ui
                            .button("Turn It Off")
                            .on_hover_text("Turn the microphone's automatic gain off")
                            .clicked()
                        {
                            self.gain_error = g.set_agc(false).err();
                            self.read_gain();
                        }
                    });
                }
            }
            _ => {
                theme::row(ui, "Input Level", Some(hint), |ui| {
                    // Wrapped, so a longer line doesn't widen the Watch pane.
                    ui.add(
                        egui::Label::new(
                            RichText::new(if cfg!(target_os = "linux") {
                                "not adjustable here"
                            } else {
                                "set it in the system's sound settings"
                            })
                            .color(pal.text_secondary),
                        )
                        .wrap(),
                    );
                });
            }
        }
        let level_hint = "Loudest sample in the last half second, with plain marks at \
                          -10 and -6 dBFS for reference. The bar turns red when the \
                          sound clips.";
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
    /// The train the Steadiness tests name cycles after, when a calibre is
    /// picked; `None` leaves them to the beat rate's usual wheels.
    fn calibre_wheels(&self) -> Option<Vec<Wheel>> {
        self.cal.wheels(&self.calibre)
    }

    /// Pick a calibre: its lift angle, where the list or the user gives
    /// one, and its wheels for naming the cycles Steadiness finds.
    fn set_calibre(&mut self, name: String) {
        if name == self.calibre {
            return;
        }
        self.calibre = name;
        if let Some(l) = self.cal.lift(&self.calibre) {
            self.set_lift(l);
            self.lift_context = (l, true);
        }
        let n = self.calibre.clone();
        self.cal.remember(&n);
        // Named afresh with the new wheels.
        self.steady = None;
        self.steady_job = None;
    }

    /// The Calibre row under Watch, and what is known about the one picked.
    /// Manage Calibres, when open, over everything else.
    fn manage_calibres(&mut self, ctx: &egui::Context) {
        let current = self.calibre.clone();
        let out = self.cal.manager(ctx, &current, env!("CARGO_PKG_VERSION"));
        let name = out.name.clone();
        self.calibre_changed(&name, out.change);
        if let Some(p) = out.picked {
            self.set_calibre(p);
        }
    }

    /// Act on what was changed for the calibre `name`: only this watch's
    /// calibre moves anything outside the list.
    fn calibre_changed(&mut self, name: &str, change: calibre_ui::Change) {
        if name.is_empty() || name != self.calibre {
            return;
        }
        if let Some(n) = change.renamed {
            self.calibre = n.clone();
            self.cal.recent.retain(|r| r != name);
            self.cal.remember(&n);
        }
        if change.deleted {
            self.cal.recent.retain(|r| r != name);
            self.set_calibre(String::new());
        }
        if let Some(l) = change.lift {
            self.set_lift(l);
            self.lift_context = (l, true);
        }
        if change.wheels {
            self.steady = None;
            self.steady_job = None;
        }
    }

    fn calibre_settings(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let current = self.calibre.clone();
        let picked = theme::row(
            ui,
            "Calibre",
            Some(
                "The movement, so that the lift angle is filled in and cycles Steadiness \
                 finds are named after the right wheels. The menu offers your short list \
                 and recent picks; Manage Calibres has every calibre, to search and to \
                 choose your short list.",
            ),
            |ui| self.cal.picker(ui, &current),
        );
        if let Some(p) = picked {
            self.set_calibre(p);
        }
        let current = self.calibre.clone();
        self.cal.manage_button(ui, &current);
        if self.cal.find(&self.calibre).is_some() {
            let heard = self.live.as_ref().and_then(|l| l.bph());
            if let Some((b, cb)) = heard
                .zip(self.cal.bph(&self.calibre))
                .filter(|&(b, cb)| b != cb)
            {
                ui.label(
                    RichText::new(format!(
                        "The {} beats at {cb} bph, but this watch beats at {b}.",
                        self.calibre
                    ))
                    .small()
                    .color(pal.text_secondary),
                );
            }
            let current = self.calibre.clone();
            let change = self.cal.panel(ui, &current, env!("CARGO_PKG_VERSION"));
            self.calibre_changed(&current, change);
        } else if self.lift_deg == DEFAULT_LIFT_DEG && !self.lift_known() {
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "No calibre: the lift angle is the default {DEFAULT_LIFT_DEG}°, and \
                         every amplitude is worked out from it. A calibre picked above, or a \
                         lift angle typed below, replaces it."
                    ))
                    .small()
                    .color(pal.text_secondary),
                )
                .wrap(),
            );
        }
    }

    /// Restore known/assumed context only for the value it described. Numeric
    /// edits and reverts keep the existing default-angle inference; this does
    /// not introduce a new way to explicitly confirm an unchanged 52° in UI.
    fn lift_known(&self) -> bool {
        if self.lift_deg == self.lift_context.0 {
            self.lift_context.1
        } else {
            self.lift_deg != DEFAULT_LIFT_DEG
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
        if self.session.recording_fault().is_some() {
            return;
        }
        if let Some(r) = self.session.recorder.as_mut() {
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

    /// The three readings, each in a card: a caption with what limits the
    /// reading, the figure large with its ± beside it, and a line of detail.
    /// The reading cards across the top: Readings with its Average Over,
    /// then Rate, Amplitude, Beat Error and Beat Rate, all one width.
    /// Returns a new Average Over when one was picked.
    fn readouts(&self, ui: &mut egui::Ui) -> Readouts {
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
        // A reading whose beats scatter like noise is no watch: no figures
        // then, only a note saying so.
        let heard = r.as_ref().is_none_or(|r| r.watch_heard || r.bph.is_none());
        let unheard_line = r.as_ref().map(no_regular_beat).unwrap_or_default();
        let r = r.as_ref().filter(|_| heard);
        let caveats = r.map_or(&[][..], |r| &r.caveats[..]);
        // Each reading's ±, as an analysis of the recording gives them
        // (core `measure`).
        let pm = r.and_then(|r| r.rate_error_s_per_day);
        let be_pm = r.and_then(|r| r.beat_error_unlock_error_ms);
        let lift_now = self.lift_deg;
        let beats = r.map_or(0, |r| r.beats_used);
        // Over the whole take, once it is stopped: the mean and spread of
        // the readings beside the last window's figure.
        let whole_line = |ui: &mut egui::Ui, mean: String, sd: String, span: f64| {
            ui.label(
                RichText::new(format!(
                    "whole take {mean}, spread {sd} over {}",
                    strip::fmt_time(span)
                ))
                .small()
                .color(pal.text_secondary),
            )
            .on_hover_text(
                "The big figure is the last stretch of Average Over. This is every reading \
                 of the take in this position with a watch heard, settling left out: their \
                 mean, and their spread as a standard deviation.",
            );
        };
        let unheard = |ui: &mut egui::Ui| {
            ui.label(
                RichText::new(&unheard_line)
                    .small()
                    .color(pal.text_secondary),
            )
            .on_hover_text(help::NO_REGULAR_BEAT);
        };
        // The cards share one height, the tallest's on the last frame, so a
        // card with an extra line doesn't leave the row ragged.
        let height_id = ui.id().with("readout height");
        let height = ui
            .ctx()
            .data(|d| d.get_temp::<f32>(height_id))
            .unwrap_or(0.0);
        let tallest = std::cell::Cell::new(0f32);
        let card = |ui: &mut egui::Ui,
                    title: &str,
                    help: &[&str],
                    facts: Vec<(String, String)>,
                    body: &dyn Fn(&mut egui::Ui)| {
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
                        if !facts.is_empty() {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let short: Vec<&str> =
                                        facts.iter().map(|f| f.0.as_str()).collect();
                                    let long: Vec<&str> =
                                        facts.iter().map(|f| f.1.as_str()).collect();
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(short.join(" · "))
                                                .small()
                                                .color(pal.text_secondary),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(long.join("\n\n"));
                                },
                            );
                        }
                    });
                    body(ui);
                    let bottom = ui.cursor().top() - ui.spacing().item_spacing.y;
                    tallest.set(tallest.get().max(bottom - top));
                });
        };
        ui.spacing_mut().item_spacing.x = theme::GAP;
        let gap = theme::GAP;
        let total = ui.available_width();
        let wide = ((total - 4.0 * gap) / 5.0).max(100.0);
        let origin = ui.cursor().min;
        let mut cols: Vec<egui::Ui> = [wide; 5]
            .iter()
            .scan(origin.x, |x, &w| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(*x, origin.y), Vec2::new(w, 2000.0));
                *x += w + gap;
                Some(rect)
            })
            .map(|rect| {
                ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                )
            })
            .collect();
        let picked = std::cell::Cell::new(None);
        let keep_clock = std::cell::Cell::new(false);
        let measured = self.clock_measured().filter(|_| self.clock.is_none());
        card(
            &mut cols[0],
            "Readings",
            help::READINGS,
            Vec::new(),
            &|ui| {
                let small = |t: &str| RichText::new(t).small().color(pal.text_secondary);
                ui.add_space(4.0);
                ui.label(small("Average Over")).on_hover_text(AVERAGE_REACH);
                let mut avg = self.average_s;
                let changed = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let a =
                            fields::entry(ui, "average", &mut avg, 1.0..=3600.0, &DURATION, 56.0);
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
                if changed {
                    picked.set(Some(avg));
                }
                ui.label(small("for every pane without its own"))
                    .on_hover_text(PANE_AVERAGE_HINT);
            },
        );
        {
            let rate = r.and_then(|r| r.rate_s_per_day);
            // The input's clock is named in the Rate "?" rather than on the
            // card: nothing on the card can change it.
            let rate_facts = reading_limits("Rate", caveats, lift_now, beats)
                .into_iter()
                .filter(|f| f.0 != CLOCK_UNKNOWN)
                .collect();
            card(&mut cols[1], "Rate", help::RATE, rate_facts, &|ui| {
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
                        if let Some(left) = self.settling_left() {
                            ui.label(small(format!(
                                "settling in the new position, {:.0} s",
                                left.ceil()
                            )))
                            .on_hover_text(SETTLE_HINT);
                        } else if heard {
                            ui.label(small("waiting for beats".into()));
                        } else {
                            unheard(ui);
                        }
                        return;
                    };
                    if let Some(at) = self.stopped_in_settle() {
                        ui.label(small(format!("before the move at {}", strip::fmt_time(at))))
                            .on_hover_text(
                                "Stopped while the watch settled in its new position, so \
                                     these are the last readings of the position before it.",
                            );
                        ui.label(small("·".into()));
                    }
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
                             beats than Average Over asks for, early in a session or after a \
                             pause, so the readings use what there is."
                    } else {
                        "The beats the readings are fitted to. Set the time with Average \
                             Over under Readings."
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
                if let Some(f) = measured {
                    // Timed long enough while listening: offered, never kept
                    // unasked.
                    ui.horizontal(|ui| {
                        let slow = if f.ppm >= 0.0 { "slow" } else { "fast" };
                        ui.label(
                            RichText::new(format!(
                                "input clock timed: {:.1} ppm {slow}",
                                f.ppm.abs()
                            ))
                            .small()
                            .color(pal.text_secondary),
                        )
                        .on_hover_text(format!(
                            "Timed against the computer's clock over {} of listening, this \
                             input's clock runs {:.1} ± {:.1} ppm {slow}: {:+.1} seconds per \
                             day on every rate. Keep it to show the corrected rate as True \
                             Clock from now on; the command line and the MCP server use it \
                             too. It is only right if the computer's clock is kept by \
                             internet time.",
                            strip::fmt_time(f.span_s),
                            f.ppm.abs(),
                            f.ppm_sd,
                            f.rate_error_s_per_day
                        ));
                        if ui
                            .small_button("Keep")
                            .on_hover_text("Keep this input's clock error and correct its rates")
                            .clicked()
                        {
                            keep_clock.set(true);
                        }
                    });
                }
                if let Some((m, sd, span)) = self.whole_take(|p| p.rate) {
                    whole_line(
                        ui,
                        format!("{m:+.1} s/d").replace('-', "−"),
                        format!("{sd:.1} s/d"),
                        span,
                    );
                }
            });
            let amp = r.and_then(|r| r.amplitude_deg);
            let lift = fields::plain(self.lift_deg);
            card(
                &mut cols[2],
                "Amplitude",
                help::AMPLITUDE,
                reading_limits("Amplitude", caveats, lift_now, beats),
                &|ui| {
                    // Whole degrees: a live reading's ± is about a degree, so a
                    // decimal would claim more than the beats can say.
                    let unit = match r.and_then(|r| r.amplitude_error_deg) {
                        Some(e) if e < 0.95 => format!("± {e:.1}°"),
                        Some(e) => format!("± {e:.0}°"),
                        None => String::new(),
                    };
                    figure(ui, amp.map(|v| format!("{v:.0}°")), &unit);
                    if !heard {
                        unheard(ui);
                        return;
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if let Some(&LiveReading {
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
                    if let Some((m, sd, span)) = self.whole_take(|p| p.amplitude) {
                        whole_line(ui, format!("{m:.0}°"), format!("{sd:.0}°"), span);
                    }
                },
            );
            let unlock = r.and_then(|r| r.beat_error_unlock_ms);
            let drop = r.and_then(|r| r.beat_error_ms);
            card(
                &mut cols[3],
                "Beat Error",
                help::BEAT_ERROR,
                reading_limits("Beat Error", caveats, lift_now, beats),
                &|ui| {
                    let unit = match be_pm.filter(|_| unlock.is_some()) {
                        Some(e) if e >= 0.005 => format!("± {e:.2} milliseconds"),
                        _ => "milliseconds".to_string(),
                    };
                    // The size only: which beat is the tick can flip between runs.
                    figure(
                        ui,
                        unlock.or(drop).map(|v| format!("{:.2}", v.abs())),
                        &unit,
                    );
                    if !heard {
                        unheard(ui);
                        return;
                    }
                    ui.label(
                        RichText::new(match (unlock, drop) {
                            (Some(_), Some(d)) => {
                                format!(
                                    "from the unlock, the usual reading · from the drop {:.2} ms",
                                    d.abs()
                                )
                            }
                            (None, Some(_)) => "from the drop".into(),
                            _ => "from the unlock, the usual reading".into(),
                        })
                        .small()
                        .color(pal.text_secondary),
                    )
                    .on_hover_text(
                        "The big figure is timed from the unlock, as tg shows it: quote \
                         that one. The drop figure is a second opinion.",
                    );
                    if let Some((m, sd, span)) = self.whole_take(|p| p.beat_error_unlock) {
                        whole_line(
                            ui,
                            format!("{:.2} ms", m.abs()),
                            format!("{sd:.2} ms"),
                            span,
                        );
                    }
                },
            );
            let heard_bph = r.and_then(|r| r.bph).or_else(|| self.live.as_ref()?.bph());
            let set_bph = self.bph;
            card(
                &mut cols[4],
                "Beat Rate",
                help::BEAT_RATE,
                Vec::new(),
                &|ui| {
                    figure(ui, set_bph.or(heard_bph).map(|b| b.to_string()), "bph");
                    let small = |t: String| RichText::new(t).small().color(pal.text_secondary);
                    match (set_bph, heard_bph) {
                        (Some(_), _) => {
                            ui.label(small("set under Watch".into())).on_hover_text(
                                "Beat Rate is set under Watch, so the readings use it rather than \
                             finding it from the beats. Auto finds it again.",
                            );
                        }
                        (None, Some(_)) => {
                            ui.label(small("found from the beats".into()))
                                .on_hover_text(
                                "Beat Rate under Watch is Auto: the app found it from the first \
                             seconds of beats, as the nearest standard rate.",
                            );
                        }
                        (None, None) => {
                            ui.label(small("not found yet".into()));
                        }
                    }
                },
            );
        }
        let bottom = cols
            .iter()
            .map(|c| c.min_rect().bottom())
            .fold(origin.y, f32::max);
        ui.allocate_space(Vec2::new(total, bottom - origin.y));
        if tallest.get() != height {
            ui.ctx()
                .data_mut(|d| d.insert_temp(height_id, tallest.get()));
            ui.ctx().request_repaint();
        }
        Readouts {
            average: picked.get(),
            keep_clock: keep_clock.get(),
        }
    }

    /// The microphone's clock as timed so far, once it has run
    /// [`CLOCK_MEASURE_S`] of audio and is known to half a ppm.
    fn clock_measured(&self) -> Option<&timegrapher_core::clock::ClockFit> {
        self.clock_track
            .as_ref()?
            .fit
            .as_ref()
            .filter(|f| f.ppm_sd.is_finite() && f.ppm_sd < 0.5)
    }

    /// Keep the microphone's clock as timed: corrected rates from now on,
    /// for the command line and the MCP server too.
    fn keep_clock(&mut self) {
        let (Some(fit), Some(t)) = (self.clock_measured(), self.clock_track.as_ref()) else {
            return;
        };
        let d = clockstore::DeviceClock::from_fit(
            &t.device,
            fit,
            timegrapher_core::recorder::utc(std::time::SystemTime::now()),
            "measure",
        );
        let saved = clockstore::ClockStore::load().and_then(|mut s| {
            s.set(d.clone());
            s.save()
        });
        match saved {
            Ok(p) => {
                self.info_msg(format!(
                    "Kept this input's clock, {:.1} ppm {}, in {}",
                    d.ppm.abs(),
                    if d.ppm >= 0.0 { "slow" } else { "fast" },
                    p.display()
                ));
                self.clock = Some(d);
            }
            Err(e) => self.error(format!("The input's clock couldn't be kept: {e}")),
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            // The links on the right first, so that what is on the left is
            // cut short with an ellipsis rather than running under them.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Report a Problem…").small().color(pal.accent),
                        )
                        .frame_when_inactive(false),
                    )
                    .on_hover_text(
                        "Make a zip of what went wrong, with up to five minutes of the sound, \
                         to attach to a GitHub issue. The app sends nothing itself.",
                    )
                    .clicked()
                {
                    self.open_report();
                }
                let checking = self.updates.checking();
                if ui
                    .add_enabled(
                        !checking,
                        egui::Button::new(
                            RichText::new(if checking {
                                "Checking for Updates…"
                            } else {
                                "Check for Updates…"
                            })
                            .small()
                            .color(pal.accent),
                        )
                        .frame_when_inactive(false),
                    )
                    .on_hover_text(
                        "Ask GitHub whether a newer Timegrapher is published. Nothing \
                         downloads or installs itself.",
                    )
                    .clicked()
                {
                    self.updates.start(ui.ctx(), true);
                }
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        self.position.code(),
                        self.position.name().to_lowercase()
                    ))
                    .small()
                    .color(pal.text_secondary),
                )
                .on_hover_text("The position, set in the Watch pane");
                if let Some(b) = self.reading().and_then(|r| r.bph) {
                    ui.label(
                        RichText::new(format!("{b} beats per hour"))
                            .small()
                            .color(pal.text_secondary),
                    )
                    .on_hover_text("Found from the beats, or set in the Watch pane");
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.status_left(ui);
                });
            });
        });
    }

    /// The status bar's left side: what is showing, the signal, the kept
    /// sound and the latest message.
    fn status_left(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
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
            let text = format!("{what} {}", self.source_label);
            ui.add(
                egui::Label::new(RichText::new(&text).small().color(pal.text_secondary)).truncate(),
            )
            .on_hover_text(text);
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
                    RichText::new(format!("signal {s:.0}×"))
                        .small()
                        .color(pal.text_secondary),
                )
                .on_hover_text(help::SIGNAL);
            }
        }
        if let Some(r) = &self.session.recorder {
            let since = self.unsaved_s();
            let (text, color, hint) = if let Some(fault) = self.session.recording_fault() {
                (
                    "Recording stopped; recovery files retained".to_string(),
                    pal.bad,
                    fault.description(),
                )
            } else {
                match self.session.saved_to() {
                    Some(p) => (
                        if since >= 1.0 {
                            format!(
                                "{} of sound, the last {} not saved",
                                strip::fmt_time(r.duration_s()),
                                strip::fmt_time(since)
                            )
                        } else if self.metadata_unsaved() {
                            format!(
                                "{} of sound, settings not saved",
                                strip::fmt_time(r.duration_s())
                            )
                        } else {
                            format!("{} of sound, saved", strip::fmt_time(r.duration_s()))
                        },
                        pal.text_secondary,
                        format!(
                            "Saved in {}. Sound since then is kept too; Save Again adds it, \
                         and New Session or quitting asks first.",
                            p.display()
                        ),
                    ),
                    None => (
                        format!("{} of sound, not saved", strip::fmt_time(r.duration_s())),
                        pal.text_secondary,
                        "The session's sound is kept as it goes, so Save can keep it at any \
                     point. New Session or quitting throws it away unless saved."
                            .to_string(),
                    ),
                }
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
            ui.add(egui::Label::new(RichText::new(m).small().color(pal.text_secondary)).truncate())
                .on_hover_text(m);
        }
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

    /// Nothing that sounds like a watch has been heard in the take, after a
    /// few seconds of listening: one sentence then, not empty charts and
    /// cards of figures that would describe noise.
    fn no_watch_yet(&self) -> bool {
        let Some(live) = &self.live else {
            return false;
        };
        self.batch.is_none()
            && live.duration_s() > 6.0
            && !self.trend.iter().any(|p| p.heard)
            && self.reading().is_none_or(|r| !r.watch_heard)
    }

    fn no_watch(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let line = self.reading().map_or_else(
            || "No regular beat found".to_string(),
            |r| no_regular_beat(&r),
        );
        ui.add_space((ui.available_height() * 0.16).clamp(16.0, 110.0));
        ui.vertical_centered(|ui| {
            let w = 520.0_f32.min(ui.available_width());
            ui.allocate_ui(Vec2::new(w, 0.0), |ui| {
                theme::card().fill(pal.card).show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        ui.label(RichText::new(&line).font(theme::semibold(17.0)))
                            .on_hover_text(help::NO_REGULAR_BEAT);
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "The sound so far has no beats at regular intervals. The \
                                     readings and charts appear once a regular beat is found.",
                                )
                                .small()
                                .color(pal.text_secondary),
                            )
                            .wrap(),
                        );
                    });
                });
            });
        });
    }

    /// Recording chosen while a stopped microphone session is in hand: the
    /// screen offers a recording instead of showing the session's figures
    /// as if they were one. The session is kept.
    fn mic_session_set_aside(&self) -> bool {
        self.input == Input::File && self.session.mic() && !self.running()
    }

    fn ways_in(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.label(RichText::new("Timegrapher").font(theme::semibold(20.0)));
        if self.mic_session_set_aside() {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Your microphone session is kept, with its sound. Choose Microphone \
                         at the top to see it again, or open a recording.",
                    )
                    .small()
                    .color(pal.text_secondary),
                )
                .wrap(),
            );
        }
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
            "Put the watch on the microphone, dial up. Choose the microphone and set its \
             level in the Watch pane.",
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
                self.open_recording(&p, false, true);
            }
        }
        if row(
            ui,
            theme::secondary(ui, "Open a Folder…"),
            "Choose a folder saved by this app, or a folder of WAV or FLAC segments of \
             one long recording",
            FOLDER_RULE,
        ) {
            self.input = Input::File;
            if let Some(p) = self.open_folder_dialog() {
                self.open_recording(&p, false, true);
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
        // The bar's track in the control colour, to stand out on the card.
        ui.scope(|ui| {
            ui.visuals_mut().extreme_bg_color = pal.control;
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
        });
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
                    "{} files joined in order, {} to {}.",
                    b.files, b.first, b.last
                ))
                .small()
                .color(pal.text_tertiary),
            );
        }
        if let Some(n) = &b.order_note {
            ui.label(RichText::new(n).small().color(pal.warn));
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

    /// Begin the Positions Test, listening first if need be, with the
    /// watch dial up: just after winding, or as the 24 hour half after the
    /// full-wind test `half`.
    fn start_guided(&mut self, half: Option<guided::FullWind>) {
        if !self.running() {
            if self.can_resume() {
                self.resume_microphone();
            } else {
                self.start_microphone();
            }
        }
        if !self.running() {
            return;
        }
        if self.position != guided::ORDER[0] {
            self.change_position(guided::ORDER[0]);
        }
        let test = match half {
            Some(full) => guided::Test::after(full),
            None => guided::Test::new(),
        };
        self.guided = Some(test.timed(self.guided_settle_s, self.guided_measure_s));
        self.guided_unused.clear();
        self.guided_measured.clear();
        self.reveal_pane(Pane::PositionsTest);
    }

    /// After a break in the sound while listening: was the watch moved?
    /// The position buttons mark it; it goes by itself a minute later.
    fn move_banner(&mut self, ui: &mut egui::Ui) {
        let Some((from, to)) = self.move_prompt else {
            return;
        };
        let now = self.live.as_ref().map_or(to, |l| l.duration_s());
        if self.guided.is_some() || now - to > 60.0 || !self.running() {
            self.move_prompt = None;
            return;
        }
        let pal = theme::pal(ui);
        let mut pick = None;
        let mut dismiss = false;
        banner(ui, pal.accent, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                dismiss = ui
                    .add(egui::Button::new("Not Moved").frame_when_inactive(false))
                    .on_hover_text("Keep the position as it is")
                    .clicked();
                for p in Position::ALL.into_iter().rev() {
                    if ui
                        .add_enabled(p != self.position, egui::Button::new(p.code()))
                        .on_hover_text(format!("The watch is now {}", p.label()))
                        .clicked()
                    {
                        pick = Some(p);
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!(
                                "The sound stopped for {:.0} s and came back. If the watch \
                                 was moved, mark its new position:",
                                to - from
                            ))
                            .color(pal.text),
                        )
                        .wrap(),
                    );
                });
            });
        });
        if let Some(p) = pick {
            self.change_position(p);
        } else if dismiss {
            self.move_prompt = None;
        }
        ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
    }

    /// Move the Positions Test on with the sound, whether its pane shows
    /// or not: read each position as it is measured, and keep what the
    /// strip marks.
    fn guided_tick(&mut self, ctx: &egui::Context) {
        let Some(test) = self.guided.as_mut() else {
            return;
        };
        let Some(now) = self.live.as_ref().map(|l| l.duration_s()) else {
            return;
        };
        let phase = test.tick(now);
        if test.needs_reading() {
            if let Some(log) = self.live.as_ref().and_then(steady::log_of) {
                let clock = self
                    .clock
                    .as_ref()
                    .and_then(|c| timegrapher_core::session::fixed_clock(c.ppm, log.duration_s));
                test.read_new(&log, clock.as_ref());
            }
        }
        self.guided_unused = test.unused(now);
        self.guided_measured = test.measured(now);
        if matches!(
            phase,
            guided::Phase::Settling(_) | guided::Phase::Measuring(_) | guided::Phase::Paused
        ) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// The watch and how it is read, for the test's saved and copied
    /// results.
    fn guided_about(&self) -> guided::About {
        guided::About {
            watch: self.watch.trim().to_string(),
            calibre: self.calibre.clone(),
            bph: self.bph.or(self.live.as_ref().and_then(|l| l.bph())),
            lift_angle_deg: self.lift_deg,
        }
    }

    /// Whether a Positions Test is under way, not yet finished.
    fn guided_running(&self) -> bool {
        self.guided.as_ref().is_some_and(|t| t.readings.is_none())
    }

    /// Where the test's files are offered first: beside the saved take,
    /// or the folder Save offers.
    fn guided_dir(&self) -> PathBuf {
        self.session
            .saved_to()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from(&self.save_dir))
    }

    /// The Positions Test pane's toolbar: start the test, load the
    /// full-wind results for the 24 hour half, the times in each
    /// position, and saving or copying the results.
    fn positions_settings(&mut self, ui: &mut egui::Ui) {
        let running = self.guided_running();
        let (settle, measure) = (self.guided_settle_s, self.guided_measure_s);
        let start_hint = match &self.guided_earlier {
            Some(_) => format!(
                "Run the six positions again, {settle:.0} s settling and {measure:.0} s measuring \
                 in each, against the full-wind results loaded, starting dial up. Listens to \
                 the microphone first if it isn't already."
            ),
            None => format!(
                "Lead the watch through the six positions, starting dial up: each settles \
                 {settle:.0} s and is measured for {measure:.0} s, and the table fills in as it \
                 goes. Listens to the microphone first if it isn't already."
            ),
        };
        let label = if self.guided.is_some() && !running {
            "Start a New Test"
        } else {
            "Start Test"
        };
        // While it runs, the banner below carries the test's buttons.
        if !running {
            if ui
                .add(theme::primary(ui, label))
                .on_hover_text(start_hint)
                .clicked()
            {
                let earlier = self.guided_earlier.clone();
                self.start_guided(earlier);
            }
            if ui
                .button("Load Full-Wind Results…")
                .on_hover_text(format!(
                    "For the test 24 hours after winding: load the results saved from the test \
                     just after it ({} in a saved session's folder, or the file Save Results for \
                     24 Hours… wrote). The next test is then set against them.",
                    guided::RESULTS_FILE
                ))
                .clicked()
            {
                if let Some(full) = self.load_full_wind() {
                    self.guided_earlier = Some(full);
                }
            }
            ui.separator();
        }
        ui.add_enabled_ui(!running, |ui| {
            theme::row(
                ui,
                "Settle",
                Some("Seconds the watch is left to settle after each turn before it is measured"),
                |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.guided_settle_s)
                            .range(guided::SETTLE_RANGE)
                            .speed(1.0)
                            .max_decimals(0)
                            .suffix(" s"),
                    );
                },
            );
            theme::row(
                ui,
                "Measure",
                Some(
                    "Seconds each position is measured for. Longer brings the rate's ± down; a \
                     minute gives about half a second a day.",
                ),
                |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.guided_measure_s)
                            .range(guided::MEASURE_RANGE)
                            .speed(1.0)
                            .max_decimals(0)
                            .suffix(" s"),
                    );
                },
            );
        });
        let has = self
            .guided
            .as_ref()
            .is_some_and(|t| !t.readings_so_far().is_empty());
        ui.separator();
        let about = self.guided_about();
        if ui
            .add_enabled(has, egui::Button::new("Save as CSV…"))
            .on_hover_text(
                "Save the watch, its calibre, the table, the means and the differences as a \
                 CSV file a spreadsheet opens",
            )
            .on_disabled_hover_text("Nothing measured yet")
            .clicked()
        {
            if let Some(t) = &self.guided {
                let text = guided::export_text(t, &about, ',');
                if let Some(m) = guided::save_csv(&text, &self.guided_dir()) {
                    self.info_msg(m);
                }
            }
        }
        if ui
            .add_enabled(has, egui::Button::new("Copy"))
            .on_hover_text(
                "Copy the watch, its calibre and the table with tabs between the columns, to \
                 paste into a spreadsheet or a message",
            )
            .on_disabled_hover_text("Nothing measured yet")
            .clicked()
        {
            if let Some(t) = &self.guided {
                ui.ctx().copy_text(guided::export_text(t, &about, '\t'));
                self.info_msg("Copied the Positions Test table".to_string());
            }
        }
        if let Some(k) = self.guided.as_ref().and_then(|t| t.kept(&about)) {
            if ui
                .button("Save Results for 24 Hours…")
                .on_hover_text(format!(
                    "Save these results to load when you run the test again 24 hours after \
                     winding. Saving the session keeps them too, as {} in its folder.",
                    guided::RESULTS_FILE
                ))
                .clicked()
            {
                if let Some(m) = guided::save_results(&k, &self.guided_dir()) {
                    self.info_msg(m);
                }
            }
        }
    }

    /// The Positions Test pane: the banner and the tables, and what its
    /// buttons ask for.
    fn positions_pane(&mut self, ui: &mut egui::Ui) {
        let now = self.live.as_ref().map(|l| l.duration_s());
        let phase = match (self.guided.as_mut(), now) {
            (Some(t), Some(now)) => Some(t.tick(now)),
            _ => None,
        };
        let action = guided::pane(
            ui,
            self.guided.as_ref(),
            phase,
            self.guided_earlier.as_ref(),
            self.guided_settle_s,
            self.guided_measure_s,
        );
        let Some(action) = action else { return };
        if action == guided::Action::ClearEarlier {
            self.guided_earlier = None;
            return;
        }
        let Some(now) = now else { return };
        let Some(mut test) = self.guided.take() else {
            return;
        };
        match action {
            guided::Action::Next => match test.advance(now) {
                // Noted in the take like a position picked by hand, so its
                // session.json follows the test.
                Some(p) => self.change_position(p),
                None => {
                    if let Some(log) = self.live.as_ref().and_then(steady::log_of) {
                        let clock = self.clock.as_ref().and_then(|c| {
                            timegrapher_core::session::fixed_clock(c.ppm, log.duration_s)
                        });
                        test.finish(&log, clock.as_ref());
                    }
                }
            },
            guided::Action::Redo => test.redo(now),
            guided::Action::Pause => test.pause(now),
            guided::Action::Resume => test.resume(now),
            guided::Action::ClearEarlier => {}
            guided::Action::Stop => {
                test.stop(now);
                self.guided_unused = test.unused(now);
                self.guided_measured = test.measured(now);
                return;
            }
        }
        self.guided_unused = test.unused(now);
        self.guided_measured = test.measured(now);
        self.guided = Some(test);
    }

    /// Show a pane and bring its tab to the front.
    fn reveal_pane(&mut self, pane: Pane) {
        set_pane_visible(&mut self.panes.tiles, pane, true);
        let Some(id) = self.panes.tiles.find_pane(&pane) else {
            return;
        };
        if let Some(parent) = self.panes.tiles.parent_of(id) {
            if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t))) =
                self.panes.tiles.get_mut(parent)
            {
                t.set_active(id);
            }
        }
    }

    /// A newer release is published: its version and a button to its page.
    fn update_banner(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.updates.notice.clone() else {
            return;
        };
        let pal = theme::pal(ui);
        let (mut download, mut dismiss) = (false, false);
        banner(ui, pal.accent, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                dismiss = ui
                    .add(egui::Button::new("Dismiss").frame_when_inactive(false))
                    .on_hover_text("Hide this until the app starts again")
                    .clicked();
                download = ui
                    .add(theme::primary(ui, "Download"))
                    .on_hover_text(
                        "Open the release's page in your browser, to download it from. \
                         Nothing installs itself.",
                    )
                    .clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("Timegrapher {} is available", r.version))
                                .color(pal.text),
                        )
                        .wrap(),
                    );
                });
            });
        });
        if download {
            ui.ctx()
                .open_url(egui::OpenUrl::new_tab(crate::update::page(&r)));
        }
        if download || dismiss {
            self.updates.notice = None;
        }
        ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
    }

    /// After the app closed unexpectedly: an offer to report it. The report
    /// is a GitHub issue the user opens with the crash record pasted in;
    /// nothing is sent from the app.
    fn crash_banner(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.crash.clone() else {
            return;
        };
        let pal = theme::pal(ui);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let (mut report, mut show, mut dismiss) = (false, false, false);
        banner(ui, pal.warn, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                dismiss = ui
                    .add(egui::Button::new("Dismiss").frame_when_inactive(false))
                    .on_hover_text("Don't ask about this crash again")
                    .clicked();
                show = ui
                    .add(egui::Button::new(if self.crash_shown {
                        "Hide the Record"
                    } else {
                        "Show the Record"
                    }))
                    .on_hover_text("What the app wrote down as it closed")
                    .clicked();
                report = ui
                    .add(theme::primary(ui, "Make a Report…"))
                    .on_hover_text(
                        "Copy the crash record and open a new bug report on GitHub in \
                         your browser, to paste it into. Nothing is sent until you submit it.",
                    )
                    .clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "The timegrapher closed unexpectedly last time. Make a report?",
                            )
                            .color(pal.text),
                        )
                        .wrap(),
                    );
                });
            });
        });
        if self.crash_shown {
            egui::ScrollArea::vertical()
                .id_salt("crash-record")
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&text).monospace().small()).selectable(true),
                    );
                });
        }
        if show {
            self.crash_shown = !self.crash_shown;
        }
        if report {
            ui.ctx().copy_text(text);
            ui.ctx().open_url(egui::OpenUrl::new_tab(BUG_REPORT_URL));
            self.info_msg(
                "The crash record is on the clipboard: paste it into the report that opened \
                 in your browser."
                    .into(),
            );
        }
        if report || dismiss {
            applog::mark_seen(&path);
            self.crash = None;
            self.crash_shown = false;
        }
    }

    /// A problem, where it can't be missed, with a way to put it away.
    fn error_banner(&mut self, ui: &mut egui::Ui) {
        let fault = self.session.recording_fault().map(|f| f.description());
        let m = match (&fault, &self.message) {
            (Some(fault), Some((message, true))) => {
                let fault = plain_error(fault);
                if message.starts_with(&fault) {
                    message.clone()
                } else {
                    format!("{fault}\n{message}")
                }
            }
            (Some(fault), _) => fault.clone(),
            (None, Some((message, true))) => message.clone(),
            _ => return,
        };
        let pal = theme::pal(ui);
        let mut dismiss = false;
        banner(ui, pal.bad, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                dismiss = fault.is_none()
                    && ui
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
    }

    /// A line over the readings while the sound they come from clips.
    fn clipping_banner(&self, ui: &mut egui::Ui) {
        let Some(r) = self.reading().filter(|r| r.clipped_beats > 0) else {
            return;
        };
        let pal = theme::pal(ui);
        let pct = r.clipped_fraction.unwrap_or(0.0) * 100.0;
        // Under 1%, a whole number would read "0%".
        let share = if pct < 0.5 {
            "under 1".to_string()
        } else {
            format!("{pct:.0}")
        };
        let resp = banner(ui, pal.warn, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Clipping on {share}% of beats.")).color(pal.text),
                )
                .wrap(),
            );
        });
        resp.on_hover_text(help::CLIPPING_BANNER.join("\n\n"));
        ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
    }

    fn main_view(&mut self, ui: &mut egui::Ui) {
        self.crash_banner(ui);
        self.update_banner(ui);
        // Errors float over the top of the main area rather than pushing
        // everything down.
        if self.session.recording_fault().is_some() || matches!(self.message, Some((_, true))) {
            let top = ui.max_rect();
            let pal = theme::pal(ui);
            egui::Area::new(egui::Id::new("error banner"))
                .order(egui::Order::Foreground)
                .fixed_pos(top.left_top())
                .show(ui.ctx(), |ui| {
                    ui.set_width(top.width());
                    egui::Frame::new()
                        .fill(pal.window)
                        .corner_radius(theme::CARD_RADIUS)
                        .shadow(ui.style().visuals.popup_shadow)
                        .show(ui, |ui| self.error_banner(ui));
                });
        }
        if self.live.is_none() || self.mic_session_set_aside() {
            self.beside_watch(ui, Self::empty_state);
            return;
        }
        self.move_banner(ui);
        if self.no_watch_yet() {
            self.beside_watch(ui, Self::no_watch);
            return;
        }
        if self.show_readings {
            self.clipping_banner(ui);
            let out = self.readouts(ui);
            if let Some(avg) = out.average {
                self.set_average(avg);
            }
            if out.keep_clock {
                self.keep_clock();
            }
            ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
        }

        // Looking back through a finished or analysed recording, however
        // short: a replay stopped early can still be looked back over.
        let total = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        if !self.running() && total > 1.0 {
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
                        "Every pane is hidden. Turn one on with its icon at the top or in the \
                         View menu, or choose Reset Panes in the View menu.",
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

    /// Before there is anything to show: the Watch pane at the left, as
    /// it sits once there is, unless it is hidden, the Positions Test pane
    /// above the rest when it is shown, so a test can start from it, and
    /// `rest` beside them.
    fn beside_watch(&mut self, ui: &mut egui::Ui, rest: fn(&mut Self, &mut egui::Ui)) {
        let watch = pane_visible(&self.panes.tiles, Pane::Watch);
        let test = pane_visible(&self.panes.tiles, Pane::PositionsTest);
        if !watch && !test {
            rest(self, ui);
            return;
        }
        let full = ui.available_rect_before_wrap();
        let pal = theme::pal(ui);
        let card = |ui: &egui::Ui, r: egui::Rect| {
            ui.painter()
                .rect_filled(r, egui::CornerRadius::same(theme::CARD_RADIUS), pal.card);
            r.shrink2(Vec2::new(8.0, 6.0))
        };
        let mut right = full;
        if watch {
            let w = (full.width() * WATCH_SHARE).clamp(260.0, 360.0);
            let left = egui::Rect::from_min_size(full.min, Vec2::new(w, full.height()));
            right = egui::Rect::from_min_max(
                egui::pos2(left.right() + theme::GAP, full.top()),
                full.max,
            );
            let inner = card(ui, left);
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                self.watch_pane(ui)
            });
        }
        if test {
            // Room for its toolbar and a few lines until a test has a table.
            let h = if self.guided.is_some() || self.guided_earlier.is_some() {
                (right.height() * 0.6).max(240.0)
            } else {
                170.0
            }
            .min(right.height());
            let top = egui::Rect::from_min_size(right.min, Vec2::new(right.width(), h));
            let inner = card(ui, top);
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                if self.pane_tools(ui, Pane::PositionsTest) {
                    ui.add_space(4.0);
                }
                self.positions_pane(ui);
            });
            right = egui::Rect::from_min_max(
                egui::pos2(
                    right.left(),
                    (top.bottom() + theme::GAP).min(right.bottom()),
                ),
                right.max,
            );
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| rest(self, ui));
        ui.allocate_rect(full, egui::Sense::hover());
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
        // The strip's own Average Over, as its overlays use.
        let rate_line = self
            .rate_line
            .then(|| self.reading_over(self.average_for(Pane::Strip)))
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
        let trend = self.trend_for(self.average_for(Pane::Strip));
        let series = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<[f64; 2]> {
            trend
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
        let settles = self.strip_settles();
        let extras = strip::Extras {
            note: note.as_deref(),
            rate_line,
            guides: self.rate_guides,
            overlays: &overlays,
            cursor_t: self.cursor_t,
            gutter: GUTTER,
            bands: &bands,
            settles: &settles,
            measured: &self.guided_measured,
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

    /// The Tick Tock profiles nearest `end`, as the profile pane shows them:
    /// the latest while listening, else what the session kept, else worked
    /// out from the recording (none until that is done).
    fn profiles_at(&mut self, end: f64) -> Profiles {
        let latest = self.view_end.is_none() && self.running();
        let i = self.sound_history.partition_point(|h| h.0 <= end + 1e-9);
        let kept = i
            .checked_sub(1)
            .map(|i| &self.sound_history[i])
            .filter(|(t, _)| end - t <= SOUND_EVERY_S + 0.5)
            .map(|(_, p)| p.clone());
        match kept {
            _ if latest => self
                .live
                .as_ref()
                .map_or([None, None], |l| l.tick_profiles().clone()),
            Some(p) => p,
            None => {
                self.poll_sound_job();
                match &self.sound_at {
                    Some((t, p)) if (t - end).abs() < 0.5 => p.clone(),
                    _ => {
                        self.start_sound_job(end);
                        [None, None]
                    }
                }
            }
        }
    }

    /// The Scope pane: the latest beats' sound up to the time on show, from
    /// what the live engine still holds, else read from the recording.
    fn scope_pane(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let view = self.scope_view;
        let profiles = self.profiles_at(self.end_s());
        let Some(live) = &self.live else {
            ui.label(
                RichText::new("Listen to a watch or open a recording to see its sound here.")
                    .color(pal.text_tertiary),
            );
            return;
        };
        let end = self.end_s();
        // One Beat follows the watch's beat rate, 28,800 bph until known.
        let beat_s = 3600.0 / f64::from(live.bph().unwrap_or(28_800).max(1));
        let split = view.zoom.split();
        let specs: Vec<(&'static str, bool, Color32)> = if split {
            vec![("Tick", true, pal.tick), ("Tock", false, pal.tock)]
        } else {
            vec![("Tick", true, pal.text_secondary)]
        };
        let picks: Vec<Vec<timegrapher_core::beats::Beat>> = specs
            .iter()
            .map(|&(_, even, _)| {
                scope::pick(live.beats(), view.zoom, beat_s, view.ring, end, Some(even))
            })
            .collect();
        // The marks from the profile nearest the moment shown, as the
        // Tick Tock Profile shows them.
        let fs = live.sample_rate() as f64;
        let (before, after) = view.zoom.window(beat_s);
        // Each beat from the traces already drawn, else from the sound the
        // engine still holds; only the rest need reading from the recording.
        let key = |b: &timegrapher_core::beats::Beat| {
            (b.index, b.time.to_bits(), view.zoom, beat_s.to_bits())
        };
        let mut known: std::collections::HashMap<ScopeKey, Vec<f32>> = Default::default();
        for b in picks.iter().flatten() {
            let k = key(b);
            if let Some((_, t)) = self.scope_traces.iter().find(|(c, _)| *c == k) {
                known.insert(k, t.clone());
                continue;
            }
            let (a, z) = (b.time - before - 0.015, b.time + after + 0.005);
            if let Some(t) = live
                .sound(a, z - a)
                .and_then(|x| scope::trace(x, fs, a, b, view.zoom, beat_s))
            {
                self.scope_traces.push_back((k, t.clone()));
                known.insert(k, t);
            }
        }
        while self.scope_traces.len() > SCOPE_KEPT {
            self.scope_traces.pop_front();
        }
        let missing: Vec<&timegrapher_core::beats::Beat> = picks
            .iter()
            .flatten()
            .filter(|b| !known.contains_key(&key(b)))
            .collect();
        let times = missing.iter().map(|b| b.time);
        // A little either side, so rounding never cuts a trace short.
        let lo = (times.clone().fold(f64::INFINITY, f64::min) - before - 0.015).max(0.0);
        let hi = times.fold(f64::NEG_INFINITY, f64::max) + after + 0.005;
        let mut note = None;
        let listening = self.view_end.is_none() && self.running();
        let audio: Option<(Vec<f32>, f64, f64)> = if lo >= hi || (listening && !known.is_empty()) {
            // Nothing more to read, or while listening a beat whose sound
            // went before it was drawn: the others carry on without it.
            None
        } else {
            self.poll_scope_job();
            match &self.scope_audio {
                // What was asked for covers these beats: use what was read,
                // even when the recording ended before it, and never ask
                // again for a stretch that couldn't be read.
                Some((a, b, got)) if *a <= lo && *b >= hi => match got {
                    Ok(sp) => {
                        let i0 =
                            (((lo - sp.t0) * sp.rate).floor().max(0.0) as usize).min(sp.mono.len());
                        let i1 = (((hi - sp.t0) * sp.rate).ceil().max(0.0) as usize)
                            .clamp(i0, sp.mono.len());
                        Some((
                            sp.mono[i0..i1].to_vec(),
                            sp.t0 + i0 as f64 / sp.rate,
                            sp.rate,
                        ))
                    }
                    Err(e) => {
                        note = Some(format!("Couldn't read the sound here: {e}."));
                        None
                    }
                },
                _ => {
                    note = Some(
                        if self.start_scope_job(lo, hi) {
                            "Reading the sound from the recording…"
                        } else {
                            "The sound of these beats isn't kept: the Scope shows the last \
                             few seconds while listening, and any moment of a recording."
                        }
                        .to_string(),
                    );
                    None
                }
            }
        };
        let rows: Vec<scope::Row> = specs
            .iter()
            .zip(&picks)
            .map(|(&(label, even, color), beats)| {
                let traces = beats
                    .iter()
                    .filter_map(|b| {
                        known.get(&key(b)).cloned().or_else(|| {
                            let (x, t0, rate) = audio.as_ref()?;
                            scope::trace(x, *rate, *t0, b, view.zoom, beat_s)
                        })
                    })
                    .collect();
                let mut marks = Vec::new();
                if view.zoom.marks() {
                    if let Some(p) = &profiles[usize::from(!even)] {
                        if let Some(u) = p.unlock_ms {
                            marks.push((u, pal.unlock, "Unlock"));
                        }
                        if let Some(d) = p.drop_ms {
                            marks.push((d, pal.drop, "Drop"));
                        }
                    }
                }
                scope::Row {
                    label: if split { label } else { "" },
                    traces,
                    color,
                    marks,
                }
            })
            .collect();
        if let Some(n) = note {
            ui.label(RichText::new(n).small().color(pal.text_tertiary));
        }
        let rate = audio.as_ref().map_or(fs, |a| a.2);
        scope::draw(ui, &rows, view.zoom, beat_s, rate);
    }

    fn poll_scope_job(&mut self) {
        if let Some(rx) = &self.scope_job {
            match rx.try_recv() {
                Ok(cut) => {
                    self.scope_audio = Some(cut);
                    self.scope_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.scope_job = None;
                    self.scope_audio = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    /// Read the sound from `lo` to `hi` seconds (and two seconds either
    /// side, for small moves of the time slider) from the recording open.
    /// False when there is no recording to read it from.
    fn start_scope_job(&mut self, lo: f64, hi: f64) -> bool {
        if self.scope_job.is_some() {
            return true;
        }
        if self.input != Input::File || self.session.mic() {
            return false;
        }
        let Ok(files) = recordings_in(&PathBuf::from(self.file_path.trim())) else {
            return false;
        };
        let (a, b) = ((lo - 2.0).max(0.0), hi + 2.0);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((a, b, crate::span::read(&files, a, b - a)));
        });
        self.scope_job = Some(rx);
        true
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
        if self.input != Input::File || self.session.mic() {
            return false;
        }
        let Ok(files) = recordings_in(&path) else {
            return false;
        };
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
            // Only this stretch is read, however long the recording; when
            // it can't be, the answer is no profile, not another try.
            let a0 = a0.max(0.0);
            let p = match crate::span::read(&files, a0, a1 - a0) {
                Ok(sp) => {
                    let beats: Vec<_> = beats
                        .iter()
                        .map(|b| timegrapher_core::beats::Beat {
                            time: b.time + a0 - sp.t0,
                            ..*b
                        })
                        .collect();
                    timegrapher_core::profile::profiles_from_audio(
                        &sp.mono, sp.rate, &beats, period, &cfg,
                    )
                }
                Err(_) => [None, None],
            };
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
                Err(TryRecvError::Disconnected) => {
                    self.sound_at = Some((*t, [None, None]));
                    self.sound_job = None;
                }
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

    /// The stretch of time the charts cover: the strip's length, ending
    /// where the strip does, or `None` for the whole session.
    fn chart_window(&self) -> Option<(f64, f64)> {
        self.span_of_strip.then(|| self.strip_window())
    }

    /// The stretch of time the distributions cover, as for the charts but
    /// set on its own.
    fn hist_window(&self) -> Option<(f64, f64)> {
        self.hist_span_of_strip.then(|| self.strip_window())
    }

    fn strip_window(&self) -> (f64, f64) {
        let end = self.end_s();
        (end - self.strip.span_s, end)
    }

    /// How often each value of a reading came up, over the session or the
    /// strip's length: bars, or the cumulative share.
    fn histogram(&mut self, ui: &mut egui::Ui, pane: Pane) {
        use timegrapher_core::histogram as hist;
        let pal = theme::pal(ui);
        let window = self.hist_window();
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
        let every = self.average_for(pane);
        let trend = self.trend_for(every);
        let readings = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<f64> {
            let mut last = f64::NEG_INFINITY;
            trend
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
        // Over what stretch, so a short window isn't taken for the session.
        let over = match window {
            Some((a, b)) => format!(" in the last {}", fields::duration(b - a)),
            None => " in the session".to_string(),
        };
        let mut line = format!(
            "{} {per}{over} · median {} · middle 80% {} to {}",
            h.n,
            f(h.median),
            f(h.p10),
            f(h.p90),
        );
        if !self.session.stretches().is_empty() {
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
            .allow_axis_zoom_drag(false)
            .allow_boxed_zoom(false)
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
        let average = self.average_for(pane);
        let trend = self.trend_for(average);
        let pick = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<[f64; 2]> {
            trend
                .iter()
                .filter_map(|p| f(p).map(|v| [p.t, v]))
                .collect()
        };
        // Each line in its reading's colour, as on the strip and in the
        // distributions. Tick's amplitude dashes purple over blue and Tock's
        // purple over orange, the key drawn the same; beat error from the
        // drop is a fainter dashed yellow.
        let solid = LineStyle::Solid;
        let (series, unit, decimals): (Vec<ChartLine>, &'static str, usize) = match pane {
            Pane::Rate => (
                vec![ChartLine {
                    name: format!(
                        "Rate in seconds per day, {} average",
                        fields::duration(average)
                    ),
                    pts: pick(|p| p.rate),
                    color: pal.trace_rate,
                    style: solid,
                    key: pal.trace_rate,
                    under: None,
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
                        under: None,
                        main: true,
                    },
                    ChartLine {
                        name: "Amplitude from Tick".into(),
                        pts: pick(|p| p.amplitude_a),
                        color: pal.trace_amplitude,
                        style: LineStyle::Dashed { length: 6.0 },
                        key: pal.tick,
                        under: Some(pal.tick),
                        main: false,
                    },
                    ChartLine {
                        name: "Amplitude from Tock".into(),
                        pts: pick(|p| p.amplitude_b),
                        color: pal.trace_amplitude,
                        style: LineStyle::Dashed { length: 6.0 },
                        key: pal.tock,
                        under: Some(pal.tock),
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
                        pts: pick(|p| p.beat_error_unlock.map(f64::abs)),
                        color: pal.trace_beat_error,
                        style: solid,
                        key: pal.trace_beat_error,
                        under: None,
                        main: true,
                    },
                    ChartLine {
                        name: "From the Drop".into(),
                        pts: pick(|p| p.beat_error_drop.map(f64::abs)),
                        color: pal.trace_beat_error.gamma_multiply(0.6),
                        style: LineStyle::dashed_dense(),
                        key: pal.trace_beat_error.gamma_multiply(0.6),
                        under: None,
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
            // The wheel zooms and a drag pans; the axes stay put.
            .allow_axis_zoom_drag(false)
            .allow_boxed_zoom(false)
            .x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
            .custom_x_axes(vec![
                egui_plot::AxisHints::new_x().formatter(|m, r| chart_time_label(m.value, r))
            ])
            // Short charts still get a label at the top and the bottom:
            // the labels are painted below, in small text, so they fit
            // closer than the plot's own.
            .y_grid_spacer(theme::short_y_grid)
            .y_axis_min_width(GUTTER)
            .y_axis_formatter(|_, _| String::new())
            // The values at the pointer are shown for every line at once
            // (below), instead of only when the pointer is on a line.
            .show_x(false)
            .show_y(false);
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
        // A new averaging time spreads the values wider or narrower, so the
        // scale follows it, keeping the time shown.
        let rescale = self
            .chart_average_drawn
            .insert(pane, average)
            .is_some_and(|was| was != average);
        let resp = plot.show(ui, |p| {
            if let Some((a, b)) = window {
                p.set_plot_bounds_x(a..=b);
            } else if to_live {
                p.set_auto_bounds([true, true]);
            }
            if let (Some((lo, hi)), true) = (y_range, rescale || (to_live && window.is_some())) {
                p.set_plot_bounds_y(lo..=hi);
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
                if let Some(c) = l.under {
                    p.line(
                        Line::new(format!("{} gaps", l.name), l.pts.clone())
                            .color(c)
                            .width(w),
                    );
                }
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
        paint_y_labels(ui, &t, unit);
        let at = |x| egui::pos2(t.position_from_point_x(x), frame.top());
        let test = self.in_test();
        if test {
            strip::paint_phases(
                &painter,
                frame,
                &self.strip_settles(),
                &self.guided_measured,
                at,
                true,
                ui.visuals(),
            );
        }
        strip::paint_bands(
            &painter,
            frame,
            &self.position_bands(),
            at,
            true,
            true,
            !test,
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

/// The break before the beats newer than `seen`, from and to, when it is
/// longer than [`MOVE_GAP_S`] and no position change falls in it.
fn sound_break(
    beats: &[timegrapher_core::beats::Beat],
    seen: f64,
    moves: &[(f64, Position)],
) -> Option<(f64, f64)> {
    if seen <= 0.0 {
        return None;
    }
    let back = beats.get(beats.partition_point(|b| b.time <= seen))?.time;
    let moved = moves
        .iter()
        .any(|&(at, _)| at >= seen - 1.0 && at <= back + 1.0);
    (back - seen > MOVE_GAP_S && !moved).then_some((seen, back))
}

/// The Watch settings a take is read with.
#[derive(Clone, Debug, PartialEq)]
struct WatchSetup {
    watch: String,
    calibre: String,
    lift_deg: f64,
    lift_context: (f64, bool),
    bph: Option<u32>,
    position: Position,
}

impl Default for WatchSetup {
    fn default() -> Self {
        WatchSetup {
            watch: String::new(),
            calibre: String::new(),
            lift_deg: DEFAULT_LIFT_DEG,
            lift_context: (DEFAULT_LIFT_DEG, false),
            bph: None,
            position: Position::DialUp,
        }
    }
}

/// The position a recording's file or folder name carries, as a word of
/// its own: `reverso_DU_01.flac` is dial up.
fn position_in_name(path: &Path) -> Option<Position> {
    let name = path.file_stem()?.to_str()?;
    let words: Vec<&str> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let found: Vec<Position> = words
        .iter()
        .filter(|w| w.len() == 2)
        .filter_map(|w| {
            let up = w.to_ascii_uppercase();
            Position::ALL.into_iter().find(|p| p.code() == up)
        })
        .collect();
    // Exactly one, or it is no guess at all.
    match found.as_slice() {
        [p] => Some(*p),
        _ => None,
    }
}

/// A chart's time label at `t` seconds, in view `r`.
fn chart_time_label(t: f64, r: &std::ops::RangeInclusive<f64>) -> String {
    // Nothing before the session starts: a time there read as a second
    // "0:00".
    if t < -1e-6 {
        return String::new();
    }
    // A long time ("1:30:00") at the right end would be cut.
    let label = strip::fmt_time(t);
    let frac = if label.len() > 5 { 0.03 } else { 0.0 };
    theme::clear_of_right_edge(label, t, r, frac)
}

/// The names in a chart's key and their colours, when it has more than one
/// line.
/// A second colour is drawn as a dash of the two, as the line is.
fn chart_key(pane: Pane, pal: &theme::Palette) -> Vec<(&'static str, Color32, Option<Color32>)> {
    match pane {
        Pane::Amplitude => vec![
            ("Average Amplitude", pal.trace_amplitude, None),
            ("From Tick", pal.trace_amplitude, Some(pal.tick)),
            ("From Tock", pal.trace_amplitude, Some(pal.tock)),
        ],
        Pane::BeatError => vec![
            ("From the Unlock", pal.trace_beat_error, None),
            (
                "From the Drop",
                pal.trace_beat_error.gamma_multiply(0.6),
                None,
            ),
        ],
        _ => Vec::new(),
    }
}

/// A chart's value labels, small, beside its grid lines in the gutter.
fn paint_y_labels(ui: &egui::Ui, t: &egui_plot::PlotTransform, unit: &str) {
    let frame = *t.frame();
    let b = t.bounds();
    let (lo, hi) = (b.min()[1], b.max()[1]);
    if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) || frame.height() <= 0.0 {
        return;
    }
    let marks = theme::short_y_grid(egui_plot::GridInput {
        bounds: (lo, hi),
        base_step_size: (hi - lo) / frame.height() as f64 * 8.0,
    });
    let painter = ui.painter();
    let color = theme::pal(ui).text_secondary;
    let font = egui::TextStyle::Small.resolve(ui.style());
    for m in marks {
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
        let y = t.position_from_point_y(m.value);
        painter.text(
            egui::pos2(frame.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{v:.d$}{unit}"),
            font.clone(),
            color,
        );
    }
}

/// The 2nd to 98th percentile of the values, padded a little.
fn percentile_range(points: &[[f64; 2]]) -> Option<(f64, f64)> {
    if points.len() < 2 {
        return None;
    }
    let mut ys: Vec<f64> = points.iter().map(|p| p[1]).collect();
    ys.sort_by(f64::total_cmp);
    let q = |f: f64| ys[((ys.len() - 1) as f64 * f).round() as usize];
    let (mut lo, mut hi) = (q(0.02), q(0.98));
    // A burst of glitches longer than 2% of the points (a few seconds of
    // knocking on a short window) still mustn't set the scale: nothing
    // further than 8 robust spreads from the middle counts.
    let mid = q(0.5);
    let mut dev: Vec<f64> = ys.iter().map(|y| (y - mid).abs()).collect();
    dev.sort_by(f64::total_cmp);
    let sigma = 1.4826 * dev[(dev.len() - 1) / 2];
    if sigma > 0.0 {
        lo = lo.max(mid - 8.0 * sigma);
        hi = hi.min(mid + 8.0 * sigma);
    }
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
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            if self.app.pane_tools(ui, *pane) {
                ui.add_space(4.0);
            }
            match pane {
                Pane::Watch => self.app.watch_pane(ui),
                Pane::Strip => self.app.strip_pane(ui),
                Pane::Sound => self.app.sound_pane(ui),
                p if p.steady_view().is_some() => self
                    .app
                    .steady_pane(ui, p.steady_view().unwrap_or_default()),
                Pane::Isochronism => self.app.isochronism_pane(ui),
                Pane::PositionsTest => self.app.positions_pane(ui),
                Pane::Scope => self.app.scope_pane(ui),
                p if Pane::HISTOGRAMS.contains(p) => self.app.histogram(ui, *p),
                p => self.app.chart(ui, *p),
            }
        });
        UiResponse::None
    }

    /// A pane averaging over a time of its own names it.
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        let title = match self.app.pane_average.get(pane) {
            Some(&s) => format!("{} · {}", pane.title(), fields::duration(s)),
            None => pane.title().to_string(),
        };
        RichText::new(title).font(theme::semibold(12.5)).into()
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
            self.app.info_msg(format!(
                "Hid {}. Its icon at the top, or the View menu, brings it back.",
                p.title()
            ));
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

    /// The selected tab takes the card's colour, joined to the card below
    /// it like a folder's tab.
    fn tab_bg_color(
        &self,
        visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Color32 {
        let p = theme::palette(visuals.dark_mode);
        if state.active {
            p.card
        } else {
            p.window
        }
    }

    fn on_tab_button(
        &mut self,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        button_response: egui::Response,
    ) -> egui::Response {
        button_response.on_hover_text(
            "Drag a tab to move the pane, or beside another to show both. The × hides it; \
             its icon at the top, or the View menu, brings it back.",
        )
    }

    /// A chart's key at the right of its tab bar, so it never covers the
    /// lines.
    fn top_bar_right_ui(
        &mut self,
        tiles: &Tiles<Pane>,
        ui: &mut egui::Ui,
        _tile_id: TileId,
        tabs: &egui_tiles::Tabs,
        _scroll_offset: &mut f32,
    ) {
        let Some(egui_tiles::Tile::Pane(pane)) = tabs.active.and_then(|a| tiles.get(a)) else {
            return;
        };
        let pal = theme::pal(ui);
        ui.spacing_mut().item_spacing.x = 4.0;
        // Right to left: a chart's "?" and Match Strip, then its key.
        if Pane::CHARTS.contains(pane) {
            ui.add_space(4.0);
            theme::help(ui, pane.title(), help::CHARTS);
            ui.add_space(6.0);
            theme::toggle(ui, "Match Strip", &mut self.app.span_of_strip).on_hover_text(
                "Show the same stretch of time as the paper strip, so the charts line up with \
                 it and move with it. Off, the charts show the whole session.",
            );
            ui.add_space(6.0);
            self.app.pane_average_choice(ui, *pane);
            ui.label(RichText::new("Average Over").size(12.5))
                .on_hover_text(PANE_AVERAGE_HINT);
            ui.add_space(8.0);
        }
        // The key only when all of it fits beside the tab's title.
        let key = chart_key(*pane, pal);
        let font = egui::TextStyle::Small.resolve(ui.style());
        let needed: f32 = key
            .iter()
            .map(|(name, _, _)| {
                ui.fonts_mut(|f| {
                    f.layout_no_wrap(name.to_string(), font.clone(), pal.text_secondary)
                })
                .size()
                .x + 26.0
            })
            .sum();
        if needed + 80.0 > ui.available_width() {
            return;
        }
        for (name, color, gaps) in key.into_iter().rev() {
            ui.add_space(6.0);
            ui.label(RichText::new(name).small().color(pal.text_secondary));
            match gaps {
                Some(g) => theme::dash(ui, color, g),
                None => theme::dot(ui, color),
            };
        }
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

/// A break in the beats while listening longer than this, seconds, may be
/// the watch being moved, and the app asks.
const MOVE_GAP_S: f64 = 3.0;

/// What the settling after a move is, wherever it is shown.
const SETTLE_HINT: &str = "For the first 10 seconds after the watch is put in a new position \
     (from Resume, when it was moved while stopped) it settles: the hand leaves the stand and the \
     balance steadies. Those beats are drawn faintly on the Paper Strip and left out of the \
     readings, the charts' points, the Distributions and the Steadiness tests. The sound is \
     still kept, and a saved take's positions start after it.";

/// Where a bug report starts: the repository's bug form.
const BUG_REPORT_URL: &str =
    "https://github.com/peterhudson/timegrapher/issues/new?template=bug.yml";

/// The session file a recording's folder may carry (see `timegrapher
/// session`), which the app writes when it saves a take.
const MANIFEST: &str = "session.toml";

/// The session file beside a recording or in its folder, and its entries
/// for that recording: none when there is no session file or it doesn't
/// list the recording.
fn read_manifest(path: &Path) -> Result<Option<(Manifest, Vec<RecordingEntry>)>, String> {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        match path.parent() {
            Some(p) => p.to_path_buf(),
            None => return Ok(None),
        }
    };
    let file = dir.join(MANIFEST);
    if !file.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let m: Manifest =
        toml::from_str(&text).map_err(|e| format!("Can't read {}: {e}", file.display()))?;
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let entries: Vec<RecordingEntry> = m
        .recordings
        .iter()
        .filter(|e| same(&dir.join(&e.file), path))
        .cloned()
        .collect();
    if entries.is_empty() {
        return Ok(None);
    }
    m.validate_analysis_inputs()
        .map_err(|e| format!("Can't read {}: {e}", file.display()))?;
    Ok(Some((m, entries)))
}

/// How the files in a folder are put together, said where a folder is
/// opened. The command line's `long` and `series` read folders the same way.
const FOLDER_RULE: &str = "A recording this app saved (its own folder, or the folder you saved it \
     into), or a folder of WAV or FLAC segments of one long run, joined end to end in the order \
     its session.toml lists them, else in file-name order with numbers counted (part2 before \
     part10).";

/// The recordings to analyse for `path`: the file itself, or a folder's
/// WAV and FLAC files (the segments of one long recording) in the order
/// its session file lists them, then any it doesn't list in natural name
/// order.
fn recordings_in(path: &Path) -> Result<Vec<PathBuf>, String> {
    let listed = if path.is_dir() {
        listed_order(path).unwrap_or_default()
    } else {
        Vec::new()
    };
    timegrapher_core::recording_order::recordings_in(path, &listed)
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Names compared with each run of digits read as a number, so part2
/// comes before part10, and otherwise ignoring case.
pub(crate) fn natural_order(a: &str, b: &str) -> std::cmp::Ordering {
    timegrapher_core::recording_order::natural_order(a, b)
}

/// File entries from a valid session manifest. The shared folder selector
/// ranks existing direct files and ignores whole-folder or missing entries.
fn listed_order(dir: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(dir.join(MANIFEST)).ok()?;
    let manifest: Manifest = toml::from_str(&text).ok()?;
    Some(
        manifest
            .recordings
            .into_iter()
            .map(|entry| entry.file)
            .collect(),
    )
}

/// Where the order the files are joined in disagrees with when they were
/// written: a file whose modification time is earlier than the one before
/// it by more than that file's own length. Said so the order can be
/// checked, not changed.
fn order_warning(files: &[PathBuf]) -> Option<String> {
    let when = |f: &PathBuf| std::fs::metadata(f).and_then(|m| m.modified()).ok();
    let length = |f: &PathBuf| {
        timegrapher_core::audio::info(f)
            .ok()
            .and_then(|i| i.frames.map(|n| n as f64 / i.sample_rate.max(1) as f64))
    };
    for pair in files.windows(2) {
        let (Some(a), Some(b)) = (when(&pair[0]), when(&pair[1])) else {
            continue;
        };
        let Ok(back) = a.duration_since(b) else {
            continue;
        };
        if back.as_secs_f64() > length(&pair[0]).unwrap_or(0.0) {
            return Some(format!(
                "{} was written before {}, which comes ahead of it: check the order the files \
                 are joined in.",
                file_name(&pair[1]),
                file_name(&pair[0])
            ));
        }
    }
    None
}

/// The folder to open for `path`: itself when it holds sound (or isn't a
/// folder), else the one take folder inside it, which is where the app
/// saves a take. Several takes inside are named so one can be picked.
fn take_folder(path: &Path) -> Result<PathBuf, String> {
    if !path.is_dir() || recordings_in(path).is_ok() {
        return Ok(path.to_path_buf());
    }
    let mut takes: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| format!("Can't read the folder {}: {e}", path.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && recordings_in(p).is_ok())
        .collect();
    takes.sort();
    match takes.len() {
        0 => recordings_in(path).map(|_| path.to_path_buf()),
        1 => Ok(takes.remove(0)),
        _ => Err(format!(
            "{} holds {} takes; open one of them: {}.",
            path.display(),
            takes.len(),
            takes
                .iter()
                .map(|p| capture_label(p))
                .collect::<Vec<_>>()
                .join(", ")
        )),
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
    /// The menus and pane icons across the top, the status line along the
    /// bottom and the readings and panes between, all on the window colour
    /// with the content in cards.
    fn panels(&mut self, ctx: &egui::Context) {
        let pal = theme::palette(ctx.style().visuals.dark_mode);
        let bar = egui::Frame::new().fill(pal.window);
        egui::TopBottomPanel::top("menus")
            .frame(bar.inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ctx, |ui| self.menu_bar(ui));
        egui::TopBottomPanel::bottom("status")
            .frame(bar.inner_margin(egui::Margin::symmetric(14, 5)))
            .show(ctx, |ui| self.status_bar(ui));
        let command = |key| egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key);
        // The File menu's shortcuts.
        if ctx.input_mut(|i| i.consume_shortcut(&command(egui::Key::O))) {
            self.open_from_menu(false);
        }
        // Save As first: Cmd+S would also take Cmd+Shift+S.
        if ctx.input_mut(|i| i.consume_shortcut(&save_as_shortcut()))
            && self.session.allows(Action::Save)
        {
            self.save_recording();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&command(egui::Key::S)))
            && self.session.allows(Action::Save)
        {
            self.save_again();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&command(egui::Key::Q))) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        egui::CentralPanel::default()
            .frame(bar.inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 8,
                bottom: 12,
            }))
            .show(ctx, |ui| self.main_view(ui));
        self.manage_calibres(ctx);
    }
}

impl eframe::App for TimegrapherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.check_pending) {
            self.updates.start(ctx, false);
        }
        if let Some(m) = self.updates.poll() {
            self.info_msg(m);
        }
        // What the readings rest on: a calibre's lift angle and a measured
        // clock.
        let lift_known = self.lift_known();
        let clock_known = self.clock.is_some();
        if let Some(l) = &mut self.live {
            l.set_known(lift_known, clock_known);
        }
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
            self.open_recording(&p, false, true);
        }

        self.poll();

        // Closing the window with unsaved sound asks first.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.may_close
            && self.needs_save_confirmation()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::Quit);
        }
        self.guided_tick(ctx);
        self.panels(ctx);
        self.confirm_dialog(ctx);
        if let Some(m) = self.report.ui(ctx) {
            self.info_msg(m);
        }
        // The cursor shared by the strip and the charts follows the pointer;
        // repaint once more when it leaves so the line goes too.
        let next = self.cursor_next.take();
        if next != self.cursor_t {
            self.cursor_t = next;
            ctx.request_repaint();
        }

        if self.running() || self.batch.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        } else if self.sound_job.is_some() || self.scope_job.is_some() || self.steady_job.is_some()
        {
            // Show the profile when it's worked out, without waiting for
            // the pointer to move.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// What shows where no panel paints (the seams between panels): the
    /// window colour, not eframe's near-black.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        theme::palette(visuals.dark_mode)
            .window
            .to_normalized_gamma_f32()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        crate::settings::store(storage, &self.settings());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop();
        self.session.discard_recording();
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

/// A beat on the Scope: its index, its time's bits (so a new session's
/// beat of the same index is another) and the span drawn.
type ScopeKey = (i64, u64, scope::Zoom, u64);

/// What a pane's Average Over does.
const PANE_AVERAGE_HINT: &str = "How much of the beats each point in this pane is averaged over. \
     As Readings follows Average Over in the Readings card. A time of its own sets \
     everything in this pane that Average Over sets, and is named on the pane's tab, until \
     Average Over in the Readings card is changed: then every pane follows that again.";

/// Everything Average Over sets, and what it leaves alone.
/// The speeds Replay offers, times the recording's own.
const REPLAY_SPEEDS: [f32; 6] = [0.5, 1.0, 2.0, 4.0, 8.0, 16.0];

fn speed_text(x: f32) -> String {
    if x == 1.0 {
        "1× (as recorded)".into()
    } else {
        format!("{}×", fields::plain(x as f64))
    }
}

const AVERAGE_REACH: &str = "How much of the latest beats each reading is fitted over. It sets: \
     the three readings and their ±; every point on the Rate, \
     Amplitude and Beat Error charts; the Paper Strip's Rate Line and the Amplitude, Rate \
     and Beat Error lines drawn over it; the Distributions when Values is Readings; and the Tick Tock Profile, \
     the typical beat over the same time up to 60 s, with the Unlock and Drop marks on the \
     Scope. It doesn't change Steadiness and Isochronism (always 10 s rate and 2 s \
     amplitude readings) or the Positions Test (a minute a position).";

/// How many beats' traces the Scope keeps: the most it draws, both sides.
const SCOPE_KEPT: usize = 40;

/// A stretch of a recording read for the scope: the start and end asked
/// for, s, and the sound read (shorter at the end of the recording), or why
/// it couldn't be.
type ScopeCut = (f64, f64, Result<crate::span::Span, String>);

/// The lift angle used until a calibre is picked or one is typed in.
const DEFAULT_LIFT_DEG: f64 = 52.0;

/// An error as a person reads it: the system's own words for the common
/// file errors, and no "(os error 2)" codes. The log keeps the original.
fn plain_error(m: &str) -> String {
    let mut t = m
        .replace("No such file or directory", "nothing is there")
        .replace("Permission denied", "it isn't allowed to be read here")
        .replace("Is a directory", "it is a folder, not a file");
    while let Some(i) = t.find(" (os error ") {
        match t[i..].find(')') {
            Some(j) => t.replace_range(i..i + j + 1, ""),
            None => break,
        }
    }
    t
}

/// What limits a reading, as facts: a short form for the card's caption
/// and a sentence for its hover. Nothing graded: the ± beside the figure
/// says how far it could be off; these say what the ± can't show.
/// "No regular beat found", with the timing scatter that decided it when
/// there is one: the beats' median jitter in ms and as a share of a beat.
fn no_regular_beat(r: &LiveReading) -> String {
    match (r.jitter_us, r.bph) {
        (Some(j), Some(bph)) => format!(
            "No regular beat found (timing scatter {:.1} ms, {:.1}% of a beat)",
            j / 1000.0,
            j / (3.6e9 / bph.max(1) as f64) * 100.0
        ),
        _ => "No regular beat found".to_string(),
    }
}

fn reading_limits(
    title: &str,
    caveats: &[Caveat],
    lift_deg: f64,
    beats: usize,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for c in caveats {
        let fact = match (c, title) {
            (Caveat::ShortSpan { span_s }, _) => Some((
                format!("{span_s:.0} s of beats"),
                format!(
                    "The beats span {span_s:.0} s, under the {:.0} s an analysis of a \
                     recording asks for.",
                    timegrapher_core::measure::SHORT_SPAN_S
                ),
            )),
            (Caveat::BeatsMissing { fraction }, _) => {
                let expected = (beats as f64 / fraction.max(1e-9)).round() as usize;
                let short = if beats > 0 {
                    format!("{beats} of {expected} expected beats found")
                } else {
                    format!("{:.0}% of expected beats found", fraction * 100.0)
                };
                Some((
                    short,
                    format!(
                        "{:.0}% of the beats the beat rate expects in this stretch were found.",
                        fraction * 100.0
                    ),
                ))
            }
            (Caveat::Clipped { fraction }, "Amplitude" | "Beat Error") => Some((
                format!("{:.0}% of beats clipped", fraction * 100.0),
                format!(
                    "{:.0}% of the beats have a clipped sample, which moves the timing of \
                     the sounds inside a beat.",
                    fraction * 100.0
                ),
            )),
            (Caveat::LiftUnknown, "Amplitude") => Some((
                format!("lift angle {lift_deg:.0}°, the default (calibre not set)"),
                format!(
                    "The lift angle is the default {lift_deg:.0}°, not one set for the \
                     calibre. Every amplitude is worked out from it."
                ),
            )),
            (Caveat::ClockUnknown, "Rate") => Some((
                CLOCK_UNKNOWN.to_string(),
                "This input's clock hasn't been measured, so the rate includes the sound \
                 card's own clock error."
                    .to_string(),
            )),
            (Caveat::SetRateDisagrees { heard }, _) => Some((
                format!("{heard} bph heard"),
                format!("The beat rate set differs from the {heard} bph heard."),
            )),
            (Caveat::Breaks { count }, _) => Some((
                format!("{count} pause{}", if *count == 1 { "" } else { "s" }),
                format!("The take was paused {count} times in this stretch."),
            )),
            _ => None,
        };
        out.extend(fact);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use timegrapher_core::recorder::SETTLE_S;

    #[test]
    fn errors_read_without_codes() {
        assert_eq!(
            plain_error("Can't read /x/y.flac: No such file or directory (os error 2)"),
            "Can't read /x/y.flac: nothing is there"
        );
        assert_eq!(plain_error("Saving failed"), "Saving failed");
    }

    #[test]
    fn a_reading_says_what_limits_it_as_facts() {
        let caveats = [
            Caveat::ShortSpan { span_s: 4.0 },
            Caveat::LiftUnknown,
            Caveat::ClockUnknown,
        ];
        let amp = reading_limits("Amplitude", &caveats, 52.0, 30);
        let short: Vec<&str> = amp.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(
            short,
            [
                "4 s of beats",
                "lift angle 52°, the default (calibre not set)"
            ]
        );
        let rate = reading_limits("Rate", &caveats, 52.0, 30);
        let short: Vec<&str> = rate.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(short, ["4 s of beats", "input clock not measured"]);
        let missing = reading_limits("Rate", &[Caveat::BeatsMissing { fraction: 0.9 }], 52.0, 90);
        assert_eq!(missing[0].0, "90 of 100 expected beats found");
        for (_, long) in amp.iter().chain(&rate) {
            for word in [
                "Confidence",
                "Low",
                "High",
                "Medium",
                "trust",
                "pick",
                "Turn",
            ] {
                assert!(!long.contains(word), "{long}");
            }
        }
    }
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

    /// Stopped after the watch is taken off the stand, the readings stay on
    /// the last stretch with a watch heard, and a long enough take shows
    /// its whole-take mean and spread.
    #[test]
    fn stopping_keeps_the_last_heard_readings() {
        let mut app = synthetic(60.0, 12.0);
        let before = app.reading().unwrap();
        let whole = app.whole_take(|p| p.rate).expect("whole take");
        assert!((whole.0 - 12.0).abs() < 2.0, "{whole:?}");
        assert!(whole.2 > 40.0, "{whole:?}");
        // Then 20 s of hiss with the watch gone.
        let mut seed = 1u32;
        let noise: Vec<f32> = (0..48000 * 20)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 * 0.02 - 0.01
            })
            .collect();
        let live = app.live.as_mut().unwrap();
        for b in noise.chunks(960) {
            if live.push(b) {
                let r = live.reading(app.average_s);
                app.trend.push(TrendPoint::of(r.time_s, &r));
            }
        }
        let after = app.reading().unwrap();
        let (a, b) = (
            before.rate_s_per_day.unwrap(),
            after.rate_s_per_day.unwrap(),
        );
        assert!((a - b).abs() < 1.0, "{a} then {b}");
        assert!(after.time_s <= 62.0, "{}", after.time_s);
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

    /// After a move the watch settles for SETTLE_S: nothing is read then,
    /// its beats are left out of the position's spans, and a move during a
    /// settling starts it again.
    #[test]
    fn a_move_settles_before_it_is_read() {
        let mut app = synthetic(60.0, 0.0);
        app.session
            .load(&[(0.0, Position::DialUp), (30.0, Position::DialDown)], &[]);
        app.position = Position::DialDown;
        app.session.start_settle(30.0);
        assert_eq!(app.session.settles(), [(30.0, 30.0 + SETTLE_S)]);
        assert!(!app.in_position(35.0) && app.in_position(45.0));
        assert_eq!(app.current_spans(), [(30.0 + SETTLE_S, f64::INFINITY)]);
        app.view_end = Some(35.0);
        assert!(app.settling_left().is_some() && app.reading().is_none());
        app.view_end = Some(50.0);
        assert!(app.settling_left().is_none() && app.reading().is_some());
        // Moved again 4 s in: the first settling ends there.
        app.session.start_settle(34.0);
        assert_eq!(
            app.session.settles(),
            [(30.0, 34.0), (34.0, 34.0 + SETTLE_S)]
        );
        assert_eq!(app.session.settle_to(), Some(34.0 + SETTLE_S));
    }

    /// The folder a take was saved into opens the take inside it; with two
    /// takes inside, the message names them.
    #[test]
    fn a_save_folder_opens_the_take_inside() {
        let base = std::env::temp_dir().join(format!("tg-app-take-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let take = base.join("2026-10-11_DD3235");
        std::fs::create_dir_all(&take).unwrap();
        let tone = timegrapher_core::audio::Audio {
            samples: vec![0.0; 800],
            sample_rate: 8000,
        };
        timegrapher_core::audio::write_wav(&take.join("audio-001.wav"), &tone).unwrap();
        assert_eq!(take_folder(&base).unwrap(), take);
        assert_eq!(take_folder(&take).unwrap(), take);
        let other = base.join("2026-10-12_DD3235");
        std::fs::create_dir_all(&other).unwrap();
        timegrapher_core::audio::write_wav(&other.join("audio-001.wav"), &tone).unwrap();
        let e = take_folder(&base).unwrap_err();
        assert!(
            e.contains("2 takes") && e.contains("2026-10-12_DD3235"),
            "{e}"
        );
        let _ = std::fs::remove_dir_all(&base);
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
        assert!(take_folder(&empty).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Numbers in names count as numbers; a session file's list comes
    /// first; files written out of the joining order are pointed out.
    #[test]
    fn folder_files_join_in_the_agreed_order() {
        use std::cmp::Ordering::Less;
        assert_eq!(natural_order("part2.wav", "part10.wav"), Less);
        assert_eq!(natural_order("take_part01-009", "take_part01-010"), Less);
        assert_eq!(natural_order("take_part01-099", "take_part02-001"), Less);
        assert_eq!(natural_order("A.wav", "b.wav"), Less);

        let dir = std::env::temp_dir().join(format!("tg-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = |name: &str| {
            let a = timegrapher_core::audio::Audio {
                samples: vec![0.0; 8000],
                sample_rate: 8000,
            };
            timegrapher_core::audio::write_wav(&dir.join(name), &a).unwrap();
        };
        for n in ["b-1.wav", "a-2.wav", "a-10.wav", "a-1.wav"] {
            part(n);
        }
        let names = |d: &Path| -> Vec<String> {
            recordings_in(d)
                .unwrap()
                .iter()
                .map(|f| file_name(f))
                .collect()
        };
        assert_eq!(names(&dir), ["a-1.wav", "a-2.wav", "a-10.wav", "b-1.wav"]);
        // Written within a second of each other, one second long each.
        assert_eq!(order_warning(&recordings_in(&dir).unwrap()), None);
        std::fs::write(
            dir.join(MANIFEST),
            "[[recording]]\nfile = \"b-1.wav\"\n\n[[recording]]\nfile = \"a-2.wav\"\n",
        )
        .unwrap();
        assert_eq!(names(&dir), ["b-1.wav", "a-2.wav", "a-1.wav", "a-10.wav"]);
        // a-2 written an hour before b-1, which is joined ahead of it.
        let f = std::fs::File::options()
            .write(true)
            .open(dir.join("a-2.wav"))
            .unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
        let w = order_warning(&recordings_in(&dir).unwrap()).unwrap();
        assert!(w.starts_with("a-2.wav was written before b-1.wav"), "{w}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn save_failure_fixture(name: &str) -> (TimegrapherApp, PathBuf) {
        let base =
            std::env::temp_dir().join(format!("tg-app-save-failure-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.reset_session(48000, "test".into());
        app.session.listen();
        app.session.recorder =
            Some(Recorder::start(&base.join("live"), app.info("test", 48000, 16)).unwrap());
        append_save_test_second(&mut app, 1);
        (app, base)
    }

    fn append_save_test_second(app: &mut TimegrapherApp, at: u64) {
        let block = timegrapher_core::capture::Block {
            samples: vec![0.0; 48000],
            at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(at),
        };
        app.session
            .recorder
            .as_mut()
            .unwrap()
            .write(&block)
            .unwrap();
        for samples in block.samples.chunks(960) {
            app.live.as_mut().unwrap().push(samples);
        }
    }

    fn saved_file_bytes(dir: &Path) -> std::collections::BTreeMap<std::ffi::OsString, Vec<u8>> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().unwrap().is_file())
            .map(|e| (e.file_name(), std::fs::read(e.path()).unwrap()))
            .collect()
    }

    // Replay supplies a real Capture without opening hardware. Its tiny file
    // has no samples; replace only its public receiver with a controlled queue.
    fn queued_capture(app: &mut TimegrapherApp, base: &Path) -> std::sync::mpsc::Sender<Event> {
        let path = base.join("empty-input.wav");
        hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap()
        .finalize()
        .unwrap();
        let mut capture = capture::replay_file(&path).unwrap();
        // End is sent after audio::stream closes the input. Wait before
        // replacing rx so Windows cleanup cannot race that reader thread.
        assert!(matches!(
            capture
                .rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            Event::End
        ));
        let (tx, rx) = std::sync::mpsc::channel();
        capture.rx = rx;
        capture.is_file = false;
        app.capture = Some(capture);
        app.opened_at = Instant::now();
        tx
    }

    fn queue_segment_open_failure(
        app: &mut TimegrapherApp,
        base: &Path,
    ) -> std::sync::mpsc::Sender<Event> {
        let now = app.live.as_ref().unwrap().duration_s();
        app.session.pause(now).unwrap().unwrap();
        app.session.resume("test").unwrap();
        app.session.resumed_at(now);
        // A real, deterministic failure before writing the next segment.
        // This does not simulate a mid-sample write or a full disk.
        std::fs::create_dir(
            app.session
                .recorder
                .as_ref()
                .unwrap()
                .dir()
                .join("audio-002.wav"),
        )
        .unwrap();
        let tx = queued_capture(app, base);
        tx.send(Event::Audio(timegrapher_core::capture::Block {
            samples: vec![0.0; 960],
            at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(2),
        }))
        .unwrap();
        tx
    }

    #[test]
    fn recording_failure_stops_capture_and_retains_scratch_with_or_without_input_error() {
        for input_error in [false, true] {
            let (mut app, base) = save_failure_fixture(if input_error {
                "recording-input"
            } else {
                "recording-only"
            });
            app.save_into(&base.join("saved"));
            let saved = app.session.saved_to().unwrap().to_path_buf();
            let saved_bytes = saved_file_bytes(&saved);
            let scratch = app.session.recorder.as_ref().unwrap().dir().to_path_buf();
            let tx = queue_segment_open_failure(&mut app, &base);
            let before = app.live.as_ref().unwrap().duration_s();
            if input_error {
                tx.send(Event::Error("queued input failure".into()))
                    .unwrap();
            }
            app.poll();
            assert!(!app.running());
            assert!(app.session.paused());
            assert!(app.session.recorder.is_some());
            assert!(!app.can_resume());
            assert!(!app.session.allows(Action::Save));
            assert_eq!(app.live.as_ref().unwrap().duration_s(), before);
            assert_eq!(app.session.recorder.as_ref().unwrap().duration_s(), 1.0);
            let fault = app.session.recording_fault().unwrap();
            assert_eq!(fault.recovery_dir, scratch);
            assert!(!fault.reason.contains("queued input failure"));
            assert!(fault.marker_error.is_none());
            assert!(scratch.join("RECORDING-ERROR.txt").is_file());
            assert_eq!(
                hound::WavReader::open(scratch.join("audio-001.wav"))
                    .unwrap()
                    .duration(),
                48000
            );
            assert!(
                app.needs_save_confirmation(),
                "even a saved, short faulted take needs a retention warning"
            );
            // Direct methods must not reach hardware, a file dialog, or a save.
            app.devices_listed = false;
            app.start_microphone();
            app.resume_microphone();
            assert!(!app.devices_listed);
            assert!(!app.running());
            app.save_into(&base.join("must-not-exist"));
            app.save_again();
            app.save_recording();
            assert!(!base.join("must-not-exist").exists());
            assert_eq!(saved_file_bytes(&saved), saved_bytes);
            app.info_msg("Another notice".into());
            assert!(
                app.session.recording_fault().is_some(),
                "ordinary messages cannot dismiss the fault"
            );
            app.poll();
            assert!(!app.running());
            drop(tx);
            if input_error {
                eframe::App::on_exit(&mut app, None);
            } else {
                app.new_session();
            }
            assert!(scratch.join("audio-001.wav").is_file());
            assert_eq!(saved_file_bytes(&saved), saved_bytes);
            assert!(app.session.recorder.is_none());
            assert!(app.session.recording_fault().is_none());
            std::fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn input_only_failure_keeps_the_existing_resume_and_save_contract() {
        let (mut app, base) = save_failure_fixture("input-only");
        let tx = queued_capture(&mut app, &base);
        tx.send(Event::Error("input disconnected".into())).unwrap();
        app.poll();
        assert!(!app.running());
        assert!(app.can_resume());
        assert!(app.session.allows(Action::Save));
        assert!(app.session.recording_fault().is_none());
        app.save_into(&base.join("saved"));
        assert!(app.session.saved_to().unwrap().join(MANIFEST).is_file());
        app.new_session();
        drop(tx);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn pause_and_input_stop_close_failures_retain_scratch_and_report_marker_failure() {
        for input_stop in [false, true] {
            let (mut app, base) = save_failure_fixture(if input_stop {
                "input-close"
            } else {
                "pause-close"
            });
            let scratch = app.session.recorder.as_ref().unwrap().dir().to_path_buf();
            std::fs::remove_file(scratch.join("session.json")).unwrap();
            std::fs::create_dir(scratch.join("session.json")).unwrap();
            std::fs::create_dir(scratch.join("RECORDING-ERROR.txt")).unwrap();
            let tx = queued_capture(&mut app, &base);
            if input_stop {
                tx.send(Event::Error("input disconnected".into())).unwrap();
                app.poll();
            } else {
                app.pause();
            }
            assert!(!app.running());
            assert!(!app.can_resume());
            assert!(!app.session.allows(Action::Save));
            let fault = app.session.recording_fault().unwrap();
            assert!(fault.reason.contains("Couldn't close"));
            assert!(fault.marker_error.is_some());
            assert!(fault
                .description()
                .contains("Couldn't write the recording-error marker"));
            let message = &app.message.as_ref().unwrap().0;
            assert!(message.contains("Recording stopped"));
            if input_stop {
                assert!(message.contains("input disconnected"));
            }
            app.new_session();
            assert!(scratch.join("audio-001.wav").is_file());
            drop(tx);
            std::fs::remove_dir_all(base).unwrap();
        }
    }

    #[test]
    fn persisted_lift_context_roundtrips_without_guessing_legacy_history() {
        for (text, expected) in [
            ("lift = 52\nlift_known = false", false),
            ("lift = 52\nlift_known = true", true),
            ("lift = 49\nlift_known = false", false),
            ("lift = 52", false), // Legacy app inference, unlike legacy CLI.
            ("lift = 49", true),
            ("lift_known = true", false), // No accompanying value.
            ("calibre = \"Seagull ST36\"", false),
            ("calibre = \"Unitas 6497-2\"", true),
            (
                "calibre = \"Unitas 6497-2\"\nlift = 52\nlift_known = false",
                false,
            ),
        ] {
            let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
            let m: Manifest = toml::from_str(text).unwrap();
            app.apply_manifest(&m, &[]);
            assert_eq!(app.lift_known(), expected, "{text}");
            assert_eq!(app.live_config().analysis.lift_known, expected);
            let value = app.lift_deg;
            let setup = app.watch_setup();
            let saved = toml::to_string(&app.take_manifest()).unwrap();
            let again: Manifest = toml::from_str(&saved).unwrap();
            assert_eq!(again.lift, Some(value));
            assert_eq!(again.lift_known, Some(expected));
            let mut reopened = TimegrapherApp::with_devices(Vec::new(), None, false);
            reopened.apply_manifest(&again, &[]);
            assert_eq!(
                (reopened.lift_deg, reopened.lift_known()),
                (value, expected)
            );
            reopened.set_watch_setup(WatchSetup::default());
            reopened.set_watch_setup(setup);
            assert_eq!(
                (reopened.lift_deg, reopened.lift_known()),
                (value, expected)
            );
            reopened.set_lift(value + 1.0);
            reopened.set_lift(value);
            assert_eq!(
                reopened.lift_known(),
                expected,
                "numeric revert retains context"
            );
        }
    }

    #[test]
    fn persisted_lift_context_only_edit_stays_dirty_until_successful_save() {
        let (mut app, base) = save_failure_fixture("lift-context");
        app.save_into(&base.join("saved"));
        let dest = app.session.saved_to().unwrap().to_path_buf();
        let old = std::fs::read(dest.join(MANIFEST)).unwrap();
        let explicit: Manifest = toml::from_str("lift = 52\nlift_known = true").unwrap();
        app.apply_manifest(&explicit, &[]);
        assert!(app.lift_known());
        assert!(
            app.metadata_unsaved(),
            "same value, different persisted context"
        );
        std::fs::remove_file(dest.join(MANIFEST)).unwrap();
        std::fs::create_dir(dest.join(MANIFEST)).unwrap();
        app.save_again();
        assert!(app.metadata_unsaved());
        assert!(app.message.as_ref().is_some_and(|(_, error)| *error));
        std::fs::remove_dir(dest.join(MANIFEST)).unwrap();
        std::fs::write(dest.join(MANIFEST), old).unwrap();
        app.save_again();
        assert!(!app.metadata_unsaved());
        let m: Manifest =
            toml::from_str(&std::fs::read_to_string(dest.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(m.lift_known, Some(true));
        app.session.discard_recording();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn saved_metadata_edits_warn_without_new_sound_and_reverts_are_clean() {
        let (mut app, base) = save_failure_fixture("metadata-edits");
        app.watch = "original".into();
        app.save_into(&base.join("saved"));
        app.session
            .pause(app.live.as_ref().unwrap().duration_s())
            .unwrap()
            .unwrap();
        assert!(!app.needs_save_confirmation());
        let original = app.save_metadata();
        app.watch = "renamed".into();
        assert!(app.metadata_unsaved());
        assert!(app.needs_save_confirmation());
        assert_eq!(app.unsaved_s(), 0.0);
        app.open_recording(&base.join("other.wav"), false, true);
        assert!(matches!(app.confirm, Some(Confirm::Open { .. })));
        assert!(app.session.recorder.is_some());
        app.confirm = None;
        app.watch = original.watch.clone();
        assert!(!app.needs_save_confirmation());
        app.bph = Some(21600);
        assert!(app.metadata_unsaved());
        app.bph = original.bph;
        app.set_lift(original.lift_deg + 1.0);
        assert!(app.metadata_unsaved());
        app.set_lift(original.lift_deg);
        app.calibre = "another calibre".into();
        assert!(app.metadata_unsaved());
        app.calibre = original.calibre.clone();
        app.hist_cumulative = !app.hist_cumulative;
        app.hist_span_of_strip = !app.hist_span_of_strip;
        assert!(
            !app.metadata_unsaved(),
            "view preferences are not saved take edits"
        );
        app.change_position(Position::DialDown);
        app.change_position(Position::DialUp);
        assert!(
            app.metadata_unsaved(),
            "returning to DU retains appended position notes"
        );
        app.save_again();
        assert!(!app.needs_save_confirmation());
        app.session.discard_recording();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn short_audio_tail_does_not_become_an_unsaved_metadata_warning() {
        let (mut app, base) = save_failure_fixture("metadata-audio");
        app.change_position(Position::DialDown);
        app.save_into(&base.join("saved"));
        let ranges = app.take_manifest().recordings;
        append_save_test_second(&mut app, 2);
        assert_eq!(app.unsaved_s(), 1.0);
        assert_ne!(
            app.take_manifest().recordings.last().unwrap().start_s,
            ranges.last().unwrap().start_s
        );
        assert!(!app.metadata_unsaved());
        assert!(!app.needs_save_confirmation());
        app.session.discard_recording();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn failed_metadata_only_save_retains_dirty_state_and_the_saved_bytes() {
        let (mut app, base) = save_failure_fixture("metadata-failure");
        let parent = base.join("saved");
        app.save_into(&parent);
        let dest = app.session.saved_to().unwrap().to_path_buf();
        app.session
            .pause(app.live.as_ref().unwrap().duration_s())
            .unwrap()
            .unwrap();
        app.watch = "Updated watch".into();
        let previous = std::fs::read(dest.join(MANIFEST)).unwrap();
        std::fs::remove_file(dest.join(MANIFEST)).unwrap();
        std::fs::create_dir(dest.join(MANIFEST)).unwrap();
        let before = saved_file_bytes(&dest);
        app.save_again();
        assert!(app.message.as_ref().is_some_and(|(_, error)| *error));
        assert!(app.metadata_unsaved());
        assert!(
            app.needs_save_confirmation(),
            "the confirmation must not continue after failure"
        );
        assert_eq!(app.unsaved_s(), 0.0);
        assert_eq!(app.session.saved_to(), Some(dest.as_path()));
        assert_eq!(saved_file_bytes(&dest), before);
        std::fs::remove_dir(dest.join(MANIFEST)).unwrap();
        std::fs::write(dest.join(MANIFEST), previous).unwrap();
        app.save_again();
        assert!(!app.needs_save_confirmation());
        let manifest: Manifest =
            toml::from_str(&std::fs::read_to_string(dest.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(manifest.watch.as_deref(), Some("Updated watch"));
        let info: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dest.join("session.json")).unwrap()).unwrap();
        assert_eq!(info["watch"], "Updated watch");
        app.session.discard_recording();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn completed_guided_metadata_is_unsaved_until_the_required_file_succeeds() {
        let (mut app, base) = save_failure_fixture("metadata-guided");
        let parent = base.join("saved");
        app.save_into(&parent);
        let mut test = guided::Test::new();
        test.readings = Some(Vec::new());
        app.guided = Some(test);
        assert!(app.metadata_unsaved());
        let dest = app.session.saved_to().unwrap().to_path_buf();
        std::fs::create_dir(dest.join(guided::RESULTS_FILE)).unwrap();
        app.save_again();
        assert!(app.metadata_unsaved());
        std::fs::remove_dir(dest.join(guided::RESULTS_FILE)).unwrap();
        app.save_again();
        assert!(!app.metadata_unsaved());
        assert!(dest.join(guided::RESULTS_FILE).is_file());
        app.session.discard_recording();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn successful_save_keeps_the_callers_path_for_messages_and_save_again() {
        let (mut app, base) = save_failure_fixture("caller-path");
        let parent = base.join("saved").join("..").join("saved");
        app.save_into(&parent);
        let dest = app.session.saved_to().unwrap();
        assert_eq!(dest.parent().unwrap().as_os_str(), parent.as_os_str());
        assert_eq!(
            app.message.as_ref(),
            Some(&(
                format!(
                    "Saved {} of sound in {}",
                    strip::fmt_time(1.0),
                    dest.display()
                ),
                false
            ))
        );
        assert_eq!(app.save_dir, parent.display().to_string());
        assert!(dest.join(MANIFEST).is_file());
        app.session.discard_recording();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn failed_required_manifest_does_not_report_or_mark_a_successful_save() {
        let (mut app, base) = save_failure_fixture("manifest");
        let parent = base.join("saved");
        // Prepare the destination name without marking an app save. A directory
        // at the required file path deterministically rejects the file write.
        let dest = app
            .session
            .recorder
            .as_mut()
            .unwrap()
            .save_copy(&parent, "", None)
            .unwrap();
        std::fs::create_dir(dest.join(MANIFEST)).unwrap();
        let before = saved_file_bytes(&dest);
        app.save_into(&parent);
        assert!(app
            .message
            .as_ref()
            .is_some_and(|(m, error)| *error && m.contains(MANIFEST)));
        assert_eq!(app.session.saved_to(), None);
        assert_eq!(app.unsaved_s(), 1.0);
        assert_eq!(saved_file_bytes(&dest), before);
        std::fs::remove_dir(dest.join(MANIFEST)).unwrap();
        app.save_into(&parent);
        assert_eq!(app.session.saved_to(), Some(dest.as_path()));
        assert_eq!(app.unsaved_s(), 0.0);
        let manifest: Manifest =
            toml::from_str(&std::fs::read_to_string(dest.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(manifest.recordings.len(), 1);
        app.session.discard_recording();
        assert_eq!(
            timegrapher_core::audio::load(&dest.join("audio-001.wav"))
                .unwrap()
                .samples
                .len(),
            48000
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn failed_required_guided_result_preserves_the_previous_save_and_unsaved_sound() {
        let (mut app, base) = save_failure_fixture("guided");
        let parent = base.join("saved");
        app.save_into(&parent);
        let dest = app.session.saved_to().unwrap().to_path_buf();
        let before = saved_file_bytes(&dest);
        append_save_test_second(&mut app, 2);
        // Only the finished-state marker is needed here: this test covers
        // required result persistence, not the measurement of guided positions.
        let mut test = guided::Test::new();
        test.readings = Some(Vec::new());
        app.guided = Some(test);
        std::fs::create_dir(dest.join(guided::RESULTS_FILE)).unwrap();
        app.save_into(&parent);
        assert!(app
            .message
            .as_ref()
            .is_some_and(|(m, error)| *error && m.contains(guided::RESULTS_FILE)));
        assert_eq!(app.session.saved_to(), Some(dest.as_path()));
        assert_eq!(app.unsaved_s(), 1.0);
        assert_eq!(saved_file_bytes(&dest), before);
        std::fs::remove_dir(dest.join(guided::RESULTS_FILE)).unwrap();
        app.save_into(&parent);
        assert_eq!(app.unsaved_s(), 0.0);
        let kept = std::fs::read(dest.join(guided::RESULTS_FILE)).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&kept).unwrap();
        assert_eq!(json["format"], guided::RESULTS_FORMAT);
        // A later unfinished test (like a 24-hour half) supplies no new full-wind
        // result. Preserve the earlier result as a separate comparison artifact.
        app.guided = Some(guided::Test::new());
        app.save_into(&parent);
        assert_eq!(
            std::fs::read(dest.join(guided::RESULTS_FILE)).unwrap(),
            kept
        );
        app.session.discard_recording();
        assert_eq!(
            timegrapher_core::audio::load(&dest.join("audio-001.wav"))
                .unwrap()
                .samples
                .len(),
            96000
        );
        let manifest: Manifest =
            toml::from_str(&std::fs::read_to_string(dest.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(manifest.recordings.len(), 1);
        std::fs::remove_dir_all(base).unwrap();
    }

    /// A microphone session's sound is kept as it goes, can be saved at
    /// any point, and goes with a new session.
    #[test]
    fn sessions_keep_save_and_discard_their_sound() {
        let base = std::env::temp_dir().join(format!("tg-app-save-{}", std::process::id()));
        let mut app = synthetic(12.0, 20.0);
        app.session.listen();
        let info = app.info("test", 48000, 16);
        let mut r = Recorder::start(&base.join("kept"), info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 6],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        let kept = r.dir().to_path_buf();
        app.session.recorder = Some(r);
        assert!(app.unsaved_s() > 5.0, "unsaved sound to ask about");
        app.save_into(&base.join("saved"));
        let saved = app
            .session
            .saved_to()
            .map(Path::to_path_buf)
            .expect("saved");
        assert!(saved.join("audio-001.wav").is_file());
        assert_eq!(app.unsaved_s(), 0.0);
        // Paused, a session can carry on; a new session forgets it.
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        assert!(app.can_resume());
        app.new_session();
        assert!(app.live.is_none() && app.session.recorder.is_none() && !app.can_resume());
        assert!(!kept.exists(), "the unsaved copy is thrown away");
        assert!(
            saved.join("audio-001.wav").is_file(),
            "the saved copy stays"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Opening a recording over a stopped microphone session with unsaved
    /// sound asks first and keeps the sound; Recording chosen sets the
    /// session aside instead of showing it as a recording.
    #[test]
    fn opening_a_recording_asks_before_unsaved_sound_goes() {
        let base = std::env::temp_dir().join(format!("tg-app-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = synthetic(5.0, 0.0);
        app.session.listen();
        let info = app.info("test", 48000, 16);
        let mut r = Recorder::start(&base, info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 20],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        app.session.recorder = Some(r);
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        app.input = Input::File;
        assert!(app.mic_session_set_aside());
        let elsewhere = base.join("other.wav");
        app.open_recording(&elsewhere, false, true);
        assert!(matches!(app.confirm, Some(Confirm::Open { .. })));
        assert!(app.session.recorder.is_some() && app.unsaved_s() > 19.0);
        app.input = Input::Microphone;
        assert!(!app.mic_session_set_aside());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A break of more than a few seconds in the beats asks whether the
    /// watch moved, unless a move was marked in it.
    #[test]
    fn a_break_in_the_beats_asks_about_a_move() {
        let beat = |t: f64| timegrapher_core::beats::Beat {
            index: 0,
            time: t,
            quality: 1.0,
        };
        let beats: Vec<_> = [10.0, 10.125, 10.25, 16.0, 16.125]
            .into_iter()
            .map(beat)
            .collect();
        assert_eq!(sound_break(&beats, 10.25, &[]), Some((10.25, 16.0)));
        assert_eq!(sound_break(&beats, 10.125, &[]), None);
        assert_eq!(sound_break(&beats, 0.0, &[]), None);
        let marked = [(0.0, Position::DialUp), (12.0, Position::DialDown)];
        assert_eq!(sound_break(&beats, 10.25, &marked), None);
    }

    /// A position as a word of its own in a file's name, and only one.
    #[test]
    fn positions_are_read_from_file_names() {
        let p = |n: &str| position_in_name(Path::new(n));
        assert_eq!(p("reverso_DU_01.flac"), Some(Position::DialUp));
        assert_eq!(p("/x/ym42_dd_30min/ym42-cl.wav"), Some(Position::CrownLeft));
        assert_eq!(p("ducks.flac"), None);
        assert_eq!(p("du_then_dd.flac"), None);
    }

    /// Record, save, carry on, pause, change position, resume, save again
    /// and reopen: sound after a save still counts as unsaved, Save Again
    /// keeps it, and the saved take keeps the position it started in.
    #[test]
    fn a_take_saved_twice_across_a_move_keeps_everything() {
        let base = std::env::temp_dir().join(format!("tg-app-twice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.bph = Some(28800);
        app.reset_session(48000, "test".into());
        app.session.listen();
        let audio = generate(
            &SynthConfig {
                duration_s: 90.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let third = audio.samples.len() / 3;
        let began = std::time::SystemTime::now();
        let feed = |app: &mut TimegrapherApp, i: usize| {
            let block = timegrapher_core::capture::Block {
                samples: audio.samples[i * third..(i + 1) * third].to_vec(),
                // Blocks arrive at their final frame, with a 60 s pause
                // between the second and third blocks.
                at: began + std::time::Duration::from_secs([30, 60, 150][i]),
            };
            app.session
                .recorder
                .as_mut()
                .unwrap()
                .write(&block)
                .unwrap();
            // Like poll(), send the same sound to the recorder and the
            // live analyser so their clocks agree before each transition.
            for samples in block.samples.chunks(960) {
                app.live.as_mut().unwrap().push(samples);
            }
        };
        let info = app.info("test", 48000, 16);
        app.session.recorder = Some(Recorder::start(&base.join("kept"), info).unwrap());
        feed(&mut app, 0);
        app.save_into(&base.join("saved"));
        assert_eq!(app.unsaved_s(), 0.0);
        // Carrying on after the save: that sound isn't saved yet.
        feed(&mut app, 1);
        assert!((app.unsaved_s() - 30.0).abs() < 0.1, "{}", app.unsaved_s());
        // Paused, moved to dial down, resumed through the same session
        // lifecycle as pause()/resume_microphone(), without opening a mic.
        let paused_at = app.live.as_ref().unwrap().duration_s();
        assert_eq!(paused_at, 60.0);
        assert_eq!(app.session.recorder.as_ref().unwrap().duration_s(), 60.0);
        app.session.pause(paused_at).unwrap().unwrap();
        app.change_position(Position::DialDown);
        assert!(
            app.session.settles().is_empty(),
            "settling waits for Resume"
        );
        app.session.resume("test").unwrap();
        // Inject a deterministic elapsed pause instead of waiting for the
        // wall-clock duration returned by resume().
        let live = app.live.as_mut().unwrap();
        live.skip(60.0);
        app.session.resumed_at(live.duration_s());
        assert_eq!(app.session.settles(), [(120.0, 120.0 + SETTLE_S)]);
        for t in [60.0, 90.0, 120.0] {
            assert_eq!(app.session.audio_of(t), Some(60.0));
        }
        feed(&mut app, 2);
        assert_eq!(app.live.as_ref().unwrap().duration_s(), 150.0);
        assert_eq!(app.session.recorder.as_ref().unwrap().duration_s(), 90.0);
        assert_eq!(app.session.audio_of(120.0 + SETTLE_S), Some(70.0));
        assert_eq!(app.session.audio_of(150.0), Some(90.0));
        assert!((app.unsaved_s() - 60.0).abs() < 0.1, "{}", app.unsaved_s());
        app.save_into(&base.join("saved"));
        assert_eq!(app.unsaved_s(), 0.0);
        let saved = app
            .session
            .saved_to()
            .map(Path::to_path_buf)
            .expect("saved");
        let text = std::fs::read_to_string(saved.join(MANIFEST)).unwrap();
        let m: Manifest = toml::from_str(&text).unwrap();
        let got: Vec<_> = m
            .recordings
            .iter()
            .map(|e| (e.position.clone().unwrap(), e.start_s, e.end_s))
            .collect();
        assert_eq!(
            got,
            [
                ("DU".to_string(), Some(0.0), Some(60.0)),
                ("DD".to_string(), Some(60.0 + SETTLE_S), None)
            ],
            "{text}"
        );
        // The pause is a break, its sound in a file of its own.
        let session: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(saved.join("session.json")).unwrap())
                .unwrap();
        assert_eq!(session["breaks"].as_array().map(Vec::len), Some(1));
        assert_eq!(session["breaks"][0]["segment"], "audio-002.wav");
        assert_eq!(session["breaks"][0]["audio_s"], 60.0);
        assert_eq!(session["breaks"][0]["gap_s"], 60.0);
        assert_eq!(session["duration_s"], 90.0);
        // Reopened, all 90 s are there in both positions.
        let mut again = TimegrapherApp::with_devices(Vec::new(), None, false);
        again.start_batch(&saved);
        let t = Instant::now();
        while again.batch.is_some() && t.elapsed().as_secs() < 120 {
            again.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let live = again.live.as_ref().expect("analysed");
        assert!(
            (live.duration_s() - 90.0).abs() < 0.5,
            "{}",
            live.duration_s()
        );
        assert_eq!(
            again.session.stretches(),
            [(0.0, Position::DialUp), (60.0, Position::DialDown)]
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A Positions Test begun with the watch already dial up: the strip
    /// marks Dial Up from the start, with the time before the test named,
    /// though the watch never moved.
    #[test]
    fn a_positions_test_is_marked_from_its_first_position() {
        let mut app = synthetic(20.0, 0.0);
        assert!(app.position_bands().is_empty(), "one position, no test");
        let mut test = guided::Test::new();
        test.tick(5.0);
        app.guided_unused = test.unused(10.0);
        app.guided_measured = test.measured(10.0);
        let bands = app.position_bands();
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].2, "Dial Up (DU)");
        let settles = app.strip_settles();
        assert_eq!(settles[0], (0.0, 5.0, guided::BEFORE_TEST));
        assert_eq!(settles[1], (5.0, 10.0, guided::SETTLING));
    }

    /// A new position keeps the strip's beats and marks the stretch; the
    /// readings and the distributions take only the position the watch is
    /// in, and going back to an earlier position joins its stretches for
    /// the steadiness tests.
    #[test]
    fn a_new_position_marks_a_stretch_and_keeps_the_strip() {
        let mut app = synthetic(20.0, 0.0);
        app.session.listen();
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
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
        assert_eq!(app.session.stretches().len(), 3);
        let bands = app.position_bands();
        assert_eq!(
            bands.iter().map(|b| b.2).collect::<Vec<_>>(),
            ["Dial Up (DU)", "Dial Down (DD)", "Dial Up (DU)"]
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

    /// Guided elapsed-time endpoints use the recorder timeline even when
    /// the watch never changes position and a pause spans an endpoint.
    #[test]
    fn positions_test_manifest_maps_paused_elapsed_time_to_recorded_sound() {
        let base = std::env::temp_dir().join(format!("tg-app-guided-pause-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.session.listen();
        let info = app.info("test", 100, 16);
        app.session.recorder = Some(Recorder::start(&base, info).unwrap());
        app.session
            .recorder
            .as_mut()
            .unwrap()
            .write(&timegrapher_core::capture::Block {
                samples: vec![0.0; 60 * 100],
                at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(60),
            })
            .unwrap();
        app.session.pause(60.0).unwrap().unwrap();
        app.session.resume("test").unwrap();
        app.session.resumed_at(120.0);
        app.session
            .recorder
            .as_mut()
            .unwrap()
            .write(&timegrapher_core::capture::Block {
                samples: vec![0.0; 10 * 100],
                at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(130),
            })
            .unwrap();
        for (measure, end) in [(60.0, 60.0), (110.0, 70.0)] {
            let mut test = guided::Test::new().timed(20.0, measure);
            test.tick(0.0);
            test.tick(130.0);
            app.guided = Some(test);
            let m = app.take_manifest();
            assert_eq!(m.recordings.len(), 1);
            assert_eq!(m.recordings[0].position.as_deref(), Some("DU"));
            assert_eq!(m.recordings[0].start_s, Some(20.0));
            assert_eq!(m.recordings[0].end_s, Some(end));
        }
        app.session.discard_recording();
    }

    /// A take saved during a Positions Test lists the test's own measured
    /// stretches, so `timegrapher session` reads the seconds the table did.
    #[test]
    fn a_positions_test_take_lists_its_measured_stretches() {
        let base = std::env::temp_dir().join(format!("tg-app-guided-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = synthetic(5.0, 0.0);
        app.session.listen();
        let info = app.info("test", 48000, 16);
        app.session.recorder = Some(Recorder::start(&base.join("kept"), info).unwrap());
        let audio = generate(
            &SynthConfig {
                duration_s: 80.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let r = app.session.recorder.as_mut().unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: audio.samples,
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        let mut test = guided::Test::new();
        test.tick(0.0);
        test.tick(guided::SETTLE_S + guided::MEASURE_S);
        let next = test.advance(80.0).expect("a second position");
        app.session.move_to(80.0, Position::DialUp, next).unwrap();
        test.tick(80.0);
        test.tick(80.0 + guided::SETTLE_S + guided::MEASURE_S);
        app.session
            .recorder
            .as_mut()
            .unwrap()
            .write(&timegrapher_core::capture::Block {
                samples: vec![0.0; 80 * 48000],
                at: std::time::SystemTime::now() + std::time::Duration::from_secs(80),
            })
            .unwrap();
        app.guided = Some(test);
        let m = app.take_manifest();
        let got: Vec<_> = m
            .recordings
            .iter()
            .map(|e| (e.position.clone().unwrap(), e.start_s, e.end_s))
            .collect();
        assert_eq!(
            got,
            [
                ("DU".to_string(), Some(20.0), Some(80.0)),
                ("DD".to_string(), Some(100.0), Some(160.0)),
            ]
        );
        assert!(m.recordings.iter().all(|e| e.wind_h.is_none()), "full wind");
        // Finished, the session's folder keeps its results for the 24
        // hour half.
        app.guided.as_mut().unwrap().readings = Some(Vec::new());
        app.save_into(&base.join("saved"));
        let dir = app.session.saved_to().unwrap().to_path_buf();
        let kept: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(guided::RESULTS_FILE)).unwrap())
                .unwrap();
        assert_eq!(kept["format"], guided::RESULTS_FORMAT);
        // The 24 hour half's stretches carry the hours since full wind.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let full = guided::FullWind {
            format: guided::RESULTS_FORMAT.into(),
            began_utc: String::new(),
            began_unix: now - 24.0 * 3600.0,
            watch: String::new(),
            calibre: String::new(),
            bph: None,
            lift_angle_deg: None,
            settle_s: None,
            measure_s: None,
            positions: Vec::new(),
        };
        let mut half = guided::Test::after(full);
        half.tick(0.0);
        half.tick(guided::SETTLE_S + guided::MEASURE_S);
        app.guided = Some(half);
        let m = app.take_manifest();
        assert_eq!(m.recordings.len(), 1);
        assert_eq!(m.recordings[0].wind_h, Some(24.0));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A saved take carries a session file with the watch, its calibre,
    /// lift angle, beat rate and positions, and opening it takes them back.
    #[test]
    fn a_saved_take_brings_back_its_calibre_and_positions() {
        let base = std::env::temp_dir().join(format!("tg-app-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = synthetic(5.0, 0.0);
        app.session.listen();
        app.watch = "Test Watch".into();
        app.cal.user.put_custom(calibre_ui::custom(
            "Bench Calibre",
            [("escape wheel", 5.0), ("fourth wheel", 60.0)],
        ));
        app.calibre = "Bench Calibre".into();
        app.set_lift(49.0);
        app.bph = Some(28800);
        let audio = generate(
            &SynthConfig {
                duration_s: 90.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let info = app.info("test", 48000, 16);
        app.session.recorder = Some(Recorder::start(&base.join("kept"), info).unwrap());
        let third = audio.samples.len() / 3;
        let moves = [
            (0, None),
            (1, Some((Position::DialUp, Position::DialDown))),
            (2, Some((Position::DialDown, Position::DialUp))),
        ];
        for (i, pos) in moves {
            if let Some((old, new)) = pos {
                let r = app.session.recorder.as_mut().unwrap();
                r.note(&format!("position {}", new.code())).unwrap();
                app.session.move_to(i as f64 * 30.0, old, new).unwrap();
            }
            let r = app.session.recorder.as_mut().unwrap();
            r.write(&timegrapher_core::capture::Block {
                samples: audio.samples[i * third..(i + 1) * third].to_vec(),
                at: std::time::SystemTime::now(),
            })
            .unwrap();
        }
        app.save_into(&base.join("saved"));
        let saved = app
            .session
            .saved_to()
            .map(Path::to_path_buf)
            .expect("saved");
        let text = std::fs::read_to_string(saved.join(MANIFEST)).unwrap();
        let m: Manifest = toml::from_str(&text).unwrap();
        assert_eq!(m.calibre.as_deref(), Some("Bench Calibre"));
        assert_eq!(m.wheels.len(), 2, "{text}");
        let starts: Vec<_> = m.recordings.iter().map(|e| e.start_s).collect();
        // Each later position starts once it has settled.
        assert_eq!(starts, [Some(0.0), Some(40.0), Some(70.0)], "{text}");
        let ends: Vec<_> = m.recordings.iter().map(|e| e.end_s).collect();
        assert_eq!(ends, [Some(30.0), Some(60.0), None], "{text}");
        assert_eq!(m.recordings[2].end_s, None);

        // Opened on a fresh app, the take brings it all back.
        let mut again = TimegrapherApp::with_devices(Vec::new(), None, false);
        again.start_batch(&saved);
        let t = Instant::now();
        while again.batch.is_some() && t.elapsed().as_secs() < 120 {
            again.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(again.live.is_some(), "analysed");
        assert_eq!(again.watch, "Test Watch");
        assert_eq!(again.calibre, "Bench Calibre");
        // Not known there: added to its own calibres from the wheels.
        assert!(again
            .cal
            .find("Bench Calibre")
            .is_some_and(|e| e.is_custom()));
        assert_eq!(again.calibre_wheels().map(|w| w.len()), Some(2));
        assert_eq!((again.lift_deg, again.bph), (49.0, Some(28800)));
        assert_eq!(
            again.session.stretches(),
            [
                (0.0, Position::DialUp),
                (30.0, Position::DialDown),
                (60.0, Position::DialUp)
            ]
        );
        assert_eq!(again.session.settles(), [(30.0, 40.0), (60.0, 70.0)]);
        assert_eq!(
            again.live.as_ref().unwrap().breaks(),
            &[30.0, 40.0, 60.0, 70.0]
        );
        assert_eq!(again.position, Position::DialUp);
        // A recording with no session file opens as before.
        assert!(read_manifest(&base).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn applicable_invalid_manifest_stops_actual_open_entrypoints_before_audio() {
        let root =
            std::env::temp_dir().join(format!("tg-invalid-manifest-open-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("watch.wav");
        // Deliberately not an audio file: manifest error must win over header error.
        std::fs::write(&path, []).unwrap();
        for (fields, field) in [
            ("bph=0", "bph"),
            ("lift=nan", "lift"),
            ("lift_angle=0", "lift"),
            ("escape_teeth=0", "escape_teeth"),
            ("card_ppm=-1000000", "card_ppm"),
        ] {
            std::fs::write(
                root.join(MANIFEST),
                format!("{fields}\n[[recording]]\nfile=\"watch.wav\"\nposition=\"DU\"\n"),
            )
            .unwrap();
            for replay in [true, false] {
                let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
                if replay {
                    app.start_replay(&path);
                } else {
                    app.start_batch(&path);
                }
                assert!(app.capture.is_none() && app.batch.is_none());
                let (message, error) = app.message.as_ref().unwrap();
                assert!(*error);
                assert!(
                    message.contains(MANIFEST) && message.contains(&format!("{field}:")),
                    "{replay}: {message}"
                );
            }
        }
        // Applicability cannot be established from a syntactically corrupt file.
        std::fs::write(root.join(MANIFEST), "not valid TOML [").unwrap();
        for replay in [true, false] {
            let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
            if replay {
                app.start_replay(&path);
            } else {
                app.start_batch(&path);
            }
            assert!(app.capture.is_none() && app.batch.is_none());
            assert!(app
                .message
                .as_ref()
                .is_some_and(|(m, e)| *e && m.contains(MANIFEST)));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn valid_absent_and_unrelated_manifests_keep_actual_open_paths() {
        use std::time::Duration;
        use timegrapher_core::audio;
        let root =
            std::env::temp_dir().join(format!("tg-valid-manifest-open-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("watch.wav");
        // Empty valid WAV: no acoustic signal or device; enough to admit each path.
        audio::write_wav(
            &path,
            &audio::Audio {
                samples: Vec::new(),
                sample_rate: 48000,
            },
        )
        .unwrap();
        let valid = "lift_angle=52\nlift_known=false\ncard_ppm=0\n[[recording]]\nfile=\"watch.wav\"\nposition=\"DU\"\n";
        let unrelated = "bph=0\nlift=nan\n[[recording]]\nfile=\"other.wav\"\nposition=\"DU\"\n";
        for text in [None, Some(valid), Some(unrelated)] {
            let manifest = root.join(MANIFEST);
            match text {
                Some(text) => std::fs::write(&manifest, text).unwrap(),
                None => {
                    let _ = std::fs::remove_file(&manifest);
                }
            }
            if text == Some(unrelated) {
                assert!(read_manifest(&path).unwrap().is_none());
            }
            // Existing exact-path applicability does not attach child-file
            // metadata to the folder view itself.
            assert!(read_manifest(&root).unwrap().is_none());
            for replay in [true, false] {
                let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
                if replay {
                    app.start_replay(&path);
                    let capture = app.capture.as_ref().expect("replay admitted");
                    assert!(matches!(
                        capture.rx.recv_timeout(Duration::from_secs(5)),
                        Ok(Event::End)
                    ));
                } else {
                    app.start_batch(&path);
                    let batch = app.batch.as_ref().expect("analysis admitted");
                    // Completion (even a no-beats result) establishes that the
                    // source reader closed before deleting the fixture.
                    let _result = batch
                        .rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("empty batch completes");
                }
                assert!(!app.message.as_ref().is_some_and(|(_, error)| *error));
                assert_eq!(app.lift_deg, 52.0);
                assert!(!app.lift_known());
                app.stop();
            }
        }
        std::fs::remove_dir_all(root).unwrap();
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

    fn replay_context_fixture(name: &str, single: bool) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("tg-replay-context-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let audio = timegrapher_core::audio::Audio {
            sample_rate: 100,
            samples: vec![0.0; if single { 9000 } else { 4500 }],
        };
        let first = dir.join("audio-001.wav");
        timegrapher_core::audio::write_wav(&first, &audio).unwrap();
        if !single {
            timegrapher_core::audio::write_wav(&dir.join("audio-002.wav"), &audio).unwrap();
        }
        let file = if single { "audio-001.wav" } else { "." };
        std::fs::write(
            dir.join(MANIFEST),
            format!(
                "watch = 'Replay Watch'\nbph = 28800\nlift = 49.0\n\n\
             [[recording]]\nfile = '{file}'\nposition = 'DU'\nstart_s = 0.0\nend_s = 30.0\n\n\
             [[recording]]\nfile = '{file}'\nposition = 'DD'\nstart_s = 40.0\nend_s = 60.0\n\n\
             [[recording]]\nfile = '{file}'\nposition = 'DU'\nstart_s = 70.0\n"
            ),
        )
        .unwrap();
        let path = if single { first } else { dir.clone() };
        (dir, path)
    }

    // Drain the real replay source without analyzing it. End follows file close,
    // so later directory cleanup cannot race the reader on Windows.
    fn drain_replay_source(app: &TimegrapherApp) -> usize {
        let c = app.capture.as_ref().unwrap();
        let mut samples = 0;
        let started = Instant::now();
        loop {
            assert!(
                started.elapsed().as_secs() < 10,
                "replay source did not end"
            );
            match c
                .rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
            {
                Event::Audio(b) => samples += b.samples.len(),
                Event::End => return samples,
                Event::Error(e) => panic!("{e}"),
            }
        }
    }

    #[test]
    fn whole_recording_viewer_keeps_known_jumps_with_or_without_position_cuts() {
        for with_positions in [false, true] {
            let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
            app.average_s = 60.0;
            let log = BeatLog {
                sample_rate: 48000,
                duration_s: 120.0,
                bph: 28800,
                lift_deg: 52.0,
                beats: (0..960)
                    .map(|index| timegrapher_core::beats::Beat {
                        index,
                        time: index as f64 / 8.0 + if index >= 480 { 0.03 } else { 0.0 },
                        quality: if index == 500 { 0.1 } else { 1.0 },
                    })
                    .collect(),
                amplitude_windows: vec![],
                clipped_beats: vec![],
                clipped_samples: 0,
                jumps: vec![60.03],
            };
            let original: Vec<_> = log
                .beats
                .iter()
                .map(|b| (b.index, b.time, b.quality))
                .collect();
            let (positions, settles) = if with_positions {
                (
                    vec![
                        (0.0, Position::DialUp),
                        (30.0, Position::DialDown),
                        (90.0, Position::DialUp),
                    ],
                    vec![(30.0, 40.0), (90.0, 100.0)],
                )
            } else {
                (vec![], vec![])
            };
            app.show_log(log, "known phase step".into(), positions, settles);
            let live = app.live.as_ref().unwrap();
            let expected = if with_positions {
                vec![30.0, 40.0, 60.03, 90.0, 100.0]
            } else {
                vec![60.03]
            };
            assert_eq!(
                live.breaks(),
                expected,
                "position cuts cannot erase an intrinsic jump"
            );
            assert_eq!(
                live.beats()
                    .iter()
                    .map(|b| (b.index, b.time, b.quality))
                    .collect::<Vec<_>>(),
                original
            );
            app.view_end = Some(75.0);
            let reading = app.reading().unwrap();
            assert!(reading.watch_heard);
            assert!(reading.span_s <= 75.0 - 60.03 + 1e-9, "{reading:?}");
            assert!(reading.rate_s_per_day.unwrap().abs() < 0.001, "{reading:?}");
            app.view_end = Some(20.0);
            assert!(app.reading().unwrap().rate_s_per_day.unwrap().abs() < 0.001);
        }
    }

    #[test]
    fn replay_restores_saved_history_and_clear_and_selection_preserve_it() {
        let (dir, path) = replay_context_fixture("folder", false);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.replay_speed = 100.0;
        app.start_replay(&path);
        assert_eq!(
            drain_replay_source(&app),
            9000,
            "both segments, in recorded-audio time"
        );
        let positions = [
            (0.0, Position::DialUp),
            (30.0, Position::DialDown),
            (60.0, Position::DialUp),
        ];
        let settles = [(30.0, 40.0), (60.0, 70.0)];
        let breaks = [30.0, 40.0, 60.0, 70.0];
        assert_eq!(app.session.stretches(), positions);
        assert_eq!(app.session.settles(), settles);
        assert_eq!(app.live.as_ref().unwrap().breaks(), breaks);
        assert_eq!(
            (app.watch.as_str(), app.lift_deg, app.bph),
            ("Replay Watch", 49.0, Some(28800))
        );
        app.change_position(Position::DialDown);
        assert_eq!(app.position, Position::DialDown);
        assert_eq!(app.session.stretches(), positions);
        assert_eq!(app.session.settles(), settles);
        assert_eq!(app.live.as_ref().unwrap().breaks(), breaks);
        app.clear_readings();
        assert_eq!(app.live.as_ref().unwrap().breaks(), breaks);
        app.stop();
        assert!(app.session.has_replay_context());
        // A segment alone does not match the folder's '.' entries.
        app.start_replay(&dir.join("audio-001.wav"));
        assert_eq!(drain_replay_source(&app), 4500);
        assert!(!app.session.has_replay_context());
        assert!(app.session.stretches().is_empty());
        assert!(app.session.settles().is_empty());
        assert!(app.live.as_ref().unwrap().breaks().is_empty());
        app.change_position(Position::DialDown);
        assert!(
            !app.session.stretches().is_empty(),
            "plain replay keeps manual moves"
        );
        app.start_replay(&path);
        assert_eq!(drain_replay_source(&app), 9000);
        assert_eq!(app.session.stretches(), positions);
        assert_eq!(app.live.as_ref().unwrap().breaks(), breaks);
        app.new_session();
        assert!(!app.session.has_replay_context());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replay_uses_the_same_matching_file_manifest_as_whole_take_analysis() {
        let (dir, path) = replay_context_fixture("file", true);
        let mut batch = TimegrapherApp::with_devices(Vec::new(), None, false);
        let (manifest, entries) = read_manifest(&path).unwrap().unwrap();
        let (positions, settles) = batch.apply_manifest(&manifest, &entries);
        let mut replay = TimegrapherApp::with_devices(Vec::new(), None, false);
        replay.replay_speed = 100.0;
        replay.start_replay(&path);
        assert_eq!(drain_replay_source(&replay), 9000);
        assert_eq!(replay.session.stretches(), positions);
        assert_eq!(replay.session.settles(), settles);
        assert_eq!(replay.watch, batch.watch);
        assert_eq!((replay.lift_deg, replay.bph), (batch.lift_deg, batch.bph));
        replay.stop();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replay_poll_does_not_publish_readings_inside_saved_settling() {
        let base =
            std::env::temp_dir().join(format!("tg-replay-settle-poll-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let mut app = synthetic(12.0, 0.0);
        app.session.replay();
        app.session.load(
            &[(0.0, Position::DialUp), (8.0, Position::DialDown)],
            &[(8.0, 14.0)],
        );
        app.live
            .as_mut()
            .unwrap()
            .set_breaks(app.session.replay_breaks().unwrap());
        app.clear_trends();
        app.other_trends.insert(trend_key(5.0), Vec::new());
        let tx = queued_capture(&mut app, &base);
        app.capture.as_mut().unwrap().is_file = true;
        tx.send(Event::Audio(timegrapher_core::capture::Block {
            samples: vec![0.0; 24000],
            at: std::time::SystemTime::now(),
        }))
        .unwrap();
        app.poll();
        let at = app.live.as_ref().unwrap().reading(app.average_s).time_s;
        assert!(
            at > 8.0 && at <= 14.0,
            "fixture must read within the saved settle: {at}"
        );
        assert!(app.trend.is_empty());
        assert!(app.other_trends.values().all(Vec::is_empty));
        assert!(app.reading().is_none());
        assert_eq!(app.live.as_ref().unwrap().breaks(), &[8.0, 14.0]);
        app.stop();
        drop(tx);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn replay_readouts_and_rebuilt_trends_respect_imported_settling() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        let log = BeatLog {
            sample_rate: 48000,
            duration_s: 90.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: (0..720)
                .map(|i| timegrapher_core::beats::Beat {
                    index: i,
                    time: i as f64 / 8.0,
                    quality: 1.0,
                })
                .collect(),
            amplitude_windows: vec![],
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![],
        };
        app.live = Some(LiveAnalyzer::from_log(log, app.live_config()));
        app.session.replay();
        app.session.load(
            &[(0.0, Position::DialUp), (30.0, Position::DialDown)],
            &[(30.0, 40.0)],
        );
        app.live
            .as_mut()
            .unwrap()
            .set_breaks(app.session.replay_breaks().unwrap());
        app.pane_average.insert(Pane::Rate, 5.0);
        app.rebuild_trend();
        assert!(!app.trend.is_empty());
        assert!(!app.other_trends[&trend_key(5.0)].is_empty());
        assert!(app.trend.iter().all(|p| p.t <= 30.0 || p.t > 40.0));
        assert!(app
            .other_trends
            .values()
            .flatten()
            .all(|p| p.t <= 30.0 || p.t > 40.0));
        app.view_end = Some(35.0);
        assert!(app.reading().is_none());
        app.view_end = Some(41.0);
        let after = app.reading().unwrap();
        assert!(
            after.span_s <= 1.0,
            "no fit crosses the settling end: {}",
            after.span_s
        );
        // Endpoint convention is inherited unchanged: settling_at is (from,to].
        app.view_end = Some(40.0);
        assert!(app.reading().is_none());
    }

    #[test]
    fn a_replay_can_be_analysed_whole_keeping_its_settings() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        let dir = std::env::temp_dir().join("tg-app-replay-whole");
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
        assert_eq!(app.session.state(), session::State::Replaying);
        app.lift_deg = 49.0;
        app.analyse_replayed();
        assert!(app.batch.is_some(), "analysing the whole recording");
        assert_eq!(app.lift_deg, 49.0, "the replay's lift angle kept");
        assert_eq!(app.opened_path, path.display().to_string());
    }

    #[test]
    fn a_calibre_names_the_wheels_and_a_typed_one_is_kept() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        assert!(app.calibre_wheels().is_none());
        // From the table: its train, and its lift angle where it has one.
        let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
        app.set_calibre(c.calibre.clone());
        let wheels = app.calibre_wheels().expect("wheels");
        assert_eq!(wheels.len(), c.wheels.len());
        if let Some(l) = c.lift_angle_deg {
            assert_eq!(app.lift_deg, l);
        }
        // Typed in: tidied for the tests; the recent picks are kept with
        // the settings, the calibre itself in the user calibre file.
        app.cal.user.put_custom(calibre_ui::custom(
            "Bench Watch",
            [
                ("escape wheel", 5.0),
                ("fourth wheel", 60.0),
                ("centre wheel", 3600.0),
                (" Third Wheel ", 450.0),
                ("", 10.0),
            ],
        ));
        app.set_calibre("Bench Watch".into());
        let wheels = app.calibre_wheels().expect("wheels");
        assert_eq!(wheels.len(), 4, "the unnamed wheel is left out");
        assert!(wheels.iter().any(|w| w.name == "third wheel"));
        let mut next = TimegrapherApp::with_devices(Vec::new(), None, false);
        next.apply_settings(app.settings());
        assert_eq!(
            next.cal.recent,
            ["Bench Watch".to_string(), c.calibre.clone()]
        );
        assert!(app.settings().custom_calibres.is_empty());
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
        let old: Settings = serde_json::from_str(r#"{"hist_cumulative": true}"#).unwrap();
        assert!(old.hist_cumulative && old.rate_line);
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
        let id = tree.tiles.find_pane(&Pane::SteadyReadings).unwrap();
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
        let steady = again.panes.tiles.find_pane(&Pane::SteadyReadings).unwrap();
        assert_eq!(
            again.panes.tiles.parent_of(sound),
            again.panes.tiles.parent_of(steady)
        );
    }

    /// A layout saved before the Watch pane gains it in a column of its
    /// own at the left, keeping the rest as it was.
    #[test]
    fn an_old_layout_gains_the_watch_pane_at_the_left() {
        let app = synthetic(40.0, 0.0);
        let mut tree = app.panes.clone();
        let watch = tree.tiles.find_pane(&Pane::Watch).unwrap();
        let tabs = tree.tiles.parent_of(watch).unwrap();
        let root = tree.root().unwrap();
        let rest = tree
            .tiles
            .get_container(root)
            .unwrap()
            .children()
            .copied()
            .find(|&c| c != tabs)
            .unwrap();
        // What an older app saved: everything but the Watch column.
        tree.tiles.remove(watch);
        tree.tiles.remove(tabs);
        tree.tiles.remove(root);
        tree.root = Some(rest);
        assert!(!layout_is_whole(&tree));
        let mut before = app.settings();
        before.panes = Some(tree);
        let mut again = TimegrapherApp::with_devices(Vec::new(), None, false);
        again.apply_settings(before);
        assert!(layout_is_whole(&again.panes));
        let root = again.panes.root().unwrap();
        let Some(egui_tiles::Container::Linear(l)) = again.panes.tiles.get_container(root) else {
            panic!("the root is a row");
        };
        assert_eq!(l.dir, LinearDir::Horizontal);
        assert_eq!(
            l.children[1], rest,
            "the old layout sits to the right, as it was"
        );
        let watch = again.panes.tiles.find_pane(&Pane::Watch).unwrap();
        assert_eq!(again.panes.tiles.parent_of(watch), Some(l.children[0]));
    }

    #[test]
    fn moving_steadiness_out_leaves_the_profile_showing() {
        let app = synthetic(40.0, 0.0);
        let mut tree = app.panes.clone();
        let steady = tree.tiles.find_pane(&Pane::SteadyReadings).unwrap();
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

    /// A burst of glitches doesn't set a chart's scale.
    #[test]
    fn a_glitch_burst_does_not_set_the_scale() {
        let mut pts: Vec<[f64; 2]> = (0..60).map(|i| [i as f64, 5.0 + (i % 5) as f64]).collect();
        pts.extend((60..66).map(|i| [i as f64, 800.0]));
        let (lo, hi) = percentile_range(&pts).unwrap();
        assert!(lo < 5.0 && hi > 9.0 && hi < 40.0, "{lo} {hi}");
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
    fn tick_and_tock_amplitude_dash_over_their_own_colours() {
        let pal = theme::palette(true);
        let key = chart_key(Pane::Amplitude, pal);
        assert_eq!(key[0], ("Average Amplitude", pal.trace_amplitude, None));
        assert_eq!(key[1], ("From Tick", pal.trace_amplitude, Some(pal.tick)));
        assert_eq!(key[2], ("From Tock", pal.trace_amplitude, Some(pal.tock)));
    }

    #[test]
    fn a_chart_labels_no_time_before_the_start() {
        let r = -30.0..=600.0;
        assert_eq!(chart_time_label(-30.0, &r), "");
        assert_eq!(chart_time_label(0.0, &r), "0:00");
        assert_eq!(chart_time_label(120.0, &r), "2:00");
    }

    #[test]
    fn the_input_clock_is_offered_after_twenty_minutes() {
        // A card 20 ppm slow: 48,000 frames arrive every 1.00002 s of
        // the computer's time, in blocks of a tenth of a second.
        let mut t = ClockTrack {
            device: "Test Mic".into(),
            tracker: timegrapher_core::clock::ClockTracker::new(48_000),
            next_s: CLOCK_MEASURE_S,
            fit: None,
        };
        use std::time::Duration;
        let start = std::time::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mut audio = 0.0;
        while audio < CLOCK_MEASURE_S - 1.0 {
            audio += 0.1;
            t.push(4_800, start + Duration::from_secs_f64(audio * 1.000_02));
        }
        assert!(t.fit.is_none(), "nothing offered before 20 minutes");
        while audio < CLOCK_MEASURE_S + 1.0 {
            audio += 0.1;
            t.push(4_800, start + Duration::from_secs_f64(audio * 1.000_02));
        }
        let f = t.fit.as_ref().expect("timed at 20 minutes");
        assert!((f.ppm - 20.0).abs() < 0.5, "{} ppm", f.ppm);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None, false);
        app.clock_track = Some(t);
        assert!(app.clock_measured().is_some());
    }

    /// A pane follows Readings' Average Over until given a time of its
    /// own, which keeps its own readings until Readings' Average Over is
    /// changed: then every pane follows it again.
    #[test]
    fn a_pane_follows_readings_unless_set() {
        let mut app = synthetic(40.0, 0.0);
        assert_eq!(app.average_for(Pane::Rate), 10.0);
        app.set_pane_average(Pane::Rate, Some(30.0));
        assert_eq!(app.average_for(Pane::Rate), 30.0);
        assert_eq!(app.average_for(Pane::Amplitude), 10.0);
        let own = app.trend_for(30.0);
        assert!(!own.is_empty());
        assert!(own.last().unwrap().t > 30.0);
        // Back to following: its own readings are dropped.
        app.set_pane_average(Pane::Rate, None);
        assert_eq!(app.average_for(Pane::Rate), 10.0);
        assert!(app.other_trends.is_empty());
        // A new time for Readings takes over every pane's own.
        app.set_pane_average(Pane::Rate, Some(30.0));
        app.set_pane_average(Pane::Strip, Some(60.0));
        app.set_average(5.0);
        assert_eq!(app.average_for(Pane::Rate), 5.0);
        assert_eq!(app.average_for(Pane::Strip), 5.0);
        assert_eq!(app.average_for(Pane::Amplitude), 5.0);
        assert!(app.pane_average.is_empty() && app.other_trends.is_empty());
        // Picking Readings' own time is the same as following it.
        app.set_pane_average(Pane::Rate, Some(5.0));
        assert!(app.pane_average.is_empty());
    }

    /// The per-pane times and each Steadiness pane's series come back next
    /// time, with every series off read as all on, and the one choice an
    /// older app kept for its single Steadiness pane given to every view.
    #[test]
    fn pane_times_and_series_are_kept() {
        use steady::View;
        let mut app = synthetic(5.0, 0.0);
        app.set_pane_average(Pane::Strip, Some(60.0));
        app.steady_series.insert(View::Fft, [false, true, false]);
        let s = app.settings();
        let mut back = TimegrapherApp::with_devices(Vec::new(), None, false);
        back.apply_settings(s.clone());
        assert_eq!(back.average_for(Pane::Strip), 60.0);
        assert_eq!(back.steady_series[&View::Fft], [false, true, false]);
        assert_eq!(back.steady_series[&View::Cycle], [true; 3]);
        let mut none = s;
        none.steady_shown = vec![(View::Fft, [false; 3])];
        none.pane_average = vec![(Pane::Watch, 30.0), (Pane::Rate, 7.0)];
        back.apply_settings(none);
        assert_eq!(back.steady_series[&View::Fft], [true; 3]);
        assert!(back.pane_average.is_empty());
        let mut older = app.settings();
        older.steady_shown.clear();
        older.steady_series = [true, false, false];
        back.apply_settings(older);
        for p in Pane::STEADY {
            assert_eq!(
                back.steady_series[&p.steady_view().unwrap()],
                [true, false, false]
            );
        }
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
