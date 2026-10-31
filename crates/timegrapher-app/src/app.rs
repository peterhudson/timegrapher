//! The main window: input and settings on the left, the readings across
//! the top, and below them panes (the paper strip and the charts over time)
//! that can be dragged by their tabs into any arrangement.

use crate::applog;
use crate::calibre_ui;
use crate::export;
use crate::fields::{self, Format};
use crate::guided;
use crate::help;
use crate::isochronism;
use crate::photos;
use crate::profiles;
use crate::report::{self, ReportDialog};
use crate::reserve;
use crate::scope;
use crate::settings::{Settings, RATE_ONLY};
use crate::settings_window::{self, Tab};
use crate::steady::{self, Steadiness};
use crate::strip::{self, Anchor, StripInput, StripView};
use crate::theme::{self, DialogButton, Icon, Role};
use crate::watches;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{GridMark, Line, LineStyle, Plot, Points};
use egui_tiles::{Linear, LinearDir, SimplificationOptions, TileId, Tiles, Tree, UiResponse};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};
use timegrapher_core::beats::STANDARD_BPH;
use timegrapher_core::breaks::{GapWatch, Live};
use timegrapher_core::capture::{self, Capture, Event, InputDevice, Level};
use timegrapher_core::clockstore;
use timegrapher_core::library;
use timegrapher_core::library::{length_text, size_text};
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading, MAX_PROFILE_S};
use timegrapher_core::measure::Caveat;
use timegrapher_core::mixer::{self, GainState, InputGain};
use timegrapher_core::periodicity::Wheel;
use timegrapher_core::power_reserve;
use timegrapher_core::profile::TickProfile;
use timegrapher_core::steadiness::SeriesKind;

mod charts;
mod dialogs;
mod distributions;
mod dock;
mod library_folder;
mod listen_card;
mod masks;
mod menus;
mod pane_export;
mod paper_strip;
mod positions_test;
mod power_reserve_pane;
mod readouts;
mod scope_panes;
mod settings_pane;
mod steadiness_panes;
mod take_io;
mod watches_pane;

use dock::PaneBehavior;
use pane_export::PaneExport;

/// The A and B sides' sounds.
type Profiles = [Option<TickProfile>; 2];

/// How often the sound is kept, for the profile once stopped, seconds.
const SOUND_EVERY_S: f64 = 2.0;
/// What the reading cards asked for.
struct Readouts {
    /// Where each card's controls go.
    slots: Vec<(Slot, egui::Rect)>,
    /// Current Position and Listen in their compact form, the window too
    /// narrow for their full width.
    position_compact: bool,
    listen_compact: bool,
    /// The Calibre card's limits, for its chip beside the Watch line.
    calibre_limits: Vec<(String, String)>,
}

/// The controls the readings' cards carry, each in a strip at its card's
/// foot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
    /// The six positions.
    Position,
    /// Listen or Stop, Save, and Average Over.
    Listen,
    /// The Watch line: the Calibre card's title, the watch on the
    /// microphone, with its menu.
    WatchLine,
    /// The calibre, on the Calibre card's figure row.
    Calibre,
    /// The beat rate and the lift angle, under the calibre.
    RateAndLift,
}

/// Seconds of listening before the input's clock, timed against the
/// computer's, is offered to keep: the twenty minutes `timegrapher clock
/// measure` takes by default. It is timed again each minute after.
const CLOCK_MEASURE_S: f64 = timegrapher_core::clock::MEASURE_S;

/// How far the clock timed while listening may be from the one kept for
/// the input's name before the Rate card says so and offers the new one.
/// It decides only when that fact is shown.
const CLOCK_DIFFERS_PPM: f64 = 2.0;

/// The microphone's clock, timed against the computer's while listening.
struct ClockTrack {
    /// The input's name, as the clock file keeps it.
    device: String,
    /// Its name as the microphone menu shows it.
    shown: String,
    tracker: timegrapher_core::clock::ClockTracker,
    /// Audio seconds at which to fit again.
    next_s: f64,
    /// The latest fit, from [`CLOCK_MEASURE_S`] of audio on.
    fit: Option<timegrapher_core::clock::ClockFit>,
}

impl ClockTrack {
    /// Start timing afresh, after a gap the clock can't be timed across.
    fn restart(&mut self, sample_rate: u32) {
        self.tracker = timegrapher_core::clock::ClockTracker::new(sample_rate);
        self.next_s = CLOCK_MEASURE_S;
        self.fit = None;
    }

    /// Count a block of audio that arrived at `at`, fitting once a minute.
    fn push(&mut self, samples: usize, at: std::time::SystemTime) {
        self.tracker.push(samples, at);
        if self.tracker.audio_s() >= self.next_s {
            self.next_s = self.tracker.audio_s() + 60.0;
            self.fit = self.tracker.fit();
        }
    }
}

/// Settings…: Cmd+, on macOS, Ctrl+, elsewhere.
fn settings_shortcut() -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Comma)
}

/// Each view's key: Cmd+1 for the Bench, Cmd+2 for Watches (Ctrl off a
/// Mac).
fn view_shortcut(view: View) -> egui::KeyboardShortcut {
    let key = match view {
        View::Bench => egui::Key::Num1,
        View::Watches => egui::Key::Num2,
    };
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key)
}

/// The two views, as the mode switch and the View menu name them.
const VIEWS: [(View, &str, &str); 2] = [
    (
        View::Bench,
        "Bench",
        "The take on the microphone or opened: the readings cards and the panes",
    ),
    (
        View::Watches,
        "Watches",
        "Your watches and their takes: pick a watch to start Listen, a Positions Test or a \
         Power Reserve Test, or open one of its takes",
    ),
];

/// The Watch line's words with no watch picked.
const NO_WATCH: &str = "No Watch (Quick Listen)";
/// The Watch line's title on the narrowest card.
const NO_WATCH_SHORT: &str = "No Watch";
/// Below this width the Calibre card's title steps down to 12 pt.
const NARROW_TITLE_W: f32 = 160.0;
/// The Watch line's menu's width at least.
const WATCH_MENU_W: f32 = 220.0;
/// How many recent watches the Watch line's menu lists.
const RECENT_WATCHES: usize = 5;

/// The day of a UTC timestamp kept as text ("2026-10-12T…").
fn day_of(utc: &str) -> &str {
    utc.get(..10).unwrap_or(utc)
}

/// The Rate caveat for an input whose clock hasn't been measured.
/// Plain marks on the level meter, for reference only: the level core
/// aims a recording at and the one it calls hot.
const REFERENCE_DBFS: [f64; 2] = [
    timegrapher_core::diagnose::TARGET_PEAK_DBFS,
    timegrapher_core::diagnose::HOT_PEAK_DBFS,
];
/// The width of the Settings window's sidebar.
const SETTINGS_SIDEBAR_W: f32 = 146.0;
use crate::session::{self, Action, Session};
use timegrapher_core::analysis_take;
use timegrapher_core::recorder::{self, Recorder, SavedCopy, SessionInfo};
use timegrapher_core::session::{self as core_session, Manifest, RecordedPosition, RecordingEntry};
use timegrapher_core::stream::{BeatLog, StreamConfig};

/// The kinds of take New Take starts for the watch picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewTake {
    Listen,
    Positions,
    PowerReserve,
}

/// Open Recording… or Open Folder…, asked before anything is read: add
/// the recording to a watch, with what its files don't say filled in, or
/// just open it where it is.
#[derive(Debug, Clone)]
struct OpenAsk {
    path: PathBuf,
    /// Add to a watch, else just open it.
    add: bool,
    /// The watch it goes into, by id.
    watch: Option<String>,
    /// Into a new watch of the name `new_name` instead.
    new_watch: bool,
    new_name: String,
    /// What the recording's own files say.
    has: OpenHas,
    bph: Option<u32>,
    lift: f64,
    /// The lift angle was typed or picked, not left at the default.
    lift_set: bool,
    position: Option<Position>,
    /// When it started, in this computer's time, as 2026-10-22 14:30.
    started: String,
    error: Option<String>,
    /// Asked by Keep in a Watch…, of the recording already on show: to
    /// keep it, not to open it.
    keeping: bool,
}

/// What a recording's files say about it: its session file, if it has
/// one, and its session.json.
#[derive(Debug, Clone, Default)]
struct OpenHas {
    manifest: bool,
    bph: Option<u32>,
    lift: Option<f64>,
    calibre: Option<String>,
    /// Every reading's position, from its session file or its file name.
    position: bool,
    started: bool,
}

/// What a recording added to a watch needs filled in: what neither its
/// files nor the watch (with its calibre) say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct OpenNeed {
    bph: bool,
    lift: bool,
    position: bool,
    started: bool,
}

/// What a watch picked in the Watches view's list does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListPick {
    /// Shows it, refused while a watch's take listens.
    Pick,
    /// Asks about the unsaved sound, then stops and picks it.
    Ask,
    /// Stops listening, with nothing to ask about, and picks it.
    StopThenPick,
}

/// The way a watch picked in the list goes: `running`, whether a Quick
/// Listen is what listens, and whether its sound would be asked about.
fn list_pick_route(running: bool, quick: bool, asks: bool) -> ListPick {
    match (running && quick, asks) {
        (false, _) => ListPick::Pick,
        (true, true) => ListPick::Ask,
        (true, false) => ListPick::StopThenPick,
    }
}

/// Set Up the Microphone's width, and its status line's height: two
/// lines, whatever it says.
const SHEET_W: f32 = 440.0;
const SHEET_STATUS_H: f32 = 36.0;
/// The sheet's steps' contents start this far in, under each title.
const SHEET_INDENT: f32 = 30.0;
/// The names in the sheet's third step, right-aligned in this width.
const SHEET_LABEL_W: f32 = 104.0;

/// A step of Set Up the Microphone: its number in a small grey circle,
/// then its title.
fn sheet_step(ui: &mut egui::Ui, n: u8, title: &str) {
    let pal = theme::pal(ui);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 11.0, pal.control_hover);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            n.to_string(),
            theme::semibold(12.0),
            pal.text,
        );
        // The title starts where the step's contents do.
        ui.add_space(SHEET_INDENT - 22.0 - 2.0 * ui.spacing().item_spacing.x);
        ui.label(RichText::new(title).font(theme::semibold(13.5)));
    });
}

/// A step's contents, in from its number.
fn sheet_indent(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(SHEET_INDENT - ui.spacing().item_spacing.x);
        ui.vertical(|ui| add(ui));
    });
}

/// A row of the sheet's third step: its name right-aligned, its control
/// after it on one left edge.
fn sheet_field(ui: &mut egui::Ui, label: &str, hint: &str, add: impl FnOnce(&mut egui::Ui)) {
    let pal = theme::pal(ui);
    ui.horizontal_top(|ui| {
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(SHEET_LABEL_W, 20.0), egui::Sense::hover());
        if !label.is_empty() {
            ui.painter().text(
                egui::pos2(rect.right(), rect.center().y),
                egui::Align2::RIGHT_CENTER,
                label,
                egui::FontId::proportional(13.0),
                pal.text,
            );
            resp.on_hover_text(hint);
        }
        ui.vertical(|ui| add(ui));
    });
}

/// What a question before throwing away unsaved sound is about.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Confirm {
    NewSession,
    /// A new take of the watch picked, of the kind asked for.
    NewTake(NewTake),
    /// Another watch, or No Watch (`None`), picked on the Watch line
    /// while a take is on show: the take is put away.
    PickWatch(Option<String>),
    Quit,
    /// Start listening to the microphone, over a recording whose masks
    /// changed.
    Microphone,
    /// Quick Listen: a new session on the microphone, with no watch.
    QuickListen,
    /// Open a recording, analysed whole.
    Open {
        path: PathBuf,
    },
    /// Start Test on the Power Reserve pane: a new take of the watch.
    ReserveStart,
}

/// A test that ended with no watch picked, asked where it goes: Keep in
/// Unsorted, Keep in a Watch… or Throw Away (Quick Listen's sound goes
/// without a question; a test's never does).
#[derive(Debug, Clone, Copy, PartialEq)]
enum TestKeep {
    /// A Positions Test's take, not saved yet.
    Positions,
    /// A Power Reserve Test's take, its results read, not saved yet.
    PowerReserve,
}

/// Where a session's sound is kept until it is saved or thrown away: a
/// folder of the app's own that the system doesn't clear, so sound kept
/// when the app quits without asking (or is stopped) is still there at the
/// next start. Tests use the temporary folder.
fn scratch_dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join("timegrapher-unsaved");
    }
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(target_os = "windows") {
        env("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local").join("share")))
    };
    base.map_or_else(
        || std::env::temp_dir().join("timegrapher-unsaved"),
        |b| b.join("timegrapher").join("unsaved"),
    )
}

/// Sound a session recorded that was never saved or thrown away: the app
/// quit without asking (on macOS, Cmd+Q and Quit in the app menu or the
/// Dock), or was stopped (a crash, a logout, a shutdown).
#[derive(Debug, Clone, PartialEq)]
struct KeptSound {
    dir: PathBuf,
    /// "2026-10-21 14.32", from its session.json, in this computer's
    /// time.
    started: Option<String>,
    duration_s: Option<f64>,
    bytes: u64,
}

/// The sound kept in `scratch` from earlier sessions, oldest first,
/// leaving out any folder another copy of the app is recording into now.
fn kept_sounds(scratch: &Path) -> Vec<KeptSound> {
    let mut v: Vec<KeptSound> = std::fs::read_dir(scratch)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|d| !timegrapher_core::recorder::in_use(d))
        .filter_map(|dir| {
            let files: Vec<std::fs::DirEntry> = std::fs::read_dir(&dir).ok()?.flatten().collect();
            let has_sound = files.iter().any(|f| {
                let n = f.file_name().to_string_lossy().to_lowercase();
                n.ends_with(".wav") || n.ends_with(".flac")
            });
            if !has_sound {
                return None;
            }
            let bytes = files
                .iter()
                .filter_map(|f| f.metadata().ok())
                .map(|m| m.len())
                .sum();
            let about: Option<serde_json::Value> = std::fs::read(dir.join("session.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            let started = about
                .as_ref()
                .and_then(|a| a["started_utc"].as_str())
                .map(|u| {
                    // In this computer's time, as takes are named; as
                    // written, marked UTC, when it can't be read.
                    watches::local_minute(u).unwrap_or_else(|| {
                        let at = u.get(..16).unwrap_or(u).replacen('T', " ", 1);
                        format!("{} UTC", at.replace(':', "."))
                    })
                });
            Some(KeptSound {
                started,
                duration_s: about.as_ref().and_then(|a| a["duration_s"].as_f64()),
                bytes,
                dir,
            })
        })
        .collect();
    v.sort_by(|a, b| a.dir.cmp(&b.dir));
    v
}

/// Move the recording folder `dir` into `parent`, under its own name:
/// renamed, or onto another disk copied, each file written to the disk,
/// and only then removed. Returns where it went.
fn move_kept(dir: &Path, parent: &Path) -> Result<PathBuf, String> {
    let name = dir.file_name().ok_or("The kept sound has no folder name")?;
    let to = library::unused_dir(parent, &name.to_string_lossy());
    timegrapher_core::recorder::remove_stale_lock(dir);
    if std::fs::rename(dir, &to).is_ok() {
        return Ok(to);
    }
    let copied = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&to)?;
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let mut r = std::fs::File::open(e.path())?;
            let mut w = std::fs::File::create(to.join(e.file_name()))?;
            std::io::copy(&mut r, &mut w)?;
            w.sync_all()?;
        }
        Ok(())
    })();
    match copied {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(dir);
            Ok(to)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&to);
            Err(format!("Couldn't move it to {}: {e}", parent.display()))
        }
    }
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
    pub(crate) fn from_name(s: &str) -> Option<Position> {
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

    /// Witschi's name: the hour that points up for a crown position, CH
    /// and CB (cadran haut, cadran bas) lying flat.
    pub fn witschi(self) -> &'static str {
        match self {
            Position::DialUp => "CH",
            Position::DialDown => "CB",
            Position::CrownUp => "3H",
            Position::CrownDown => "9H",
            Position::CrownLeft => "6H",
            Position::CrownRight => "12H",
        }
    }

    /// How the watch lies, for its picture on the Current Position card.
    fn lying(self) -> crate::icons::Lying {
        use crate::icons::Lying;
        match self {
            Position::DialUp => Lying::DialUp,
            Position::DialDown => Lying::DialDown,
            Position::CrownUp => Lying::Hour(3),
            Position::CrownDown => Lying::Hour(9),
            Position::CrownLeft => Lying::Hour(6),
            Position::CrownRight => Lying::Hour(12),
        }
    }

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

/// The window's two views: the Bench, one take in the readings and the
/// panes, and Watches, the library of watches and their takes. They take
/// turns filling the window between the mode row and the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    #[default]
    Bench,
    Watches,
}

/// The panes below the readings. Tick shape and the periodicity (FFT)
/// views will join these.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Pane {
    /// The Watches view's content and toolbar. Once a pane: a layout saved
    /// with it (or with the Microphone pane, gone since) has it taken out
    /// as it is read, and it is never in the pane tree.
    #[serde(alias = "Microphone")]
    Watch,
    Strip,
    Sound,
    /// Rate, amplitude and beat error over the take, one chart each in
    /// the one pane. Layouts saved when each was a pane of its own name
    /// them one by one.
    #[serde(alias = "Rate", alias = "Amplitude", alias = "BeatError")]
    Charts,
    /// How often each value came up, one plot for each reading in the one
    /// pane; once three panes.
    #[serde(
        alias = "RateHistogram",
        alias = "AmplitudeHistogram",
        alias = "BeatErrorHistogram"
    )]
    Distributions,
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
    SteadyStft,
    SteadyWavelet,
    Scope,
    Isochronism,
    PositionsTest,
    PowerReserve,
}

impl Pane {
    /// Every pane, in the order the Panes list shows them.
    const ALL: [Pane; 17] = [
        Pane::PositionsTest,
        Pane::PowerReserve,
        Pane::Strip,
        Pane::Sound,
        Pane::Scope,
        Pane::Isochronism,
        Pane::Charts,
        Pane::Distributions,
        Pane::SteadyReadings,
        Pane::SteadyPeriods,
        Pane::SteadyCycle,
        Pane::SteadyAutocorrelation,
        Pane::SteadyAllan,
        Pane::SteadyCusum,
        Pane::SteadyFft,
        Pane::SteadyStft,
        Pane::SteadyWavelet,
    ];
    /// The Steadiness views, in the order the icon bar groups them.
    const STEADY: [Pane; 9] = [
        Pane::SteadyReadings,
        Pane::SteadyPeriods,
        Pane::SteadyCycle,
        Pane::SteadyAutocorrelation,
        Pane::SteadyAllan,
        Pane::SteadyCusum,
        Pane::SteadyFft,
        Pane::SteadyStft,
        Pane::SteadyWavelet,
    ];
    /// The core panes, first in the icon bar.
    const CORE: [Pane; 3] = [Pane::Strip, Pane::Sound, Pane::Scope];
    /// The analysis views: the charts, the distributions, the Steadiness
    /// views and Isochronism.
    const ANALYSIS: [Pane; 12] = [
        Pane::Charts,
        Pane::Distributions,
        Pane::SteadyReadings,
        Pane::SteadyPeriods,
        Pane::SteadyCycle,
        Pane::SteadyAutocorrelation,
        Pane::SteadyAllan,
        Pane::SteadyCusum,
        Pane::SteadyFft,
        Pane::SteadyStft,
        Pane::SteadyWavelet,
        Pane::Isochronism,
    ];
    const TESTS: [Pane; 2] = [Pane::PositionsTest, Pane::PowerReserve];

    fn title(self) -> &'static str {
        match self {
            Pane::Watch => "Watches",
            Pane::Strip => "Paper Strip",
            Pane::Sound => "Tick Tock Profile",
            Pane::Charts => "Charts",
            Pane::Distributions => "Distributions",
            Pane::SteadyReadings => "Readings Over Time",
            Pane::SteadyPeriods => "Periods",
            Pane::SteadyCycle => "Cycle",
            Pane::SteadyAutocorrelation => "Autocorrelation",
            Pane::SteadyAllan => "Allan Deviation",
            Pane::SteadyCusum => "CUSUM",
            Pane::SteadyFft => "FFT",
            Pane::SteadyStft => "STFT Map",
            Pane::SteadyWavelet => "Wavelet Map",
            Pane::Scope => "Scope",
            Pane::Isochronism => "Isochronism",
            Pane::PositionsTest => "Positions Test",
            Pane::PowerReserve => "Power Reserve",
        }
    }

    /// What the pane shows, for its icon and its line in the View menu.
    fn hint(self) -> &'static str {
        match self {
            Pane::Watch => {
                "Your watches and their takes: pick a watch to start Listen, a Positions \
                 Test or a Power Reserve Test, or open one of its takes"
            }
            Pane::Strip => "The beats as dots on a paper strip, like a printing timegrapher",
            Pane::Sound => "The typical tick and tock sound, with the edges the readings come from",
            Pane::Charts => {
                "The rate, amplitude and beat error over the whole take, each as a line in a \
                 chart of its own"
            }
            Pane::Distributions => {
                "How often each value of the rate, amplitude and beat error came up, on \
                 probability paper: one steady state falls on a straight line, two states \
                 bend it"
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
            | Pane::SteadyFft
            | Pane::SteadyStft
            | Pane::SteadyWavelet => self.steady_view().map_or("", steady::View::hint),
            Pane::Scope => {
                "The sound of the latest beats as it came in, from 5 ms around the drop to \
                 400 ms or One Beat, the latest few drawn over each other. Each row is \
                 scaled to its own loudest beat."
            }
            Pane::Isochronism => {
                "How the rate moves with the amplitude as the mainspring lets down"
            }
            Pane::PositionsTest => {
                "The six positions one after another, settled and measured in turn, with \
                 their table filling in as it goes, and the test 24 hours after winding \
                 against the one just after"
            }
            Pane::PowerReserve => {
                "The watch wound fully and listened to in one position until it stops: how \
                 long it ran, kept in the watch's history"
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
            Pane::SteadyStft => View::Stft,
            Pane::SteadyWavelet => View::Wavelet,
            _ => return None,
        })
    }
}

/// A pane's icon. Every pane button is drawn in the secondary text
/// colour, and in the accent while its pane is shown: none takes a
/// reading's colour.
fn pane_icon(pane: Pane) -> theme::Icon {
    match pane {
        Pane::Watch => Icon::Watch,
        Pane::Strip => Icon::Strip,
        Pane::Sound => Icon::Profile,
        Pane::Scope => Icon::Scope,
        Pane::SteadyReadings => Icon::Steadiness,
        Pane::SteadyPeriods => Icon::Periods,
        Pane::SteadyCycle => Icon::Cycle,
        Pane::SteadyAutocorrelation => Icon::Autocorrelation,
        Pane::SteadyAllan => Icon::Allan,
        Pane::SteadyCusum => Icon::Cusum,
        Pane::SteadyFft => Icon::Fft,
        Pane::SteadyStft => Icon::Stft,
        Pane::SteadyWavelet => Icon::Wavelet,
        Pane::Isochronism => Icon::Isochronism,
        Pane::PositionsTest => Icon::Positions,
        Pane::PowerReserve => Icon::Reserve,
        Pane::Charts => Icon::Charts,
        Pane::Distributions => Icon::Distributions,
    }
}

/// Save As: Ctrl+Shift+S, or Cmd+Shift+S on a Mac.
fn save_as_shortcut() -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::S,
    )
}

/// The Input Level's hover.
const LEVEL_HINT: &str = "The microphone's gain. For the system default this is the sound \
     server's input volume, which it puts back on the microphone each time it opens it; for a \
     direct device it is the card's capture level. Aim for ticks peaking in the shaded band, \
     around -10 dBFS, and never clipping.";

/// The Peak meter's hover.
const PEAK_HINT: &str = "Loudest sample in the last half second, over a shaded band from -10 \
     to -6 dBFS. Clipping is said in words, not by colour.";

/// Under the Peak meter.
const BAND_WORDS: &str = "Shaded band: −10 to −6 dBFS.";

/// What the sheet and Settings say while the sound clips.
const CLIPPING_WORDS: &str = "Clipping: lower the Input Level.";

/// Automatic Gain's standing warning, beside its switch.
const AGC_WARNING: &str = "Keep this off. It raises the level between ticks and lowers it on \
     each one, so the sound the readings come from keeps changing.";

/// The Automatic Gain switch's hover, in Settings > Microphone.
const AGC_HINT: &str = "The microphone's own automatic gain, which raises its level between \
     ticks and lowers it on each one. Off, the level stays where Input Level puts it, as a \
     timegrapher needs. A stand turns it back on each time it is plugged in, so the app sets \
     it again each time it opens the input.";

const MASK_HINT: &str = "Drag across a stretch to leave it out of the readings. Nothing is \
     deleted: the beats stay, hatched. Right-click a mask to remove it; Edit > Remove All \
     Masks removes them all.";
/// The Steadiness panes' Info button.
const INFO_HINT: &str =
    "Info: what this view shows, and each reading's findings and statistics, in full.";

const EXPORT_HINT: &str = "Save this pane as a picture: a PNG of it as it is on the screen, \
     or an SVG drawing whose lines and text stay sharp at any size. The file name's ending, \
     .png or .svg, decides which. The pane's name, and a chart's key, go at the top in place \
     of its controls.";
const BRUSH_HINT: &str = "Mark a stretch of the strip as one position, on a take live or \
     opened: pick the position, then drag along the strip. Drag a change of position along \
     the track to move it. Esc stops painting.";
const SAVE_MARKS_HINT: &str = "Save the masked stretches and the positions to the recording's \
     session file, so they come back when it is opened again; nothing else in the file \
     changes. A recording with no session file gets one, which makes its folder a take: \
     refused where the folder holds other recordings, or a session file that doesn't list the \
     recording.";
const QUICK_LISTEN_HINT: &str = "Listen: the watch on the microphone, with no watch picked \
     first. Each Listen is a new take. It isn't kept unless Save keeps it, in one of your \
     watches or in a folder: listening again or quitting lets it go without asking.";
const STOP_HINT: &str = "Stop listening and let go of the microphone. Everything so far stays \
     on show, and the strip can be dragged back through it. A take of a watch is saved into it.";
const POSITION_HINT: &str = "The position the watch is in. Changing it while listening marks a \
     new stretch on the strip and the charts, and the readings, profile, distributions and \
     Steadiness then use only this position's beats. It is noted in a saved take.";
const SOUND_SETTINGS_HINT: &str = "The app can't set this input's level, so the system \
     does: this opens the system's sound input settings (Sound > Input on a Mac, Sound on \
     Windows), where the input volume is. Watch the Peak meter here as you change it.";
const LIFT_HINT: &str = "No calibre or lift angle is set, so every amplitude is worked out \
     from the default 52°. Type the calibre's lift angle, pick one, or pick the calibre above \
     it.";
/// The takes a watch's row starts, each saved into the watch.
/// The Watches view's list column, pt.
pub(crate) const WATCH_LIST_W: f32 = 280.0;
/// How wide the watch's side grows, pt: across a 2560 window a figure
/// would sit far from its date.
const WATCH_SIDE_MAX: f32 = 1100.0;
/// The Watches view's width from which the list and the watch sit side by
/// side; narrower, one shows at a time.
const WATCHES_SIDE_BY_SIDE: f32 = 900.0;
/// Why Keep in a Watch is greyed while a take is heard.
const KEEP_AFTER_STOP: &str = "Stop listening first, then keep the take in a watch.";
/// Why a Positions Test's take takes no Position Brush.
const POSITIONS_TAKE_PAINT: &str = "A Positions Test sets its own positions; paint a Listen take.";
/// The hover on a test's setting it can't change while it runs.
const FIXED_IN_TEST: &str = "Fixed while the test runs.";
const NEW_TAKES: [(NewTake, &str, &str); 3] = [
    (
        NewTake::Listen,
        "Listen",
        "Listen to the watch in the position on the {card} card, for as long as you like; \
         Stop saves the take into the watch",
    ),
    (
        NewTake::Positions,
        "Positions Test",
        "The six positions one after another, settled and measured in turn, saved into the \
         watch",
    ),
    (
        NewTake::PowerReserve,
        "Power Reserve Test",
        "Wind the watch fully and listen in one position until it stops, saved into the watch",
    ),
];
const NEW_SESSION_HINT: &str = "Clear everything and start again with the next watch or \
     position. Listening carries on into the new session, unless closing the kept sound \
     fails. Unsaved sound is asked about first.";
const SAVE_HINT: &str = "Save the sound of this session so far, with its clock log and \
     settings, to analyse again later. With a watch picked in the Watches view, the take is \
     saved into that watch, named for when it started; after Quick Listen, into Unsorted. \
     Otherwise the first time it asks \
     where: the take goes in a folder of its own, named for its time and watch, inside the \
     folder you choose. After \
     that, Save Again adds the sound since into the same take without asking. Listening \
     carries on, and so does the window: the copy is written in the background, with \
     Saving… in the status bar until it is done, and the take saved before stays as it was \
     until then. New Session and quitting ask before unsaved sound is thrown away; a quit \
     that can't ask (on macOS, Cmd+Q, the app menu or the Dock) keeps it when there is \
     more than 5 s of it, to offer back at the next start.";

const SAVING_HINT: &str = "Saving now: wait until the sound so far is copied into its take";

const SAVE_AS_HINT: &str = "Save the session so far as a new take, in a folder you choose; \
     Save Again then goes there";

const ANALYSIS_ONLY_HINT: &str = "Keep this session's readings without its sound: every beat, \
     the amplitude windows and the Tick Tock profiles, measured from the whole take again, in a \
     new folder inside the one you choose. It takes a small part of the sound's space, but it \
     can't be measured again, and the Scope needs the sound. Opening the folder shows \
     the readings, charts and tests as before.";

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
    // The charts and distributions are tabs beside the profile, as the
    // other analysis panes are: opening one never takes the strip's room.
    // Dragged out beside another pane, [`keep_room`] keeps both readable.
    let charts = tiles.insert_pane(Pane::Charts);
    let hists = tiles.insert_pane(Pane::Distributions);
    // Scope behind the profile, then the rest, as more tabs.
    let scope = tiles.insert_pane(Pane::Scope);
    let steady: Vec<TileId> = Pane::STEADY.map(|p| tiles.insert_pane(p)).to_vec();
    let isochronism = tiles.insert_pane(Pane::Isochronism);
    let positions = tiles.insert_pane(Pane::PositionsTest);
    let reserve = tiles.insert_pane(Pane::PowerReserve);
    if let Some(egui_tiles::Tile::Container(c)) = tiles.get_mut(sound) {
        c.add_child(scope);
        c.add_child(charts);
        c.add_child(hists);
        for v in steady {
            c.add_child(v);
        }
        c.add_child(isochronism);
        c.add_child(positions);
        c.add_child(reserve);
    }
    // The strip across the middle and the profile across the bottom, with
    // the panes hidden until wanted as tabs behind the profile.
    let dir = if horizontal {
        LinearDir::Vertical
    } else {
        LinearDir::Horizontal
    };
    let mut middle = Linear::new(dir, vec![strip, sound]);
    middle.shares.set_share(strip, 1.6);
    let root = tiles.insert_container(middle);
    let mut tree = Tree::new("panes", root, tiles);
    for p in Pane::ALL {
        if !matches!(p, Pane::Strip | Pane::Sound | Pane::Scope) {
            set_pane_visible(&mut tree.tiles, p, false);
        }
    }
    tree
}

/// The mode row's height, in both views.
const MODE_ROW_H: f32 = 28.0;

/// The readings cards' widths: Current Position, Listen and Calibre keep
/// these while the three reading cards share the rest; on a narrower
/// window they step down (see `readouts`) before the reading cards do.
/// The readings cards left to right, by their place in `readouts`'
/// widths: Position, Calibre, Listen, Rate, Amplitude, Beat Error.
const CARD_ORDER: [usize; 6] = [0, 5, 1, 2, 3, 4];
const POSITION_W: f32 = 250.0;
const LISTEN_W: f32 = 210.0;
const CALIBRE_W: f32 = 250.0;
/// A reading card wide enough for its figure in full, and for its
/// compact figure with room to spare.
const READING_FULL_W: f32 = 210.0;
const READING_ROOMY_W: f32 = 150.0;
const POSITION_COMPACT_W: f32 = 112.0;
/// A compact Listen card gives the take's length beside its caption; on
/// the narrowest windows it drops to `LISTEN_NARROWEST_W`, still room for
/// both, and the reading cards keep `CAPTION_FIT_W`, room for their
/// captions whole.
const LISTEN_COMPACT_W: f32 = 140.0;
const LISTEN_NARROWEST_W: f32 = 126.0;
const CAPTION_FIT_W: f32 = 118.0;
/// The compact Calibre card: wide enough for the beat rate and the lift
/// side by side where the reading cards keep their captions whole beside
/// it, else narrower, the beat rates then in the Calibre menu.
const CALIBRE_ROOMY_W: f32 = 250.0;
const CALIBRE_COMPACT_W: f32 = 126.0;
/// The narrowest Calibre card's controls row that holds the beat rate
/// and the lift side by side.
const TWO_FIELDS_W: f32 = CALIBRE_ROOMY_W - 2.0 * 16.0;
/// The Current Position card's picture, and on a compact card.
const POSITION_PIC: f32 = 66.0;
const POSITION_PIC_COMPACT: f32 = 40.0;
/// The Listen card's buttons.
const LISTEN_BUTTON: f32 = 40.0;

/// A readings card's three rows inside its padding.
#[derive(Clone)]
struct CardRows {
    inner: egui::Rect,
    title: egui::Rect,
    figure: egui::Rect,
    controls: egui::Rect,
}

impl CardRows {
    fn of(card: egui::Rect) -> Self {
        let inner = card.shrink(f32::from(theme::CARD_PADDING));
        let band = |top: f32, h: f32| {
            egui::Rect::from_min_size(
                egui::pos2(inner.left(), inner.top() + top),
                Vec2::new(inner.width(), h),
            )
        };
        CardRows {
            inner,
            title: band(0.0, theme::TITLE_ROW_H),
            figure: band(theme::TITLE_ROW_H + theme::ROW_GAP, theme::FIGURE_ROW_H),
            controls: band(
                theme::TITLE_ROW_H + theme::FIGURE_ROW_H + 2.0 * theme::ROW_GAP,
                theme::CONTROL_H,
            ),
        }
    }
}

/// The input's clock in words, as Quartz Correction gives it.
struct ClockWords {
    /// "20.2 ppm fast", or "Not Timed".
    value: String,
    hint: String,
}

/// A reading's limit in short in Title Case, for its chip: "9 s of Beats",
/// "3% Clipped", "28800 bph Heard".
/// The beat rate's choices: Auto, then the standard rates, the one in use
/// ticked.
fn bph_choices(ui: &mut egui::Ui, bph: &mut Option<u32>) {
    let choices = std::iter::once((None, "Auto: found from the beats".to_string())).chain(
        STANDARD_BPH
            .into_iter()
            .map(|b| (Some(b), format!("{b} bph"))),
    );
    for (v, words) in choices {
        if theme::menu_item(ui, *bph == v, words).clicked() {
            *bph = v;
        }
    }
}

fn chip_words(short: &str) -> String {
    short
        .split(' ')
        .enumerate()
        .map(|(n, w)| {
            let keep = matches!(w, "s" | "bph" | "ms" | "is")
                || (n > 0 && matches!(w, "of" | "by" | "the"))
                || w.chars().next().is_none_or(|c| !c.is_alphabetic());
            if keep {
                w.to_string()
            } else {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where a galley's first line sits: its baseline, down from its top.
fn baseline(g: &egui::Galley) -> f32 {
    g.rows
        .first()
        .and_then(|r| Some(r.pos.y + r.row.glyphs.first()?.pos.y))
        .unwrap_or(g.size().y * 0.8)
}

/// A readings card's figure row: the big figure ("—" while there is none),
/// then its ± and unit on the figure's baseline, or what the reading waits
/// on in their place. On a compact card the figure is smaller and the ±
/// sits over the unit beside it. A figure too wide for the card (noise
/// read as beats can give thousands of s/d) is made smaller to show whole.
fn figure_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    value: Option<&str>,
    unit: &str,
    status: Option<&(String, String)>,
    hover: &str,
    compact: bool,
) {
    let pal = theme::pal(ui);
    let (text, colour) = match value {
        // A true minus sign, as wide as the plus.
        Some(v) => (v.replace('-', "−"), pal.text),
        None => ("—".to_string(), pal.text_tertiary),
    };
    let side_text = status.map_or(unit, |s| s.0.as_str());
    // Beside the figure: one line in Body, or on a compact card two small
    // lines, the ± over the unit.
    let lines: Vec<String> = if side_text.is_empty() {
        Vec::new()
    } else if !compact {
        vec![side_text.to_string()]
    } else if status.is_some() {
        // Two lines as even as the words allow: "Settling," over "12 s",
        // "Waiting for" over "Beats".
        let splits = side_text.match_indices(' ').map(|(i, _)| i);
        match splits.min_by_key(|&i| i.abs_diff(side_text.len() - i)) {
            Some(i) => vec![side_text[..i].to_string(), side_text[i + 1..].to_string()],
            None => vec![side_text.to_string()],
        }
    } else {
        // "± 8.6 s/d" stacks; "± 0.8°" and "bph" stay one line.
        match side_text.rsplit_once(' ') {
            Some((a, b)) if a.starts_with('±') && a.contains(' ') => {
                vec![a.to_string(), b.to_string()]
            }
            _ => vec![side_text.to_string()],
        }
    };
    let mut side_font = egui::FontId::proportional(if compact { 12.0 } else { 15.0 });
    let lay_side = |font: &egui::FontId| -> Vec<_> {
        lines
            .iter()
            .map(|l| {
                ui.painter()
                    .layout_no_wrap(l.clone(), font.clone(), pal.text_secondary)
            })
            .collect()
    };
    let mut side = lay_side(&side_font);
    let mut side_w = side.iter().map(|g| g.size().x).fold(0.0, f32::max);
    // Words in the figure's place ("No Regular Beat") show whole: with no
    // figure, the dash gives them its room, and they are made smaller
    // only when even that is too narrow.
    let dash_w = ui
        .painter()
        .layout_no_wrap(
            text.clone(),
            theme::display(if compact {
                theme::FIGURE_COMPACT_PT
            } else {
                theme::FIGURE_PT
            }),
            colour,
        )
        .size()
        .x;
    let no_dash = value.is_none() && status.is_some() && dash_w + 6.0 + side_w > rect.width();
    if no_dash && side_w > rect.width() {
        side_font.size = (side_font.size * rect.width() / side_w).floor().max(9.0);
        side = lay_side(&side_font);
        side_w = side.iter().map(|g| g.size().x).fold(0.0, f32::max);
    }
    let room = rect.width() - if side_w > 0.0 { side_w + 6.0 } else { 0.0 };
    let full = if compact {
        theme::FIGURE_COMPACT_PT
    } else {
        theme::FIGURE_PT
    };
    let lay = |size: f32| {
        ui.painter()
            .layout_no_wrap(text.clone(), theme::display(size), colour)
    };
    let mut big = lay(full);
    if big.size().x > room && value.is_some() {
        big = lay((full * room / big.size().x).floor().max(12.0));
    }
    let top = rect.center().y - big.size().y / 2.0;
    let base = top + baseline(&big);
    let painter = ui.painter().with_clip_rect(rect.expand(2.0));
    let x = if no_dash {
        rect.left()
    } else {
        rect.left() + big.size().x + 6.0
    };
    if !no_dash {
        painter.galley(egui::pos2(rect.left(), top), big, colour);
    }
    let step = side_font.size * 1.2;
    let n = side.len();
    for (i, g) in side.into_iter().enumerate() {
        let line_base = base - (n - 1 - i) as f32 * step;
        let y = line_base - baseline(&g);
        painter.galley(egui::pos2(x, y), g, pal.text_secondary);
    }
    let hover = match status {
        Some((_, h)) if !h.is_empty() && !hover.is_empty() => format!("{h}\n\n{hover}"),
        Some((_, h)) if !h.is_empty() => h.clone(),
        _ => hover.to_string(),
    };
    if !hover.is_empty() {
        ui.interact(
            rect,
            ui.id().with(("figure", rect.left() as i32)),
            egui::Sense::hover(),
        )
        .on_hover_text(hover);
    }
}

/// `text` then `tail`, the text cut short with "…" where the two are
/// wider than `room`, so the tail always shows.
fn elide(ui: &egui::Ui, text: &str, tail: &str, font: &egui::FontId, room: f32) -> String {
    let wide = |t: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
    };
    let whole = format!("{text}{tail}");
    if wide(&whole) <= room {
        return whole;
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |n: usize| {
        format!(
            "{}…{tail}",
            chars[..n].iter().collect::<String>().trim_end()
        )
    };
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if wide(&cut(mid)) <= room {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    cut(lo)
}

/// The Calibre card's figure, when it is words and not the pop-up: the
/// calibre, or "No Calibre", at the readings' figure size, smaller where
/// the card is too narrow for it, as a figure is.
fn calibre_figure(ui: &mut egui::Ui, text: &str, colour: Color32) -> egui::Response {
    let room = ui.available_width();
    let wide = ui
        .painter()
        .layout_no_wrap(text.to_string(), theme::display(theme::FIGURE_PT), colour)
        .size()
        .x;
    let size = if wide > room {
        (theme::FIGURE_PT * room / wide).floor().max(12.0)
    } else {
        theme::FIGURE_PT
    };
    ui.add(
        egui::Label::new(RichText::new(text).font(theme::display(size)).color(colour)).truncate(),
    )
}

/// The USB microphone behind an input, for its automatic gain switch on
/// macOS: the one whose name the input's carries (or the other way
/// round), or for the system default (`label` `None`) the only one there
/// is.
fn usb_agc_for(list: Vec<mixer::UsbAgc>, label: Option<&str>) -> Option<mixer::UsbAgc> {
    let named = |m: &mixer::UsbAgc| {
        let (a, b) = (m.name.to_lowercase(), label?.to_lowercase());
        (!a.is_empty() && (a.contains(&b) || b.contains(&a))).then_some(())
    };
    if label.is_none() && list.len() == 1 {
        return list.into_iter().next();
    }
    list.into_iter().find(|m| named(m).is_some())
}

/// Open the system's sound input settings, where the input volume is set
/// on a system whose inputs the app can't turn up or down.
fn open_sound_settings() -> std::io::Result<()> {
    let (program, page) = if cfg!(target_os = "macos") {
        // System Settings (macOS 13 and later) and System Preferences
        // before it name the page differently.
        let major = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|v| v.trim().split('.').next()?.parse::<u32>().ok());
        let page = if major.is_some_and(|m| m < 13) {
            "x-apple.systempreferences:com.apple.preference.sound?input"
        } else {
            "x-apple.systempreferences:com.apple.Sound-Settings.extension?input"
        };
        ("open", page)
    } else if cfg!(target_os = "windows") {
        ("explorer", "ms-settings:sound")
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no sound settings page to open on this system",
        ));
    };
    std::process::Command::new(program)
        .arg(page)
        .spawn()
        .map(|_| ())
}

/// The layout's version: a layout saved before this one is replaced by the
/// default once, as the panes moved.
/// 3: the charts and the distributions became one pane each.
pub(crate) const LAYOUT: u32 = 3;

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

/// The least height a pane is laid out at, one above another, and the
/// least width, side by side: room for its toolbar and one plot.
const MIN_PANE_H: f32 = 140.0;
const MIN_PANE_W: f32 = 240.0;
/// The height of a pane's tab bar.
const TAB_BAR_H: f32 = 24.0;

/// Give every pane on show at least [`MIN_PANE_H`] (or [`MIN_PANE_W`]
/// across), or the height `need` says its tab on show needs if more,
/// taken from the panes with room to spare, so opening a pane never
/// crushes the others into a sliver. Worked from the sizes of the last
/// frame; a window too small for them all shares its room in proportion.
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn keep_room(
    tiles: &mut Tiles<Pane>,
    need: impl Fn(Pane) -> Option<f32>,
    dragged: &std::collections::HashSet<TileId>,
) {
    // The Paper Strip's row, whose least is kept first.
    let strip_row: Vec<TileId> = tiles
        .find_pane(&Pane::Strip)
        .map(|mut id| {
            let mut up = vec![id];
            while let Some(p) = tiles.parent_of(id) {
                up.push(p);
                id = p;
            }
            up
        })
        .unwrap_or_default();
    let ids: Vec<TileId> = tiles.tile_ids().collect();
    for id in ids {
        // A divider the user dragged stands where they left it.
        if dragged.contains(&id) {
            continue;
        }
        let Some(rect) = tiles.rect(id) else { continue };
        let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) = tiles.get(id)
        else {
            continue;
        };
        let shown: Vec<TileId> = l
            .children
            .iter()
            .copied()
            .filter(|&c| tiles.is_visible(c))
            .collect();
        let n = shown.len();
        let vertical = l.dir == LinearDir::Vertical;
        let room = if vertical {
            rect.height()
        } else {
            rect.width()
        };
        let room = room - 8.0 * (n as f32 - 1.0);
        if n < 2 || room <= 0.0 {
            continue;
        }
        // Each child's least: a tab bar's panes may need more than the
        // usual height, under its tab bar. The most any of its tabs needs,
        // not the one on show, so changing tabs never moves the divider.
        let mut least: Vec<f32> = shown
            .iter()
            .map(|&c| {
                if !vertical {
                    return MIN_PANE_W;
                }
                let more = match tiles.get(c) {
                    Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t))) => t
                        .children
                        .iter()
                        .filter(|&&a| tiles.is_visible(a))
                        .filter_map(|&a| match tiles.get(a) {
                            Some(egui_tiles::Tile::Pane(p)) => need(*p),
                            _ => None,
                        })
                        .reduce(f32::max)
                        .map(|h| h + TAB_BAR_H),
                    _ => None,
                };
                more.unwrap_or(0.0).max(MIN_PANE_H)
            })
            .collect();
        // A window too small for them all keeps the Paper Strip's least,
        // and shares the rest out in proportion.
        let sum: f32 = least.iter().sum();
        if sum > room {
            let first: f32 = shown
                .iter()
                .zip(&least)
                .filter(|(c, _)| strip_row.contains(c))
                .map(|(_, l)| *l)
                .sum();
            let (keep, k) = if first < room && sum > first {
                (first, (room - first) / (sum - first))
            } else {
                (0.0, room / sum)
            };
            for (c, l) in shown.iter().zip(&mut least) {
                if keep == 0.0 || !strip_row.contains(c) {
                    *l *= k;
                }
            }
        }
        let least: Vec<f32> = least.iter().map(|l| l / room).collect();
        let share: Vec<f32> = shown.iter().map(|&c| l.shares[c]).collect();
        let total: f32 = share.iter().sum();
        // Raise the smallest to their least, then share the rest out as
        // before, until none of the rest falls under theirs.
        let mut small = vec![false; n];
        let rest_and_left = |small: &[bool]| {
            let rest: f32 = (0..n).filter(|&i| !small[i]).map(|i| share[i]).sum();
            let left = 1.0 - (0..n).filter(|&i| small[i]).map(|i| least[i]).sum::<f32>();
            (rest, left)
        };
        loop {
            let (rest, left) = rest_and_left(&small);
            let under: Vec<usize> = (0..n)
                .filter(|&i| !small[i] && share[i] / rest * left < least[i])
                .collect();
            if under.is_empty() {
                break;
            }
            for i in under {
                small[i] = true;
            }
        }
        if !small.contains(&true) {
            continue;
        }
        let (rest, left) = rest_and_left(&small);
        if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) =
            tiles.get_mut(id)
        {
            for (i, &c) in shown.iter().enumerate() {
                let f = if small[i] {
                    least[i]
                } else {
                    share[i] / rest * left
                };
                l.shares.set_share(c, f * total);
            }
        }
    }
}

/// Each row or column of panes' shares, to see which a drag changed.
fn shares_of(tiles: &Tiles<Pane>) -> std::collections::HashMap<TileId, Vec<u32>> {
    tiles
        .tile_ids()
        .filter_map(|id| match tiles.get(id) {
            Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) => Some((
                id,
                l.children.iter().map(|&c| l.shares[c].to_bits()).collect(),
            )),
            _ => None,
        })
        .collect()
}

/// The status bar's progress words while a take is read: how far, as the
/// take's name is already at the left of the bar.
fn batch_words(frac: f64, known: bool) -> String {
    if known {
        format!("Analysing, {:.0}%", frac * 100.0)
    } else {
        "Analysing…".to_string()
    }
}

/// Bring the row holding `pane` (its tabs, or the pane itself) down to
/// `h` points where it is taller, the other rows sharing the rest as they
/// did; from where the panes were last laid out.
fn fit_row(tiles: &mut Tiles<Pane>, pane: Pane, h: f32) {
    size_row(tiles, pane, h, false);
}

/// Bring the row holding `pane` up to `h` points where it is shorter, no
/// more than half the rows' room, so the rows above keep the larger part.
fn grow_row(tiles: &mut Tiles<Pane>, pane: Pane, h: f32) {
    size_row(tiles, pane, h, true);
}

fn size_row(tiles: &mut Tiles<Pane>, pane: Pane, h: f32, grow: bool) {
    let Some(id) = tiles.find_pane(&pane) else {
        return;
    };
    let row = tiles
        .parent_of(id)
        .filter(|&p| {
            matches!(
                tiles.get(p),
                Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(_)))
            )
        })
        .unwrap_or(id);
    let (Some(linear), Some(now)) = (tiles.parent_of(row), tiles.rect(row)) else {
        return;
    };
    let Some(rect) = tiles.rect(linear) else {
        return;
    };
    let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) = tiles.get(linear)
    else {
        return;
    };
    let shown: Vec<TileId> = l
        .children
        .iter()
        .copied()
        .filter(|&c| tiles.is_visible(c))
        .collect();
    let room = rect.height() - 8.0 * (shown.len() as f32 - 1.0);
    // Grown, never past half the room, nor into another row's least.
    let others_least = MIN_PANE_H * (shown.len() as f32 - 1.0);
    let h = if grow {
        h.min(room / 2.0).min(room - others_least)
    } else {
        h
    };
    let wrong_way = if grow {
        now.height() >= h
    } else {
        now.height() <= h
    };
    if l.dir != LinearDir::Vertical || shown.len() < 2 || wrong_way || room <= h {
        return;
    }
    let others: f32 = shown
        .iter()
        .filter(|&&c| c != row)
        .map(|&c| l.shares[c])
        .sum();
    let f = h / room;
    if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) =
        tiles.get_mut(linear)
    {
        l.shares.set_share(row, others * f / (1.0 - f));
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

/// The longest a frame spends saving and reading sound that has come in;
/// any more waits for the next frame.
const FRAME_BUDGET: Duration = Duration::from_millis(20);
/// The same for [`TimegrapherApp::pump`], which runs off the window's
/// thread and only holds the window up while it works.
pub(crate) const PUMP_BUDGET: Duration = Duration::from_millis(50);
/// Averaging times offered for the readings, seconds.
const AVERAGES: [f64; 6] = [2.0, 4.0, 10.0, 20.0, 30.0, 60.0];
/// Strip widths offered, ms either side of the centre. 62.5 ms is half a
/// beat at 28,800 bph, the widest view in which tick and tock can't wrap
/// onto each other; the wider ones keep a long take's drift from wrapping.
const WIDTHS: [f64; 16] = [
    1.0, 2.5, 5.0, 10.0, 20.0, 50.0, 62.5, 100.0, 250.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0,
    30_000.0, 60_000.0,
];
/// Strip lengths offered, seconds: from a few seconds of beats to a two-hour
/// run. Fit, the whole session, is a switch of its own beside them.
const SPANS: [f64; 11] = [
    10.0, 30.0, 60.0, 300.0, 1800.0, 3600.0, 7200.0, 21_600.0, 86_400.0, 172_800.0, 259_200.0,
];

fn span_label(v: f64) -> String {
    fields::duration(v)
}

/// The strip's width either side of the centre: ±250 ms, ±2 s.
fn width_label(v: f64) -> String {
    if v >= 1000.0 {
        format!("±{} s", fields::plain(v / 1000.0))
    } else {
        format!("±{} ms", fields::plain(v))
    }
}

/// A width typed as 250, ±250 ms or ±2 s, in ms.
fn parse_width(s: &str) -> Option<f64> {
    let t = s.trim().to_lowercase();
    let v = fields::parse_number(&t)?;
    Some(if t.ends_with('s') && !t.ends_with("ms") {
        v * 1000.0
    } else {
        v
    })
}

/// The strip's width, as its field shows and reads it.
const HALF_WIDTH: Format = Format {
    show: width_label,
    parse: parse_width,
};
/// Settle times offered on the Positions Test pane, seconds.
const SETTLES: [f64; 4] = [10.0, 20.0, 30.0, 60.0];
/// Measure times offered on the Positions Test pane, seconds.
const MEASURES: [f64; 6] = [30.0, 60.0, 90.0, 120.0, 180.0, 300.0];
/// Lift angles offered, degrees.
const LIFTS: [f64; 6] = [44.0, 49.0, 50.0, 52.0, 53.0, 55.0];
/// Most points kept on a chart when it is rebuilt from the whole session.
const TREND_POINTS: f64 = 4000.0;

const MS: Format = Format {
    show: fields::plain,
    parse: fields::parse_number,
};
/// An angle in whole or half degrees, with its °.
const DEGREES: Format = Format {
    show: show_degrees,
    parse: fields::parse_number,
};
fn show_degrees(v: f64) -> String {
    format!("{}°", fields::plain(v))
}
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
    /// Tick's and Tock's amplitude dash violet with blue or deep orange.
    under: Option<Color32>,
    /// The reading itself, drawn heavier than the lines beside it.
    main: bool,
}

/// The rate over each 2 seconds of beats, on a grid from the first beat,
/// worked out on as beats arrive: (start, s/day). Whatever it was worked
/// out from is kept, so it starts again when that changes.
#[derive(Debug, Default)]
struct RateTwoS {
    first: Option<f64>,
    beats: usize,
    bph: Option<u32>,
    breaks: Vec<f64>,
    masked: Vec<(f64, f64)>,
    /// The next stretch on the grid to work out.
    next: usize,
    values: Vec<(f64, f64)>,
}

impl RateTwoS {
    /// Seconds of beats in each value, as the amplitude's stretches.
    const STEP_S: f64 = timegrapher_core::distribution::STRETCH_S;

    /// Bring the values up to `live`'s settled beats. None reaches across a
    /// break (a jump or a new position) or into a masked stretch, as no
    /// reading does.
    fn refresh(&mut self, live: &LiveAnalyzer, masked: &[(f64, f64)]) {
        let beats = live.beats();
        let first = beats.first().map(|b| b.time);
        if self.first != first
            || self.beats > beats.len()
            || self.bph != live.bph()
            || self.breaks != live.breaks()
            || self.masked != masked
        {
            *self = RateTwoS {
                first,
                bph: live.bph(),
                breaks: live.breaks().to_vec(),
                masked: masked.to_vec(),
                ..Default::default()
            };
        }
        self.beats = beats.len();
        let (Some(t0), Some(bph)) = (first, self.bph) else {
            return;
        };
        self.next = timegrapher_core::distribution::rates_2s(
            beats,
            bph,
            t0,
            self.next,
            &self.breaks,
            &self.masked,
            &mut self.values,
        );
    }
}

/// How long the Listen button takes no tap after it replaces Stop, s: a
/// lagging window can still show Stop for a moment after it has gone.
const LISTEN_SETTLE_S: f64 = 0.5;

/// The Listen button's state now (`now`, s), from last frame's (`None` when
/// Stop stood there): when its count began, from the frame after Listen first
/// shows, and whether the latest press came [`LISTEN_SETTLE_S`] after that.
/// A press delivered sooner was made while Stop was on screen (#725).
fn listen_tap(was: Option<(f64, bool)>, now: f64, pressed: bool) -> (f64, bool) {
    let (since, armed) = was.map_or((f64::INFINITY, false), |(t, a)| (t.min(now), a));
    let settled = since.is_finite() && now - since >= LISTEN_SETTLE_S;
    (since, if pressed { settled } else { armed })
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

/// What the take on show's readings rest on, a counter for each part,
/// moved only where that part is set: every figure kept for later keys on
/// the parts it reads, so a change in one place reaches every pane. Moves
/// to core with #294's one measurement context per take.
///
/// Bumped in the put_* setters and in these places only: positions_moved
/// (breaks; also after the Settle field resettles the take) and
/// set_position (breaks), calibre_changed when the wheels change
/// (settings), and new_take from reset_session and show_log (every
/// part). Any other write to `rev` is a defect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Revision {
    /// The lift angle, the calibre and the beat rate.
    settings: u64,
    /// The take's own clock and the clock kept for the input.
    clock: u64,
    /// The take's breaks and its changes of position.
    breaks: u64,
    /// The stretches masked out.
    masks: u64,
}

impl Revision {
    /// A take opened or a new session: every part is new.
    fn new_take(&mut self) {
        self.settings += 1;
        self.clock += 1;
        self.breaks += 1;
        self.masks += 1;
    }
}

/// What the strip's own readings were worked out for: the window and the
/// spacing in steps, the averaging time, how much sound there was and
/// what the readings rest on.
type StripKey = (i64, i64, u64, u64, u64, Revision);

/// How far apart the strip's overlay readings should be at most over a
/// window `span_s` long: fine enough to follow a swing of a few seconds
/// on a short strip, without working out more than a few hundred.
fn strip_step(span_s: f64) -> f64 {
    (span_s / 400.0).max(0.5)
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
    /// The reading's beat error as core reads one (`beat_error_signed`).
    fn beat_error(&self) -> Option<f64> {
        core_session::beat_error_signed(self.beat_error_unlock, self.beat_error_drop)
    }

    /// The reading at `t`; a reading whose averaging time
    /// is not a whole bin (just after the start, a break or a masked
    /// stretch, or over a dropout; see `LiveReading::whole`) has no
    /// values, since it isn't the average the chart is of.
    fn of(t: f64, r: &LiveReading) -> Self {
        if !r.whole {
            return TrendPoint {
                t,
                rate: None,
                amplitude: None,
                amplitude_a: None,
                amplitude_b: None,
                beat_error_unlock: None,
                beat_error_drop: None,
                heard: false,
            };
        }
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

/// A take opened by core's `take::open_entry`, for the window: its beats,
/// clock and breaks, and for a take kept as its analysis alone, the Tick
/// Tock profiles it kept.
#[derive(Debug)]
struct Opened {
    log: BeatLog,
    clock: Option<timegrapher_core::clock::ClockFit>,
    /// Where the clock came from, in core's words.
    clock_source: Option<String>,
    breaks: Vec<timegrapher_core::breaks::Break>,
    unlisted: Option<timegrapher_core::breaks::Unlisted>,
    profiles: Option<Vec<analysis_take::StretchProfiles>>,
}

struct Batch {
    rx: std::sync::mpsc::Receiver<Result<Option<Opened>, String>>,
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

/// The session being kept as its analysis alone
/// ([`timegrapher_core::analysis_take`]): its sound read again in the
/// background, then the new take written.
struct AnalysisSave {
    rx: std::sync::mpsc::Receiver<Result<PathBuf, String>>,
    /// Seconds of sound read so far, over both readings of it.
    progress: std::sync::Arc<std::sync::atomic::AtomicU64>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The take's length, seconds.
    total_s: f64,
}

/// A copy of the session's sound being written on a thread of its own,
/// so the window and listening carry on meanwhile.
struct Saving {
    thread: std::thread::JoinHandle<std::io::Result<SavedCopy>>,
    /// The take's folder.
    dest: PathBuf,
    /// The folder it is in, for the next Save… to start in.
    parent: PathBuf,
    /// Seconds of sound it holds.
    secs: f64,
    metadata: session::MetadataSnapshot,
    /// Read the library again once it is written: a new take in a watch.
    rescan: bool,
}

impl Drop for AnalysisSave {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // Cancelled just as it finished: the folder it wrote, never
        // reported, goes, so Cancel leaves nothing written.
        if let Ok(Ok(dir)) = self.rx.try_recv() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Unwound through the analysis to stop it when it is cancelled.
struct Cancelled;

/// What a full-wind search was made for: the watch, the Wind setting,
/// the minute, and the watch's takes folder: when it last changed and
/// the takes in it, as a folder's time can stay the same over a take
/// added and removed at once (on Windows, within its time's resolution).
type FullWindKey = (
    Option<String>,
    bool,
    i64,
    Option<std::time::SystemTime>,
    Vec<std::ffi::OsString>,
);

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
    /// Sound kept from earlier sessions, offered back one at a time.
    kept: Vec<KeptSound>,
    /// The kept sound Delete… is asking about.
    kept_delete_ask: Option<PathBuf>,
    /// Which view fills the window. The app always opens on the Bench.
    view: View,
    /// Find in Watches… asked for the search field's focus.
    watch_search_focus: bool,
    /// The watch the New Watch form adds goes on the microphone, as the
    /// Watch line picks it: New Watch… there opened the form.
    new_watch_on_mic: bool,
    /// The Watches view: the watch picked (its id), the list's search,
    /// order, and the details form.
    watch_open: Option<String>,
    /// The pane shows Unsorted's takes instead of the list.
    unsorted_open: bool,
    /// The list shown over the watch picked, or Unsorted, by ‹ All
    /// Watches: kept with what was picked then, so picking anything else
    /// shows that instead. Its row is marked in the list.
    list_for: Option<(Option<String>, bool)>,
    /// Unsorted Takes was picked with readings on show: the Quick Listen
    /// settings wait for the next Listen, so the readings keep theirs.
    quick_defaults: bool,
    /// This session began with Quick Listen: kept only by Keep in a
    /// Watch…, or by Save… into Unsorted.
    quick_listen: bool,
    watch_search: String,
    watch_sort: watches::Sort,
    /// The added field the list shows under each watch's name, by its
    /// label; none, the calibre.
    watch_list_shows: Option<String>,
    /// The Watches view was wide enough last frame for the list and the
    /// watch side by side.
    watches_wide: bool,
    watch_form: Option<watches::Form>,
    /// The form was opened from Keep in a Watch…: Done keeps what is on
    /// show in the new watch.
    keep_after_form: bool,
    /// A test ended with no watch picked: where it goes, asked.
    test_keep: Option<TestKeep>,
    /// Where the Positions Test and Power Reserve panes were last drawn,
    /// for the question to sit clear of the results it asks about.
    results_rects: [Option<egui::Rect>; 2],
    /// This take ran a Positions Test.
    positions_take: bool,
    /// The position card's title as last drawn, for the words naming it.
    position_card_title: &'static str,
    /// A question about a take (Delete, Keep Analysis Only, Add to
    /// a Watch), and the take being kept as its analysis in the background.
    take_ask: Option<watches::Ask>,
    /// The watch (or Unsorted) picked before Keep in a Watch… showed the
    /// one it keeps into, put back if its question is cancelled.
    keep_back: Option<(Option<String>, bool)>,
    take_keep: Option<(PathBuf, AnalysisSave)>,
    /// The takes' own readings the Watches list shows, worked out on a
    /// thread of their own.
    take_readings: watches::TakeReadings,
    /// The watches' photos as thumbnails, read in the background.
    thumbs: photos::Thumbs,
    /// Report a Problem.
    report: ReportDialog,
    updates: crate::update::Updates,
    /// The Positions Test, while it runs and once finished, until a new
    /// test or a new session.
    guided: Option<guided::Test>,
    /// The Power Reserve Test, while it runs and once ended, until a new
    /// test or a new session.
    reserve: Option<reserve::Test>,
    /// The keeping picked for the next Power Reserve Test. It runs in the
    /// Current Position card's position: there is no second setting.
    reserve_keep_sound: bool,
    /// When the running test last read the beats, audio seconds.
    reserve_read_s: f64,
    /// When the running Power Reserve Test's hours were last measured,
    /// audio seconds.
    reserve_hours_s: f64,
    /// The time the Positions Test didn't use, marked on the strip; kept
    /// after the test ends, until a new session.
    guided_unused: Vec<(f64, f64, &'static str)>,
    /// The stretches the Positions Test measured, the one under way
    /// included, kept like `guided_unused`.
    guided_measured: Vec<(f64, f64)>,
    /// The full-wind test's results the next test is set against as its
    /// 24 hour half: picked from `guided_choices`, or loaded from a file.
    guided_earlier: Option<guided::FullWind>,
    /// For a watch set up for 24 Hours After, its full-wind tests from 12
    /// to 36 hours ago, newest first, for Compare With to pick from.
    guided_choices: Vec<guided::FullWind>,
    /// Whether `guided_earlier` was loaded from a file, so it stays when
    /// the watch's takes are searched again.
    guided_earlier_file: bool,
    /// What `guided_choices` was found for: the watch, the Wind setting,
    /// the minute and the watch's takes folder. Any of them otherwise, it
    /// is searched for again, so a test passing the 12 hour mark, or a
    /// take added or removed, is seen.
    guided_earlier_for: Option<FullWindKey>,
    /// For a watch, the next Positions Test is its 24 hours after half,
    /// set against the watch's own full-wind test (Wind on its toolbar).
    guided_after: bool,
    /// A test set up on a clear Bench, waiting for Start Test on the
    /// Listen card: the Positions Test or the Power Reserve Test.
    test_ready: Option<NewTake>,
    /// Stop Test… pressed on the Listen card, its question open.
    stop_test_ask: bool,
    /// Seconds of settling and of measuring in each position, for the
    /// next test.
    guided_settle_s: f64,
    guided_measure_s: f64,
    /// Look for a newer release each time the app starts.
    check_at_start: bool,
    /// Keep the computer from sleeping on its own while listening.
    keep_awake: bool,
    /// Listen to the microphone as soon as the app opens.
    listen_at_open: bool,
    /// The inputs whose automatic gain is left on (Settings > Microphone).
    agc_on: Vec<String>,
    /// The selected input's automatic gain switch as last read back, when
    /// it has one.
    agc_read: Option<bool>,
    /// The microphone has been set up, or opened once.
    mic_set_up: bool,
    /// Not Now was pressed on Set Up the Microphone: it doesn't come back
    /// by itself, though the app still doesn't listen at launch until a
    /// microphone is set up.
    setup_declined: bool,
    /// Set Up the Microphone is open (the first launch).
    setup_sheet: bool,
    /// Quick Listen starts on the first frame (Listen When the App Opens).
    listen_pending: bool,
    /// Set Up the Microphone has tried listening once.
    setup_tried: bool,
    /// Why the computer isn't being kept awake although asked, set by the
    /// listening thread.
    pub(crate) awake_fault: Option<String>,
    /// Why the input last failed to open, for its row in Settings; none
    /// once one opens.
    input_fault: Option<String>,
    /// Why the listening thread didn't start, if it didn't: then sound is
    /// only taken while the window draws.
    pub(crate) pump_fault: Option<String>,
    /// The check at start is still to be made (on the first frame, which
    /// has the context it repaints).
    check_pending: bool,
    /// Listening, paused, the positions and settling, the kept sound and
    /// what of it is saved.
    session: Session,
    average_s: f64,
    watch: String,
    /// The calibre picked on the Calibre card: a name in the table or one of the
    /// user's own, or empty when not set.
    calibre: String,
    /// The user's calibres, the short list and the recent picks.
    cal: calibre_ui::Calibres,
    /// The Settings window, open at a section, and the kept clocks as read
    /// when it opened or last changed.
    settings_tab: Option<Tab>,
    /// The section Settings opened at last, to open at again.
    settings_last: Tab,
    kept_clocks: Result<clockstore::ClockStore, String>,
    /// The input whose kept clock Forget is asking about.
    forget_ask: Option<String>,
    /// The library's folder when the user chose one; `None` is the
    /// default, `~/Timegrapher Data`.
    library_dir: Option<PathBuf>,
    /// The library as read when Settings opened or it last changed:
    /// `Ok(None)` while there is none at its folder.
    library: Result<Option<library::Library>, String>,
    /// Where Move Library… is asking to move it to.
    library_move_ask: Option<PathBuf>,
    /// A move under way: where to, and its outcome when done.
    library_moving: Option<(PathBuf, LibraryMove)>,
    /// Readings worked out afresh over the strip's window when the run kept
    /// for the whole take is too sparse there, with what they were for.
    strip_points: Option<(StripKey, Vec<TrendPoint>)>,
    /// Stretches masked out of the readings on the strip, start and end in
    /// the session's seconds, sorted and apart. Saved with the take.
    masked: Vec<(f64, f64)>,
    /// For a recording opened from disk: the stretches its session file
    /// masked when it was opened or last saved to, so a change can be
    /// asked about before it is thrown away. None for a microphone session.
    opened_masks: Option<Vec<(f64, f64)>>,
    /// The strip's Mask tool is on: dragging along it masks a stretch, and
    /// a click on a masked one brings it back.
    masking: bool,
    /// For a recording opened from disk: the positions and settling its
    /// session file gave when it was opened or last saved to, so a change
    /// made with the Position Brush can be asked about before it is thrown
    /// away. None for a microphone session.
    opened_positions: Option<Layers>,
    /// Read again at a new beat rate: the positions as they were, and as
    /// the session file gave them, to put back once it is in.
    carried_positions: Option<(Layers, Option<Layers>)>,
    /// The positions as recorded under the painting, from the session file
    /// of the recording being opened, to load with the positions shown.
    pending_recorded: Option<Positions>,
    /// The sound card's clock for the take on show, as core's
    /// `take::open_entry` reads it for every surface: from its clock log,
    /// USB lines or the correction stored for the input it was recorded
    /// on. The readings, tables and True Clock use it in place of the
    /// clock kept for the input.
    take_clock: Option<timegrapher_core::clock::ClockFit>,
    /// Where [`Self::take_clock`] came from, in core's words
    /// (`OpenedEntry::clock_source`), so the Rate's hover names it.
    take_clock_source: Option<String>,
    /// The take on show's breaks, as core reads them with its beats: the
    /// pauses it lists, its clock's steps and the jumps in its beats.
    /// `None` for a session heard here, whose readings go by the clock
    /// kept for the input.
    take_breaks: Option<Vec<timegrapher_core::breaks::Break>>,
    /// Why those breaks are not all the take's own list's, where they
    /// aren't (core's `breaks::Unlisted`), for the caveat that says so.
    take_unlisted: Option<timegrapher_core::breaks::Unlisted>,
    /// The strip's Position Brush is on, with this position: dragging
    /// along the strip sets that stretch to it.
    brush: Option<Position>,
    /// Where the last Save put the session, if it has been saved.
    /// The folder Save offers first.
    save_dir: String,
    /// Asking before something throws away unsaved sound.
    confirm: Option<Confirm>,
    /// Edit ▸ Remove All Masks asked, waiting on its yes.
    remove_masks_asked: bool,
    /// Open Recording… or Open Folder… asking where the recording goes.
    open_ask: Option<OpenAsk>,
    /// The recording last opened, which the path box goes back to when
    /// another can't be opened.
    opened_path: String,
    /// The take picked in the Watches view's list.
    take_picked: Option<PathBuf>,
    /// The watch set up for the microphone, kept while a recording with
    /// its own (or no) settings is open, and put back on listening again.
    mic_setup: Option<WatchSetup>,
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
    /// The readings the Charts pane draws: rate, amplitude, beat error.
    chart_series: [bool; 3],
    /// The readings the Distributions pane draws.
    dist_series: [bool; 3],
    /// The charts drawn last frame: those turned on that had room. The
    /// key names only these.
    charts_drawn: [bool; 3],
    /// The height each pane with plots needed last frame for its toolbar
    /// and one plot at a readable size, kept clear by [`keep_room`].
    pane_need: std::collections::HashMap<Pane, f32>,
    /// Rows and columns of panes whose divider the user dragged: their
    /// sizes stand, a pane's least apart, until a pane in them is shown
    /// or hidden.
    panes_dragged: std::collections::HashSet<TileId>,
    /// Panes drawn too short for their plot, last frame.
    panes_short: std::collections::HashSet<Pane>,
    /// The top of the pane being drawn, for working out its need.
    pane_top: f32,
    /// Show the cumulative share instead of the probability plot.
    hist_cumulative: bool,
    /// The histograms count the readings rather than each 2 seconds of
    /// beats.
    hist_readings: bool,
    /// The rate over each 2 seconds of beats, for the rate's distribution.
    rate_2s: RateTwoS,
    /// The series each Steadiness pane shows: rate, amplitude, beat error.
    steady_series: std::collections::BTreeMap<steady::View, [bool; 3]>,
    /// The Periods pane's scores in dB.
    periods_db: bool,
    /// The FFT pane's rate as the beat timing's spectrum, in ms.
    fft_timing: bool,
    /// The STFT and Wavelet Map panes' colours in dB.
    maps_db: bool,
    /// The calibre's wheels marked on the period axes.
    show_wheels: bool,
    /// The Steadiness panes' Info panel open, in every view alike.
    steady_info: bool,
    scope_view: scope::View,
    /// Sound read from the recording for the Scope once stopped: the
    /// stretch asked for (seconds) and what was read of it.
    scope_audio: Option<ScopeCut>,
    /// Each beat's trace once drawn, so a beat stays on the Scope (fading
    /// as newer ones come) after the live engine has let its sound go.
    scope_traces: std::collections::VecDeque<(ScopeKey, Vec<f32>)>,
    scope_job: Option<std::sync::mpsc::Receiver<ScopeCut>>,
    /// The steadiness tests' latest answer over the session, and the run
    /// in the background that will replace it.
    steady: Option<(Revision, Steadiness)>,
    steady_job: Option<(Revision, std::sync::mpsc::Receiver<Steadiness>)>,
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
    /// Watches view groups folded down to their caption.
    folded: std::collections::BTreeSet<String>,
    /// The rate, amplitude and beat error figures above the panes.
    show_readings: bool,
    /// Asked to put the charts back on the whole session, following new
    /// beats: set by the button, applied by every chart on the next frame.
    charts_to_live: bool,
    charts_to_live_now: bool,
    /// The averaging time each chart last drew with: a new one rescales
    /// its values, which spread wider or narrower with it.
    chart_average_drawn: std::collections::HashMap<&'static str, f64>,
    /// `None` follows the newest beat; otherwise the time at the newest
    /// edge of the strip. Only the strip (and the chart marker for it)
    /// follows this: the readings and the other panes show the newest.
    strip_end: Option<f64>,
    panes: Tree<Pane>,

    capture: Option<Capture>,
    live: Option<LiveAnalyzer>,
    source_label: String,
    batch: Option<Batch>,
    /// The Listen button since it last stood in for Stop: see
    /// [`listen_tap`] (#725).
    listen_tap: Option<(f64, bool)>,
    anchor: Option<Anchor>,
    /// The settings a take on show is read again with, for a new beat
    /// rate, in place of what the take saved.
    reread_with: Option<WatchSetup>,
    /// The strip's width and length before an opened take was fitted to
    /// it, and the fitted view: Listen brings the first back, unless the
    /// strip was changed by hand since.
    live_strip: Option<(StripView, StripView)>,
    /// Fit, on the Paper Strip: while on, the strip's length is the whole
    /// session so far, growing as beats arrive. Any zoom or pan of the
    /// strip's time turns it off.
    strip_fit: bool,
    /// Whether Fit was on before an opened take was fitted.
    live_fit: bool,
    trend: Vec<TrendPoint>,
    /// What the readings in [`Self::trend`] were read on: the revision
    /// and Average Over.
    trend_key: (Revision, u64),
    /// What the take on show's readings rest on.
    rev: Revision,
    /// The last [`Self::whole_take`] and what it was read from.
    whole_take: std::cell::RefCell<Option<(WholeKey, Option<WholeTake>)>>,
    /// The last [`Self::free_table`] and what it was read from.
    free_table: std::cell::RefCell<Option<(FreeKey, Option<guided::FreeTable>)>>,
    /// The next table while listening, read in the background, and what
    /// it is read from.
    free_job: std::cell::RefCell<Option<FreeJob>>,
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
    /// A pane being saved as a picture, and the folder the last one went to.
    export: Option<PaneExport>,
    export_dir: Option<PathBuf>,
    /// The tick and tock sound: how it is drawn, what it was every few
    /// seconds of this session, and one worked out from the file for a
    /// moment that history doesn't cover.
    sound_opts: profiles::Options,
    /// The moment the profile pane shows, for its toolbar's right end.
    sound_note: Option<String>,
    sound_history: Vec<(f64, Profiles)>,
    /// The profiles worked out from the recording at a moment, at the
    /// settings revision they were read with.
    sound_at: Option<(f64, u64, Profiles)>,
    /// Keeping the session as its analysis alone, while that is worked out.
    analysis_save: Option<AnalysisSave>,
    /// Saving the session's sound, while the copy is written.
    saving: Option<Saving>,
    /// The take on show was kept as its analysis alone, without its sound.
    no_sound: bool,
    /// Where this session's analysis was last kept without its sound, and
    /// the seconds of sound it covers. The sound itself stays unsaved.
    analysis_kept_to: Option<(PathBuf, f64)>,
    /// The jumps in the live sound's beats as of the last pass, to see a
    /// new one.
    jumps_seen: Vec<f64>,
    /// Masks on the recording on show let go by the answer to the ask
    /// before listening, so starting doesn't ask again.
    masks_let_go: bool,
    /// The Positions Test to begin once the ask before listening is
    /// answered: just after winding (`None`) or the 24 hour half.
    guided_after_ask: Option<Option<guided::FullWind>>,
    sound_job: Option<(f64, u64, std::sync::mpsc::Receiver<Profiles>)>,
    /// Offer every input the system lists, not just one per microphone.
    all_inputs: bool,
    /// When the microphone was opened and when it last sent audio.
    opened_at: Instant,
    audio_at: Option<Instant>,
    /// Core's gap rule over a live input's blocks: a break where the
    /// computer slept or the input stalled.
    gaps: Option<GapWatch>,
    /// When the sound was last taken in, to tell when nothing ran at all
    /// (the computer asleep, a dialog open) from an input gone quiet.
    polled_at: Instant,
}

impl TimegrapherApp {
    pub fn new(file: Option<PathBuf>, saved: Option<Settings>) -> Self {
        // No device is touched at launch: see `devices_listed`.
        let mut app = Self::with_devices(Vec::new(), file);
        app.devices_listed = false;
        app.cal = calibre_ui::Calibres::open(timegrapher_core::calibre_store::default_path());
        if let Some(s) = saved {
            app.apply_settings(s);
        }
        app.at_launch();
        app.crash = applog::dir().and_then(|d| applog::unseen_crashes(&d).pop());
        app.kept = kept_sounds(&scratch_dir());
        app.read_library();
        app.check_pending = app.check_at_start;
        app
    }

    /// Once the microphone is set up the app opens listening; the first
    /// time it asks which microphone first. A recording asked for opens
    /// instead.
    fn at_launch(&mut self) {
        if self.input != Input::File {
            self.listen_pending = self.mic_set_up && self.listen_at_open;
            self.setup_sheet = !self.mic_set_up && !self.setup_declined;
        }
    }

    /// The views and choices kept from the last time the app ran.
    pub fn settings(&self) -> Settings {
        Settings {
            device: self.device.clone(),
            average_s: self.average_s,
            strip: self.live_view(),
            rate_line: self.rate_line,
            rate_guides: self.rate_guides,
            overlays: self.overlays,
            follow: self.follow,
            chart_series: self.chart_series,
            dist_series: self.dist_series,
            hist_cumulative: self.hist_cumulative,
            hist_readings: self.hist_readings,
            sound: self.sound_opts,
            folded: self.folded.iter().cloned().collect(),
            theme: self.theme,
            steady_series: RATE_ONLY,
            steady_shown: self.steady_series.iter().map(|(v, s)| (*v, *s)).collect(),
            periods_db: self.periods_db,
            fft_timing: self.fft_timing,
            maps_db: self.maps_db,
            show_wheels: self.show_wheels,
            steady_info: self.steady_info,
            positions_settle_s: self.guided_settle_s,
            positions_measure_s: self.guided_measure_s,
            scope: self.scope_view,
            panes: Some(self.panes.clone()),
            panes_dragged: self.panes_dragged.iter().copied().collect(),
            layout: LAYOUT,
            // Drop legacy entries only after their user-catalogue save succeeds.
            custom_calibres: self.cal.pending_legacy().to_vec(),
            recent_calibres: self.cal.recent.clone(),
            short_added: self.cal.short_added.clone(),
            short_removed: self.cal.short_removed.clone(),
            check_updates: self.check_at_start,
            keep_awake: self.keep_awake,
            listen_at_open: self.listen_at_open,
            mic_set_up: self.mic_set_up,
            setup_declined: self.setup_declined,
            show_readings: self.show_readings,
            library: self.library_dir.clone(),
            settings_section: self.settings_last.key().into(),
            agc_on: self.agc_on.clone(),
            watch_list_shows: self.watch_list_shows.clone(),
        }
    }

    /// Put back the views and choices from the last time. A pane layout
    /// from another version of the app, missing a pane or holding one twice,
    /// is set aside for the starting one.
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    pub fn apply_settings(&mut self, s: Settings) {
        let Settings {
            device,
            average_s,
            strip,
            rate_line,
            rate_guides,
            overlays,
            follow,
            chart_series,
            dist_series,
            hist_cumulative,
            hist_readings,
            sound,
            folded,
            theme,
            steady_series,
            steady_shown,
            periods_db,
            fft_timing,
            maps_db,
            show_wheels,
            steady_info,
            positions_settle_s,
            positions_measure_s,
            scope,
            panes,
            panes_dragged,
            layout,
            custom_calibres,
            recent_calibres,
            short_added,
            short_removed,
            check_updates,
            keep_awake,
            listen_at_open,
            mic_set_up,
            setup_declined,
            show_readings,
            library,
            settings_section,
            agc_on,
            watch_list_shows,
        } = s;
        self.watch_list_shows = watch_list_shows;
        self.settings_last = Tab::from_key(&settings_section);
        self.listen_at_open = listen_at_open;
        self.mic_set_up = mic_set_up;
        self.setup_declined = setup_declined;
        self.agc_on = agc_on;
        // A layout from before the panes moved starts afresh.
        let panes = panes.filter(|_| layout >= LAYOUT);
        self.library_dir = library;
        self.check_at_start = check_updates;
        self.keep_awake = keep_awake;
        self.show_readings = show_readings;
        // One Average Over for every pane; a pane's own time, saved by an
        // older app, is read and left out.
        if AVERAGES.contains(&average_s) {
            self.set_average(average_s);
        }
        self.cal.migrate(custom_calibres);
        self.cal
            .restore(recent_calibres, short_added, short_removed);
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
        // One left on is kept: a pane with none shown would be empty.
        let some = |s: [bool; 3]| if s.contains(&true) { s } else { RATE_ONLY };
        self.chart_series = some(chart_series);
        self.dist_series = some(dist_series);
        self.hist_cumulative = hist_cumulative;
        self.hist_readings = hist_readings;
        self.sound_opts = sound;
        self.folded = folded.into_iter().collect();
        self.theme = theme;
        self.periods_db = periods_db;
        self.fft_timing = fft_timing;
        self.maps_db = maps_db;
        self.show_wheels = show_wheels;
        self.steady_info = steady_info;
        self.guided_settle_s =
            positions_settle_s.clamp(*guided::SETTLE_RANGE.start(), *guided::SETTLE_RANGE.end());
        self.session.set_settle_s(self.guided_settle_s);
        self.guided_measure_s =
            positions_measure_s.clamp(*guided::MEASURE_RANGE.start(), *guided::MEASURE_RANGE.end());
        // Each pane's series, at least one shown; a pane not set yet takes
        // the choice the one Steadiness pane had, unless that was every
        // series, which older apps wrote whatever was chosen: those panes
        // show Rate alone, as at first.
        let every = Some(steady_series).filter(|s| *s != [true; 3]);
        self.steady_series = steady_shown
            .into_iter()
            .chain(
                Pane::STEADY
                    .iter()
                    .filter_map(|p| Some((p.steady_view()?, every?))),
            )
            .filter(|(_, s)| s.contains(&true))
            .fold(Default::default(), |mut m, (v, s)| {
                m.entry(v).or_insert(s);
                m
            });
        self.scope_view = scope;
        self.panes = match panes.map(|mut p| {
            // Watches is a view of the window now, not a pane.
            if let Some(w) = p.tiles.find_pane(&Pane::Watch) {
                p.remove_recursively(w);
            }
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
            if p.tiles.find_pane(&Pane::PowerReserve).is_none() {
                add_missing(&mut p, Pane::PowerReserve);
                set_pane_visible(&mut p.tiles, Pane::PowerReserve, false);
            }
            p
        }) {
            Some(p) if layout_is_whole(&p) => {
                self.panes_dragged = panes_dragged.into_iter().collect();
                p
            }
            _ => default_layout(self.strip.horizontal),
        };
    }

    /// The app with a given device list. Tests pass an empty one: listing
    /// devices from several test threads at once crashes on Windows.
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn with_devices(devices: Vec<InputDevice>, file: Option<PathBuf>) -> Self {
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
            kept: Vec::new(),
            kept_delete_ask: None,
            report: ReportDialog::default(),
            updates: Default::default(),
            guided: None,
            reserve: None,
            reserve_keep_sound: true,
            reserve_read_s: 0.0,
            reserve_hours_s: f64::NEG_INFINITY,
            guided_unused: Vec::new(),
            guided_measured: Vec::new(),
            guided_earlier: None,
            guided_choices: Vec::new(),
            guided_earlier_file: false,
            guided_earlier_for: None,
            guided_after: false,
            test_ready: None,
            stop_test_ask: false,
            guided_settle_s: guided::SETTLE_S,
            guided_measure_s: guided::MEASURE_S,
            check_at_start: true,
            keep_awake: true,
            listen_at_open: true,
            mic_set_up: false,
            setup_declined: false,
            setup_sheet: false,
            listen_pending: false,
            setup_tried: false,
            awake_fault: None,
            input_fault: None,
            pump_fault: None,
            check_pending: false,
            average_s: 10.0,
            watch: String::new(),
            calibre: String::new(),
            cal: calibre_ui::Calibres::in_memory(),
            settings_tab: None,
            settings_last: Tab::General,
            kept_clocks: Ok(clockstore::ClockStore::default()),
            forget_ask: None,
            library_dir: None,
            library: Ok(None),
            library_move_ask: None,
            library_moving: None,
            view: View::Bench,
            watch_search_focus: false,
            new_watch_on_mic: false,
            watch_open: None,
            unsorted_open: false,
            list_for: None,
            quick_defaults: false,
            quick_listen: false,
            watch_search: String::new(),
            watch_sort: watches::Sort::default(),
            watch_list_shows: None,
            watches_wide: true,
            watch_form: None,
            keep_after_form: false,
            test_keep: None,
            results_rects: [None; 2],
            positions_take: false,
            position_card_title: "Current Position",
            take_ask: None,
            keep_back: None,
            take_keep: None,
            take_readings: watches::TakeReadings::default(),
            thumbs: Default::default(),
            strip_points: None,
            masked: Vec::new(),
            opened_masks: None,
            masking: false,
            opened_positions: None,
            carried_positions: None,
            pending_recorded: None,
            take_clock: None,
            take_clock_source: None,
            take_breaks: None,
            take_unlisted: None,
            brush: None,
            confirm: None,
            remove_masks_asked: false,
            open_ask: None,
            opened_path: String::new(),
            take_picked: None,
            mic_setup: None,
            open_note: None,
            may_close: false,
            save_dir: home.join("Timegrapher recordings").display().to_string(),
            strip: StripView {
                half_width_ms: 10.0,
                span_s: 30.0,
                horizontal: true,
            },
            follow: false,
            rate_line: true,
            rate_guides: true,
            overlays: [true, false, false],
            chart_series: RATE_ONLY,
            dist_series: RATE_ONLY,
            charts_drawn: [true; 3],
            pane_need: std::collections::HashMap::new(),
            panes_dragged: std::collections::HashSet::new(),
            panes_short: std::collections::HashSet::new(),
            pane_top: 0.0,
            hist_cumulative: false,
            steady_series: Default::default(),
            periods_db: false,
            fft_timing: false,
            maps_db: false,
            show_wheels: true,
            steady_info: false,
            scope_view: scope::View::default(),
            scope_audio: None,
            scope_traces: std::collections::VecDeque::new(),
            scope_job: None,
            steady: None,
            steady_job: None,
            steady_work: (Default::default(), std::time::Instant::now()),
            hist_readings: true,
            rate_2s: RateTwoS::default(),
            cursor_t: None,
            cursor_next: None,
            folded: Default::default(),
            theme: egui::ThemePreference::System,
            strip_end: None,
            panes: default_layout(true),
            show_readings: true,
            charts_to_live: false,
            charts_to_live_now: false,
            chart_average_drawn: Default::default(),
            capture: None,
            live: None,
            source_label: String::new(),
            batch: None,
            listen_tap: None,
            anchor: None,
            reread_with: None,
            live_strip: None,
            strip_fit: false,
            live_fit: false,
            trend: Vec::new(),
            trend_key: (Revision::default(), 10.0f64.to_bits()),
            rev: Revision::default(),
            whole_take: Default::default(),
            free_table: Default::default(),
            free_job: Default::default(),
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
            agc_on: Vec::new(),
            agc_read: None,
            message: None,
            export: None,
            export_dir: None,
            sound_opts: profiles::Options::default(),
            sound_note: None,
            sound_history: Vec::new(),
            sound_at: None,
            analysis_save: None,
            saving: None,
            no_sound: false,
            analysis_kept_to: None,
            jumps_seen: Vec::new(),
            masks_let_go: false,
            guided_after_ask: None,
            sound_job: None,
            all_inputs: false,
            opened_at: Instant::now(),
            audio_at: None,
            gaps: None,
            polled_at: Instant::now(),
        };
        if let Some(f) = file {
            app.start_batch(&f);
        }
        app
    }

    pub(crate) fn running(&self) -> bool {
        self.capture.is_some()
    }

    /// Keep listening while the window isn't drawing: save and read the
    /// sound that has come in. Called from [`crate::pump`]'s thread.
    pub(crate) fn pump(&mut self) -> usize {
        if self.capture.is_none() {
            return 0;
        }
        self.poll_for(PUMP_BUDGET)
    }

    /// Whether to hold the computer awake now: listening to the
    /// microphone, with Keep Awake on.
    pub(crate) fn wants_awake(&self) -> bool {
        (self.keep_awake || self.reserve_running()) && self.running() && self.session.mic()
    }

    /// Seconds of sound heard so far in this session, for the log.
    pub(crate) fn heard_s(&self) -> f64 {
        self.live.as_ref().map_or(0.0, |l| l.duration_s())
    }

    /// Work the charts out again over the whole session, for a new
    /// averaging time or lift angle.
    fn rebuild_trend(&mut self) {
        self.strip_points = None;
        self.trend = self.context_trend(self.average_s);
        self.trend_key = (self.rev, self.average_s.to_bits());
    }

    /// Read the run of readings again when what it rests on changed
    /// since: the take's settings, clock, breaks or masks, or Average Over.
    fn refresh_trend(&mut self) {
        if self.trend_key != (self.rev, self.average_s.to_bits()) {
            self.rebuild_trend();
        }
    }

    fn context_trend(&self, seconds: f64) -> Vec<TrendPoint> {
        let mut points = self
            .live
            .as_ref()
            .map_or_else(Vec::new, |l| trend_over(l, seconds));
        points.retain(|p| self.session.replay_reading_allowed(p.t));
        points
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

    /// The take in the position the watch is in now, once it is stopped:
    /// its settled stretches, at whatever length session reads, read as
    /// `timegrapher session` reads a recording (core
    /// `session::measure_masked`), the time between them masked, and the
    /// seconds they cover. A take opened from disk is read to its end, as
    /// session reads it; one just listened to, to its last reading with a
    /// watch heard.
    fn whole_take(&self) -> Option<WholeTake> {
        if self.running() {
            return None;
        }
        let live = self.live.as_ref()?;
        // To the take's end, as session reads it, not its last beat.
        let opened = self.take_breaks.is_some();
        let end = if opened {
            live.duration_s()
        } else {
            self.last_heard_s().min(live.duration_s())
        };
        let spans: Vec<(f64, f64)> = self
            .current_spans()
            .into_iter()
            .map(|(a, b)| (a.max(0.0), b.min(end)))
            .filter(|(a, b)| a < b)
            .collect();
        let key: WholeKey = (
            live.beats().len(),
            spans.clone(),
            self.masked.clone(),
            self.rev,
        );
        if let Some((k, w)) = self.whole_take.borrow().as_ref() {
            if *k == key {
                return w.clone();
            }
        }
        let read = || {
            let span: f64 = spans.iter().map(|(a, b)| b - a).sum();
            let (&(from, _), &(_, to)) = (spans.first()?, spans.last()?);
            let mut log = steady::log_of(live)?;
            let clock = self.reading_clock(log.duration_s);
            // The cuts the app adds (a settling's end, a move, a mask's ends)
            // at the reading's or a mask's edges change nothing it reads, and
            // session has none there (#682).
            let edge =
                |j: f64| j <= from || j >= to || self.masked.iter().any(|m| j == m.0 || j == m.1);
            log.jumps.retain(|&j| !edge(j));
            let breaks = self.reading_breaks(&log);
            // The time between the stretches, another position or its
            // settling, is left out as a mask is.
            let mut masked = self.masked.clone();
            masked.extend(spans.windows(2).map(|w| (w[0].1, w[1].0)));
            let mut m = timegrapher_core::session::measure_masked(
                &log,
                clock.as_ref(),
                &breaks,
                &masked,
                from,
                to,
            );
            m.take_doubts(self.lift_known(), self.take_unlisted);
            Some(WholeTake { m, span })
        };
        let w = read();
        *self.whole_take.borrow_mut() = Some((key, w.clone()));
        w
    }

    /// The Positions Test table of the take on show or under way, when no
    /// guided test is: each position's settled stretches read by core
    /// `session::positions_table`. While listening it is read afresh every
    /// second, in the background, the last table showing until the next is
    /// ready; a take on show is read at once.
    fn free_table(&self, ctx: &egui::Context) -> Option<guided::FreeTable> {
        let stretches: Vec<(core_session::Position, f64, f64)> = guided::ORDER
            .iter()
            .filter_map(|&p| Some((p, core_session::Position::parse(p.code())?)))
            .flat_map(|(p, sp)| {
                self.session
                    .settled_spans(self.position, p)
                    .into_iter()
                    .map(move |(a, b)| (sp, a, b))
            })
            .collect();
        self.table_of(ctx, stretches)
    }

    /// The guided test's figures for the position being measured, so far:
    /// its measuring up to now read as [`Self::free_table`] reads a
    /// position, every second. None until it has 10 s.
    fn guided_live(&self, ctx: &egui::Context) -> Option<guided::FreeCell> {
        let t = self.guided.as_ref()?;
        let now = self.live.as_ref()?.duration_s();
        let &(from, to) = t.measured(now).get(t.stretches().len())?;
        let p = core_session::Position::parse(t.position().code())?;
        let table = self.table_of(ctx, vec![(p, from, to)])?;
        let label = guided::position_label(p);
        let row = table.rows.into_iter().find(|r| r.0 == label)?;
        matches!(row.2, guided::FreeCell::Read(..)).then_some(row.2)
    }

    /// A Positions Test table of `stretches` (position, start, end), each
    /// cut at the take's end so far: read afresh every second, in the
    /// background, while listening; at once for a take on show.
    fn table_of(
        &self,
        ctx: &egui::Context,
        stretches: Vec<(core_session::Position, f64, f64)>,
    ) -> Option<guided::FreeTable> {
        let live = self.live.as_ref()?;
        let latest = self.latest_s();
        // A take on show reads to the end of its sound, as `timegrapher
        // session` reads its last position.
        let end = if self.running() {
            (latest / FREE_TABLE_STEP_S).floor() * FREE_TABLE_STEP_S
        } else {
            latest.max(live.duration_s())
        };
        let stretches: Vec<(core_session::Position, f64, f64)> = stretches
            .into_iter()
            .map(|(p, a, b)| (p, a.max(0.0), b.min(end)))
            .filter(|s| s.1 < s.2)
            .collect();
        let masked = core_session::masked_spans(
            &self.masked.iter().map(|&(a, b)| [a, b]).collect::<Vec<_>>(),
        );
        let beats = if self.running() {
            (end / FREE_TABLE_STEP_S) as usize
        } else {
            live.beats().len()
        };
        let key: FreeKey = (beats, stretches.clone(), masked.clone(), self.rev);
        let mut job = self.free_job.borrow_mut();
        // A table being read on what the readings rested on before is let go.
        if job.as_ref().is_some_and(|j| j.key.3 != self.rev) {
            *job = None;
        }
        if let Some(j) = job.as_ref() {
            match j.rx.try_recv() {
                Ok(t) => {
                    *self.free_table.borrow_mut() = Some((j.key.clone(), t));
                    *job = None;
                }
                Err(TryRecvError::Disconnected) => *job = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some((k, t)) = self.free_table.borrow().as_ref() {
            if *k == key || self.running() && job.is_some() {
                return t.clone();
            }
        }
        let (take_clock, opened, ppm) = (
            self.take_clock.clone(),
            self.take_breaks.clone(),
            self.clock.as_ref().map(|c| c.ppm),
        );
        let read = move |log: BeatLog| {
            let clock = reading_clock(take_clock.as_ref(), opened.is_some(), ppm, log.duration_s);
            let breaks = steady::reading_breaks(opened.unwrap_or_default(), &log);
            let t = core_session::positions_table(
                &log,
                clock.as_ref(),
                &breaks,
                &masked,
                &stretches,
                core_session::POSITION_MIN_S,
            );
            guided::free_table(&t)
        };
        let log = steady::log_of(live);
        let shown = self.free_table.borrow().as_ref().map(|(_, t)| t.clone());
        if let (true, Some(shown), Some(log)) = (self.running(), shown, log.clone()) {
            // Listening, with a table on show: read the next one in the
            // background and keep this one until it comes.
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Some(read(log)));
                ctx.request_repaint();
            });
            *job = Some(FreeJob { key, rx });
            return shown;
        }
        let t = log.map(read);
        *self.free_table.borrow_mut() = Some((key, t.clone()));
        t
    }

    /// With Fit on, the strip's length is the session so far.
    fn follow_fit(&mut self) {
        if self.strip_fit {
            self.strip_end = None;
            self.strip.set_span(self.latest_s());
        }
    }

    fn reading(&self) -> Option<LiveReading> {
        self.reading_over(self.average_s)
    }

    /// The newest reading, averaged over `average` seconds.
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
        let reading = if !self.running() {
            // Stopped: the last window with a watch heard, so the silence
            // after it is taken off the stand doesn't change the readings.
            live.reading_at(self.latest_s().min(self.last_heard_s()), average)
        } else {
            live.reading(average)
        };
        self.session
            .replay_reading_allowed(reading.time_s)
            .then_some(reading)
    }

    /// The reading at `t` in the session, as the newest is read: none
    /// while the watch settles. The tests read the session at a moment
    /// with it.
    #[cfg(test)]
    fn reading_at_time(&self, t: f64) -> Option<LiveReading> {
        let live = self.live.as_ref()?;
        if self.session.settling_left(t).is_some() {
            return None;
        }
        let reading = live.reading_at(t, self.average_s);
        self.session
            .replay_reading_allowed(reading.time_s)
            .then_some(reading)
    }

    /// Put the beats just before the strip's newest edge on the centre line.
    fn centre(&mut self) {
        let end = self.strip_end_s();
        if let Some(l) = &self.live {
            if let Some(bph) = l.bph() {
                let beats = l.beats();
                let breaks = self.take_breaks.as_deref().unwrap_or_default();
                let hi = beats.partition_point(|b| b.time <= end);
                let clock = Self::rate_clock(&self.take_clock);
                if let Some(a) =
                    Anchor::centre_on_true(&beats[..hi], 3600.0 / bph as f64, 8, breaks, clock)
                {
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

    /// Compare With: the full-wind test the 24 hours after test is set
    /// against, picked each time from the watch's own tests from 12 to 36
    /// hours ago, newest first, or loaded from a file.
    fn compare_with(&mut self, ui: &mut egui::Ui) {
        let shown = match &self.guided_earlier {
            Some(e) if self.guided_earlier_file => {
                format!("{} (from a file)", guided::choice_text(e))
            }
            Some(e) => guided::choice_text(e),
            None if self.guided_choices.is_empty() => "None from 12 to 36 hours ago".to_string(),
            None => "Pick a full-wind test".to_string(),
        };
        let mut picked: Option<usize> = None;
        let mut from_file = false;
        theme::tool_field(
            ui,
            "Compare With",
            &format!(
                "The full-wind test this one is set against: one of the watch's own from {}, \
                 or results saved to a file",
                guided::full_wind_ago()
            ),
            |ui| {
                egui::ComboBox::from_id_salt("compare with")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for (i, f) in self.guided_choices.iter().enumerate() {
                            let on = !self.guided_earlier_file
                                && self.guided_earlier.as_ref() == Some(f);
                            if ui.selectable_label(on, guided::choice_text(f)).clicked() {
                                picked = Some(i);
                            }
                        }
                        if !self.guided_choices.is_empty() {
                            ui.separator();
                        }
                        if ui
                            .selectable_label(false, RichText::new("From a File…").small())
                            .clicked()
                        {
                            from_file = true;
                        }
                    });
            },
        );
        if let Some(i) = picked {
            self.pick_full_wind(i);
        }
        if from_file {
            self.load_earlier();
        }
    }

    /// Set the next test against the `i`th of the watch's full-wind tests
    /// on offer.
    fn pick_full_wind(&mut self, i: usize) {
        if let Some(f) = self.guided_choices.get(i) {
            self.guided_earlier = Some(f.clone());
            self.guided_earlier_file = false;
        }
    }

    /// Load the full-wind test's saved results from a file the user picks,
    /// for the next test to be set against.
    fn load_earlier(&mut self) {
        if let Some(full) = self.load_full_wind() {
            self.guided_earlier = Some(full);
            self.guided_earlier_file = true;
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
        self.read_full_wind(&path)
    }

    /// Read the full-wind test's results saved at `path`; None if they
    /// can't be read, which says so.
    fn read_full_wind(&mut self, path: &Path) -> Option<guided::FullWind> {
        let read = std::fs::read_to_string(path)
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

    /// The microphone picked, as its menu names it.
    fn picked_label(&self) -> String {
        let listed = self.devices_listed;
        self.input_choices()
            .iter()
            .find(|c| Some(&c.id) == self.device.as_ref())
            .map_or_else(
                || match (&self.device, listed) {
                    // Not listed yet: name the one kept from last time, when
                    // its id is a name (ALSA's are; Core Audio's and
                    // WASAPI's are not).
                    (Some(d), false) => match d.strip_prefix("alsa:") {
                        Some(name) => name.into(),
                        None => "The Microphone Used Last".to_string(),
                    },
                    (None, false) => "System Default".to_string(),
                    _ => "Choose a Microphone".to_string(),
                },
                |c| c.label.clone(),
            )
    }

    /// The microphone menu with Rescan after it, `w` wide; a name too long
    /// for it cut in the middle where `middle`, else at its end.
    fn device_menu(&mut self, ui: &mut egui::Ui, w: f32, middle: bool) {
        let choices = self.input_choices();
        let listed = self.devices_listed;
        let sel = self.picked_label();
        let mut opened = false;
        let row = egui::vec2(w, ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            row,
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                // Kept to the pane's width, the name cut short to fit: a
                // combo box in a row is otherwise as wide as its words, and a
                // long name (as macOS gives) widens the whole pane past its
                // card. Rescan is laid first, from the right, so the box takes
                // what is left of the row.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                if ui
                    .add(egui::Button::new("Rescan").wrap_mode(egui::TextWrapMode::Extend))
                    .on_hover_text("Look for microphones again, after plugging one in")
                    .clicked()
                {
                    self.rescan();
                }
                let full = sel.clone();
                let box_w = ui.available_width().max(60.0);
                // Cut in the middle where it can't show whole: similar
                // devices often differ only at the end.
                let font = egui::TextStyle::Button.resolve(ui.style());
                let shown = if !middle {
                    sel.clone()
                } else {
                    theme::middle_cut(
                        ui,
                        &sel,
                        &font,
                        box_w - 2.0 * ui.spacing().icon_width - 12.0,
                    )
                };
                egui::ComboBox::from_id_salt("device")
                    .selected_text(shown)
                    .width(box_w)
                    .show_ui(ui, |ui| {
                        opened = true;
                        if !listed {
                            ui.weak("Looking for microphones…");
                        }
                        for c in &choices {
                            let r =
                                ui.selectable_value(&mut self.device, Some(c.id.clone()), &c.label);
                            if !c.detail.is_empty() {
                                r.on_hover_text(&c.detail);
                            }
                        }
                    })
                    .response
                    .on_hover_text(format!(
                        "{full}\n\nThe microphone to listen to. Show Every Input, below, lists \
                     every device the system offers."
                    ));
            },
        );
        if opened && !listed {
            self.ensure_devices();
            ui.ctx().request_repaint();
        }
    }

    /// A pane's toolbar: its settings in a row and its "?" at the right.
    fn pane_tools(&mut self, ui: &mut egui::Ui, pane: Pane) -> bool {
        let title = pane.title();
        match pane {
            Pane::Watch => theme::pane_toolbar(ui, title, watches::HELP, |ui| {
                self.watches_tools(ui);
            }),
            Pane::Strip => theme::pane_toolbar(ui, title, help::STRIP, |ui| {
                self.strip_settings(ui);
            }),
            Pane::Sound => {
                let note = self.sound_note.clone();
                theme::pane_toolbar_noted(ui, title, help::PROFILE, note.as_deref(), |ui| {
                    self.profile_settings(ui);
                })
            }
            Pane::Scope => theme::pane_toolbar(ui, title, help::SCOPE, |ui| {
                scope::settings(ui, &mut self.scope_view);
                self.export_button(ui, pane);
            }),
            p if p.steady_view().is_some() => {
                let mut info = self.steady_info;
                let mut pin = |ui: &mut egui::Ui| {
                    let accent = theme::pal(ui).accent;
                    if theme::icon_toggle(ui, Icon::Info, info, accent)
                        .on_hover_text(INFO_HINT)
                        .clicked()
                    {
                        info = !info;
                    }
                };
                theme::pane_toolbar_pinned(ui, title, help::STEADINESS, Some(&mut pin), |ui| {
                    self.steady_settings(ui, p);
                    self.export_button(ui, p);
                });
                self.steady_info = info;
            }
            Pane::Isochronism => theme::pane_toolbar(ui, title, &help::ISOCHRONISM, |ui| {
                self.export_button(ui, pane)
            }),
            Pane::PositionsTest => theme::pane_toolbar(ui, title, &help::POSITIONS_TEST, |ui| {
                self.positions_settings(ui);
                self.export_button(ui, pane);
            }),
            Pane::PowerReserve => {
                let help = help::power_reserve(self.position_card_title);
                let help: Vec<&str> = help.iter().map(String::as_str).collect();
                theme::pane_toolbar(ui, title, &help, |ui| {
                    self.reserve_settings(ui);
                })
            }
            Pane::Distributions => theme::pane_toolbar(ui, title, &help::HISTOGRAMS, |ui| {
                steady::series_switches(ui, &mut self.dist_series);
                theme::bar_gap(ui);
                self.histogram_settings(ui);
                self.export_button(ui, pane);
            }),
            Pane::Charts => theme::pane_toolbar(ui, title, help::CHARTS, |ui| {
                steady::series_switches(ui, &mut self.chart_series);
                theme::bar_gap(ui);
                self.export_button(ui, pane);
            }),
            _ => return false,
        }
        true
    }

    /// What the strip shows, for the status line: the take's name, its
    /// watch and its state.
    fn status_words(&self) -> String {
        let file_name = |p: &Path| {
            p.file_name()
                .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into())
        };
        // A take not yet saved or opened, with no watch, is Quick Listen's,
        // as the Calibre card's title says (No Watch (Quick Listen)).
        let file = if let Some(to) = self.session.saved_to() {
            Some(file_name(to))
        } else if self.input == Input::File && !self.opened_path.is_empty() {
            Some(file_name(Path::new(&self.opened_path)))
        } else {
            None
        };
        // The watch the Calibre card names, else the take's own.
        let named = self
            .open_watch()
            .map(|e| e.watch.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| self.watch.clone());
        let quick = self.quick_listen || file.is_none() && named.is_empty();
        let name =
            file.unwrap_or_else(|| if quick { "Quick Listen" } else { "New Take" }.to_string());
        let watch = if !named.is_empty() {
            named.clone()
        } else if quick {
            "No watch".to_string()
        } else {
            "No watch picked".to_string()
        };
        // A test set up or under way is named, with how it stands.
        let test = if let Some(kind) = self.ready_test() {
            Some((kind, self.state_words().to_string()))
        } else if self.reserve_running() {
            Some((NewTake::PowerReserve, self.state_words().to_string()))
        } else if let Some(t) = self.guided.as_ref().filter(|_| self.test_listening()) {
            let of = guided::ORDER.len();
            let step = (t.step + 1).min(of);
            Some((
                NewTake::Positions,
                format!("{}, {step} of {of}", t.position().code()),
            ))
        } else {
            None
        };
        if let Some((kind, state)) = test {
            let name = if kind == NewTake::Positions {
                "Positions Test"
            } else {
                "Power Reserve Test"
            };
            let watch = if named.is_empty() {
                "No watch picked".to_string()
            } else {
                named
            };
            return format!("{name} · {watch} · {state}");
        }
        format!("{name} · {watch} · {}", self.state_words())
    }

    /// Whether the take on show is listening, stopped or opened.
    fn state_words(&self) -> &'static str {
        match self.session.state() {
            session::State::Idle if self.ready_test().is_some() => "Ready, not started",
            session::State::Idle => "Not Started",
            session::State::Listening => "Listening",
            session::State::Paused { .. } => "Stopped",
            // Only tests play a recording through as if live.
            session::State::Replaying => "Listening",
            session::State::ReplayStopped => "Stopped",
            session::State::Analysing => "Opening",
            session::State::Analysed => "Opened",
        }
    }

    /// A button for each pane, its icon pressed while the pane shows.
    /// The icon bar: the pane icons under their groups' names, then the
    /// Settings gear. With `shown` false (the Watches view) the pane icons
    /// and names hide but keep their room, so the gear never moves.
    fn pane_icons(&mut self, ui: &mut egui::Ui, shown: bool) {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.scope(|ui| {
            if !shown {
                ui.set_invisible();
            }
            self.pane_icon_groups(ui);
        });
        ui.add_space(6.0);
        ui.separator();
        self.settings_gear(ui);
    }

    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn pane_icon_groups(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.x = 2.0;
        let pal = theme::pal(ui);
        // The readings' cards, which are no pane, then the core panes,
        // Watches first, the analysis views and the tests, each group under
        // its name.
        if theme::icon_toggle(ui, Icon::Readings, self.show_readings, pal.accent)
            .on_hover_text(
                "Readings: the Position, Listen, Rate, Amplitude, Beat Error and Beat Rate \
                 cards across the top",
            )
            .clicked()
        {
            self.show_readings = !self.show_readings;
        }
        let groups: [(&str, &str, &[Pane]); 3] = [
            ("", "", &Pane::CORE),
            (
                "Analysis",
                "The readings over the take as charts and as distributions, how the rate, \
                 amplitude and beat error each change over the session, from the same tests \
                 the command line runs, and how the rate moves with the amplitude: each \
                 view a pane of its own, so two can sit side by side",
                &Pane::ANALYSIS,
            ),
            (
                "Tests",
                "The Positions Test and the Power Reserve Test, each with its results",
                &Pane::TESTS,
            ),
        ];
        // Where the bar is too narrow for every icon, the largest groups
        // fold into a menu under their name, until the rest fit.
        let small = egui::TextStyle::Small.resolve(ui.style());
        let words = |ui: &egui::Ui, t: &str| {
            ui.fonts_mut(|f| f.layout_no_wrap(t.to_string(), small.clone(), Color32::WHITE))
                .size()
                .x
        };
        let icon_w = 28.0;
        let open_w = |ui: &egui::Ui, name: &str, n: usize| {
            let label = if name.is_empty() {
                0.0
            } else {
                14.0 + words(ui, name) + 4.0
            };
            label + n as f32 * icon_w
        };
        let folded_w = |ui: &egui::Ui, name: &str| 14.0 + words(ui, name) + 28.0;
        let mut folded = [false; 5];
        let width = |folded: &[bool; 5], ui: &egui::Ui| {
            groups
                .iter()
                .zip(folded)
                .map(|((name, _, panes), &f)| {
                    if f {
                        folded_w(ui, name)
                    } else {
                        open_w(ui, name, panes.len())
                    }
                })
                .sum::<f32>()
                + 14.0
                + 2.0 * icon_w
        };
        let room = ui.available_width();
        let mut order: Vec<usize> = (1..groups.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(groups[i].2.len()));
        for i in order {
            if width(&folded, ui) <= room {
                break;
            }
            folded[i] = true;
        }
        for (g, (name, hint, panes)) in groups.into_iter().enumerate() {
            if !name.is_empty() {
                ui.add_space(6.0);
                ui.separator();
            }
            if folded[g] {
                let on = panes
                    .iter()
                    .filter(|&&p| pane_visible(&self.panes.tiles, p))
                    .count();
                let label = if on > 0 {
                    format!("{name} ({on}) ⏷")
                } else {
                    format!("{name} ⏷")
                };
                let button = ui
                    .add(egui::Button::new(RichText::new(label).small()).frame_when_inactive(false))
                    .on_hover_text(format!(
                        "{hint}. Folded into a menu while the window is narrow: {}.",
                        panes
                            .iter()
                            .map(|p| p.title())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                egui::Popup::menu(&button).show(|ui| {
                    for &pane in panes {
                        let mut shown = pane_visible(&self.panes.tiles, pane);
                        if ui
                            .checkbox(&mut shown, pane.title())
                            .on_hover_text(pane.hint())
                            .changed()
                        {
                            self.show_pane(pane, shown);
                        }
                    }
                });
                continue;
            }
            if !name.is_empty() {
                ui.label(RichText::new(name).small().color(pal.text_tertiary))
                    .on_hover_text(hint);
            }
            for &pane in panes {
                let on = pane_visible(&self.panes.tiles, pane);
                let icon = pane_icon(pane);
                if theme::icon_toggle_off(ui, icon, on, pal.accent, pal.text_secondary)
                    .on_hover_text(format!("{}: {}", pane.title(), pane.hint()))
                    .clicked()
                {
                    self.show_pane(pane, !on);
                }
            }
        }
    }

    /// Open the Settings window at `tab`, reading the kept clocks afresh.
    fn open_settings(&mut self, tab: Tab) {
        self.cal.drop_rate_preset();
        self.kept_clocks = clockstore::ClockStore::load();
        self.read_library();
        self.settings_tab = Some(tab);
    }

    // Each part the readings rest on is set here, and only here, so every
    // figure that keys on it is read again (see [`Revision`]).

    /// Show a pane at the front of its tabs, or hide it: what its icon and
    /// its View menu item do.
    fn show_pane(&mut self, pane: Pane, on: bool) {
        if on {
            self.reveal_pane(pane);
        } else {
            self.undrag(pane);
            set_pane_visible(&mut self.panes.tiles, pane, false);
        }
    }

    /// A pane shown or hidden: the rows holding it share their room out
    /// afresh, each pane at its least, rather than as last dragged.
    fn undrag(&mut self, pane: Pane) {
        let mut at = self.panes.tiles.find_pane(&pane);
        while let Some(id) = at {
            self.panes_dragged.remove(&id);
            at = self.panes.tiles.parent_of(id);
        }
    }

    /// Show a pane and bring its tab to the front.
    fn reveal_pane(&mut self, pane: Pane) {
        let was = pane_visible(&self.panes.tiles, pane);
        if !was {
            self.undrag(pane);
        }
        set_pane_visible(&mut self.panes.tiles, pane, true);
        // A test's pane opens at its content's height, no less than a
        // pane's least: the Paper Strip keeps the rest until dragged.
        if pane == Pane::PowerReserve && !was {
            fit_row(&mut self.panes.tiles, pane, MIN_PANE_H);
        }
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

    /// The row under the menus: the Bench | Watches switch at the left, and
    /// in Watches the view's toolbar after it. The same height in both, so
    /// nothing below moves when the view changes.
    fn mode_row(&mut self, ui: &mut egui::Ui) {
        let size = Vec2::new(ui.available_width(), MODE_ROW_H);
        let row = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(size, row, |ui| {
            ui.set_min_size(size);
            let mut view = self.view;
            theme::segmented(ui, &mut view, &VIEWS);
            self.view = view;
            if self.view == View::Watches {
                ui.add_space(theme::GAP);
                ui.vertical(|ui| {
                    self.pane_tools(ui, Pane::Watch);
                });
            }
        });
    }

    /// The Watches view: the watches and their takes, filling the window
    /// below the mode row, as a card.
    fn watches_view(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::CARD_RADIUS), pal.card);
        let inner = rect.shrink2(Vec2::new(8.0, 6.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            self.watch_pane(ui);
        });
    }

    fn main_view(&mut self, ui: &mut egui::Ui) {
        // Nothing appears above the readings: notices are in the status
        // bar, so the window never moves under the pointer.
        // The readings, and on them what a take needs, unless hidden.
        if self.show_readings {
            let out = self.readouts(ui);
            self.card_controls(ui, &out);
            ui.add_space(theme::GAP - ui.spacing().item_spacing.y);
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
        let need = self.pane_need.clone();
        keep_room(
            &mut panes.tiles,
            |p| need.get(&p).copied(),
            &self.panes_dragged,
        );
        let before = shares_of(&panes.tiles);
        panes.ui(
            &mut PaneBehavior {
                app: self,
                row: None,
            },
            ui,
        );
        // Shares changed while drawn were dragged by hand: they stand.
        for (id, s) in shares_of(&panes.tiles) {
            if before.get(&id).is_some_and(|b| *b != s) {
                self.panes_dragged.insert(id);
            }
        }
        self.panes = panes;
    }

    /// Whether `pane`'s plot would be under [`SHORT_PLOT_H`] tall here
    /// (with a little give, so it doesn't flicker at the edge): then the
    /// pane says so in one line in its place.
    fn too_short(&mut self, ui: &mut egui::Ui, pane: Pane) -> bool {
        let below = match pane {
            // Their own least is kept, or they hold no plot.
            Pane::Watch | Pane::Strip | Pane::PositionsTest | Pane::PowerReserve => return false,
            // A map's key sits under its picture.
            Pane::SteadyStft | Pane::SteadyWavelet => crate::tfmap::KEY_H,
            _ => 0.0,
        };
        let was = self.panes_short.contains(&pane);
        let least = SHORT_PLOT_H + below + if was { SHORT_PLOT_GIVE } else { 0.0 };
        if ui.available_height() >= least {
            self.panes_short.remove(&pane);
            return false;
        }
        self.panes_short.insert(pane);
        let pal = theme::pal(ui);
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new(no_room(
                    pane.title(),
                    "Drag the divider up or make the window taller.",
                ))
                .color(pal.text_secondary),
            );
        });
        true
    }

    /// Keep the height `pane` needs: what is above its plots, one plot
    /// `least` tall, the line naming those with no room, and the margin.
    fn note_need(&mut self, ui: &egui::Ui, pane: Pane, least: f32) {
        let above = ui.cursor().top() - self.pane_top;
        self.pane_need
            .insert(pane, above + least + STACKED_NOTE_H + 6.0);
    }
}

/// The usual spacing of points sorted by x: the median gap.
/// Points sorted by x in runs, split wherever one is missing: a gap in
/// them ([`strip::gap_before`]), not a change in their spacing.
fn segments(points: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    for (i, &p) in points.iter().enumerate() {
        if i == 0 || strip::gap_before(points, i) {
            out.push(Vec::new());
        }
        if let Some(seg) = out.last_mut() {
            seg.push(p);
        }
    }
    out
}

/// The value of the point nearest `x` in points sorted by x: none in a gap
/// in the line ([`strip::gap_before`]), or further past either end than
/// the step beside it.
fn value_at(points: &[[f64; 2]], x: f64) -> Option<f64> {
    let n = points.len();
    let i = points.partition_point(|p| p[0] < x);
    let step = |j: usize| points[j][0] - points[j - 1][0];
    let near = match i {
        _ if n == 0 => return None,
        0 => (n < 2 || points[0][0] - x <= step(1)).then_some(0)?,
        _ if i == n => (n < 2 || x - points[n - 1][0] <= step(n - 1)).then_some(n - 1)?,
        _ if points[i][0] == x => i,
        _ if strip::gap_before(points, i) => return None,
        _ if x - points[i - 1][0] < points[i][0] - x => i - 1,
        _ => i,
    };
    Some(points[near][1])
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
fn chart_time_label(t: f64, step: f64, r: &std::ops::RangeInclusive<f64>) -> String {
    // Nothing before the session starts: a time there read as a second
    // "0:00".
    if t < -1e-6 {
        return String::new();
    }
    // A long time ("1:30:00") at the right end would be cut.
    // Marks less than a second apart name their tenths, so no two read
    // the same.
    let label = if step < 1.0 - 1e-9 {
        let fine = strip::fmt_time_fine(t);
        fine[..fine.len() - 1].to_string()
    } else {
        length_text(t)
    };
    let frac = if label.len() > 5 { 0.03 } else { 0.0 };
    theme::clear_of_right_edge(label, t, r, frac)
}

/// The names in a chart's key and their colours, when it has more than one
/// line.
/// A second colour is drawn as a dash of the two, as the line is.
fn chart_key(
    shown: [bool; 3],
    pal: &theme::Palette,
) -> Vec<(&'static str, Color32, Option<Color32>)> {
    let mut key = Vec::new();
    if shown[1] {
        key.extend([
            ("Average Amplitude", pal.trace_amplitude, None),
            ("From Tick", pal.trace_amplitude, Some(pal.tick)),
            ("From Tock", pal.trace_amplitude, Some(pal.tock)),
        ]);
    }
    if shown[2] {
        key.extend([
            ("From the Unlock", pal.trace_beat_error, None),
            (
                "From the Drop",
                pal.trace_beat_error.gamma_multiply(0.6),
                None,
            ),
        ]);
    }
    key
}

/// A chart's key, right to left from its top right corner: each name
/// after a dot, or a short dash for a line drawn dashed over another
/// colour.
pub fn paint_key(
    painter: &egui::Painter,
    frame: egui::Rect,
    key: &[(&str, Color32, Option<Color32>)],
    ui: &egui::Ui,
    pal: &theme::Palette,
) {
    if key.is_empty() {
        return;
    }
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galleys: Vec<_> = key
        .iter()
        .map(|&(name, _, _)| painter.layout_no_wrap(name.into(), font.clone(), pal.text_secondary))
        .collect();
    let mark = |under: Option<Color32>| if under.is_some() { 14.0 } else { 6.0 };
    let (gap, space) = (5.0, 12.0);
    let width: f32 = key
        .iter()
        .zip(&galleys)
        .map(|(&(_, _, under), g)| mark(under) + gap + g.size().x)
        .sum::<f32>()
        + space * (key.len() as f32 - 1.0);
    let h = galleys.iter().map(|g| g.size().y).fold(0.0, f32::max);
    let right = frame.right() - 6.0;
    let top = frame.top() + 3.0;
    let y = top + h / 2.0;
    // On a backing of the card's colour, so the lines under it don't
    // muddle it.
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(right - width - 4.0, top - 1.0),
            egui::pos2(right + 2.0, top + h + 1.0),
        ),
        3.0,
        pal.card,
    );
    let mut x = right - width;
    for (&(_, color, under), g) in key.iter().zip(galleys) {
        match under {
            Some(u) => {
                painter.hline(x..=x + 14.0, y, egui::Stroke::new(2.0_f32, u));
                for i in 0..3 {
                    let a = x + i as f32 * 5.0;
                    painter.hline(a..=a + 3.0, y, egui::Stroke::new(2.0_f32, color));
                }
            }
            None => {
                painter.circle_filled(egui::pos2(x + 3.0, y), 3.0, color);
            }
        }
        x += mark(under) + gap;
        let w = g.size().x;
        painter.galley(egui::pos2(x, top), g, pal.text_secondary);
        x += w + space;
    }
}

/// One plot for each reading `shown`, one above the other, sharing the
/// height left in `ui` evenly. A pane too short for them all draws the
/// first ones at a readable height and names the rest, rather than
/// squeezing every plot flat or spilling out of the pane.
fn stacked(
    ui: &mut egui::Ui,
    shown: [bool; 3],
    least: f32,
    mut draw: impl FnMut(&mut egui::Ui, SeriesKind),
) -> [bool; 3] {
    let kinds: Vec<SeriesKind> = steady::SERIES
        .into_iter()
        .zip(shown)
        .filter_map(|(k, on)| on.then_some(k))
        .collect();
    let (gap, note) = (6.0, STACKED_NOTE_H);
    let room = ui.available_height();
    let fits = |n: usize| n as f32 * least + (n as f32 - 1.0) * gap + note;
    let mut n = kinds.len();
    while n > 1 && fits(n) > room {
        n -= 1;
    }
    let left_out = &kinds[n..];
    let room = if left_out.is_empty() {
        room
    } else {
        room - note
    };
    let h = ((room - gap * (n as f32 - 1.0)) / n.max(1) as f32).max(24.0);
    let w = ui.available_width();
    for (i, &k) in kinds[..n].iter().enumerate() {
        if i > 0 {
            ui.add_space(gap);
        }
        ui.allocate_ui_with_layout(
            Vec2::new(w, h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_size(Vec2::new(w, h));
                ui.set_max_height(h);
                draw(ui, k);
            },
        );
    }
    if !left_out.is_empty() {
        let names: Vec<&str> = left_out.iter().map(|&k| steady::title(k)).collect();
        // The switches are pictures: say which picture turns which off.
        let switches: Vec<String> = kinds[..n]
            .iter()
            .map(|&k| {
                let picture = match k {
                    SeriesKind::Rate => "the hare",
                    SeriesKind::Amplitude => "the swing",
                    SeriesKind::BeatError => "the seesaw",
                };
                format!("{picture} ({})", steady::title(k))
            })
            .collect();
        let line = no_room(
            &names.join(" and "),
            &format!(
                "Make the pane taller, or turn off {}.",
                switches.join(" or ")
            ),
        );
        let pal = theme::pal(ui);
        ui.add(egui::Label::new(RichText::new(line).small().color(pal.text_secondary)).truncate());
    }
    steady::SERIES.map(|k| kinds[..n].contains(&k))
}

/// The least height a chart is drawn at in the Charts pane, its time axis
/// included.
const STACKED_MIN_H: f32 = 70.0;
/// The least height a distribution is drawn at, its summary line and
/// axis included: about 60 pt of plot.
const DIST_MIN_H: f32 = 100.0;
/// The least height of a pane's plot; under it the pane says it has no
/// room instead of drawing a sliver.
const SHORT_PLOT_H: f32 = 48.0;
/// The give above `SHORT_PLOT_H` a pane already saying it has no room
/// needs before it draws its plot again, so a height on the edge doesn't
/// flicker between the two.
const SHORT_PLOT_GIVE: f32 = 2.0;

/// The line in place of plots with no room: what is not shown, then the
/// remedy.
fn no_room(what: &str, remedy: &str) -> String {
    format!("{what} not shown: no room. {remedy}")
}
/// The line under stacked plots naming those with no room.
const STACKED_NOTE_H: f32 = 18.0;
/// The least height of the Paper Strip under its toolbar: its lane, at
/// least 100 pt of dots and the time axis, the amplitude scale's labels
/// apart. The strip keeps it whatever pane is opened under it; only a
/// divider dragged by hand takes it lower.
const STRIP_MIN_H: f32 = STRIP_MIN_PLOT_H + 39.0;
/// The least height of the Paper Strip's dots.
const STRIP_MIN_PLOT_H: f32 = 100.0;

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

/// From the lowest value to the highest, padded a little, so every point
/// drawn is inside the chart: core's scale, which the MCP view uses too.
fn data_range(points: &[[f64; 2]]) -> Option<(f64, f64)> {
    let ys: Vec<f64> = points.iter().map(|p| p[1]).collect();
    timegrapher_core::chart_scale::data_range(&ys)
}

/// The least a pane is squeezed to, points.
const PANE_MIN_H: f32 = 120.0;

/// What the settling after a move is, wherever it is shown, for a
/// settling of `s` seconds: the Positions Test pane's Settle, one for
/// every change of position.
fn settle_hint(s: f64) -> String {
    format!(
        "For the first {s:.0} seconds after the watch is put in a new position (the \
         Positions Test pane's Settle) it settles: the hand leaves the stand and the \
         balance steadies. Those beats are drawn faintly on the Paper Strip and left out of \
         the readings, the charts' points, the Distributions and the Steadiness tests. The \
         sound is still kept, and a saved take's positions start after it."
    )
}

/// The beats the strip draws: `live`'s, or the take's numbered on its
/// audio seconds (`audio`) while they are the same beats.
/// The clock readings are put on, for a log `duration_s` long: for a
/// take opened (`opened`), its own clock as core read it, or none; for a
/// session heard here, the clock kept for the input (`ppm` slow).
fn reading_clock(
    take_clock: Option<&timegrapher_core::clock::ClockFit>,
    opened: bool,
    ppm: Option<f64>,
    duration_s: f64,
) -> Option<timegrapher_core::clock::ClockFit> {
    if opened {
        take_clock.cloned()
    } else {
        // The kept correction, as core puts a take with no clock log on it.
        timegrapher_core::take::clock(None, 0, 0, None, ppm, duration_s)
            .ok()
            .flatten()
    }
}

/// What core opens for the take at `path`, a recording or a take's
/// folder: the folder its session file is read from, the session file
/// (an empty one without), and one entry for the whole recording, on the
/// clock log the first entry the session file gives it names. The entry
/// leaves its masks to the window, which keeps them open to change.
fn take_entry(
    path: &Path,
    manifest: Option<&(Manifest, Vec<RecordingEntry>)>,
) -> (PathBuf, Manifest, RecordingEntry) {
    let (base, file) = if path.is_dir() {
        (path.to_path_buf(), ".".to_string())
    } else {
        (
            path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            file_name(path),
        )
    };
    let (m, first) = match manifest {
        Some((m, e)) => (m.clone(), e.first()),
        None => (Manifest::default(), None),
    };
    let entry = RecordingEntry {
        file,
        clock: first.and_then(|e| e.clock.clone()),
        ..Default::default()
    };
    (base, m, entry)
}

/// Open the take `opening` ([`take_entry`]) with core's `take::open_entry`
/// on a thread of its own, sending what it read to `tx`: `progress` holds
/// the seconds of sound read (for a take kept as its analysis alone, its
/// length `kept_s` once read) and setting `cancel` stops it, sending
/// nothing.
fn open_in_background(
    opening: (PathBuf, Manifest, RecordingEntry),
    settings: timegrapher_core::take::EntrySettings,
    tx: std::sync::mpsc::Sender<Result<Option<Opened>, String>>,
    progress: std::sync::Arc<std::sync::atomic::AtomicU64>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    kept_s: Option<f64>,
) {
    use std::sync::atomic::Ordering::Relaxed;
    use timegrapher_core::take;
    std::thread::spawn(move || {
        let (base, m, e) = opening;
        // A cancel unwinds out of the analysis from its progress report
        // (resume_unwind runs no panic hook, so nothing is printed).
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            take::open_entry(
                &base,
                &m,
                &e,
                &settings,
                take::usb,
                |first| take::correction_for(&m, first, None, None),
                |s, _| {
                    progress.store(s.to_bits(), Relaxed);
                    if cancel.load(Relaxed) {
                        std::panic::resume_unwind(Box::new(Cancelled));
                    }
                },
            )
        }));
        match r {
            Ok(r) => {
                if let Some(s) = kept_s {
                    progress.store(s.to_bits(), Relaxed);
                }
                let _ = tx.send(r.map(|o| {
                    Some(Opened {
                        log: o.log,
                        clock: o.clock,
                        clock_source: o.clock_source,
                        breaks: o.breaks,
                        unlisted: o.unlisted,
                        profiles: o.kept.map(|k| k.profiles),
                    })
                }));
            }
            Err(e) if e.is::<Cancelled>() => {}
            Err(e) => std::panic::resume_unwind(e),
        }
    });
}

/// Where a bug report starts: the repository's bug form.
const BUG_REPORT_URL: &str =
    "https://github.com/peterhudson/timegrapher/issues/new?template=bug.yml";

/// The session file a recording's folder may carry (see `timegrapher
/// session`), which the app writes when it saves a take.
use timegrapher_core::take::MANIFEST;

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
    let entries: Vec<RecordingEntry> = m
        .recordings
        .iter()
        .filter(|e| lists(&dir, &e.file, path))
        .cloned()
        .collect();
    if entries.is_empty() {
        return Ok(None);
    }
    m.validate_analysis_inputs()
        .map_err(|e| format!("Can't read {}: {e}", file.display()))?;
    Ok(Some((m, entries)))
}

/// What a recording added to a watch is given, of what its files and the
/// watch didn't say.
struct Fill {
    bph: Option<u32>,
    /// The lift angle, and whether it was given rather than left at the
    /// default.
    lift: Option<(f64, bool)>,
    position: Option<Position>,
    started_utc: Option<String>,
    duration_s: f64,
}

/// When the take in `dir` started, from its session.json.
fn started_in(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("session.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("started_utc")?.as_str().map(str::to_string)
}

/// The session.json for a take given `fill`: the one at `existing`, if
/// there is one, with its start and length added where it has none.
fn with_start(existing: Option<&Path>, fill: &Fill) -> Result<String, String> {
    let mut v = match existing.filter(|p| p.is_file()) {
        Some(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("Can't read {}: {e}", p.display()))?
        }
        None => serde_json::json!({}),
    };
    let o = v
        .as_object_mut()
        .ok_or("session.json doesn't hold an object")?;
    if let Some(s) = &fill.started_utc {
        o.entry("started_utc").or_insert_with(|| s.clone().into());
    }
    if fill.duration_s > 0.0 {
        o.entry("duration_s")
            .or_insert_with(|| fill.duration_s.into());
    }
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// Write `fill` into the session files of the take at `take`, added to
/// `watch` from outside: only what they leave out, keeping the rest of
/// session.toml as it was, comments too.
fn fill_take(take: &Path, watch: &library::Watch, fill: &Fill) -> Result<(), String> {
    let file = take.join(MANIFEST);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("Can't read {}: {e}", file.display()))?;
    if !doc.contains_key("watch") {
        doc["watch"] = toml_edit::value(watch.name.as_str());
    }
    if let Some(c) = watch.calibre.as_deref().filter(|c| !c.is_empty()) {
        if !doc.contains_key("calibre") {
            doc["calibre"] = toml_edit::value(c);
        }
    }
    if let Some(b) = fill.bph {
        doc["bph"] = toml_edit::value(i64::from(b));
    }
    if let Some((l, known)) = fill.lift {
        doc["lift"] = toml_edit::value(l);
        doc["lift_known"] = toml_edit::value(known);
    }
    if let Some(p) = fill.position {
        let set = |t: &mut dyn toml_edit::TableLike| {
            if !t.contains_key("position") {
                t.insert("position", toml_edit::value(p.code()));
            }
        };
        match doc.get_mut("recording") {
            Some(toml_edit::Item::ArrayOfTables(tables)) => tables.iter_mut().for_each(|t| set(t)),
            Some(toml_edit::Item::Value(toml_edit::Value::Array(entries))) => entries
                .iter_mut()
                .filter_map(|v| v.as_inline_table_mut())
                .for_each(|t| set(t)),
            _ => {}
        }
    }
    write_replacing(&file, &doc.to_string())?;
    if fill.started_utc.is_some() {
        let json = take.join("session.json");
        write_replacing(&json, &with_start(Some(&json), fill)?)?;
    }
    Ok(())
}

/// `text` as a minute in this computer's time, written 2026-10-22 14:30.
fn parse_local_minute(text: &str) -> Option<std::time::SystemTime> {
    use chrono::TimeZone;
    let t = chrono::NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M").ok()?;
    chrono::Local
        .from_local_datetime(&t)
        .earliest()
        .map(std::time::SystemTime::from)
}

/// A recording's length in seconds, when its header says.
fn audio_length(f: &Path) -> Option<f64> {
    timegrapher_core::audio::info(f)
        .ok()
        .and_then(|i| i.frames.map(|n| n as f64 / i.sample_rate.max(1) as f64))
}

/// When the first of `files` started, by when it was last written less
/// its length: a guess, offered to be corrected.
fn started_guess(files: &[PathBuf]) -> Option<chrono::DateTime<chrono::Local>> {
    let f = files.first()?;
    let written = std::fs::metadata(f).ok()?.modified().ok()?;
    let length = Duration::from_secs_f64(audio_length(f).unwrap_or(0.0).max(0.0));
    Some(written.checked_sub(length).unwrap_or(written).into())
}

/// Whether a session file in `dir` whose entry names `file` lists `path`:
/// the same file or folder, or the whole folder when `path` is the only
/// recording in it, as a take saved for a lone file lists it.
fn lists(dir: &Path, file: &str, path: &Path) -> bool {
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let named = dir.join(file);
    same(&named, path)
        || (!path.is_dir()
            && named.is_dir()
            && path.parent().is_some_and(|p| same(p, &named))
            && recordings_in(&named).is_ok_and(|f| f.len() == 1 && same(&f[0], path)))
}

/// Save `masked` to the entries for `path` in its take's session file,
/// leaving the rest of the file as it was, comments and keys this app
/// doesn't read too, and with `positions` (start, end and position
/// code of each stretch, as [`App::opened_entries`] gives them) put one
/// entry per stretch in place of those entries, each keeping what the
/// first of them said besides. Positions are refused where the entries
/// list more than one file.
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
fn save_marks_to_take(
    path: &Path,
    masked: &[[f64; 2]],
    positions: Option<&Entries>,
    recorded: &[RecordedPosition],
) -> Result<PathBuf, String> {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .ok_or_else(|| format!("{} has no folder", path.display()))?
            .to_path_buf()
    };
    let file = dir.join(MANIFEST);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("Can't read {}: {e}", file.display()))?;
    let listed = |t: &dyn toml_edit::TableLike| {
        t.get("file")
            .and_then(|f| f.as_str())
            .is_some_and(|f| lists(&dir, f, path))
    };
    let mut spans = toml_edit::Array::new();
    for m in masked {
        spans.push(toml_edit::Array::from_iter([m[0], m[1]]));
    }
    let set = |t: &mut dyn toml_edit::TableLike| {
        if masked.is_empty() {
            t.remove("masked");
        } else {
            t.insert("masked", toml_edit::value(spans.clone()));
        }
    };
    // One stretch's entry: the first entry listed, with its position and
    // times.
    let place = |t: &mut dyn toml_edit::TableLike,
                 (start, end, code): &(Option<f64>, Option<f64>, String)| {
        t.insert("position", toml_edit::value(code.as_str()));
        for (key, v) in [("start_s", start), ("end_s", end)] {
            match v {
                Some(v) => t.insert(key, toml_edit::value(*v)),
                None => t.remove(key),
            };
        }
        // Each start is already after its settling: nothing to skip again.
        t.insert("settle_s", toml_edit::value(0.0));
    };
    let one_file = |files: Vec<String>| {
        if files.windows(2).any(|w| w[0] != w[1]) {
            Err(format!(
                "{} lists {} as more than one file, so its positions can't be saved",
                file.display(),
                capture_label(path)
            ))
        } else {
            Ok(())
        }
    };
    let file_of = |t: &dyn toml_edit::TableLike| {
        t.get("file")
            .and_then(|f| f.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let mut changed = 0;
    match doc.get_mut("recording") {
        Some(toml_edit::Item::ArrayOfTables(tables)) => match positions {
            None => {
                for t in tables.iter_mut().filter(|t| listed(*t)) {
                    set(t);
                    changed += 1;
                }
            }
            Some(positions) => {
                one_file(
                    tables
                        .iter()
                        .filter(|t| listed(*t))
                        .map(|t| file_of(t))
                        .collect(),
                )?;
                let mut out = toml_edit::ArrayOfTables::new();
                let mut done = false;
                for t in tables.iter() {
                    if !listed(t) {
                        out.push(t.clone());
                        continue;
                    }
                    changed += 1;
                    if std::mem::replace(&mut done, true) {
                        continue;
                    }
                    for (i, p) in positions.iter().enumerate() {
                        let mut e = t.clone();
                        // A comment stays on the first copy alone.
                        if i > 0 {
                            e.decor_mut().set_prefix("\n");
                            for (_, item) in e.iter_mut() {
                                if let Some(v) = item.as_value_mut() {
                                    v.decor_mut().clear();
                                }
                            }
                        }
                        place(&mut e, p);
                        set(&mut e);
                        out.push(e);
                    }
                }
                *tables = out;
            }
        },
        Some(toml_edit::Item::Value(toml_edit::Value::Array(entries))) => match positions {
            None => {
                for t in entries.iter_mut().filter_map(|v| v.as_inline_table_mut()) {
                    if listed(t) {
                        set(t);
                        changed += 1;
                    }
                }
            }
            Some(positions) => {
                one_file(
                    entries
                        .iter()
                        .filter_map(|v| v.as_inline_table())
                        .filter(|t| listed(*t))
                        .map(|t| file_of(t))
                        .collect(),
                )?;
                let mut out = toml_edit::Array::new();
                let mut done = false;
                for v in entries.iter() {
                    let Some(t) = v.as_inline_table().filter(|t| listed(*t)) else {
                        out.push_formatted(v.clone());
                        continue;
                    };
                    changed += 1;
                    if std::mem::replace(&mut done, true) {
                        continue;
                    }
                    for p in positions {
                        let mut e = t.clone();
                        place(&mut e, p);
                        set(&mut e);
                        out.push(e);
                    }
                }
                *entries = out;
            }
        },
        _ => {}
    }
    if changed == 0 {
        return Err(format!(
            "{} doesn't list {}",
            file.display(),
            capture_label(path)
        ));
    }
    // With the positions, what was recorded under any painting of them.
    if positions.is_some() {
        if recorded.is_empty() {
            doc.remove("recorded");
        } else {
            let mut out = toml_edit::ArrayOfTables::new();
            for r in recorded {
                let mut t = toml_edit::Table::new();
                t.insert("position", toml_edit::value(r.position.as_str()));
                for (key, v) in [("start_s", r.start_s), ("end_s", r.end_s)] {
                    if let Some(v) = v {
                        t.insert(key, toml_edit::value(v));
                    }
                }
                out.push(t);
            }
            doc.insert("recorded", toml_edit::Item::ArrayOfTables(out));
        }
    }
    write_replacing(&file, &doc.to_string())?;
    Ok(file)
}

/// `t` seconds to the nearest tenth.
fn tenth(t: f64) -> f64 {
    (t * 10.0).round() / 10.0
}

/// Write `text` to `target` through a file beside it, renamed over it once
/// whole, so a failed write never leaves half a file.
fn write_replacing(target: &Path, text: &str) -> Result<(), String> {
    let name = format!(".{}.saving", file_name(target));
    let temp = target.with_file_name(name);
    std::fs::write(&temp, text)
        .and_then(|()| std::fs::rename(&temp, target))
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            format!("{}: {e}", target.display())
        })
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

/// Why a folder whose session file says it was kept without its sound
/// can't be read, when it has no `analysis/` folder: it is refused as
/// `timegrapher session` refuses it, sound in it or not.
fn no_analysis_to_read(path: &Path) -> Option<String> {
    let dir = if path.is_dir() { path } else { path.parent()? };
    if analysis_take::is_analysis_only(dir) || !dir.join(MANIFEST).is_file() {
        return None;
    }
    let m: Manifest = toml::from_str(&std::fs::read_to_string(dir.join(MANIFEST)).ok()?).ok()?;
    (m.audio == Some(false)).then(|| {
        format!(
            "{}: its {MANIFEST} says audio = false, but there is no {}/ folder to read.",
            capture_label(dir),
            analysis_take::DIR
        )
    })
}

/// The folder to open for `path`: itself when it holds sound (or isn't a
/// folder), else the one take folder inside it, which is where the app
/// saves a take. Several takes inside are named so one can be picked.
fn take_folder(path: &Path) -> Result<PathBuf, String> {
    let is_take = |p: &Path| recordings_in(p).is_ok() || analysis_take::is_analysis_only(p);
    if !path.is_dir() || is_take(path) {
        return Ok(path.to_path_buf());
    }
    let mut takes: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| format!("Can't read the folder {}: {e}", path.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir() && is_take(p))
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

/// Bytes in the files under `dir`, all the way down.
fn folder_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |d| {
        d.filter_map(|e| e.ok())
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => folder_size(&e.path()),
                Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
                _ => 0,
            })
            .sum()
    })
}

/// A library move's outcome, from its thread ([`library::move_library`]).
type LibraryMove = std::sync::mpsc::Receiver<Result<Option<String>, String>>;

/// Open `dir` in the system's file manager.
fn reveal(dir: &Path) -> std::io::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(program).arg(dir).spawn()?;
    // Waited on apart, so it leaves no zombie process behind.
    std::thread::spawn(move || child.wait());
    Ok(())
}

fn capture_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl TimegrapherApp {
    /// The menus and pane icons across the top, the status line along the
    /// bottom and the readings and panes between, all on the window colour
    /// with the content in cards.
    fn panels(&mut self, ctx: &egui::Context) {
        self.refresh_trend();
        // A test set up against another watch than the one picked now.
        self.find_full_wind();
        if let Some(wait) = self.full_wind_recheck(guided::unix_now()) {
            ctx.request_repaint_after(wait);
        }
        let pal = theme::palette(ctx.style().visuals.dark_mode);
        let bar = egui::Frame::new().fill(pal.window);
        egui::TopBottomPanel::top("menus")
            .frame(bar.inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ctx, |ui| self.menu_bar(ui));
        egui::TopBottomPanel::top("mode")
            .exact_height(MODE_ROW_H + 4.0)
            .frame(bar.inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 4,
                bottom: 0,
            }))
            .show(ctx, |ui| self.mode_row(ui));
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
        // Esc puts the tool in hand down: the Position Brush or Mask.
        if (self.brush.is_some() || self.masking)
            && !egui::Popup::is_any_open(ctx)
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.brush = None;
            self.masking = false;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&command(egui::Key::S))) {
            if self.marks_unsaved() {
                self.save_marks();
            } else if self.session.allows(Action::Save) {
                self.save_again();
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&command(egui::Key::Q))) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Cmd+1 and Cmd+2 pick the view, before the bare digits are read.
        if ctx.input_mut(|i| i.consume_shortcut(&view_shortcut(View::Bench))) {
            self.view = View::Bench;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&view_shortcut(View::Watches))) {
            self.view = View::Watches;
        }
        // Keys 1 to 6, alone, put the watch in a position: never with a
        // modifier held, nor in the Watches view, nor while a field is
        // being typed in. The modifiers are the key event's own, as a
        // shortcut's are: one pressed and let go within a frame is gone
        // from the frame's by the time it is read.
        if self.view == View::Bench && !ctx.wants_keyboard_input() {
            let keys = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
            ];
            let pressed = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.is_none() => keys.iter().position(|k| k == key),
                    _ => None,
                })
            });
            if let Some(p) = pressed
                .map(|n| Position::ALL[n])
                .filter(|_| self.position_editable())
            {
                if p != self.position {
                    self.change_position(p);
                }
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&settings_shortcut())) {
            self.open_settings(self.settings_last);
        }
        egui::CentralPanel::default()
            .frame(bar.inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 8,
                bottom: 12,
            }))
            .show(ctx, |ui| match self.view {
                View::Bench => self.main_view(ui),
                View::Watches => self.watches_view(ui),
            });
        self.settings_window(ctx);
    }
}

impl eframe::App for TimegrapherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::frame_built();
        // One settling for every change of position: the pane's Settle.
        self.session.set_settle_s(self.guided_settle_s);
        if std::mem::take(&mut self.listen_pending) && !self.running() {
            self.quick_listen(false);
        }
        // A microphone that opened is one set up.
        if self.running() && self.session.mic() && !self.setup_sheet {
            self.mic_set_up = true;
        }
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
        // A file dropped on the window is opened, where the desktop passes
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
            self.open_recording(&p, true);
        }

        self.poll();

        self.export_tick(ctx);

        // Closing the window while the library is being moved is refused,
        // and the app stays open: stopping a copy part way would leave half
        // a library behind.
        if ctx.input(|i| i.viewport().close_requested()) && self.library_moving.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.info_msg(
                "Your watches are still being moved: Timegrapher can be closed once it is done."
                    .into(),
            );
        }
        // Closing the window while a save is written waits for it: the
        // take would be left half copied.
        if ctx.input(|i| i.viewport().close_requested())
            && self.saving.is_some()
            && self.library_moving.is_none()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.info_msg(
                "Saving: Timegrapher can be closed once the sound so far is copied into its take."
                    .into(),
            );
        }
        // Closing the window with unsaved sound asks first.
        if ctx.input(|i| i.viewport().close_requested())
            && self.saving.is_none()
            && !self.may_close
            && self.needs_save_confirmation()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::Quit);
        }
        self.guided_tick(ctx);
        self.reserve_tick(ctx);
        self.reserve_saved();
        self.panels(ctx);
        self.confirm_dialog(ctx);
        self.remove_masks_dialog(ctx);
        self.kept_delete_dialog(ctx);
        self.watch_form_dialog(ctx);
        self.test_keep_dialog(ctx);
        self.stop_test_dialog(ctx);
        self.open_ask_dialog(ctx);
        self.setup_sheet_dialog(ctx);
        self.take_ask_dialog(ctx);
        self.poll_take_keep();
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
        } else if self.sound_job.is_some()
            || self.scope_job.is_some()
            || self.steady_job.is_some()
            || self.saving.is_some()
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
        // A save being written finishes first: quitting part way would
        // leave half a take, and the sound it copies is let go below.
        self.wait_for_save();
        // Sound the user hasn't been asked about is kept for the next start
        // to offer back: on macOS, Cmd+Q and Quit in the app menu or the
        // Dock quit without the window's close being asked first, and
        // can't be cancelled.
        if !self.may_close && self.unsaved_s() > 5.0 && !self.quick_unsaved() {
            self.session.keep_recording();
        } else {
            self.session.discard_recording();
        }
    }
}

/// A status-bar notice's pop-over: the alert's icon and its words in
/// the text colour, then its buttons as they fit.
fn notice(
    ui: &mut egui::Ui,
    kind: Option<theme::Alert>,
    words: &str,
    buttons: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    theme::card()
        .fill(theme::pal(ui).control)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let text = theme::pal(ui).text;
            ui.horizontal_top(|ui| {
                if let Some(kind) = kind {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
                    theme::alert_icon(ui.painter(), rect, kind, theme::pal(ui));
                }
                ui.add(egui::Label::new(RichText::new(words).color(text)).wrap());
            });
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), buttons);
        })
        .response
}

/// A beat on the Scope: its index, its time's bits (so a new session's
/// beat of the same index is another) and the span drawn.
type ScopeKey = (i64, u64, scope::Zoom, u64);

/// Everything Average Over sets, and what it leaves alone.
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
pub use timegrapher_core::take::DEFAULT_LIFT_DEG;

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

/// "No regular beat found", in core's words: with the timing scatter
/// that decided it, or that the beats' sound shows no unlock or drop.
fn no_regular_beat(r: &LiveReading) -> String {
    use timegrapher_core::measure;
    let why = measure::no_watch_reason_in(&r.caveats).unwrap_or_default();
    measure::no_watch_text(why, r.jitter_us, r.timing_scatter_fraction)
}

/// The Beat Error card's line under the whole take, when it is of sizes.
/// What [`TimegrapherApp::whole_take`] was read from: the beats heard,
/// the stretches, the masks and what the readings rest on.
type WholeKey = (usize, Vec<(f64, f64)>, Vec<(f64, f64)>, Revision);

/// What [`TimegrapherApp::free_table`] was read from: the beats heard,
/// each position's settled stretches, the masks and the True Clock.
/// A take's changes of position and the settling after each.
type Positions = (Vec<(f64, Position)>, Vec<(f64, f64)>);
/// The positions shown, and those recorded under them where painting
/// changed them.
type Layers = (Positions, Option<Positions>);

/// One entry per stretch in one position, as a session file lists it:
/// start, end and the position's code.
type Entries = Vec<(Option<f64>, Option<f64>, String)>;

/// The positions a session file lists, each stretch's start, the start
/// of what is read once it has settled, its end and its position, as
/// changes oldest first and the settling before each: from the end of the
/// one before (or the take's start, for the first) to where its reading
/// starts.
fn positions_of(listed: impl Iterator<Item = (f64, f64, Option<f64>, Position)>) -> Positions {
    let mut listed: Vec<(f64, f64, Option<f64>, Position)> = listed.collect();
    listed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut settles = Vec::new();
    for (i, &(start, from, _, p)) in listed.iter().enumerate() {
        // The position began where the one before it ended, the first at
        // the take's start: the app saves each entry from the end of its
        // settling, the first's too.
        let began = match i.checked_sub(1).map(|j| listed[j].2) {
            None => 0.0,
            Some(Some(end)) if end < start => end,
            Some(_) => start,
        };
        if from > began {
            settles.push((began, from));
        }
        out.push((began, p));
    }
    (out, settles)
}

/// The session file's entries for `spans` (start, end, position) and the
/// settling that begins them: one per stretch, from the end of its
/// settling to the next change; one with no times while the watch stayed
/// put.
fn entries_of(spans: &[(f64, f64, Position)], settles: &[(f64, f64)]) -> Entries {
    let one = spans.len() == 1;
    spans
        .iter()
        .enumerate()
        .map(|(i, &(a, b, p))| {
            let a = if i == 0 { 0.0 } else { a };
            let start = settles
                .iter()
                .find(|s| (s.0 - a).abs() < 1e-6)
                .map_or(a, |s| s.1.min(b));
            let end = b.is_finite().then_some(b);
            (
                (!one).then_some(start),
                end.filter(|_| !one),
                p.code().to_string(),
            )
        })
        .collect()
}

type FreeKey = (
    usize,
    Vec<(core_session::Position, f64, f64)>,
    Vec<(f64, f64)>,
    Revision,
);

/// How often the Positions Test table is read again while listening.
const FREE_TABLE_STEP_S: f64 = 1.0;

/// A Positions Test table being read in the background, and what from.
struct FreeJob {
    key: FreeKey,
    rx: std::sync::mpsc::Receiver<Option<guided::FreeTable>>,
}

/// The whole take in one position, and the seconds it covers.
#[derive(Clone)]
struct WholeTake {
    m: timegrapher_core::session::Measurement,
    span: f64,
}

/// The breaks a whole take is read across, in the caveat's words: a
/// card's own reading never reaches across one, so the whole take's hover
/// says them.
fn whole_breaks_text(w: &WholeTake) -> String {
    cut_caveats_text(&w.m.caveats)
}

/// Of `caveats`, the breaks and the sides named by the beat count across
/// them, as the cards word them, each after a space.
fn cut_caveats_text(caveats: &[Caveat]) -> String {
    let picked = |keep: fn(&Caveat) -> bool| -> Vec<Caveat> {
        caveats.iter().filter(|c| keep(c)).cloned().collect()
    };
    let breaks = picked(|c| matches!(c, Caveat::Breaks { .. } | Caveat::BreaksUnlisted { .. }));
    let sides = picked(|c| matches!(c, Caveat::SidesByCount { .. }));
    reading_limits("", &breaks, 0.0, 0)
        .into_iter()
        .chain(reading_limits("Beat Error", &sides, 0.0, 0))
        .map(|(_, hover)| format!(" {hover}"))
        .collect()
}

const WHOLE_AS_SIZES: &str = "whole take is given as a size: its sign differs either side of a cut";

/// What limits a reading, as facts: a short form for the card's caption
/// and a sentence for its hover, in core's words. Nothing graded: the ±
/// beside the figure says how far it could be off; these say what the ±
/// can't show.
fn reading_limits(
    title: &str,
    caveats: &[Caveat],
    lift_deg: f64,
    beats: usize,
) -> Vec<(String, String)> {
    caveats
        .iter()
        .filter(|c| shown_on(c, title))
        .filter_map(|c| {
            let long = c.long(Some(lift_deg))?;
            let long = match c {
                Caveat::Clipped { .. } => format!("{long} {}", help::CLIPPING),
                _ => long,
            };
            Some((c.short(lift_deg, beats)?, long))
        })
        .collect()
}

/// Whether a card titled `title` names caveat `c` beside its figure. No
/// regular beat, the clock sources apart and a beat error given as a size
/// are said elsewhere; so are the lift angle, on the Calibre card, and
/// the input's clock, on the Rate's controls row.
fn shown_on(c: &Caveat, title: &str) -> bool {
    match c {
        Caveat::NoWatch { .. }
        | Caveat::ClockSourcesDisagree { .. }
        | Caveat::BeatErrorAsSize
        | Caveat::LiftUnknown
        | Caveat::ClockUnknown => false,
        Caveat::Clipped { .. } => matches!(title, "Amplitude" | "Beat Error"),
        Caveat::SidesByCount { .. } => title == "Beat Error",
        _ => true,
    }
}

/// How long closing an input may take before the app carries on.
const RELEASE_WAIT: Duration = Duration::from_secs(2);

/// Drop `v` on a thread of its own, waiting up to `wait` for it: true
/// when it finished in time. Otherwise it is left to finish, or not, on
/// that thread.
fn drop_within<T: Send + 'static>(v: T, wait: Duration) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("close input".into())
        .spawn(move || {
            drop(v);
            let _ = tx.send(());
        });
    spawned.is_ok() && rx.recv_timeout(wait).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The settling the tests of a move set the pane's Settle to.
    const SETTLE_S: f64 = 10.0;

    /// An input that hangs as it closes doesn't hang the app: the wait
    /// ends and the app carries on, as it does at once when closing is
    /// quick.
    #[test]
    fn a_hung_input_closes_without_hanging_the_app() {
        struct Hangs(std::sync::mpsc::Receiver<()>);
        impl Drop for Hangs {
            fn drop(&mut self) {
                // As a dead sound server's close: no answer comes.
                let _ = self.0.recv();
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let started = Instant::now();
        assert!(!drop_within(Hangs(rx), Duration::from_millis(100)));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(tx);
        assert!(drop_within(String::from("quick"), Duration::from_secs(2)));
    }

    /// On macOS the input's automatic gain is the USB microphone's whose
    /// name it carries; the system default takes the only one there is.
    #[test]
    fn an_input_finds_its_usb_microphones_automatic_gain() {
        let mic = |name: &str| mixer::UsbAgc {
            name: name.into(),
            vendor_id: 0x0d8c,
            product_id: 0x013c,
            control: timegrapher_core::usbagc::AgcControl {
                interface: 0,
                unit: 2,
                channel: 0,
                volume: true,
            },
            before: Some(true),
            after: None,
            volume_db: None,
            error: None,
        };
        let name = |m: Option<mixer::UsbAgc>| m.map(|m| m.name);
        let one = || vec![mic("USB PnP Sound Device")];
        let two = || vec![mic("USB PnP Sound Device"), mic("Blue Yeti")];
        assert_eq!(
            name(usb_agc_for(one(), Some("USB PnP Sound Device"))).as_deref(),
            Some("USB PnP Sound Device")
        );
        assert_eq!(
            name(usb_agc_for(two(), Some("Blue Yeti Stereo Microphone"))).as_deref(),
            Some("Blue Yeti")
        );
        assert_eq!(
            name(usb_agc_for(one(), None)).as_deref(),
            Some("USB PnP Sound Device")
        );
        // The built-in microphone has no such switch, and the default
        // with two USB microphones is neither.
        assert!(usb_agc_for(one(), Some("MacBook Pro Microphone")).is_none());
        assert!(usb_agc_for(two(), None).is_none());
        assert!(usb_agc_for(Vec::new(), None).is_none());
    }

    /// Which inputs keep their automatic gain on is kept with the settings.
    #[test]
    fn the_automatic_gain_choice_is_kept_per_input() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.agc_on = vec!["hw:2,0".into()];
        let s = app.settings();
        assert_eq!(s.agc_on, vec!["hw:2,0".to_string()]);
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
        again.apply_settings(s);
        assert_eq!(again.agc_on, vec!["hw:2,0".to_string()]);
        assert!(
            Settings::default().agc_on.is_empty(),
            "off for every input at first"
        );
    }

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
        // The lift angle and the input's clock are said elsewhere.
        assert_eq!(short, ["4 s of beats"]);
        let rate = reading_limits("Rate", &caveats, 52.0, 30);
        let short: Vec<&str> = rate.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(short, ["4 s of beats"]);
        let missing = reading_limits("Rate", &[Caveat::BeatsMissing { fraction: 0.9 }], 52.0, 90);
        assert_eq!(missing[0].0, "90 of 100 beats");
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
    #[test]
    fn a_beat_error_says_where_its_sides_were_named_by_count() {
        let caveats = [Caveat::SidesByCount { count: 2 }];
        let be = reading_limits("Beat Error", &caveats, 52.0, 30);
        assert_eq!(be.len(), 1);
        assert_eq!(be[0].0, "sides named by count");
        assert!(be[0].1.contains("not told apart by their sound"));
        // It doesn't touch the rate or the amplitude.
        assert!(reading_limits("Rate", &caveats, 52.0, 30).is_empty());
        assert!(reading_limits("Amplitude", &caveats, 52.0, 30).is_empty());
    }

    use timegrapher_core::synth::{generate, SynthConfig};

    pub(super) fn synthetic(seconds: f64, rate: f64) -> TimegrapherApp {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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

    /// Words in a reading's place on a narrow card show whole: "No
    /// Regular Beat" takes the dash's room rather than being cut.
    #[test]
    fn a_narrow_cards_words_show_whole() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let status = ("No Regular Beat".to_string(), String::new());
        for compact in [true, false] {
            let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(96.0, 40.0));
            let mut texts = Vec::new();
            for _ in 0..2 {
                let out = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        figure_row(ui, rect, None, "s/d", Some(&status), "", compact);
                    });
                });
                texts = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Text(t) => Some((
                            t.galley.text().to_string(),
                            t.galley.rect.translate(t.pos.to_vec2()),
                        )),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
            }
            let words: String = texts
                .iter()
                .filter(|t| t.0 != "—")
                .map(|t| t.0.clone())
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(words, "No Regular Beat", "{texts:?}");
            for t in &texts {
                assert!(t.1.right() <= rect.right() + 0.5, "{compact}: {t:?} is cut");
            }
        }
    }

    /// On the Tick Tock Profile an edge's bar and a sound's ring are drawn
    /// on the trace, the ring hollow; on a short plot, with the edges'
    /// names inside it, no bar or ring crosses a name.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn the_profiles_marks_keep_off_the_names() {
        let mut app = synthetic(30.0, 0.0);
        // Stopped, the profile is the one kept at the end.
        let end = app.latest_s();
        let sides = app.live.as_ref().unwrap().tick_profiles().clone();
        app.sound_history = vec![(end, sides)];
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let run = |app: &mut TimegrapherApp, w: f32, h: f32| {
            let mut shapes = Vec::new();
            for _ in 0..4 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(w, h),
                    )),
                    ..Default::default()
                };
                shapes = ctx.run(input, |ctx| app.panels(ctx)).shapes;
            }
            let names: Vec<egui::Rect> = shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t)
                        if ["Unlock", "Drop", "Peak"].contains(&t.galley.text()) =>
                    {
                        Some(t.galley.rect.translate(t.pos.to_vec2()))
                    }
                    _ => None,
                })
                .collect();
            let rings: Vec<egui::Rect> = shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Circle(r) if r.fill == egui::Color32::TRANSPARENT => Some(
                        egui::Rect::from_center_size(r.center, egui::Vec2::splat(2.0 * r.radius)),
                    ),
                    _ => None,
                })
                .collect();
            let bars: Vec<egui::Rect> = shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::LineSegment { points, stroke }
                        if stroke.width == 2.0 && points[0].y == points[1].y =>
                    {
                        Some(egui::Rect::from_two_pos(points[0], points[1]))
                    }
                    _ => None,
                })
                .collect();
            let facts: Vec<egui::Rect> = shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t)
                        if ["Amplitude", "Unlock to Drop"].contains(&t.galley.text())
                            || t.galley.text().ends_with('°')
                            || t.galley.text().ends_with(" ms") =>
                    {
                        Some(t.galley.rect.translate(t.pos.to_vec2()))
                    }
                    _ => None,
                })
                .collect();
            (names, rings, bars, facts)
        };
        // Where the facts box is two rows, over a trace that rises under
        // the left corner and is low on the right, it takes the right, and
        // keeps off the marks.
        let kept = app.sound_history.clone();
        for (_, sides) in &mut app.sound_history {
            for p in sides.iter_mut().flatten() {
                let top = p.p90.iter().copied().fold(0.0f32, f32::max);
                let n = p.median.len() / 5;
                p.median[..n].fill(0.9 * top);
            }
        }
        app.sound_opts.side_by_side = true;
        let (_, rings, bars, facts) = run(&mut app, 1440.0, 820.0);
        let plots: Vec<egui::Rect> = (0..2usize)
            .filter_map(|k| {
                ctx.data(|d| {
                    d.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(("profile plot", k)))
                })
                .map(|p| p.0)
            })
            .collect();
        assert_eq!(plots.len(), 2);
        for p in &plots {
            let here: Vec<_> = facts.iter().filter(|f| p.contains_rect(**f)).collect();
            assert_eq!(here.len(), 4, "two rows: {here:?}");
            assert!(
                here.iter().all(|f| f.left() > p.center().x),
                "{p:?}: {here:?}"
            );
        }
        for m in rings.iter().chain(&bars) {
            assert!(
                facts.iter().all(|f| !f.intersects(*m)),
                "{m:?} under the facts: {facts:?}"
            );
        }
        // Over a trace high under both corners, the box keeps no rows: the
        // trace and its marks come first, and the hover has the facts.
        for (_, sides) in &mut app.sound_history {
            for p in sides.iter_mut().flatten() {
                let top = p.p90.iter().copied().fold(0.0f32, f32::max);
                let n = p.median.len();
                p.median[n - n / 4..].fill(0.9 * top);
            }
        }
        let (_, _, _, facts) = run(&mut app, 1440.0, 820.0);
        assert!(
            facts
                .iter()
                .all(|f| plots.iter().all(|p| !p.contains_rect(*f))),
            "{facts:?}"
        );
        app.sound_history = kept;
        app.sound_opts.side_by_side = false;
        let (names, rings, bars, _) = run(&mut app, 1440.0, 1000.0);
        assert!(!names.is_empty(), "the edges named");
        assert!(!rings.is_empty() && !bars.is_empty(), "{rings:?} {bars:?}");
        // A short window that still draws the plot: at 800 x 600 it
        // says there is no room instead.
        let (names, rings, bars, _) = run(&mut app, 800.0, 640.0);
        assert!(!names.is_empty(), "the edges named in the short plot");
        for m in rings.iter().chain(&bars) {
            assert!(
                names.iter().all(|n| !n.intersects(*m)),
                "{m:?} crosses a name: {names:?}"
            );
        }
    }

    /// In Quick Listen, a calibre picked while listening, at the beat rate
    /// Auto already found, adds to the take: the beats and the strip's
    /// readings heard so far stay. A calibre at another rate starts the
    /// beats again.
    #[test]
    fn a_calibre_picked_while_listening_keeps_what_was_heard() {
        let mut app = synthetic(30.0, 0.0);
        assert_eq!(app.bph, None);
        let heard = app.live.as_ref().unwrap().beats().len();
        let trend = app.trend.len();
        assert!(heard > 100 && trend > 5, "{heard} {trend}");
        assert_eq!(app.cal.bph("ETA 2824-2"), Some(28_800));
        app.set_calibre("ETA 2824-2".into());
        assert_eq!(app.bph, Some(28_800));
        assert_eq!(app.live.as_ref().unwrap().beats().len(), heard);
        assert_eq!(app.trend.len(), trend);
        // Auto again, with no calibre: still all of it.
        app.set_calibre(String::new());
        assert_eq!(app.bph, None);
        assert_eq!(app.live.as_ref().unwrap().beats().len(), heard);
        assert_eq!(app.trend.len(), trend);
        let other = ["Seiko 7S26", "Seiko 7S26A", "Miyota 8215", "Seiko NH35"]
            .into_iter()
            .find(|c| app.cal.bph(c).is_some_and(|b| b != 28_800))
            .expect("a calibre at another rate");
        app.set_calibre(other.into());
        assert!(app.live.as_ref().unwrap().beats().is_empty());
    }

    /// Noise with no regular beat: the strip shows the peaks heard,
    /// faintly and said to be possible beats, rather than an empty card.
    #[test]
    fn noise_shows_its_possible_beats_on_the_strip() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.reset_session(48000, "noise".into());
        let mut seed = 12_345_u32;
        let x: Vec<f32> = (0..48_000 * 20)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 * 0.02 - 0.01
            })
            .collect();
        let live = app.live.as_mut().unwrap();
        for b in x.chunks(960) {
            if live.push(b) {
                let r = live.reading(app.average_s);
                app.trend.push(TrendPoint::of(r.time_s, &r));
            }
        }
        app.poll();
        assert!(app.no_watch_yet());
        assert!(!app.live.as_ref().unwrap().candidates().is_empty());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.strip_pane(ui));
        });
        let drawn = format!("{:?}", out.shapes);
        assert!(drawn.contains("No regular beat found yet"), "{drawn}");
        assert!(drawn.contains("Circle"), "the possible beats are drawn");
    }

    /// A paused session carries on only with the input it was listening to:
    /// another input's clock correction and clock measurement aren't its.
    #[test]
    fn resuming_on_another_input_is_refused() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.live = Some(LiveAnalyzer::new(48_000, LiveConfig::default()));
        app.clock_track = Some(ClockTrack {
            device: "Mic A".into(),
            shown: "Mic A (default)".into(),
            tracker: timegrapher_core::clock::ClockTracker::new(48_000),
            next_s: CLOCK_MEASURE_S,
            fit: None,
        });
        // Compared by the input's name, named as the microphone menu shows it.
        assert_eq!(app.resume_refusal("Mic A", "Mic A (direct)", 48_000), None);
        let why = app
            .resume_refusal("Mic B", "Mic B (direct)", 48_000)
            .expect("another input refused");
        assert!(
            why.contains("Mic A (default)") && why.contains("Mic B (direct)"),
            "{why}"
        );
        assert!(app.resume_refusal("Mic A", "Mic A", 44_100).is_some());
    }

    /// Stopped after the watch is taken off the stand, the readings stay on
    /// the last stretch with a watch heard, and a long enough take shows
    /// its whole-take reading.
    #[test]
    fn stopping_keeps_the_last_heard_readings() {
        let mut app = synthetic(60.0, 12.0);
        let before = app.reading().unwrap();
        let whole = app.whole_take().expect("whole take");
        let rate = whole.m.rate_s_per_day.unwrap();
        assert!((rate - 12.0).abs() < 2.0, "{rate}");
        assert!(whole.span > 40.0, "{}", whole.span);
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

    fn screen() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1280.0, 820.0),
            )),
            ..Default::default()
        }
    }

    fn export_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tg-app-export-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Export as SVG: the next frame draws the pane with its name in place
    /// of its toolbar and writes what it painted.
    #[test]
    fn a_pane_is_exported_as_an_svg_drawing() {
        let mut app = synthetic(12.0, 20.0);
        let ctx = themed();
        let _ = ctx.run(screen(), |ctx| app.panels(ctx));
        let dir = export_dir("svg");
        let path = dir.join("Strip.svg");
        app.start_export(Pane::Strip, path.clone());
        let _ = ctx.run(screen(), |ctx| {
            app.export_tick(ctx);
            app.panels(ctx)
        });
        assert!(app.export.is_none(), "export still waiting");
        let doc = std::fs::read_to_string(&path).unwrap();
        assert!(doc.starts_with("<svg "), "{doc}");
        assert!(
            doc.contains(">Paper Strip<") || doc.contains(&format!(">{}<", Pane::Strip.title()))
        );
        assert!(
            doc.contains("<polyline") || doc.contains("<line"),
            "no lines"
        );
        // Its controls aren't in the picture, and the window's frame goes on
        // as before: the profile below keeps its toolbar (its Layout
        // setting, first in the row, stands for it).
        assert!(!doc.contains("Width"), "toolbar drawn into the picture");
        assert!(!doc.contains("Export…"), "toolbar drawn into the picture");
        assert!(
            theme::tools_drawn(&ctx).contains(&format!("{}/Layout", Pane::Sound.title())),
            "other panes lost their toolbars"
        );
        assert!(matches!(&app.message, Some((m, false)) if m.starts_with("Saved ")));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Export as PNG: a screenshot is asked for, and the pane is cut from it.
    #[test]
    fn a_pane_is_exported_as_a_png_from_a_screenshot() {
        let mut app = synthetic(12.0, 20.0);
        let ctx = themed();
        let _ = ctx.run(screen(), |ctx| app.panels(ctx));
        let dir = export_dir("png");
        let path = dir.join("Strip.png");
        app.start_export(Pane::Strip, path.clone());
        let out = ctx.run(screen(), |ctx| {
            app.export_tick(ctx);
            app.panels(ctx)
        });
        let asked = out
            .viewport_output
            .values()
            .flat_map(|v| &v.commands)
            .any(|c| matches!(c, egui::ViewportCommand::Screenshot(_)));
        assert!(asked, "no screenshot asked for");
        let rect = app
            .export
            .as_ref()
            .and_then(|e| e.rect)
            .expect("pane drawn");
        // The window as a screen of one colour, the pane's corner marked.
        let mut image = egui::ColorImage::filled([1280, 820], Color32::from_gray(40));
        let corner = rect.min.round();
        image.pixels[corner.y as usize * 1280 + corner.x as usize] = Color32::RED;
        let mut input = screen();
        input.events.push(egui::Event::Screenshot {
            viewport_id: egui::ViewportId::ROOT,
            user_data: Default::default(),
            image: std::sync::Arc::new(image),
        });
        let _ = ctx.run(input, |ctx| {
            app.export_tick(ctx);
            app.panels(ctx)
        });
        assert!(app.export.is_none(), "export still waiting");
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!(info.width, rect.width().round() as u32);
        assert_eq!(info.height, rect.height().round() as u32);
        assert_eq!(&buf[..4], &[255, 0, 0, 255], "not cut at the pane");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A pane that's never drawn gives up with a message, not a wait.
    #[test]
    fn an_export_that_cannot_be_drawn_gives_up() {
        let mut app = synthetic(2.0, 0.0);
        let ctx = themed();
        app.start_export(Pane::Strip, export_dir("never").join("x.png"));
        for _ in 0..61 {
            let _ = ctx.run(screen(), |ctx| app.export_tick(ctx));
        }
        assert!(app.export.is_none());
        assert!(matches!(&app.message, Some((_, true))));
    }

    /// Every piece of text a frame painted.
    pub(super) fn painted_text(out: &egui::FullOutput) -> String {
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Clipped sound shows in a chip on the Amplitude and Beat Error cards,
    /// with nothing pushed in over the readings; a stored clock error is
    /// in Settings, not on the Rate card, the corrected rate in the Rate
    /// figure's hover.
    #[test]
    fn clipping_shows_in_a_chip_and_the_true_clock_in_the_rate_hover() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
        for w in [400.0, 400.0, 1610.0, 1610.0, 1610.0] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 820.0),
                )),
                ..Default::default()
            };
            text = painted_text(&ctx.run(input, |ctx| app.panels(ctx)));
        }
        assert!(!text.contains("Clipping on"), "no banner: {text}");
        assert!(
            text.lines().filter(|l| l.ends_with("% Clipped")).count() >= 2,
            "{text}"
        );
        // The clock kept is Settings' to show.
        assert!(!text.contains("20.0 ppm slow"), "{text}");
        // The rate on it, in the Rate figure's hover: 20 ppm slow takes
        // 1.7 s/d off a rate read on the card.
        let card = app.reading().unwrap().rate_s_per_day.unwrap();
        let shown = format!("{:+.1}", clockstore::correct_rate(card, 20.0)).replace('-', "−");
        let hover = app.true_clock_words(card).expect("a clock kept");
        assert!(hover.starts_with(&format!("True Clock {shown}")), "{hover}");
    }

    /// A context with the app's fonts and styles, as the window has.
    pub(super) fn themed() -> egui::Context {
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
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
        app.session.set_settle_s(SETTLE_S);
        app.session
            .load(&[(0.0, Position::DialUp), (30.0, Position::DialDown)], &[]);
        app.position = Position::DialDown;
        app.session.start_settle(30.0);
        assert_eq!(app.session.settles(), [(30.0, 30.0 + SETTLE_S)]);
        assert!(!app.in_position(35.0) && app.in_position(45.0));
        assert_eq!(app.current_spans(), [(30.0 + SETTLE_S, f64::INFINITY)]);
        assert!(app.session.settling_left(35.0).is_some() && app.reading_at_time(35.0).is_none());
        assert!(app.session.settling_left(50.0).is_none() && app.reading_at_time(50.0).is_some());
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

        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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

    pub(super) fn save_failure_fixture(name: &str) -> (TimegrapherApp, PathBuf) {
        let base =
            std::env::temp_dir().join(format!("tg-app-save-failure-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
    pub(super) fn queued_capture(
        app: &mut TimegrapherApp,
        base: &Path,
    ) -> std::sync::mpsc::Sender<Event> {
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
    #[expect(
        clippy::cognitive_complexity,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn recording_failure_stops_capture_and_retains_scratch_with_or_without_input_error() {
        for input_error in [false, true] {
            let (mut app, base) = save_failure_fixture(if input_error {
                "recording-input"
            } else {
                "recording-only"
            });
            app.save_into(&base.join("saved"));
            app.wait_for_save();
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
            app.wait_for_save();
            app.save_again();
            app.wait_for_save();
            app.save_recording();
            app.wait_for_save();
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
        app.wait_for_save();
        assert!(app.session.saved_to().unwrap().join(MANIFEST).is_file());
        app.new_session();
        drop(tx);
        std::fs::remove_dir_all(base).unwrap();
    }

    /// Sound that stops while listening says so in Settings > Microphone,
    /// as an input that won't open does.
    #[test]
    fn sound_that_stops_while_listening_says_so_in_the_microphone_row() {
        let (mut app, base) = save_failure_fixture("input-fault");
        let tx = queued_capture(&mut app, &base);
        assert!(app.input_fault.is_none());
        tx.send(Event::Error("input disconnected".into())).unwrap();
        app.poll();
        assert!(!app.running());
        let fault = app.input_fault.clone().unwrap();
        assert!(fault.starts_with("Input stopped."), "{fault}");
        assert_eq!(app.message.as_ref().unwrap().0, fault);
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
    fn persisted_lift_context_only_edit_stays_dirty_until_successful_save() {
        let (mut app, base) = save_failure_fixture("lift-context");
        app.save_into(&base.join("saved"));
        app.wait_for_save();
        let dest = app.session.saved_to().unwrap().to_path_buf();
        let old = std::fs::read(dest.join(MANIFEST)).unwrap();
        let explicit: Manifest = toml::from_str("lift = 52\nlift_known = true").unwrap();
        app.apply_manifest(&explicit, &[], None);
        assert!(app.lift_known());
        assert!(
            app.metadata_unsaved(),
            "same value, different persisted context"
        );
        std::fs::remove_file(dest.join(MANIFEST)).unwrap();
        std::fs::create_dir(dest.join(MANIFEST)).unwrap();
        app.save_again();
        app.wait_for_save();
        assert!(app.metadata_unsaved());
        assert!(app.message.as_ref().is_some_and(|(_, error)| *error));
        std::fs::remove_dir(dest.join(MANIFEST)).unwrap();
        std::fs::write(dest.join(MANIFEST), old).unwrap();
        app.save_again();
        app.wait_for_save();
        assert!(!app.metadata_unsaved());
        let m: Manifest =
            toml::from_str(&std::fs::read_to_string(dest.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(m.lift_known, Some(true));
        app.session.discard_recording();
        std::fs::remove_dir_all(base).unwrap();
    }

    /// Save hands the copy to a thread of its own and returns: the session
    /// isn't marked saved until the copy is written, listening carries on
    /// meanwhile, and nothing that would start another save or throw the
    /// sound away runs until it is done. The copy holds the sound up to
    /// the Save.
    #[test]
    fn save_copies_in_the_background_while_listening_carries_on() {
        let (mut app, base) = save_failure_fixture("background");
        append_save_test_second(&mut app, 2);
        app.save_into(&base.join("saved"));
        assert!(
            app.saving.is_some(),
            "the copy is written on its own thread"
        );
        assert_eq!(
            app.session.saved_to(),
            None,
            "not saved until it is written"
        );
        append_save_test_second(&mut app, 3);
        app.save_again();
        app.new_session();
        assert!(
            app.session.recorder.is_some(),
            "the sound being copied is kept"
        );
        assert!(app.saving.is_some());
        app.wait_for_save();
        let dest = app.session.saved_to().unwrap().to_path_buf();
        let audio = timegrapher_core::audio::load(&dest.join("audio-001.wav")).unwrap();
        assert_eq!(audio.samples.len(), 2 * 48000);
        assert_eq!(app.unsaved_s(), 1.0);
        app.save_again();
        app.wait_for_save();
        assert_eq!(app.session.saved_to(), Some(dest.as_path()));
        let audio = timegrapher_core::audio::load(&dest.join("audio-001.wav")).unwrap();
        assert_eq!(audio.samples.len(), 3 * 48000);
        app.session.discard_recording();
        std::fs::remove_dir_all(base).unwrap();
    }

    /// The question before a new session, answered Save…, waits for the
    /// save it started, then carries on.
    #[test]
    fn a_save_from_the_question_carries_on_once_it_is_written() {
        let (mut app, base) = save_failure_fixture("confirm-wait");
        app.save_into(&base.join("saved"));
        app.wait_for_save();
        let dest = app.session.saved_to().unwrap().to_path_buf();
        for second in 2..=8 {
            append_save_test_second(&mut app, second);
        }
        assert!(app.needs_save_confirmation());
        app.confirm = Some(Confirm::NewSession);
        // What the question's Save… does.
        app.save_again();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let _ = ctx.run(Default::default(), |ctx| app.confirm_dialog(ctx));
        assert!(app.confirm.is_some(), "still waiting for the save");
        assert!(app.session.recorder.is_some());
        app.wait_for_save();
        let _ = ctx.run(Default::default(), |ctx| app.confirm_dialog(ctx));
        assert!(app.confirm.is_none());
        assert!(app.session.recorder.is_none(), "the new session started");
        let audio = timegrapher_core::audio::load(&dest.join("audio-001.wav")).unwrap();
        assert_eq!(audio.samples.len(), 8 * 48000);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn saved_metadata_edits_warn_without_new_sound_and_reverts_are_clean() {
        let (mut app, base) = save_failure_fixture("metadata-edits");
        app.watch = "original".into();
        app.save_into(&base.join("saved"));
        app.wait_for_save();
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
        app.open_recording(&base.join("other.wav"), true);
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
        app.wait_for_save();
        assert!(!app.needs_save_confirmation());
        app.session.discard_recording();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn short_audio_tail_does_not_become_an_unsaved_metadata_warning() {
        let (mut app, base) = save_failure_fixture("metadata-audio");
        app.change_position(Position::DialDown);
        app.save_into(&base.join("saved"));
        app.wait_for_save();
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
        app.wait_for_save();
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
        app.wait_for_save();
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
        app.wait_for_save();
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
        app.wait_for_save();
        let mut test = guided::Test::new();
        test.readings = Some(Vec::new());
        app.guided = Some(test);
        assert!(app.metadata_unsaved());
        let dest = app.session.saved_to().unwrap().to_path_buf();
        std::fs::create_dir(dest.join(guided::RESULTS_FILE)).unwrap();
        app.save_again();
        app.wait_for_save();
        assert!(app.metadata_unsaved());
        std::fs::remove_dir(dest.join(guided::RESULTS_FILE)).unwrap();
        app.save_again();
        app.wait_for_save();
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
        app.wait_for_save();
        let dest = app.session.saved_to().unwrap();
        assert_eq!(dest.parent().unwrap().as_os_str(), parent.as_os_str());
        assert_eq!(
            app.message.as_ref(),
            Some(&(
                format!("Saved {} of sound in {}", length_text(1.0), dest.display()),
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
        app.wait_for_save();
        assert!(app
            .message
            .as_ref()
            .is_some_and(|(m, error)| *error && m.contains(MANIFEST)));
        assert_eq!(app.session.saved_to(), None);
        assert_eq!(app.unsaved_s(), 1.0);
        assert_eq!(saved_file_bytes(&dest), before);
        std::fs::remove_dir(dest.join(MANIFEST)).unwrap();
        app.save_into(&parent);
        app.wait_for_save();
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
        app.wait_for_save();
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
        app.wait_for_save();
        assert!(app
            .message
            .as_ref()
            .is_some_and(|(m, error)| *error && m.contains(guided::RESULTS_FILE)));
        assert_eq!(app.session.saved_to(), Some(dest.as_path()));
        assert_eq!(app.unsaved_s(), 1.0);
        assert_eq!(saved_file_bytes(&dest), before);
        std::fs::remove_dir(dest.join(guided::RESULTS_FILE)).unwrap();
        app.save_into(&parent);
        app.wait_for_save();
        assert_eq!(app.unsaved_s(), 0.0);
        let kept = std::fs::read(dest.join(guided::RESULTS_FILE)).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&kept).unwrap();
        assert_eq!(json["format"], guided::RESULTS_FORMAT);
        // A later unfinished test (like a 24-hour half) supplies no new full-wind
        // result. Preserve the earlier result as a separate comparison artifact.
        app.guided = Some(guided::Test::new());
        app.save_into(&parent);
        app.wait_for_save();
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
        app.wait_for_save();
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
        app.open_recording(&elsewhere, true);
        assert!(matches!(app.confirm, Some(Confirm::Open { .. })));
        assert!(app.session.recorder.is_some() && app.unsaved_s() > 19.0);
        app.input = Input::Microphone;
        assert!(!app.mic_session_set_aside());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Quitting without the close being asked first (on macOS, Cmd+Q and
    /// Quit in the app menu or the Dock) keeps the unsaved sound, and the
    /// next start offers it back; a quit the user answered throws it away
    /// as before. A folder another copy is recording into is never offered.
    /// The Watches view lists the library's watches, adds one (making
    /// the library for the first), shows a watch's details and takes, and
    /// Save… then saves into that watch with no question.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn the_watches_pane_adds_a_watch_and_saves_its_takes_into_it() {
        let base = std::env::temp_dir().join(format!("tg-app-watches-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut app = synthetic(5.0, 0.0);
        app.use_library(Some(root.clone()));
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let pane = |ctx: &egui::Context, app: &mut TimegrapherApp| {
            let mut all = String::new();
            for _ in 0..2 {
                let out = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.pane_tools(ui, Pane::Watch);
                        app.watch_pane(ui);
                    });
                    app.watch_form_dialog(ctx);
                });
                all = format!("{:?} {:?}", out.shapes, theme::tools_drawn(ctx));
            }
            all
        };
        let drawn = pane(&ctx, &mut app);
        assert!(
            drawn.contains("No watches yet") && drawn.contains("New Watch…"),
            "{drawn}"
        );
        // Nothing is made on disk until the first watch.
        assert!(!root.exists());

        let mut w = library::Watch::new("Pepsi");
        w.lift = Some(53.0);
        w.bph = Some(28800);
        w.notes = Some(String::new());
        app.watch_form = Some(watches::Form {
            watch: w,
            new: true,
            error: None,
        });
        let form = pane(&ctx, &mut app);
        for t in ["New Watch", "Add Field", "Add Watch", "Lift Angle"] {
            assert!(form.contains(t), "{t}: {form}");
        }
        assert!(!form.contains("Archived"), "no Archived mark: {form}");
        // Nothing on show: the new watch's details are the next take's.
        // (Readings on show keep theirs: see
        // a_recording_on_show_is_kept_in_a_new_watch_with_its_charts.)
        app.new_session();
        app.save_watch_form();
        assert!(app.watch_form.is_none());
        assert!(library::Library::is_library(&root));
        assert_eq!(
            (app.watch.as_str(), app.lift_deg, app.bph),
            ("Pepsi", 53.0, Some(28800))
        );
        let drawn = pane(&ctx, &mut app);
        for t in [
            "Pepsi",
            "No takes yet",
            "New Take",
            "Edit…",
            "Add Photos…",
            "Listen",
            "Positions Test",
            "Power Reserve Test",
        ] {
            assert!(drawn.contains(t), "{t}: {drawn}");
        }
        assert_eq!(app.status_words(), "New Take · Pepsi · Not Started");

        // Save… with the watch picked saves into it, named by its start.
        let scratch = base.join("unsaved");
        app.session.listen();
        let info = app.info("Pepsi", 48000, 16);
        let mut r = Recorder::start(&scratch, info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 20],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        let started = watches::local_minute(r.started_utc()).unwrap();
        app.session.recorder = Some(r);
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        app.save_again();
        app.wait_for_save();
        let takes = root.join("Pepsi").join(library::TAKES);
        let saved: Vec<PathBuf> = std::fs::read_dir(&takes)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(saved.len(), 1, "{saved:?}");
        let name = saved[0].file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(&started), "{name}");
        assert!(saved[0].join(MANIFEST).is_file());
        assert_eq!(app.session.saved_to(), Some(saved[0].as_path()));
        let drawn = pane(&ctx, &mut app);
        // Its row says when it started, as its folder's name does.
        let row = watches::started_words(&app.open_group().unwrap().takes[0]);
        assert!(drawn.contains(&row), "{row}: {drawn}");
        // Save Again saves into the same take.
        app.save_again();
        app.wait_for_save();
        assert_eq!(std::fs::read_dir(&takes).unwrap().count(), 1);

        // The list says so; a name already in use is refused.
        app.watch_open = None;
        let drawn = pane(&ctx, &mut app);
        assert!(drawn.contains("1 take"), "{drawn}");
        app.watch_form = Some(watches::Form {
            watch: library::Watch::new("pepsi"),
            new: true,
            error: None,
        });
        app.save_watch_form();
        let refused = app.watch_form.as_ref().and_then(|f| f.error.clone());
        assert!(refused.is_some_and(|e| e.contains("already")));
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A take whose sound is open while it is read for the Watches list
    /// isn't moved, renamed or deleted until it is done (Windows refuses
    /// to while a file in it is open): the app says to wait.
    #[test]
    fn a_take_being_read_for_the_list_waits_to_be_moved() {
        let app = TimegrapherApp::with_devices(Vec::new(), None);
        let take = std::env::temp_dir().join("Takes").join("2026-10-20 14.32");
        assert_eq!(app.take_busy(&take, false), None);
        app.take_readings.set_now(Some(take.clone()));
        for keep in [false, true] {
            let why = app.take_busy(&take, keep).unwrap();
            assert!(why.contains("being read for the Watches list"), "{why}");
        }
        app.take_readings
            .set_now(Some(take.with_file_name("another")));
        assert_eq!(app.take_busy(&take, false), None);
    }

    /// A take's row in the Watches list shows the figures core's
    /// `library::reading` gives for that take, worked out from its sound:
    /// the app's path to them, not only a reading handed to the table.
    #[test]
    fn a_takes_row_shows_its_own_reading_from_core() {
        use timegrapher_core::synth::{generate, SynthConfig};
        let base = std::env::temp_dir().join(format!("tg-app-take-reading-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        let dir = lib.add(&library::Watch::new("Pepsi")).unwrap();
        let id = lib.watches[0].watch.id.clone();
        drop(lib);
        // A take as the app saves one: 40 s of a watch 10 s/d fast at
        // 270°, its session file, and other figures in session.json.
        let take = dir.join(library::TAKES).join("2026-10-20 14.32 Dial Up");
        std::fs::create_dir_all(&take).unwrap();
        std::fs::write(
            take.join(MANIFEST),
            "bph = 28800\n\n[[recording]]\nfile = \".\"\nposition = \"DU\"\n",
        )
        .unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 40.0,
                rate_s_per_day: 10.0,
                beat_error_ms: 0.6,
                ..Default::default()
            },
            |_| 270.0,
            |_| 0.0,
        );
        timegrapher_core::audio::write_wav(&take.join("take_part01-001.wav"), &audio).unwrap();
        std::fs::write(
            take.join("session.json"),
            r#"{"started_utc":"2026-10-20T21:32:00Z","duration_s":40.0,
                "result":{"rate_s_per_day":4.2,"amplitude_deg":285.0,"beat_error_unlock_ms":-0.6}}"#,
        )
        .unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        app.pick_watch(&id);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let pane = |app: &mut TimegrapherApp| {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.watch_pane(ui));
            });
            format!("{:?}", out.shapes)
        };
        pane(&mut app);
        app.take_readings.wait();
        let drawn = pane(&mut app);
        let want = library::reading(&take).unwrap();
        assert_eq!(app.take_readings.reading(&take), Some(&want));
        let t = app.open_watch().unwrap().takes[0].clone();
        let cells = watches::take_cells(&t, None, Some(&want));
        for c in &cells[3..6] {
            assert!(!c.is_empty() && drawn.contains(c.as_str()), "{c}: {drawn}");
        }
        // Not the figures the take was saved with.
        assert!(!cells[3..6].contains(&"+4.2".to_string()), "{cells:?}");
        // A report written into the take's folder (as `timegrapher
        // session` leaves one) changes its size but not what its reading
        // reads: the figures stay on show while it is asked again.
        let report = take.join(timegrapher_core::take::REPORT_DIR);
        std::fs::create_dir_all(&report).unwrap();
        std::fs::write(report.join("summary.json"), "{}").unwrap();
        app.take_readings.want(&take, t.bytes + 2, &ctx);
        assert_eq!(app.take_readings.reading(&take), Some(&want));
        app.take_readings.wait();
        assert_eq!(app.take_readings.reading(&take), Some(&want));
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A watch's photos lead its row in the list and show as a strip in
    /// its details, read in the background; one that can't be read says
    /// so by name. Delete… asks, then deletes only that photo.
    #[test]
    fn a_watchs_photos_show_as_thumbnails_and_one_can_be_deleted() {
        let base = std::env::temp_dir().join(format!("tg-app-photos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        let dir = lib.add(&library::Watch::new("Pepsi")).unwrap();
        let id = lib.watches[0].watch.id.clone();
        let data = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("photos");
        let broken = base.join("broken.jpg");
        std::fs::write(&broken, b"not a photo").unwrap();
        lib.add_photos(&id, &[data.join("plain.heic"), broken])
            .unwrap();
        drop(lib);
        let photos = dir.join(library::PHOTOS);
        let (good, bad) = (photos.join("plain.heic"), photos.join("broken.jpg"));
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let pane = |ctx: &egui::Context, app: &mut TimegrapherApp| {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, Pane::Watch);
                    app.watch_pane(ui);
                });
                app.take_ask_dialog(ctx);
            });
            format!("{:?} {:?}", out.shapes, theme::tools_drawn(ctx))
        };
        // The list asks for the first photo (by name: broken.jpg) only.
        pane(&ctx, &mut app);
        app.pick_watch(&id);
        let ready = |app: &mut TimegrapherApp, p: &Path| {
            matches!(app.thumbs.get(&ctx, p), photos::Thumb::Ready(_))
        };
        let failed = |app: &mut TimegrapherApp, p: &Path| {
            matches!(app.thumbs.get(&ctx, p), photos::Thumb::Failed(_))
        };
        for _ in 0..200 {
            pane(&ctx, &mut app);
            if ready(&mut app, &good) && failed(&mut app, &bad) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(ready(&mut app, &good) && failed(&mut app, &bad));
        let drawn = pane(&ctx, &mut app);
        for t in ["Add Photos…", "broken.jpg", "Can't be shown"] {
            assert!(drawn.contains(t), "{t}: {drawn}");
        }
        app.take_ask = Some(watches::Ask::DeletePhoto { photo: bad.clone() });
        pane(&ctx, &mut app);
        let asked = pane(&ctx, &mut app);
        assert!(asked.contains("Delete \\\"broken.jpg\\\"?"), "{asked}");
        let ask = app.take_ask.take().unwrap();
        app.answer_take_ask(ask);
        assert!(!bad.exists() && good.exists());
        assert_eq!(app.open_watch().unwrap().photos, [good]);
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn open_adds_a_recording_to_a_watch_asking_only_for_what_is_missing() {
        let base = std::env::temp_dir().join(format!("tg-app-open-into-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let wav = |p: &Path| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut w = hound::WavWriter::create(p, spec).unwrap();
            for _ in 0..8000 {
                w.write_sample(0i16).unwrap();
            }
            w.finalize().unwrap();
        };
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        app.pick_watch(&pepsi);

        // A lone file: its position is in its name, the rest is asked for.
        let lone = base.join("outside").join("pepsi_DU.wav");
        wav(&lone);
        let mut a = app.open_ask_for(&lone).unwrap();
        assert!(a.add && a.watch.as_deref() == Some(pepsi.as_str()));
        let need = app.open_need(&a);
        assert_eq!(
            need,
            OpenNeed {
                bph: true,
                lift: true,
                position: false,
                started: true
            }
        );
        let ctx = egui::Context::default();
        theme::install(&ctx);
        app.open_ask = Some(a.clone());
        let mut text = String::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| app.open_ask_dialog(ctx));
            text = format!("{:?}", out.shapes);
        }
        for asked in ["Beat Rate", "Lift Angle", "Started", "Add and Open"] {
            assert!(text.contains(asked), "{asked}");
        }
        assert!(!text.contains("Choose a Position"));
        app.open_ask = None;
        a.bph = Some(21600);
        a.started = "2026-10-22 14:30".into();
        app.answer_open_ask(a, need);
        assert!(app.open_ask.is_none());
        let Ok(Some(lib)) = &app.library else {
            panic!()
        };
        let takes = &lib.find("Pepsi").unwrap().takes;
        assert_eq!(takes.len(), 1);
        assert!(takes[0].name.starts_with("2026-10-22 14.30"));
        let take = lib.find("Pepsi").unwrap().take_dir(&takes[0]);
        let m: Manifest =
            toml::from_str(&std::fs::read_to_string(take.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!((m.bph, m.lift_known), (Some(21600), Some(false)));
        assert_eq!(m.watch.as_deref(), Some("Pepsi"));
        assert_eq!(m.recordings[0].position.as_deref(), Some("DU"));
        assert!(takes[0].started_utc.is_some() && take.join("pepsi_DU.wav").is_file());
        assert!(lone.is_file());

        // A take folder: only what its session file leaves out is written,
        // and the rest of the file is kept as it was.
        let folder = base.join("outside take");
        wav(&folder.join("take_part01-001.wav"));
        std::fs::write(
            folder.join(MANIFEST),
            "# by hand\nlift = 50.0\n\n[[recording]]\nfile = \"take_part01-001.wav\"\n",
        )
        .unwrap();
        let mut a = app.open_ask_for(&folder).unwrap();
        let need = app.open_need(&a);
        assert_eq!(
            need,
            OpenNeed {
                bph: true,
                lift: false,
                position: true,
                started: true
            }
        );
        a.bph = Some(28800);
        a.position = Some(Position::CrownLeft);
        a.started = "2026-10-22 15:00".into();
        app.answer_open_ask(a, need);
        let Ok(Some(lib)) = &app.library else {
            panic!()
        };
        let entry = lib.find("Pepsi").unwrap();
        assert_eq!(entry.takes.len(), 2);
        let take = entry.dir.join("takes").join("outside take");
        let text = std::fs::read_to_string(take.join(MANIFEST)).unwrap();
        assert!(text.starts_with("# by hand"));
        let m: Manifest = toml::from_str(&text).unwrap();
        assert_eq!((m.bph, m.lift), (Some(28800), Some(50.0)));
        assert_eq!(m.recordings[0].position.as_deref(), Some("CL"));
        assert!(started_in(&take).is_some());
        // What is in the watches already opens straight away.
        assert!(app.open_ask_for(&take).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Keep in a Watch… of a recording on show asks only what the take
    /// on show doesn't say: not its position, beat rate or lift angle,
    /// which are written with it. And Keep stays greyed out until what is
    /// asked is given, for an existing watch as for a new one.
    #[test]
    fn keeping_a_recording_asks_only_what_is_not_on_show() {
        let base = std::env::temp_dir().join(format!("tg-app-keep-shown-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        // No position in its name, no session file: nothing it says.
        let lone = base.join("outside").join("bench.wav");
        std::fs::create_dir_all(lone.parent().unwrap()).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&lone, spec).unwrap();
        for _ in 0..8000 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        let mut app = synthetic(5.0, 0.0);
        app.use_library(Some(root.clone()));
        app.input = Input::File;
        app.opened_path = lone.display().to_string();
        app.position = Position::CrownLeft;
        app.bph = Some(28800);
        app.lift_deg = 50.0;

        // Opening it from outside asks for all of it, and Add and Open
        // waits for it even with a watch picked.
        let mut a = app.open_ask_for(&lone).unwrap();
        a.watch = Some(pepsi.clone());
        a.new_watch = false;
        a.add = true;
        // Its start, guessed from the file, left out to see it asked for.
        a.started.clear();
        let need = app.open_need(&a);
        assert!(need.bph && need.position && need.started);
        assert!(!TimegrapherApp::open_ready(&a, need));
        a.bph = Some(21600);
        a.position = Some(Position::DialUp);
        assert!(!TimegrapherApp::open_ready(&a, need));
        a.started = "2026-10-22 14:30".into();
        assert!(TimegrapherApp::open_ready(&a, need));

        // Keep in a Watch: only when it started is asked (guessed from
        // the file, to be checked).
        app.keep_into(&pepsi);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut text = String::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| app.open_ask_dialog(ctx));
            text = format!("{:?}", out.shapes);
        }
        assert!(text.contains("Started"), "{text}");
        for not_asked in ["Choose a Position", "Choose a Beat Rate", "Lift Angle"] {
            assert!(!text.contains(not_asked), "{not_asked}");
        }
        // The watch shows while asked; Esc (as Cancel) puts back what was
        // picked before, here no watch.
        assert_eq!(app.watch_open.as_deref(), Some(pepsi.as_str()));
        let esc = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(esc, |ctx| app.open_ask_dialog(ctx));
        assert!(app.open_ask.is_none());
        assert_eq!(app.watch_open, None);
        app.keep_into(&pepsi);
        let mut a = app.open_ask.take().expect("asked when it started");
        a.started.clear();
        assert!(!TimegrapherApp::open_ready(&a, app.open_need(&a)));
        a.started = "2026-10-22 14:30".into();
        let need = app.open_need(&a);
        assert!(TimegrapherApp::open_ready(&a, need));
        app.answer_open_ask(a, need);
        let Ok(Some(lib)) = &app.library else {
            panic!()
        };
        let e = lib.find("Pepsi").unwrap();
        let take = e.take_dir(&e.takes[0]);
        let m: Manifest =
            toml::from_str(&std::fs::read_to_string(take.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!((m.bph, m.lift), (Some(28800), Some(50.0)));
        assert_eq!(m.recordings[0].position.as_deref(), Some("CL"));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// One part of a take of several, opened alone, is kept in a watch
    /// alone, with what is on show; a take of one part goes in whole.
    #[test]
    fn one_part_of_a_take_is_kept_alone() {
        let base = std::env::temp_dir().join(format!("tg-app-keep-part-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let take = base.join("outside").join("run");
        std::fs::create_dir_all(&take).unwrap();
        std::fs::write(take.join("session.json"), "{}").unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let part = |n: usize| {
            let p = take.join(format!("run_part{n:02}.wav"));
            let mut w = hound::WavWriter::create(&p, spec).unwrap();
            for _ in 0..8000 {
                w.write_sample(0i16).unwrap();
            }
            w.finalize().unwrap();
            p
        };
        let first = part(1);
        let mut app = synthetic(5.0, 0.0);
        app.use_library(Some(root.clone()));
        app.input = Input::File;
        app.opened_path = first.display().to_string();
        // Alone in its folder, the take goes in whole.
        app.keep_into(&pepsi);
        assert!(
            matches!(&app.take_ask, Some(watches::Ask::Add { from, .. }) if *from == take),
            "{:?}",
            app.take_ask
        );
        app.take_ask = None;
        // One of two: it goes in alone, asked as a recording.
        part(2);
        app.keep_into(&pepsi);
        assert!(app.take_ask.is_none(), "{:?}", app.take_ask);
        let shown = app.open_ask.as_ref().map(|a| a.path.clone());
        assert!(shown.is_none() || shown == Some(first.clone()), "{shown:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_recording_on_show_is_kept_in_a_new_watch_with_its_charts() {
        let base = std::env::temp_dir().join(format!("tg-app-keep-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let lone = base.join("outside").join("meteorite_DU.wav");
        std::fs::create_dir_all(lone.parent().unwrap()).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&lone, spec).unwrap();
        for _ in 0..8000 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        // A recording opened from outside the watches, its readings on show.
        let mut app = synthetic(5.0, 0.0);
        app.use_library(Some(root.clone()));
        app.input = Input::File;
        app.opened_path = lone.display().to_string();
        let shown = (app.trend.len(), app.bph, app.lift_deg, app.watch.clone());
        assert!(shown.0 > 0);
        assert_eq!(app.addable_take(), Some(lone.clone()));

        // Keep in a Watch… > New Watch…, filled in, then Done.
        app.open_new_watch_form();
        app.keep_after_form = true;
        let f = app.watch_form.as_mut().unwrap();
        f.watch.name = "Meteorite".into();
        f.watch.bph = Some(21600);
        f.watch.lift = Some(50.0);
        app.save_watch_form();
        assert!(app.watch_form.is_none() && !app.keep_after_form);
        // The readings on show keep the settings they were measured with.
        assert_eq!(
            (app.trend.len(), app.bph, app.lift_deg, app.watch.clone()),
            shown
        );
        let id = app.open_watch().unwrap().watch.id.clone();
        // Then it goes into the new watch, asking only what neither says.
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut text = String::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| app.open_ask_dialog(ctx));
            text = format!("{:?}", out.shapes);
        }
        assert!(
            text.contains("Keep \\\"meteorite_DU.wav\\\" in a Watch"),
            "{text}"
        );
        assert!(
            text.contains("Started") && !text.contains("Just Open It"),
            "{text}"
        );
        let mut a = app.open_ask.take().expect("asked what is missing");
        assert!(a.add && !a.new_watch && a.watch.as_deref() == Some(id.as_str()));
        a.started = "2026-10-23 10:50".into();
        let need = app.open_need(&a);
        app.answer_open_ask(a, need);
        let Ok(Some(lib)) = &app.library else {
            panic!()
        };
        let e = lib.find("Meteorite").unwrap();
        assert_eq!(e.takes.len(), 1);
        let take = e.take_dir(&e.takes[0]);
        assert!(take.join("meteorite_DU.wav").is_file() && lone.is_file());
        assert!(Path::new(&app.opened_path).starts_with(&root));
        assert_eq!(app.addable_take(), None);

        // File > New Session lets go of the recording: a new take follows.
        app.new_session();
        assert!(app.input == Input::Microphone && app.opened_path.is_empty());
        assert_eq!(app.status_words(), "New Take · Meteorite · Not Started");
        // And the watch's settings apply to it (set up as New Take does,
        // without opening a real input).
        app.take_setup();
        assert_eq!(
            (app.watch.as_str(), app.bph, app.lift_deg),
            ("Meteorite", Some(21600), 50.0)
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    #[expect(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn quick_listen_keeps_its_take_in_unsorted_until_filed() {
        let base = std::env::temp_dir().join(format!("tg-app-quick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        app.pick_watch(&pepsi);
        assert_eq!(app.watch, "Pepsi");
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let pane = |ctx: &egui::Context, app: &mut TimegrapherApp| {
            let mut all = String::new();
            for _ in 0..2 {
                let out = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.pane_tools(ui, Pane::Watch);
                        app.watch_pane(ui);
                    });
                });
                all = format!("{:?} {:?}", out.shapes, theme::tools_drawn(ctx));
            }
            all
        };
        // A session with 20 s of sound not yet saved, paused.
        let record = |app: &mut TimegrapherApp, scratch: &Path| {
            app.session.listen();
            let info = app.info("", 48000, 16);
            let mut r = Recorder::start(scratch, info).unwrap();
            r.write(&timegrapher_core::capture::Block {
                samples: vec![0.0; 48000 * 20],
                at: std::time::SystemTime::now(),
            })
            .unwrap();
            app.session.recorder = Some(r);
            app.session.set_state(session::State::Paused {
                since: Instant::now(),
            });
        };
        record(&mut app, &base.join("unsaved-1"));
        assert!(!app.quick_listen_ready(false));
        assert!(matches!(app.confirm, Some(Confirm::QuickListen)));
        assert!(!app.quick_listen && app.watch_open.is_some(), "nothing yet");
        // Answered (no input is opened here: the test touches no device).
        app.confirm = None;
        assert!(app.quick_listen_ready(true));
        assert!(app.session.recorder.is_none(), "the unsaved session went");
        assert!(app.quick_listen && app.unsorted_open && app.watch_open.is_none());
        assert_eq!(app.watch, "");
        assert!(!app.running());
        // The list, Unsorted Takes marked; its row shows its takes.
        assert!(app.list_shown());
        let drawn = pane(&ctx, &mut app);
        for t in ["Unsorted Takes", "Quick Listen, no watch", "Pepsi"] {
            assert!(drawn.contains(t), "{t}: {drawn}");
        }
        app.list_for = None;
        let drawn = pane(&ctx, &mut app);
        for t in ["Unsorted Takes", "No takes here", "Takes"] {
            assert!(drawn.contains(t), "{t}: {drawn}");
        }

        // Stop keeps nothing; Save… saves the take into Unsorted, and a
        // Stop after that adds the sound since to it.
        record(&mut app, &base.join("unsaved-2"));
        app.stop_listening();
        app.wait_for_save();
        let unsorted = root.join(library::UNSORTED).join(library::TAKES);
        let none = std::fs::read_dir(&unsorted).map_or(0, |d| d.count());
        assert_eq!(none, 0, "a Quick Listen take isn't kept at Stop");
        // Nor asked about: it goes without a question unless saved.
        assert!(!app.needs_save_confirmation());
        assert!(app.quick_unsaved());
        // The Listen card's Save offers Unsorted Takes until it is saved.
        assert!(app.saves_into_unsorted());
        app.save_again();
        app.wait_for_save();
        let saved: Vec<PathBuf> = std::fs::read_dir(&unsorted)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(saved.len(), 1, "{saved:?}");
        assert_eq!(app.session.saved_to(), Some(saved[0].as_path()));
        assert!(!app.saves_into_unsorted());
        assert!(!app.needs_save_confirmation());
        app.stop_listening();
        app.wait_for_save();
        assert_eq!(std::fs::read_dir(&unsorted).unwrap().count(), 1);
        let name = file_name(&saved[0]);
        let drawn = pane(&ctx, &mut app);
        let row = watches::started_words(&app.open_group().unwrap().takes[0]);
        assert!(drawn.contains(&row), "{row}: {drawn}");

        // The list shows Unsorted at its top, with Quick Listen.
        app.unsorted_open = false;
        let drawn = pane(&ctx, &mut app);
        for t in ["Unsorted Takes", "Quick Listen, no watch · 1 take", "Pepsi"] {
            assert!(drawn.contains(t), "{t}: {drawn}");
        }
        app.unsorted_open = true;
        let e = app.open_group().unwrap();
        assert!(e.watch.id.is_empty() && e.takes.len() == 1);

        // Filed under Pepsi while stopped: Save Again follows it there.
        app.take_readings.wait();
        app.move_take(&saved[0], &pepsi);
        let filed = root.join("Pepsi").join(library::TAKES).join(&name);
        assert!(filed.join(MANIFEST).is_file() && !saved[0].exists());
        assert!(app
            .message
            .as_ref()
            .is_some_and(|(m, _)| m.contains("Filed") && m.contains("Pepsi")));
        assert_eq!(app.session.saved_to(), Some(filed.as_path()));
        assert!(app.open_group().unwrap().takes.is_empty());
        // Deleted while the stopped session is saved into it: the session
        // lets go of it first, rather than refusing.
        app.answer_take_ask(watches::Ask::Delete {
            take: filed.clone(),
            bytes: 0,
        });
        assert!(!filed.exists(), "{:?}", app.message);
        assert_eq!(app.session.saved_to(), None);

        // Unsorted Takes shows its takes at once, not a list left over.
        app.list_for = Some((None, true));
        app.pick_unsorted();
        assert!(!app.list_shown());

        // Picking a watch ends Quick Listen: Save… saves into the watch.
        app.list_for = Some((None, true));
        app.pick_watch(&pepsi);
        assert!(!app.quick_listen && !app.unsorted_open);
        assert!(app.list_for.is_none());
        assert_eq!(
            app.watch_takes_dir(),
            Some(root.join("Pepsi").join(library::TAKES))
        );
        app.watch_open = None;
        assert_eq!(app.watch_takes_dir(), None, "no watch: Save… asks where");
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A take of the watch picked is saved into it at Stop; New Take asks
    /// before unsaved sound goes; Keep in a Watch… saves a Quick Listen
    /// take into the watch picked from its menu.
    #[test]
    fn a_watchs_take_is_kept_at_stop_and_quick_listen_is_kept_in_a_watch() {
        let base = std::env::temp_dir().join(format!("tg-app-new-take-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let takes = root.join("Pepsi").join(library::TAKES);
        let count = || std::fs::read_dir(&takes).map_or(0, |d| d.count());
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        // 20 s of sound as a run would keep it (no device is opened).
        let record = |app: &mut TimegrapherApp, scratch: &Path| {
            app.session.listen();
            let info = app.info("", 48000, 16);
            let mut r = Recorder::start(scratch, info).unwrap();
            r.write(&timegrapher_core::capture::Block {
                samples: vec![0.0; 48000 * 20],
                at: std::time::SystemTime::now(),
            })
            .unwrap();
            app.session.recorder = Some(r);
        };
        app.pick_watch(&pepsi);
        record(&mut app, &base.join("unsaved-1"));
        app.stop_listening();
        app.wait_for_save();
        assert_eq!(count(), 1, "kept in the watch at Stop");
        assert!(!app.needs_save_confirmation());

        // Quick Listen: nothing kept at Stop, and nothing asked: a new take
        // lets it go.
        app.confirm = None;
        assert!(app.quick_listen_ready(true));
        record(&mut app, &base.join("unsaved-2"));
        app.stop_listening();
        app.wait_for_save();
        assert_eq!(count(), 1);
        assert!(!app.needs_save_confirmation() && app.quick_unsaved());
        assert!(
            app.session.recorder.is_some(),
            "until something replaces it"
        );
        // Keep in a Watch… saves it into Pepsi, as a take of Pepsi.
        app.keep_in_watch(&pepsi);
        app.wait_for_save();
        assert_eq!(count(), 2);
        assert!(!app.quick_listen && app.watch_open.as_deref() == Some(pepsi.as_str()));
        assert_eq!(app.watch, "Pepsi");
        assert!(!app.needs_save_confirmation());
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A Quick Listen take kept in a watch is saved with the watch's
    /// calibre, lift angle and beat rate; a take an older app kept there
    /// without them opens with the watch's.
    #[test]
    fn a_take_kept_in_a_watch_takes_the_watchs_calibre() {
        let base = std::env::temp_dir().join(format!("tg-app-keep-cal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
        let mut diver = library::Watch::new("Diver");
        diver.calibre = Some(c.calibre.clone());
        diver.lift = Some(55.0);
        lib.add(&diver).unwrap();
        let id = lib.find("Diver").unwrap().watch.id.clone();
        drop(lib);
        let takes = root.join("Diver").join(library::TAKES);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        assert!(app.quick_listen_ready(true));
        assert!(app.calibre.is_empty() && app.bph.is_none());
        app.session.listen();
        let info = app.info("", 48000, 16);
        let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 20],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        app.session.recorder = Some(r);
        app.stop_listening();
        app.wait_for_save();
        app.keep_in_watch(&id);
        app.wait_for_save();
        let take = std::fs::read_dir(&takes)
            .unwrap()
            .next()
            .expect("a take in the watch")
            .unwrap()
            .path();
        let (m, _) = read_manifest(&take).unwrap().expect("its session file");
        assert_eq!(m.calibre.as_deref(), Some(c.calibre.as_str()));
        // The calibre's lift angle over the watch's own, where it gives one.
        let lift = c.lift_angle_deg.expect("the table gives the 2824-2's lift");
        assert_ne!(lift, 55.0);
        assert_eq!(m.lift, Some(lift));
        assert_eq!(m.bph, c.bph);
        drop(app);

        // One kept without them opens with the watch's.
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        let bare = Manifest {
            lift: Some(DEFAULT_LIFT_DEG),
            ..Default::default()
        };
        app.apply_manifest(&bare, &[], None);
        app.watch_fills_in(&take, &bare);
        assert_eq!(app.calibre, c.calibre);
        assert_eq!(app.lift_deg, lift);
        assert_eq!(app.bph, c.bph);
        // A take outside the library keeps what its file says.
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        app.apply_manifest(&bare, &[], None);
        app.watch_fills_in(&base.join("elsewhere"), &bare);
        assert!(app.calibre.is_empty() && app.bph.is_none());
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A watch's takes are kept as their analysis alone, moved
    /// and deleted from the Watches view, and a take from outside is added
    /// to a watch; the recording opened follows a take that moves.
    #[test]
    #[expect(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn a_watchs_takes_are_kept_moved_deleted_and_added() {
        let base = std::env::temp_dir().join(format!("tg-app-take-ops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        let a = lib.add(&library::Watch::new("Pepsi")).unwrap();
        lib.add(&library::Watch::new("Daytona")).unwrap();
        drop(lib);
        // A take with its sound, as the app saves one.
        let take_at = |dir: &Path| {
            std::fs::create_dir_all(dir).unwrap();
            let audio = generate(
                &SynthConfig {
                    duration_s: 40.0,
                    rate_s_per_day: 4.0,
                    ..Default::default()
                },
                |_| 280.0,
                |_| 0.0,
            );
            timegrapher_core::audio::write_wav(&dir.join("take_part01-001.wav"), &audio).unwrap();
            std::fs::write(
                dir.join(MANIFEST),
                "# kept as written\nbph = 28800\nlift = 52\n\n[[recording]]\nfile = \".\"\n\
                 position = \"DU\"\n",
            )
            .unwrap();
            std::fs::write(
                dir.join("session.json"),
                r#"{"started_utc":"2026-10-20T17:00:00Z","duration_s":40.0}"#,
            )
            .unwrap();
        };
        let take = a.join(library::TAKES).join("2026-10-20 10.00 Dial Up");
        take_at(&take);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        let pepsi = app
            .library
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .find("Pepsi")
            .unwrap();
        let daytona_id = app
            .library
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .find("Daytona")
            .unwrap()
            .watch
            .id
            .clone();
        let pepsi_id = pepsi.watch.id.clone();
        app.pick_watch(&pepsi_id);

        // Keep Analysis Only: the sound goes only once the analysis is in place.
        app.take_readings.wait();
        app.start_take_keep(take.clone());
        assert!(app.take_busy(&take, false).is_some());
        let t = Instant::now();
        while app.take_keep.is_some() && t.elapsed().as_secs() < 120 {
            app.poll_take_keep();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.take_keep.is_none(), "it finished");
        assert!(analysis_take::is_analysis_only(&take), "{:?}", app.message);
        assert!(!take.join("take_part01-001.wav").exists());
        let toml = std::fs::read_to_string(take.join(MANIFEST)).unwrap();
        assert!(
            toml.contains("# kept as written") && toml.contains("audio = false"),
            "{toml}"
        );
        assert!(take.join("session.json").is_file());
        let e = app.open_watch().unwrap();
        assert!(!e.takes[0].audio && !e.takes[0].incomplete);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.watch_side(ui));
        });
        assert!(format!("{:?}", out.shapes).contains("Analysis Only"));
        // A click picks the take without opening it.
        app.take_action(Some(watches::TakeAction::Pick(take.clone())));
        assert_eq!(app.take_picked.as_ref(), Some(&take));
        assert!(app.batch.is_none() && app.live.is_none());
        // Its opened analysis reads back.
        app.take_action(Some(watches::TakeAction::Open(take.clone())));
        assert!(app.batch.is_some());
        let t = Instant::now();
        while app.batch.is_some() && t.elapsed().as_secs() < 60 {
            app.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.live.is_some());
        assert_eq!(app.take_on_show().as_ref(), Some(&take));
        // Opened again while on show, it is not read again.
        app.take_action(Some(watches::TakeAction::Open(take.clone())));
        assert!(app.batch.is_none());

        let name = file_name(&take);

        // Moved to Daytona, then deleted there.
        app.take_readings.wait();
        app.move_take(&take, &daytona_id);
        let d = root.join("Daytona").join(library::TAKES).join(&name);
        assert!(d.is_dir() && !take.exists());
        assert_eq!(Path::new(&app.opened_path), d);
        app.answer_take_ask(watches::Ask::Delete {
            take: d.clone(),
            bytes: 0,
        });
        assert!(!d.exists());
        // The take was on show: the window is empty again, as at the start.
        assert!(app.live.is_none() && app.opened_path.is_empty());
        assert!(app.take_on_show().is_none());

        // A take from outside, opened, is added by a move.
        let outside = base.join("From a USB Stick");
        take_at(&outside);
        app.input = Input::File;
        app.opened_path = outside.display().to_string();
        assert_eq!(app.addable_take(), Some(outside.clone()));
        app.answer_take_ask(watches::Ask::Add {
            from: outside.clone(),
            watch: Some(pepsi_id.clone()),
            keep_original: false,
        });
        let added = a.join(library::TAKES).join("From a USB Stick");
        assert!(added.join("take_part01-001.wav").is_file() && !outside.exists());
        assert_eq!(Path::new(&app.opened_path), added);
        assert_eq!(app.addable_take(), None);
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_quit_that_cannot_ask_keeps_unsaved_sound_to_offer_back() {
        let base = std::env::temp_dir().join(format!("tg-app-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let listening = |base: &Path| {
            let mut app = synthetic(5.0, 0.0);
            app.session.listen();
            let info = app.info("Kept", 48000, 16);
            let mut r = Recorder::start(base, info).unwrap();
            r.write(&timegrapher_core::capture::Block {
                samples: vec![0.0; 48000 * 20],
                at: std::time::SystemTime::now(),
            })
            .unwrap();
            let dir = r.dir().to_path_buf();
            app.session.recorder = Some(r);
            app.session.set_state(session::State::Paused {
                since: Instant::now(),
            });
            (app, dir)
        };
        let (mut app, dir) = listening(&base);
        // Being recorded into, it is not offered.
        assert!(kept_sounds(&base).is_empty());
        eframe::App::on_exit(&mut app, None);
        let kept = kept_sounds(&base);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0].dir, dir);
        assert!((kept[0].duration_s.unwrap() - 20.0).abs() < 0.5, "{kept:?}");
        let about: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("session.json")).unwrap()).unwrap();
        let local = watches::local_minute(about["started_utc"].as_str().unwrap());
        assert!(local.is_some());
        assert_eq!(kept[0].started, local, "in this computer's time: {kept:?}");
        assert!(kept[0].bytes > 48000 * 2 * 19);

        // The next start says so, with the ways to keep it.
        let mut next = TimegrapherApp::with_devices(Vec::new(), None);
        next.kept = kept;
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut drawn = String::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| next.kept_notice(ui));
            });
            drawn = format!("{:?}", out.shapes);
        }
        assert!(drawn.contains("wasn't saved"), "{drawn}");
        for b in ["Open", "Move To…", "Delete…", "Later"] {
            assert!(drawn.contains(b), "{b}");
        }
        // Open reads it as a recording.
        next.open_recording(&dir, true);
        let t = Instant::now();
        while next.batch.is_some() && t.elapsed().as_secs() < 60 {
            next.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        let live = next.live.as_ref().expect("opened");
        assert!(
            (live.duration_s() - 20.0).abs() < 0.5,
            "{}",
            live.duration_s()
        );
        drop(next);
        // Move To… puts the folder where the user says.
        let mine = base.join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        let moved = move_kept(&dir, &mine).unwrap();
        assert!(!dir.exists() && moved.join("audio-001.wav").is_file());
        assert!(kept_sounds(&base).is_empty());

        // A quit the user answered ("Quit Without Saving") throws it away.
        let (mut app, dir) = listening(&base);
        app.may_close = true;
        eframe::App::on_exit(&mut app, None);
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&base);
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

    /// Wait for the analysis being kept, then for the take being opened.
    fn wait_for_jobs(app: &mut TimegrapherApp) {
        let t = Instant::now();
        while (app.analysis_save.is_some() || app.batch.is_some()) && t.elapsed().as_secs() < 180 {
            app.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            app.analysis_save.is_none() && app.batch.is_none(),
            "still working"
        );
    }

    /// A session kept as its analysis alone has no sound in it, and opens
    /// with the same beats, readings and charts as the take with its sound,
    /// the Tick Tock profiles it kept, and no Replay or waveform.
    #[test]
    #[expect(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn a_session_kept_as_its_analysis_opens_as_the_sound_does() {
        let base = std::env::temp_dir().join(format!("tg-app-analysis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.bph = Some(28800);
        app.reset_session(48000, "test".into());
        app.session.listen();
        let audio = generate(
            &SynthConfig {
                duration_s: 60.0,
                beat_error_ms: 0.6,
                odd_gains: Some([0.9, 0.25, 1.0]),
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let info = app.info("test", 48000, 16);
        app.session.recorder = Some(Recorder::start(&base.join("live"), info).unwrap());
        // A second of sound at a time, each arriving as its last frame is
        // heard, as the capture delivers it, so the clock log reads as a
        // real take's does.
        let began = std::time::SystemTime::now();
        for (k, second) in audio.samples.chunks(48000).enumerate() {
            let block = timegrapher_core::capture::Block {
                samples: second.to_vec(),
                at: began + std::time::Duration::from_secs(k as u64 + 1),
            };
            app.session
                .recorder
                .as_mut()
                .unwrap()
                .write(&block)
                .unwrap();
        }
        for samples in audio.samples.chunks(960) {
            app.live.as_mut().unwrap().push(samples);
        }
        app.masked = vec![(30.0, 32.0)];
        assert!(app.can_save_analysis());
        app.start_analysis_save(&base.join("kept"));
        assert!(app.analysis_save_busy(), "nothing else while it reads");
        wait_for_jobs(&mut app);
        assert!(
            !app.message.as_ref().is_some_and(|m| m.1),
            "{:?}",
            app.message
        );
        let (kept, _) = app.analysis_kept_to.clone().expect("kept");
        assert!(app.session.saved_to().is_none(), "the sound isn't saved");
        assert!(
            file_name(&kept).ends_with("_analysis"),
            "{}",
            kept.display()
        );
        assert!(analysis_take::is_analysis_only(&kept));
        assert!(recordings_in(&kept).is_err(), "no sound in it");
        let m: Manifest =
            toml::from_str(&std::fs::read_to_string(kept.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(m.audio, Some(false));
        assert!(kept.join("session.json").is_file() && kept.join("clock.csv").is_file());
        // The same session with its sound, to compare.
        app.save_into(&base.join("full"));
        app.wait_for_save();
        let full = app
            .session
            .saved_to()
            .map(Path::to_path_buf)
            .expect("saved");
        let open = |path: &Path| {
            let mut a = TimegrapherApp::with_devices(Vec::new(), None);
            a.start_batch(path);
            wait_for_jobs(&mut a);
            a
        };
        let (mut a, b) = (open(&kept), open(&full));
        assert!(a.no_sound && !b.no_sound);
        assert_eq!(a.masked, b.masked);
        assert_eq!(a.masked, vec![(30.0, 32.0)]);
        let (la, lb) = (a.live.as_ref().unwrap(), b.live.as_ref().unwrap());
        assert_eq!(la.beats().len(), lb.beats().len());
        assert_eq!(la.amplitude_windows().len(), lb.amplitude_windows().len());
        let (ra, rb) = (la.reading(20.0), lb.reading(20.0));
        let close = |x: Option<f64>, y: Option<f64>, tol: f64| match (x, y) {
            (Some(x), Some(y)) => (x - y).abs() <= tol,
            (None, None) => true,
            _ => false,
        };
        assert!(
            close(ra.rate_s_per_day, rb.rate_s_per_day, 0.05),
            "{ra:?} {rb:?}"
        );
        assert!(close(ra.amplitude_deg, rb.amplitude_deg, 1e-9));
        assert!(close(ra.beat_error_ms, rb.beat_error_ms, 0.001));
        assert_eq!(a.trend.len(), b.trend.len());
        // The profiles kept for each 2 seconds stand in for the sound.
        assert!(a.sound_history.len() >= 29, "{}", a.sound_history.len());
        assert!(a
            .sound_history
            .iter()
            .all(|(_, p)| p[0].is_some() && p[1].is_some()));
        // Masks changed on it are saved into its session file, which still
        // says it has no sound.
        a.masked = vec![(30.0, 32.0), (50.0, 52.0)];
        a.save_marks();
        let m: Manifest =
            toml::from_str(&std::fs::read_to_string(kept.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(m.audio, Some(false));
        assert_eq!(m.recordings[0].masked, vec![[30.0, 32.0], [50.0, 52.0]]);
        // The folder it was kept in opens it too.
        assert_eq!(take_folder(&base.join("kept")).unwrap(), kept);
        // A session file saying no sound needs the analysis to read, as
        // `timegrapher session` needs it: refused with the sound beside it
        // or without.
        let odd = base.join("odd");
        std::fs::create_dir_all(&odd).unwrap();
        for f in std::fs::read_dir(&full).unwrap() {
            let f = f.unwrap().path();
            if f.is_file() {
                std::fs::copy(&f, odd.join(f.file_name().unwrap())).unwrap();
            }
        }
        let text = std::fs::read_to_string(odd.join(MANIFEST)).unwrap();
        std::fs::write(odd.join(MANIFEST), format!("audio = false\n{text}")).unwrap();
        let mut d = TimegrapherApp::with_devices(Vec::new(), None);
        d.open_recording(&odd, false);
        let (msg, error) = d.message.clone().unwrap();
        assert!(error && msg.contains("audio = false"), "{msg}");
        // Opened through the folder it sits in, refused before the take on
        // show loses its masks.
        let nest = base.join("nest").join("odd");
        std::fs::create_dir_all(&nest).unwrap();
        for f in std::fs::read_dir(&odd).unwrap() {
            let f = f.unwrap().path();
            std::fs::copy(&f, nest.join(f.file_name().unwrap())).unwrap();
        }
        a.open_recording(&base.join("nest"), false);
        let (msg, error) = a.message.clone().unwrap();
        assert!(error && msg.contains("audio = false"), "{msg}");
        assert_eq!(a.masked, vec![(30.0, 32.0), (50.0, 52.0)]);
        for f in recordings_in(&odd).unwrap() {
            std::fs::remove_file(f).unwrap();
        }
        let mut d = TimegrapherApp::with_devices(Vec::new(), None);
        d.open_recording(&odd, false);
        let (msg, error) = d.message.clone().unwrap();
        assert!(error && msg.contains("audio = false"), "{msg}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The settling's hint names the settling the app uses, the Positions
    /// Test pane's Settle, in a free take as in a test: 20 s unless set.
    #[test]
    fn the_settle_hint_names_the_settle_in_use() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        assert_eq!(app.session.settle_len(), guided::SETTLE_S);
        let hint = settle_hint(app.session.settle_len());
        assert!(hint.starts_with("For the first 20 seconds"), "{hint}");
        app.guided_settle_s = 30.0;
        app.session.set_settle_s(app.guided_settle_s);
        let hint = settle_hint(app.session.settle_len());
        assert!(hint.starts_with("For the first 30 seconds"), "{hint}");
    }

    /// Record, save, carry on, pause, change position, resume, save again
    /// and reopen: sound after a save still counts as unsaved, Save Again
    /// keeps it, and the saved take keeps the position it started in.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn a_take_saved_twice_across_a_move_keeps_everything() {
        let base = std::env::temp_dir().join(format!("tg-app-twice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
        app.wait_for_save();
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
        app.session.set_settle_s(SETTLE_S);
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
        app.wait_for_save();
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
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
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
    /// Each position button's bar fills with the seconds heard in that
    /// position: settled and not masked, other positions' time left out.
    #[test]
    fn each_position_counts_its_own_seconds() {
        let mut app = synthetic(20.0, 0.0);
        app.session.listen();
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        // One position so far: every second is Dial Up's.
        let end = app.latest_s();
        assert!(
            (app.recorded_in(Position::DialUp) - end).abs() < 0.5,
            "{end}"
        );
        assert_eq!(app.recorded_in(Position::DialDown), 0.0);
        let cfg = SynthConfig {
            duration_s: 40.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let moved = app.live.as_ref().unwrap().duration_s();
        app.set_position(Position::DialDown);
        // Paused, the settling starts at Resume: here, at once.
        app.session.start_settle(moved);
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            live.push(b);
        }
        let du = app.recorded_in(Position::DialUp);
        let dd = app.recorded_in(Position::DialDown);
        assert!(
            (du - moved).abs() < 0.5,
            "Dial Up keeps its {du} s of {moved}"
        );
        // Dial Down's time to the latest beat, less its settling.
        let want = app.latest_s() - moved - app.guided_settle_s;
        assert!((dd - want).abs() < 1.0, "Dial Down {dd} s, not {want}");
        // A mask comes off the position it lies in.
        app.masked.push((moved + 25.0, moved + 30.0));
        assert!((app.recorded_in(Position::DialDown) - (want - 5.0)).abs() < 1.0);
        assert_eq!(app.recorded_in(Position::CrownUp), 0.0);
    }

    /// With no guided test, the Positions Test pane reads every position
    /// marked in the take: each one's settled duration and readings, the
    /// positions never marked, and the largest rate difference.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn the_positions_table_reads_a_free_take() {
        let mut app = synthetic(40.0, 0.0);
        app.session.listen();
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        let cfg = SynthConfig {
            duration_s: 40.0,
            rate_s_per_day: 60.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        // Moved while listening, so the settling starts at once.
        let moved = |app: &mut TimegrapherApp, p: Position| {
            app.session.set_state(session::State::Listening);
            app.set_position(p);
            app.session.set_state(session::State::Paused {
                since: Instant::now(),
            });
        };
        moved(&mut app, Position::DialDown);
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            live.push(b);
        }
        let t = app.free_table(&egui::Context::default()).expect("a table");
        let row = |label: &str| t.rows.iter().find(|r| r.0 == label).unwrap().clone();
        let du = row("Dial Up (DU)");
        let dd = row("Dial Down (DD)");
        let (guided::FreeCell::Read(du_rate, ..), guided::FreeCell::Read(dd_rate, ..)) =
            (&du.2, &dd.2)
        else {
            panic!("{t:?}");
        };
        assert!(du_rate.unwrap().abs() < 5.0, "{du_rate:?}");
        assert!((dd_rate.unwrap() - 60.0).abs() < 5.0, "{dd_rate:?}");
        // Dial Down's settling is left out of its duration.
        assert!(
            (dd.1 - (40.0 - app.guided_settle_s)).abs() < 1.0,
            "{}",
            dd.1
        );
        assert_eq!(row("Crown Up (CU, 3H)").2, guided::FreeCell::NotMarked);
        assert_eq!(t.read, 2);
        assert!((t.largest[0].unwrap() - 60.0).abs() < 8.0);
        // The mean and the largest differences are core's, over the two
        // positions read.
        let (
            guided::FreeCell::Read(Some(du_rate), Some(du_amp), Some(du_be)),
            guided::FreeCell::Read(Some(dd_rate), Some(dd_amp), Some(dd_be)),
        ) = (&du.2, &dd.2)
        else {
            panic!("{t:?}");
        };
        let near = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-9;
        assert!(near(t.mean[0], (du_rate + dd_rate) / 2.0));
        assert!(near(t.mean[1], (du_amp + dd_amp) / 2.0));
        assert!(near(t.mean[2], (du_be + dd_be) / 2.0));
        assert!(near(t.largest[0], (du_rate - dd_rate).abs()));
        assert!(near(t.largest[1], (du_amp - dd_amp).abs()));
        assert!(near(t.largest[2], (du_be - dd_be).abs()));

        // The pane draws it, with Each Position's word for a free take.
        let ctx = themed();
        let mut drawn = String::new();
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.pane_tools(ui, Pane::PositionsTest);
                    app.positions_pane(ui);
                });
            });
            drawn = format!("{:?} {:?}", out.shapes, theme::tools_drawn(&ctx));
        }
        for want in [
            "This Take",
            "Duration",
            "Not marked",
            "Average",
            "Largest Difference",
            "2 of 6",
            "Each Position applies only to the guided test.",
        ] {
            assert!(drawn.contains(want), "{want}: {drawn}");
        }

        // A position too short to read says so.
        moved(&mut app, Position::CrownUp);
        let audio = generate(
            &SynthConfig {
                duration_s: app.guided_settle_s + 5.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            live.push(b);
        }
        let t = app.free_table(&egui::Context::default()).unwrap();
        let cu = t.rows.iter().find(|r| r.0 == "Crown Up (CU, 3H)").unwrap();
        assert_eq!(cu.2, guided::FreeCell::Short, "{t:?}");
        // Seven seconds more: 12 s measured.
        let audio = generate(
            &SynthConfig {
                duration_s: 7.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let live = app.live.as_mut().unwrap();
        for b in audio.samples.chunks(960) {
            live.push(b);
        }
        let t = app.free_table(&egui::Context::default()).unwrap();
        let cu = t.rows.iter().find(|r| r.0 == "Crown Up (CU, 3H)").unwrap();
        assert!(
            matches!(cu.2, guided::FreeCell::Read(Some(_), Some(_), Some(_))),
            "{t:?}"
        );
        // A second masked in its middle leaves 11 s with no 10 s clear to
        // read: the row says so, never a row of dashes.
        let end = app.latest_s();
        app.masked.push((end - 7.0, end - 6.0));
        let t = app.free_table(&egui::Context::default()).unwrap();
        let cu = t.rows.iter().find(|r| r.0 == "Crown Up (CU, 3H)").unwrap();
        assert_eq!(cu.2, guided::FreeCell::Unread, "{t:?}");
    }

    #[test]
    fn the_positions_table_reads_again_every_second_in_the_background_while_listening() {
        let base = std::env::temp_dir().join(format!("tg-free-table-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let mut app = synthetic(20.0, 0.0);
        app.session.listen();
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        let feed = |app: &mut TimegrapherApp, s: f64| {
            let cfg = SynthConfig {
                duration_s: s,
                ..Default::default()
            };
            let audio = generate(&cfg, |_| 280.0, |_| 0.0);
            let live = app.live.as_mut().unwrap();
            for b in audio.samples.chunks(960) {
                live.push(b);
            }
        };
        feed(&mut app, 30.0);
        let _tx = queued_capture(&mut app, &base);
        assert!(app.running());
        let ctx = egui::Context::default();
        let du = |t: &guided::FreeTable| t.rows.iter().find(|r| r.0 == "Dial Up (DU)").unwrap().1;
        // The first table is read at once, to the last whole second.
        let first = app.free_table(&ctx).expect("a table");
        assert_eq!(du(&first), app.latest_s().floor());
        // Under a second more: the same second, the same table, nothing read.
        feed(&mut app, 0.5);
        assert_eq!(app.latest_s().floor(), du(&first));
        assert_eq!(du(&app.free_table(&ctx).unwrap()), du(&first));
        assert!(app.free_job.borrow().is_none());
        // Into the next seconds: the table on show stays while the next is
        // read in the background, then the next one shows.
        feed(&mut app, 2.0);
        assert_eq!(du(&app.free_table(&ctx).unwrap()), du(&first));
        assert!(app.free_job.borrow().is_some());
        let since = Instant::now();
        let next = loop {
            let t = app.free_table(&ctx).unwrap();
            if du(&t) != du(&first) {
                break t;
            }
            assert!(since.elapsed().as_secs() < 30, "never read again");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(du(&next), app.latest_s().floor());
        assert!(
            du(&next) >= du(&first) + 2.0,
            "{} {}",
            du(&first),
            du(&next)
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_guided_test_reads_the_position_under_way_as_it_is_measured() {
        let mut app = synthetic(20.0, 0.0);
        let start = app.live.as_ref().unwrap().duration_s();
        let mut test = guided::Test::new().timed(5.0, 60.0);
        test.tick(start);
        app.guided = Some(test);
        let ctx = egui::Context::default();
        // One stretch of sound at +30 s/d, heard in two parts.
        let audio = generate(
            &SynthConfig {
                duration_s: 40.0,
                rate_s_per_day: 30.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let (first, rest) = audio.samples.split_at(12 * 48_000);
        let feed = |app: &mut TimegrapherApp, samples: &[f32]| {
            let live = app.live.as_mut().unwrap();
            for b in samples.chunks(960) {
                live.push(b);
            }
        };
        // Settled, but under 10 s measured: nothing to show yet.
        feed(&mut app, first);
        assert_eq!(app.guided_live(&ctx), None);
        // 35 s measured: its figures so far.
        feed(&mut app, rest);
        let Some(guided::FreeCell::Read(Some(rate), Some(_), _)) = app.guided_live(&ctx) else {
            panic!("{:?}", app.guided_live(&ctx));
        };
        assert!((rate - 30.0).abs() < 5.0, "{rate}");
    }

    /// While a take is read, the status bar's bar says how far, not the
    /// take's name again: that is already at the bar's left.
    #[test]
    fn the_analysing_bar_says_how_far_not_the_name() {
        assert_eq!(batch_words(0.374, true), "Analysing, 37%");
        assert_eq!(batch_words(0.0, false), "Analysing…");
    }

    /// Stop Test… on the Listen card, answered Stop Test, keeps the
    /// positions measured in the table, stops listening, and leaves the
    /// position free to set again.
    #[test]
    fn a_positions_tests_stop_keeps_its_rows_and_stops_listening() {
        let base = std::env::temp_dir().join(format!("tg-app-guided-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let mut app = synthetic(20.0, 0.0);
        app.session.listen();
        let tx = queued_capture(&mut app, &base);
        let start = app.live.as_ref().unwrap().duration_s();
        let mut test = guided::Test::new().timed(1.0, 5.0);
        test.tick(start);
        app.guided = Some(test);
        assert!(app.guided_running());
        assert!(app.test_listening());
        app.stop_test();
        assert!(app.session.paused(), "listening stopped");
        let kept = app.guided.as_ref().expect("the table stays");
        assert!(kept.readings.is_some(), "finished");
        assert!(!app.guided_running());
        assert!(app.capture.is_none());
        // A finished test sets nothing while listening goes on.
        let _tx2 = queued_capture(&mut app, &base);
        app.session.listen();
        assert!(app.running() && app.position_editable());
        drop(tx);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// New Test sets the Positions Test up on the Bench and starts
    /// nothing: the pane shows, the Position card reads Dial Up, and the
    /// status bar says the test is ready, not started.
    #[test]
    fn a_new_positions_test_waits_ready_on_the_bench() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.view = View::Watches;
        set_pane_visible(&mut app.panes.tiles, Pane::PositionsTest, false);
        app.set_position(Position::CrownDown);
        app.new_take(NewTake::Positions, false);
        assert_eq!(app.ready_test(), Some(NewTake::Positions));
        assert!(app.guided.is_none() && !app.running());
        assert_eq!(app.view, View::Bench);
        assert!(pane_visible(&app.panes.tiles, Pane::PositionsTest));
        assert_eq!(app.position, Position::DialUp);
        let words = app.status_words();
        assert!(
            words.contains("Positions Test") && words.contains("Ready, not started"),
            "{words}"
        );
        // With no watch, nothing holds Start Test back.
        assert_eq!(app.start_test_why_not(), None);
    }

    /// A Positions Test set up for 24 Hours After is set against one of
    /// the picked watch's own full-wind tests, picked in Compare With:
    /// picking another watch while it waits offers that watch's, and
    /// Start Test waits for the pick.
    #[test]
    fn a_ready_test_finds_the_picked_watchs_full_wind() {
        let base = std::env::temp_dir().join(format!("tg-app-ready-wind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Seiko 5")).unwrap();
        lib.add(&library::Watch::new("Daytona")).unwrap();
        let seiko = lib.find("Seiko 5").unwrap().watch.id.clone();
        let daytona = lib.find("Daytona").unwrap();
        let (daytona, take) = (
            daytona.watch.id.clone(),
            daytona.dir.join(library::TAKES).join("full wind"),
        );
        drop(lib);
        // Daytona's full-wind test, begun 31 h ago.
        std::fs::create_dir_all(&take).unwrap();
        let full = guided::FullWind {
            format: guided::RESULTS_FORMAT.to_string(),
            began_utc: String::new(),
            began_unix: guided::unix_now() - 31.0 * 3600.0,
            watch: "Daytona".into(),
            calibre: String::new(),
            bph: None,
            lift_angle_deg: None,
            settle_s: None,
            measure_s: None,
            positions: vec![guided::KeptRow {
                position: "DU".into(),
                rate_s_per_day: Some(4.0),
                amplitude_deg: Some(290.0),
                beat_error_ms: Some(0.3),
            }],
        };
        std::fs::write(take.join(guided::RESULTS_FILE), full.to_json()).unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root));
        app.pick_watch(&seiko);
        app.guided_after = true;
        app.new_take(NewTake::Positions, false);
        assert_eq!(app.ready_test(), Some(NewTake::Positions));
        let why = app.start_test_why_not();
        assert!(
            why.as_deref()
                .is_some_and(|w| w.starts_with("Seiko 5 has no")),
            "{why:?}"
        );
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let frame = |app: &mut TimegrapherApp| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.panels(ctx));
        };
        app.pick_watch(&daytona);
        frame(&mut app);
        assert_eq!(app.ready_test(), Some(NewTake::Positions));
        // Offered, not picked for you.
        assert_eq!(app.guided_choices.len(), 1);
        assert_eq!(app.guided_earlier, None);
        let why = app.start_test_why_not();
        assert!(
            why.as_deref()
                .is_some_and(|w| w.starts_with("Pick the full-wind test")),
            "{why:?}"
        );
        app.pick_full_wind(0);
        assert_eq!(app.start_test_why_not(), None);
        assert_eq!(
            app.guided_earlier.as_ref().map(|f| f.watch.as_str()),
            Some("Daytona")
        );
        // And back: Seiko 5 has none.
        app.pick_watch(&seiko);
        frame(&mut app);
        assert!(app.start_test_why_not().is_some());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A test set up just before the full-wind test's 12 hour mark can
    /// start once the mark passes, and a full-wind take removed is no
    /// longer found: the search sees the time and the takes, not only the
    /// watch and the Wind setting.
    #[test]
    fn a_ready_tests_full_wind_search_sees_the_time_and_the_takes() {
        let base = std::env::temp_dir().join(format!("tg-app-wind-time-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Seiko 5")).unwrap();
        let entry = lib.find("Seiko 5").unwrap();
        let (seiko, take) = (
            entry.watch.id.clone(),
            entry.dir.join(library::TAKES).join("full wind"),
        );
        drop(lib);
        // Its full-wind test began 11 h 30 min ago.
        let now = guided::unix_now();
        std::fs::create_dir_all(&take).unwrap();
        let full = guided::FullWind {
            format: guided::RESULTS_FORMAT.to_string(),
            began_utc: String::new(),
            began_unix: now - 11.5 * 3600.0,
            watch: "Seiko 5".into(),
            calibre: String::new(),
            bph: None,
            lift_angle_deg: None,
            settle_s: None,
            measure_s: None,
            positions: vec![guided::KeptRow {
                position: "DU".into(),
                rate_s_per_day: Some(4.0),
                amplitude_deg: Some(290.0),
                beat_error_ms: Some(0.3),
            }],
        };
        std::fs::write(take.join(guided::RESULTS_FILE), full.to_json()).unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root));
        app.pick_watch(&seiko);
        app.guided_after = true;
        app.new_take(NewTake::Positions, false);
        app.find_full_wind_at(now);
        assert!(app.start_test_why_not().is_some(), "too soon");
        // An idle window still draws at the next minute, to search again.
        let at = (now / 60.0).floor() * 60.0 + 15.0;
        let wait = app
            .full_wind_recheck(at)
            .expect("waits on its full-wind test");
        assert!((wait.as_secs_f64() - 45.05).abs() < 1e-6, "{wait:?}");
        // An hour on, past the mark, with nothing else changed: offered,
        // and once picked, the test can start.
        app.find_full_wind_at(now + 3600.0);
        assert_eq!(app.guided_choices.len(), 1);
        assert!(app.start_test_why_not().is_some(), "not yet picked");
        app.pick_full_wind(0);
        assert_eq!(app.start_test_why_not(), None);
        // The take removed, in the same minute: no longer found, even with
        // the takes folder's time as it was, as Windows can leave it.
        let takes = take.parent().unwrap().to_path_buf();
        let was = std::fs::metadata(&takes).unwrap().modified().unwrap();
        std::fs::remove_dir_all(&take).unwrap();
        // Set back by hand where a folder can be opened to set it.
        #[cfg(unix)]
        {
            std::fs::File::open(&takes)
                .unwrap()
                .set_modified(was)
                .unwrap();
            assert_eq!(std::fs::metadata(&takes).unwrap().modified().unwrap(), was);
        }
        #[cfg(not(unix))]
        let _ = was;
        app.find_full_wind_at(now + 3600.0);
        assert!(app.start_test_why_not().is_some(), "removed");
        assert_eq!(app.guided_earlier, None, "the pick goes with its take");
        // With 24 Hours After off, nothing waits on the minute.
        app.guided_after = false;
        assert_eq!(app.full_wind_recheck(at), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The sixth position measured and read, the Positions Test ends by
    /// itself: listening stops and the table keeps all six.
    #[test]
    fn a_positions_test_ends_by_itself_after_the_sixth_position() {
        let base = std::env::temp_dir().join(format!("tg-app-guided-end-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let mut app = synthetic(20.0, 0.0);
        app.session.listen();
        let tx = queued_capture(&mut app, &base);
        let end = app.live.as_ref().unwrap().duration_s();
        let mut test = guided::Test::new().timed(0.5, 2.0);
        let mut t = end - 18.0;
        test.tick(t);
        for i in 0..guided::ORDER.len() {
            t += 2.75;
            assert_eq!(test.tick(t), guided::Phase::Turn, "position {i}");
            if i + 1 < guided::ORDER.len() {
                test.advance(t);
            }
        }
        app.guided = Some(test);
        assert!(app.test_listening());
        let ctx = egui::Context::default();
        app.guided_tick(&ctx);
        assert!(!app.guided_running(), "ended");
        assert!(app.session.paused(), "listening stopped");
        let kept = app.guided.as_ref().expect("the table stays");
        assert_eq!(kept.readings.as_ref().map(|r| r.len()), Some(6));
        drop(tx);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The Watches view's Power Reserve Test only sets the take up: the
    /// pane shows on the Bench, and nothing starts until its Start Test,
    /// the position set first on the card.
    #[test]
    fn the_watches_power_reserve_test_waits_on_the_bench() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.view = View::Watches;
        set_pane_visible(&mut app.panes.tiles, Pane::PowerReserve, false);
        app.new_take(NewTake::PowerReserve, false);
        assert!(app.reserve.is_none() && !app.running());
        assert_eq!(app.view, View::Bench);
        assert!(pane_visible(&app.panes.tiles, Pane::PowerReserve));
        assert!(app.position_editable(), "the card sets it");
    }

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
        let joined = steady::join(&log, &spans, &[], None).0;
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
        let want = steady::joined_duration(&spans, live.duration_s(), None);
        assert!((joined.duration_s - want).abs() < 1e-9);
        assert!((want - (live.duration_s() - 20.0 + timegrapher_core::joined::JOIN_S)).abs() < 1.0);
        let fit = timegrapher_core::timing::fit(&joined.beats, joined.bph).unwrap();
        assert!(fit.rate_s_per_day.abs() < 5.0, "{}", fit.rate_s_per_day);
        // And it all draws, bands and all.
        let ctx = themed();
        for p in [Pane::Charts, Pane::Distributions] {
            set_pane_visible(&mut app.panes.tiles, p, true);
        }
        frames(&mut app, &ctx, 2);
        app.relayout(false);
        frames(&mut app, &ctx, 2);
    }

    /// A stretch dragged over on the strip is masked out of the readings
    /// and the charts; a click on it brings it back.
    #[test]
    fn a_masked_stretch_leaves_the_readings_and_comes_back() {
        let mut app = synthetic(60.0, 0.0);
        let heard = |app: &TimegrapherApp| {
            app.trend
                .iter()
                .filter(|p| p.t > 30.0 && p.t < 40.0 && p.rate.is_some())
                .count()
        };
        // The 2-second stretches the distributions count, too.
        let stretches = |app: &TimegrapherApp| {
            app.unmasked_windows()
                .iter()
                .filter(|w| w.end_s > 30.0 && w.start_s < 45.0)
                .count()
        };
        assert!(heard(&app) > 0);
        assert!(stretches(&app) > 0);
        app.mask(30.2, 30.4);
        assert!(app.masked.is_empty(), "a slip of the mouse");
        app.mask(40.0, 30.0);
        app.mask(35.0, 45.0);
        assert_eq!(app.masked, [(30.0, 45.0)], "joined");
        assert_eq!(app.live.as_ref().unwrap().masked(), app.masked);
        assert_eq!(heard(&app), 0);
        assert_eq!(stretches(&app), 0);
        app.unmask_at(10.0);
        assert_eq!(app.masked.len(), 1, "nothing masked there");
        app.unmask_at(33.0);
        assert!(app.masked.is_empty());
        assert!(heard(&app) > 0);
        assert!(stretches(&app) > 0);
    }

    /// A masked stretch's edges move, and one moved over another joins it
    /// for good; one moved to nothing comes back.
    #[test]
    fn a_masked_stretch_can_be_reshaped() {
        let mut app = synthetic(60.0, 0.0);
        app.set_masked(vec![(10.0, 15.0), (30.0, 40.0)]);
        app.reshape_mask((30.0, 40.0), (25.0, 40.0));
        assert_eq!(app.masked, [(10.0, 15.0), (25.0, 40.0)]);
        app.reshape_mask((10.0, 15.0), (10.0, 28.0));
        assert_eq!(app.masked, [(10.0, 40.0)], "joined");
        app.reshape_mask((99.0, 100.0), (1.0, 2.0));
        assert_eq!(app.masked, [(10.0, 40.0)], "no such stretch");
        app.reshape_mask((10.0, 40.0), (10.0, 10.2));
        assert!(app.masked.is_empty());
    }

    #[test]
    fn fit_shows_the_whole_session() {
        let mut app = synthetic(40.0, 0.0);
        app.strip_end = Some(10.0);
        app.fit_strip();
        assert!(app.strip_fit);
        assert_eq!(app.strip_end, None);
        assert!((app.strip.span_s - app.latest_s()).abs() < 1e-9);
        assert!(app.latest_s() > 35.0);
    }

    /// A recording of `seconds` written to `dir/name`, opened and analysed
    /// whole in a new app.
    fn opened_recording(dir: &Path, name: &str, seconds: f64) -> TimegrapherApp {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        if !path.exists() {
            let cfg = SynthConfig {
                duration_s: seconds,
                ..Default::default()
            };
            let audio = generate(&cfg, |_| 280.0, |_| 0.0);
            timegrapher_core::audio::write_wav(&path, &audio).unwrap();
        }
        reopened(&path)
    }

    pub(super) fn reopened(path: &Path) -> TimegrapherApp {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.start_batch(path);
        let t = Instant::now();
        while app.batch.is_some() && t.elapsed().as_secs() < 120 {
            app.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(app.live.is_some(), "{:?}", app.message);
        app
    }

    /// Painting on a take opened lights Save, whose menu offers Save
    /// Changes first, and the status bar says the changes aren't saved.
    #[test]
    fn painting_on_an_opened_take_lights_save() {
        let base = std::env::temp_dir().join(format!("tg-app-paint-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = opened_recording(&base, "reverso_DU.wav", 30.0);
        app.brush = Some(Position::CrownLeft);
        app.paint_position(10.0, 20.0);
        assert!(app.marks_unsaved());
        let ctx = themed();
        let status = ctx.run(screen(), |ctx| {
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| app.status_bar(ui));
        });
        assert!(painted_text(&status).contains("Changes not saved"));
        let mut time = 0.0;
        let mut pass = |app: &mut TimegrapherApp, events: Vec<egui::Event>| {
            time += 0.25;
            let input = egui::RawInput {
                time: Some(time),
                events,
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.save_menu(ui, 32.0));
            });
            out.shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some((
                        t.galley.text().to_string(),
                        t.galley.rect.translate(t.pos.to_vec2()).center(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let click = |at: egui::Pos2| {
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            vec![egui::Event::PointerMoved(at), button(true), button(false)]
        };
        pass(&mut app, Vec::new());
        pass(&mut app, click(egui::pos2(8.0 + 16.0, 8.0 + 16.0)));
        let drawn = pass(&mut app, Vec::new());
        let at = drawn
            .iter()
            .find(|t| t.0 == "Save Changes")
            .unwrap_or_else(|| panic!("{drawn:?}"))
            .1;
        pass(&mut app, click(at));
        pass(&mut app, Vec::new());
        assert!(!app.marks_unsaved(), "{:?}", app.message);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// After a Positions Test, Start a New Test starts a new take, asking
    /// first about sound not saved; it never carries on the stopped one.
    #[test]
    fn a_new_test_after_one_is_a_new_take() {
        let base = std::env::temp_dir().join(format!("tg-app-new-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.reset_session(48000, "test".into());
        app.session.listen();
        let info = app.info("", 48000, 16);
        let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 20],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        app.session.recorder = Some(r);
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        app.guided = Some(guided::Test::new());
        app.positions_take = true;
        assert!(app.can_resume());
        app.start_guided(None);
        assert!(!app.running(), "the stopped take isn't carried on");
        assert!(matches!(
            app.confirm,
            Some(Confirm::NewTake(NewTake::Positions))
        ));
        assert!(app.guided.is_some(), "nothing let go before the answer");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Masks on a lone recording are asked about, and saved to a new
    /// session file that makes its folder a take, written as a take
    /// recorded here with the same settings would be; they come back when
    /// it is opened again.
    #[test]
    fn masks_on_a_lone_recording_are_saved_beside_it() {
        let base = std::env::temp_dir().join(format!("tg-app-mask-lone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = opened_recording(&base, "reverso_DD.wav", 30.0);
        assert!(!app.needs_save_confirmation(), "nothing changed yet");
        app.mask(10.0, 14.0);
        assert!(app.marks_unsaved());
        app.open_recording(&base.join("reverso_DD.wav"), true);
        assert!(
            matches!(app.confirm, Some(Confirm::Open { .. })),
            "asked first"
        );
        app.confirm = None;
        app.save_marks();
        assert!(!app.marks_unsaved());
        let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
        let m: Manifest = toml::from_str(&text).unwrap();
        // What the recorder writes for a take with these settings: the
        // default lift is written as assumed, not chosen.
        let mut want = app.manifest_header();
        want.recordings = vec![RecordingEntry {
            file: ".".into(),
            position: Some("DD".into()),
            settle_s: Some(0.0),
            masked: vec![[10.0, 14.0]],
            ..Default::default()
        }];
        assert_eq!(text, toml::to_string_pretty(&want).unwrap());
        assert_eq!(m.lift_known, Some(false));
        assert_eq!(m.calibre, None);
        // The app reads the folder, and the file in it, as a take.
        for open in [base.clone(), base.join("reverso_DD.wav")] {
            let again = reopened(&open);
            assert_eq!(again.masked, [(10.0, 14.0)]);
            assert_eq!(again.position, Position::DialDown);
            assert!(!again.needs_save_confirmation());
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Starting the microphone, or a Positions Test, over a recording whose
    /// masks changed asks first, and keeps them until listening starts.
    #[test]
    fn listening_asks_before_unsaved_masks_go() {
        let base = std::env::temp_dir().join(format!("tg-app-mask-mic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = opened_recording(&base, "a.wav", 12.0);
        app.mask(2.0, 5.0);
        app.start_microphone();
        assert!(matches!(app.confirm, Some(Confirm::Microphone)));
        assert_eq!(app.masked, [(2.0, 5.0)], "nothing let go yet");
        // A Positions Test asked for waits on the same answer.
        app.confirm = None;
        app.start_guided(None);
        assert!(matches!(app.confirm, Some(Confirm::Microphone)));
        assert_eq!(app.guided_after_ask, Some(None));
        // Answered, with an input that can't be opened: the masks stay on
        // show, still to save.
        app.devices_listed = true;
        app.device = Some("no such input".into());
        app.confirm = None;
        app.listen_after_ask();
        assert!(!app.running());
        assert_eq!(app.masked, [(2.0, 5.0)]);
        assert!(app.marks_unsaved());
        assert_eq!(app.guided_after_ask, None);
        assert!(!app.masks_let_go);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A lone recording beside others isn't made a take of: saving its
    /// masks is refused and nothing is written.
    #[test]
    fn masks_are_not_saved_into_a_folder_of_other_recordings() {
        let base = std::env::temp_dir().join(format!("tg-app-mask-others-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        opened_recording(&base, "a.wav", 12.0);
        let mut app = opened_recording(&base, "b.wav", 12.0);
        app.mask(2.0, 5.0);
        app.save_marks();
        assert!(app.marks_unsaved(), "still unsaved");
        assert!(!base.join(MANIFEST).exists());
        let said = app
            .message
            .as_ref()
            .map(|m| m.0.clone())
            .unwrap_or_default();
        assert!(said.contains("1 other recording"), "{said}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Saving masks to a take changes only `masked` on the entries for what
    /// was opened: comments, keys this app doesn't read and other entries
    /// stay, and the file still reads as the take it was.
    #[test]
    fn masks_saved_to_a_take_leave_the_rest_of_its_file() {
        let base = std::env::temp_dir().join(format!("tg-app-mask-take-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let original = "# A hand-written note\n\
            watch = \"Test\"\n\
            lift = 52.0\n\
            owner = \"kept\" # and its note\n\n\
            [[recording]]\n\
            file = \"take.wav\"\n\
            position = \"CU\"\n\
            masked = [[1.0, 2.0]]\n\n\
            [[recording]]\n\
            file = \"other.wav\"\n\
            position = \"CD\"\n";
        std::fs::write(base.join(MANIFEST), original).unwrap();
        let mut app = opened_recording(&base, "take.wav", 20.0);
        assert_eq!(app.masked, [(1.0, 2.0)]);
        app.mask(6.0, 9.5);
        app.save_marks();
        assert!(!app.marks_unsaved());
        let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
        assert_eq!(
            text,
            original.replace("[[1.0, 2.0]]", "[[1.0, 2.0], [6.0, 9.5]]"),
            "only the masks changed"
        );
        let (_, entries) = read_manifest(&base.join("take.wav")).unwrap().unwrap();
        assert_eq!(entries[0].masked, [[1.0, 2.0], [6.0, 9.5]]);
        // Every mask brought back: the key goes.
        app.set_masked(Vec::new());
        app.save_marks();
        let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
        assert_eq!(text, original.replace("masked = [[1.0, 2.0]]\n", ""));
        assert!(!base.join(".session.toml.saving").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The Position Brush on a recording opened from disk: a stretch
    /// painted is asked about before it goes, and Save puts one entry per
    /// stretch in its session file in place of the entries for what was
    /// opened, each keeping the rest of what the first said; the take opens
    /// with them again. Comments, keys this app doesn't read and the other
    /// recording's entry stay.
    /// Positions painted on a take in a watch and saved show in the
    /// Watches view's row for it at once.
    #[test]
    fn a_painted_take_in_a_watch_shows_what_was_saved() {
        let base = std::env::temp_dir().join(format!("tg-app-paint-row-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        drop(lib);
        let take = root
            .join("Pepsi")
            .join(library::TAKES)
            .join("2026-10-25 10.00 Dial Up");
        std::fs::create_dir_all(&take).unwrap();
        std::fs::write(
            take.join(MANIFEST),
            "lift = 52.0\n\n[[recording]]\nfile = \"take.wav\"\nposition = \"DU\"\n",
        )
        .unwrap();
        let mut app = opened_recording(&take, "take.wav", 80.0);
        app.use_library(Some(root.clone()));
        let positions = |app: &TimegrapherApp| match &app.library {
            Ok(Some(l)) => l.find("Pepsi").unwrap().takes[0].positions.clone(),
            _ => panic!("no library"),
        };
        assert_eq!(positions(&app), ["DU"]);
        app.brush = Some(Position::CrownUp);
        app.paint_position(10.04, 40.0);
        app.save_marks();
        assert!(!app.marks_unsaved(), "{:?}", app.message);
        assert_eq!(positions(&app), ["DU", "CU"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn positions_painted_on_a_take_are_saved_to_it() {
        let base = std::env::temp_dir().join(format!("tg-app-paint-take-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let original = "# A hand-written note\n\
            watch = \"Test\"\n\
            lift = 52.0\n\n\
            [[recording]]\n\
            file = \"take.wav\"\n\
            position = \"DU\"\n\
            notes = \"kept\" # and its note\n\n\
            [[recording]]\n\
            file = \"other.wav\"\n\
            position = \"CD\"\n";
        std::fs::write(base.join(MANIFEST), original).unwrap();
        let mut app = opened_recording(&base, "take.wav", 80.0);
        assert_eq!(app.position, Position::DialUp);
        assert!(!app.marks_unsaved());
        app.brush = Some(Position::CrownUp);
        app.paint_position(10.04, 40.0);
        assert!(app.marks_unsaved());
        assert_eq!(app.position, Position::DialUp, "Dial Up again after it");
        assert_eq!(
            app.session.stretches(),
            [
                (0.0, Position::DialUp),
                (10.0, Position::CrownUp),
                (40.0, Position::DialUp)
            ]
        );
        // The readings are cut at each change: none reaches across one.
        let breaks = app.live.as_ref().unwrap().breaks().to_vec();
        assert!(
            breaks.contains(&10.0) && breaks.contains(&40.0),
            "{breaks:?}"
        );
        app.open_recording(&base.join("take.wav"), true);
        assert!(
            matches!(app.confirm, Some(Confirm::Open { .. })),
            "asked first"
        );
        app.confirm = None;
        app.save_marks();
        assert!(!app.marks_unsaved());
        let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
        let entry = |pos: &str, times: &str, comment: bool| {
            format!(
                "[[recording]]\nfile = \"take.wav\"\nposition = \"{pos}\"\nnotes = \"kept\"{}\n{times}\n",
                if comment { " # and its note" } else { "" }
            )
        };
        // Painted, so nothing settles at either edge; what was recorded,
        // Dial Up throughout, is kept under it. The take's own settling
        // at its start is cut where the paint begins (the design guide's
        // mark 51), so its Dial Up before the paint is all settling and
        // nothing of it is read.
        let want = format!(
            "# A hand-written note\nwatch = \"Test\"\nlift = 52.0\n\n{}\n{}\n{}\n\
             [[recording]]\nfile = \"other.wav\"\nposition = \"CD\"\n\n\
             [[recorded]]\nposition = \"DU\"\n",
            entry("DU", "start_s = 10.0\nend_s = 10.0\nsettle_s = 0.0", true),
            entry("CU", "start_s = 10.0\nend_s = 40.0\nsettle_s = 0.0", false),
            entry("DU", "start_s = 40.0\nsettle_s = 0.0", false),
        );
        assert_eq!(text, want);
        // That Dial Up has nothing to read, which every surface leaves
        // out of the take's readings rather than failing on it.
        let saved: Manifest = toml::from_str(&text).unwrap();
        let read: Vec<bool> = saved
            .recordings
            .iter()
            .filter(|e| e.file == "take.wav")
            .map(timegrapher_core::take::reads_nothing)
            .collect();
        assert_eq!(read, [true, false, false]);
        let mut again = reopened(&base.join("take.wav"));
        assert_eq!(again.session.stretches(), app.session.stretches());
        assert_eq!(again.session.settles(), app.session.settles());
        assert_eq!(
            again.session.painted_spans(again.position),
            [(10.0, 40.0, Position::CrownUp)]
        );
        assert!(!again.marks_unsaved());
        // Remove Painting, opened again: Dial Up throughout, as recorded,
        // and saved so, with nothing kept under it.
        again.remove_painting(20.0);
        assert_eq!(again.session.stretches(), [(0.0, Position::DialUp)]);
        assert!(again.marks_unsaved());
        again.save_marks();
        let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
        assert!(!text.contains("[[recorded]]"), "{text}");
        assert_eq!(text.matches("take.wav").count(), 1, "{text}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A change of position dragged moves with its settling; painting to
    /// the newest beat puts the watch in that position from then on; the
    /// brush is put down by Esc (as Mask is) and by the Mask tool, and isn't offered
    /// during a Positions Test.
    #[test]
    fn the_position_brush_paints_and_moves_changes() {
        let mut app = synthetic(60.0, 0.0);
        app.brush = Some(Position::CrownLeft);
        app.paint_position(20.0, 30.0);
        app.move_position_change(20.0, 15.0);
        assert_eq!(app.session.stretches()[1], (15.0, Position::CrownLeft));
        assert!(
            app.session.settles().is_empty(),
            "painting starts no settling"
        );
        let latest = app.latest_s();
        app.paint_position(40.0, latest - 0.5);
        assert_eq!(app.position, Position::CrownLeft, "to the end");
        assert_eq!(
            app.session.stretches().last(),
            Some(&(40.0, Position::CrownLeft))
        );
        // Esc puts it down.
        let ctx = themed();
        let esc = |pressed| egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        let _ = ctx.run(
            egui::RawInput {
                events: vec![esc(true), esc(false)],
                ..Default::default()
            },
            |ctx| app.panels(ctx),
        );
        assert_eq!(app.brush, None);
        // Esc puts the Mask tool down as well.
        app.masking = true;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![esc(true), esc(false)],
                ..Default::default()
            },
            |ctx| app.panels(ctx),
        );
        assert!(!app.masking, "Esc puts Mask down");
        // Picking the Mask tool puts it down too.
        app.brush = Some(Position::DialUp);
        app.masking = true;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                theme::pane_toolbar(ui, "Paper Strip", &[], |ui| app.strip_settings(ui))
            });
        });
        assert_eq!(app.brush, None);
        assert!(app.masking);
        // Not during a Positions Test.
        app.masking = false;
        app.brush = Some(Position::DialUp);
        app.guided_measured = vec![(0.0, 10.0)];
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                theme::pane_toolbar(ui, "Paper Strip", &[], |ui| app.strip_settings(ui))
            });
        });
        assert_eq!(app.brush, None);
    }

    /// Full-wind results loaded from a file stay while the test waits, the
    /// minutes passing, with no watch as with a watch's own tests on offer.
    #[test]
    fn results_loaded_from_a_file_stay_while_the_test_waits() {
        let full = guided::FullWind {
            format: guided::RESULTS_FORMAT.to_string(),
            began_utc: "2026-10-25T09:00:00.000Z".into(),
            began_unix: 1_792_918_800.0,
            watch: "Pepsi".into(),
            calibre: String::new(),
            bph: None,
            lift_angle_deg: None,
            settle_s: None,
            measure_s: None,
            positions: vec![guided::KeptRow {
                position: "DU".into(),
                rate_s_per_day: Some(4.0),
                amplitude_deg: None,
                beat_error_ms: None,
            }],
        };
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.new_take(NewTake::Positions, false);
        let now = guided::unix_now();
        app.find_full_wind_at(now);
        // As Load Full-Wind Results… leaves it.
        app.guided_earlier = Some(full.clone());
        app.guided_earlier_file = true;
        for later in [60.0, 120.0, 3600.0] {
            app.find_full_wind_at(now + later);
            assert_eq!(app.guided_earlier.as_ref(), Some(&full), "{later} s on");
        }
    }

    /// Cancel Test leaves nothing of the test set up: the full-wind
    /// results loaded for it, with no watch, are gone from the Bench.
    #[test]
    fn a_cancelled_test_leaves_no_full_wind_results() {
        let full = guided::FullWind {
            format: guided::RESULTS_FORMAT.to_string(),
            began_utc: "2026-10-25T09:00:00.000Z".into(),
            began_unix: 1_792_918_800.0,
            watch: "Daytona".into(),
            calibre: String::new(),
            bph: None,
            lift_angle_deg: None,
            settle_s: None,
            measure_s: None,
            positions: Vec::new(),
        };
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.new_take(NewTake::Positions, false);
        app.find_full_wind_at(guided::unix_now());
        app.guided_earlier = Some(full.clone());
        app.guided_earlier_file = true;
        app.leave_ready_test();
        assert_eq!(app.test_ready, None);
        assert_eq!(app.guided_earlier, None);
        assert!(!app.guided_earlier_file);
        // Set up again, it starts with nothing loaded.
        app.new_take(NewTake::Positions, false);
        app.find_full_wind_at(guided::unix_now());
        assert_eq!(app.guided_earlier, None);
    }

    /// Load Full-Wind Results… reads the results the test just after
    /// winding saved, and the next test runs set against them; a file
    /// that isn't such results says so and loads nothing.
    #[test]
    fn full_wind_results_load_and_the_next_test_is_set_against_them() {
        let base = std::env::temp_dir().join(format!("tg-app-full-wind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let full = guided::FullWind {
            format: guided::RESULTS_FORMAT.to_string(),
            began_utc: "2026-10-25T09:00:00.000Z".into(),
            began_unix: 1_792_918_800.0,
            watch: "Pepsi".into(),
            calibre: "ETA 2824-2".into(),
            bph: Some(28_800),
            lift_angle_deg: Some(52.0),
            settle_s: Some(10.0),
            measure_s: Some(60.0),
            positions: ["DU", "DD", "CD", "CL", "CU", "CR"]
                .into_iter()
                .map(|p| guided::KeptRow {
                    position: p.into(),
                    rate_s_per_day: Some(4.0),
                    amplitude_deg: Some(290.0),
                    beat_error_ms: Some(0.3),
                })
                .collect(),
        };
        let path = base.join(guided::RESULTS_FILE);
        std::fs::write(&path, full.to_json()).unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let read = app.read_full_wind(&path);
        assert_eq!(read.as_ref(), Some(&full));
        // Not results: said, and nothing loaded.
        let bad = base.join("other.json");
        std::fs::write(&bad, "{}").unwrap();
        assert!(app.read_full_wind(&bad).is_none());
        assert!(app
            .message
            .as_ref()
            .is_some_and(|(m, error)| *error && m.contains("other.json")));
        // Loaded, the next test runs against them.
        app.guided_earlier = read;
        app.reset_session(48000, "test".into());
        app.session.listen();
        let _tx = queued_capture(&mut app, &base);
        app.start_guided(app.guided_earlier.clone());
        let test = app.guided.as_ref().expect("the test runs");
        assert_eq!(test.full.as_ref().map(|f| &f.0), Some(&full));
        app.capture = None;
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A Positions Test's take, run here or opened later, takes no
    /// Position Brush: the test set its positions. Another take opened
    /// takes it again.
    #[test]
    fn a_positions_test_take_takes_no_brush() {
        let mut app = synthetic(60.0, 0.0);
        let ctx = themed();
        let draw = |app: &mut TimegrapherApp| {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    theme::pane_toolbar(ui, "Paper Strip", &[], |ui| app.strip_settings(ui))
                });
            });
        };
        let dir =
            std::env::temp_dir().join(format!("tg-app-positions-brush-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let take = dir.join("take.flac");
        // A plain take: the brush stays in the hand.
        app.load_reserve(&take);
        assert!(!app.positions_take);
        app.brush = Some(Position::DialUp);
        draw(&mut app);
        assert_eq!(app.brush, Some(Position::DialUp));
        // A Positions Test's results beside it: put down, and greyed.
        std::fs::write(dir.join(guided::RESULTS_FILE), "{}").unwrap();
        app.load_reserve(&take);
        assert!(app.positions_take);
        assert!(!app.in_test(), "the test itself is over");
        draw(&mut app);
        assert_eq!(app.brush, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Position Brush is put down and greyed while a Power Reserve
    /// Test runs, as for a Positions Test: the run keeps one position.
    #[test]
    fn the_brush_waits_for_a_power_reserve_run() {
        let mut app = synthetic(60.0, 0.0);
        let ctx = themed();
        let draw = |app: &mut TimegrapherApp| {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    theme::pane_toolbar(ui, "Paper Strip", &[], |ui| app.strip_settings(ui))
                });
            });
        };
        app.brush = Some(Position::DialUp);
        draw(&mut app);
        assert_eq!(app.brush, Some(Position::DialUp));
        let mut t = reserve::Test::new(Position::DialUp, true, String::new(), 0.0);
        let beats: Vec<timegrapher_core::beats::Beat> = (0..100)
            .map(|i| timegrapher_core::beats::Beat {
                index: i,
                time: 1.0 + i as f64 * 0.125,
                quality: 1.0,
            })
            .collect();
        t.update(&beats, 0.125, 20.0);
        app.reserve = Some(t);
        assert!(app.reserve_running());
        draw(&mut app);
        assert_eq!(app.brush, None);
        app.brush = Some(Position::CrownUp);
        draw(&mut app);
        assert_eq!(app.brush, None, "it stays down while the run goes on");
    }

    /// After a masked stretch the charts stay empty until a whole
    /// averaging time has been heard again: no reading over the few beats
    /// just past the mask, and no line drawn across it.
    #[test]
    fn readings_resume_a_whole_average_after_a_mask() {
        let mut app = synthetic(60.0, 0.0);
        app.mask(20.0, 30.0);
        let after: Vec<_> = app
            .trend
            .iter()
            .filter(|p| p.t > 20.0 && p.rate.is_some())
            .map(|p| p.t)
            .collect();
        assert!(!after.is_empty());
        assert!(
            after.iter().all(|&t| t >= 30.0 + app.average_s - 1e-9),
            "{after:?}"
        );
        let points: Vec<[f64; 2]> = app
            .trend
            .iter()
            .filter_map(|p| Some([p.t, p.rate?]))
            .collect();
        let parts = segments(&points);
        assert_eq!(parts.len(), 2, "one line each side of the mask");
        assert!(parts[0].last().unwrap()[0] < 20.0);
    }

    /// On a slow beat (12,000 bph) and a short average the beats' span falls up to a
    /// beat short of the averaging time; that still counts as covered, so
    /// clean readings aren't left off the chart.
    #[test]
    fn slow_beats_fill_a_short_average() {
        let cfg = SynthConfig {
            duration_s: 30.0,
            bph: 12000,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 280.0, |_| 0.0);
        let mut live = LiveAnalyzer::new(audio.sample_rate, LiveConfig::default());
        for b in audio.samples.chunks(960) {
            live.push(b);
        }
        let points = trend_over(&live, 2.0);
        // The last point, at the newest sound, isn't settled yet.
        let end = live.duration_s() - 1.0;
        let late: Vec<_> = points.iter().filter(|p| p.t > 8.0 && p.t < end).collect();
        assert!(!late.is_empty());
        // None left off that the engine read.
        let left_off: Vec<f64> = late
            .iter()
            .filter(|p| p.rate.is_none() && live.reading_at(p.t, 2.0).rate_s_per_day.is_some())
            .map(|p| p.t)
            .collect();
        assert!(left_off.is_empty(), "{left_off:?} of {}", late.len());
    }

    #[test]
    fn a_line_breaks_where_its_points_stop() {
        let pts = [[0.0, 1.0], [1.0, 1.0], [2.0, 1.0], [10.0, 1.0], [11.0, 1.0]];
        let parts = segments(&pts);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1], [[10.0, 1.0], [11.0, 1.0]]);
        assert_eq!(segments(&[]).len(), 0);
    }

    /// A take listened to for more than a day keeps its first hours: the
    /// app keeps as many beats as a power reserve run does (a 26 h take at
    /// 28,800 bph lost its first 1 h 40 min to the engine's day), and the
    /// strip can be that long.
    #[test]
    fn a_long_take_keeps_its_first_hours() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let kept = app.live_config().max_beats;
        assert!(kept >= reserve::RUN_BEATS_KEPT);
        assert!(kept as f64 >= 80.0 * 3600.0 * 8.0, "{kept}");
        app.strip.set_span(26.0 * 3600.0);
        assert_eq!(app.strip.span_s, 26.0 * 3600.0);
        app.strip.set_span(1e9);
        assert_eq!(app.strip.span_s, power_reserve::LIMIT_S);
    }

    /// A long take's charts worked out again keep a reading every 20 s or
    /// so, and those read live since come every half second: the line runs
    /// on through both, and the value under the pointer is found in both,
    /// where the line used to break at every one of the sparse readings.
    #[test]
    fn a_line_runs_on_where_its_spacing_changes() {
        let mut pts: Vec<[f64; 2]> = (0..4000).map(|i| [i as f64 * 20.0, 1.0]).collect();
        let t0 = pts.last().unwrap()[0];
        pts.extend((1..20_000).map(|i| [t0 + i as f64 * 0.5, 2.0]));
        assert_eq!(segments(&pts).len(), 1);
        assert_eq!(value_at(&pts, 1009.0), Some(1.0));
        assert_eq!(value_at(&pts, t0 + 100.2), Some(2.0));
        // A stretch with nothing read is still a gap, in either.
        let holed: Vec<[f64; 2]> = pts
            .iter()
            .copied()
            .filter(|p| !(1000.0..1100.0).contains(&p[0]))
            .collect();
        assert_eq!(segments(&holed).len(), 2);
        assert_eq!(value_at(&holed, 1050.0), None);
    }

    /// Masked stretches are saved in the take's own seconds, which leave
    /// out pauses, and come back from its session file on the strip.
    #[test]
    fn masked_stretches_are_saved_with_the_take() {
        let base = std::env::temp_dir().join(format!("tg-app-masked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.session.listen();
        let info = app.info("test", 100, 16);
        app.session.recorder = Some(Recorder::start(&base, info).unwrap());
        let write = |app: &mut TimegrapherApp, s: usize, at: u64| {
            app.session
                .recorder
                .as_mut()
                .unwrap()
                .write(&timegrapher_core::capture::Block {
                    samples: vec![0.0; s * 100],
                    at: std::time::UNIX_EPOCH + std::time::Duration::from_secs(at),
                })
                .unwrap();
        };
        write(&mut app, 60, 60);
        app.session.pause(60.0).unwrap().unwrap();
        app.session.resume("test").unwrap();
        app.session.resumed_at(120.0);
        write(&mut app, 10, 130);
        let before = app.save_metadata();
        app.set_masked(vec![(125.0, 128.0), (10.0, 20.0)]);
        assert_ne!(app.save_metadata(), before, "masking is an edit to save");
        let m = app.take_manifest();
        // Every entry lists every mask, so none is lost outside a stretch.
        assert!(!m.recordings.is_empty());
        for r in &m.recordings {
            assert_eq!(r.masked, [[10.0, 20.0], [65.0, 68.0]]);
        }
        let text = toml::to_string_pretty(&m).unwrap();
        let again: Manifest = toml::from_str(&text).unwrap();
        let mut opened = TimegrapherApp::with_devices(Vec::new(), None);
        opened.apply_manifest(&again, &again.recordings, None);
        assert_eq!(opened.masked, [(10.0, 20.0), (65.0, 68.0)]);
        // Nothing masked: no masked line in the file.
        app.set_masked(Vec::new());
        assert!(!toml::to_string_pretty(&app.take_manifest())
            .unwrap()
            .contains("masked"));
        app.session.discard_recording();
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A take is read with its calibre's settings where its session file
    /// gives none, as the Watches list and `session` read it.
    #[test]
    fn a_take_is_read_with_its_calibres_settings() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.cal
            .user
            .custom
            .push(timegrapher_core::calibre_store::CustomCalibre {
                name: "Test 21".into(),
                maker: None,
                aliases: Vec::new(),
                bph: Some(21600),
                lift_angle_deg: Some(49.0),
                escape_teeth: Some(21),
                wheels: Vec::new(),
                notes: None,
            });
        let read = |text: &str| {
            let m: Manifest = toml::from_str(text).unwrap();
            app.entry_settings(&m).stream.analysis
        };
        let entry = "\n[[recording]]\nfile = '.'\nposition = 'DU'\n";
        let a = read(&format!("calibre = 'Test 21'\n{entry}"));
        assert_eq!(a.escape_teeth, 21);
        assert_eq!(a.bph, Some(21600));
        // The session file's own figures come first.
        let a = read(&format!("calibre = 'Test 21'\nescape_teeth = 18\n{entry}"));
        assert_eq!(a.escape_teeth, 18);
        // No calibre, or one not known: core's 15 teeth.
        assert_eq!(read(entry).escape_teeth, 15);
        assert_eq!(
            read(&format!("calibre = 'Unknown'\n{entry}")).escape_teeth,
            15
        );
    }

    /// A take whose entries say how long each position settles, as
    /// `timegrapher record` writes them, is read once each has settled, as
    /// `session` reads it: the settling is marked, not read. The app's own
    /// takes say `settle_s = 0`, so theirs read from their start.
    #[test]
    fn a_recorded_takes_positions_are_read_once_settled() {
        let text = "audio = true\nsettle_s = 30.0\n\n\
                    [[recording]]\nfile = '.'\nposition = 'DU'\nstart_s = 0.0\nend_s = 90.0\n\n\
                    [[recording]]\nfile = '.'\nposition = 'DD'\nstart_s = 90.0\nend_s = 180.0\n\
                    measure_s = 40.0\n";
        let m: Manifest = toml::from_str(text).unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let (stretches, settles) = app.apply_manifest(&m, &m.recordings, None);
        assert_eq!(
            stretches,
            [(0.0, Position::DialUp), (90.0, Position::DialDown)]
        );
        assert_eq!(settles, [(0.0, 30.0), (90.0, 120.0)]);
        // Settled at once, as the app writes its own takes.
        let text = text.replace("settle_s = 30.0", "settle_s = 0.0");
        let m: Manifest = toml::from_str(&text).unwrap();
        let (_, settles) = app.apply_manifest(&m, &m.recordings, None);
        assert!(settles.is_empty(), "{settles:?}");
        // A take shorter than twice its settling settles to halfway, as
        // session reads it, once its length is known.
        let text = "audio = true\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n";
        let short: Manifest = toml::from_str(text).unwrap();
        let (_, settles) = app.apply_manifest(&short, &short.recordings, Some(25.0));
        assert_eq!(settles, [(0.0, 12.5)]);
        let (_, settles) = app.apply_manifest(&short, &short.recordings, None);
        assert_eq!(settles, [(0.0, timegrapher_core::take::SETTLE_S)]);
    }

    /// A take of one position whose session file gives no settling (one
    /// made by `timegrapher record`) is read once settled, as `session`
    /// reads it: its first 20 s are left out of its row.
    #[test]
    fn a_recorded_take_of_one_position_is_read_once_settled() {
        let dir = std::env::temp_dir().join(format!("tg-app-one-settle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 50.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        timegrapher_core::audio::write_wav(&dir.join("audio-001.wav"), &audio).unwrap();
        std::fs::write(
            dir.join(MANIFEST),
            "bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n",
        )
        .unwrap();
        let app = reopened(&dir);
        assert_eq!(app.session.settles(), [(0.0, 20.0)]);
        let spans = app.session.settled_spans(app.position, Position::DialUp);
        assert_eq!(spans.len(), 1, "{spans:?}");
        assert_eq!(spans[0].0, 20.0, "{spans:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A take that fails to open leaves the take on show as it was, read
    /// on its own clock and breaks.
    #[test]
    fn a_failed_open_leaves_the_takes_clock() {
        let base = std::env::temp_dir().join(format!("tg-app-failed-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let good = base.join("good");
        std::fs::create_dir_all(&good).unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 30.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        timegrapher_core::audio::write_wav(&good.join("audio-001.wav"), &audio).unwrap();
        std::fs::write(
            good.join(MANIFEST),
            "bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n",
        )
        .unwrap();
        let mut app = reopened(&good);
        let breaks = app.take_breaks.clone();
        assert!(breaks.is_some());
        // Sound that can't be read: found, then refused.
        let bad = base.join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("audio-001.wav"), b"not a wav").unwrap();
        std::fs::write(bad.join(MANIFEST), "[[recording]]\nfile = '.'\n").unwrap();
        app.start_batch(&bad);
        assert!(
            app.message.as_ref().is_some_and(|m| m.1),
            "{:?}",
            app.message
        );
        assert!(app.batch.is_none() && app.live.is_some());
        assert_eq!(app.take_breaks, breaks);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A take refused while it is read (here, a clock log of one line)
    /// says why and leaves the window no longer opening.
    #[test]
    fn a_take_refused_while_read_is_no_longer_opening() {
        let base = std::env::temp_dir().join(format!("tg-app-refused-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 20.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        timegrapher_core::audio::write_wav(&base.join("audio-001.wav"), &audio).unwrap();
        std::fs::write(base.join("clock.csv"), "1700000000.000000,0\n").unwrap();
        std::fs::write(
            base.join(MANIFEST),
            "bph = 28800\nlift = 52.0\n\n[[recording]]\nfile = '.'\nposition = 'DU'\n\
             clock = 'clock.csv'\nsettle_s = 0.0\n",
        )
        .unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.start_batch(&base);
        assert!(app.batch.is_some(), "{:?}", app.message);
        assert_eq!(app.session.state(), session::State::Analysing);
        let t = Instant::now();
        while app.batch.is_some() && t.elapsed().as_secs() < 120 {
            app.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(app.batch.is_none());
        assert!(
            app.message
                .as_ref()
                .is_some_and(|m| m.1 && m.0.contains("need at least 2")),
            "{:?}",
            app.message
        );
        assert_eq!(app.session.state(), session::State::Idle);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Guided elapsed-time endpoints use the recorder timeline even when
    /// the watch never changes position and a pause spans an endpoint.
    #[test]
    fn positions_test_manifest_maps_paused_elapsed_time_to_recorded_sound() {
        let base = std::env::temp_dir().join(format!("tg-app-guided-pause-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
            // Its start is after the settling: nothing more to skip.
            assert_eq!(m.recordings[0].settle_s, Some(0.0));
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
        app.wait_for_save();
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
    #[expect(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
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
        app.session.set_settle_s(SETTLE_S);
        let third = audio.samples.len() / 3;
        // Each 30 s block arrives as its last frame is heard, as the
        // capture delivers sound, so the clock log reads as a real take's.
        let began = std::time::SystemTime::now();
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
                at: began + std::time::Duration::from_secs(30 * (i as u64 + 1)),
            })
            .unwrap();
        }
        app.save_into(&base.join("saved"));
        app.wait_for_save();
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
        // Each start is already after its settling, and the entry says so,
        // so `timegrapher session` skips nothing more: it reads the seconds
        // the app's table read.
        assert!(
            m.recordings.iter().all(|e| e.settle_s == Some(0.0)),
            "{text}"
        );
        // A mask in the settling gap after the first move is on every
        // entry, though inside none, and comes back when the file is read.
        app.set_masked(vec![(32.0, 38.0)]);
        let m2 = app.take_manifest();
        assert_eq!(m2.recordings.len(), 3);
        for r in &m2.recordings {
            assert_eq!(r.masked, [[32.0, 38.0]]);
        }
        let mut opened = TimegrapherApp::with_devices(Vec::new(), None);
        opened.apply_manifest(&m2, &m2.recordings, None);
        assert_eq!(opened.masked, [(32.0, 38.0)]);
        app.set_masked(Vec::new());
        // Nothing painted: nothing kept as recorded.
        assert!(m.recorded.is_empty(), "{text}");
        // Painted over the last stretch: what was recorded is kept under
        // it, in the sound's seconds, as the recordings are listed; then
        // painted back, it goes.
        app.session
            .paint(75.0, 85.0, Position::CrownLeft, Position::DialUp, 90.0);
        let recorded: Vec<_> = app
            .take_manifest()
            .recorded
            .iter()
            .map(|r| (r.position.clone(), r.start_s, r.end_s))
            .collect();
        assert_eq!(
            recorded,
            [
                ("DU".to_string(), Some(0.0), Some(30.0)),
                ("DD".to_string(), Some(40.0), Some(60.0)),
                ("DU".to_string(), Some(70.0), None)
            ]
        );
        app.session
            .paint(75.0, 85.0, Position::DialUp, Position::DialUp, 90.0);
        assert!(app.take_manifest().recorded.is_empty());

        // Opened on a fresh app, the take brings it all back.
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
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
                let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
            let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
                let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
        for p in [Pane::Charts, Pane::Distributions, Pane::Strip] {
            set_pane_visible(&mut app.panes.tiles, p, false);
        }
        frames(&mut app, &ctx, 2);
        app.strip.horizontal = true;
        app.relayout(true);
        frames(&mut app, &ctx, 3);
        for p in [Pane::Charts, Pane::Distributions, Pane::Strip] {
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
        // The charts and distributions shown, as they are in use.
        for p in [Pane::Charts, Pane::Distributions] {
            set_pane_visible(tiles, p, true);
        }
        let all = visible_containers(tiles);
        set_pane_visible(tiles, Pane::Charts, false);
        assert!(!pane_visible(tiles, Pane::Charts));
        // They are tabs behind the profile, so its tab bar stays while
        // any pane shows in it,
        assert_eq!(visible_containers(tiles), all);
        set_pane_visible(tiles, Pane::Distributions, false);
        set_pane_visible(tiles, Pane::Scope, false);
        assert_eq!(visible_containers(tiles), all);
        // and goes with the last of them, nothing else.
        set_pane_visible(tiles, Pane::Sound, false);
        assert_eq!(visible_containers(tiles), all - 1);
        for p in Pane::ALL {
            set_pane_visible(tiles, p, false);
        }
        assert_eq!(visible_containers(tiles), 0, "root still shown");
        set_pane_visible(tiles, Pane::Distributions, true);
        assert!(pane_visible(tiles, Pane::Distributions));
        assert!(tiles.is_visible(tree.root.unwrap()));
        assert!(!pane_visible(tiles, Pane::Charts));

        // A new strip direction keeps the choice.
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        set_pane_visible(&mut app.panes.tiles, Pane::Sound, false);
        app.relayout(true);
        assert!(!pane_visible(&app.panes.tiles, Pane::Sound));
        assert!(pane_visible(&app.panes.tiles, Pane::Strip));
    }

    #[test]
    fn histograms_start_hidden_and_keep_their_switch_through_a_relayout() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        assert!(!pane_visible(&app.panes.tiles, Pane::Distributions));
        // The charts start hidden too, in Quick Listen's layout.
        assert!(!pane_visible(&app.panes.tiles, Pane::Charts));
        set_pane_visible(&mut app.panes.tiles, Pane::Distributions, true);
        app.relayout(true);
        assert!(pane_visible(&app.panes.tiles, Pane::Distributions));
        assert!(!pane_visible(&app.panes.tiles, Pane::Charts));
    }

    #[test]
    fn launching_and_replaying_touch_no_input() {
        // `new` lists nothing; a replay never needs to.
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
                // Settled at each entry's start, as the app writes its takes.
                "watch = 'Replay Watch'\nbph = 28800\nlift = 49.0\nsettle_s = 0.0\n\n\
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
            let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
                sided: Vec::new(),
                by_count: Vec::new(),
                snippets: Vec::new(),
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
            let reading = app.reading_at_time(75.0).unwrap();
            assert!(reading.watch_heard);
            assert!(reading.span_s <= 75.0 - 60.03 + 1e-9, "{reading:?}");
            assert!(reading.rate_s_per_day.unwrap().abs() < 0.001, "{reading:?}");
            assert!(
                app.reading_at_time(20.0)
                    .unwrap()
                    .rate_s_per_day
                    .unwrap()
                    .abs()
                    < 0.001
            );
        }
    }

    #[test]
    fn replay_restores_saved_history_and_a_position_change_preserves_it() {
        let (dir, path) = replay_context_fixture("folder", false);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.start_replay_at(&path, 100.0);
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
        app.stop();
        assert!(app.session.has_replay_context());
        // A segment alone does not match the folder's '.' entries.
        app.start_replay_at(&dir.join("audio-001.wav"), 100.0);
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
        app.start_replay_at(&path, 100.0);
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
        let mut batch = TimegrapherApp::with_devices(Vec::new(), None);
        let (manifest, entries) = read_manifest(&path).unwrap().unwrap();
        let (positions, settles) = batch.apply_manifest(&manifest, &entries, None);
        let mut replay = TimegrapherApp::with_devices(Vec::new(), None);
        replay.start_replay_at(&path, 100.0);
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
        assert!(app.reading().is_none());
        assert_eq!(app.live.as_ref().unwrap().breaks(), &[8.0, 14.0]);
        app.stop();
        drop(tx);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn replay_readouts_and_rebuilt_trends_respect_imported_settling() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
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
        app.rebuild_trend();
        assert!(!app.trend.is_empty());
        assert!(app.trend.iter().all(|p| p.t <= 30.0 || p.t > 40.0));
        assert!(app.reading_at_time(35.0).is_none());
        let after = app.reading_at_time(41.0).unwrap();
        assert!(
            after.span_s <= 1.0,
            "no fit crosses the settling end: {}",
            after.span_s
        );
        // Endpoint convention is inherited unchanged: settling_at is (from,to].
        assert!(app.reading_at_time(40.0).is_none());
    }

    #[test]
    fn a_calibre_names_the_wheels_and_a_typed_one_is_kept() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
        let mut next = TimegrapherApp::with_devices(Vec::new(), None);
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
    fn unsaved_legacy_calibres_remain_in_app_settings() {
        let root = std::env::temp_dir().join(format!(
            "tg-app-legacy-calibre-protection-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("calibres.json");
        let original = b"unreadable original catalogue";
        std::fs::write(&path, original).unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.cal = calibre_ui::Calibres::open(Some(path.clone()));
        let mut settings = Settings::default();
        settings
            .custom_calibres
            .push(crate::settings::CustomCalibre {
                name: "Legacy Bench".into(),
                wheels: vec![crate::settings::CustomWheel {
                    name: " Third Wheel ".into(),
                    period_s: 450.0,
                }],
            });
        let originals = settings.custom_calibres.clone();
        app.apply_settings(settings);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        // Actual app settings serialization preserves the original legacy form,
        // rather than only its normalized, temporary catalogue entry.
        let json = serde_json::to_string(&app.settings()).unwrap();
        let persisted: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(persisted.custom_calibres, originals);
        let mut next = TimegrapherApp::with_devices(Vec::new(), None);
        next.apply_settings(persisted.clone()); // no persistent path
        assert_eq!(next.settings().custom_calibres, originals);
        assert!(next.cal.find("Legacy Bench").is_some());
        // A new instance can load a repaired path and finish the migration.
        std::fs::remove_file(&path).unwrap();
        let mut repaired = TimegrapherApp::with_devices(Vec::new(), None);
        repaired.cal = calibre_ui::Calibres::open(Some(path.clone()));
        repaired.apply_settings(persisted);
        assert!(repaired.settings().custom_calibres.is_empty());
        assert!(calibre_ui::Calibres::open(Some(path))
            .find("Legacy Bench")
            .is_some());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A first start, with nothing saved, has the strip across and laid
    /// out above the charts, as Across lays it out.
    #[test]
    fn a_first_start_lays_the_strip_across() {
        let fresh = TimegrapherApp::with_devices(Vec::new(), None);
        assert!(fresh.strip.horizontal);
        // The first row or column above the strip: a column (the strip
        // over the charts) for Across, a row for Down.
        let holder = |t: &Tree<Pane>| {
            let mut id = t.tiles.find_pane(&Pane::Strip)?;
            loop {
                id = t.tiles.parent_of(id)?;
                if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(l))) =
                    t.tiles.get(id)
                {
                    return Some(l.dir);
                }
            }
        };
        assert_eq!(holder(&fresh.panes), holder(&default_layout(true)));
        assert_ne!(holder(&fresh.panes), holder(&default_layout(false)));
    }

    #[test]
    fn settings_come_back_as_they_were_left() {
        // A fresh app opens with the defaults the settings describe.
        let fresh = TimegrapherApp::with_devices(Vec::new(), None);
        let mut first = fresh.settings();
        first.panes = None;
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(Settings::default()).unwrap()
        );
        // Change a few views, keep them, and open a new app with them.
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        // Down, now that Across is the default: a saved Down stays.
        app.strip.horizontal = false;
        app.relayout(false);
        app.show_readings = false;
        app.sound_opts.scale = profiles::Scale::Decibels;
        app.hist_cumulative = true;
        app.folded.insert("Window".into());
        set_pane_visible(&mut app.panes.tiles, Pane::Distributions, true);
        app.set_average(30.0);
        let json = serde_json::to_string(&app.settings()).unwrap();
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
        again.apply_settings(serde_json::from_str(&json).unwrap());
        assert!(!again.strip.horizontal);
        assert!(!again.show_readings);
        assert_eq!(again.sound_opts.scale, profiles::Scale::Decibels);
        assert!(again.hist_cumulative);
        assert!(again.folded.contains("Window"));
        assert_eq!(again.average_s, 30.0);
        assert!(pane_visible(&again.panes.tiles, Pane::Distributions));
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
        again.strip.horizontal = true;
        again.apply_settings(before);
        assert!(layout_is_whole(&again.panes));
        assert!(!again.strip.horizontal, "the saved arrangement was kept");
        let sound = again.panes.tiles.find_pane(&Pane::Sound).unwrap();
        let steady = again.panes.tiles.find_pane(&Pane::SteadyReadings).unwrap();
        assert_eq!(
            again.panes.tiles.parent_of(sound),
            again.panes.tiles.parent_of(steady)
        );
    }

    /// Every word one frame of the whole window draws, at `w` × 800.
    fn window_texts(app: &mut TimegrapherApp, ctx: &egui::Context, w: f32) -> Vec<String> {
        let mut texts = Vec::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 800.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            texts.clear();
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    texts.push(t.galley.text().to_string());
                }
            }
        }
        texts
    }

    /// The app opens on Quick Listen's layout: the readings, the Paper
    /// Strip with its Rate Line and Parallel Guides, and the Tick Tock
    /// Profile with the Scope behind it. Every other pane is hidden, there
    /// is no Take or Microphone pane, and Watches is a view, not a pane.
    #[test]
    fn the_app_opens_on_the_quick_listen_layout() {
        let app = TimegrapherApp::with_devices(Vec::new(), None);
        let shown: Vec<Pane> = Pane::ALL
            .into_iter()
            .filter(|&p| pane_visible(&app.panes.tiles, p))
            .collect();
        assert_eq!(shown, [Pane::Strip, Pane::Sound, Pane::Scope]);
        assert!(app.panes.tiles.find_pane(&Pane::Watch).is_none());
        assert!(app.rate_line && app.rate_guides);
        // The Scope shares the profile's tabs, behind it.
        let sound = app.panes.tiles.find_pane(&Pane::Sound).unwrap();
        let scope = app.panes.tiles.find_pane(&Pane::Scope).unwrap();
        let tabs = app.panes.tiles.parent_of(sound).unwrap();
        assert_eq!(app.panes.tiles.parent_of(scope), Some(tabs));
        let Some(egui_tiles::Container::Tabs(t)) = app.panes.tiles.get_container(tabs) else {
            panic!("the profile sits in tabs");
        };
        assert_eq!(t.active, Some(sound));
        // A pane saved as Microphone by an older app reads back as one the
        // app has, so the rest of the settings aren't lost with it.
        let old: Pane = serde_json::from_str("\"Microphone\"").unwrap();
        assert_eq!(old, Pane::Watch);
    }

    /// A pane too short for every reading's plot draws the first ones at a
    /// readable height and names the rest; a tall one draws them all.
    #[test]
    fn a_short_pane_names_the_plots_it_has_no_room_for() {
        let run = |height: f32| {
            let ctx = egui::Context::default();
            let mut drawn = Vec::new();
            let mut fit = [false; 3];
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800.0, height),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        fit = stacked(ui, [true; 3], STACKED_MIN_H, |ui, k| {
                            drawn.push((k, ui.max_rect().height()));
                        });
                    });
                },
            );
            (drawn, painted_text(&out), fit)
        };
        // What it says it drew, for the key, is what it drew.
        let (drawn, text, fit) = run(140.0);
        assert_eq!(fit, [true, false, false]);
        assert_eq!(drawn.len(), 1, "{drawn:?}");
        assert!(drawn[0].1 >= STACKED_MIN_H, "{drawn:?}");
        assert!(
            text.contains("Amplitude and Beat Error not shown: no room"),
            "{text}"
        );
        let (drawn, text, fit) = run(600.0);
        assert_eq!(fit, [true; 3]);
        assert_eq!(drawn.len(), 3, "{drawn:?}");
        assert!(!text.contains("not shown"), "{text}");
    }

    /// Settings from before the charts and the distributions were one pane
    /// each still read: each old chart is the Charts pane, each old
    /// distribution the Distributions pane, the old charts' Match Strip is
    /// left, and the layout starts afresh with one of each, hidden, while
    /// the other settings are kept.
    #[test]
    fn an_old_layout_with_a_pane_for_each_chart_reads_back() {
        for (name, pane) in [
            ("Rate", Pane::Charts),
            ("Amplitude", Pane::Charts),
            ("BeatError", Pane::Charts),
            ("RateHistogram", Pane::Distributions),
            ("AmplitudeHistogram", Pane::Distributions),
            ("BeatErrorHistogram", Pane::Distributions),
        ] {
            let old: Pane = serde_json::from_str(&format!("\"{name}\"")).unwrap();
            assert_eq!(old, pane, "{name}");
        }
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.hist_cumulative = true;
        // An older app's file, naming a chart and a distribution the old
        // way, with Match Strip on.
        let json = serde_json::to_string(&app.settings()).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let tree = serde_json::to_string(&value["panes"])
            .unwrap()
            .replacen("\"Charts\"", "\"Rate\"", 1)
            .replacen("\"Distributions\"", "\"AmplitudeHistogram\"", 1);
        value["panes"] = serde_json::from_str(&tree).unwrap();
        value["layout"] = 2.into();
        value["span_of_strip"] = true.into();
        value.as_object_mut().unwrap().remove("chart_series");
        let old: Settings = serde_json::from_value(value).unwrap();
        let mut back = TimegrapherApp::with_devices(Vec::new(), None);
        back.apply_settings(old);
        assert!(back.hist_cumulative, "the rest is kept");
        assert_eq!(back.chart_series, RATE_ONLY);
        for p in [Pane::Charts, Pane::Distributions] {
            let n = back
                .panes
                .tiles
                .tiles()
                .filter(|t| matches!(t, egui_tiles::Tile::Pane(q) if *q == p))
                .count();
            assert_eq!(n, 1, "{p:?}");
            assert!(!pane_visible(&back.panes.tiles, p), "{p:?}");
        }
        // Each pane's readings come back as they were left.
        app.chart_series = [false, true, false];
        app.dist_series = [true, false, true];
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
        again.apply_settings(app.settings());
        assert_eq!(again.chart_series, [false, true, false]);
        assert_eq!(again.dist_series, [true, false, true]);
    }

    /// A layout saved before Quick Listen's starts afresh once; one saved
    /// since comes back as it was left.
    #[test]
    fn an_old_layout_starts_afresh_on_the_quick_listen_layout() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        set_pane_visible(&mut app.panes.tiles, Pane::Charts, true);
        let mut before = app.settings();
        assert_eq!(before.layout, LAYOUT);
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
        again.apply_settings(before.clone());
        assert!(pane_visible(&again.panes.tiles, Pane::Charts), "kept");
        // A file from before the version was kept reads as the oldest.
        let none: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(none.layout, 0);
        before.layout = LAYOUT - 1;
        let mut old = TimegrapherApp::with_devices(Vec::new(), None);
        old.apply_settings(before);
        assert!(!pane_visible(&old.panes.tiles, Pane::Charts), "afresh");
    }

    /// The Current Position card's six buttons stay big enough to read and
    /// press, in one row, where the window has room for every card's full
    /// form (1610). Narrower (1000, 800), the card is captioned Position
    /// and one pop-up of the six takes the buttons' place. Either way every
    /// card of the row stays inside the window with the Calibre card's
    /// caption whole.
    #[test]
    fn the_position_buttons_keep_their_size() {
        let ctx = themed();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let rects = |app: &mut TimegrapherApp, w: f32| {
            let mut found = Vec::new();
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(w, 800.0),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| app.panels(ctx));
                found.clear();
                for c in &out.shapes {
                    if let egui::Shape::Text(t) = &c.shape {
                        found.push((t.galley.text().to_string(), t.visual_bounding_rect()));
                    }
                }
            }
            found
        };
        let at = |found: &[(String, egui::Rect)], text: &str| {
            found
                .iter()
                .find(|(t, _)| t == text)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("{text}: {found:?}"))
        };
        for (w, buttons, caption) in [
            (1610.0, true, "Current Position"),
            (1000.0, false, "Position"),
            (800.0, false, "Position"),
        ] {
            let found = rects(&mut app, w);
            at(&found, caption);
            if buttons {
                let (du, cd) = (at(&found, "DU"), at(&found, "CD"));
                assert!((du.center().y - cd.center().y).abs() < 1.0, "{w}");
                // Next to each other, the codes' centres are far enough
                // apart for a button of 28 points or more each.
                let cu = at(&found, "CU");
                assert!(cu.center().x - du.center().x >= 2.0 * 28.0, "{w}");
            } else {
                // The pop-up shows the current position's code alone.
                at(&found, "DU");
                assert!(!found.iter().any(|(t, _)| t == "CR"), "{w}: {found:?}");
            }
            // The Calibre card's title, its Watch line.
            let calibre = found
                .iter()
                .find(|(t, _)| t.starts_with(NO_WATCH_SHORT))
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("the Watch line: {found:?}"));
            assert!(calibre.right() <= w - 12.0, "{w}: {calibre:?}");
        }
    }

    #[test]
    fn the_calibre_cards_fields_end_on_one_right_edge() {
        let ctx = themed();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        for w in [1000.0, 1280.0, 1610.0, 2560.0] {
            let (mut texts, mut frames) = (Vec::new(), Vec::new());
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(w, 900.0),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| app.panels(ctx));
                texts.clear();
                frames.clear();
                for c in &out.shapes {
                    match &c.shape {
                        egui::Shape::Text(t) => {
                            texts.push((t.galley.text().to_string(), t.visual_bounding_rect()))
                        }
                        egui::Shape::Rect(r) => frames.push(r.rect),
                        _ => {}
                    }
                }
            }
            // The smallest frame round each field's words.
            let field = |words: &str| {
                let at = texts
                    .iter()
                    .find(|(t, _)| t == words)
                    .map(|(_, r)| r.center())
                    .unwrap_or_else(|| panic!("{w}: {words}: {texts:?}"));
                frames
                    .iter()
                    .filter(|r| r.contains(at) && r.height() < 40.0)
                    .min_by(|a, b| a.area().total_cmp(&b.area()))
                    .copied()
                    .unwrap_or_else(|| panic!("{w}: no frame round {words}"))
            };
            let (calibre, bph) = (field("Not Set"), field("Auto"));
            assert!(bph.top() > calibre.bottom(), "{w}: {calibre:?} {bph:?}");
            assert!(
                (bph.right() - calibre.right()).abs() < 0.5,
                "{w}: Auto ends at {} where Not Set ends at {}",
                bph.right(),
                calibre.right()
            );
        }
    }

    /// The Current Position card sets the position only while it can:
    /// before anything is on show its six buttons can be pressed; with a
    /// take on show they only show the position, as history, their codes
    /// dimmed. Either way the card names the position with both its codes
    /// and has all six.
    #[test]
    fn the_position_card_only_shows_a_takes_position() {
        let ctx = themed();
        // The colour "DD" (never the current position here) is drawn in.
        let ink = |app: &mut TimegrapherApp| {
            let mut colour = None;
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1610.0, 800.0),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| app.panels(ctx));
                colour = out.shapes.iter().find_map(|c| match &c.shape {
                    egui::Shape::Text(t) if t.galley.text() == "DD" => {
                        Some(t.galley.job.sections[0].format.color)
                    }
                    _ => None,
                });
            }
            colour.expect("the DD button")
        };
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        assert!(app.position_editable());
        let settable = ink(&mut app);
        let texts = window_texts(&mut app, &ctx, 1610.0);
        assert!(texts.iter().any(|t| t == "Current Position"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "DU · CH"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Dial Up"), "{texts:?}");
        for p in Position::ALL {
            assert!(
                texts.iter().any(|t| t == p.code()),
                "{}: {texts:?}",
                p.code()
            );
        }
        let mut app = synthetic(20.0, 0.0);
        assert!(!app.position_editable());
        let texts = window_texts(&mut app, &ctx, 1610.0);
        for p in Position::ALL {
            assert!(
                texts.iter().any(|t| t == p.code()),
                "{}: {texts:?}",
                p.code()
            );
        }
        // Compact, the two-letter code beside the picture, once, not the
        // name, which a compact card cuts short.
        let texts = window_texts(&mut app, &ctx, 1000.0);
        assert!(texts.iter().any(|t| t == "Position"), "{texts:?}");
        // The cards are drawn before the panes; the Paper Strip's Position
        // Brush shows a code of its own.
        let cards = texts.iter().take_while(|t| *t != "Paper Strip");
        assert_eq!(cards.filter(|t| *t == "DU").count(), 1, "{texts:?}");
        for gone in ["Dial", "Up", "Dial Up"] {
            assert!(!texts.iter().any(|t| t == gone), "{gone}: {texts:?}");
        }
        let pal = theme::palette(ctx.style().visuals.dark_mode);
        assert_eq!(settable, pal.text);
        assert_eq!(ink(&mut app), pal.text_tertiary);
    }

    /// Each position's second code is Witschi's: the hour that points up
    /// standing, CH and CB lying flat; and the picture turns the watch so
    /// that hour is at the top.
    #[test]
    fn each_position_has_witschis_code_and_its_hour_up() {
        use crate::icons::Lying;
        let pairs: Vec<(&str, &str)> = Position::ALL
            .iter()
            .map(|p| (p.code(), p.witschi()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("DU", "CH"),
                ("DD", "CB"),
                ("CU", "3H"),
                ("CD", "9H"),
                ("CL", "6H"),
                ("CR", "12H")
            ]
        );
        for p in Position::ALL {
            match p.lying() {
                Lying::Hour(h) => assert_eq!(format!("{h}H"), p.witschi()),
                Lying::DialUp => assert_eq!(p, Position::DialUp),
                Lying::DialDown => assert_eq!(p, Position::DialDown),
            }
        }
    }

    /// Every pane has one place in the icon bar's groups: the core panes
    /// with Watches first, Analysis (the charts, the distributions, the
    /// Steadiness views and Isochronism) and Tests. At 1000 points every
    /// icon fits; narrower, the largest group folds into a menu, and the
    /// Tests still show.
    #[test]
    fn every_pane_has_one_place_in_the_icon_bar() {
        let mut all: Vec<Pane> = [&Pane::CORE[..], &Pane::ANALYSIS, &Pane::TESTS].concat();
        assert_eq!(Pane::CORE[0], Pane::Strip);
        assert!(!Pane::ALL.contains(&Pane::Watch), "Watches is a view");
        assert!(Pane::ANALYSIS.contains(&Pane::Isochronism));
        assert_eq!(Pane::ANALYSIS[..2], [Pane::Charts, Pane::Distributions]);
        assert_eq!(all.len(), Pane::ALL.len());
        all.sort();
        all.dedup();
        assert_eq!(all.len(), Pane::ALL.len());
        let ctx = themed();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        // At 1000 points every icon fits, with the Charts and
        // Distributions groups gone into Analysis.
        let mid = window_texts(&mut app, &ctx, 1000.0);
        assert!(mid.iter().any(|t| t == "Analysis"), "{mid:?}");
        assert!(mid.iter().any(|t| t == "Tests"), "{mid:?}");
        // No group folded (the Watch line's ⏷ aside).
        let folded = |t: &String| t.contains('⏷') && !t.starts_with(NO_WATCH_SHORT);
        assert!(!mid.iter().any(folded), "{mid:?}");
        let narrow = window_texts(&mut app, &ctx, 760.0);
        assert!(
            narrow
                .iter()
                .any(|t| t.starts_with("Analysis") && t.contains('⏷')),
            "{narrow:?}"
        );
        assert!(narrow.iter().any(|t| t == "Tests"), "{narrow:?}");
        let wide = window_texts(&mut app, &ctx, 2000.0);
        assert!(wide.iter().any(|t| t == "Analysis"), "{wide:?}");
    }

    /// Readings, in the View menu and as the first icon in the bar, hides
    /// and shows the cards.
    #[test]
    fn readings_hides_the_cards() {
        let ctx = themed();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let shown = window_texts(&mut app, &ctx, 1440.0);
        assert!(shown.iter().any(|t| t == "Listen"), "{shown:?}");
        app.show_readings = false;
        let hidden = window_texts(&mut app, &ctx, 1440.0);
        for card in ["Current Position", "Listen", "Beat Rate"] {
            assert!(!hidden.iter().any(|t| t == card), "{card}: {hidden:?}");
        }
    }

    /// Unsorted Takes puts the app back in Quick Listen: no watch, no
    /// calibre, the default lift angle and the beat rate found from the
    /// beats; with readings on show they keep their settings until the
    /// next Listen. ‹ All Watches shows the list with the watch picked
    /// still picked and its row marked.
    #[test]
    fn unsorted_takes_goes_back_to_quick_listen() {
        let base =
            std::env::temp_dir().join(format!("tg-app-test-unsorted-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        let mut w = library::Watch::new("Pepsi");
        w.lift = Some(55.0);
        w.bph = Some(21600);
        lib.add(&w).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        app.pick_watch(&pepsi);
        app.set_calibre("ETA 2824-2".into());
        app.set_lift(55.0);
        assert_eq!(app.bph, Some(21600));

        // ‹ All Watches: the list, Pepsi still picked.
        app.list_for = Some((app.watch_open.clone(), false));
        assert!(app.list_shown());
        assert_eq!(app.watch_open.as_deref(), Some(pepsi.as_str()));

        app.pick_unsorted();
        assert!(!app.list_shown(), "Unsorted Takes on show");
        assert!(app.watch_open.is_none() && app.unsorted_open);
        assert!(app.calibre.is_empty() && app.watch.is_empty());
        assert_eq!(app.lift_deg, DEFAULT_LIFT_DEG);
        assert!(!app.lift_known());
        assert_eq!(app.bph, None);

        // With readings on show, the settings wait for the next Listen.
        drop(app);
        let mut shown = synthetic(15.0, 0.0);
        shown.use_library(Some(root.clone()));
        shown.set_lift(55.0);
        shown.pick_unsorted();
        assert_eq!(shown.lift_deg, 55.0, "the readings keep their lift");
        assert!(shown.quick_defaults);
        assert!(shown.quick_listen_ready(true));
        assert_eq!(shown.lift_deg, DEFAULT_LIFT_DEG);
        assert!(!shown.quick_defaults);
        drop(shown);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A watch an older version marked Archived is listed and offered
    /// like any other, and saving it in the form clears the mark.
    #[test]
    fn an_archived_watch_is_listed_and_saving_clears_the_mark() {
        let base =
            std::env::temp_dir().join(format!("tg-app-test-archived-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        let mut w = library::Watch::new("Old Seiko");
        w.archived = true;
        lib.add(&w).unwrap();
        let id = lib.find("Old Seiko").unwrap().watch.id.clone();
        drop(lib);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        let watch = {
            let Ok(Some(l)) = &app.library else {
                panic!("no library")
            };
            let listed = watches::shown(l, "", app.watch_sort);
            assert_eq!(listed.len(), 1, "the archived watch is listed");
            l.find("Old Seiko").unwrap().watch.clone()
        };
        app.pick_watch(&id);
        assert_eq!(app.watch_open.as_deref(), Some(id.as_str()));
        app.watch_form = Some(watches::Form {
            watch,
            new: false,
            error: None,
        });
        app.save_watch_form();
        let again = library::Library::open(&root).unwrap();
        assert!(!again.find("Old Seiko").unwrap().watch.archived);
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Two minutes of beats at 28,800 bph from a watch gaining `gain`
    /// (a fraction: 1e-4 is 8.64 s a day), as a take's log.
    fn gaining_log(gain: f64) -> BeatLog {
        BeatLog {
            sample_rate: 48000,
            duration_s: 120.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: (0..960)
                .map(|index| timegrapher_core::beats::Beat {
                    index,
                    time: index as f64 / 8.0 * (1.0 - gain),
                    quality: 1.0,
                })
                .collect(),
            amplitude_windows: vec![],
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![],
            sided: Vec::new(),
            by_count: Vec::new(),
            snippets: Vec::new(),
        }
    }

    /// A take opened shows whole: Fit along the strip, and the narrowest
    /// listed width that holds every beat, with Auto-Centre off or on.
    #[test]
    fn an_opened_take_fits_the_strip() {
        for follow in [false, true] {
            let mut app = TimegrapherApp::with_devices(Vec::new(), None);
            app.follow = follow;
            app.strip.set_span(10.0);
            app.strip.set_half_width(1.0);
            // 12 ms gained over the two minutes.
            app.show_log(gaining_log(1e-4), "gaining".into(), vec![], vec![]);
            assert!((app.strip.span_s - app.latest_s()).abs() < 1e-9);
            assert!(app.strip.span_s > 119.0);
            let half = app.strip.half_width_ms;
            // Centred, the spread is ±6 ms; kept on the newest beats, it
            // reaches 12 ms on one side.
            assert_eq!(half, if follow { 20.0 } else { 10.0 }, "follow {follow}");
            let live = app.live.as_ref().unwrap();
            let anchor = app.anchor.unwrap();
            for b in live.beats() {
                let lead = anchor.lead_ms(b, 0.125);
                assert!(lead.abs() < half, "{lead} outside ±{half}");
            }
            // Listen goes back to the strip as it was, unless it was
            // changed by hand since.
            assert_eq!(
                (app.live_view().span_s, app.live_view().half_width_ms),
                (10.0, 1.0)
            );
            app.strip.set_half_width(5.0);
            assert_eq!(app.live_view(), app.strip);
        }
    }

    /// A watch's take on show, then Unsorted Takes: the take is put away
    /// and the settings go back to Quick Listen's, not by chance.
    #[test]
    fn unsorted_takes_after_a_watch_take_resets_the_settings() {
        let base =
            std::env::temp_dir().join(format!("tg-app-test-unsorted-take-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        let mut w = library::Watch::new("Pepsi");
        w.lift = Some(55.0);
        w.bph = Some(28800);
        lib.add(&w).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        app.pick_watch(&pepsi);
        app.set_calibre("ETA 2824-2".into());
        app.show_log(gaining_log(0.0), "take".into(), vec![], vec![]);
        app.input = Input::File;
        app.opened_path = base.join("take.flac").display().to_string();
        assert!(app.take_on_show().is_some());
        assert_eq!(app.bph, Some(28800));

        app.pick_unsorted();
        assert!(app.take_on_show().is_none(), "the watch's take is put away");
        assert!(app.live.is_none());
        assert!(app.calibre.is_empty());
        assert_eq!(app.bph, None);
        assert_eq!(app.lift_deg, DEFAULT_LIFT_DEG);
        assert!(!app.quick_defaults);
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A new beat rate reads the take on show again at that rate, with
    /// the settings as they are now, not as the take saved them.
    #[test]
    fn a_new_beat_rate_reads_the_take_again() {
        let dir = std::env::temp_dir().join(format!("tg-app-test-reread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let audio = generate(
            &SynthConfig {
                duration_s: 20.0,
                ..Default::default()
            },
            |_| 280.0,
            |_| 0.0,
        );
        let take = dir.join("take.wav");
        timegrapher_core::audio::write_wav(&take, &audio).unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let finish = |app: &mut TimegrapherApp| {
            let t = Instant::now();
            while app.batch.is_some() && t.elapsed().as_secs() < 120 {
                app.poll();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        };
        app.start_batch(&take);
        finish(&mut app);
        assert!(app.take_on_show().is_some());
        app.set_lift(49.0);
        app.change_bph(Some(21600));
        assert!(app.batch.is_some(), "read again");
        finish(&mut app);
        assert!(
            app.take_on_show().is_some(),
            "still on show, no Open needed"
        );
        assert_eq!(app.bph, Some(21600));
        assert_eq!(app.live.as_ref().unwrap().bph(), Some(21600));
        assert_eq!(app.lift_deg, 49.0, "the settings as they were");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The readings' cards carry what a take needs: the position, Listen
    /// and Save with the take's length (on the card from 1000 up; in its
    /// title row below that, the reading cards keeping their captions
    /// whole) and Average Over on a card of their own, the lift angle, and the beat rate's menu with the calibre: in
    /// full where the window has room (1610), one control each where it
    /// hasn't (1000). Nothing says Take, Pause or Resume, and no Replay is
    /// offered.
    #[test]
    fn the_readings_cards_carry_the_take_controls() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = themed();
        for w in [800.0, 1000.0, 1610.0] {
            let texts = window_texts(&mut app, &ctx, w);
            // The Calibre card's title is its Watch line, No Watch while
            // none is picked.
            let line = if w > 800.0 {
                format!("{NO_WATCH} ⏷")
            } else {
                format!("{NO_WATCH_SHORT} ⏷")
            };
            let mut wants = vec!["DU", "Listen", "Not Set", "Watches", "Lift", line.as_str()];
            if w > 800.0 {
                // At 800 the beat rate heads the Calibre menu, and the lift
                // has the row: nothing finds the lift, Auto finds the rate.
                wants.push("Auto");
            }
            // The take's length shows at every width, and the reading cards
            // keep their captions whole beside it.
            wants.extend(["0:00", "Rate", "Amplitude", "Beat Error"]);
            if w <= 800.0 {
                wants.extend(["Position", "Avg 10 s"]);
            }
            if w > 1000.0 {
                wants.extend([
                    "Current Position",
                    "Dial Up",
                    "10 s",
                    "CR",
                    "DU · CH",
                    "0:00",
                    "Not Started",
                    "Average Over",
                    "Auto",
                ]);
            } else if w > 800.0 {
                // The take's length in the compact Listen card's title row,
                // Average Over's pop-up saying what it is, and the beat
                // rate beside the lift on the Calibre card.
                wants.extend(["Position", "Avg 10 s", "Auto"]);
            }
            for want in wants {
                assert!(texts.iter().any(|t| t == want), "{want} at {w}: {texts:?}");
            }
            // No beats yet: the profile's level axis has no marks.
            assert!(!texts.iter().any(|t| t.starts_with("0.0000")), "{texts:?}");
            for gone in [
                "Input Clock",
                "Beat Rate",
                "Take",
                "New Take",
                "Pause",
                "Resume",
                "Replay",
                "Microphone",
            ] {
                assert!(!texts.iter().any(|t| t == gone), "{gone} at {w}: {texts:?}");
            }
            assert!(
                texts
                    .iter()
                    .any(|t| t.starts_with("Quick Listen · No watch · ")),
                "the status line: {texts:?}"
            );
        }
    }

    /// A reading with limits keeps its card's caption whole on the
    /// narrowest window: a limit with no room for its chip beside the
    /// caption is in the caption's hover.
    #[test]
    fn a_readings_limits_never_cut_its_caption() {
        let mut app = synthetic(6.0, 0.0);
        let r = app.reading().expect("a reading");
        assert!(!r.caveats.is_empty(), "6 s of beats is short");
        let ctx = themed();
        app.session.listen();
        for w in [740.0, 760.0, 780.0, 800.0, 1000.0] {
            let texts = window_texts(&mut app, &ctx, w);
            for caption in ["Rate", "Amplitude", "Beat Error"] {
                assert!(
                    texts.iter().any(|t| t == caption),
                    "{caption} at {w}: {texts:?}"
                );
            }
        }
    }

    /// Left to right the cards read Position, Calibre, Listen, Rate,
    /// Amplitude, Beat Error, at every width.
    #[test]
    fn the_cards_read_position_calibre_listen_then_the_readings() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = themed();
        for w in [800.0, 1000.0, 1610.0] {
            let mut xs = Vec::new();
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(w, 800.0),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| app.panels(ctx));
                xs.clear();
                for c in &out.shapes {
                    if let egui::Shape::Text(t) = &c.shape {
                        xs.push((t.galley.text().to_string(), t.pos.x));
                    }
                }
            }
            let at = |caption: &str| {
                xs.iter()
                    .find(|(t, _)| t == caption)
                    .map(|&(_, x)| x)
                    .unwrap_or_else(|| panic!("{caption} at {w}: {xs:?}"))
            };
            let position = if w > 1000.0 {
                "Current Position"
            } else {
                "Position"
            };
            // The Calibre card's title is its Watch line.
            let line = if w > 800.0 {
                format!("{NO_WATCH} ⏷")
            } else {
                format!("{NO_WATCH_SHORT} ⏷")
            };
            let order = [
                position,
                line.as_str(),
                "Listen",
                "Rate",
                "Amplitude",
                "Beat Error",
            ];
            for pair in order.windows(2) {
                assert!(at(pair[0]) < at(pair[1]), "{pair:?} at {w}");
            }
        }
    }

    /// Keys 1 to 6 put the watch in each position in turn, as the
    /// Position card lists them; not while a field is typed in.
    #[test]
    fn keys_one_to_six_pick_the_position() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = themed();
        let keys = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
        ];
        for (n, key) in keys.into_iter().enumerate().rev() {
            let press = |pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            };
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![press(true), press(false)],
                    ..Default::default()
                },
                |ctx| app.panels(ctx),
            );
            assert_eq!(app.position, Position::ALL[n], "key {}", n + 1);
        }
    }

    /// A key press, alone or with modifiers held, for one frame.
    fn key_frame(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
        let press = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        egui::RawInput {
            modifiers,
            events: vec![press(true), press(false)],
            ..screen()
        }
    }

    /// Cmd+1 and Cmd+2 pick the view and never move the watch. The digits
    /// alone still pick a position on the Bench, but not in Watches, nor
    /// with a modifier held. The app opens on the Bench.
    #[test]
    fn cmd_1_and_2_pick_the_view_and_the_digits_alone_the_position() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = themed();
        let _ = ctx.run(screen(), |ctx| app.panels(ctx));
        assert_eq!(app.view, View::Bench);
        let start = app.position;
        assert_ne!(start, Position::ALL[2]);
        let press = |app: &mut TimegrapherApp, key, m| {
            let _ = ctx.run(key_frame(key, m), |ctx| app.panels(ctx));
        };
        press(&mut app, egui::Key::Num2, egui::Modifiers::COMMAND);
        assert_eq!((app.view, app.position), (View::Watches, start));
        press(&mut app, egui::Key::Num3, egui::Modifiers::NONE);
        assert_eq!(app.position, start, "no positions in Watches");
        press(&mut app, egui::Key::Num1, egui::Modifiers::COMMAND);
        assert_eq!((app.view, app.position), (View::Bench, start));
        for held in [
            egui::Modifiers::SHIFT,
            egui::Modifiers::ALT,
            egui::Modifiers::CTRL,
        ] {
            press(&mut app, egui::Key::Num3, held);
            assert_eq!((app.view, app.position), (View::Bench, start), "{held:?}");
            // Pressed and let go within the frame: only the key event
            // carries the modifier, the frame's are none.
            let quick = egui::RawInput {
                modifiers: egui::Modifiers::NONE,
                ..key_frame(egui::Key::Num3, held)
            };
            let _ = ctx.run(quick, |ctx| app.panels(ctx));
            assert_eq!(app.position, start, "{held:?} on the event only");
        }
        press(&mut app, egui::Key::Num3, egui::Modifiers::NONE);
        assert_eq!(app.position, Position::ALL[2]);
    }

    /// The Watches view is the Watches view's toolbar and content, moved
    /// whole: everything the pane drew, the view draws, and the view adds
    /// only the window's own frame (the menus, the mode switch and the
    /// status bar). The pane icons and the readings cards are the Bench's
    /// and don't show.
    /// The Watches view per its design: in a wide window the list sits
    /// beside the watch, whose side stops at about 1100 pt, with Takes
    /// straight after New Take and Details and Photos after them; in a
    /// narrow one the watch shows alone, with ‹ All Watches back to the
    /// list.
    #[test]
    fn the_watches_view_puts_the_list_beside_the_watch_when_wide() {
        let base = std::env::temp_dir().join(format!("tg-app-wv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(base.join("Timegrapher Data")));
        for name in ["Rolex", "Pepsi"] {
            let mut w = library::Watch::new(name);
            w.notes = Some(String::new());
            app.watch_form = Some(watches::Form {
                watch: w,
                new: true,
                error: None,
            });
            app.save_watch_form();
        }
        assert_eq!(
            app.open_watch().map(|e| e.watch.name.as_str()),
            Some("Pepsi")
        );
        let ctx = themed();
        let tops = |app: &mut TimegrapherApp, w: f32| {
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(w, 900.0),
                )),
                ..Default::default()
            };
            let mut at: Vec<(String, egui::Rect)> = Vec::new();
            for _ in 0..3 {
                let out = ctx.run(input(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.pane_tools(ui, Pane::Watch);
                        app.watch_pane(ui);
                    });
                });
                at = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Text(t) => {
                            Some((t.galley.text().to_string(), c.shape.visual_bounding_rect()))
                        }
                        _ => None,
                    })
                    .collect();
            }
            at
        };
        let find =
            |at: &[(String, egui::Rect)], t: &str| at.iter().find(|(s, _)| s == t).map(|(_, r)| *r);
        let wide = tops(&mut app, 2560.0);
        for t in [
            "Rolex",
            "Pepsi",
            "New Take",
            "Takes",
            "Details",
            "Photos",
            "Edit…",
            "Add Photos…",
        ] {
            assert!(find(&wide, t).is_some(), "{t} not drawn: {wide:?}");
        }
        assert!(find(&wide, "All Watches").is_none());
        let y = |t: &str| find(&wide, t).unwrap().top();
        assert!(
            y("New Take") < y("Takes") && y("Takes") < y("Details") && y("Details") < y("Photos")
        );
        let right = find(&wide, "Add Photos…").unwrap().right();
        assert!(
            right <= WATCH_LIST_W + 17.0 + WATCH_SIDE_MAX + 20.0,
            "the watch's side reaches {right}"
        );
        let narrow = tops(&mut app, 800.0);
        assert!(
            find(&narrow, "Rolex").is_none(),
            "the list is hidden: {narrow:?}"
        );
        assert!(find(&narrow, "New Take").is_some());
        assert!(
            theme::tools_drawn(&ctx)
                .iter()
                .any(|k| k.contains("All Watches")),
            "{:?}",
            theme::tools_drawn(&ctx)
        );
        // Nothing on the Bench: New Take and Open are free.
        assert_eq!(app.watches_busy(), None);
        assert!(find(&wide, "Stop the take on the Bench first.").is_none());
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_watches_view_is_the_watches_content_in_the_whole_window() {
        let base = std::env::temp_dir().join(format!("tg-app-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(base.join("Timegrapher Data")));
        let mut w = library::Watch::new("Pepsi");
        w.notes = Some(String::new());
        app.watch_form = Some(watches::Form {
            watch: w,
            new: true,
            error: None,
        });
        app.save_watch_form();
        let ctx = themed();
        let wide = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(2560.0, 1440.0),
            )),
            ..Default::default()
        };
        let lines = |t: String| -> std::collections::BTreeSet<String> {
            t.lines().map(str::to_string).collect()
        };
        for listed in [false, true] {
            if listed {
                app.list_for = Some((app.watch_open.clone(), false));
            }
            let mut content = String::new();
            for _ in 0..3 {
                content = painted_text(&ctx.run(wide(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.pane_tools(ui, Pane::Watch);
                        app.watch_pane(ui);
                    });
                }));
            }
            let mut status = String::new();
            for _ in 0..2 {
                status = painted_text(&ctx.run(wide(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.status_bar(ui));
                }));
            }
            app.view = View::Watches;
            let mut window = String::new();
            for _ in 0..3 {
                window = painted_text(&ctx.run(wide(), |ctx| app.panels(ctx)));
            }
            let (content, status, window) = (lines(content), lines(status), lines(window));
            let missing: Vec<_> = content.difference(&window).collect();
            assert!(missing.is_empty(), "lost {missing:?}");
            let frame: std::collections::BTreeSet<String> =
                ["File", "Edit", "View", "Watches", "Help", "Bench"]
                    .map(str::to_string)
                    .into_iter()
                    .chain(status)
                    .collect();
            let added: Vec<_> = window
                .difference(&content)
                .filter(|t| !frame.contains(*t))
                .collect();
            assert!(added.is_empty(), "added {added:?}");
            assert!(!window.contains("Analysis"), "pane icons shown");
            assert!(!window.contains("Average Over"), "cards shown");
            assert_eq!(app.view, View::Watches, "the view stays put");
            app.view = View::Bench;
        }
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Find in Watches… shows the Watches view's list with the search
    /// field ready to type in, what it held selected so typing replaces
    /// it; Open on a take there goes to the Bench.
    #[test]
    fn find_in_watches_readies_the_search_and_open_goes_to_the_bench() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = themed();
        let _ = ctx.run(screen(), |ctx| app.panels(ctx));
        app.watch_search = "Pepsi".into();
        app.find_in_watches();
        assert_eq!(app.view, View::Watches);
        assert!(app.list_shown());
        for _ in 0..2 {
            let _ = ctx.run(screen(), |ctx| app.panels(ctx));
        }
        assert!(!app.watch_search_focus);
        assert!(ctx.wants_keyboard_input(), "search field focused");
        let typed = egui::RawInput {
            events: vec![egui::Event::Text("S".into())],
            ..screen()
        };
        let _ = ctx.run(typed, |ctx| app.panels(ctx));
        assert_eq!(app.watch_search, "S", "the old search was selected");
        let missing = std::env::temp_dir().join("tg-app-no-such-take");
        app.take_action(Some(watches::TakeAction::Open(missing)));
        assert_eq!(app.view, View::Bench);
    }

    /// A library in a folder of its own holding these watches, the first
    /// with `calibre`, for the Watch line's tests.
    fn line_app(tag: &str, names: &[&str], calibre: Option<&str>) -> (TimegrapherApp, PathBuf) {
        let base = std::env::temp_dir().join(format!("tg-app-line-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(base.join("Timegrapher Data")));
        for (i, name) in names.iter().enumerate() {
            let mut w = library::Watch::new(name);
            if i == 0 {
                w.calibre = calibre.map(str::to_string);
            }
            app.watch_form = Some(watches::Form {
                watch: w,
                new: true,
                error: None,
            });
            app.save_watch_form();
        }
        // Saving a new watch picks it: start from No Watch.
        app.pick_unsorted();
        (app, base)
    }

    fn watch_id(app: &TimegrapherApp, name: &str) -> String {
        match &app.library {
            Ok(Some(l)) => l
                .watches
                .iter()
                .find(|e| e.watch.name == name)
                .unwrap()
                .watch
                .id
                .clone(),
            _ => panic!("no library"),
        }
    }

    /// A watch picked in the list over a Quick Listen still listening asks
    /// about its unsaved sound first, or stops at once with nothing to ask
    /// about; over a watch's take listening it goes to `pick_watch`, which
    /// refuses; with nothing listening it is picked.
    #[test]
    fn a_watch_picked_over_a_quick_listen_asks_then_stops() {
        assert_eq!(list_pick_route(true, true, true), ListPick::Ask);
        assert_eq!(list_pick_route(true, true, false), ListPick::StopThenPick);
        assert_eq!(list_pick_route(true, false, true), ListPick::Pick);
        assert_eq!(list_pick_route(false, true, true), ListPick::Pick);
        // Asked and answered with nothing listening, it picks the watch.
        let (mut app, base) = line_app("list-pick", &["Pepsi"], None);
        let pepsi = watch_id(&app, "Pepsi");
        app.list_pick(pepsi.clone());
        assert_eq!(app.watch_open.as_deref(), Some(pepsi.as_str()));
        app.pick_unsorted();
        app.confirmed(
            Confirm::PickWatch(Some(pepsi.clone())),
            &egui::Context::default(),
        );
        assert_eq!(app.watch_open.as_deref(), Some(pepsi.as_str()));
        let _ = std::fs::remove_dir_all(base);
    }

    /// List Shows is kept between runs. A field no watch has shows as the
    /// calibre, the choice still kept.
    #[test]
    fn list_shows_is_kept_between_runs() {
        let (mut app, base) = line_app("list-shows", &["Pepsi"], None);
        app.watch_list_shows = Some("Customer".into());
        assert_eq!(app.list_shows(), None, "no watch has Customer");
        if let Ok(Some(l)) = &mut app.library {
            l.watches[0].watch.fields.push(library::Field {
                label: "Customer".into(),
                kind: library::FieldKind::Text,
                value: "Ada".into(),
            });
        }
        assert_eq!(app.list_shows(), Some("Customer"));
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
        again.apply_settings(app.settings());
        assert_eq!(again.watch_list_shows.as_deref(), Some("Customer"));
        let _ = std::fs::remove_dir_all(base);
    }

    /// What the Watch line's menu draws, in order.
    fn line_menu(app: &mut TimegrapherApp, rates_here: bool) -> Vec<String> {
        let ctx = themed();
        let mut text = String::new();
        for _ in 0..2 {
            text = painted_text(&ctx.run(screen(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.watch_line_menu(ui, rates_here));
            }));
        }
        text.lines().map(str::to_string).collect()
    }

    /// The Calibre card's title menu: No Watch (Quick Listen), then the
    /// watches under Recent, the newest take first, then New Watch… and
    /// Find in Watches…. Quick Listen is no longer in the Watches menu.
    #[test]
    fn the_watch_line_menu_offers_no_watch_the_recent_watches_new_and_find() {
        let (mut app, base) = line_app("menu", &["Alpha", "Beta", "Gamma"], None);
        if let Ok(Some(l)) = &mut app.library {
            let take = |when: &str| library::Take {
                name: when.into(),
                bytes: 0,
                audio: true,
                incomplete: false,
                started_utc: Some(when.into()),
                duration_s: None,
                positions: Vec::new(),
                at_end: None,
                power_reserve: None,
            };
            for e in &mut l.watches {
                match e.watch.name.as_str() {
                    "Gamma" => e.takes.push(take("2026-10-20T10:00:00Z")),
                    "Beta" => e.takes.push(take("2026-10-24T10:00:00Z")),
                    _ => {}
                }
            }
        }
        let menu = line_menu(&mut app, false);
        assert_eq!(
            menu,
            [
                format!("✓ {NO_WATCH}").as_str(),
                "Recent",
                "Beta",
                "Gamma",
                "Alpha",
                "New Watch…",
                "Find in Watches…"
            ],
        );
        let ctx = themed();
        let mut watches = String::new();
        for _ in 0..2 {
            watches = painted_text(&ctx.run(screen(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.watches_menu(ui));
            }));
        }
        assert!(!watches.contains("Quick Listen"), "{watches}");
        assert!(watches.contains("Find in Watches…"), "{watches}");
        app.pick_watch(&watch_id(&app, "Gamma"));
        assert_eq!(app.open_watch().unwrap().watch.name, "Gamma");
        let menu = line_menu(&mut app, false);
        assert_eq!(menu[0], NO_WATCH);
        assert!(menu.contains(&"✓ Gamma".to_string()), "{menu:?}");
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// For a watch with no calibre on a card too narrow for its Beat Rate,
    /// the beat rates head the title menu, the one in use ticked; a rate
    /// picked there is the card's and saved to the watch.
    #[test]
    fn the_title_menu_leads_with_the_hidden_beat_rate_for_a_watch_with_no_calibre() {
        let (mut app, base) = line_app("rates", &["Seiko"], None);
        app.pick_watch(&watch_id(&app, "Seiko"));
        assert!(app.watch_sets_here());
        let menu = line_menu(&mut app, true);
        assert_eq!(
            menu[..4],
            [
                "Beat Rate (hidden at this width)",
                "✓ Auto",
                "Other Rates",
                NO_WATCH
            ]
        );
        app.bph = Some(21600);
        let menu = line_menu(&mut app, true);
        assert_eq!(menu[1..3], ["✓ 21600 bph", "Auto"]);
        assert_eq!(
            line_menu(&mut app, false)[0],
            NO_WATCH,
            "only where it is hidden"
        );
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A watch picked on the Watch line while a stopped take is on show
    /// puts the take away, so the card shows the watch picked at once:
    /// its calibre, lift and beat rate, never the old take's.
    #[test]
    fn picking_on_the_watch_line_over_a_take_shows_the_watch_picked() {
        let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
        let (mut app, base) = line_app("over", &["Pepsi", "Seiko"], Some(&c.calibre));
        let seiko = watch_id(&app, "Seiko");
        if let Ok(Some(l)) = &mut app.library {
            let e = l.watches.iter().find(|e| e.watch.id == seiko).unwrap();
            let mut w = e.watch.clone();
            w.lift = Some(53.0);
            l.update(&w).unwrap();
        }
        // A take on show measured with other settings.
        let on_show = |app: &mut TimegrapherApp| {
            app.set_lift(56.0);
            app.change_bph(Some(19800));
            app.reset_session(48000, "syn".into());
            let audio = generate(
                &SynthConfig {
                    duration_s: 4.0,
                    ..Default::default()
                },
                |_| 280.0,
                |_| 0.0,
            );
            let live = app.live.as_mut().unwrap();
            for b in audio.samples.chunks(960) {
                live.push(b);
            }
            assert!(app.live.is_some() && !app.running());
        };
        on_show(&mut app);
        app.line_pick(Some(watch_id(&app, "Pepsi")), false);
        assert!(app.live.is_none(), "the take is put away");
        assert_eq!(app.calibre, c.calibre);
        assert_eq!(app.lift_deg, c.lift_angle_deg.unwrap());
        assert_eq!(app.bph, c.bph);
        on_show(&mut app);
        app.line_pick(Some(seiko.clone()), false);
        assert!(app.calibre.is_empty());
        assert_eq!((app.lift_deg, app.bph), (53.0, None));
        assert!(app.watch_sets_here());
        on_show(&mut app);
        app.line_pick(None, false);
        assert!(app.open_watch().is_none() && app.live.is_none());
        assert_eq!((app.lift_deg, app.bph), (DEFAULT_LIFT_DEG, None));
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Unsaved sound, Quick Listen's included, is asked about before the
    /// Watch line puts its take away: Cancel leaves everything as it was,
    /// Don't Save picks the watch.
    /// The status bar names a take with no watch as Quick Listen's, however
    /// No Watch was reached: picked on the Watch line over a watch's take
    /// that was put away, or there from the start.
    #[test]
    fn the_status_bar_names_no_watch_quick_listen_however_it_was_reached() {
        let (mut app, base) = line_app("status", &["Pepsi"], None);
        assert_eq!(app.status_words(), "Quick Listen · No watch · Not Started");
        let pepsi = watch_id(&app, "Pepsi");
        app.line_pick(Some(pepsi), false);
        assert_eq!(app.status_words(), "New Take · Pepsi · Not Started");
        // A take on show, put away by picking No Watch on the line.
        app.reset_session(48000, "test".into());
        app.session.listen();
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        app.line_pick(None, true);
        assert_eq!(app.status_words(), "Quick Listen · No watch · Not Started");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_watch_line_asks_before_putting_away_unsaved_sound() {
        let (mut app, base) = line_app("ask", &["Pepsi"], None);
        let pepsi = watch_id(&app, "Pepsi");
        app.quick_listen = true;
        app.session.listen();
        let info = app.info("", 48000, 16);
        let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 20],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        app.session.recorder = Some(r);
        app.session.set_state(session::State::Listening);
        // Stopped as the Listen card's Stop does.
        app.stop_listening();
        app.wait_for_save();
        assert!(app.quick_unsaved() && !app.needs_save_confirmation());
        app.line_pick(Some(pepsi.clone()), false);
        assert_eq!(app.confirm, Some(Confirm::PickWatch(Some(pepsi.clone()))));
        // The question shows, and stays until answered.
        let ctx = themed();
        for _ in 0..2 {
            let _ = ctx.run(screen(), |ctx| app.confirm_dialog(ctx));
        }
        assert_eq!(app.confirm, Some(Confirm::PickWatch(Some(pepsi.clone()))));
        assert!(app.open_watch().is_none() && app.session.recorder.is_some());
        // Cancel: nothing changes.
        app.confirm = None;
        assert!(app.open_watch().is_none() && app.session.recorder.is_some());
        // Don't Save.
        app.line_pick(Some(pepsi), true);
        assert!(
            app.session.recorder.is_none(),
            "the sound went, as answered"
        );
        assert_eq!(app.open_watch().unwrap().watch.name, "Pepsi");
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The lift and beat rate set on the card are saved to a watch with no
    /// calibre, as Edit Watch would, and never to one whose calibre sets
    /// them.
    #[test]
    fn the_card_saves_its_lift_and_beat_rate_to_a_watch_with_no_calibre() {
        let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
        let (mut app, base) = line_app("save", &["Pepsi", "Seiko"], Some(&c.calibre));
        let saved = |name: &str| {
            let lib = library::Library::open(&base.join("Timegrapher Data")).unwrap();
            let w = lib.watches.iter().find(|e| e.watch.name == name).unwrap();
            (w.watch.lift, w.watch.bph)
        };
        app.pick_watch(&watch_id(&app, "Seiko"));
        app.set_lift(47.0);
        app.change_bph(Some(21600));
        app.save_card_to_watch();
        assert_eq!(saved("Seiko"), (Some(47.0), Some(21600)));
        app.pick_watch(&watch_id(&app, "Pepsi"));
        assert!(!app.watch_sets_here());
        let before = saved("Pepsi");
        app.lift_deg = 44.0;
        app.save_card_to_watch();
        assert_eq!(saved("Pepsi"), before, "the calibre's to set");
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The title is cut before its ⏷, never the ⏷, and the narrowest card
    /// says No Watch.
    #[test]
    fn the_watch_line_cuts_the_name_and_keeps_its_arrow() {
        let (mut app, base) = line_app("cut", &["Seiko 6R15 Diver Bench Movement"], None);
        let ctx = themed();
        let title = |app: &mut TimegrapherApp, w: f32| {
            let mut text = String::new();
            for _ in 0..2 {
                text = painted_text(&ctx.run(screen(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.allocate_ui(Vec2::new(w, theme::TITLE_ROW_H), |ui| {
                            ui.set_max_width(w);
                            app.watch_line(ui, &[]);
                        });
                    });
                }));
            }
            let line = text.lines().find(|l| l.ends_with('⏷'));
            line.unwrap_or_default().to_string()
        };
        assert_eq!(title(&mut app, 100.0), "No Watch ⏷");
        assert_eq!(title(&mut app, 230.0), format!("{NO_WATCH} ⏷"));
        app.pick_watch(&watch_id(&app, "Seiko 6R15 Diver Bench Movement"));
        let cut = title(&mut app, 100.0);
        assert!(cut.starts_with("Seiko") && cut.ends_with("… ⏷"), "{cut}");
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The Settings gear is in the same place in both views: in Watches
    /// the pane icons hide but keep their room.
    #[test]
    fn the_gear_stays_where_it_is_in_the_watches_view() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = themed();
        let wide = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(2560.0, 1440.0),
            )),
            ..Default::default()
        };
        let bar = |app: &mut TimegrapherApp| {
            let (mut right, mut text) = (0.0, String::new());
            for _ in 0..2 {
                text = painted_text(&ctx.run(wide(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.menu_bar(ui);
                        right = ui.min_rect().right();
                    });
                }));
            }
            (right, text)
        };
        let (bench, shown) = bar(&mut app);
        app.view = View::Watches;
        let (watches, hidden) = bar(&mut app);
        assert_eq!(bench, watches);
        assert!(shown.contains("Analysis") && !hidden.contains("Analysis"));
    }

    /// A layout saved while Watches was a pane comes back without it, the
    /// rest as it was left.
    #[test]
    fn a_layout_saved_with_the_watches_pane_comes_back_without_it() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        set_pane_visible(&mut app.panes.tiles, Pane::Charts, true);
        let mut saved = app.settings();
        let mut panes = saved.panes.take().unwrap();
        add_missing(&mut panes, Pane::Watch);
        let w = panes.tiles.find_pane(&Pane::Watch).unwrap();
        assert!(panes.tiles.is_visible(w));
        saved.panes = Some(panes);
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
        again.apply_settings(saved);
        assert!(again.panes.tiles.find_pane(&Pane::Watch).is_none());
        assert!(layout_is_whole(&again.panes));
        assert!(pane_visible(&again.panes.tiles, Pane::Charts), "kept");
    }

    /// A Quick Listen still on show when a watch is picked, as New Watch…
    /// picks the watch it adds, is asked about before a take of that watch
    /// starts: Listen asks, and a Power Reserve Test waits for it to be
    /// saved or set aside.
    #[test]
    fn a_take_of_a_watch_asks_about_quick_listens_sound() {
        let base = std::env::temp_dir().join(format!("tg-app-quick-ask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let mut app = synthetic(5.0, 0.0);
        app.use_library(Some(root.clone()));
        app.session.listen();
        let info = app.info("Quick", 48000, 16);
        let mut r = Recorder::start(&base.join("scratch"), info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 15],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        app.session.recorder = Some(r);
        app.session.set_state(session::State::Paused {
            since: Instant::now(),
        });
        app.quick_listen = true;
        app.pick_watch(&pepsi);
        assert!(app.quick_unsaved(), "picking keeps the readings on show");
        // New Watch… then Add Watch picks the watch it adds.
        app.watch_form = Some(watches::Form {
            watch: library::Watch::new("Speedy"),
            new: true,
            error: None,
        });
        app.save_watch_form();
        assert_eq!(
            app.open_watch().map(|e| e.watch.name.clone()).as_deref(),
            Some("Speedy")
        );
        assert!(
            app.quick_unsaved(),
            "the readings on show are Quick Listen's"
        );
        app.new_take(NewTake::Listen, false);
        assert_eq!(app.confirm, Some(Confirm::NewTake(NewTake::Listen)));
        // The question shows, rather than carrying on unasked.
        let ctx = themed();
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| app.confirm_dialog(ctx));
        }
        assert_eq!(app.confirm, Some(Confirm::NewTake(NewTake::Listen)));
        assert!(app.session.recorder.is_some(), "nothing let go yet");
        app.confirm = None;
        app.start_reserve();
        assert!(app.session.recorder.is_some() && app.reserve.is_none());
        // Start Test asks the same question.
        assert_eq!(app.confirm, Some(Confirm::ReserveStart));
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| app.confirm_dialog(ctx));
        }
        assert_eq!(
            app.confirm,
            Some(Confirm::ReserveStart),
            "shown, not passed"
        );
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Quick Listen's sound that wasn't saved goes without a question at a
    /// quit or a new take, and isn't kept to offer back.
    #[test]
    fn quick_listen_sound_goes_without_asking() {
        let base = std::env::temp_dir().join(format!("tg-app-quick-go-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = synthetic(5.0, 0.0);
        app.session.listen();
        let info = app.info("Quick", 48000, 16);
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
        app.quick_listen = true;
        assert!(app.unsaved_s() > 5.0);
        assert!(!app.needs_save_confirmation());
        eframe::App::on_exit(&mut app, None);
        assert!(kept_sounds(&base).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The Calibre card's controls stay inside their rows: the Calibre
    /// pop-up on the figure row, and under it the lift alone on the
    /// narrowest card, the beat rate beside it on a roomier one and on the
    /// full card.
    #[test]
    fn the_calibre_controls_fit_a_narrow_card() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.bph = Some(28800);
        let ctx = themed();
        let inner = |card: f32| card - 2.0 * f32::from(theme::CARD_PADDING);
        for w in [
            inner(CALIBRE_COMPACT_W),
            inner(CALIBRE_ROOMY_W),
            inner(CALIBRE_W),
        ] {
            let mut used = egui::Rect::NOTHING;
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let rect = egui::Rect::from_min_size(
                            ui.cursor().min,
                            Vec2::new(w, theme::CONTROL_H),
                        );
                        let mut child = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        child.spacing_mut().item_spacing.x = 8.0;
                        child.spacing_mut().interact_size.y = theme::CONTROL_H;
                        app.calibre_control(&mut child);
                        used = child.min_rect();
                        let mut child = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(rect.translate(Vec2::new(0.0, 40.0)))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        child.spacing_mut().item_spacing.x = 8.0;
                        child.spacing_mut().interact_size.y = theme::CONTROL_H;
                        app.rate_and_lift(&mut child);
                        let row = child.min_rect();
                        used = egui::Rect::from_min_size(
                            used.min,
                            Vec2::new(
                                used.width().max(row.width()),
                                used.height().max(row.height()),
                            ),
                        );
                    });
                });
            }
            assert!(
                used.width() <= w + 0.5 && used.height() <= theme::CONTROL_H + 0.5,
                "{:?} in a {w} pt row",
                used.size()
            );
        }
    }

    /// A card's limits show in a chip in its title row, in Title Case, the
    /// first of them with how many more; the whole sentences are its
    /// hover. Where the words would push the card's caption off it, the
    /// chip gives how many limits there are.
    #[test]
    fn a_cards_limits_show_in_a_chip_with_the_whole_in_its_hover() {
        assert_eq!(chip_words("9 s of beats"), "9 s of Beats");
        assert_eq!(chip_words("3% clipped"), "3% Clipped");
        assert_eq!(chip_words("28800 bph heard"), "28800 bph Heard");
        assert_eq!(chip_words("sides named by count"), "Sides Named by Count");
        let ctx = themed();
        // Fewer seconds of beats than an analysis asks for: a limit on
        // every reading.
        let noise = synthetic(6.0, 0.0);
        let r = noise.reading().expect("a reading");
        let limits = reading_limits("Beat Error", &r.caveats, noise.lift_deg, r.beats_used);
        assert!(!limits.is_empty(), "{:?}", r.caveats);
        let chip = match limits.len() {
            1 => chip_words(&limits[0].0),
            n => format!("{} +{}", chip_words(&limits[0].0), n - 1),
        };
        let drawn = |w: f32| {
            let mut texts = Vec::new();
            for _ in 0..2 {
                let out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(w, 820.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let _ = noise.readouts(ui);
                        });
                    },
                );
                texts = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
            }
            texts
        };
        let wide = drawn(1610.0);
        assert!(wide.contains(&chip), "{chip}: {wide:?}");
        for caption in ["Rate", "Amplitude", "Beat Error"] {
            assert!(wide.iter().any(|t| t == caption), "{caption}: {wide:?}");
        }
        // Too narrow for the words beside the caption, they go to the
        // caption's hover: never a bare count.
        let narrow = drawn(800.0);
        let count = limits.len().to_string();
        assert!(!narrow.contains(&count), "{count}: {narrow:?}");
        assert!(narrow.iter().any(|t| t == "Rate"), "{narrow:?}");
    }

    /// Hiss with the beat rate set: a reading of whatever peaks there are,
    /// with its limits named.
    fn noisy_app() -> TimegrapherApp {
        let mut noise = TimegrapherApp::with_devices(Vec::new(), None);
        noise.bph = Some(28_800);
        noise.reset_session(48000, "noise".into());
        let mut seed = 12_345_u32;
        let hiss: Vec<f32> = (0..48_000 * 20)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 * 0.02 - 0.01
            })
            .collect();
        let live = noise.live.as_mut().unwrap();
        for b in hiss.chunks(960) {
            live.push(b);
        }
        noise.poll();
        assert!(noise.reading().is_some_and(|r| !r.watch_heard));
        noise
    }

    /// The reading cards are one height at every width, whatever they
    /// show: waiting, reading, settling, or with their limits named.
    #[test]
    fn the_cards_keep_their_height_whatever_they_show() {
        let ctx = themed();
        let height = |app: &TimegrapherApp, w: f32| {
            let mut h = 0.0;
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(w, 820.0),
                    )),
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let top = ui.cursor().top();
                        let _ = app.readouts(ui);
                        h = ui.cursor().top() - top;
                    });
                });
            }
            h
        };
        // A take opened.
        let waiting = synthetic(1.0, 0.0);
        assert!(waiting.reading().is_none_or(|r| r.rate_s_per_day.is_none()));
        let reading = synthetic(60.0, 0.0);
        assert!(reading.reading().is_some());
        let mut settling = synthetic(60.0, 0.0);
        settling
            .session
            .load(&[(0.0, Position::DialUp), (55.0, Position::DialDown)], &[]);
        settling.position = Position::DialDown;
        settling.session.start_settle(55.0);
        let mut masked = synthetic(60.0, 0.0);
        masked.mask(5.0, 30.0);
        let noise = noisy_app();
        for w in [800.0, 1280.0, 1900.0] {
            let h = height(&waiting, w);
            assert_eq!(
                h,
                theme::READINGS_CARD_H + ctx.style().spacing.item_spacing.y
            );
            for app in [&reading, &settling, &masked, &noise] {
                assert_eq!(height(app, w), h, "at {w} pt wide");
            }
        }
    }

    /// The watch form's lift angle and beat rate are the calibre's where it
    /// gives them, shown and not set; with no calibre both are set there,
    /// the beat rate Auto until picked, and there is no Clear.
    #[test]
    fn the_watch_form_takes_lift_and_beat_rate_from_the_calibre_or_from_you() {
        let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
        let lift = c.lift_angle_deg.expect("its lift");
        let bph = c.bph.expect("its beat rate");
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = themed();
        let drawn = |app: &mut TimegrapherApp| {
            let mut all = String::new();
            for _ in 0..2 {
                let out = ctx.run(egui::RawInput::default(), |ctx| {
                    app.watch_form_dialog(ctx);
                });
                all = format!("{:?}", out.shapes);
            }
            all
        };
        let mut w = library::Watch::new("Diver");
        w.calibre = Some(c.calibre.clone());
        w.lift = Some(55.0);
        app.watch_form = Some(watches::Form {
            watch: w,
            new: true,
            error: None,
        });
        let d = drawn(&mut app);
        let from = format!("{}° from the calibre", fields::plain(lift));
        assert!(d.contains(&from), "{d}");
        assert!(d.contains(&format!("{bph} bph from the calibre")), "{d}");
        assert!(!d.contains("Clear"), "{d}");
        let w = &app.watch_form.as_ref().unwrap().watch;
        assert_eq!((w.lift, w.bph), (Some(lift), Some(bph)), "the calibre's");

        let mut bare = library::Watch::new("Pepsi");
        app.watch_form = Some(watches::Form {
            watch: bare.clone(),
            new: true,
            error: None,
        });
        let d = drawn(&mut app);
        assert!(d.contains("52°") && d.contains("(default)"), "{d}");
        bare.lift = Some(48.0);
        app.watch_form = Some(watches::Form {
            watch: bare,
            new: true,
            error: None,
        });
        let d = drawn(&mut app);
        assert!(d.contains("48°") && d.contains("\"Auto\""), "{d}");
        assert!(
            !d.contains("from the calibre") && !d.contains("Clear"),
            "{d}"
        );
    }

    /// With no watch picked, or a watch with no calibre, the Calibre card
    /// asks for the beat rate and the lift angle under the Calibre pop-up;
    /// a watch with a calibre the table knows names it instead.
    #[test]
    fn the_cards_ask_for_beat_rate_and_lift_only_without_a_calibre() {
        let base = std::env::temp_dir().join(format!("tg-app-cards-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        let c = timegrapher_core::calibres::find("2824-2").expect("ETA 2824-2 in the table");
        let mut known = library::Watch::new("Diver");
        known.calibre = Some(c.calibre.clone());
        lib.add(&known).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let diver = lib.find("Diver").unwrap().watch.id.clone();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        let ctx = themed();
        let drawn = |app: &mut TimegrapherApp| {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(CALIBRE_W - 2.0 * f32::from(theme::CARD_PADDING));
                    ui.horizontal(|ui| app.calibre_control(ui));
                    ui.horizontal(|ui| app.rate_and_lift(ui));
                });
            });
            // The lift angle's field: the one control a control's height.
            let field = out.shapes.iter().any(|c| match &c.shape {
                egui::Shape::Rect(r) => (r.rect.height() - theme::CONTROL_H).abs() < 0.5,
                _ => false,
            });
            format!("{:?}{}", out.shapes, if field { "LIFT FIELD" } else { "" })
        };
        // The beat rate's pop-up reads Auto, and the lift angle has its
        // field; a calibre's angle is a label.
        let asks = |d: &str| d.contains("\"Auto\"") && d.contains("LIFT FIELD");
        let none = drawn(&mut app);
        assert!(asks(&none) && none.contains("Not Set"), "{none}");
        app.pick_watch(&diver);
        let named = drawn(&mut app);
        assert!(!asks(&named), "{named}");
        assert!(named.contains(&c.calibre), "the calibre named: {named}");
        app.pick_watch(&pepsi);
        assert!(
            app.calibre.is_empty(),
            "not the calibre of the watch before"
        );
        let bare = drawn(&mut app);
        assert!(asks(&bare), "{bare}");
        assert!(
            !bare.contains("Not Set"),
            "no calibre menu for a watch: {bare}"
        );
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The first launch asks which microphone; once one is set up (or has
    /// opened), every launch starts Quick Listen, unless Listen When the
    /// App Opens is off. A recording asked for opens instead.
    #[test]
    fn the_app_asks_for_a_microphone_once_then_opens_listening() {
        let launch = |s: Settings| {
            let mut app = TimegrapherApp::with_devices(Vec::new(), None);
            app.apply_settings(s);
            app.at_launch();
            (app.setup_sheet, app.listen_pending)
        };
        assert_eq!(launch(Settings::default()), (true, false), "first launch");
        let set_up = Settings {
            mic_set_up: true,
            ..Settings::default()
        };
        assert_eq!(launch(set_up.clone()), (false, true));
        assert_eq!(
            launch(Settings {
                listen_at_open: false,
                ..set_up.clone()
            }),
            (false, false)
        );
        // Not Now: the sheet doesn't come back, and the app doesn't listen
        // by itself until a microphone is set up.
        assert_eq!(
            launch(Settings {
                setup_declined: true,
                ..Settings::default()
            }),
            (false, false),
            "after Not Now"
        );
        let mut opening = TimegrapherApp::with_devices(Vec::new(), None);
        opening.apply_settings(set_up);
        opening.input = Input::File;
        opening.at_launch();
        assert!(!opening.setup_sheet && !opening.listen_pending);
    }

    /// Where each text is painted, its top left.
    fn painted_at(out: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos)),
                _ => None,
            })
            .collect()
    }

    /// Set Up the Microphone's status line holds a failure in its own two
    /// lines: the buttons below it don't move, and the failure is said
    /// there, not in a paragraph under the controls. Before anything
    /// starts it says "Not listening.".
    #[test]
    fn the_sheets_status_line_holds_a_failure_without_moving_the_buttons() {
        let ctx = themed();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.setup_sheet = true;
        // Already tried: a test never opens an input.
        app.setup_tried = true;
        let draw = |app: &mut TimegrapherApp| {
            let mut out = None;
            for _ in 0..3 {
                out = Some(ctx.run(screen(), |ctx| app.setup_sheet_dialog(ctx)));
            }
            painted_at(&out.unwrap())
        };
        let quiet = draw(&mut app);
        let at = |drawn: &[(String, egui::Pos2)], t: &str| {
            drawn
                .iter()
                .find(|(s, _)| s == t)
                .map(|d| d.1)
                .unwrap_or_else(|| panic!("no {t}: {drawn:?}"))
        };
        assert!(quiet.iter().any(|(t, _)| t == "Not listening."));
        app.message = Some((
            "Can't open the input. This input is busy or missing. If the desktop sound \
             server is running it is probably holding it: choose the system default instead, \
             or close what is using it. Otherwise check the microphone is plugged in. (The \
             requested device is no longer available. For example, it has been unplugged.)"
                .into(),
            true,
        ));
        let failed = draw(&mut app);
        assert!(failed
            .iter()
            .any(|(t, _)| t.starts_with("Can't open the input")));
        assert!(!failed.iter().any(|(t, _)| t == "Not listening."));
        for button in ["Done", "Not Now"] {
            assert_eq!(at(&quiet, button), at(&failed, button), "{button} moved");
        }
        assert!(app.setup_sheet, "still open");
    }

    /// A message about a setting takes its row's caption: Keep Awake's
    /// fault replaces "While listening…", and the rows below it stay put.
    #[test]
    fn a_settings_message_takes_its_rows_caption() {
        let ctx = themed();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let draw = |app: &mut TimegrapherApp| {
            let mut out = None;
            for _ in 0..2 {
                out = Some(ctx.run(screen(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.general_settings(ui));
                }));
            }
            painted_at(&out.unwrap())
        };
        let fine = draw(&mut app);
        app.keep_awake = true;
        // As a desktop with no session bus says it: plain words in the
        // caption, on one line, and the system's own words in the hover.
        app.awake_fault = Some(
            "Failed to connect to address `unix:path=/run/user/0/bus`: No such file or \
             directory (os error 2)"
                .into(),
        );
        let fault = draw(&mut app);
        let y = |drawn: &[(String, egui::Pos2)], t: &str| {
            drawn
                .iter()
                .find(|(s, _)| s == t)
                .map(|d| d.1.y)
                .unwrap_or_else(|| panic!("no {t}: {drawn:?}"))
        };
        assert!(fault
            .iter()
            .any(|(t, _)| t == "Not being kept awake: the system didn't allow it."));
        assert!(!fault.iter().any(|(t, _)| t.contains("unix:path")));
        assert!(!fault.iter().any(|(t, _)| t.starts_with("While listening")));
        assert_eq!(
            y(&fine, "Check for Updates at Start"),
            y(&fault, "Check for Updates at Start")
        );
        for title in ["Appearance", "Listening", "Updates"] {
            assert!(fine.iter().any(|(t, _)| t == title), "{title}");
        }
    }

    /// An input that won't open says so in the Microphone row's caption,
    /// on one line, and the row below stays put.
    #[test]
    fn an_input_that_wont_open_says_so_in_its_rows_caption() {
        let ctx = themed();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let draw = |app: &mut TimegrapherApp| {
            let mut out = None;
            for _ in 0..2 {
                out = Some(ctx.run(screen(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.microphone_page(ui));
                }));
            }
            painted_at(&out.unwrap())
        };
        let fine = draw(&mut app);
        assert!(fine
            .iter()
            .any(|(t, _)| t == "The input the watch sits on."));
        let m = "Can't open the input. A backend-specific error has occurred: ALSA \
                 function 'snd_pcm_open' failed with error 'Connection refused (111)'";
        app.input_fault = Some(m.into());
        let failed = draw(&mut app);
        assert!(failed.iter().any(|(t, _)| t == m), "{failed:?}");
        assert!(!failed
            .iter()
            .any(|(t, _)| t == "The input the watch sits on."));
        let y = |drawn: &[(String, egui::Pos2)]| {
            drawn
                .iter()
                .find(|(s, _)| s == "Show Every Input")
                .map(|d| d.1.y)
                .unwrap()
        };
        assert_eq!(y(&fine), y(&failed));
    }

    /// The panes start below the readings' cards, whatever the cards'
    /// controls take: with a watch picked the Calibre card names the
    /// calibre rather than offering its menu.
    #[test]
    fn the_panes_start_below_the_cards_and_their_controls() {
        let base = std::env::temp_dir().join(format!("tg-app-below-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        let ctx = themed();
        let check = |app: &mut TimegrapherApp| {
            let mut top_of = std::collections::HashMap::new();
            let mut bottom_of = std::collections::HashMap::new();
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1000.0, 700.0),
                    )),
                    ..Default::default()
                };
                let out = ctx.run(input, |ctx| app.panels(ctx));
                top_of.clear();
                bottom_of.clear();
                for c in &out.shapes {
                    if let egui::Shape::Text(t) = &c.shape {
                        let r = c.shape.visual_bounding_rect();
                        let k = t.galley.text().to_string();
                        // The first drawn of a text: the cards come before
                        // the panes (the Position Brush shows a code too).
                        top_of.entry(k.clone()).or_insert(r.top());
                        bottom_of.entry(k).or_insert(r.bottom());
                    }
                }
            }
            let pane = top_of["Paper Strip"];
            // The Calibre card's controls: the Calibre pop-up (or a
            // watch's word for it) and the beat rate's pop-up under it.
            let calibre = ["Not Set", "No Calibre"]
                .into_iter()
                .find(|k| bottom_of.contains_key(*k))
                .expect("the calibre");
            for control in ["DU", calibre, "Auto"] {
                assert!(
                    bottom_of[control] < pane,
                    "{control} ends at {} under the panes at {pane}",
                    bottom_of[control]
                );
            }
        };
        check(&mut app);
        app.pick_watch(&pepsi);
        check(&mut app);
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The app opens on Quick Listen, whose sound goes without a question;
    /// a Positions Test started after it, with no watch picked, is not
    /// Quick Listen, so its take is never let go unasked. (A Power Reserve
    /// Test clears the same flag before it opens the microphone.)
    #[test]
    fn a_test_after_quick_listen_is_not_quick_listen() {
        let base = std::env::temp_dir().join(format!("tg-app-test-ql-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut app = opened_recording(&base, "a.wav", 12.0);
        // Changed masks make the test ask before it opens an input, so no
        // input is opened here.
        app.mask(2.0, 5.0);
        app.quick_listen = true;
        app.start_guided(None);
        assert!(matches!(app.confirm, Some(Confirm::Microphone)));
        assert!(!app.quick_listen);
        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A Positions Test or Power Reserve Test that ends with no watch
    /// picked asks where it goes: Keep in Unsorted, Keep in a Watch… or
    /// Throw Away. Quick Listen's sound still goes without a question.
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn a_test_with_no_watch_asks_where_it_goes() {
        let base = std::env::temp_dir().join(format!("tg-app-test-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        // A take as Stop leaves it, with no input opened.
        let take = |app: &mut TimegrapherApp, n: usize| {
            app.new_session();
            app.session.listen();
            let info = app.info("test", 48000, 16);
            let mut r = Recorder::start(&base.join(format!("unsaved-{n}")), info).unwrap();
            r.write(&timegrapher_core::capture::Block {
                samples: vec![0.0; 48000 * 5],
                at: std::time::SystemTime::now(),
            })
            .unwrap();
            app.session.recorder = Some(r);
        };
        let takes_in = |dir: PathBuf| std::fs::read_dir(dir).map_or(0, |d| d.count());
        let unsorted = root.join(library::UNSORTED).join(library::TAKES);
        let in_pepsi = root.join("Pepsi").join(library::TAKES);

        // Quick Listen: no question.
        take(&mut app, 0);
        app.quick_listen = true;
        app.stop_listening();
        assert_eq!(app.test_keep, None);

        // A Positions Test: asked, with the three answers on show.
        take(&mut app, 1);
        app.quick_listen = false;
        app.positions_take = true;
        app.stop_listening();
        assert_eq!(app.test_keep, Some(TestKeep::Positions));
        let ctx = themed();
        assert!(app.confirm.is_none() && app.watch_form.is_none());
        let mut drawn = String::new();
        // Clear of the test's pane, top or foot, leaving its table in view.
        let full = screen().screen_rect.unwrap();
        let (upper, lower) = full.split_top_bottom_at_fraction(0.5);
        for (pane, clear) in [(lower, upper), (upper, lower)] {
            app.results_rects[0] = Some(pane.shrink(8.0));
            let mut title = None;
            for _ in 0..3 {
                let out = ctx.run(screen(), |ctx| app.test_keep_dialog(ctx));
                drawn = painted_text(&out);
                for c in &out.shapes {
                    if let egui::Shape::Text(t) = &c.shape {
                        if t.galley.text() == "Keep This Positions Test?" {
                            title = Some(c.shape.visual_bounding_rect());
                        }
                    }
                }
            }
            let title = title.expect("the sheet's title");
            assert!(
                clear.contains_rect(title),
                "the sheet's title at {title:?}, the pane at {pane:?}"
            );
        }
        for t in [
            "Keep This Positions Test?",
            "Keep in Unsorted",
            "Keep in a Watch…",
            "Throw Away",
        ] {
            assert!(drawn.contains(t), "{t}: {drawn}");
        }
        // Keep in Unsorted.
        app.keep_test(TestKeep::Positions, None);
        app.wait_for_save();
        assert_eq!(app.test_keep, None);
        assert_eq!(takes_in(unsorted.clone()), 1);

        // Keep in a Watch: Pepsi.
        take(&mut app, 2);
        app.positions_take = true;
        app.watch_open = None;
        app.stop_listening();
        assert_eq!(app.test_keep, Some(TestKeep::Positions));
        app.keep_test(TestKeep::Positions, Some(&pepsi));
        app.wait_for_save();
        assert_eq!(takes_in(in_pepsi.clone()), 1);

        // Throw Away: nothing kept, nothing left to ask about.
        take(&mut app, 3);
        app.positions_take = true;
        app.watch_open = None;
        app.stop_listening();
        assert_eq!(app.test_keep, Some(TestKeep::Positions));
        app.new_session();
        assert_eq!(app.test_keep, None);
        assert!(!app.needs_save_confirmation());
        assert_eq!(takes_in(unsorted.clone()), 1);
        assert_eq!(takes_in(in_pepsi.clone()), 1);

        // A Power Reserve Test with no watch: asked before it is saved,
        // then kept in Unsorted.
        take(&mut app, 4);
        app.watch_open = None;
        let started = app
            .session
            .recorder
            .as_ref()
            .unwrap()
            .started_utc()
            .to_string();
        app.reserve = Some(reserve::Test::new(Position::DialUp, true, started, 0.0));
        app.stop_listening();
        assert_eq!(app.test_keep, Some(TestKeep::PowerReserve));
        assert_eq!(takes_in(unsorted.clone()), 1, "not saved before the answer");
        app.keep_test(TestKeep::PowerReserve, None);
        app.wait_for_save();
        assert_eq!(takes_in(unsorted.clone()), 2);

        drop(app);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_paper_strip_is_the_largest_area_at_1000_by_700() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = themed();
        let mut top_of = std::collections::HashMap::new();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1000.0, 700.0),
                )),
                ..Default::default()
            };
            let out = ctx.run(input, |ctx| app.panels(ctx));
            for c in &out.shapes {
                if let egui::Shape::Text(t) = &c.shape {
                    let r = c.shape.visual_bounding_rect();
                    top_of.insert(t.galley.text().to_string(), r.top());
                }
            }
        }
        let cards = top_of["Paper Strip"] - top_of["Beat Error"];
        let strip = top_of["Tick Tock Profile"] - top_of["Paper Strip"];
        let profile = top_of[&app.status_words()] - top_of["Tick Tock Profile"];
        assert!(
            strip > cards && strip > profile,
            "the strip {strip} against the cards {cards} and the profile {profile}"
        );
        // The profile keeps about a third of the room below the cards.
        assert!(
            profile > (strip + profile) * 0.38,
            "the profile {profile} against the strip {strip}"
        );
        // Each toolbar keeps to one row, from its first control to its last
        // (or its "…" menu, when the pane is too narrow for them all).
        let tools = theme::tools_drawn_at(&ctx);
        let row = |p: Pane| {
            let ys: Vec<f32> = tools
                .iter()
                .filter(|(k, _)| k.starts_with(&format!("{}/", p.title())))
                .map(|(_, r)| r.center().y)
                .collect();
            ys.len() > 1 && ys.iter().all(|y| (y - ys[0]).abs() < 4.0)
        };
        assert!(row(Pane::Strip), "{tools:?}");
        assert!(row(Pane::Sound), "{tools:?}");
    }

    /// A chart's scale holds every point, a glitch too.
    #[test]
    fn the_scale_holds_every_point() {
        let mut pts: Vec<[f64; 2]> = (0..60).map(|i| [i as f64, 5.0 + (i % 5) as f64]).collect();
        pts.extend((60..66).map(|i| [i as f64, 800.0]));
        let (lo, hi) = data_range(&pts).unwrap();
        assert!(lo < 5.0 && hi > 800.0, "{lo} {hi}");
        assert_eq!(data_range(&[]), None);
        let (lo, hi) = data_range(&[[0.0, 3.0]]).unwrap();
        assert!(lo < 3.0 && hi > 3.0);
    }

    #[test]
    fn tick_and_tock_amplitude_dash_over_their_own_colours() {
        let pal = theme::palette(true);
        let key = chart_key([false, true, false], pal);
        assert_eq!(key[0], ("Average Amplitude", pal.trace_amplitude, None));
        assert_eq!(key[1], ("From Tick", pal.trace_amplitude, Some(pal.tick)));
        assert_eq!(key[2], ("From Tock", pal.trace_amplitude, Some(pal.tock)));
        assert_eq!(key.len(), 3);
        // The key follows the charts shown: none for the rate alone.
        assert!(chart_key([true, false, false], pal).is_empty());
        assert_eq!(chart_key([true; 3], pal).len(), 5);
    }

    #[test]
    fn a_chart_labels_no_time_before_the_start() {
        let r = -30.0..=600.0;
        assert_eq!(chart_time_label(-30.0, 60.0, &r), "");
        assert_eq!(chart_time_label(0.0, 60.0, &r), "0:00");
        assert_eq!(chart_time_label(120.0, 60.0, &r), "2:00");
        // Marks under a second apart don't repeat a label.
        let r = 9.0..=10.0;
        assert_eq!(chart_time_label(9.2, 0.2, &r), "0:09.2");
        assert_eq!(chart_time_label(9.4, 0.2, &r), "0:09.4");
    }

    /// Settings' Library section reads the library at the folder chosen,
    /// draws what it holds, moves it and keeps using it where it went,
    /// and the choice is kept between runs.
    #[test]
    fn settings_moves_the_library_and_keeps_its_place() {
        let base = std::env::temp_dir().join(format!("tg-app-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("here").join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        app.open_settings(Tab::WatchesFolder);
        assert_eq!(
            app.library
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .watches
                .len(),
            1
        );
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let text = |ctx: &egui::Context, app: &mut TimegrapherApp| {
            let mut all = String::new();
            for _ in 0..2 {
                let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
                all = format!("{:?}", out.shapes);
            }
            all
        };
        let drawn = text(&ctx, &mut app);
        assert!(drawn.contains("1 watch, 0 takes"), "{drawn}");
        assert!(drawn.contains("Move Watches…") && drawn.contains("Use the Default"));
        // The Watches folder is a category of its own in the sidebar, beside
        // every other; nothing is called Library, and the kept clocks are
        // under Microphone, not here.
        for name in ["General", "Microphone", "Watches Folder", "Calibres"] {
            assert!(drawn.contains(name), "{name} not in the sidebar: {drawn}");
        }
        assert!(
            !drawn.contains("Library") && !drawn.contains("Quartz Correction"),
            "{drawn}"
        );
        assert!(!drawn.contains("Sound Card Clock"), "{drawn}");
        // While another copy holds them, this one can only read them, and says so.
        assert!(drawn.contains("can only read them"), "{drawn}");
        drop(lib);
        app.read_library();
        assert!(!text(&ctx, &mut app).contains("can only read them"));
        // Opening Settings again reads it afresh, and this copy's own lock
        // is no other copy.
        app.open_settings(Tab::WatchesFolder);
        assert!(!text(&ctx, &mut app).contains("can only read them"));

        // Move it into another folder, and it is used there.
        let to = base.join("there").join("Timegrapher Data");
        app.library_move_ask = Some(to.clone());
        let asked = text(&ctx, &mut app);
        assert!(asked.contains("Move your watches to"), "{asked}");
        app.library_move_ask = None;
        app.move_library(root.clone(), to.clone());
        let t = Instant::now();
        while app.library_moving.is_some() && t.elapsed().as_secs() < 30 {
            app.poll_library_move(&ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(app.library_dir.as_deref(), Some(to.as_path()));
        assert!(!root.exists() && library::Library::is_library(&to));
        assert_eq!(
            app.library
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .watches
                .len(),
            1
        );
        assert_eq!(app.settings().library.as_deref(), Some(to.as_path()));
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
        again.apply_settings(app.settings());
        assert_eq!(again.library_root().as_deref(), Some(to.as_path()));

        // A folder with no library yet reads as none, and is left alone.
        app.use_library(Some(base.join("empty")));
        assert!(matches!(app.library, Ok(None)));
        assert!(!base.join("empty").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The Rate's hover names where an opened take's clock came from in
    /// core's words: its clock log, its USB lines or the correction kept
    /// for its input.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a test's clock fixture, not a take being opened"
    )]
    fn the_rate_hover_names_where_a_takes_clock_came_from() {
        use timegrapher_core::take::{clock_source_text, ClockFrom};
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let clock = timegrapher_core::clock::ClockFit::fixed(18.9, 600.0);
        let usb = "from 12 USB lines in the recording, 0.1 ppm apart";
        let kept = "stored for 'USB Mic', 2026-10-20T10:00:00Z";
        let log = std::path::Path::new("take/clock.log");
        for from in [ClockFrom::Log, ClockFrom::Usb, ClockFrom::Correction] {
            let source = clock_source_text(from, Some(usb), Some(kept), Some(log));
            app.put_take_clock(Some(clock.clone()), Some(source.clone()));
            let words = app.true_clock_words(0.0).unwrap();
            assert!(
                words.contains(&format!("18.9 ppm slow, {source},")),
                "{words}"
            );
        }
        // The USB lines set the rate over a log kept for its breaks: core's
        // words for both, and what the log measured.
        let mut rated = clock.clone();
        rated.rate_from_usb(18.9);
        let source = clock_source_text(ClockFrom::LogRatedByUsb, Some(usb), None, Some(log));
        app.put_take_clock(Some(rated.clone()), Some(source.clone()));
        let words = app.true_clock_words(0.0).unwrap();
        let note = rated.withheld.unwrap().note();
        assert!(
            words.contains(&format!("18.9 ppm slow, {source},")) && words.contains(&note[1..]),
            "{words}"
        );
    }

    /// Settings lists each kept clock; Forget removes that one from the
    /// file the command line reads, and the input listening loses its
    /// True Clock with it. Manage Calibres… opens Settings at Calibres.
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a test's clock fixture, not a take being opened"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn settings_forgets_a_kept_clock() {
        const MARKER: &str = "TIMEGRAPHER_FORGET_TEST_CHILD";
        if std::env::var_os(MARKER).is_none() {
            let root =
                std::env::temp_dir().join(format!("tg-app-forget-clock-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let mut store = clockstore::ClockStore::default();
            for (device, ppm) in [("USB Mic", 12.5), ("Built-in", -3.0)] {
                store.set(clockstore::DeviceClock::from_fit(
                    device,
                    &timegrapher_core::clock::ClockFit::fixed(ppm, 1500.0),
                    "2026-10-20T10:00:00Z".into(),
                    "measure",
                ));
            }
            store.save_to(&root.join("clocks.json")).unwrap();
            // Isolate the config environment in a child test process.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    &format!(
                        "{}::settings_forgets_a_kept_clock",
                        module_path!().split_once("::").unwrap().1
                    ),
                    "--nocapture",
                ])
                .env(MARKER, "1")
                .env("TIMEGRAPHER_CONFIG_DIR", &root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            let left = clockstore::ClockStore::load_from(&root.join("clocks.json")).unwrap();
            let names: Vec<&str> = left.devices.iter().map(|d| d.device.as_str()).collect();
            assert_eq!(names, ["Built-in"]);
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.clock = clockstore::ClockStore::load()
            .unwrap()
            .get("USB Mic")
            .cloned();
        assert!(app.clock.is_some());
        app.open_settings(Tab::Microphone);
        let listed = app.kept_clocks.as_ref().unwrap();
        assert_eq!(listed.devices.len(), 2);
        assert_eq!(
            settings_window::error_words(listed.get("USB Mic").unwrap()),
            "12.5 ppm slow, +1.08 s/d on every rate"
        );
        // The window draws both, each with its Forget.
        let ctx = egui::Context::default();
        theme::install(&ctx);
        // A window is measured on its first frame and drawn from the next.
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
        let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
        let text = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            text.iter().any(|t| t == "USB Mic") && text.iter().any(|t| t == "Built-in"),
            "{text:?}"
        );
        assert_eq!(
            text.iter().filter(|t| *t == "Forget").count(),
            2,
            "{text:?}"
        );
        // Under Microphone, as Quartz Correction, kept per input: the input
        // listening is marked, and a kept clock is under its own input.
        assert!(text.iter().any(|t| t == "Quartz Correction"), "{text:?}");
        app.clock_track = Some(ClockTrack {
            device: "USB Mic".into(),
            shown: "USB Mic".into(),
            tracker: timegrapher_core::clock::ClockTracker::new(48_000),
            next_s: CLOCK_MEASURE_S,
            fit: None,
        });
        let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
        let text: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        assert!(
            text.iter().any(|t| t == "USB Mic  (listening)")
                && text.iter().any(|t| t == "Built-in"),
            "{text:?}"
        );
        assert!(
            text.iter().any(|t| t.starts_with("3.0 ppm fast, ")),
            "{text:?}"
        );
        // An input listening with none kept says so, beside the others'.
        app.clock_track.as_mut().unwrap().device = "Line In".into();
        let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
        let said = out.shapes.iter().any(|s| match &s.shape {
            egui::Shape::Text(t) => t.galley.text().starts_with("None kept for \"Line In\""),
            _ => false,
        });
        assert!(said, "the input listening with no correction kept is named");
        app.clock_track = None;
        // Forget asks first, saying what is lost; nothing goes until it is
        // confirmed.
        app.forget_ask = Some("USB Mic".into());
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
        let out = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
        let asked =
            out.shapes.iter().any(|s| match &s.shape {
                egui::Shape::Text(t) => t.galley.text()
                    == "Forget the clock kept for \"USB Mic\", 12.5 ppm slow, measured 2026-10-20?",
                _ => false,
            });
        assert!(asked, "the question names the input, its error and its day");
        assert!(app.clock.is_some() && app.forget_ask.is_some());
        assert_eq!(app.kept_clocks.as_ref().unwrap().devices.len(), 2);
        app.forget_ask = None;
        app.forget_clock("USB Mic");
        assert!(
            app.clock.is_none(),
            "the input listening loses its True Clock"
        );
        assert_eq!(app.kept_clocks.as_ref().unwrap().devices.len(), 1);
        assert!(!app.message.as_ref().unwrap().1);
        // Manage Calibres… opens Settings at Calibres, and Settings opens
        // where it was left.
        app.settings_tab = None;
        app.cal.open_manager("");
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.settings_window(ctx));
        assert_eq!(app.settings_tab, Some(Tab::Calibres));
        assert!(!app.cal.manager_open);
        app.settings_tab = None;
        app.open_settings(app.settings_last);
        assert_eq!(app.settings_tab, Some(Tab::Calibres));
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a test's clock fixture, not a take being opened"
    )]
    fn invalid_clock_store_blocks_fresh_listening_and_keep_without_hardware() {
        const MARKER: &str = "TIMEGRAPHER_CLOCK_STORE_TEST_CHILD";
        if std::env::var_os(MARKER).is_none() {
            let root =
                std::env::temp_dir().join(format!("tg-app-invalid-clock-{}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let original = r#"{"schema":"timegrapher.clocks/1","devices":[{"device":"USB","ppm":-1000000,"ppm_sd":null,"span_s":0,"measured_utc":"legacy","source":"set"}]}"#;
            std::fs::write(root.join("clocks.json"), original).unwrap();
            // Isolate the config environment in a child test process, not parallel app tests.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    &format!(
                        "{}::invalid_clock_store_blocks_fresh_listening_and_keep_without_hardware",
                        module_path!().split_once("::").unwrap().1
                    ),
                    "--nocapture",
                ])
                .env(MARKER, "1")
                .env("TIMEGRAPHER_CONFIG_DIR", &root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                "child must execute the actual entrypoint test"
            );
            assert_eq!(
                std::fs::read_to_string(root.join("clocks.json")).unwrap(),
                original
            );
            std::fs::remove_dir_all(root).unwrap();
            return;
        }
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.reset_session(48_000, "retained context".into());
        let old = clockstore::DeviceClock::from_fit(
            "old",
            &timegrapher_core::clock::ClockFit::fixed(12.0, 60.0),
            "legacy".into(),
            "set",
        );
        app.clock = Some(old.clone());
        app.devices_listed = false;
        app.start_microphone();
        assert!(app.capture.is_none() && app.batch.is_none());
        assert!(app.devices.is_empty());
        assert!(
            !app.devices_listed,
            "invalid store must prevent device enumeration"
        );
        assert_eq!(app.clock, Some(old.clone()));
        assert_eq!(app.live.as_ref().unwrap().sample_rate(), 48_000);
        let (message, error) = app.message.as_ref().unwrap();
        assert!(
            *error && message.contains("clocks.json") && message.contains("ppm"),
            "{message}"
        );
        app.clock_track = Some(ClockTrack {
            device: "USB".into(),
            shown: "USB".into(),
            tracker: timegrapher_core::clock::ClockTracker::new(48_000),
            next_s: CLOCK_MEASURE_S,
            fit: Some(timegrapher_core::clock::ClockFit::fixed(20.0, 1200.0)),
        });
        app.keep_clock();
        assert_eq!(app.clock, Some(old));
        let (message, error) = app.message.as_ref().unwrap();
        assert!(
            *error && message.contains("couldn't be kept") && message.contains("ppm"),
            "{message}"
        );
    }

    /// In Quick Listen a calibre picked sets the beat rate and, where the
    /// table gives one, the lift angle, both then shown as text; Not Set
    /// puts back Auto and the default lift angle.
    #[test]
    fn a_calibre_picked_sets_the_beat_rate_and_not_set_puts_back_auto() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        assert_eq!(app.bph, None);
        app.set_calibre("ETA 2824-2".into());
        let bph = app.cal.bph(&app.calibre).expect("the 2824-2's beat rate");
        assert_eq!(app.bph, Some(bph));
        assert!(app.bph_set_by_calibre());
        if let Some(l) = app.cal.lift(&app.calibre) {
            assert_eq!(app.lift_deg, l);
            assert!(!app.lift_settable());
        }
        app.set_calibre(String::new());
        assert_eq!(app.bph, None);
        assert_eq!(app.lift_deg, DEFAULT_LIFT_DEG);
        assert!(!app.bph_set_by_calibre() && app.lift_settable());
    }

    #[test]
    fn the_input_clock_is_offered_after_twenty_minutes() {
        // A card 20 ppm slow: 48,000 frames arrive every 1.00002 s of
        // the computer's time, in blocks of a tenth of a second.
        let mut t = ClockTrack {
            device: "Test Mic".into(),
            shown: "Test Mic".into(),
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
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.clock_track = Some(t);
        assert!(app.clock_measured().is_some());
        assert!(app.clock_offer().is_some(), "offered with none kept");
        // One kept for the same name: offered again only when it is more
        // than a couple of ppm away, as when another card answers to it.
        let kept = |ppm: f64| clockstore::DeviceClock {
            device: "Test Mic".into(),
            ppm,
            ppm_sd: Some(0.1),
            span_s: 1200.0,
            measured_utc: "2026-10-12T20:00:00Z".into(),
            source: "measure".into(),
        };
        app.clock = Some(kept(21.0));
        assert!(app.clock_offer().is_none(), "the kept clock agrees");
        app.clock = Some(kept(-14.0));
        assert!(app.clock_offer().is_some(), "the kept clock is 34 ppm off");
        assert_eq!(day_of("2026-10-12T20:00:00Z"), "2026-10-12");
    }

    /// One Average Over, on the Listen card, for the readings and every
    /// pane: a pane's own time, saved by an older app, is read and left
    /// out.
    #[test]
    fn one_average_over_sets_every_pane() {
        let mut app = synthetic(40.0, 0.0);
        app.set_average(30.0);
        assert!(!app.trend.is_empty());
        assert!(app.trend.last().unwrap().t > 30.0);
        let mut s = app.settings();
        let mut json = serde_json::to_value(&s).unwrap();
        json["pane_average"] = serde_json::json!([["Charts", 5.0]]);
        s = serde_json::from_value(json).unwrap();
        let mut back = TimegrapherApp::with_devices(Vec::new(), None);
        back.apply_settings(s);
        assert_eq!(back.average_s, 30.0);
    }

    #[test]
    fn a_long_take_still_draws_over_a_short_strip() {
        // Opened whole, a take over 5 h 33 min keeps a reading only every
        // take/4000 s, more than 5 s apart: thinned here to the 6.7 s of
        // the 7 h 28 min Pepsi take, the strip works out its own.
        let mut app = synthetic(40.0, 20.0);
        let thin = |app: &mut TimegrapherApp| {
            let step = 26_880.0 / TREND_POINTS;
            let mut t = 0.0;
            let mut thin = Vec::new();
            while t <= 40.0 {
                if let Some(p) = app.trend.iter().find(|p| p.t >= t) {
                    thin.push(*p);
                }
                t += step;
            }
            app.trend = thin;
        };
        thin(&mut app);
        let (pts, spacing) = app.strip_trend(5.0, 40.0, app.average_s);
        assert_eq!(spacing, strip_step(35.0));
        assert!(pts.len() >= 60, "{} points", pts.len());
        assert!(pts.iter().filter(|p| p.rate.is_some()).count() >= 40);
        // A new lift angle works the strip's readings out again, so the
        // amplitude line follows it at once.
        let amplitude = |pts: &[TrendPoint]| pts.iter().find_map(|p| p.amplitude).unwrap();
        let before = amplitude(&pts);
        app.set_lift(app.lift_deg + 10.0);
        assert!(app.strip_points.is_none());
        thin(&mut app);
        let (pts, _) = app.strip_trend(5.0, 40.0, app.average_s);
        assert!(amplitude(&pts) != before, "amplitude {before} kept");
        // The kept run serves when it is fine enough.
        let mut app = synthetic(40.0, 20.0);
        let kept = app.trend.len();
        let (pts, _) = app.strip_trend(0.0, 40.0, app.average_s);
        assert!(app.strip_points.is_none());
        assert!(pts.len() <= kept);
    }

    /// Recent is the six calibres the app was last set to, by a menu pick,
    /// a watch opened or a take saved: the latest first, none twice, kept
    /// across restarts.
    #[test]
    fn recent_follows_what_the_app_was_set_to() {
        let names: Vec<String> = timegrapher_core::calibres::all()
            .iter()
            .take(8)
            .map(|c| c.calibre.clone())
            .collect();
        let (mut app, base) = save_failure_fixture("recent");
        // Menu picks: the latest first.
        for n in &names[..3] {
            app.set_calibre(n.clone());
        }
        assert_eq!(
            app.cal
                .recent
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [names[2].as_str(), names[1].as_str(), names[0].as_str()]
        );
        // Set again to a calibre already there: it moves to the front, once.
        app.set_calibre(names[0].clone());
        assert_eq!(
            app.cal
                .recent
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [names[0].as_str(), names[2].as_str(), names[1].as_str()]
        );
        app.set_calibre(names[0].clone());
        assert_eq!(
            app.cal
                .recent
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [names[0].as_str(), names[2].as_str(), names[1].as_str()]
        );
        // A watch opened sets its calibre.
        let mut w = library::Watch::new("Bench");
        w.calibre = Some(names[3].clone());
        app.apply_watch(&w);
        assert_eq!(app.cal.recent[0], names[3]);
        // A take saved puts its calibre first, though nothing else set it.
        app.cal.recent.retain(|r| r != names[3].as_str());
        app.save_into(&base.join("saved"));
        app.wait_for_save();
        assert_eq!(app.cal.recent[0], names[3]);
        // Six at most, the oldest going first; Not Set is never one.
        for n in &names[4..] {
            app.set_calibre(n.clone());
        }
        app.set_calibre(String::new());
        assert_eq!(app.cal.recent.len(), calibre_ui::RECENT);
        assert_eq!(
            app.cal
                .recent
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                names[7].as_str(),
                names[6].as_str(),
                names[5].as_str(),
                names[4].as_str(),
                names[3].as_str(),
                names[0].as_str()
            ]
        );
        // Kept across restarts, with the Settings category last left on.
        app.settings_last = Tab::Calibres;
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
        again.apply_settings(app.settings());
        assert_eq!(again.cal.recent, app.cal.recent);
        assert_eq!(again.settings_last, Tab::Calibres);
        app.new_session();
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_take_saved_under_a_former_calibre_name_picks_the_new_one() {
        let c = timegrapher_core::calibres::all()
            .iter()
            .find(|c| !c.former_names.is_empty())
            .expect("a renamed calibre in the table");
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.set_calibre(c.former_names[0].clone());
        assert_eq!(app.calibre, c.calibre);
        assert_eq!(app.cal.recent.first(), Some(&c.calibre));
    }
}

#[cfg(test)]
#[path = "app_layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "app_corpus_tests.rs"]
mod corpus_tests;

#[cfg(test)]
#[path = "app_steady_tests.rs"]
mod steady_tests;

#[cfg(test)]
#[path = "app_lift_tests.rs"]
mod lift_tests;

#[cfg(test)]
#[path = "app_strip_tests.rs"]
mod strip_tests;

#[cfg(test)]
#[path = "app_status_tests.rs"]
mod status_tests;

#[cfg(test)]
#[path = "app_revision_tests.rs"]
mod revision_tests;

#[cfg(test)]
#[path = "app_whole_tests.rs"]
mod whole_tests;

#[cfg(test)]
#[path = "app_charts_tests.rs"]
mod charts_tests;

#[cfg(test)]
#[path = "app_reserve_tests.rs"]
mod reserve_tests;

#[cfg(test)]
#[path = "app_capture_tests.rs"]
mod capture_tests;

#[cfg(test)]
#[path = "app_listen_tests.rs"]
mod listen_tests;
