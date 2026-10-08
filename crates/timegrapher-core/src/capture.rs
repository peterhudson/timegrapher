//! Audio input without a screen: the sound input devices and what they
//! offer, capture from one of them, a file replayed at its own speed, and
//! the level of a block of samples. The desktop app is one user of this;
//! the CLI or an agent setting up a microphone can make the same calls.
//!
//! Device access needs the `capture` feature, which links the platform's
//! audio library (ALSA on Linux, CoreAudio, WASAPI); file replay and level
//! checks work without it.

use crate::audio;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// A block of mono samples in [-1, 1] and the system time it arrived.
#[derive(Debug, Clone)]
pub struct Block {
    pub samples: Vec<f32>,
    pub at: SystemTime,
}

#[derive(Debug)]
pub enum Event {
    Audio(Block),
    /// A replayed file has reached its end.
    End,
    Error(String),
}

/// A running source of audio. Dropping it stops the source.
pub struct Capture {
    pub rx: Receiver<Event>,
    pub sample_rate: u32,
    /// Bits per sample the source delivers (16 for the usual USB
    /// timegrapher microphones), for the format of a saved recording.
    pub bits: u16,
    /// Device name or file name.
    pub label: String,
    pub is_file: bool,
    stop: Arc<AtomicBool>,
    #[cfg(feature = "capture")]
    _stream: Option<cpal::Stream>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Peak and RMS of some audio, and how much of it is clipped.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Level {
    pub samples: usize,
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
    /// Fraction of samples at or within 0.1% of full scale.
    pub clipped_fraction: f64,
}

impl Level {
    pub fn of(x: &[f32]) -> Level {
        if x.is_empty() {
            return Level {
                peak_dbfs: f64::NEG_INFINITY,
                rms_dbfs: f64::NEG_INFINITY,
                ..Default::default()
            };
        }
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
        let ms = x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len() as f64;
        let clipped = x.iter().filter(|v| v.abs() >= 0.999).count();
        Level {
            samples: x.len(),
            peak_dbfs: 20.0 * peak.max(1e-12).log10(),
            rms_dbfs: 10.0 * ms.max(1e-24).log10(),
            clipped_fraction: clipped as f64 / x.len() as f64,
        }
    }
}

/// Replay a WAV or FLAC file at its own speed, as if the watch were on the
/// microphone.
pub fn replay_file(path: &Path) -> Result<Capture, String> {
    let info = audio::info(path).map_err(|e| e.to_string())?;
    let fs = info.sample_rate;
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let p = path.to_path_buf();
    std::thread::spawn(move || {
        let start = Instant::now();
        let mut sent = 0u64;
        let r = audio::stream(&p, (fs / 50).max(1) as usize, |b| {
            if stop2.load(Ordering::Relaxed) {
                return;
            }
            sent += b.len() as u64;
            let _ = tx.send(Event::Audio(Block {
                samples: b.to_vec(),
                at: SystemTime::now(),
            }));
            let due = Duration::from_secs_f64(sent as f64 / fs as f64);
            if let Some(wait) = due.checked_sub(start.elapsed()) {
                std::thread::sleep(wait);
            }
        });
        let _ = tx.send(match r {
            Ok(_) => Event::End,
            Err(e) => Event::Error(e.to_string()),
        });
    });
    Ok(Capture {
        rx,
        sample_rate: fs,
        bits: (info.bytes_per_frame.max(2) * 8).min(24) as u16,
        label: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        is_file: true,
        stop,
        #[cfg(feature = "capture")]
        _stream: None,
    })
}

/// One range of formats a device offers.
#[derive(Debug, Clone, Serialize)]
pub struct InputFormat {
    pub channels: u16,
    pub min_rate: u32,
    pub max_rate: u32,
    pub sample_format: String,
}

/// A sound input device and the formats it offers.
#[derive(Debug, Clone, Serialize)]
pub struct InputDevice {
    pub name: String,
    pub is_default: bool,
    pub formats: Vec<InputFormat>,
}

/// The sample rate asked of a device when it offers it. USB timegrapher
/// microphones commonly do 44.1 or 48 kHz; the recordings so far are 48 kHz.
pub const PREFERRED_RATE: u32 = 48000;

#[cfg(feature = "capture")]
pub use device::{input_devices, open_input};

#[cfg(feature = "capture")]
mod device {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SizedSample};
    use std::sync::mpsc::Sender;

    /// Every sound input device, the default first.
    pub fn input_devices() -> Vec<InputDevice> {
        let host = cpal::default_host();
        let default = host.default_input_device().and_then(|d| d.name().ok());
        let mut out: Vec<InputDevice> = host
            .input_devices()
            .map(|it| {
                it.filter_map(|d| {
                    let name = d.name().ok()?;
                    let formats = d
                        .supported_input_configs()
                        .map(|c| {
                            c.map(|r| InputFormat {
                                channels: r.channels(),
                                min_rate: r.min_sample_rate().0,
                                max_rate: r.max_sample_rate().0,
                                sample_format: r.sample_format().to_string(),
                            })
                            .collect()
                        })
                        .unwrap_or_default();
                    Some(InputDevice {
                        is_default: Some(&name) == default.as_ref(),
                        name,
                        formats,
                    })
                })
                .collect()
            })
            .unwrap_or_default();
        out.sort_by_key(|d| !d.is_default);
        out
    }

    /// Open a sound input device by name (`None` for the default) at
    /// `rate` (or 48 kHz) if it offers it, in as few channels as it offers,
    /// mixed down to mono.
    pub fn open_input(name: Option<&str>, rate: Option<u32>) -> Result<Capture, String> {
        let rate = rate.unwrap_or(PREFERRED_RATE);
        let host = cpal::default_host();
        let device = match name {
            Some(n) => host
                .input_devices()
                .map_err(|e| e.to_string())?
                .find(|d| d.name().map(|x| x == n).unwrap_or(false))
                .ok_or_else(|| format!("no input device named {n}"))?,
            None => host
                .default_input_device()
                .ok_or("no sound input device found")?,
        };
        let label = device.name().unwrap_or_else(|_| "input".into());
        let ranges: Vec<_> = device
            .supported_input_configs()
            .map_err(|e| e.to_string())?
            .collect();
        let pick = ranges
            .iter()
            .filter(|r| r.min_sample_rate().0 <= rate && r.max_sample_rate().0 >= rate)
            .min_by_key(|r| (r.channels(), format_rank(r.sample_format())))
            .map(|r| r.with_sample_rate(cpal::SampleRate(rate)));
        let supported = match pick {
            Some(c) => c,
            None => device.default_input_config().map_err(|e| e.to_string())?,
        };
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.config();
        let (tx, rx) = channel();
        let stream = match format {
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, tx),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, tx),
            cpal::SampleFormat::I32 => build::<i32>(&device, &config, tx),
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, tx),
            cpal::SampleFormat::I8 => build::<i8>(&device, &config, tx),
            cpal::SampleFormat::U8 => build::<u8>(&device, &config, tx),
            cpal::SampleFormat::F64 => build::<f64>(&device, &config, tx),
            other => return Err(format!("unsupported sample format {other}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Capture {
            rx,
            sample_rate: config.sample_rate.0,
            bits: if format.sample_size() <= 2 { 16 } else { 24 },
            label,
            is_file: false,
            stop: Arc::new(AtomicBool::new(false)),
            _stream: Some(stream),
        })
    }

    /// Prefer 16-bit integer, the native format of most timegrapher microphones.
    fn format_rank(f: cpal::SampleFormat) -> u8 {
        match f {
            cpal::SampleFormat::I16 => 0,
            cpal::SampleFormat::I32 => 1,
            cpal::SampleFormat::F32 => 2,
            _ => 3,
        }
    }

    fn build<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        tx: Sender<Event>,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let ch = config.channels.max(1) as usize;
        let err_tx = tx.clone();
        device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let samples: Vec<f32> = data
                    .chunks(ch)
                    .map(|f| f.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / ch as f32)
                    .collect();
                let _ = tx.send(Event::Audio(Block {
                    samples,
                    at: SystemTime::now(),
                }));
            },
            move |e| {
                let _ = err_tx.send(Event::Error(e.to_string()));
            },
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_of_a_sine() {
        let x: Vec<f32> = (0..48000).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        let l = Level::of(&x);
        assert!((l.peak_dbfs + 6.02).abs() < 0.05, "{}", l.peak_dbfs);
        assert!((l.rms_dbfs + 9.03).abs() < 0.05, "{}", l.rms_dbfs);
        assert_eq!(l.clipped_fraction, 0.0);
    }
}
