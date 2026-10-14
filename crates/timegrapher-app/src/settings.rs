//! What the app remembers between runs: the views and choices a watchmaker
//! sets once (the strip's direction and scale, the panes and their layout,
//! the profile's scale), not anything about the watch on the
//! pickup. The window's size and place are kept by eframe itself.

use crate::app::Pane;
use crate::profiles;
use crate::scope;
use crate::steady;
use crate::strip::StripView;
use eframe::egui::ThemePreference;
use serde::{Deserialize, Serialize};

/// Where the settings sit in eframe's storage.
const KEY: &str = "timegrapher-settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The microphone last listened to, used again when it is plugged in.
    pub device: Option<String>,
    pub average_s: f64,
    pub strip: StripView,
    pub rate_line: bool,
    pub rate_guides: bool,
    pub overlays: [bool; 3],
    pub follow: bool,
    pub span_of_strip: bool,
    pub hist_span_of_strip: bool,
    pub hist_cumulative: bool,
    pub hist_readings: bool,
    pub sound: profiles::Options,
    pub folded: Vec<String>,
    pub theme: ThemePreference,
    /// The series the one Steadiness pane showed, before each view was a
    /// pane of its own: read for the panes not set yet.
    pub steady_series: [bool; 3],
    /// The series each Steadiness pane shows: rate, amplitude, beat error.
    pub steady_shown: Vec<(steady::View, [bool; 3])>,
    /// The Periods pane's scores on a log scale.
    pub periods_log: bool,
    /// The FFT pane's rate as the beat timing's spectrum, in ms.
    pub fft_timing: bool,
    /// The Positions Test's settling and measuring in each position,
    /// seconds.
    pub positions_settle_s: f64,
    pub positions_measure_s: f64,
    pub scope: scope::View,
    pub panes: Option<egui_tiles::Tree<Pane>>,
    /// Calibres the watchmaker entered by hand, as earlier versions kept
    /// them: read once and moved to the user calibre file, never written.
    pub custom_calibres: Vec<CustomCalibre>,
    /// Calibres picked lately, the latest first.
    pub recent_calibres: Vec<String>,
    /// Calibres put on the Calibre menu's short list, and the shipped
    /// short list's ones taken off it.
    pub short_added: Vec<String>,
    pub short_removed: Vec<String>,
    /// Panes averaged over their own time rather than Readings' Average
    /// Over.
    pub pane_average: Vec<(Pane, f64)>,
    /// Look for a newer release each time the app starts.
    pub check_updates: bool,
}

/// A calibre's train as earlier versions kept it in the settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomCalibre {
    pub name: String,
    pub wheels: Vec<CustomWheel>,
}

/// One wheel of a typed-in train.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomWheel {
    pub name: String,
    /// One full turn, seconds.
    pub period_s: f64,
}

impl Default for Settings {
    /// How the app first opens.
    fn default() -> Self {
        Settings {
            device: None,
            average_s: 10.0,
            strip: StripView {
                half_width_ms: 10.0,
                span_s: 30.0,
                horizontal: false,
            },
            rate_line: true,
            rate_guides: true,
            overlays: [true, false, false],
            follow: false,
            span_of_strip: false,
            hist_span_of_strip: false,
            hist_cumulative: false,
            hist_readings: true,
            sound: profiles::Options::default(),
            folded: Vec::new(),
            theme: ThemePreference::System,
            steady_series: [true; 3],
            steady_shown: Vec::new(),
            periods_log: false,
            fft_timing: false,
            positions_settle_s: crate::guided::SETTLE_S,
            positions_measure_s: crate::guided::MEASURE_S,
            scope: scope::View::default(),
            panes: None,
            custom_calibres: Vec::new(),
            recent_calibres: Vec::new(),
            short_added: Vec::new(),
            short_removed: Vec::new(),
            pane_average: Vec::new(),
            check_updates: true,
        }
    }
}

/// The settings kept last time, if any could be read.
pub fn load(storage: &dyn eframe::Storage) -> Option<Settings> {
    serde_json::from_str(&storage.get_string(KEY)?).ok()
}

pub fn store(storage: &mut dyn eframe::Storage, s: &Settings) {
    if let Ok(json) = serde_json::to_string(s) {
        storage.set_string(KEY, json);
    }
}
