//! A take kept as its analysis alone, kept on the disk as it is recorded
//! (#1514): every [`EVERY_S`] of sound, the beats, amplitude windows and
//! Tick Tock profiles found since are written as a part of their own in
//! `analysis.partial/` in the take's folder, and `partial.json` is
//! rewritten to list it. Should the recording end without its save (a
//! crash, a power cut), the take can still be opened up to its last part
//! ([`read`]).
//!
//! A part is never rewritten: it holds only what the reading can no longer
//! change ([`Writer::after_chunk`]), but for the take's sides being named
//! the other way round, which `partial.json` records
//! ([`Partial::renamed_before`]) and [`read`] applies. Each part's files
//! are written beside their names, put on the disk and moved into place,
//! and the folder put on the disk, before `partial.json` lists them;
//! `partial.json` lists each file's length and SHA-256, and [`read`]
//! reads nothing it doesn't list. The take's own save is written from the
//! reading, not from the parts ([`crate::keeper::Keeper::finish`]); the
//! parts are removed once it is in place ([`remove`]).

use crate::amplitude::{self, AmplitudeWindow};
use crate::analysis_take::{self, Header, Stored, StretchProfiles};
use crate::archive::sha256_hex;
use crate::beats::Beat;
use crate::stream::{self, Seen, StreamConfig};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The folder the parts are kept in, inside the take's folder.
pub const DIR: &str = "analysis.partial";
/// The list of parts.
pub const HEADER: &str = "partial.json";
/// [`Partial::format`].
pub const FORMAT: &str = "timegrapher.analysis.partial/1";
/// Sound, seconds, each part holds: about 250 kB a part.
pub const EVERY_S: f64 = 600.0;
/// The most `partial.json` may hold: a 50-hour take lists 300 parts in
/// about 150 kB.
const MAX_HEADER: u64 = 16 << 20;

/// What `partial.json` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Partial {
    /// [`FORMAT`].
    pub format: String,
    /// The analysis as far as the parts reach: [`Header::duration_s`] is
    /// the last part's end, and the rows are the parts' together. No USB
    /// line clock, signal median or beat shape: those are measured when
    /// the take is saved. [`Header::clipped_samples`] counts up to the
    /// chunk read last, which may be past the last part's end.
    pub header: Header,
    pub parts: Vec<Part>,
    /// The parts, from the first, written before the take's sides were
    /// named the other way round ([`crate::beats::SideCheck`]): every beat
    /// in them is numbered one further, and each window's and profile's
    /// sides swapped, when they are read.
    pub renamed_before: Option<usize>,
}

/// One part: the sound it covers, seconds, and its files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    pub from_s: f64,
    pub to_s: f64,
    pub beats: PartFile,
    pub windows: PartFile,
    pub profiles: PartFile,
    /// The windows' sides the amplitude gate left out (`rows`: the
    /// windows with one).
    pub culled: PartFile,
}

/// One file of a part, as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartFile {
    pub rows: usize,
    pub bytes: u64,
    pub sha256: String,
}

/// The name of part `i`'s file of `kind`: its number from 1, then the
/// file `analysis/` keeps the rows in.
fn part_name(i: usize, kind: &str) -> String {
    format!("{:04}-{kind}", i + 1)
}

/// How far the parts reach, as the recording goes on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Checkpointed {
    /// The last part's end, seconds of sound; 0 before the first.
    pub to_s: f64,
    /// Why the last part or list couldn't be written; none once one is.
    /// The recording and its reading go on, and the part is tried again
    /// after the next chunk.
    pub failed: Option<String>,
}

/// Writes the parts of a take being kept ([`crate::keeper::KeepAs`]).
pub(crate) struct Writer {
    dir: PathBuf,
    take: PathBuf,
    every_s: f64,
    partial: Partial,
    /// Beats, profiles and side swaps of the reading written so far.
    beats: usize,
    profiles: usize,
    swaps: usize,
    /// `partial.json` is behind what the parts say.
    stale: bool,
    status: Arc<Mutex<Checkpointed>>,
}

/// What a [`Writer`] needs besides each chunk read.
pub(crate) struct Writing<'a> {
    pub take: &'a Path,
    pub every_s: f64,
    pub sample_rate: u32,
    pub bytes_per_frame: u32,
    pub cfg: &'a StreamConfig,
    pub software: serde_json::Value,
    pub status: Arc<Mutex<Checkpointed>>,
}

impl Writer {
    pub(crate) fn new(w: Writing) -> Writer {
        let header = Header {
            format: analysis_take::FORMAT.into(),
            software: w.software,
            audio: false,
            note: analysis_take::NO_SOUND.into(),
            sample_rate: w.sample_rate,
            bytes_per_frame: w.bytes_per_frame,
            duration_s: 0.0,
            bph: 0,
            lift_deg: w.cfg.analysis.amplitude.lift_deg,
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            settings: analysis_take::settings(w.cfg),
            beats: 0,
            windows: 0,
            profiles: 0,
            usb_clock: None,
            signal_median: None,
            shape: Vec::new(),
        };
        Writer {
            dir: w.take.join(DIR),
            take: w.take.to_path_buf(),
            every_s: w.every_s,
            partial: Partial {
                format: FORMAT.into(),
                header,
                parts: Vec::new(),
                renamed_before: None,
            },
            beats: 0,
            profiles: 0,
            swaps: 0,
            stale: false,
            status: w.status,
        }
    }

    /// The chunk just read, `seen`, with the stretches profiled so far
    /// (`profiles`, every one ending by `profiled_to_s`): a part written
    /// for each [`Writer::every_s`] of sound that nothing read later can
    /// change. That is once the watch is heard (its beat rate is settled),
    /// before the chunk's own stretch (whose beats the next chunk may
    /// change), before a jump whose sides are still to be told apart (a
    /// swap there renumbers the beats after it), and where every
    /// amplitude window and profile is in.
    pub(crate) fn after_chunk(
        &mut self,
        seen: &Seen,
        profiles: &[StretchProfiles],
        profiled_to_s: f64,
    ) {
        for &(_, t) in &seen.swaps[self.swaps..] {
            // A swap at a jump renumbers only beats from the jump on, all
            // after the parts; naming the sides renumbers every beat.
            debug_assert!(t >= self.partial.header.duration_s || t == f64::NEG_INFINITY);
            if t == f64::NEG_INFINITY && !self.partial.parts.is_empty() {
                debug_assert!(self.partial.renamed_before.is_none());
                self.partial.renamed_before = Some(self.partial.parts.len());
                self.stale = true;
            }
        }
        self.swaps = seen.swaps.len();
        let safe_s = seen
            .core_a
            .min(seen.pending.unwrap_or(f64::INFINITY))
            .min(seen.windows_to_s)
            .min(profiled_to_s);
        let mut failed = None;
        while seen.heard && self.next_to_s() <= safe_s {
            if let Err(e) = self.write_part(seen, profiles) {
                failed = Some(e);
                break;
            }
        }
        if failed.is_none() && self.stale {
            failed = self.write_header().err();
        }
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        status.to_s = self.partial.header.duration_s;
        status.failed = failed;
    }

    /// The next part's end, seconds.
    fn next_to_s(&self) -> f64 {
        self.partial.header.duration_s + self.every_s
    }

    /// The next part, from `seen` and `profiles`, written and listed.
    fn write_part(&mut self, seen: &Seen, profiles: &[StretchProfiles]) -> Result<(), String> {
        let (from_s, to_s) = (self.partial.header.duration_s, self.next_to_s());
        let n_beats = seen.beats[self.beats..].partition_point(|b| b.time < to_s);
        let beats = &seen.beats[self.beats..self.beats + n_beats];
        let clipped: Vec<f64> = seen
            .clipped_beats
            .iter()
            .copied()
            .filter(|&t| from_s <= t && t < to_s)
            .collect();
        let mut windows: Vec<(usize, AmplitudeWindow)> = seen
            .windows
            .iter()
            .flat_map(|(k, ws)| ws.iter().map(move |w| (*k, w.clone())))
            .filter(|(_, w)| from_s <= w.start_s && w.start_s < to_s)
            .collect();
        // In chunk order, as the windows are put together.
        windows.sort_by_key(|w| w.0);
        let windows: Vec<AmplitudeWindow> = windows
            .into_iter()
            .map(|(k, mut w)| {
                stream::swap_found(&mut w, k, seen.swaps);
                w
            })
            .collect();
        let n_profiles = profiles[self.profiles..].partition_point(|p| p.end_s <= to_s);
        let profiles = &profiles[self.profiles..self.profiles + n_profiles];
        let i = self.partial.parts.len();
        let gz = |bytes: &[u8]| analysis_take::gzip(bytes).map_err(|e| e.to_string());
        let files = [
            (
                analysis_take::BEATS,
                beats.len(),
                gz(analysis_take::beats_rows(beats, &clipped, seen.bph).as_bytes())?,
            ),
            (
                analysis_take::WINDOWS,
                windows.len(),
                gz(analysis_take::windows_csv(&windows).as_bytes())?,
            ),
            (
                analysis_take::PROFILES,
                profiles.len(),
                gz(&analysis_take::profiles_bin(profiles))?,
            ),
            (
                analysis_take::CULLED,
                windows.iter().filter(|w| w.culled_ms.is_some()).count(),
                gz(analysis_take::culled_csv(&windows).as_bytes())?,
            ),
        ];
        self.make_dir()?;
        for (kind, _, bytes) in &files {
            crate::library::write_atomic(&self.dir.join(part_name(i, kind)), bytes)?;
        }
        sync_folder(&self.dir)?;
        let [b, w, p, c] = files.map(|(_, rows, bytes)| PartFile {
            rows,
            bytes: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        });
        let was = (self.partial.header.clone(), self.partial.parts.len());
        let h = &mut self.partial.header;
        h.duration_s = to_s;
        h.bph = seen.bph;
        h.clipped_samples = seen.clipped_samples;
        h.jumps = seen.jumps.iter().copied().filter(|&t| t < to_s).collect();
        h.sided = seen.sided.iter().copied().filter(|&t| t < to_s).collect();
        h.beats += b.rows;
        h.windows += w.rows;
        h.profiles += p.rows;
        self.partial.parts.push(Part {
            from_s,
            to_s,
            beats: b,
            windows: w,
            profiles: p,
            culled: c,
        });
        if let Err(e) = self.write_header() {
            // Not listed: written again, under the same names, next time.
            self.partial.header = was.0;
            self.partial.parts.truncate(was.1);
            return Err(e);
        }
        self.beats += n_beats;
        self.profiles += n_profiles;
        Ok(())
    }

    /// The parts' folder, made once.
    fn make_dir(&self) -> Result<(), String> {
        if !self.dir.is_dir() {
            std::fs::create_dir_all(&self.dir)
                .map_err(|e| format!("{}: {e}", self.dir.display()))?;
            sync_folder(&self.take)?;
        }
        Ok(())
    }

    fn write_header(&mut self) -> Result<(), String> {
        self.make_dir()?;
        let text = serde_json::to_string_pretty(&self.partial).map_err(|e| e.to_string())? + "\n";
        crate::library::write_atomic(&self.dir.join(HEADER), text.as_bytes())?;
        sync_folder(&self.dir)?;
        self.stale = false;
        Ok(())
    }
}

fn sync_folder(dir: &Path) -> Result<(), String> {
    crate::recorder::sync_folder(dir).map_err(|e| format!("{}: {e}", dir.display()))
}

fn invalid(m: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m)
}

/// Whether the take folder `take` holds parts kept while it was recorded.
pub fn exists(take: &Path) -> bool {
    take.join(DIR).join(HEADER).is_file()
}

/// The take in the folder `take` as far as the parts kept while it was
/// recorded reach: every part `partial.json` lists, each file checked
/// against its length and SHA-256, read as `analysis/` would be.
pub fn read(take: &Path) -> io::Result<Stored> {
    let dir = take.join(DIR);
    let partial = read_list(&dir)?;
    let header = partial.header;
    let mut all = Rows::default();
    let mut end_s = 0.0;
    for (i, part) in partial.parts.iter().enumerate() {
        if part.from_s != end_s || part.to_s <= part.from_s {
            return Err(invalid(format!(
                "{DIR}/{HEADER}: part {} doesn't follow on",
                i + 1
            )));
        }
        end_s = part.to_s;
        let renamed = partial.renamed_before.is_some_and(|n| i < n);
        let mut rows = read_part(&dir, i, part, header.bph)?;
        if let (Some(f), Some(l)) = (rows.beats.first(), all.beats.last()) {
            if f.time <= l.time || f.index + i64::from(renamed) <= l.index {
                return Err(invalid(format!(
                    "{DIR}/{}: its first beat doesn't follow the part before",
                    part_name(i, analysis_take::BEATS)
                )));
            }
        }
        if renamed {
            rows.beats.iter_mut().for_each(|b| b.index += 1);
            rows.windows
                .iter_mut()
                .for_each(AmplitudeWindow::swap_sides);
            rows.profiles
                .iter_mut()
                .for_each(StretchProfiles::swap_sides);
        }
        all.beats.append(&mut rows.beats);
        all.clipped.append(&mut rows.clipped);
        all.windows.append(&mut rows.windows);
        all.profiles.append(&mut rows.profiles);
    }
    let Rows {
        beats,
        clipped,
        mut windows,
        profiles,
    } = all;
    if end_s != header.duration_s
        || (beats.len(), windows.len(), profiles.len())
            != (header.beats, header.windows, header.profiles)
    {
        return Err(invalid(format!(
            "{DIR}/{HEADER} doesn't add up to its parts"
        )));
    }
    amplitude::reject_side_outliers(&mut windows, amplitude::OUTLIER_SPAN_S);
    Ok(analysis_take::stored_from(
        header,
        (beats, clipped),
        windows,
        profiles,
    ))
}

/// `partial.json` in the parts' folder `dir`, checked.
fn read_list(dir: &Path) -> io::Result<Partial> {
    let file = std::fs::File::open(dir.join(HEADER))
        .map_err(|e| io::Error::new(e.kind(), format!("{DIR}/{HEADER}: {e}")))?;
    let raw = crate::archive::read_limited(file, MAX_HEADER)?;
    if raw.len() as u64 > MAX_HEADER {
        return Err(invalid(format!("{DIR}/{HEADER} is too large")));
    }
    let partial: Partial =
        serde_json::from_slice(&raw).map_err(|e| invalid(format!("{DIR}/{HEADER}: {e}")))?;
    if partial.format != FORMAT {
        return Err(invalid(format!("{DIR}/{HEADER} is not a take's parts")));
    }
    analysis_take::check_header(&partial.header)
        .map_err(|e| invalid(format!("{DIR}/{HEADER}: {e}")))?;
    Ok(partial)
}

/// What a part holds, or the parts together.
#[derive(Default)]
struct Rows {
    beats: Vec<Beat>,
    clipped: Vec<f64>,
    windows: Vec<AmplitudeWindow>,
    profiles: Vec<StretchProfiles>,
}

/// Part `i` in the parts' folder `dir`, as `part` lists it, its beats at
/// `bph`.
fn read_part(dir: &Path, i: usize, part: &Part, bph: u32) -> io::Result<Rows> {
    let at = |kind: &str| format!("{DIR}/{}", part_name(i, kind));
    let load = |kind: &str, f: &PartFile| -> io::Result<Vec<u8>> {
        let bytes = std::fs::read(dir.join(part_name(i, kind)))
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", at(kind))))?;
        if bytes.len() as u64 != f.bytes || sha256_hex(&bytes) != f.sha256 {
            return Err(invalid(format!("{} isn't as {HEADER} lists it", at(kind))));
        }
        let limit = analysis_take::MAX_ANALYSIS_FILE;
        let out = crate::archive::read_limited(GzDecoder::new(&bytes[..]), limit)
            .map_err(|e| invalid(format!("{}: {e}", at(kind))))?;
        if out.len() as u64 > limit {
            return Err(invalid(format!("{} unpacks to too much", at(kind))));
        }
        Ok(out)
    };
    let text = |kind: &str, f: &PartFile| -> io::Result<String> {
        String::from_utf8(load(kind, f)?).map_err(|e| invalid(format!("{}: {e}", at(kind))))
    };
    let bad = |kind: &'static str| move |e: String| invalid(format!("{}: {e}", at(kind)));
    use analysis_take::{BEATS, CULLED, PROFILES, WINDOWS};
    let (beats, clipped) =
        analysis_take::parse_beats(&text(BEATS, &part.beats)?, bph).map_err(bad(BEATS))?;
    let mut windows =
        analysis_take::parse_windows(&text(WINDOWS, &part.windows)?).map_err(bad(WINDOWS))?;
    analysis_take::parse_culled(&text(CULLED, &part.culled)?, &mut windows).map_err(bad(CULLED))?;
    let profiles =
        analysis_take::parse_profiles(&load(PROFILES, &part.profiles)?).map_err(bad(PROFILES))?;
    let culled = windows.iter().filter(|w| w.culled_ms.is_some()).count();
    for (kind, want, got) in [
        (BEATS, part.beats.rows, beats.len()),
        (WINDOWS, part.windows.rows, windows.len()),
        (PROFILES, part.profiles.rows, profiles.len()),
        (CULLED, part.culled.rows, culled),
    ] {
        if want != got {
            return Err(invalid(format!(
                "{} holds {got} rows where {HEADER} says {want}",
                at(kind)
            )));
        }
    }
    Ok(Rows {
        beats,
        clipped,
        windows,
        profiles,
    })
}

/// Remove the parts from the take folder `take`, once its save is in
/// place; none there is no error.
pub fn remove(take: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(take.join(DIR)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
