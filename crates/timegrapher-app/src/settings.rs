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

/// A pane of several plots shows Rate alone until its switches are
/// changed: rate, amplitude, beat error.
pub const RATE_ONLY: [bool; 3] = [true, false, false];

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
    /// The readings the Charts and Distributions panes draw: rate,
    /// amplitude, beat error. The charts' Match Strip and the
    /// distributions' Time Span, kept by older versions, are ignored on
    /// load: both panes always cover the whole take.
    pub chart_series: [bool; 3],
    pub dist_series: [bool; 3],
    pub hist_cumulative: bool,
    /// The Power Reserve and Positions Test panes' Info panels open.
    pub reserve_info: bool,
    pub positions_info: bool,
    pub hist_readings: bool,
    /// The Isochronism pane's view: the curve, or the walk.
    pub iso_view: crate::isochronism::View,
    pub sound: profiles::Options,
    pub folded: Vec<String>,
    pub theme: ThemePreference,
    /// The series the one Steadiness pane showed, before each view was a
    /// pane of its own: read for the panes not set yet.
    pub steady_series: [bool; 3],
    /// The series each Steadiness pane shows: rate, amplitude, beat error.
    pub steady_shown: Vec<(steady::View, [bool; 3])>,
    /// The Periods pane's scores in dB (a build before the dB Scale
    /// called it a log scale).
    #[serde(alias = "periods_log")]
    pub periods_db: bool,
    /// The FFT pane's rate as the beat timing's spectrum, in ms.
    pub fft_timing: bool,
    /// The STFT and Wavelet Map panes' colours in dB, for a pane not
    /// switched yet: before each pane kept its own, the one choice.
    #[serde(alias = "maps_log")]
    pub maps_db: bool,
    /// Each map pane's own dB Scale.
    pub maps_db_set: Vec<(steady::View, bool)>,
    /// The calibre's wheels marked on the Periods, FFT, STFT Map and
    /// Wavelet Map panes' period axes, for a pane not switched yet.
    pub show_wheels: bool,
    /// Each of those panes' own Wheel Periods.
    pub show_wheels_set: Vec<(steady::View, bool)>,
    /// The Steadiness panes' Info panel open, for a pane not switched yet.
    pub steady_info: bool,
    /// Each Steadiness pane's own Info panel.
    pub steady_info_set: Vec<(steady::View, bool)>,
    /// The guided tests' settling and measuring in each position or
    /// stretch, seconds: one pair every guided test shares (#1577).
    pub positions_settle_s: f64,
    pub positions_measure_s: f64,
    /// The Current Position card's 6 | 10: ten offers the four 45° stops
    /// too. One setting with the Dynamic Poise pane's.
    pub ten_positions: bool,
    /// The Dynamic Poise pane's Plot | Dial, and its Info panel.
    pub poise_view: crate::app::PoiseView,
    pub poise_info: bool,
    /// The Complication pane's Info panel.
    pub complication_info: bool,
    pub scope: scope::View,
    pub panes: Option<egui_tiles::Tree<Pane>>,
    /// The rows and columns of `panes` whose divider was dragged by hand.
    pub panes_dragged: Vec<egui_tiles::TileId>,
    /// The version of the layout in `panes`: an older one is replaced by
    /// the default.
    #[serde(default)]
    pub layout: u32,
    /// Calibres the watchmaker entered by hand, as earlier versions kept
    /// them: read once and moved to the user calibre file, never written.
    pub custom_calibres: Vec<CustomCalibre>,
    /// The calibres the app was last set to, the latest first.
    pub recent_calibres: Vec<String>,
    /// Calibres put on the Calibre menu's short list, and the shipped
    /// short list's ones taken off it.
    pub short_added: Vec<String>,
    pub short_removed: Vec<String>,
    /// Look for a newer release each time the app starts.
    pub check_updates: bool,
    /// Keep the computer from sleeping on its own while listening.
    pub keep_awake: bool,
    /// Listen to the microphone as soon as the app opens.
    pub listen_at_open: bool,
    /// The microphone has been set up (Set Up the Microphone, or a listen
    /// that opened it): until then the app asks at launch.
    pub mic_set_up: bool,
    /// Not Now was pressed on Set Up the Microphone: it isn't shown at
    /// launch again, and this doesn't count as set up.
    pub setup_declined: bool,
    /// The readings' cards across the top shown.
    pub show_readings: bool,
    /// The readings' cards pinned open in the layout, rather than the
    /// strip that opens them over the panes on a click.
    pub readings_pinned: bool,
    /// The library's folder when one was chosen in Settings; `None` is
    /// `~/Timegrapher Data`.
    pub library: Option<std::path::PathBuf>,
    /// The Settings window's category last left on, which it opens at.
    pub settings_section: String,
    /// The inputs whose own automatic gain is left on, by device id
    /// ("default" for the system default). Every other input with the
    /// switch has it turned off each time the app opens it.
    pub agc_on: Vec<String>,
    /// The field the Watches view's list shows under each name (List
    /// Shows); `None` is the calibre.
    pub watch_list_shows: Option<String>,
    /// Settings > General > Run After Saving a Take: a command run once a
    /// take is saved with listening stopped, the take's folder passed to it
    /// ([`crate::after_save`]). Empty runs nothing.
    pub after_save_command: String,
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
            average_s: timegrapher_core::live::DEFAULT_AVERAGE_S,
            strip: StripView {
                half_width_ms: 10.0,
                span_s: timegrapher_core::live::DEFAULT_STRIP_S,
                horizontal: true,
                still: false,
            },
            rate_line: true,
            rate_guides: true,
            overlays: [true, false, false],
            follow: false,
            chart_series: RATE_ONLY,
            dist_series: RATE_ONLY,
            hist_cumulative: false,
            reserve_info: false,
            positions_info: false,
            hist_readings: true,
            iso_view: Default::default(),
            sound: profiles::Options::default(),
            folded: Vec::new(),
            theme: ThemePreference::System,
            steady_series: RATE_ONLY,
            steady_shown: Vec::new(),
            periods_db: false,
            fft_timing: false,
            maps_db: false,
            maps_db_set: Vec::new(),
            show_wheels: true,
            show_wheels_set: Vec::new(),
            steady_info: false,
            steady_info_set: Vec::new(),
            positions_settle_s: crate::guided::SETTLE_S,
            positions_measure_s: crate::guided::MEASURE_S,
            ten_positions: false,
            poise_view: Default::default(),
            poise_info: false,
            complication_info: false,
            scope: scope::View::default(),
            panes: None,
            panes_dragged: Vec::new(),
            layout: crate::app::LAYOUT,
            custom_calibres: Vec::new(),
            recent_calibres: Vec::new(),
            short_added: Vec::new(),
            short_removed: Vec::new(),
            check_updates: true,
            keep_awake: true,
            listen_at_open: true,
            mic_set_up: false,
            setup_declined: false,
            show_readings: true,
            readings_pinned: true,
            library: None,
            settings_section: "general".into(),
            agc_on: Vec::new(),
            watch_list_shows: None,
            after_save_command: String::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings kept before the strip had still paper read as moving
    /// paper, every other choice kept.
    #[test]
    fn older_settings_read_as_moving_paper() {
        let mut v = serde_json::to_value(Settings::default()).unwrap();
        let strip = v["strip"].as_object_mut().unwrap();
        strip.remove("still");
        strip.insert("span_s".into(), serde_json::json!(120.0));
        let s: Settings = serde_json::from_value(v).unwrap();
        assert!(!s.strip.still);
        assert_eq!(s.strip.span_s, 120.0);
    }

    /// Settings kept by a build before the dB Scale keep their choice:
    /// a log scale then is the dB Scale now.
    #[test]
    fn an_older_log_scale_choice_is_kept_as_db() {
        let mut v = serde_json::to_value(Settings::default()).unwrap();
        let o = v.as_object_mut().unwrap();
        o.remove("periods_db");
        o.remove("maps_db");
        o.insert("periods_log".into(), true.into());
        o.insert("maps_log".into(), true.into());
        let s: Settings = serde_json::from_value(v).unwrap();
        assert!(s.periods_db && s.maps_db);
    }
}
