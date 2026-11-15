//! One measurement context per take (#294): the take opened once, with
//! its settings, clock, breaks and masks, each sound file's place on the
//! take's timeline and each session entry's window, all in take seconds.
//!
//! A take's sound files are read end to end as one recording, so a time
//! in the take is a time in its sound counted from the first file's
//! start. Each entry of its session file names a recording (one of the
//! files, or the take's folder) and gives its stretch in seconds into
//! that recording; here every such stretch is moved by its recording's
//! offset, so every surface reads the same windows, cuts and masks.
//!
//! This first step builds the context; no surface reads it yet.

use crate::breaks::{self, Break};
use crate::clock::ClockFit;
use crate::session::{Manifest, Position, RecordingEntry};
use crate::stream::BeatLog;
use crate::take::{self, ClockFrom, EntrySettings, NoReading, Window};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// One sound file's place on its take's timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct FilePart {
    pub path: PathBuf,
    /// Where it starts in the take and how long it is, seconds.
    pub offset_s: f64,
    pub duration_s: f64,
}

/// The stretch one session entry reads, in take seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryWindow {
    /// Its place in the session file's list, from 0.
    pub entry: usize,
    pub position: Option<Position>,
    /// The take's sound files its recording is: indexes into
    /// [`TakeContext::files`]; empty for a take kept as its analysis alone.
    pub parts: Range<usize>,
    /// Where the recording it names starts in the take, seconds: its
    /// window less this is the window `session` reads in that recording.
    pub offset_s: f64,
    /// [`take::reading_window`] in the recording it names, moved by
    /// `offset_s`; or why it reads nothing.
    pub window: Result<Window, NoReading>,
}

/// What opening a take does with the stretches masked by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Masks {
    /// Cut from the beats, as every reading of a take does.
    Cut,
    /// Kept as data only, the beats read whole: for an editor that
    /// applies them itself and lets them be changed.
    Kept,
}

/// The stretches of a take `series` and `long` read ([`TakeContext::spans`]),
/// take seconds, in time order; `P` is how a stretch's position is named.
#[derive(Clone, Debug, PartialEq)]
pub struct Stretches<P = Option<Position>> {
    /// The stretches read, joined end to end where they change position.
    pub read: Vec<(f64, f64)>,
    /// What is left out inside a stretch, between two entries in one
    /// position: cut like a stretch masked by hand, at its width in true
    /// time, not joined.
    pub holes: Vec<(f64, f64)>,
    /// Each stretch's position.
    pub at: Vec<P>,
}

impl<P> Default for Stretches<P> {
    fn default() -> Self {
        Stretches {
            read: Vec::new(),
            holes: Vec::new(),
            at: Vec::new(),
        }
    }
}

/// The stretches `windows` read, each window (start, end, whether its
/// file carries on the one before it, its position) in time order, as
/// [`TakeContext::spans`] reads a take's entries and the app the
/// stretches it was in each position: a window carrying on the one before
/// runs on from it as one stretch; what is left out between two windows
/// in one position, one after the other, is a hole in that run, kept at
/// its width in true time as a mask is; between two positions the
/// stretches are joined. None when two windows overlap (they disagree
/// about where the watch was, and joined the sound they share would be
/// read twice).
pub fn stretches_of<P: PartialEq>(
    windows: impl IntoIterator<Item = (f64, f64, bool, P)>,
) -> Option<Stretches<P>> {
    let mut out = Stretches::default();
    for (a, b, carries_on, at) in windows {
        match out.read.last_mut() {
            Some(last) if a < last.1 - EDGE_S => return None,
            // The file's start is where the one before it ends, to the
            // rounding of the two lengths in seconds.
            Some(last) if carries_on && a <= last.1 + EDGE_S => last.1 = b,
            Some(last) if out.at.last() == Some(&at) => {
                if a > last.1 + EDGE_S {
                    out.holes.push((last.1, a));
                }
                last.1 = b;
            }
            _ => {
                out.read.push((a, b));
                out.at.push(at);
            }
        }
    }
    Some(out)
}

/// A take, opened once: what every surface reads it from.
pub struct TakeContext {
    /// 0 when opened. Nothing here changes a context once opened; a
    /// holder that changes it (the app editing masks, #294 step 5) bumps
    /// this, and a cache of anything read from it keys on it.
    pub rev: u64,
    /// How the take is read: beat rate, lift, escape teeth, the card's
    /// clock or not, and a settling time given to the reader.
    pub settings: EntrySettings,
    /// The clock the readings are on, as [`take::open_entry`] picks it.
    pub clock: Option<ClockFit>,
    pub clock_from: Option<ClockFrom>,
    pub clock_ppm: Option<f64>,
    pub clock_source: Option<String>,
    pub breaks: Vec<Break>,
    pub unlisted: Option<breaks::Unlisted>,
    /// The take's sound files in the order read; empty for a take kept as
    /// its analysis alone.
    pub files: Vec<FilePart>,
    /// The stretches masked by hand, in take seconds: each entry's masks
    /// moved by its recording's offset, in order and merged where they
    /// meet ([`crate::session::masked_spans`]).
    pub masked: Vec<(f64, f64)>,
    /// Each session entry's window, in the session file's order.
    pub entries: Vec<EntryWindow>,
    /// For a take kept as its analysis alone, the Tick Tock profiles it
    /// kept; `None` for a take read from its sound.
    pub profiles: Option<Vec<crate::analysis_take::StretchProfiles>>,
}

/// The recording entry `e` names, as a run of `parts`: which they are,
/// where it starts in the take and how long it is. `rate` is the files' sample rate and
/// `frames` each part's length in frames, so lengths come out exactly as
/// the beat log counts them (frames over the rate).
fn placed(
    base: &Path,
    e: &RecordingEntry,
    parts: &[FilePart],
    frames: &[u64],
    rate: u32,
) -> Result<Place, String> {
    let files = take::sound_files(&[base.join(&e.file)])?;
    let first = files
        .first()
        .and_then(|f| parts.iter().position(|p| same_file(&p.path, f)))
        .ok_or_else(|| format!("{}: not one of the take's recordings", e.file))?;
    let run = parts.get(first..first + files.len());
    if run.is_none_or(|run| run.iter().zip(&files).any(|(p, f)| !same_file(&p.path, f))) {
        return Err(format!(
            "{}: its recordings are not in the take's order",
            e.file
        ));
    }
    let before: u64 = frames[..first].iter().sum();
    let own: u64 = frames[first..first + files.len()].iter().sum();
    Ok(Place {
        parts: first..first + files.len(),
        offset_s: before as f64 / rate as f64,
        length_s: own as f64 / rate as f64,
    })
}

/// Whether two paths name one file, however each is spelt (`./a.wav`,
/// a link): as the paths themselves when either can't be resolved.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The take's sound files with their places, and each one's length in
/// frames and their sample rate.
fn file_parts(files: &[PathBuf]) -> Result<(Vec<FilePart>, Vec<u64>, u32), String> {
    let mut parts = Vec::new();
    let mut frames = Vec::new();
    let mut rate = None;
    let mut at = 0u64;
    for f in files {
        let i = crate::audio::info(f).map_err(|err| format!("{}: {err}", f.display()))?;
        let n = i
            .frames
            .ok_or_else(|| format!("{}: its header gives no length", f.display()))?;
        if *rate.get_or_insert(i.sample_rate) != i.sample_rate {
            return Err(format!(
                "{}: recorded at {} Hz, unlike the take's other recordings",
                f.display(),
                i.sample_rate
            ));
        }
        let r = i.sample_rate as f64;
        parts.push(FilePart {
            path: f.clone(),
            offset_s: at as f64 / r,
            duration_s: n as f64 / r,
        });
        frames.push(n);
        at += n;
    }
    Ok((parts, frames, rate.unwrap_or(1)))
}

/// A window moved later by `by` seconds.
fn moved(w: Window, by: f64) -> Window {
    Window {
        start_s: w.start_s + by,
        stop_s: w.stop_s + by,
        from_s: w.from_s + by,
        to_s: w.to_s + by,
    }
}

/// Each entry of `m` placed on the take's timeline at `places` (its
/// recording's offset and length): its window there, as `session` reads
/// it in that recording, moved by the offset.
fn entry_windows(m: &Manifest, given_settle_s: Option<f64>, places: &[Place]) -> Vec<EntryWindow> {
    m.recordings
        .iter()
        .zip(places)
        .enumerate()
        .map(|(i, (e, p))| EntryWindow {
            entry: i,
            position: e.position.as_deref().and_then(Position::parse),
            parts: p.parts.clone(),
            offset_s: p.offset_s,
            window: take::reading_window(e, m, given_settle_s, p.length_s)
                .map(|w| moved(w, p.offset_s)),
        })
        .collect()
}

/// The stretches `entries` mask by hand, in take seconds: each entry's
/// masks moved by its recording's offset (`offsets`, one per entry, in
/// order), not yet merged. What [`TakeContext::open`] puts on the take and
/// the app on its strip when it opens one (#294 step 5).
pub fn masks_in_take(
    entries: &[RecordingEntry],
    offsets: impl IntoIterator<Item = f64>,
) -> Vec<[f64; 2]> {
    entries
        .iter()
        .zip(offsets)
        .flat_map(|(e, at)| e.masked.iter().map(move |[a, b]| [a + at, b + at]))
        .collect()
}

impl TakeContext {
    /// Open the take described by `m` in the folder `base` once, as one
    /// recording over all its sound ([`take::open_entry`]), with each
    /// entry's masks moved to take seconds, and place each entry's window
    /// on it. `masks` says whether the masks are cut from the beats or
    /// only kept as data; [`TakeContext::masked`] holds them either way.
    /// `usb`, `correction` and `progress` are as
    /// [`take::open_entry`] takes them. Gives the context and the take's
    /// beats.
    pub fn open(
        base: &Path,
        m: &Manifest,
        settings: EntrySettings,
        masks: Masks,
        usb: impl FnOnce(&[PathBuf]) -> Result<Option<(f64, String)>, String>,
        correction: impl FnOnce(&Path) -> Result<Option<(f64, String)>, String>,
        progress: impl FnMut(f64, f64),
    ) -> Result<(TakeContext, BeatLog), String> {
        let whole = whole_recording(m)?;
        let kept = crate::checkpoint::is_kept(&base.join(&whole.file));
        let (files, mut places) = if kept {
            (Vec::new(), kept_places(m, f64::INFINITY)?)
        } else {
            layout(base, m, &whole)?
        };
        // The places come from the files' headers, so the masks are put
        // in take seconds before the beats are read.
        let masked = masks_in_take(&m.recordings, places.iter().map(|p| p.offset_s));
        let entry = RecordingEntry {
            masked: match masks {
                Masks::Cut => masked.clone(),
                Masks::Kept => Vec::new(),
            },
            ..whole
        };
        let opened = take::open_entry(base, m, &entry, &settings, usb, correction, progress)?;
        if kept {
            for p in &mut places {
                p.length_s = opened.log.duration_s;
            }
        }
        let entries = entry_windows(m, settings.settle_s, &places);
        let ctx = TakeContext {
            rev: 0,
            settings,
            clock: opened.clock,
            clock_from: opened.clock_from,
            clock_ppm: opened.clock_ppm,
            clock_source: opened.clock_source,
            breaks: opened.breaks,
            unlisted: opened.unlisted,
            files,
            masked: crate::session::masked_spans(&masked),
            entries,
            profiles: opened.kept.map(|k| k.profiles),
        };
        Ok((ctx, opened.log))
    }
}

impl TakeContext {
    /// Each entry of `m` (the session file the context was opened from)
    /// that names a position, as [`take::listed_positions`] gives them but
    /// in take seconds: the stretch's start, the start of what is read once
    /// the watch has settled and the end of that (an entry that reads
    /// nothing settling to its end), and the position.
    pub fn listed_positions(&self, m: &Manifest) -> Vec<take::ListedPosition<Position>> {
        m.recordings
            .iter()
            .zip(&self.entries)
            .filter_map(|(e, w)| {
                take::listed_position(e, m, w.window.as_ref().ok().copied(), w.offset_s)
            })
            .collect()
    }

    /// The stretches of the take that are read, take seconds, in time
    /// order: each entry of `m` (the session file the context was opened
    /// from) read from its window ([`EntryWindow::window`]), so `series`
    /// and `long` read exactly what `session` reads of each entry, joined
    /// by [`stretches_of`]: a file carries on the one before it in the
    /// same position as [`take::continues_position`] says. An entry for
    /// the take's whole folder, beside entries for its files, that says no
    /// more than their position reads on from the first file's settling to
    /// the take's end.
    /// None, and the take read whole, when an entry names no position,
    /// when two entries' stretches overlap, or when the one stretch is the
    /// whole take and holds no hole.
    pub fn spans(&self, m: &Manifest) -> Option<Stretches> {
        if m.recordings.iter().any(|e| e.position.is_none()) {
            return None;
        }
        let out = stretches_of(self.windows_read(m))?;
        let total = self.length_s();
        match out.read.as_slice() {
            [] => None,
            [(a, b)] if *a <= 0.0 && *b >= total - EDGE_S && out.holes.is_empty() => None,
            _ => Some(out),
        }
    }

    /// The take's length, seconds: where its last recording ends.
    fn length_s(&self) -> f64 {
        self.files
            .iter()
            .map(|f| f.offset_s + f.duration_s)
            .fold(0.0, f64::max)
    }

    /// Each entry's window that reads something, in time order, with
    /// whether its file carries on the one before it and its position.
    /// An entry for the folder that only says the first file's position
    /// reads from that file's settling to the take's end, as `session`
    /// reads the folder (#1145); the files' own windows inside it are
    /// read there.
    fn windows_read(&self, m: &Manifest) -> Vec<(f64, f64, bool, Option<Position>)> {
        let folder = |e: &RecordingEntry| take::names_whole_folder(&e.file);
        let first_file = m.recordings.iter().position(|e| !folder(e));
        let says_only_position = |e: &RecordingEntry| {
            folder(e)
                && first_file.is_some_and(|f| {
                    e.position == m.recordings[f].position
                        && e.start_s.is_none()
                        && e.end_s.is_none()
                        && e.settle_s.is_none()
                        && e.measure_s.is_none()
                })
        };
        let position = |e: &RecordingEntry| e.position.as_deref().and_then(Position::parse);
        let window = |w: &EntryWindow| w.window.as_ref().ok().map(|w| (w.from_s, w.to_s));
        let total = self.length_s();
        let whole = m
            .recordings
            .iter()
            .find(|e| says_only_position(e))
            .and_then(|e| {
                let (a, _) = window(&self.entries[first_file?])?;
                Some((a, total, false, position(e)))
            });
        let inside_whole = |a: f64, b: f64| {
            whole.is_some_and(|(wa, wb, _, _)| a >= wa - EDGE_S && b <= wb + EDGE_S)
        };
        let mut read: Vec<_> = m
            .recordings
            .iter()
            .zip(&self.entries)
            .filter(|(e, _)| !says_only_position(e))
            .filter_map(|(e, w)| {
                let (a, b) = window(w)?;
                (b > a && !inside_whole(a, b))
                    .then(|| (a, b, take::continues_position(e, m), position(e)))
            })
            .chain(whole)
            .collect();
        read.sort_by(|a, b| a.0.total_cmp(&b.0));
        read
    }

    /// The take's changes of position and the settling before each, in
    /// take seconds ([`take::positions_of`] over [`Self::listed_positions`]):
    /// what `m` (the session file the context was opened from) lists, an
    /// entry carrying on the one before it no change.
    pub fn positions(&self, m: &Manifest) -> take::Positions<Position> {
        take::positions_of(self.listed_positions(m).into_iter())
    }

    /// The take's settlings, in take seconds: from each change of position
    /// (the first, the take's start, included) to where its reading starts,
    /// as [`Self::positions`] gives them. What a reading over the whole take
    /// leaves out as a mask ([`crate::session::with_holes`]), so none falls
    /// where the watch was just handled (#1233).
    pub fn holes(&self, m: &Manifest) -> Vec<(f64, f64)> {
        self.positions(m).1
    }

    /// The take's breaks with a cut, of no gap, wherever its readings are
    /// cut and no break already is ([`take::reading_cuts`]): each change of
    /// position after the first ([`Self::positions`], a restart in the same
    /// position included, as the watch was handled there) and the end of
    /// each settling. What a reading over the whole take, such as a long
    /// take's hours, is cut by, as the app cuts the take it opens.
    pub fn reading_breaks(&self, m: &Manifest) -> Vec<Break> {
        let (stretches, settles) = self.positions(m);
        let changes: Vec<f64> = stretches.iter().map(|s| s.0).collect();
        breaks::with_jumps(
            self.breaks.clone(),
            &take::reading_cuts(&self.breaks, &changes, &settles),
        )
    }
}

/// The one recording a take is opened as: [`take::whole_take`]'s, which
/// may hold every entry's masks only while they all name one recording;
/// here the masks are placed in take seconds by the caller.
fn whole_recording(m: &Manifest) -> Result<RecordingEntry, String> {
    let unmasked = Manifest {
        recordings: m
            .recordings
            .iter()
            .map(|e| RecordingEntry {
                masked: Vec::new(),
                ..e.clone()
            })
            .collect(),
        ..m.clone()
    };
    take::whole_take(&unmasked)
}

/// An entry of a take kept as its analysis alone, which is one recording:
/// at the take's start, the take's length long. An entry naming one of
/// the files it was made from can't be placed.
fn kept_place(e: &RecordingEntry, length_s: f64) -> Result<Place, String> {
    match e.file.as_str() {
        f if crate::take::names_whole_folder(f) => Ok(Place {
            parts: 0..0,
            offset_s: 0.0,
            length_s,
        }),
        f => Err(format!(
            "{f}: the take is kept as its analysis alone, so its entries read it whole"
        )),
    }
}

/// Every entry of a take kept as its analysis alone, `length_s` long.
fn kept_places(m: &Manifest, length_s: f64) -> Result<Vec<Place>, String> {
    m.recordings
        .iter()
        .map(|e| kept_place(e, length_s))
        .collect()
}

/// The session entries of a take, laid on its timeline as [`TakeContext`]
/// lays them, from the sound files' headers alone (or, for a take kept as
/// its analysis alone, its kept length): for a reader that needs the
/// windows before (or without) finding the beats.
pub fn entry_layout(
    base: &Path,
    m: &Manifest,
    given_settle_s: Option<f64>,
) -> Result<(Vec<FilePart>, Vec<EntryWindow>), String> {
    let whole = whole_recording(m)?;
    let path = base.join(&whole.file);
    let (files, places) = if crate::checkpoint::is_kept(&path) {
        let length_s = crate::checkpoint::read_kept(&path)
            .map_err(|err| format!("{}: {err}", path.display()))?
            .log
            .duration_s;
        (Vec::new(), kept_places(m, length_s)?)
    } else {
        layout(base, m, &whole)?
    };
    Ok((files, entry_windows(m, given_settle_s, &places)))
}

/// How far apart two times may be and still be one place on the take's
/// timeline: a file's end and the next one's start, each summed from
/// lengths in seconds, can differ in their last bits.
const EDGE_S: f64 = 1e-9;

/// An entry's recording on the take's timeline: the sound files it is,
/// where it starts in the take and its length, seconds.
struct Place {
    parts: Range<usize>,
    offset_s: f64,
    length_s: f64,
}

/// The stretch `a` to `b` of the take, in take seconds, as it lies in
/// each sound file it crosses: the file's index in `files` and the
/// stretch in that file's own seconds, split where one file ends and the
/// next begins. What a mask drawn on the take is saved as.
pub fn in_files(files: &[FilePart], a: f64, b: f64) -> Vec<(usize, f64, f64)> {
    files
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let from = a.max(f.offset_s);
            let to = b.min(f.offset_s + f.duration_s);
            (from < to).then_some((i, from - f.offset_s, to - f.offset_s))
        })
        .collect()
}

/// The take's sound files, as `whole` reads them, with each entry's
/// place among them (its recording's offset and length).
fn layout(
    base: &Path,
    m: &Manifest,
    whole: &RecordingEntry,
) -> Result<(Vec<FilePart>, Vec<Place>), String> {
    let (files, frames, rate) = file_parts(&take::sound_files(&[base.join(&whole.file)])?)?;
    let places = m
        .recordings
        .iter()
        .map(|e| placed(base, e, &files, &frames, rate))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((files, places))
}
