//! Saving what the microphone hears, so every test can be analysed again.
//!
//! A recording is a folder holding the audio as files of at most an hour
//! each (`audio-001.flac`, `audio-002.flac`, ...), a clock log
//! (`clock.csv`: system time against frames captured, every 10 s) and
//! `session.json` with the watch, position, lift angle and settings.
//! `timegrapher long <folder>/ --clock <folder>/clock.csv` reads it as one
//! continuous recording calibrated against the system clock.
//!
//! Each hour is written as a WAV (`audio-001.wav`), whose header a flush
//! keeps whole should the program stop. Once the hour closes (it is full,
//! listening pauses), a thread of its own keeps it as FLAC ([`crate::flac::keep_as_flac`], #503), and `session.json` names
//! the FLAC from then on. A copy saved meanwhile holds every closed hour
//! as FLAC too; only an hour still being written is copied as a WAV.
//!
//! A pause leaves the audio running straight on but the time between out:
//! listening again starts a new file, the clock log gets a line either side
//! of the gap (frames running on, none made up across it), and
//! `session.json` gains a `breaks` entry with the frame it starts at, its
//! file, the host time of its first frame, the gap and the input. A take
//! never paused has no `breaks` key at all. `frame` counts from the take's
//! first frame (frame 0 of `audio-001.wav`) and runs on across files, so
//! `audio_s` is `frame` over the sample rate; readers key on `frame`, and
//! `segment`, `utc` and `device` are for people.

use crate::capture::Block;
use crate::take;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod closed_hour;
#[cfg(test)]
mod session_file_tests;
mod settle;
use closed_hour::{open_closed_hour, open_in_place};
use settle::{held_open, settled};
pub use settle::{read_settled, rename_settled, sync_file, sync_folder};

/// Bits a sample of every take's sound is kept at, whatever the
/// microphone gives. Four 24-bit takes held nothing in their low 8 bits
/// and read the same kept at 16 (#1176); the stand's noise is a few
/// 16-bit steps, rounding adds a third of one.
pub const STORED_BITS: u16 = 16;

/// A sample of sound between -1 and 1 as it is kept at [`STORED_BITS`]:
/// rounded to the nearest step, full scale one step short of the top.
/// The recorder keeps a microphone's sound this way, and a floating-point
/// WAV is kept as FLAC this way ([`crate::flac::encode_wav`]). Not a
/// number keeps as 0.
pub fn stored_sample(s: f32) -> i32 {
    let full = ((1u64 << (STORED_BITS - 1)) - 1) as f32;
    (s.clamp(-1.0, 1.0) * full).round() as i32
}

/// A `bits`-bit integer sample as it is kept at [`STORED_BITS`]: rounded
/// to the nearest step, half away from zero, the top kept in range. One
/// already at [`STORED_BITS`] or fewer keeps as it is, so sound from a
/// 16-bit microphone in a wider file keeps exactly.
pub fn stored_int(x: i32, bits: u16) -> i32 {
    match bits.checked_sub(STORED_BITS) {
        None | Some(0) => x,
        Some(drop) => {
            let top = (1i64 << (STORED_BITS - 1)) - 1;
            let r = (x as f64 / (1u64 << drop) as f64).round() as i64;
            r.clamp(-top - 1, top) as i32
        }
    }
}

/// What is known about a recording when it starts.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub watch: String,
    /// Position code: DU, DD, CU, CD, CL or CR.
    pub position: String,
    pub lift_deg: f64,
    /// Beat rate set by hand; `None` when it is guessed.
    pub bph: Option<u32>,
    pub device: String,
    pub sample_rate: u32,
    /// The microphone's own depth; its sound is kept at [`STORED_BITS`].
    pub bits: u16,
    /// Program and version that made the recording.
    pub software: String,
}

/// Something that happened during the recording, at a time in the audio.
#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub audio_s: f64,
    pub utc: String,
    pub text: String,
}

#[derive(Serialize)]
struct SessionFile<'a> {
    #[serde(flatten)]
    info: &'a SessionInfo,
    started_utc: &'a str,
    ended_utc: Option<String>,
    duration_s: f64,
    segments: Vec<String>,
    notes: &'a [Note],
    /// Left out altogether while there are none, so a take that was never
    /// paused reads as it always has.
    #[serde(skip_serializing_if = "<[Break]>::is_empty")]
    breaks: &'a [Break],
    /// Readings at the end, as the program saw them.
    result: Option<&'a serde_json::Value>,
}

/// Where listening carried on after a pause. The audio runs straight on
/// across it, but the time between is missing, so a reader starts afresh
/// there: a fresh lock on the beats, and a clock fit that doesn't span it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Break {
    /// Seconds of audio before it, for people.
    pub audio_s: f64,
    /// Frames in the whole take before it; readers key on this.
    pub frame: u64,
    /// The first file after it: each resume starts a new one.
    pub segment: String,
    /// Host time of the first frame after it.
    pub utc: String,
    /// Host time from the last frame before it to the first after it, from
    /// the clock log's lines either side.
    pub gap_s: f64,
    /// The input listened to after it (another card has its own clock
    /// error).
    pub device: String,
}

const SEGMENT_S: u64 = 3600;

/// A take kept while it records has its sound added to its copy each time
/// this much more has been heard ([`Recorder::begin_stream_save`]): a
/// crash or a power cut costs no more.
pub const STREAM_EVERY_S: f64 = 5.0;
/// In a recording folder whose take records into its copy as it goes, the
/// file naming that copy's folder: sound left here after a crash is
/// already there, but for its last few seconds. Never copied with a save.
pub const STREAMED_TO: &str = "streamed-to.txt";

/// After a failed save of a take kept while it records, the next is tried
/// this much later.
pub const STREAM_RETRY: std::time::Duration = std::time::Duration::from_secs(60);
const CLOCK_EVERY_S: f64 = 10.0;
/// Over the first seconds of a take the clock log gets a line every
/// [`CLOCK_EARLY_EVERY_S`] instead, so a take of a few seconds still has
/// the entries a clock fit needs ([`crate::clock::ClockFit`] wants three
/// after it drops a stray one). Few enough that a log still reaches
/// [`crate::clock::MIN_POINTS`] entries, where it may correct a rate,
/// only after about a minute.
const CLOCK_EARLY_S: f64 = 3.0;
const CLOCK_EARLY_EVERY_S: f64 = 1.0;

/// Whether the clock log takes a line for a block that arrived at `at`,
/// with `audio_s` of the take written by its end, when the last line on
/// the regular cadence was for a block that arrived at `last` (`None`: no
/// line yet, or a break since): every [`CLOCK_EVERY_S`], or
/// [`CLOCK_EARLY_EVERY_S`] over the first [`CLOCK_EARLY_S`].
pub fn clock_line_due(audio_s: f64, last: Option<SystemTime>, at: SystemTime) -> bool {
    let every = if audio_s <= CLOCK_EARLY_S + 1e-9 {
        CLOCK_EARLY_EVERY_S
    } else {
        CLOCK_EVERY_S
    };
    match last {
        None => true,
        Some(t) => at
            .duration_since(t)
            .map(|d| d.as_secs_f64() >= every - 1e-9)
            .unwrap_or(false),
    }
}

pub struct Recorder {
    dir: PathBuf,
    info: SessionInfo,
    /// The position at the start, which `set_info` never changes: later
    /// positions are the "position" notes.
    first_position: String,
    started_utc: String,
    writer: Option<hound::WavWriter<BufWriter<File>>>,
    segments: Vec<String>,
    /// Closed hours being kept as FLAC, by their WAV's name.
    converting: Vec<(String, Converting)>,
    /// Whether closed hours are kept as FLAC; only a test keeps them WAVs.
    keep_flac: bool,
    seg_frames: u64,
    frames: u64,
    clock: BufWriter<File>,
    last_clock: Option<SystemTime>,
    /// The arrival of the block the clock log's last line is for.
    logged: Option<SystemTime>,
    /// The latest block's arrival and the frames written by then.
    last_block: Option<(SystemTime, u64)>,
    /// Paused: the host time of the last frame before the pause, until
    /// listening carries on.
    paused_at: Option<SystemTime>,
    /// Sound lost before the next block ([`Recorder::sound_lost`]).
    lost_s: Option<f64>,
    /// The input listened to after the pause, when it was named.
    resumed_on: Option<String>,
    breaks: Vec<Break>,
    notes: Vec<Note>,
    /// The folder name the first copy was saved under, so Save Again lands
    /// in the same place.
    saved_name: Option<std::ffi::OsString>,
    /// The hour still being written as the last save left it in the copy,
    /// and its length there: the next save into the same copy adds only
    /// what came since ([`Recorder::begin_stream_save`]).
    grown: Option<(PathBuf, u64)>,
    /// Held locked while recording ([`in_use`]), beside the folder rather
    /// than in it, so a save never copies it.
    lock: Option<File>,
}

/// A closed hour being kept as FLAC on a thread of its own.
type Converting = std::thread::JoinHandle<Result<PathBuf, crate::audio::AudioError>>;

/// Whether `name` is one of a recording's sound files, as the recorder
/// names them: no companion file may take such a name.
fn is_sound(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("audio-")
}

/// The lock file beside a recording folder while it is being recorded.
fn lock_path(dir: &Path) -> PathBuf {
    let mut name = dir.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    dir.with_file_name(name)
}

/// Whether the recording folder `dir` is being recorded into now, by this
/// app or another copy of it: its lock is held. The system lets go of a
/// lock when its holder quits or is killed, so a folder left behind reads
/// as not in use.
pub fn in_use(dir: &Path) -> bool {
    match File::open(lock_path(dir)) {
        Ok(f) => matches!(f.try_lock(), Err(fs::TryLockError::WouldBlock)),
        Err(_) => false,
    }
}

/// Remove the lock file a recording left beside `dir`, when nothing holds
/// it.
pub fn remove_stale_lock(dir: &Path) {
    if !in_use(dir) {
        let _ = fs::remove_file(lock_path(dir));
    }
}

fn io_err(e: hound::Error) -> io::Error {
    match e {
        hound::Error::IoError(e) => e,
        other => io::Error::other(other.to_string()),
    }
}

impl Recorder {
    /// Start a recording in a new folder under `parent`, named after the
    /// start time, the watch and the position.
    pub fn start(parent: &Path, info: SessionInfo) -> io::Result<Recorder> {
        let now = SystemTime::now();
        let started_utc = utc(now);
        let mut name = stamp_of(&started_utc);
        let watch = safe_name(&info.watch);
        if !watch.is_empty() {
            name = format!("{name}_{watch}");
        }
        name = format!("{name}_{}", safe_name(&info.position));
        let dir = new_dir(parent, &name)?;
        // Best effort: a system that can't lock only loses the in-use check.
        let lock = File::create(lock_path(&dir))
            .ok()
            .filter(|f| f.try_lock().is_ok());
        let mut clock = BufWriter::new(File::create(dir.join(take::CLOCK_LOG))?);
        writeln!(
            clock,
            "# system time (Unix seconds) against audio frames captured"
        )?;
        writeln!(clock, "unix_s,frames")?;
        let r = Recorder {
            dir,
            first_position: info.position.clone(),
            info,
            started_utc,
            writer: None,
            segments: Vec::new(),
            converting: Vec::new(),
            keep_flac: true,
            seg_frames: 0,
            frames: 0,
            clock,
            last_clock: None,
            logged: None,
            last_block: None,
            paused_at: None,
            lost_s: None,
            resumed_on: None,
            breaks: Vec::new(),
            notes: Vec::new(),
            saved_name: None,
            grown: None,
            lock,
        };
        r.write_session(None, None)?;
        Ok(r)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// What was known when the recording started.
    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    /// The position the recording started in. [`Recorder::info`] has the
    /// latest one.
    pub fn first_position(&self) -> &str {
        &self.first_position
    }

    /// What happened during it, oldest first.
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn duration_s(&self) -> f64 {
        self.frames as f64 / self.info.sample_rate as f64
    }

    /// Mark a break before block `b`, the first after a pause whose last
    /// frame was heard at `before`, and start a new file for it.
    fn start_break(&mut self, b: &Block, before: SystemTime, device: String) -> io::Result<()> {
        if self.frames == 0 {
            return Ok(());
        }
        self.close_segment()?;
        self.open_segment()?;
        let fs = self.info.sample_rate as f64;
        // A block arrives with its last frame; its first came earlier.
        let first =
            b.at.checked_sub(std::time::Duration::from_secs_f64(
                b.samples.len() as f64 / fs,
            ))
            .unwrap_or(b.at);
        let gap_s = match first.duration_since(before) {
            Ok(d) => d.as_secs_f64(),
            Err(e) => -e.duration().as_secs_f64(),
        };
        self.breaks.push(Break {
            audio_s: self.frames as f64 / fs,
            frame: self.frames,
            segment: self.segments.last().cloned().unwrap_or_default(),
            utc: utc(first),
            gap_s,
            device,
        });
        Ok(())
    }

    /// Where listening carried on after a pause, oldest first.
    pub fn breaks(&self) -> &[Break] {
        &self.breaks
    }

    /// Close the hour being written, and keep it as FLAC on a thread of
    /// its own.
    fn close_segment(&mut self) -> io::Result<()> {
        let Some(w) = self.writer.take() else {
            return Ok(());
        };
        w.finalize().map_err(io_err)?;
        if let Some(name) = self.segments.last().filter(|_| self.keep_flac).cloned() {
            let wav = self.dir.join(&name);
            let job = std::thread::spawn(move || crate::flac::keep_as_flac(&wav));
            self.converting.push((name, job));
        }
        Ok(())
    }

    /// Take in the hours kept as FLAC since last time (all of them, waiting
    /// on each, with `wait`): `segments` and the breaks name the FLAC from
    /// now on. An hour that could not be converted stays a WAV, whole.
    /// Returns whether a name changed, for `session.json` to follow.
    fn reap(&mut self, wait: bool) -> bool {
        let mut renamed = false;
        let mut i = 0;
        while i < self.converting.len() {
            if !wait && !self.converting[i].1.is_finished() {
                i += 1;
                continue;
            }
            let (wav, job) = self.converting.remove(i);
            let Ok(Ok(flac)) = job.join() else {
                continue;
            };
            let flac = flac.file_name().unwrap_or_default().to_string_lossy();
            let names = self.segments.iter_mut();
            for n in names.chain(self.breaks.iter_mut().map(|b| &mut b.segment)) {
                if *n == wav {
                    *n = flac.to_string();
                    renamed = true;
                }
            }
        }
        renamed
    }

    fn open_segment(&mut self) -> io::Result<()> {
        let name = format!("audio-{:03}.wav", self.segments.len() + 1);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.info.sample_rate,
            bits_per_sample: STORED_BITS,
            sample_format: hound::SampleFormat::Int,
        };
        self.writer = Some(hound::WavWriter::create(self.dir.join(&name), spec).map_err(io_err)?);
        self.segments.push(name);
        self.seg_frames = 0;
        Ok(())
    }

    /// Append a block of audio.
    pub fn write(&mut self, b: &Block) -> io::Result<()> {
        let fs = self.info.sample_rate as u64;
        if let Some(gap) = self.lost_s.take() {
            // As if paused where the gap began: the break lists the gap.
            let first_s = b.samples.len() as f64 / fs as f64 + gap;
            self.paused_at =
                b.at.checked_sub(std::time::Duration::from_secs_f64(first_s.max(0.0)));
        }
        if let Some(before) = self.paused_at.take() {
            let device = self
                .resumed_on
                .take()
                .unwrap_or_else(|| self.info.device.clone());
            self.start_break(b, before, device)?;
        }
        for &s in &b.samples {
            if self.writer.is_none() || self.seg_frames >= SEGMENT_S * fs {
                self.close_segment()?;
                self.open_segment()?;
            }
            let w = self.writer.as_mut().expect("segment open");
            w.write_sample(stored_sample(s)).map_err(io_err)?;
            self.seg_frames += 1;
        }
        self.frames += b.samples.len() as u64;
        let due = clock_line_due(self.duration_s(), self.last_clock, b.at);
        self.last_block = Some((b.at, self.frames));
        if due {
            self.log_last_block()?;
            // Keep the WAV header valid in case the program stops abruptly.
            if let Some(w) = self.writer.as_mut() {
                w.flush().map_err(io_err)?;
            }
            self.last_clock = Some(b.at);
            if self.reap(false) {
                self.write_session(None, None)?;
            }
        }
        Ok(())
    }

    /// A clock log line for the latest block, unless the last line is
    /// already for it: every 10 s, and wherever the sound stops (a pause,
    /// a save, the end), so a take of a few seconds still has two lines
    /// to read its clock from.
    fn log_last_block(&mut self) -> io::Result<()> {
        if let Some((at, frames)) = self.last_block {
            if self.logged != Some(at) {
                let unix = at.duration_since(UNIX_EPOCH).unwrap_or_default();
                writeln!(
                    self.clock,
                    "{}.{:09},{}",
                    unix.as_secs(),
                    unix.subsec_nanos(),
                    frames
                )?;
                self.logged = Some(at);
            }
        }
        self.clock.flush()
    }

    /// Write out what is buffered, so the files on disk hold everything
    /// so far and can be read while the recording carries on.
    pub fn flush(&mut self) -> io::Result<()> {
        if let Some(w) = self.writer.as_mut() {
            w.flush().map_err(io_err)?;
        }
        self.clock.flush()
    }

    /// The audio files so far, in order (after [`Recorder::flush`] they
    /// hold everything written).
    pub fn segment_paths(&self) -> Vec<PathBuf> {
        self.segments.iter().map(|n| self.dir.join(n)).collect()
    }

    /// Note an event (a change of position, say) at the current audio time.
    pub fn note(&mut self, text: &str) -> io::Result<()> {
        self.notes.push(Note {
            audio_s: self.duration_s(),
            utc: utc(SystemTime::now()),
            text: text.to_string(),
        });
        self.write_session(None, None)
    }

    /// Update the settings saved with the recording (a new position or lift angle).
    pub fn set_info(&mut self, info: SessionInfo) -> io::Result<()> {
        self.info.position = info.position;
        self.info.lift_deg = info.lift_deg;
        self.info.bph = info.bph;
        self.info.watch = info.watch;
        self.write_session(None, None)
    }

    /// `session.json` replaced whole (#1639): a failed write leaves the
    /// last good one, never an empty file.
    fn write_session(
        &self,
        ended: Option<String>,
        result: Option<&serde_json::Value>,
    ) -> io::Result<()> {
        let text = self.session_text(ended, result, false)?;
        crate::library::write_atomic(&self.dir.join(take::RECORD), text.as_bytes())
            .map_err(io::Error::other)
    }

    /// `session.json`'s text; for a saved copy (`copy`), naming every
    /// closed hour as the FLAC the copy holds it as.
    fn session_text(
        &self,
        ended: Option<String>,
        result: Option<&serde_json::Value>,
        copy: bool,
    ) -> io::Result<String> {
        let name = |n: &String| match copy && !self.is_open(n) {
            true => as_flac(n),
            false => n.clone(),
        };
        let breaks: Vec<Break> = self
            .breaks
            .iter()
            .map(|b| Break {
                segment: name(&b.segment),
                ..b.clone()
            })
            .collect();
        let f = SessionFile {
            info: &self.info,
            started_utc: &self.started_utc,
            ended_utc: ended,
            duration_s: self.duration_s(),
            segments: self.segments.iter().map(name).collect(),
            notes: &self.notes,
            breaks: &breaks,
            result,
        };
        serde_json::to_string_pretty(&f).map_err(io::Error::other)
    }

    /// Note a pause in listening. The clock log gets a line for the last
    /// block before it and another for the first block after it, so both
    /// sides of the gap are measured; audio written after [`Recorder::resume`]
    /// starts a new file and a [`Break`].
    pub fn pause(&mut self) -> io::Result<()> {
        self.log_last_block()?;
        if let Some((at, _)) = self.last_block {
            self.paused_at = Some(at);
        }
        self.last_clock = None;
        if let Some(w) = self.writer.as_mut() {
            w.flush().map_err(io_err)?;
        }
        self.note("paused")?;
        // Listening again starts a new hour, so this one is done: it is
        // kept as FLAC while paused, and a save then holds it so. A pause
        // that could not be noted leaves the hour as it is, for recovery.
        self.close_segment()
    }

    /// Note sound lost from a live input, `gap_s` true seconds of it, as
    /// [`crate::breaks::GapWatch`] finds it: the next block written starts
    /// a break listed with that gap, and the clock log adds no line before
    /// it. Sound lost more than once with no block between adds up, so
    /// a drop and the gap found after it are one break of both (#1051).
    pub fn sound_lost(&mut self, gap_s: f64) {
        self.lost_s = Some(self.lost_s.unwrap_or(0.0) + gap_s);
        self.last_clock = None;
    }

    /// Carry on after [`Recorder::pause`], listening to `device` at the
    /// take's own sample rate (the caller refuses any other). The next block
    /// written starts a break either way; this names its input.
    pub fn resume(&mut self, device: &str) {
        if self.paused_at.is_some() {
            self.resumed_on = Some(device.to_string());
        }
    }

    /// The name for a copy: the start time, then the watch's name and the
    /// calibre when they are known by the time of the copy. No position:
    /// session.json and session.toml say where the watch was and when.
    /// When the recording started, as `2026-10-20T21:32:00Z`.
    pub fn started_utc(&self) -> &str {
        &self.started_utc
    }

    /// Name the first saved copy `name` (a take folder in a watch of the
    /// library) rather than by its start, watch and calibre. A copy
    /// already saved keeps its name, so Save Again saves into it.
    pub fn name_first_copy(&mut self, name: std::ffi::OsString) {
        self.saved_name.get_or_insert(name);
    }

    fn copy_name(&self, calibre: &str) -> std::ffi::OsString {
        let mut name = stamp_of(&self.started_utc);
        for part in [safe_name(&self.info.watch), safe_name(calibre)] {
            if !part.is_empty() {
                name = format!("{name}_{part}");
            }
        }
        name.into()
    }

    /// What a copy kept as its analysis alone holds beside its `analysis/`
    /// folder ([`crate::analysis_take::save`]): every file of the recording
    /// but its sound, with session.json as a copy has it (`result` saved as
    /// the readings at the time), then `required_files`; and the copy's
    /// folder name, the copy's name ending `_analysis`.
    pub fn analysis_copy_files(
        &mut self,
        calibre: &str,
        result: Option<&serde_json::Value>,
        required_files: &[(&str, &[u8])],
    ) -> io::Result<(std::ffi::OsString, NamedFiles)> {
        self.flush()?;
        let mut name = self.copy_name(calibre);
        name.push("_analysis");
        let session = self.session_text(Some(utc(SystemTime::now())), result, false)?;
        let mut files = vec![(take::RECORD.to_string(), session.into_bytes())];
        let mut names: Vec<_> = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != take::RECORD && !is_sound(n))
            .collect();
        names.sort();
        for n in names {
            if required_files
                .iter()
                .any(|(r, _)| r.eq_ignore_ascii_case(&n))
            {
                continue;
            }
            let bytes = fs::read(self.dir.join(&n))?;
            files.push((n, bytes));
        }
        files.extend(
            required_files
                .iter()
                .map(|&(n, b)| (n.to_string(), b.to_vec())),
        );
        Ok((name, files))
    }

    /// Copy the recording so far into a new folder under `parent`, named
    /// after the start time, the watch and `calibre` (either left out when
    /// empty), keeping on recording here. `result` is saved with the
    /// settings as the readings at the time of the copy. Returns the copy.
    ///
    /// Saved again into the same place, only what changed is written: an
    /// audio file closed by a pause that is already there at its full
    /// length is left alone, so Save Again copies the new sound, the open
    /// file, clock.csv and session.json.
    pub fn save_copy(
        &mut self,
        parent: &Path,
        calibre: &str,
        result: Option<&serde_json::Value>,
    ) -> io::Result<PathBuf> {
        self.save_copy_with_files(parent, calibre, result, &[])
    }

    /// Save the sound and required companion files as one recoverable update.
    /// Names must be single file names, distinct from the recorder's own files.
    /// All changed files are staged before replacing the previous copy. An I/O
    /// failure during installation rolls back earlier replacements; if rollback
    /// itself fails, the error names the retained recovery directory.
    /// This is not a process-crash or power-loss atomicity guarantee.
    pub fn save_copy_with_files(
        &mut self,
        parent: &Path,
        calibre: &str,
        result: Option<&serde_json::Value>,
        required_files: &[(&str, &[u8])],
    ) -> io::Result<PathBuf> {
        self.save_copy_using(parent, calibre, result, required_files, |_, _| Ok(()))
    }

    fn save_copy_using(
        &mut self,
        parent: &Path,
        calibre: &str,
        result: Option<&serde_json::Value>,
        required_files: &[(&str, &[u8])],
        before: impl FnMut(SaveStep, &Path) -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        let copy = self
            .begin_save(parent, calibre, result, required_files)?
            .write_using(before)?;
        self.note_saved(&copy);
        Ok(copy.dir)
    }

    /// The quick part of [`Recorder::save_copy_with_files`]: check where
    /// the copy goes and note the recording as it stands now, for
    /// [`PendingSave::write`] to copy on another thread while recording
    /// carries on here. The copy holds the sound up to now and no more:
    /// the open audio file up to its frames now, with its header as it is
    /// now, and clock.csv up to its last line now.
    pub fn begin_save(
        &mut self,
        parent: &Path,
        calibre: &str,
        result: Option<&serde_json::Value>,
        required_files: &[(&str, &[u8])],
    ) -> io::Result<PendingSave> {
        self.begin_save_ending(parent, calibre, result, required_files, true)
    }

    /// [`Recorder::begin_save`] for a take whose copy is kept up to date
    /// every few seconds while it records: the clock log takes no line of
    /// its own for the save, so it keeps the cadence it has unsaved (a
    /// save's own line would add one every few seconds), and the copy's
    /// log ends at its last line on that cadence. Into the copy saved
    /// before, the hour still being written gets only the sound since.
    pub fn begin_stream_save(
        &mut self,
        parent: &Path,
        calibre: &str,
        result: Option<&serde_json::Value>,
        required_files: &[(&str, &[u8])],
    ) -> io::Result<PendingSave> {
        self.begin_save_ending(parent, calibre, result, required_files, false)
    }

    fn begin_save_ending(
        &mut self,
        parent: &Path,
        calibre: &str,
        result: Option<&serde_json::Value>,
        required_files: &[(&str, &[u8])],
        log_end: bool,
    ) -> io::Result<PendingSave> {
        let mut names = std::collections::BTreeSet::new();
        for &(name, _) in required_files {
            let mut parts = Path::new(name).components();
            if !matches!(parts.next(), Some(std::path::Component::Normal(_)))
                || parts.next().is_some()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || [take::RECORD, take::CLOCK_LOG].contains(&name.to_ascii_lowercase().as_str())
                || is_sound(name)
                || self.dir.join(name).exists()
                || !names.insert(name.to_ascii_lowercase())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid or reserved companion file name: {name}"),
                ));
            }
        }
        // The copy's clock log ends where its sound does.
        if log_end {
            self.log_last_block()?;
        }
        let name = self
            .saved_name
            .clone()
            .unwrap_or_else(|| self.copy_name(calibre));
        fs::create_dir_all(parent)?;
        // Resolve identities only for the overlap check. Keep the caller's
        // spelling for filesystem work and paths returned or shown to them.
        let canonical_parent = fs::canonicalize(parent)?;
        let dest = parent.join(&name);
        let source = fs::canonicalize(&self.dir)?;
        let resolved = if dest.exists() {
            fs::canonicalize(&dest)?
        } else {
            canonical_parent.join(&name)
        };
        if resolved.starts_with(&source) || source.starts_with(&resolved) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the saved copy must be separate from the live recording folder",
            ));
        }
        self.flush()?;
        if self.reap(false) {
            self.write_session(None, None)?;
        }
        let session = self.session_text(Some(utc(SystemTime::now())), result, true)?;
        let mut files = Vec::new();
        // Enumerate the hours explicitly: a missing one must not disappear
        // from read_dir and turn an incomplete take into a success.
        for n in &self.segments {
            let mut f = self.pending_segment(n)?;
            // Only a streamed save grows the copy in place: Save and Save
            // Again stage every file, as before.
            if !log_end {
                f.grow_from = self.grown_from(&dest.join(f.name()), &f);
            }
            files.push(f);
        }
        // The other files, but any sound file the hours left (a WAV beside
        // its FLAC, a FLAC being written).
        let mut sources = std::collections::BTreeSet::from([self.dir.join(take::CLOCK_LOG)]);
        for e in fs::read_dir(&self.dir)? {
            let e = e?;
            let n = e.file_name();
            if e.file_type()?.is_file()
                && n != take::RECORD
                && n != STREAMED_TO
                && !is_sound(&n.to_string_lossy())
            {
                sources.insert(e.path());
            }
        }
        for source in sources {
            let metadata = regular_file(&source)?;
            // A file still being written only grows, but each flush writes
            // the open audio file's header again: keep its start as it is
            // now.
            let mut head = Vec::new();
            let opened = File::open(&source)?;
            (&opened)
                .take(SNAPSHOT_HEAD.min(metadata.len()))
                .read_to_end(&mut head)?;
            files.push(PendingFile {
                source,
                len: metadata.len(),
                head,
                opened: Some(opened),
                held: None,
                as_flac: false,
                open_sound: false,
                grow_from: None,
            });
        }
        let mut written = vec![(take::RECORD.to_string(), session.into_bytes())];
        written.extend(
            required_files
                .iter()
                .map(|&(n, b)| (n.to_string(), b.to_vec())),
        );
        Ok(PendingSave {
            parent: parent.to_path_buf(),
            dest,
            name,
            files,
            written,
        })
    }

    /// Whether `name` is the hour still being written.
    fn is_open(&self, name: &str) -> bool {
        self.writer.is_some() && self.segments.last().is_some_and(|s| s == name)
    }

    /// The hour `name` as a save copies it. The open one is copied as it
    /// is now. A closed one never changes again, and is held open so that
    /// keeping it as FLAC meanwhile cannot take it from the save: a WAV
    /// the copy holds as FLAC, a FLAC as it is (the WAV may have become
    /// one since its name was noted).
    fn pending_segment(&self, name: &str) -> io::Result<PendingFile> {
        if self.is_open(name) {
            let source = self.dir.join(name);
            let len = regular_file(&source)?.len();
            let mut head = Vec::new();
            let opened = File::open(&source)?;
            (&opened)
                .take(SNAPSHOT_HEAD.min(len))
                .read_to_end(&mut head)?;
            return Ok(PendingFile {
                source,
                len,
                head,
                opened: Some(opened),
                held: None,
                as_flac: false,
                open_sound: true,
                grow_from: None,
            });
        }
        let (source, held, len) = open_closed_hour(&self.dir.join(name))?;
        Ok(PendingFile {
            as_flac: crate::flac::is_wav(&source),
            source,
            len,
            head: Vec::new(),
            opened: None,
            held: Some(held),
            open_sound: false,
            grow_from: None,
        })
    }

    /// Where the copy at `to` of the hour still being written, `f`, already
    /// holds what the last save left there: its length then, when the
    /// hour has only grown since and that length is past the start a save
    /// writes again.
    fn grown_from(&self, to: &Path, f: &PendingFile) -> Option<u64> {
        let (path, len) = self.grown.as_ref()?;
        (f.open_sound && path == to && *len >= f.head.len() as u64 && *len <= f.len).then_some(*len)
    }

    /// The copy saved last moved from `from` to `to` (filed under a
    /// watch): Save Again saves into it there, and a take recording into
    /// its folder grows it there.
    pub fn copy_moved(&mut self, from: &Path, to: &Path) {
        if let Some(name) = to.file_name() {
            self.saved_name = Some(name.to_owned());
        }
        if let Some((path, _)) = self.grown.as_mut() {
            if let Ok(rest) = path.strip_prefix(from) {
                *path = to.join(rest);
            }
        }
    }

    /// Note a copy [`PendingSave::write`] wrote, so Save Again saves into
    /// it.
    pub fn note_saved(&mut self, copy: &SavedCopy) {
        self.saved_name = Some(copy.name.clone());
        self.grown = copy.grown.clone();
    }

    /// Close the files, saving `result` (the readings at the end) with the
    /// settings. Returns the folder.
    ///
    /// Waits for the hours being kept as FLAC, so the folder is left as it
    /// stays. An hour still being written (no pause since it began) is
    /// left a WAV.
    pub fn finish(mut self, result: Option<serde_json::Value>) -> io::Result<PathBuf> {
        if let Some(w) = self.writer.take() {
            w.finalize().map_err(io_err)?;
        }
        self.reap(true);
        self.log_last_block()?;
        self.write_session(Some(utc(SystemTime::now())), result.as_ref())?;
        if self.lock.take().is_some() {
            let _ = fs::remove_file(lock_path(&self.dir));
        }
        Ok(self.dir.clone())
    }

    /// [`Recorder::finish`], keeping the hour still being written as FLAC
    /// too, so every hour of the take is: for a recording that ends here,
    /// as `timegrapher record` does. Waits for its conversion (about 20 s
    /// for a whole hour at 48 kHz). An hour that cannot be converted stays
    /// a whole WAV.
    pub fn finish_as_flac(mut self, result: Option<serde_json::Value>) -> io::Result<PathBuf> {
        self.close_segment()?;
        self.finish(result)
    }
}

/// A recorder let go without [`Recorder::finish`] (a new session, a
/// fault, the app closing) waits for the hours still being kept as FLAC,
/// as `finish` does, and writes `session.json` naming them. Left to run
/// on, a conversion would delete a WAV the folder's `session.json` still
/// names, after the app had moved on from the folder.
impl Drop for Recorder {
    fn drop(&mut self) {
        if self.reap(true) {
            let _ = self.write_session(None, None);
        }
    }
}

/// Files by name, with their bytes.
pub type NamedFiles = Vec<(String, Vec<u8>)>;

/// How much of the start of a file still being written a save keeps as
/// it was when the save began: the open audio file's header lies within
/// it, and every flush writes that header again.
const SNAPSHOT_HEAD: u64 = 4096;

/// A copy of a recording as it stood at [`Recorder::begin_save`], to write
/// on any thread with [`PendingSave::write`] while the recording carries
/// on.
#[derive(Debug)]
pub struct PendingSave {
    parent: PathBuf,
    dest: PathBuf,
    name: std::ffi::OsString,
    files: Vec<PendingFile>,
    /// session.json and the companion files, written whole.
    written: NamedFiles,
}

#[derive(Debug)]
struct PendingFile {
    source: PathBuf,
    /// Its length at the start of the save: no more is copied.
    len: u64,
    /// Its first bytes then, copied instead of what is there by the time
    /// of the copy. Empty for a closed audio file.
    head: Vec<u8>,
    /// The file `head` was read from, held open from the start of the save:
    /// the open hour may be kept as FLAC, its WAV gone, before the copy (#1409).
    opened: Option<File>,
    /// A closed audio file, held open from the start of the save; left
    /// alone when the copy already holds it whole.
    held: Option<File>,
    /// A closed WAV, which the copy holds as FLAC.
    as_flac: bool,
    /// The hour still being written.
    open_sound: bool,
    /// Its copy's length after the last save into the same copy, when
    /// only the sound since needs adding ([`Recorder::grown_from`]).
    grow_from: Option<u64>,
}

impl PendingFile {
    /// Its name in the copy.
    fn name(&self) -> std::ffi::OsString {
        let file = Path::new(self.source.file_name().expect("recording file"));
        match self.as_flac {
            true => crate::flac::flac_path(file).into_os_string(),
            false => file.as_os_str().to_os_string(),
        }
    }

    /// Whether `there`, in the copy already, holds it whole: a closed audio
    /// file at its full length, or with the same samples as the WAV it is
    /// the FLAC of. A file still being written is always copied again.
    fn already(&self, there: &Path, len: u64) -> bool {
        match (&self.held, self.as_flac) {
            (Some(held), true) => crate::flac::same_samples_file(held, there).unwrap_or(false),
            (Some(_), false) => len == self.len,
            (None, _) => false,
        }
    }

    /// The file to copy from: held open since the save began, else by name.
    fn reader(&self) -> io::Result<File> {
        match &self.opened {
            Some(f) => f.try_clone(),
            None => File::open(&self.source),
        }
    }

    /// Write it to `to`, as it was at the start of the save.
    fn copy_to(&self, to: &Path) -> io::Result<()> {
        let Some(held) = &self.held else {
            return copy_as_it_was(self, to);
        };
        if self.as_flac {
            crate::flac::encode_wav_file(held, to).map_err(io::Error::other)?;
            return Ok(());
        }
        let mut from = held;
        from.seek(io::SeekFrom::Start(0))?;
        let copied = io::copy(&mut from.take(self.len), &mut File::create(to)?)?;
        if copied != self.len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("{} got shorter during the save", self.source.display()),
            ));
        }
        Ok(())
    }
}

/// `name` with `.flac` in place of its extension.
fn as_flac(name: &str) -> String {
    crate::flac::flac_path(Path::new(name))
        .to_string_lossy()
        .into_owned()
}

/// The metadata of `p`, which must be a regular file and no link.
fn regular_file(p: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(p)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", p.display()),
        ));
    }
    Ok(metadata)
}

/// A copy [`PendingSave::write`] wrote.
#[derive(Debug, Clone)]
pub struct SavedCopy {
    /// The copy's folder, as the caller spelled its parent.
    pub dir: PathBuf,
    name: std::ffi::OsString,
    /// The hour still being written as copied, and its length there.
    grown: Option<(PathBuf, u64)>,
}

impl PendingSave {
    /// Where the copy goes.
    pub fn dest(&self) -> &Path {
        &self.dest
    }

    /// Write the copy: stage every changed file, then put them in place
    /// together, keeping the copy saved before until they all are (see
    /// [`Recorder::save_copy_with_files`]). Pass the result to
    /// [`Recorder::note_saved`].
    pub fn write(self) -> io::Result<SavedCopy> {
        self.write_using(|_, _| Ok(()))
    }

    fn write_using(
        self,
        mut before: impl FnMut(SaveStep, &Path) -> io::Result<()>,
    ) -> io::Result<SavedCopy> {
        let mut update = SaveUpdate::new(&self.parent, &self.dest)?;
        let mut grown = None;
        for f in &self.files {
            let file = f.name();
            let to = self.dest.join(&file);
            if f.open_sound {
                grown = Some((to.clone(), f.len));
            }
            if f.grow_from
                .is_some_and(|from| grow_in_place(f, from, &to).unwrap_or(false))
            {
                continue;
            }
            let there = match fs::symlink_metadata(&to) {
                Ok(m) if !m.is_file() => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("{} is not a regular file", to.display()),
                    ))
                }
                Ok(m) => f.already(&to, m.len()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => false,
                Err(e) => return Err(e),
            };
            if there {
                continue;
            }
            before(SaveStep::Stage, &to)?;
            f.copy_to(&update.staged.join(&file))?;
        }
        for (file, bytes) in &self.written {
            before(SaveStep::Stage, &self.dest.join(file))?;
            fs::write(update.staged.join(file), bytes)?;
        }
        update.install(&mut before)?;
        // An hour the copy held as a WAV before, and holds as FLAC now.
        for f in &self.files {
            let file = Path::new(&f.name()).to_path_buf();
            if f.held.is_some() && !crate::flac::is_wav(&file) {
                let wav = self.dest.join(file.with_extension("wav"));
                if fs::symlink_metadata(&wav).is_ok_and(|m| m.is_file()) {
                    let _ = fs::remove_file(wav);
                }
            }
        }
        Ok(SavedCopy {
            dir: self.dest,
            name: self.name,
            grown,
        })
    }
}

/// Add to `to`, the copy of the hour still being written as the last save
/// left it (`from` bytes long), the sound since: the bytes from there up
/// to the hour's length at the start of the save, then its first bytes as
/// they were then, which hold its header with the new length. False, with
/// `to` untouched, where the copy is no longer as that save left it: the
/// whole hour is then copied as on a first save.
///
/// Written in place, not staged: the sound already in the copy stays put
/// and a few seconds' more is added, where staging would copy the whole
/// hour again every few seconds. Stopped part way, the copy's header still
/// gives the length before, so it reads as it did.
fn grow_in_place(f: &PendingFile, from: u64, to: &Path) -> io::Result<bool> {
    let mut out = open_in_place(to)?;
    if out.metadata()?.len() != from {
        return Ok(false);
    }
    let mut source = f.reader()?;
    source.seek(io::SeekFrom::Start(from))?;
    out.seek(io::SeekFrom::Start(from))?;
    let rest = f.len - from;
    let copied = io::copy(&mut source.take(rest), &mut out)?;
    if copied != rest {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("{} got shorter during the save", f.source.display()),
        ));
    }
    // The new sound is on disk before the header that counts it: a power
    // cut between the two leaves the header giving the length before.
    out.sync_data()?;
    out.seek(io::SeekFrom::Start(0))?;
    out.write_all(&f.head)?;
    out.sync_data()?;
    Ok(true)
}

/// Copy `f` as it was at the start of the save: its first bytes as they
/// were then, and the rest up to its length then.
fn copy_as_it_was(f: &PendingFile, to: &Path) -> io::Result<()> {
    let mut out = File::create(to)?;
    out.write_all(&f.head)?;
    let mut from = f.reader()?;
    from.seek(io::SeekFrom::Start(f.head.len() as u64))?;
    let rest = f.len - f.head.len() as u64;
    let copied = io::copy(&mut from.take(rest), &mut out)?;
    if copied != rest {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("{} got shorter during the save", f.source.display()),
        ));
    }
    Ok(())
}

/// Narrow fault-injection points for the save transaction's filesystem work.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SaveStep {
    Stage,
    Install,
    Restore,
}

struct SaveUpdate {
    root: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    dest: PathBuf,
    retain: bool,
}

impl SaveUpdate {
    fn new(parent: &Path, dest: &Path) -> io::Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = loop {
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root = parent.join(format!(".timegrapher-save-{}-{id}", std::process::id()));
            match fs::create_dir(&root) {
                Ok(()) => break root,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        };
        let update = Self {
            staged: root.join("staged"),
            backup: root.join("backup"),
            root,
            dest: dest.to_path_buf(),
            retain: false,
        };
        fs::create_dir(&update.staged)?;
        fs::create_dir(&update.backup)?;
        Ok(update)
    }

    fn install(
        &mut self,
        before: &mut impl FnMut(SaveStep, &Path) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut files: Vec<_> = fs::read_dir(&self.staged)?
            .map(|e| e.map(|e| e.file_name()))
            .collect::<io::Result<_>>()?;
        files.sort();
        // Reject directory/symlink collisions before touching the previous copy.
        for file in &files {
            match fs::symlink_metadata(self.dest.join(file)) {
                Ok(m) if !m.is_file() => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("{} is not a regular file", self.dest.join(file).display()),
                    ))
                }
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        let created = match fs::create_dir(&self.dest) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && self.dest.is_dir() => false,
            Err(e) => return Err(e),
        };
        let mut changed = Vec::new();
        let result = (|| {
            for file in files {
                let to = self.dest.join(&file);
                let backup = self.backup.join(&file);
                let had_old = match settled(|| fs::rename(&to, &backup), held_open) {
                    Ok(()) => true,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => false,
                    Err(e) => return Err(e),
                };
                changed.push((file.clone(), had_old));
                before(SaveStep::Install, &to)?;
                settled(|| fs::rename(self.staged.join(&file), &to), held_open)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let mut rollback_error = None;
            for (file, had_old) in changed.into_iter().rev() {
                let to = self.dest.join(&file);
                let restored = (|| {
                    before(SaveStep::Restore, &to)?;
                    match settled(|| fs::remove_file(&to), held_open) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                    if had_old {
                        settled(|| fs::rename(self.backup.join(&file), &to), held_open)?;
                    }
                    Ok(())
                })();
                if let Err(e) = restored {
                    rollback_error = Some(e);
                }
            }
            if created {
                let _ = fs::remove_dir(&self.dest);
            }
            if let Some(rollback) = rollback_error {
                self.retain = true;
                return Err(io::Error::other(format!(
                    "{error}; rollback failed: {rollback}; recovery files retained in {}",
                    self.root.display()
                )));
            }
            return Err(error);
        }
        Ok(())
    }
}

impl Drop for SaveUpdate {
    fn drop(&mut self) {
        if !self.retain {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

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

/// A new folder `name` under `parent`, or `name_2`, `name_3` and on when
/// that is taken (a take started in the same second as another, with the
/// same watch and position): never one that already exists, whose files a
/// new recording would overwrite.
fn new_dir(parent: &Path, name: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(parent)?;
    for n in 1.. {
        let dir = match n {
            1 => parent.join(name),
            n => parent.join(format!("{name}_{n}")),
        };
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// `2026-10-08T21-04-05` for a folder name, from a [`utc`] time.
fn stamp_of(utc: &str) -> String {
    utc[..19]
        .chars()
        .map(|c| if c == ':' { '-' } else { c })
        .collect()
}

fn safe_name(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `2026-10-08T21:04:05.123Z` for a system time.
pub fn utc(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        d.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_dates() {
        let t = UNIX_EPOCH + Duration::from_millis(1_791_493_065_123);
        assert_eq!(utc(t), "2026-10-08T20:57:45.123Z");
        assert_eq!(utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let leap = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(&utc(leap)[..10], "2000-02-29");
    }

    /// A wider integer sample keeps at 16 bits rounded half away from
    /// zero, the top in range; 16 bits or fewer keep as they are.
    #[test]
    fn a_wide_sample_rounds_to_the_nearest_16_bit_step() {
        assert_eq!(stored_int(127, 24), 0);
        assert_eq!(stored_int(128, 24), 1);
        assert_eq!(stored_int(-128, 24), -1);
        assert_eq!(stored_int(-129, 24), -1);
        assert_eq!(stored_int(0x7f_ff_ff, 24), 32_767);
        assert_eq!(stored_int(-0x80_00_00, 24), -32_768);
        assert_eq!(stored_int(i32::MAX, 32), 32_767);
        assert_eq!(stored_int(i32::MIN, 32), -32_768);
        assert_eq!(stored_int(0x12_34 << 8, 24), 0x12_34);
        assert_eq!(stored_int(-5, 16), -5);
        assert_eq!(stored_int(100, 8), 100);
    }

    /// A 24-bit microphone's take is kept at 16 bits, each sample the
    /// sound rounded to the nearest 16-bit step (#1176).
    #[test]
    fn a_24_bit_microphone_is_kept_at_16_bits() {
        let parent = std::env::temp_dir().join(format!("tg-rec-16-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        let info = SessionInfo {
            watch: "Depth".into(),
            position: "DU".into(),
            lift_deg: 52.0,
            bph: None,
            device: "24-bit".into(),
            sample_rate: 8000,
            bits: 24,
            software: "test".into(),
        };
        let mut r = Recorder::start(&parent, info).unwrap();
        let samples = vec![0.0, 0.5, -0.5, 1.0, -1.0, 1.4e-5, -2.0e-5, 0.123_456];
        r.write(&Block {
            samples: samples.clone(),
            at: SystemTime::now(),
        })
        .unwrap();
        let wav = r.dir().join("audio-001.wav");
        r.flush().unwrap();
        let mut read = hound::WavReader::open(&wav).unwrap();
        assert_eq!(read.spec().bits_per_sample, 16);
        let kept: Vec<i32> = read.samples::<i32>().map(Result::unwrap).collect();
        let want: Vec<i32> = samples
            .iter()
            .map(|s| (s * 32_767.0).round() as i32)
            .collect();
        assert_eq!(kept, want);
        drop(r);
        let _ = fs::remove_dir_all(&parent);
    }

    /// A folder being recorded into reads as in use, to this process and
    /// any other; once finished, or its recorder gone without finishing,
    /// it doesn't.
    #[test]
    fn a_recording_folder_is_in_use_only_while_recorded() {
        let parent = std::env::temp_dir().join(format!("tg-rec-lock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        let info = || SessionInfo {
            watch: "Lock".into(),
            position: "DU".into(),
            lift_deg: 52.0,
            bph: None,
            device: "test".into(),
            sample_rate: 8000,
            bits: 16,
            software: "test".into(),
        };
        let r = Recorder::start(&parent, info()).unwrap();
        let dir = r.dir().to_path_buf();
        assert!(in_use(&dir));
        let done = r.finish(None).unwrap();
        assert!(!in_use(&done));
        // The lock lives beside the folder, never in it, and goes on finish.
        assert!(!lock_path(&done).exists());
        assert!(fs::read_dir(&done).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".lock")));
        let _ = fs::remove_dir_all(&done);
        // Dropped without finishing (a crash, a recording fault): let go.
        let r = Recorder::start(&parent.join("again"), info()).unwrap();
        let dir = r.dir().to_path_buf();
        drop(r);
        assert!(!in_use(&dir));
        remove_stale_lock(&dir);
        assert!(!lock_path(&dir).exists());
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn two_takes_started_in_one_second_keep_their_own_folders() {
        // A Quick Listen stopped and started again within the second: the
        // first take's folder and files stay as they were.
        let parent = std::env::temp_dir().join(format!("tg-rec-same-{}", std::process::id()));
        let _ = fs::remove_dir_all(&parent);
        let info = || SessionInfo {
            watch: "Same".into(),
            position: "DU".into(),
            lift_deg: 52.0,
            bph: None,
            device: "test".into(),
            sample_rate: 8000,
            bits: 16,
            software: "test".into(),
        };
        let block = |at| Block {
            samples: vec![0.25; 8000],
            at,
        };
        let (first, second) = loop {
            let mut a = Recorder::start(&parent, info()).unwrap();
            a.write(&block(SystemTime::now())).unwrap();
            let a = a.finish(None).unwrap();
            let kept: Vec<(std::ffi::OsString, Vec<u8>)> = fs::read_dir(&a)
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    (e.file_name(), fs::read(e.path()).unwrap())
                })
                .collect();
            let b = Recorder::start(&parent, info()).unwrap();
            if a.file_name().unwrap().to_string_lossy()[..19] == stamp_of(b.started_utc()) {
                break ((a, kept), b);
            }
            drop(b);
            let _ = fs::remove_dir_all(&parent);
        };
        let ((a, kept), b) = (first, second);
        assert_ne!(a, b.dir());
        assert_eq!(
            b.dir().file_name(),
            Some(format!("{}_2", a.file_name().unwrap().to_string_lossy()).as_ref())
        );
        for (name, bytes) in &kept {
            assert_eq!(&fs::read(a.join(name)).unwrap(), bytes, "{name:?}");
        }
        drop(b);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn records_audio_clock_and_settings() {
        let parent = std::env::temp_dir().join(format!("tg-rec-{}", std::process::id()));
        let info = SessionInfo {
            watch: "Test 3235".into(),
            position: "DU".into(),
            lift_deg: 52.0,
            bph: None,
            device: "test".into(),
            sample_rate: 8000,
            bits: 16,
            software: "test".into(),
        };
        let mut r = Recorder::start(&parent, info).unwrap();
        let t0 = SystemTime::now();
        for k in 0..25 {
            let samples: Vec<f32> = (0..8000).map(|i| ((i + k) % 100) as f32 / 200.0).collect();
            r.write(&Block {
                samples,
                at: t0 + Duration::from_secs(k),
            })
            .unwrap();
        }
        // Readable while it carries on, after a flush, as a report cuts it.
        r.flush().unwrap();
        let now = crate::excerpt::cut(&r.segment_paths(), 20.0, 300.0).unwrap();
        assert_eq!(now.frames(), 5 * 8000);
        r.note("turned to DD").unwrap();
        let dir = r.finish(Some(serde_json::json!({"rate": 1.0}))).unwrap();
        let audio = crate::audio::load(&dir.join("audio-001.wav")).unwrap();
        assert_eq!(audio.samples.len(), 25 * 8000);
        assert!((audio.samples[3] - 3.0 / 200.0).abs() < 1e-4);
        let clock = fs::read_to_string(dir.join("clock.csv")).unwrap();
        let pairs = crate::clock::parse_log(&clock, 8000, 2).unwrap();
        // A line a second over the first 3 s of sound (the blocks at 0, 1
        // and 2 s), then every 10 s (12 and 22 s), and the end at 24 s.
        assert_eq!(pairs.len(), 6);
        assert!((pairs[1].0 - 2.0).abs() < 1e-9, "{:?}", pairs[1]);
        assert!((pairs[3].0 - 13.0).abs() < 1e-9, "{:?}", pairs[3]);
        let session = fs::read_to_string(dir.join("session.json")).unwrap();
        assert!(session.contains("\"position\": \"DU\""));
        // Never paused: no breaks key at all, as before there were breaks.
        assert!(!session.contains("breaks"), "{session}");
        assert!(session.contains("turned to DD"));
        assert!(dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("_Test_3235_DU"));
        fs::remove_dir_all(&parent).unwrap();
    }

    pub(super) fn save_fixture(name: &str) -> (Recorder, PathBuf) {
        let base = std::env::temp_dir().join(format!("tg-save-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let info = SessionInfo {
            watch: "Save test".into(),
            position: "DU".into(),
            lift_deg: 52.0,
            bph: None,
            device: "test".into(),
            sample_rate: 8000,
            bits: 16,
            software: "test".into(),
        };
        let mut r = Recorder::start(&base.join("live"), info).unwrap();
        append_second(&mut r, 1);
        (r, base)
    }

    /// Wait for the closed hours to be kept as FLAC, and `session.json` to
    /// name them so.
    fn settle(r: &mut Recorder) {
        if r.reap(true) {
            r.write_session(None, None).unwrap();
        }
    }

    pub(super) fn append_second(r: &mut Recorder, second: u64) {
        r.write(&Block {
            samples: vec![0.1; 8000],
            at: UNIX_EPOCH + Duration::from_secs(second),
        })
        .unwrap();
    }

    fn saved_bytes(dir: &Path) -> std::collections::BTreeMap<std::ffi::OsString, Vec<u8>> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name(), fs::read(e.path()).unwrap())
            })
            .collect()
    }

    /// A take kept up to date while it records: each save into the copy
    /// adds the sound since to the hour still being written, in place,
    /// and the copy reads as the recording did at each save. The clock log
    /// takes no line for the saves, so it reads as it would unsaved.
    #[test]
    fn a_streamed_save_adds_the_sound_since_in_place() {
        let (mut r, base) = save_fixture("streamed");
        let parent = base.join("saved");
        let mut lines = Vec::new();
        // The copy's file, by its identity on disk: the same file grown,
        // not a new one put in its place.
        let mut file_id = None;
        for second in 2..=20 {
            append_second(&mut r, second);
            let copy = r
                .begin_stream_save(&parent, "", None, &[])
                .unwrap()
                .write()
                .unwrap();
            r.note_saved(&copy);
            let wav = copy.dir.join("audio-001.wav");
            let (path, len) = r.grown.clone().expect("the open hour noted");
            assert_eq!(
                (path, len),
                (wav.clone(), fs::metadata(&wav).unwrap().len())
            );
            let saved = crate::audio::load(&wav).unwrap();
            assert_eq!(saved.samples.len() as u64, second * 8000, "at {second} s");
            // The same bytes as the recording's own hour at the save.
            r.flush().unwrap();
            let live = fs::read(r.dir().join("audio-001.wav")).unwrap();
            assert_eq!(fs::read(&wav).unwrap(), live, "at {second} s");
            let id = file_identity(&wav);
            assert_eq!(
                *file_id.get_or_insert(id),
                id,
                "grown in place at {second} s"
            );
            lines.push(fs::read_to_string(copy.dir.join("clock.csv")).unwrap());
        }
        // The copy's log is the recording's own, lines on its cadence only:
        // a line a second over the first 3 s, then every 10 s.
        let clock = lines.last().unwrap();
        let pairs = crate::clock::parse_log(clock, 8000, 2).unwrap();
        assert_eq!(pairs.len(), 4, "{clock}");
        // A copy changed behind the save's back is copied whole again.
        let wav = base
            .join("saved")
            .join(r.saved_name.clone().unwrap())
            .join("audio-001.wav");
        let mut f = fs::OpenOptions::new().append(true).open(&wav).unwrap();
        f.write_all(&[0; 7]).unwrap();
        drop(f);
        append_second(&mut r, 21);
        let copy = r
            .begin_stream_save(&parent, "", None, &[])
            .unwrap()
            .write()
            .unwrap();
        r.note_saved(&copy);
        r.flush().unwrap();
        assert_eq!(
            fs::read(&wav).unwrap(),
            fs::read(r.dir().join("audio-001.wav")).unwrap()
        );
        if let Some(id) = file_identity(&wav) {
            assert_ne!(Some(id), file_id.flatten(), "copied whole");
        }
        fs::remove_dir_all(&base).unwrap();
    }

    /// A copy moved (filed under a watch) and noted with
    /// [`Recorder::copy_moved`] goes on growing in place where it is, and
    /// Save Again saves into it there under its new name.
    #[test]
    fn a_moved_copy_goes_on_growing_where_it_is() {
        let (mut r, base) = save_fixture("moved");
        let stream = |r: &mut Recorder, parent: &Path| {
            let copy = r
                .begin_stream_save(parent, "", None, &[])
                .unwrap()
                .write()
                .unwrap();
            r.note_saved(&copy);
            copy.dir
        };
        append_second(&mut r, 2);
        let from = stream(&mut r, &base.join("unsorted"));
        let to = base.join("watch").join("moved take");
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::rename(&from, &to).unwrap();
        r.copy_moved(&from, &to);
        let wav = to.join("audio-001.wav");
        let id = file_identity(&wav);
        append_second(&mut r, 3);
        assert_eq!(stream(&mut r, to.parent().unwrap()), to);
        assert_eq!(file_identity(&wav), id, "grown in place");
        let saved = crate::audio::load(&wav).unwrap();
        assert_eq!(saved.samples.len(), 3 * 8000);
        fs::remove_dir_all(&base).unwrap();
    }

    /// Which file `p` is on disk (a new file put in its place is
    /// another), where the system says: Windows may give a file put in
    /// another's place the other's creation time.
    fn file_identity(p: &Path) -> Option<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(fs::metadata(p).unwrap().ino())
        }
        #[cfg(not(unix))]
        {
            let _ = p;
            None
        }
    }

    /// A save copies the recording as it stood when it began, though the
    /// recording carries on before the copy is written: more sound, a
    /// clock line, and the file it was writing closed with its full length
    /// in its header.
    #[test]
    fn a_save_copies_the_recording_as_it_stood_when_it_began() {
        let (mut r, base) = save_fixture("as-it-stood");
        append_second(&mut r, 2);
        let pending = r.begin_save(&base.join("saved"), "", None, &[]).unwrap();
        let clock_then = fs::read(r.dir().join("clock.csv")).unwrap();
        for second in 3..=14 {
            append_second(&mut r, second);
        }
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 20);
        r.flush().unwrap();
        let copy = pending.write().unwrap();
        r.note_saved(&copy);
        let first = crate::audio::load(&copy.dir.join("audio-001.wav")).unwrap();
        assert_eq!(first.samples.len(), 2 * 8000);
        assert!(!copy.dir.join("audio-002.wav").exists());
        assert_eq!(fs::read(copy.dir.join("clock.csv")).unwrap(), clock_then);
        // Save Again brings the same copy up to now.
        let again = r.save_copy(&base.join("saved"), "", None).unwrap();
        assert_eq!(again, copy.dir);
        // Closed by the pause since, the first hour is saved as FLAC, in
        // place of the WAV saved before.
        let first = crate::audio::load(&again.join("audio-001.flac")).unwrap();
        assert_eq!(first.samples.len(), 14 * 8000);
        assert!(!again.join("audio-001.wav").exists());
        let second = crate::audio::load(&again.join("audio-002.wav")).unwrap();
        assert_eq!(second.samples.len(), 8000);
        assert_eq!(
            fs::read(again.join("clock.csv")).unwrap(),
            fs::read(r.dir().join("clock.csv")).unwrap()
        );
        drop(r);
        let _ = fs::remove_dir_all(&base);
    }

    /// A save begun while an hour is being written still copies it when a
    /// pause keeps that hour as FLAC, its WAV gone, before the copy (#1409).
    #[test]
    fn a_save_copies_the_open_hour_though_it_is_kept_as_flac_before_the_copy() {
        let (mut r, base) = save_fixture("open-hour-converted");
        append_second(&mut r, 2);
        let pending = r.begin_save(&base.join("saved"), "", None, &[]).unwrap();
        for second in 3..=14 {
            append_second(&mut r, second);
        }
        r.pause().unwrap();
        let t = std::time::Instant::now();
        while !r.converting.iter().all(|(_, job)| job.is_finished()) {
            assert!(t.elapsed().as_secs() < 60, "the hour was not kept as FLAC");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!r.dir().join("audio-001.wav").exists());
        assert!(r.dir().join("audio-001.flac").exists());
        let copy = pending.write().unwrap();
        r.note_saved(&copy);
        let first = crate::audio::load(&copy.dir.join("audio-001.wav")).unwrap();
        assert_eq!(first.samples.len(), 2 * 8000);
        drop(r);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn failed_staging_and_installation_preserve_the_previous_copy_and_retry() {
        let files: &[(&str, &[u8])] = &[
            ("session.toml", b"old manifest"),
            ("positions-test.json", b"old results"),
        ];
        for step in [SaveStep::Stage, SaveStep::Install] {
            for failed_file in [
                "audio-001.wav",
                "clock.csv",
                "session.json",
                "session.toml",
                "positions-test.json",
            ] {
                let (mut r, base) = save_fixture(&format!("{step:?}-{failed_file}"));
                let parent = base.join("saved");
                let dest = r.save_copy_with_files(&parent, "", None, files).unwrap();
                fs::write(dest.join("owners-note.txt"), "keep this").unwrap();
                let old = saved_bytes(&dest);
                append_second(&mut r, 2);
                let newer: &[(&str, &[u8])] = &[
                    ("session.toml", b"new manifest"),
                    ("positions-test.json", b"new results"),
                ];
                let error = r
                    .save_copy_using(&parent, "", None, newer, |at, path| {
                        if at == step && path.file_name().unwrap() == failed_file {
                            Err(io::Error::other("injected save failure"))
                        } else {
                            Ok(())
                        }
                    })
                    .unwrap_err();
                assert!(error.to_string().contains("injected save failure"));
                assert_eq!(saved_bytes(&dest), old, "{step:?} at {failed_file}");
                assert_eq!(
                    fs::read_dir(&parent).unwrap().count(),
                    1,
                    "no abandoned staging"
                );
                let live: serde_json::Value = serde_json::from_str(
                    &fs::read_to_string(r.dir().join("session.json")).unwrap(),
                )
                .unwrap();
                assert!(live["ended_utc"].is_null(), "live metadata stays open");
                assert_eq!(
                    r.save_copy_with_files(&parent, "", None, newer).unwrap(),
                    dest
                );
                assert_eq!(
                    fs::read(dest.join("session.toml")).unwrap(),
                    b"new manifest"
                );
                assert_eq!(
                    fs::read(dest.join("positions-test.json")).unwrap(),
                    b"new results"
                );
                assert_eq!(
                    fs::read(dest.join("owners-note.txt")).unwrap(),
                    b"keep this"
                );
                let wav = hound::WavReader::open(dest.join("audio-001.wav")).unwrap();
                assert_eq!(wav.duration(), 16000);
                drop(wav);
                drop(r);
                fs::remove_dir_all(base).unwrap();
            }
        }
    }

    #[test]
    fn a_missing_recorded_segment_prevents_an_incomplete_copy() {
        let (mut r, base) = save_fixture("missing-segment");
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 62);
        settle(&mut r);
        let first = r.dir().join("audio-001.flac");
        let bytes = fs::read(&first).unwrap();
        // The first segment is closed, so this fault is portable to Windows.
        fs::remove_file(&first).unwrap();
        let parent = base.join("saved");
        assert!(r.save_copy(&parent, "", None).is_err());
        assert!(r.saved_name.is_none());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::write(&first, &bytes).unwrap();
        let dest = r.save_copy(&parent, "", None).unwrap();
        assert_eq!(fs::read(dest.join("audio-001.flac")).unwrap(), bytes);
        assert_eq!(
            hound::WavReader::open(dest.join("audio-002.wav"))
                .unwrap()
                .duration(),
            8000
        );
        drop(r);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn failed_first_save_does_not_claim_a_copy_name_or_leave_a_partial_take() {
        let (mut r, base) = save_fixture("first");
        let parent = base.join("saved");
        let result = r.save_copy_using(&parent, "First", None, &[], |step, path| {
            if step == SaveStep::Install && path.file_name().unwrap() == "session.json" {
                Err(io::Error::other("injected first save failure"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert!(r.saved_name.is_none());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        let dest = r.save_copy(&parent, "Retry", None).unwrap();
        assert!(dest
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("_Retry"));
        drop(r);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn failed_rollback_keeps_recovery_files_and_reports_their_location() {
        let (mut r, base) = save_fixture("rollback");
        let parent = base.join("saved").join("..").join("saved");
        let dest = r.save_copy(&parent, "", None).unwrap();
        let old_audio = fs::read(dest.join("audio-001.wav")).unwrap();
        append_second(&mut r, 2);
        let error = r
            .save_copy_using(&parent, "", None, &[], |step, path| {
                if (step == SaveStep::Install && path.file_name().unwrap() == "clock.csv")
                    || (step == SaveStep::Restore && path.file_name().unwrap() == "audio-001.wav")
                {
                    Err(io::Error::other("injected failure"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        let recovery = fs::read_dir(&parent)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".timegrapher-save-")
            })
            .unwrap();
        let message = error.to_string();
        assert!(message.contains("rollback failed"));
        let reported = PathBuf::from(
            message
                .split_once("; recovery files retained in ")
                .expect("the error must give the complete recovery path")
                .1,
        );
        assert!(
            reported.is_dir(),
            "reported recovery path is unusable: {message}"
        );
        // Windows' temp path can use RUNNER~1 while canonical paths use
        // runneradmin. Check identity, then read the exact reported location.
        assert_eq!(
            fs::canonicalize(&reported).unwrap(),
            fs::canonicalize(&recovery).unwrap(),
            "{message}; discovered recovery: {}",
            recovery.display()
        );
        assert_eq!(
            fs::canonicalize(reported.parent().unwrap()).unwrap(),
            fs::canonicalize(&parent).unwrap(),
            "{message}; expected save parent: {}",
            parent.display()
        );
        assert_eq!(
            fs::read(reported.join("backup/audio-001.wav")).unwrap(),
            old_audio
        );
        assert_eq!(
            reported.parent().unwrap().as_os_str(),
            parent.as_os_str(),
            "the recovery error must retain the caller's path spelling: {message}"
        );
        drop(r);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn saved_copy_retains_the_callers_relative_path() {
        let (mut r, base) = save_fixture("caller-path");
        // No process-wide current-directory change: exercise a relative caller
        // path even when the platform's temp directory already is canonical.
        let relative = PathBuf::from(format!("tg-save-relative-{}", std::process::id()));
        let parent = relative.join("saved").join("..").join("saved");
        let dest = r.save_copy(&parent, "", None).unwrap();
        assert_eq!(
            dest.as_os_str(),
            parent.join(r.saved_name.as_ref().unwrap()).as_os_str()
        );
        assert!(dest.join("session.json").is_file());
        append_second(&mut r, 2);
        let again = r
            .save_copy_using(&parent, "", None, &[], |_, path| {
                assert_eq!(path.parent().unwrap().as_os_str(), dest.as_os_str());
                Ok(())
            })
            .unwrap();
        assert_eq!(again.as_os_str(), dest.as_os_str());

        // Canonical identity checks must still work for relative paths. The
        // descendant destination does not yet exist; the ancestor does.
        let mut local = Recorder::start(&relative.join("live"), r.info.clone()).unwrap();
        let nested = local.dir().join("nested");
        assert!(!nested.join(local.copy_name("")).exists());
        assert!(local.save_copy(&nested, "", None).is_err());
        local.saved_name = Some(local.dir().file_name().unwrap().to_os_string());
        assert!(local.save_copy(&relative.join("live"), "", None).is_err());
        local.saved_name = Some("live".into());
        assert!(local.save_copy(&relative, "", None).is_err());
        append_second(&mut local, 1);
        assert_eq!(local.duration_s(), 1.0);
        drop(local);
        drop(r);
        fs::remove_dir_all(relative).unwrap();
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_parents_retain_spelling_without_bypassing_alias_checks() {
        let (mut r, base) = save_fixture("parent-alias");
        let real = base.join("saved");
        fs::create_dir(&real).unwrap();
        let alias = base.join("saved-alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let dest = r.save_copy(&alias, "", None).unwrap();
        assert_eq!(dest.parent().unwrap().as_os_str(), alias.as_os_str());
        assert_eq!(
            fs::canonicalize(&dest).unwrap(),
            fs::canonicalize(real.join(dest.file_name().unwrap())).unwrap()
        );
        let live_alias = base.join("live-alias");
        std::os::unix::fs::symlink(r.dir(), &live_alias).unwrap();
        assert!(!live_alias.join(r.saved_name.as_ref().unwrap()).exists());
        let before = fs::read(r.dir().join("session.json")).unwrap();
        assert!(r.save_copy(&live_alias, "", None).is_err());
        assert_eq!(fs::read(r.dir().join("session.json")).unwrap(), before);
        append_second(&mut r, 2);
        assert_eq!(r.duration_s(), 2.0);
        drop(r);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn companion_names_cannot_replace_recorder_files_or_escape_the_take() {
        let (mut r, base) = save_fixture("names");
        for name in [
            "../outside",
            "folder/file",
            "folder\\file",
            "/absolute",
            "session.json",
            "SESSION.JSON",
            "clock.csv",
            "audio-999.wav",
            "file:stream",
        ] {
            assert!(
                r.save_copy_with_files(&base.join("saved"), "", None, &[(name, b"bad")])
                    .is_err(),
                "{name}"
            );
        }
        assert!(r
            .save_copy_with_files(
                &base.join("saved"),
                "",
                None,
                &[("session.toml", b"one"), ("SESSION.TOML", b"two")]
            )
            .is_err());
        assert!(r.saved_name.is_none());
        assert!(!base.join("saved").exists());
        drop(r);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_copy_cannot_alias_the_live_recording() {
        let (mut r, base) = save_fixture("alias");
        r.saved_name = Some(r.dir().file_name().unwrap().to_os_string());
        let before = fs::read(r.dir().join("session.json")).unwrap();
        let source_parent = r.dir().parent().unwrap().to_path_buf();
        assert_eq!(source_parent.join(r.saved_name.as_ref().unwrap()), r.dir());
        assert!(r.save_copy(&source_parent, "", None).is_err());
        let nested = r.dir().join("nested");
        assert!(r.save_copy(&nested, "", None).is_err());
        assert_eq!(fs::read(r.dir().join("session.json")).unwrap(), before);
        append_second(&mut r, 2);
        assert_eq!(r.duration_s(), 2.0);
        drop(r);
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_destination_cannot_alias_live_files() {
        let (mut r, base) = save_fixture("symlink-alias");
        let parent = base.join("saved");
        fs::create_dir_all(&parent).unwrap();
        let name = r.copy_name("");
        std::os::unix::fs::symlink(r.dir(), parent.join(name)).unwrap();
        let before = fs::read(r.dir().join("session.json")).unwrap();
        assert!(r.save_copy(&parent, "", None).is_err());
        assert_eq!(fs::read(r.dir().join("session.json")).unwrap(), before);
        append_second(&mut r, 2);
        drop(r);
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_closed_saved_segment_cannot_link_back_to_live_audio() {
        let (mut r, base) = save_fixture("closed-symlink");
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 62);
        settle(&mut r);
        let parent = base.join("saved");
        let dest = r.save_copy(&parent, "", None).unwrap();
        let first = dest.join("audio-001.flac");
        let original = fs::read(&first).unwrap();
        fs::remove_file(&first).unwrap();
        std::os::unix::fs::symlink(r.dir().join("audio-001.flac"), &first).unwrap();
        let metadata = fs::read(dest.join("session.json")).unwrap();
        append_second(&mut r, 63);
        assert!(r.save_copy(&parent, "", None).is_err());
        assert_eq!(fs::read(dest.join("session.json")).unwrap(), metadata);
        // Repair the unsafe destination and retry; discarding scratch must
        // leave a self-contained saved take, not dangling links.
        fs::remove_file(&first).unwrap();
        fs::write(&first, &original).unwrap();
        r.save_copy(&parent, "", None).unwrap();
        let scratch = r.dir().to_path_buf();
        drop(r);
        fs::remove_dir_all(scratch).unwrap();
        assert_eq!(fs::read(&first).unwrap(), original);
        assert!(hound::WavReader::open(dest.join("audio-002.wav")).is_ok());
        fs::remove_dir_all(base).unwrap();
    }

    /// A copy is named after the start time, the watch and the calibre as
    /// they are when it is saved, with no position however the watch
    /// moved, and is saved again in the same place.
    #[test]
    fn a_copy_is_named_after_the_watch_and_calibre() {
        let base = std::env::temp_dir().join(format!("tg-moved-{}", std::process::id()));
        let info = SessionInfo {
            watch: "Test Watch".into(),
            position: "DU".into(),
            lift_deg: 52.0,
            bph: Some(28800),
            device: "test".into(),
            sample_rate: 8000,
            bits: 16,
            software: "test".into(),
        };
        let mut r = Recorder::start(&base.join("tmp"), info).unwrap();
        let block = Block {
            samples: vec![0.1; 8000],
            at: SystemTime::now(),
        };
        r.write(&block).unwrap();
        r.note("position DD").unwrap();
        let mut info = r.info().clone();
        info.watch = "Sea Dweller".into();
        info.position = "DD".into();
        r.set_info(info).unwrap();
        let first = r
            .save_copy(&base.join("saved"), "ETA 2824-2", None)
            .unwrap();
        let name = first.file_name().unwrap().to_str().unwrap().to_string();
        assert!(name.ends_with("_Sea_Dweller_ETA_2824-2"), "{name}");
        assert_eq!(name.len(), "2026-10-08T21-04-05".len() + 23, "{name}");
        r.note("position CL").unwrap();
        assert_eq!(
            r.save_copy(&base.join("saved"), "Other", None).unwrap(),
            first
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_take_of_a_few_seconds_has_a_clock_log_to_fit() {
        // Takes far shorter than the 10 s between clock lines, in blocks of
        // 0.1 s arriving a few ms apart from where they are due and one
        // 60 ms late: the clock fits from the log, in a copy saved while
        // listening and in the take when it stops, and corrects nothing.
        for (n, take_s) in [(0, 2.5), (1, 15.0)] {
            let base = std::env::temp_dir().join(format!("tg-short-{}-{n}", std::process::id()));
            let info = SessionInfo {
                watch: String::new(),
                position: "DU".into(),
                lift_deg: 52.0,
                bph: Some(28800),
                device: "test".into(),
                sample_rate: 8000,
                bits: 16,
                software: "test".into(),
            };
            let mut r = Recorder::start(&base.join("tmp"), info).unwrap();
            let t0 = SystemTime::now();
            let blocks = (take_s * 10.0) as u64;
            for k in 0..blocks {
                let late_ms = if k == 15 { 60 } else { (k * 7) % 5 };
                r.write(&Block {
                    samples: vec![0.1; 800],
                    at: t0 + Duration::from_millis(100 * (k + 1) + late_ms),
                })
                .unwrap();
            }
            let fit = |dir: &Path| {
                let clock = fs::read_to_string(dir.join("clock.csv")).unwrap();
                let pairs = crate::clock::parse_log(&clock, 8000, 2).unwrap();
                let fit = crate::clock::ClockFit::for_rates(&pairs)
                    .unwrap_or_else(|e| panic!("{take_s} s: {e}\n{clock}"));
                assert!(!fit.corrects(), "{take_s} s: {clock}");
                pairs.len()
            };
            let copy = r.save_copy(&base.join("saved"), "", None).unwrap();
            let lines = fit(&copy);
            let dir = r.finish(None).unwrap();
            assert_eq!(fit(&dir), lines);
            assert!(lines < crate::clock::MIN_POINTS, "{take_s} s: {lines}");
            if n == 0 {
                // Where each line falls: the first block (0.1 s of sound,
                // at 0.100 s), one a second after it (1.1 s at 1.100 s and
                // 2.1 s at 2.100 s), and the end (2.5 s at 2.503 s).
                let clock = fs::read_to_string(dir.join("clock.csv")).unwrap();
                let pairs = crate::clock::parse_log(&clock, 8000, 2).unwrap();
                let audio: Vec<f64> = pairs.iter().map(|q| q.0).collect();
                let host: Vec<f64> = pairs.iter().map(|q| q.1 - pairs[0].1).collect();
                for (got, want) in audio.iter().zip([0.1, 1.1, 2.1, 2.5]) {
                    assert!((got - want).abs() < 1e-9, "{audio:?}");
                }
                for (got, want) in host.iter().zip([0.0, 1.0, 2.0, 2.403]) {
                    assert!((got - want).abs() < 1e-6, "{host:?}");
                }
                assert_eq!(pairs.len(), 4, "{clock}");
            }
            std::fs::remove_dir_all(&base).ok();
        }
    }

    #[test]
    fn pauses_and_saves_a_copy_while_recording() {
        let base = std::env::temp_dir().join(format!("tg-copy-{}", std::process::id()));
        let info = SessionInfo {
            watch: String::new(),
            position: "DU".into(),
            lift_deg: 52.0,
            bph: Some(28800),
            device: "test".into(),
            sample_rate: 8000,
            bits: 16,
            software: "test".into(),
        };
        let mut r = Recorder::start(&base.join("tmp"), info).unwrap();
        let t0 = SystemTime::now();
        let block = |k: u64| Block {
            samples: vec![0.1; 8000],
            at: t0 + Duration::from_secs(k),
        };
        for k in 0..3 {
            r.write(&block(k)).unwrap();
        }
        r.pause().unwrap();
        // A minute later, listening again on another input.
        r.resume("USB mic");
        for k in 63..65 {
            r.write(&block(k)).unwrap();
        }
        settle(&mut r);
        let copy = r.save_copy(&base.join("saved"), "", None).unwrap();
        // Saved again: the file closed by the pause is not copied again
        // (a mark put in the saved copy stays), the open one is.
        let first = copy.join("audio-001.flac");
        let mut bytes = fs::read(&first).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&first, &bytes).unwrap();
        r.write(&block(65)).unwrap();
        assert_eq!(r.save_copy(&base.join("saved"), "", None).unwrap(), copy);
        assert_eq!(fs::read(&first).unwrap(), bytes);
        bytes[last] ^= 1;
        fs::write(&first, &bytes).unwrap();
        let load = |f: &str| crate::audio::load(&copy.join(f)).unwrap().samples.len();
        assert_eq!(load("audio-002.wav"), 3 * 8000);
        // The sound after the pause is in a file of its own.
        assert_eq!(
            (load("audio-001.flac"), load("audio-002.wav")),
            (3 * 8000, 3 * 8000)
        );
        let clock = fs::read_to_string(copy.join("clock.csv")).unwrap();
        let pairs = crate::clock::parse_log(&clock, 8000, 2).unwrap();
        // A line a second over the first 3 s (the last of them the last
        // block before the pause), the first block after it, and the last
        // block at each save; frames running on, no line made up across
        // the gap.
        let frames: Vec<u64> = clock
            .lines()
            .filter_map(|l| l.split(',').nth(1)?.parse().ok())
            .collect();
        assert_eq!(
            frames,
            [8000, 2 * 8000, 3 * 8000, 4 * 8000, 5 * 8000, 6 * 8000],
            "{clock}"
        );
        assert_eq!(pairs.len(), 6, "{clock}");
        let session = fs::read_to_string(copy.join("session.json")).unwrap();
        assert!(session.contains("paused"));
        let v: serde_json::Value = serde_json::from_str(&session).unwrap();
        let b = &v["breaks"][0];
        assert_eq!(b["frame"], 3 * 8000);
        assert_eq!(b["audio_s"], 3.0);
        assert_eq!(b["segment"], "audio-002.wav");
        assert_eq!(b["device"], "USB mic");
        // Last frame before at 2 s, first after at 63 - 1 = 62 s.
        assert!((b["gap_s"].as_f64().unwrap() - 60.0).abs() < 1e-6, "{b}");
        assert_eq!(b["utc"], utc(t0 + Duration::from_secs(62)));
        assert_eq!(v["breaks"].as_array().unwrap().len(), 1);
        // Recording carried on in the original folder, in the new file
        // (the block written before the second save).
        // A pause with no resume named still breaks, on the take's input.
        r.pause().unwrap();
        r.write(&block(90)).unwrap();
        assert_eq!(r.breaks().len(), 2);
        assert_eq!(r.breaks()[1].device, "test");
        assert_eq!(r.breaks()[1].segment, "audio-003.wav");
        let dir = r.finish(None).unwrap();
        // Each hour a pause closed is kept as FLAC; the last, never
        // paused, stays a WAV.
        let names = ["audio-001.flac", "audio-002.flac", "audio-003.wav"];
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("session.json")).unwrap()).unwrap();
        assert_eq!(v["segments"], serde_json::json!(names));
        assert_eq!(v["breaks"][0]["segment"], "audio-002.flac");
        let total: usize = names
            .iter()
            .map(|f| crate::audio::load(&dir.join(f)).unwrap().samples.len())
            .sum();
        assert_eq!(total, 7 * 8000);
        fs::remove_dir_all(&base).unwrap();
    }

    /// A copy kept as its analysis alone holds every file of the recording
    /// but its sound, session.json as a copy has it, and the required files,
    /// which take the place of a file of the same name; it is named as the
    /// copy is, ending `_analysis`.
    #[test]
    fn an_analysis_copy_holds_all_but_the_sound() {
        let (mut r, base) = save_fixture("analysis-copy");
        fs::write(r.dir().join("notes.txt"), b"kept").unwrap();
        fs::write(r.dir().join("Report.txt"), b"old").unwrap();
        let (name, files) = r
            .analysis_copy_files("ETA 2824-2", None, &[("report.txt", b"new")])
            .unwrap();
        let name = name.to_string_lossy().into_owned();
        assert!(name.ends_with("_ETA_2824-2_analysis"), "{name}");
        assert!(name.contains("Save_test"), "{name}");
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [take::RECORD, take::CLOCK_LOG, "notes.txt", "report.txt"]
        );
        assert_eq!(files[2].1, b"kept");
        assert_eq!(files[3].1, b"new");
        let session: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
        assert_eq!(session["watch"], "Save test");
        drop(r);
        fs::remove_dir_all(&base).unwrap();
    }

    /// A plain companion name is saved beside the recording, and one the
    /// live folder already holds is refused.
    #[test]
    fn a_plain_companion_file_is_saved_with_the_copy() {
        let (mut r, base) = save_fixture("companion");
        let copy = r
            .save_copy_with_files(&base.join("saved"), "", None, &[("session.toml", b"one")])
            .unwrap();
        assert_eq!(fs::read(copy.join("session.toml")).unwrap(), b"one");
        fs::write(r.dir().join("extra.txt"), b"live").unwrap();
        assert!(r
            .save_copy_with_files(&base.join("saved"), "", None, &[("extra.txt", b"x")])
            .is_err());
        drop(r);
        fs::remove_dir_all(&base).unwrap();
    }

    /// The audio file still being written when a save begins is copied
    /// with its header as it was then, so the copy's header gives the
    /// length of the sound it holds, not the length the live file has
    /// grown to by the time the copy is written.
    #[test]
    fn a_copy_keeps_the_open_files_header_from_when_the_save_began() {
        let (mut r, base) = save_fixture("open-header");
        append_second(&mut r, 2);
        let pending = r.begin_save(&base.join("saved"), "", None, &[]).unwrap();
        for second in 3..=5 {
            append_second(&mut r, second);
        }
        r.flush().unwrap();
        let copy = pending.write().unwrap();
        let wav = hound::WavReader::open(copy.dir.join("audio-001.wav")).unwrap();
        assert_eq!(wav.duration(), 2 * 8000);
        drop(r);
        fs::remove_dir_all(&base).unwrap();
    }

    /// A save copies the other files the live folder holds beside the
    /// sound and the clock log, and passes over a folder in it.
    #[test]
    fn a_save_copies_the_folders_other_files_and_passes_over_folders() {
        let (mut r, base) = save_fixture("other-files");
        fs::write(r.dir().join("notes.txt"), b"kept").unwrap();
        fs::create_dir_all(r.dir().join("photos")).unwrap();
        let copy = r.save_copy(&base.join("saved"), "", None).unwrap();
        assert_eq!(fs::read(copy.join("notes.txt")).unwrap(), b"kept");
        assert!(!copy.join("photos").exists());
        drop(r);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_closed_hour_is_kept_as_flac_and_named_so() {
        let (mut r, base) = save_fixture("kept-as-flac");
        append_second(&mut r, 2);
        r.pause().unwrap();
        let wav = crate::audio::load(&r.dir().join("audio-001.wav")).unwrap();
        r.resume("test");
        append_second(&mut r, 62);
        settle(&mut r);
        let dir = r.dir().to_path_buf();
        // The hour closed by the pause is its FLAC alone, the same samples.
        assert!(!dir.join("audio-001.wav").exists());
        assert!(!dir.join("audio-001.flac.part").exists());
        let flac = crate::audio::load(&dir.join("audio-001.flac")).unwrap();
        assert_eq!(flac.samples, wav.samples);
        assert_eq!(flac.samples.len(), 2 * 8000);
        let session = || -> serde_json::Value {
            serde_json::from_str(&fs::read_to_string(dir.join("session.json")).unwrap()).unwrap()
        };
        assert_eq!(
            session()["segments"],
            serde_json::json!(["audio-001.flac", "audio-002.wav"])
        );
        assert_eq!(session()["breaks"][0]["segment"], "audio-002.wav");
        // The folder reads as the take it is, its break where it was.
        let files = crate::recording_order::recordings_in(&dir, &[]).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.file_name().unwrap()).collect();
        assert_eq!(names, ["audio-001.flac", "audio-002.wav"]);
        let refs: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
        let found = crate::breaks::of_files(&refs, None, 8000);
        assert_eq!(found.unlisted, None);
        drop(r);
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_save_holds_a_closed_wav_as_flac_and_leaves_no_wav_twin() {
        use std::os::unix::fs::MetadataExt;
        let (mut r, base) = save_fixture("save-as-flac");
        // Hours not yet kept as FLAC when the save begins.
        r.keep_flac = false;
        let parent = base.join("saved");
        let dest = r.save_copy(&parent, "", None).unwrap();
        assert!(dest.join("audio-001.wav").exists());
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 62);
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 64);
        assert_eq!(r.save_copy(&parent, "", None).unwrap(), dest);
        let scratch = r.dir().to_path_buf();
        let live = |n: &str| crate::audio::load(&scratch.join(n)).unwrap().samples;
        let saved = |n: &str| crate::audio::load(&dest.join(n)).unwrap().samples;
        // Each closed WAV is held as FLAC of the same samples, the WAV the
        // copy held before gone; the open one is a WAV.
        for n in ["audio-001", "audio-002"] {
            assert_eq!(saved(&format!("{n}.flac")), live(&format!("{n}.wav")));
            assert!(!dest.join(format!("{n}.wav")).exists());
        }
        assert_eq!(saved("audio-003.wav"), live("audio-003.wav"));
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dest.join("session.json")).unwrap()).unwrap();
        assert_eq!(
            v["segments"],
            serde_json::json!(["audio-001.flac", "audio-002.flac", "audio-003.wav"])
        );
        assert_eq!(v["breaks"][0]["segment"], "audio-002.flac");
        assert_eq!(v["breaks"][1]["segment"], "audio-003.wav");
        // Saved again, a FLAC that holds its WAV's samples is left alone.
        let ino = fs::metadata(dest.join("audio-001.flac")).unwrap().ino();
        append_second(&mut r, 65);
        r.save_copy(&parent, "", None).unwrap();
        assert_eq!(
            fs::metadata(dest.join("audio-001.flac")).unwrap().ino(),
            ino
        );
        // One that does not is written again.
        fs::write(dest.join("audio-002.flac"), b"not the sound").unwrap();
        r.save_copy(&parent, "", None).unwrap();
        assert_eq!(saved("audio-002.flac"), live("audio-002.wav"));
        drop(r);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_save_while_an_hour_is_being_kept_as_flac_holds_it_whole() {
        let (mut r, base) = save_fixture("save-mid-swap");
        for second in 2..=30 {
            append_second(&mut r, second);
        }
        r.pause().unwrap();
        let wav = crate::audio::load(&r.dir().join("audio-001.wav")).unwrap();
        assert_eq!(wav.samples.len(), 30 * 8000);
        r.resume("test");
        append_second(&mut r, 90);
        // Begun at once, the save may find the hour a WAV, its FLAC, or
        // both; either way the copy holds its FLAC, whole.
        let pending = r.begin_save(&base.join("saved"), "", None, &[]).unwrap();
        settle(&mut r);
        let copy = pending.write().unwrap();
        let flac = crate::audio::load(&copy.dir.join("audio-001.flac")).unwrap();
        assert_eq!(flac.samples, wav.samples);
        assert!(!copy.dir.join("audio-001.wav").exists());
        let names: Vec<_> = fs::read_dir(&copy.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.starts_with("audio-"))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(names, ["audio-001.flac", "audio-002.wav"]);
        drop(r);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn an_hour_that_cannot_be_kept_as_flac_stays_a_whole_wav() {
        let (mut r, base) = save_fixture("flac-fails");
        append_second(&mut r, 2);
        let dir = r.dir().to_path_buf();
        // A folder where the FLAC is written first: the conversion fails.
        fs::create_dir(dir.join("audio-001.flac.part")).unwrap();
        r.pause().unwrap();
        let wav = crate::audio::load(&dir.join("audio-001.wav")).unwrap();
        assert_eq!(wav.samples.len(), 2 * 8000);
        r.resume("test");
        append_second(&mut r, 62);
        settle(&mut r);
        let whole = || {
            assert_eq!(
                crate::audio::load(&dir.join("audio-001.wav"))
                    .unwrap()
                    .samples,
                wav.samples
            );
            assert!(!dir.join("audio-001.flac").exists());
        };
        whole();
        let session = || -> serde_json::Value {
            serde_json::from_str(&fs::read_to_string(dir.join("session.json")).unwrap()).unwrap()
        };
        // Nothing was renamed, so nothing in session.json was either.
        assert_eq!(session()["segments"], serde_json::json!(["audio-001.wav"]));
        assert_eq!(r.segments, ["audio-001.wav", "audio-002.wav"]);
        assert_eq!(r.breaks()[0].segment, "audio-002.wav");
        // A save still holds the hour, as FLAC of the same samples, and
        // leaves the recording's WAV as it was.
        let copy = r.save_copy(&base.join("saved"), "", None).unwrap();
        let saved = crate::audio::load(&copy.join("audio-001.flac")).unwrap();
        assert_eq!(saved.samples, wav.samples);
        whole();
        // Finished, the recording still names and holds the WAV.
        let done = r.finish(None).unwrap();
        assert_eq!(done, dir);
        whole();
        assert_eq!(
            session()["segments"],
            serde_json::json!(["audio-001.wav", "audio-002.wav"])
        );
        assert_eq!(session()["breaks"][0]["segment"], "audio-002.wav");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_save_while_paused_holds_every_hour_as_flac() {
        let (mut r, base) = save_fixture("paused-save");
        append_second(&mut r, 2);
        r.pause().unwrap();
        // Paused, no hour is being written: the save holds them all as
        // FLAC, whether or not the last has been converted yet.
        let copy = r.save_copy(&base.join("saved"), "", None).unwrap();
        let names: Vec<_> = fs::read_dir(&copy)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.starts_with("audio-"))
            .collect();
        assert_eq!(names, ["audio-001.flac"]);
        let saved = crate::audio::load(&copy.join("audio-001.flac")).unwrap();
        assert_eq!(saved.samples.len(), 2 * 8000);
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(copy.join("session.json")).unwrap()).unwrap();
        assert_eq!(v["segments"], serde_json::json!(["audio-001.flac"]));
        // Listening again starts the next hour, as after any pause.
        r.resume("test");
        append_second(&mut r, 30);
        assert_eq!(r.segments.len(), 2);
        assert_eq!(r.breaks().len(), 1);
        // Paused before finishing, the last hour is kept as FLAC too.
        r.pause().unwrap();
        let dir = r.finish(None).unwrap();
        assert!(!dir.join("audio-002.wav").exists());
        assert_eq!(
            crate::audio::load(&dir.join("audio-002.flac"))
                .unwrap()
                .samples
                .len(),
            8000
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_take_finished_as_flac_holds_every_hour_as_flac() {
        let (mut r, base) = save_fixture("finish-as-flac");
        append_second(&mut r, 2);
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 30);
        append_second(&mut r, 31);
        let dir = r.finish_as_flac(None).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("session.json")).unwrap()).unwrap();
        assert_eq!(
            v["segments"],
            serde_json::json!(["audio-001.flac", "audio-002.flac"])
        );
        assert_eq!(v["breaks"][0]["segment"], "audio-002.flac");
        assert!(v["ended_utc"].is_string());
        let sounds: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.starts_with("audio-"))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(sounds, ["audio-001.flac", "audio-002.flac"]);
        let len = |n: &str| crate::audio::load(&dir.join(n)).unwrap().samples.len();
        assert_eq!(
            (len("audio-001.flac"), len("audio-002.flac")),
            (2 * 8000, 2 * 8000)
        );
        assert!(!lock_path(&dir).exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_saved_take_holding_wavs_is_kept_as_flac_and_named_so() {
        let (mut r, base) = save_fixture("saved-wavs");
        // Saved as takes were before hours were kept as FLAC.
        r.keep_flac = false;
        append_second(&mut r, 2);
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 30);
        let dir = r.finish(None).unwrap();
        let session = || fs::read_to_string(dir.join("session.json")).unwrap();
        let before = session();
        let samples = |n: &str| crate::audio::load(&dir.join(n)).unwrap().samples;
        let heard = [samples("audio-001.wav"), samples("audio-002.wav")];
        // A session folder holding it, as `timegrapher record` makes.
        let outer = base.join("live");
        assert_eq!(holding_wav(&outer), std::slice::from_ref(&dir));
        assert_eq!(holding_wav(&dir), std::slice::from_ref(&dir));
        assert_eq!(keep_saved_as_flac(&dir).unwrap(), 2);
        assert_eq!(samples("audio-001.flac"), heard[0]);
        assert_eq!(samples("audio-002.flac"), heard[1]);
        assert!(!dir.join("audio-001.wav").exists() && !dir.join("audio-002.wav").exists());
        // session.json is as it was but for the names.
        assert_eq!(
            session(),
            before
                .replace("\"audio-001.wav\"", "\"audio-001.flac\"")
                .replace("\"audio-002.wav\"", "\"audio-002.flac\"")
        );
        let v: serde_json::Value = serde_json::from_str(&session()).unwrap();
        assert_eq!(v["breaks"][0]["segment"], "audio-002.flac");
        // Nothing is left to do, and it reads as the take it was.
        assert!(holding_wav(&outer).is_empty());
        assert_eq!(keep_saved_as_flac(&dir).unwrap(), 0);
        let files = crate::recording_order::recordings_in(&dir, &[]).unwrap();
        let refs: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
        assert_eq!(crate::breaks::of_files(&refs, None, 8000).unlisted, None);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_saved_hour_kept_as_a_bloated_flac_is_written_again_in_its_place() {
        let (mut r, base) = save_fixture("saved-bloated");
        append_second(&mut r, 2);
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 30);
        let dir = r.finish_as_flac(None).unwrap();
        assert!(holding_wav(&dir).is_empty());
        // Its second hour as flacenc wrote some before each block was
        // checked: far bigger than its samples.
        let hour = dir.join("audio-002.flac");
        let samples = crate::flac::write_bloated(&hour);
        let session = || fs::read_to_string(dir.join("session.json")).unwrap();
        let before = session();
        let big = fs::metadata(&hour).unwrap().len();
        assert_eq!(holding_wav(&dir), std::slice::from_ref(&dir));
        assert_eq!(keep_saved_as_flac(&dir).unwrap(), 1);
        assert!(fs::metadata(&hour).unwrap().len() < big / 2);
        assert_eq!(
            crate::audio::load(&hour).unwrap().samples.len(),
            samples.len()
        );
        // The same file, name and session.json; nothing left to do.
        assert_eq!(session(), before);
        assert!(holding_wav(&dir).is_empty());
        assert_eq!(keep_saved_as_flac(&dir).unwrap(), 0);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_take_opened_while_its_hours_are_kept_stops_at_the_next_hour() {
        let (mut r, base) = save_fixture("saved-wavs-stopped");
        r.keep_flac = false;
        append_second(&mut r, 2);
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 30);
        let dir = r.finish(None).unwrap();
        // Opened after the first hour is kept.
        let asked = std::cell::Cell::new(0);
        let stop = || {
            asked.set(asked.get() + 1);
            asked.get() > 1
        };
        assert_eq!(keep_saved_as_flac_unless(&dir, &stop).unwrap(), 1);
        assert!(dir.join("audio-001.flac").exists() && dir.join("audio-002.wav").exists());
        // Half kept, its files are still the take's segments, in order,
        // and the rest is left for another time.
        let files = crate::recording_order::recordings_in(&dir, &[]).unwrap();
        let refs: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
        assert!(crate::breaks::are_take_segments(&refs));
        assert!(!crate::breaks::are_take_segments(&refs[1..]));
        assert_eq!(holding_wav(&dir), std::slice::from_ref(&dir));
        assert_eq!(keep_saved_as_flac(&dir).unwrap(), 1);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_saved_hour_that_cannot_be_kept_as_flac_stays_a_wav_and_is_said() {
        let (mut r, base) = save_fixture("saved-wav-fails");
        r.keep_flac = false;
        append_second(&mut r, 2);
        let dir = r.finish(None).unwrap();
        let before = fs::read(dir.join("session.json")).unwrap();
        let wav = fs::read(dir.join("audio-001.wav")).unwrap();
        fs::create_dir(dir.join("audio-001.flac.part")).unwrap();
        let e = keep_saved_as_flac(&dir).unwrap_err();
        assert!(e.contains("audio-001.wav"), "{e}");
        assert_eq!(fs::read(dir.join("audio-001.wav")).unwrap(), wav);
        assert!(!dir.join("audio-001.flac").exists());
        assert_eq!(fs::read(dir.join("session.json")).unwrap(), before);
        assert_eq!(holding_wav(&dir), std::slice::from_ref(&dir));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_take_being_recorded_is_never_converted_from_under_its_recorder() {
        let (mut r, base) = save_fixture("saved-live");
        r.keep_flac = false;
        append_second(&mut r, 2);
        r.pause().unwrap();
        r.resume("test");
        append_second(&mut r, 30);
        r.note("turned to DD").unwrap();
        r.flush().unwrap();
        let dir = r.dir().to_path_buf();
        // Its session.json lists both hours as WAVs: one closed, one open.
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("session.json")).unwrap()).unwrap();
        assert_eq!(
            v["segments"],
            serde_json::json!(["audio-001.wav", "audio-002.wav"])
        );
        let before = saved_bytes(&dir);
        assert!(holding_wav(&base.join("live")).is_empty());
        assert!(holding_wav(&dir).is_empty());
        assert_eq!(keep_saved_as_flac(&dir).unwrap(), 0);
        assert_eq!(saved_bytes(&dir), before);
        // The recording carries on into its open hour.
        append_second(&mut r, 31);
        let done = r.finish(None).unwrap();
        assert_eq!(
            crate::audio::load(&done.join("audio-002.wav"))
                .unwrap()
                .samples
                .len(),
            2 * 8000
        );
        // Finished, it is a saved take like any other.
        assert_eq!(holding_wav(&done), std::slice::from_ref(&done));
        assert_eq!(keep_saved_as_flac(&done).unwrap(), 2);
        fs::remove_dir_all(&base).unwrap();
    }
}
