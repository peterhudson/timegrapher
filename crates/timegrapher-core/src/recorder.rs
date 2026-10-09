//! Saving what the microphone hears, so every test can be analysed again.
//!
//! A recording is a folder holding the audio as WAV files of at most an
//! hour each (`audio-001.wav`, `audio-002.wav`, ...), a clock log
//! (`clock.csv`: system time against frames captured, every 10 s) and
//! `session.json` with the watch, position, lift angle and settings.
//! `timegrapher long <folder>/ --clock <folder>/clock.csv` reads it as one
//! continuous recording calibrated against the system clock.

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
    /// Readings at the end, as the program saw them.
    result: Option<&'a serde_json::Value>,
}

const SEGMENT_S: u64 = 3600;
const CLOCK_EVERY_S: f64 = 10.0;

pub struct Recorder {
    dir: PathBuf,
    info: SessionInfo,
    started_utc: String,
    writer: Option<hound::WavWriter<BufWriter<File>>>,
    segments: Vec<String>,
    seg_frames: u64,
    frames: u64,
    clock: BufWriter<File>,
    last_clock: Option<SystemTime>,
    notes: Vec<Note>,
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
            info,
            started_utc,
            writer: None,
            segments: Vec::new(),
            seg_frames: 0,
            frames: 0,
            clock,
            last_clock: None,
            notes: Vec::new(),
        };
        r.write_session(None, None)?;
        Ok(r)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn duration_s(&self) -> f64 {
        self.frames as f64 / self.info.sample_rate as f64
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
        Ok(())
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
            result,
        };
        let text = serde_json::to_string_pretty(&f).map_err(io::Error::other)?;
        fs::write(self.dir.join("session.json"), text)
    }

    /// Note a pause in listening. Audio written after it follows straight
    /// on in the WAV file, and the clock log gets a line with the first block
    /// after it, so the gap shows in `clock.csv`.
    pub fn pause(&mut self) -> io::Result<()> {
        self.last_clock = None;
        if let Some(w) = self.writer.as_mut() {
            w.flush().map_err(io_err)?;
        }
        self.clock.flush()?;
        self.note("paused")
    }

    /// Copy the recording so far into a new folder of the same name under
    /// `parent`, keeping on recording here. `result` is saved with the
    /// settings as the readings at the time of the copy. Returns the copy.
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
        let name = self
            .dir
            .file_name()
            .ok_or_else(|| io::Error::other("no folder name"))?;
        let dest = parent.join(name);
        if dest == self.dir {
            return Ok(dest);
        }
        fs::create_dir_all(&dest)?;
        for e in fs::read_dir(&self.dir)? {
            let e = e?;
            if e.file_type()?.is_file() {
                fs::copy(e.path(), dest.join(e.file_name()))?;
            }
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
        assert!(session.contains("turned to DD"));
        assert!(dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("_Test_3235_DU"));
        fs::remove_dir_all(&parent).unwrap();
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
        // A minute later, listening again.
        for k in 63..65 {
            r.write(&block(k)).unwrap();
        }
        let copy = r.save_copy(&base.join("saved"), None).unwrap();
        let audio = crate::audio::load(&copy.join("audio-001.wav")).unwrap();
        assert_eq!(audio.samples.len(), 5 * 8000);
        let clock = fs::read_to_string(copy.join("clock.csv")).unwrap();
        let pairs = crate::clock::parse_log(&clock, 8000, 2).unwrap();
        // One line at the start, one after the pause.
        assert_eq!(pairs.len(), 2, "{clock}");
        let session = fs::read_to_string(copy.join("session.json")).unwrap();
        assert!(session.contains("paused"));
        // Recording carries on in the original folder.
        r.write(&block(65)).unwrap();
        let dir = r.finish(None).unwrap();
        assert_eq!(
            crate::audio::load(&dir.join("audio-001.wav"))
                .unwrap()
                .samples
                .len(),
            6 * 8000
        );
        fs::remove_dir_all(&base).unwrap();
    }
}
