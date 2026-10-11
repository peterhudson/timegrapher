//! Cutting a stretch out of a recording without changing a sample: read
//! the integer samples of WAV or FLAC files, and write them as FLAC, which
//! is lossless, so the cut holds exactly what the original held.
//!
//! FLAC is written by the `flacenc` crate, and every stream it writes is
//! decoded again with `claxon` and checked sample for sample before it is
//! used, so an encoder fault can never change a reading in a bundle.

use crate::audio::{self, AudioError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Integer samples, interleaved by channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub samples: Vec<i32>,
}

impl Pcm {
    /// Samples per channel.
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration_s(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }
}

/// `len_s` seconds from `from_s` of the files read one after another (the
/// segments of one recording), with their samples untouched. The files must
/// share a sample rate, channel count and sample size.
pub fn cut(files: &[PathBuf], from_s: f64, len_s: f64) -> Result<Pcm, AudioError> {
    let mut out: Option<Pcm> = None;
    let mut pos = 0u64; // frames before the current file
    for f in files {
        let info = audio::info(f)?;
        let rate = info.sample_rate as f64;
        let a = (from_s.max(0.0) * rate).round() as u64;
        let b = ((from_s.max(0.0) + len_s.max(0.0)) * rate).round() as u64;
        if pos >= b {
            break;
        }
        if let Some(n) = info.frames {
            if pos + n <= a {
                pos += n;
                continue;
            }
        }
        let p = read(f)?;
        let n = p.frames() as u64;
        if let Some(o) = &out {
            if (o.sample_rate, o.channels, o.bits_per_sample)
                != (p.sample_rate, p.channels, p.bits_per_sample)
            {
                return Err(AudioError::Unsupported(format!(
                    "{} does not match the segments before it",
                    f.display()
                )));
            }
        }
        let lo = a.saturating_sub(pos).min(n) as usize;
        let hi = b.saturating_sub(pos).min(n) as usize;
        let ch = p.channels as usize;
        let part = &p.samples[lo * ch..hi * ch];
        match &mut out {
            Some(o) => o.samples.extend_from_slice(part),
            None => {
                out = Some(Pcm {
                    samples: part.to_vec(),
                    ..p
                })
            }
        }
        pos += n;
    }
    out.ok_or_else(|| AudioError::Unsupported("no audio in that stretch".into()))
}

/// Every sample of a WAV or FLAC file.
pub fn read(path: &Path) -> Result<Pcm, AudioError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "wav" => {
            let mut r = hound::WavReader::open(path).map_err(AudioError::Wav)?;
            let s = r.spec();
            if s.sample_format != hound::SampleFormat::Int {
                return Err(AudioError::Unsupported(
                    "floating-point WAV can't be cut losslessly to FLAC".into(),
                ));
            }
            let samples = r
                .samples::<i32>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(AudioError::Wav)?;
            Ok(Pcm {
                sample_rate: s.sample_rate,
                channels: s.channels,
                bits_per_sample: s.bits_per_sample,
                samples,
            })
        }
        "flac" => {
            let mut r = claxon::FlacReader::open(path).map_err(AudioError::Flac)?;
            let s = r.streaminfo();
            let samples = r
                .samples()
                .collect::<Result<Vec<_>, _>>()
                .map_err(AudioError::Flac)?;
            Ok(Pcm {
                sample_rate: s.sample_rate,
                channels: s.channels as u16,
                bits_per_sample: s.bits_per_sample as u16,
                samples,
            })
        }
        other => Err(AudioError::Unsupported(format!("file extension '{other}'"))),
    }
}

/// Write `pcm` as a FLAC file.
pub fn write_flac(path: &Path, pcm: &Pcm) -> Result<(), AudioError> {
    let bytes = encode_flac(pcm)?;
    let mut f = std::fs::File::create(path).map_err(AudioError::Io)?;
    f.write_all(&bytes).map_err(AudioError::Io)
}

/// The bytes of a FLAC file holding `pcm`, checked by decoding them again.
pub fn encode_flac(pcm: &Pcm) -> Result<Vec<u8>, AudioError> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    let ch = pcm.channels as usize;
    let bps = pcm.bits_per_sample as usize;
    let unsupported = |what: String| Err(AudioError::Unsupported(what));
    if !(4..=24).contains(&bps) {
        return unsupported(format!("{bps}-bit samples"));
    }
    if !(1..=8).contains(&ch) || pcm.samples.len() % ch != 0 {
        return unsupported(format!("{ch} channels of {bps}-bit samples"));
    }
    if !(1..=655_350).contains(&pcm.sample_rate) {
        return unsupported(format!("sample rate {}", pcm.sample_rate));
    }
    let failed = |e: String| AudioError::Unsupported(format!("FLAC encoder: {e}"));
    let cfg = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, e)| failed(format!("{e:?}")))?;
    let src =
        flacenc::source::MemSource::from_samples(&pcm.samples, ch, bps, pcm.sample_rate as usize);
    let mut stream = flacenc::encode_with_fixed_block_size(&cfg, src, cfg.block_size)
        .map_err(|e| failed(format!("{e:?}")))?;
    // flacenc counts a short last block in the minimum block size, which
    // can then be under 16, and claxon (rightly) refuses such a stream. A
    // fixed-block stream names its block size as both, as `flac` writes it.
    stream
        .stream_info_mut()
        .set_block_sizes(cfg.block_size, cfg.block_size)
        .map_err(|e| failed(format!("{e:?}")))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| failed(format!("{e:?}")))?;
    let bytes = sink.as_slice().to_vec();
    if !decodes_to(&bytes, pcm) {
        return Err(failed("the stream does not decode to its samples".into()));
    }
    Ok(bytes)
}

/// Whether `bytes` is a FLAC stream holding exactly `pcm`.
fn decodes_to(bytes: &[u8], pcm: &Pcm) -> bool {
    let Ok(mut r) = claxon::FlacReader::new(std::io::Cursor::new(bytes)) else {
        return false;
    };
    let s = r.streaminfo();
    if s.sample_rate != pcm.sample_rate
        || s.channels != pcm.channels as u32
        || s.bits_per_sample != pcm.bits_per_sample as u32
    {
        return false;
    }
    let mut back = r.samples();
    pcm.samples
        .iter()
        .all(|&want| matches!(back.next(), Some(Ok(v)) if v == want))
        && back.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(p: &Pcm) {
        let bytes = encode_flac(p).unwrap();
        let mut r = claxon::FlacReader::new(std::io::Cursor::new(bytes)).unwrap();
        let s = r.streaminfo();
        assert_eq!(s.sample_rate, p.sample_rate);
        assert_eq!(s.channels as u16, p.channels);
        assert_eq!(s.bits_per_sample as u16, p.bits_per_sample);
        assert_eq!(s.samples, Some(p.frames() as u64));
        let back: Vec<i32> = r.samples().map(|v| v.unwrap()).collect();
        assert_eq!(back, p.samples);
    }

    fn noise(n: usize, seed: u64, amp: i32) -> Vec<i32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((s >> 33) as i64 % (2 * amp as i64 + 1) - amp as i64) as i32
            })
            .collect()
    }

    /// Ticks at 8 a second over a quiet background, as a watch on a stand.
    fn ticks(n: usize) -> Vec<i32> {
        let mut x = noise(n, 1, 40);
        for t in (0..x.len()).step_by(6000) {
            for i in 0..300.min(x.len() - t) {
                x[t + i] += (20000.0 * (-(i as f64) / 60.0).exp() * (i as f64 * 0.9).sin()) as i32;
            }
        }
        x
    }

    /// A minute of ticks took 2,592,913 bytes with the encoder this crate
    /// had before `flacenc` (fixed predictors, 4096-sample blocks); a bundle
    /// may not grow by more than 5%.
    #[test]
    fn excerpts_stay_as_small() {
        let p = Pcm {
            sample_rate: 48000,
            channels: 1,
            bits_per_sample: 16,
            samples: ticks(48000 * 60),
        };
        let n = encode_flac(&p).unwrap().len();
        assert!(n as f64 <= 2_592_913.0 * 1.05, "{n} bytes");
    }

    #[test]
    fn a_stream_that_does_not_decode_to_its_samples_is_caught() {
        let p = Pcm {
            sample_rate: 48000,
            channels: 1,
            bits_per_sample: 16,
            samples: ticks(48000),
        };
        let bytes = encode_flac(&p).unwrap();
        assert!(decodes_to(&bytes, &p));
        let mut other = p.clone();
        other.samples[1000] += 1;
        assert!(!decodes_to(&bytes, &other));
        other.samples.pop();
        assert!(!decodes_to(&bytes, &other));
        assert!(!decodes_to(&bytes[..bytes.len() / 2], &p));
    }

    #[test]
    fn mono_16_bit_ticks_and_noise() {
        let x = ticks(48000 * 3 + 123);
        round_trip(&Pcm {
            sample_rate: 48000,
            channels: 1,
            bits_per_sample: 16,
            samples: x,
        });
    }

    #[test]
    fn extremes_silence_and_short_blocks() {
        let mut x = vec![0i32; 5000];
        x.extend([i16::MAX as i32, i16::MIN as i32].repeat(3000));
        x.extend(noise(3, 2, 30000));
        round_trip(&Pcm {
            sample_rate: 44100,
            channels: 1,
            bits_per_sample: 16,
            samples: x,
        });
        round_trip(&Pcm {
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 16,
            samples: vec![7],
        });
    }

    #[test]
    fn stereo_24_bit() {
        let l = noise(10_000, 3, 8_000_000);
        let r = noise(10_000, 4, 1000);
        let x = l.iter().zip(&r).flat_map(|(&a, &b)| [a, b]).collect();
        round_trip(&Pcm {
            sample_rate: 96000,
            channels: 2,
            bits_per_sample: 24,
            samples: x,
        });
    }

    #[test]
    fn frame_numbers_past_one_byte() {
        // 200 blocks: frame numbers past 127 take two bytes.
        round_trip(&Pcm {
            sample_rate: 48000,
            channels: 1,
            bits_per_sample: 16,
            samples: noise(4096 * 200 + 5, 5, 100),
        });
    }

    #[test]
    fn cut_spans_segments() {
        let dir = std::env::temp_dir().join(format!("tg-excerpt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 1000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut files = Vec::new();
        for k in 0..3 {
            let f = dir.join(format!("audio-{k}.wav"));
            let mut w = hound::WavWriter::create(&f, spec).unwrap();
            for i in 0..2000 {
                w.write_sample((k * 2000 + i) as i16).unwrap();
            }
            w.finalize().unwrap();
            files.push(f);
        }
        let p = cut(&files, 1.5, 3.0).unwrap();
        assert_eq!(p.samples, (1500..4500).collect::<Vec<i32>>());
        let flac = dir.join("cut.flac");
        write_flac(&flac, &p).unwrap();
        assert_eq!(read(&flac).unwrap(), p);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
