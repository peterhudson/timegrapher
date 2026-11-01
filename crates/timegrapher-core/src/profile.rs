//! The sound of a tick and of a tock, for drawing and debugging.
//!
//! A profile is one side's median envelope over a stretch of beats, the
//! spread around it, and the marks the engine measured on it: the unlock and
//! drop edges that amplitude and beat error use, and the three sounds the
//! shape module finds. It is built from the same template as
//! [`crate::amplitude`], so the marks are exactly the ones behind the
//! numbers on screen.

use crate::amplitude::{amplitude_deg, edges_guided, AmplitudeConfig};
use crate::beats::{Beat, POST_S, PRE_S, TEMPLATE_TRIM};
use crate::dsp::envelope;
use crate::shape::{self, ShapeConfig};
use serde::Serialize;

/// Which beats a profile is built from: `A` is the tick (even beats) and
/// `B` the tock (odd beats). People-facing text says Tick and Tock.
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

/// One side's typical sound with the engine's marks. Times are ms from the
/// beat's reference point (near the drop); levels are envelope values.
#[derive(Debug, Clone, Serialize)]
pub struct TickProfile {
    pub side: Side,
    /// Beats that went into the profile.
    pub beats: usize,
    /// Time of the first point and the spacing between points, ms.
    pub t0_ms: f64,
    pub step_ms: f64,
    /// The beats' typical envelope, point by point: the mean of the middle
    /// half of the beats (a 25% trimmed mean, the template amplitude is
    /// measured on; named `median` for compatibility), and the 10th and
    /// 90th percentiles.
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
/// times are seconds from the start of `x`. A few seconds of beats are
/// guided as [`from_snippets`] says, by `guides`: Tick's and Tock's.
pub fn profiles_from_audio(
    x: &[f32],
    fs: f64,
    beats: &[Beat],
    osc_period_s: f64,
    cfg: &crate::analysis::AnalysisConfig,
    guides: [Option<f64>; 2],
) -> [Option<TickProfile>; 2] {
    let env = envelope(x, fs, &cfg.envelope);
    let to = x.len() as f64 / fs;
    let [a, b] = guides;
    [(Side::A, a), (Side::B, b)].map(|(side, guide)| {
        let snips = side_snippets(&env, fs, beats, side, 0.0, to);
        from_snippets(&snips, fs, side, osc_period_s, &cfg.amplitude, guide)
    })
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
    let snips = side_snippets(env, fs, beats, side, from_s, to_s);
    from_snippets(&snips, fs, side, osc_period_s, cfg, None)
}

/// The windows of one side's good beats in `[from_s, to_s)`.
fn side_snippets<'a>(
    env: &'a [f32],
    fs: f64,
    beats: &[Beat],
    side: Side,
    from_s: f64,
    to_s: f64,
) -> Vec<&'a [f32]> {
    let times: Vec<f64> = beats
        .iter()
        .filter(|b| {
            b.time >= from_s
                && b.time < to_s
                && b.is_good()
                && (b.index.rem_euclid(2) == 0) == side.even()
        })
        .map(|b| b.time)
        .collect();
    let len = template_len(fs);
    windows(env, fs, &times)
        .into_iter()
        .map(|w| &env[w..w + len])
        .collect()
}

/// Samples in one beat's window: `PRE_S` before the beat to `POST_S` after.
pub fn template_len(fs: f64) -> usize {
    (PRE_S * fs).round() as usize + (POST_S * fs).round() as usize
}

/// One beat's envelope window, as [`from_snippets`] takes it, or `None`
/// when it runs off either end of `env`. `t` is the beat time in `env`'s
/// time base.
pub fn snippet(env: &[f32], fs: f64, t: f64) -> Option<&[f32]> {
    let w = *windows(env, fs, &[t]).first()?;
    Some(&env[w..w + template_len(fs)])
}

/// A profile from beat windows already cut out of the envelope (each
/// [`template_len`] samples, the beat `PRE_S` in), so a live view can keep
/// windows from many passes. On a few seconds of beats a bump of noise
/// before the unlock can pass for sound 1 for a moment; `guide_unlock_ms`,
/// where this side's latest amplitude reading put its unlock (ms from the
/// beat time, [`crate::amplitude::AmplitudeWindow`]), keeps the marks on
/// the sound the reading measured ([`edges_guided`]).
pub fn from_snippets(
    snips: &[&[f32]],
    fs: f64,
    side: Side,
    osc_period_s: f64,
    cfg: &AmplitudeConfig,
    guide_unlock_ms: Option<f64>,
) -> Option<TickProfile> {
    let len = template_len(fs);
    let snips: Vec<&[f32]> = snips.iter().copied().filter(|w| w.len() == len).collect();
    if snips.len() < 3 {
        return None;
    }
    let mut col = vec![0.0f32; snips.len()];
    let mut sorted_at = |k: usize| -> Vec<f32> {
        for (c, w) in col.iter_mut().zip(&snips) {
            *c = w[k];
        }
        col.sort_unstable_by(|a, b| a.total_cmp(b));
        col.clone()
    };
    let q = |c: &[f32], p: f64| c[((c.len() - 1) as f64 * p).round() as usize];
    // The same trimmed-mean template the amplitude measurement builds.
    let tmpl: Vec<f32> = (0..len)
        .map(|k| crate::dsp::trimmed_mean_f32(&mut sorted_at(k), TEMPLATE_TRIM))
        .collect();
    let origin = (PRE_S * fs).round() as usize;
    let to_ms = |s: f64| (s - origin as f64) / fs * 1000.0;
    let guide = guide_unlock_ms.map(|ms| origin as f64 + ms / 1000.0 * fs);
    let e = edges_guided(&tmpl, fs, origin, cfg.onset_fraction, guide);
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
    for k in (0..len).step_by(stride) {
        let c = sorted_at(k);
        median.push(tmpl[k]);
        p10.push(q(&c, 0.1));
        p90.push(q(&c, 0.9));
    }
    let quiet = ((0.003 * fs) as usize).min(len);
    let mut floor_v = tmpl[..quiet].to_vec();
    Some(TickProfile {
        side,
        beats: snips.len(),
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

    /// One side's good beats from `from_s` up to but not at `to_s`, each
    /// cut out where [`snippet`] cuts it.
    #[test]
    fn a_sides_snippets_are_its_good_beats_in_the_stretch() {
        let fs = 1000.0;
        let env: Vec<f32> = (0..2000).map(|i| i as f32).collect();
        let beats: Vec<Beat> = (0..11)
            .map(|i| Beat {
                index: i,
                time: 0.5 + 0.1 * i as f64,
                quality: if i == 4 { 0.0 } else { 1.0 },
            })
            .collect();
        let (from, to) = (beats[2].time, beats[7].time);
        let at = |side, idx: &[usize]| {
            let got: Vec<*const f32> = side_snippets(&env, fs, &beats, side, from, to)
                .iter()
                .map(|s| s.as_ptr())
                .collect();
            let want: Vec<*const f32> = idx
                .iter()
                .map(|&i| snippet(&env, fs, beats[i].time).unwrap().as_ptr())
                .collect();
            assert_eq!(got, want, "{side:?}");
        };
        // Even beats 2 and 6 (4 is not good); odd 3 and 5 (7 is at the end).
        at(Side::A, &[2, 6]);
        at(Side::B, &[3, 5]);
    }

    #[test]
    fn a_bump_before_sound_1_is_not_the_unlock_when_guided() {
        // Twelve beats whose envelope has a bump of noise 15 ms before the
        // drop, sound 1 at 8 ms and the drop at the beat time.
        let fs = 48_000.0;
        let len = template_len(fs);
        let origin = (PRE_S * fs).round() as usize;
        let pulse = |i: usize, at_ms: f64, h: f32| {
            let d = (i as f64 - origin as f64) / fs * 1000.0 - at_ms;
            h * (-(d / 0.3).powi(2)).exp() as f32
        };
        let snips: Vec<Vec<f32>> = (0..12)
            .map(|k| {
                (0..len)
                    .map(|i| {
                        0.01 + 0.001 * ((i * 7 + k * 13) % 5) as f32
                            + pulse(i, -15.0, 0.2)
                            + pulse(i, -8.0, 0.3)
                            + pulse(i, 0.0, 1.0)
                    })
                    .collect()
            })
            .collect();
        let snips: Vec<&[f32]> = snips.iter().map(Vec::as_slice).collect();
        let cfg = AmplitudeConfig::default();
        let unlock = |guide| {
            from_snippets(&snips, fs, Side::B, 0.25, &cfg, guide)
                .and_then(|p| p.unlock_ms)
                .expect("an unlock")
        };
        assert!(unlock(None) < -14.0, "unguided, the bump: {}", unlock(None));
        // Guided by where the reading put the unlock, sound 1.
        let u = unlock(Some(-8.3));
        assert!((-8.8..-7.8).contains(&u), "guided: {u}");
    }

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
        let [pa, pb] =
            profiles_from_audio(&audio.samples, fs, &a.beats, 2.0 * beat, &cfg, [None, None]);
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
