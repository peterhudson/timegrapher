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
        let stamp: String = started_utc[..19]
            .chars()
            .map(|c| if c == ':' { '-' } else { c })
            .collect();
        let mut name = stamp;
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
        let text = serde_json::to_string_pretty(&f).map_err(io::Error::other)?;
        fs::write(self.dir.join("session.json"), text)
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

    /// The name for a copy: the take's own, but a take that has moved
    /// through positions ends in `_positions`, not the one it began in.
    fn copy_name(&self) -> io::Result<std::ffi::OsString> {
        let name = self
            .dir
            .file_name()
            .ok_or_else(|| io::Error::other("no folder name"))?;
        let moved = self.notes.iter().any(|n| n.text.starts_with("position "));
        let suffix = format!("_{}", safe_name(&self.first_position));
        Ok(match name.to_str().and_then(|n| n.strip_suffix(&suffix)) {
            Some(stem) if moved => format!("{stem}_positions").into(),
            _ => name.to_os_string(),
        })
    }

    /// Copy the recording so far into a new folder of the same name under
    /// `parent`, keeping on recording here. `result` is saved with the
    /// settings as the readings at the time of the copy. Returns the copy.
    ///
    /// Saved again into the same place, only what changed is written: an
    /// audio file closed by a pause that is already there at its full
    /// length is left alone, so Save Again copies the new sound, the open
    /// file, clock.csv and session.json.
    pub fn save_copy(
        &mut self,
        parent: &Path,
        result: Option<&serde_json::Value>,
    ) -> io::Result<PathBuf> {
        if let Some(w) = self.writer.as_mut() {
            w.flush().map_err(io_err)?;
        }
        self.clock.flush()?;
        self.write_session(Some(utc(SystemTime::now())), result)?;
        let name = match &self.saved_name {
            Some(n) => n.clone(),
            None => self.copy_name()?,
        };
        self.saved_name = Some(name.clone());
        let dest = parent.join(&name);
        if dest == self.dir {
            return Ok(dest);
        }
        fs::create_dir_all(&dest)?;
        let open = self
            .writer
            .as_ref()
            .and_then(|_| self.segments.last())
            .map(|n| n.as_str());
        for e in fs::read_dir(&self.dir)? {
            let e = e?;
            if !e.file_type()?.is_file() {
                continue;
            }
            let name = e.file_name();
            let to = dest.join(&name);
            let closed = name
                .to_str()
                .is_some_and(|n| self.segments.iter().any(|s| s == n) && Some(n) != open);
            let there =
                fs::metadata(&to).is_ok_and(|m| m.len() == e.metadata().map_or(0, |m| m.len()));
            if closed && there {
                continue;
            }
            fs::copy(e.path(), to)?;
        }
        // Back to an open session here.
        self.write_session(None, None)?;
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

    /// A take that moved through positions is saved under `_positions`,
    /// not the position it began in, and saved again in the same place.
    #[test]
    fn a_moved_take_is_saved_as_positions() {
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
        let first = r.save_copy(&base.join("saved"), None).unwrap();
        let name = first.file_name().unwrap().to_str().unwrap().to_string();
        assert!(name.ends_with("_Test_Watch_positions"), "{name}");
        r.note("position CL").unwrap();
        assert_eq!(r.save_copy(&base.join("saved"), None).unwrap(), first);
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
        let copy = r.save_copy(&base.join("saved"), None).unwrap();
        // Saved again: the file closed by the pause is not copied again
        // (a mark put in the saved copy stays), the open one is.
        let first = copy.join("audio-001.wav");
        let mut bytes = fs::read(&first).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&first, &bytes).unwrap();
        r.write(&block(65)).unwrap();
        assert_eq!(r.save_copy(&base.join("saved"), None).unwrap(), copy);
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
