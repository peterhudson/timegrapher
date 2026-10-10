//! The app's own log and crash records, for a problem report.
//!
//! `timegrapher.log` in the settings folder (rotated at 1 MB, three files
//! kept) records devices opened and closed, sample rates, beat lock gained
//! and lost, input level changes, warnings and errors: never audio, and no
//! paths outside a recording. A panic writes `crash-<utc>.txt` beside it
//! with the message, where it happened, a backtrace, the version and
//! platform, and the last 200 log lines; the next launch offers it for a
//! report.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use timegrapher_core::recorder::utc;

/// Largest log file before it is rotated, bytes.
const MAX_BYTES: u64 = 1 << 20;
/// Log files kept: the current one and two older.
const KEEP: usize = 3;
/// Lines kept in memory for a crash record.
const RECENT: usize = 200;
const LOG: &str = "timegrapher.log";
const LOGS: &str = "logs";

/// Version, build and platform, for the log and a crash record.
pub fn version() -> String {
    format!(
        "Timegrapher {} on {} {}",
        env!("TIMEGRAPHER_LONG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

struct Log {
    dir: PathBuf,
    file: Option<File>,
    written: u64,
    recent: VecDeque<String>,
}

static LOGGER: Mutex<Option<Log>> = Mutex::new(None);

/// Start logging in `dir`'s `logs` folder (created if missing) and record
/// crashes in `dir`, where `timegrapher report` collects both.
/// Without a folder the log is kept in memory only.
pub fn init(dir: Option<PathBuf>) {
    start(dir);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let trace = std::backtrace::Backtrace::force_capture();
        crash(&message, &place, &trace.to_string());
        previous(info);
    }));
}

/// Start the log alone, without the panic hook.
fn start(dir: Option<PathBuf>) {
    let mut log = Log {
        dir: dir.clone().unwrap_or_default(),
        file: None,
        written: 0,
        recent: VecDeque::new(),
    };
    if let Some(d) = &dir {
        if fs::create_dir_all(d.join(LOGS)).is_ok() {
            open(&mut log);
        }
    }
    if let Ok(mut g) = LOGGER.lock() {
        *g = Some(log);
    }
    line("info", &version());
}

fn open(log: &mut Log) {
    let path = log.dir.join(LOGS).join(LOG);
    log.written = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    log.file = OpenOptions::new().create(true).append(true).open(path).ok();
}

/// `timegrapher.log` → `.log.1` → `.log.2`, the oldest dropped.
fn rotate(log: &mut Log) {
    log.file = None;
    let name = |i: usize| match i {
        0 => log.dir.join(LOGS).join(LOG),
        i => log.dir.join(LOGS).join(format!("{LOG}.{i}")),
    };
    for i in (0..KEEP - 1).rev() {
        let _ = fs::rename(name(i), name(i + 1));
    }
    open(log);
}

/// The folder the log and crash records are kept in, once logging has
/// started there.
pub fn dir() -> Option<PathBuf> {
    let g = LOGGER.lock().ok()?;
    g.as_ref()
        .filter(|l| l.file.is_some())
        .map(|l| l.dir.clone())
}

/// Add a line to the log: `level` is info, warn or error.
pub fn line(level: &str, text: &str) {
    let Ok(mut g) = LOGGER.lock() else { return };
    let Some(log) = g.as_mut() else { return };
    let l = format!("{} {level:<5} {text}", utc(SystemTime::now()));
    if log.recent.len() == RECENT {
        log.recent.pop_front();
    }
    log.recent.push_back(l.clone());
    if log.file.is_some() && log.written + l.len() as u64 + 1 > MAX_BYTES {
        rotate(log);
    }
    if let Some(f) = log.file.as_mut() {
        if writeln!(f, "{l}").is_ok() {
            log.written += l.len() as u64 + 1;
        }
    }
}

pub fn info(text: &str) {
    line("info", text);
}

pub fn warn(text: &str) {
    line("warn", text);
}

pub fn error(text: &str) {
    line("error", text);
}

/// Write a crash record. Called from the panic hook, so it never panics
/// itself and takes the lock only if it is free.
fn crash(message: &str, place: &str, trace: &str) {
    let Ok(g) = LOGGER.try_lock() else { return };
    let Some(log) = g.as_ref() else { return };
    if log.file.is_none() {
        return;
    }
    let now = utc(SystemTime::now());
    let mut text = format!(
        "{}\nCrashed at {now}\n\n{message}\nat {place}\n\nBacktrace:\n{trace}\n\nThe last log lines:\n",
        version()
    );
    for l in &log.recent {
        text.push_str(l);
        text.push('\n');
    }
    let stamp: String = now
        .chars()
        .map(|c| if c == ':' { '-' } else { c })
        .collect();
    let _ = fs::write(log.dir.join(format!("crash-{stamp}.txt")), text);
}

/// The crash records in `dir` not yet looked at, oldest first.
pub fn unseen_crashes(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with("crash-") && n.ends_with(".txt") && !n.ends_with(".seen.txt")
            })
        })
        .collect();
    v.sort();
    v
}

/// Mark a crash record as looked at, so it isn't offered again.
pub fn mark_seen(path: &Path) {
    let seen = path.with_extension("seen.txt");
    let _ = fs::rename(path, seen);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_rotates_and_keeps_three_files() {
        let dir = std::env::temp_dir().join(format!("tg-applog-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(LOGS)).unwrap();
        let mut log = Log {
            dir: dir.clone(),
            file: None,
            written: 0,
            recent: VecDeque::new(),
        };
        open(&mut log);
        for _ in 0..4 {
            writeln!(log.file.as_mut().unwrap(), "x").unwrap();
            rotate(&mut log);
        }
        let mut names: Vec<String> = fs::read_dir(dir.join(LOGS))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, [LOG, "timegrapher.log.1", "timegrapher.log.2"]);

        // A crash record is offered until it is looked at.
        let c = dir.join("crash-2026-10-10T21-00-00.000Z.txt");
        fs::write(&c, "boom").unwrap();
        assert_eq!(unseen_crashes(&dir), std::slice::from_ref(&c));
        mark_seen(&c);
        assert!(unseen_crashes(&dir).is_empty());

        // The logger writes lines, and a crash record carries the message
        // and the lines before it.
        start(Some(dir.clone()));
        info("microphone opened: Test, 48000 Hz, 16-bit");
        crash("index out of bounds", "src/app.rs:1", "frames");
        let records = unseen_crashes(&dir);
        assert_eq!(records.len(), 1);
        let text = fs::read_to_string(&records[0]).unwrap();
        assert!(
            text.contains("index out of bounds") && text.contains("48000 Hz"),
            "{text}"
        );
        assert!(text.starts_with("Timegrapher "), "{text}");
        assert!(fs::read_to_string(dir.join(LOGS).join(LOG))
            .unwrap()
            .contains("microphone opened"));
        let _ = fs::remove_dir_all(&dir);
    }
}
