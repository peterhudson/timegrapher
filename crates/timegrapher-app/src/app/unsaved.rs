//! What a session left unsaved: its sound, or for a Power Reserve Test
//! kept as its analysis alone, its analysis kept in parts as it went
//! (2c of #1514), offered back at the next start.

use super::*;

/// A session that was never saved or thrown away: the app quit without
/// asking (on macOS, Cmd+Q and Quit in the app menu or the Dock), or was
/// stopped (a crash, a logout, a shutdown).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct KeptSound {
    pub(super) dir: PathBuf,
    /// "2026-10-21 14.32", from its session.json, in this computer's time.
    pub(super) started: Option<String>,
    pub(super) duration_s: Option<f64>,
    pub(super) bytes: u64,
    /// The take it was recording into as it went, which holds all of it
    /// but its last few seconds.
    pub(super) streamed_to: Option<PathBuf>,
    /// A Power Reserve Test kept as its analysis alone: no sound, its
    /// analysis kept in parts up to `duration_s` ([`checkpoint::only`]).
    pub(super) parts: bool,
}

impl KeptSound {
    /// What it is, as the notice and its dialogs name it: "sound", or a
    /// test's "analysis".
    pub(super) fn what(&self) -> &'static str {
        if self.parts {
            "analysis"
        } else {
            "sound"
        }
    }

    /// What the start notice says of it: when it started, how much it
    /// holds, and what can be done with it.
    pub(super) fn notice(&self) -> String {
        let started = self
            .started
            .as_ref()
            .map_or(String::new(), |s| format!(": started {s}"));
        let size = size_text(self.bytes);
        if self.parts {
            // A test kept as its analysis alone: how much it recorded,
            // never its run (2c of #1514, #1806).
            let recorded = self
                .duration_s
                .map_or(String::new(), |d| format!(", {}", recorded_text(d)));
            format!(
                "An unsaved Power Reserve Test was found (Analysis Only){started}{recorded}, \
                 {size}. Open it, move it to a folder of your own, or delete it?"
            )
        } else {
            let long = self
                .duration_s
                .map_or(String::new(), |d| format!(", {} long", length_text(d)));
            format!(
                "Sound from an earlier session wasn't saved{started}{long}, {size}. Open it, \
                 move it into a folder of your own, or delete it?"
            )
        }
    }

    /// [`Self::what`] in a title's capitals.
    pub(super) fn what_title(&self) -> &'static str {
        if self.parts {
            "Analysis"
        } else {
            "Sound"
        }
    }
}

/// What was left in `scratch` from earlier sessions, oldest first: sound,
/// or a test's analysis kept in parts, leaving out any folder another
/// copy of the app is recording into now.
pub(super) fn kept_sounds(scratch: &Path) -> Vec<KeptSound> {
    let mut v: Vec<KeptSound> = std::fs::read_dir(scratch)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|d| !timegrapher_core::recorder::in_use(d))
        .filter_map(|dir| {
            let files: Vec<std::fs::DirEntry> = std::fs::read_dir(&dir).ok()?.flatten().collect();
            let has_sound = files.iter().any(|f| {
                let n = f.file_name().to_string_lossy().to_lowercase();
                n.ends_with(".wav") || n.ends_with(".flac")
            });
            let parts = !has_sound && checkpoint::only(&dir);
            if !has_sound && !parts {
                return None;
            }
            let about: Option<serde_json::Value> = std::fs::read(dir.join("session.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            let started = started_of(about.as_ref());
            // Of a test kept in parts, as far as its parts reach.
            let duration_s = match parts {
                true => checkpoint::length_s(&dir).ok(),
                false => about.as_ref().and_then(|a| a["duration_s"].as_f64()),
            };
            let streamed_to = take_io::streamed_to(&dir);
            Some(KeptSound {
                started,
                duration_s,
                bytes: tree_bytes(&dir),
                streamed_to,
                parts,
                dir,
            })
        })
        .collect();
    v.sort_by(|a, b| a.dir.cmp(&b.dir));
    v
}

/// When a recording started, from its `session.json` (`about`), in this
/// computer's time, as takes are named: "2026-10-21 14.32"; as written,
/// marked UTC, when it can't be read.
pub(super) fn started_of(about: Option<&serde_json::Value>) -> Option<String> {
    let u = about?["started_utc"].as_str()?;
    Some(watches::local_minute(u).unwrap_or_else(|| {
        let at = u.get(..16).unwrap_or(u).replacen('T', " ", 1);
        format!("{} UTC", at.replace(':', "."))
    }))
}

/// How much a test kept in parts recorded, as the start notice and Open
/// say it (Hugh and Norm on #1806): the last part's end, never its run,
/// which the Power Reserve pane gives.
pub(super) fn recorded_text(s: f64) -> String {
    format!("recorded {}", power_reserve::run_text(s))
}

/// The bytes the files under `dir` take, its folders' included (a test's
/// parts are kept in one, Felix).
fn tree_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => tree_bytes(&e.path()),
            _ => e.metadata().map_or(0, |m| m.len()),
        })
        .sum()
}

/// Move the recording folder `dir` into `parent`, under its own name:
/// renamed, or onto another disk copied, its folders with it, each file
/// written to the disk, and only then removed. Returns where it went.
pub(super) fn move_kept(dir: &Path, parent: &Path) -> Result<PathBuf, String> {
    let name = dir.file_name().ok_or("The kept sound has no folder name")?;
    let to = library::unused_dir(parent, &name.to_string_lossy());
    timegrapher_core::recorder::remove_stale_lock(dir);
    if std::fs::rename(dir, &to).is_ok() {
        return Ok(to);
    }
    match copy_tree(dir, &to) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(dir);
            Ok(to)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&to);
            Err(format!("Couldn't move it to {}: {e}", parent.display()))
        }
    }
}

/// Copy the folder `from` to `to`, each file and folder written to the
/// disk (Felix).
pub(super) fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            copy_tree(&e.path(), &to.join(e.file_name()))?;
            continue;
        }
        let mut r = std::fs::File::open(e.path())?;
        let mut w = std::fs::File::create(to.join(e.file_name()))?;
        std::io::copy(&mut r, &mut w)?;
        w.sync_all()?;
    }
    timegrapher_core::recorder::sync_folder(to)
}
