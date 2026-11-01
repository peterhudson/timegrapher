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
        let v = timegrapher_core::chart_scale::axis_number(m.value, m.step_size);
        let y = t.position_from_point_y(m.value);
        painter.text(
            egui::pos2(frame.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{v}{unit}"),
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
#[path = "app_shared_tests.rs"]
mod tests;

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

#[cfg(test)]
#[path = "app_readouts_tests.rs"]
mod readouts_tests;

#[cfg(test)]
#[path = "app_save_tests.rs"]
mod save_tests;

#[cfg(test)]
#[path = "app_keep_tests.rs"]
mod keep_tests;

#[cfg(test)]
#[path = "app_positions_tests.rs"]
mod positions_tests;

#[cfg(test)]
#[path = "app_masks_tests.rs"]
mod masks_tests;

#[cfg(test)]
#[path = "app_open_tests.rs"]
mod open_tests;

#[cfg(test)]
#[path = "app_replay_tests.rs"]
mod replay_tests;

#[cfg(test)]
#[path = "app_cards_tests.rs"]
mod cards_tests;

#[cfg(test)]
#[path = "app_watch_line_tests.rs"]
mod watch_line_tests;

#[cfg(test)]
#[path = "app_settings_tests.rs"]
mod settings_tests;
