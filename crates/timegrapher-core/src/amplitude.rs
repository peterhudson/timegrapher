//! Amplitude from the time between the unlock and the drop.
//!
//! The balance swings through the lift angle `L` while the escapement is
//! engaged. If `t` is the time from the unlock to the drop and `T` the
//! period of a full oscillation, the amplitude is
//! `A = L / (2 sin(pi t / T))` (the formula Witschi and tg use).
//!
//! `t` is measured on templates of a few seconds of beats (at each point the mean
//! of the middle half of the beats, which ignores stray clicks), one for
//! each side (tick and toc), because single beats are too noisy for a
//! stable onset.
//!
//! The same templates give the beat error measured from the unlock, as tg
//! and commercial timegraphers measure it. Beat times sit near the drop, so
//! the fitted beat error is the drop's. If the even side's unlock comes
//! `u_e` after its beat time and the odd side's `u_o` after its own, the
//! unlocks fall at `t0 + k*P + s*e/2 + u_s`, which is the same model with
//! beat error `e + u_e - u_o`. When the beat times sit exactly on the drop
//! edges this is the drop's beat error minus the difference between the
//! two sides' unlock-to-drop times.

use crate::beats::{trimmed_template, Beat, PRE_S};
use crate::dsp::{median_f32, moving_average};
use crate::timing;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AmplitudeConfig {
    /// Lift angle in degrees.
    pub lift_deg: f64,
    /// Minimum height of sound 1 (the unlock) as a fraction of the drop's
    /// height above the noise floor; 4 noise SDs is used when that is larger.
    pub onset_fraction: f32,
}

impl Default for AmplitudeConfig {
    fn default() -> Self {
        AmplitudeConfig {
            lift_deg: 52.0,
            onset_fraction: 0.02,
        }
    }
}

pub fn amplitude_deg(unlock_to_drop_s: f64, osc_period_s: f64, lift_deg: f64) -> f64 {
    lift_deg / (2.0 * (std::f64::consts::PI * unlock_to_drop_s / osc_period_s).sin())
}

/// Unlock-to-drop time on a template whose drop sits `PRE_S` from its start.
pub fn unlock_to_drop(template: &[f32], fs: f64, onset_fraction: f32) -> Option<f64> {
    let e = edges(template, fs, (PRE_S * fs).round() as usize, onset_fraction)?;
    Some((e.drop - e.unlock) / fs)
}

/// Landmarks of one template, in samples from its start.
#[derive(Debug, Clone, Copy)]
pub struct Edges {
    /// Unlock and drop edges (fractional samples).
    pub unlock: f64,
    pub drop: f64,
    /// Drop peak and its level.
    pub peak: usize,
    pub peak_level: f32,
    /// Median of the template's first 3 ms.
    pub floor: f32,
}

/// Unlock and drop edges on a template whose drop sits near `origin`.
///
/// Both ends are taken on rising edges, which are sharper and steadier than
/// peaks: the unlock is where the template first rises `onset_fraction` of
/// the drop's height above the noise floor, and the drop is where it first
/// reaches half the drop's height. The unlock is often much quieter than the
/// impulse that follows it, so the threshold is relative to the drop rather
/// than to the loudest pre-drop sound.
pub fn edges(template: &[f32], fs: f64, origin: usize, onset_fraction: f32) -> Option<Edges> {
    edges_after(template, fs, origin, onset_fraction, 0)
}

/// [`edges`], with sound 1 looked for no earlier than sample `from`.
fn edges_after(
    template: &[f32],
    fs: f64,
    origin: usize,
    onset_fraction: f32,
    from: usize,
) -> Option<Edges> {
    let t = moving_average(template, ((0.0002 * fs) as usize).max(1));
    let w = (0.0015 * fs) as usize;
    let (peak, peak_v) = crate::dsp::argmax(&t, origin.saturating_sub(w), origin + w)?;
    let quiet = (0.003 * fs) as usize;
    let mut floor_v = t[..quiet.min(t.len())].to_vec();
    let floor = median_f32(&mut floor_v);
    let height = peak_v - floor;
    if height <= 0.0 {
        return None;
    }
    let crossing = |from: usize, to: usize, level: f32| -> Option<f64> {
        let i = (from.max(1)..to).find(|&i| t[i] > level)?;
        let (y0, y1) = (t[i - 1], t[i]);
        Some((i - 1) as f64 + ((level - y0) / (y1 - y0)) as f64)
    };
    // Drop edge: where the rise into the peak crosses half height, within
    // 2 ms before the peak. Walking back from the peak (rather than taking
    // the first crossing in those 2 ms) keeps the edge on that rise when an
    // earlier sound sits near half the drop's height.
    let half = floor + 0.5 * height;
    let start = peak.saturating_sub((0.002 * fs) as usize).max(1);
    let mut i = peak;
    while i > start && t[i - 1] > half {
        i -= 1;
    }
    let drop = if i > start {
        let (y0, y1) = (t[i - 1], t[i]);
        (i - 1) as f64 + ((half - y0) / (y1 - y0)) as f64
    } else {
        crossing(start, peak + 1, half)?
    };
    // Unlock edge: sound 1 is found as the first sustained rise above the
    // floor by `onset_fraction` of the drop or 4 noise SDs, whichever is
    // larger, and its edge is where it crosses half its own height. A
    // threshold relative to the drop alone misses a sound 1 that is quiet
    // next to the drop and falls through to sound 2.
    let end = (drop as usize).checked_sub((0.0015 * fs) as usize)?;
    let noise = {
        let q: Vec<f64> = t[..quiet.min(t.len())].iter().map(|&v| v as f64).collect();
        crate::dsp::robust_sd(&q) as f32
    };
    let detect = floor + (onset_fraction * height).max(4.0 * noise);
    let hold = ((0.00025 * fs) as usize).max(1);
    let first = (quiet.max(from).max(1)..end)
        .find(|&i| t[i..(i + hold).min(end)].iter().all(|&v| v > detect))?;
    let look = (first + (0.0006 * fs) as usize).min(end);
    let (top_i, top) = crate::dsp::argmax(&t, first, look + 1)?;
    let back = first.saturating_sub((0.001 * fs) as usize).max(quiet);
    let unlock = crossing(back, top_i + 1, floor + 0.5 * (top - floor))?;
    Some(Edges {
        unlock,
        drop,
        peak,
        peak_level: peak_v,
        floor,
    })
}

/// How far before the unlock on a template of all the beats given a short
/// window's sound 1 may be found, seconds. Amplitude falling from 300° to
/// 250° moves the unlock about 1.3 ms earlier.
const UNLOCK_SEARCH_S: f64 = 0.003;

#[derive(Debug, Clone, Serialize)]
pub struct AmplitudeWindow {
    pub start_s: f64,
    pub end_s: f64,
    /// Amplitude from even beats and from odd beats, degrees.
    pub even_deg: Option<f64>,
    pub odd_deg: Option<f64>,
    /// Signed beat error from the window's beat times (near the drop) and
    /// from the unlock edges, ms; even beats late is positive.
    pub beat_error_ms: Option<f64>,
    pub beat_error_unlock_ms: Option<f64>,
    /// Where the unlock and drop edges sat on each side's template, ms from
    /// the beat time, so a jump of an edge between sounds shows.
    pub even_unlock_ms: Option<f64>,
    pub odd_unlock_ms: Option<f64>,
    pub even_drop_ms: Option<f64>,
    pub odd_drop_ms: Option<f64>,
}

impl AmplitudeWindow {
    pub fn mean(&self) -> Option<f64> {
        match (self.even_deg, self.odd_deg) {
            (Some(a), Some(b)) => Some((a + b) / 2.0),
            (a, b) => a.or(b),
        }
    }
}

/// Standard error of the median of window amplitudes taken in time order,
/// degrees: 1.25 s / sqrt(n), with s the spread from one window to the
/// next (the MAD of successive differences over sqrt 2), so a slow drift
/// or cycle in the watch's amplitude does not count as noise. None with
/// fewer than three values.
pub fn standard_error(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }
    let mut d: Vec<f64> = values.windows(2).map(|w| w[1] - w[0]).collect();
    let mid = crate::dsp::median(&mut d.clone());
    for v in d.iter_mut() {
        *v = (*v - mid).abs();
    }
    let s = 1.4826 * crate::dsp::median(&mut d) / std::f64::consts::SQRT_2;
    Some(1.2533 * s / (values.len() as f64).sqrt())
}

/// Amplitude in consecutive windows of `window_s` seconds.
pub fn windows(
    env: &[f32],
    fs: f64,
    beats: &[Beat],
    osc_period_s: f64,
    window_s: f64,
    cfg: &AmplitudeConfig,
) -> Vec<AmplitudeWindow> {
    let (Some(first), Some(last)) = (beats.first(), beats.last()) else {
        return Vec::new();
    };
    windows_between(
        env,
        fs,
        beats,
        osc_period_s,
        window_s,
        first.time,
        last.time,
        cfg,
    )
}

/// Amplitude in consecutive windows of `window_s` seconds from `from_s`,
/// up to the last window that ends by `to_s`. Beat times and the window
/// bounds are in the envelope's time base.
#[allow(clippy::too_many_arguments)]
pub fn windows_between(
    env: &[f32],
    fs: f64,
    beats: &[Beat],
    osc_period_s: f64,
    window_s: f64,
    from_s: f64,
    to_s: f64,
    cfg: &AmplitudeConfig,
) -> Vec<AmplitudeWindow> {
    let mut out = Vec::new();
    let plausible = |a: f64| (100.0..=380.0).contains(&a);
    let origin = (PRE_S * fs).round() as usize;
    let side_times = |win: &[Beat], even: bool| -> Vec<f64> {
        win.iter()
            .filter(|b| b.quality > 0.4 && (b.index.rem_euclid(2) == 0) == even)
            .map(|b| b.time)
            .collect()
    };
    // Where each side's unlock sits on a template of every beat given, as a
    // guide for the short windows: on a few beats a bump of noise well
    // before the unlock can pass for sound 1 (the Daytona read 130–170° in
    // about one window in 40 that way), so a window's sound 1 is looked for
    // from UNLOCK_SEARCH_S before this one.
    let guide = |even: bool| -> usize {
        let times = side_times(beats, even);
        let e = (times.len() >= 3)
            .then(|| {
                edges(
                    &trimmed_template(env, fs, &times),
                    fs,
                    origin,
                    cfg.onset_fraction,
                )
            })
            .flatten();
        e.map_or(0, |e| (e.unlock - UNLOCK_SEARCH_S * fs).max(0.0) as usize)
    };
    let from = [guide(true), guide(false)];
    let mut start = from_s;
    let mut lo = 0usize;
    while start + window_s <= to_s + 1e-9 {
        while lo < beats.len() && beats[lo].time < start {
            lo += 1;
        }
        let mut hi = lo;
        while hi < beats.len() && beats[hi].time < start + window_s {
            hi += 1;
        }
        let win = &beats[lo..hi];
        // Amplitude, and the unlock's and drop's offsets from the beat
        // times, seconds.
        let side = |even: bool| -> Option<(f64, f64, f64)> {
            let times = side_times(win, even);
            if times.len() < 3 {
                return None;
            }
            let tmpl = trimmed_template(env, fs, &times);
            let e = edges_after(
                &tmpl,
                fs,
                origin,
                cfg.onset_fraction,
                from[usize::from(!even)],
            )?;
            let a = amplitude_deg((e.drop - e.unlock) / fs, osc_period_s, cfg.lift_deg);
            let at = |i: f64| (i - origin as f64) / fs;
            plausible(a).then_some((a, at(e.unlock), at(e.drop)))
        };
        let (even, odd) = (side(true), side(false));
        let beat_error_ms = timing::beat_error_ms(win);
        let beat_error_unlock_ms = match (beat_error_ms, even, odd) {
            (Some(e), Some((_, ue, _)), Some((_, uo, _))) => Some(e + (ue - uo) * 1000.0),
            _ => None,
        };
        out.push(AmplitudeWindow {
            start_s: start,
            end_s: start + window_s,
            even_deg: even.map(|s| s.0),
            odd_deg: odd.map(|s| s.0),
            beat_error_ms,
            beat_error_unlock_ms,
            even_unlock_ms: even.map(|s| s.1 * 1000.0),
            odd_unlock_ms: odd.map(|s| s.1 * 1000.0),
            even_drop_ms: even.map(|s| s.2 * 1000.0),
            odd_drop_ms: odd.map(|s| s.2 * 1000.0),
        });
        start += window_s;
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn standard_error_ignores_slow_change() {
        // Gaussian noise of SD 2 from a fixed seed, alone and on a steep
        // ramp: the ramp must not count as noise.
        let mut seed = 12345u64;
        let mut uniform = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let noise: Vec<f64> = (0..400)
            .map(|_| {
                let (u, v) = (uniform(), uniform());
                2.0 * (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
            })
            .collect();
        let want = 1.2533 * 2.0 / 20.0;
        let flat = super::standard_error(&noise).unwrap();
        let ramp: Vec<f64> = noise
            .iter()
            .enumerate()
            .map(|(i, n)| 200.0 + 0.5 * i as f64 + n)
            .collect();
        let sloped = super::standard_error(&ramp).unwrap();
        assert!((flat / want - 1.0).abs() < 0.2, "{flat} vs {want}");
        assert!((sloped - flat).abs() < 1e-9, "{sloped} vs {flat}");
        assert_eq!(super::standard_error(&[1.0, 2.0]), None);
    }

    use super::*;

    #[test]
    fn drop_edge_stays_on_the_rise_into_the_peak() {
        // Floor, a short sound 1 at -7 ms, a shelf just under half the drop's
        // height from -2.3 to -1 ms with a brief bump above half on it, then
        // the drop rising to its peak at 0.
        let fs = 48_000.0;
        let origin = (PRE_S * fs).round() as usize;
        let len = origin + (0.01 * fs) as usize;
        let at = |ms: f64| (origin as f64 + ms / 1000.0 * fs).round() as usize;
        let mut t = vec![0.01f32; len];
        t[at(-7.0)..at(-6.7)].fill(0.3);
        t[at(-2.3)..at(-1.0)].fill(0.45);
        t[at(-1.8)..at(-1.5)].fill(0.6);
        t[at(-1.0)..at(0.0)].fill(0.8);
        t[at(0.0)..at(0.3)].fill(1.0);
        let e = edges(&t, fs, origin, 0.02).unwrap();
        let drop_ms = (e.drop - origin as f64) / fs * 1000.0;
        assert!((drop_ms + 1.0).abs() < 0.15, "drop at {drop_ms} ms");
        let unlock_ms = (e.unlock - origin as f64) / fs * 1000.0;
        assert!((unlock_ms + 7.0).abs() < 0.15, "unlock at {unlock_ms} ms");
    }

    #[test]
    fn a_bump_of_noise_before_the_unlock_is_not_sound_1() {
        // 20 s of beats at 28,800 bph: floor, sound 1 at -7 ms, the drop at
        // the beat time. In one 2 s window every Tick also has a bump 15 ms
        // before the drop, louder than the unlock threshold.
        let fs = 48_000.0;
        let period = 0.125;
        let mut env = vec![0.01f32; (20.5 * fs) as usize];
        let at = |t: f64| (t * fs).round() as usize;
        let mut beats = Vec::new();
        for k in 0..160i64 {
            let t = 0.1 + k as f64 * period;
            env[at(t - 0.007)..at(t - 0.0067)].fill(0.3);
            env[at(t)..at(t + 0.0003)].fill(1.0);
            if k % 2 == 0 && (6.0..8.0).contains(&t) {
                env[at(t - 0.015)..at(t - 0.0145)].fill(0.06);
            }
            beats.push(Beat {
                index: k,
                time: t,
                quality: 1.0,
            });
        }
        let cfg = AmplitudeConfig::default();
        let w = windows_between(&env, fs, &beats, 2.0 * period, 2.0, 0.0, 20.0, &cfg);
        let bumped = w.iter().find(|w| w.start_s == 6.0).unwrap();
        let clean = w.iter().find(|w| w.start_s == 10.0).unwrap();
        let (b, c) = (
            bumped.even_unlock_ms.unwrap(),
            clean.even_unlock_ms.unwrap(),
        );
        assert!(
            (b - c).abs() < 0.1,
            "unlock at {b} ms with the bump, {c} without"
        );
    }
}
