//! The take a path belongs to (#1573): the session file in a take's
//! folder, or beside a sound file in one, and its entries for that path.
//! One rule for the app, the command line and MCP, so a sound file inside
//! a take is read with the take's lift, calibre and watch wherever it is
//! opened.

use std::path::{Path, PathBuf};

use crate::session::{Manifest, RecordingEntry};
use crate::take::MANIFEST;

/// The folder a path's take would be in: the path itself when it is a
/// folder, else the folder the file is in.
pub fn folder(path: &Path) -> Option<&Path> {
    path.is_dir().then_some(path).or_else(|| path.parent())
}

/// The session file in `path`'s take folder ([`folder`]), parsed and with
/// its entries for `path`: none when there is no session file or it
/// doesn't list `path`. An error when the session file can't be read, or
/// its analysis settings are not ones a reading can be made with.
pub fn take_of(path: &Path) -> Result<Option<(Manifest, Vec<RecordingEntry>)>, String> {
    let Some(dir) = folder(path) else {
        return Ok(None);
    };
    let file = dir.join(MANIFEST);
    if !file.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let m: Manifest =
        toml::from_str(&text).map_err(|e| format!("Can't read {}: {e}", file.display()))?;
    let entries: Vec<RecordingEntry> = m
        .recordings
        .iter()
        .filter(|e| lists(dir, &e.file, path))
        .cloned()
        .collect();
    if entries.is_empty() {
        return Ok(None);
    }
    m.validate_analysis_inputs()
        .map_err(|e| format!("Can't read {}: {e}", file.display()))?;
    Ok(Some((m, entries)))
}

/// The files the session file in `dir` lists, in its order: none when it
/// has none or can't be read.
pub fn listed_order(dir: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(dir.join(MANIFEST)).ok()?;
    let manifest: Manifest = toml::from_str(&text).ok()?;
    Some(manifest.recordings.into_iter().map(|e| e.file).collect())
}

/// The recordings to analyse for `path`: the file itself, or a folder's
/// WAV and FLAC files (the segments of one long recording) in the order
/// its session file lists them, then any it doesn't list in natural name
/// order ([`crate::recording_order::recordings_in`]).
pub fn recordings_in(path: &Path) -> Result<Vec<PathBuf>, String> {
    let listed = if path.is_dir() {
        listed_order(path).unwrap_or_default()
    } else {
        Vec::new()
    };
    crate::recording_order::recordings_in(path, &listed)
}

/// Whether a session file in `dir` whose entry names `file` lists `path`:
/// the same file or folder, or the whole folder when `path` is the only
/// recording in it, as a take saved for a lone file lists it.
pub fn lists(dir: &Path, file: &str, path: &Path) -> bool {
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let named = dir.join(file);
    same(&named, path)
        || (path.is_dir()
            && recordings_in(path).is_ok_and(|files| files.iter().any(|p| same(p, &named))))
        || (!path.is_dir()
            && named.is_dir()
            && path.parent().is_some_and(|p| same(p, &named))
            && recordings_in(&named).is_ok_and(|f| f.len() == 1 && same(&f[0], path)))
}
