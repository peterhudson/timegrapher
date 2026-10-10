//! What the app remembers between runs: the views and choices a watchmaker
//! sets once (the strip's direction and scale, the panes and their layout,
//! the profile's scale, the sidebar), not anything about the watch on the
//! pickup. The window's size and place are kept by eframe itself.

use crate::app::Pane;
use crate::profiles;
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
    pub hist_cumulative: bool,
    pub hist_readings: bool,
    pub sound: profiles::Options,
    pub folded: Vec<String>,
    pub sidebar: bool,
    pub theme: ThemePreference,
    pub steady_view: steady::View,
    pub panes: Option<egui_tiles::Tree<Pane>>,
    /// Calibres the watchmaker entered by hand, for movements the built-in
    /// table doesn't have.
    pub custom_calibres: Vec<CustomCalibre>,
}

/// A calibre's train, typed in once and picked again later.
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

impl CustomCalibre {
    /// A new entry with the wheels whose periods most trains share: a
    /// 28,800 bph Swiss lever's 20-tooth escape wheel, and the fourth and
    /// centre wheels that carry the seconds and minute hands.
    pub fn new(name: String) -> Self {
        let w = |name: &str, period_s: f64| CustomWheel {
            name: name.into(),
            period_s,
        };
        CustomCalibre {
            name,
            wheels: vec![
                w("escape wheel", 5.0),
                w("fourth wheel", 60.0),
                w("centre wheel", 3600.0),
            ],
        }
    }
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
            steady_view: steady::View::default(),
            panes: None,
            custom_calibres: Vec::new(),
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
