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

/// A range of formats a device offers.
#[derive(Debug, Clone, Serialize)]
pub struct InputConfig {
    pub min_channels: u16,
    pub max_channels: u16,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub sample_format: String,
}

/// A sound input device and the formats it offers.
#[derive(Debug, Clone, Serialize)]
pub struct InputDevice {
    /// Stable identifier; it, or part of the name, selects the device.
    /// It is cpal's device id, `<host>:<device>`; on Linux the device part
    /// is the ALSA PCM name, as in `alsa:hw:CARD=Device,DEV=0`, so the
    /// sound card (for its mixer) can be read from it.
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    pub is_default: bool,
    /// The device's own default configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_config: Option<InputConfig>,
    pub supported: Vec<InputConfig>,
}

/// What a short recording was made with.
#[derive(Debug, Clone, Serialize)]
pub struct Recorded {
    pub device: InputDevice,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
    pub started_utc: String,
}

/// The sample rate asked of a device when it offers it. USB timegrapher
/// microphones commonly do 44.1 or 48 kHz; the recordings so far are 48 kHz.
pub const PREFERRED_RATE: u32 = 48000;

#[cfg(feature = "capture")]
pub use device::{find, list, open, open_input, record};

#[cfg(feature = "capture")]
mod device {
    use super::*;
    use crate::audio::Audio;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample};
    use std::sync::mpsc::Sender;

    fn describe(dev: &cpal::Device, default_id: Option<&str>) -> InputDevice {
        let id = dev.id().map(|i| i.to_string()).unwrap_or_default();
        let desc = dev.description().ok();
        let supported = dev
            .supported_input_configs()
            .map(|it| {
                merge(it.map(|c| InputConfig {
                    min_channels: c.channels(),
                    max_channels: c.channels(),
                    min_sample_rate: c.min_sample_rate(),
                    max_sample_rate: c.max_sample_rate(),
                    sample_format: c.sample_format().to_string(),
                }))
            })
            .unwrap_or_default();
        let default_config = dev.default_input_config().ok().map(|c| InputConfig {
            min_channels: c.channels(),
            max_channels: c.channels(),
            min_sample_rate: c.sample_rate(),
            max_sample_rate: c.sample_rate(),
            sample_format: c.sample_format().to_string(),
        });
        InputDevice {
            is_default: default_id == Some(id.as_str()),
            name: desc
                .as_ref()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|| id.clone()),
            manufacturer: desc.and_then(|d| d.manufacturer().map(str::to_string)),
            id,
            default_config,
            supported,
        }
    }

    /// Fold configurations that differ only by channel count into ranges:
    /// virtual devices list every count from 1 to 32 for every format.
    fn merge(configs: impl Iterator<Item = InputConfig>) -> Vec<InputConfig> {
        let mut out: Vec<InputConfig> = Vec::new();
        for c in configs {
            match out.iter_mut().find(|o| {
                o.sample_format == c.sample_format
                    && o.min_sample_rate == c.min_sample_rate
                    && o.max_sample_rate == c.max_sample_rate
            }) {
                Some(o) => {
                    o.min_channels = o.min_channels.min(c.min_channels);
                    o.max_channels = o.max_channels.max(c.max_channels);
                }
                None => out.push(c),
            }
        }
        out
    }

    fn default_id(host: &cpal::Host) -> Option<String> {
        host.default_input_device()
            .and_then(|d| d.id().ok())
            .map(|i| i.to_string())
    }

    /// Every input the default audio host offers, the default first.
    pub fn list() -> Result<Vec<InputDevice>, String> {
        let host = cpal::default_host();
        let def = default_id(&host);
        let devs = host.input_devices().map_err(|e| e.to_string())?;
        let mut out: Vec<InputDevice> = devs.map(|d| describe(&d, def.as_deref())).collect();
        out.sort_by_key(|d| !d.is_default);
        Ok(out)
    }

    /// The input whose id or name is, or else uniquely contains, `query`
    /// (case-insensitive), or the default input when `query` is `None`.
    pub fn find(query: Option<&str>) -> Result<(cpal::Device, InputDevice), String> {
        let host = cpal::default_host();
        let def = default_id(&host);
        let Some(q) = query else {
            let d = host
                .default_input_device()
                .ok_or("no default sound input; name one (see the list of devices)")?;
            let info = describe(&d, def.as_deref());
            return Ok((d, info));
        };
        let q = q.to_lowercase();
        let mut hits = Vec::new();
        for d in host.input_devices().map_err(|e| e.to_string())? {
            let info = describe(&d, def.as_deref());
            if info.id.to_lowercase() == q || info.name.to_lowercase() == q {
                return Ok((d, info));
            }
            if info.id.to_lowercase().contains(&q) || info.name.to_lowercase().contains(&q) {
                hits.push((d, info));
            }
        }
        match hits.len() {
            1 => Ok(hits.pop().expect("one hit")),
            0 => Err(format!("no sound input matches '{q}'")),
            _ => Err(format!(
                "'{q}' matches several inputs: {}; give more of the id",
                hits.iter()
                    .map(|h| h.1.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// The configuration used for a device: `rate` (48 kHz by default) if
    /// it offers it, in as few channels as possible, 16-bit if it has it;
    /// otherwise its own default.
    fn pick(dev: &cpal::Device, rate: u32) -> Result<cpal::SupportedStreamConfig, String> {
        let ranges: Vec<_> = dev
            .supported_input_configs()
            .map_err(|e| e.to_string())?
            .collect();
        let best = ranges
            .iter()
            .filter(|r| r.min_sample_rate() <= rate && r.max_sample_rate() >= rate)
            .min_by_key(|r| (r.channels(), format_rank(r.sample_format())))
            .map(|r| r.with_sample_rate(rate));
        match best {
            Some(c) => Ok(c),
            None => dev.default_input_config().map_err(|e| e.to_string()),
        }
    }

    /// Prefer 16-bit integer, the native format of most timegrapher microphones.
    fn format_rank(f: SampleFormat) -> u8 {
        match f {
            SampleFormat::I16 => 0,
            SampleFormat::I32 => 1,
            SampleFormat::F32 => 2,
            _ => 3,
        }
    }

    /// Start capturing from an input found by `query` (see [`find`]).
    pub fn open_input(query: Option<&str>, rate: Option<u32>) -> Result<Capture, String> {
        let (dev, info) = find(query)?;
        open(&dev, &info, rate).map(|(c, _)| c)
    }

    /// Start capturing from `dev`, mixed down to mono, as blocks on a channel.
    pub fn open(
        dev: &cpal::Device,
        info: &InputDevice,
        rate: Option<u32>,
    ) -> Result<(Capture, Recorded), String> {
        let supported = pick(dev, rate.unwrap_or(PREFERRED_RATE))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.config();
        let (tx, rx) = channel();
        let stream = match format {
            SampleFormat::I16 => build::<i16>(dev, &config, tx),
            SampleFormat::U16 => build::<u16>(dev, &config, tx),
            SampleFormat::I32 => build::<i32>(dev, &config, tx),
            SampleFormat::F32 => build::<f32>(dev, &config, tx),
            SampleFormat::I8 => build::<i8>(dev, &config, tx),
            SampleFormat::U8 => build::<u8>(dev, &config, tx),
            SampleFormat::F64 => build::<f64>(dev, &config, tx),
            other => return Err(format!("unsupported sample format {other}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        let rec = Recorded {
            device: info.clone(),
            sample_rate: config.sample_rate,
            channels: config.channels,
            sample_format: format.to_string(),
            started_utc: crate::recorder::utc(SystemTime::now()),
        };
        Ok((
            Capture {
                rx,
                sample_rate: config.sample_rate,
                bits: if format.sample_size() <= 2 { 16 } else { 24 },
                label: info.name.clone(),
                is_file: false,
                stop: Arc::new(AtomicBool::new(false)),
                _stream: Some(stream),
            },
            rec,
        ))
    }

    /// Record `seconds` from `dev` (configured as for [`open`]), mixed to mono.
    pub fn record(
        dev: &cpal::Device,
        info: InputDevice,
        seconds: f64,
    ) -> Result<(Audio, Recorded), String> {
        let (cap, rec) = open(dev, &info, None)?;
        let want = (seconds * rec.sample_rate as f64).round() as usize;
        let mut samples = Vec::with_capacity(want);
        let deadline = Instant::now() + Duration::from_secs_f64(seconds + 2.0);
        while samples.len() < want {
            let left = deadline.saturating_duration_since(Instant::now());
            match cap.rx.recv_timeout(left) {
                Ok(Event::Audio(b)) => samples.extend_from_slice(&b.samples),
                Ok(Event::Error(e)) => return Err(format!("recording failed: {e}")),
                Ok(Event::End) => break,
                Err(_) => return Err("the input stopped delivering audio".into()),
            }
        }
        samples.truncate(want);
        Ok((
            Audio {
                samples,
                sample_rate: rec.sample_rate,
            },
            rec,
        ))
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
                    .map(|f| {
                        f.iter()
                            .map(|&s| <f32 as FromSample<T>>::from_sample_(s))
                            .sum::<f32>()
                            / ch as f32
                    })
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
