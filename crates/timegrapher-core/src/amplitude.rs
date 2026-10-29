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
//! The same templates give the beat error measured from the unlock, the
//! figure checked against tg's beat error in regress. Beat times sit near the drop, so
//! the fitted beat error is the drop's. If the even side's unlock comes
//! `u_e` after its beat time and the odd side's `u_o` after its own, the
//! unlocks fall at `t0 + k*P + s*e/2 + u_s`, which is the same model with
//! beat error `e + u_e - u_o`. When the beat times sit exactly on the drop
//! edges this is the drop's beat error minus the difference between the
//! two sides' unlock-to-drop times.

use crate::beats::{trimmed_template, trimmed_template_from, Beat, PRE_S};
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
            lift_deg: crate::take::DEFAULT_LIFT_DEG,
            onset_fraction: 0.02,
        }
    }
}

pub fn amplitude_deg(unlock_to_drop_s: f64, osc_period_s: f64, lift_deg: f64) -> f64 {
    lift_deg / (2.0 * (std::f64::consts::PI * unlock_to_drop_s / osc_period_s).sin())
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
    /// Top of sound 1, the unlock.
    pub unlock_level: f32,
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
        unlock_level: top,
        floor,
    })
}

/// [`edges`] on a template of a few beats, with sound 1 looked for from
/// [`UNLOCK_SEARCH_S`] before `guide_unlock` (samples into `template`):
/// where the unlock sat on a template of more beats. On a few beats a bump
/// of noise well before the unlock can pass for sound 1; the guide keeps
/// the search past it. Without a guide, from the start, as [`edges`].
pub fn edges_guided(
    template: &[f32],
    fs: f64,
    origin: usize,
    onset_fraction: f32,
    guide_unlock: Option<f64>,
) -> Option<Edges> {
    let from = guide_unlock.map_or(0, |u| (u - UNLOCK_SEARCH_S * fs).max(0.0) as usize);
    edges_after(template, fs, origin, onset_fraction, from)
}

/// How far before the unlock on a template of all the beats given a short
/// window's sound 1 is looked for first, seconds. Amplitude falling from
/// 300° to 250° moves the unlock about 1.3 ms earlier.
const UNLOCK_SEARCH_S: f64 = 0.003;

/// A sound 1 found from the start of a template [`template_pre_s`] long,
/// earlier than the one found from the guide and at least this share of
/// its height, is taken instead: the unlock lay before the search from the
/// guide (an amplitude that fell within the chunk, or a low one that put
/// the unlock before the 20 ms template), and what was found from the guide
/// is the impulse. A bump of noise is far quieter than either sound.
const EARLIER_SOUND_MIN: f32 = 0.5;

/// Lowest amplitude a window reads, degrees.
pub const MIN_AMPLITUDE_DEG: f64 = 100.0;

/// Seconds before the drop a template must start for sound 1 to be found
/// at [`MIN_AMPLITUDE_DEG`] at this oscillation period and lift: the unlock,
/// after the 3 ms the floor is taken on and a margin for its edge; never
/// less than [`PRE_S`]. At 18,000 bph and a 52° lift, 180° puts the unlock
/// 18.5 ms before the drop, inside the floor of a 20 ms template.
pub fn template_pre_s(osc_period_s: f64, lift_deg: f64) -> f64 {
    let s = (lift_deg / (2.0 * MIN_AMPLITUDE_DEG)).clamp(0.0, 1.0);
    let unlock_s = osc_period_s / std::f64::consts::PI * s.asin();
    (unlock_s + 0.005).max(PRE_S)
}

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
    /// The same window with Tick and Tock the other way round, for a
    /// stretch whose beats were numbered from the other side (see
    /// [`crate::breaks::renumber`]).
    pub fn swap_sides(&mut self) {
        std::mem::swap(&mut self.even_deg, &mut self.odd_deg);
        std::mem::swap(&mut self.even_unlock_ms, &mut self.odd_unlock_ms);
        std::mem::swap(&mut self.even_drop_ms, &mut self.odd_drop_ms);
        self.beat_error_ms = self.beat_error_ms.map(|v| -v);
        self.beat_error_unlock_ms = self.beat_error_unlock_ms.map(|v| -v);
    }

    pub fn mean(&self) -> Option<f64> {
        mean_of_sides(self.even_deg, self.odd_deg)
    }
}

/// A window's amplitude from its two sides': their mean, or the one side
/// read when the other is not.
pub fn mean_of_sides(even_deg: Option<f64>, odd_deg: Option<f64>) -> Option<f64> {
    match (even_deg, odd_deg) {
        (Some(a), Some(b)) => Some((a + b) / 2.0),
        (a, b) => a.or(b),
    }
}

/// Seconds of neighbouring windows each side's amplitude is checked against.
pub const OUTLIER_SPAN_S: f64 = 60.0;

/// Leave out one side's amplitude in a window when it sits more than five
/// robust SDs (at least 0.5°) from the median of that side's neighbours
/// within `span_s` while the other side moved less than a quarter as many
/// degrees from its own: an unlock or drop edge marked on the wrong sound,
/// which moves one side by tens of degrees while the other stays put. When
/// both sides move
/// together the watch moved (a dip once a turn of a wheel), and both
/// stay. Only the
/// amplitude is left out: the edges stay for the shape view, and the
/// unlock beat error is a median over windows already. Windows must be in
/// time order. Returns how many sides were left out.
pub fn reject_side_outliers(wins: &mut [AmplitudeWindow], span_s: f64) -> usize {
    // Each side's distance from its neighbours' median, degrees, and in
    // robust SDs.
    let score = |get: &dyn Fn(&AmplitudeWindow) -> Option<f64>| -> Vec<Option<(f64, f64)>> {
        let vals: Vec<Option<f64>> = wins.iter().map(get).collect();
        (0..wins.len())
            .map(|i| {
                let v = vals[i]?;
                let mid = |w: &AmplitudeWindow| (w.start_s + w.end_s) / 2.0;
                let lo = wins.partition_point(|w| mid(w) < mid(&wins[i]) - span_s / 2.0);
                let hi = wins.partition_point(|w| mid(w) <= mid(&wins[i]) + span_s / 2.0);
                let mut near: Vec<f64> = (lo..hi)
                    .filter(|&j| j != i)
                    .filter_map(|j| vals[j])
                    .collect();
                if near.len() < 5 {
                    return None;
                }
                let med = crate::dsp::median(&mut near.clone());
                for x in near.iter_mut() {
                    *x = (*x - med).abs();
                }
                let sd = (1.4826 * crate::dsp::median(&mut near)).max(0.5);
                Some(((v - med).abs(), (v - med).abs() / sd))
            })
            .collect()
    };
    let (even, odd) = (score(&|w| w.even_deg), score(&|w| w.odd_deg));
    let lone = |a: Option<(f64, f64)>, b: Option<(f64, f64)>| matches!((a, b), (Some(a), Some(b)) if a.1 > 5.0 && b.0 < 0.25 * a.0);
    let (even, odd): (Vec<bool>, Vec<bool>) = even
        .iter()
        .zip(&odd)
        .map(|(&e, &o)| (lone(e, o), lone(o, e)))
        .unzip();
    let mut n = 0;
    for (w, (e, o)) in wins.iter_mut().zip(even.into_iter().zip(odd)) {
        if e {
            w.even_deg = None;
        }
        if o {
            w.odd_deg = None;
        }
        n += usize::from(e) + usize::from(o);
    }
    n
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
    let plausible = |a: f64| (MIN_AMPLITUDE_DEG..=380.0).contains(&a);
    let origin = (PRE_S * fs).round() as usize;
    let long_pre_s = template_pre_s(osc_period_s, cfg.lift_deg);
    let long_origin = (long_pre_s * fs).round() as usize;
    let side_times = |win: &[Beat], even: bool| -> Vec<f64> {
        win.iter()
            .filter(|b| b.is_good() && (b.index.rem_euclid(2) == 0) == even)
            .map(|b| b.time)
            .collect()
    };
    // Where each side's unlock sits on a template of every beat given, as a
    // guide for the short windows: on a few beats a bump of noise well
    // before the unlock can pass for sound 1 (the Daytona read 130–170° in
    // about one window in 40 that way), so a window's sound 1 is looked for
    // from UNLOCK_SEARCH_S before this one, unless a sound as loud as
    // EARLIER_SOUND_MIN comes before it.
    let guide = |even: bool| -> Option<f64> {
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
        e.map(|e| e.unlock)
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
            let guided = edges_guided(
                &tmpl,
                fs,
                origin,
                cfg.onset_fraction,
                from[usize::from(!even)],
            )?;
            // Sound 1 looked for from the start of a template long enough
            // for the lowest amplitude, as seconds from the beat time.
            let height = |e: &Edges| e.unlock_level - e.floor;
            let at = |i: f64, origin: usize| (i - origin as f64) / fs;
            let earlier = edges_after(
                &trimmed_template_from(env, fs, &times, long_pre_s),
                fs,
                long_origin,
                cfg.onset_fraction,
                0,
            )
            .filter(|e| {
                at(e.unlock, long_origin) < at(guided.unlock, origin) - 0.0003
                    && height(e) >= EARLIER_SOUND_MIN * height(&guided)
            });
            let (unlock, drop) = match earlier {
                Some(e) => (at(e.unlock, long_origin), at(e.drop, long_origin)),
                None => (at(guided.unlock, origin), at(guided.drop, origin)),
            };
            let a = amplitude_deg(drop - unlock, osc_period_s, cfg.lift_deg);
            plausible(a).then_some((a, unlock, drop))
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
    fn side_outliers_drop_one_side_only() {
        let mut wins: Vec<super::AmplitudeWindow> = (0..60)
            .map(|i| super::AmplitudeWindow {
                start_s: 2.0 * i as f64,
                end_s: 2.0 * i as f64 + 2.0,
                // A slow 48 s cycle of ±4° on both sides is the watch.
                even_deg: Some(
                    250.0
                        + 4.0 * (i as f64 * 2.0 * std::f64::consts::PI / 24.0).sin()
                        + 0.3 * (i % 3) as f64,
                ),
                odd_deg: Some(
                    240.0 + 4.0 * (i as f64 * 2.0 * std::f64::consts::PI / 24.0).sin()
                        - 0.3 * (i % 2) as f64,
                ),
                beat_error_ms: None,
                beat_error_unlock_ms: Some(0.3),
                even_unlock_ms: None,
                odd_unlock_ms: None,
                even_drop_ms: None,
                odd_drop_ms: None,
            })
            .collect();
        // Window 30's Tock edge lands on the wrong sound.
        wins[30].odd_deg = Some(290.0);
        let n = super::reject_side_outliers(&mut wins, super::OUTLIER_SPAN_S);
        assert_eq!(n, 1);
        assert_eq!(wins[30].odd_deg, None);
        assert!(wins[30].even_deg.is_some());
        assert_eq!(wins[30].beat_error_unlock_ms, Some(0.3));
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

    /// Each side's amplitude in the 2 s windows from `from_s` to `to_s`
    /// of a 48 kHz synthetic watch at `bph` whose amplitude is `amp(t)`.
    fn read_windows(
        bph: u32,
        duration_s: f64,
        amp: impl Fn(f64) -> f64,
        from_s: f64,
        to_s: f64,
    ) -> Vec<(f64, Option<f64>, Option<f64>)> {
        use crate::stream::{analyze_samples, StreamConfig};
        use crate::synth::{generate, SynthConfig};
        let audio = generate(
            &SynthConfig {
                duration_s,
                bph,
                ..Default::default()
            },
            amp,
            |_| 0.0,
        );
        let cfg = StreamConfig {
            analysis: crate::analysis::AnalysisConfig {
                bph: Some(bph),
                ..Default::default()
            },
            ..Default::default()
        };
        analyze_samples(&audio.samples, audio.sample_rate, &cfg)
            .amplitude_windows
            .iter()
            .filter(|w| w.start_s >= from_s && w.end_s <= to_s)
            .map(|w| (w.start_s, w.even_deg, w.odd_deg))
            .collect()
    }

    #[test]
    fn a_low_amplitude_on_a_slow_watch_reads_low() {
        // At 18,000 bph and a 52° lift, 180° puts the unlock 18.5 ms before
        // the drop, which a 20 ms template with its first 3 ms taken as the
        // floor left out: the impulse was taken for it, reading 326°.
        for (bph, amp) in [
            (14_400, 150.0),
            (18_000, 110.0),
            (18_000, 180.0),
            (21_600, 150.0),
            (28_800, 110.0),
            (28_800, 270.0),
        ] {
            // A side whose edge lands on noise in one window is left out
            // there, as a side outlier; every side read is right.
            let w = read_windows(bph, 20.0, |_| amp, 4.0, 18.0);
            let read: Vec<f64> = w.iter().flat_map(|w| [w.1, w.2]).flatten().collect();
            assert!(read.len() + 1 >= 2 * w.len() && w.len() == 7, "{w:?}");
            for a in read {
                assert!((a - amp).abs() < 8.0, "{bph} bph at {amp}°: {w:?}");
            }
        }
    }

    #[test]
    fn a_dip_in_amplitude_reads_as_a_dip() {
        // 270° with 10 s at a lower amplitude: the unlock moves earlier than
        // the search from the guide allows, which read the impulse as the
        // unlock (a dip to 180° read 326°).
        for low in [140.0, 180.0, 200.0] {
            let amp = move |t: f64| {
                if (30.0..40.0).contains(&t) {
                    low
                } else {
                    270.0
                }
            };
            for (start, even, odd) in read_windows(28_800, 70.0, amp, 24.0, 46.0) {
                let want = amp(start + 1.0);
                if (start - 30.0).abs() < 1.0 || (start - 40.0).abs() < 1.0 {
                    continue;
                }
                for a in [even, odd].into_iter().flatten() {
                    assert!(
                        (a - want).abs() < 8.0,
                        "dip to {low}°: {a}° in the window from {start} s, {want}° heard"
                    );
                }
                assert!(
                    even.is_some() || odd.is_some(),
                    "dip to {low}°: none at {start} s"
                );
            }
        }
    }
}
