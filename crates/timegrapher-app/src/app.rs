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
use crate::settings::Settings;
use crate::settings_window::{self, Tab};
use crate::steady::{self, Steadiness};
use crate::strip::{self, Anchor, StripInput, StripView};
use crate::theme::{self, Icon};
use crate::watches;
use eframe::egui::{self, Color32, RichText, Vec2};
use egui_plot::{GridMark, Line, LineStyle, Plot, Points};
use egui_tiles::{Linear, LinearDir, SimplificationOptions, TileId, Tiles, Tree, UiResponse};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};
use timegrapher_core::beats::STANDARD_BPH;
use timegrapher_core::capture::{self, Block, Capture, Event, InputDevice, Level};
use timegrapher_core::clockstore;
use timegrapher_core::library;
use timegrapher_core::live::{LiveAnalyzer, LiveConfig, LiveReading, MAX_PROFILE_S};
use timegrapher_core::measure::Caveat;
use timegrapher_core::mixer::{self, GainState, InputGain};
use timegrapher_core::periodicity::Wheel;
use timegrapher_core::power_reserve;
use timegrapher_core::profile::TickProfile;
use timegrapher_core::steadiness::SeriesKind;

/// The A and B sides' sounds.
type Profiles = [Option<TickProfile>; 2];

/// How often the sound is kept, for the profile once stopped, seconds.
const SOUND_EVERY_S: f64 = 2.0;
/// What the reading cards asked for.
struct Readouts {
    /// Keep the input's clock as timed.
    keep_clock: bool,
    /// Where each card's controls go, at its foot.
    slots: Vec<(Slot, egui::Rect)>,
}

/// The controls the readings' cards carry, each in a strip at its card's
/// foot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
    /// The six positions.
    Position,
    /// Listen or Stop, Save, and Average Over.
    Listen,
    /// The lift angle.
    Lift,
    /// The beat rate and the calibre.
    BeatRate,
}

/// Seconds of listening before the input's clock, timed against the
/// computer's, is offered to keep: the twenty minutes `timegrapher clock
/// measure` takes by default. It is timed again each minute after.
const CLOCK_MEASURE_S: f64 = 1200.0;

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

/// The day of a UTC timestamp kept as text ("2026-10-12T…").
fn day_of(utc: &str) -> &str {
    utc.get(..10).unwrap_or(utc)
}

/// The Rate caveat for an input whose clock hasn't been measured.
const CLOCK_UNKNOWN: &str = "input clock not measured";
/// Plain marks on the level meter, for reference only.
const REFERENCE_DBFS: [f64; 2] = [-10.0, -6.0];
/// The width of the Settings window's sidebar.
const SETTINGS_SIDEBAR_W: f32 = 146.0;
use crate::session::{self, Action, Session};
use timegrapher_core::analysis_take;
use timegrapher_core::recorder::{self, Recorder, SavedCopy, SessionInfo};
use timegrapher_core::session::{Manifest, RecordingEntry};
use timegrapher_core::stream::{self, BeatLog, StreamConfig};

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

/// What a question before throwing away unsaved sound is about.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Confirm {
    NewSession,
    /// A new take of the watch picked, of the kind asked for.
    NewTake(NewTake),
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

/// The panes below the readings. Tick shape and the periodicity (FFT)
/// views will join these.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Pane {
    /// Layouts saved with the Microphone pane, gone since, read as
    /// another Watches pane and so replaced by the default.
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
    const ALL: [Pane; 18] = [
        Pane::Watch,
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
    const CORE: [Pane; 4] = [Pane::Watch, Pane::Strip, Pane::Sound, Pane::Scope];
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
    /// The panes that average the beats, each over Readings' Average Over
    /// or a time of its own.
    const AVERAGING: [Pane; 4] = [Pane::Strip, Pane::Sound, Pane::Charts, Pane::Distributions];

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

/// A pane's icon and the colour it is drawn in: one green for the charts,
/// the distributions and the Steadiness views so they read as a family,
/// and a colour of its own for each of the rest.
fn pane_icon(pane: Pane, pal: &theme::Palette) -> (theme::Icon, Color32) {
    match pane {
        Pane::Watch => (Icon::Watch, pal.sound),
        Pane::Strip => (Icon::Strip, pal.pink),
        Pane::Sound => (Icon::Profile, pal.tick),
        Pane::Scope => (Icon::Scope, pal.tock),
        Pane::SteadyReadings => (Icon::Steadiness, pal.unlock),
        Pane::SteadyPeriods => (Icon::Periods, pal.unlock),
        Pane::SteadyCycle => (Icon::Cycle, pal.unlock),
        Pane::SteadyAutocorrelation => (Icon::Autocorrelation, pal.unlock),
        Pane::SteadyAllan => (Icon::Allan, pal.unlock),
        Pane::SteadyCusum => (Icon::Cusum, pal.unlock),
        Pane::SteadyFft => (Icon::Fft, pal.unlock),
        Pane::SteadyStft => (Icon::Stft, pal.unlock),
        Pane::SteadyWavelet => (Icon::Wavelet, pal.unlock),
        Pane::Isochronism => (Icon::Isochronism, pal.unlock),
        Pane::PositionsTest => (Icon::Positions, pal.teal),
        Pane::PowerReserve => (Icon::Reserve, pal.teal),
        Pane::Charts => (Icon::Charts, pal.unlock),
        Pane::Distributions => (Icon::Distributions, pal.unlock),
    }
}

/// Save As: Ctrl+Shift+S, or Cmd+Shift+S on a Mac.
fn save_as_shortcut() -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::S,
    )
}

/// The Automatic Gain switch's hover, in Settings > Microphone.
const AGC_HINT: &str = "The microphone's own automatic gain, which raises its level between \
     ticks and lowers it on each one. Off, the level stays where Input Level puts it, as a \
     timegrapher needs. A stand turns it back on each time it is plugged in, so the app sets \
     it again each time it opens the input.";

const MASK_HINT: &str = "Leave stretches out of the readings, such as a bump of the stand: \
     with Mask on, drag along the strip over the beats to mask them, drag a masked \
     stretch's edge to move it, and click a masked stretch to bring it back. A right click \
     on one brings it back too, Mask on or off. Stretches that overlap join into one. A take \
     saved from the app keeps them; a recording opened from disk keeps them when it is \
     saved. Each point on the strip's line is the reading over the Average Over that ends \
     there, so just after a masked stretch the line is blank for about one Average Over, \
     while the sound after it builds up.";
const EXPORT_HINT: &str = "Save this pane as a picture: a PNG of it as it is on the screen, \
     or an SVG drawing whose lines and text stay sharp at any size. The file name's ending, \
     .png or .svg, decides which. The pane's name, and a chart's key, go at the top in place \
     of its controls.";
const SAVE_MASKS_HINT: &str = "Save the masked stretches to the recording's session file, so \
     they come back when it is opened again; nothing else in the file changes. A recording \
     with no session file gets one, which makes its folder a take: refused where the folder \
     holds other recordings, or a session file that doesn't list the recording.";
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
     from the default 52°. Type the calibre's lift angle, pick one, or pick the calibre on the \
     Beat Rate card.";
/// The takes a watch's row starts, each saved into the watch.
const NEW_TAKES: [(NewTake, &str, &str); 3] = [
    (
        NewTake::Listen,
        "Listen",
        "Listen to the watch in the position on the Current Position card, for as long as you like; \
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
     settings, to analyse again later. With a watch picked in the Watches pane, the take is \
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
    let watch = tabbed(Pane::Watch);
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
    let middle = tiles.insert_container(middle);
    // The Watches pane at the left when it is shown.
    let root = tiles.insert_container(Linear::new_binary(
        LinearDir::Horizontal,
        [watch, middle],
        WATCH_SHARE,
    ));
    let mut tree = Tree::new("panes", root, tiles);
    for p in Pane::ALL {
        if !matches!(p, Pane::Strip | Pane::Sound | Pane::Scope) {
            set_pane_visible(&mut tree.tiles, p, false);
        }
    }
    tree
}

/// How much of the window's width the Watches pane takes, at the left, when
/// it is shown.
const WATCH_SHARE: f32 = 0.34;

/// The heights of the strips of controls at the readings' cards' feet: a
/// row of controls, the Beat Rate card's two, the Position card's picture
/// of the position.
const CONTROL_SLOT_H: f32 = 28.0;
/// The size of a card's big figure.
const FIGURE_PT: f32 = 30.0;
const BEAT_RATE_SLOT_H: f32 = 72.0;
/// The Beat Rate card's controls narrower than this put the Calibre menu
/// under its name, and the card leaves this much more room for them.
const BEAT_RATE_NARROW_W: f32 = 130.0;
const BEAT_RATE_NARROW_EXTRA_H: f32 = 20.0;
/// The Current Position card's picture and label, then its buttons: one
/// row of six where each button has `POSITION_BUTTON_W`, else two rows of
/// three.
const POSITION_PIC_H: f32 = 72.0;
const POSITION_SEG_H: f32 = 26.0;
const POSITION_BUTTON_W: f32 = 30.0;
/// The Current Position card's width: its share of the window, between
/// these.
const POSITION_MIN_W: f32 = 124.0;
const POSITION_MAX_W: f32 = 240.0;
/// The room the Amplitude card's lift row takes: Lift, its field, °, the
/// presets menu and the default's mark. Narrower, Lift goes on a line of
/// its own above the rest, and the card is this much taller.
const LIFT_ROW_W: f32 = 130.0;
const LIFT_NARROW_EXTRA_H: f32 = 20.0;
/// The narrowest a card of figures can be and still show its caption and
/// "?" whole.
const FIGURE_MIN_W: f32 = 118.0;
/// The Current Position card narrower than this is captioned Position.
const POSITION_CAPTION_W: f32 = 160.0;
/// Dark text on the teal of the current position's button.
const POSITION_ON_INK: Color32 = Color32::from_rgb(0x0d, 0x26, 0x29);

/// The rows of the Current Position card's buttons across `inner`, the
/// card less its margins: six in one row where each has room, else three
/// in each of two.
fn position_rows(inner: f32) -> usize {
    if (inner - 4.0) / 6.0 >= POSITION_BUTTON_W {
        1
    } else {
        2
    }
}

/// Where the Current Position card puts its label: the name and codes
/// beside the picture, the codes alone beside it, or the codes under it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PositionLabel {
    Beside,
    Codes,
    Under,
}

/// The height of the Current Position card's buttons, in `rows` rows.
fn position_buttons_h(rows: usize) -> f32 {
    rows as f32 * POSITION_SEG_H + (rows as f32 - 1.0) * 4.0
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

/// The Current Position card's slot across `inner`: the picture, then its
/// buttons.
fn position_slot_h(inner: f32) -> f32 {
    POSITION_PIC_H + 6.0 + position_buttons_h(position_rows(inner))
}
const LISTEN_SLOT_H: f32 = 60.0;

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
fn keep_room(tiles: &mut Tiles<Pane>, need: impl Fn(Pane) -> Option<f32>) {
    let ids: Vec<TileId> = tiles.tile_ids().collect();
    for id in ids {
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
        // Each child's least: a tab bar's active pane may need more than
        // the usual height, under its tab bar.
        let mut least: Vec<f32> = shown
            .iter()
            .map(|&c| {
                if !vertical {
                    return MIN_PANE_W;
                }
                let more = match tiles.get(c) {
                    Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(t))) => t
                        .active
                        .and_then(|a| match tiles.get(a) {
                            Some(egui_tiles::Tile::Pane(p)) => need(*p),
                            _ => None,
                        })
                        .map(|h| h + TAB_BAR_H),
                    _ => None,
                };
                more.unwrap_or(0.0).max(MIN_PANE_H)
            })
            .collect();
        // A window too small for them all shares it out in proportion.
        let sum: f32 = least.iter().sum();
        if sum > room {
            for l in &mut least {
                *l *= room / sum;
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

/// Silence between two of the microphone's blocks longer than this, by
/// the system clock, is the computer having slept.
const SLEPT_S: f64 = 5.0;

/// The seconds the computer slept before block `b`, which came after a
/// block that arrived at `before`; `None` if it didn't.
fn slept(before: Option<std::time::SystemTime>, b: &Block, sample_rate: u32) -> Option<f64> {
    let block_s = b.samples.len() as f64 / sample_rate.max(1) as f64;
    let gap = b.at.duration_since(before?).ok()?.as_secs_f64() - block_s;
    (gap > SLEPT_S).then_some(gap)
}

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
/// run.
/// The strip's lengths, then Fit, which stands for the whole session.
const SPANS: [f64; 8] = [
    10.0,
    30.0,
    60.0,
    300.0,
    1800.0,
    3600.0,
    7200.0,
    f64::INFINITY,
];

fn span_label(v: f64) -> String {
    if v.is_infinite() {
        "Fit".into()
    } else {
        fields::duration(v)
    }
}
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
    const STEP_S: f64 = 2.0;

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
        let (Some(t0), Some(bph), Some(last)) = (first, self.bph, beats.last()) else {
            return;
        };
        loop {
            let a = t0 + self.next as f64 * Self::STEP_S;
            let b = a + Self::STEP_S;
            if b > last.time + 1e-9 {
                break;
            }
            self.next += 1;
            if self.breaks.iter().any(|&x| a < x && x < b)
                || self.masked.iter().any(|&(m0, m1)| a < m1 && b > m0)
            {
                continue;
            }
            let lo = beats.partition_point(|x| x.time < a);
            let hi = beats.partition_point(|x| x.time < b);
            if let Some(f) = timegrapher_core::timing::fit(&beats[lo..hi], bph) {
                self.values.push((a, f.rate_s_per_day));
            }
        }
    }
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

/// What the strip's own readings were worked out for: the window and the
/// spacing in steps, the averaging time and how much sound there was.
type StripKey = (i64, i64, u64, u64, u64);

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
    /// For a take kept as its analysis alone, its Tick Tock profiles, put
    /// here with the beats.
    kept: Option<std::sync::Arc<std::sync::Mutex<Vec<analysis_take::StretchProfiles>>>>,
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
    /// The Watches pane: the watch picked (its id), the list's search,
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
    /// A question about a take (Delete, Keep Analysis Only, Add to
    /// a Watch), and the take being kept as its analysis in the background.
    take_ask: Option<watches::Ask>,
    take_keep: Option<(PathBuf, AnalysisSave)>,
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
    /// The position and the keeping picked for the next Power Reserve Test.
    reserve_position: Position,
    reserve_keep_sound: bool,
    /// When the running test last read the beats, audio seconds.
    reserve_read_s: f64,
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
    /// Set Up the Microphone is open (the first launch).
    setup_sheet: bool,
    /// Quick Listen starts on the first frame (Listen When the App Opens).
    listen_pending: bool,
    /// Set Up the Microphone has tried listening once.
    setup_tried: bool,
    /// Why the computer isn't being kept awake although asked, set by the
    /// listening thread.
    pub(crate) awake_fault: Option<String>,
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
    /// The calibre picked on the Beat Rate card: a name in the table or one of the
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
    /// Panes that average over their own time rather than Readings'
    /// Average Over, and that time in seconds.
    pane_average: std::collections::BTreeMap<Pane, f64>,
    /// The readings over the session at each averaging time a pane uses
    /// other than Readings' (whose are `trend`), by milliseconds.
    other_trends: std::collections::BTreeMap<u64, Vec<TrendPoint>>,
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
    /// Where the last Save put the session, if it has been saved.
    /// The folder Save offers first.
    save_dir: String,
    /// Asking before something throws away unsaved sound.
    confirm: Option<Confirm>,
    /// Open Recording… or Open Folder… asking where the recording goes.
    open_ask: Option<OpenAsk>,
    /// The recording last opened, which the path box goes back to when
    /// another can't be opened.
    opened_path: String,
    /// The take picked in the Watches pane's list.
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
    /// The Periods pane's scores on a log scale.
    periods_log: bool,
    /// The FFT pane's rate as the beat timing's spectrum, in ms.
    fft_timing: bool,
    /// The STFT and Wavelet Map panes' colours on a log scale.
    maps_log: bool,
    /// The calibre's wheels marked on the period axes.
    show_wheels: bool,
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
    /// Watches pane groups folded down to their caption.
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
    anchor: Option<Anchor>,
    /// The settings a take on show is read again with, for a new beat
    /// rate, in place of what the take saved.
    reread_with: Option<WatchSetup>,
    /// The strip's width and length before an opened take was fitted to
    /// it, and the fitted view: Listen brings the first back, unless the
    /// strip was changed by hand since.
    live_strip: Option<(StripView, StripView)>,
    trend: Vec<TrendPoint>,
    /// The last [`Self::whole_take`] and what it was read from.
    whole_take: std::cell::RefCell<Option<(WholeKey, Option<WholeTake>)>>,
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
    sound_history: Vec<(f64, Profiles)>,
    sound_at: Option<(f64, Profiles)>,
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
    sound_job: Option<(f64, std::sync::mpsc::Receiver<Profiles>)>,
    /// Offer every input the system lists, not just one per microphone.
    all_inputs: bool,
    /// When the microphone was opened and when it last sent audio.
    opened_at: Instant,
    audio_at: Option<Instant>,
    /// When the microphone's last block arrived, by the system clock, to
    /// tell when the computer slept between two blocks.
    block_at: Option<std::time::SystemTime>,
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
            self.setup_sheet = !self.mic_set_up;
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
            steady_series: [true; 3],
            steady_shown: self.steady_series.iter().map(|(v, s)| (*v, *s)).collect(),
            periods_log: self.periods_log,
            fft_timing: self.fft_timing,
            maps_log: self.maps_log,
            show_wheels: self.show_wheels,
            positions_settle_s: self.guided_settle_s,
            positions_measure_s: self.guided_measure_s,
            scope: self.scope_view,
            panes: Some(self.panes.clone()),
            layout: LAYOUT,
            // Drop legacy entries only after their user-catalogue save succeeds.
            custom_calibres: self.cal.pending_legacy().to_vec(),
            recent_calibres: self.cal.recent.clone(),
            short_added: self.cal.short_added.clone(),
            short_removed: self.cal.short_removed.clone(),
            pane_average: self.pane_average.iter().map(|(&p, &s)| (p, s)).collect(),
            check_updates: self.check_at_start,
            keep_awake: self.keep_awake,
            listen_at_open: self.listen_at_open,
            mic_set_up: self.mic_set_up,
            show_readings: self.show_readings,
            library: self.library_dir.clone(),
            settings_section: self.settings_last.key().into(),
            agc_on: self.agc_on.clone(),
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
            chart_series,
            dist_series,
            hist_cumulative,
            hist_readings,
            sound,
            folded,
            theme,
            steady_series,
            steady_shown,
            periods_log,
            fft_timing,
            maps_log,
            show_wheels,
            positions_settle_s,
            positions_measure_s,
            scope,
            panes,
            layout,
            custom_calibres,
            recent_calibres,
            short_added,
            short_removed,
            pane_average,
            check_updates,
            keep_awake,
            listen_at_open,
            mic_set_up,
            show_readings,
            library,
            settings_section,
            agc_on,
        } = s;
        self.settings_last = Tab::from_key(&settings_section);
        self.listen_at_open = listen_at_open;
        self.mic_set_up = mic_set_up;
        self.agc_on = agc_on;
        // A layout from before the panes moved starts afresh.
        let panes = panes.filter(|_| layout >= LAYOUT);
        self.library_dir = library;
        self.check_at_start = check_updates;
        self.keep_awake = keep_awake;
        self.show_readings = show_readings;
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
        let some = |s: [bool; 3]| if s.contains(&true) { s } else { [true; 3] };
        self.chart_series = some(chart_series);
        self.dist_series = some(dist_series);
        self.hist_cumulative = hist_cumulative;
        self.hist_readings = hist_readings;
        self.sound_opts = sound;
        self.folded = folded.into_iter().collect();
        self.theme = theme;
        self.periods_log = periods_log;
        self.fft_timing = fft_timing;
        self.maps_log = maps_log;
        self.show_wheels = show_wheels;
        self.guided_settle_s =
            positions_settle_s.clamp(*guided::SETTLE_RANGE.start(), *guided::SETTLE_RANGE.end());
        self.session.set_settle_s(self.guided_settle_s);
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
            if p.tiles.find_pane(&Pane::PowerReserve).is_none() {
                add_missing(&mut p, Pane::PowerReserve);
                set_pane_visible(&mut p.tiles, Pane::PowerReserve, false);
            }
            p
        }) {
            Some(p) if layout_is_whole(&p) => p,
            _ => default_layout(self.strip.horizontal),
        };
    }

    /// The app with a given device list. Tests pass an empty one: listing
    /// devices from several test threads at once crashes on Windows.
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
            reserve_position: Position::DialUp,
            reserve_keep_sound: true,
            reserve_read_s: 0.0,
            guided_unused: Vec::new(),
            guided_measured: Vec::new(),
            guided_earlier: None,
            guided_settle_s: guided::SETTLE_S,
            guided_measure_s: guided::MEASURE_S,
            check_at_start: true,
            keep_awake: true,
            listen_at_open: true,
            mic_set_up: false,
            setup_sheet: false,
            listen_pending: false,
            setup_tried: false,
            awake_fault: None,
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
            watch_open: None,
            unsorted_open: false,
            list_for: None,
            quick_defaults: false,
            quick_listen: false,
            watch_search: String::new(),
            watch_sort: watches::Sort::default(),
            watch_form: None,
            keep_after_form: false,
            test_keep: None,
            results_rects: [None; 2],
            positions_take: false,
            take_ask: None,
            take_keep: None,
            thumbs: Default::default(),
            pane_average: Default::default(),
            other_trends: Default::default(),
            strip_points: None,
            masked: Vec::new(),
            opened_masks: None,
            masking: false,
            confirm: None,
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
            chart_series: [true; 3],
            dist_series: [true; 3],
            charts_drawn: [true; 3],
            pane_need: std::collections::HashMap::new(),
            pane_top: 0.0,
            hist_cumulative: false,
            steady_series: Default::default(),
            periods_log: false,
            fft_timing: false,
            maps_log: false,
            show_wheels: true,
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
            anchor: None,
            reread_with: None,
            live_strip: None,
            trend: Vec::new(),
            whole_take: Default::default(),
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
            block_at: None,
            polled_at: Instant::now(),
        };
        if let Some(f) = file {
            app.start_batch(&f);
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
        self.strip_points = None;
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

    pub(crate) fn running(&self) -> bool {
        self.capture.is_some()
    }

    fn reset_session(&mut self, sample_rate: u32, label: String) {
        // Never throw away the sound a save is still copying.
        self.wait_for_save();
        self.session.discard_recording();
        self.session.clear();
        self.clock = None;
        let mut live = LiveAnalyzer::new(sample_rate, self.live_config());
        live.set_masked(&self.masked);
        self.live = Some(live);
        self.source_label = label;
        self.anchor = None;
        self.clear_trends();
        self.strip_end = None;
        self.level = Level::default();
        self.clipped_at = None;
        self.message = None;
        self.sound_history.clear();
        self.no_sound = false;
        self.analysis_kept_to = None;
        self.guided = None;
        self.reserve = None;
        self.guided_unused.clear();
        self.guided_measured.clear();
        self.sound_at = None;
        self.sound_job = None;
        self.steady = None;
        self.steady_job = None;
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

    /// Listen, the masks on the recording on show saved or let go: and
    /// begin the Positions Test when that is what asked. They stay on show
    /// until listening starts, so a microphone that fails to open leaves
    /// them to save.
    fn listen_after_ask(&mut self) {
        self.masks_let_go = true;
        match self.guided_after_ask.take() {
            Some(half) => self.start_guided(half),
            None => self.start_microphone(),
        }
        self.masks_let_go = false;
    }

    /// Listen to the microphone: carry on the paused session if there is
    /// one, else start a new one.
    fn start_microphone(&mut self) {
        let let_go = std::mem::take(&mut self.masks_let_go);
        if self.refuse_faulted_recording() || self.analysis_save_busy() {
            return;
        }
        // A save being written carries on through a resume, but not a new
        // session, which throws away the sound it copies.
        if !self.can_resume() && self.saving_busy() {
            return;
        }
        // Listening replaces the recording on show: changed masks on it
        // are asked about first.
        if !let_go && self.masks_unsaved() {
            self.confirm = Some(Confirm::Microphone);
            return;
        }
        if self.can_resume() {
            self.ensure_devices();
            return self.resume_microphone();
        }
        // A take's fitted view is for the take: listening goes back to the
        // strip as it was.
        self.strip = self.live_view();
        self.live_strip = None;
        let clocks = match clockstore::ClockStore::load() {
            Ok(s) => s,
            Err(e) => return self.error(format!("Can't load the input's clock correction: {e}")),
        };
        self.ensure_devices();
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
                // A new take: nothing of it masked yet.
                self.masked.clear();
                self.opened_masks = None;
                self.masking = false;
                self.reset_session(
                    c.sample_rate,
                    format!("{label} at {} Hz, {}-bit", c.sample_rate, c.bits),
                );
                self.opened_at = Instant::now();
                self.audio_at = None;
                self.block_at = None;
                self.session.listen();
                self.clock = clocks.get(&c.label).cloned();
                self.clock_track = Some(ClockTrack {
                    device: c.label.clone(),
                    shown: label.clone(),
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
                self.apply_agc();
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

    /// Stop: pause, and with a watch picked save the take into it (or the
    /// sound since into the take saved before). A Quick Listen take isn't
    /// kept until Keep in a Watch… or Save….
    fn stop_listening(&mut self) {
        // Stop during a Power Reserve Test ends the test, which keeps its take.
        if self.reserve_running() {
            if let Some(t) = self.reserve.as_mut() {
                t.stop();
            }
            return self.end_reserve();
        }
        self.pause();
        // A Positions Test with no watch picked: asked where it goes.
        if self.positions_take
            && self.open_watch().is_none()
            && self.session.saved_to().is_none()
            && self.session.allows(Action::Save)
        {
            self.test_keep = Some(TestKeep::Positions);
            return;
        }
        // Never a folder dialog at a Stop: only where Save… needs none.
        // A Quick Listen take kept already takes the sound since too.
        let into_watch = if self.quick_listen {
            self.session.saved_to().is_some()
        } else {
            self.open_watch().is_some()
        };
        let quiet = self.session.saved_to().is_some() || self.watch_takes_dir().is_some();
        if into_watch && quiet && self.session.allows(Action::Save) {
            self.save_again();
        }
    }

    /// Carry on listening after a pause, in the same session.
    fn resume_microphone(&mut self) {
        if self.refuse_faulted_recording() || self.analysis_save_busy() {
            return;
        }
        let rate = self.live.as_ref().map(|l| l.sample_rate());
        match capture::open_input(self.device.as_deref(), rate) {
            Ok(c) => {
                let shown = self.device_label(&c);
                if let Some(why) = self.resume_refusal(&c.label, &shown, c.sample_rate) {
                    return self.error(why);
                }
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
                self.block_at = None;
                self.strip_end = None;
                self.message = None;
                applog::info(&format!("microphone resumed at {} Hz", c.sample_rate));
                self.capture = Some(c);
                self.apply_agc();
            }
            Err(e) => self.error(format!(
                "Can't open the input. {}",
                capture::explain_error(&e)
            )),
        }
    }

    /// Why the paused session can't carry on with the input `label` (shown
    /// in the microphone menu as `shown`) opened at `sample_rate`: another
    /// sample rate, or another input, whose clock correction and clock
    /// measurement aren't this session's.
    fn resume_refusal(&self, label: &str, shown: &str, sample_rate: u32) -> Option<String> {
        let rate = self.live.as_ref().map(|l| l.sample_rate());
        if Some(sample_rate) != rate {
            return Some(format!(
                "The microphone opened at {sample_rate} Hz this time, not the {} Hz this session \
                 was listening at, so it can't carry on. Press New Session to start again.",
                rate.unwrap_or_default()
            ));
        }
        let track = self.clock_track.as_ref()?;
        let first = &track.shown;
        (track.device != label).then(|| {
            format!(
                "This session was listening to {first}, and its sound is timed by that input's \
                 clock, so it can't carry on with {shown}. Choose {first} to carry on, or press \
                 New Session to start again with {shown}."
            )
        })
    }

    /// Clear everything for the next watch or position. Listening to the
    /// microphone carries on in the new session.
    fn new_session(&mut self) {
        if self.analysis_save_busy() || self.saving_busy() {
            return;
        }
        let listening = self.running() && self.session.mic();
        self.message = None;
        self.stop();
        // A failed close leaves files behind: say so, and don't start
        // listening again over it.
        let close_error = self
            .session
            .recording_fault()
            .and_then(|_| self.message.clone());
        self.session.discard_recording();
        self.session.clear();
        self.live = None;
        self.masked.clear();
        self.opened_masks = None;
        self.masking = false;
        self.clear_trends();
        self.sound_history.clear();
        self.no_sound = false;
        self.analysis_kept_to = None;
        self.guided = None;
        self.reserve = None;
        self.positions_take = false;
        self.test_keep = None;
        self.guided_unused.clear();
        self.guided_measured.clear();
        self.source_label.clear();
        // The recording opened goes too: what follows is a new take.
        if self.input == Input::File {
            self.input = Input::Microphone;
            self.opened_path.clear();
            if let Some(s) = self.mic_setup.take() {
                self.set_watch_setup(s);
            }
        }
        if listening && close_error.is_none() {
            self.start_microphone();
        } else if !self.quick_listen {
            // The next take is of the watch picked.
            if let Some(w) = self.open_watch().map(|e| e.watch.clone()) {
                self.apply_watch(&w);
            }
        }
        if close_error.is_some() {
            self.message = close_error;
        }
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
            masked: self.masked.clone(),
        }
    }

    fn metadata_unsaved(&self) -> bool {
        self.session.saved_to().is_some() && self.session.metadata_changed(&self.save_metadata())
    }

    /// Keep the existing sound threshold; metadata edits have no duration.
    fn needs_save_confirmation(&self) -> bool {
        self.session.recording_fault().is_some()
            || !self.quick_unsaved() && (self.unsaved_s() > 5.0 || self.metadata_unsaved())
            || self.masks_unsaved()
    }

    /// Unsaved sound from Quick Listen goes without a question: listening
    /// again, opening a recording, a take of a watch or quitting lets it
    /// go. Save keeps it.
    fn quick_unsaved(&self) -> bool {
        self.quick_listen && self.session.mic() && self.session.saved_to().is_none()
    }

    /// A recording opened from disk whose masked stretches have changed
    /// since it was opened or last saved to.
    fn masks_unsaved(&self) -> bool {
        !self.session.mic()
            && self
                .opened_masks
                .as_ref()
                .is_some_and(|m| *m != self.masked)
    }

    /// Save the masked stretches of the recording opened from disk to its
    /// session file, so they come back when it is opened again. A take's
    /// session file keeps everything else it says, comments too, and only
    /// its `masked` changes on the entries for what was opened. A lone file,
    /// or a folder with no session file, gets a new one, as a take
    /// recorded here with the same settings would have, which makes its
    /// folder a take; refused where the folder holds other recordings.
    fn save_masks(&mut self) {
        let path = PathBuf::from(&self.opened_path);
        let masked: Vec<[f64; 2]> = self.masked.iter().map(|&(a, b)| [a, b]).collect();
        let saved = match read_manifest(&path) {
            Ok(Some(_)) => save_masks_to_take(&path, &masked),
            Ok(None) => self.new_take_file(&path, &masked),
            Err(e) => Err(e),
        };
        match saved {
            Ok(file) => {
                self.opened_masks = Some(self.masked.clone());
                self.info_msg(format!("Saved the masked stretches to {}.", file.display()));
            }
            Err(e) => self.error(format!("Couldn't save the masked stretches: {e}")),
        }
    }

    /// A new session file for `path`, a lone file or a folder with none,
    /// listing it with `masked`: what saving a take recorded here writes.
    fn new_take_file(&self, path: &Path, masked: &[[f64; 2]]) -> Result<PathBuf, String> {
        // The whole folder, as the recorder lists a take's sound: a lone
        // file is the whole of its folder's take (see `lists`).
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .ok_or_else(|| format!("{} has no folder", path.display()))?
                .to_path_buf()
        };
        let file = ".".to_string();
        let target = dir.join(MANIFEST);
        if target.exists() {
            return Err(format!(
                "{} doesn't list {}, so it is left as it is.",
                target.display(),
                capture_label(path)
            ));
        }
        if !path.is_dir() {
            let others = recordings_in(&dir)?
                .into_iter()
                .filter(|f| f.file_name() != path.file_name())
                .count();
            if others > 0 {
                return Err(format!(
                    "saving them makes {} a take of {} alone, but it holds {others} other {}. \
                     Move the recording into a folder of its own first.",
                    dir.display(),
                    capture_label(path),
                    if others == 1 {
                        "recording"
                    } else {
                        "recordings"
                    }
                ));
            }
        }
        let mut m = self.manifest_header();
        m.recordings = vec![RecordingEntry {
            file,
            position: Some(self.position.code().into()),
            masked: masked.to_vec(),
            ..Default::default()
        }];
        let text = toml::to_string_pretty(&m).map_err(|e| e.to_string())?;
        write_replacing(&target, &text)?;
        Ok(target)
    }

    /// Save Again: the sound so far into the take saved before, with no
    /// question. The first time, Save… saves into the watch picked in the
    /// Watches pane, else asks where.
    fn save_again(&mut self) {
        if self.refuse_faulted_recording() || self.saving_busy() {
            return;
        }
        match self.session.saved_to().and_then(|p| p.parent()) {
            Some(parent) => {
                let parent = parent.to_path_buf();
                self.save_into(&parent);
            }
            None => match self.watch_takes_dir() {
                Some(takes) => self.save_into_watch(&takes),
                None => self.save_recording(),
            },
        }
    }

    /// Save the session as a new take in a watch's `takes`, named by when
    /// it started (in this computer's time) and its one position, if it
    /// used only one.
    fn save_into_watch(&mut self, takes: &Path) {
        let positions: Vec<timegrapher_core::session::Position> = self
            .take_manifest()
            .recordings
            .iter()
            .filter_map(|r| {
                r.position
                    .as_deref()
                    .and_then(timegrapher_core::session::Position::parse)
            })
            .collect();
        let Some(r) = self.session.recorder.as_mut() else {
            return;
        };
        let started = watches::local_minute(r.started_utc())
            .unwrap_or_else(|| r.started_utc().replace(':', "."));
        let dir = library::unused_dir(takes, &library::take_name(&started, &positions));
        if let Some(name) = dir.file_name() {
            r.name_first_copy(name.to_owned());
        }
        self.save_into(takes);
        if let Some(s) = &mut self.saving {
            s.rescan = true;
        }
    }

    /// Save a copy of the session's sound so far, with its clock log and
    /// settings, in a folder the user picks. Listening carries on.
    fn save_recording(&mut self) {
        if self.refuse_faulted_recording() || self.saving_busy() {
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

    /// Save the session's sound so far into a take in `parent`. The copy
    /// is written on a thread of its own ([`Saving`]): listening carries
    /// on, and the window with it.
    fn save_into(&mut self, parent: &Path) {
        if self.refuse_faulted_recording() || self.saving_busy() {
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
            let kept = self.results_files()?;
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
        files.extend(kept.iter().map(|(n, b)| (*n, b.as_slice())));
        // With no new full-wind result (including a 24-hour half), preserve
        // the earlier result file for comparison, as Save Again always has.
        let pending = match r.begin_save(parent, &self.calibre, result.as_ref(), &files) {
            Ok(p) => p,
            Err(e) => return self.error(format!("Saving the recording failed: {e}")),
        };
        // A saved take's calibre is among the recent ones.
        let c = self.calibre.clone();
        self.cal.remember(&c);
        let dest = pending.dest().to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("saving".into())
            .spawn(move || pending.write());
        match spawned {
            Ok(thread) => {
                self.saving = Some(Saving {
                    thread,
                    dest,
                    parent: parent.to_path_buf(),
                    secs,
                    metadata,
                    rescan: false,
                })
            }
            Err(e) => self.error(format!("Saving the recording failed: {e}")),
        }
    }

    /// The take's results files, written beside its session.toml by both
    /// Save… and Save Analysis Only…: the positions test's and the power
    /// reserve test's, when there are.
    fn results_files(&self) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
        let about = self.guided_about();
        let mut files = Vec::new();
        if let Some(k) = self.guided.as_ref().and_then(|t| t.kept(&about)) {
            let text = serde_json::to_string_pretty(&k)
                .map_err(|e| format!("Couldn't prepare {}: {e}", guided::RESULTS_FILE))?;
            files.push((guided::RESULTS_FILE, (text + "\n").into_bytes()));
        }
        if let Some(r) = self.reserve.as_ref().and_then(|t| t.results.as_ref()) {
            files.push((power_reserve::FILE, r.to_json().into_bytes()));
        }
        Ok(files)
    }

    /// While a save is being written, say so and refuse what would start
    /// another or throw the sound it is copying away.
    fn saving_busy(&mut self) -> bool {
        if self.saving.is_none() {
            return false;
        }
        self.info_msg(
            "Saving: the sound so far is being copied into its take, so wait until it is done."
                .into(),
        );
        true
    }

    /// The save being written, once it is done.
    fn poll_saving(&mut self) {
        if self.saving.as_ref().is_some_and(|s| s.thread.is_finished()) {
            self.wait_for_save();
        }
    }

    /// Wait for the save being written, if there is one, and say how it
    /// went.
    fn wait_for_save(&mut self) {
        let Some(s) = self.saving.take() else {
            return;
        };
        match s.thread.join() {
            Ok(Ok(copy)) => {
                if let Some(r) = self.session.recorder.as_mut() {
                    r.note_saved(&copy);
                }
                self.info_msg(format!(
                    "Saved {} of sound in {}",
                    strip::fmt_time(s.secs),
                    copy.dir.display()
                ));
                self.save_dir = s.parent.display().to_string();
                self.session.mark_saved(s.secs, copy.dir, s.metadata);
                if s.rescan {
                    if let Ok(Some(l)) = &mut self.library {
                        l.scan();
                    }
                }
            }
            Ok(Err(e)) => self.error(format!("Saving the recording failed: {e}")),
            Err(_) => self.error("Saving the recording failed: it stopped part way".into()),
        }
    }

    /// While the session is being kept as its analysis alone, say so and
    /// refuse what would throw its sound away or add to it.
    fn analysis_save_busy(&mut self) -> bool {
        if self.analysis_save.is_none() {
            return false;
        }
        self.info_msg(
            "Keeping the analysis of this session: it is reading the session's sound and \
             writing its analysis, so wait until it is done, or press Cancel beside its \
             progress."
                .into(),
        );
        true
    }

    /// Whether the session can be kept as its analysis alone now: a
    /// microphone session with sound, not listening.
    fn can_save_analysis(&self) -> bool {
        self.session.allows(Action::Save)
            && !self.running()
            && self.analysis_save.is_none()
            && self.session.recorder.is_some()
    }

    /// Keep the session as its analysis alone, in a new folder inside one
    /// the user picks.
    fn save_analysis_only(&mut self) {
        if self.refuse_faulted_recording() || !self.can_save_analysis() {
            return;
        }
        let mut d = rfd::FileDialog::new()
            .set_title("Choose Where to Keep the Analysis (the Take Gets Its Own Folder Inside)");
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
        self.start_analysis_save(&parent);
    }

    /// Read the session's sound again for its analysis, in the background,
    /// then write it as a take with no sound inside `parent`.
    fn start_analysis_save(&mut self, parent: &Path) {
        let mut manifest = self.take_manifest();
        manifest.audio = Some(false);
        let prepared = (|| -> Result<_, String> {
            let result = self
                .reading()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| format!("Couldn't prepare the saved reading: {e}"))?;
            let text = toml::to_string_pretty(&manifest)
                .map_err(|e| format!("Couldn't prepare {MANIFEST}: {e}"))?;
            let kept = self.results_files()?;
            Ok((result, text, kept))
        })();
        let (result, text, kept) = match prepared {
            Ok(p) => p,
            Err(e) => return self.error(e),
        };
        self.note_settings();
        let Some(r) = self.session.recorder.as_mut() else {
            return;
        };
        let total_s = r.duration_s();
        let mut required = vec![(MANIFEST, text.as_bytes())];
        required.extend(kept.iter().map(|(n, b)| (*n, b.as_slice())));
        let (name, companions) =
            match r.analysis_copy_files(&self.calibre, result.as_ref(), &required) {
                Ok(c) => c,
                Err(e) => return self.error(format!("Keeping the analysis failed: {e}")),
            };
        let files = r.segment_paths();
        let dest = parent.join(name);
        let mut cfg = StreamConfig::default();
        cfg.analysis.bph = self.bph;
        cfg.analysis.amplitude.lift_deg = self.lift_deg;
        cfg.analysis.lift_known = self.lift_known();
        let shapes = timegrapher_core::analysis_take::shape_stretches(&manifest, total_s);
        let software = report::software();
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (p2, c2) = (progress.clone(), cancel.clone());
        std::thread::spawn(move || {
            let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let stored = timegrapher_core::analysis_take::measure(
                    &paths,
                    &cfg,
                    &shapes,
                    software,
                    |s| {
                        p2.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
                        if c2.load(std::sync::atomic::Ordering::Relaxed) {
                            std::panic::resume_unwind(Box::new(Cancelled));
                        }
                    },
                )
                .map_err(|e| e.to_string())?;
                let cancelled = || c2.load(std::sync::atomic::Ordering::Relaxed);
                if cancelled() {
                    std::panic::resume_unwind(Box::new(Cancelled));
                }
                let dir = timegrapher_core::analysis_take::save(&dest, &companions, &stored)
                    .map_err(|e| e.to_string())?;
                // Cancelled while it was being written: the folder is this
                // job's own (the save refuses one already there), so it goes.
                if cancelled() {
                    let _ = std::fs::remove_dir_all(&dir);
                    std::panic::resume_unwind(Box::new(Cancelled));
                }
                Ok(dir)
            }));
            match r {
                Ok(r) => {
                    // No one left to tell (cancelled after its last check):
                    // the folder it wrote goes too.
                    if let Err(std::sync::mpsc::SendError(Ok(dir))) = tx.send(r) {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                }
                Err(e) if e.is::<Cancelled>() => {}
                Err(e) => std::panic::resume_unwind(e),
            }
        });
        self.save_dir = parent.display().to_string();
        self.analysis_save = Some(AnalysisSave {
            rx,
            progress,
            cancel,
            total_s,
        });
    }

    /// The analysis being kept, once it is written.
    fn poll_analysis_save(&mut self) {
        let Some(job) = &self.analysis_save else {
            return;
        };
        let done = match job.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("it stopped part way".into()),
        };
        let Some(job) = self.analysis_save.take() else {
            return;
        };
        match done {
            Ok(dir) => {
                let size = folder_size(&dir);
                self.info_msg(format!(
                    "Kept the analysis of {} of sound in {}, without the sound ({}).",
                    strip::fmt_time(job.total_s),
                    dir.display(),
                    fmt_bytes(size)
                ));
                self.analysis_kept_to = Some((dir, job.total_s));
            }
            Err(e) => self.error(format!("Keeping the analysis failed: {e}")),
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
                "at_s": self.latest_s(),
                // Stretches masked out on the strip, in the session's
                // seconds, so an analysis of the excerpt can leave them out too.
                "masked": self.masked,
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
        // The five minutes up to the strip's newest edge, when the strip
        // has been moved back; else the start.
        let end = match self.strip_end {
            Some(_) if self.live.is_some() => self.strip_end_s(),
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
        let mut m = self.manifest_header();
        // The stretches masked out, in the saved sound's own time. Every
        // entry lists them all, so each comes back when the take is
        // opened, even one in a settling gap; `timegrapher session`
        // counts only the part inside an entry's stretch.
        let masked: Vec<[f64; 2]> = self
            .masked
            .iter()
            .filter_map(|&(a, b)| Some([self.session.audio_of(a)?, self.session.audio_of(b)?]))
            .filter(|m| m[1] > m[0])
            .collect();
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
                masked: masked.clone(),
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
                let (start_s, end_s) = (self.session.audio_of(from)?, self.session.audio_of(to)?);
                Some(RecordingEntry {
                    file: ".".into(),
                    position: Some(pos.code().to_string()),
                    wind_h,
                    clock: Some("clock.csv".into()),
                    start_s: Some(start_s),
                    end_s: Some(end_s),
                    masked: masked.clone(),
                    ..Default::default()
                })
            })
            .collect();
        if !test.is_empty() {
            m.recordings = test;
        }
        m
    }

    /// What a take's session file says about the watch: its name, calibre
    /// (with the wheels of a custom one), beat rate and lift angle, and
    /// whether that lift was chosen or assumed.
    fn manifest_header(&self) -> Manifest {
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
        m
    }

    /// Take what a session file says about a recording: the watch, its
    /// calibre (a custom one is added to the custom calibres from its wheels),
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
        // What the take masked out; its seconds are the strip's.
        let masked: Vec<[f64; 2]> = entries.iter().flat_map(|e| e.masked.clone()).collect();
        self.masked = timegrapher_core::session::masked_spans(&masked);
        self.opened_masks = Some(self.masked.clone());
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

    /// Open a recording, analysed whole. With `ask`, unsaved sound from a
    /// microphone session is not thrown away without asking first.
    fn open_recording(&mut self, path: &Path, ask: bool) {
        if self.analysis_save_busy() || self.saving_busy() {
            return;
        }
        if ask && (self.session.mic() || self.masks_unsaved()) && self.needs_save_confirmation() {
            self.confirm = Some(Confirm::Open {
                path: path.to_path_buf(),
            });
        } else {
            self.start_batch(path);
        }
    }

    /// Play a recording through as if live, at its own speed. The app
    /// offers no replay; tests use it to feed the live path from a file
    /// without opening an input.
    #[cfg(test)]
    fn start_replay(&mut self, path: &Path) {
        self.start_replay_at(path, 1.0);
    }

    /// Play a recording through as if live, `speed` times its own. A
    /// folder plays its files one after the other in the order it is
    /// analysed in, with the watch from its session file when it has one.
    #[cfg(test)]
    fn start_replay_at(&mut self, path: &Path, speed: f32) {
        // Refused before anything on show is stopped or its masks cleared.
        if let Some(why) = no_analysis_to_read(path) {
            return self.open_failed(why);
        }
        if let Ok(inner) = take_folder(path) {
            if analysis_take::is_analysis_only(&inner) {
                return self.open_failed(format!(
                    "{} has had its sound removed, so it can't be played again. Open shows \
                     its readings.",
                    capture_label(&inner)
                ));
            }
            if let Some(why) = no_analysis_to_read(&inner) {
                return self.open_failed(why);
            }
        }
        self.stop();
        self.masked.clear();
        self.masking = false;
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
                c.set_speed(speed);
                self.capture = Some(c);
                if let Some(n) = self.open_note.take() {
                    self.info_msg(n);
                }
            }
            Err(e) => self.open_failed(format!("Can't read {}: {e}", path.display())),
        }
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
        self.opened_masks = Some(Vec::new());
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
                 calibre is Not Set and the lift angle {DEFAULT_LIFT_DEG}°: set them on the readings' cards.",
                p.label()
            ),
            None => format!(
                "The file has no watch settings, so the calibre is Not Set, the lift angle \
                 {DEFAULT_LIFT_DEG}° and the position Dial Up: set them on the readings' cards."
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

    /// Analyse `path` whole. Without a session file it starts from
    /// `carried`, else from the defaults.
    fn analyse_whole(&mut self, path: &Path, carried: Option<WatchSetup>) {
        // Refused before anything on show is stopped.
        if let Some(why) = no_analysis_to_read(path).or_else(|| {
            take_folder(path)
                .ok()
                .and_then(|inner| no_analysis_to_read(&inner))
        }) {
            return self.open_failed(why);
        }
        self.stop();
        // Masks carried with the settings stay; anything else opened
        // starts with what its session file masks.
        let carried_masks = carried.is_some().then(|| std::mem::take(&mut self.masked));
        self.masked.clear();
        if carried_masks.is_none() {
            self.masking = false;
        }
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
        if analysis_take::is_analysis_only(path) {
            return self.open_analysis_only(path, manifest);
        }
        if let Some(why) = no_analysis_to_read(path) {
            return self.open_failed(why);
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
        // Read again for a new beat rate: the settings as they were, not
        // as the take saved them.
        if let Some(s) = self.reread_with.take() {
            self.set_watch_setup(s);
        }
        if let Some(m) = carried_masks {
            self.masked = m;
        }
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
            kept: None,
        });
    }

    /// Open a take kept as its analysis alone: its beats, amplitude
    /// windows and profiles read from `analysis/` in the background, then
    /// shown as an analysed recording is.
    fn open_analysis_only(
        &mut self,
        path: &Path,
        manifest: Option<(Manifest, Vec<RecordingEntry>)>,
    ) {
        let name = capture_label(path);
        if recordings_in(path).is_ok() {
            return self.open_failed(format!(
                "{name} holds both sound and an {} folder: a take is kept either with its \
                 sound or as its analysis alone. Move one of them out to open the other.",
                analysis_take::DIR
            ));
        }
        let Some((m, entries)) = manifest.filter(|(m, _)| m.audio == Some(false)) else {
            return self.open_failed(format!(
                "{name} has an {} folder, but its {MANIFEST} doesn't say audio = false, so \
                 it isn't known to be a take kept as its analysis alone.",
                analysis_take::DIR
            ));
        };
        let header_file = path.join(analysis_take::DIR).join(analysis_take::HEADER);
        let header = std::fs::read_to_string(&header_file)
            .ok()
            .and_then(|t| serde_json::from_str::<analysis_take::Header>(&t).ok())
            .filter(|h| h.format == analysis_take::FORMAT);
        let Some(header) = header else {
            let why = match analysis_take::read(path) {
                Err(e) => e.to_string(),
                Ok(_) => format!("{} can't be read", header_file.display()),
            };
            return self.open_failed(format!("Can't open {name}: {why}"));
        };
        self.keep_mic_setup();
        self.set_watch_setup(WatchSetup::default());
        self.open_note = None;
        let (stretches, settles) = self.apply_manifest(&m, &entries);
        let label = format!("{name} (analysis only)");
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let kept = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (p2, k2, dir) = (progress.clone(), kept.clone(), path.to_path_buf());
        let duration_s = header.duration_s;
        std::thread::spawn(move || {
            let r = analysis_take::read(&dir)
                .map_err(|e| e.to_string())
                .map(|s| {
                    if let Ok(mut k) = k2.lock() {
                        *k = s.profiles;
                    }
                    p2.store(duration_s.to_bits(), std::sync::atomic::Ordering::Relaxed);
                    Some(s.log)
                });
            let _ = tx.send(r);
        });
        self.opened_path = path.display().to_string();
        self.reset_session(header.sample_rate, label.clone());
        self.no_sound = true;
        self.session.analyse();
        self.live = None;
        self.batch = Some(Batch {
            rx,
            progress,
            cancel,
            duration_s: Some(duration_s),
            label,
            started: Instant::now(),
            files: 0,
            first: String::new(),
            last: String::new(),
            stretches,
            settles,
            order_note: None,
            kept: Some(kept),
        });
    }

    /// Stop listening, and any analysis under way. A
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
        self.poll_for(FRAME_BUDGET);
    }

    /// Save and read the sound that has come in, for up to `budget`, so a
    /// backlog never holds up the window for long. Returns the blocks of
    /// sound taken.
    fn poll_for(&mut self, budget: Duration) -> usize {
        let started = Instant::now();
        // After a while with nothing run, the input gets the usual grace
        // before it counts as quiet.
        if self.polled_at.elapsed() > Duration::from_secs(2) && self.audio_at.is_some() {
            self.audio_at = Some(started);
        }
        self.polled_at = started;
        let mut taken = 0;
        let mut ended = false;
        let mut failure = None;
        if let (Some(cap), Some(live)) = (self.capture.as_ref(), self.live.as_mut()) {
            loop {
                if taken > 0 && started.elapsed() > budget {
                    break;
                }
                let event = cap.rx.try_recv();
                if matches!(event, Ok(Event::Audio(_))) {
                    taken += 1;
                }
                match event {
                    Ok(Event::Audio(b)) => {
                        if !cap.is_file {
                            if let Some(gap) = slept(self.block_at, &b, live.sample_rate()) {
                                // The computer slept: carry on, as after a
                                // pause, with the time between out.
                                if let Ok(Err(e)) = self.session.pause(live.duration_s()) {
                                    failure = Some(CaptureFailure::Recording(e.to_string()));
                                    break;
                                }
                                if self.session.resume(&cap.label).is_ok() {
                                    live.skip(gap);
                                    self.session.resumed_at(live.duration_s());
                                }
                                if let Some(t) = self.clock_track.as_mut() {
                                    t.restart(live.sample_rate());
                                }
                                let m = format!(
                                    "The sound's timestamps jumped by {}, as they do when the \
                                     computer sleeps. The jump is left out of the take as a \
                                     break, and listening carries on.",
                                    fields::duration(gap.round())
                                );
                                applog::warn(&m);
                                self.message = Some((m, false));
                            }
                            self.block_at = Some(b.at);
                        }
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
                            // A jump the beats' timing shows is found a second
                            // or two after it: the points read since reached
                            // across it, so they go.
                            let jumps = live.jumps();
                            if jumps != self.jumps_seen.as_slice() {
                                let recent = live.duration_s() - 10.0;
                                let new = jumps
                                    .iter()
                                    .copied()
                                    .filter(|j| *j > recent && !self.jumps_seen.contains(j))
                                    .reduce(f64::min);
                                if let Some(j) = new {
                                    self.trend.retain(|p| p.t <= j);
                                    for t in self.other_trends.values_mut() {
                                        t.retain(|p| p.t <= j);
                                    }
                                    self.sound_history.retain(|h| h.0 <= j);
                                }
                                self.jumps_seen = jumps.to_vec();
                            }
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

        self.poll_analysis_save();
        self.poll_saving();
        let mut done = None;
        if let Some(b) = &self.batch {
            match b.rx.try_recv() {
                Ok(r) => done = Some(r),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => done = Some(Ok(None)),
            }
        }
        if let Some(r) = done {
            let (label, stretches, settles, kept) = self
                .batch
                .take()
                .map(|b| {
                    (
                        b.label.clone(),
                        b.stretches.clone(),
                        b.settles.clone(),
                        b.kept.clone(),
                    )
                })
                .unwrap_or_default();
            match r {
                Ok(Some(log)) => {
                    self.show_log(log, label, stretches, settles);
                    // A take with no sound shows the profiles it kept.
                    if let Some(k) =
                        kept.and_then(|k| k.lock().ok().map(|mut k| std::mem::take(&mut *k)))
                    {
                        self.sound_history = k.into_iter().map(|p| (p.end_s, p.sides)).collect();
                    }
                }
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
        taken
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
        live.set_masked(&self.masked);
        self.live = Some(live);
        self.steady = None;
        self.steady_job = None;
        self.rebuild_trend();
        self.source_label = label;
        self.anchor = None;
        self.strip_end = None;
        self.fit_take();
        let done = if self.no_sound {
            "Opened the analysis kept without its sound: the readings, charts and tests are as \
             they were measured, and the Scope needs the sound. Drag the strip to look \
             back."
        } else {
            "Analysed the whole recording. Drag the strip to look back."
        };
        let m = match self.open_note.take() {
            Some(n) => format!("{done} {n}"),
            None => done.to_string(),
        };
        self.info_msg(m);
    }

    /// Work the charts out again over the whole session, for a new
    /// averaging time or lift angle.
    fn rebuild_trend(&mut self) {
        self.strip_points = None;
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

    /// Mask out the stretches `spans` (start and end in the session's
    /// seconds) in place of those masked before: the readings, the charts
    /// and the steadiness tests are worked out again without them.
    fn set_masked(&mut self, spans: Vec<(f64, f64)>) {
        let pairs: Vec<[f64; 2]> = spans.iter().map(|&(a, b)| [a, b]).collect();
        self.masked = timegrapher_core::session::masked_spans(&pairs);
        if let Some(live) = self.live.as_mut() {
            live.set_masked(&self.masked);
        }
        // A Positions Test's positions are read again without them.
        if let (Some(test), Some(log)) = (
            self.guided.as_mut(),
            self.live.as_ref().and_then(steady::log_of),
        ) {
            let clock = self
                .clock
                .as_ref()
                .and_then(|c| timegrapher_core::session::fixed_clock(c.ppm, log.duration_s));
            test.reread(&log, clock.as_ref(), &self.masked);
        }
        self.steady = None;
        self.steady_job = None;
        self.rebuild_trend();
    }

    /// Add the stretch from `a` to `b` to those masked; one under half a
    /// second, a slip of the mouse, is left alone.
    fn mask(&mut self, a: f64, b: f64) {
        let (a, b) = (a.min(b).max(0.0), a.max(b).min(self.latest_s().max(0.0)));
        if b - a < 0.5 {
            return;
        }
        let mut v = self.masked.clone();
        v.push((a, b));
        self.set_masked(v);
    }

    /// Move the edges of the masked stretch `was` to those of `now`; one
    /// left under half a second is brought back. Stretches it now
    /// overlaps join it.
    fn reshape_mask(&mut self, was: (f64, f64), now: (f64, f64)) {
        let Some(i) = self.masked.iter().position(|&m| m == was) else {
            return;
        };
        let mut v = self.masked.clone();
        v.remove(i);
        let (a, b) = (now.0.max(0.0), now.1.min(self.latest_s().max(0.0)));
        if b - a >= 0.5 {
            v.push((a, b));
        }
        self.set_masked(v);
    }

    /// Bring back the masked stretch over `t`, if there is one.
    fn unmask_at(&mut self, t: f64) {
        let v: Vec<(f64, f64)> = self
            .masked
            .iter()
            .copied()
            .filter(|&(a, b)| !(t >= a && t <= b))
            .collect();
        if v.len() != self.masked.len() {
            self.set_masked(v);
        }
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

    /// The take in the position the watch is in now, once it is stopped
    /// and longer than a few readings: its settled stretches up to the
    /// last reading with a watch heard, read as `timegrapher session`
    /// reads a recording (core `session::measure_masked`), the time
    /// between them masked, and the seconds they cover.
    fn whole_take(&self) -> Option<WholeTake> {
        if self.running() {
            return None;
        }
        let live = self.live.as_ref()?;
        let end = self.last_heard_s().min(self.latest_s());
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
            self.clock.as_ref().map(|c| c.ppm),
        );
        if let Some((k, w)) = self.whole_take.borrow().as_ref() {
            if *k == key {
                return w.clone();
            }
        }
        let read = || {
            let span: f64 = spans.iter().map(|(a, b)| b - a).sum();
            let (&(from, _), &(_, to)) = (spans.first()?, spans.last()?);
            if span < 3.0 * self.average_s {
                return None;
            }
            let log = steady::log_of(live)?;
            let clock = self
                .clock
                .as_ref()
                .and_then(|c| timegrapher_core::session::fixed_clock(c.ppm, log.duration_s));
            let breaks: Vec<_> = log
                .jumps
                .iter()
                .map(|&audio_s| timegrapher_core::breaks::Break {
                    audio_s,
                    gap_s: 0.0,
                })
                .collect();
            // The time between the stretches, another position or its
            // settling, is left out as a mask is.
            let mut masked = self.masked.clone();
            masked.extend(spans.windows(2).map(|w| (w[0].1, w[1].0)));
            let m = timegrapher_core::session::measure_masked(
                &log,
                clock.as_ref(),
                &breaks,
                &masked,
                from,
                to,
            );
            Some(WholeTake { m, span })
        };
        let w = read();
        *self.whole_take.borrow_mut() = Some((key, w.clone()));
        w
    }

    /// Show the whole session so far along the strip, ending at its latest
    /// beat.
    fn fit_strip(&mut self) {
        self.strip_end = None;
        self.strip.set_span(self.latest_s());
    }

    /// An opened take shows whole: the strip's length fits all of it, and
    /// its width is the narrowest of the offered widths that holds every
    /// beat without wrapping. The beats are centred between the earliest
    /// and the latest, or, with Auto-Centre on, the newest stay on the
    /// centre line as it keeps them. The odd stray dot (one beat in a
    /// thousand at each end) is left out of the width, so one bad
    /// detection can't flatten the trace. The view before is kept for the
    /// next Listen.
    fn fit_take(&mut self) {
        let before = self.strip;
        self.fit_view();
        let live = self.live_strip.map_or(before, |(live, _)| live);
        self.live_strip = Some((live, self.strip));
    }

    /// The strip's view a live session goes back to: the one before a take
    /// was fitted, unless the strip was changed by hand since.
    fn live_view(&self) -> StripView {
        match self.live_strip {
            Some((live, fitted)) if fitted == self.strip => live,
            _ => self.strip,
        }
    }

    /// Fit the strip's length and width to the take on show.
    fn fit_view(&mut self) {
        self.fit_strip();
        let Some(live) = &self.live else { return };
        let Some(bph) = live.bph() else { return };
        let period = 3600.0 / bph as f64;
        let beats = live.beats();
        let centred = if self.follow {
            Anchor::centre_on(beats, period, 8)
        } else {
            Anchor::centre_on(&beats[..beats.len().min(16)], period, 16)
        };
        let Some(anchor) = centred else { return };
        let mut leads: Vec<f64> = beats.iter().map(|b| anchor.lead_ms(b, period)).collect();
        leads.sort_by(f64::total_cmp);
        let stray = leads.len() / 1000;
        let (lo, hi) = (leads[stray], leads[leads.len() - 1 - stray]);
        let (anchor, half) = if self.follow {
            (anchor, lo.abs().max(hi.abs()))
        } else {
            // An earlier anchor time makes every beat read later by as
            // much, which puts the middle of the spread on the centre line.
            let mid = (lo + hi) / 2.0;
            let shifted = Anchor {
                time: anchor.time - mid / 1000.0,
                ..anchor
            };
            (shifted, (hi - lo) / 2.0)
        };
        self.anchor = Some(anchor);
        // A little room at the edges, so the outermost dots aren't cut.
        let need = half * 1.1;
        let width = WIDTHS
            .iter()
            .copied()
            .find(|&w| w >= need)
            .unwrap_or(StripView::MAX_HALF_WIDTH_MS);
        self.strip.set_half_width(width);
    }

    /// The time at the strip's newest edge.
    fn strip_end_s(&self) -> f64 {
        let latest = self.latest_s();
        self.strip_end.unwrap_or(latest).min(latest)
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
        // A save is being written (its Save… started it, or one before):
        // wait for it. Once nothing is left unsaved, carry on; otherwise
        // (it failed, and the status bar says why, or there is sound since)
        // ask as before.
        if self.saving.is_some() {
            return self.confirm_saving_dialog(ctx);
        }
        if !self.needs_save_confirmation() {
            self.confirm = None;
            return self.confirmed(what, ctx);
        }
        let mut choice = None;
        let unsaved = strip::fmt_time(self.unsaved_s());
        let metadata = self.metadata_unsaved();
        let masks = self.masks_unsaved();
        let fault = self.session.recording_fault().map(|f| f.description());
        egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                RichText::new(match what {
                    Confirm::NewSession => "Start a new session?",
                    Confirm::NewTake(_) => "Start a new take without saving?",
                    Confirm::Quit => "Quit without saving?",
                    Confirm::Open { .. } => "Open the recording without saving?",
                    Confirm::Microphone | Confirm::QuickListen => {
                        "Start listening without saving?"
                    }
                })
                .font(theme::semibold(15.0)),
            );
            ui.add_space(4.0);
            ui.label(if let Some(fault) = &fault {
                fault.clone()
            } else if masks {
                "Changes to the masked stretches haven't been saved to the recording's session \
                 file and will be thrown away."
                    .to_string()
            } else if metadata && self.unsaved_s() <= 0.0 {
                "Changes to this session's settings or position history haven't been saved and will be thrown away.".to_string()
            } else if metadata {
                format!("This session's {unsaved} of sound and changes to its settings or position history haven't been saved and will be thrown away.")
            } else {
                format!("This session's {unsaved} of sound hasn't been saved and will be thrown away.")
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let save = if masks { "Save" } else { "Save…" };
                if fault.is_none() && ui.add(theme::primary(ui, save)).clicked() {
                    choice = Some(0);
                }
                let go = match (&fault, &what) {
                    (Some(_), Confirm::Quit) => "Leave Files and Quit",
                    (Some(_), _) => "Leave Files and Continue",
                    (
                        None,
                        Confirm::NewSession
                        | Confirm::NewTake(_)
                        | Confirm::Open { .. }
                        | Confirm::Microphone
                        | Confirm::QuickListen,
                    ) => "Don't Save",
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
            Some(0) if masks => {
                self.save_masks();
                if self.needs_save_confirmation() {
                    return; // saving failed
                }
            }
            Some(0) => {
                // Saved before: the rest goes to the same copy.
                self.save_again();
                if self.saving.is_some() {
                    return; // asked again, or carried on, once it is written
                }
                if self.needs_save_confirmation() {
                    return; // the folder dialog was cancelled, or saving failed
                }
            }
            Some(1) => {}
            Some(_) => {
                self.confirm = None;
                self.guided_after_ask = None;
                return;
            }
            None => return,
        }
        self.confirm = None;
        self.confirmed(what, ctx);
    }

    /// The question before throwing away unsaved sound, while a save is
    /// written.
    fn confirm_saving_dialog(&mut self, ctx: &egui::Context) {
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(RichText::new("Saving…").font(theme::semibold(15.0)));
            });
            ui.add_space(4.0);
            ui.label(
                "The sound so far is being copied into its take. This carries on once it is saved.",
            );
            ui.add_space(8.0);
            cancel = ui
                .add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 26.0)))
                .on_hover_text("Stay here; the save carries on")
                .clicked();
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if cancel {
            self.confirm = None;
            self.guided_after_ask = None;
        }
    }

    /// Do what the question before throwing away unsaved sound asked about.
    fn confirmed(&mut self, what: Confirm, ctx: &egui::Context) {
        match what {
            Confirm::NewSession => self.new_session(),
            Confirm::NewTake(kind) => self.new_take(kind, true),
            Confirm::Open { path } => self.open_recording(&path, false),
            Confirm::Microphone => self.listen_after_ask(),
            Confirm::QuickListen => self.quick_listen(true),
            Confirm::Quit => {
                self.may_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// The Watches pane: where the sound comes from and what to do with it,
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
                let picked = self.open_watch().is_some();
                if self.list_shown() {
                    self.section(ui, "Watches", watches::HELP, Self::watch_list_card);
                } else if picked {
                    self.watch_details(ui);
                    self.watch_actions(ui);
                    self.section(ui, "Takes", watches::HELP, Self::watch_takes_card);
                } else if self.unsorted_open {
                    watches::unsorted_details(ui);
                    self.section(ui, "Takes", watches::HELP, Self::watch_takes_card);
                } else {
                    self.section(ui, "Watches", watches::HELP, Self::watch_list_card);
                }
            });
    }

    /// The watch picked's takes to start: Listen, the Positions Test and
    /// the Power Reserve Test, each saved into it; Stop while one runs.
    fn watch_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if self.running() && self.session.mic() && !self.quick_listen {
                if ui
                    .add(theme::destructive(ui, "Stop"))
                    .on_hover_text(STOP_HINT)
                    .clicked()
                {
                    self.stop_listening();
                }
                return;
            }
            let free = !self.running() && self.batch.is_none();
            for (kind, name, hint) in NEW_TAKES {
                let b = if kind == NewTake::Listen {
                    theme::primary(ui, name)
                } else {
                    theme::secondary(ui, name)
                };
                if ui
                    .add_enabled(free, b)
                    .on_hover_text(hint)
                    .on_disabled_hover_text("Stop listening first")
                    .clicked()
                {
                    self.new_take(kind, false);
                }
            }
        });
    }

    /// The watch picked in the Watches pane, as the library last read it.
    fn open_watch(&self) -> Option<&library::WatchEntry> {
        let id = self.watch_open.as_ref()?;
        match &self.library {
            Ok(Some(l)) => l.watches.iter().find(|e| &e.watch.id == id),
            _ => None,
        }
    }

    /// The watch picked, or Unsorted when the pane shows it.
    fn open_group(&self) -> Option<library::WatchEntry> {
        match &self.library {
            Ok(Some(l)) if self.unsorted_open && self.watch_open.is_none() => {
                Some(l.unsorted.clone())
            }
            _ => self.open_watch().cloned(),
        }
    }

    /// Where Save… keeps a take: the `takes/` of the watch picked, or
    /// Unsorted's after Quick Listen or with Unsorted shown, where the
    /// library can be written.
    fn watch_takes_dir(&self) -> Option<PathBuf> {
        match &self.library {
            Ok(Some(l)) if l.read_only.is_none() => match self.open_watch() {
                Some(e) => Some(e.dir.join(library::TAKES)),
                None if self.quick_listen || self.unsorted_open => l.unsorted_dir().ok(),
                None => None,
            },
            _ => None,
        }
    }

    /// The library's watches, or what stands in for them.
    fn watch_list_card(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let mut show_unsorted = false;
        let picked = match &self.library {
            Ok(Some(l)) => {
                let shown = watches::shown(l, &self.watch_search, self.watch_sort);
                // Unsorted Takes first, any word about the list under it.
                let tiled = shown.iter().any(|e| !e.photos.is_empty());
                let marked = self.watch_open.as_deref();
                if watches::unsorted_row(
                    ui,
                    &l.unsorted,
                    tiled,
                    marked.is_none() && self.unsorted_open,
                ) {
                    show_unsorted = true;
                }
                if l.watches.is_empty() {
                    ui.label(
                        RichText::new(
                            "No watches yet. New Watch… adds one, and the takes of it you \
                             save are kept with it.",
                        )
                        .color(pal.text_secondary),
                    );
                } else if shown.is_empty() {
                    ui.label(
                        RichText::new("No watch matches the search.").color(pal.text_secondary),
                    );
                }
                let picked = watches::list(ui, &shown, marked, &mut self.thumbs);
                if let Some(why) = &l.read_only {
                    ui.label(
                        RichText::new(format!("{why}."))
                            .small()
                            .color(pal.text_secondary),
                    );
                }
                picked
            }
            Ok(None) => {
                ui.label(
                    RichText::new(
                        "No watches yet. New Watch… adds one, and the takes of it you save \
                         are kept with it.",
                    )
                    .color(pal.text_secondary),
                );
                None
            }
            Err(e) => {
                ui.label(RichText::new(format!("Your watches can't be read: {e}")).color(pal.bad));
                None
            }
        };
        if show_unsorted {
            self.pick_unsorted();
        }
        if let Some(id) = picked {
            self.pick_watch(&id);
        }
    }

    /// The picked watch's details and photos.
    fn watch_details(&mut self, ui: &mut egui::Ui) {
        let Some(e) = self.open_watch().cloned() else {
            return;
        };
        watches::details(ui, &e);
        if e.photos.is_empty() {
            return;
        }
        let writable = matches!(&self.library, Ok(Some(l)) if l.read_only.is_none());
        match watches::photo_strip(ui, &e, &mut self.thumbs, writable) {
            Some(watches::PhotoAction::Open(p)) => {
                if let Err(err) = reveal(&p) {
                    self.error(format!("Couldn't open {}: {err}", p.display()));
                }
            }
            Some(watches::PhotoAction::Show(p)) => {
                let dir = p.parent().unwrap_or(&p).to_path_buf();
                if let Err(err) = reveal(&dir) {
                    self.error(format!("Couldn't open {}: {err}", dir.display()));
                }
            }
            Some(watches::PhotoAction::Delete(photo)) => {
                self.take_ask = Some(watches::Ask::DeletePhoto { photo });
            }
            None => {}
        }
    }

    /// Add Photos…: copy the photos picked into the open watch's.
    fn add_photos(&mut self) {
        let Some(id) = self.open_watch().map(|e| e.watch.id.clone()) else {
            return;
        };
        let Some(files) = rfd::FileDialog::new()
            .set_title("Add Photos of the Watch")
            .add_filter(
                "Photos",
                &[
                    "jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "JPG", "JPEG", "PNG",
                    "HEIC", "HEIF", "TIF", "TIFF",
                ],
            )
            .pick_files()
        else {
            return;
        };
        let Ok(Some(lib)) = &mut self.library else {
            return;
        };
        match lib.add_photos(&id, &files) {
            Ok(added) => self.info_msg(format!(
                "Added {}.",
                if added.len() == 1 {
                    "1 photo".to_string()
                } else {
                    format!("{} photos", added.len())
                }
            )),
            Err(e) => self.error(format!("The photos couldn't all be added: {e}")),
        }
    }

    /// The picked watch's takes.
    fn watch_takes_card(&mut self, ui: &mut egui::Ui) {
        let Some(e) = self.open_group() else {
            return;
        };
        let (others, writable) = match &self.library {
            Ok(Some(l)) => (
                l.watches
                    .iter()
                    .filter(|w| w.watch.id != e.watch.id)
                    .map(|w| (w.watch.id.clone(), w.watch.name.clone()))
                    .collect(),
                l.read_only.is_none(),
            ),
            _ => (Vec::new(), false),
        };
        let keeping = self.take_keep.as_ref().map(|(t, job)| {
            let read = f64::from_bits(job.progress.load(std::sync::atomic::Ordering::Relaxed));
            (
                t.as_path(),
                (read / (2.0 * job.total_s).max(1e-9)).clamp(0.0, 1.0) as f32,
            )
        });
        if keeping.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        let on_show = self.take_on_show();
        let picked = self.take_picked.clone();
        let cx = watches::TakesContext {
            others: &others,
            writable,
            keeping,
            unsorted: e.watch.id.is_empty(),
            picked: picked.as_deref(),
            on_show: on_show.as_deref(),
        };
        let act = watches::takes(ui, &e, &cx);
        self.take_action(act);
    }

    /// Do what was asked of a take in the Watches pane's list.
    fn take_action(&mut self, act: Option<watches::TakeAction>) {
        match act {
            Some(watches::TakeAction::Pick(dir)) => self.take_picked = Some(dir),
            // The take on show is not read again.
            Some(watches::TakeAction::Open(dir)) if self.take_on_show().as_ref() == Some(&dir) => {
                self.take_picked = Some(dir);
            }
            Some(watches::TakeAction::Open(dir)) => {
                self.take_picked = Some(dir.clone());
                self.input = Input::File;
                self.file_path = dir.display().to_string();
                self.open_recording(&dir, true);
            }
            Some(watches::TakeAction::Show(dir)) => {
                if let Err(err) = reveal(&dir) {
                    self.error(format!("Couldn't open {}: {err}", dir.display()));
                }
            }
            Some(watches::TakeAction::Delete(take)) => {
                let bytes = folder_size(&take);
                self.take_ask = Some(watches::Ask::Delete { take, bytes });
            }
            Some(watches::TakeAction::KeepAnalysis(take)) => {
                let sound = recordings_in(&take)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|f| std::fs::metadata(f).ok())
                    .map(|m| m.len())
                    .sum();
                let bytes = folder_size(&take);
                self.take_ask = Some(watches::Ask::Keep { take, bytes, sound });
            }
            Some(watches::TakeAction::MoveTo(take, id)) => self.move_take(&take, &id),
            Some(watches::TakeAction::CancelKeep) => self.take_keep = None,
            None => {}
        }
    }

    /// Move a take into another watch (by its id): one in Unsorted is
    /// filed under it.
    fn move_take(&mut self, take: &Path, id: &str) {
        if let Some(why) = self.take_busy(take, false) {
            return self.info_msg(why);
        }
        let mut filed = false;
        let moved = match &mut self.library {
            Ok(Some(l)) => {
                filed = l.is_unsorted(take);
                l.move_take(take, id)
            }
            _ => Err("There is no Watches folder".into()),
        };
        match moved {
            Ok(to) => {
                self.take_moved(take, &to);
                let name = self
                    .library
                    .as_ref()
                    .ok()
                    .and_then(|l| l.as_ref())
                    .and_then(|l| l.watch_of(&to))
                    .map(|w| w.watch.name.clone())
                    .unwrap_or_default();
                if filed {
                    self.info_msg(format!("Filed \"{}\" under {name}.", file_name(&to)));
                } else {
                    self.info_msg(format!("Moved \"{}\" to {name}.", file_name(&to)));
                }
            }
            Err(err) => self.error(format!("The take couldn't be moved: {err}")),
        }
    }

    /// The recording opened, a lone file or a take, when it is outside
    /// the library and the library can take it.
    fn addable_take(&self) -> Option<PathBuf> {
        if self.input != Input::File || self.opened_path.is_empty() {
            return None;
        }
        let p = PathBuf::from(&self.opened_path);
        let root = match &self.library {
            Ok(Some(l)) if l.read_only.is_none() => l.root.clone(),
            // Made with the first watch.
            Ok(None) => self.library_root()?,
            _ => return None,
        };
        (!p.starts_with(root)).then_some(p)
    }

    /// Keep what is on show in the watch `id`: the unsaved Quick Listen
    /// take is saved into it, or the recording opened is copied in
    /// (asking for what neither it nor the watch says) and opened whole
    /// from there.
    fn keep_into(&mut self, id: &str) {
        if let Some(from) = self.addable_take() {
            // The pane shows the watch it goes into.
            self.watch_open = Some(id.to_string());
            self.unsorted_open = false;
            // A take folder with its session file goes in whole, copied
            // or moved.
            let dir = if from.is_dir() {
                from.clone()
            } else {
                from.parent().map(Path::to_path_buf).unwrap_or_default()
            };
            if dir.join(MANIFEST).is_file() || dir.join("session.json").is_file() {
                self.take_ask = Some(watches::Ask::Add {
                    from: dir,
                    watch: Some(id.to_string()),
                    keep_original: true,
                });
                return;
            }
            let Some(mut a) = self.open_ask_for(&from) else {
                return self.error(format!(
                    "\"{}\" can't be added to a watch.",
                    file_name(&from)
                ));
            };
            a.add = true;
            a.keeping = true;
            a.watch = Some(id.to_string());
            a.new_watch = false;
            let need = self.open_need(&a);
            if need == OpenNeed::default() {
                self.answer_open_ask(a, need);
            } else {
                self.open_ask = Some(a);
            }
        } else {
            self.keep_in_watch(id);
        }
    }

    /// Why `take` can't be moved or deleted now (or, with `keep`, kept
    /// as its analysis), if it can't: it is being recorded or read, or
    /// its analysis is being kept. A stopped session saved into it is let
    /// go of first, by [`Self::answer_take_ask`].
    fn take_busy(&self, take: &Path, keep: bool) -> Option<String> {
        let inside = |p: &Path| p.starts_with(take);
        if self.running() && self.session.saved_to().is_some_and(inside) {
            return Some("This take is still being recorded. Press Stop first.".into());
        }
        if self.running() && inside(Path::new(&self.opened_path)) {
            return Some("This take is being played. Press Stop first.".into());
        }
        if self.take_keep.as_ref().is_some_and(|(t, _)| t == take) {
            return Some(
                "This take's sound is already being removed. Wait for it to finish, or press \
                 Cancel."
                    .into(),
            );
        }
        if !keep && self.batch.is_some() && inside(Path::new(&self.opened_path)) {
            return Some(
                "This take is still being analysed. Wait for it to finish, or press Cancel.".into(),
            );
        }
        None
    }

    /// A take moved: the recording opened from it follows, and so does
    /// Save Again.
    fn take_moved(&mut self, from: &Path, to: &Path) {
        self.session.saved_moved(from, to);
        if let Ok(rest) = Path::new(&self.opened_path).strip_prefix(from) {
            let now = to.join(rest);
            self.opened_path = now.display().to_string();
            if Path::new(self.file_path.trim()).starts_with(from) {
                self.file_path = self.opened_path.clone();
            }
        }
    }

    /// The question about a take, while it is open, and what follows a yes.
    fn take_ask_dialog(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.take_ask.as_mut() else {
            return;
        };
        let watches: Vec<(String, String)> = match &self.library {
            Ok(Some(l)) => l
                .watches
                .iter()
                .map(|w| (w.watch.id.clone(), w.watch.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        match watches::ask_dialog(ctx, ask, &watches) {
            Some(true) => {
                if let Some(ask) = self.take_ask.take() {
                    self.answer_take_ask(ask);
                }
            }
            Some(false) => self.take_ask = None,
            None => {}
        }
    }

    fn answer_take_ask(&mut self, ask: watches::Ask) {
        // The session just saved into the take, stopped, lets go of it:
        // nothing more is saved there, and what is on show goes.
        let changed = match &ask {
            watches::Ask::Delete { take, .. } | watches::Ask::Keep { take, .. } => Some(take),
            _ => None,
        };
        let saved_into = changed
            .is_some_and(|take| self.session.saved_to().is_some_and(|p| p.starts_with(take)));
        if saved_into && !self.running() {
            self.new_session();
        }
        let busy = match &ask {
            watches::Ask::Delete { take, .. } => self.take_busy(take, false),
            watches::Ask::DeletePhoto { .. } => None,
            watches::Ask::Keep { take, .. } => self.take_busy(take, true),
            watches::Ask::Add {
                from,
                keep_original,
                ..
            } => (!keep_original)
                .then(|| self.take_busy(from, false))
                .flatten(),
        };
        if let Some(why) = busy {
            return self.info_msg(why);
        }
        let Ok(Some(lib)) = &mut self.library else {
            return self.error("There is no Watches folder.".into());
        };
        match ask {
            watches::Ask::Delete { take, .. } => match lib.delete_take(&take) {
                Ok(()) => {
                    // A take on show that is deleted leaves the window
                    // empty, as at the start, rather than showing sound
                    // that is gone.
                    if !self.running() && Path::new(&self.opened_path).starts_with(&take) {
                        self.new_session();
                    }
                    self.info_msg(format!("Deleted \"{}\".", file_name(&take)))
                }
                Err(e) => self.error(format!("The take couldn't be deleted: {e}")),
            },
            watches::Ask::Keep { take, .. } => self.start_take_keep(take),
            watches::Ask::DeletePhoto { photo } => match lib.delete_photo(&photo) {
                Ok(()) => {
                    self.thumbs.forget(&photo);
                    self.info_msg(format!("Deleted \"{}\".", file_name(&photo)));
                }
                Err(e) => self.error(format!("The photo couldn't be deleted: {e}")),
            },
            watches::Ask::Add {
                from,
                watch: Some(id),
                keep_original,
            } => match lib.add_take(&from, &id, keep_original) {
                Ok((to, left)) => {
                    // What is on show is now the watch's take, copied or
                    // moved.
                    self.take_moved(&from, &to);
                    self.watch_open = Some(id);
                    self.unsorted_open = false;
                    match left {
                        Some(why) => self.error(why),
                        None => self.info_msg(format!(
                            "Added \"{}\" to the watch's takes.",
                            file_name(&to)
                        )),
                    }
                }
                Err(e) => self.error(format!("The take couldn't be added: {e}")),
            },
            watches::Ask::Add { watch: None, .. } => {}
        }
    }

    /// Read a saved take's sound again for its analysis, in the
    /// background, writing it beside the take; [`Self::poll_take_keep`]
    /// then puts it in the take's place. Only a take whose session file
    /// names the whole folder in each entry, as the app's own takes do.
    fn start_take_keep(&mut self, take: PathBuf) {
        if self.take_keep.is_some() {
            return;
        }
        let prepared = (|| -> Result<_, String> {
            let file = take.join(MANIFEST);
            let text =
                std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let m: Manifest =
                toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
            if m.audio == Some(false) {
                return Err("its sound has already been removed".into());
            }
            if m.recordings.is_empty() || m.recordings.iter().any(|r| r.file != ".") {
                return Err(format!(
                    "its {MANIFEST} lists each sound file by name, and only takes whose \
                     {MANIFEST} names the whole folder, as the app writes it, can have their \
                     sound removed"
                ));
            }
            let files = recordings_in(&take)?;
            let mut total_s = 0.0;
            for f in &files {
                let i = timegrapher_core::audio::info(f)
                    .map_err(|e| format!("{}: {e}", f.display()))?;
                let frames = i
                    .frames
                    .ok_or_else(|| format!("{} doesn't say how long it is", f.display()))?;
                total_s += frames as f64 / f64::from(i.sample_rate.max(1));
            }
            let mut doc: toml_edit::DocumentMut = text
                .parse()
                .map_err(|e| format!("{}: {e}", file.display()))?;
            doc.insert("audio", toml_edit::value(false));
            let mut companions = vec![(MANIFEST.to_string(), doc.to_string().into_bytes())];
            for e in std::fs::read_dir(&take).map_err(|e| format!("{}: {e}", take.display()))? {
                let e = e.map_err(|e| e.to_string())?;
                let name = e.file_name().to_string_lossy().into_owned();
                if !e.file_type().is_ok_and(|t| t.is_file())
                    || name == MANIFEST
                    || files
                        .iter()
                        .any(|f| f.file_name() == Some(e.file_name().as_os_str()))
                {
                    continue;
                }
                let bytes = std::fs::read(e.path()).map_err(|err| format!("{name}: {err}"))?;
                companions.push((name, bytes));
            }
            let mut cfg = StreamConfig::default();
            cfg.analysis.bph = m.bph;
            cfg.analysis.amplitude.lift_deg = m.lift.unwrap_or(DEFAULT_LIFT_DEG);
            cfg.analysis.lift_known = m.lift_known.unwrap_or(m.lift.is_some());
            let shapes = timegrapher_core::analysis_take::shape_stretches(&m, total_s);
            Ok((files, total_s, companions, cfg, shapes))
        })();
        let (files, total_s, companions, cfg, shapes) = match prepared {
            Ok(p) => p,
            Err(e) => {
                return self.error(format!(
                    "The sound couldn't be removed from this take, so nothing was changed: {e}"
                ))
            }
        };
        let dest = take.with_file_name(format!(".{}.analysis", file_name(&take)));
        let _ = std::fs::remove_dir_all(&dest);
        let software = report::software();
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (p2, c2) = (progress.clone(), cancel.clone());
        std::thread::spawn(move || {
            let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let stored = timegrapher_core::analysis_take::measure(
                    &paths,
                    &cfg,
                    &shapes,
                    software,
                    |s| {
                        p2.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
                        if c2.load(std::sync::atomic::Ordering::Relaxed) {
                            std::panic::resume_unwind(Box::new(Cancelled));
                        }
                    },
                )
                .map_err(|e| e.to_string())?;
                timegrapher_core::analysis_take::save(&dest, &companions, &stored)
                    .map_err(|e| e.to_string())
            }));
            match r {
                Ok(r) => {
                    if let Err(std::sync::mpsc::SendError(Ok(dir))) = tx.send(r) {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                }
                Err(e) if e.is::<Cancelled>() => {}
                Err(e) => std::panic::resume_unwind(e),
            }
        });
        self.take_keep = Some((
            take,
            AnalysisSave {
                rx,
                progress,
                cancel,
                total_s,
            },
        ));
    }

    /// The kept analysis, once written, put in its take's place.
    fn poll_take_keep(&mut self) {
        let Some((_, job)) = &self.take_keep else {
            return;
        };
        let done = match job.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("it stopped part way".into()),
        };
        // Its outcome taken, the job has nothing left to clean up.
        let Some((take, _)) = self.take_keep.take() else {
            return;
        };
        let before = folder_size(&take);
        let placed = match (done, &mut self.library) {
            (Ok(kept), Ok(Some(l))) => l.replace_take(&take, &kept).inspect_err(|_| {
                let _ = std::fs::remove_dir_all(&kept);
            }),
            (Ok(kept), _) => {
                let _ = std::fs::remove_dir_all(&kept);
                Err("the Watches folder is no longer open".into())
            }
            (Err(e), _) => Err(e),
        };
        match placed {
            Ok(None) => self.info_msg(format!(
                "Removed the sound from \"{}\" and kept its analysis. It now uses {}, down \
                 from {}.",
                file_name(&take),
                fmt_bytes(folder_size(&take)),
                fmt_bytes(before)
            )),
            Ok(Some(why)) => self.error(why),
            Err(e) => self.error(format!(
                "The sound couldn't be removed from this take, so it still has its sound: {e}"
            )),
        }
    }

    /// Pick a watch: the pane shows it, and a new take starts with its
    /// name, calibre, lift angle and beat rate. Refused while listening,
    /// since the readings so far are of the watch already set.
    fn pick_watch(&mut self, id: &str) {
        if self.running() {
            self.info_msg("Stop listening before picking another watch.".into());
            return;
        }
        self.watch_open = Some(id.to_string());
        self.unsorted_open = false;
        // The watch picked shows, not the list left over from before.
        self.list_for = None;
        self.quick_defaults = false;
        let Some(w) = self.open_watch().map(|e| e.watch.clone()) else {
            self.watch_open = None;
            return;
        };
        // Readings on show keep the settings they were measured with; the
        // watch's apply to its next take.
        if self.live.is_some() {
            return;
        }
        self.quick_listen = false;
        self.apply_watch(&w);
    }

    /// Whether the pane shows the list over what is picked.
    fn list_shown(&self) -> bool {
        self.list_for
            .as_ref()
            .is_some_and(|(w, u)| *w == self.watch_open && *u == self.unsorted_open)
    }

    /// Unsorted Takes: Quick Listen again, with no watch, its takes on
    /// show and the settings back to the defaults: no calibre, the
    /// default lift angle and the beat rate found from the beats.
    fn pick_unsorted(&mut self) {
        if self.running() {
            self.info_msg("Stop listening before picking Unsorted Takes.".into());
            return;
        }
        // A watch's take on show, all of it saved, is put away: Unsorted
        // Takes starts again from Quick Listen's settings.
        let watch_take = self.watch_open.is_some()
            && self.live.is_some()
            && !self.needs_save_confirmation()
            && self.session.unsaved_s() < 0.5;
        self.watch_open = None;
        self.unsorted_open = true;
        // Its takes show, not the list left over from before.
        self.list_for = None;
        self.quick_listen = false;
        if watch_take {
            self.new_session();
        }
        // Other readings on show keep the settings they were measured with.
        if self.live.is_some() {
            self.quick_defaults = true;
        } else {
            self.quick_listen_defaults();
        }
    }

    /// A beat rate picked, or Auto. A take on show is read again at it,
    /// with the settings and masks as they are now, since its readings
    /// were worked out at the old one.
    fn change_bph(&mut self, bph: Option<u32>) {
        let shown = (!self.running()).then(|| self.take_on_show()).flatten();
        self.bph = bph;
        if let Some(l) = self.live.as_mut() {
            l.set_bph(bph);
        }
        self.anchor = None;
        self.clear_trends();
        if let Some(take) = shown {
            let masked = self.masked.clone();
            self.reread_with = Some(self.watch_setup());
            self.analyse_whole(&take, None);
            self.reread_with = None;
            self.masked = masked;
        }
    }

    /// Quick Listen's settings: no watch, no calibre, the default lift
    /// angle and the beat rate found from the beats.
    fn quick_listen_defaults(&mut self) {
        self.quick_defaults = false;
        self.watch.clear();
        if !self.calibre.is_empty() {
            self.calibre.clear();
            self.steady = None;
            self.steady_job = None;
        }
        if self.lift_deg != DEFAULT_LIFT_DEG {
            self.set_lift(DEFAULT_LIFT_DEG);
        }
        self.lift_context = (DEFAULT_LIFT_DEG, false);
        if self.bph.is_some() {
            self.bph = None;
            if let Some(l) = self.live.as_mut() {
                l.set_bph(None);
            }
            self.anchor = None;
            self.clear_trends();
        }
        self.note_settings();
    }

    /// A watch's details as this take's settings.
    fn apply_watch(&mut self, w: &library::Watch) {
        self.watch = w.name.clone();
        match w.calibre.clone().filter(|c| !c.is_empty()) {
            Some(c) => self.set_calibre(c),
            // Not the calibre of the watch picked before.
            None if !self.calibre.is_empty() => {
                self.calibre.clear();
                self.steady = None;
                self.steady_job = None;
            }
            None => {}
        }
        if let Some(l) = w.lift {
            self.set_lift(l);
            self.lift_context = (l, true);
        }
        if w.bph.is_some() && w.bph != self.bph {
            self.bph = w.bph;
            if let Some(l) = self.live.as_mut() {
                l.set_bph(w.bph);
            }
            self.anchor = None;
            self.clear_trends();
        }
        self.note_settings();
    }

    /// The Watches pane's toolbar: the list's search, order and New
    /// Watch…, or, with a watch picked, the way back and Edit….
    fn watches_tools(&mut self, ui: &mut egui::Ui) {
        let read_only = "This copy of the app can only read your watches";
        if self.list_shown() {
            // The list's own tools, below.
        } else if self.unsorted_open && self.watch_open.is_none() {
            if theme::tool_button(
                ui,
                Icon::Back,
                "‹ All Watches",
                "Back to the list of watches, Unsorted Takes marked",
                None,
            ) {
                self.list_for = Some((None, true));
            }
            return;
        } else if let Some(e) = self.open_watch() {
            let w = e.watch.clone();
            if theme::tool_button(
                ui,
                Icon::Back,
                "‹ All Watches",
                "Back to the list of watches, this one marked: Listen and the tests still take \
                 it until another row is picked",
                None,
            ) {
                self.list_for = Some((self.watch_open.clone(), false));
            }
            let writable = matches!(&self.library, Ok(Some(l)) if l.read_only.is_none());
            let locked = (!writable).then_some(read_only);
            theme::bar_gap(ui);
            if theme::tool_button(
                ui,
                Icon::Screwdriver,
                "Edit…",
                "Change this watch's details",
                locked,
            ) {
                self.watch_form = Some(watches::Form {
                    watch: w,
                    new: false,
                    error: None,
                });
            }
            if theme::tool_button(
                ui,
                Icon::NewPhoto,
                "Add Photos…",
                "Copy photos of this watch into its folder (JPEG, PNG, HEIC from an iPhone, or \
                 TIFF). The first by name leads its row in the list.",
                locked,
            ) {
                self.add_photos();
            }
            return;
        }
        // The pane's main action first, in words, and never folded: the
        // empty list names it.
        let writable = !matches!(&self.library, Ok(Some(l)) if l.read_only.is_some())
            && self.library_root().is_some();
        theme::keep_next(ui);
        if theme::tool_word_button(
            ui,
            Icon::NewWatch,
            "New Watch…",
            "Add a watch: only its name is needed",
            false,
            (!writable).then_some(
                "This copy of the app can only read your watches, or there is no folder for them",
            ),
        ) {
            self.open_new_watch_form();
        }
        theme::bar_gap(ui);
        // The search stays in the row too: Sort folds first.
        theme::keep_next(ui);
        theme::tool_search(
            ui,
            "Show the watches whose name, calibre, notes or fields hold this",
            |ui| {
                // Narrower in a narrow pane, leaving room for "…" after it.
                let w = (ui.available_width() - 48.0).clamp(72.0, 120.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.watch_search)
                        .hint_text("Search")
                        .desired_width(w),
                )
                .on_hover_text("Show the watches whose name, calibre, notes or fields hold this");
            },
        );
        let mut sort = self.watch_sort;
        theme::tool_field(ui, "Sort", "The order of the list", |ui| {
            egui::ComboBox::from_id_salt("watch sort")
                .selected_text(sort.words())
                .show_ui(ui, |ui| {
                    for s in watches::Sort::ALL {
                        ui.selectable_value(&mut sort, s, s.words());
                    }
                });
        });
        self.watch_sort = sort;
    }

    /// The form for a new watch: only its name is needed.
    fn open_new_watch_form(&mut self) {
        let mut w = library::Watch::new("");
        w.calibre = Some(String::new());
        w.notes = Some(String::new());
        self.watch_form = Some(watches::Form {
            watch: w,
            new: true,
            error: None,
        });
    }

    /// The form for a watch's details, while it is open.
    fn watch_form_dialog(&mut self, ctx: &egui::Context) {
        let Some(f) = self.watch_form.as_mut() else {
            return;
        };
        match watches::form(ctx, f, &mut self.cal) {
            Some(watches::FormAction::Save) => self.save_watch_form(),
            Some(watches::FormAction::Cancel) => {
                self.watch_form = None;
                self.keep_after_form = false;
            }
            None => {}
        }
    }

    /// The take opened whole (or being opened), if one is on show.
    fn take_on_show(&self) -> Option<PathBuf> {
        (matches!(
            self.session.state(),
            session::State::Analysing | session::State::Analysed
        ) && !self.opened_path.is_empty())
        .then(|| PathBuf::from(&self.opened_path))
    }

    /// Make the library at its folder when there is none yet.
    fn ensure_library(&mut self) {
        if matches!(self.library, Ok(None)) {
            self.library = match self.library_root() {
                Some(root) => library::Library::open_or_create(&root).map(Some),
                None => Err("there is no folder for it: choose one in Settings".into()),
            };
        }
    }

    /// Quick Listen: show Unsorted and listen to the microphone in a new
    /// session, without a watch picked, its name cleared. Stop keeps
    /// nothing until Keep in a Watch… or Save…. Unsaved sound is asked about first, unless
    /// `asked`.
    fn quick_listen(&mut self, asked: bool) {
        if self.quick_listen_ready(asked) {
            self.start_microphone();
        }
    }

    /// All of Quick Listen but opening the microphone: whether to open it.
    fn quick_listen_ready(&mut self, asked: bool) -> bool {
        if self.running() {
            self.info_msg("Stop listening first.".into());
            return false;
        }
        self.ensure_library();
        let ready = match &self.library {
            Ok(Some(l)) => match &l.read_only {
                Some(why) => Err(why.clone()),
                None => l.unsorted_dir().map(|_| ()),
            },
            Ok(None) => Err("There is no Watches folder".into()),
            Err(e) => Err(format!("Your watches can't be read: {e}")),
        };
        if let Err(e) = ready {
            self.error(format!("Quick Listen can't keep its take: {e}"));
            return false;
        }
        if !asked && self.needs_save_confirmation() {
            self.confirm = Some(Confirm::QuickListen);
            return false;
        }
        // Not even a resume: Quick Listen starts a new session.
        if self.saving_busy() {
            return false;
        }
        if self.live.is_some() || self.session.recorder.is_some() {
            self.new_session();
        }
        if self.quick_defaults {
            self.quick_listen_defaults();
        }
        self.watch_open = None;
        self.unsorted_open = true;
        // The Watches pane shows the list, Unsorted Takes marked.
        self.list_for = Some((None, true));
        self.quick_listen = true;
        self.input = Input::Microphone;
        self.watch.clear();
        self.note_settings();
        true
    }

    /// Write the form's watch into the library, making the library first
    /// for the first watch.
    fn save_watch_form(&mut self) {
        let Some(f) = self.watch_form.clone() else {
            return;
        };
        let mut w = f.watch;
        w.name = w.name.trim().to_string();
        // Empty is the same as not set.
        w.calibre = w.calibre.filter(|c| !c.trim().is_empty());
        w.notes = w.notes.filter(|n| !n.trim().is_empty());
        w.fields
            .retain(|x| !(x.label.trim().is_empty() && x.value.trim().is_empty()));
        // The app keeps no Archived mark: one an older version set goes
        // when the watch is saved, so the command line lists it too.
        w.archived = false;
        self.ensure_library();
        let done = match &mut self.library {
            Ok(Some(lib)) if f.new => lib.add(&w).map(|_| ()),
            Ok(Some(lib)) => lib.update(&w),
            Ok(None) => Err("There is no Watches folder".into()),
            Err(e) => Err(format!("Your watches can't be read: {e}")),
        };
        match done {
            Ok(()) => {
                self.watch_form = None;
                if f.new && std::mem::take(&mut self.keep_after_form) {
                    match self.test_keep.take() {
                        Some(kind) => self.keep_test(kind, Some(&w.id)),
                        None => self.keep_into(&w.id),
                    }
                } else if f.new {
                    self.pick_watch(&w.id);
                } else if self.watch_open.as_ref() == Some(&w.id)
                    && !self.running()
                    && self.live.is_none()
                {
                    // The take not yet started follows the watch's new details.
                    self.apply_watch(&w);
                }
            }
            Err(e) => {
                if let Some(f) = self.watch_form.as_mut() {
                    f.error = Some(e);
                }
            }
        }
    }

    /// The microphone menu, the level, listening and saving.
    fn microphone_card(&mut self, ui: &mut egui::Ui) {
        let choices = self.input_choices();
        let listed = self.devices_listed;
        let sel = choices
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
            );
        let mut opened = false;
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            row,
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                // Kept to the pane's width, the name cut short to fit: a
                // combo box in a row is otherwise as wide as its words, and a
                // long name (as macOS gives) widens the whole pane past its
                // card. Rescan is laid first, from the right, so the box takes
                // what is left of the row, less its own frame and arrow.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                if ui
                    .add(egui::Button::new("Rescan").wrap_mode(egui::TextWrapMode::Extend))
                    .on_hover_text("Look for microphones again, after plugging one in")
                    .clicked()
                {
                    self.rescan();
                }
                let sp = ui.spacing();
                let frame = sp.icon_width + sp.icon_spacing + 2.0 * sp.button_padding.x;
                let full = sel.clone();
                egui::ComboBox::from_id_salt("device")
                    .selected_text(sel)
                    .width((ui.available_width() - frame).max(60.0))
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
        ui.add_space(2.0);
        self.microphone_settings(ui);
    }

    /// A group of settings in the Watches pane under its caption, which
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
    fn pane_tools(&mut self, ui: &mut egui::Ui, pane: Pane) -> bool {
        let title = pane.title();
        match pane {
            Pane::Watch => theme::pane_toolbar(ui, title, watches::HELP, |ui| {
                self.watches_tools(ui);
            }),
            Pane::Strip => theme::pane_toolbar(ui, title, help::STRIP, |ui| {
                self.strip_settings(ui);
            }),
            Pane::Sound => theme::pane_toolbar(ui, title, help::PROFILE, |ui| {
                self.profile_settings(ui);
            }),
            Pane::Scope => theme::pane_toolbar(ui, title, help::SCOPE, |ui| {
                scope::settings(ui, &mut self.scope_view);
                self.export_button(ui, pane);
            }),
            p if p.steady_view().is_some() => {
                theme::pane_toolbar(ui, title, help::STEADINESS, |ui| {
                    self.steady_settings(ui, p);
                    self.export_button(ui, p);
                })
            }
            Pane::Isochronism => theme::pane_toolbar(ui, title, help::ISOCHRONISM, |ui| {
                self.export_button(ui, pane)
            }),
            Pane::PositionsTest => theme::pane_toolbar(ui, title, help::POSITIONS_TEST, |ui| {
                self.positions_settings(ui);
                self.export_button(ui, pane);
            }),
            Pane::PowerReserve => theme::pane_toolbar(ui, title, help::POWER_RESERVE, |ui| {
                self.reserve_settings(ui);
            }),
            Pane::Distributions => theme::pane_toolbar(ui, title, help::HISTOGRAMS, |ui| {
                steady::series_switches(ui, &mut self.dist_series);
                theme::bar_gap(ui);
                self.histogram_settings(ui);
                self.pane_average_setting(ui, pane);
                self.export_button(ui, pane);
            }),
            Pane::Charts => theme::pane_toolbar(ui, title, help::CHARTS, |ui| {
                steady::series_switches(ui, &mut self.chart_series);
                theme::bar_gap(ui);
                self.pane_average_setting(ui, pane);
                self.export_button(ui, pane);
            }),
            _ => return false,
        }
        true
    }

    /// Export: save the pane as a PNG picture or an SVG drawing.
    fn export_button(&mut self, ui: &mut egui::Ui, pane: Pane) {
        if theme::tool_button(ui, Icon::Export, "Export…", EXPORT_HINT, None) {
            let dir = self.export_dir.clone().or_else(|| {
                Path::new(self.file_path.trim())
                    .parent()
                    .map(Path::to_path_buf)
            });
            if let Some(path) = export::ask_path(pane.title(), dir.as_deref()) {
                self.export_dir = path.parent().map(Path::to_path_buf);
                self.start_export(pane, path);
                ui.ctx().request_repaint();
            }
        }
    }

    /// Draw `pane` for a picture on the next frames and save it to `path`.
    fn start_export(&mut self, pane: Pane, path: PathBuf) {
        self.export = Some(PaneExport {
            pane,
            path,
            rect: None,
            asked: false,
            frames: 0,
        });
    }

    /// A pane's name as its tab shows it, with its own averaging time.
    fn pane_heading(&self, pane: Pane) -> String {
        match self.pane_average.get(&pane) {
            Some(&s) => format!("{} · {}", pane.title(), fields::duration(s)),
            None => pane.title().to_string(),
        }
    }

    /// Where a picture of a pane has its toolbar: the pane's name, and a
    /// chart's key, since the tab bar isn't in the picture.
    fn export_heading(&self, ui: &mut egui::Ui, pane: Pane) {
        let pal = theme::pal(ui);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(self.pane_heading(pane))
                    .font(theme::semibold(13.0))
                    .color(pal.text),
            );
            let key = if pane == Pane::Charts {
                chart_key(self.charts_drawn, pal)
            } else {
                Vec::new()
            };
            for (name, color, gaps) in key {
                ui.add_space(10.0);
                match gaps {
                    Some(g) => theme::dash(ui, color, g),
                    None => theme::dot(ui, color),
                };
                ui.label(RichText::new(name).small().color(pal.text_secondary));
            }
        });
    }

    /// The pane's shapes are in: an SVG is written now, and a PNG waits for
    /// the screenshot of the frame, asked for here.
    fn pane_drawn_for_export(
        &mut self,
        ctx: &egui::Context,
        rect: egui::Rect,
        shapes: Vec<egui::epaint::ClippedShape>,
        background: Color32,
    ) {
        let Some(e) = &mut self.export else {
            return;
        };
        if export::is_svg(&e.path) {
            let (pane, path) = (e.pane, e.path.clone());
            self.export = None;
            let doc = export::svg(&shapes, rect, background);
            self.export_done(pane, &path, std::fs::write(&path, doc));
        } else {
            e.rect = Some(rect);
            if !e.asked {
                e.asked = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint();
        }
    }

    /// A screenshot came back: cut the pane out of it and save it.
    fn screenshot_for_export(&mut self, image: &egui::ColorImage, pixels_per_point: f32) {
        let Some(PaneExport {
            pane,
            path,
            rect: Some(rect),
            asked: true,
            ..
        }) = self.export.take()
        else {
            return;
        };
        let part = export::crop(image, rect, pixels_per_point);
        self.export_done(pane, &path, export::write_png(&path, &part));
    }

    /// A pane being saved as a PNG: the screenshot of its frame, or giving
    /// up when none comes.
    fn export_tick(&mut self, ctx: &egui::Context) {
        if self.export.is_some() {
            let shot = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                self.screenshot_for_export(&image, ctx.pixels_per_point());
            } else if let Some(e) = &mut self.export {
                // A pane that isn't drawn, or a screen that can't be read.
                e.frames += 1;
                if e.frames > 60 {
                    let pane = e.pane;
                    self.export = None;
                    self.error(format!(
                        "Couldn't save {}: the pane wasn't drawn for the picture",
                        pane.title()
                    ));
                }
            }
        }
    }

    fn export_done(&mut self, pane: Pane, path: &Path, result: std::io::Result<()>) {
        match result {
            Ok(()) => self.info_msg(format!("Saved {} to {}", pane.title(), path.display())),
            Err(e) => self.error(format!(
                "Couldn't save {} to {}: {e}",
                pane.title(),
                path.display()
            )),
        }
    }

    /// Average Over for one pane: Readings' time, or one of its own.
    fn pane_average_setting(&mut self, ui: &mut egui::Ui, pane: Pane) {
        theme::tool_field(ui, "Average Over", PANE_AVERAGE_HINT, |ui| {
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
                None => format!("{readings} as Readings"),
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
        let accent = theme::pal(ui).accent;
        if view == steady::View::Periods {
            theme::bar_gap(ui);
            theme::tool_switch(
                ui,
                Icon::Log,
                accent,
                "Log Scale",
                &mut self.periods_log,
                "Draw the scores on a log scale, so a small peak beside a tall one still shows",
            );
        }
        if view == steady::View::Fft {
            theme::bar_gap(ui);
            theme::tool_switch(
                ui,
                Icon::Stopwatch,
                accent,
                "Timing",
                &mut self.fft_timing,
                "Draw rate as the beat timing's spectrum, in ms, at every period, rather than \
                 the swing it makes in 10 s rate readings, in s/d",
            );
        }
        if matches!(view, steady::View::Stft | steady::View::Wavelet) {
            theme::bar_gap(ui);
            theme::tool_switch(
                ui,
                Icon::Log,
                accent,
                "Log Scale",
                &mut self.maps_log,
                "Colour the sizes on a log scale, over a range of 300 to 1, so weak lines \
                 beside strong ones still show",
            );
        }
        if matches!(
            view,
            steady::View::Periods | steady::View::Fft | steady::View::Stft | steady::View::Wavelet
        ) {
            theme::tool_switch(
                ui,
                Icon::Gears,
                accent,
                "Wheel Periods",
                &mut self.show_wheels,
                steady::WHEELS_HINT,
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
                maps_log: self.maps_log,
                wheels: self.show_wheels,
            },
        );
    }

    fn histogram_settings(&mut self, ui: &mut egui::Ui) {
        theme::tool_choice(
            ui,
            "Show",
            "The values on probability paper, or their cumulative share",
            &mut self.hist_cumulative,
            &[
                (
                    false,
                    Icon::ProbLine,
                    "Probability",
                    "Each value against its rank on a scale of standard deviations: \
                             one steady state falls on the dashed straight line, two states \
                             draw two lines joined by a bend",
                ),
                (
                    true,
                    Icon::SCurve,
                    "Cumulative",
                    "The share of values at or below each value: one state rises in \
                             one steep stretch, two states rise twice with a flatter stretch \
                             between",
                ),
            ],
        );
        theme::tool_choice(
            ui,
            "Values",
            "What the distributions count",
            &mut self.hist_readings,
            &[
                (
                    true,
                    Icon::Bucket,
                    "Readings",
                    "The readings, each averaged over Average Over, as on the charts \
                             and the strip, one per Average Over so that no two share beats: \
                             states lasting longer than that stand out clearly",
                ),
                (
                    false,
                    Icon::TwoSeconds,
                    "2 s",
                    "Each 2 seconds of beats, the finest the app measures: catches \
                             quicker changes, with more scatter",
                ),
            ],
        );
    }

    /// What the strip shows, for the status line: the take's name, its
    /// watch and its state.
    fn status_words(&self) -> String {
        let file_name = |p: &Path| {
            p.file_name()
                .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into())
        };
        let name = if let Some(to) = self.session.saved_to() {
            file_name(to)
        } else if self.input == Input::File && !self.opened_path.is_empty() {
            file_name(Path::new(&self.opened_path))
        } else if self.quick_listen {
            "Quick Listen".to_string()
        } else {
            "New Take".to_string()
        };
        let watch = if !self.watch.is_empty() {
            self.watch.clone()
        } else if self.quick_listen {
            "No watch".to_string()
        } else {
            "No watch picked".to_string()
        };
        let state = match self.session.state() {
            session::State::Idle => "Not started",
            session::State::Listening => "Listening",
            session::State::Paused { .. } => "Stopped",
            // Only tests play a recording through as if live.
            session::State::Replaying => "Listening",
            session::State::ReplayStopped => "Stopped",
            session::State::Analysing => "Opening",
            session::State::Analysed => "Opened",
        };
        format!("{name} · {watch} · {state}")
    }

    /// The controls on the readings' cards, each in the strip its card
    /// left for it.
    fn card_controls(&mut self, ui: &mut egui::Ui, out: &Readouts) {
        for &(slot, rect) in &out.slots {
            // A child of its own, not a scope: a scope would move the
            // row's cursor back up to the foot of what it drew, and the
            // panes below would be laid over the cards.
            let mut ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("card controls", slot))
                    .max_rect(rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            match slot {
                Slot::Position => self.position_buttons(&mut ui),
                Slot::Listen => self.listen_buttons(&mut ui),
                Slot::Lift => self.lift_control(&mut ui),
                Slot::BeatRate => self.beat_rate_controls(&mut ui),
            }
        }
    }

    /// Whether the Position card sets the position: while listening, or
    /// before anything is on show, and never during a test, which sets it
    /// itself. A take's positions are its history.
    fn position_editable(&self) -> bool {
        let testing = (self.running() && self.guided.is_some()) || self.reserve_running();
        let listening = self.running() && self.session.mic();
        !testing && (listening || self.live.is_none())
    }

    /// The position the Position card shows: the one set while it can be
    /// set, else the take's at the moment the readings show.
    fn shown_position(&self) -> Position {
        if self.position_editable() || self.running() {
            return self.position;
        }
        let at = self.latest_s();
        self.session
            .stretches()
            .into_iter()
            .take_while(|&(t, _)| t <= at + 1e-6)
            .last()
            .map_or(self.position, |(_, p)| p)
    }

    /// The Current Position card: the watch as it lies and its name in the
    /// middle, and six buttons under them, DU to CR, the current one
    /// filled: one row where the card has room, else two rows of three.
    /// The buttons set the position in a free live take; on a take on show
    /// or during a test they only show it.
    fn position_buttons(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let here = self.shown_position();
        let editable = self.position_editable();
        let area = ui.max_rect();
        let rows = position_rows(area.width());
        let buttons_h = position_buttons_h(rows);
        let top = egui::Rect::from_min_max(
            area.min,
            egui::pos2(area.right(), area.bottom() - buttons_h - 6.0),
        );
        // The picture and the label together, centred; the label falls
        // back to the codes alone, then to the codes under the picture, as
        // the card narrows.
        let name_font = theme::semibold(12.5);
        let code_font = egui::FontId::proportional(10.5);
        let codes_of = |p: Position| format!("{} · {}", p.code(), p.witschi());
        let codes = codes_of(here);
        // Sized for the widest of the six, so the picture keeps its size
        // whichever position is set.
        let (name_w, codes_w, code_h) = ui.fonts_mut(|f| {
            let mut w = |t: String, font: &egui::FontId| {
                f.layout_no_wrap(t, font.clone(), pal.text).size().x
            };
            let (n, c) = Position::ALL
                .into_iter()
                .fold((0.0f32, 0.0f32), |(n, c), p| {
                    (
                        n.max(w(p.name().to_string(), &name_font)),
                        c.max(w(codes_of(p), &code_font)),
                    )
                });
            (n, c, f.row_height(&code_font))
        });
        let side_for = |label_w: f32| {
            (top.width() - label_w - 8.0)
                .min(top.height())
                .min(POSITION_PIC_H)
        };
        let (side, label) = if side_for(name_w.max(codes_w)) >= 44.0 {
            (side_for(name_w.max(codes_w)), PositionLabel::Beside)
        } else if side_for(codes_w) >= 44.0 {
            (side_for(codes_w), PositionLabel::Codes)
        } else {
            (
                (top.height() - code_h - 2.0)
                    .min(top.width())
                    .min(POSITION_PIC_H),
                PositionLabel::Under,
            )
        };
        let label_w = match label {
            PositionLabel::Beside => name_w.max(codes_w) + 8.0,
            PositionLabel::Codes => codes_w + 8.0,
            PositionLabel::Under => 0.0,
        };
        let pic = if label == PositionLabel::Under {
            egui::Rect::from_min_size(
                egui::pos2(
                    top.center().x - side / 2.0,
                    top.center().y - (side + 2.0 + code_h) / 2.0,
                ),
                Vec2::splat(side),
            )
        } else {
            egui::Rect::from_min_size(
                egui::pos2(
                    top.center().x - (side + label_w) / 2.0,
                    top.center().y - side / 2.0,
                ),
                Vec2::splat(side),
            )
        };
        crate::icons::watch(
            ui.painter(),
            pic,
            here.lying(),
            pal.teal,
            pal.text,
            pal.card,
        );
        let settle = format!(
            "Each change of position settles for {} s (Settle, on the Positions Test pane), \
             and those seconds are left out of the readings.",
            fields::plain(self.guided_settle_s)
        );
        ui.interact(pic, ui.id().with("position picture"), egui::Sense::hover())
            .on_hover_text(if editable {
                format!("{}. {POSITION_HINT} {settle}", here.label())
            } else {
                format!(
                    "{}: shown, not set here. A take's positions are kept as they were; \
                     during a test the test sets them. {settle}",
                    here.label()
                )
            });
        let x = pic.right() + 8.0;
        let painter = ui.painter();
        match label {
            PositionLabel::Beside => {
                painter.text(
                    egui::pos2(x, pic.center().y - 1.0),
                    egui::Align2::LEFT_BOTTOM,
                    here.name(),
                    name_font,
                    pal.text,
                );
                painter.text(
                    egui::pos2(x, pic.center().y + 1.0),
                    egui::Align2::LEFT_TOP,
                    &codes,
                    code_font,
                    pal.text_secondary,
                );
            }
            PositionLabel::Codes => {
                painter.text(
                    egui::pos2(x, pic.center().y),
                    egui::Align2::LEFT_CENTER,
                    &codes,
                    code_font,
                    pal.text_secondary,
                );
            }
            PositionLabel::Under => {
                painter.text(
                    egui::pos2(pic.center().x, pic.bottom() + 2.0),
                    egui::Align2::CENTER_TOP,
                    &codes,
                    code_font,
                    pal.text_secondary,
                );
            }
        }
        // The six buttons, a segmented row across the card, or two.
        let per_row = 6 / rows;
        let mut picked = None;
        for (n, p) in Position::ALL.into_iter().enumerate() {
            let (line, col) = (n / per_row, n % per_row);
            let track = egui::Rect::from_min_size(
                egui::pos2(
                    area.left(),
                    area.bottom() - buttons_h + line as f32 * (POSITION_SEG_H + 4.0),
                ),
                Vec2::new(area.width(), POSITION_SEG_H),
            );
            if col == 0 {
                ui.painter().rect_filled(
                    track,
                    egui::CornerRadius::same(theme::CONTROL_RADIUS + 1),
                    pal.control,
                );
            }
            let w = (track.width() - 4.0) / per_row as f32;
            let r = egui::Rect::from_min_size(
                egui::pos2(track.left() + 2.0 + w * col as f32, track.top() + 2.0),
                Vec2::new(w, track.height() - 4.0),
            )
            .shrink2(Vec2::new(1.0, 0.0));
            let sense = if editable {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let resp = ui.interact(r, ui.id().with(("position", n)), sense);
            let on = p == here;
            let fill = if on {
                pal.teal
            } else if editable && resp.hovered() {
                pal.control_hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter()
                .rect_filled(r, egui::CornerRadius::same(theme::CONTROL_RADIUS), fill);
            let ink = if on {
                if ui.visuals().dark_mode {
                    POSITION_ON_INK
                } else {
                    Color32::WHITE
                }
            } else if editable {
                pal.text
            } else {
                pal.text_tertiary
            };
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                p.code(),
                theme::semibold(12.0),
                ink,
            );
            let tip = format!(
                "{} ({}, {}) · key {}",
                p.name(),
                p.code(),
                p.witschi(),
                n + 1
            );
            let resp = if editable {
                resp.on_hover_text(format!("{tip}. {settle}"))
            } else {
                resp.on_hover_text(format!(
                    "{tip}. Shown, not set here: a take's positions are kept as they were, \
                     and during a test the test sets them."
                ))
            };
            if resp.clicked() {
                picked = Some(p);
            }
        }
        if let Some(p) = picked {
            if p != self.position {
                self.change_position(p);
            }
        }
    }

    /// Listen or Stop and Save, with Average Over under them, on the
    /// Listen card.
    fn listen_buttons(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if self.running() && self.session.mic() {
                if theme::icon_button(ui, Icon::Stop, 26.0, pal.bad, true, false)
                    .on_hover_text(STOP_HINT)
                    .clicked()
                {
                    self.stop_listening();
                }
            } else {
                // With a watch picked, a take of it; else Quick Listen.
                let watch = self.open_watch().map(|e| e.watch.name.clone());
                let hint = match &watch {
                    Some(name) => format!(
                        "Listen to {name}: a new take, saved into it at Stop. For a Quick \
                         Listen with no watch, pick Unsorted Takes in the Watches pane, or \
                         Quick Listen (No Watch) in the Watches menu."
                    ),
                    None => QUICK_LISTEN_HINT.to_string(),
                };
                if ui
                    .add_enabled_ui(!self.running() && self.batch.is_none(), |ui| {
                        theme::icon_button(ui, Icon::Listen, 26.0, pal.accent, true, false)
                    })
                    .inner
                    .on_hover_text(hint)
                    .clicked()
                {
                    if watch.is_some() {
                        self.new_take(NewTake::Listen, false);
                    } else {
                        // Asks only about what isn't Quick Listen's: changed
                        // masks on a recording opened, a watch's unsaved
                        // details.
                        self.quick_listen(false);
                    }
                }
            }
            // A watch's take saves itself at Stop.
            if !self.session.mic() || self.quick_listen {
                self.save_menu(ui);
            }
        });
        ui.horizontal(|ui| {
            let mut avg = self.average_s;
            egui::ComboBox::from_id_salt("card average")
                .width(ui.available_width())
                .selected_text(format!("Average {}", fields::duration(avg)))
                .show_ui(ui, |ui| {
                    for a in AVERAGES {
                        ui.selectable_value(&mut avg, a, fields::duration(a));
                    }
                })
                .response
                .on_hover_text(format!("Average Over: {AVERAGE_REACH}"));
            if avg != self.average_s {
                self.set_average(avg);
            }
        });
    }

    /// Save: what is on show kept in a watch, or as a take in a folder.
    /// Whether the Save menu offers Unsorted Takes: a Quick Listen not
    /// saved yet goes there, as File > Save puts it.
    fn saves_into_unsorted(&self) -> bool {
        self.quick_listen && self.session.saved_to().is_none() && self.watch_takes_dir().is_some()
    }

    fn save_menu(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let can = self.session.allows(Action::Save) && self.saving.is_none();
        let watches: Vec<(String, String)> = match &self.library {
            Ok(Some(l)) if l.read_only.is_none() => l
                .watches
                .iter()
                .map(|e| (e.watch.id.clone(), e.watch.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        let into_watch = self.addable_take().is_some() || self.quick_listen;
        let into_unsorted = can && self.saves_into_unsorted();
        let mut picked = None;
        let (mut new, mut folder, mut unsorted) = (false, false, false);
        ui.add_enabled_ui(can || self.addable_take().is_some(), |ui| {
            let button = theme::icon_button(ui, Icon::Save, 26.0, pal.accent, false, false);
            let saved = self.session.saved_to().is_some();
            egui::Popup::menu(&button)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
                .show(|ui| {
                    if into_unsorted {
                        if ui
                            .button("Unsorted Takes")
                            .on_hover_text(
                                "Keep this in Unsorted Takes, in the Watches pane, to file \
                                 under a watch later",
                            )
                            .clicked()
                        {
                            unsorted = true;
                        }
                        ui.separator();
                    }
                    if into_watch {
                        ui.label(
                            RichText::new("Keep in a Watch")
                                .small()
                                .color(pal.text_secondary),
                        );
                        for (id, name) in &watches {
                            if ui.button(name).clicked() {
                                picked = Some(id.clone());
                            }
                        }
                        if ui
                            .button("New Watch…")
                            .on_hover_text("Add a watch, and keep this in it when you add it")
                            .clicked()
                        {
                            new = true;
                        }
                        ui.separator();
                    }
                    if can
                        && ui
                            .button(if saved {
                                "Save Again"
                            } else {
                                "Save in a Folder…"
                            })
                            .on_hover_text(SAVE_HINT)
                            .clicked()
                    {
                        folder = true;
                    }
                });
            button
                .on_hover_text(
                    "Save: keep this in one of your watches, as a take of it, or as a take \
                     in a folder you choose",
                )
                .on_disabled_hover_text("Nothing to save yet, or a save is under way");
        });
        if let Some(id) = picked {
            self.keep_into(&id);
        }
        if unsorted {
            self.save_again();
        }
        if new {
            self.open_new_watch_form();
            self.keep_after_form = true;
        }
        if folder {
            // Save Again into the folder saved to before; else ask where.
            if self.session.saved_to().is_some() {
                self.save_again();
            } else {
                self.save_recording();
            }
        }
    }

    /// The lift angle, on the Amplitude card: typed or picked, unless the
    /// watch's calibre sets it.
    fn lift_control(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        // On a narrow card, Lift on a line of its own above the rest.
        let narrow = ui.available_width() < LIFT_ROW_W;
        if narrow {
            ui.label(RichText::new("Lift").color(pal.text_secondary));
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if !narrow {
                ui.label(RichText::new("Lift").color(pal.text_secondary));
            }
            if !self.take_shows_rate_and_lift() {
                ui.label(format!("{}°", fields::plain(self.lift_deg)))
                    .on_hover_text("The watch's calibre sets it. Edit… on the watch changes it.");
                return;
            }
            // A calibre picked with a lift angle of its own sets it.
            if !self.lift_settable() {
                ui.label(format!("{}°", fields::plain(self.lift_deg)))
                    .on_hover_text(format!(
                        "The {} calibre sets it. Set Calibre to Not Set to type another.",
                        self.calibre
                    ));
                return;
            }
            let mut lift = self.lift_deg;
            let a = fields::entry(ui, "lift", &mut lift, 10.0..=90.0, &MS, 32.0);
            ui.label("°");
            let b = fields::presets(ui, "lift-presets", &mut lift, &LIFTS, |v| {
                format!("{}°", fields::plain(v))
            });
            if a || b {
                self.set_lift(lift);
            }
            if !self.lift_known() {
                // As much of it as the card has room for.
                let small = egui::TextStyle::Small.resolve(ui.style());
                let tiny = egui::FontId::proportional(9.0);
                let fits = |ui: &egui::Ui, t: &str, font: &egui::FontId| {
                    ui.fonts_mut(|f| f.layout_no_wrap(t.into(), font.clone(), Color32::WHITE))
                        .size()
                        .x
                        <= ui.available_width()
                };
                let word = [
                    ("the default", small.clone()),
                    ("default", small.clone()),
                    ("default", tiny),
                ]
                .into_iter()
                .find(|(t, font)| fits(ui, t, font));
                if let Some((t, font)) = word {
                    ui.label(RichText::new(t).font(font).color(pal.text_secondary))
                        .on_hover_text(LIFT_HINT);
                } else {
                    ui.label(RichText::new("*").color(pal.text_secondary))
                        .on_hover_text(LIFT_HINT);
                }
            }
        });
    }

    /// The beat rate and the calibre, on the Beat Rate card.
    fn beat_rate_controls(&mut self, ui: &mut egui::Ui) {
        if !self.take_shows_rate_and_lift() {
            return self.calibre_line(ui);
        }
        ui.spacing_mut().item_spacing.y = 4.0;
        // Taken before anything is drawn: a control wider than the card
        // would widen what is left.
        let narrow = ui.available_width() < BEAT_RATE_NARROW_W;
        if narrow {
            ui.spacing_mut().button_padding.x = 4.0;
        }
        let pal = theme::pal(ui);
        let mut bph = self.bph;
        let heard = self.live.as_ref().and_then(|l| l.bph());
        // The figure is the menu: the beat rate set, or the one found, as
        // large as the card has room for, with Auto named where it fits;
        // its hover says which always.
        let (figure, colour) = match bph.or(heard) {
            Some(b) => (b.to_string(), pal.text),
            None => ("—".to_string(), pal.text_tertiary),
        };
        // `unit` false leaves "bph" to the card's name and the hover, where
        // the card is too narrow for it.
        let make = |size: f32, auto: bool, unit: bool| {
            let mut job = egui::text::LayoutJob::default();
            job.append(
                &figure,
                0.0,
                egui::TextFormat::simple(theme::display(size), colour),
            );
            if unit {
                job.append(
                    if auto { "bph · Auto" } else { "bph" },
                    6.0,
                    egui::TextFormat::simple(egui::FontId::proportional(13.0), pal.text_secondary),
                );
            }
            job
        };
        // The menu's arrow and padding beside the text.
        let room = ui.available_width() - 2.0 * ui.spacing().button_padding.x - 24.0;
        let wide = |ui: &egui::Ui, job: &egui::text::LayoutJob| {
            ui.fonts_mut(|f| f.layout_job(job.clone())).size().x
        };
        let tries = [
            (FIGURE_PT - 4.0, bph.is_none(), true),
            (FIGURE_PT - 4.0, false, true),
            (FIGURE_PT - 8.0, false, true),
            (FIGURE_PT - 12.0, false, true),
            (FIGURE_PT - 8.0, false, false),
            (FIGURE_PT - 12.0, false, false),
        ];
        let job = tries
            .iter()
            .map(|&(size, auto, unit)| make(size, auto, unit))
            .find(|j| wide(ui, j) <= room)
            .unwrap_or_else(|| make(FIGURE_PT - 16.0, false, false));
        let how = match (bph, heard) {
            (Some(_), _) => {
                "Beats per hour, set here, so the readings use it rather than finding it from \
                 the beats. Auto finds it again."
            }
            (None, Some(_)) => {
                "Beats per hour, found from the first seconds of beats as the nearest \
                 standard rate. Pick a rate to set it, for a watch that beats at an unusual \
                 rate or runs far off."
            }
            (None, None) => {
                "Beats per hour: Auto finds it from the first few seconds of beats. Pick a \
                 rate to set it."
            }
        };
        egui::ComboBox::from_id_salt("bph")
            .width(ui.available_width())
            .height(320.0)
            .selected_text(job)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut bph, None, "Auto: found from the beats");
                for b in STANDARD_BPH {
                    ui.selectable_value(&mut bph, Some(b), format!("{b} bph"));
                }
            })
            .response
            .on_hover_text(how);
        if bph != self.bph {
            self.change_bph(bph);
        }
        if self.open_watch().is_none() {
            let current = self.calibre.clone();
            // The rate Manage Calibres… starts its table at.
            let rate = match self.bph {
                Some(b) => Some((b, "the rate set on the Beat Rate card")),
                None => self
                    .live
                    .as_ref()
                    .and_then(|l| l.bph())
                    .map(|b| (b, "the rate found from the beats")),
            };
            let name = |ui: &mut egui::Ui| {
                ui.label(
                    RichText::new("Calibre")
                        .small()
                        .color(theme::pal(ui).text_secondary),
                );
            };
            // A narrow card puts the menu under its name, so both fit.
            let picked = if narrow {
                name(ui);
                self.cal.picker(ui, &current, rate)
            } else {
                ui.horizontal(|ui| {
                    name(ui);
                    self.cal.picker(ui, &current, rate)
                })
                .inner
            };
            if let Some(p) = picked {
                self.set_calibre(p);
            }
        }
        // A calibre that beats at another rate than the one heard.
        let heard = self.live.as_ref().and_then(|l| l.bph());
        if let Some((b, cb)) = heard
            .zip(self.cal.bph(&self.calibre))
            .filter(|&(b, cb)| b != cb)
        {
            let pal = theme::pal(ui);
            let note = format!(
                "The {} beats at {cb} bph; this watch beats at {b}.",
                self.calibre
            );
            ui.add(
                egui::Label::new(RichText::new(&note).small().color(pal.text_secondary)).truncate(),
            )
            .on_hover_text(note);
        }
    }

    /// A new take of the watch picked: a new session on the microphone,
    /// asking first when there is unsaved sound, then listening, or the
    /// Positions Test or the Power Reserve Test.
    fn new_take(&mut self, kind: NewTake, asked: bool) {
        if self.running() || self.saving_busy() {
            return;
        }
        if !asked && self.needs_save_confirmation() {
            self.confirm = Some(Confirm::NewTake(kind));
            return;
        }
        self.take_setup();
        match kind {
            NewTake::Listen => self.start_microphone(),
            NewTake::Positions => self.start_guided(None),
            NewTake::PowerReserve => self.start_reserve(),
        }
    }

    /// What a new take starts from: an empty session and the open watch's
    /// details. Opens no input, so tests can check it.
    fn take_setup(&mut self) {
        if self.live.is_some() || self.session.recorder.is_some() {
            self.new_session();
        }
        if let Some(w) = self.open_watch().map(|e| e.watch.clone()) {
            // The watch's details over what the microphone had before a
            // recording was opened.
            if let Some(s) = self.mic_setup.take() {
                self.set_watch_setup(s);
            }
            self.quick_listen = false;
            self.apply_watch(&w);
        }
        self.input = Input::Microphone;
    }

    /// Save the unsaved Quick Listen take into the watch `id`, as a take of
    /// it: the watch is picked, and the take named after it.
    fn keep_in_watch(&mut self, id: &str) {
        if self.running() {
            return;
        }
        let Some(name) = (match &self.library {
            Ok(Some(l)) => l.watches.iter().find(|e| e.watch.id == id),
            _ => None,
        })
        .map(|e| e.watch.name.clone()) else {
            return;
        };
        self.watch_open = Some(id.to_string());
        self.unsorted_open = false;
        self.quick_listen = false;
        self.watch = name;
        self.note_settings();
        self.save_again();
    }

    /// Whether the cards offer Beat Rate and Lift Angle: when the
    /// take's watch has no calibre the table knows, or no watch is picked.
    /// Whether the lift angle can be typed: not when a watch's calibre or
    /// a calibre picked in Quick Listen sets it.
    fn lift_settable(&self) -> bool {
        self.take_shows_rate_and_lift() && self.cal.lift(&self.calibre).is_none()
    }

    fn take_shows_rate_and_lift(&self) -> bool {
        self.open_watch().is_none() || self.cal.find(&self.calibre).is_none()
    }

    /// One line for a watch's calibre and what it sets: the beat rate and
    /// the lift angle.
    fn calibre_line(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let heard = self.live.as_ref().and_then(|l| l.bph());
        let bph = match (self.bph, heard) {
            (Some(b), _) => format!("{b} bph"),
            (None, Some(b)) => format!("{b} bph, found"),
            (None, None) => "beat rate found when heard".to_string(),
        };
        // The beat rate and lift show on their own cards: the name is enough,
        // on one line cut short where the card is narrow, so the card keeps
        // its height. A calibre whose beat rate isn't the one heard says its
        // own beside the name, the whole sentence in the hover.
        let other = heard
            .zip(self.cal.bph(&self.calibre))
            .filter(|&(b, cb)| b != cb);
        let line = match other {
            Some((_, cb)) => format!("{} · beats at {cb}", self.calibre),
            None => self.calibre.clone(),
        };
        let mut hover = format!(
            "{}: {bph}, lift {}°. The watch's calibre sets the beat rate and lift angle. \
             Edit… on the watch changes them.",
            self.calibre,
            fields::plain(self.lift_deg)
        );
        if let Some((b, cb)) = other {
            hover = format!(
                "The {} beats at {cb} bph, but this watch beats at {b}.\n\n{hover}",
                self.calibre
            );
        }
        ui.add(egui::Label::new(RichText::new(line).color(pal.text_secondary)).truncate())
            .on_hover_text(hover);
    }

    /// The position picked: a new stretch, noted in the recording.
    fn change_position(&mut self, pos: Position) {
        if self.refuse_faulted_recording() {
            return;
        }
        if self.reserve_running() && pos != self.position {
            self.info_msg(
                "A Power Reserve Test runs in one position: stop the test to change it.".into(),
            );
            return;
        }
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

    /// Seconds of settling still to come now.
    fn settling_left(&self) -> Option<f64> {
        self.session.settling_left(self.latest_s())
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

    /// When stopped inside the settle after a move: when that move was,
    /// the end of the position before it.
    fn stopped_in_settle(&self) -> Option<f64> {
        if self.running() {
            return None;
        }
        let t = self.latest_s();
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

    /// Keep Awake, and what is wrong with keeping the computer awake or
    /// taking sound in the background.
    fn keep_awake_setting(&mut self, ui: &mut egui::Ui) {
        theme::switch_row(
            ui,
            "Keep Awake",
            &mut self.keep_awake,
            "While listening, stop the computer going to sleep on its own, so a long take \
             isn't cut short. The screen can still lock and switch off.",
        );
        let pal = theme::pal(ui);
        if let (true, Some(e)) = (self.keep_awake, &self.awake_fault) {
            ui.label(
                RichText::new(format!(
                    "The computer isn't being kept awake: {e}. It may sleep on its own \
                     during a long take."
                ))
                .small()
                .color(pal.warn),
            );
        }
        if let Some(e) = &self.pump_fault {
            ui.label(
                RichText::new(format!(
                    "Sound is taken only while the window is showing, and the computer \
                     isn't kept awake: the thread that does both didn't start ({e})."
                ))
                .small()
                .color(pal.warn),
            );
        }
    }

    fn strip_settings(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        theme::tool_field(
            ui,
            "Width",
            "Milliseconds either side of the centre line. Shift or Ctrl and the mouse wheel \
             on the strip change it too.",
            |ui| {
                ui.label("±");
                let mut w = self.strip.half_width_ms;
                let a = fields::entry(
                    ui,
                    "width",
                    &mut w,
                    StripView::MIN_HALF_WIDTH_MS..=StripView::MAX_HALF_WIDTH_MS,
                    &MS,
                    40.0,
                );
                ui.label("ms");
                let b = fields::presets(ui, "width-presets", &mut w, &WIDTHS, |v| {
                    if v >= 1000.0 {
                        format!("±{} s", fields::plain(v / 1000.0))
                    } else {
                        format!("±{} ms", fields::plain(v))
                    }
                });
                if a || b {
                    self.strip.set_half_width(w);
                }
            },
        );
        theme::tool_field(
            ui,
            "Length",
            "Time shown along the strip, such as 30 s, 5 min or 2 h. Fit shows the whole \
             session so far, up to its latest beat, or its last 24 hours when it is longer. \
             The mouse wheel on the strip changes it too.",
            |ui| {
                let mut s = self.strip.span_s;
                let a = fields::entry(
                    ui,
                    "span",
                    &mut s,
                    StripView::MIN_SPAN_S..=StripView::MAX_SPAN_S,
                    &DURATION,
                    48.0,
                );
                let b = fields::presets(ui, "span-presets", &mut s, &SPANS, span_label);
                // Fitted to the take on show: a length for the take only.
                if self
                    .live_strip
                    .is_some_and(|(_, fitted)| fitted == self.strip)
                {
                    ui.label(RichText::new("Fit").small().color(pal.text_secondary))
                        .on_hover_text(
                            "Fitted to the take on show. Listen brings back the strip's own \
                             width and length.",
                        );
                }
                if b && s.is_infinite() {
                    self.fit_strip();
                } else if a || b {
                    self.strip.set_span(s);
                }
            },
        );
        theme::bar_gap(ui);
        theme::tool_switch(
            ui,
            Icon::RateLine,
            pal.trace_rate,
            "Rate Line",
            &mut self.rate_line,
            "Draw the Rate reading as a red line over the beats it was fitted to, to check \
             that it follows the dots",
        );
        theme::tool_switch(
            ui,
            Icon::Rails,
            pal.trace_rate,
            "Parallel Guides",
            &mut self.rate_guides,
            "Faint lines at the Rate reading's slope across the whole strip, so it is easy \
             to see whether the dots run parallel to it. They show with or without the Rate \
             Line.",
        );
        theme::bar_gap(ui);
        if theme::tool_button(
            ui,
            Icon::Truing,
            "Centre",
            "Put the latest beats on the centre line",
            None,
        ) {
            self.centre();
        }
        theme::tool_switch(
            ui,
            Icon::AutoCentre,
            pal.accent,
            "Auto-Centre",
            &mut self.follow,
            "Keep the newest beats on the centre line and slide the rest",
        );
        theme::bar_gap(ui);
        let live = self.live.is_some();
        theme::note_icon(ui, Icon::Masks, "Mask");
        theme::bar_item(ui, "Mask", |ui| {
            ui.add_enabled_ui(live, |ui| {
                if theme::in_toolbar(ui) {
                    let r = theme::icon_toggle(ui, Icon::Masks, self.masking, pal.accent);
                    if r.clicked() {
                        self.masking = !self.masking;
                    }
                    r.on_hover_text(format!("Mask: {MASK_HINT}"))
                        .on_disabled_hover_text(
                            "Mask: Nothing to mask yet. Listen, or open a recording.",
                        );
                } else {
                    theme::switch_row(ui, "Mask", &mut self.masking, MASK_HINT);
                }
            });
        });
        let unmask_hint = if self.masked.is_empty() {
            "Bring every masked stretch back into the readings".to_string()
        } else {
            let total: f64 = self.masked.iter().map(|m| m.1 - m.0).sum();
            format!(
                "Bring every masked stretch back into the readings. {} {} masked, {} in all.",
                self.masked.len(),
                if self.masked.len() == 1 {
                    "stretch"
                } else {
                    "stretches"
                },
                strip::fmt_time(total)
            )
        };
        let nothing = self.masked.is_empty().then_some("Nothing is masked");
        if theme::tool_button(ui, Icon::Unmask, "Unmask All", &unmask_hint, nothing) {
            self.set_masked(Vec::new());
        }
        theme::bar_gap(ui);
        let mut h = self.strip.horizontal;
        theme::tool_choice(
            ui,
            "Direction",
            "Which way time runs along the strip",
            &mut h,
            &[
                (
                    false,
                    Icon::ArrowDown,
                    "Down",
                    "Newest beats at the top, the strip beside the charts",
                ),
                (
                    true,
                    Icon::ArrowRight,
                    "Across",
                    "Newest beats at the right, the strip above the charts",
                ),
            ],
        );
        if h != self.strip.horizontal {
            self.strip.horizontal = h;
            self.relayout(h);
        }
        let overlays = &mut self.overlays;
        for (icon, name) in [
            (Icon::Rate, "Rate"),
            (Icon::Amplitude, "Amplitude"),
            (Icon::BeatError, "Beat Error"),
        ] {
            theme::note_icon(ui, icon, &format!("Draw Over the Strip: {name}"));
        }
        theme::bar_item(ui, "Draw Over the Strip", |ui| {
            let over = "Readings drawn as lines over the beats against the same time, each \
                        on its own scale";
            // Rate, Amplitude, Beat Error, as everywhere; `overlays` keeps
            // its own order (amplitude, rate, beat error) for old settings.
            let lines = [
                (
                    1,
                    Icon::Rate,
                    pal.trace_rate,
                    "Rate",
                    "The rate readings as a red line against the same time, on their own \
                     scale",
                ),
                (
                    0,
                    Icon::Amplitude,
                    pal.trace_amplitude,
                    "Amplitude",
                    "The amplitude readings as a purple line against the same time, on \
                     their own scale",
                ),
                (
                    2,
                    Icon::BeatError,
                    pal.trace_beat_error,
                    "Beat Error",
                    "The beat error readings, from the unlock, as a yellow line against the \
                     same time, on their own scale",
                ),
            ];
            if theme::in_toolbar(ui) {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, icon, colour, name, hint) in lines {
                    if theme::icon_toggle(ui, icon, overlays[i], colour)
                        .on_hover_text(format!("Draw Over the Strip: {name}: {hint}"))
                        .clicked()
                    {
                        overlays[i] = !overlays[i];
                    }
                }
            } else {
                ui.label(RichText::new("Draw Over the Strip").strong())
                    .on_hover_text(over);
                for (i, _, _, name, hint) in lines {
                    theme::switch_row(ui, name, &mut overlays[i], hint);
                }
            }
        });
        // Average Over's name in words sets it apart: no line before it.
        self.pane_average_setting(ui, Pane::Strip);
        self.export_button(ui, Pane::Strip);
    }

    fn profile_settings(&mut self, ui: &mut egui::Ui) {
        let accent = theme::pal(ui).accent;
        let o = &mut self.sound_opts;
        theme::tool_choice(
            ui,
            "Layout",
            "Tick above tock, or side by side as tg shows them",
            &mut o.side_by_side,
            &[
                (false, Icon::Stacked, "Stacked", "Tick above tock"),
                (
                    true,
                    Icon::Beside,
                    "Beside",
                    "Tick on the left, tock on the right, as tg shows them",
                ),
            ],
        );
        theme::tool_choice(
            ui,
            "Scale",
            "How the loudness is drawn",
            &mut o.scale,
            &[
                (
                    profiles::Scale::Linear,
                    Icon::Ruler,
                    "Linear",
                    "The sound's loudness as the engine measures it",
                ),
                (
                    profiles::Scale::Decibels,
                    Icon::Log,
                    "dB",
                    "Decibels below the loudest point, which makes the quiet unlock easier \
                     to see",
                ),
            ],
        );
        theme::bar_gap(ui);
        theme::tool_switch(
            ui,
            Icon::PalletFork,
            accent,
            "Edges",
            &mut o.edges,
            "The solid unlock, drop and peak lines: the edges amplitude and the beat error \
             are read from",
        );
        theme::tool_switch(
            ui,
            Icon::Notes,
            accent,
            "Sounds 1, 2 and 3",
            &mut o.sounds,
            "The dashed brown lines where the three sounds of each beat rise: unlock, \
             impulse and drop",
        );
        theme::bar_gap(ui);
        theme::tool_choice(
            ui,
            "Time From",
            "Where each side's time is measured from",
            &mut o.shared_clock,
            &[
                (
                    true,
                    Icon::OneClock,
                    "Shared",
                    "Tick and tock on one clock: the drops stand the beat error from the \
                     drop apart, and the unlocks the beat error from the unlock",
                ),
                (
                    false,
                    Icon::TwoClocks,
                    "Own Drop",
                    "Each side from its own drop, so both drops sit at 0 ms",
                ),
            ],
        );
        theme::bar_gap(ui);
        self.pane_average_setting(ui, Pane::Sound);
        self.export_button(ui, Pane::Sound);
    }

    /// The menus across the very top, File, Edit, View and Help, and beside
    /// them an icon for each pane that shows or hides it.
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| self.file_menu(ui));
            ui.menu_button("Edit", |ui| self.edit_menu(ui));
            ui.menu_button("View", |ui| self.view_menu(ui));
            ui.menu_button("Watches", |ui| self.watches_menu(ui));
            ui.menu_button("Help", |ui| self.help_menu(ui));
            ui.add_space(16.0);
            self.pane_icons(ui);
        });
    }

    /// The Watches menu: the pane, New Watch…, and each watch, to pick.
    fn watches_menu(&mut self, ui: &mut egui::Ui) {
        let on = pane_visible(&self.panes.tiles, Pane::Watch);
        if ui
            .button(if on { "Hide Watches" } else { "Show Watches" })
            .on_hover_text(Pane::Watch.hint())
            .clicked()
        {
            ui.close();
            set_pane_visible(&mut self.panes.tiles, Pane::Watch, !on);
        }
        if ui
            .button("New Watch…")
            .on_hover_text("Add a watch: its name, calibre, lift angle and beat rate")
            .clicked()
        {
            ui.close();
            set_pane_visible(&mut self.panes.tiles, Pane::Watch, true);
            self.open_new_watch_form();
        }
        let watches: Vec<(String, String)> = match &self.library {
            Ok(Some(l)) => l
                .watches
                .iter()
                .map(|e| (e.watch.id.clone(), e.watch.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        ui.separator();
        if ui
            .add_enabled(
                !self.running(),
                egui::Button::new("Quick Listen (No Watch)"),
            )
            .on_hover_text(QUICK_LISTEN_HINT)
            .on_disabled_hover_text("Stop listening first")
            .clicked()
        {
            ui.close();
            self.quick_listen(false);
        }
        for (id, name) in watches {
            let picked = self.watch_open.as_deref() == Some(id.as_str());
            if ui
                .selectable_label(picked, &name)
                .on_hover_text("Show this watch and its takes in the Watches pane")
                .clicked()
            {
                ui.close();
                set_pane_visible(&mut self.panes.tiles, Pane::Watch, true);
                self.pick_watch(&id);
            }
        }
    }

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        let command = |key| egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key);
        if ui
            .add(
                egui::Button::new("Open Recording…")
                    .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::O))),
            )
            .on_hover_text(
                "Choose a WAV or FLAC recording and analyse it all. From outside your \
                 watches, it asks first whether to add it to one of them.",
            )
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
        let saving = self.saving.is_some();
        if self.masks_unsaved() {
            if ui
                .add(
                    egui::Button::new("Save")
                        .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::S))),
                )
                .on_hover_text(SAVE_MASKS_HINT)
                .clicked()
            {
                ui.close();
                self.save_masks();
            }
        } else if ui
            .add_enabled(
                can_save && !saving,
                egui::Button::new(if saved { "Save Again" } else { "Save…" })
                    .shortcut_text(ui.ctx().format_shortcut(&command(egui::Key::S))),
            )
            .on_hover_text(SAVE_HINT)
            .on_disabled_hover_text(if saving {
                SAVING_HINT
            } else {
                "Nothing to save yet: listen to a watch first"
            })
            .clicked()
        {
            ui.close();
            self.save_again();
        }
        if ui
            .add_enabled(
                can_save && saved && !saving,
                egui::Button::new("Save As…")
                    .shortcut_text(ui.ctx().format_shortcut(&save_as_shortcut())),
            )
            .on_hover_text(SAVE_AS_HINT)
            .on_disabled_hover_text(if saving {
                SAVING_HINT
            } else {
                "Save once first; Save… asks where"
            })
            .clicked()
        {
            ui.close();
            self.save_recording();
        }
        if ui
            .add_enabled(
                self.can_save_analysis(),
                egui::Button::new("Save Analysis Only…"),
            )
            .on_hover_text(ANALYSIS_ONLY_HINT)
            .on_disabled_hover_text(if self.analysis_save.is_some() {
                "The analysis is being kept now"
            } else if can_save {
                "Stop listening first: the whole take's sound is read again"
            } else {
                "Nothing to save yet: listen to a watch first"
            })
            .clicked()
        {
            ui.close();
            self.save_analysis_only();
        }
        // Anything shown, from the microphone or a recording, can be
        // cleared, listening or not.
        if ui
            .add_enabled(self.live.is_some(), egui::Button::new("New Session"))
            .on_hover_text(NEW_SESSION_HINT)
            .on_disabled_hover_text(
                "Nothing to clear yet: listen to a watch or open a recording first",
            )
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

    fn edit_menu(&mut self, ui: &mut egui::Ui) {
        if ui
            .add(
                egui::Button::new("Settings…")
                    .shortcut_text(ui.ctx().format_shortcut(&settings_shortcut())),
            )
            .on_hover_text(
                "How the app looks, the calibres and each input's kept clock, in a window \
                 of their own",
            )
            .clicked()
        {
            ui.close();
            self.open_settings(self.settings_last);
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
                self.show_pane(pane, on);
            }
        };
        for pane in Pane::ALL {
            if !Pane::ANALYSIS.contains(&pane) {
                pane_box(ui, pane);
            }
        }
        ui.menu_button("Analysis", |ui| {
            for pane in Pane::ANALYSIS {
                pane_box(ui, pane);
            }
        });
        ui.separator();
        if ui
            .button("Reset Panes")
            .on_hover_text("Put the panes back as the app starts: the readings, the Paper Strip and the Tick Tock Profile, every other pane hidden")
            .clicked()
        {
            ui.close();
            self.show_readings = true;
            self.panes = default_layout(self.strip.horizontal);
        }
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
            match self.open_ask_for(&p) {
                Some(a) => self.open_ask = Some(a),
                None => self.open_recording(&p, true),
            }
        }
    }

    /// The question Open asks for `path`, when there are watches to add it
    /// to and it isn't in them already. None means open it straight away
    /// (a recording that can't be read says why when opened).
    fn open_ask_for(&self, path: &Path) -> Option<OpenAsk> {
        let Ok(Some(l)) = &self.library else {
            return None;
        };
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        if l.read_only.is_some() || canon(path).starts_with(canon(&l.root)) {
            return None;
        }
        let inner = take_folder(path).ok()?;
        // A folder's session file speaks for all of it, each recording
        // listed or not; a lone file's only through an entry listing it.
        let manifest = if inner.is_dir() && inner.join(MANIFEST).is_file() {
            let text = std::fs::read_to_string(inner.join(MANIFEST)).ok()?;
            let m: Manifest = toml::from_str(&text).ok()?;
            let entries = m.recordings.clone();
            Some((m, entries))
        } else {
            read_manifest(&inner).ok()?
        };
        let files = recordings_in(&inner).unwrap_or_default();
        let named =
            position_in_name(&inner).or_else(|| files.first().and_then(|f| position_in_name(f)));
        let dir = if inner.is_dir() {
            inner.clone()
        } else {
            inner.parent()?.to_path_buf()
        };
        // A lone file's folder speaks for it only through a session file
        // listing it.
        let started = (manifest.is_some() || inner.is_dir()) && started_in(&dir).is_some();
        let has = match &manifest {
            Some((m, entries)) => OpenHas {
                manifest: true,
                bph: m.bph,
                lift: m.lift,
                calibre: m.calibre.clone().filter(|c| !c.is_empty()),
                position: named.is_some() || entries.iter().all(|e| e.position.is_some()),
                started,
            },
            None => OpenHas {
                position: named.is_some(),
                started,
                ..Default::default()
            },
        };
        let watch = self
            .watch_open
            .clone()
            .filter(|id| l.watches.iter().any(|e| &e.watch.id == id));
        Some(OpenAsk {
            path: path.to_path_buf(),
            add: watch.is_some(),
            new_watch: l.watches.is_empty(),
            watch,
            new_name: String::new(),
            has,
            bph: None,
            lift: DEFAULT_LIFT_DEG,
            lift_set: false,
            position: named,
            started: started_guess(&files)
                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default(),
            error: None,
            keeping: false,
        })
    }

    /// What adding `a` to its watch asks to be filled in: what neither the
    /// recording's files nor the watch and its calibre say.
    fn open_need(&self, a: &OpenAsk) -> OpenNeed {
        let w = match (&self.library, &a.watch) {
            (Ok(Some(l)), Some(id)) if !a.new_watch => l
                .watches
                .iter()
                .find(|e| &e.watch.id == id)
                .map(|e| &e.watch),
            _ => None,
        };
        let calibre = a
            .has
            .calibre
            .clone()
            .or_else(|| w.and_then(|w| w.calibre.clone()))
            .filter(|c| !c.is_empty());
        let of_calibre = |f: &dyn Fn(&str) -> bool| calibre.as_deref().is_some_and(f);
        OpenNeed {
            bph: a.has.bph.is_none()
                && w.and_then(|w| w.bph).is_none()
                && !of_calibre(&|c| self.cal.bph(c).is_some()),
            lift: a.has.lift.is_none()
                && w.and_then(|w| w.lift).is_none()
                && !of_calibre(&|c| self.cal.lift(c).is_some()),
            position: !a.has.position,
            started: !a.has.started,
        }
    }

    /// The window Open asks in, while it is open.
    /// Set Up the Microphone, at the first launch: which microphone, and
    /// its level set against the watch on the stand, listening all the
    /// while so the meter moves. Done keeps listening; Not Now stops and
    /// asks again at the next launch.
    fn setup_sheet_dialog(&mut self, ctx: &egui::Context) {
        if !self.setup_sheet || self.open_ask.is_some() || self.confirm.is_some() {
            return;
        }
        // Listen as soon as the sheet shows, once: a failure says why in
        // the banner, and choosing another microphone tries again.
        if !self.setup_tried && !self.running() {
            self.setup_tried = true;
            self.quick_listen(true);
        }
        let before = self.device.clone();
        let (mut done, mut later) = (false, false);
        egui::Modal::new(egui::Id::new("set up the microphone")).show(ctx, |ui| {
            ui.set_width(440.0);
            let pal = theme::pal(ui);
            ui.label(RichText::new("Set Up the Microphone").font(theme::semibold(15.0)));
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Choose the microphone the watch sits on, put a ticking watch on it, and \
                     set the Input Level while the Peak meter moves. The shaded band is \
                     between -10 and -6 dBFS; at 0 dBFS the sound clips and the meter turns \
                     red. From then on the app listens as soon as it opens (Settings > \
                     General), and Settings > Microphone changes these.",
                )
                .color(pal.text_secondary),
            );
            ui.add_space(8.0);
            self.microphone_card(ui);
            // Why the microphone didn't start, here rather than in the
            // banner the sheet covers.
            if let Some((m, true)) = &self.message {
                ui.add_space(4.0);
                ui.label(RichText::new(m).color(pal.bad));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                done = ui
                    .add(theme::primary(ui, "Done"))
                    .on_hover_text("Keep listening to this microphone, with no watch picked")
                    .clicked();
                later = ui
                    .add(theme::secondary(ui, "Not Now"))
                    .on_hover_text(
                        "Stop listening and close this; it asks again the next time the app \
                         opens",
                    )
                    .clicked();
            });
        });
        if self.device != before && self.running() && self.session.mic() {
            // Another microphone chosen: listen to that one instead.
            self.stop_listening();
            self.quick_listen(true);
        }
        if later {
            self.setup_sheet = false;
            self.message = None;
            if self.running() && self.session.mic() {
                self.stop_listening();
            }
        }
        if done {
            self.setup_sheet = false;
            self.mic_set_up = true;
            if !self.running() {
                self.quick_listen(true);
            }
        }
    }

    /// A test that ended with no watch picked: Keep in Unsorted, Keep in a
    /// Watch… or Throw Away.
    fn test_keep_dialog(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.test_keep else {
            return;
        };
        if self.watch_form.is_some() || self.confirm.is_some() {
            return;
        }
        let watches: Vec<(String, String)> = match &self.library {
            Ok(Some(l)) if l.read_only.is_none() => l
                .watches
                .iter()
                .map(|e| (e.watch.id.clone(), e.watch.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        let name = match kind {
            TestKeep::Positions => "Positions Test",
            TestKeep::PowerReserve => "Power Reserve Test",
        };
        let mut answer: Option<Option<Option<String>>> = None;
        let mut new = false;
        // At the top or foot of the window, whichever is clear of the
        // test's pane, over a light backdrop, so its results stay in view
        // while it asks.
        let id = egui::Id::new("keep the test");
        let pane = self.results_rects[match kind {
            TestKeep::Positions => 0,
            TestKeep::PowerReserve => 1,
        }];
        let screen = ctx.content_rect();
        let anchor = match pane {
            Some(r) if r.center().y < screen.center().y => {
                (egui::Align2::CENTER_BOTTOM, [0.0, -48.0])
            }
            _ => (egui::Align2::CENTER_TOP, [0.0, 48.0]),
        };
        let area = egui::Modal::default_area(id).anchor(anchor.0, anchor.1);
        egui::Modal::new(id)
            .area(area)
            .backdrop_color(egui::Color32::from_black_alpha(40))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                let pal = theme::pal(ui);
                ui.label(RichText::new(format!("Keep This {name}?")).font(theme::semibold(15.0)));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "No watch was picked, so this {name} isn't kept anywhere yet. Unsorted \
                     holds it until you file it under a watch or delete it."
                    ))
                    .color(pal.text_secondary),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(theme::primary(ui, "Keep in Unsorted"))
                        .on_hover_text(
                            "Keep the take in Unsorted, in the Watches pane, to file under a \
                         watch later",
                        )
                        .clicked()
                    {
                        answer = Some(Some(None));
                    }
                    let keep = ui
                        .add(theme::secondary(ui, "Keep in a Watch…"))
                        .on_hover_text(
                            "Keep the take in one of your watches. It keeps the settings it ran \
                         with; the watch's own apply to its next take.",
                        );
                    egui::Popup::menu(&keep).show(|ui| {
                        for (id, name) in &watches {
                            if ui.button(name).clicked() {
                                answer = Some(Some(Some(id.clone())));
                            }
                        }
                        if !watches.is_empty() {
                            ui.separator();
                        }
                        if ui
                            .button("New Watch…")
                            .on_hover_text("Add a watch, and keep the take in it when you add it")
                            .clicked()
                        {
                            new = true;
                        }
                    });
                    if ui
                        .add(theme::secondary(ui, "Throw Away"))
                        .on_hover_text("Delete the take: its sound and its readings")
                        .clicked()
                    {
                        answer = Some(None);
                    }
                });
            });
        if new {
            self.open_new_watch_form();
            self.keep_after_form = true;
            return;
        }
        match answer {
            Some(Some(watch)) => {
                self.test_keep = None;
                self.keep_test(kind, watch.as_deref());
            }
            Some(None) => {
                self.test_keep = None;
                self.new_session();
            }
            None => {}
        }
    }

    /// Keep the test's take in the watch `watch`, or in Unsorted with none.
    fn keep_test(&mut self, kind: TestKeep, watch: Option<&str>) {
        self.test_keep = None;
        match (kind, watch) {
            (TestKeep::Positions, Some(id)) => self.keep_in_watch(id),
            (TestKeep::Positions, None) => {
                self.watch_open = None;
                self.unsorted_open = true;
                self.save_again();
            }
            (TestKeep::PowerReserve, watch) => {
                self.watch_open = watch.map(str::to_string);
                self.unsorted_open = watch.is_none();
                // The take is named for the watch it goes into.
                if let Some(e) = self.open_watch() {
                    self.watch = e.watch.name.clone();
                }
                self.save_reserve();
            }
        }
    }

    fn open_ask_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut a) = self.open_ask.take() else {
            return;
        };
        let watches: Vec<(String, String)> = match &self.library {
            Ok(Some(l)) => l
                .watches
                .iter()
                .map(|e| (e.watch.id.clone(), e.watch.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        let need = self.open_need(&a);
        let started = parse_local_minute(&a.started);
        let ready = !a.add
            || ((a.watch.is_some() && !a.new_watch)
                || (a.new_watch && !a.new_name.trim().is_empty()))
                && (!need.bph || a.bph.is_some())
                && (!need.position || a.position.is_some())
                && (!need.started || started.is_some());
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::new("open question")).show(ctx, |ui| {
            ui.set_width(440.0);
            let pal = theme::pal(ui);
            let title = if a.keeping {
                format!("Keep \"{}\" in a Watch", file_name(&a.path))
            } else {
                format!("Open \"{}\"", file_name(&a.path))
            };
            ui.label(RichText::new(title).font(theme::semibold(15.0)));
            // Already on show: only keeping it is asked about.
            if !a.keeping {
                theme::segmented(
                    ui,
                    &mut a.add,
                    &[
                        (
                            true,
                            "Add to a Watch",
                            "Copy the recording into one of your watches, where it is kept with \
                         the watch's takes. The original stays where it is.",
                        ),
                        (
                            false,
                            "Just Open It",
                            "Open the recording where it is. It is not added to your watches: \
                         Save, on the Listen card, keeps it in a watch later.",
                        ),
                    ],
                );
            }
            if a.add {
                let shown = if a.new_watch {
                    "New Watch…"
                } else {
                    a.watch
                        .as_ref()
                        .and_then(|id| watches.iter().find(|(w, _)| w == id))
                        .map_or("Choose a Watch", |(_, n)| n.as_str())
                };
                theme::row(
                    ui,
                    "Watch",
                    Some("The watch the recording is added to"),
                    |ui| {
                        egui::ComboBox::from_id_salt("open into watch")
                            .width(ui.available_width())
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                for (id, name) in &watches {
                                    let on = !a.new_watch && a.watch.as_ref() == Some(id);
                                    if ui.selectable_label(on, name).clicked() {
                                        a.watch = Some(id.clone());
                                        a.new_watch = false;
                                    }
                                }
                                if ui.selectable_label(a.new_watch, "New Watch…").clicked() {
                                    a.new_watch = true;
                                }
                            });
                    },
                );
                if a.new_watch {
                    theme::row(
                        ui,
                        "Name",
                        Some(
                            "The new watch's name. Its other details can be added later \
                             with Edit… in the Watches pane.",
                        ),
                        |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut a.new_name)
                                    .desired_width(f32::INFINITY),
                            );
                        },
                    );
                }
                if need == OpenNeed::default() {
                    ui.label(
                        RichText::new("Its files and the watch say everything a take needs.")
                            .color(pal.text_secondary),
                    );
                } else {
                    ui.label(
                        RichText::new(
                            "Neither its files nor the watch say these, so they are saved \
                             with it:",
                        )
                        .color(pal.text_secondary),
                    );
                }
                if need.bph {
                    theme::row(ui, "Beat Rate", Some("Beats per hour"), |ui| {
                        egui::ComboBox::from_id_salt("open bph")
                            .width(ui.available_width())
                            .selected_text(match a.bph {
                                Some(b) => format!("{b} bph"),
                                None => "Choose a Beat Rate".into(),
                            })
                            .show_ui(ui, |ui| {
                                for b in STANDARD_BPH {
                                    ui.selectable_value(&mut a.bph, Some(b), format!("{b} bph"));
                                }
                            });
                    });
                }
                if need.lift {
                    let changed = theme::row(
                        ui,
                        "Lift Angle",
                        Some(
                            "The calibre's lift angle in degrees: type it and press Enter, or \
                             pick a common one from the menu. Amplitude depends on it.",
                        ),
                        |ui| {
                            let x =
                                fields::entry(ui, "open lift", &mut a.lift, 10.0..=90.0, &MS, 44.0);
                            ui.label("°");
                            let y = fields::presets(
                                ui,
                                "open lift presets",
                                &mut a.lift,
                                &LIFTS,
                                |v| format!("{}°", fields::plain(v)),
                            );
                            x || y
                        },
                    );
                    a.lift_set |= changed;
                }
                if need.position {
                    theme::row(
                        ui,
                        "Position",
                        Some("The position the watch was in"),
                        |ui| {
                            egui::ComboBox::from_id_salt("open position")
                                .width(ui.available_width())
                                .selected_text(
                                    a.position.map_or("Choose a Position", |p| p.label()),
                                )
                                .show_ui(ui, |ui| {
                                    for p in Position::ALL {
                                        ui.selectable_value(&mut a.position, Some(p), p.label());
                                    }
                                });
                        },
                    );
                }
                if need.started {
                    theme::row(
                        ui,
                        "Started",
                        Some(
                            "When the recording started, in this computer's time, as \
                             2026-10-22 14:30. Filled in from when its file was last written, \
                             less its length: correct it if the file was copied since.",
                        ),
                        |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut a.started)
                                    .desired_width(f32::INFINITY),
                            );
                        },
                    );
                    if started.is_none() {
                        ui.label(
                            RichText::new("Write it as 2026-10-22 14:30.")
                                .small()
                                .color(pal.text_secondary),
                        );
                    }
                }
            }
            if let Some(e) = &a.error {
                ui.label(RichText::new(e).color(pal.bad));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let go = match (a.keeping, a.add) {
                    (true, _) => "Keep",
                    (false, true) => "Add and Open",
                    (false, false) => "Open",
                };
                if ui
                    .add_enabled(ready, theme::primary(ui, go))
                    .on_hover_text("Go ahead")
                    .on_disabled_hover_text("Fill in what is asked for first")
                    .clicked()
                {
                    answer = Some(true);
                }
                if ui
                    .add(theme::secondary(ui, "Cancel"))
                    .on_hover_text("Close without opening anything")
                    .clicked()
                {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        match answer {
            Some(true) => self.answer_open_ask(a, need),
            Some(false) => {}
            None => self.open_ask = Some(a),
        }
    }

    /// Open `a`'s recording where it is, or add it to its watch first and
    /// open the take it makes there. A failed add leaves the question open,
    /// saying why.
    fn answer_open_ask(&mut self, mut a: OpenAsk, need: OpenNeed) {
        if !a.add {
            return self.open_recording(&a.path, true);
        }
        match self.add_opened(&a, need) {
            Ok((take, id, fill_error)) => {
                self.watch_open = Some(id);
                self.unsorted_open = false;
                match fill_error {
                    Some(e) => self.error(e),
                    None => self.info_msg(format!(
                        "Added \"{}\" to the watch's takes.",
                        file_name(&take)
                    )),
                }
                self.open_recording(&take, true);
            }
            Err(e) => {
                a.error = Some(e);
                self.open_ask = Some(a);
            }
        }
    }

    /// Copy `a`'s recording into its watch (a new one made first, when
    /// asked for), with what `need` says is missing written into its
    /// session files. Returns the take, the watch's id, and why what was
    /// missing couldn't be written, when the take is in but that failed.
    fn add_opened(
        &mut self,
        a: &OpenAsk,
        need: OpenNeed,
    ) -> Result<(PathBuf, String, Option<String>), String> {
        let inner = take_folder(&a.path)?;
        let files = recordings_in(&inner)?;
        let started = if need.started {
            Some(recorder::utc(
                parse_local_minute(&a.started)
                    .ok_or("Write when it started as 2026-10-22 14:30.")?,
            ))
        } else {
            None
        };
        let fill = Fill {
            bph: if need.bph { a.bph } else { None },
            lift: need.lift.then_some((a.lift, a.lift_set)),
            position: if need.position { a.position } else { None },
            started_utc: started,
            duration_s: files.iter().filter_map(|f| audio_length(f)).sum(),
        };
        let Ok(Some(lib)) = &mut self.library else {
            return Err("There is no Watches folder.".into());
        };
        let id = if a.new_watch {
            let w = library::Watch::new(&a.new_name);
            lib.add(&w)?;
            w.id
        } else {
            a.watch.clone().ok_or("Choose a watch first.")?
        };
        let watch = lib
            .watches
            .iter()
            .find(|e| e.watch.id == id)
            .map(|e| e.watch.clone())
            .ok_or("That watch is no longer in your watches")?;
        if a.has.manifest {
            let dir = if inner.is_dir() {
                inner.clone()
            } else {
                inner
                    .parent()
                    .ok_or("The recording has no folder")?
                    .to_path_buf()
            };
            let (take, _) = lib.add_take(&dir, &id, true)?;
            let filled = fill_take(&take, &watch, &fill).err().map(|e| {
                format!("The take is in the watch, but what it was missing couldn't be saved with it: {e}")
            });
            lib.scan();
            return Ok((take, id, filled));
        }
        let named =
            position_in_name(&inner).or_else(|| files.first().and_then(|f| position_in_name(f)));
        let position = fill.position.or(named);
        let lift = fill.lift.or(watch.lift.map(|l| (l, true)));
        let m = Manifest {
            watch: Some(watch.name.clone()),
            calibre: watch.calibre.clone().filter(|c| !c.is_empty()),
            bph: fill.bph.or(watch.bph),
            lift: lift.map(|l| l.0),
            lift_known: lift.map(|l| l.1),
            recordings: vec![RecordingEntry {
                file: ".".into(),
                position: position.map(|p| p.code().into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let toml = toml::to_string_pretty(&m).map_err(|e| e.to_string())?;
        let dir_json = inner.is_dir().then(|| inner.join("session.json"));
        let json = with_start(dir_json.as_deref(), &fill)?;
        let started = fill
            .started_utc
            .clone()
            .or_else(|| started_in(&inner))
            .unwrap_or_default();
        let minute = watches::local_minute(&started).unwrap_or_else(|| started.replace(':', "."));
        let name = library::take_name(
            &minute,
            &position
                .and_then(|p| timegrapher_core::session::Position::parse(p.code()))
                .into_iter()
                .collect::<Vec<_>>(),
        );
        let take = lib.add_recording(
            &inner,
            &id,
            &name,
            &[(MANIFEST, toml), ("session.json", json)],
        )?;
        Ok((take, id, None))
    }

    /// A button for each pane, its icon pressed while the pane shows.
    fn pane_icons(&mut self, ui: &mut egui::Ui) {
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
                 as `timegrapher series`, and how the rate moves with the amplitude: each \
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
                let (icon, color) = pane_icon(pane, pal);
                if theme::icon_toggle(ui, icon, on, color)
                    .on_hover_text(format!("{}: {}", pane.title(), pane.hint()))
                    .clicked()
                {
                    self.show_pane(pane, !on);
                }
            }
        }
        ui.add_space(6.0);
        ui.separator();
        if theme::icon_toggle(
            ui,
            theme::Icon::Settings,
            self.settings_tab.is_some(),
            pal.accent,
        )
        .on_hover_text("Settings: how the app looks, the calibres and each input's kept clock")
        .clicked()
        {
            self.open_settings(self.settings_last);
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
        // The band between the reference marks, shaded.
        let at = |db: f64| rect.left() + rect.width() * ((db + 60.0) / 60.0) as f32;
        p.rect_filled(
            egui::Rect::from_x_y_ranges(
                at(REFERENCE_DBFS[0])..=at(REFERENCE_DBFS[1]),
                rect.y_range(),
            ),
            0.0,
            pal.accent.gamma_multiply(0.25),
        );
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
        self.agc_read = self.input_agc(None).and_then(Result::ok);
    }

    /// The key Settings keeps the selected input's automatic gain under.
    fn agc_key(&self) -> String {
        self.device.clone().unwrap_or_else(|| "default".into())
    }

    /// The selected input's automatic gain switch, read back, after
    /// turning it on or off first with `set`. `None` when the input has no
    /// switch the app can reach: the sound card's mixer on Linux, the USB
    /// microphone itself on macOS.
    fn input_agc(&self, set: Option<bool>) -> Option<Result<bool, String>> {
        if cfg!(target_os = "macos") {
            let label = capture::choices(&self.devices)
                .into_iter()
                .find(|c| Some(&c.id) == self.device.as_ref())
                .map(|c| c.label);
            let read = |list: Result<Vec<mixer::UsbAgc>, String>| {
                usb_agc_for(list.ok()?, label.as_deref())
            };
            let found = read(mixer::usb_agc(None))?;
            let now = found.before;
            return Some(match set {
                // Over USB the request reaches every USB microphone's
                // switch, so it goes only when this input's needs it.
                Some(want) if now != Some(want) => match read(mixer::usb_agc(Some(want))) {
                    Some(mixer::UsbAgc { error: Some(e), .. }) => Err(e),
                    Some(mixer::UsbAgc { after: Some(v), .. }) => Ok(v),
                    _ => Err("the microphone didn't say whether it changed".into()),
                },
                _ => now.ok_or_else(|| {
                    found
                        .error
                        .unwrap_or_else(|| "the microphone didn't answer".into())
                }),
            });
        }
        let g = self.gain.as_ref()?;
        self.gain_state.as_ref()?.agc?;
        if let Some(want) = set {
            if let Err(e) = g.set_agc(want) {
                return Some(Err(e));
            }
        }
        g.read()?.agc.map(Ok)
    }

    /// Turn the selected input's automatic gain off, or leave it on where
    /// Settings says so: each time the app opens the input, since a stand
    /// turns it back on when it is plugged in.
    fn apply_agc(&mut self) {
        let want = self.agc_on.contains(&self.agc_key());
        if self.gain_device != self.device {
            self.read_gain();
        }
        match self.input_agc(Some(want)) {
            Some(Ok(now)) => {
                self.agc_read = Some(now);
                applog::info(&format!(
                    "automatic gain {}",
                    if now { "on" } else { "off" }
                ));
            }
            Some(Err(e)) => {
                self.agc_read = None;
                self.gain_error = Some(format!(
                    "Couldn't turn the microphone's automatic gain {}: {e}",
                    if want { "on" } else { "off" }
                ));
            }
            None => self.agc_read = None,
        }
    }

    /// The input level control and meter, in Settings > Microphone and Set Up the Microphone.
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
            }
            _ => {
                theme::row(ui, "Input Level", Some(hint), |ui| {
                    if cfg!(any(target_os = "macos", target_os = "windows")) {
                        // The system sets it: open the system's own page.
                        if ui
                            .button("Open Sound Settings…")
                            .on_hover_text(SOUND_SETTINGS_HINT)
                            .clicked()
                        {
                            self.gain_error = open_sound_settings()
                                .err()
                                .map(|e| format!("Couldn't open the sound settings: {e}"));
                        }
                    } else {
                        // Wrapped, so a longer line doesn't widen the Watches pane.
                        ui.add(
                            egui::Label::new(
                                RichText::new("not adjustable here").color(pal.text_secondary),
                            )
                            .wrap(),
                        );
                    }
                });
            }
        }
        // Only for an input whose own automatic gain the app can reach.
        if let Some(now) = self.agc_read {
            let key = self.agc_key();
            let mut on = self.agc_on.contains(&key);
            if theme::switch_row(ui, "Automatic Gain", &mut on, AGC_HINT) {
                self.agc_on.retain(|k| k != &key);
                if on {
                    self.agc_on.push(key);
                }
                self.apply_agc();
            }
            // What the microphone says, where it isn't what was asked.
            if now != on {
                ui.label(
                    RichText::new(format!(
                        "The microphone reports its automatic gain {}.",
                        if now { "on" } else { "off" }
                    ))
                    .small()
                    .color(pal.warn),
                );
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
        // A take saved before the table renamed its calibre.
        let name = self.cal.current_name(&name);
        // Recent is what the app was last set to, even set again.
        self.cal.remember(&name);
        if name == self.calibre {
            return;
        }
        self.calibre = name;
        if let Some(l) = self.cal.lift(&self.calibre) {
            self.set_lift(l);
            self.lift_context = (l, true);
        }
        // Named afresh with the new wheels.
        self.steady = None;
        self.steady_job = None;
    }

    /// Open the Settings window at `tab`, reading the kept clocks afresh.
    fn open_settings(&mut self, tab: Tab) {
        self.cal.drop_rate_preset();
        self.kept_clocks = clockstore::ClockStore::load();
        self.read_library();
        self.settings_tab = Some(tab);
    }

    /// The library's folder: the one chosen, else `~/Timegrapher Data`.
    fn library_root(&self) -> Option<PathBuf> {
        self.library_dir.clone().or_else(library::default_location)
    }

    /// Read the library afresh: none yet where its folder holds no
    /// `library.toml`.
    fn read_library(&mut self) {
        // Let go of this copy's own lock first, or the new one finds it
        // held and reads as another copy's.
        self.library = Ok(None);
        self.library = match self.library_root() {
            Some(root) if library::Library::is_library(&root) => {
                library::Library::open(&root).map(Some)
            }
            _ => Ok(None),
        };
    }

    /// Act on a button in Settings' Watches Folder section.
    fn library_action(&mut self, act: settings_window::LibraryAction) {
        use settings_window::LibraryAction;
        match act {
            LibraryAction::Show => {
                if let Some(root) = self.library_root() {
                    if let Err(e) = reveal(&root) {
                        self.error(format!("Couldn't open {}: {e}", root.display()));
                    }
                }
            }
            LibraryAction::Move => {
                let (Some(root), Ok(Some(_))) = (self.library_root(), &self.library) else {
                    return;
                };
                let mut d = rfd::FileDialog::new().set_title("Move the Library Into a Folder");
                if let Some(parent) = root.parent() {
                    d = d.set_directory(parent);
                }
                let Some(parent) = d.pick_folder() else {
                    return;
                };
                let name = root
                    .file_name()
                    .map_or_else(|| library::DEFAULT_NAME.into(), |n| n.to_owned());
                self.library_move_ask = Some(parent.join(name));
            }
            LibraryAction::Choose => {
                let Some(dir) = rfd::FileDialog::new()
                    .set_title("Choose a Watches Folder, or an Empty Folder for a New One")
                    .pick_folder()
                else {
                    return;
                };
                let empty = std::fs::read_dir(&dir).map_or(true, |mut d| d.next().is_none());
                if !library::Library::is_library(&dir) && !empty {
                    return self.error(format!(
                        "{} holds files but no watches: choose a Watches folder, or an empty folder \
                         for a new one",
                        dir.display()
                    ));
                }
                self.use_library(Some(dir));
            }
            LibraryAction::UseDefault => self.use_library(None),
        }
    }

    /// Use the library at `dir` (`None`: the default) from now on.
    fn use_library(&mut self, dir: Option<PathBuf>) {
        // The default chosen by name is the default.
        self.library_dir = dir.filter(|d| Some(d) != library::default_location().as_ref());
        self.read_library();
    }

    /// Move Library… asks first, saying what moves and where.
    fn library_move_dialog(&mut self, ctx: &egui::Context) {
        let (Some(to), Some(from)) = (self.library_move_ask.clone(), self.library_root()) else {
            return;
        };
        let what = match &self.library {
            Ok(Some(l)) => format!(
                "Its {} and their takes ({}) are moved there,",
                if l.watches.len() == 1 {
                    "1 watch".to_string()
                } else {
                    format!("{} watches", l.watches.len())
                },
                fmt_bytes(l.bytes())
            ),
            _ => "Everything in it is moved there,".to_string(),
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("move library")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(
                RichText::new(format!("Move your watches to {}?", to.display()))
                    .font(theme::semibold(15.0)),
            );
            ui.add_space(4.0);
            ui.label(format!(
                "{what} and the app keeps them there from now on. Onto another disk they are \
                 copied, checked and only then removed from {}.",
                from.display()
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary(ui, "Move")).clicked() {
                    choice = Some(true);
                }
                if ui
                    .add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 26.0)))
                    .clicked()
                {
                    choice = Some(false);
                }
            });
        });
        if choice.is_none() && modal.should_close() {
            choice = Some(false);
        }
        if let Some(go) = choice {
            self.library_move_ask = None;
            if go {
                self.move_library(from, to);
            }
        }
    }

    /// Move the library on a thread of its own: a copy onto another disk
    /// can take a while.
    fn move_library(&mut self, from: PathBuf, to: PathBuf) {
        // A library this copy of the app holds can't be moved.
        if let Ok(Some(l)) = &mut self.library {
            l.unlock();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let dest = to.clone();
        std::thread::spawn(move || {
            let _ = tx.send(library::move_library(&from, &dest));
        });
        self.library_moving = Some((to, rx));
    }

    /// The move's outcome, once it is done.
    fn poll_library_move(&mut self, ctx: &egui::Context) {
        let Some((to, rx)) = &self.library_moving else {
            return;
        };
        let to = to.clone();
        match rx.try_recv() {
            Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(200)),
            Ok(Ok(left)) => {
                // The whole library is at `to` either way, so it is used there.
                self.library_moving = None;
                self.use_library(Some(to.clone()));
                match left {
                    None => self.info_msg(format!("Moved your watches to {}.", to.display())),
                    Some(why) => self.error(why),
                }
            }
            Ok(Err(e)) => {
                self.library_moving = None;
                self.read_library();
                self.error(format!("Your watches couldn't be moved: {e}"));
            }
            Err(TryRecvError::Disconnected) => {
                self.library_moving = None;
                self.read_library();
                self.error("Moving your watches stopped without finishing.".into());
            }
        }
    }

    /// The Settings window, when open, over everything else. Manage
    /// Calibres… opens it at Calibres.
    fn settings_window(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.cal.manager_open) {
            self.open_settings(Tab::Calibres);
            self.cal.apply_rate_preset();
        }
        self.poll_library_move(ctx);
        let Some(mut tab) = self.settings_tab else {
            return;
        };
        let screen = ctx.content_rect();
        let size = Vec2::new(
            (screen.width() - 24.0).clamp(320.0, 1100.0),
            (screen.height() - 64.0).clamp(240.0, 760.0),
        );
        let mut done = false;
        let modal = egui::Modal::new(egui::Id::new("settings")).show(ctx, |ui| {
            ui.set_width(size.x);
            ui.set_height(size.y);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Settings").strong().size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(theme::primary(ui, "Done"))
                        .on_hover_text("Close Settings")
                        .clicked()
                    {
                        done = true;
                    }
                });
            });
            ui.separator();
            ui.horizontal_top(|ui| {
                let h = ui.available_height();
                let down = egui::Layout::top_down(egui::Align::Min);
                ui.allocate_ui_with_layout(Vec2::new(SETTINGS_SIDEBAR_W, h), down, |ui| {
                    ui.set_min_size(Vec2::new(SETTINGS_SIDEBAR_W, h));
                    settings_window::sidebar(ui, &mut tab);
                });
                ui.separator();
                let rest = Vec2::new(ui.available_width(), h);
                ui.allocate_ui_with_layout(rest, down, |ui| {
                    ui.set_min_size(rest);
                    self.settings_category(ui, tab);
                });
            });
        });
        self.settings_last = tab;
        self.settings_tab = (!done && !modal.should_close()).then_some(tab);
        self.forget_dialog(ctx);
        self.library_move_dialog(ctx);
    }

    /// The category picked in Settings, under its name.
    fn settings_category(&mut self, ui: &mut egui::Ui, tab: Tab) {
        let name = settings_window::TABS
            .iter()
            .find(|t| t.0 == tab)
            .map_or("", |t| t.2);
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).strong().size(16.0));
            match tab {
                Tab::Microphone => {
                    theme::help(ui, "Microphone", help::MICROPHONE);
                }
                Tab::WatchesFolder => {
                    theme::help(ui, "Watches Folder", settings_window::LIBRARY_HELP);
                }
                Tab::Calibres => {
                    theme::help(ui, "Calibres", calibre_ui::MANAGE_HELP);
                }
                Tab::General => {}
            }
        });
        ui.add_space(6.0);
        match tab {
            Tab::General => {
                egui::ScrollArea::vertical()
                    .id_salt("general settings")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        self.general_settings(ui);
                    });
            }
            Tab::Microphone => {
                egui::ScrollArea::vertical()
                    .id_salt("microphone settings")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        self.microphone_card(ui);
                        ui.add_space(12.0);
                        let listening = self.clock_track.as_ref().map(|t| t.device.clone());
                        if let Some(device) = settings_window::quartz_section(
                            ui,
                            &self.kept_clocks,
                            listening.as_deref(),
                        ) {
                            self.forget_ask = Some(device);
                        }
                    });
            }
            Tab::WatchesFolder => {
                egui::ScrollArea::vertical()
                    .id_salt("watches folder settings")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_max_width(620.0);
                        let root = self.library_root();
                        let act = settings_window::library_section(
                            ui,
                            root.as_deref(),
                            &self.library,
                            self.library_moving.as_ref().map(|(to, _)| to.as_path()),
                            self.library_dir.is_some(),
                        );
                        if let Some(act) = act {
                            self.library_action(act);
                        }
                    });
            }
            Tab::Calibres => {
                let current = self.calibre.clone();
                let out = self.cal.contents(ui, &current, env!("CARGO_PKG_VERSION"));
                let name = out.name.clone();
                self.calibre_changed(&name, out.change);
            }
        }
    }

    /// Forget asks first, saying what is lost: the kept clock took 20
    /// minutes or more of listening to measure.
    fn forget_dialog(&mut self, ctx: &egui::Context) {
        let Some(device) = self.forget_ask.clone() else {
            return;
        };
        let kept = self
            .kept_clocks
            .as_ref()
            .ok()
            .and_then(|s| s.get(&device))
            .cloned();
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("forget clock")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                RichText::new(match &kept {
                    Some(c) => format!(
                        "Forget the clock kept for \"{device}\", {}, measured {}?",
                        settings_window::ppm_words(c.ppm),
                        day_of(&c.measured_utc)
                    ),
                    None => format!("Forget the clock kept for \"{device}\"?"),
                })
                .font(theme::semibold(15.0)),
            );
            ui.add_space(4.0);
            ui.label(
                "Rates from this input are then read on its own clock, uncorrected, here, on \
                 the command line and in the MCP server. In the app, getting it back takes 20 \
                 minutes or more of listening and Keep.",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(theme::destructive(ui, "Forget")).clicked() {
                    choice = Some(true);
                }
                if ui
                    .add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 26.0)))
                    .clicked()
                {
                    choice = Some(false);
                }
            });
        });
        if choice.is_none() && modal.should_close() {
            choice = Some(false);
        }
        if let Some(go) = choice {
            self.forget_ask = None;
            if go {
                self.forget_clock(&device);
            }
        }
    }

    /// Settings' General section: how the app looks and whether it asks
    /// about updates.
    fn general_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Appearance").strong());
        let mut t = ui.ctx().options(|o| o.theme_preference);
        ui.horizontal(|ui| {
            theme::segmented(
                ui,
                &mut t,
                &[
                    (
                        egui::ThemePreference::System,
                        "Auto",
                        "Follow the system's light or dark setting",
                    ),
                    (egui::ThemePreference::Light, "Light", "Always light"),
                    (egui::ThemePreference::Dark, "Dark", "Always dark"),
                ],
            );
        });
        if t != ui.ctx().options(|o| o.theme_preference) {
            ui.ctx().set_theme(t);
            self.theme = t;
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Updates").strong());
        theme::switch_row(
            ui,
            "Check for Updates at Start",
            &mut self.check_at_start,
            "Ask GitHub whether a newer Timegrapher is published each time the app starts, \
             and say so if there is. It sends no account or identifier, and nothing installs \
             itself. Help > Check for Updates… asks now.",
        );
        ui.add_space(12.0);
        ui.label(RichText::new("Listening").strong());
        theme::switch_row(
            ui,
            "Listen When the App Opens",
            &mut self.listen_at_open,
            "Start listening to the microphone as soon as the app opens, for the watch on the \
             stand. Off, Listen on the Listen card starts it.",
        );
        self.keep_awake_setting(ui);
    }

    /// Forget the kept clock of `device`: its rates are read on its own
    /// clock again, here and for the command line and the MCP server.
    fn forget_clock(&mut self, device: &str) {
        let forgot = clockstore::ClockStore::load().and_then(|mut s| {
            s.remove(device);
            s.save().map(|_| s)
        });
        match forgot {
            Ok(s) => {
                // The microphone session in hand, listening or stopped,
                // loses its True Clock too.
                if self.clock.as_ref().is_some_and(|c| c.device == device) {
                    self.clock = None;
                }
                self.kept_clocks = Ok(s);
                self.info_msg(format!(
                    "Forgot the clock kept for \"{device}\": its rates are read on its own \
                     clock again."
                ));
            }
            Err(e) => {
                self.kept_clocks = clockstore::ClockStore::load();
                self.error(format!(
                    "The clock kept for \"{device}\" couldn't be forgotten: {e}"
                ));
            }
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
        ui.spacing_mut().item_spacing.x = theme::GAP;
        let gap = theme::GAP;
        let total = ui.available_width();
        // The Current Position card wide enough for its six buttons in a
        // row where the window allows (else they go in two rows of three),
        // and Listen; the four of figures share the rest, Amplitude taking
        // the room its lift row needs from the other three.
        let listen = (total * 0.13).clamp(144.0, 180.0);
        let narrow = (total * 0.18)
            .clamp(POSITION_MIN_W, POSITION_MAX_W)
            .min(total - listen - 5.0 * gap - 4.0 * FIGURE_MIN_W)
            .max(POSITION_MIN_W);
        let share = ((total - narrow - listen - 5.0 * gap) / 4.0).max(100.0);
        let amp_w = (LIFT_ROW_W + 2.0 * 14.0)
            .min(4.0 * share - 3.0 * FIGURE_MIN_W)
            .max(share);
        let wide = (4.0 * share - amp_w) / 3.0;
        // The width inside a card of figures, less its margins: the room
        // kept for whole lines is reckoned at it, the same on every card.
        let inner = wide - 2.0 * 14.0;
        // Every line of a card is a row of fixed height, whatever is on it
        // or not, so the cards (and every pane under them) never change
        // height with the readings: only with the window's width. What
        // doesn't fit a row is cut short with its whole in the hover.
        let small_h = ui.text_style_height(&egui::TextStyle::Small) + 2.0;
        let body_h = ui.text_style_height(&egui::TextStyle::Body);
        let display_h = ui.fonts_mut(|f| f.row_height(&theme::display(FIGURE_PT)));
        let row = |ui: &mut egui::Ui, h: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::hover());
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.set_clip_rect(rect.intersect(ui.clip_rect()));
            child.spacing_mut().item_spacing.x = 6.0;
            add(&mut child);
        };
        // A small line of its own, cut short where it doesn't fit.
        let note = |ui: &mut egui::Ui, text: &str, hover: &str| {
            row(ui, small_h, &mut |ui| {
                let l = ui.add(
                    egui::Label::new(RichText::new(text).small().color(pal.text_secondary))
                        .truncate(),
                );
                if !hover.is_empty() {
                    l.on_hover_text(hover);
                }
            });
        };
        // The big figure, with what stands behind it in its hover. The
        // unit beside it, or under it on a card too narrow for `widest`,
        // the widest figure and unit the card shows: the width decides,
        // never the figure.
        let figure = |ui: &mut egui::Ui,
                      value: Option<String>,
                      unit: &str,
                      hover: &str,
                      widest: (&str, &str)| {
            let (v, c) = match value {
                // A true minus sign, as wide as the plus.
                Some(v) => (v.replace('-', "−"), pal.text),
                None => ("—".to_string(), pal.text_tertiary),
            };
            // In short, the same with or without a figure, so the card
            // keeps one look while it waits and once it reads.
            let unit = unit
                .replace("seconds per day", "s/d")
                .replace("milliseconds", "ms");
            let wide = |text: &str, font: egui::FontId| {
                ui.fonts_mut(|f| f.layout_no_wrap(text.into(), font, c).size().x)
            };
            // An Amplitude with no ± yet still keeps the unit's row.
            let beside = widest.1.is_empty()
                || wide(widest.0, theme::display(FIGURE_PT))
                    + 6.0
                    + wide(widest.1, egui::TextStyle::Body.resolve(ui.style()))
                    <= ui.available_width();
            // A figure wider than its card (noise read as beats can give
            // thousands of s/d) is made smaller to show whole.
            let unit_w = if beside && !unit.is_empty() {
                6.0 + wide(&unit, egui::TextStyle::Body.resolve(ui.style()))
            } else {
                0.0
            };
            let room = ui.available_width() - unit_w;
            let fig_w = wide(&v, theme::display(FIGURE_PT));
            let size = if fig_w > room {
                (FIGURE_PT * room / fig_w).floor().max(12.0)
            } else {
                FIGURE_PT
            };
            let big = |ui: &mut egui::Ui| {
                let l = ui.add(
                    egui::Label::new(RichText::new(&v).font(theme::display(size)).color(c))
                        .extend()
                        .sense(egui::Sense::hover()),
                );
                if !hover.is_empty() {
                    l.on_hover_text(hover);
                }
            };
            let small = |ui: &mut egui::Ui| {
                ui.add(egui::Label::new(RichText::new(&unit).color(pal.text_secondary)).extend());
            };
            if beside {
                row(ui, display_h, &mut |ui| {
                    big(ui);
                    if !unit.is_empty() {
                        small(ui);
                    }
                });
            } else {
                row(ui, display_h, &mut |ui| big(ui));
                row(ui, body_h, &mut |ui| small(ui));
            }
        };
        // A reading whose beats scatter like noise is no watch: no figures
        // then, only a note saying so.
        let heard = r.as_ref().is_none_or(|r| r.watch_heard || r.bph.is_none());
        // On the cards the fact in short, whole; the sentence with the
        // timing scatter, as the command line gives it, in the hover.
        let unheard_line = "No regular beat".to_string();
        let unheard_hover = format!(
            "{}.\n\n{}",
            r.as_ref().map(no_regular_beat).unwrap_or_default(),
            help::NO_REGULAR_BEAT
        );
        let r = r.as_ref().filter(|_| heard);
        let caveats = r.map_or(&[][..], |r| &r.caveats[..]);
        // Each reading's ±, as an analysis of the recording gives them
        // (core `measure`).
        let pm = r.and_then(|r| r.rate_error_s_per_day);
        let be_pm = r.and_then(|r| r.beat_error_unlock_error_ms);
        let lift_now = self.lift_deg;
        let beats = r.map_or(0, |r| r.beats_used);
        // Over the whole take, once it is stopped: `session`'s reading of
        // it beside the last window's figure.
        let whole = self.whole_take();
        // The whole take's reading, for the figure's hover.
        let whole_text = |figure: String, spread: Option<String>, span: f64| {
            let spread = spread.map_or_else(String::new, |s| format!(", {s}"));
            format!(
                "\n\nWhole take {figure}{spread} over {}. The big figure is the last stretch \
                 of Average Over; this is the take in this position, settling left out, read \
                 as timegrapher session reads a recording.",
                strip::fmt_time(span)
            )
        };
        // The line under the figure that says no watch is heard, or
        // nothing.
        let unheard = |ui: &mut egui::Ui| {
            if heard {
                whole_lines(ui, "", "", LONGEST_STATUS, inner);
            } else {
                whole_lines(ui, &unheard_line, &unheard_hover, LONGEST_STATUS, inner);
            }
        };
        // The cards share one height, the tallest's on the last frame. With
        // every line of fixed height, that changes only with the window's
        // width.
        let height_id = ui.id().with("readout height");
        let (height, height_w) = ui
            .ctx()
            .data(|d| d.get_temp::<(f32, f32)>(height_id))
            .unwrap_or((0.0, 0.0));
        let tallest = std::cell::Cell::new(0f32);
        let slots = std::cell::RefCell::new(Vec::new());
        let card = |ui: &mut egui::Ui,
                    title: &str,
                    help: &[&str],
                    facts: Option<Vec<(String, String)>>,
                    slot: Option<(Slot, f32)>,
                    body: &dyn Fn(&mut egui::Ui)| {
            let margin = egui::Margin::symmetric(14, 10);
            let shown = theme::card()
                .fill(pal.card)
                .inner_margin(margin)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    // Before anything is laid out: the minimum is taken from
                    // the cursor down.
                    ui.set_min_height(height);
                    let top = ui.min_rect().top();
                    let short = facts
                        .iter()
                        .flatten()
                        .map(|f| f.0.as_str())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    let long = facts
                        .iter()
                        .flatten()
                        .map(|f| f.1.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    ui.horizontal(|ui| {
                        ui.label(theme::caption(title).color(pal.text_secondary));
                        theme::help(ui, title, help);
                    });
                    // A reading card's limits under its title, whole, never
                    // cut short: they may be what says the figure isn't a
                    // watch's. Their room is there (empty) or not, as many
                    // lines as the longest usual limits take at this width,
                    // so the card's height follows the width alone; only
                    // limits longer still take more.
                    if facts.is_some() {
                        whole_lines(ui, &short, &long, LONGEST_LIMITS, inner);
                    }
                    body(ui);
                    // Room for the controls, which go in once every card
                    // is drawn, at the foot of the row's height.
                    if let Some((_, h)) = slot {
                        ui.add_space(6.0);
                        ui.allocate_space(Vec2::new(ui.available_width(), h));
                    }
                    let bottom = ui.cursor().top() - ui.spacing().item_spacing.y;
                    tallest.set(tallest.get().max(bottom - top));
                });
            if let Some((kind, h)) = slot {
                let r = shown.response.rect;
                let (mx, my) = (f32::from(margin.left), f32::from(margin.bottom));
                slots.borrow_mut().push((
                    kind,
                    egui::Rect::from_min_max(
                        egui::pos2(r.left() + mx, r.bottom() - my - h),
                        egui::pos2(r.right() - mx, r.bottom() - my),
                    ),
                ));
            }
        };
        let origin = ui.cursor().min;
        let mut cols: Vec<egui::Ui> = [narrow, listen, wide, amp_w, wide, wide]
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
        let keep_clock = std::cell::Cell::new(false);
        let measured = self.clock_offer();
        let kept = self.clock.clone();
        {
            let rate = r.and_then(|r| r.rate_s_per_day);
            // The input's clock is named in the Rate "?" rather than on the
            // card: nothing on the card can change it.
            let rate_facts = reading_limits("Rate", caveats, lift_now, beats)
                .into_iter()
                .filter(|f| f.0 != CLOCK_UNKNOWN)
                .collect();
            card(
                &mut cols[0],
                if narrow >= POSITION_CAPTION_W {
                    "Current Position"
                } else {
                    "Position"
                },
                help::POSITION,
                None,
                Some((Slot::Position, position_slot_h(narrow - 2.0 * 14.0))),
                &|_| {},
            );
            card(
                &mut cols[1],
                "Listen",
                help::LISTEN,
                None,
                Some((Slot::Listen, LISTEN_SLOT_H)),
                &|_| {},
            );
            card(
                &mut cols[2],
                "Rate",
                help::RATE,
                Some(rate_facts),
                None,
                &|ui| {
                    let unit = match pm {
                        Some(e) if e < 0.05 => "seconds per day".to_string(),
                        Some(e) => format!("± {e:.1} seconds per day"),
                        None => "seconds per day".to_string(),
                    };
                    // The beats behind the figure, in its hover.
                    let behind = r.map_or_else(String::new, |r| {
                        let short = r.span_s + 1.0 < self.average_s;
                        let mut t = if short {
                            format!(
                                "{} beats in {} of {}: there are fewer seconds of beats than \
                                 Average Over asks for, early in a session or after a break in \
                                 the sound, so the readings use what there is.",
                                r.beats_used,
                                fields::duration(r.span_s.round()),
                                fields::duration(self.average_s)
                            )
                        } else {
                            format!(
                                "{} beats in {}: the beats the readings are fitted to, over \
                                 Average Over.",
                                r.beats_used,
                                fields::duration(self.average_s)
                            )
                        };
                        if let Some(j) = r.jitter_us {
                            t += &format!(
                                "\n\nJitter {j:.0} µs: how far single beats land from the \
                                 rate's straight line, the spread of the dots across the strip."
                            );
                        }
                        t
                    });
                    let mut behind = behind;
                    if let Some((w, v)) =
                        whole.as_ref().and_then(|w| Some((w, w.m.rate_s_per_day?)))
                    {
                        let se = w.m.confidence.and_then(|c| c.rate.se);
                        let pm = se.map_or_else(String::new, |e| format!(" ± {e:.1}"));
                        let spread = w.m.rate_p05.zip(w.m.rate_p95).map(|(a, b)| {
                            format!("10 s rates {a:+.1} to {b:+.1} s/d").replace('-', "−")
                        });
                        behind += &whole_text(
                            format!("{v:+.1}{pm} s/d").replace('-', "−"),
                            spread,
                            w.span,
                        );
                    }
                    // Under the figure, two lines: what the reading waits on
                    // (or why it is the position before), and the rate on the
                    // input's own clock or the clock timed to keep.
                    let status: Option<(String, String)> = if r.is_none() {
                        if let Some(left) = self.settling_left() {
                            Some((
                                format!("settling, {:.0} s", left.ceil()),
                                format!(
                                    "Settling in the new position for {:.0} s more.\n\n{SETTLE_HINT}",
                                    left.ceil()
                                ),
                            ))
                        } else if heard {
                            Some(("waiting for beats".into(), String::new()))
                        } else {
                            Some((unheard_line.clone(), unheard_hover.clone()))
                        }
                    } else {
                        self.stopped_in_settle().map(|at| {
                            (
                                "before the move".to_string(),
                                format!(
                                    "Stopped while the watch settled in its new position, so \
                                     these are the last readings of the position before it, \
                                     before the move at {}.",
                                    strip::fmt_time(at)
                                ),
                            )
                        })
                    };
                    let true_clock = rate.zip(self.clock.as_ref()).map(|(rate, c)| {
                        let v = format!("{:+.1}", c.correct_rate(rate)).replace('-', "−");
                        let slow = if c.ppm >= 0.0 { "slow" } else { "fast" };
                        (
                            format!(
                                "True Clock {v} seconds per day, with the clock kept {}",
                                day_of(&c.measured_utc)
                            ),
                            format!(
                                "The rate corrected for this input's own clock, which runs \
                             {:.1} ppm {slow} ({:+.1} s/d on every rate), measured {} from \
                             {}. The big figure stays on the card's clock, the one tg and an \
                             analysis of the recording read, so the two compare directly. \
                             The clock kept is under Settings > Microphone > Quartz \
                             Correction.",
                                c.ppm.abs(),
                                c.rate_error_s_per_day(),
                                c.measured_utc,
                                match c.source.as_str() {
                                    "log" => "a recording's clock log",
                                    "measure" => "listening against the system clock",
                                    _ => "a value typed in",
                                }
                            ),
                        )
                    });
                    // True Clock in the figure's hover too, for when the two
                    // lines under it are taken.
                    if let Some((t, _)) = &true_clock {
                        behind += &format!("\n\n{t}.");
                    }
                    figure(
                        ui,
                        rate.map(|v| format!("{v:+.1}")),
                        &unit,
                        behind.trim_start(),
                        ("−88.8", "± 8.8 s/d"),
                    );
                    // Timed long enough while listening: offered, never kept
                    // unasked.
                    let offer = measured.map(|f| {
                        let slow = if f.ppm >= 0.0 { "slow" } else { "fast" };
                        let mut text = format!("input clock timed: {:.1} ppm {slow}", f.ppm.abs());
                        let mut hint = format!(
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
                        );
                        if let Some(c) = &kept {
                            let off = f.ppm - c.ppm;
                            text += &format!(
                                ", {:.1} ppm from the clock kept {}",
                                off.abs(),
                                day_of(&c.measured_utc)
                            );
                            hint += &format!(
                                "\n\nThe clock kept for \"{}\" runs {:.1} ppm {}, kept {}. \
                             This timing is {:.1} ppm from it, {:.1} seconds per day on \
                             True Clock. An input reached by a shared name, such as the \
                             system default, may be a different sound card from the one \
                             timed then. Keep replaces the kept clock with this one.",
                                c.device,
                                c.ppm.abs(),
                                if c.ppm >= 0.0 { "slow" } else { "fast" },
                                day_of(&c.measured_utc),
                                off.abs(),
                                timegrapher_core::clock::rate_error_s_per_day(off.abs())
                            );
                        }
                        (text, hint)
                    });
                    let (first, second) = match (status, true_clock) {
                        (Some(s), clock) => (Some(s), clock),
                        (None, clock) => (clock, None),
                    };
                    // What the figure waits on, or that no watch is heard,
                    // whole; True Clock (in the figure's hover too) where
                    // the line is free.
                    match first {
                        Some((t, h)) => whole_lines(ui, &t, &h, LONGEST_STATUS, inner),
                        None => whole_lines(ui, "", "", LONGEST_STATUS, inner),
                    }
                    match (offer, second) {
                        (Some((text, hint)), _) => row(ui, small_h, &mut |ui| {
                            // The button first, so a long line is cut short
                            // rather than pushing it off the card.
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("Keep")
                                        .on_hover_text(
                                            "Keep this input's clock error and correct its rates",
                                        )
                                        .clicked()
                                    {
                                        keep_clock.set(true);
                                    }
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&text)
                                                        .small()
                                                        .color(pal.text_secondary),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(&hint);
                                        },
                                    );
                                },
                            );
                        }),
                        (None, Some((t, h))) => note(ui, &t, &h),
                        (None, None) => note(ui, "", ""),
                    }
                },
            );
            let amp = r.and_then(|r| r.amplitude_deg);
            card(
                &mut cols[3],
                "Amplitude",
                help::AMPLITUDE,
                // A default lift angle is said by "the default" beside Lift,
                // at the card's foot, rather than again in its heading.
                Some(
                    reading_limits("Amplitude", caveats, lift_now, beats)
                        .into_iter()
                        .filter(|f| !f.0.starts_with("lift angle "))
                        .collect(),
                ),
                Some((
                    Slot::Lift,
                    if amp_w - 2.0 * 14.0 < LIFT_ROW_W {
                        CONTROL_SLOT_H + LIFT_NARROW_EXTRA_H
                    } else {
                        CONTROL_SLOT_H
                    },
                )),
                &|ui| {
                    // Whole degrees: a live reading's ± is about a degree, so a
                    // decimal would claim more than the beats can say.
                    let unit = match r.and_then(|r| r.amplitude_error_deg) {
                        Some(e) if e < 0.95 => format!("± {e:.1}°"),
                        Some(e) => format!("± {e:.0}°"),
                        None => String::new(),
                    };
                    let mut behind = String::new();
                    if let Some(&LiveReading {
                        amplitude_even_deg: Some(a),
                        amplitude_odd_deg: Some(b),
                        ..
                    }) = r
                    {
                        behind = format!("Tick {a:.0}°, Tock {b:.0}°: each side's own amplitude.");
                    }
                    if let Some((w, v)) = whole.as_ref().and_then(|w| Some((w, w.m.amplitude_deg?)))
                    {
                        let se = w.m.confidence.and_then(|c| c.amplitude.se);
                        let pm = se.map_or_else(String::new, |e| format!(" ± {e:.0}"));
                        let spread =
                            w.m.amplitude_p05
                                .zip(w.m.amplitude_p95)
                                .map(|(a, b)| format!("2 s windows {a:.0}° to {b:.0}°"));
                        behind += &whole_text(format!("{v:.0}{pm}°"), spread, w.span);
                    }
                    figure(
                        ui,
                        amp.map(|v| format!("{v:.0}°")),
                        &unit,
                        behind.trim_start(),
                        ("888°", "± 8°"),
                    );
                    unheard(ui);
                },
            );
            let unlock = r.and_then(|r| r.beat_error_unlock_ms);
            let drop = r.and_then(|r| r.beat_error_ms);
            card(
                &mut cols[4],
                "Beat Error",
                help::BEAT_ERROR,
                Some(reading_limits("Beat Error", caveats, lift_now, beats)),
                None,
                &|ui| {
                    let unit = match be_pm.filter(|_| unlock.is_some()) {
                        Some(e) if e >= 0.005 => format!("± {e:.2} milliseconds"),
                        _ => "milliseconds".to_string(),
                    };
                    // Which edge it is timed from, in the figure's hover.
                    let from = match (unlock, drop) {
                        (Some(_), Some(d)) => format!(
                            "From the unlock, the usual reading, as tg shows it: quote this one. \
                             From the drop, a second opinion: {:.2} ms.",
                            d.abs()
                        ),
                        (None, Some(_)) => "From the drop: the unlock couldn't be timed.".into(),
                        _ => "From the unlock, the usual reading, as tg shows it.".into(),
                    };
                    let mut from = from;
                    if let Some((w, v)) = whole.as_ref().and_then(|w| Some((w, w.m.beat_error()?)))
                    {
                        let se = w.m.confidence.and_then(|c| c.beat_error.se);
                        let pm = se.map_or_else(String::new, |e| format!(" ± {e:.2}"));
                        from += &whole_text(format!("{v:.2}{pm} ms"), None, w.span);
                        if w.m.caveats.contains(&Caveat::BeatErrorAsSize) {
                            from += &format!(
                                " The {WHOLE_AS_SIZES}, so the sizes either side are combined \
                                 rather than the signed values, as timegrapher session does."
                            );
                        }
                    }
                    // The size only: which beat is the tick can flip between runs.
                    figure(
                        ui,
                        unlock.or(drop).map(|v| format!("{:.2}", v.abs())),
                        &unit,
                        &from,
                        ("8.88", "± 0.88 ms"),
                    );
                    unheard(ui);
                },
            );
            let heard_bph = r.and_then(|r| r.bph).or_else(|| self.live.as_ref()?.bph());
            let set_bph = self.bph;
            // Set on the card, the figure is the card's menu, in its slot.
            let menu = self.take_shows_rate_and_lift();
            card(
                &mut cols[5],
                "Beat Rate",
                help::BEAT_RATE,
                None,
                Some((
                    Slot::BeatRate,
                    if !menu {
                        CONTROL_SLOT_H
                    } else if wide - 2.0 * 14.0 < BEAT_RATE_NARROW_W {
                        // The card less its side margins.
                        BEAT_RATE_SLOT_H + BEAT_RATE_NARROW_EXTRA_H
                    } else {
                        BEAT_RATE_SLOT_H
                    },
                )),
                &|ui| {
                    if menu {
                        return;
                    }
                    let how = match (set_bph, heard_bph) {
                        (Some(_), _) => {
                            "Set on this card, so the readings use it rather than finding it \
                             from the beats. Auto finds it again."
                        }
                        (None, Some(_)) => {
                            "Found from the beats: Beat Rate is Auto, so the app found it from \
                             the first seconds of beats, as the nearest standard rate."
                        }
                        (None, None) => "",
                    };
                    figure(
                        ui,
                        set_bph.or(heard_bph).map(|b| b.to_string()),
                        "bph",
                        how,
                        ("28800", "bph"),
                    );
                },
            );
        }
        let bottom = cols
            .iter()
            .map(|c| c.min_rect().bottom())
            .fold(origin.y, f32::max);
        ui.allocate_space(Vec2::new(total, bottom - origin.y));
        let fit = tallest.get();
        if (fit, total) != (height, height_w) {
            ui.ctx()
                .data_mut(|d| d.insert_temp(height_id, (fit, total)));
            ui.ctx().request_repaint();
        }
        Readouts {
            keep_clock: keep_clock.get(),
            slots: slots.into_inner(),
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

    /// The clock timed while listening, offered to keep: when none is kept
    /// for the input, or the kept one is more than [`CLOCK_DIFFERS_PPM`]
    /// from it.
    fn clock_offer(&self) -> Option<&timegrapher_core::clock::ClockFit> {
        self.clock_measured().filter(|f| {
            self.clock
                .as_ref()
                .is_none_or(|c| (f.ppm - c.ppm).abs() > CLOCK_DIFFERS_PPM)
        })
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
                .on_hover_text(
                    "The position, set on the Current Position card or with keys 1 to 6",
                );
                if let Some(b) = self.reading().and_then(|r| r.bph) {
                    ui.label(
                        RichText::new(format!("{b} beats per hour"))
                            .small()
                            .color(pal.text_secondary),
                    )
                    .on_hover_text("Found from the beats, or set on the Beat Rate card");
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
        if let Some(job) = &self.analysis_save {
            let done = f64::from_bits(job.progress.load(std::sync::atomic::Ordering::Relaxed));
            // The sound is read twice: once for the beats, once more for
            // the profiles and the shape.
            let frac = (done / (2.0 * job.total_s).max(1e-9)).clamp(0.0, 1.0);
            ui.add(
                egui::ProgressBar::new(frac as f32)
                    .desired_width(240.0)
                    .desired_height(14.0)
                    .fill(pal.accent)
                    .text(RichText::new("Keeping the analysis").small()),
            );
            let cancel = ui
                .small_button("Cancel")
                .on_hover_text("Stop keeping the analysis; nothing is written")
                .clicked();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
            if cancel {
                self.analysis_save = None;
            }
        }
        if let Some(s) = &self.saving {
            ui.add(egui::Spinner::new().size(12.0).color(pal.text_secondary));
            ui.label(RichText::new("Saving…").small().color(pal.text_secondary))
                .on_hover_text(format!(
                    "Copying {} of sound into {}. Listening carries on; the take saved before \
                     stays as it was until the copy is done.",
                    strip::fmt_time(s.secs),
                    s.dest.display()
                ));
        }
        // Always on screen: where the take is going, and whether the strip
        // is live or a take opened.
        ui.label(
            RichText::new(self.status_words())
                .font(theme::semibold(13.5))
                .color(theme::pal(ui).text),
        )
        .on_hover_text("The take on show, its watch, and whether it is listening");
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
                            "Saved in {}. Sound since then is kept too; Save Again adds it. \
                         New Session and quitting ask first, and a quit that can't ask \
                         keeps it when there is more than 5 s of it, \
                         to offer back at the next start.",
                            p.display()
                        ),
                    ),
                    None if self.analysis_kept_to.is_some() => {
                        let (dir, through_s) = self.analysis_kept_to.clone().unwrap_or_default();
                        let kept = if r.duration_s() - through_s >= 1.0 {
                            format!(
                                "the analysis of its first {} kept",
                                strip::fmt_time(through_s)
                            )
                        } else {
                            "its analysis kept".to_string()
                        };
                        (
                            format!(
                                "{} of sound, not saved; {kept}",
                                strip::fmt_time(r.duration_s())
                            ),
                            pal.text_secondary,
                            format!(
                                "The analysis of the first {} was kept without the sound in {}. \
                                 The sound itself isn't saved: Save keeps it. New Session and \
                                 quitting ask before throwing it away, and a quit that can't \
                                 ask keeps it when there is more than 5 s of it, \
                                 to offer back at the next start.",
                                strip::fmt_time(through_s),
                                dir.display()
                            ),
                        )
                    }
                    None => (
                        format!("{} of sound, not saved", strip::fmt_time(r.duration_s())),
                        pal.text_secondary,
                        "The session's sound is kept as it goes, so Save can keep it at any \
                     point. New Session and quitting ask before throwing it away, and a quit that \
                     can't ask keeps it when there is more than 5 s of it, \
                     to offer back at the next start."
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
        // Space above the card, but never so much that the card (as tall as
        // on the last frame) runs out of the pane.
        let h_id = ui.id().with("empty state height");
        let last_h: f32 = ui.data(|d| d.get_temp(h_id)).unwrap_or(0.0);
        let room = (ui.available_height() - last_h).max(0.0);
        ui.add_space((ui.available_height() * 0.16).clamp(16.0, 110.0).min(room));
        ui.vertical_centered(|ui| {
            let w = 520.0_f32.min(ui.available_width());
            ui.allocate_ui(Vec2::new(w, 0.0), |ui| {
                let shown = theme::card().fill(pal.card).show(ui, |ui| {
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
                let h = shown.response.rect.height();
                if h != last_h {
                    ui.data_mut(|d| d.insert_temp(h_id, h));
                    ui.ctx().request_repaint();
                }
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

    /// Before anything is open: where to start, since the Watches pane
    /// and the File menu hold the ways in.
    fn ways_in(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        ui.label(RichText::new("Timegrapher").font(theme::semibold(20.0)));
        let line = |ui: &mut egui::Ui, text: &str| {
            ui.add(egui::Label::new(RichText::new(text).color(pal.text_secondary)).wrap());
        };
        if self.mic_session_set_aside() {
            line(
                ui,
                "Your microphone session is set aside while this shows: Listen, on the \
                 Listen card, starts a new one.",
            );
        }
        line(
            ui,
            "Listen, on the Listen card, listens to the watch on the microphone without \
             picking a watch; Save keeps what it heard. For a watch's history, Watches (at \
             the top) lists your watches: pick one, then Listen, Positions Test or Power \
             Reserve Test, and each take is saved into it. New Watch… adds one.",
        );
        line(
            ui,
            "Recordings and folders open from the File menu, analysed all at once.",
        );
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
        // A test is never Quick Listen: its sound is saved or asked about,
        // not let go, even when it follows Quick Listen or runs on from it.
        self.quick_listen = false;
        if !self.running() {
            if self.can_resume() {
                self.resume_microphone();
            } else {
                self.start_microphone();
                if self.confirm == Some(Confirm::Microphone) {
                    // The test begins once the masks are answered for.
                    self.guided_after_ask = Some(half);
                    return;
                }
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
        self.positions_take = true;
        self.guided_unused.clear();
        self.guided_measured.clear();
        self.reveal_pane(Pane::PositionsTest);
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
                test.read_new(&log, clock.as_ref(), &self.masked);
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
            theme::keep_next(ui);
            if theme::tool_word_button(ui, Icon::Listen, label, &start_hint, true, None) {
                let earlier = self.guided_earlier.clone();
                self.start_guided(earlier);
            }
            let load_hint = format!(
                "For the test 24 hours after winding: load the results saved from the test just \
                 after it ({} in a saved session's folder, or the file Save Results for 24 \
                 Hours… wrote). The next test is then set against them.",
                guided::RESULTS_FILE
            );
            if theme::tool_word_button(
                ui,
                Icon::BarrelIn,
                "Load Full-Wind Results…",
                &load_hint,
                false,
                None,
            ) {
                if let Some(full) = self.load_full_wind() {
                    self.guided_earlier = Some(full);
                }
            }
            theme::bar_gap(ui);
        }
        theme::tool_field(
            ui,
            "Settle",
            "Seconds the watch is left to settle after every change of position, in any take \
             and in the guided test, before its beats are read. They show faint on the strip \
             and are left out of every reading.",
            |ui| {
                let changed = ui
                    .add_enabled(
                        !running,
                        egui::DragValue::new(&mut self.guided_settle_s)
                            .range(guided::SETTLE_RANGE)
                            .speed(1.0)
                            .max_decimals(0)
                            .suffix(" s"),
                    )
                    .changed();
                if changed {
                    self.session.set_settle_s(self.guided_settle_s);
                }
            },
        );
        theme::tool_field(
            ui,
            "Measure",
            "Seconds each position is measured for. Longer brings the rate's ± down; a minute \
             gives about half a second a day.",
            |ui| {
                ui.add_enabled(
                    !running,
                    egui::DragValue::new(&mut self.guided_measure_s)
                        .range(guided::MEASURE_RANGE)
                        .speed(1.0)
                        .max_decimals(0)
                        .suffix(" s"),
                );
            },
        );
        let has = self
            .guided
            .as_ref()
            .is_some_and(|t| !t.readings_so_far().is_empty());
        let nothing = (!has).then_some("Nothing measured yet");
        theme::bar_gap(ui);
        let about = self.guided_about();
        if theme::tool_button(
            ui,
            Icon::SaveTable,
            "Save as CSV…",
            "Save the watch, its calibre, the table, the means and the differences as a CSV \
             file a spreadsheet opens",
            nothing,
        ) {
            if let Some(t) = &self.guided {
                let text = guided::export_text(t, &about, ',');
                if let Some(m) = guided::save_csv(&text, &self.guided_dir()) {
                    self.info_msg(m);
                }
            }
        }
        if theme::tool_word_button(
            ui,
            Icon::Copy,
            "Copy",
            "Copy the watch, its calibre and the table with tabs between the columns, to paste \
             into a spreadsheet or a message",
            false,
            nothing,
        ) {
            if let Some(t) = &self.guided {
                ui.ctx().copy_text(guided::export_text(t, &about, '\t'));
                self.info_msg("Copied the Positions Test table".to_string());
            }
        }
        if let Some(k) = self.guided.as_ref().and_then(|t| t.kept(&about)) {
            let hint = format!(
                "Save these results to load when you run the test again 24 hours after winding. \
                 Saving the session keeps them too, as {} in its folder.",
                guided::RESULTS_FILE
            );
            if theme::tool_word_button(
                ui,
                Icon::Barrel24,
                "Save Results for 24 Hours…",
                &hint,
                false,
                None,
            ) {
                if let Some(m) = guided::save_results(&k, &self.guided_dir()) {
                    self.info_msg(m);
                }
            }
        }
    }

    /// The Positions Test pane: the banner and the tables, and what its
    /// buttons ask for.
    fn positions_pane(&mut self, ui: &mut egui::Ui) {
        self.results_rects[0] = Some(ui.max_rect());
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
                        test.finish(&log, clock.as_ref(), &self.masked);
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

    /// Whether a Power Reserve Test is under way, not yet ended.
    fn reserve_running(&self) -> bool {
        self.reserve.as_ref().is_some_and(|t| t.running())
    }

    /// Where a Power Reserve Test keeps its take: the picked watch's
    /// takes, or the library's Unsorted with none picked.
    fn reserve_takes_dir(&mut self) -> Result<PathBuf, String> {
        self.ensure_library();
        match &self.library {
            Ok(Some(l)) => {
                if let Some(why) = &l.read_only {
                    return Err(why.clone());
                }
                match self.open_watch() {
                    Some(e) => Ok(e.dir.join(library::TAKES)),
                    None => l.unsorted_dir(),
                }
            }
            Ok(None) => Err("There is no library".into()),
            Err(e) => Err(format!("The library can't be read: {e}")),
        }
    }

    /// Audio seconds to true seconds for the session: the input's clock
    /// as measured while listening, else its stored correction.
    fn to_true(&self) -> impl Fn(f64) -> f64 + use<> {
        let fit = self
            .clock_track
            .as_ref()
            .and_then(|c| c.fit.clone())
            .or_else(|| {
                let d = self.live.as_ref().map_or(0.0, |l| l.duration_s());
                self.clock
                    .as_ref()
                    .and_then(|c| timegrapher_core::session::fixed_clock(c.ppm, d))
            });
        move |s| fit.as_ref().map_or(s, |f| f.map(s))
    }

    /// The Power Reserve pane's toolbar: the position and the keeping,
    /// and Start Test.
    fn reserve_settings(&mut self, ui: &mut egui::Ui) {
        let running = self.reserve_running();
        theme::tool_field(
            ui,
            "Position",
            "The one position the watch runs down in, from wind to stop",
            |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    egui::ComboBox::from_id_salt("reserve position")
                        .selected_text(self.reserve_position.label())
                        .show_ui(ui, |ui| {
                            for p in Position::ALL {
                                ui.selectable_value(&mut self.reserve_position, p, p.label());
                            }
                        });
                });
            },
        );
        ui.add_enabled_ui(!running, |ui| {
            theme::tool_choice(
                ui,
                "Keep",
                "What the take keeps once the run ends",
                &mut self.reserve_keep_sound,
                &[
                    (
                        true,
                        Icon::Wave,
                        "Keep Sound",
                        "Keep the run's sound in the take, to measure again later; about a \
                         third of a GB an hour",
                    ),
                    (
                        false,
                        Icon::Sheet,
                        "Analysis Only",
                        "Keep the take as its analysis only, tens of MB: every pane still \
                         reads it, but it can't be measured again. The sound is still written \
                         while the test runs and is deleted once the analysis is written.",
                    ),
                ],
            );
        });
        if !running {
            theme::bar_gap(ui);
            if theme::tool_word_button(
                ui,
                Icon::Listen,
                "Start Test",
                "Start listening in the position picked and run until the watch stops, keeping \
                 the computer awake. Wind the watch fully first.",
                true,
                None,
            ) {
                self.start_reserve();
            }
        }
        let done = self.reserve.as_ref().and_then(|t| t.results.clone());
        let nothing = done.is_none().then_some("No run ended yet");
        theme::bar_gap(ui);
        let watch = self.watch.trim().to_string();
        if theme::tool_button(
            ui,
            Icon::SaveTable,
            "Save as CSV…",
            "Save the watch, the run and its hour-by-hour table as a CSV file a spreadsheet \
             opens",
            nothing,
        ) {
            if let Some(r) = &done {
                let dir = self.guided_dir();
                if let Some(m) = reserve::save_csv(&reserve::results_text(r, &watch, ','), &dir) {
                    self.info_msg(m);
                }
            }
        }
        if theme::tool_word_button(
            ui,
            Icon::Copy,
            "Copy",
            "Copy the watch, the run and its hour-by-hour table with tabs between the columns, \
             to paste into a spreadsheet or a message",
            false,
            nothing,
        ) {
            if let Some(r) = &done {
                ui.ctx().copy_text(reserve::results_text(r, &watch, '\t'));
                self.info_msg("Copied the Power Reserve Test".to_string());
            }
        }
    }

    /// Begin a Power Reserve Test: a new microphone session in the
    /// position picked, kept in the watch's takes when it ends.
    fn start_reserve(&mut self) {
        if let Err(e) = self.reserve_takes_dir() {
            return self.error(format!("A Power Reserve Test can't keep its take: {e}"));
        }
        if self.running() {
            self.info_msg(
                "Stop listening first: a Power Reserve Test starts a new session.".into(),
            );
            return;
        }
        if self.needs_save_confirmation() {
            self.info_msg(
                "Save or set aside this session first: a Power Reserve Test starts a new one."
                    .into(),
            );
            return;
        }
        if self.live.is_some() || self.session.recorder.is_some() {
            self.new_session();
        }
        self.input = Input::Microphone;
        // Never Quick Listen, whose sound goes without a question: a test
        // keeps its take.
        self.quick_listen = false;
        self.start_microphone();
        if !self.running() || self.session.recorder.is_none() {
            return;
        }
        let pos = self.reserve_position;
        if self.position != pos {
            self.change_position(pos);
        }
        let started = self
            .session
            .recorder
            .as_ref()
            .map_or_else(String::new, |r| r.started_utc().to_string());
        let now = self.live.as_ref().map_or(0.0, |l| l.duration_s());
        self.reserve = Some(reserve::Test::new(
            pos,
            self.reserve_keep_sound,
            started,
            now,
        ));
        self.reserve_read_s = now;
        applog::info(&format!(
            "power reserve test started in {}, {}",
            pos.code(),
            if self.reserve_keep_sound {
                "keeping the sound"
            } else {
                "keeping the analysis only"
            }
        ));
        self.reveal_pane(Pane::PowerReserve);
    }

    /// Follow a running Power Reserve Test, whether its pane shows or
    /// not: read the beats every few seconds, and end it at the stop.
    fn reserve_tick(&mut self, ctx: &egui::Context) {
        if !self.reserve_running() {
            return;
        }
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let now = live.duration_s();
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        if now - self.reserve_read_s < 2.0 {
            return;
        }
        self.reserve_read_s = now;
        let beat_s = 3600.0 / f64::from(live.bph().unwrap_or(18_000).max(1));
        let ended = self
            .reserve
            .as_mut()
            .and_then(|t| t.update(live.beats(), beat_s, now));
        if ended.is_some() {
            self.end_reserve();
        }
    }

    /// The test has ended: stop listening, keep the take in the watch's
    /// history with its results, and as its analysis only if so picked.
    fn end_reserve(&mut self) {
        self.pause();
        let to_true = self.to_true();
        let Some(t) = self.reserve.as_mut() else {
            return;
        };
        if let Some(e) = t.ending {
            applog::info(&format!(
                "power reserve test ended: {}",
                reserve::ending_text(e)
            ));
        }
        let mut results = t.finish(to_true).cloned();
        // Hour by hour, as `timegrapher session` reads each stretch.
        if let (Some(r), Some(log)) = (
            results.as_mut(),
            self.live.as_ref().and_then(steady::log_of),
        ) {
            let fit = self
                .clock_track
                .as_ref()
                .and_then(|c| c.fit.clone())
                .or_else(|| {
                    self.clock
                        .as_ref()
                        .and_then(|c| timegrapher_core::session::fixed_clock(c.ppm, log.duration_s))
                });
            let breaks = timegrapher_core::breaks::with_jumps(Vec::new(), &log.jumps);
            r.hours = power_reserve::hours(
                &log,
                fit.as_ref(),
                &breaks,
                &self.masked,
                r.first_beat_s,
                r.last_beat_s,
            );
        }
        // The results go into the take with its sound, through
        // `results_files`.
        if let Some(t) = self.reserve.as_mut() {
            t.results = results;
        }
        // With no watch picked, asked where it goes before it is saved.
        if self.open_watch().is_none() {
            self.test_keep = Some(TestKeep::PowerReserve);
            return;
        }
        self.save_reserve();
    }

    /// Save the ended Power Reserve Test's take, with its results, into
    /// the picked watch's takes or Unsorted.
    fn save_reserve(&mut self) {
        let takes = match self.reserve_takes_dir() {
            Ok(d) => d,
            Err(e) => return self.error(format!("The test's take couldn't be kept: {e}")),
        };
        let Some(r) = self.session.recorder.as_mut() else {
            return;
        };
        let started = watches::local_minute(r.started_utc())
            .unwrap_or_else(|| r.started_utc().replace(':', "."));
        let dir = library::unused_dir(&takes, &format!("{started} Power Reserve"));
        if let Some(name) = dir.file_name() {
            r.name_first_copy(name.to_owned());
        }
        self.save_into(&takes);
        if let Some(t) = self.reserve.as_mut() {
            t.saving = self.saving.is_some();
        }
        self.reserve_saved();
    }

    /// Once the test's take is written: note where, and keep it as its
    /// analysis only if so picked.
    fn reserve_saved(&mut self) {
        if self.saving.is_some() {
            return;
        }
        let Some(t) = self.reserve.as_mut().filter(|t| t.saving) else {
            return;
        };
        t.saving = false;
        let keep_sound = t.keep_sound;
        let Some(saved) = self.session.saved_to().map(Path::to_path_buf) else {
            return;
        };
        if let Some(t) = self.reserve.as_mut() {
            t.saved = Some(saved.clone());
        }
        if let Ok(Some(l)) = &mut self.library {
            l.scan();
        }
        if !keep_sound {
            self.start_take_keep(saved);
        }
    }

    /// The Power Reserve pane: the run so far, or its result.
    fn reserve_pane(&mut self, ui: &mut egui::Ui) {
        let free = if self.reserve.is_none() {
            self.reserve_takes_dir().ok().and_then(|d| {
                let have = reserve::free_bytes(&d)?;
                let (rate, bits) = self
                    .capture
                    .as_ref()
                    .map_or((48_000, 16), |c| (c.sample_rate, c.bits));
                Some((have, reserve::bytes_needed(rate, bits)))
            })
        } else {
            None
        };
        if reserve::pane(ui, self.reserve.as_ref(), free) == Some(reserve::Action::Stop) {
            if let Some(t) = self.reserve.as_mut() {
                t.stop();
            }
            self.end_reserve();
        }
    }

    /// Show a pane at the front of its tabs, or hide it: what its icon and
    /// its View menu item do.
    fn show_pane(&mut self, pane: Pane, on: bool) {
        if on {
            self.reveal_pane(pane);
        } else {
            set_pane_visible(&mut self.panes.tiles, pane, false);
        }
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

    /// Sound kept from an earlier session that was never saved: Open,
    /// Move To… or Delete…, one kept recording at a time.
    fn kept_banner(&mut self, ui: &mut egui::Ui) {
        let Some(k) = self.kept.first().cloned() else {
            return;
        };
        let pal = theme::pal(ui);
        let (mut open, mut move_to, mut delete, mut later) = (false, false, false, false);
        let what = format!(
            "Sound from an earlier session wasn't saved{}{}, {}. Open it, move it into a \
             folder of your own, or delete it?",
            k.started
                .as_ref()
                .map_or(String::new(), |s| format!(": started {s}")),
            k.duration_s
                .map_or(String::new(), |d| format!(", {} long", strip::fmt_time(d))),
            fmt_bytes(k.bytes),
        );
        banner(ui, pal.warn, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                later = ui
                    .add(egui::Button::new("Later").frame_when_inactive(false))
                    .on_hover_text("Leave it where it is: it is offered again next time")
                    .clicked();
                delete = ui
                    .add(egui::Button::new("Delete…"))
                    .on_hover_text("Delete this sound for good. It asks first.")
                    .clicked();
                move_to = ui
                    .add(egui::Button::new("Move To…"))
                    .on_hover_text(
                        "Move the recording's folder into a folder you pick, to open again \
                         with Open Folder… whenever you like",
                    )
                    .clicked();
                open = ui
                    .add(theme::primary(ui, "Open"))
                    .on_hover_text(
                        "Analyse it here. It stays where it is and is offered again next time \
                         until it is moved or deleted.",
                    )
                    .clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(&what).color(pal.text)).wrap());
                });
            });
        });
        if open {
            self.kept.remove(0);
            self.input = Input::File;
            self.file_path = k.dir.display().to_string();
            self.open_recording(&k.dir, true);
        } else if move_to {
            if let Some(parent) = rfd::FileDialog::new()
                .set_title("Move the Unsaved Sound Into a Folder")
                .pick_folder()
            {
                match move_kept(&k.dir, &parent) {
                    Ok(to) => {
                        self.kept.remove(0);
                        self.info_msg(format!("Moved the unsaved sound to {}.", to.display()));
                    }
                    Err(e) => self.error(e),
                }
            }
        } else if delete {
            self.kept_delete_ask = Some(k.dir.clone());
        } else if later {
            self.kept.clear();
        }
    }

    /// Delete… on kept sound asks first: it can't be got back.
    fn kept_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(dir) = self.kept_delete_ask.clone() else {
            return;
        };
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("delete kept sound")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new("Delete the unsaved sound?").font(theme::semibold(15.0)));
            ui.add_space(4.0);
            ui.label("It was never saved anywhere else, so it can't be got back.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(theme::destructive(ui, "Delete")).clicked() {
                    choice = Some(true);
                }
                if ui
                    .add(egui::Button::new("Cancel").min_size(Vec2::new(0.0, 26.0)))
                    .clicked()
                {
                    choice = Some(false);
                }
            });
        });
        if choice.is_none() && modal.should_close() {
            choice = Some(false);
        }
        if let Some(go) = choice {
            self.kept_delete_ask = None;
            if go {
                timegrapher_core::recorder::remove_stale_lock(&dir);
                match std::fs::remove_dir_all(&dir) {
                    Ok(()) => self.kept.retain(|k| k.dir != dir),
                    Err(e) => self.error(format!("Couldn't delete {}: {e}", dir.display())),
                }
            }
        }
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
        if self.setup_sheet {
            // The sheet says it.
            return;
        }
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
        self.kept_banner(ui);
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
        let shown = self.live.is_some() && !self.mic_session_set_aside();
        if shown {
            self.clipping_banner(ui);
        }
        // The readings, and on them what a take needs, unless hidden.
        if self.show_readings {
            let out = self.readouts(ui);
            if out.keep_clock {
                self.keep_clock();
            }
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
        keep_room(&mut panes.tiles, |p| need.get(&p).copied());
        panes.ui(&mut PaneBehavior { app: self }, ui);
        self.panes = panes;
    }

    /// The readings averaged over `seconds` for the strip's window from
    /// `from` to `end`, and how far apart they are. The run kept for the
    /// whole take serves when it is close enough there; a long take opened
    /// whole keeps one only every take/4000 s (6.7 s over 7½ hours), too
    /// sparse for a short strip, so they are worked out afresh.
    fn strip_trend(&mut self, from: f64, end: f64, seconds: f64) -> (Vec<TrendPoint>, f64) {
        let step = strip_step(end - from);
        let kept: Vec<TrendPoint> = self
            .trend_for(seconds)
            .iter()
            .filter(|p| p.t >= from && p.t <= end)
            .copied()
            .collect();
        let spacing = if kept.len() > 1 {
            (kept[kept.len() - 1].t - kept[0].t) / (kept.len() - 1) as f64
        } else {
            f64::INFINITY
        };
        let Some(live) = &self.live else {
            return (kept, spacing.min(step));
        };
        if spacing <= 2.0 * step {
            return (kept, spacing);
        }
        let key: StripKey = (
            (from / step).floor() as i64,
            (end / step).ceil() as i64,
            trend_key(seconds),
            step.to_bits(),
            live.duration_s().to_bits(),
        );
        if let Some((k, pts)) = &self.strip_points {
            if *k == key {
                return (pts.clone(), step);
            }
        }
        let mut pts = Vec::new();
        let mut t = key.0 as f64 * step;
        while t <= (key.1 as f64 * step).min(live.duration_s()) + 1e-9 {
            if t > 0.0 && self.session.replay_reading_allowed(t) {
                pts.push(TrendPoint::of(t, &live.reading_at(t, seconds)));
            }
            t += step;
        }
        self.strip_points = Some((key, pts.clone()));
        (pts, step)
    }

    fn strip_pane(&mut self, ui: &mut egui::Ui) {
        // Before there is a take to show, or while nothing in it beats
        // regularly: where things stand, in the strip's place.
        if self.live.is_none() || self.mic_session_set_aside() {
            return self.empty_state(ui);
        }
        // No regular beat yet: the peaks heard, faint, if any.
        let guessing = self.no_watch_yet();
        let candidates = match &self.live {
            Some(l) if guessing => l.candidates(),
            _ => Vec::new(),
        };
        if guessing && candidates.is_empty() {
            return self.no_watch(ui);
        }
        if self.follow {
            self.centre();
        }
        let end = self.strip_end_s();
        let looking_back = self.strip_end.is_some() && end < self.latest_s() - 0.5;
        let note = if guessing {
            Some(
                "No regular beat found yet. The faint dots are sounds that might be beats."
                    .to_string(),
            )
        } else {
            looking_back.then(|| {
                format!(
                    "at {}, double-click for the newest beats",
                    strip::fmt_time(end)
                )
            })
        };
        let from = end - self.strip.span_s - 5.0;
        let (trend, step) = self.strip_trend(from, end, self.average_for(Pane::Strip));
        let Some(live) = &self.live else { return };
        let period = live.bph().map_or(0.125, |b| 3600.0 / b as f64);
        // The strip's own Average Over, as its overlays use.
        // The guides follow the rate line's slope, so it is worked out for
        // either.
        let rate_line = (self.rate_line || self.rate_guides)
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
        // A gap in the readings, not just their spacing, is a gap in a line.
        let gap_s = (2.5 * step).max(5.0);
        let series = |f: fn(&TrendPoint) -> Option<f64>| -> Vec<[f64; 2]> {
            trend
                .iter()
                .filter(|p| p.t >= from && p.t <= end)
                .filter_map(|p| f(p).map(|v| [p.t, v]))
                .collect()
        };
        let mut overlays = Vec::new();
        // Rate, Amplitude, Beat Error, as the toolbar orders them.
        if self.overlays[1] {
            overlays.push(strip::Overlay {
                name: "Rate",
                unit: " s/d",
                decimals: 1,
                color: pal.trace_rate,
                min_span: 4.0,
                points: series(|p| p.rate),
                gap_s,
            });
        }
        if self.overlays[0] {
            overlays.push(strip::Overlay {
                name: "Amplitude",
                unit: "°",
                decimals: 0,
                color: pal.trace_amplitude,
                min_span: 10.0,
                points: series(|p| p.amplitude),
                gap_s,
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
                gap_s,
            });
        }
        let bands = self.position_bands();
        let mut settles = self.strip_settles();
        if guessing {
            // Drawn faintly, as the beats the readings leave out are.
            overlays.clear();
            settles = vec![(from, end + 1.0, "Possible Beats")];
        }
        let rate_line = rate_line.filter(|_| !guessing);
        let extras = strip::Extras {
            note: note.as_deref(),
            rate_line,
            line: self.rate_line,
            guides: self.rate_guides,
            overlays: &overlays,
            cursor_t: self.cursor_t,
            gutter: GUTTER,
            bands: &bands,
            settles: &settles,
            measured: &self.guided_measured,
            masked: &self.masked,
            masking: self.masking,
        };
        let Some(live) = &self.live else { return };
        // The possible beats numbered at the strip's beat period from the
        // first, so each lands on the strip, wrapped as the beats are.
        let candidates: Vec<timegrapher_core::beats::Beat> = candidates
            .first()
            .map(|first| {
                candidates
                    .iter()
                    .map(|b| timegrapher_core::beats::Beat {
                        index: first.index + ((b.time - first.time) / period).round() as i64,
                        time: b.time,
                        quality: 1.0,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let anchor = if guessing {
            candidates.first().map(|b| strip::Anchor {
                index: b.index,
                time: b.time,
            })
        } else {
            self.anchor
        };
        let input = strip::draw_strip(
            ui,
            if guessing { &candidates } else { live.beats() },
            period,
            anchor,
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
        let end = self.latest_s();
        let latest = self.running();
        let mut note = None;
        let shown: Profiles = if latest {
            self.live
                .as_ref()
                .map_or([None, None], |l| l.tick_profiles().clone())
        } else {
            // Stopped: what the session kept nearest before the end,
            // else work it out from the file.
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
        let latest = self.running();
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
        let profiles = self.profiles_at(self.latest_s());
        let Some(live) = &self.live else {
            ui.label(
                RichText::new("Listen to a watch or open a recording to see its sound here.")
                    .color(pal.text_tertiary),
            );
            return;
        };
        if self.no_sound {
            ui.label(
                RichText::new("This take's sound was removed, so there is no waveform to show.")
                    .color(pal.text_tertiary),
            );
            return;
        }
        let end = self.latest_s();
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
        let listening = self.running();
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
    /// side) from the recording open.
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
        if let Some((a, b)) = input.mask {
            self.mask(a, b);
        }
        if let Some(t) = input.unmask_at {
            self.unmask_at(t);
        }
        if let Some((was, now)) = input.reshape {
            self.reshape_mask(was, now);
        }
        if input.span_factor != 1.0 {
            let was = self.strip.span_s;
            self.strip.set_span(was * input.span_factor);
            // The time under the pointer stays under it.
            if let Some(t) = input.zoom_at {
                let latest = self.latest_s();
                let to = t + (end - t) * self.strip.span_s / was;
                self.strip_end = if to >= latest {
                    None
                } else {
                    Some(to.max(self.strip.span_s.min(latest)))
                };
            }
        }
        if input.width_factor != 1.0 {
            self.strip
                .set_half_width(self.strip.half_width_ms * input.width_factor);
        }
        if input.time_shift_s != 0.0 {
            let latest = self.latest_s();
            let to = end + input.time_shift_s;
            self.strip_end = if to >= latest {
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
            self.strip_end = None;
            self.centre();
        }
    }

    /// The 2-second stretches of amplitude and beat error, leaving out any
    /// that overlap a masked stretch or reach across a break, as the
    /// readings do.
    fn unmasked_windows(&self) -> Vec<&timegrapher_core::amplitude::AmplitudeWindow> {
        let breaks = self.live.as_ref().map_or(&[][..], |l| l.breaks());
        self.live
            .as_ref()
            .map_or(&[][..], |l| l.amplitude_windows())
            .iter()
            .filter(|w| !breaks.iter().any(|&b| w.start_s < b && b < w.end_s))
            .filter(|w| {
                !self
                    .masked
                    .iter()
                    .any(|&(a, b)| w.start_s < b && w.end_s > a)
            })
            .collect()
    }

    /// The Distributions pane: a plot for each reading shown, one above
    /// the other, sharing the pane's height.
    fn distributions_pane(&mut self, ui: &mut egui::Ui) {
        let shown = self.dist_series;
        self.note_need(ui, Pane::Distributions, DIST_MIN_H);
        stacked(ui, shown, DIST_MIN_H, |ui, k| self.histogram(ui, k));
    }

    /// How often each value of a reading came up, over the session or the
    /// strip's length: bars, or the cumulative share.
    fn histogram(&mut self, ui: &mut egui::Ui, reading: SeriesKind) {
        use timegrapher_core::histogram as hist;
        let pal = theme::pal(ui);
        let short = !self.hist_readings;
        let pane = Pane::Distributions;
        if reading == SeriesKind::Rate && short {
            match self.live.as_ref() {
                Some(live) => self.rate_2s.refresh(live, &self.masked),
                None => self.rate_2s = RateTwoS::default(),
            }
        }
        // Only the position the watch is in now: values from another
        // position are a different measurement.
        let inside = |t: f64| self.in_position(t);
        let windows = self.unmasked_windows();
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
        let (values, resolution, unit, decimals, color, per): (
            Vec<f64>,
            f64,
            &str,
            usize,
            _,
            &str,
        ) = match reading {
            SeriesKind::Rate if short => (
                self.rate_2s
                    .values
                    .iter()
                    .filter(|&&(a, _)| inside(a + RateTwoS::STEP_S) && self.in_position(a + 1e-6))
                    .map(|&(_, v)| v)
                    .collect(),
                0.1,
                " s/day",
                1,
                pal.trace_rate,
                "2-second stretches",
            ),
            SeriesKind::Rate => (
                readings(|p| p.rate),
                0.1,
                " s/day",
                1,
                pal.trace_rate,
                "readings",
            ),
            SeriesKind::Amplitude if short => (
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
            SeriesKind::Amplitude => (
                readings(|p| p.amplitude),
                0.5,
                "°",
                0,
                pal.trace_amplitude,
                "readings",
            ),
            SeriesKind::BeatError if short => (
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
            SeriesKind::BeatError => (
                readings(|p| p.beat_error_unlock),
                0.01,
                " ms",
                2,
                pal.trace_beat_error,
                "readings",
            ),
        };
        let opt = hist::Options {
            resolution,
            width_factor: 1.0,
            trim: 0.005,
        };
        let Some(h) = hist::histogram_with(&values, &opt) else {
            ui.label(
                RichText::new(format!(
                    "{}: not enough readings yet",
                    steady::title(reading)
                ))
                .small()
                .color(pal.text_secondary),
            );
            return;
        };
        let peaks = h.peaks(0.05);
        let f = |v: f64| format!("{v:.decimals$}{unit}");
        let over = " in the session";
        let mut line = format!(
            "{}: {} {per}{over} · median {} · middle 80% {} to {}",
            steady::title(reading),
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
        let plot = Plot::new(format!("{} Distribution", steady::title(reading)))
            // As short as its share of the pane, rather than spilling out.
            .min_size(Vec2::new(64.0, 24.0))
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

    /// The Charts pane: a chart for each reading shown, one above the
    /// other, sharing the pane's height and its time axis.
    fn charts_pane(&mut self, ui: &mut egui::Ui) {
        let shown = self.chart_series;
        self.note_need(ui, Pane::Charts, STACKED_MIN_H);
        self.charts_drawn = stacked(ui, shown, STACKED_MIN_H, |ui, k| self.chart(ui, k));
    }

    /// Keep the height `pane` needs: what is above its plots, one plot
    /// `least` tall, the line naming those with no room, and the margin.
    fn note_need(&mut self, ui: &egui::Ui, pane: Pane, least: f32) {
        let above = ui.cursor().top() - self.pane_top;
        self.pane_need
            .insert(pane, above + least + STACKED_NOTE_H + 6.0);
    }

    fn chart(&mut self, ui: &mut egui::Ui, reading: SeriesKind) {
        let pal = theme::pal(ui);
        let weak = pal.text_tertiary;
        let average = self.average_for(Pane::Charts);
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
        let (series, unit, decimals): (Vec<ChartLine>, &'static str, usize) = match reading {
            SeriesKind::Rate => (
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
            SeriesKind::Amplitude => (
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
            SeriesKind::BeatError => (
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
        };
        let mut plot = Plot::new(steady::title(reading))
            // As short as its share of the pane, rather than spilling out.
            .min_size(Vec2::new(64.0, 24.0))
            .link_axis("trend", [true, false])
            .link_cursor("trend", [true, false])
            .allow_scroll(false)
            .allow_zoom([false, true])
            // The wheel zooms and a drag pans; the axes stay put.
            .allow_axis_zoom_drag(false)
            .allow_boxed_zoom(false)
            .x_grid_spacer(|g| theme::even_grid(g, 80.0, &theme::TIME_STEPS))
            .custom_x_axes(vec![egui_plot::AxisHints::new_x()
                .formatter(|m, r| chart_time_label(m.value, m.step_size, r))])
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
        // Scale to hold every point of the lines: the whole take.
        let all: Vec<[f64; 2]> = series.iter().flat_map(|s| s.pts.iter().copied()).collect();
        let y_range = data_range(&all);
        if let Some((lo, hi)) = y_range {
            plot = plot.default_y_bounds(lo, hi);
        }
        let marker = self.strip_end.is_some().then(|| self.strip_end_s());
        let lookup = series.clone();
        let colour = series.first().map_or(pal.text_secondary, |l| l.key);
        let to_live = self.charts_to_live_now;
        // A new averaging time spreads the values wider or narrower, so the
        // scale follows it, keeping the time shown.
        let rescale = self
            .chart_average_drawn
            .insert(steady::title(reading), average)
            .is_some_and(|was| was != average);
        let resp = plot.show(ui, |p| {
            if to_live {
                p.set_auto_bounds([true, true]);
            }
            if let (Some((lo, hi)), true) = (y_range, rescale) {
                p.set_plot_bounds_y(lo..=hi);
            }
            if p.response().hovered() {
                let s = p.ctx().input(|i| i.smooth_scroll_delta);
                let wheel = s.x + s.y;
                if wheel != 0.0 {
                    p.zoom_bounds_around_hovered(Vec2::new((wheel * 0.003).exp(), 1.0));
                }
            }
            for l in series {
                let w = if l.main { 1.8_f32 } else { 1.2_f32 };
                // Broken where there is no reading (a masked stretch, a
                // pause, beats not found), not drawn straight across.
                for seg in segments(&l.pts) {
                    if let Some(c) = l.under {
                        p.line(
                            Line::new(format!("{} gaps", l.name), seg.clone())
                                .color(c)
                                .width(w),
                        );
                    }
                    p.line(
                        Line::new(l.name.clone(), seg)
                            .color(l.color)
                            .style(l.style)
                            .width(w),
                    );
                }
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
        let t = resp.transform;
        paint_y_labels(ui, &t, unit);
        // Which reading this chart is, in its colour, at its top left.
        painter.text(
            frame.left_top() + Vec2::new(6.0, 3.0),
            egui::Align2::LEFT_TOP,
            steady::title(reading),
            egui::TextStyle::Small.resolve(ui.style()),
            colour,
        );
        // Its lines' key at its top right, beside the lines it names.
        let only = steady::SERIES.map(|k| k == reading);
        paint_key(&painter, frame, &chart_key(only, pal), ui, pal);
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
                self.strip_end = Some(at.x);
            }
        }
    }
}

/// The usual spacing of points sorted by x: the median gap.
fn spacing(points: &[[f64; 2]]) -> f64 {
    let mut gaps: Vec<f64> = points.windows(2).map(|w| w[1][0] - w[0][0]).collect();
    gaps.sort_by(f64::total_cmp);
    gaps.get(gaps.len() / 2).copied().unwrap_or(f64::INFINITY)
}

/// Points sorted by x in runs, split wherever one is missing: a gap of
/// more than half again their usual spacing.
fn segments(points: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
    let gap = 1.5 * spacing(points);
    let mut out: Vec<Vec<[f64; 2]>> = Vec::new();
    for (i, &p) in points.iter().enumerate() {
        if i == 0 || p[0] - points[i - 1][0] > gap {
            out.push(Vec::new());
        }
        if let Some(seg) = out.last_mut() {
            seg.push(p);
        }
    }
    out
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
    ((points[near][0] - x).abs() <= 3.0 * spacing(points)).then_some(points[near][1])
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
        strip::fmt_time(t)
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
fn paint_key(
    painter: &egui::Painter,
    frame: egui::Rect,
    key: &[(&'static str, Color32, Option<Color32>)],
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
        let line = format!(
            "{} not shown: no room. Make the pane taller, or turn off {}.",
            names.join(" and "),
            switches.join(" or ")
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
/// The line under stacked plots naming those with no room.
const STACKED_NOTE_H: f32 = 18.0;

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
/// drawn is inside the chart.
fn data_range(points: &[[f64; 2]]) -> Option<(f64, f64)> {
    let (lo, hi) = points
        .iter()
        .map(|p| p[1])
        .filter(|y| y.is_finite())
        .fold(None, |r: Option<(f64, f64)>, y| {
            Some(r.map_or((y, y), |(lo, hi)| (lo.min(y), hi.max(y))))
        })?;
    let pad = ((hi - lo) * 0.08).max(0.05);
    Some((lo - pad, hi + pad))
}

/// A pane being saved as a picture: it's drawn with its name in place of
/// its toolbar until saved, an SVG from that frame's shapes, a PNG from a
/// screenshot of it.
struct PaneExport {
    pane: Pane,
    path: PathBuf,
    /// Where the pane was drawn, once it has been.
    rect: Option<egui::Rect>,
    /// Whether the screenshot has been asked for.
    asked: bool,
    /// Frames waited, so a picture that can't be taken gives up.
    frames: u32,
}

struct PaneBehavior<'a> {
    app: &'a mut TimegrapherApp,
}

impl egui_tiles::Behavior<Pane> for PaneBehavior<'_> {
    /// Each pane is a card under its tab.
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile: TileId, pane: &mut Pane) -> UiResponse {
        let pal = theme::pal(ui);
        let rect = ui.max_rect();
        self.app.pane_top = rect.top();
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::CARD_RADIUS), pal.card);
        let inner = rect.shrink2(Vec2::new(8.0, 6.0));
        // Saving the pane as a picture: its name in place of its toolbar,
        // and what it paints kept for an SVG.
        let exporting = self.app.export.as_ref().is_some_and(|e| e.pane == *pane);
        let layer = ui.layer_id();
        let first = ui
            .ctx()
            .graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            if exporting {
                self.app.export_heading(ui, *pane);
                ui.add_space(4.0);
            } else if self.app.pane_tools(ui, *pane) {
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
                Pane::PowerReserve => {
                    self.app.results_rects[1] = Some(ui.max_rect());
                    self.app.reserve_pane(ui)
                }
                Pane::Scope => self.app.scope_pane(ui),
                Pane::Distributions => self.app.distributions_pane(ui),
                Pane::Charts => self.app.charts_pane(ui),
                _ => {}
            }
        });
        if exporting {
            let shapes = ui.ctx().graphics(|g| {
                g.get(layer)
                    .map(|l| l.all_entries().skip(first).cloned().collect())
                    .unwrap_or_default()
            });
            self.app
                .pane_drawn_for_export(ui.ctx(), inner, shapes, pal.card);
        }
        UiResponse::None
    }

    /// A pane averaging over a time of its own names it.
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        RichText::new(self.app.pane_heading(*pane))
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
            self.app.info_msg(format!(
                "Hid {}. Its icon at the top, or the View menu, brings it back.",
                p.title()
            ));
        }
        false
    }

    /// No pane squeezed to a sliver on a short window: the Paper Strip,
    /// the main view, keeps room to read under the cards.
    fn min_size(&self) -> f32 {
        PANE_MIN_H
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
        TAB_BAR_H
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

/// Small text on a reading card, wrapped whole and never cut short, for
/// lines that may be what says the figure isn't a watch's: the limits
/// under the title, and the line under the figure. Its room is there
/// (empty) or not, as many lines as `longest` takes at `inner`, the width
/// inside every card of figures alike, so the cards' lines stay level and
/// their height follows the window's width alone; only text longer still
/// takes more.
fn whole_lines(ui: &mut egui::Ui, short: &str, long: &str, longest: &str, inner: f32) {
    let pal = theme::pal(ui);
    let w = ui.available_width();
    let font = egui::TextStyle::Small.resolve(ui.style());
    let lines = |ui: &egui::Ui, text: &str, w: f32| {
        ui.fonts_mut(|f| {
            f.layout(text.to_string(), font.clone(), pal.text_secondary, w)
                .rows
                .len()
        })
    };
    let n = lines(ui, longest, inner).max(lines(ui, short, w)).max(1);
    let h = n as f32 * ui.text_style_height(&egui::TextStyle::Small) + 2.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if !short.is_empty() {
        let l = child
            .add(egui::Label::new(RichText::new(short).small().color(pal.text_secondary)).wrap());
        if !long.is_empty() {
            l.on_hover_text(long);
        }
    }
}

/// The least a pane is squeezed to, points.
const PANE_MIN_H: f32 = 120.0;

/// The longest limits a reading card usually names, as on noise taken for
/// beats: the card keeps room for them at every width.
const LONGEST_LIMITS: &str = "9 s of beats · 25 of 66 beats";

/// The longest line a reading card gives under its figure that must show
/// whole: the card keeps room for it at every width.
const LONGEST_STATUS: &str = "waiting for beats";

/// What the settling after a move is, wherever it is shown.
const SETTLE_HINT: &str = "For the first 10 seconds after the watch is put in a new position \
     it settles: the hand leaves the stand and the \
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

/// Set `masked` on the entries of the session file beside `path` that
/// list it, leaving the rest of the file as it was, comments and keys this
/// app doesn't read too. Returns the session file.
fn save_masks_to_take(path: &Path, masked: &[[f64; 2]]) -> Result<PathBuf, String> {
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
    let mut changed = 0;
    match doc.get_mut("recording") {
        Some(toml_edit::Item::ArrayOfTables(tables)) => {
            for t in tables.iter_mut().filter(|t| listed(*t)) {
                set(t);
                changed += 1;
            }
        }
        Some(toml_edit::Item::Value(toml_edit::Value::Array(entries))) => {
            for t in entries.iter_mut().filter_map(|v| v.as_inline_table_mut()) {
                if listed(t) {
                    set(t);
                    changed += 1;
                }
            }
        }
        _ => {}
    }
    if changed == 0 {
        return Err(format!(
            "{} doesn't list {}",
            file.display(),
            capture_label(path)
        ));
    }
    write_replacing(&file, &doc.to_string())?;
    Ok(file)
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

/// A size on disk, as a file manager shows it: 7.7 GB, 10.5 MB, 812 kB.
pub(crate) fn fmt_bytes(n: u64) -> String {
    let n = n as f64;
    match n {
        n if n >= 1e9 => format!("{:.1} GB", n / 1e9),
        n if n >= 1e6 => format!("{:.1} MB", n / 1e6),
        n if n >= 1e3 => format!("{:.0} kB", n / 1e3),
        n => format!("{n:.0} bytes"),
    }
}

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
        if ctx.input_mut(|i| i.consume_shortcut(&command(egui::Key::S))) {
            if self.masks_unsaved() {
                self.save_masks();
            } else if self.session.allows(Action::Save) {
                self.save_again();
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&command(egui::Key::Q))) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Keys 1 to 6 put the watch in a position, unless a field is
        // being typed in.
        if !ctx.wants_keyboard_input() {
            let keys = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
                egui::Key::Num6,
            ];
            let pressed = ctx.input(|i| keys.iter().position(|&k| i.key_pressed(k)));
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
            .show(ctx, |ui| self.main_view(ui));
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
        self.kept_delete_dialog(ctx);
        self.watch_form_dialog(ctx);
        self.test_keep_dialog(ctx);
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
     As Readings follows Average Over on the Listen card. A time of its own sets \
     everything in this pane that Average Over sets, and is named on the pane's tab, until \
     Average Over on the Listen card is changed: then every pane follows that again.";

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
    use timegrapher_core::measure::{self, NoWatchReason};
    if let Some(why @ NoWatchReason::NoBeatSound) = measure::no_watch_reason_in(&r.caveats) {
        return measure::no_watch_text(why, None, None);
    }
    match (r.jitter_us, r.bph) {
        (Some(j), Some(bph)) => format!(
            "No regular beat found (timing scatter {:.1} ms, {:.1}% of a beat)",
            j / 1000.0,
            j / (3.6e9 / bph.max(1) as f64) * 100.0
        ),
        _ => "No regular beat found".to_string(),
    }
}

/// The Beat Error card's line under the whole take, when it is of sizes.
/// What [`TimegrapherApp::whole_take`] was read from: the beats heard,
/// the stretches, the masks and the clock's ppm.
type WholeKey = (usize, Vec<(f64, f64)>, Vec<(f64, f64)>, Option<f64>);

/// The whole take in one position, and the seconds it covers.
#[derive(Clone)]
struct WholeTake {
    m: timegrapher_core::session::Measurement,
    span: f64,
}

const WHOLE_AS_SIZES: &str = "whole take is given as a size: its sign differs either side of a cut";

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
                    format!("{beats} of {expected} beats")
                } else {
                    format!("{:.0}% of beats found", fraction * 100.0)
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
                format!("{:.0}% clipped", fraction * 100.0),
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
            (Caveat::SidesByCount { count }, "Beat Error") => Some((
                "sides named by count".to_string(),
                format!(
                    "Tick and Tock are named by the beat count across {count} break{} in \
                     this stretch, not told apart by their sound.",
                    if *count == 1 { "" } else { "s" }
                ),
            )),
            (Caveat::Breaks { count }, _) => Some((
                format!("{count} break{}", if *count == 1 { "" } else { "s" }),
                format!(
                    "Listening broke off {count} {} in this stretch, such as when the \
                     computer slept.",
                    if *count == 1 { "time" } else { "times" }
                ),
            )),
            // A sliver that would show as 0.0 s is left unsaid.
            (Caveat::Masked { seconds }, _) if *seconds < 0.05 => None,
            (Caveat::Masked { seconds }, _) => {
                // A mask reaching just into the averaging time is under a
                // second, which whole seconds would show as 0.
                let s = if *seconds < 9.95 {
                    format!("{seconds:.1}")
                } else {
                    format!("{seconds:.0}")
                };
                Some((
                    format!("{s} s masked"),
                    format!(
                        "{s} s of this stretch were masked out on the Paper Strip and left out \
                         of the readings."
                    ),
                ))
            }
            _ => None,
        };
        out.extend(fact);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The settling the tests of a move set the pane's Settle to.
    const SETTLE_S: f64 = 10.0;

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

    fn synthetic(seconds: f64, rate: f64) -> TimegrapherApp {
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

    /// The lift angle is typed only with no calibre setting it, in Quick
    /// Listen too.
    #[test]
    fn a_calibre_picked_sets_the_lift_angle() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        assert!(app.open_watch().is_none());
        assert!(app.lift_settable());
        app.set_calibre("ETA 2824-2".into());
        assert!(app.cal.lift("ETA 2824-2").is_some());
        assert!(!app.lift_settable());
        app.set_calibre(String::new());
        assert!(app.lift_settable());
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

    /// The rate distribution's 2 s values are the rate over each 2 seconds
    /// of beats, none across a break or into a masked stretch.
    #[test]
    fn rate_distribution_counts_two_second_stretches() {
        let mut app = synthetic(30.0, 12.0);
        let live = app.live.as_mut().unwrap();
        live.set_breaks(vec![15.0]);
        let mut r = RateTwoS::default();
        r.refresh(live, &[(5.0, 7.5)]);
        assert!(r.values.len() >= 10, "{}", r.values.len());
        for &(a, v) in &r.values {
            let b = a + RateTwoS::STEP_S;
            assert!(!(a < 15.0 && 15.0 < b), "across the break at {a}");
            assert!(!(a < 7.5 && b > 5.0), "into the mask at {a}");
            assert!((v - 12.0).abs() < 20.0, "{v} s/day at {a}");
        }
        // Refreshing again adds nothing; a change of mask starts again.
        let n = r.values.len();
        r.refresh(live, &[(5.0, 7.5)]);
        assert_eq!(r.values.len(), n);
        r.refresh(live, &[]);
        assert!(r.values.len() > n);
        // The pane follows Values for rate too.
        app.hist_readings = false;
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.histogram(ui, SeriesKind::Rate));
        });
        assert!(!app.rate_2s.values.is_empty());
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

    /// The whole take's beat error, where its sign turns over at a jump
    /// in the beats, combines the sizes either side and says so, as
    /// `session` does, not a figure that cancels to nothing; where the
    /// beats agree in sign either side it stays signed.
    #[test]
    fn whole_take_beat_error_keeps_its_size_across_a_turn() {
        // 120 s at 28,800 bph with a jump at 80 s: the tock 0.6 ms late
        // for the 640 beats before it and `after` ms for the 320 after.
        let app_with = |after: f64| {
            let beats = (0..960)
                .map(|index| {
                    let be = if index < 640 { 0.6 } else { after };
                    let half = be / 2000.0 * if index % 2 == 1 { 1.0 } else { -1.0 };
                    timegrapher_core::beats::Beat {
                        time: index as f64 * 0.125 + half,
                        index,
                        quality: 1.0,
                    }
                })
                .collect();
            let log = BeatLog {
                sample_rate: 48_000,
                duration_s: 120.0,
                bph: 28_800,
                lift_deg: 52.0,
                beats,
                amplitude_windows: Vec::new(),
                clipped_beats: Vec::new(),
                clipped_samples: 0,
                jumps: vec![80.0],
                sided: Vec::new(),
                by_count: Vec::new(),
                snippets: Vec::new(),
            };
            let mut app = TimegrapherApp::with_devices(Vec::new(), None);
            app.live = Some(LiveAnalyzer::from_log(log, LiveConfig::default()));
            app
        };
        let sizes = |w: &WholeTake| w.m.caveats.contains(&Caveat::BeatErrorAsSize);
        // Sizes across the turn, beat-weighted: (0.6 × 640 + 0.4 × 320) / 960.
        let size = (0.6 * 640.0 + 0.4 * 320.0) / 960.0;
        // What a signed combination would cancel to: (0.6 × 640 − 0.4 × 320) / 960.
        let cancelled = (0.6 * 640.0 - 0.4 * 320.0) / 960.0;
        let w = app_with(-0.4).whole_take().expect("whole take");
        let be = w.m.beat_error_ms.unwrap();
        assert!(sizes(&w));
        assert!((be - size).abs() < 1e-3, "{be} vs {size}");
        assert_eq!(w.m.beat_error(), Some(be));
        assert!((be - cancelled).abs() > 0.2, "{be} all but cancelled");
        // Where the beats keep one sign either side, the figure keeps it:
        // the same size, signed.
        let w = app_with(0.4).whole_take().expect("whole take");
        let be = w.m.beat_error_ms.unwrap();
        assert!(!sizes(&w));
        assert!((be.abs() - size).abs() < 1e-3, "{be} vs {size}");
        assert!(be < 0.0, "signed: {be}");
    }

    /// The whole take line is `session`'s reading of the same stretch:
    /// the rate fitted over every beat, not a mean of the readings shown.
    #[test]
    fn whole_take_is_the_session_reading() {
        let app = synthetic(60.0, 12.0);
        let w = app.whole_take().expect("whole take");
        let log = steady::log_of(app.live.as_ref().unwrap()).unwrap();
        let spans = app.current_spans();
        let (from, to) = (spans[0].0.max(0.0), app.latest_s());
        let m = timegrapher_core::session::measure(&log, None, from, to);
        assert_eq!(w.m.rate_s_per_day, m.rate_s_per_day);
        assert_eq!(w.m.amplitude_deg, m.amplitude_deg);
        assert!((w.span - (to - from)).abs() < 1e-9, "{}", w.span);
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

    /// The Charts and Distributions panes draw a plot for each reading
    /// turned on, and its switches keep at least one on.
    #[test]
    fn the_charts_and_distributions_draw_the_readings_turned_on() {
        let mut app = synthetic(30.0, 20.0);
        for p in [Pane::Charts, Pane::Distributions] {
            set_pane_visible(&mut app.panes.tiles, p, true);
        }
        // Room for every plot: the strip, the profile and the Scope out of
        // the way.
        for p in [Pane::Strip, Pane::Sound, Pane::Scope] {
            set_pane_visible(&mut app.panes.tiles, p, false);
        }
        let ctx = themed();
        // Each pane in turn: they are tabs side by side, one on show.
        let draw = |app: &mut TimegrapherApp| {
            let mut all = String::new();
            for p in [Pane::Charts, Pane::Distributions] {
                for q in [Pane::Charts, Pane::Distributions] {
                    set_pane_visible(&mut app.panes.tiles, q, q == p);
                }
                let mut text = String::new();
                for _ in 0..3 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(1440.0, 1000.0),
                        )),
                        ..Default::default()
                    };
                    text = painted_text(&ctx.run(input, |ctx| app.panels(ctx)));
                }
                all.push_str(&text);
            }
            all
        };
        let text = draw(&mut app);
        for t in [
            "Charts",
            "Distributions",
            "s/day",
            "Beat Error:",
            "Amplitude:",
        ] {
            assert!(text.contains(t), "{t}: {text}");
        }
        assert!(!text.contains("Match Strip"), "{text}");
        // Only the rate: no amplitude or beat error plot in either pane.
        app.chart_series = [true, false, false];
        app.dist_series = [true, false, false];
        let text = draw(&mut app);
        assert!(text.contains("Rate"), "{text}");
        for t in ["Beat Error:", "Amplitude:", "From the Unlock", "From Tick"] {
            assert!(!text.contains(t), "{t}: {text}");
        }
        // The last one on stays on; another comes on beside it.
        steady::flip_series(&mut app.chart_series, 0);
        assert_eq!(app.chart_series, [true, false, false]);
        steady::flip_series(&mut app.chart_series, 2);
        assert_eq!(app.chart_series, [true, false, true]);
        steady::flip_series(&mut app.chart_series, 0);
        assert_eq!(app.chart_series, [false, false, true]);
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

    fn save_failure_fixture(name: &str) -> (TimegrapherApp, PathBuf) {
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
            let mut app = TimegrapherApp::with_devices(Vec::new(), None);
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
            let mut reopened = TimegrapherApp::with_devices(Vec::new(), None);
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
        app.wait_for_save();
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
    /// The Watches pane lists the library's watches, adds one (making
    /// the library for the first), shows a watch's details and takes, and
    /// Save… then saves into that watch with no question.
    #[test]
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
            "‹ All Watches",
            "Edit…",
            "Listen",
            "Positions Test",
            "Power Reserve Test",
        ] {
            assert!(drawn.contains(t), "{t}: {drawn}");
        }
        assert_eq!(app.status_words(), "New Take · Pepsi · Not started");

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
        assert!(drawn.contains(&name), "{drawn}");
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

    /// Quick Listen asks before unsaved sound goes, then shows Unsorted
    /// and listens with no watch picked; Stop keeps nothing, Save… saves
    /// the take into Unsorted, which the list shows at its top, and File
    /// Under a Watch moves it into a watch. Picking a watch ends it.
    #[test]
    fn a_power_reserve_test_holds_its_position_and_keeps_its_take_with_its_run() {
        let base = std::env::temp_dir().join(format!("tg-app-reserve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Timegrapher Data");
        let mut lib = library::Library::create(&root).unwrap();
        lib.add(&library::Watch::new("Pepsi")).unwrap();
        let pepsi = lib.find("Pepsi").unwrap().watch.id.clone();
        drop(lib);
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.use_library(Some(root.clone()));
        app.pick_watch(&pepsi);
        // 20 s of sound kept, as a run would keep it (no device is opened).
        app.session.listen();
        let info = app.info("", 48000, 16);
        let mut r = Recorder::start(&base.join("unsaved"), info).unwrap();
        r.write(&timegrapher_core::capture::Block {
            samples: vec![0.0; 48000 * 20],
            at: std::time::SystemTime::now(),
        })
        .unwrap();
        app.session.recorder = Some(r);
        app.position = Position::CrownLeft;
        let mut t = reserve::Test::new(
            Position::CrownLeft,
            true,
            "2026-10-22T16:15:00Z".into(),
            0.0,
        );
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
        // One position for the whole run.
        app.change_position(Position::DialUp);
        assert_eq!(app.position, Position::CrownLeft);

        // Stop ends the test and keeps the take in the watch's history.
        app.stop_listening();
        assert!(!app.reserve_running());
        assert!(app.reserve.as_ref().unwrap().saving);
        app.wait_for_save();
        app.reserve_saved();
        let saved = app.session.saved_to().unwrap().to_path_buf();
        assert_eq!(app.reserve.as_ref().unwrap().saved.as_ref(), Some(&saved));
        assert!(
            saved.starts_with(root.join("Pepsi").join(library::TAKES)),
            "{saved:?}"
        );
        assert!(file_name(&saved).ends_with(" Power Reserve"), "{saved:?}");
        let r = power_reserve::Results::from_json(
            &std::fs::read_to_string(saved.join(power_reserve::FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(r.ending, power_reserve::Ending::ByHand);
        assert_eq!(r.position, "CL");
        assert!((r.run_s - 99.0 * 0.125).abs() < 1e-6, "{}", r.run_s);
        let take = app.open_watch().unwrap().takes[0].clone();
        assert!(watches::take_line(&take).starts_with("Power Reserve 0 h 00 min (still running"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
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
        assert_eq!(app.status_words(), "New Take · Meteorite · Not started");
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
        for t in ["Unsorted Takes", "No takes here", "‹ All Watches"] {
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
        assert!(drawn.contains(&name), "{drawn}");

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

    /// A watch's takes are kept as their analysis alone, moved
    /// and deleted from the Watches pane, and a take from outside is added
    /// to a watch; the recording opened follows a take that moves.
    #[test]
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
            egui::CentralPanel::default().show(ctx, |ui| app.watch_takes_card(ui));
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
                egui::CentralPanel::default().show(ctx, |ui| next.kept_banner(ui));
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
        let block = timegrapher_core::capture::Block {
            samples: audio.samples.clone(),
            at: std::time::SystemTime::now(),
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
        a.save_masks();
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

    /// Record, save, carry on, pause, change position, resume, save again
    /// and reopen: sound after a save still counts as unsaved, Save Again
    /// keeps it, and the saved take keeps the position it started in.
    #[test]
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

    fn reopened(path: &Path) -> TimegrapherApp {
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
        assert!(app.masks_unsaved());
        app.open_recording(&base.join("reverso_DD.wav"), true);
        assert!(
            matches!(app.confirm, Some(Confirm::Open { .. })),
            "asked first"
        );
        app.confirm = None;
        app.save_masks();
        assert!(!app.masks_unsaved());
        let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
        let m: Manifest = toml::from_str(&text).unwrap();
        // What the recorder writes for a take with these settings: the
        // default lift is written as assumed, not chosen.
        let mut want = app.manifest_header();
        want.recordings = vec![RecordingEntry {
            file: ".".into(),
            position: Some("DD".into()),
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
        assert!(app.masks_unsaved());
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
        app.save_masks();
        assert!(app.masks_unsaved(), "still unsaved");
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
        app.save_masks();
        assert!(!app.masks_unsaved());
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
        app.save_masks();
        let text = std::fs::read_to_string(base.join(MANIFEST)).unwrap();
        assert_eq!(text, original.replace("masked = [[1.0, 2.0]]\n", ""));
        assert!(!base.join(".session.toml.saving").exists());
        let _ = std::fs::remove_dir_all(&base);
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
        opened.apply_manifest(&again, &again.recordings);
        assert_eq!(opened.masked, [(10.0, 20.0), (65.0, 68.0)]);
        // Nothing masked: no masked line in the file.
        app.set_masked(Vec::new());
        assert!(!toml::to_string_pretty(&app.take_manifest())
            .unwrap()
            .contains("masked"));
        app.session.discard_recording();
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
        // A mask in the settling gap after the first move is on every
        // entry, though inside none, and comes back when the file is read.
        app.set_masked(vec![(32.0, 38.0)]);
        let m2 = app.take_manifest();
        assert_eq!(m2.recordings.len(), 3);
        for r in &m2.recordings {
            assert_eq!(r.masked, [[32.0, 38.0]]);
        }
        let mut opened = TimegrapherApp::with_devices(Vec::new(), None);
        opened.apply_manifest(&m2, &m2.recordings);
        assert_eq!(opened.masked, [(32.0, 38.0)]);
        app.set_masked(Vec::new());

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
        let (positions, settles) = batch.apply_manifest(&manifest, &entries);
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
        app.pane_average.insert(Pane::Charts, 5.0);
        app.rebuild_trend();
        assert!(!app.trend.is_empty());
        assert!(!app.other_trends[&trend_key(5.0)].is_empty());
        assert!(app.trend.iter().all(|p| p.t <= 30.0 || p.t > 40.0));
        assert!(app
            .other_trends
            .values()
            .flatten()
            .all(|p| p.t <= 30.0 || p.t > 40.0));
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
    /// Profile with the Scope behind it. Every other pane is hidden, the
    /// Watches pane too, and there is no Take or Microphone pane.
    #[test]
    fn the_app_opens_on_the_quick_listen_layout() {
        let app = TimegrapherApp::with_devices(Vec::new(), None);
        let shown: Vec<Pane> = Pane::ALL
            .into_iter()
            .filter(|&p| pane_visible(&app.panes.tiles, p))
            .collect();
        assert_eq!(shown, [Pane::Strip, Pane::Sound, Pane::Scope]);
        assert!(app.panes.tiles.find_pane(&Pane::Watch).is_some());
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
        assert_eq!(back.chart_series, [true; 3]);
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
        set_pane_visible(&mut app.panes.tiles, Pane::Watch, true);
        let mut before = app.settings();
        assert_eq!(before.layout, LAYOUT);
        let mut again = TimegrapherApp::with_devices(Vec::new(), None);
        again.apply_settings(before.clone());
        assert!(pane_visible(&again.panes.tiles, Pane::Watch), "kept");
        // A file from before the version was kept reads as the oldest.
        let none: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(none.layout, 0);
        before.layout = LAYOUT - 1;
        let mut old = TimegrapherApp::with_devices(Vec::new(), None);
        old.apply_settings(before);
        assert!(!pane_visible(&old.panes.tiles, Pane::Watch), "afresh");
    }

    /// The Current Position card's six buttons stay big enough to read and
    /// press: one row where the card gives each 30 points (1280), else two
    /// rows of three (1000, 800). On the narrowest window the card is
    /// captioned Position, and every card of the row stays inside the
    /// window with the Beat Rate card's caption whole.
    #[test]
    fn the_position_buttons_keep_their_size() {
        assert_eq!(position_rows(198.0), 1);
        assert_eq!(position_rows(148.0), 2);
        assert!(position_slot_h(148.0) > position_slot_h(198.0));
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
        for (w, rows, caption) in [
            (1280.0, 1, "Current Position"),
            (1000.0, 2, "Current Position"),
            (800.0, 2, "Position"),
        ] {
            let found = rects(&mut app, w);
            at(&found, caption);
            let (du, cd) = (at(&found, "DU"), at(&found, "CD"));
            if rows == 1 {
                assert!((du.center().y - cd.center().y).abs() < 1.0, "{w}");
            } else {
                assert!(cd.center().y > du.bottom(), "{w}");
            }
            // Next to each other, the codes' centres are far enough apart
            // for a button of 28 points or more each.
            let cu = at(&found, "CU");
            assert!(cu.center().x - du.center().x >= 2.0 * 28.0, "{w}");
            let beat_rate = at(&found, "Beat Rate");
            assert!(beat_rate.right() <= w - 12.0, "{w}: {beat_rate:?}");
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
                        Vec2::new(1000.0, 800.0),
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
        let texts = window_texts(&mut app, &ctx, 1000.0);
        assert!(texts.iter().any(|t| t == "Current Position"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "DU · CH"), "{texts:?}");
        // With room, the name too.
        let texts = window_texts(&mut app, &ctx, 1440.0);
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
        let texts = window_texts(&mut app, &ctx, 1000.0);
        for p in Position::ALL {
            assert!(
                texts.iter().any(|t| t == p.code()),
                "{}: {texts:?}",
                p.code()
            );
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
        assert_eq!(Pane::CORE[0], Pane::Watch);
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
        assert!(!mid.iter().any(|t| t.contains('⏷')), "{mid:?}");
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

    /// The readings' cards carry what a take needs: the position with the
    /// arrows that tip the watch, Listen and Save with Average Over on a
    /// card of their own, the lift angle and the beat rate's menu with the
    /// calibre. Nothing says Take, Pause or Resume, and no Replay is
    /// offered.
    #[test]
    fn the_readings_cards_carry_the_take_controls() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        let ctx = themed();
        for w in [1000.0, 1440.0] {
            let texts = window_texts(&mut app, &ctx, w);
            // The name where it fits on one line, else the code alone.
            if w > 1000.0 {
                assert!(texts.iter().any(|t| t == "Dial Up"), "at {w}: {texts:?}");
            }
            for want in [
                "Current Position",
                "DU",
                "CR",
                "DU · CH",
                "Listen",
                "Lift",
                "Average 10 s",
                "Calibre",
                "Not Set",
                "Watches",
            ] {
                assert!(texts.iter().any(|t| t == want), "{want} at {w}: {texts:?}");
            }
            assert!(
                texts
                    .iter()
                    .any(|t| t.starts_with('—') && t.contains("bph")),
                "the beat rate's menu at {w}: {texts:?}"
            );
            for gone in [
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
                texts.iter().any(|t| t.starts_with("New Take · ")),
                "the status line: {texts:?}"
            );
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

    /// On a card as narrow as the smallest window gives it, the Beat Rate
    /// card's menus stay inside it: the figure drops "bph" and the Calibre
    /// menu goes under its name.
    #[test]
    fn the_beat_rate_controls_fit_a_narrow_card() {
        let mut app = TimegrapherApp::with_devices(Vec::new(), None);
        app.bph = Some(28800);
        let ctx = themed();
        for w in [84.0, 160.0] {
            let mut used = egui::Rect::NOTHING;
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let rect = egui::Rect::from_min_size(ui.cursor().min, Vec2::new(w, 200.0));
                        let mut child = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                        );
                        app.beat_rate_controls(&mut child);
                        used = child.min_rect();
                    });
                });
            }
            assert!(
                used.width() <= w + 0.5,
                "{} pt wide in a {w} pt card",
                used.width()
            );
        }
    }

    /// A card's limits show whole, wrapped, never cut short, in room that
    /// is the same whether they are there or not: at the narrowest card
    /// and a wide one.
    #[test]
    fn a_cards_limits_show_whole_in_room_kept_for_them() {
        let ctx = themed();
        let noise = "9 s of beats · 25 of 66 beats";
        for w in [92.0, 190.0] {
            let mut heights = Vec::new();
            for short in ["", "12 s of beats", noise] {
                let out = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let rect = egui::Rect::from_min_size(ui.cursor().min, Vec2::new(w, 400.0));
                        let mut child = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(rect)
                                .layout(egui::Layout::top_down(egui::Align::Min)),
                        );
                        let top = child.cursor().top();
                        whole_lines(&mut child, short, "", LONGEST_LIMITS, w);
                        heights.push(child.cursor().top() - top);
                    });
                });
                let drawn: String = out
                    .shapes
                    .iter()
                    .filter_map(|c| match &c.shape {
                        egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                        _ => None,
                    })
                    .collect();
                assert!(!drawn.contains('…'), "{drawn}");
                assert_eq!(drawn.replace('\n', " ").trim(), short, "at {w} pt");
            }
            assert!(
                heights.iter().all(|&h| h == heights[0]),
                "{heights:?} at {w} pt"
            );
        }
    }

    /// The reading cards are one height at a width, whatever they show:
    /// waiting, reading, settling, or with their limits named.
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
        // A take opened: the Beat Rate card shows its figure, not its menu.
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
        // Hiss with the beat rate set: a reading of whatever peaks there
        // are, with its limits named.
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
        for w in [800.0, 1280.0, 1900.0] {
            let h = height(&waiting, w);
            assert!(h > 0.0);
            for app in [&reading, &settling, &masked, &noise] {
                assert_eq!(height(app, w), h, "at {w} pt wide");
            }
        }
    }

    /// With no watch picked, or a watch with no calibre, the Beat Rate
    /// card asks for the beat rate and the Amplitude card for the lift
    /// angle; a watch with a calibre the table knows names it instead.
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
                    app.beat_rate_controls(ui);
                    app.lift_control(ui);
                });
            });
            format!("{:?}", out.shapes)
        };
        // The lift angle's field has its ° beside it; a calibre's angle is
        // one label.
        let asks = |d: &str| d.contains("bph · Auto") && d.contains("text: \"°\"");
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
        let mut opening = TimegrapherApp::with_devices(Vec::new(), None);
        opening.apply_settings(set_up);
        opening.input = Input::File;
        opening.at_launch();
        assert!(!opening.setup_sheet && !opening.listen_pending);
    }

    /// The panes start below the readings' cards, whatever the cards'
    /// controls take: with a watch picked the Beat Rate card has one menu
    /// fewer.
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
                for c in &out.shapes {
                    if let egui::Shape::Text(t) = &c.shape {
                        let r = c.shape.visual_bounding_rect();
                        let k = t.galley.text().to_string();
                        top_of.insert(k.clone(), r.top());
                        bottom_of.insert(k, r.bottom());
                    }
                }
            }
            let pane = top_of["Paper Strip"];
            let bph = bottom_of
                .keys()
                .find(|k| k.starts_with('—') && k.contains("bph"))
                .expect("the beat rate's menu")
                .clone();
            for control in ["CR", "Lift", bph.as_str()] {
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
        let cards = top_of["Paper Strip"] - top_of["Current Position"];
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

    /// Settings lists each kept clock; Forget removes that one from the
    /// file the command line reads, and the input listening loses its
    /// True Clock with it. Manage Calibres… opens Settings at Calibres.
    #[test]
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

    /// A pane follows Readings' Average Over until given a time of its
    /// own, which keeps its own readings until Readings' Average Over is
    /// changed: then every pane follows it again.
    #[test]
    fn a_pane_follows_readings_unless_set() {
        let mut app = synthetic(40.0, 0.0);
        assert_eq!(app.average_for(Pane::Charts), 10.0);
        app.set_pane_average(Pane::Charts, Some(30.0));
        assert_eq!(app.average_for(Pane::Charts), 30.0);
        assert_eq!(app.average_for(Pane::Distributions), 10.0);
        let own = app.trend_for(30.0);
        assert!(!own.is_empty());
        assert!(own.last().unwrap().t > 30.0);
        // Back to following: its own readings are dropped.
        app.set_pane_average(Pane::Charts, None);
        assert_eq!(app.average_for(Pane::Charts), 10.0);
        assert!(app.other_trends.is_empty());
        // A new time for Readings takes over every pane's own.
        app.set_pane_average(Pane::Charts, Some(30.0));
        app.set_pane_average(Pane::Strip, Some(60.0));
        app.set_average(5.0);
        assert_eq!(app.average_for(Pane::Charts), 5.0);
        assert_eq!(app.average_for(Pane::Strip), 5.0);
        assert_eq!(app.average_for(Pane::Distributions), 5.0);
        assert!(app.pane_average.is_empty() && app.other_trends.is_empty());
        // Picking Readings' own time is the same as following it.
        app.set_pane_average(Pane::Charts, Some(5.0));
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
        let mut back = TimegrapherApp::with_devices(Vec::new(), None);
        back.apply_settings(s.clone());
        assert_eq!(back.average_for(Pane::Strip), 60.0);
        assert_eq!(back.steady_series[&View::Fft], [false, true, false]);
        assert_eq!(back.steady_series[&View::Cycle], [true; 3]);
        let mut none = s;
        none.steady_shown = vec![(View::Fft, [false; 3])];
        none.pane_average = vec![(Pane::Watch, 30.0), (Pane::Charts, 7.0)];
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
        app.apply_strip_input(back, app.strip_end_s());
        assert!((app.strip_end_s() - (latest - 5.0)).abs() < 1e-6);
        let forward = StripInput {
            time_shift_s: 20.0,
            ..Default::default()
        };
        app.apply_strip_input(forward, app.strip_end_s());
        assert_eq!(
            app.strip_end, None,
            "past the newest beat follows live again"
        );

        app.follow = true;
        let slide = StripInput {
            lead_shift_ms: 1.0,
            ..Default::default()
        };
        let before = app.anchor.unwrap().time;
        app.apply_strip_input(slide, app.strip_end_s());
        assert!((app.anchor.unwrap().time - before - 0.001).abs() < 1e-12);
        assert!(!app.follow, "sliding by hand ends auto-centring");

        let zoom = StripInput {
            span_factor: 2.0,
            width_factor: 0.5,
            ..Default::default()
        };
        app.apply_strip_input(zoom, app.strip_end_s());
        assert_eq!(app.strip.span_s, 60.0);
        assert_eq!(app.strip.half_width_ms, 5.0);

        // Zoomed about a time, that time stays where it was on the strip.
        let end = app.strip_end_s();
        let at = end - 15.0;
        let zoom = StripInput {
            span_factor: 0.5,
            zoom_at: Some(at),
            ..Default::default()
        };
        app.apply_strip_input(zoom, end);
        assert_eq!(app.strip.span_s, 30.0);
        let now = app.strip_end.expect("looking back");
        assert!(((now - at) / 30.0 - 15.0 / 60.0).abs() < 1e-9, "{now}");
    }

    /// New Session while listening, when closing the kept sound fails:
    /// the error stays shown and listening doesn't start again over it.
    #[test]
    fn new_session_keeps_a_close_error() {
        let (mut app, base) = save_failure_fixture("new-session-close");
        let scratch = app.session.recorder.as_ref().unwrap().dir().to_path_buf();
        std::fs::remove_file(scratch.join("session.json")).unwrap();
        std::fs::create_dir(scratch.join("session.json")).unwrap();
        let tx = queued_capture(&mut app, &base);
        assert!(app.running() && app.session.mic());
        app.new_session();
        assert!(!app.running());
        let (m, error) = app.message.clone().expect("the close error is shown");
        assert!(error, "{m}");
        assert!(m.contains("pause"), "{m}");
        drop(tx);
        let _ = std::fs::remove_dir_all(base);
    }

    /// With the window drawing no frames at all (a locked screen, a
    /// minimised window), the sound is still saved and read as it comes in.
    #[test]
    fn listening_carries_on_without_frames() {
        let (mut app, base) = save_failure_fixture("no-frames");
        let tx = queued_capture(&mut app, &base);
        let before = app.heard_s();
        let saved_before = app.session.recorder.as_ref().unwrap().duration_s();
        let shell = crate::pump::Shell::new(app);
        // 10 s of sound in the 20 ms blocks a microphone sends: several
        // passes of the listening thread, and quick enough in a debug build
        // run alongside the other tests.
        for k in 0..500u64 {
            tx.send(Event::Audio(timegrapher_core::capture::Block {
                samples: vec![0.0; 960],
                at: std::time::UNIX_EPOCH + Duration::from_millis(2000 + 20 * k),
            }))
            .unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let (heard, saved) = {
                let a = shell.app();
                (
                    a.heard_s(),
                    a.session.recorder.as_ref().unwrap().duration_s(),
                )
            };
            if heard - before >= 10.0 - 1e-6 && saved - saved_before >= 10.0 - 1e-6 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "heard {heard} s and saved {saved} s without frames"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(shell);
        drop(tx);
        let _ = std::fs::remove_dir_all(base);
    }

    /// A backlog is taken a frame's worth at a time, so the window never
    /// freezes while it catches up.
    #[test]
    fn a_backlog_is_taken_in_slices() {
        let (mut app, base) = save_failure_fixture("backlog");
        let tx = queued_capture(&mut app, &base);
        for k in 0..3000u64 {
            tx.send(Event::Audio(timegrapher_core::capture::Block {
                samples: vec![0.0; 960],
                at: std::time::UNIX_EPOCH + Duration::from_millis(2000 + 20 * k),
            }))
            .unwrap();
        }
        let first = app.poll_for(Duration::from_millis(1));
        assert!((1..3000).contains(&first), "took {first} blocks at once");
        let mut taken = first;
        while taken < 3000 {
            let n = app.poll_for(Duration::from_millis(200));
            assert!(n > 0);
            taken += n;
        }
        assert!(app.running());
        drop(tx);
        let _ = std::fs::remove_dir_all(base);
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

    /// A computer that slept mid-take carries on, with the gap a break.
    #[test]
    fn a_sleep_becomes_a_break() {
        let (mut app, base) = save_failure_fixture("slept");
        let tx = queued_capture(&mut app, &base);
        let block = |ms: u64| {
            Event::Audio(timegrapher_core::capture::Block {
                samples: vec![0.0; 960],
                at: std::time::UNIX_EPOCH + Duration::from_millis(ms),
            })
        };
        for k in 0..50 {
            tx.send(block(2000 + 20 * k)).unwrap();
        }
        // Everything queued, however long it takes.
        while app.poll_for(Duration::from_secs(60)) > 0 {}
        let heard = app.heard_s();
        assert!(app.session.recorder.as_ref().unwrap().breaks().is_empty());
        // An hour asleep, then the sound runs on.
        let woke = 2000 + 20 * 49 + 3_600_000;
        for k in 0..50 {
            tx.send(block(woke + 20 * k)).unwrap();
        }
        while app.poll_for(Duration::from_secs(60)) > 0 {}
        assert!(app.running());
        let breaks = app.session.recorder.as_ref().unwrap().breaks();
        assert_eq!(breaks.len(), 1);
        assert!(
            (breaks[0].gap_s - 3600.0).abs() < 0.05,
            "{}",
            breaks[0].gap_s
        );
        assert!((app.heard_s() - heard - 3601.0).abs() < 0.05);
        let (m, error) = app.message.clone().unwrap();
        assert!(!error);
        assert!(m.contains("timestamps jumped by"), "{m}");
        drop(tx);
        let _ = std::fs::remove_dir_all(base);
    }
}

#[cfg(test)]
#[path = "app_layout_tests.rs"]
mod layout_tests;
