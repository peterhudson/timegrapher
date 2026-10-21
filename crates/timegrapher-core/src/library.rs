//! The library of watches: one folder the app manages, holding a folder
//! per watch with its details, its photos and every take made of it.
//!
//! ```text
//! Timegrapher Data/              the library
//!   library.toml                 its format
//!   Rolex Yacht-Master 42/       one folder per watch
//!     watch.toml                 its details
//!     photos/
//!     takes/
//!       2026-10-20 14.32 Dial Up/   one standard take, unchanged
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
//! The scan reads each take's size and what its `session.json` says, and
//! keeps them in `.cache.json` at the top of the library, rebuilt when
//! missing: a take whose files haven't changed is not read again. The
//! folders stay the truth. Nothing here deletes or repairs anything: a
//! take with sound files missing (half synced, or copied in part) is
//! listed as incomplete, as it is.

use crate::session::{Manifest, Position};
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
    /// Random, made when the watch is added; never shown.
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

impl Watch {
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
/// never sees half of it.
fn write_atomic(p: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = p.with_extension("tmp");
    fs::write(&tmp, bytes)
        .and_then(|_| fs::rename(&tmp, p))
        .map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("{}: {e}", p.display())
        })
}

/// A folder name for `name` that every system accepts: the characters
/// Windows forbids and control characters become `-`, trailing dots and
/// spaces go, a name Windows keeps for devices gets `_` after it, and it
/// is cut to 100 characters. The name as typed stays in the files.
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

/// The readings `session.json` keeps from the end of a take, as the app
/// saw them then.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AtEnd {
    pub rate_s_per_day: Option<f64>,
    pub amplitude_deg: Option<f64>,
    /// Signed, from the unlock.
    pub beat_error_ms: Option<f64>,
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
    /// From `session.json`.
    pub started_utc: Option<String>,
    pub duration_s: Option<f64>,
    /// The positions `session.toml` names, in order, each once.
    pub positions: Vec<String>,
    pub at_end: Option<AtEnd>,
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
    /// Folders that look like a watch or a take but couldn't be read, and
    /// why: listed, never changed.
    pub unreadable: Vec<String>,
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
                "{} already holds files, and a new library needs an empty folder",
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
        let p = root.join(LIBRARY_FILE);
        let text = fs::read_to_string(&p).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "{} is not a Timegrapher library (it has no {LIBRARY_FILE})",
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
        let mut lib = Library {
            root: root.to_path_buf(),
            watches: Vec::new(),
            unreadable: Vec::new(),
        };
        lib.scan();
        Ok(lib)
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
            .filter(|c: &Cache| c.format == FORMAT)
            .unwrap_or_default();
        let mut cache = Cache {
            format: FORMAT,
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
            let folder = file_name(&dir);
            let mut takes = Vec::new();
            for t in subfolders(&dir.join(TAKES)) {
                if !(t.join("session.toml").is_file() || t.join("session.json").is_file()) {
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
        self.watches
            .sort_by_key(|w| (w.watch.name.to_lowercase(), w.watch.name.clone()));
        // A cache that can't be written is only slower next time.
        if let Ok(b) = serde_json::to_vec(&cache) {
            let _ = write_atomic(&cache_path, &b);
        }
    }

    /// Every take's bytes.
    pub fn bytes(&self) -> u64 {
        self.watches.iter().map(WatchEntry::bytes).sum()
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
        let name = watch.name.trim();
        if name.is_empty() {
            return Err("A watch needs a name".into());
        }
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
        let name = watch.name.trim();
        if name.is_empty() {
            return Err("A watch needs a name".into());
        }
        if self.find(name).is_some_and(|w| w.watch.id != watch.id) {
            return Err(format!("There is already a watch called \"{name}\""));
        }
        let dir = self
            .watches
            .iter()
            .find(|w| w.watch.id == watch.id)
            .map(|w| w.dir.clone())
            .ok_or_else(|| format!("\"{name}\" is no longer in the library"))?;
        watch.write(&dir)?;
        self.scan();
        Ok(())
    }
}

/// `p` with links followed: the nearest folder of it that exists,
/// resolved, with the rest of `p` after it.
fn resolved(p: &Path) -> PathBuf {
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

/// Move the library at `from` to `to`, which must be missing or empty:
/// renamed where both are on one disk, else copied whole, written to the
/// disk, checked and only then removed from `from`. `Ok(None)` when it
/// is all at `to`. `Ok(Some(why))` when it is all at `to` but some of
/// `from` couldn't be removed: `from` is no longer a library then (its
/// `library.toml` goes first), and `why` says what is left.
pub fn move_library(from: &Path, to: &Path) -> Result<Option<String>, String> {
    if !Library::is_library(from) {
        return Err(format!("{} is not a Timegrapher library", from.display()));
    }
    let (real_from, real_to) = (resolved(from), resolved(to));
    if real_to.starts_with(&real_from) {
        return Err("A library can't be moved inside itself".into());
    }
    if fs::read_dir(to).is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!("{} already holds files", to.display()));
    }
    // An empty folder left there is replaced.
    let _ = fs::remove_dir(to);
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(None);
    }
    copy_across(from, to)
}

/// [`move_library`] onto another disk: copy, check, then remove `from`.
fn copy_across(from: &Path, to: &Path) -> Result<Option<String>, String> {
    let copied = copy_tree(from, to).and_then(|_| {
        let (a, b) = (stamp(from), stamp(to));
        if (a.bytes, a.entries) == (b.bytes, b.entries) {
            Ok(())
        } else {
            Err(format!(
                "The copy in {} doesn't match the library ({} of {} bytes)",
                to.display(),
                b.bytes,
                a.bytes
            ))
        }
    });
    match copied {
        Ok(()) => {
            let gone =
                fs::remove_file(from.join(LIBRARY_FILE)).and_then(|_| fs::remove_dir_all(from));
            Ok(gone.err().map(|e| {
                format!(
                    "The library is now in {}, but some of {} couldn't be removed ({e}): \
                     what is left there is a copy and can be deleted",
                    to.display(),
                    from.display()
                )
            }))
        }
        Err(e) => {
            let _ = fs::remove_dir_all(to);
            Err(e)
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| format!("{}: {e}", from.display()))?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            copy_tree(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)
                // Opened for writing: Windows flushes only a file it may write.
                .and_then(|_| fs::OpenOptions::new().write(true).open(&dst)?.sync_all())
                .map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    // The folder's own entries too, where the system allows it (not on
    // Windows, which can't open a folder as a file).
    if let Ok(d) = fs::File::open(to) {
        let _ = d.sync_all();
    }
    Ok(())
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

fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "heic" | "webp" | "gif" | "tif" | "tiff"
        )
    })
}

/// Every file under `dir`: their bytes, the newest change, and how many
/// files and folders there are.
fn stamp(dir: &Path) -> Stamp {
    let mut s = Stamp {
        bytes: 0,
        entries: 0,
        newest_ns: 0,
    };
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            s.entries += 1;
            if m.is_dir() {
                stack.push(e.path());
            } else {
                s.bytes += m.len();
                let ns = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                s.newest_ns = s.newest_ns.max(ns);
            }
        }
    }
    s
}

/// What a take folder says about itself.
fn read_take(dir: &Path, bytes: u64) -> Result<Take, String> {
    let manifest: Option<Manifest> = match fs::read_to_string(dir.join("session.toml")) {
        Ok(text) => Some(
            toml::from_str(&text)
                .map_err(|e| format!("{}: {e}", dir.join("session.toml").display()))?,
        ),
        Err(_) => None,
    };
    let about: Option<serde_json::Value> = fs::read(dir.join("session.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let audio = manifest.as_ref().and_then(|m| m.audio) != Some(false);
    let incomplete = match &manifest {
        Some(_) if !audio => !dir.join("analysis").is_dir(),
        Some(m) => m.recordings.iter().any(|r| !dir.join(&r.file).is_file()),
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
    let at_end = about
        .as_ref()
        .and_then(|a| a.get("result"))
        .filter(|r| r.is_object())
        .map(|r| AtEnd {
            rate_s_per_day: at(r, "rate_s_per_day"),
            amplitude_deg: at(r, "amplitude_deg"),
            beat_error_ms: at(r, "beat_error_unlock_ms"),
        });
    Ok(Take {
        name: file_name(dir),
        bytes,
        audio,
        incomplete,
        started_utc: about
            .as_ref()
            .and_then(|a| a.get("started_utc"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        duration_s: about.as_ref().and_then(|a| at(a, "duration_s")),
        positions,
        at_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tg-library-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    /// A standard take folder: a manifest naming `files`, the ones in
    /// `present` written, and a session.json.
    fn take(dir: &Path, files: &[(&str, &str)], present: usize, started: &str) {
        fs::create_dir_all(dir).unwrap();
        let mut toml = String::from("watch = \"YM42\"\nbph = 28800\n");
        for (f, p) in files {
            toml += &format!("\n[[recording]]\nfile = \"{f}\"\nposition = \"{p}\"\n");
        }
        fs::write(dir.join("session.toml"), toml).unwrap();
        for (f, _) in &files[..present] {
            fs::write(dir.join(f), vec![0u8; 1000]).unwrap();
        }
        fs::write(
            dir.join("session.json"),
            format!(
                r#"{{"started_utc":"{started}","duration_s":120.0,
                    "result":{{"rate_s_per_day":3.5,"amplitude_deg":281.0,"beat_error_unlock_ms":-0.4}}}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn a_library_lists_its_watches_and_their_takes() {
        let root = scratch("lists");
        let mut lib = Library::create(&root).unwrap();
        assert!(lib.watches.is_empty());
        let mut w = Watch::new("Rolex Yacht-Master 42");
        w.calibre = Some("Rolex 3235".into());
        w.bph = Some(28800);
        w.fields.push(Field {
            label: "Serial Number".into(),
            kind: FieldKind::Text,
            value: "X123".into(),
        });
        let dir = lib.add(&w).unwrap();
        assert_eq!(file_name(&dir), "Rolex Yacht-Master 42");
        let takes = dir.join(TAKES);
        take(
            &takes.join("2026-10-20 14.32 Dial Up"),
            &[("take_part01-001.flac", "DU")],
            1,
            "2026-10-20T21:32:00Z",
        );
        take(
            &takes.join("2026-10-21 09.10"),
            &[
                ("take_part01-001.flac", "DU"),
                ("take_part02-001.flac", "DD"),
            ],
            1,
            "2026-10-21T16:10:00Z",
        );
        fs::write(dir.join(PHOTOS).join("dial.jpg"), b"jpeg").unwrap();
        fs::write(dir.join(PHOTOS).join("notes.txt"), b"not a photo").unwrap();
        lib.scan();
        let e = &lib.watches[0];
        assert_eq!(e.watch, w);
        assert_eq!(e.photos, [dir.join(PHOTOS).join("dial.jpg")]);
        let names: Vec<&str> = e.takes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["2026-10-21 09.10", "2026-10-20 14.32 Dial Up"]);
        // The six-position take is missing its second file.
        assert!(e.takes[0].incomplete);
        assert!(!e.takes[1].incomplete);
        assert_eq!(e.takes[0].positions, ["DU", "DD"]);
        let one = &e.takes[1];
        assert!(one.audio);
        assert_eq!(one.duration_s, Some(120.0));
        assert_eq!(
            one.at_end,
            Some(AtEnd {
                rate_s_per_day: Some(3.5),
                amplitude_deg: Some(281.0),
                beat_error_ms: Some(-0.4),
            })
        );
        let file_bytes = |t: &str| -> u64 {
            fs::read_dir(takes.join(t))
                .unwrap()
                .map(|e| e.unwrap().metadata().unwrap().len())
                .sum()
        };
        assert_eq!(one.bytes, file_bytes("2026-10-20 14.32 Dial Up"));
        assert_eq!(
            lib.bytes(),
            file_bytes("2026-10-20 14.32 Dial Up") + file_bytes("2026-10-21 09.10")
        );
        // Found again from the folders alone, after the library is moved.
        let moved = scratch("lists-moved");
        move_library(&root, &moved).unwrap();
        assert!(!root.exists());
        let again = Library::open(&moved).unwrap();
        assert_eq!(again.watches[0].watch, w);
        assert_eq!(again.watches[0].takes, lib.watches[0].takes);
        let _ = fs::remove_dir_all(&moved);
    }

    #[test]
    fn a_take_kept_as_analysis_only_reads_as_such() {
        let root = scratch("analysis-only");
        let mut lib = Library::create(&root).unwrap();
        let dir = lib.add(&Watch::new("Pepsi")).unwrap();
        let t = dir.join(TAKES).join("2026-10-22 22.00 Dial Up");
        take(
            &t,
            &[("take_part01-001.flac", "DU")],
            0,
            "2026-10-23T05:00:00Z",
        );
        let toml = fs::read_to_string(t.join("session.toml")).unwrap();
        fs::write(t.join("session.toml"), format!("audio = false\n{toml}")).unwrap();
        lib.scan();
        let k = &lib.watches[0].takes[0];
        assert!(!k.audio);
        // Without its analysis/ folder it is incomplete; with it, whole,
        // though its sound files are gone.
        assert!(k.incomplete);
        fs::create_dir(t.join("analysis")).unwrap();
        lib.scan();
        assert!(!lib.watches[0].takes[0].incomplete);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_cache_is_used_until_a_take_changes() {
        let root = scratch("cache");
        let mut lib = Library::create(&root).unwrap();
        let dir = lib.add(&Watch::new("Daytona")).unwrap();
        let t = dir.join(TAKES).join("2026-10-20 10.00 Dial Up");
        take(
            &t,
            &[("take_part01-001.flac", "DU")],
            1,
            "2026-10-20T17:00:00Z",
        );
        lib.scan();
        assert!(root.join(CACHE_FILE).is_file());
        // A cache entry is trusted while the take's files are unchanged...
        let cache = fs::read_to_string(root.join(CACHE_FILE)).unwrap();
        fs::write(root.join(CACHE_FILE), cache.replace("3.5", "9.5")).unwrap();
        lib.scan();
        let rate = |l: &Library| {
            l.watches[0].takes[0]
                .at_end
                .as_ref()
                .unwrap()
                .rate_s_per_day
        };
        assert_eq!(rate(&lib), Some(9.5));
        // ...and read afresh when they change.
        fs::write(t.join("clock.csv"), "x\n").unwrap();
        lib.scan();
        assert_eq!(rate(&lib), Some(3.5));
        // A cache that is gone or unreadable is made again.
        fs::write(root.join(CACHE_FILE), "not json").unwrap();
        lib.scan();
        assert_eq!(rate(&lib), Some(3.5));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn names_are_unique_and_folders_safe_everywhere() {
        let root = scratch("names");
        let mut lib = Library::create(&root).unwrap();
        lib.add(&Watch::new("Seiko 5")).unwrap();
        assert!(lib.add(&Watch::new("  seiko 5 ")).is_err());
        assert!(lib.add(&Watch::new("   ")).is_err());
        // Another name that cleans to the same folder gets a number.
        let a = lib.add(&Watch::new("Omega: 321")).unwrap();
        let b = lib.add(&Watch::new("Omega? 321")).unwrap();
        assert_eq!(file_name(&a), "Omega- 321");
        assert_eq!(file_name(&b), "Omega- 321 2");
        assert_eq!(lib.find("omega? 321").unwrap().watch.name, "Omega? 321");
        // Renaming a watch keeps its folder; taking another's name is refused.
        let mut w = lib.find("Omega: 321").unwrap().watch.clone();
        w.name = "Speedmaster".into();
        lib.update(&w).unwrap();
        assert_eq!(lib.find("Speedmaster").unwrap().dir, a);
        w.name = "Seiko 5".into();
        assert!(lib.update(&w).is_err());
        assert_eq!(folder_name("CON"), "CON_");
        assert_eq!(folder_name("lpt1.txt"), "lpt1_.txt");
        assert_eq!(folder_name("Com2"), "Com2_");
        assert_eq!(folder_name("COM¹"), "COM¹_");
        assert_eq!(folder_name("CONSOLE"), "CONSOLE");
        assert_eq!(folder_name("COM10"), "COM10");
        assert_eq!(folder_name("Tudor..."), "Tudor");
        assert_eq!(folder_name("a/b\\c\u{7}"), "a-b-c-");
        assert_eq!(folder_name(""), "Watch");
        assert_eq!(folder_name(&"x".repeat(300)).len(), 100);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_library_is_made_only_in_an_empty_folder_and_read_by_its_version() {
        let root = scratch("create");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("something.txt"), "x").unwrap();
        assert!(Library::create(&root).is_err());
        assert!(Library::open(&root)
            .unwrap_err()
            .contains("not a Timegrapher library"));
        assert!(Library::open_or_create(&root).is_err());
        fs::remove_file(root.join("something.txt")).unwrap();
        Library::open_or_create(&root).unwrap();
        fs::write(root.join(LIBRARY_FILE), "format = 99\n").unwrap();
        assert!(Library::open(&root).unwrap_err().contains("newer version"));
        // A move never lands on files already there.
        let other = scratch("create-other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("keep.txt"), "x").unwrap();
        fs::write(root.join(LIBRARY_FILE), "format = 1\n").unwrap();
        assert!(move_library(&root, &other).is_err());
        assert!(move_library(&root, &root.join("inside")).is_err());
        // Nor inside itself by way of a link to it.
        #[cfg(unix)]
        {
            let link = scratch("create-link");
            std::os::unix::fs::symlink(&root, &link).unwrap();
            assert!(move_library(&root, &link.join("inside")).is_err());
            let _ = fs::remove_file(&link);
        }
        assert!(Library::is_library(&root) && other.join("keep.txt").is_file());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&other);
    }

    /// Onto another disk the library is copied, checked, and only then
    /// removed, its library.toml first.
    #[test]
    fn a_move_onto_another_disk_copies_then_removes() {
        let root = scratch("across");
        let mut lib = Library::create(&root).unwrap();
        let dir = lib.add(&Watch::new("Pepsi")).unwrap();
        take(
            &dir.join(TAKES).join("2026-10-20 10.00 Dial Up"),
            &[("take_part01-001.flac", "DU")],
            1,
            "2026-10-20T17:00:00Z",
        );
        lib.scan();
        let to = scratch("across-to");
        assert_eq!(copy_across(&root, &to), Ok(None));
        assert!(!root.exists());
        let moved = Library::open(&to).unwrap();
        assert_eq!(moved.watches[0].takes, lib.watches[0].takes);
        let _ = fs::remove_dir_all(&to);
    }

    #[test]
    fn a_take_is_named_by_its_start_and_its_one_position() {
        assert_eq!(
            take_name("2026-10-20 14.32", &[Position::CH]),
            "2026-10-20 14.32 Dial Up"
        );
        assert_eq!(
            take_name("2026-10-20 18.05", &[Position::H6, Position::H6]),
            "2026-10-20 18.05 Crown Left"
        );
        assert_eq!(
            take_name("2026-10-21 09.10", &[Position::CH, Position::CB]),
            "2026-10-21 09.10"
        );
        assert_eq!(take_name("2026-10-21 09.10", &[]), "2026-10-21 09.10");
    }

    #[test]
    fn watch_toml_reads_as_a_person_wrote_it() {
        let w: Watch = toml::from_str(
            r#"
id = "0123456789abcdef"
name = "Pepsi"
calibre = "Rolex 3285"
bph = 28800
archived = true

[[field]]
label = "Purchase Price"
kind = "money"
value = "£9,000"
"#,
        )
        .unwrap();
        assert!(w.archived && w.lift.is_none());
        assert_eq!(w.fields[0].kind, FieldKind::Money);
        let back: Watch = toml::from_str(&toml::to_string_pretty(&w).unwrap()).unwrap();
        assert_eq!(back, w);
        assert_ne!(Watch::new("a").id, Watch::new("a").id);
    }
}
