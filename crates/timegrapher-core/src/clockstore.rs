//! Each sound card's clock error, measured once and kept.
//!
//! A sound card's crystal is off by a steady 10–50 ppm (1–4 s/d), so a
//! rate read against it is off by the same amount on every watch. A clock
//! log corrects one recording (see [`crate::clock`]); this store keeps each
//! input's measured error so that live readings and recordings without a
//! log are corrected too. It is a small JSON file in the user's settings
//! folder (`clocks.json` under `timegrapher/`, or under the folder named by
//! `TIMEGRAPHER_CONFIG_DIR`), shared by the command line and the app.
//!
//! Inputs are keyed by the name they are opened by. Under PipeWire or
//! PulseAudio the system's default input is one name whatever card is
//! behind it, so a correction stored for it holds only while the same
//! card stays the default: a new card made the default is read with the
//! old card's correction until that entry is forgotten or replaced.

use crate::clock::ClockFit;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "timegrapher.clocks/1";

/// One input's measured clock error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceClock {
    /// The input's name, as `timegrapher devices` lists it.
    pub device: String,
    /// How much faster true time runs than the card, ppm. Positive means
    /// the card is slow, so uncorrected rates read fast by
    /// `ppm` × 0.0864 s/d.
    pub ppm: f64,
    /// Standard error of `ppm`.
    #[serde(default)]
    pub ppm_sd: Option<f64>,
    /// Seconds of audio the measurement spans.
    pub span_s: f64,
    /// When it was measured, UTC.
    pub measured_utc: String,
    /// How: "measure" (listening against the system clock), "log" (a
    /// recording's clock log) or "set" (typed in).
    pub source: String,
}

impl DeviceClock {
    pub fn from_fit(device: &str, fit: &ClockFit, measured_utc: String, source: &str) -> Self {
        DeviceClock {
            device: device.to_string(),
            ppm: fit.ppm,
            ppm_sd: fit.ppm_sd.is_finite().then_some(fit.ppm_sd),
            span_s: fit.span_s,
            measured_utc,
            source: source.to_string(),
        }
    }

    /// What the card's error does to a rate, s/d: subtract it from a rate
    /// read on the card's clock.
    pub fn rate_error_s_per_day(&self) -> f64 {
        crate::clock::rate_error_s_per_day(self.ppm)
    }

    /// A rate read on the card's clock, corrected onto true time, s/d.
    pub fn correct_rate(&self, card_rate_s_per_day: f64) -> f64 {
        correct_rate(card_rate_s_per_day, self.ppm)
    }
}

/// A rate read on a clock `ppm` slow, corrected onto true time, s/d.
pub fn correct_rate(card_rate_s_per_day: f64, ppm: f64) -> f64 {
    (1.0 + card_rate_s_per_day / 86400.0) / (1.0 + ppm * 1e-6) * 86400.0 - 86400.0
}

/// The analyze document's correction beside its unchanged sound-card rate.
/// A zero-ppm correction is still a known correction; a missing raw fit stays null.
pub fn correction_json(ppm: f64, from: &str, card_rate: Option<f64>) -> serde_json::Value {
    serde_json::json!({
        "ppm": ppm,
        "from": from,
        "rate_error_s_per_day": crate::clock::rate_error_s_per_day(ppm),
        "rate_s_per_day": card_rate.map(|r| correct_rate(r, ppm)),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockStore {
    pub schema: String,
    pub devices: Vec<DeviceClock>,
}

impl Default for ClockStore {
    fn default() -> Self {
        ClockStore {
            schema: SCHEMA.to_string(),
            devices: Vec::new(),
        }
    }
}

/// The folder for the user's timegrapher settings: `TIMEGRAPHER_CONFIG_DIR`
/// if set, else the platform's settings folder plus `timegrapher`.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("TIMEGRAPHER_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(target_os = "windows") {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    };
    base.map(|b| b.join("timegrapher"))
}

/// Where the store lives.
pub fn path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("clocks.json"))
}

/// A recording's provenance sidecar: its own name with `.json` added.
pub fn sidecar_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_owned();
    s.push(".json");
    PathBuf::from(s)
}

/// The input a recording was made on, from its provenance sidecar
/// (`take.wav.json`, written by `doctor --save`) or the `session.json` of
/// an app recording folder.
pub fn recorded_device(file: &Path) -> Option<String> {
    let read = |p: PathBuf| -> Option<serde_json::Value> {
        serde_json::from_str(&fs::read_to_string(p).ok()?).ok()
    };
    if let Some(v) = read(sidecar_path(file)) {
        let name = v.pointer("/recording/source/device/name")?;
        return name.as_str().map(String::from);
    }
    let v = read(file.parent()?.join("session.json"))?;
    v.get("device")?.as_str().map(String::from)
}

impl ClockStore {
    /// The user's store; empty if there is none yet.
    pub fn load() -> Result<ClockStore, String> {
        match path() {
            Some(p) => ClockStore::load_from(&p),
            None => Ok(ClockStore::default()),
        }
    }

    /// A store file; empty if it does not exist.
    pub fn load_from(p: &Path) -> Result<ClockStore, String> {
        match fs::read_to_string(p) {
            Ok(text) => {
                let store: Self =
                    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
                store
                    .validate()
                    .map_err(|e| format!("{}: {e}", p.display()))?;
                Ok(store)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ClockStore::default()),
            Err(e) => Err(format!("{}: {e}", p.display())),
        }
    }

    /// Write the user's store, creating its folder.
    pub fn save(&self) -> Result<PathBuf, String> {
        let p = path().ok_or("no settings folder (set TIMEGRAPHER_CONFIG_DIR)")?;
        self.save_to(&p)?;
        Ok(p)
    }

    /// Validate the stored mathematical domains, without imposing calibration ranges.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!(
                "unsupported clock store schema {:?}; expected {SCHEMA}",
                self.schema
            ));
        }
        for (i, d) in self.devices.iter().enumerate() {
            let field = |name: &str, detail: &str| {
                format!("devices[{i}] ({:?}) {name}: {detail}", d.device)
            };
            crate::input_domain::card_ppm(d.ppm).map_err(|e| field("ppm", &e))?;
            if d.ppm_sd.is_some_and(|v| !v.is_finite() || v < 0.0) {
                return Err(field(
                    "ppm_sd",
                    "must be finite and nonnegative, or null when unavailable",
                ));
            }
            if !d.span_s.is_finite() || d.span_s < 0.0 {
                return Err(field("span_s", "must be finite and nonnegative"));
            }
        }
        Ok(())
    }

    pub fn save_to(&self, p: &Path) -> Result<(), String> {
        self.validate()
            .map_err(|e| format!("{}: {e}", p.display()))?;
        if let Some(d) = p.parent() {
            fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(p, text + "\n").map_err(|e| format!("{}: {e}", p.display()))
    }

    /// The entry for `device`: an exact match, else one that matches
    /// ignoring case.
    pub fn get(&self, device: &str) -> Option<&DeviceClock> {
        self.devices
            .iter()
            .find(|d| d.device == device)
            .or_else(|| {
                self.devices
                    .iter()
                    .find(|d| d.device.eq_ignore_ascii_case(device))
            })
    }

    /// Add or replace the entry for `d.device`.
    pub fn set(&mut self, d: DeviceClock) {
        match self.devices.iter_mut().find(|e| e.device == d.device) {
            Some(e) => *e = d,
            None => self.devices.push(d),
        }
    }

    /// Remove `device`'s entry; whether there was one.
    pub fn remove(&mut self, device: &str) -> bool {
        let n = self.devices.len();
        self.devices.retain(|d| d.device != device);
        self.devices.len() != n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(device: &str, ppm: f64) -> DeviceClock {
        DeviceClock {
            device: device.into(),
            ppm,
            ppm_sd: Some(0.2),
            span_s: 1200.0,
            measured_utc: "2026-10-10T03:00:00Z".into(),
            source: "set".into(),
        }
    }

    #[test]
    fn validates_store_domains_before_load_or_write() {
        let root = std::env::temp_dir().join(format!("tg-clock-domains-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("clocks.json");
        let mut valid = ClockStore::default();
        let mut d = entry("USB", 0.0);
        d.ppm_sd = None;
        d.span_s = 0.0;
        valid.set(d);
        valid.save_to(&path).unwrap();
        let original = fs::read(&path).unwrap();
        for (field, value) in [
            ("ppm", -1_000_000.0),
            ("ppm", f64::NAN),
            ("ppm", f64::INFINITY),
            ("ppm", f64::NEG_INFINITY),
            ("ppm_sd", -0.1),
            ("ppm_sd", f64::NAN),
            ("ppm_sd", f64::INFINITY),
            ("ppm_sd", f64::NEG_INFINITY),
            ("span_s", -1.0),
            ("span_s", f64::NAN),
            ("span_s", f64::INFINITY),
            ("span_s", f64::NEG_INFINITY),
        ] {
            let mut bad = valid.clone();
            match field {
                "ppm" => bad.devices[0].ppm = value,
                "ppm_sd" => bad.devices[0].ppm_sd = Some(value),
                _ => bad.devices[0].span_s = value,
            }
            let error = bad.save_to(&path).unwrap_err();
            assert!(error.contains(field) && error.contains("USB"), "{error}");
            assert_eq!(fs::read(&path).unwrap(), original);
            let absent = root.join("absent/clocks.json");
            assert!(bad.save_to(&absent).is_err());
            assert!(!absent.parent().unwrap().exists());
            if value.is_finite() {
                fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
                assert!(ClockStore::load_from(&path).unwrap_err().contains(field));
                fs::write(&path, &original).unwrap();
            }
        }
        let mut bad = valid.clone();
        bad.schema = "future".into();
        assert!(bad.save_to(&path).unwrap_err().contains("schema"));
        fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(ClockStore::load_from(&path).unwrap_err().contains("schema"));
        // Missing SD is the historical unavailable sentinel; manual span zero is valid.
        for sd in [None, Some(0.0), Some(0.2)] {
            valid.devices[0].ppm_sd = sd;
            for ppm in [-999_999.0, -12.0, 0.0, f64::MAX] {
                valid.devices[0].ppm = ppm;
                valid.save_to(&path).unwrap();
                assert_eq!(ClockStore::load_from(&path).unwrap().devices, valid.devices);
            }
        }
        valid.devices[0].ppm_sd = Some(f64::MAX);
        valid.devices[0].span_s = f64::MAX;
        valid.save_to(&path).unwrap();
        assert_eq!(ClockStore::load_from(&path).unwrap().devices, valid.devices);
        let mut doc = serde_json::to_value(&valid).unwrap();
        doc["devices"][0].as_object_mut().unwrap().remove("ppm_sd");
        fs::write(&path, doc.to_string()).unwrap();
        assert_eq!(
            ClockStore::load_from(&path).unwrap().devices[0].ppm_sd,
            None
        );
        assert!(ClockStore::load_from(&root.join("missing.json"))
            .unwrap()
            .devices
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrects_a_rate_read_on_a_slow_card() {
        // 20 ppm slow: the card's second is long, so a watch reads 1.728
        // s/d fast on it.
        let d = entry("USB", 20.0);
        assert!((d.rate_error_s_per_day() - 1.728).abs() < 1e-9);
        assert!((d.correct_rate(10.0) - (10.0 - 1.728)).abs() < 0.001);
        assert!(correct_rate(0.0, 0.0).abs() < 1e-9);
    }

    #[test]
    fn keeps_one_entry_per_device_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("tg-clocks-{}", std::process::id()));
        let p = dir.join("clocks.json");
        let mut s = ClockStore::load_from(&p).unwrap();
        assert!(s.devices.is_empty());
        s.set(entry("USB PnP Sound Device", 20.0));
        s.set(entry("Built-in Microphone", -3.0));
        s.set(entry("USB PnP Sound Device", 19.5));
        assert_eq!(s.devices.len(), 2);
        s.save_to(&p).unwrap();
        let t = ClockStore::load_from(&p).unwrap();
        assert_eq!(t.schema, SCHEMA);
        assert_eq!(t.get("usb pnp sound device").map(|d| d.ppm), Some(19.5));
        let mut t = t;
        assert!(t.remove("Built-in Microphone"));
        assert!(!t.remove("Built-in Microphone"));
        assert!(t.get("Built-in Microphone").is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
