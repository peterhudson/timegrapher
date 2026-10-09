//! The sound of a tick and of a tock, for drawing and debugging.
//!
//! A profile is one side's median envelope over a stretch of beats, the
//! spread around it, and the marks the engine measured on it: the unlock and
//! drop edges that amplitude and beat error use, and the three sounds the
//! shape module finds. It is built from the same template as
//! [`crate::amplitude`], so the marks are exactly the ones behind the
//! numbers on screen.

use crate::amplitude::{amplitude_deg, edges, AmplitudeConfig};
use crate::beats::{median_template, Beat, POST_S, PRE_S};
use crate::dsp::envelope;
use crate::shape::{self, ShapeConfig};
use serde::Serialize;

/// Which beats a profile is built from. `A` is the even beats and `B` the
/// odd ones, as on the paper strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    A,
    B,
}

impl Side {
    fn even(self) -> bool {
        self == Side::A
    }
}

/// One side's median sound with the engine's marks. Times are ms from the
/// beat's reference point (near the drop); levels are envelope values.
#[derive(Debug, Clone, Serialize)]
pub struct TickProfile {
    pub side: Side,
    /// Beats that went into the profile.
    pub beats: usize,
    /// Time of the first point and the spacing between points, ms.
    pub t0_ms: f64,
    pub step_ms: f64,
    /// Median envelope and its 10th and 90th percentiles, point by point.
    pub median: Vec<f32>,
    pub p10: Vec<f32>,
    pub p90: Vec<f32>,
    /// Noise floor of the median (the first 3 ms).
    pub floor: f32,
    /// Unlock and drop edges and the drop's peak, as amplitude reads them.
    pub unlock_ms: Option<f64>,
    pub drop_ms: Option<f64>,
    pub peak_ms: Option<f64>,
    /// Amplitude from those edges, degrees, when it is plausible.
    pub amplitude_deg: Option<f64>,
    /// Where sounds 1, 2 and 3 cross halfway up their own rise, from the
    /// shape module; sound 2 or 1 is missing when it can't be told apart.
    pub sound1_ms: Option<f64>,
    pub sound2_ms: Option<f64>,
    pub sound3_ms: Option<f64>,
}

/// Points of the profile are this far apart, ms (the envelope is already
/// smoothed over 0.2 ms, so nothing is lost).
const STEP_MS: f64 = 0.1;

/// Profiles of both sides from beats in `[from_s, to_s)` of an envelope.
/// Beat times and bounds are in the envelope's time base.
pub fn profiles(
    env: &[f32],
    fs: f64,
    beats: &[Beat],
    osc_period_s: f64,
    from_s: f64,
    to_s: f64,
    cfg: &AmplitudeConfig,
) -> [Option<TickProfile>; 2] {
    [Side::A, Side::B].map(|side| profile(env, fs, beats, side, osc_period_s, from_s, to_s, cfg))
}

/// Profiles of both sides from raw audio and beats found in it. `beats`
/// times are seconds from the start of `x`.
pub fn profiles_from_audio(
    x: &[f32],
    fs: f64,
    beats: &[Beat],
    osc_period_s: f64,
    cfg: &crate::analysis::AnalysisConfig,
) -> [Option<TickProfile>; 2] {
    let env = envelope(x, fs, &cfg.envelope);
    let to = x.len() as f64 / fs;
    profiles(&env, fs, beats, osc_period_s, 0.0, to, &cfg.amplitude)
}

#[allow(clippy::too_many_arguments)]
pub fn profile(
    env: &[f32],
    fs: f64,
    beats: &[Beat],
    side: Side,
    osc_period_s: f64,
    from_s: f64,
    to_s: f64,
    cfg: &AmplitudeConfig,
) -> Option<TickProfile> {
    let times: Vec<f64> = beats
        .iter()
        .filter(|b| {
            b.time >= from_s
                && b.time < to_s
                && b.quality > 0.4
                && (b.index.rem_euclid(2) == 0) == side.even()
        })
        .map(|b| b.time)
        .collect();
    let windows = windows(env, fs, &times);
    if windows.len() < 3 {
        return None;
    }
    let tmpl = median_template(env, fs, &times);
    let origin = (PRE_S * fs).round() as usize;
    let to_ms = |s: f64| (s - origin as f64) / fs * 1000.0;
    let e = edges(&tmpl, fs, origin, cfg.onset_fraction);
    let amp = e
        .map(|e| amplitude_deg((e.drop - e.unlock) / fs, osc_period_s, cfg.lift_deg))
        .filter(|a| (100.0..=380.0).contains(a));
    let sh = shape::measure(&tmpl, fs, origin, &ShapeConfig::default());
    let from_unlock = |t: Option<f64>| -> Option<f64> {
        let s = sh.as_ref()?;
        Some(s.unlock_at_ms + t?)
    };

    let stride = ((STEP_MS / 1000.0 * fs).round() as usize).max(1);
    let (mut median, mut p10, mut p90) = (Vec::new(), Vec::new(), Vec::new());
    let mut col = vec![0.0f32; windows.len()];
    for k in (0..tmpl.len()).step_by(stride) {
        for (c, w) in col.iter_mut().zip(&windows) {
            *c = env[w + k];
        }
        col.sort_unstable_by(|a, b| a.total_cmp(b));
        let q = |p: f64| col[((col.len() - 1) as f64 * p).round() as usize];
        median.push(tmpl[k]);
        p10.push(q(0.1));
        p90.push(q(0.9));
    }
    let quiet = ((0.003 * fs) as usize).min(tmpl.len());
    let mut floor_v = tmpl[..quiet].to_vec();
    Some(TickProfile {
        side,
        beats: windows.len(),
        t0_ms: to_ms(0.0),
        step_ms: stride as f64 / fs * 1000.0,
        median,
        p10,
        p90,
        floor: crate::dsp::median_f32(&mut floor_v),
        unlock_ms: e.map(|e| to_ms(e.unlock)),
        drop_ms: e.map(|e| to_ms(e.drop)),
        peak_ms: e.map(|e| to_ms(e.peak as f64)),
        amplitude_deg: amp,
        sound1_ms: from_unlock(sh.as_ref().and_then(|s| s.t1_ms)),
        sound2_ms: from_unlock(sh.as_ref().and_then(|s| s.t2_ms)),
        sound3_ms: from_unlock(sh.as_ref().map(|s| s.t3_ms)),
    })
}

/// Start samples of the template windows around `times` that fit in `env`,
/// as `median_template` takes them.
fn windows(env: &[f32], fs: f64, times: &[f64]) -> Vec<usize> {
    let pre = (PRE_S * fs).round() as usize;
    let len = pre + (POST_S * fs).round() as usize;
    times
        .iter()
        .filter_map(|&t| {
            let c = (t * fs).round() as isize - pre as isize;
            (c >= 0 && c as usize + len <= env.len()).then_some(c as usize)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{analyze, AnalysisConfig};
    use crate::synth::{generate, SynthConfig};

    #[test]
    fn profile_marks_match_the_amplitude_reading() {
        let sc = SynthConfig {
            duration_s: 12.0,
            ..SynthConfig::default()
        };
        let audio = generate(&sc, |_| 280.0, |_| 0.0);
        let fs = audio.sample_rate as f64;
        let cfg = AnalysisConfig::default();
        let a = analyze(&audio, &cfg);
        let beat = 3600.0 / a.summary.bph as f64;
        let [pa, pb] = profiles_from_audio(&audio.samples, fs, &a.beats, 2.0 * beat, &cfg);
        for p in [pa.unwrap(), pb.unwrap()] {
            assert!(p.beats > 20, "{} beats", p.beats);
            assert_eq!(p.median.len(), p.p10.len());
            assert!((p.t0_ms + PRE_S * 1000.0).abs() < 1e-9);
            let (u, d) = (p.unlock_ms.unwrap(), p.drop_ms.unwrap());
            assert!(u < d && d.abs() < 1.5, "unlock {u} drop {d}");
            let amp = p.amplitude_deg.unwrap();
            assert!((amp - 280.0).abs() < 10.0, "amplitude {amp}");
            assert!(p.p10.iter().zip(&p.p90).all(|(a, b)| a <= b));
            assert!(p.sound3_ms.is_some());
        }
    }
}
