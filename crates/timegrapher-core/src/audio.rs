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

/// The sample rates a recording may have. No tick can be heard below
/// 100 Hz (the floor keeps the quick silent files tests write), and a
/// header past 768 kHz is broken, not a sound card's.
const RATES: std::ops::RangeInclusive<u32> = 100..=768_000;

/// A header's format, refused unless it can be read: a rate of 0 made
/// every chunk 0 samples long, a panic in `analyze` and a hang in the
/// streamed reading (#1643); no channels or bits, or more than 32 bits,
/// would divide by zero or shift past an integer.
fn readable(sample_rate: u32, channels: u32, bits: u32) -> Result<(), AudioError> {
    let bad = |what: String| Err(AudioError::Unsupported(what));
    if !RATES.contains(&sample_rate) {
        return bad(format!(
            "a sample rate of {sample_rate} Hz (this reads {} to {} Hz)",
            RATES.start(),
            RATES.end()
        ));
    }
    if channels == 0 {
        return bad("no channels".into());
    }
    if !(1..=32).contains(&bits) {
        return bad(format!("{bits} bits a sample"));
    }
    Ok(())
}

/// Load a WAV or FLAC file (chosen by extension), averaging channels to mono
/// and scaling integers to [-1, 1).
pub fn load(path: &Path) -> Result<Audio, AudioError> {
    kept(path, |path| match extension(path).as_str() {
        "wav" => load_wav(path),
        "flac" => load_flac(path),
        other => Err(AudioError::Unsupported(format!("file extension '{other}'"))),
    })
}

/// Whether `e` says a file wasn't there to open.
fn not_found(e: &AudioError) -> bool {
    match e {
        AudioError::Io(e) | AudioError::Wav(hound::Error::IoError(e)) => {
            e.kind() == std::io::ErrorKind::NotFound
        }
        AudioError::Flac(claxon::Error::IoError(e)) => e.kind() == std::io::ErrorKind::NotFound,
        _ => false,
    }
}

/// Read `path` with `read`, or, when it names a WAV that is no longer
/// there but its FLAC is, read that: the hour was kept as FLAC (#503)
/// after the list naming it was made, for instance while a take was open.
/// The FLAC holds the same samples, checked before the WAV went.
pub(crate) fn kept<T>(
    path: &Path,
    mut read: impl FnMut(&Path) -> Result<T, AudioError>,
) -> Result<T, AudioError> {
    match read(path) {
        Err(e) if not_found(&e) => match kept_path(path) {
            Some(flac) => read(&flac),
            None => Err(e),
        },
        r => r,
    }
}

/// The FLAC that `path` was kept as, when `path` names a WAV that is no
/// longer there and that FLAC is ([`kept`]'s rule, for a reader of its
/// own such as the app's Scope).
pub fn kept_path(path: &Path) -> Option<std::path::PathBuf> {
    if !crate::flac::is_wav(path) || path.exists() {
        return None;
    }
    Some(crate::flac::flac_path(path)).filter(|f| f.is_file())
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
    readable(
        spec.sample_rate,
        spec.channels.into(),
        spec.bits_per_sample.into(),
    )?;
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
    readable(info.sample_rate, info.channels, info.bits_per_sample)?;
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
    kept(path, info_of)
}

fn info_of(path: &Path) -> Result<AudioInfo, AudioError> {
    match extension(path).as_str() {
        "wav" => {
            let r = hound::WavReader::open(path).map_err(AudioError::Wav)?;
            let spec = r.spec();
            readable(
                spec.sample_rate,
                spec.channels.into(),
                spec.bits_per_sample.into(),
            )?;
            Ok(AudioInfo {
                sample_rate: spec.sample_rate,
                frames: Some(r.duration() as u64),
                bytes_per_frame: spec.channels as u32 * spec.bits_per_sample.div_ceil(8) as u32,
            })
        }
        "flac" => {
            let r = claxon::FlacReader::open(path).map_err(AudioError::Flac)?;
            let i = r.streaminfo();
            readable(i.sample_rate, i.channels, i.bits_per_sample)?;
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
pub fn stream(path: &Path, block: usize, f: impl FnMut(&[f32])) -> Result<AudioInfo, AudioError> {
    stream_head(path, usize::MAX, block, f)
}

/// [`stream`] for the first `frames` frames only: decoding stops there,
/// so the rest of the file is neither read nor checked.
pub fn stream_head(
    path: &Path,
    frames: usize,
    block: usize,
    mut f: impl FnMut(&[f32]),
) -> Result<AudioInfo, AudioError> {
    // A file is only found missing on opening it, before any frame is
    // passed on, so reading the FLAC instead passes each frame once.
    kept(path, |p| stream_head_of(p, frames, block, &mut f))
}

fn stream_head_of(
    path: &Path,
    frames: usize,
    block: usize,
    f: &mut impl FnMut(&[f32]),
) -> Result<AudioInfo, AudioError> {
    let block = block.max(1);
    let mut buf: Vec<f32> = Vec::with_capacity(block);
    let mut left = frames;
    // Pass one frame on; false once `frames` have been.
    let mut push = |v: f32, buf: &mut Vec<f32>| {
        if left == 0 {
            return false;
        }
        left -= 1;
        buf.push(v);
        if buf.len() == block {
            f(buf);
            buf.clear();
        }
        left > 0
    };
    let info = info_of(path)?;
    if frames > 0 {
        match extension(path).as_str() {
            "wav" => {
                let mut reader = hound::WavReader::open(path).map_err(AudioError::Wav)?;
                let spec = reader.spec();
                let ch = spec.channels as usize;
                let mut frame = Vec::with_capacity(ch);
                let mut emit = |v: f32, buf: &mut Vec<f32>| {
                    frame.push(v);
                    if frame.len() < ch {
                        return true;
                    }
                    let more = push(frame.iter().sum::<f32>() / ch as f32, buf);
                    frame.clear();
                    more
                };
                match spec.sample_format {
                    hound::SampleFormat::Float => {
                        for s in reader.samples::<f32>() {
                            if !emit(s.map_err(AudioError::Wav)?, &mut buf) {
                                break;
                            }
                        }
                    }
                    hound::SampleFormat::Int => {
                        let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
                        for s in reader.samples::<i32>() {
                            if !emit(s.map_err(AudioError::Wav)? as f32 * scale, &mut buf) {
                                break;
                            }
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
                'blocks: while let Some(b) =
                    blocks.read_next_or_eof(store).map_err(AudioError::Flac)?
                {
                    let ch = b.channels();
                    for i in 0..b.duration() {
                        let sum: i32 = (0..ch).map(|c| b.sample(c, i)).sum();
                        if !push(sum as f32 * scale / ch as f32, &mut buf) {
                            break 'blocks;
                        }
                    }
                    store = b.into_buffer();
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Every block `stream_head` hands on, joined.
    fn head(path: &Path, frames: usize) -> Result<Vec<f32>, AudioError> {
        let mut x = Vec::new();
        stream_head(path, frames, 1000, |b| x.extend_from_slice(b))?;
        Ok(x)
    }

    /// A WAV and a FLAC of the same second of sound, each also cut short
    /// a quarter before its end, as a file still being written or a damaged
    /// one is.
    fn files(dir: &Path) -> [(PathBuf, PathBuf); 2] {
        let fs = 8000;
        let samples: Vec<f32> = (0..fs)
            .map(|i| ((i % 200) as f32 - 100.0) / 200.0)
            .collect();
        let wav = dir.join("a.wav");
        write_wav(
            &wav,
            &Audio {
                samples: samples.clone(),
                sample_rate: fs as u32,
            },
        )
        .unwrap();
        let flac = dir.join("a.flac");
        let pcm = crate::excerpt::Pcm {
            sample_rate: fs as u32,
            channels: 1,
            bits_per_sample: 16,
            samples: samples
                .iter()
                .map(|v| (v * 32767.0).round() as i32)
                .collect(),
        };
        crate::excerpt::write_flac(&flac, &pcm).unwrap();
        [wav, flac].map(|p| {
            let bytes = std::fs::read(&p).unwrap();
            let cut = p.with_file_name(format!("cut-{}", p.file_name().unwrap().to_string_lossy()));
            std::fs::write(&cut, &bytes[..bytes.len() * 3 / 4]).unwrap();
            (p, cut)
        })
    }

    /// A WAV whose header says 0 Hz, or 2 GHz, is refused with a plain
    /// error by every reader, the streamed analysis too, where it made
    /// `analyze` panic and the stream hang (#1643).
    #[test]
    fn a_header_with_no_sample_rate_is_refused_by_every_reader() {
        let dir = std::env::temp_dir().join(format!("tg-audio-rate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.wav");
        let audio = Audio {
            samples: vec![0.25; 8000],
            sample_rate: 8000,
        };
        write_wav(&good, &audio).unwrap();
        let bytes = std::fs::read(&good).unwrap();
        assert_eq!(&bytes[24..28], &8000u32.to_le_bytes(), "the rate's place");
        for rate in [0u32, 99, 2_000_000_000] {
            let mut b = bytes.clone();
            b[24..28].copy_from_slice(&rate.to_le_bytes());
            b[28..32].copy_from_slice(&rate.wrapping_mul(2).to_le_bytes());
            let p = dir.join(format!("rate-{rate}.wav"));
            std::fs::write(&p, &b).unwrap();
            let want = format!("{rate} Hz");
            let says = |e: AudioError| {
                let e = e.to_string();
                assert!(e.contains(&want), "{rate}: {e}");
            };
            says(info(&p).unwrap_err());
            says(load(&p).unwrap_err());
            says(stream(&p, 1000, |_| panic!("no samples from a broken header")).unwrap_err());
            let cfg = crate::stream::StreamConfig::default();
            says(crate::stream::analyze_file(&p, &cfg, |_| {}).unwrap_err());
        }
        // The edges read.
        for rate in [100u32, 768_000] {
            let p = dir.join(format!("edge-{rate}.wav"));
            let a = Audio {
                samples: vec![0.0; 10],
                sample_rate: rate,
            };
            write_wav(&p, &a).unwrap();
            assert_eq!(info(&p).unwrap().sample_rate, rate);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_head_is_the_streams_first_frames_and_reads_no_further() {
        let dir = std::env::temp_dir().join(format!("tg-audio-head-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (whole, cut) in files(&dir) {
            let all = head(&whole, usize::MAX).unwrap();
            assert_eq!(all.len(), 8000, "{}", whole.display());
            // The first frames, exactly as the whole stream gives them,
            // across a block boundary and on one.
            for n in [0, 1, 999, 1000, 2500, 8000, 9000] {
                assert_eq!(
                    head(&whole, n).unwrap(),
                    all[..n.min(8000)],
                    "{n} of {}",
                    whole.display()
                );
            }
            // A file cut short fails to stream to its end, but its head
            // reads: decoding stopped before the cut.
            assert!(head(&cut, usize::MAX).is_err(), "{}", cut.display());
            assert_eq!(head(&cut, 2500).unwrap(), all[..2500], "{}", cut.display());
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_wav_kept_as_flac_since_it_was_listed_is_read_from_its_flac() {
        let dir = std::env::temp_dir().join(format!("tg-audio-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("audio-001.wav");
        let sound = Audio {
            sample_rate: 8000,
            samples: (0..8000)
                .map(|i| ((i % 50) as f32 - 25.0) / 100.0)
                .collect(),
        };
        write_wav(&wav, &sound).unwrap();
        let heard = load(&wav).unwrap().samples;
        // Listed as a WAV, then kept as FLAC before it was read.
        crate::flac::keep_as_flac(&wav).unwrap();
        assert!(!wav.exists());
        assert_eq!(load(&wav).unwrap().samples, heard);
        assert_eq!(info(&wav).unwrap().frames, Some(8000));
        assert_eq!(head(&wav, usize::MAX).unwrap(), heard);
        assert_eq!(head(&wav, 100).unwrap(), heard[..100]);
        let pcm = crate::excerpt::read(&wav).unwrap();
        assert_eq!(pcm.samples.len(), 8000);
        // A WAV with no FLAC is still missing, and said so.
        let gone = dir.join("audio-002.wav");
        assert!(not_found(&load(&gone).unwrap_err()));
        assert!(not_found(&info(&gone).unwrap_err()));
        assert!(not_found(&head(&gone, 10).unwrap_err()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
