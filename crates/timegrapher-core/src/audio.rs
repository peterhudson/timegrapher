//! Loading recordings from WAV or FLAC into mono `f32` samples.

use std::fmt;
use std::path::Path;

/// A mono recording.
#[derive(Debug, Clone)]
pub struct Audio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Audio {
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }
}

#[derive(Debug)]
pub enum AudioError {
    Io(std::io::Error),
    Wav(hound::Error),
    Flac(claxon::Error),
    Unsupported(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::Io(e) => write!(f, "{e}"),
            AudioError::Wav(e) => write!(f, "WAV: {e}"),
            AudioError::Flac(e) => write!(f, "FLAC: {e}"),
            AudioError::Unsupported(s) => write!(f, "unsupported audio: {s}"),
        }
    }
}

impl std::error::Error for AudioError {}

/// Load a WAV or FLAC file (chosen by extension), averaging channels to mono
/// and scaling integers to [-1, 1).
pub fn load(path: &Path) -> Result<Audio, AudioError> {
    match extension(path).as_str() {
        "wav" => load_wav(path),
        "flac" => load_flac(path),
        other => Err(AudioError::Unsupported(format!("file extension '{other}'"))),
    }
}

fn to_mono(interleaved: Vec<f32>, channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved;
    }
    interleaved
        .chunks_exact(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn load_wav(path: &Path) -> Result<Audio, AudioError> {
    let mut reader = hound::WavReader::open(path).map_err(AudioError::Wav)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(AudioError::Wav)?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(AudioError::Wav)?
        }
    };
    Ok(Audio {
        samples: to_mono(samples, spec.channels as usize),
        sample_rate: spec.sample_rate,
    })
}

fn load_flac(path: &Path) -> Result<Audio, AudioError> {
    let mut reader = claxon::FlacReader::open(path).map_err(AudioError::Flac)?;
    let info = reader.streaminfo();
    let scale = 1.0 / (1u64 << (info.bits_per_sample - 1)) as f32;
    let samples: Vec<f32> = reader
        .samples()
        .map(|s| s.map(|v| v as f32 * scale))
        .collect::<Result<_, _>>()
        .map_err(AudioError::Flac)?;
    Ok(Audio {
        samples: to_mono(samples, info.channels as usize),
        sample_rate: info.sample_rate,
    })
}

/// Format of a recording, read from its header without decoding it.
#[derive(Debug, Clone, Copy)]
pub struct AudioInfo {
    pub sample_rate: u32,
    /// Length in frames, when the header gives it.
    pub frames: Option<u64>,
    /// Bytes per frame as captured (channels times bytes per sample).
    pub bytes_per_frame: u32,
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn info(path: &Path) -> Result<AudioInfo, AudioError> {
    match extension(path).as_str() {
        "wav" => {
            let r = hound::WavReader::open(path).map_err(AudioError::Wav)?;
            let spec = r.spec();
            Ok(AudioInfo {
                sample_rate: spec.sample_rate,
                frames: Some(r.duration() as u64),
                bytes_per_frame: spec.channels as u32 * spec.bits_per_sample.div_ceil(8) as u32,
            })
        }
        "flac" => {
            let r = claxon::FlacReader::open(path).map_err(AudioError::Flac)?;
            let i = r.streaminfo();
            Ok(AudioInfo {
                sample_rate: i.sample_rate,
                frames: i.samples,
                bytes_per_frame: i.channels * i.bits_per_sample.div_ceil(8),
            })
        }
        other => Err(AudioError::Unsupported(format!("file extension '{other}'"))),
    }
}

/// Decode a WAV or FLAC file block by block, calling `f` with mono samples
/// in blocks of `block` frames (the last may be shorter). Memory stays
/// bounded, so recordings of hours or days can be read.
pub fn stream(
    path: &Path,
    block: usize,
    mut f: impl FnMut(&[f32]),
) -> Result<AudioInfo, AudioError> {
    let block = block.max(1);
    let mut buf: Vec<f32> = Vec::with_capacity(block);
    let mut push = |v: f32, buf: &mut Vec<f32>| {
        buf.push(v);
        if buf.len() == block {
            f(buf);
            buf.clear();
        }
    };
    let info = info(path)?;
    match extension(path).as_str() {
        "wav" => {
            let mut reader = hound::WavReader::open(path).map_err(AudioError::Wav)?;
            let spec = reader.spec();
            let ch = spec.channels as usize;
            let mut frame = Vec::with_capacity(ch);
            let mut emit = |v: f32, buf: &mut Vec<f32>| {
                frame.push(v);
                if frame.len() == ch {
                    push(frame.iter().sum::<f32>() / ch as f32, buf);
                    frame.clear();
                }
            };
            match spec.sample_format {
                hound::SampleFormat::Float => {
                    for s in reader.samples::<f32>() {
                        emit(s.map_err(AudioError::Wav)?, &mut buf);
                    }
                }
                hound::SampleFormat::Int => {
                    let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
                    for s in reader.samples::<i32>() {
                        emit(s.map_err(AudioError::Wav)? as f32 * scale, &mut buf);
                    }
                }
            }
        }
        _ => {
            let mut reader = claxon::FlacReader::open(path).map_err(AudioError::Flac)?;
            let si = reader.streaminfo();
            let scale = 1.0 / (1u64 << (si.bits_per_sample - 1)) as f32;
            let mut blocks = reader.blocks();
            let mut store = Vec::new();
            while let Some(b) = blocks.read_next_or_eof(store).map_err(AudioError::Flac)? {
                let ch = b.channels();
                for i in 0..b.duration() {
                    let sum: i32 = (0..ch).map(|c| b.sample(c, i)).sum();
                    push(sum as f32 * scale / ch as f32, &mut buf);
                }
                store = b.into_buffer();
            }
        }
    }
    if !buf.is_empty() {
        f(&buf);
    }
    Ok(info)
}

/// Write a mono 16-bit WAV (used for synthetic test signals).
pub fn write_wav(path: &Path, audio: &Audio) -> Result<(), AudioError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: audio.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(AudioError::Wav)?;
    for &s in &audio.samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        w.write_sample(v).map_err(AudioError::Wav)?;
    }
    w.finalize().map_err(AudioError::Wav)
}
