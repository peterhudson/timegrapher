//! The library of watches: one folder the app manages, holding a folder
//! per watch with its details, its photos and every take made of it.
//!
//! ```text
//! Timegrapher Data/              the library
//!   library.toml                 its format
//!   Yacht-Master 42 replica/     one folder per watch
//!     watch.toml                 its details
//!     photos/
//!     takes/
//!       2026-10-20 14.32 Dial Up/   one standard take, unchanged
//!   Unsorted/                    takes made with Quick Listen
//!     takes/
//! ```
//!
//! A take folder is exactly a standard take (`session.toml`,
//! `session.json`, `clock.csv`, the sound, or `analysis/` for one kept as
//! its analysis alone), so everything that reads a take reads one here.
//! Everything is found by scanning, nothing by a stored path: a folder
//! with a `watch.toml` is a watch, and a folder under its `takes/` with a
//! `session.toml` or `session.json` is a take. So the library can be
//! moved, synced or restored from a backup whole. Folder names are for
//! people; each watch also has a random id in its `watch.toml`, so
//! renaming its folder loses nothing.
//!
//! `Unsorted/` (with no `watch.toml`) holds the takes made with Quick
//! Listen, which listens without picking a watch first, until each is
//! filed under a watch or deleted. No watch can be called Unsorted.
//!
//! The scan reads each take's size and what its `session.json` says, and
//! keeps them in `.cache.json` at the top of the library, rebuilt when
//! missing: a take whose files haven't changed is not read again. The
//! folders stay the truth. Nothing here deletes or repairs anything: a
//! take with sound files missing (half synced, or copied in part) is
//! listed as incomplete, as it is.
//!
//! The first copy of the app to open a library holds a lock on
//! `.library.lock` at its top while it has it open. Another copy opens it
//! read-only: it lists the watches but changes nothing, not even the
//! cache. The system lets go of the lock when the app ends, however it
//! ends. The command line's changes take the same lock, but for adding a
//! take ([`Library::open_to_add`]), which needs none.

use crate::session::{Manifest, Position};
use crate::take;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The library's format, in `library.toml`.
pub const FORMAT: u32 = 1;
pub const LIBRARY_FILE: &str = "library.toml";
pub const WATCH_FILE: &str = "watch.toml";
pub const TAKES: &str = "takes";
pub const PHOTOS: &str = "photos";
pub const CACHE_FILE: &str = ".cache.json";
/// The format of what `.cache.json` keeps of each take, raised whenever a
/// take reads differently, so a cache an earlier build wrote is read
/// afresh. Apart from [`FORMAT`]: a new cache never stops an older build
/// opening the library, which just makes its own cache again. 2: `at_end`
/// gained `rate_corrected_s_per_day`. 3: a recording's USB lines correct
/// that rate over its clock log.
const CACHE_FORMAT: u32 = 3;
pub const LOCK_FILE: &str = ".library.lock";
/// The folder of takes not yet filed under a watch.
pub const UNSORTED: &str = "Unsorted";
/// The file a Positions Test keeps its results in, beside its take's
/// sound: a take with it is a Positions Test's ([`is_positions_test`]).
pub const POSITIONS_TEST_FILE: &str = "positions-test.json";
/// The library's folder name in the user's home folder, by default.
pub const DEFAULT_NAME: &str = "Timegrapher Data";

/// Fields a watch can be given beyond the standard ones, offered by Add
/// Field; any can be picked or ignored, beside a field of the user's own.
pub const SUGGESTED_FIELDS: [&str; 14] = [
    "Make",
    "Model",
    "Reference",
    "Serial Number",
    "Year",
    "Purchased",
    "Purchase Price",
    "Owner",
    "Customer",
    "Job Number",
    "Received",
    "Returned",
    "Service History",
    "Work Done",
];

/// `~/Timegrapher Data` on every system, or `None` with no home folder.
pub fn default_location() -> Option<PathBuf> {
    let home = if cfg!(target_os = "windows") {
        std::env::var_os("USERPROFILE")
    } else {
        std::env::var_os("HOME")
    };
    home.filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(DEFAULT_NAME))
}

/// `library.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LibraryFile {
    format: u32,
}

/// What a field the user added holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Date,
    Number,
    Money,
}

/// A field the user added: a label and its value, kept as typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub label: String,
    pub kind: FieldKind,
    #[serde(default)]
    pub value: String,
}

/// A watch's details, `watch.toml`. Only the name is required; a standard
/// field left out (`None`) is one the user removed or never filled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watch {
    /// Random, made when the watch is added; never shown in the app, and
    /// given in `watches list` and `show` JSON so a program can find the
    /// watch again after a rename.
    pub id: String,
    /// Shown everywhere, unique in the library.
    pub name: String,
    /// The calibre table's row: its name and, with `bph`, which of the
    /// rows of that name (a calibre can come at more than one beat rate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lift: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bph: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Hidden from the list (gone back to its owner, or sold), not deleted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    /// The fields the user added, in their order.
    #[serde(default, rename = "field", skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
}

/// The lift angle and beat rate a watch's calibre gives, each where the
/// calibre table has it. A figure given is the calibre's: the watch takes
/// it, and it can't be set by hand while that calibre is picked.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FromCalibre {
    pub lift: Option<f64>,
    pub bph: Option<u32>,
}

impl FromCalibre {
    /// What `c` gives.
    pub fn of(c: &crate::take::Calibre) -> Self {
        FromCalibre {
            lift: c.lift_angle_deg,
            bph: c.bph,
        }
    }
}

impl Watch {
    /// Pick the calibre `name` (empty: none), which gives `given`: the
    /// watch takes each figure the calibre gives and keeps its own for
    /// one it doesn't.
    pub fn pick_calibre(&mut self, name: &str, given: FromCalibre) {
        let name = name.trim();
        self.calibre = (!name.is_empty()).then(|| name.to_string());
        self.hold_calibre(given);
    }

    /// Keep the figures the watch's calibre gives (`given`) over any set
    /// by hand.
    pub fn hold_calibre(&mut self, given: FromCalibre) {
        self.lift = given.lift.or(self.lift);
        self.bph = given.bph.or(self.bph);
    }

    /// Set the lift angle by hand: refused when the calibre gives one
    /// (`given`), or out of range.
    pub fn set_lift(&mut self, lift: f64, given: FromCalibre) -> Result<(), String> {
        if let Some(l) = given.lift {
            return Err(format!(
                "the calibre gives the lift angle ({l}°): set no calibre to set it by hand"
            ));
        }
        self.lift = Some(crate::input_domain::lift_deg(lift)?);
        Ok(())
    }

    /// Set the beat rate by hand (`None`: found from the sound): refused
    /// when the calibre gives one (`given`).
    pub fn set_bph(&mut self, bph: Option<u32>, given: FromCalibre) -> Result<(), String> {
        if let Some(b) = given.bph {
            return Err(format!(
                "the calibre gives the beat rate ({b} bph): set no calibre to set it by hand"
            ));
        }
        self.bph = match bph {
            Some(b) => Some(crate::input_domain::bph(u64::from(b))?),
            None => None,
        };
        Ok(())
    }

    /// Set the added field `label` (ignoring case) to `value`, or add it
    /// as text at the end when the watch has none of that label.
    pub fn set_field(&mut self, label: &str, value: &str) -> Result<(), String> {
        let label = label.trim();
        if label.is_empty() {
            return Err("a field needs a label".into());
        }
        match self
            .fields
            .iter_mut()
            .find(|f| f.label.eq_ignore_ascii_case(label))
        {
            Some(f) => f.value = value.to_string(),
            None => self.fields.push(Field {
                label: label.to_string(),
                kind: FieldKind::Text,
                value: value.to_string(),
            }),
        }
        Ok(())
    }

    /// Remove the added field `label` (ignoring case): an error when the
    /// watch has none.
    pub fn remove_field(&mut self, label: &str) -> Result<(), String> {
        let before = self.fields.len();
        self.fields
            .retain(|f| !f.label.eq_ignore_ascii_case(label.trim()));
        if self.fields.len() == before {
            return Err(format!(
                "\"{}\" has no field \"{}\"",
                self.name,
                label.trim()
            ));
        }
        Ok(())
    }

    /// A new watch of this name, with a fresh id.
    pub fn new(name: &str) -> Watch {
        Watch {
            id: new_id(),
            name: name.trim().to_string(),
            calibre: None,
            lift: None,
            bph: None,
            notes: None,
            archived: false,
            fields: Vec::new(),
        }
    }

    pub fn read(dir: &Path) -> Result<Watch, String> {
        let p = dir.join(WATCH_FILE);
        let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let w: Watch = toml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        if w.name.trim().is_empty() {
            return Err(format!("{}: the watch has no name", p.display()));
        }
        Ok(w)
    }

    fn write(&self, dir: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(&dir.join(WATCH_FILE), text.as_bytes())
    }
}

/// 16 hex digits from the time, the process and a counter, hashed.
fn new_id() -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mut h = Sha256::new();
    h.update(now.as_nanos().to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.update(COUNT.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    h.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Write `bytes` to a file beside `p`, then move it over `p`, so a reader
/// never sees half of it. Each write has a file of its own, created new
/// ([`new_id`]), so two writes at once can't write into each other's,
/// and one that fails removes only its own (#872).
pub fn write_atomic(p: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic_with(p, bytes, &ON_DISK)
}

/// [`write_atomic`]'s steps after the new file is made: its bytes, onto
/// the disk, and into place; a test makes any one fail (#1639).
pub(crate) struct AtomicSteps {
    pub write: fn(&mut fs::File, &[u8]) -> std::io::Result<()>,
    pub sync: fn(&fs::File) -> std::io::Result<()>,
    pub rename: fn(&Path, &Path) -> std::io::Result<()>,
}

const ON_DISK: AtomicSteps = AtomicSteps {
    write: write_all,
    sync: fs::File::sync_all,
    rename: crate::recorder::rename_settled,
};

fn write_all(f: &mut fs::File, bytes: &[u8]) -> std::io::Result<()> {
    std::io::Write::write_all(f, bytes)
}

/// [`write_atomic`] with `steps`: whichever fails, `p` is as it was and
/// the new file is gone.
pub(crate) fn write_atomic_with(p: &Path, bytes: &[u8], steps: &AtomicSteps) -> Result<(), String> {
    let name = p
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let tmp = p.with_file_name(format!(".{name}.{}.tmp", new_id()));
    let failed = |e: std::io::Error| format!("{}: {e}", p.display());
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(failed)?;
    let written = (steps.write)(&mut f, bytes).and_then(|_| (steps.sync)(&f));
    drop(f);
    written.and_then(|_| (steps.rename)(&tmp, p)).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        failed(e)
    })
}

/// A folder name for `name` that every system accepts: the characters
/// Windows forbids and control characters become `-`, trailing dots and
/// spaces go, a name Windows keeps for devices (`CON`, `LPT1`, also
/// with an extension) gets `_` before its first dot, and it is cut to 100
/// characters first. The name as typed stays in the files.
pub fn folder_name(name: &str) -> String {
    let mut s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .take(100)
        .collect();
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    // Windows keeps these names for devices whatever follows a dot
    // ("lpt1.txt" too), so the `_` goes before the first dot.
    let stem_len = s.find('.').unwrap_or(s.len());
    let stem = s[..stem_len].trim_end().to_uppercase();
    let numbered = |p: &str| {
        stem.strip_prefix(p).is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    };
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || numbered("COM")
        || numbered("LPT");
    if reserved {
        s.insert(stem_len, '_');
    }
    if s.is_empty() {
        s = "Watch".into();
    }
    s
}

/// `parent/base`, or `parent/base 2`, `base 3`… for the first that isn't
/// there.
pub fn unused_dir(parent: &Path, base: &str) -> PathBuf {
    let first = parent.join(base);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| parent.join(format!("{base} {n}")))
        .find(|p| !p.exists())
        .expect("a free name")
}

/// `dir/name`, or with ` 2`, ` 3`… before its extension (`dial 2.jpg`)
/// for the first that isn't there.
fn unused_file(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} {n}{ext}")))
        .find(|p| fs::symlink_metadata(p).is_err())
        .expect("a free name")
}

/// A take folder's name: the date and time it started, as
/// `2026-10-20 14.32`, plus the position in words when it used only one
/// (`2026-10-20 14.32 Dial Up`).
pub fn take_name(started: &str, positions: &[Position]) -> String {
    let mut one = positions.to_vec();
    one.dedup();
    match one.as_slice() {
        [p] => format!("{started} {}", p.name()),
        _ => started.to_string(),
    }
}

/// A Power Reserve Test's take folder's name: the date and time it
/// started and "Power Reserve" (`2026-10-20 14.32 Power Reserve`).
pub fn power_reserve_take_name(started: &str) -> String {
    format!("{started} Power Reserve")
}

/// Where a Power Reserve Test's take is kept among a watch's `takes`: one
/// folder named by [`power_reserve_take_name`], holding the recording with
/// its own session file (`file = "."`) and the run's
/// `power-reserve.json` beside it, so the app opens the take on its run.
/// The app and `timegrapher record --power-reserve` both file a test here
/// (#1732).
pub fn power_reserve_take_dir(takes: &Path, started: &str) -> PathBuf {
    unused_dir(takes, &power_reserve_take_name(started))
}

/// The readings `session.json` keeps from the end of a take, as the app
/// saw them then.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AtEnd {
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// Signed: from the unlock, else the drop
    /// ([`crate::session::beat_error_signed`]).
    pub beat_error_ms: Option<f64>,
    /// `rate_s_per_day` corrected for the take's sound card from its clock
    /// log, as `timegrapher session` and the app read the take; left out
    /// when the take has no clock log that can be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_corrected_s_per_day: Option<f64>,
}

impl AtEnd {
    /// The rate to show: on the take's clock log where it has one, else as
    /// heard.
    pub fn rate(&self) -> Option<f64> {
        self.rate_corrected_s_per_day.or(self.rate_s_per_day)
    }
}

/// One take, as the scan found it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Take {
    /// The folder's name; the app never relies on it.
    pub name: String,
    /// Every file in the folder, bytes.
    pub bytes: u64,
    /// False for a take kept as its analysis alone.
    pub audio: bool,
    /// Files `session.toml` lists (or the `analysis/` folder an
    /// analysis-only take needs) aren't there.
    pub incomplete: bool,
    /// From `session.json`: none for a take copied in without one.
    pub started_utc: Option<String>,
    /// For a take with no `started_utc`, when its folder's name says it
    /// started ([`named_start`], #1272): `2026-10-22T09:10`, in its
    /// recording computer's time, no zone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_start: Option<String>,
    /// From `session.json`, else the length of the sound its files'
    /// headers give ([`sound_length`], #1272).
    pub duration_s: Option<f64>,
    /// The positions `session.toml` names, in order, each once.
    pub positions: Vec<String>,
    pub at_end: Option<AtEnd>,
    /// For a Power Reserve Test's take, its run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_reserve: Option<crate::power_reserve::Summary>,
}

impl Take {
    /// What the take measured and is, as the facts the app and
    /// `timegrapher watches` list for it: its kind when it is a Positions
    /// Test's (`positions_test`, from [`is_positions_test`]), a Power
    /// Reserve run, its positions, its length, the readings it ended on
    /// (beat error as its size; none for a test, whose last window doesn't
    /// stand for the test: its figures are in its table, or hour by hour)
    /// and its marks. Each surface joins them as it lays them out.
    pub fn facts(&self, positions_test: bool) -> Vec<String> {
        let figures = self
            .at_end
            .as_ref()
            .map(|a| (a.rate(), a.amplitude_deg, a.beat_error_ms));
        self.facts_with(positions_test, figures)
    }

    /// [`Take::facts`] with the figures of the take's own reading
    /// ([`reading`]) in place of the ones the app showed when it saved
    /// it: none when it has no reading, nor for a test, as there.
    pub fn facts_read(&self, positions_test: bool, reading: Option<&Reading>) -> Vec<String> {
        let figures = reading.map(|r| (r.rate_s_per_day, r.amplitude_deg, r.beat_error_ms));
        self.facts_with(positions_test, figures)
    }

    /// The facts with `figures`: rate, amplitude and signed beat error.
    fn facts_with(
        &self,
        positions_test: bool,
        figures: Option<(Option<f64>, Option<f64>, Option<f64>)>,
    ) -> Vec<String> {
        let mut parts: Vec<String> = Vec::new();
        if positions_test {
            parts.push("Positions Test".into());
        }
        if let Some(r) = &self.power_reserve {
            parts.push(format!(
                "Power Reserve {}",
                // A take's file holds a test's run: "at least" when its
                // recording failed, as on every surface (#1380).
                crate::power_reserve::run_words(
                    r.run_s,
                    r.ending,
                    crate::power_reserve::Source::Test
                )
            ));
            // A run the watch didn't end says how it ended.
            if r.ending != crate::power_reserve::Ending::Stopped {
                parts.push(r.ending.text());
            }
        }
        let names: Vec<String> = self
            .positions
            .iter()
            .map(|p| Position::parse(p).map_or_else(|| p.clone(), |x| x.name().to_string()))
            .collect();
        if !names.is_empty() {
            parts.push(names.join(", "));
        }
        if let Some(d) = self.duration_s.filter(|_| self.power_reserve.is_none()) {
            parts.push(length_text(d));
        }
        let test = positions_test || self.power_reserve.is_some();
        if let Some((rate, amplitude, beat_error)) = figures.filter(|_| !test) {
            let [r, x, b] = figure_texts(rate, amplitude, beat_error);
            parts.extend(r.map(|r| format!("{r} s/d")));
            parts.extend(x.map(|x| format!("{x}°")));
            parts.extend(b.map(|b| format!("{b} ms")));
        }
        if !self.audio {
            parts.push("Analysis Only".into());
        }
        if self.incomplete {
            parts.push("Incomplete".into());
        }
        parts
    }
}

/// A take's rate, amplitude and beat error as its listed figures, without
/// units: each at its base decimals ([`crate::figure_text::base_digits`],
/// the decimals a card shows with no ± to follow), the rate signed and
/// the beat error as its size. The one rule for the takes the app and
/// `timegrapher watches` list (#1267: a beat error of 0.04 ms is never
/// "0.0").
pub fn figure_texts(
    rate: Option<f64>,
    amplitude: Option<f64>,
    beat_error: Option<f64>,
) -> [Option<String>; 3] {
    use crate::figure_text::{base_digits, figure};
    [
        rate.map(|r| figure(r, None, base_digits::RATE, true)),
        amplitude.map(|x| figure(x, None, base_digits::AMPLITUDE, false)),
        beat_error.map(|b| figure(b.abs(), None, base_digits::BEAT_ERROR, false)),
    ]
}

/// A take's own reading, as `timegrapher session` reads it: its last
/// recorded position, after the watch settled, on the take's clock. What
/// the Watches list shows for a take, worked out from its sound (or its
/// kept analysis) once and kept ([`reading`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    /// The position read, as the take's `session.toml` names it.
    pub position: Option<String>,
    pub rate_s_per_day: Option<f64>,
    /// Whether the rate is on the take's clock (a clock log, USB lines or
    /// a stored correction) rather than the sound card's.
    pub calibrated: bool,
    pub amplitude_deg: Option<f64>,
    /// Signed: from the unlock, else the drop
    /// ([`crate::session::beat_error_signed`]).
    pub beat_error_ms: Option<f64>,
    /// Source of the selected beat error; absent in older saved readings.
    #[serde(default)]
    pub beat_error_source: Option<crate::session::BeatErrorSource>,
    /// Factual conditions carried from the core measurement.
    #[serde(default)]
    pub caveats: Vec<crate::measure::Caveat>,
    /// False where no regular beat was found; the figures are then none.
    pub watch_heard: bool,
}

/// The file the readings are kept in, in the user's settings folder
/// ([`crate::clockstore::config_dir`]), not in the library, so a library
/// opened to read only can still keep them.
pub const READINGS_FILE: &str = "readings.json";

/// What a kept reading was worked out from outside its take's folder,
/// which the take's own stamp can't see: the engine that read it, the
/// calibre its session file names as the user's calibres resolve it, and
/// the clock correction stored for the input it was recorded on
/// ([`crate::take::correction_for`]), with the sound file that correction
/// was asked for. A reading is kept only while all of these are as they
/// were (audit F1: a correction set after the reading was kept left the
/// library showing the old rate). Also the fingerprint of each sound file
/// or folder and clock log its session file names ([`linked`]), `None`
/// where there is none to read: one outside the take's folder changes
/// without the take's own stamp changing (audit R2-1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Outside {
    engine: String,
    calibre: Option<String>,
    first: Option<PathBuf>,
    correction: Option<(f64, String)>,
    linked: Vec<(PathBuf, Option<Stamp>)>,
}

/// A take's reading as kept, with what it was worked out from.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Kept {
    stamp: Stamp,
    outside: Outside,
    reading: Reading,
}

/// Readings kept before [`Outside`] was kept with them don't read as
/// these, so each is worked out once again.
type Readings = BTreeMap<String, Kept>;

/// The engine that works the readings out: this build's version and a
/// hash of core's source (`build.rs`), so a reading kept by other code
/// within the same version is worked out again (audit: engine identity).
pub(crate) const ENGINE: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "+",
    env!("TIMEGRAPHER_ENGINE_SOURCE")
);

/// The engine a kept reading was read by, as the Watches list names it
/// (#1521): this build's version as it is (a suffix such as `-dev`
/// kept) and the first 8 hex digits of core's source hash, which tell
/// two builds of one version apart. Every reading [`reading`] gives is
/// this engine's: one kept by another is read again.
pub fn engine() -> (&'static str, &'static str) {
    let (version, source) = ENGINE.split_once('+').unwrap_or((ENGINE, ""));
    (version, source.get(..8).unwrap_or(source))
}

fn readings_path() -> Option<PathBuf> {
    crate::clockstore::config_dir().map(|d| d.join(READINGS_FILE))
}

fn readings_kept() -> Readings {
    readings_path()
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn reading_key(take_dir: &Path) -> String {
    std::path::absolute(take_dir)
        .unwrap_or_else(|_| take_dir.to_path_buf())
        .display()
        .to_string()
}

/// The reading kept for the take in `take_dir`, when it was worked out
/// since the take last changed; quick, it reads no sound.
pub fn kept_reading(take_dir: &Path) -> Option<Reading> {
    let k = readings_kept().remove(&reading_key(take_dir))?;
    (k.stamp == reading_stamp(take_dir)
        && outside(take_dir, k.outside.first.as_deref()).ok()? == k.outside)
        .then_some(k.reading)
}

/// The take in `take_dir`'s own reading ([`Reading`]): the one kept when
/// the take hasn't changed since, else worked out and kept. Working it out
/// reads the take's sound through the stream engine, as `session` does,
/// about a minute for a 9 h take; a reading that can't be kept is only
/// worked out again next time.
pub fn reading(take_dir: &Path) -> Result<Reading, String> {
    if let Some(r) = kept_reading(take_dir) {
        return Ok(r);
    }
    let stamp = reading_stamp(take_dir);
    let (r, outside) = measure_take(take_dir)?;
    if let Some(p) = readings_path() {
        let mut kept = readings_kept();
        kept.insert(
            reading_key(take_dir),
            Kept {
                stamp,
                outside,
                reading: r.clone(),
            },
        );
        if let (Some(dir), Ok(b)) = (p.parent(), serde_json::to_vec(&kept)) {
            let _ = fs::create_dir_all(dir).map(|_| write_atomic(&p, &b));
        }
    }
    Ok(r)
}

/// The take's last position with anything to read
/// ([`crate::take::reads_nothing`]), read as `timegrapher session` reads
/// it, with no flags: its settings from its `session.toml` and calibre
/// ([`crate::take::stream_settings`]), its clock from its log, its USB
/// lines, its `card_ppm_fast` or the correction stored for its input
/// ([`crate::take::measure_entry`]). Also what it was worked out from
/// outside the take's folder ([`Outside`]).
fn measure_take(dir: &Path) -> Result<(Reading, Outside), String> {
    measure_take_with(dir, |m, first| {
        crate::take::correction_for(m, first, None, None)
    })
}

/// [`measure_take`], with the correction for the take's first sound file
/// from `correction`. The [`Outside`] it gives holds the very correction
/// the reading used, not one looked up again after, so a correction
/// changed while the take is read can't be kept beside a reading made
/// without it.
fn measure_take_with(
    dir: &Path,
    mut correction: impl FnMut(&Manifest, &Path) -> Result<Option<(f64, String)>, String>,
) -> Result<(Reading, Outside), String> {
    use crate::take::EntryError;
    let m = manifest_of(dir)?;
    let linked = linked(dir, &m);
    let cal = calibre_of(&m)?;
    let settings = crate::take::EntrySettings {
        stream: crate::take::stream_settings(&m, cal.as_ref(), crate::take::Given::default()),
        card_clock: false,
        settle_s: None,
    };
    let mut used = None;
    let mut unread = None;
    let mut found = None;
    for e in m.recordings.iter().rev() {
        if crate::take::reads_nothing(e) {
            continue;
        }
        used = None;
        let read = crate::take::measure_entry(dir, &m, e, &settings, crate::take::usb, |first| {
            let c = correction(&m, first);
            if let Ok(c) = &c {
                used = Some((first.to_path_buf(), c.clone()));
            }
            c
        });
        match read {
            Ok(read) => {
                found = Some((e, read));
                break;
            }
            // As `session` leaves such a position out, so does the
            // reading, and it takes the one before (audit R2-3).
            Err(EntryError::NoReading(why)) => {
                unread.get_or_insert(why.to_string());
            }
            Err(EntryError::Failed(why)) => return Err(why),
        }
    }
    let Some((e, read)) = found else {
        return Err(unread.unwrap_or_else(|| {
            format!(
                "{}: no recording listed with anything to read",
                dir.join(crate::take::MANIFEST).display()
            )
        }));
    };
    let x = &read.measurement;
    let reading = Reading {
        position: e.position.clone(),
        rate_s_per_day: x.rate_s_per_day,
        calibrated: x.calibrated,
        amplitude_deg: x.amplitude_deg,
        beat_error_ms: x.beat_error_signed(),
        beat_error_source: x.beat_error_source,
        caveats: x.caveats.clone(),
        watch_heard: x.watch_heard,
    };
    let (first, correction) = used.unzip();
    Ok((
        reading,
        outside_from(cal.as_ref(), first, correction.flatten(), linked),
    ))
}

/// The take in `dir`'s session file.
fn manifest_of(dir: &Path) -> Result<Manifest, String> {
    let file = dir.join(crate::take::MANIFEST);
    let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))
}

/// The calibre session file `m` names, as the user's calibres resolve it.
fn calibre_of(m: &Manifest) -> Result<Option<crate::take::Calibre>, String> {
    let user = match (&m.calibre, crate::calibre_store::default_path()) {
        (Some(_), Some(p)) => crate::calibre_store::UserCalibres::load(&p)?,
        _ => crate::calibre_store::UserCalibres::default(),
    };
    Ok(m.calibre
        .as_deref()
        .and_then(|n| crate::take::calibre_named(n, &user)))
}

/// What a reading of the take in `dir` would read outside its folder
/// now, the correction looked up afresh for `first` as the kept reading's
/// was asked for: quick, it reads no sound.
fn outside(dir: &Path, first: Option<&Path>) -> Result<Outside, String> {
    let m = manifest_of(dir)?;
    let cal = calibre_of(&m)?;
    let correction = match first {
        Some(f) => crate::take::correction_for(&m, f, None, None)?,
        None => None,
    };
    Ok(outside_from(
        cal.as_ref(),
        first.map(Path::to_path_buf),
        correction,
        linked(dir, &m),
    ))
}

/// The [`Outside`] of a reading with calibre `cal`, its correction asked
/// for `first`: one place for a kept reading's and a fresh look's.
fn outside_from(
    cal: Option<&crate::take::Calibre>,
    first: Option<PathBuf>,
    correction: Option<(f64, String)>,
    linked: Vec<(PathBuf, Option<Stamp>)>,
) -> Outside {
    Outside {
        engine: ENGINE.into(),
        calibre: cal.map(|c| format!("{c:?}")),
        first,
        correction,
        linked,
    }
}

/// The first sound file or folder or clock log that the take in `dir`'s
/// session file names by a path that won't lead to it once the folder is
/// copied or moved: one leading out of the folder, such as
/// `../audio.wav`, which the copy or move leaves behind (audit R2-2), or
/// one from the root into the folder itself, which then names where the
/// file was. Either way the take would read nothing, or the original.
/// A path from the root to elsewhere still leads there, so it isn't one.
fn named_outside(dir: &Path) -> Option<String> {
    let m = manifest_of(dir).ok()?;
    let inside = resolved(dir);
    m.recordings
        .iter()
        .flat_map(|e| std::iter::once(&e.file).chain(e.clock.as_ref()))
        .find(|f| {
            let p = Path::new(f);
            leads_out(p) || (p.is_absolute() && resolved(p).starts_with(&inside))
        })
        .cloned()
}

/// Refuses, before anything moves, a take in `dir` whose session file
/// names a file by a path that won't lead to it once the take is in a
/// watch ([`named_outside`]); `again` ends the sentence with what to do
/// once the file is in the take's folder. Add to Watch and File Under a
/// Watch both ask it (#876, #882). The take is named by its folder's
/// name, not its whole path, so the message stays short in the app.
fn stays_readable(dir: &Path, again: &str) -> Result<(), String> {
    // A path from the root is shown by its file's name, so it doesn't
    // wrap either.
    let short = |f: String| {
        if Path::new(&f).is_absolute() {
            format!("…/{}", file_name(Path::new(&f)))
        } else {
            f
        }
    };
    match named_outside(dir).map(short) {
        Some(f) => Err(format!(
            "\"{}\" names {f}, a path that won't lead to the file once the take is in the \
             watch. Put the file in the take's folder, name it by its path there (such as \
             audio.wav), and {again}.",
            file_name(dir)
        )),
        None => Ok(()),
    }
}

/// Whether the relative path `p` leads out of the folder it is taken
/// from.
fn leads_out(p: &Path) -> bool {
    use std::path::Component;
    let mut depth = 0i32;
    for c in p.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => return false,
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            Component::Normal(_) => depth += 1,
        }
    }
    false
}

/// The fingerprint of each sound file or folder and clock log that the
/// take in `dir`'s session file `m` names outside its folder (by a path
/// leading out of it, or from the root), in the order named: `None` for
/// one that isn't there. What is inside is the take's own stamp's
/// ([`reading_stamp`]). Quick, it reads no sound.
fn linked(dir: &Path, m: &Manifest) -> Vec<(PathBuf, Option<Stamp>)> {
    m.recordings
        .iter()
        .flat_map(|e| std::iter::once(&e.file).chain(e.clock.as_ref()))
        .filter(|f| Path::new(f).is_absolute() || leads_out(Path::new(f)))
        .map(|f| {
            let p = dir.join(f);
            let s = path_stamp(&p);
            (p, s)
        })
        .collect()
}

/// The fingerprint of the file or folder at `p`, `None` when it isn't
/// there.
fn path_stamp(p: &Path) -> Option<Stamp> {
    let m = fs::metadata(p).ok()?;
    if m.is_dir() {
        return Some(stamp(p));
    }
    Some(Stamp {
        bytes: m.len(),
        entries: 1,
        newest_ns: modified_ns(&m),
    })
}

/// Whether the take in the folder `take_dir` is a Positions Test's: it
/// keeps the test's results ([`POSITIONS_TEST_FILE`]) beside its sound.
pub fn is_positions_test(take_dir: &Path) -> bool {
    take_dir.join(POSITIONS_TEST_FILE).is_file()
}

/// A length of time as the app writes it: `1:30:00`, or `4:05` under an
/// hour.
pub fn length_text(t: f64) -> String {
    let s = t.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

// A figure's text (`signed_text`, `fixed_text`, `pm_text`) and its
// place (`figure_digits`, #1470) live in `figure_text`; these paths
// stay for every caller.
pub use crate::figure_text::{base_digits, figure_digits, fixed_text, pm_text, signed_text};

/// A size on disk as the app shows it: `7.7 GB`, `58.3 MB`, `912 kB`,
/// `40 bytes`.
pub fn size_text(n: u64) -> String {
    let n = n as f64;
    match n {
        n if n >= 1e9 => format!("{:.1} GB", n / 1e9),
        n if n >= 1e6 => format!("{:.1} MB", n / 1e6),
        n if n >= 1e3 => format!("{:.0} kB", n / 1e3),
        n => format!("{n:.0} bytes"),
    }
}

/// One watch and its takes, newest first.
#[derive(Debug, Clone)]
pub struct WatchEntry {
    pub dir: PathBuf,
    pub watch: Watch,
    pub takes: Vec<Take>,
    /// Images in `photos/`, by name; the first is the thumbnail.
    pub photos: Vec<PathBuf>,
}

impl WatchEntry {
    pub fn bytes(&self) -> u64 {
        self.takes.iter().map(|t| t.bytes).sum()
    }

    pub fn take_dir(&self, take: &Take) -> PathBuf {
        self.dir.join(TAKES).join(&take.name)
    }
}

/// The library as the last scan found it.
#[derive(Debug, Clone)]
pub struct Library {
    pub root: PathBuf,
    /// By name.
    pub watches: Vec<WatchEntry>,
    /// The takes in `Unsorted/`, newest first, as a watch called Unsorted
    /// with an empty id (no watch has one) and no photos.
    pub unsorted: WatchEntry,
    /// Folders that look like a watch or a take but couldn't be read, and
    /// why: listed, never changed.
    pub unreadable: Vec<String>,
    /// Why this copy of the app can only read the library (another copy
    /// has it open, or its folder can't be written), else `None`.
    pub read_only: Option<String>,
    /// The lock held while the library is open here, shared by clones.
    lock: Option<std::sync::Arc<fs::File>>,
    /// Opened with [`Library::open_to_add`] without the lock: takes may
    /// be added, nothing else changed.
    adding: bool,
}

/// A take's fingerprint for the cache: the bytes and newest change of
/// every file in it, and how many files and folders it holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Stamp {
    bytes: u64,
    entries: u64,
    newest_ns: u128,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    format: u32,
    /// By the take's path in the library, `/`-separated.
    takes: BTreeMap<String, (Stamp, Take)>,
}

impl Library {
    /// Make a new library at `root`: the folder must be missing or empty.
    pub fn create(root: &Path) -> Result<Library, String> {
        if fs::read_dir(root).is_ok_and(|mut d| d.next().is_some()) {
            return Err(format!(
                "{} already holds files, and a new Watches folder needs an empty folder",
                root.display()
            ));
        }
        fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
        let text = toml::to_string(&LibraryFile { format: FORMAT }).map_err(|e| e.to_string())?;
        write_atomic(&root.join(LIBRARY_FILE), text.as_bytes())?;
        Library::open(root)
    }

    /// Whether `root` holds a library.
    pub fn is_library(root: &Path) -> bool {
        root.join(LIBRARY_FILE).is_file()
    }

    /// The library at `root`, scanned. Its `library.toml` must be one this
    /// version reads.
    pub fn open(root: &Path) -> Result<Library, String> {
        Library::open_as(root, true)
    }

    /// The library at `root`, scanned, to read only: it takes no lock and
    /// writes nothing, not even the cache, so a copy of the app that has
    /// it open is not disturbed. For the command line.
    pub fn read(root: &Path) -> Result<Library, String> {
        Library::open_as(root, false)
    }

    /// The library at `root`, to add takes to even while a copy of the app
    /// has it open: with the lock when it is free, so anything can be
    /// changed, else without it, and then only [`Library::add_take`]
    /// works. A take is put together beside its place and moved in whole,
    /// a new folder nothing else writes, so the app reads it at its next
    /// scan; a watch's details, which the app may be writing, are left
    /// alone. For the command line.
    pub fn open_to_add(root: &Path) -> Result<Library, String> {
        let mut lib = Library::open(root)?;
        lib.adding = lib.lock.is_none();
        Ok(lib)
    }

    fn open_as(root: &Path, locked: bool) -> Result<Library, String> {
        let p = root.join(LIBRARY_FILE);
        let text = fs::read_to_string(&p).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "{} is not a Timegrapher Watches folder (it has no {LIBRARY_FILE})",
                    root.display()
                )
            } else {
                format!("{}: {e}", p.display())
            }
        })?;
        let f: LibraryFile = toml::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        if f.format > FORMAT {
            return Err(format!(
                "{} was made by a newer version of Timegrapher (library format {}, this \
                 version reads {FORMAT})",
                root.display(),
                f.format
            ));
        }
        let (lock, read_only) = match locked.then(|| lock(root)) {
            Some(Ok(f)) => (Some(std::sync::Arc::new(f)), None),
            Some(Err(why)) => (None, Some(why)),
            None => (None, Some("It was opened to read only".into())),
        };
        let mut lib = Library {
            root: root.to_path_buf(),
            watches: Vec::new(),
            unsorted: unsorted_entry(root),
            unreadable: Vec::new(),
            read_only,
            lock,
            adding: false,
        };
        lib.scan();
        Ok(lib)
    }

    /// Let go of the lock, so the library can be moved; it reads as
    /// read-only from then on.
    pub fn unlock(&mut self) {
        self.lock = None;
        self.read_only.get_or_insert_with(|| {
            "The Watches folder is being moved, so it can only be read".into()
        });
    }

    /// `Err` saying why, where this copy of the app can only read.
    fn writable(&self) -> Result<(), String> {
        self.read_only.clone().map_or(Ok(()), Err)
    }

    /// The library at `root`, made there first when the folder is missing
    /// or empty: the app's first start at the default location.
    pub fn open_or_create(root: &Path) -> Result<Library, String> {
        if Library::is_library(root) {
            Library::open(root)
        } else {
            Library::create(root)
        }
    }

    /// Read the folders again.
    pub fn scan(&mut self) {
        let cache_path = self.root.join(CACHE_FILE);
        let old: Cache = fs::read(&cache_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .filter(|c: &Cache| c.format == CACHE_FORMAT)
            .unwrap_or_default();
        let mut cache = Cache {
            format: CACHE_FORMAT,
            takes: BTreeMap::new(),
        };
        self.watches.clear();
        self.unreadable.clear();
        for dir in subfolders(&self.root) {
            if !dir.join(WATCH_FILE).is_file() {
                continue;
            }
            let watch = match Watch::read(&dir) {
                Ok(w) => w,
                Err(e) => {
                    self.unreadable.push(e);
                    continue;
                }
            };
            let takes = self.scan_takes(&dir, &old, &mut cache);
            let mut photos: Vec<PathBuf> = fs::read_dir(dir.join(PHOTOS))
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| is_image(p))
                .collect();
            photos.sort();
            self.watches.push(WatchEntry {
                dir,
                watch,
                takes,
                photos,
            });
        }
        self.unsorted = unsorted_entry(&self.root);
        if self.unsorted_dir().is_ok() {
            let dir = self.unsorted.dir.clone();
            self.unsorted.takes = self.scan_takes(&dir, &old, &mut cache);
        }
        self.watches
            .sort_by_key(|w| (w.watch.name.to_lowercase(), w.watch.name.clone()));
        // A cache that can't be written is only slower next time.
        if let (None, Ok(b)) = (&self.read_only, serde_json::to_vec(&cache)) {
            let _ = write_atomic(&cache_path, &b);
        }
    }

    /// The takes in the folder `dir/takes`, newest first, from the cache
    /// where unchanged; each goes into the new cache.
    fn scan_takes(&mut self, dir: &Path, old: &Cache, cache: &mut Cache) -> Vec<Take> {
        let folder = file_name(dir);
        let mut takes = Vec::new();
        for t in subfolders(&dir.join(TAKES)) {
            if !(t.join(take::MANIFEST).is_file() || t.join(take::RECORD).is_file()) {
                continue;
            }
            let key = format!("{folder}/{TAKES}/{}", file_name(&t));
            let stamp = stamp(&t);
            let take = match old.takes.get(&key) {
                Some((s, take)) if *s == stamp && take.name == file_name(&t) => take.clone(),
                _ => match read_take(&t, stamp.bytes) {
                    Ok(take) => take,
                    Err(e) => {
                        self.unreadable.push(e);
                        continue;
                    }
                },
            };
            cache.takes.insert(key, (stamp, take.clone()));
            takes.push(take);
        }
        // Newest first: by when each started, then by name.
        takes.sort_by(|a, b| (&b.started_utc, &b.name).cmp(&(&a.started_utc, &a.name)));
        takes
    }

    /// Every take's bytes, Unsorted's too.
    pub fn bytes(&self) -> u64 {
        self.watches.iter().map(WatchEntry::bytes).sum::<u64>() + self.unsorted.bytes()
    }

    /// Where a take made with Quick Listen is kept: `Unsorted/takes`. `Err`
    /// saying why when the folder called Unsorted is a watch's (one made
    /// outside the app).
    pub fn unsorted_dir(&self) -> Result<PathBuf, String> {
        if self.unsorted.dir.join(WATCH_FILE).exists() {
            return Err(format!(
                "{} holds a watch, so takes made with Quick Listen have nowhere to go: rename \
                 that folder",
                self.unsorted.dir.display()
            ));
        }
        Ok(self.unsorted.dir.join(TAKES))
    }

    /// The watch named `name`, ignoring case and spaces at either end.
    pub fn find(&self, name: &str) -> Option<&WatchEntry> {
        let key = name.trim().to_lowercase();
        self.watches
            .iter()
            .find(|w| w.watch.name.trim().to_lowercase() == key)
    }

    /// Add `watch`: its folder, named from its name, with its details,
    /// `takes/` and `photos/`. Returns the folder.
    pub fn add(&mut self, watch: &Watch) -> Result<PathBuf, String> {
        self.writable()?;
        let name = watch.name.trim();
        if name.is_empty() {
            return Err("A watch needs a name".into());
        }
        unsorted_name(name)?;
        if self.find(name).is_some() {
            return Err(format!("There is already a watch called \"{name}\""));
        }
        let dir = unused_dir(&self.root, &folder_name(name));
        fs::create_dir_all(dir.join(TAKES))
            .and_then(|_| fs::create_dir_all(dir.join(PHOTOS)))
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        watch.write(&dir)?;
        self.scan();
        Ok(dir)
    }

    /// Write a watch's changed details into its folder (found by its id).
    /// The folder keeps its name.
    pub fn update(&mut self, watch: &Watch) -> Result<(), String> {
        self.writable()?;
        let name = watch.name.trim();
        if name.is_empty() {
            return Err("A watch needs a name".into());
        }
        unsorted_name(name)?;
        if self.find(name).is_some_and(|w| w.watch.id != watch.id) {
            return Err(format!("There is already a watch called \"{name}\""));
        }
        let dir = self
            .watches
            .iter()
            .find(|w| w.watch.id == watch.id)
            .map(|w| w.dir.clone())
            .ok_or_else(|| format!("\"{name}\" is no longer in your watches"))?;
        watch.write(&dir)?;
        self.scan();
        Ok(())
    }

    /// The watch whose `takes/` holds the take folder `take`. A path that
    /// isn't plain ([`plain`]) is no take: `<watch>/takes/..` has
    /// `takes/` for its parent but is the watch's own folder (Felix on
    /// #1487).
    pub fn watch_of(&self, take: &Path) -> Option<&WatchEntry> {
        if !plain(take) {
            return None;
        }
        let parent = take.parent()?;
        self.watches.iter().find(|w| w.dir.join(TAKES) == parent)
    }

    /// Whether `take` is a take folder in Unsorted.
    pub fn is_unsorted(&self, take: &Path) -> bool {
        plain(take)
            && self
                .unsorted_dir()
                .is_ok_and(|d| take.parent() == Some(d.as_path()))
    }

    /// `take`'s watch (Unsorted for one there), where this copy of the app
    /// may change the library.
    fn take_in_library(&self, take: &Path) -> Result<&WatchEntry, String> {
        self.writable()?;
        self.watch_of(take)
            .or_else(|| self.is_unsorted(take).then_some(&self.unsorted))
            .filter(|_| take.is_dir())
            .ok_or_else(|| format!("{} is not a take of these watches", take.display()))
    }

    /// Rename a take's folder to `name` (cleaned for every system, as a
    /// watch's is). Returns the folder.
    pub fn rename_take(&mut self, take: &Path, name: &str) -> Result<PathBuf, String> {
        self.take_in_library(take)?;
        let name = name.trim();
        if name.is_empty() {
            return Err("A take needs a name".into());
        }
        let to = take.with_file_name(folder_name(name));
        if to == take {
            return Ok(to);
        }
        // Another take of that name; a change of case alone renames.
        if fs::symlink_metadata(&to).is_ok()
            && !file_name(&to).eq_ignore_ascii_case(&file_name(take))
        {
            return Err(format!(
                "There is already a take called \"{}\"",
                file_name(&to)
            ));
        }
        fs::rename(take, &to).map_err(|e| format!("Renaming {}: {e}", take.display()))?;
        self.scan();
        Ok(to)
    }

    /// Move a take into another watch (by its id), keeping its name, or
    /// with a number added where that watch has a take of that name. A
    /// take in Unsorted is filed under a watch this way.
    pub fn move_take(&mut self, take: &Path, watch_id: &str) -> Result<PathBuf, String> {
        self.take_in_library(take)?;
        let to_watch = self
            .watches
            .iter()
            .find(|w| w.watch.id == watch_id)
            .ok_or("That watch is no longer in your watches")?;
        let takes = to_watch.dir.join(TAKES);
        if take.parent() == Some(takes.as_path()) {
            return Ok(take.to_path_buf());
        }
        stays_readable(take, "move it again")?;
        fs::create_dir_all(&takes).map_err(|e| format!("{}: {e}", takes.display()))?;
        let to = unused_dir(&takes, &file_name(take));
        fs::rename(take, &to).map_err(|e| format!("Moving {}: {e}", take.display()))?;
        self.scan();
        Ok(to)
    }

    /// Copy the image files `files` into the `photos/` of the watch of id
    /// `watch_id`, each keeping its name, with a number added where the
    /// watch has a photo of that name. A copy that fails leaves nothing
    /// behind. Returns the copies.
    pub fn add_photos(
        &mut self,
        watch_id: &str,
        files: &[PathBuf],
    ) -> Result<Vec<PathBuf>, String> {
        self.writable()?;
        let dir = self
            .watches
            .iter()
            .find(|w| w.watch.id == watch_id)
            .map(|w| w.dir.join(PHOTOS))
            .ok_or("That watch is no longer in your watches")?;
        if let Some(f) = files.iter().find(|f| !is_image(f)) {
            return Err(format!(
                "{} is not a photo the app can show (JPEG, PNG, HEIC or TIFF)",
                f.display()
            ));
        }
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut added = Vec::new();
        let mut failed = None;
        for f in files {
            let to = unused_file(&dir, &file_name(f));
            let part = dir.join(format!(".{}.part", file_name(&to)));
            let copied = fs::copy(f, &part)
                // Opened to write: Windows refuses to flush a file opened
                // only to read.
                .and_then(|_| fs::OpenOptions::new().write(true).open(&part)?.sync_all())
                .and_then(|_| crate::recorder::rename_settled(&part, &to));
            match copied {
                Ok(()) => added.push(to),
                Err(e) => {
                    let _ = fs::remove_file(&part);
                    failed = Some(format!("Copying {}: {e}", f.display()));
                    break;
                }
            }
        }
        self.scan();
        failed.map_or(Ok(added), Err)
    }

    /// Delete one of a watch's photos.
    pub fn delete_photo(&mut self, photo: &Path) -> Result<(), String> {
        self.writable()?;
        if !self
            .watches
            .iter()
            .any(|w| w.photos.iter().any(|p| p == photo))
        {
            return Err(format!(
                "{} is not a photo of these watches",
                photo.display()
            ));
        }
        let gone = fs::remove_file(photo);
        self.scan();
        gone.map_err(|e| format!("Deleting {}: {e}", photo.display()))
    }

    /// Delete a take's folder and everything in it.
    pub fn delete_take(&mut self, take: &Path) -> Result<(), String> {
        self.take_in_library(take)?;
        let gone = fs::remove_dir_all(take);
        self.scan();
        gone.map_err(|e| format!("Deleting {}: {e}", take.display()))
    }

    /// Take back a watch just added (by its id) while it is still empty:
    /// no takes and no photos, as when what it was added for failed
    /// (#1275). A watch with anything in it is left as it is.
    pub fn remove_empty(&mut self, id: &str) -> Result<(), String> {
        self.writable()?;
        let w = self
            .watches
            .iter()
            .find(|w| w.watch.id == id)
            .ok_or("That watch is no longer in your watches")?;
        // Only a folder not there is empty: one unreadable may hold anything.
        let empty = |d: PathBuf| match fs::read_dir(d) {
            Ok(mut e) => e.next().is_none(),
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        };
        if !empty(w.dir.join(TAKES)) || !empty(w.dir.join(PHOTOS)) {
            return Err(format!("\"{}\" has takes or photos in it", w.watch.name));
        }
        let dir = w.dir.clone();
        let gone = fs::remove_dir_all(&dir);
        self.scan();
        gone.map_err(|e| format!("Removing {}: {e}", dir.display()))
    }

    /// Put `kept` (the take kept as its analysis alone, written beside it)
    /// in place of `take`: the take is moved aside, `kept` takes its name,
    /// and only then is the take with its sound deleted. `Ok(Some(why))`
    /// when the kept take is in place but some of the old one couldn't be
    /// deleted. On an error the take is as it was, and so is `kept`.
    pub fn replace_take(&mut self, take: &Path, kept: &Path) -> Result<Option<String>, String> {
        self.take_in_library(take)?;
        if kept.parent() != take.parent() || !crate::analysis_take::is_analysis_only(kept) {
            return Err(format!("{} is not a kept take beside it", kept.display()));
        }
        let aside = take.with_file_name(format!(".{}.with-sound", file_name(take)));
        let _ = fs::remove_dir_all(&aside);
        fs::rename(take, &aside).map_err(|e| format!("Moving {} aside: {e}", take.display()))?;
        if let Err(e) = fs::rename(kept, take) {
            // Put back as it was.
            let _ = fs::rename(&aside, take);
            return Err(format!("Putting the kept take in place: {e}"));
        }
        let gone = fs::remove_dir_all(&aside);
        self.scan();
        Ok(gone.err().map(|e| {
            format!(
                "The take is kept as its analysis, but some of its sound in {} couldn't be \
                 deleted ({e})",
                aside.display()
            )
        }))
    }

    /// Keep Analysis Only's last step, one for the app and the CLI: the
    /// take `take` written as its analysis alone in `kept`
    /// ([`crate::analysis_take::write_kept`]) put in its place
    /// ([`Library::replace_take`]). On an error the take is as it was,
    /// and `kept` is removed where it is the folder `write_kept` writes
    /// beside the take (never any other).
    pub fn put_kept(&mut self, take: &Path, kept: &Path) -> Result<Option<String>, String> {
        let placed = self.replace_take(take, kept);
        if placed.is_err() && kept == crate::analysis_take::kept_beside(take) {
            let _ = fs::remove_dir_all(kept);
        }
        placed
    }

    /// Add the take folder `from`, from outside the library, to a watch
    /// (by its id): copied, or moved when `keep_original` is false. It is
    /// copied beside its new place first and moved in whole, so a copy cut
    /// short is not a take; a move onto another disk copies, then deletes
    /// the original. `Ok((folder, Some(why)))` when it is in the watch but
    /// some of the original couldn't be deleted.
    pub fn add_take(
        &mut self,
        from: &Path,
        watch_id: &str,
        keep_original: bool,
    ) -> Result<(PathBuf, Option<String>), String> {
        if !self.adding {
            self.writable()?;
        } else if !keep_original {
            // Without the lock the app may move or delete the watch while
            // the take goes in: only a copy keeps the original safe.
            return Err(
                "the app has these watches open, so a take can only be copied in, not moved: \
                 leave out --move, or close the app first"
                    .into(),
            );
        }
        if !(from.join(take::MANIFEST).is_file() || from.join(take::RECORD).is_file()) {
            return Err(format!(
                "{} is not a take folder (it has no session.toml or session.json)",
                from.display()
            ));
        }
        self.not_already_in(from)?;
        if from.join(take::MANIFEST).is_file() {
            // A session file that doesn't read would show as a broken take.
            manifest_of(from).map_err(|e| format!("this take's session file doesn't read: {e}"))?;
        }
        stays_readable(from, "add the take again")?;
        let to_watch = self
            .watches
            .iter()
            .find(|w| w.watch.id == watch_id)
            .ok_or("That watch is no longer in your watches")?;
        let takes = to_watch.dir.join(TAKES);
        if self.adding {
            // Never make again a watch the app has just renamed or deleted.
            if !to_watch.dir.join(WATCH_FILE).is_file() {
                return Err(format!(
                    "\"{}\" is no longer in your watches",
                    to_watch.watch.name
                ));
            }
            match fs::create_dir(&takes) {
                Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
                    return Err(format!("{}: {e}", takes.display()))
                }
                _ => {}
            }
        } else {
            fs::create_dir_all(&takes).map_err(|e| format!("{}: {e}", takes.display()))?;
        }
        let to = unused_dir(&takes, &file_name(from));
        if !keep_original && fs::rename(from, &to).is_ok() {
            self.scan();
            return Ok((to, None));
        }
        let staged = staging(&takes, &to);
        let copied = copy_tree(from, &staged, &[])
            .and_then(|_| move_in(&staged, &takes, &file_name(from), to));
        let to = match copied {
            Ok(to) => to,
            Err(e) => {
                let _ = fs::remove_dir_all(&staged);
                return Err(e);
            }
        };
        let left = if keep_original {
            None
        } else {
            fs::remove_dir_all(from).err().map(|e| {
                format!(
                    "The take is in the watch, but some of {} couldn't be deleted ({e}): what \
                     is left there is a copy",
                    from.display()
                )
            })
        };
        self.scan();
        Ok((to, left))
    }

    /// The one guard both imports use (#1656), before anything is staged:
    /// `from` is refused when it is already inside the watches folder, or
    /// holds it, where the copy would be made inside what it copies and
    /// copy itself again and again (#1642).
    fn not_already_in(&self, from: &Path) -> Result<(), String> {
        let (from_at, root) = (resolved(from), resolved(&self.root));
        if from_at.starts_with(&root) {
            return Err(format!("{} is already in your watches", from.display()));
        }
        if root.starts_with(&from_at) {
            return Err(format!(
                "{} holds your watches folder: add the take's own folder instead",
                from.display()
            ));
        }
        Ok(())
    }

    /// Add a recording from outside the library that has no session file
    /// (a lone WAV or FLAC file, or a folder of them) to a watch (by its
    /// id), as a new take named `name`: the file, or everything in the
    /// folder, is copied in with `written` (file name and text: the session
    /// files that make it a take) beside it. It is put together beside its
    /// new place and moved in whole, so a copy cut short is not a take.
    pub fn add_recording(
        &mut self,
        from: &Path,
        watch_id: &str,
        name: &str,
        written: &[(&str, String)],
    ) -> Result<PathBuf, String> {
        self.writable()?;
        self.not_already_in(from)?;
        let to_watch = self
            .watches
            .iter()
            .find(|w| w.watch.id == watch_id)
            .ok_or("That watch is no longer in your watches")?;
        let takes = to_watch.dir.join(TAKES);
        fs::create_dir_all(&takes).map_err(|e| format!("{}: {e}", takes.display()))?;
        let to = unused_dir(&takes, name);
        let staged = staging(&takes, &to);
        let put = (|| {
            if from.is_dir() {
                copy_tree(from, &staged, &[])?;
            } else {
                fs::create_dir_all(&staged).map_err(|e| format!("{}: {e}", staged.display()))?;
                let dst = staged.join(from.file_name().ok_or("The recording has no name")?);
                fs::copy(from, &dst).map_err(|e| format!("{}: {e}", from.display()))?;
            }
            for (file, text) in written {
                let dst = staged.join(file);
                fs::write(&dst, text).map_err(|e| format!("{}: {e}", dst.display()))?;
            }
            move_in(&staged, &takes, name, to)
        })();
        let to = match put {
            Ok(to) => to,
            Err(e) => {
                let _ = fs::remove_dir_all(&staged);
                return Err(e);
            }
        };
        self.scan();
        Ok(to)
    }
}

/// Whether `p` names a folder plainly: no `.` or `..` in it, and a name
/// at its end, so its parent is the folder it is in.
fn plain(p: &Path) -> bool {
    use std::path::Component;
    p.file_name().is_some()
        && p.components()
            .all(|c| !matches!(c, Component::CurDir | Component::ParentDir))
}

/// A folder in `takes` to put a take together in before it is moved in
/// whole as `to`: hidden, so no scan lists it, and of its own, so two
/// programs adding a take of one name at once (the app, and the command
/// line adding while the app is open) never clear each other's.
fn staging(takes: &Path, to: &Path) -> PathBuf {
    takes.join(format!(".adding-{}-{}", file_name(to), new_id()))
}

/// Move the take put together in `staged` into `takes` as `to`, or, where
/// another program took that name meanwhile, as the next free name for
/// `base` ([`unused_dir`]). Returns where it went.
fn move_in(staged: &Path, takes: &Path, base: &str, to: PathBuf) -> Result<PathBuf, String> {
    match crate::recorder::rename_settled(staged, &to) {
        Ok(()) => Ok(to),
        Err(_) if to.exists() => {
            let again = unused_dir(takes, base);
            crate::recorder::rename_settled(staged, &again)
                .map(|()| again.clone())
                .map_err(|e| format!("{}: {e}", again.display()))
        }
        Err(e) => Err(format!("{}: {e}", to.display())),
    }
}

/// The entry standing for `root/Unsorted`, with no takes yet.
fn unsorted_entry(root: &Path) -> WatchEntry {
    WatchEntry {
        dir: root.join(UNSORTED),
        watch: Watch {
            id: String::new(),
            name: UNSORTED.into(),
            calibre: None,
            lift: None,
            bph: None,
            notes: None,
            archived: false,
            fields: Vec::new(),
        },
        takes: Vec::new(),
        photos: Vec::new(),
    }
}

/// `Err` for a watch name whose folder would be Unsorted's (on a system
/// that ignores case, too).
fn unsorted_name(name: &str) -> Result<(), String> {
    if folder_name(name).eq_ignore_ascii_case(UNSORTED) {
        return Err(format!(
            "\"{UNSORTED}\" is kept for takes made with Quick Listen: please choose another name"
        ));
    }
    Ok(())
}

/// `p` with links followed: the nearest folder of it that exists,
/// resolved, with the rest of `p` after it. A `..` in the part that
/// doesn't exist is left as it is (no caller passes one). The command
/// line's `record` decides by it whether a folder is a library's take
/// (#1641).
pub fn resolved(p: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut at = p;
    loop {
        if let Ok(real) = fs::canonicalize(at) {
            return rest.iter().rev().fold(real, |acc, c| acc.join(c));
        }
        match (at.parent(), at.file_name()) {
            (Some(up), Some(name)) => {
                rest.push(name.to_owned());
                at = up;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// Take the library's lock: `Err` saying why it can only be read.
fn lock(root: &Path) -> Result<fs::File, String> {
    let f = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(LOCK_FILE))
        .map_err(|e| {
            format!(
                "{} can't be written ({e}), so it can only be read",
                root.display()
            )
        })?;
    // Tried for a second: a program this app or another just started
    // holds a lock let go of a moment before, until the program is under
    // way.
    let mut tries = 20;
    loop {
        match f.try_lock() {
            Err(fs::TryLockError::WouldBlock) if tries > 0 => {
                tries -= 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            other => break lock_outcome(root, f, other),
        }
    }
}

fn lock_outcome(
    root: &Path,
    f: fs::File,
    tried: Result<(), fs::TryLockError>,
) -> Result<fs::File, String> {
    match tried {
        Ok(()) => Ok(f),
        Err(fs::TryLockError::WouldBlock) => Err(
            "Another copy of Timegrapher has these watches open, so this one can only read them"
                .into(),
        ),
        Err(fs::TryLockError::Error(e)) => Err(format!(
            "{} can't be locked ({e}), so it can only be read",
            root.display()
        )),
    }
}

/// Move the library at `from` to `to`, which must be missing or empty
/// and not inside `from`, links followed. Refused while another copy of
/// the app has it open (so the caller [`Library::unlock`]s its own
/// first). A rename is tried first; where it fails (another disk), the
/// library is copied, each file written to the disk with its
/// modification time, `library.toml` last, the copy's bytes and entries
/// checked against `from`, and only then `from` removed, its
/// `library.toml` first. A link or any other file that is neither a
/// plain file nor a folder stops the copy. `Ok(None)` when it is all at
/// `to` and `from` is gone. `Ok(Some(why))` when it is all at `to` but
/// some of `from` couldn't be removed; `why` says so. A copy that fails
/// or doesn't match is removed, its `library.toml` first, and `from` left
/// as it was. A copy cut short by the app ending leaves `from` whole and,
/// at `to`, files without a `library.toml`, which is not a library.
pub fn move_library(from: &Path, to: &Path) -> Result<Option<String>, String> {
    if !Library::is_library(from) {
        return Err(format!(
            "{} is not a Timegrapher Watches folder",
            from.display()
        ));
    }
    let (real_from, real_to) = (resolved(from), resolved(to));
    if real_to.starts_with(&real_from) {
        return Err("A Watches folder can't be moved inside itself".into());
    }
    if fs::read_dir(to).is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!("{} already holds files", to.display()));
    }
    // An empty folder left there is replaced.
    let _ = fs::remove_dir(to);
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // Checked, then let go of: Windows renames no folder with a file in
    // it held open.
    drop(lock(from).map_err(|e| format!("The Watches folder can't be moved: {e}"))?);
    if fs::rename(from, to).is_ok() {
        return Ok(None);
    }
    let held = lock(from).map_err(|e| format!("The Watches folder can't be moved: {e}"))?;
    copy_across(from, to, Some(held))
}

/// [`move_library`] onto another disk: copy, check, then remove `from`.
/// `held` is `from`'s lock, let go of before `from` is removed.
fn copy_across(from: &Path, to: &Path, held: Option<fs::File>) -> Result<Option<String>, String> {
    // library.toml last, so a copy cut short is not a library.
    let copied = copy_tree(from, to, &[LIBRARY_FILE, LOCK_FILE])
        .and_then(|_| copy_file(&from.join(LIBRARY_FILE), &to.join(LIBRARY_FILE)))
        .and_then(|_| {
            // The folder's new entry written to the disk too.
            if let Ok(d) = fs::File::open(to) {
                let _ = d.sync_all();
            }
            // The lock is not copied.
            let locks = |d: &Path| u64::from(d.join(LOCK_FILE).exists());
            let (a, b) = (stamp(from), stamp(to));
            if (a.bytes, a.entries - locks(from)) == (b.bytes, b.entries - locks(to)) {
                Ok(())
            } else {
                Err(format!(
                    "The copy in {} doesn't match the Watches folder ({} of {} bytes)",
                    to.display(),
                    b.bytes,
                    a.bytes
                ))
            }
        });
    match copied {
        Ok(()) => {
            drop(held);
            let gone =
                fs::remove_file(from.join(LIBRARY_FILE)).and_then(|_| fs::remove_dir_all(from));
            Ok(gone.err().map(|e| {
                format!(
                    "The Watches folder is now in {}, but some of {} couldn't be removed ({e}): \
                     what is left there is a copy and can be deleted",
                    to.display(),
                    from.display()
                )
            }))
        }
        Err(e) => {
            // library.toml first, so what can't be removed is not a library.
            let _ = fs::remove_file(to.join(LIBRARY_FILE));
            let _ = fs::remove_dir_all(to);
            Err(e)
        }
    }
}

/// Copy `from` into `to`, leaving out the top-level files in `skip`.
fn copy_tree(from: &Path, to: &Path, skip: &[&str]) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| format!("{}: {e}", from.display()))?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if skip.iter().any(|s| e.file_name() == *s) {
            continue;
        }
        let kind = e
            .file_type()
            .map_err(|e| format!("{}: {e}", src.display()))?;
        if kind.is_dir() {
            copy_tree(&src, &dst, &[])?;
        } else if kind.is_file() {
            copy_file(&src, &dst)?;
        } else {
            // A link, a named pipe or a device: reading a pipe would never
            // end, and a link's target may be outside the library.
            return Err(format!(
                "{} is a link or a special file, which can't be copied to another disk: \
                 move it out of the Watches folder first",
                src.display()
            ));
        }
    }
    // The folder's own entries too, where the system allows it (not on
    // Windows, which can't open a folder as a file).
    if let Ok(d) = fs::File::open(to) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Copy one file with its modification time and write it to the disk
/// through the handle that wrote it, then give it the original's
/// permissions (so a read-only file stays read-only, and is never opened
/// for writing).
fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    let copied = (|| {
        let mut r = fs::File::open(src)?;
        let mut w = fs::File::create(dst)?;
        std::io::copy(&mut r, &mut w)?;
        w.set_modified(r.metadata()?.modified()?)?;
        w.sync_all()?;
        drop(w);
        fs::set_permissions(dst, r.metadata()?.permissions())
    })();
    copied.map_err(|e| format!("Copying {} to {}: {e}", src.display(), dst.display()))
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Folders in `dir` by name, leaving out hidden ones.
fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// A photo the app can show: JPEG, PNG, HEIC (as an iPhone takes them)
/// or TIFF.
pub fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "heic" | "heif" | "tif" | "tiff"
        )
    })
}

/// Bytes in the files under `dir`, all the way down (links not followed
/// nor counted): a take's size, before and after Keep Analysis Only.
pub fn folder_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir).map_or(0, |d| {
        d.filter_map(|e| e.ok())
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => folder_bytes(&e.path()),
                Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
                _ => 0,
            })
            .sum()
    })
}

/// Every file under `dir`: their bytes, the newest change, and how many
/// files and folders there are.
fn stamp(dir: &Path) -> Stamp {
    stamp_of(dir, |_| true)
}

/// The fingerprint of what a take's reading reads: everything in the take
/// (its session file, its sound wherever its entries name it, its clock
/// log and kept analysis) but the report `timegrapher session` writes into
/// it ([`crate::take::REPORT_DIR`]).
fn reading_stamp(dir: &Path) -> Stamp {
    let report = dir.join(crate::take::REPORT_DIR);
    stamp_of(dir, |sub| sub != report)
}

/// The fingerprint of the files in `dir`, looking into each folder in it
/// that `into` takes.
fn stamp_of(dir: &Path, into: impl Fn(&Path) -> bool) -> Stamp {
    let mut s = Stamp {
        bytes: 0,
        entries: 0,
        newest_ns: 0,
    };
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() && !into(&e.path()) {
                continue;
            }
            s.entries += 1;
            if m.is_dir() {
                stack.push(e.path());
            } else {
                s.bytes += m.len();
                s.newest_ns = s.newest_ns.max(modified_ns(&m));
            }
        }
    }
    s
}

/// When the file `m` describes last changed, in nanoseconds since 1970
/// (0 when that can't be told).
fn modified_ns(m: &fs::Metadata) -> u128 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos())
}

/// The sound card's error from the USB lines of `files`' first, ppm fast
/// ([`crate::clock::usb_lines`]), kept per file and its modified time, so
/// a library read again doesn't decode its takes again.
fn usb_ppm(files: &[PathBuf]) -> Option<f64> {
    type Seen = std::collections::HashMap<PathBuf, (Option<std::time::SystemTime>, Option<f64>)>;
    static SEEN: std::sync::Mutex<Option<Seen>> = std::sync::Mutex::new(None);
    let first = files.first()?;
    let modified = fs::metadata(first).and_then(|m| m.modified()).ok();
    if let Ok(seen) = SEEN.lock() {
        if let Some((at, ppm)) = seen.as_ref().and_then(|s| s.get(first)) {
            if *at == modified {
                return *ppm;
            }
        }
    }
    let ppm = crate::clock::usb_lines(files).ok().flatten().map(|u| u.ppm);
    if let Ok(mut seen) = SEEN.lock() {
        seen.get_or_insert_with(Default::default)
            .insert(first.clone(), (modified, ppm));
    }
    ppm
}

/// How a take folder's name starts, as `strftime` writes it: the minute it
/// started in its recording computer's time, `2026-10-22 09.10`. The app
/// and `timegrapher record` name takes by it, and [`named_start`] reads it.
pub const TAKE_NAME_TIME: &str = "%Y-%m-%d %H.%M";

/// When a take folder named by [`TAKE_NAME_TIME`] says it started, as
/// `2026-10-22T09:10` (its recording computer's time, no zone): None for a
/// name that doesn't start so, or names no real minute.
pub fn named_start(name: &str) -> Option<String> {
    let b = name.as_bytes().get(..16)?;
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let d = b.get(r)?;
        d.iter()
            .all(u8::is_ascii_digit)
            .then(|| d.iter().fold(0, |n, c| n * 10 + u32::from(c - b'0')))
    };
    let seps = [(4, b'-'), (7, b'-'), (10, b' '), (13, b'.')];
    if !seps.iter().all(|&(i, c)| b[i] == c) {
        return None;
    }
    let (y, mo, d, h, mi) = (
        num(0..4)?,
        num(5..7)?,
        num(8..10)?,
        num(11..13)?,
        num(14..16)?,
    );
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match mo {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => return None,
    };
    (d >= 1 && d <= days && h < 24 && mi < 60)
        .then(|| format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}"))
}

/// How long a take's sound is, seconds, from its files' headers, for a
/// take copied in without the `session.json` that says (#1272): a corpus
/// take, or one from another computer. None when a header doesn't give
/// its length.
fn sound_length(dir: &Path, m: &Manifest) -> Option<f64> {
    let seconds = |f: &PathBuf| {
        let i = crate::audio::info(f).ok()?;
        Some(i.frames? as f64 / f64::from(i.sample_rate.max(1)))
    };
    // Each entry is a sound file, or a folder of them read in order; a
    // file two entries name (one recording split in stretches, #1197) is
    // one sound, counted once.
    let mut files: Vec<PathBuf> = Vec::new();
    for r in &m.recordings {
        let p = dir.join(&r.file);
        let found = if p.is_dir() {
            crate::recording_order::recordings_in(&p, &[]).ok()?
        } else {
            vec![p]
        };
        for f in found {
            if !files.contains(&f) {
                files.push(f);
            }
        }
    }
    files
        .iter()
        .map(seconds)
        .sum::<Option<f64>>()
        .filter(|s| *s > 0.0)
}

/// How long a take kept as its analysis ran, from the analysis's own
/// header (its sound, which gave the length, is gone).
fn kept_length(dir: &Path) -> Option<f64> {
    crate::analysis_take::read_header(dir)
        .ok()
        .map(|h| h.duration_s)
}

/// What a take folder says about itself.
fn read_take(dir: &Path, bytes: u64) -> Result<Take, String> {
    let manifest: Option<Manifest> = match fs::read_to_string(dir.join(take::MANIFEST)) {
        Ok(text) => Some(
            toml::from_str(&text)
                .map_err(|e| format!("{}: {e}", dir.join(take::MANIFEST).display()))?,
        ),
        Err(_) => None,
    };
    let about: Option<serde_json::Value> = fs::read(dir.join(take::RECORD))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let audio = manifest.as_ref().and_then(|m| m.audio) != Some(false);
    let incomplete = match &manifest {
        Some(_) if !audio => !dir.join("analysis").is_dir(),
        // An entry naming a folder (`file = "."`, as the app's own takes
        // do) is there when the folder holds sound files.
        Some(m) => m.recordings.iter().any(|r| {
            let p = dir.join(&r.file);
            if p.is_dir() {
                crate::recording_order::recordings_in(&p, &[]).is_err()
            } else {
                !p.is_file()
            }
        }),
        None => false,
    };
    let mut positions: Vec<String> = Vec::new();
    for r in manifest.iter().flat_map(|m| &m.recordings) {
        if let Some(p) = &r.position {
            if !positions.contains(p) {
                positions.push(p.clone());
            }
        }
    }
    let at = |v: &serde_json::Value, k: &str| v.get(k).and_then(serde_json::Value::as_f64);
    // The rate kept is as heard, on the sound card's clock; the take's
    // clock reads it again as reading the take does
    // ([`crate::take::clock_of_files`]):
    // the recording's USB lines set the rate wherever they are, its clock
    // log placing only breaks, else the log when it measures the clock
    // well enough to.
    let ppm = || {
        let m = manifest.as_ref().filter(|_| audio && !incomplete)?;
        let first = m.recordings.first()?;
        let listed: Vec<String> = m.recordings.iter().map(|r| r.file.clone()).collect();
        let files = crate::recording_order::recordings_in(dir, &listed).ok()?;
        take::clock_of_files(dir, first.clock.as_deref(), &files, usb_ppm(&files), None)
            .ok()?
            .filter(crate::clock::ClockFit::corrects)
            .map(|f| f.ppm)
    };
    let at_end = about
        .as_ref()
        .and_then(|a| a.get("result"))
        .filter(|r| r.is_object())
        .map(|r| {
            let rate = at(r, "rate_s_per_day");
            AtEnd {
                rate_s_per_day: rate,
                amplitude_deg: at(r, "amplitude_deg"),
                beat_error_ms: crate::session::beat_error_signed(
                    at(r, "beat_error_unlock_ms"),
                    at(r, "beat_error_ms"),
                ),
                rate_corrected_s_per_day: rate
                    .and_then(|v| Some(crate::clockstore::correct_rate(v, ppm()?))),
            }
        });
    let started_utc = about
        .as_ref()
        .and_then(|a| a.get("started_utc"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let name = file_name(dir);
    Ok(Take {
        named_start: started_utc.is_none().then(|| named_start(&name)).flatten(),
        name,
        bytes,
        audio,
        incomplete,
        started_utc,
        duration_s: about
            .as_ref()
            .and_then(|a| at(a, "duration_s"))
            .or_else(|| {
                let m = manifest.as_ref().filter(|_| audio && !incomplete)?;
                sound_length(dir, m)
            })
            .or_else(|| (!audio && !incomplete).then(|| kept_length(dir)).flatten()),
        positions,
        at_end,
        power_reserve: fs::read_to_string(dir.join(crate::power_reserve::FILE))
            .ok()
            .and_then(|t| crate::power_reserve::Results::from_json(&t).ok())
            .map(|r| r.summary()),
    })
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
