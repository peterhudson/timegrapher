//! `timegrapher watches`: the watches in the library the app keeps, each
//! with its details and its takes, and the takes Quick Listen left in
//! Unsorted (see `timegrapher_core::library`). It only reads: it takes no
//! lock and writes nothing, so the app can have the library open.

use crate::output;
use serde_json::{json, Value};
use std::path::Path;
use timegrapher_core::library::{self, Library, Take, WatchEntry, UNSORTED};
use timegrapher_core::session::Position;

/// The library at `library`, or at `~/Timegrapher Data`, read: an error
/// naming the folder when there is none there.
fn open(library: Option<&Path>) -> Result<Library, String> {
    let root = library
        .map(Path::to_path_buf)
        .or_else(library::default_location)
        .ok_or("there is no home folder: give the library with --library")?;
    // Absolute, so every folder in the document can be passed on as it is.
    let root = std::path::absolute(&root).unwrap_or(root);
    if !Library::is_library(&root) {
        return Err(format!(
            "{} is not a Timegrapher library (it has no {}); the app makes one with its \
             first watch, and --library names one elsewhere",
            root.display(),
            library::LIBRARY_FILE
        ));
    }
    Library::read(&root)
}

fn warn_unreadable(lib: &Library) {
    for why in &lib.unreadable {
        eprintln!("couldn't read: {why}");
    }
}

/// `watches list`: one line or entry per watch, and how many takes wait in
/// Unsorted. A watch is kept or deleted: one an older app marked archived
/// is listed like any other, and `all` is accepted and changes nothing.
pub fn list(library: Option<&Path>, all: bool, json: bool) -> Result<(), String> {
    let lib = open(library)?;
    let watches: Vec<&WatchEntry> = lib.watches.iter().collect();
    if json {
        let input = json!({ "library": lib.root.display().to_string(), "all": all });
        let body = json!({
            "library": lib.root.display().to_string(),
            "bytes": lib.bytes(),
            "watches": watches.iter().map(|e| {
                let w = &e.watch;
                json!({
                    "name": w.name,
                    "calibre": w.calibre,
                    "bph": w.bph,
                    "lift_deg": w.lift,
                    "archived": w.archived,
                    "takes": e.takes.len(),
                    "bytes": e.bytes(),
                    "newest_started_utc": last_take(e),
                })
            }).collect::<Vec<_>>(),
            "unsorted": lib.unsorted.takes.len(),
            "unreadable": lib.unreadable,
            "caveats": [],
        });
        return output::print("watches", input, &body);
    }
    println!(
        "{}: {}, {}",
        lib.root.display(),
        count(watches.len(), "watch", "watches"),
        bytes(lib.bytes())
    );
    if !lib.unsorted.takes.is_empty() {
        println!(
            "  {UNSORTED}: {}, made with Quick Listen, {}",
            count(lib.unsorted.takes.len(), "take", "takes"),
            bytes(lib.unsorted.bytes())
        );
    }
    for e in &watches {
        let mut parts = Vec::new();
        if let Some(c) = e.watch.calibre.as_deref().filter(|c| !c.is_empty()) {
            parts.push(c.to_string());
        }
        parts.push(count(e.takes.len(), "take", "takes"));
        if let Some(t) = last_take(e) {
            parts.push(format!("newest started {t}"));
        }
        parts.push(bytes(e.bytes()));
        println!("  {}: {}", e.watch.name, parts.join(", "));
    }
    warn_unreadable(&lib);
    Ok(())
}

/// `watches show NAME`: one watch (by its name, ignoring case and spaces
/// at either end), or Unsorted, with every take.
pub fn show(name: &str, library: Option<&Path>, json: bool) -> Result<(), String> {
    let lib = open(library)?;
    let e = if name.trim().eq_ignore_ascii_case(UNSORTED) {
        &lib.unsorted
    } else {
        lib.find(name).ok_or_else(|| {
            format!(
                "there is no watch called \"{}\" in the library",
                name.trim()
            )
        })?
    };
    if json {
        let w = &e.watch;
        let input = json!({ "library": lib.root.display().to_string(), "name": name });
        let body = json!({
            "library": lib.root.display().to_string(),
            "name": w.name,
            "unsorted": w.id.is_empty(),
            "folder": e.dir.display().to_string(),
            "calibre": w.calibre,
            "bph": w.bph,
            "lift_deg": w.lift,
            "notes": w.notes,
            "archived": w.archived,
            "fields": w.fields,
            "photos": e.photos.len(),
            "bytes": e.bytes(),
            "takes": takes_json(e),
            "unreadable": lib.unreadable,
            "caveats": [],
        });
        return output::print("watch", input, &body);
    }
    print_watch(e);
    warn_unreadable(&lib);
    Ok(())
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A size as the app's Watches pane shows it: `7.7 GB`, `58.3 MB`,
/// `912 kB`, `40 bytes`.
fn bytes(n: u64) -> String {
    let n = n as f64;
    match n {
        n if n >= 1e9 => format!("{:.1} GB", n / 1e9),
        n if n >= 1e6 => format!("{:.1} MB", n / 1e6),
        n if n >= 1e3 => format!("{:.0} kB", n / 1e3),
        n => format!("{n:.0} bytes"),
    }
}

/// When the watch's newest take started, UTC.
fn last_take(e: &WatchEntry) -> Option<&str> {
    e.takes
        .iter()
        .filter_map(|t| t.started_utc.as_deref())
        .max()
}

fn takes_json(e: &WatchEntry) -> Vec<Value> {
    e.takes
        .iter()
        .map(|t| {
            json!({
                "path": e.take_dir(t).display().to_string(),
                "name": t.name,
                "started_utc": t.started_utc,
                "duration_s": t.duration_s,
                "positions": t.positions,
                "audio": t.audio,
                "incomplete": t.incomplete,
                "bytes": t.bytes,
                "at_end": t.at_end,
            })
        })
        .collect()
}

fn print_watch(e: &WatchEntry) {
    let w = &e.watch;
    println!("{}", w.name);
    println!("  Folder: {}", e.dir.display());
    if let Some(c) = &w.calibre {
        println!("  Calibre: {c}");
    }
    if let Some(l) = w.lift {
        println!("  Lift Angle: {l}°");
    }
    if let Some(b) = w.bph {
        println!("  Beat Rate: {b} bph");
    }
    for f in &w.fields {
        println!("  {}: {}", f.label, f.value);
    }
    if !e.photos.is_empty() {
        println!("  Photos: {}", e.photos.len());
    }
    if let Some(n) = w.notes.as_deref().filter(|n| !n.trim().is_empty()) {
        println!("  Notes: {n}");
    }
    print_takes(e);
}

/// The takes, newest first, each on a line.
fn print_takes(e: &WatchEntry) {
    println!(
        "  {}, {}:",
        count(e.takes.len(), "take", "takes"),
        bytes(e.bytes())
    );
    for t in &e.takes {
        println!("    {}: {}", t.name, take_line(t));
    }
}

/// A take's positions, length, the readings the app showed when it was
/// saved (beat error as its size), its size and marks.
fn take_line(t: &Take) -> String {
    let mut parts: Vec<String> = Vec::new();
    let names: Vec<String> = t
        .positions
        .iter()
        .map(|p| Position::parse(p).map_or_else(|| p.clone(), |x| x.name().to_string()))
        .collect();
    if !names.is_empty() {
        parts.push(names.join(", "));
    }
    if let Some(d) = t.duration_s {
        parts.push(format!("{:.0} s", d));
    }
    if let Some(a) = &t.at_end {
        if let Some(r) = a.rate() {
            parts.push(format!("{r:+.1} s/d"));
        }
        if let Some(x) = a.amplitude_deg {
            parts.push(format!("{x:.0}°"));
        }
        if let Some(b) = a.beat_error_ms {
            parts.push(format!("{:.1} ms", b.abs()));
        }
    }
    parts.push(bytes(t.bytes));
    if !t.audio {
        parts.push("analysis only".into());
    }
    if t.incomplete {
        parts.push("incomplete".into());
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_the_app_shows_them() {
        assert_eq!(bytes(40), "40 bytes");
        assert_eq!(bytes(912_400), "912 kB");
        assert_eq!(bytes(58_300_000), "58.3 MB");
        assert_eq!(bytes(7_700_000_000), "7.7 GB");
    }
}
