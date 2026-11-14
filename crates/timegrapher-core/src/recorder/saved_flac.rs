//! Hours of saved recordings kept as FLAC after the fact (#503): the
//! takes still holding a WAV, and the conversion of each.

use super::{in_use, is_sound};
use crate::take;
use std::fs;
use std::path::{Path, PathBuf};

/// The recordings in `take` whose `session.json` lists an hour still
/// kept as a WAV, or kept as a [`crate::flac::bloated`] FLAC: `take`
/// itself, and the takes a session folder holds (`timegrapher record`
/// makes one per position). These were saved before hours were kept as
/// FLAC (#503), stopped before one was converted, or converted before
/// each block's size was checked. A recording being made ([`in_use`]) is
/// never listed.
pub fn holding_wav(take: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![take.to_path_buf()];
    if let Ok(entries) = fs::read_dir(take) {
        let mut inner: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .collect();
        inner.sort();
        dirs.extend(inner);
    }
    dirs.retain(|d| !hours_to_keep(d).is_empty());
    dirs
}

/// The hours `session.json` in `dir` lists that are WAVs there now, or
/// [`crate::flac::bloated`] FLACs. None while it is being recorded
/// ([`in_use`]): its open hour is a WAV the recorder is still writing, and
/// its closed ones the recorder converts.
fn hours_to_keep(dir: &Path) -> Vec<String> {
    if in_use(dir) {
        return Vec::new();
    }
    let Ok(text) = fs::read_to_string(dir.join(take::RECORD)) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    v["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str())
        .filter(|n| is_sound(n) && Path::new(n).components().count() == 1)
        .filter(|n| {
            let p = dir.join(n);
            p.is_file() && (crate::flac::is_wav(&p) || crate::flac::bloated(&p))
        })
        .map(str::to_string)
        .collect()
}

/// Keep each hour of the saved recording in `dir` that is still a WAV as
/// FLAC, one at a time ([`crate::flac::keep_as_flac`]: encoded, checked
/// sample for sample, then the WAV deleted), and name the FLAC in
/// `session.json` (its `segments` and breaks) after each. An hour kept as
/// a [`crate::flac::bloated`] FLAC is written again
/// ([`crate::flac::shrink`]), under its own name. An hour that cannot be
/// converted stays as it was, whole, and the error says which. A
/// recording being made ([`in_use`]) is left alone. Returns how many hours
/// were converted.
pub fn keep_saved_as_flac(dir: &Path) -> Result<usize, String> {
    keep_saved_as_flac_unless(dir, &|| false)
}

/// [`keep_saved_as_flac`], asking `stop` before each hour: when it says
/// so, the hours not yet converted are left as WAVs, for another time (a
/// take opened in the app while its hours are kept, #503).
pub fn keep_saved_as_flac_unless(dir: &Path, stop: &dyn Fn() -> bool) -> Result<usize, String> {
    let record = dir.join(take::RECORD);
    let mut done = 0;
    for wav in hours_to_keep(dir) {
        if stop() {
            break;
        }
        let path = dir.join(&wav);
        if !crate::flac::is_wav(&path) {
            crate::flac::shrink(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            done += 1;
            continue;
        }
        let flac =
            crate::flac::keep_as_flac(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let flac = flac
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        // Only the names change: the file is otherwise as it was written.
        let text = fs::read_to_string(&record).map_err(|e| format!("{}: {e}", record.display()))?;
        let quoted = |n: &str| serde_json::Value::from(n).to_string();
        let text = text.replace(&quoted(&wav), &quoted(&flac));
        crate::library::write_atomic(&record, text.as_bytes())?;
        done += 1;
    }
    Ok(done)
}
