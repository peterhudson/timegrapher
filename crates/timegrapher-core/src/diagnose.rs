//! Signal checks for setting up a microphone: is the level right, does it
//! clip, is automatic gain fighting us, and can we hear the ticks?
//!
//! Works on a few seconds of audio from a live device or a file. It only
//! measures and names the problems; what to change on the computer is the
//! caller's business (see the CLI's `doctor` command).

use crate::audio::Audio;
use crate::beats;
use crate::dsp::{envelope, median, EnvelopeConfig};
use crate::timing;
use serde::Serialize;

/// Tick peaks aimed for, dBFS: loud enough to sit well above the
/// sound card's own noise, with headroom for a louder watch.
pub const TARGET_PEAK_DBFS: f64 = -10.0;

#[derive(Debug, Clone)]
pub struct DiagnoseConfig {
    pub envelope: EnvelopeConfig,
    /// Beat rate; `None` guesses it.
    pub bph: Option<u32>,
    /// A sample at or above this magnitude counts as clipped.
    pub clip_level: f32,
}

impl Default for DiagnoseConfig {
    fn default() -> Self {
        DiagnoseConfig {
            envelope: EnvelopeConfig::default(),
            bph: None,
            clip_level: 0.99,
        }
    }
}

/// How bad an issue is: a `problem` spoils measurements; `advice` is
/// worth fixing but readings are still usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Problem,
    Advice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    /// Nothing at all: muted, unplugged or the wrong device.
    Silent,
    /// Ticks are there but far below full scale.
    TooQuiet,
    /// Samples hit full scale; tick shapes and amplitude are distorted.
    Clipping,
    /// Peaks within 1 dB of full scale; one louder watch away from clipping.
    Hot,
    /// The background rises between ticks: the input's automatic gain
    /// turns itself down on each tick and back up in the gaps.
    AgcSuspected,
    /// No regular beat found.
    NoTicks,
    /// Ticks found but the background is loud next to them.
    Noisy,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub code: IssueCode,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignalCheck {
    pub sample_rate: u32,
    pub duration_s: f64,
    /// Largest sample magnitude, dBFS.
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
    /// Mean of the samples relative to full scale.
    pub dc_offset: f64,
    /// Samples at the clip level, plus samples in flat tops (four or more
    /// equal samples at the recording's peak), which is how clipping in
    /// the analogue stage shows up below digital full scale.
    pub clipped_samples: usize,
    /// Beat rate used, bph (guessed unless given).
    pub bph: u32,
    pub beats_expected: usize,
    /// Beats found with a typical tick shape.
    pub beats_found: usize,
    /// Median tick peak in the high-passed signal, dBFS.
    pub tick_level_dbfs: Option<f64>,
    /// Median background level between ticks in the high-passed signal, dBFS.
    pub noise_level_dbfs: f64,
    /// Tick peak over background, dB.
    pub tick_to_noise_db: Option<f64>,
    /// Background late in the gap between ticks over early in it, dB.
    /// Near 0 for a fixed gain; several dB when automatic gain pumps.
    pub gap_rise_db: Option<f64>,
    pub rate_s_per_day: Option<f64>,
    pub beat_error_ms: Option<f64>,
    /// Gain change that would bring the ticks to the target level, dB.
    /// Negative means turn the input down.
    pub suggested_gain_change_db: Option<f64>,
    pub issues: Vec<Issue>,
}

impl SignalCheck {
    /// True when nothing would spoil a measurement.
    pub fn ok(&self) -> bool {
        self.issues.iter().all(|i| i.severity != Severity::Problem)
    }
    pub fn has(&self, code: IssueCode) -> bool {
        self.issues.iter().any(|i| i.code == code)
    }
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-10).log10()
}

fn mean(v: &[f32]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64
}

fn count_clipped(x: &[f32], clip_level: f32) -> usize {
    let peak = x.iter().fold(0f32, |m, v| m.max(v.abs()));
    let mut n = x.iter().filter(|v| v.abs() >= clip_level).count();
    if peak > 0.25 && peak < clip_level {
        // Flat tops: runs of equal samples at the peak.
        let near = |v: f32| (v.abs() - peak).abs() <= peak * 1e-3;
        let mut run = 0;
        for w in x.windows(2) {
            if near(w[0]) && near(w[1]) && (w[0] - w[1]).abs() <= peak * 1e-3 {
                run += 1;
            } else {
                if run >= 3 {
                    n += run + 1;
                }
                run = 0;
            }
        }
        if run >= 3 {
            n += run + 1;
        }
    }
    n
}

/// Measure a short recording and name what is wrong with it.
pub fn check(audio: &Audio, cfg: &DiagnoseConfig) -> SignalCheck {
    let fs = audio.sample_rate as f64;
    let x = &audio.samples;
    let duration_s = audio.duration();
    let peak = x.iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
    let dc = mean(x);
    let rms =
        (x.iter().map(|&v| (v as f64 - dc).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt();
    let clipped = count_clipped(x, cfg.clip_level);

    let env = envelope(x, fs, &cfg.envelope);
    let bph = cfg.bph.unwrap_or_else(|| beats::guess_bph(&env, fs));
    let beat_s = 3600.0 / bph as f64;
    let expected = (duration_s / beat_s).floor() as usize;
    let (found, _) = if peak > 0.0 {
        beats::detect(&env, fs, bph)
    } else {
        (Vec::new(), Vec::new())
    };
    let good: Vec<_> = found.iter().filter(|b| b.quality >= 0.5).copied().collect();
    let fit = timing::fit(&good, bph);

    // Levels around each tick, from the envelope (high-passed, rectified):
    // tick peak just around the beat, background in the gap after it.
    let at = |t: f64| ((t * fs).round().max(0.0) as usize).min(env.len());
    let level = |a: f64, b: f64| -> Option<f64> {
        let (i, j) = (at(a), at(b));
        (j > i).then(|| mean(&env[i..j]))
    };
    let mut ticks = Vec::new();
    let mut early = Vec::new();
    let mut late = Vec::new();
    for b in &good {
        let (i, j) = (at(b.time - 0.015), at(b.time + 0.005));
        if j > i {
            ticks.push(env[i..j].iter().fold(0f32, |m, &v| m.max(v)) as f64);
        }
        if let (Some(e), Some(l)) = (
            level(b.time + 0.30 * beat_s, b.time + 0.45 * beat_s),
            level(b.time + 0.75 * beat_s, b.time + 0.90 * beat_s),
        ) {
            early.push(e);
            late.push(l);
        }
    }
    // The envelope is the mean of the rectified signal, which is about
    // 0.8 of its RMS for noise; scale back so levels read as RMS.
    let rect_to_rms = (std::f64::consts::PI / 2.0).sqrt();
    let noise = if late.is_empty() {
        let mut e: Vec<f64> = env.iter().map(|&v| v as f64).collect();
        if e.is_empty() {
            0.0
        } else {
            median(&mut e)
        }
    } else {
        let mut both: Vec<f64> = early.iter().chain(&late).copied().collect();
        median(&mut both)
    } * rect_to_rms;
    let tick = (!ticks.is_empty()).then(|| median(&mut ticks));
    let gap_rise = (early.len() >= 8).then(|| {
        let mut r: Vec<f64> = early
            .iter()
            .zip(&late)
            .filter(|(e, _)| **e > 0.0)
            .map(|(e, l)| db(l / e))
            .collect();
        if r.is_empty() {
            0.0
        } else {
            median(&mut r)
        }
    });
    let tick_to_noise = tick.map(|t| db(t) - db(noise));

    let mut issues = Vec::new();
    let mut add = |code, severity, message: String| {
        issues.push(Issue {
            code,
            severity,
            message,
        })
    };
    let silent = peak < 1e-4;
    if silent {
        add(
            IssueCode::Silent,
            Severity::Problem,
            "No signal at all: the input is muted, unplugged, or not the timegrapher microphone."
                .into(),
        );
    }
    let clipping = clipped >= 3;
    if clipping {
        add(
            IssueCode::Clipping,
            Severity::Problem,
            format!(
                "{clipped} samples clipped: the input level is too high (or automatic gain is on), \
                 which flattens the ticks and spoils amplitude and tick shape."
            ),
        );
    } else if !silent && db(peak) > -1.0 {
        add(
            IssueCode::Hot,
            Severity::Advice,
            format!(
                "Peaks reach {:.1} dBFS: no clipping yet, but a louder watch would clip. \
                 Turn the input down a little.",
                db(peak)
            ),
        );
    }
    let agc = gap_rise.is_some_and(|r| r > 3.0);
    if agc {
        add(
            IssueCode::AgcSuspected,
            Severity::Problem,
            format!(
                "The background rises by {:.1} dB between ticks: the input's automatic gain \
                 control is probably on. Turn it off so every tick is measured at the same gain.",
                gap_rise.unwrap_or(0.0)
            ),
        );
    }
    let few_beats = good.len() < expected / 2 || fit.is_none();
    if !silent && (few_beats || tick_to_noise.map_or(true, |s| s < 6.0)) {
        add(
            IssueCode::NoTicks,
            Severity::Problem,
            format!(
                "No steady beat found ({} of about {expected} beats): check the watch is \
                 running and clamped firmly against the microphone, and that this is the right input.",
                good.len()
            ),
        );
    } else if let Some(s) = tick_to_noise.filter(|&s| s < 20.0 && !clipping) {
        add(
            IssueCode::Noisy,
            Severity::Advice,
            format!(
                "Ticks are only {s:.0} dB above the background: clamp the watch firmly, \
                 move away from fans and mains hum, or raise the input level if it is low."
            ),
        );
    }
    let too_quiet = !silent && !clipping && db(peak) < -30.0;
    if too_quiet {
        add(
            IssueCode::TooQuiet,
            Severity::Advice,
            format!(
                "Peaks only reach {:.0} dBFS: raise the input level so ticks sit near {TARGET_PEAK_DBFS:.0} dBFS.",
                db(peak)
            ),
        );
    }
    let suggested = if silent {
        None
    } else if clipping {
        // The true peak is unknown once clipped; step down and re-check.
        Some(-6.0)
    } else if too_quiet || db(peak) > -1.0 {
        Some(((TARGET_PEAK_DBFS - db(peak)) * 2.0).round() / 2.0)
    } else {
        None
    };

    SignalCheck {
        sample_rate: audio.sample_rate,
        duration_s,
        peak_dbfs: db(peak),
        rms_dbfs: db(rms),
        dc_offset: dc,
        clipped_samples: clipped,
        bph,
        beats_expected: expected,
        beats_found: good.len(),
        tick_level_dbfs: tick.map(db),
        noise_level_dbfs: db(noise),
        tick_to_noise_db: tick_to_noise,
        gap_rise_db: gap_rise,
        rate_s_per_day: fit.map(|f| f.rate_s_per_day),
        beat_error_ms: fit.map(|f| f.beat_error_ms.abs()),
        suggested_gain_change_db: suggested,
        issues,
    }
}
