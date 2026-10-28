//! `timegrapher watches`: the watches in the library the app keeps, each
//! with its details and its takes, and the takes Quick Listen left in
//! Unsorted (see `timegrapher_core::library`). It takes no lock and writes
//! nothing in the library, so the app can have it open; each take's own
//! reading is worked out once and kept in the user's settings folder
//! ([`library::reading`]).

use crate::output;
use serde_json::{json, Value};
use std::path::Path;
use timegrapher_core::library::{self, Library, Reading, Take, WatchEntry, UNSORTED};

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
        library::size_text(lib.bytes())
    );
    if !lib.unsorted.takes.is_empty() {
        println!(
            "  {UNSORTED}: {}, made with Quick Listen, {}",
            count(lib.unsorted.takes.len(), "take", "takes"),
            library::size_text(lib.unsorted.bytes())
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
        parts.push(library::size_text(e.bytes()));
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
    let readings = readings(e);
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
            "takes": takes_json(e, &readings),
            "unreadable": lib.unreadable,
            "caveats": [],
        });
        return output::print("watch", input, &body);
    }
    print_watch(e, &readings);
    warn_unreadable(&lib);
    Ok(())
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// When the watch's newest take started, UTC.
fn last_take(e: &WatchEntry) -> Option<&str> {
    e.takes
        .iter()
        .filter_map(|t| t.started_utc.as_deref())
        .max()
}

/// Each take's own reading ([`library::reading`]), in the order of the
/// takes: none for an incomplete take or one that can't be read, which is
/// said on stderr. A take not read before is read now, which takes about a
/// minute for a 9 h take, and said first.
fn readings(e: &WatchEntry) -> Vec<Option<Reading>> {
    e.takes
        .iter()
        .map(|t| {
            if t.incomplete {
                return None;
            }
            let dir = e.take_dir(t);
            if library::kept_reading(&dir).is_none() {
                eprintln!("reading {} (once)", t.name);
            }
            library::reading(&dir)
                .map_err(|why| eprintln!("couldn't read {}: {why}", t.name))
                .ok()
        })
        .collect()
}

fn takes_json(e: &WatchEntry, readings: &[Option<Reading>]) -> Vec<Value> {
    e.takes
        .iter()
        .zip(readings)
        .map(|(t, reading)| {
            json!({
                "path": e.take_dir(t).display().to_string(),
                "name": t.name,
                "started_utc": t.started_utc,
                "duration_s": t.duration_s,
                "positions": t.positions,
                "audio": t.audio,
                "incomplete": t.incomplete,
                "bytes": t.bytes,
                "reading": reading,
                "at_end": t.at_end,
            })
        })
        .collect()
}

fn print_watch(e: &WatchEntry, readings: &[Option<Reading>]) {
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
    print_takes(e, readings);
}

/// The takes, newest first, each on a line.
fn print_takes(e: &WatchEntry, readings: &[Option<Reading>]) {
    println!(
        "  {}, {}:",
        count(e.takes.len(), "take", "takes"),
        library::size_text(e.bytes())
    );
    for (t, reading) in e.takes.iter().zip(readings) {
        let positions_test = library::is_positions_test(&e.take_dir(t));
        println!(
            "    {}: {}",
            t.name,
            take_line(t, positions_test, reading.as_ref())
        );
    }
}

/// A take's facts as the app lists them, with its own reading's figures,
/// then its size on disk.
fn take_line(t: &Take, positions_test: bool, reading: Option<&Reading>) -> String {
    let mut parts = t.facts_read(positions_test, reading);
    parts.push(library::size_text(t.bytes));
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CLI lists a take with the app's facts: a Power Reserve Test's
    /// take names its run, and the length reads as the app writes it.
    #[test]
    fn a_take_line_reads_as_the_app_lists_the_take() {
        let t = Take {
            name: "t".into(),
            bytes: 58_300_000,
            audio: false,
            incomplete: false,
            started_utc: None,
            duration_s: Some(5400.0),
            positions: vec!["DU".into()],
            at_end: None,
            power_reserve: Some(timegrapher_core::power_reserve::Summary {
                run_s: 5400.0,
                ending: timegrapher_core::power_reserve::Ending::Stopped,
                position: "DU".into(),
            }),
        };
        assert_eq!(
            take_line(&t, false, None),
            "Power Reserve 1 h 30 min · Dial Up · Analysis Only · 58.3 MB"
        );
        // A run the watch didn't end says how it ended, in core's words.
        let limit = Take {
            power_reserve: Some(timegrapher_core::power_reserve::Summary {
                run_s: 5400.0,
                ending: timegrapher_core::power_reserve::Ending::Limit,
                position: "DU".into(),
            }),
            ..t.clone()
        };
        assert_eq!(
            take_line(&limit, false, None),
            "Power Reserve 1 h 30 min · Reached the 80 h limit while the watch still ran \
             · Dial Up · Analysis Only · 58.3 MB"
        );
        let t = Take {
            power_reserve: None,
            ..t
        };
        assert_eq!(
            take_line(&t, false, None),
            "Dial Up · 1:30:00 · Analysis Only · 58.3 MB"
        );
        // A Positions Test's take names its kind first, as the app does.
        assert_eq!(
            take_line(&t, true, None),
            "Positions Test · Dial Up · 1:30:00 · Analysis Only · 58.3 MB"
        );
    }
}
