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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Validate an acquisition length without looking up or opening a device.
pub fn checked_duration(seconds: f64) -> Result<Duration, String> {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err("must be a finite positive number of seconds".into());
    }
    let duration = Duration::try_from_secs_f64(seconds)
        .map_err(|_| "is outside the clock's representable duration range".to_string())?;
    if duration.is_zero() {
        return Err("is too short to represent as a duration".into());
    }
    Ok(duration)
}

/// The buffered microphone check allows two seconds for delivery. Check
/// both the grace-period addition and the platform's monotonic clock.
pub fn checked_recording_deadline(seconds: f64) -> Result<Instant, String> {
    recording_deadline_from(Instant::now(), checked_duration(seconds)?)
}

fn recording_deadline_from(now: Instant, duration: Duration) -> Result<Instant, String> {
    let duration = duration
        .checked_add(Duration::from_secs(2))
        .ok_or("with its delivery grace period exceeds the clock's duration range")?;
    now.checked_add(duration)
        .ok_or_else(|| "with its delivery grace period exceeds the clock's deadline range".into())
}

/// Convert a length at the selected device/file rate, without saturating
/// a float-to-integer cast or silently accepting a zero-sample recording.
pub fn checked_sample_count(seconds: f64, sample_rate: u32) -> Result<usize, String> {
    checked_duration(seconds)?;
    if sample_rate == 0 {
        return Err("the input sample rate must be positive".into());
    }
    let samples = (seconds * f64::from(sample_rate)).round();
    if !samples.is_finite() || samples >= (usize::MAX as u128 + 1) as f64 {
        return Err(format!(
            "exceeds the sample-count range at {sample_rate} Hz"
        ));
    }
    if samples < 1.0 {
        return Err(format!("is shorter than one sample at {sample_rate} Hz"));
    }
    Ok(samples as usize)
}

#[cfg(any(feature = "capture", test))]
fn checked_buffer_capacity(samples: usize) -> Result<(), String> {
    if samples > isize::MAX as usize / std::mem::size_of::<f32>() {
        return Err("exceeds the buffered recording's byte-capacity range".into());
    }
    Ok(())
}

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
    /// A replayed file's speed, times its own, as f32 bits.
    speed: Arc<AtomicU32>,
    #[cfg(feature = "capture")]
    _stream: Option<cpal::Stream>,
}

impl Capture {
    /// Play a replayed file faster or slower than its own speed (1.0),
    /// from the next block on. No effect on a device.
    pub fn set_speed(&self, speed: f32) {
        self.speed
            .store(speed.clamp(0.1, 100.0).to_bits(), Ordering::Relaxed);
    }
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
/// microphone; [`Capture::set_speed`] plays it faster or slower.
pub fn replay_file(path: &Path) -> Result<Capture, String> {
    let label = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    replay_files(&[path.to_path_buf()], label)
}

/// Replay several files one after the other as one recording, as a take
/// saved in parts was heard: they must share a sample rate.
pub fn replay_files(paths: &[PathBuf], label: String) -> Result<Capture, String> {
    let first = paths.first().ok_or("No recordings to replay")?;
    // Each error names its file when there are several.
    let named = |p: &Path, e: String| match paths.len() {
        1 => e,
        _ => format!("{}: {e}", p.display()),
    };
    let info = audio::info(first).map_err(|e| named(first, e.to_string()))?;
    let fs = info.sample_rate;
    for p in &paths[1..] {
        let i = audio::info(p).map_err(|e| named(p, e.to_string()))?;
        if i.sample_rate != fs {
            return Err(format!(
                "{} is at {} Hz but {} is at {fs} Hz",
                p.display(),
                i.sample_rate,
                first.display()
            ));
        }
    }
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let speed = Arc::new(AtomicU32::new(1.0f32.to_bits()));
    let speed2 = speed.clone();
    let several = paths.len() > 1;
    let paths = paths.to_vec();
    std::thread::spawn(move || {
        let start = Instant::now();
        // When the sound sent so far is due, at the speed each block went
        // out at.
        let mut due = Duration::ZERO;
        for p in &paths {
            let r = audio::stream(p, (fs / 50).max(1) as usize, |b| {
                if stop2.load(Ordering::Relaxed) {
                    return;
                }
                let x = f32::from_bits(speed2.load(Ordering::Relaxed)) as f64;
                due += Duration::from_secs_f64(b.len() as f64 / fs as f64 / x);
                let _ = tx.send(Event::Audio(Block {
                    samples: b.to_vec(),
                    at: SystemTime::now(),
                }));
                if let Some(wait) = due.checked_sub(start.elapsed()) {
                    std::thread::sleep(wait);
                }
            });
            if let Err(e) = r {
                let _ = tx.send(Event::Error(if several {
                    format!("{}: {e}", p.display())
                } else {
                    e.to_string()
                }));
                return;
            }
            if stop2.load(Ordering::Relaxed) {
                break;
            }
        }
        let _ = tx.send(Event::End);
    });
    Ok(Capture {
        rx,
        sample_rate: fs,
        bits: (info.bytes_per_frame.max(2) * 8).min(24) as u16,
        label,
        is_file: true,
        stop,
        speed,
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

#[cfg_attr(not(feature = "capture"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Match {
    Exact,
    Partial,
}

/// How a device query matches an input, case-insensitively: exactly (the
/// whole id, the id without its `host:` prefix, or the whole name), as part
/// of the id or name, or not at all.
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
fn matches(id: &str, name: &str, query: &str) -> Option<Match> {
    let (id, name, q) = (id.to_lowercase(), name.to_lowercase(), query.to_lowercase());
    let bare = id.split_once(':').map_or(id.as_str(), |(_, rest)| rest);
    if id == q || bare == q || name == q {
        Some(Match::Exact)
    } else if id.contains(&q) || name.contains(&q) {
        Some(Match::Partial)
    } else {
        None
    }
}

/// One entry of a device menu for a person to choose from.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InputChoice {
    /// The device id to open (as in [`InputDevice::id`]).
    pub id: String,
    pub label: String,
    /// One or two sentences on when to use it.
    pub detail: String,
    pub is_default: bool,
}

/// The inputs worth offering a person, from a device list (see `list`).
///
/// On Linux, ALSA lists every card several times over (front, dsnoop,
/// sysdefault, surround, null and so on) and names the sound server's
/// default input "Default ALSA Output". This keeps the system default
/// (normally the desktop sound server, which shares the microphone with
/// other programs) and each card's direct `hw:` device, labelled as such.
/// Other systems' lists are kept as they are, the default first.
pub fn choices(devices: &[InputDevice]) -> Vec<InputChoice> {
    let alsa: Vec<&InputDevice> = devices
        .iter()
        .filter(|d| d.id.starts_with("alsa:"))
        .collect();
    if alsa.is_empty() {
        let mut out: Vec<InputChoice> = devices
            .iter()
            .map(|d| InputChoice {
                id: d.id.clone(),
                label: if d.is_default {
                    format!("{} (default)", d.name)
                } else {
                    d.name.clone()
                },
                detail: d.manufacturer.clone().unwrap_or_default(),
                is_default: d.is_default,
            })
            .collect();
        out.sort_by_key(|c| !c.is_default);
        return out;
    }
    let server = |name: &str| {
        let n = name.to_lowercase();
        if n.contains("pipewire") {
            Some("PipeWire")
        } else if n.contains("pulse") {
            Some("PulseAudio")
        } else {
            None
        }
    };
    let shared = "Whichever input the desktop's sound settings choose, shared with other \
                  programs. Use this while the desktop sound server is running.";
    let mut out = Vec::new();
    let mut have_default = false;
    for d in &alsa {
        if &d.id[5..] == "default" {
            have_default = true;
            out.push(InputChoice {
                id: d.id.clone(),
                label: match server(&d.name) {
                    Some(s) => format!("System default (via {s})"),
                    None => "System default".into(),
                },
                detail: shared.into(),
                is_default: true,
            });
        }
    }
    for d in &alsa {
        let bare = &d.id[5..];
        if (bare == "pipewire" || bare == "pulse") && !have_default {
            out.push(InputChoice {
                id: d.id.clone(),
                label: format!(
                    "{} sound server",
                    if bare == "pipewire" {
                        "PipeWire"
                    } else {
                        "PulseAudio"
                    }
                ),
                detail: shared.into(),
                is_default: false,
            });
        }
    }
    let direct: Vec<&&InputDevice> = alsa
        .iter()
        .filter(|d| d.id[5..].starts_with("hw:"))
        .collect();
    for d in &direct {
        let twins = direct.iter().filter(|o| o.name == d.name).count();
        let dev = d.id.rsplit("DEV=").next().filter(|_| twins > 1);
        out.push(InputChoice {
            id: d.id.clone(),
            label: match dev {
                Some(n) => format!("{} #{n} (direct)", d.name),
                None => format!("{} (direct)", d.name),
            },
            detail: "Straight to the sound card, bypassing the desktop sound server. \
                     It can't open while the sound server holds the card; then choose \
                     the system default."
                .into(),
            is_default: false,
        });
    }
    out
}

/// A plainer account of an error from opening or reading an input.
pub fn explain_error(e: &str) -> String {
    let l = e.to_lowercase();
    if l.contains("no longer available")
        || l.contains("unplugged")
        || l.contains("busy")
        || l.contains("not available")
    {
        format!(
            "This input is busy or missing. If the desktop sound server is running it \
             is probably holding it: choose the system default instead, or close what \
             is using it. Otherwise check the microphone is plugged in. ({e})"
        )
    } else {
        e.to_string()
    }
}

#[cfg(feature = "capture")]
pub use device::{find, host, list, open, open_for_duration, open_input, play, record, Playback};

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

    /// The audio host in use (ALSA, CoreAudio, WASAPI, ...).
    pub fn host() -> String {
        format!("{:?}", cpal::default_host().id())
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
    /// The id may be given with or without its host prefix, so
    /// `hw:CARD=Device,DEV=0` picks `alsa:hw:CARD=Device,DEV=0` over
    /// `alsa:plughw:CARD=Device,DEV=0`.
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
        let mut hits = Vec::new();
        for d in host.input_devices().map_err(|e| e.to_string())? {
            let info = describe(&d, def.as_deref());
            match super::matches(&info.id, &info.name, q) {
                Some(super::Match::Exact) => return Ok((d, info)),
                Some(super::Match::Partial) => hits.push((d, info)),
                None => {}
            }
        }
        match hits.len() {
            1 => Ok(hits.pop().expect("one hit")),
            0 => Err(format!("no sound input matches '{q}'")),
            _ => Err(format!(
                "'{q}' matches several inputs: {}; give the whole id",
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
        open_config(dev, info, supported)
    }

    /// Query the actual format and check its requested sample count before
    /// opening the input stream. Device-independent validation runs first.
    pub fn open_for_duration(
        dev: &cpal::Device,
        info: &InputDevice,
        rate: Option<u32>,
        seconds: f64,
    ) -> Result<(Capture, Recorded, usize), String> {
        checked_duration(seconds)?;
        let supported = pick(dev, rate.unwrap_or(PREFERRED_RATE))?;
        let want = checked_sample_count(seconds, supported.config().sample_rate)?;
        let (capture, recorded) = open_config(dev, info, supported)?;
        Ok((capture, recorded, want))
    }

    fn open_config(
        dev: &cpal::Device,
        info: &InputDevice,
        supported: cpal::SupportedStreamConfig,
    ) -> Result<(Capture, Recorded), String> {
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
                speed: Arc::new(AtomicU32::new(1.0f32.to_bits())),
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
        checked_recording_deadline(seconds)?;
        let supported = pick(dev, PREFERRED_RATE)?;
        let want = checked_sample_count(seconds, supported.config().sample_rate)?;
        checked_buffer_capacity(want)?;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(want)
            .map_err(|e| format!("could not reserve the recording buffer: {e}"))?;
        let (cap, rec) = open_config(dev, &info, supported)?;
        let deadline = checked_recording_deadline(seconds)?;
        while samples.len() < want {
            let left = deadline.saturating_duration_since(Instant::now());
            match cap.rx.recv_timeout(left) {
                Ok(Event::Audio(b)) => {
                    let take = b.samples.len().min(want - samples.len());
                    samples.extend_from_slice(&b.samples[..take]);
                }
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

    /// Sound playing on the default output. Dropping it stops the sound.
    pub struct Playback {
        /// Samples of the source played so far.
        played: Arc<std::sync::atomic::AtomicUsize>,
        len: usize,
        rate: u32,
        _stream: cpal::Stream,
    }

    impl Playback {
        /// How far through the sound it has played, seconds.
        pub fn position_s(&self) -> f64 {
            self.played.load(Ordering::Relaxed).min(self.len) as f64 / self.rate as f64
        }

        pub fn finished(&self) -> bool {
            self.played.load(Ordering::Relaxed) >= self.len
        }
    }

    /// Play mono `samples` at `rate` on the default output, on every channel,
    /// resampled if the output can't run at `rate`.
    pub fn play(samples: Arc<Vec<f32>>, rate: u32) -> Result<Playback, String> {
        let dev = cpal::default_host()
            .default_output_device()
            .ok_or("no sound output found")?;
        let supported = dev
            .supported_output_configs()
            .map_err(|e| e.to_string())?
            .filter(|c| c.min_sample_rate() <= rate && rate <= c.max_sample_rate())
            .max_by_key(|c| (c.sample_format() == SampleFormat::F32, c.channels() <= 2))
            .map(|c| c.with_sample_rate(rate));
        let supported = match supported {
            Some(c) => c,
            None => dev.default_output_config().map_err(|e| e.to_string())?,
        };
        let config: cpal::StreamConfig = supported.config();
        let played = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let feed = Feed {
            samples: samples.clone(),
            pos: 0.0,
            step: rate as f64 / config.sample_rate as f64,
            channels: config.channels.max(1) as usize,
            played: played.clone(),
        };
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build_output::<f32>(&dev, &config, feed),
            SampleFormat::I16 => build_output::<i16>(&dev, &config, feed),
            SampleFormat::U16 => build_output::<u16>(&dev, &config, feed),
            SampleFormat::I32 => build_output::<i32>(&dev, &config, feed),
            SampleFormat::F64 => build_output::<f64>(&dev, &config, feed),
            other => return Err(format!("unsupported output format {other}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Playback {
            played,
            len: samples.len(),
            rate,
            _stream: stream,
        })
    }

    /// The source as the output callback reads it.
    struct Feed {
        samples: Arc<Vec<f32>>,
        /// Position in source samples.
        pos: f64,
        /// Source samples per output frame.
        step: f64,
        channels: usize,
        played: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Feed {
        fn next(&mut self) -> f32 {
            let i = self.pos as usize;
            let s = match (self.samples.get(i), self.samples.get(i + 1)) {
                (Some(&a), Some(&b)) => {
                    let f = (self.pos - i as f64) as f32;
                    a + (b - a) * f
                }
                (Some(&a), None) => a,
                _ => 0.0,
            };
            self.pos += self.step;
            s
        }
    }

    fn build_output<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        mut feed: Feed,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: SizedSample + FromSample<f32>,
    {
        device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let ch = feed.channels;
                for frame in data.chunks_mut(ch) {
                    let s = T::from_sample(feed.next());
                    frame.iter_mut().for_each(|o| *o = s);
                }
                feed.played.store(
                    (feed.pos as usize).min(feed.samples.len()),
                    Ordering::Relaxed,
                );
            },
            |_| {},
            None,
        )
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
    #[test]
    fn acquisition_durations_reject_invalid_and_unrepresentable_values() {
        for seconds in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -1.0,
            f64::MAX,
            f64::MIN_POSITIVE,
        ] {
            assert!(checked_duration(seconds).is_err(), "{seconds}");
        }
        for seconds in [5.0, 120.0, 1200.0] {
            assert_eq!(checked_duration(seconds).unwrap().as_secs_f64(), seconds);
        }
    }

    #[test]
    fn acquisition_deadline_checks_grace_and_platform_clock_overflow() {
        let now = Instant::now();
        let grace_error = recording_deadline_from(now, Duration::MAX).unwrap_err();
        assert!(grace_error.contains("duration range"), "{grace_error}");
        let deadline_error =
            recording_deadline_from(now, Duration::from_secs(u64::MAX - 2)).unwrap_err();
        assert!(
            deadline_error.contains("deadline range"),
            "{deadline_error}"
        );
        assert_eq!(
            recording_deadline_from(now, Duration::from_secs(5)).unwrap(),
            now + Duration::from_secs(7)
        );
    }

    #[test]
    fn acquisition_sample_counts_check_the_actual_rate_before_casting() {
        assert_eq!(checked_sample_count(60.0, 48000).unwrap(), 2_880_000);
        assert_eq!(checked_sample_count(0.01, 100).unwrap(), 1);
        assert!(checked_sample_count(1e-9, 48000)
            .unwrap_err()
            .contains("one sample"));
        assert!(checked_sample_count(1.0, 0).is_err());
        assert!(checked_sample_count((usize::MAX as u128 + 1) as f64 / 48000.0, 48000).is_err());
    }

    #[test]
    fn acquisition_buffer_capacity_is_checked_without_allocating() {
        let maximum = isize::MAX as usize / std::mem::size_of::<f32>();
        assert!(checked_buffer_capacity(maximum).is_ok());
        assert!(checked_buffer_capacity(maximum + 1).is_err());
    }

    #[test]
    fn device_id_without_host_prefix_is_exact() {
        let m = |id| {
            matches(
                id,
                "USB PnP Sound Device, USB Audio",
                "hw:CARD=Device,DEV=0",
            )
        };
        assert_eq!(m("alsa:hw:CARD=Device,DEV=0"), Some(Match::Exact));
        assert_eq!(m("alsa:plughw:CARD=Device,DEV=0"), Some(Match::Partial));
        assert_eq!(m("alsa:hw:CARD=PCH,DEV=0"), None);
        assert_eq!(
            matches("alsa:default", "Default ALSA Output", "ALSA:DEFAULT"),
            Some(Match::Exact)
        );
        assert_eq!(
            matches("alsa:x", "USB PnP Sound Device", "usb pnp"),
            Some(Match::Partial)
        );
    }

    use super::*;

    fn dev(id: &str, name: &str, is_default: bool) -> InputDevice {
        InputDevice {
            id: id.into(),
            name: name.into(),
            manufacturer: None,
            is_default,
            default_config: None,
            supported: Vec::new(),
        }
    }

    /// A replay at 8 times its own speed takes an eighth of the time.
    #[test]
    fn replay_speed_sets_the_pace() {
        let dir = std::env::temp_dir().join(format!("tg-replay-speed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("two-seconds.wav");
        let audio = audio::Audio {
            samples: vec![0.0; 16_000],
            sample_rate: 8000,
        };
        audio::write_wav(&f, &audio).unwrap();
        let c = replay_file(&f).unwrap();
        c.set_speed(8.0);
        let start = Instant::now();
        let mut samples = 0;
        loop {
            match c.rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Audio(b) => samples += b.samples.len(),
                Event::End => break,
                Event::Error(e) => panic!("{e}"),
            }
        }
        let took = start.elapsed().as_secs_f64();
        assert_eq!(samples, 16_000);
        assert!(took < 1.0, "2 s at 8x took {took:.2} s");
        assert!(took > 0.15, "2 s at 8x took only {took:.2} s");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A take saved in parts replays as one recording, the parts in the
    /// order given; parts at different sample rates are refused.
    #[test]
    fn parts_replay_in_order() {
        let dir = std::env::temp_dir().join(format!("tg-replay-parts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let part = |name: &str, value: f32, rate: u32| {
            let f = dir.join(name);
            let audio = audio::Audio {
                samples: vec![value; 4000],
                sample_rate: rate,
            };
            audio::write_wav(&f, &audio).unwrap();
            f
        };
        let a = part("take_part01-001.wav", 0.25, 8000);
        let b = part("take_part01-002.wav", -0.5, 8000);
        let c = replay_files(&[a.clone(), b], "take".into()).unwrap();
        assert_eq!(c.label, "take");
        c.set_speed(100.0);
        let mut got = Vec::new();
        loop {
            match c.rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Audio(b) => got.extend(b.samples),
                Event::End => break,
                Event::Error(e) => panic!("{e}"),
            }
        }
        assert_eq!(got.len(), 8000);
        assert!((got[100] - 0.25).abs() < 1e-3 && (got[7900] + 0.5).abs() < 1e-3);
        let odd = part("take_part01-003.wav", 0.0, 11025);
        let e = replay_files(&[a, odd], "take".into()).err().unwrap();
        assert!(e.contains("11025 Hz"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn menu_keeps_the_default_and_direct_devices() {
        // As cpal lists them on a Linux workstation (some of them).
        let usb = "USB PnP Sound Device, USB Audio";
        let list = vec![
            dev(
                "alsa:default",
                "Default ALSA Output (currently PipeWire Media Server)",
                true,
            ),
            dev("alsa:pipewire", "PipeWire Sound Server", false),
            dev("alsa:null", "Discard all samples", false),
            dev("alsa:sysdefault:CARD=Device", usb, false),
            dev("alsa:front:CARD=Device,DEV=0", usb, false),
            dev("alsa:dsnoop:CARD=Device,DEV=0", usb, false),
            dev("alsa:hw:CARD=Device,DEV=0", usb, false),
            dev("alsa:plughw:CARD=Device,DEV=0", usb, false),
            dev(
                "alsa:hw:CARD=PCH,DEV=0",
                "HDA Intel PCH, ALC897 Analog",
                false,
            ),
            dev(
                "alsa:hw:CARD=PCH,DEV=2",
                "HDA Intel PCH, ALC897 Analog",
                false,
            ),
        ];
        let c = choices(&list);
        let labels: Vec<&str> = c.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "System default (via PipeWire)",
                "USB PnP Sound Device, USB Audio (direct)",
                "HDA Intel PCH, ALC897 Analog #0 (direct)",
                "HDA Intel PCH, ALC897 Analog #2 (direct)",
            ]
        );
        assert_eq!(c[0].id, "alsa:default");
        assert!(c[0].is_default);
        assert_eq!(c[1].id, "alsa:hw:CARD=Device,DEV=0");
    }

    #[test]
    fn other_systems_keep_their_list() {
        let list = vec![
            dev("wasapi:a", "Mic", false),
            dev("wasapi:b", "USB Mic", true),
        ];
        let c = choices(&list);
        assert_eq!(c[0].label, "USB Mic (default)");
        assert_eq!(c.len(), 2);
        assert!(explain_error(
            "The requested device is no longer available. For example, it has been unplugged."
        )
        .starts_with("This input is busy or missing"));
    }

    #[test]
    fn level_of_a_sine() {
        let x: Vec<f32> = (0..48000).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        let l = Level::of(&x);
        assert!((l.peak_dbfs + 6.02).abs() < 0.05, "{}", l.peak_dbfs);
        assert!((l.rms_dbfs + 9.03).abs() < 0.05, "{}", l.rms_dbfs);
        assert_eq!(l.clipped_fraction, 0.0);
    }
}
