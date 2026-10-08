//! Sound inputs: listing them and recording a few seconds from one.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use timegrapher_core::audio::Audio;

#[derive(Debug, Clone, Serialize)]
pub struct InputConfig {
    pub min_channels: u16,
    pub max_channels: u16,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub sample_format: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InputDevice {
    /// Stable identifier: pass it (or part of the name) to `--device`.
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    pub is_default: bool,
    /// The configuration a recording uses unless told otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_config: Option<InputConfig>,
    pub supported: Vec<InputConfig>,
}

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

/// Every input the default audio host offers.
pub fn list() -> Result<Vec<InputDevice>, String> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|d| d.id().ok())
        .map(|i| i.to_string());
    let devs = host.input_devices().map_err(|e| e.to_string())?;
    Ok(devs.map(|d| describe(&d, default_id.as_deref())).collect())
}

/// The input whose id or name contains `query` (case-insensitive), or the
/// default input when `query` is `None`.
pub fn find(query: Option<&str>) -> Result<(cpal::Device, InputDevice), String> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|d| d.id().ok())
        .map(|i| i.to_string());
    let Some(q) = query else {
        let d = host
            .default_input_device()
            .ok_or("no default sound input; name one with --device (see `timegrapher devices`)")?;
        let info = describe(&d, default_id.as_deref());
        return Ok((d, info));
    };
    let q = q.to_lowercase();
    let mut hits = Vec::new();
    for d in host.input_devices().map_err(|e| e.to_string())? {
        let info = describe(&d, default_id.as_deref());
        if info.id.to_lowercase() == q || info.name.to_lowercase() == q {
            return Ok((d, info));
        }
        if info.id.to_lowercase().contains(&q) || info.name.to_lowercase().contains(&q) {
            hits.push((d, info));
        }
    }
    match hits.len() {
        1 => Ok(hits.pop().unwrap()),
        0 => Err(format!(
            "no sound input matches '{q}' (see `timegrapher devices`)"
        )),
        _ => Err(format!(
            "'{q}' matches several inputs: {}; give more of the id",
            hits.iter()
                .map(|h| h.1.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// What a recording was made with.
#[derive(Debug, Clone, Serialize)]
pub struct Recorded {
    pub device: InputDevice,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
    pub started_utc: String,
}

/// Record `seconds` from `dev` in its default configuration, mixed to mono.
pub fn record(
    dev: &cpal::Device,
    info: InputDevice,
    seconds: f64,
) -> Result<(Audio, Recorded), String> {
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let channels = cfg.channels();
    let rate = cfg.sample_rate();
    let fmt = cfg.sample_format();
    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let err = Arc::new(Mutex::new(None::<String>));
    let stream_cfg = cfg.config();
    let started_utc = now_utc();
    let stream = match fmt {
        SampleFormat::I16 => build::<i16>(dev, &stream_cfg, &buf, &err),
        SampleFormat::I32 => build::<i32>(dev, &stream_cfg, &buf, &err),
        SampleFormat::U16 => build::<u16>(dev, &stream_cfg, &buf, &err),
        SampleFormat::F32 => build::<f32>(dev, &stream_cfg, &buf, &err),
        other => return Err(format!("unsupported sample format {other}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_secs_f64(seconds));
    drop(stream);
    if let Some(e) = err.lock().unwrap().take() {
        return Err(format!("recording failed: {e}"));
    }
    let interleaved = std::mem::take(&mut *buf.lock().unwrap());
    let ch = channels.max(1) as usize;
    let samples = interleaved
        .chunks_exact(ch)
        .map(|c| c.iter().sum::<f32>() / ch as f32)
        .collect();
    Ok((
        Audio {
            samples,
            sample_rate: rate,
        },
        Recorded {
            device: info,
            sample_rate: rate,
            channels,
            sample_format: fmt.to_string(),
            started_utc,
        },
    ))
}

fn build<T>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    buf: &Arc<Mutex<Vec<f32>>>,
    err: &Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    let b = buf.clone();
    let e = err.clone();
    dev.build_input_stream(
        cfg,
        move |data: &[T], _| {
            b.lock().unwrap().extend(
                data.iter()
                    .map(|&s| <f32 as cpal::FromSample<T>>::from_sample_(s)),
            );
        },
        move |x| {
            *e.lock().unwrap() = Some(x.to_string());
        },
        None,
    )
    .map_err(|e| e.to_string())
}

/// The time now as an RFC 3339 UTC string, without a date library.
pub fn now_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
