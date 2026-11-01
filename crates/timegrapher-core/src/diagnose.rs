//! Signal checks for setting up a microphone: the level, clipping, whether
//! the background rises between ticks, and whether a regular beat is
//! found.
//!
//! Works on a few seconds of audio from a live device or a file. It only
//! measures and names the conditions detected; the mixer changes for
//! `--apply` are the caller's business (see the CLI's `doctor` command).

use crate::audio::Audio;
use crate::beats;
use crate::dsp::{envelope, median, EnvelopeConfig};
use crate::timing;
use serde::Serialize;

/// The peak level, dBFS, that `doctor --apply` sets the input to: about
/// −10 dBFS.
pub const TARGET_PEAK_DBFS: f64 = -10.0;

/// Peaks above this, dBFS, are the `hot` condition.
pub const HOT_PEAK_DBFS: f64 = -6.0;

/// Peaks below this, dBFS, are the `too_quiet` condition.
pub const QUIET_PEAK_DBFS: f64 = -30.0;

/// A peak under this, linear (−80 dBFS), is no signal at all.
const SILENT_PEAK: f64 = 1e-4;

/// Background rising more than this between ticks, dB, is the
/// `agc_suspected` condition.
pub const GAP_RISE_DB: f64 = 3.0;

/// Below this `signal_x` the `no_ticks` condition is named: pure noise
/// reads about 2× (see `docs/validation.md`).
pub const NO_TICKS_SIGNAL_X: f64 = 3.0;

/// Whether a signal figure shows the ticks too faint over the noise: under
/// [`NO_TICKS_SIGNAL_X`]. With no figure (the envelope's median zero, as
/// over digital silence) the ticks are not shown faint, and the beat tests
/// decide (#784).
fn faint(signal_x: Option<f64>) -> bool {
    signal_x.is_some_and(|x| x < NO_TICKS_SIGNAL_X)
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    /// Peak under −80 dBFS: no signal.
    Silent,
    /// Peak under [`QUIET_PEAK_DBFS`].
    TooQuiet,
    /// Three or more samples at full scale or in flat tops.
    Clipping,
    /// Peak above [`HOT_PEAK_DBFS`], not clipped.
    Hot,
    /// The background rises more than [`GAP_RISE_DB`] between ticks, as
    /// it does when the input's automatic gain turns down on each tick
    /// and back up in the gaps.
    AgcSuspected,
    /// No regular beat found: fewer than half the beats expected, no fit,
    /// or `signal_x` under [`NO_TICKS_SIGNAL_X`].
    NoTicks,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    /// Stable identifier for programs and agents; the title is for people.
    pub code: IssueCode,
    /// The condition in words, with its figure.
    pub title: String,
    /// The measurements it was detected from.
    pub evidence: String,
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
    /// Median beat peak over the median of the envelope, as the desktop
    /// app's "signal Nx" ([`crate::live::signal_figure`]); None when no
    /// beat was found, or where the envelope's median is zero, as over
    /// digital silence. Pure noise reads about 2×.
    pub signal_x: Option<f64>,
    /// Background late in the gap between ticks over early in it, dB.
    /// Near 0 for a fixed gain; several dB when automatic gain pumps.
    pub gap_rise_db: Option<f64>,
    pub rate_s_per_day: Option<f64>,
    pub beat_error_ms: Option<f64>,
    /// The gain change `doctor --apply` makes, dB: it sets the peak to
    /// about −10 dBFS ([`TARGET_PEAK_DBFS`]), or steps down 6 dB when the
    /// input clips. Negative means down. `None` when the peak is between
    /// [`QUIET_PEAK_DBFS`] and [`HOT_PEAK_DBFS`].
    pub suggested_gain_change_db: Option<f64>,
    /// The conditions detected; empty when none of them is.
    pub issues: Vec<Issue>,
}

impl SignalCheck {
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
    clipped_indices(x, clip_level).len()
}

/// The samples that are clipped: at or above `clip_level`, or in a flat top
/// (four or more equal samples at the recording's peak, which is how
/// clipping before the converter shows once the level is turned down
/// after it). In order.
pub fn clipped_indices(x: &[f32], clip_level: f32) -> Vec<usize> {
    let peak = x.iter().fold(0f32, |m, v| m.max(v.abs()));
    if peak >= clip_level {
        return (0..x.len()).filter(|&i| x[i].abs() >= clip_level).collect();
    }
    let mut out = Vec::new();
    if peak > 0.25 {
        let near = |v: f32| (v.abs() - peak).abs() <= peak * 1e-3;
        let mut run = 0;
        for i in 1..x.len() {
            let (a, b) = (x[i - 1], x[i]);
            if near(a) && near(b) && (a - b).abs() <= peak * 1e-3 {
                run += 1;
            } else {
                if run >= 3 {
                    out.extend(i - run - 1..i);
                }
                run = 0;
            }
        }
        if run >= 3 {
            out.extend(x.len() - run - 1..x.len());
        }
    }
    out
}

/// Measure a short recording and name the conditions detected in it.
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
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
    let good: Vec<_> = found.iter().filter(|b| b.is_good()).copied().collect();
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
    // The readings' own signal figure (`live::signal_figure`), none with
    // no beat found or where the envelope's median is zero, as over
    // digital silence, which has no figure (#784).
    let signal_x = (!found.is_empty())
        .then(|| crate::live::signal_figure(&env, fs, &found))
        .flatten()
        .map(f64::from);

    let mut issues = Vec::new();
    let mut add = |code, title: String, evidence: String| {
        issues.push(Issue {
            code,
            title,
            evidence,
        })
    };
    let silent = peak < SILENT_PEAK;
    if silent {
        add(
            IssueCode::Silent,
            format!("No signal: peak {:.0} dBFS", db(peak)),
            format!(
                "under {:.0} dBFS, the level taken as no signal at all",
                db(SILENT_PEAK)
            ),
        );
    }
    let clipping = clipped >= 3;
    if clipping {
        add(
            IssueCode::Clipping,
            format!("Clipping: {clipped} samples"),
            format!("{clipped} samples clipped, peak {:.1} dBFS", db(peak)),
        );
    } else if !silent && db(peak) > HOT_PEAK_DBFS {
        add(
            IssueCode::Hot,
            format!("Peak {:.1} dBFS", db(peak)),
            format!(
                "peak {:.1} dBFS, {:.1} dB below full scale",
                db(peak),
                -db(peak)
            ),
        );
    }
    if let Some(r) = gap_rise.filter(|&r| r > GAP_RISE_DB) {
        add(
            IssueCode::AgcSuspected,
            format!("Background rises {r:.1} dB between ticks"),
            format!("background late in the gap {r:.1} dB above early in it"),
        );
    }
    let few_beats = good.len() < expected / 2 || fit.is_none();
    let faint = faint(signal_x);
    let signal = signal_x.map_or("no signal figure".into(), |x| format!("signal {x:.1}×"));
    if !silent && (few_beats || faint) {
        // The title names the test that fired: no timing fit, under half
        // the beats expected, or ticks too faint over the noise however
        // well they time (#772).
        let title = if fit.is_none() {
            beats::no_regular_beat_text(None, None)
        } else if few_beats {
            format!(
                "No regular beat found (only {} of about {expected} beats)",
                good.len()
            )
        } else {
            format!("Ticks too faint over the noise ({signal}, under {NO_TICKS_SIGNAL_X:.0}×)")
        };
        add(
            IssueCode::NoTicks,
            title,
            format!("{} of about {expected} beats found, {signal}", good.len()),
        );
    }
    let too_quiet = !silent && !clipping && db(peak) < QUIET_PEAK_DBFS;
    if too_quiet {
        add(
            IssueCode::TooQuiet,
            format!("Peak {:.0} dBFS", db(peak)),
            format!("under {QUIET_PEAK_DBFS:.0} dBFS, too quiet to time well"),
        );
    }
    let suggested = if silent {
        None
    } else if clipping {
        // The true peak is unknown once clipped; step down and re-check.
        Some(-6.0)
    } else if too_quiet || db(peak) > HOT_PEAK_DBFS {
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
        signal_x,
        gap_rise_db: gap_rise,
        rate_s_per_day: fit.map(|f| f.rate_s_per_day),
        beat_error_ms: fit.map(|f| f.beat_error_ms.abs()),
        suggested_gain_change_db: suggested,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A figure under 3× is faint and one at 3× or more is not; no figure
    /// at all is not shown faint, so the beat tests decide (#784).
    #[test]
    fn only_a_figure_under_the_threshold_is_faint() {
        assert!(faint(Some(2.9)));
        assert!(!faint(Some(NO_TICKS_SIGNAL_X)));
        assert!(!faint(Some(40.0)));
        assert!(!faint(None));
    }
}
