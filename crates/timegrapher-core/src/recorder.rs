//! Saving what the microphone hears, so every test can be analysed again.
//!
//! A recording is a folder holding the audio as WAV files of at most an
//! hour each (`audio-001.wav`, `audio-002.wav`, ...), a clock log
//! (`clock.csv`: system time against frames captured, every 10 s) and
//! `session.json` with the watch, position, lift angle and settings.
//! `timegrapher long <folder>/ --clock <folder>/clock.csv` reads it as one
//! continuous recording calibrated against the system clock.
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
use serde::Serialize;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Seconds after the watch is put in a new position that are left out of
/// its readings while it settles (the hand leaving the stand, the balance
/// steadying). The sound is still kept; a saved take's positions start
/// after it.
pub const SETTLE_S: f64 = 10.0;

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
const CLOCK_EVERY_S: f64 = 10.0;

pub struct Recorder {
    dir: PathBuf,
    info: SessionInfo,
    /// The position at the start, which `set_info` never changes: later
    /// positions are the "position" notes.
    first_position: String,
    started_utc: String,
    writer: Option<hound::WavWriter<BufWriter<File>>>,
    segments: Vec<String>,
    seg_frames: u64,
    frames: u64,
    clock: BufWriter<File>,
    last_clock: Option<SystemTime>,
    /// The latest block's arrival and the frames written by then.
    last_block: Option<(SystemTime, u64)>,
    /// Paused: the host time of the last frame before the pause, until
    /// listening carries on.
    paused_at: Option<SystemTime>,
    /// The input listened to after the pause, when it was named.
    resumed_on: Option<String>,
    breaks: Vec<Break>,
    notes: Vec<Note>,
    /// The folder name the first copy was saved under, so Save Again lands
    /// in the same place.
    saved_name: Option<std::ffi::OsString>,
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
        let dir = parent.join(name);
        fs::create_dir_all(&dir)?;
        let mut clock = BufWriter::new(File::create(dir.join("clock.csv"))?);
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
            seg_frames: 0,
            frames: 0,
            clock,
            last_clock: None,
            last_block: None,
            paused_at: None,
            resumed_on: None,
            breaks: Vec::new(),
            notes: Vec::new(),
            saved_name: None,
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
        if let Some(w) = self.writer.take() {
            w.finalize().map_err(io_err)?;
        }
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

    fn open_segment(&mut self) -> io::Result<()> {
        let name = format!("audio-{:03}.wav", self.segments.len() + 1);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.info.sample_rate,
            bits_per_sample: self.info.bits,
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
        if let Some(before) = self.paused_at.take() {
            let device = self
                .resumed_on
                .take()
                .unwrap_or_else(|| self.info.device.clone());
            self.start_break(b, before, device)?;
        }
        let full = ((1u64 << (self.info.bits - 1)) - 1) as f32;
        for &s in &b.samples {
            if self.writer.is_none() || self.seg_frames >= SEGMENT_S * fs {
                if let Some(w) = self.writer.take() {
                    w.finalize().map_err(io_err)?;
                }
                self.open_segment()?;
            }
            let w = self.writer.as_mut().expect("segment open");
            w.write_sample((s.clamp(-1.0, 1.0) * full).round() as i32)
                .map_err(io_err)?;
            self.seg_frames += 1;
        }
        self.frames += b.samples.len() as u64;
        let due = match self.last_clock {
            None => true,
            Some(t) => {
                b.at.duration_since(t)
                    .map(|d| d.as_secs_f64() >= CLOCK_EVERY_S)
                    .unwrap_or(false)
            }
        };
        if due {
            let unix = b.at.duration_since(UNIX_EPOCH).unwrap_or_default();
            writeln!(
                self.clock,
                "{}.{:09},{}",
                unix.as_secs(),
                unix.subsec_nanos(),
                self.frames
            )?;
            self.clock.flush()?;
            // Keep the WAV header valid in case the program stops abruptly.
            if let Some(w) = self.writer.as_mut() {
                w.flush().map_err(io_err)?;
            }
            self.last_clock = Some(b.at);
        }
        self.last_block = Some((b.at, self.frames));
        Ok(())
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

    fn write_session(
        &self,
        ended: Option<String>,
        result: Option<&serde_json::Value>,
    ) -> io::Result<()> {
        fs::write(
            self.dir.join("session.json"),
            self.session_text(ended, result)?,
        )
    }

    fn session_text(
        &self,
        ended: Option<String>,
        result: Option<&serde_json::Value>,
    ) -> io::Result<String> {
        let f = SessionFile {
            info: &self.info,
            started_utc: &self.started_utc,
            ended_utc: ended,
            duration_s: self.duration_s(),
            segments: self.segments.clone(),
            notes: &self.notes,
            breaks: &self.breaks,
            result,
        };
        serde_json::to_string_pretty(&f).map_err(io::Error::other)
    }

    /// Note a pause in listening. The clock log gets a line for the last
    /// block before it and another for the first block after it, so both
    /// sides of the gap are measured; audio written after [`Recorder::resume`]
    /// starts a new file and a [`Break`].
    pub fn pause(&mut self) -> io::Result<()> {
        if let Some((at, frames)) = self.last_block {
            if self.last_clock != Some(at) {
                let unix = at.duration_since(UNIX_EPOCH).unwrap_or_default();
                writeln!(
                    self.clock,
                    "{}.{:09},{}",
                    unix.as_secs(),
                    unix.subsec_nanos(),
                    frames
                )?;
            }
            self.paused_at = Some(at);
        }
        self.last_clock = None;
        if let Some(w) = self.writer.as_mut() {
            w.flush().map_err(io_err)?;
        }
        self.clock.flush()?;
        self.note("paused")
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
    fn copy_name(&self, calibre: &str) -> std::ffi::OsString {
        let mut name = stamp_of(&self.started_utc);
        for part in [safe_name(&self.info.watch), safe_name(calibre)] {
            if !part.is_empty() {
                name = format!("{name}_{part}");
            }
        }
        name.into()
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
        mut before: impl FnMut(SaveStep, &Path) -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        let mut names = std::collections::BTreeSet::new();
        for &(name, _) in required_files {
            let mut parts = Path::new(name).components();
            if !matches!(parts.next(), Some(std::path::Component::Normal(_)))
                || parts.next().is_some()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || matches!(
                    name.to_ascii_lowercase().as_str(),
                    "session.json" | "clock.csv"
                )
                || name.to_ascii_lowercase().starts_with("audio-")
                || self.dir.join(name).exists()
                || !names.insert(name.to_ascii_lowercase())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid or reserved companion file name: {name}"),
                ));
            }
        }
        let name = self
            .saved_name
            .clone()
            .unwrap_or_else(|| self.copy_name(calibre));
        fs::create_dir_all(parent)?;
        let parent = fs::canonicalize(parent)?;
        let dest = parent.join(&name);
        let source = fs::canonicalize(&self.dir)?;
        let resolved = if dest.exists() {
            fs::canonicalize(&dest)?
        } else {
            dest.clone()
        };
        if resolved.starts_with(&source) || source.starts_with(&resolved) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the saved copy must be separate from the live recording folder",
            ));
        }
        self.flush()?;
        let session = self.session_text(Some(utc(SystemTime::now())), result)?;
        let mut update = SaveUpdate::new(&parent, &dest)?;
        let open = self.writer.as_ref().and_then(|_| self.segments.last());
        // Enumerate required sources explicitly: a missing segment must not
        // disappear from read_dir and turn an incomplete take into a success.
        let mut sources: std::collections::BTreeSet<PathBuf> =
            self.segment_paths().into_iter().collect();
        sources.insert(self.dir.join("clock.csv"));
        for e in fs::read_dir(&self.dir)? {
            let e = e?;
            if e.file_type()?.is_file() && e.file_name() != "session.json" {
                sources.insert(e.path());
            }
        }
        for source in sources {
            let metadata = fs::symlink_metadata(&source)?;
            if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{} is not a regular file", source.display()),
                ));
            }
            let file = source.file_name().expect("recording file");
            let to = dest.join(file);
            let closed = file.to_str().is_some_and(|n| {
                self.segments.iter().any(|s| s == n) && open.map_or(true, |s| s != n)
            });
            let there = match fs::symlink_metadata(&to) {
                Ok(m) if !m.is_file() => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("{} is not a regular file", to.display()),
                    ))
                }
                Ok(m) => m.len() == metadata.len(),
                Err(e) if e.kind() == io::ErrorKind::NotFound => false,
                Err(e) => return Err(e),
            };
            if closed && there {
                continue;
            }
            let staged = update.staged.join(file);
            before(SaveStep::Stage, &to)?;
            fs::copy(&source, &staged)?;
        }
        for (file, bytes) in std::iter::once(("session.json", session.as_bytes()))
            .chain(required_files.iter().copied())
        {
            before(SaveStep::Stage, &dest.join(file))?;
            fs::write(update.staged.join(file), bytes)?;
        }
        update.install(&mut before)?;
        self.saved_name = Some(name);
        Ok(dest)
    }

    /// Close the files, saving `result` (the readings at the end) with the
    /// settings. Returns the folder.
    pub fn finish(mut self, result: Option<serde_json::Value>) -> io::Result<PathBuf> {
        if let Some(w) = self.writer.take() {
            w.finalize().map_err(io_err)?;
        }
        self.clock.flush()?;
        self.write_session(Some(utc(SystemTime::now())), result.as_ref())?;
        Ok(self.dir)
    }
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
                let had_old = match fs::rename(&to, &backup) {
                    Ok(()) => true,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => false,
                    Err(e) => return Err(e),
                };
                changed.push((file.clone(), had_old));
                before(SaveStep::Install, &to)?;
                fs::rename(self.staged.join(&file), to)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let mut rollback_error = None;
            for (file, had_old) in changed.into_iter().rev() {
                let to = self.dest.join(&file);
                let restored = (|| {
                    before(SaveStep::Restore, &to)?;
                    match fs::remove_file(&to) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                    if had_old {
                        fs::rename(self.backup.join(&file), &to)?;
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
        assert_eq!(pairs.len(), 3); // at 0, 10 and 20 s
        assert!((pairs[1].0 - 11.0).abs() < 1e-9, "{:?}", pairs[1]);
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

    fn save_fixture(name: &str) -> (Recorder, PathBuf) {
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

    fn append_second(r: &mut Recorder, second: u64) {
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
        let first = r.dir().join("audio-001.wav");
        let bytes = fs::read(&first).unwrap();
        // The first segment is closed, so this fault is portable to Windows.
        fs::remove_file(&first).unwrap();
        let parent = base.join("saved");
        assert!(r.save_copy(&parent, "", None).is_err());
        assert!(r.saved_name.is_none());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::write(&first, &bytes).unwrap();
        let dest = r.save_copy(&parent, "", None).unwrap();
        assert_eq!(fs::read(dest.join("audio-001.wav")).unwrap(), bytes);
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
        let parent = base.join("saved");
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
        let parent = base.join("saved");
        let dest = r.save_copy(&parent, "", None).unwrap();
        let first = dest.join("audio-001.wav");
        let original = fs::read(&first).unwrap();
        fs::remove_file(&first).unwrap();
        std::os::unix::fs::symlink(r.dir().join("audio-001.wav"), &first).unwrap();
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
        let copy = r.save_copy(&base.join("saved"), "", None).unwrap();
        // Saved again: the file closed by the pause is not copied again
        // (a mark put in the saved copy stays), the open one is.
        let first = copy.join("audio-001.wav");
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
            (load("audio-001.wav"), load("audio-002.wav")),
            (3 * 8000, 3 * 8000)
        );
        let clock = fs::read_to_string(copy.join("clock.csv")).unwrap();
        let pairs = crate::clock::parse_log(&clock, 8000, 2).unwrap();
        // The start, the last block before the pause, the first after it;
        // frames running on, no line made up across the gap.
        let frames: Vec<u64> = clock
            .lines()
            .filter_map(|l| l.split(',').nth(1)?.parse().ok())
            .collect();
        assert_eq!(frames, [8000, 3 * 8000, 4 * 8000], "{clock}");
        assert_eq!(pairs.len(), 3, "{clock}");
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
        let total: usize = ["audio-001.wav", "audio-002.wav", "audio-003.wav"]
            .iter()
            .map(|f| crate::audio::load(&dir.join(f)).unwrap().samples.len())
            .sum();
        assert_eq!(total, 7 * 8000);
        fs::remove_dir_all(&base).unwrap();
    }
}
