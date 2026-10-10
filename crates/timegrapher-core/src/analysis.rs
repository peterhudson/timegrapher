//! The whole pipeline: recording in, per-beat log and summary out.

use crate::amplitude::{self, AmplitudeConfig, AmplitudeWindow};
use crate::audio::Audio;
use crate::beats::{self, Beat};
use crate::dsp::{envelope, median, EnvelopeConfig};
use crate::periodicity::{self, Component, Series};
use crate::timing::{self, TimingFit, WindowFit};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// Beat rate; `None` guesses it from the recording.
    pub bph: Option<u32>,
    pub envelope: EnvelopeConfig,
    pub amplitude: AmplitudeConfig,
    /// Window for the rate-over-time series, seconds.
    pub rate_window_s: f64,
    /// Window for amplitude measurements, seconds.
    pub amplitude_window_s: f64,
    pub escape_teeth: u32,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        AnalysisConfig {
            bph: None,
            envelope: EnvelopeConfig::default(),
            amplitude: AmplitudeConfig::default(),
            rate_window_s: 10.0,
            amplitude_window_s: 2.0,
            escape_teeth: 15,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub duration_s: f64,
    pub sample_rate: u32,
    pub bph: u32,
    pub beats_found: usize,
    /// Fit over the whole recording; its beat error is the drop's.
    pub overall: Option<TimingFit>,
    /// Signed beat error measured from the unlock, as tg and commercial
    /// timegraphers measure it: median over the amplitude windows, ms.
    pub beat_error_unlock_ms: Option<f64>,
    /// Spread of the windowed rate (5th and 95th percentiles), s/d.
    pub rate_p05: Option<f64>,
    pub rate_p95: Option<f64>,
    /// Median per-beat timing jitter within the rate windows, microseconds.
    pub jitter_us: Option<f64>,
    /// Median amplitude over the recording, degrees.
    pub amplitude_deg: Option<f64>,
    pub amplitude_even_deg: Option<f64>,
    pub amplitude_odd_deg: Option<f64>,
    /// Standard errors of the three amplitudes above, degrees: their
    /// precision from the spread between windows, not their accuracy,
    /// which rests on the lift angle (see [`amplitude::standard_error`]).
    pub amplitude_se_deg: Option<f64>,
    pub amplitude_even_se_deg: Option<f64>,
    pub amplitude_odd_se_deg: Option<f64>,
    pub lift_deg: f64,
    /// Strongest periodic components of the timing and the amplitude.
    pub timing_periods: Vec<Component>,
    pub amplitude_periods: Vec<Component>,
    /// How much of the recording is clipped.
    pub clipping: Clipping,
}

/// Clipping flattens the loudest part of a tick, which moves the drop and
/// so the amplitude and the beat error; rate is unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Clipping {
    /// Clipped samples (see [`crate::diagnose::clipped_indices`]).
    pub samples: usize,
    /// Beats with a clipped sample within their template window.
    pub beats: usize,
    /// `beats` as a fraction of the beats found.
    pub beat_fraction: f64,
    /// Highest sample, dB relative to full scale.
    pub peak_dbfs: f64,
}

impl Clipping {
    /// Above this fraction of clipped beats the readings carry a warning.
    pub const WARN_FRACTION: f64 = 0.01;

    pub fn measure(x: &[f32], fs: f64, beats: &[Beat]) -> Clipping {
        let idx = crate::diagnose::clipped_indices(x, CLIP_LEVEL);
        let (pre, post) = (
            (beats::PRE_S * fs).round() as usize,
            (beats::POST_S * fs).round() as usize,
        );
        let hit = |b: &Beat| {
            let c = (b.time * fs).round().max(0.0) as usize;
            let k = idx.partition_point(|&i| i < c.saturating_sub(pre));
            idx.get(k).is_some_and(|&i| i <= c + post)
        };
        let n = beats.iter().filter(|b| hit(b)).count();
        let peak = x.iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
        Clipping {
            samples: idx.len(),
            beats: n,
            beat_fraction: if beats.is_empty() {
                0.0
            } else {
                n as f64 / beats.len() as f64
            },
            peak_dbfs: 20.0 * peak.max(1e-10).log10(),
        }
    }

    /// Enough clipped beats to doubt the amplitude and beat error.
    pub fn warns(&self) -> bool {
        self.beat_fraction > Self::WARN_FRACTION
    }
}

/// A sample at or above this magnitude counts as clipped, as in `doctor`.
pub(crate) const CLIP_LEVEL: f32 = 0.99;

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub summary: Summary,
    pub beats: Vec<Beat>,
    /// Each beat's offset from the whole-recording constant-rate fit, seconds.
    pub residuals: Vec<f64>,
    pub rate_windows: Vec<WindowFit>,
    pub amplitude_windows: Vec<AmplitudeWindow>,
}

fn percentile(v: &[f64], q: f64) -> Option<f64> {
    let mut w: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if w.is_empty() {
        return None;
    }
    w.sort_by(|a, b| a.total_cmp(b));
    Some(w[((w.len() - 1) as f64 * q).round() as usize])
}

fn median_of(v: impl Iterator<Item = f64>) -> Option<f64> {
    let mut w: Vec<f64> = v.filter(|x| x.is_finite()).collect();
    (!w.is_empty()).then(|| median(&mut w))
}

pub fn analyze(audio: &Audio, cfg: &AnalysisConfig) -> Analysis {
    let fs = audio.sample_rate as f64;
    let env = envelope(&audio.samples, fs, &cfg.envelope);
    let bph = cfg.bph.unwrap_or_else(|| beats::guess_bph(&env, fs));
    let (beats, _template) = beats::detect(&env, fs, bph);
    let overall = timing::fit(&beats, bph);
    let residuals = timing::residuals(&beats).unwrap_or_else(|| vec![f64::NAN; beats.len()]);
    let rate_windows = timing::windows(&beats, bph, cfg.rate_window_s, cfg.rate_window_s / 2.0);
    let rates: Vec<f64> = rate_windows.iter().map(|w| w.fit.rate_s_per_day).collect();
    let osc = 2.0 * overall.map(|f| f.period_s).unwrap_or(3600.0 / bph as f64);
    let amplitude_windows = amplitude::windows(
        &env,
        fs,
        &beats,
        osc,
        cfg.amplitude_window_s,
        &cfg.amplitude,
    );

    // Periodicity: timing residuals and amplitude, in 1 s bins.
    let wheels = periodicity::standard_wheels(bph, cfg.escape_teeth);
    let duration = audio.duration();
    let good: Vec<(f64, f64)> = beats
        .iter()
        .zip(&residuals)
        .filter(|(b, r)| b.quality > 0.4 && r.is_finite())
        .map(|(b, &r)| (b.time, r))
        .collect();
    let search = |s: Series| -> Vec<Component> {
        if s.t.len() < 30 {
            return Vec::new();
        }
        let s = periodicity::detrend(&s);
        let span = s.t[s.t.len() - 1] - s.t[0];
        let periods = periodicity::log_periods(4.0, (span / 3.0).max(8.0), 800);
        let power = periodicity::lomb_scargle(&s, &periods);
        periodicity::peaks(&periods, &power, &wheels, 5)
    };
    let timing_series = periodicity::bin_median(
        &good.iter().map(|g| g.0).collect::<Vec<_>>(),
        &good.iter().map(|g| g.1).collect::<Vec<_>>(),
        1.0,
    );
    let amp_series = Series {
        t: amplitude_windows
            .iter()
            .filter(|w| w.mean().is_some())
            .map(|w| (w.start_s + w.end_s) / 2.0)
            .collect(),
        y: amplitude_windows.iter().filter_map(|w| w.mean()).collect(),
    };

    let se = |f: &dyn Fn(&AmplitudeWindow) -> Option<f64>| {
        let v: Vec<f64> = amplitude_windows.iter().filter_map(f).collect();
        amplitude::standard_error(&v)
    };
    let summary = Summary {
        duration_s: duration,
        sample_rate: audio.sample_rate,
        bph,
        beats_found: beats.len(),
        overall,
        beat_error_unlock_ms: median_of(
            amplitude_windows
                .iter()
                .filter_map(|w| w.beat_error_unlock_ms),
        ),
        rate_p05: percentile(&rates, 0.05),
        rate_p95: percentile(&rates, 0.95),
        jitter_us: median_of(rate_windows.iter().map(|w| w.fit.jitter_us)),
        amplitude_deg: median_of(amplitude_windows.iter().filter_map(|w| w.mean())),
        amplitude_even_deg: median_of(amplitude_windows.iter().filter_map(|w| w.even_deg)),
        amplitude_odd_deg: median_of(amplitude_windows.iter().filter_map(|w| w.odd_deg)),
        amplitude_se_deg: se(&|w| w.mean()),
        amplitude_even_se_deg: se(&|w| w.even_deg),
        amplitude_odd_se_deg: se(&|w| w.odd_deg),
        lift_deg: cfg.amplitude.lift_deg,
        timing_periods: search(timing_series),
        amplitude_periods: search(amp_series),
        clipping: Clipping::measure(&audio.samples, fs, &beats),
    };
    Analysis {
        summary,
        beats,
        residuals,
        rate_windows,
        amplitude_windows,
    }
}
