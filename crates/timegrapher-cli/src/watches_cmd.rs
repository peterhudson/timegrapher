//! `timegrapher watches`: the watches in the library the app keeps, each
//! with its details and its takes, and the takes Quick Listen left in
//! Unsorted (see `timegrapher_core::library`). It only reads: it takes no
//! lock and writes nothing, so the app can have the library open.

use crate::output;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use timegrapher_core::library::{self, Library, Take, WatchEntry, UNSORTED};
use timegrapher_core::session::Position;

/// The library at `library`, or at `~/Timegrapher Data`.
fn root(library: Option<&Path>) -> Result<PathBuf, String> {
    library
        .map(Path::to_path_buf)
        .or_else(library::default_location)
        .ok_or_else(|| "there is no home folder: give the library with --library".into())
}

pub fn run(
    name: Option<&str>,
    library: Option<&Path>,
    archived: bool,
    json: bool,
) -> Result<(), String> {
    let root = root(library)?;
    if !Library::is_library(&root) {
        return Err(format!(
            "{} is not a Timegrapher library (it has no {}); the app makes one with its \
             first watch, and --library names one elsewhere",
            root.display(),
            library::LIBRARY_FILE
        ));
    }
    let lib = Library::read(&root)?;
    let unsorted_named = name.is_some_and(|n| n.trim().eq_ignore_ascii_case(UNSORTED));
    let watches: Vec<&WatchEntry> = match name {
        Some(_) if unsorted_named => Vec::new(),
        Some(n) => vec![lib
            .find(n)
            .ok_or_else(|| format!("there is no watch called \"{}\" in the library", n.trim()))?],
        None => lib
            .watches
            .iter()
            .filter(|e| archived || !e.watch.archived)
            .collect(),
    };
    let unsorted = (name.is_none() || unsorted_named).then_some(&lib.unsorted);
    if json {
        let input = json!({
            "library": root.display().to_string(),
            "name": name,
            "archived": archived,
        });
        let body = json!({
            "bytes": lib.bytes(),
            "watches": watches.iter().map(|e| watch_json(e)).collect::<Vec<_>>(),
            "unsorted": unsorted.map(|u| json!({
                "folder": u.dir.display().to_string(),
                "bytes": u.bytes(),
                "takes": takes_json(u),
            })),
            "unreadable": lib.unreadable,
        });
        return output::print("watches", input, &body);
    }
    match (name, unsorted) {
        (Some(_), Some(u)) => print_takes(u),
        (Some(_), None) => watches.iter().for_each(|e| print_watch(e)),
        (None, _) => {
            println!(
                "{}: {}, {}",
                root.display(),
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
                    parts.push(format!("last started {t}"));
                }
                parts.push(bytes(e.bytes()));
                if e.watch.archived {
                    parts.push("Archived".into());
                }
                println!("  {}: {}", e.watch.name, parts.join(", "));
            }
        }
    }
    for why in &lib.unreadable {
        eprintln!("couldn't read: {why}");
    }
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

fn watch_json(e: &WatchEntry) -> Value {
    let w = &e.watch;
    json!({
        "name": w.name,
        "id": w.id,
        "folder": e.dir.display().to_string(),
        "calibre": w.calibre,
        "lift_deg": w.lift,
        "bph": w.bph,
        "notes": w.notes,
        "archived": w.archived,
        "fields": w.fields,
        "photos": e.photos.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "bytes": e.bytes(),
        "last_take_utc": last_take(e),
        "takes": takes_json(e),
    })
}

fn takes_json(e: &WatchEntry) -> Vec<Value> {
    e.takes
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "folder": e.take_dir(t).display().to_string(),
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
    if w.archived {
        println!("  Archived");
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
        if let Some(r) = a.rate_s_per_day {
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
