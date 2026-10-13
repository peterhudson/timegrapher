//! The whole pipeline: recording in, per-beat log and summary out.

use crate::amplitude::{self, AmplitudeConfig, AmplitudeWindow};
use crate::audio::Audio;
use crate::beats::{self, Beat};
use crate::confidence::{Confidence, Doubts};
use crate::dsp::{envelope, median, EnvelopeConfig};
use crate::periodicity::{self, Component, Series};
use crate::timing::{self, TimingFit, WindowFit};
use crate::usbclock::{self, UsbClock};
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
    /// The lift angle comes from the calibre or the user, not a default.
    pub lift_known: bool,
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
            lift_known: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub duration_s: f64,
    pub sample_rate: u32,
    pub bph: u32,
    /// The beat rate the recording sounds like, when a set `bph` differs
    /// from it: every reading is then measured against the set rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bph_heard: Option<u32>,
    pub beats_found: usize,
    /// `beats_found` over the beats the recording's length holds at `bph`.
    pub beats_found_fraction: f64,
    /// False when a regular beat was looked for and not found, as on an
    /// empty stand: the readings below are then left out, since they would
    /// describe noise, and `jitter_us` gives the timing scatter. Regularity
    /// is assessed only with [`crate::measure::SHORT_SPAN_S`] of sound or
    /// more; a shorter recording keeps its readings and this stays true
    /// (see [`crate::measure::watch_heard`]).
    pub watch_heard: bool,
    /// Fit over the whole recording; its beat error is the drop's.
    pub overall: Option<TimingFit>,
    /// Signed beat error measured from the unlock, the figure checked
    /// against tg's beat error in regress: median over the amplitude
    /// windows, ms. Positive means Tick (the run's first beat) comes late.
    pub beat_error_unlock_ms: Option<f64>,
    /// Spread of the windowed rate (5th and 95th percentiles), s/d.
    pub rate_p05: Option<f64>,
    pub rate_p95: Option<f64>,
    /// Median per-beat timing jitter (the timing scatter) within the rate
    /// windows, microseconds.
    pub jitter_us: Option<f64>,
    /// `jitter_us` as a share of the beat period at `bph`, 0 to 1 (see
    /// [`beats::timing_scatter_fraction`]); what `watch_heard` is decided on.
    pub timing_scatter_fraction: Option<f64>,
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
    /// The sound card's clock error from the USB lines in the recording,
    /// when it has them. Rates above stay on the card's clock, as tg reads
    /// them; [`Summary::rate_corrected_s_per_day`] is the corrected rate.
    pub usb_clock: Option<UsbClock>,
    /// The whole-recording rate on the card's clock, as tg reads it: the
    /// same figure as `overall`'s, given here beside the corrected one, s/d.
    pub rate_s_per_day: Option<f64>,
    /// Rate corrected onto true time with `usb_clock`, s/d.
    pub rate_corrected_s_per_day: Option<f64>,
    /// Conditions detected, as flags; each set one is also a caveat.
    pub doubts: Doubts,
    /// Each reading's ± (standard error) for rate, amplitude and beat
    /// error; `None` when no regular beat is found.
    pub confidence: Option<Confidence>,
    /// Conditions detected that the readings rest on, each with its
    /// figure (see [`crate::measure`]); empty when none is.
    pub caveats: Vec<crate::measure::Caveat>,
    /// The shape of the Tick and Tock and the beat error early and late
    /// (see [`crate::shape::Measures`]); `None` when no readings are given.
    pub shape: Option<crate::shape::Measures>,
}

impl Summary {
    /// Clear every reading, as when no regular beat is found: they would
    /// describe noise. What says so (`watch_heard`, `jitter_us`,
    /// `timing_scatter_fraction`, `beats_found`) and what describes the recording and the sound card
    /// stay.
    pub fn withhold_readings(&mut self) {
        self.overall = None;
        self.beat_error_unlock_ms = None;
        self.rate_p05 = None;
        self.rate_p95 = None;
        self.amplitude_deg = None;
        self.amplitude_even_deg = None;
        self.amplitude_odd_deg = None;
        self.amplitude_se_deg = None;
        self.amplitude_even_se_deg = None;
        self.amplitude_odd_se_deg = None;
        self.timing_periods.clear();
        self.amplitude_periods.clear();
        self.rate_s_per_day = None;
        self.rate_corrected_s_per_day = None;
        self.confidence = None;
        self.shape = None;
    }

    /// The sound card's clock is known from elsewhere (a stored or given
    /// correction): the `clock_unknown` condition no longer holds.
    pub fn clock_known(&mut self) {
        self.doubts.clock_unknown = false;
        self.caveats
            .retain(|c| *c != crate::measure::Caveat::ClockUnknown);
    }
}

/// How much of the recording is clipped: the samples, and the beats with a
/// clipped sample in their sound.
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
    /// Above this share of clipped beats the `clipped` condition is named
    /// among the caveats.
    pub const NOTED_FRACTION: f64 = 0.01;

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

    /// More than [`Self::NOTED_FRACTION`] of the beats clipped.
    pub fn noted(&self) -> bool {
        self.beat_fraction > Self::NOTED_FRACTION
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
    let guessed = beats::guess_bph(&env, fs);
    let bph = cfg.bph.unwrap_or(guessed);
    // The beats and the amplitude windows come from the chunked stream,
    // the one beat pipeline every reading of a recording rests on (session,
    // long, series and regress read files through it).
    let log = crate::stream::analyze_samples(
        &audio.samples,
        audio.sample_rate,
        &crate::stream::StreamConfig {
            analysis: AnalysisConfig {
                bph: Some(bph),
                ..cfg.clone()
            },
            ..Default::default()
        },
    );
    let beats = log.beats;
    let amplitude_windows = log.amplitude_windows;
    // Where listening stopped and started again the beats jump, and no
    // count of beats across the hole is known: rate and timing are fitted
    // on each stretch between jumps, and no window spans one.
    let cuts: Vec<f64> = log.jumps.iter().map(|&t| t - 1e-6).collect();
    let overall = timing::fit_pieces(&beats, bph, &cuts);
    let mut residuals = Vec::with_capacity(beats.len());
    let mut from = 0;
    for end in cuts
        .iter()
        .map(|&c| beats.partition_point(|b| b.time < c))
        .chain([beats.len()])
    {
        let piece = &beats[from..end.max(from)];
        residuals.extend(timing::residuals(piece).unwrap_or_else(|| vec![f64::NAN; piece.len()]));
        from = end.max(from);
    }
    let mut rate_windows = timing::windows(&beats, bph, cfg.rate_window_s, cfg.rate_window_s / 2.0);
    rate_windows.retain(|w| !cuts.iter().any(|&c| w.start_s < c && c < w.end_s));
    let rates: Vec<f64> = rate_windows.iter().map(|w| w.fit.rate_s_per_day).collect();

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

    let jitter_us = median_of(rate_windows.iter().map(|w| w.fit.jitter_us));
    let se = |f: &dyn Fn(&AmplitudeWindow) -> Option<f64>| {
        let v: Vec<f64> = amplitude_windows.iter().filter_map(f).collect();
        amplitude::standard_error(&v)
    };
    let watch_heard = crate::measure::watch_heard(jitter_us, bph, duration);
    let clipping = Clipping::measure(&audio.samples, fs, &beats);
    let usb_clock = usbclock::measure(&audio.samples, audio.sample_rate);
    let amplitude_se_deg = se(&|w| w.mean());
    let expected = duration * bph as f64 / 3600.0;
    let assessment = crate::measure::assess(
        &rate_windows,
        &amplitude_windows.iter().collect::<Vec<_>>(),
        &crate::measure::Context {
            watch_heard,
            bph_heard: (bph != guessed).then_some(guessed),
            lift_known: cfg.lift_known,
            clock_known: usb_clock.is_some(),
            beats: beats.len(),
            expected,
            clipped_fraction: clipping.beat_fraction,
            span_s: match (beats.first(), beats.last()) {
                (Some(a), Some(b)) => b.time - a.time,
                _ => 0.0,
            },
            listened_s: duration,
            breaks: cuts.len(),
            average_s: None,
        },
    );
    let doubts = assessment.doubts;
    let confidence = assessment.confidence;
    let shape = watch_heard.then(|| {
        let beat_s = overall.map_or(3600.0 / bph as f64, |f| f.period_s);
        let r = crate::shape::analyze(&env, fs, &beats, beat_s, &Default::default());
        crate::shape::Measures::of(&r, &amplitude_windows)
    });
    let mut summary = Summary {
        duration_s: duration,
        sample_rate: audio.sample_rate,
        bph,
        bph_heard: (bph != guessed).then_some(guessed),
        beats_found: beats.len(),
        beats_found_fraction: if expected > 0.0 {
            beats.len() as f64 / expected
        } else {
            0.0
        },
        watch_heard,
        overall,
        beat_error_unlock_ms: median_of(
            amplitude_windows
                .iter()
                .filter_map(|w| w.beat_error_unlock_ms),
        ),
        rate_p05: percentile(&rates, 0.05),
        rate_p95: percentile(&rates, 0.95),
        jitter_us,
        timing_scatter_fraction: beats::timing_scatter_fraction(jitter_us, bph),
        amplitude_deg: median_of(amplitude_windows.iter().filter_map(|w| w.mean())),
        amplitude_even_deg: median_of(amplitude_windows.iter().filter_map(|w| w.even_deg)),
        amplitude_odd_deg: median_of(amplitude_windows.iter().filter_map(|w| w.odd_deg)),
        amplitude_se_deg,
        amplitude_even_se_deg: se(&|w| w.even_deg),
        amplitude_odd_se_deg: se(&|w| w.odd_deg),
        lift_deg: cfg.amplitude.lift_deg,
        timing_periods: search(timing_series),
        amplitude_periods: search(amp_series),
        clipping,
        usb_clock,
        rate_s_per_day: overall.map(|f| f.rate_s_per_day),
        rate_corrected_s_per_day: usb_clock.and_then(|c| {
            overall.map(|f| crate::clockstore::correct_rate(f.rate_s_per_day, c.ppm))
        }),
        doubts,
        confidence,
        caveats: assessment.caveats,
        shape,
    };
    if !summary.watch_heard {
        summary.withhold_readings();
    }
    Analysis {
        summary,
        beats,
        residuals,
        rate_windows,
        amplitude_windows,
    }
}
