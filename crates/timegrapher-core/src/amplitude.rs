//! Amplitude from the time between the unlock and the drop.
//!
//! The balance swings through the lift angle `L` while the escapement is
//! engaged. If `t` is the time from the unlock to the drop and `T` the
//! period of a full oscillation, the amplitude is
//! `A = L / (2 sin(pi t / T))` (the formula Witschi and tg use).
//!
//! `t` is measured on median templates of a few seconds of beats, one for
//! each side (tick and toc), because single beats are too noisy for a
//! stable onset.

use crate::beats::{median_template, Beat, PRE_S};
use crate::dsp::{median_f32, moving_average};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AmplitudeConfig {
    /// Lift angle in degrees.
    pub lift_deg: f64,
    /// Unlock threshold as a fraction of the drop's height above the noise floor.
    pub onset_fraction: f32,
}

impl Default for AmplitudeConfig {
    fn default() -> Self {
        AmplitudeConfig {
            lift_deg: 52.0,
            onset_fraction: 0.05,
        }
    }
}

pub fn amplitude_deg(unlock_to_drop_s: f64, osc_period_s: f64, lift_deg: f64) -> f64 {
    lift_deg / (2.0 * (std::f64::consts::PI * unlock_to_drop_s / osc_period_s).sin())
}

/// Unlock-to-drop time on a template whose drop sits `PRE_S` from its start.
///
/// Both ends are taken on rising edges, which are sharper and steadier than
/// peaks: the unlock is where the template first rises `onset_fraction` of
/// the drop's height above the noise floor, and the drop is where it first
/// reaches half the drop's height. The unlock is often much quieter than the
/// impulse that follows it, so the threshold is relative to the drop rather
/// than to the loudest pre-drop sound.
pub fn unlock_to_drop(template: &[f32], fs: f64, onset_fraction: f32) -> Option<f64> {
    let t = moving_average(template, ((0.0002 * fs) as usize).max(1));
    let origin = (PRE_S * fs).round() as usize;
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
    // Drop edge: within 2 ms before the peak.
    let drop = crossing(
        peak.saturating_sub((0.002 * fs) as usize),
        peak + 1,
        floor + 0.5 * height,
    )?;
    // Unlock edge: the first rise after the quiet stretch, ending well before the drop.
    let end = (drop as usize).checked_sub((0.0015 * fs) as usize)?;
    let unlock = crossing(quiet, end, floor + onset_fraction * height)?;
    Some((drop - unlock) / fs)
}

#[derive(Debug, Clone, Serialize)]
pub struct AmplitudeWindow {
    pub start_s: f64,
    pub end_s: f64,
    /// Amplitude from even beats and from odd beats, degrees.
    pub even_deg: Option<f64>,
    pub odd_deg: Option<f64>,
}

impl AmplitudeWindow {
    pub fn mean(&self) -> Option<f64> {
        match (self.even_deg, self.odd_deg) {
            (Some(a), Some(b)) => Some((a + b) / 2.0),
            (a, b) => a.or(b),
        }
    }
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
    let mut out = Vec::new();
    let Some(last) = beats.last() else { return out };
    let plausible = |a: f64| (100.0..=380.0).contains(&a);
    let mut start = beats[0].time;
    let mut lo = 0usize;
    while start + window_s <= last.time + 1e-9 {
        while lo < beats.len() && beats[lo].time < start {
            lo += 1;
        }
        let mut hi = lo;
        while hi < beats.len() && beats[hi].time < start + window_s {
            hi += 1;
        }
        let win = &beats[lo..hi];
        let side = |even: bool| -> Option<f64> {
            let times: Vec<f64> = win
                .iter()
                .filter(|b| b.quality > 0.4 && (b.index.rem_euclid(2) == 0) == even)
                .map(|b| b.time)
                .collect();
            if times.len() < 3 {
                return None;
            }
            let tmpl = median_template(env, fs, &times);
            let t = unlock_to_drop(&tmpl, fs, cfg.onset_fraction)?;
            Some(amplitude_deg(t, osc_period_s, cfg.lift_deg)).filter(|&a| plausible(a))
        };
        out.push(AmplitudeWindow {
            start_s: start,
            end_s: start + window_s,
            even_deg: side(true),
            odd_deg: side(false),
        });
        start += window_s;
    }
    out
}
