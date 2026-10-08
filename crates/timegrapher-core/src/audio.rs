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
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
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
