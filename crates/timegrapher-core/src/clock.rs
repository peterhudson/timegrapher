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
    let n = p.len() as f64;
    let mx = p.iter().map(|q| q.0).sum::<f64>() / n;
    let my = p.iter().map(|q| q.1).sum::<f64>() / n;
    let sxx: f64 = p.iter().map(|q| (q.0 - mx) * (q.0 - mx)).sum();
    let sxy: f64 = p.iter().map(|q| (q.0 - mx) * (q.1 - my)).sum();
    let b = if sxx > 0.0 { sxy / sxx } else { 1.0 };
    (my - b * mx, b)
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
        let (a, b) = line(&p);
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
            rate_error_s_per_day: -(b - 1.0) * 86400.0,
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

/// Parse a clock log: CSV or whitespace-separated, `#` comments, with a
/// header naming the columns. The audio position is `audio_s` (seconds),
/// `frames` or `samples` (divided by the sample rate); the true time is
/// `unix_s`, `unix` or `time_s` (seconds). Without a header the first two
/// columns are taken as audio seconds and Unix seconds.
pub fn parse_log(text: &str, sample_rate: u32) -> Result<Vec<(f64, f64)>, String> {
    let mut audio_col = 0usize;
    let mut audio_scale = 1.0;
    let mut true_col = 1usize;
    let mut out = Vec::new();
    let mut header_seen = false;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|c| !c.is_empty())
            .collect();
        if !header_seen && cells.iter().any(|c| c.parse::<f64>().is_err()) {
            header_seen = true;
            let find = |names: &[&str]| {
                cells
                    .iter()
                    .position(|c| names.contains(&c.to_ascii_lowercase().as_str()))
            };
            if let Some(i) = find(&["audio_s", "audio_seconds"]) {
                audio_col = i;
            } else if let Some(i) = find(&["frames", "samples", "frame", "sample"]) {
                audio_col = i;
                audio_scale = 1.0 / sample_rate as f64;
            } else {
                return Err(format!(
                    "clock log header has no audio column (audio_s, frames or samples): {line}"
                ));
            }
            true_col = find(&["unix_s", "unix", "time_s", "ntp_s", "epoch"]).ok_or_else(|| {
                format!("clock log header has no time column (unix_s, unix or time_s): {line}")
            })?;
            continue;
        }
        header_seen = true;
        let get = |i: usize| -> Result<f64, String> {
            cells
                .get(i)
                .and_then(|c| c.parse::<f64>().ok())
                .ok_or_else(|| format!("clock log line {}: cannot read column {}", n + 1, i + 1))
        };
        out.push((get(audio_col)? * audio_scale, get(true_col)?));
    }
    Ok(out)
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
        assert!((fit.rate_error_s_per_day + 1.728).abs() < 0.05);
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
    fn parses_frames_and_unix() {
        let text = "# clock log\nunix_s,frames\n1759948294.5,0\n1759948354.5,2880000\n";
        let p = parse_log(text, 48000).unwrap();
        assert_eq!(p, vec![(0.0, 1759948294.5), (60.0, 1759948354.5)]);
        let bare = "0 100.0\n60 160.001\n";
        assert_eq!(parse_log(bare, 48000).unwrap()[1], (60.0, 160.001));
    }
}
