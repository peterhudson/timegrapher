//! A take kept as its analysis alone: the beats, the amplitude windows and
//! the Tick Tock profiles the engine found in its sound, in an `analysis/`
//! folder beside `session.toml`, `session.json` and `clock.csv`, with no
//! audio. Everything read from the beats (readings, charts, steadiness,
//! the Positions Test, cycles) reads from it as from the sound; only
//! measuring the sound again, the waveform and replaying it are gone.
//!
//! The layout is documented in `docs/sessions.md`.

use crate::amplitude::AmplitudeWindow;
use crate::audio::{self, AudioError};
use crate::beats::{self, Beat};
use crate::dsp::envelope;
use crate::profile::{self, Side, TickProfile};
use crate::session::{self, Manifest, ShapeSummary};
use crate::shape;
use crate::stream::{self, BeatLog, StreamConfig};
use crate::usbclock;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// The folder the analysis is kept in, inside the take's folder.
pub const DIR: &str = "analysis";
/// Its header and marker.
pub const HEADER: &str = "analysis.json";
pub const BEATS: &str = "beats.csv.gz";
pub const WINDOWS: &str = "windows.csv.gz";
pub const PROFILES: &str = "profiles.bin.gz";
/// The layout's name and version, in the header's `format`.
pub const FORMAT: &str = "timegrapher.analysis/1";
/// The fact the header states about the missing sound.
pub const NO_SOUND: &str = "Saved without its sound: it can't be measured again, and the \
     waveform (the Scope pane and Replay) can't come back.";
/// The Tick Tock profiles are kept for each stretch of this many seconds.
pub const PROFILE_EVERY_S: f64 = 2.0;
/// Seconds of sound the beat shape is measured on, as `timegrapher
/// session` measures it.
pub const SHAPE_S: f64 = 60.0;

/// Audio either side of a profile's stretch, for its beats' windows, as
/// the app's profile pane reads it.
const PROFILE_MARGIN_S: f64 = 0.05;
/// Kept profile points are this far apart, ms (every second point of the
/// engine's 0.1 ms profile).
const PROFILE_POINT_MS: f64 = 0.2;
/// Levels are kept in quarter-dB steps below each profile's top, down to
/// this many dB; lower is kept as 0.
const PROFILE_RANGE_DB: f64 = 63.75;
const PROFILE_MAGIC: &[u8; 8] = b"TGPROF\0\x01";
const BEATS_COLUMNS: &str = "index_step,residual_us,quality,clipped";
const WINDOWS_COLUMNS: &str = "start_s,end_s,even_deg,odd_deg,beat_error_ms,\
     beat_error_unlock_ms,even_unlock_ms,odd_unlock_ms,even_drop_ms,odd_drop_ms";

/// What `analysis/analysis.json` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    /// [`FORMAT`].
    pub format: String,
    /// The software that measured the sound: name, version and commit.
    pub software: serde_json::Value,
    /// Always false: the take has no sound.
    pub audio: bool,
    /// [`NO_SOUND`].
    pub note: String,
    pub sample_rate: u32,
    /// Bytes a frame of the sound took, for reading `clock.csv`'s audio
    /// column as the sound's own file would.
    pub bytes_per_frame: u32,
    pub duration_s: f64,
    pub bph: u32,
    pub lift_deg: f64,
    pub clipped_samples: usize,
    /// Audio seconds where the beats jumped (the beat log's `jumps`).
    pub jumps: Vec<f64>,
    /// The jumps whose sides the sound told apart (the beat log's `sided`);
    /// the others are named by the beat count alone.
    pub sided: Vec<f64>,
    /// The analysis settings that shaped the beats.
    pub settings: serde_json::Value,
    /// Rows in each of the other files.
    pub beats: usize,
    pub windows: usize,
    pub profiles: usize,
    /// The card's clock error from the USB lines in the sound's first ten
    /// minutes, when they showed it.
    pub usb_clock: Option<StoredUsbClock>,
    /// The median, over the profiles' stretches, of the beats' peak over
    /// the envelope's median (the signal figure the app shows).
    pub signal_median: Option<f32>,
    /// The beat shape of each recording in the session file, measured on
    /// the sound when the take was saved.
    pub shape: Vec<StoredShape>,
}

/// [`usbclock::UsbClock`], as kept.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StoredUsbClock {
    pub ppm: f64,
    pub lines: usize,
    pub spread_ppm: f64,
}

/// One recording's beat shape, measured when the take was saved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredShape {
    /// The recording's place in the session file, from 0.
    pub recording: usize,
    /// The stretch it was measured on, audio seconds.
    pub from_s: f64,
    pub to_s: f64,
    pub summary: ShapeSummary,
}

/// The Tick Tock profiles of one stretch.
#[derive(Debug, Clone)]
pub struct StretchProfiles {
    /// The stretch's end, audio seconds; it began [`PROFILE_EVERY_S`]
    /// before.
    pub end_s: f64,
    /// The beats' peak over the envelope's median in it.
    pub signal: Option<f32>,
    pub sides: [Option<TickProfile>; 2],
}

/// Everything an analysis-only take keeps.
#[derive(Debug, Clone)]
pub struct Stored {
    pub header: Header,
    /// The beat log, with the clipped beats.
    pub log: BeatLog,
    pub profiles: Vec<StretchProfiles>,
}

impl Stored {
    /// The stored shape of recording `i` of the session file.
    pub fn shape_of(&self, i: usize) -> Option<&StoredShape> {
        self.header.shape.iter().find(|s| s.recording == i)
    }
}

/// Whether `take` (a take's folder) is kept as its analysis alone: its
/// `analysis/analysis.json` is there.
pub fn is_analysis_only(take: &Path) -> bool {
    take.join(DIR).join(HEADER).is_file()
}

/// The stretches the beat shape is measured on, one per recording of the
/// session file, as `timegrapher session` picks them with the session
/// file's own settling: the first unmasked [`SHAPE_S`] after settling, else
/// the longest unmasked stretch. None for a recording with none, or when
/// the session file turns the shape off.
pub fn shape_stretches(m: &Manifest, duration_s: f64) -> Vec<Option<(f64, f64)>> {
    m.recordings
        .iter()
        .map(|e| {
            if m.shape == Some(false) {
                return None;
            }
            let w = crate::take::reading_window(e, m, None, duration_s).ok()?;
            shape_stretch(&session::masked_spans(&e.masked), w.from_s, w.to_s)
                .map(|(from, len)| (from, from + len))
        })
        .collect()
}

/// Where the beat shape is measured, clear of masked stretches: the first
/// unmasked stretch from `from_s` to `to_s` that is [`SHAPE_S`] long, else
/// the longest; start and length, seconds. None when all of it is masked.
pub fn shape_stretch(masked: &[(f64, f64)], from_s: f64, to_s: f64) -> Option<(f64, f64)> {
    let mut clear = Vec::new();
    let mut at = from_s;
    for &(a, b) in masked {
        if a > at {
            clear.push((at, a.min(to_s)));
        }
        at = at.max(b);
    }
    clear.push((at, to_s));
    let clear: Vec<(f64, f64)> = clear.into_iter().filter(|(a, b)| b > a).collect();
    let pick = clear.iter().find(|(a, b)| b - a >= SHAPE_S).or_else(|| {
        clear
            .iter()
            .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
    })?;
    Some((pick.0, SHAPE_S.min(pick.1 - pick.0)))
}

/// Measure the take whose sound is `files` (in the order it is read) for
/// keeping as its analysis: the whole recording analysed as an opened take
/// is, then read a second time for the profiles of each
/// [`PROFILE_EVERY_S`], the signal figure, the USB-line clock and the beat
/// shape of each stretch in `shapes` ([`shape_stretches`]). `progress` is
/// called with the seconds of sound read so far, over both readings (so up
/// to twice the take's length).
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
pub fn measure(
    files: &[&Path],
    cfg: &StreamConfig,
    shapes: &[Option<(f64, f64)>],
    software: serde_json::Value,
    mut progress: impl FnMut(f64),
) -> Result<Stored, AudioError> {
    let log = stream::analyze_files(files, cfg, &mut progress)?;
    let bytes_per_frame = audio::info(files[0])?.bytes_per_frame;
    let first = log.duration_s;
    let fs = log.sample_rate as f64;
    let beat_s = 3600.0 / log.bph.max(1) as f64;
    let n_total = (log.duration_s * fs).round() as usize;
    let usb_keep = (usbclock::MAX_DURATION_S * fs) as usize;
    let mut usb: Vec<f32> = Vec::new();
    let mut cuts: Vec<(usize, usize, Vec<f32>)> = shapes
        .iter()
        .flatten()
        .map(|&(a, b)| ((a * fs) as usize, (b * fs) as usize, Vec::new()))
        .collect();
    let mut profiles = Vec::new();
    let margin = (PROFILE_MARGIN_S * fs).round() as usize;
    let step = (PROFILE_EVERY_S * fs).round() as usize;
    let mut buf: Vec<f32> = Vec::new();
    let mut buf0 = 0usize;
    let mut pos = 0usize;
    let mut k = 0usize;
    let mut next_beat = 0usize;
    let stretch = |buf: &[f32], buf0: usize, k: usize, next_beat: &mut usize| {
        let (a, b) = (k * step, (k + 1) * step);
        let lo = a.saturating_sub(margin).max(buf0);
        let hi = (b + margin).min(buf0 + buf.len());
        let x = &buf[lo - buf0..hi - buf0];
        let (from, to) = (a as f64 / fs, b as f64 / fs);
        while log.beats.get(*next_beat).is_some_and(|bt| bt.time < from) {
            *next_beat += 1;
        }
        let x0 = lo as f64 / fs;
        let local: Vec<Beat> = log.beats[*next_beat..]
            .iter()
            .take_while(|bt| bt.time < to)
            .map(|bt| Beat {
                time: bt.time - x0,
                ..*bt
            })
            .collect();
        if local.is_empty() {
            return None;
        }
        let env = envelope(x, fs, &cfg.analysis.envelope);
        let sides = profile::profiles(
            &env,
            fs,
            &local,
            2.0 * beat_s,
            0.0,
            x.len() as f64 / fs,
            &cfg.analysis.amplitude,
        );
        Some(StretchProfiles {
            end_s: to.min(log.duration_s),
            signal: crate::live::signal_figure(&env, fs, &local),
            sides,
        })
    };
    for path in files {
        audio::stream(path, 1 << 16, |block| {
            let (s, e) = (pos, pos + block.len());
            if usb.len() < usb_keep {
                let take = (usb_keep - usb.len()).min(block.len());
                usb.extend_from_slice(&block[..take]);
            }
            for (a, b, v) in &mut cuts {
                if e > *a && s < *b {
                    v.extend_from_slice(&block[(*a).max(s) - s..(*b).min(e) - s]);
                }
            }
            buf.extend_from_slice(block);
            pos = e;
            while buf0 + buf.len() >= (k + 1) * step + margin {
                profiles.extend(stretch(&buf, buf0, k, &mut next_beat));
                k += 1;
                let keep = (k * step).saturating_sub(margin);
                if keep > buf0 {
                    buf.drain(..keep - buf0);
                    buf0 = keep;
                }
            }
            progress(first + pos as f64 / fs);
        })?;
    }
    while k * step < n_total.min(buf0 + buf.len()) {
        profiles.extend(stretch(&buf, buf0, k, &mut next_beat));
        k += 1;
    }
    let shape = shapes
        .iter()
        .enumerate()
        .filter_map(|(i, s)| Some((i, (*s)?)))
        .zip(cuts)
        .filter_map(|((recording, (from_s, to_s)), (_, _, x))| {
            let summary = measure_shape(&x, fs, log.bph, from_s)?;
            Some(StoredShape {
                recording,
                from_s,
                to_s,
                summary,
            })
        })
        .collect();
    let usb_clock = usbclock::measure(&usb, log.sample_rate).map(|c| StoredUsbClock {
        ppm: c.ppm,
        lines: c.lines,
        spread_ppm: c.spread_ppm,
    });
    let mut signals: Vec<f32> = profiles.iter().filter_map(|p| p.signal).collect();
    let signal_median = (!signals.is_empty()).then(|| crate::dsp::median_f32(&mut signals));
    let header = Header {
        format: FORMAT.into(),
        software,
        audio: false,
        note: NO_SOUND.into(),
        sample_rate: log.sample_rate,
        bytes_per_frame,
        duration_s: log.duration_s,
        bph: log.bph,
        lift_deg: log.lift_deg,
        clipped_samples: log.clipped_samples,
        jumps: log.jumps.clone(),
        sided: log.sided.clone(),
        settings: settings(cfg),
        beats: log.beats.len(),
        windows: log.amplitude_windows.len(),
        profiles: profiles.len(),
        usb_clock,
        signal_median,
        shape,
    };
    Ok(Stored {
        header,
        log,
        profiles,
    })
}

/// The beat shape of `x`, from `from_s` seconds into the recording, as
/// `timegrapher session` measures it.
fn measure_shape(x: &[f32], fs: f64, bph: u32, from_s: f64) -> Option<ShapeSummary> {
    if x.is_empty() {
        return None;
    }
    // `session` measures the shape on the default envelope, whatever the
    // beats were found with.
    let env = envelope(x, fs, &crate::dsp::EnvelopeConfig::default());
    let (r, _) = shape::of_envelope(&env, fs, bph);
    Some(session::shape_summary(&r, from_s, x.len() as f64 / fs))
}

/// The analysis settings, as the header lists them.
fn settings(cfg: &StreamConfig) -> serde_json::Value {
    let a = &cfg.analysis;
    serde_json::json!({
        "bph": a.bph,
        "lift_deg": a.amplitude.lift_deg,
        "lift_known": a.lift_known,
        "onset_fraction": a.amplitude.onset_fraction,
        "escape_teeth": a.escape_teeth,
        "rate_window_s": a.rate_window_s,
        "amplitude_window_s": a.amplitude_window_s,
        "envelope": {
            "highpass_hz": a.envelope.highpass_hz,
            "notch_hz": a.envelope.notch_hz,
            "smooth_s": a.envelope.smooth_s,
            "burst_gate": a.envelope.burst_gate,
        },
        "chunk_s": cfg.chunk_s,
        "margin_s": cfg.margin_s,
        "profile_every_s": PROFILE_EVERY_S,
    })
}

fn invalid(m: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m)
}

/// The files of `analysis/`, by name, ready to write.
pub fn files(s: &Stored) -> io::Result<Vec<(&'static str, Vec<u8>)>> {
    let header = serde_json::to_string_pretty(&s.header).map_err(io::Error::other)? + "\n";
    Ok(vec![
        (HEADER, header.into_bytes()),
        (BEATS, gzip(beats_csv(&s.log).as_bytes())?),
        (
            WINDOWS,
            gzip(windows_csv(&s.log.amplitude_windows).as_bytes())?,
        ),
        (PROFILES, gzip(&profiles_bin(&s.profiles))?),
    ])
}

/// Write `analysis/` into the take folder `take`.
pub fn write(take: &Path, s: &Stored) -> io::Result<()> {
    let dir = take.join(DIR);
    fs::create_dir_all(&dir)?;
    for (name, bytes) in files(s)? {
        fs::write(dir.join(name), bytes)?;
    }
    Ok(())
}

/// Read `analysis/` from the take folder `take`.
pub fn read(take: &Path) -> io::Result<Stored> {
    let dir = take.join(DIR);
    let text = fs::read_to_string(dir.join(HEADER))
        .map_err(|e| io::Error::new(e.kind(), format!("{DIR}/{HEADER}: {e}")))?;
    let format = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("format")?.as_str().map(String::from));
    match format.as_deref() {
        Some(FORMAT) => {}
        Some(f) if f.starts_with("timegrapher.analysis/") => {
            return Err(invalid(format!(
                "{DIR}/{HEADER} is in the layout {f}, made by a newer version: \
                 this one reads {FORMAT}"
            )))
        }
        _ => return Err(invalid(format!("{DIR}/{HEADER} is not a take's analysis"))),
    }
    let header: Header =
        serde_json::from_str(&text).map_err(|e| invalid(format!("{DIR}/{HEADER}: {e}")))?;
    check_header(&header).map_err(|e| invalid(format!("{DIR}/{HEADER}: {e}")))?;
    let gunzip = |name: &str| -> io::Result<Vec<u8>> {
        let raw = fs::read(dir.join(name))
            .map_err(|e| io::Error::new(e.kind(), format!("{DIR}/{name}: {e}")))?;
        let mut out = Vec::new();
        GzDecoder::new(&raw[..])
            .read_to_end(&mut out)
            .map_err(|e| invalid(format!("{DIR}/{name}: {e}")))?;
        Ok(out)
    };
    let text = |name: &str| -> io::Result<String> {
        String::from_utf8(gunzip(name)?).map_err(|e| invalid(format!("{DIR}/{name}: {e}")))
    };
    let (beats, clipped) = parse_beats(&text(BEATS)?, header.bph)
        .map_err(|e| invalid(format!("{DIR}/{BEATS}: {e}")))?;
    let windows =
        parse_windows(&text(WINDOWS)?).map_err(|e| invalid(format!("{DIR}/{WINDOWS}: {e}")))?;
    let profiles = parse_profiles(&gunzip(PROFILES)?)
        .map_err(|e| invalid(format!("{DIR}/{PROFILES}: {e}")))?;
    for (name, want, got) in [
        (BEATS, header.beats, beats.len()),
        (WINDOWS, header.windows, windows.len()),
        (PROFILES, header.profiles, profiles.len()),
    ] {
        if want != got {
            return Err(invalid(format!(
                "{DIR}/{name} holds {got} rows where {HEADER} says {want}"
            )));
        }
    }
    let log = BeatLog {
        sample_rate: header.sample_rate,
        duration_s: header.duration_s,
        bph: header.bph,
        lift_deg: header.lift_deg,
        beats,
        amplitude_windows: windows,
        clipped_beats: clipped,
        clipped_samples: header.clipped_samples,
        by_count: crate::breaks::by_count(header.jumps.iter().copied(), &header.sided),
        jumps: header.jumps.clone(),
        sided: header.sided.clone(),
        snippets: Vec::new(),
    };
    Ok(Stored {
        header,
        log,
        profiles,
    })
}

/// `v` when it is a finite number, else an error naming `what`.
fn finite(what: &str, v: f64) -> Result<f64, String> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(format!("{what} is {v}, not a number"))
    }
}

/// `v` when it is a finite number above 0, else an error naming `what`.
fn above_0(what: &str, v: f64) -> Result<f64, String> {
    if finite(what, v)? > 0.0 {
        Ok(v)
    } else {
        Err(format!("{what} is {v}: it must be above 0"))
    }
}

/// Times that must be numbers and run forwards (equal times allowed).
fn in_order(what: &str, times: &[f64]) -> Result<(), String> {
    for (i, &t) in times.iter().enumerate() {
        finite(&format!("{what} {}", i + 1), t)?;
        if i > 0 && t < times[i - 1] {
            return Err(format!(
                "{what} {} runs backwards: {t} after {}",
                i + 1,
                times[i - 1]
            ));
        }
    }
    Ok(())
}

/// The header's figures, as a measurement would have written them: a
/// sample rate, frame size, beat rate, lift and length above 0, and every
/// other figure a number, with the jumps in order.
fn check_header(h: &Header) -> Result<(), String> {
    above_0("sample_rate", h.sample_rate as f64)?;
    above_0("bytes_per_frame", h.bytes_per_frame as f64)?;
    above_0("bph", h.bph as f64)?;
    above_0("lift_deg", h.lift_deg)?;
    above_0("duration_s", h.duration_s)?;
    in_order("jump", &h.jumps)?;
    in_order("sided jump", &h.sided)?;
    if let Some(u) = &h.usb_clock {
        finite("usb_clock.ppm", u.ppm)?;
        finite("usb_clock.spread_ppm", u.spread_ppm)?;
    }
    if let Some(v) = h.signal_median {
        finite("signal_median", v as f64)?;
    }
    for (i, sh) in h.shape.iter().enumerate() {
        let what = |f: &str| format!("shape {} {f}", i + 1);
        if finite(&what("from_s"), sh.from_s)? > finite(&what("to_s"), sh.to_s)? {
            return Err(format!("{} ends before it starts", what("stretch")));
        }
    }
    Ok(())
}

fn gzip(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut e = GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(bytes)?;
    e.finish()
}

/// The nominal beat in µs at `bph`.
fn beat_us(bph: u32) -> i64 {
    (3_600_000_000.0 / bph.max(1) as f64).round() as i64
}

/// One row a beat: the step in its number from the beat before (from 0
/// for the first), its time in whole µs less where the beat before and
/// that many nominal beats put it (from 0 µs for the first), its quality
/// to 3 decimals, and 1 when it clipped.
fn beats_csv(log: &BeatLog) -> String {
    let nominal = beat_us(log.bph);
    let mut out = String::with_capacity(log.beats.len() * 16 + 64);
    out.push_str(BEATS_COLUMNS);
    out.push('\n');
    let (mut idx, mut us) = (0i64, 0i64);
    let mut clipped = log.clipped_beats.iter().peekable();
    for b in &log.beats {
        let t = (b.time * 1e6).round() as i64;
        let step = b.index - idx;
        let residual = t - us - step * nominal;
        while clipped.next_if(|&&c| c < b.time).is_some() {}
        let clip = clipped.next_if(|&&c| c == b.time).is_some();
        out.push_str(&format!(
            "{step},{residual},{},{}\n",
            b.quality,
            u8::from(clip)
        ));
        idx = b.index;
        us = t;
    }
    out
}

fn parse_beats(text: &str, bph: u32) -> Result<(Vec<Beat>, Vec<f64>), String> {
    let nominal = beat_us(bph);
    let mut lines = text.lines();
    if lines.next() != Some(BEATS_COLUMNS) {
        return Err(format!("the first line isn't {BEATS_COLUMNS}"));
    }
    let (mut idx, mut us) = (0i64, 0i64);
    let (mut beats, mut clipped) = (Vec::new(), Vec::new());
    for (n, line) in lines.enumerate() {
        let bad = || format!("row {} isn't a beat: {line}", n + 1);
        let mut f = line.split(',');
        let mut next = || f.next().ok_or_else(bad);
        let step: i64 = next()?.parse().map_err(|_| bad())?;
        let residual: i64 = next()?.parse().map_err(|_| bad())?;
        let quality: f32 = next()?.parse().map_err(|_| bad())?;
        let clip = next()? == "1";
        // Each beat comes after the one before, in number and in time, and
        // the numbers stay in range.
        let wrong = |why: &str| format!("row {}: {why}: {line}", n + 1);
        if !quality.is_finite() {
            return Err(wrong("its quality isn't a number"));
        }
        if n > 0 && step < 1 {
            return Err(wrong("its number doesn't follow the beat before"));
        }
        let (Some(i), Some(t)) = (
            idx.checked_add(step),
            step.checked_mul(nominal)
                .and_then(|d| d.checked_add(residual))
                .and_then(|d| d.checked_add(us)),
        ) else {
            return Err(wrong("its number or time is out of range"));
        };
        if t < 0 || (n > 0 && t <= us) {
            return Err(wrong("its time doesn't come after the beat before"));
        }
        (idx, us) = (i, t);
        let time = us as f64 / 1e6;
        beats.push(Beat {
            index: idx,
            time,
            quality,
        });
        if clip {
            clipped.push(time);
        }
    }
    Ok((beats, clipped))
}

fn windows_csv(windows: &[AmplitudeWindow]) -> String {
    let o = |v: Option<f64>| v.map_or(String::new(), |v| v.to_string());
    let mut out = String::from(WINDOWS_COLUMNS);
    out.push('\n');
    for w in windows {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            w.start_s,
            w.end_s,
            o(w.even_deg),
            o(w.odd_deg),
            o(w.beat_error_ms),
            o(w.beat_error_unlock_ms),
            o(w.even_unlock_ms),
            o(w.odd_unlock_ms),
            o(w.even_drop_ms),
            o(w.odd_drop_ms)
        ));
    }
    out
}

fn parse_windows(text: &str) -> Result<Vec<AmplitudeWindow>, String> {
    let mut lines = text.lines();
    if lines.next() != Some(WINDOWS_COLUMNS) {
        return Err(format!("the first line isn't {WINDOWS_COLUMNS}"));
    }
    lines
        .enumerate()
        .map(|(n, line)| {
            let bad = || format!("row {} isn't a window: {line}", n + 1);
            let f: Vec<&str> = line.split(',').collect();
            if f.len() != 10 {
                return Err(bad());
            }
            let num = |s: &str| s.parse::<f64>().map_err(|_| bad());
            let opt = |s: &str| (!s.is_empty()).then(|| num(s)).transpose();
            let w = AmplitudeWindow {
                start_s: num(f[0])?,
                end_s: num(f[1])?,
                even_deg: opt(f[2])?,
                odd_deg: opt(f[3])?,
                beat_error_ms: opt(f[4])?,
                beat_error_unlock_ms: opt(f[5])?,
                even_unlock_ms: opt(f[6])?,
                odd_unlock_ms: opt(f[7])?,
                even_drop_ms: opt(f[8])?,
                odd_drop_ms: opt(f[9])?,
            };
            let figures = [
                Some(w.start_s),
                Some(w.end_s),
                w.even_deg,
                w.odd_deg,
                w.beat_error_ms,
                w.beat_error_unlock_ms,
                w.even_unlock_ms,
                w.odd_unlock_ms,
                w.even_drop_ms,
                w.odd_drop_ms,
            ];
            if figures.iter().flatten().any(|v| !v.is_finite()) || w.start_s > w.end_s {
                return Err(bad());
            }
            Ok(w)
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|ws| {
            in_order(
                "window start",
                &ws.iter().map(|w| w.start_s).collect::<Vec<_>>(),
            )?;
            Ok(ws)
        })
}

/// The profiles, little-endian: the 8-byte magic, the stretch length (f32
/// s), the first point and the spacing (f32 ms each), the points a curve
/// (u16), 2 bytes kept at 0 and the stretches (u32); then each stretch:
/// its end (f64 s) and signal figure (f32, NaN when none), then Tick and
/// Tock, each a byte 0 (none) or 1 followed by: beats (u16), top and floor
/// (f32 envelope levels), unlock, drop, peak, amplitude and sounds 1 to 3
/// (f32, ms or degrees, NaN when none), then the median, 10th and 90th
/// percentile curves, a byte a point: quarter-dB steps above
/// [`PROFILE_RANGE_DB`] below the top, 0 for lower.
fn profiles_bin(profiles: &[StretchProfiles]) -> Vec<u8> {
    let first = profiles
        .iter()
        .flat_map(|p| p.sides.iter().flatten())
        .next();
    let stride = first.map_or(2, |p| {
        (PROFILE_POINT_MS / p.step_ms).round().max(1.0) as usize
    });
    let t0 = first.map_or(-beats::PRE_S * 1000.0, |p| p.t0_ms);
    let step = first.map_or(PROFILE_POINT_MS, |p| p.step_ms * stride as f64);
    let points = first.map_or(0, |p| p.median.len().div_ceil(stride));
    let mut out = Vec::with_capacity(32 + profiles.len() * (13 + 2 * (40 + 3 * points)));
    out.extend_from_slice(PROFILE_MAGIC);
    out.extend_from_slice(&(PROFILE_EVERY_S as f32).to_le_bytes());
    out.extend_from_slice(&(t0 as f32).to_le_bytes());
    out.extend_from_slice(&(step as f32).to_le_bytes());
    out.extend_from_slice(&(points as u16).to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(profiles.len() as u32).to_le_bytes());
    let f = |v: Option<f64>| v.map_or(f32::NAN, |v| v as f32).to_le_bytes();
    for p in profiles {
        out.extend_from_slice(&p.end_s.to_le_bytes());
        out.extend_from_slice(&p.signal.unwrap_or(f32::NAN).to_le_bytes());
        for side in &p.sides {
            let Some(s) = side else {
                out.push(0);
                continue;
            };
            out.push(1);
            let top = s
                .p90
                .iter()
                .chain(&s.median)
                .copied()
                .fold(0.0f32, f32::max);
            out.extend_from_slice(&(s.beats.min(u16::MAX as usize) as u16).to_le_bytes());
            out.extend_from_slice(&top.to_le_bytes());
            out.extend_from_slice(&s.floor.to_le_bytes());
            for v in [
                s.unlock_ms,
                s.drop_ms,
                s.peak_ms,
                s.amplitude_deg,
                s.sound1_ms,
                s.sound2_ms,
                s.sound3_ms,
            ] {
                out.extend_from_slice(&f(v));
            }
            for curve in [&s.median, &s.p10, &s.p90] {
                for i in 0..points {
                    let v = curve.get(i * stride).copied().unwrap_or(0.0);
                    out.push(level_byte(v, top));
                }
            }
        }
    }
    out
}

fn level_byte(v: f32, top: f32) -> u8 {
    if v <= 0.0 || top <= 0.0 {
        return 0;
    }
    let db = 20.0 * (v as f64 / top as f64).log10();
    ((db + PROFILE_RANGE_DB) * 4.0).round().clamp(0.0, 255.0) as u8
}

fn byte_level(b: u8, top: f32) -> f32 {
    if b == 0 {
        return 0.0;
    }
    top * 10f64.powf((b as f64 / 4.0 - PROFILE_RANGE_DB) / 20.0) as f32
}

fn parse_profiles(raw: &[u8]) -> Result<Vec<StretchProfiles>, String> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let s = raw
            .get(at..at + n)
            .ok_or_else(|| "it ends part way through".to_string())?;
        at += n;
        Ok(s)
    };
    if take(8)? != PROFILE_MAGIC {
        return Err("it doesn't start as a profiles file".into());
    }
    let f32_of = |b: &[u8]| f32::from_le_bytes(b.try_into().expect("4 bytes"));
    let _every = f32_of(take(4)?);
    let t0 = finite("the first point", f32_of(take(4)?) as f64)?;
    let step = above_0("the spacing", f32_of(take(4)?) as f64)?;
    let points = u16::from_le_bytes(take(2)?.try_into().expect("2 bytes")) as usize;
    take(2)?;
    let count = u32::from_le_bytes(take(4)?.try_into().expect("4 bytes")) as usize;
    let mut out = Vec::with_capacity(count.min(1 << 20));
    for _ in 0..count {
        let end_s = finite(
            "a stretch's end",
            f64::from_le_bytes(take(8)?.try_into().expect("8 bytes")),
        )?;
        // NaN is how a missing figure is written; any other figure is a
        // number.
        let mark = |v: f32| -> Result<Option<f32>, String> {
            match v {
                v if v.is_nan() => Ok(None),
                v => finite("a profile figure", v as f64).map(|_| Some(v)),
            }
        };
        let signal = mark(f32_of(take(4)?))?;
        let mut sides: [Option<TickProfile>; 2] = [None, None];
        for (i, side) in [Side::A, Side::B].into_iter().enumerate() {
            if take(1)?[0] == 0 {
                continue;
            }
            let beats = u16::from_le_bytes(take(2)?.try_into().expect("2 bytes")) as usize;
            let top = finite("a profile's top", f32_of(take(4)?) as f64)? as f32;
            let floor = finite("a profile's floor", f32_of(take(4)?) as f64)? as f32;
            let mut marks = [None; 7];
            for m in &mut marks {
                *m = mark(f32_of(take(4)?))?.map(f64::from);
            }
            let mut curve = || -> Result<Vec<f32>, String> {
                Ok(take(points)?.iter().map(|&b| byte_level(b, top)).collect())
            };
            let (median, p10, p90) = (curve()?, curve()?, curve()?);
            sides[i] = Some(TickProfile {
                side,
                beats,
                t0_ms: t0,
                step_ms: step,
                median,
                p10,
                p90,
                floor,
                unlock_ms: marks[0],
                drop_ms: marks[1],
                peak_ms: marks[2],
                amplitude_deg: marks[3],
                sound1_ms: marks[4],
                sound2_ms: marks[5],
                sound3_ms: marks[6],
            });
        }
        out.push(StretchProfiles {
            end_s,
            signal,
            sides,
        });
    }
    if at != raw.len() {
        return Err("it runs on past its last stretch".into());
    }
    in_order(
        "stretch end",
        &out.iter().map(|p| p.end_s).collect::<Vec<_>>(),
    )?;
    Ok(out)
}

/// Keep the take whose sound is in `source` (its folder: `session.toml`,
/// `session.json`, `clock.csv` and the sound) as its analysis alone in a
/// new folder `dest`: every file but the sound copied, `session.toml`
/// replaced by `manifest_toml` (which says `audio = false`), and
/// `analysis/` written from `stored`. Written beside `dest` first and
/// moved into place whole, so a failure leaves nothing half-written; an
/// existing `dest` is never touched.
pub fn save(dest: &Path, companions: &[(String, Vec<u8>)], stored: &Stored) -> io::Result<PathBuf> {
    if fs::symlink_metadata(dest).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is already there", dest.display()),
        ));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder to save in"))?;
    fs::create_dir_all(parent)?;
    let name = dest
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no name to save under"))?;
    let staged = parent.join(format!(
        ".timegrapher-save-{}-{}",
        std::process::id(),
        name.to_string_lossy()
    ));
    let _ = fs::remove_dir_all(&staged);
    let result = (|| {
        fs::create_dir(&staged)?;
        for (file, bytes) in companions {
            let mut parts = Path::new(file).components();
            if !matches!(parts.next(), Some(std::path::Component::Normal(_)))
                || parts.next().is_some()
                || file.eq_ignore_ascii_case(DIR)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid companion file name: {file}"),
                ));
            }
            fs::write(staged.join(file), bytes)?;
        }
        write(&staged, stored)?;
        // Read back what was written before it goes into place.
        let back = read(&staged)?;
        if back.log.beats.len() != stored.log.beats.len() {
            return Err(invalid("the analysis didn't read back as written".into()));
        }
        fs::rename(&staged, dest)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staged);
    }
    result.map(|()| dest.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing;

    #[test]
    fn the_shape_is_measured_clear_of_masked_stretches() {
        assert_eq!(shape_stretch(&[], 30.0, 600.0), Some((30.0, SHAPE_S)));
        // A bump in the first minute: the next clear minute.
        assert_eq!(
            shape_stretch(&[(50.0, 58.0)], 30.0, 600.0),
            Some((58.0, SHAPE_S))
        );
        // No clear minute: the longest clear stretch.
        assert_eq!(
            shape_stretch(&[(40.0, 50.0), (70.0, 75.0)], 30.0, 100.0),
            Some((75.0, 25.0))
        );
        // A short clear stretch first: passed over for a whole minute.
        assert_eq!(
            shape_stretch(&[(40.0, 50.0)], 30.0, 600.0),
            Some((50.0, SHAPE_S))
        );
        // The longest clear stretch is the first one, not the last.
        assert_eq!(
            shape_stretch(&[(70.0, 75.0), (85.0, 95.0)], 30.0, 100.0),
            Some((30.0, 40.0))
        );
        assert_eq!(shape_stretch(&[(0.0, 700.0)], 30.0, 600.0), None);
    }
    use crate::synth::{generate, SynthConfig};

    fn take(dir: &Path, seconds: f64) -> PathBuf {
        let audio = generate(
            &SynthConfig {
                duration_s: seconds,
                beat_error_ms: 0.6,
                odd_gains: Some([0.9, 0.25, 1.0]),
                ..Default::default()
            },
            |t| 270.0 + 10.0 * (t / 10.0).sin(),
            |_| 0.0,
        );
        let path = dir.join("take_part01-001.wav");
        audio::write_wav(&path, &audio).unwrap();
        path
    }

    fn scratch(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("tg-analysis-take-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn software() -> serde_json::Value {
        serde_json::json!({"name": "timegrapher", "version": "test"})
    }

    /// What is kept reads back as it was measured: every beat to the µs
    /// with its number, quality and clipping, every amplitude window
    /// exactly, the profiles to a quarter dB, and the header.
    #[test]
    #[expect(
        clippy::cognitive_complexity,
        reason = "over the limit before it was set; split it (architecture guide, section 12)"
    )]
    fn a_kept_analysis_reads_back_as_measured() {
        let dir = scratch("roundtrip");
        let wav = take(&dir, 40.0);
        let m = Manifest {
            recordings: vec![session::RecordingEntry {
                file: ".".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let cfg = StreamConfig::default();
        let shapes = shape_stretches(&m, 40.0);
        let mut s = measure(&[wav.as_path()], &cfg, &shapes, software(), |_| {}).unwrap();
        // A clipped beat, to see it kept.
        let clip = s.log.beats[10].time;
        s.log.clipped_beats = vec![clip];
        assert!(s.log.beats.len() > 300);
        assert!(s.profiles.len() >= 19, "{}", s.profiles.len());
        assert!(s.header.signal_median.is_some_and(|v| v > 4.0));
        assert_eq!(s.header.shape.len(), 1);
        assert_eq!(s.header.shape[0].from_s, 20.0);
        assert!(!is_analysis_only(&dir));
        write(&dir, &s).unwrap();
        assert!(is_analysis_only(&dir));
        let back = read(&dir).unwrap();
        assert_eq!(back.log.beats.len(), s.log.beats.len());
        for (a, b) in back.log.beats.iter().zip(&s.log.beats) {
            assert_eq!(a.index, b.index);
            assert!(
                (a.time - b.time).abs() <= 0.5e-6 + 1e-9,
                "{} {}",
                a.time,
                b.time
            );
            assert_eq!(a.quality, b.quality);
        }
        assert_eq!(back.log.clipped_beats.len(), 1);
        assert!((back.log.clipped_beats[0] - clip).abs() < 1e-6);
        assert_eq!(
            windows_csv(&back.log.amplitude_windows),
            windows_csv(&s.log.amplitude_windows)
        );
        assert_eq!(back.log.jumps, s.log.jumps);
        assert_eq!(back.log.sided, s.log.sided);
        assert_eq!(back.log.by_count, s.log.by_count);
        assert_eq!(back.log.duration_s, s.log.duration_s);
        assert_eq!(back.profiles.len(), s.profiles.len());
        for (a, b) in back.profiles.iter().zip(&s.profiles) {
            assert_eq!(a.end_s, b.end_s);
            for (x, y) in a.sides.iter().zip(&b.sides) {
                let (x, y) = (x.as_ref().unwrap(), y.as_ref().unwrap());
                assert_eq!(x.beats, y.beats);
                assert_eq!(
                    x.amplitude_deg.map(|v| v as f32),
                    y.amplitude_deg.map(|v| v as f32)
                );
                let top = y.p90.iter().copied().fold(0.0f32, f32::max);
                for (i, &v) in x.median.iter().enumerate() {
                    let want = y.median[i * 2];
                    if want > top * 1e-3 {
                        let db = 20.0 * (v / want).log10();
                        assert!(db.abs() <= 0.13, "point {i}: {v} for {want}");
                    }
                }
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// The beats of the kept take give the same readings as the sound.
    #[test]
    fn readings_from_the_kept_beats_match_the_sound() {
        let dir = scratch("readings");
        let wav = take(&dir, 60.0);
        let cfg = StreamConfig::default();
        let s = measure(&[wav.as_path()], &cfg, &[], software(), |_| {}).unwrap();
        write(&dir, &s).unwrap();
        let back = read(&dir).unwrap();
        let fit = |l: &BeatLog| timing::fit(&l.beats, l.bph).unwrap();
        let (a, b) = (fit(&s.log), fit(&back.log));
        assert!((a.rate_s_per_day - b.rate_s_per_day).abs() < 0.05);
        assert!((a.beat_error_ms - b.beat_error_ms).abs() < 0.001);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A take saved as its analysis gets every file but the sound, in a
    /// new folder; an existing folder is left alone.
    #[test]
    fn saving_writes_a_new_folder_and_never_an_old_one() {
        let dir = scratch("save");
        let wav = take(&dir, 20.0);
        let s = measure(
            &[wav.as_path()],
            &StreamConfig::default(),
            &[],
            software(),
            |_| {},
        )
        .unwrap();
        let dest = dir.join("kept");
        let companions = vec![
            ("session.toml".to_string(), b"audio = false\n".to_vec()),
            ("clock.csv".to_string(), b"".to_vec()),
        ];
        save(&dest, &companions, &s).unwrap();
        assert!(is_analysis_only(&dest));
        assert!(dest.join("clock.csv").is_file());
        let names: Vec<_> = fs::read_dir(&dest)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!names
            .iter()
            .any(|n| n.ends_with(".wav") || n.ends_with(".flac")));
        let err = save(&dest, &companions, &s).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(fs::read_dir(&dir).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".timegrapher")));
        let _ = fs::remove_dir_all(&dir);
    }

    /// A header from a later layout says so, rather than misreading it.
    #[test]
    fn a_later_layout_is_refused_by_name() {
        let dir = scratch("later");
        fs::create_dir_all(dir.join(DIR)).unwrap();
        fs::write(
            dir.join(DIR).join(HEADER),
            r#"{"format": "timegrapher.analysis/2"}"#,
        )
        .unwrap();
        let e = read(&dir).unwrap_err().to_string();
        assert!(
            e.contains("timegrapher.analysis/2") && e.contains("newer"),
            "{e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A kept analysis with numbers no measurement writes is refused with
    /// the file and what is wrong, and never panics: the five cases of the
    /// 2026-10-26 audit (finding ARCH-04).
    #[test]
    fn corrupt_numbers_are_refused_by_name() {
        let dir = scratch("corrupt");
        let wav = take(&dir, 20.0);
        let good = measure(
            &[wav.as_path()],
            &StreamConfig::default(),
            &[],
            software(),
            |_| {},
        )
        .unwrap();
        let refused = |s: &Stored, edit: &dyn Fn(&Path)| -> String {
            write(&dir, s).unwrap();
            edit(&dir.join(DIR));
            read(&dir).map(|_| ()).unwrap_err().to_string()
        };
        let none = |_: &Path| {};
        assert!({
            write(&dir, &good).unwrap();
            read(&dir).is_ok()
        });
        // 1. Header figures.
        for (field, value, file) in [
            ("sample_rate", "0", HEADER),
            ("bph", "0", HEADER),
            ("lift_deg", "-52.0", HEADER),
            ("duration_s", "-10.0", HEADER),
        ] {
            let e = refused(&good, &|d: &Path| {
                let p = d.join(HEADER);
                let mut v: serde_json::Value =
                    serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
                v[field] = serde_json::from_str(value).unwrap();
                fs::write(&p, v.to_string()).unwrap();
            });
            assert!(e.contains(file) && e.contains(field), "{field}: {e}");
        }
        // 2. A beat's quality, and a window's amplitude and beat error,
        // that are not numbers.
        let set_beat = |d: &Path, i: usize, row: &str| {
            let raw = fs::read(d.join(BEATS)).unwrap();
            let mut text = String::new();
            GzDecoder::new(&raw[..]).read_to_string(&mut text).unwrap();
            let mut rows: Vec<&str> = text.lines().collect();
            rows[i] = row;
            fs::write(
                d.join(BEATS),
                gzip((rows.join("\n") + "\n").as_bytes()).unwrap(),
            )
            .unwrap();
        };
        let e = refused(&good, &|d: &Path| set_beat(d, 3, "1,0,NaN,0"));
        assert!(e.contains(BEATS) && e.contains("row 3"), "{e}");
        let mut s = good.clone();
        s.log.amplitude_windows[1].even_deg = Some(f64::INFINITY);
        let e = refused(&s, &none);
        assert!(e.contains(WINDOWS) && e.contains("row 2"), "{e}");
        let mut s = good.clone();
        s.log.amplitude_windows[0].beat_error_ms = Some(f64::NAN);
        let e = refused(&s, &none);
        assert!(e.contains(WINDOWS) && e.contains("row 1"), "{e}");
        // 3. Beats going backwards, by a negative step.
        let e = refused(&good, &|d: &Path| set_beat(d, 2, "-1,0,1,0"));
        assert!(e.contains(BEATS) && e.contains("row 2"), "{e}");
        // 4. A step that overflows the arithmetic.
        let e = refused(&good, &|d: &Path| {
            set_beat(d, 2, &format!("{},0,1,0", i64::MAX))
        });
        assert!(e.contains(BEATS) && e.contains("out of range"), "{e}");
        // 5. A profile stretch whose end is not a number.
        let mut s = good.clone();
        s.profiles[0].end_s = f64::INFINITY;
        let e = refused(&s, &none);
        assert!(e.contains(PROFILES), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn shape_stretches_skip_masks_and_settling() {
        let m = Manifest {
            settle_s: Some(10.0),
            recordings: vec![
                session::RecordingEntry {
                    file: ".".into(),
                    end_s: Some(200.0),
                    masked: vec![[30.0, 40.0]],
                    ..Default::default()
                },
                session::RecordingEntry {
                    file: ".".into(),
                    start_s: Some(200.0),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let s = shape_stretches(&m, 300.0);
        assert_eq!(s, vec![Some((40.0, 100.0)), Some((210.0, 270.0))]);
        let off = Manifest {
            shape: Some(false),
            ..m
        };
        assert_eq!(shape_stretches(&off, 300.0), vec![None, None]);
    }
}
