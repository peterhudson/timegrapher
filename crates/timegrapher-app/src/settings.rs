//! What the app remembers between runs: the views and choices a watchmaker
//! sets once (the strip's direction and scale, the panes and their layout,
//! the profile's scale, the sidebar), not anything about the watch on the
//! pickup. The window's size and place are kept by eframe itself.

use crate::app::Pane;
use crate::profiles;
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
    pub hist_cumulative: bool,
    pub hist_readings: bool,
    pub sound: profiles::Options,
    pub folded: Vec<String>,
    pub sidebar: bool,
    pub theme: ThemePreference,
    pub panes: Option<egui_tiles::Tree<Pane>>,
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
            hist_cumulative: false,
            hist_readings: true,
            sound: profiles::Options::default(),
            folded: Vec::new(),
            sidebar: true,
            theme: ThemePreference::System,
            panes: None,
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
