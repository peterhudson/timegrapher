//! Calibrating the sound card's clock against a trusted one.
//!
//! A sound card's crystal is typically off by 10–50 ppm (1–4 s/d) and
//! drifts with temperature, so a rate read against it is only as good as
//! the crystal. During a recording, a log of the audio position against
//! NTP-disciplined system time lets each beat time be mapped onto true
//! time. Each log entry is a pair: seconds of audio captured so far (from
//! the frame count) and the system clock at that moment.

use crate::dsp::{median, robust_sd};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

/// The fitted mapping from audio time to true time.
#[derive(Debug, Clone, Serialize)]
pub struct ClockFit {
    /// Entries used and entries rejected as outliers (late timestamps).
    pub points: usize,
    pub rejected: usize,
    /// Span of the log, seconds.
    pub span_s: f64,
    /// How much faster true time runs than the sound card, ppm. Positive
    /// means the sound card is slow, so uncorrected rates read too fast.
    pub ppm: f64,
    /// The same as a rate error, s/d: subtract it from an uncorrected rate.
    pub rate_error_s_per_day: f64,
    /// Standard error of `ppm` from the scatter of the entries about one
    /// straight line.
    pub ppm_sd: f64,
    /// RMS of the entries about the mapping, milliseconds.
    pub residual_ms: f64,
    /// Whether the mapping follows slow drift (local fits) rather than
    /// one straight line.
    pub tracks_drift: bool,
    /// Knots of the mapping: audio seconds and true seconds, both from the
    /// first entry's values.
    #[serde(skip)]
    knots: Vec<(f64, f64)>,
    #[serde(skip)]
    origin: (f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClockError {
    TooFewPoints(usize),
    NotIncreasing,
}

impl std::fmt::Display for ClockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClockError::TooFewPoints(n) => write!(f, "clock log has {n} usable entries; need 3"),
            ClockError::NotIncreasing => write!(f, "clock log times do not increase"),
        }
    }
}

impl std::error::Error for ClockError {}

/// Least-squares line `y = a + b x`.
fn line(p: &[(f64, f64)]) -> (f64, f64) {
    let (a, b, _) = line_sd(p);
    (a, b)
}

/// Least-squares line `y = a + b x` and the standard error of `b`.
fn line_sd(p: &[(f64, f64)]) -> (f64, f64, f64) {
    let n = p.len() as f64;
    let mx = p.iter().map(|q| q.0).sum::<f64>() / n;
    let my = p.iter().map(|q| q.1).sum::<f64>() / n;
    let sxx: f64 = p.iter().map(|q| (q.0 - mx) * (q.0 - mx)).sum();
    let sxy: f64 = p.iter().map(|q| (q.0 - mx) * (q.1 - my)).sum();
    let b = if sxx > 0.0 { sxy / sxx } else { 1.0 };
    let a = my - b * mx;
    let ss: f64 = p.iter().map(|q| (q.1 - a - b * q.0).powi(2)).sum();
    let sd = if p.len() > 2 && sxx > 0.0 {
        (ss / (n - 2.0) / sxx).sqrt()
    } else {
        f64::INFINITY
    };
    (a, b, sd)
}

/// Runs longer than this get a mapping that follows drift.
const DRIFT_SPAN_S: f64 = 4.0 * 3600.0;
/// Half-width of each local fit when following drift.
const DRIFT_HALF_S: f64 = 3600.0;

impl ClockFit {
    /// Fit `(audio_s, true_s)` pairs. Entries more than 5 robust SDs off a
    /// first straight-line fit are dropped (a timestamp taken late because
    /// the logger was descheduled). Runs over four hours are mapped with
    /// local line fits over two hours, every ten minutes, so slow
    /// temperature drift of the crystal is followed.
    pub fn new(pairs: &[(f64, f64)]) -> Result<ClockFit, ClockError> {
        let mut p: Vec<(f64, f64)> = pairs
            .iter()
            .copied()
            .filter(|q| q.0.is_finite() && q.1.is_finite())
            .collect();
        if p.len() < 3 {
            return Err(ClockError::TooFewPoints(p.len()));
        }
        p.sort_by(|a, b| a.0.total_cmp(&b.0));
        let origin = p[0];
        for q in p.iter_mut() {
            *q = (q.0 - origin.0, q.1 - origin.1);
        }
        if p[p.len() - 1].0 <= 0.0 || p[p.len() - 1].1 <= 0.0 {
            return Err(ClockError::NotIncreasing);
        }
        let total = p.len();
        let (a, b) = line(&p);
        let res: Vec<f64> = p.iter().map(|q| q.1 - (a + b * q.0)).collect();
        let sd = robust_sd(&res).max(1e-4);
        let mut med = res.clone();
        let m = median(&mut med);
        p = p
            .into_iter()
            .zip(&res)
            .filter(|(_, r)| (*r - m).abs() < 5.0 * sd)
            .map(|(q, _)| q)
            .collect();
        if p.len() < 3 {
            return Err(ClockError::TooFewPoints(p.len()));
        }
        let (a, b, b_sd) = line_sd(&p);
        let span = p[p.len() - 1].0 - p[0].0;
        let tracks_drift = span > DRIFT_SPAN_S;
        let knots = if tracks_drift {
            let step = 600.0;
            let mut k = Vec::new();
            let mut x = p[0].0;
            loop {
                let near: Vec<(f64, f64)> = p
                    .iter()
                    .copied()
                    .filter(|q| (q.0 - x).abs() <= DRIFT_HALF_S)
                    .collect();
                if near.len() >= 3 {
                    let (la, lb) = line(&near);
                    k.push((x, la + lb * x));
                }
                if x >= p[p.len() - 1].0 {
                    break;
                }
                x = (x + step).min(p[p.len() - 1].0);
            }
            k
        } else {
            let (x0, x1) = (p[0].0, p[p.len() - 1].0);
            vec![(x0, a + b * x0), (x1, a + b * x1)]
        };
        let mut fit = ClockFit {
            points: p.len(),
            rejected: total - p.len(),
            span_s: span,
            ppm: (b - 1.0) * 1e6,
            rate_error_s_per_day: (b - 1.0) * 86400.0,
            ppm_sd: b_sd * 1e6,
            residual_ms: 0.0,
            tracks_drift,
            knots,
            origin,
        };
        let ss: f64 = p
            .iter()
            .map(|q| {
                let r = fit.map_rel(q.0) - q.1;
                r * r
            })
            .sum();
        fit.residual_ms = (ss / p.len() as f64).sqrt() * 1e3;
        Ok(fit)
    }

    /// True time relative to the first entry, piecewise linear, extended
    /// along the end segments.
    fn map_rel(&self, x: f64) -> f64 {
        let k = &self.knots;
        if k.len() == 1 {
            return k[0].1 + (x - k[0].0);
        }
        let i = match k.iter().position(|q| q.0 > x) {
            Some(0) => 0,
            Some(i) => i - 1,
            None => k.len() - 2,
        }
        .min(k.len() - 2);
        let (x0, y0) = k[i];
        let (x1, y1) = k[i + 1];
        y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    }

    /// Map an audio time (seconds from the start of the recording) onto
    /// true time, in seconds from the start of the recording.
    pub fn map(&self, audio_s: f64) -> f64 {
        let zero = self.map_rel(-self.origin.0);
        self.map_rel(audio_s - self.origin.0) - zero
    }
}

/// Audio seconds per kept entry in a [`ClockTracker`].
const TRACK_BUCKET_S: f64 = 1.0;

/// Measures a live input's clock against the system clock as audio
/// arrives, without a log file.
///
/// A block's arrival time is late by however long the driver held it,
/// which varies from block to block but never makes a block early. So in
/// each second of audio only the block that arrived least late is kept,
/// and the line through those follows the sound card's clock to a
/// fraction of a ppm in twenty minutes, as long as the system clock is
/// kept by NTP.
#[derive(Debug, Clone)]
pub struct ClockTracker {
    fs: f64,
    frames: u64,
    /// The current second's least-late entry: bucket number, audio
    /// seconds and system seconds.
    best: Option<(u64, f64, f64)>,
    pairs: Vec<(f64, f64)>,
}

impl ClockTracker {
    pub fn new(sample_rate: u32) -> ClockTracker {
        ClockTracker {
            fs: sample_rate as f64,
            frames: 0,
            best: None,
            pairs: Vec::new(),
        }
    }

    /// Add a block of `samples` frames that arrived at `at` (its last
    /// sample's arrival).
    pub fn push(&mut self, samples: usize, at: SystemTime) {
        self.frames += samples as u64;
        let Ok(t) = at.duration_since(UNIX_EPOCH) else {
            return;
        };
        let audio = self.frames as f64 / self.fs;
        let sys = t.as_secs_f64();
        let bucket = (audio / TRACK_BUCKET_S) as u64;
        match self.best {
            Some((b, a, s)) if b == bucket => {
                if sys - audio < s - a {
                    self.best = Some((bucket, audio, sys));
                }
            }
            Some((_, a, s)) => {
                self.pairs.push((a, s));
                self.best = Some((bucket, audio, sys));
            }
            None => self.best = Some((bucket, audio, sys)),
        }
    }

    /// Seconds of audio counted so far.
    pub fn audio_s(&self) -> f64 {
        self.frames as f64 / self.fs
    }

    /// The kept entries: audio seconds against system (Unix) seconds.
    pub fn pairs(&self) -> &[(f64, f64)] {
        &self.pairs
    }

    /// The fit so far; `None` until there are a few seconds of entries.
    pub fn fit(&self) -> Option<ClockFit> {
        ClockFit::new(&self.pairs).ok()
    }
}

fn is_time_name(c: &str) -> bool {
    ["unix_s", "unix", "time_s", "epoch", "ntp_s", "time"].contains(&c)
        || c.ends_with("_ns")
        || c.ends_with("_ms")
}

fn is_audio_name(c: &str) -> bool {
    [
        "audio_s",
        "audio_seconds",
        "frames",
        "samples",
        "frame",
        "sample",
    ]
    .contains(&c)
        || c.starts_with("bytes")
}

/// Parse a clock log: one entry per line, CSV or whitespace-separated,
/// `#` comments, with an optional header naming the columns. Other lines
/// that aren't all numbers (a note from the logger) are skipped.
///
/// - Audio position: `audio_s` (seconds), `frames` or `samples`, or
///   `bytes` (of audio data, `bytes_per_frame` to a frame).
/// - System time: `unix_s`, `unix`, `time_s` or `epoch` (seconds), or a
///   name ending in `_ns` or `_ms` for nanoseconds or milliseconds.
///
/// Without a header, the time column is the one that looks like a Unix
/// time (seconds, milliseconds or nanoseconds, told apart by size), the
/// audio column is the other one, and its unit (seconds, frames or bytes)
/// is the one that makes it advance at one second per second. Only the
/// slope of the mapping matters, so a constant offset in the audio count
/// (a file header, a pipe buffer) does no harm.
pub fn parse_log(
    text: &str,
    sample_rate: u32,
    bytes_per_frame: u32,
) -> Result<Vec<(f64, f64)>, String> {
    let fs = sample_rate as f64;
    let mut header: Option<Vec<String>> = None;
    let mut rows: Vec<Vec<f64>> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .filter(|c| !c.is_empty())
            .collect();
        let nums: Vec<Option<f64>> = cells.iter().map(|c| c.parse::<f64>().ok()).collect();
        if nums.iter().any(|v| v.is_none()) {
            // A header names a time and an audio column; any other line
            // that isn't all numbers (a note the logger wrote) is skipped.
            let names: Vec<String> = cells.iter().map(|c| c.to_ascii_lowercase()).collect();
            if header.is_none()
                && rows.is_empty()
                && names.iter().any(|c| is_time_name(c))
                && names.iter().any(|c| is_audio_name(c))
            {
                header = Some(names);
            }
            continue;
        }
        rows.push(nums.into_iter().map(|v| v.unwrap_or(f64::NAN)).collect());
    }
    if rows.len() < 2 {
        return Err(format!("{} entries; need at least 2", rows.len()));
    }
    let width = rows.iter().map(|r| r.len()).min().unwrap_or(0);
    let col = |i: usize| -> Vec<f64> { rows.iter().map(|r| r[i]).collect() };

    // Scale a time column to seconds from the size of its values.
    let time_scale = |v: &[f64]| -> Option<f64> {
        let x = v[0].abs();
        if (1e8..1e11).contains(&x) {
            Some(1.0)
        } else if (1e11..1e14).contains(&x) {
            Some(1e-3)
        } else if (1e17..1e20).contains(&x) {
            Some(1e-9)
        } else {
            None
        }
    };

    let (ai, ascale, ti, tscale) = if let Some(h) = &header {
        let find = |pred: &dyn Fn(&str) -> bool| h.iter().position(|c| pred(c));
        let ti = find(&|c| is_time_name(c))
            .ok_or_else(|| format!("header has no time column: {}", h.join(",")))?;
        let tscale = if h[ti].ends_with("_ns") {
            1e-9
        } else if h[ti].ends_with("_ms") {
            1e-3
        } else {
            1.0
        };
        let (ai, ascale) = if let Some(i) = find(&|c| c == "audio_s" || c == "audio_seconds") {
            (i, 1.0)
        } else if let Some(i) = find(&|c| ["frames", "samples", "frame", "sample"].contains(&c)) {
            (i, 1.0 / fs)
        } else if let Some(i) = find(&|c| c.starts_with("bytes")) {
            (i, 1.0 / (fs * bytes_per_frame as f64))
        } else {
            return Err(format!(
                "header has no audio column (audio_s, frames or bytes): {}",
                h.join(",")
            ));
        };
        (ai, ascale, ti, tscale)
    } else {
        if width < 2 {
            return Err("need two columns: system time and audio position".into());
        }
        let ti = (0..width)
            .find(|&i| time_scale(&col(i)).is_some())
            .ok_or("no column looks like a Unix time")?;
        let tscale = time_scale(&col(ti)).unwrap_or(1.0);
        let ai = (0..width)
            .find(|&i| i != ti)
            .ok_or("no audio position column")?;
        let (a, t) = (col(ai), col(ti));
        let (a0, a1) = (a[0], a[a.len() - 1]);
        let dt = (t[t.len() - 1] - t[0]) * tscale;
        if dt <= 0.0 {
            return Err("times do not increase".into());
        }
        let per_s = (a1 - a0) / dt;
        let ascale = [1.0, fs, fs * bytes_per_frame as f64]
            .into_iter()
            .map(|u| (u, (per_s / u).ln().abs()))
            .filter(|c| c.1.is_finite() && c.1 < 0.05)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|c| 1.0 / c.0)
            .ok_or_else(|| {
                format!("audio column advances {per_s:.1} per second: not seconds, frames or bytes at {sample_rate} Hz")
            })?;
        (ai, ascale, ti, tscale)
    };
    if ai >= width || ti >= width {
        return Err("a row is missing a column".into());
    }
    Ok(rows
        .iter()
        .map(|r| (r[ai] * ascale, r[ti] * tscale))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::Rng;

    #[test]
    fn recovers_a_slow_sound_card() {
        // The sound card runs 20 ppm slow; timestamps have 2 ms of jitter
        // and one entry was logged 400 ms late.
        let mut rng = Rng::new(4);
        let mut pairs: Vec<(f64, f64)> = (0..120)
            .map(|i| {
                let audio = i as f64 * 60.0;
                (audio, 1.7e9 + audio * (1.0 + 20e-6) + 0.002 * rng.normal())
            })
            .collect();
        pairs[50].1 += 0.4;
        let fit = ClockFit::new(&pairs).unwrap();
        assert!((fit.ppm - 20.0).abs() < 0.5, "{}", fit.ppm);
        assert_eq!(fit.rejected, 1);
        assert!((fit.rate_error_s_per_day - 1.728).abs() < 0.05);
        assert!(fit.ppm_sd > 0.0 && fit.ppm_sd < 0.2, "{}", fit.ppm_sd);
        assert!((fit.map(3600.0) - 3600.0 * (1.0 + 20e-6)).abs() < 0.002);
        assert!(!fit.tracks_drift);
    }

    #[test]
    fn follows_drift_on_long_runs() {
        // 24 h with the error drifting from 10 to 30 ppm.
        let pairs: Vec<(f64, f64)> = (0..1440)
            .map(|i| {
                // True time is the integral of 1 + (10 + 20 t / 86400) 1e-6.
                let a = i as f64 * 60.0;
                (a, a + 1e-6 * (10.0 * a + 10.0 * a * a / 86400.0))
            })
            .collect();
        let fit = ClockFit::new(&pairs).unwrap();
        assert!(fit.tracks_drift);
        assert!(fit.residual_ms < 1.0, "{}", fit.residual_ms);
        let want = |a: f64| a + 1e-6 * (10.0 * a + 10.0 * a * a / 86400.0);
        assert!((fit.map(43200.0) - want(43200.0)).abs() < 0.005);
    }

    #[test]
    fn a_tracker_sees_through_late_blocks() {
        // A card 20 ppm slow delivering 10 ms blocks, each arriving 2 to 40
        // ms late (never early), for 20 minutes.
        let mut rng = Rng::new(7);
        let mut tr = ClockTracker::new(48_000);
        let start = UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
        for i in 1..=120_000u64 {
            let audio = i as f64 * 0.01;
            let late = 0.002 + 0.038 * rng.next_f64();
            let at = start + std::time::Duration::from_secs_f64(audio * (1.0 + 20e-6) + late);
            tr.push(480, at);
        }
        let fit = tr.fit().expect("fit");
        assert!((fit.ppm - 20.0).abs() < 0.3, "{} ppm", fit.ppm);
        assert!(fit.ppm_sd < 0.3, "{} ppm sd", fit.ppm_sd);
        assert!((tr.audio_s() - 1200.0).abs() < 1e-9);
    }

    #[test]
    fn parses_frames_and_unix() {
        let text = "# clock log\nunix_s,frames\n1759948294.5,0\n1759948354.5,2880000\n";
        let p = parse_log(text, 48000, 2).unwrap();
        assert_eq!(p, vec![(0.0, 1759948294.5), (60.0, 1759948354.5)]);
    }

    #[test]
    fn guesses_nanoseconds_and_bytes_without_a_header() {
        // System time in ns, then bytes of 16-bit mono at 48 kHz, plus a
        // 44-byte header counted in.
        let text = "1791484354282000000 5760044\n1791484414282100000 11520044\n1791484474282000000 17280044\n";
        let p = parse_log(text, 48000, 2).unwrap();
        assert!((p[1].0 - p[0].0 - 60.0).abs() < 1e-9);
        assert!((p[1].1 - p[0].1 - 60.0001).abs() < 1e-6);
        let fit = ClockFit::new(&p).unwrap();
        assert!(fit.ppm.abs() < 2.0, "{}", fit.ppm);
    }

    #[test]
    fn skips_a_note_line_and_reads_fractional_seconds() {
        // As written by the 2-hour capture's logger.
        let text = "arecord_start_utc 2026-10-08T18:31:34.281846890Z pid 2748966\n\
                    1791484294.286333874 44\n\
                    1791484354.301726276 5760044\n\
                    1791484414.300000000 11520044\n";
        let p = parse_log(text, 48000, 2).unwrap();
        assert_eq!(p.len(), 3);
        assert!((p[1].0 - p[0].0 - 60.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_an_audio_column_of_unknown_unit() {
        let text = "1791484354 1000\n1791484414 2000\n";
        assert!(parse_log(text, 48000, 2).is_err());
    }
}
