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

use crate::beats::{trimmed_template_from, Beat, PRE_S};
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

/// The start of a template the noise floor is read on, seconds: its
/// median is the floor and its spread the noise, and sound 1 is looked for
/// only after it.
const FLOOR_S: f64 = 0.003;

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
/// Both ends are taken on rising edges, each where a sound crosses half its
/// own height above the noise floor, which is sharper and steadier than a
/// peak: the drop where the rise into the drop's peak crosses half the
/// drop's height, and the unlock where sound 1 crosses half its own. Sound
/// 1 is found as the first rise held above `onset_fraction` of the drop's
/// height (or 4 noise SDs, if more): it is often much quieter than the
/// impulse that follows it, so that threshold is relative to the drop
/// rather than to the loudest pre-drop sound.
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
    let quiet = (FLOOR_S * fs) as usize;
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

/// How much later than the unlock on a template of every beat given a
/// window's sound 1 may sit before the window is taken to have found a
/// later sound, seconds. The beats given are one chunk of a stream (47 s):
/// in that time a run-down's unlock barely moves, and even a fall from
/// 300° to 250° moves it 1.3 ms. Near stopping, the impulse sits 6 ms
/// after the unlock at 28,800 bph (#1623).
const UNLOCK_LATE_S: f64 = 0.002;

/// How far apart the two sides' unlocks on their long templates may lie,
/// as a share of their mean distance from the drop, for the sides to be
/// read on long templates ([`Guide::Long`]). The unlock-to-drop time is
/// the balance crossing the lift, and both sides cross it at the same
/// amplitude, so the two differ only by the pallets' lift asymmetry: a few
/// degrees of a 50–60° lift, under a tenth. A share, not a fixed time,
/// because that time grows as the amplitude falls (14 ms near 160°, 23 ms
/// near 110° at 28,800 bph), and the real gap and the template's noise
/// grow with it: a fixed 2 ms split the sides of Peter's 69 h run-down at
/// 62 h. On Peter's Submariner at Crown Down a faint sound at 18.4 ms was
/// read as the Tock's unlock against the Tick's 14 ms, about 27% apart
/// (#1703, Norm's ruling).
const SIDES_UNLOCK_AGREE_SHARE: f64 = 0.15;

/// A sound 1 found from the start of a template [`template_pre_s`] long,
/// earlier than the one found from the guide and at least this share of
/// its height, is taken instead: the unlock lay before the search from the
/// guide (an amplitude that fell within the chunk, or a low one that put
/// the unlock before the 20 ms template), and what was found from the guide
/// is the impulse. A bump of noise is far quieter than either sound.
const EARLIER_SOUND_MIN: f32 = 0.5;

/// Lowest amplitude a window reads, degrees.
pub const MIN_AMPLITUDE_DEG: f64 = 100.0;
/// The highest amplitude a side is read as, degrees: above it the edges
/// were marked on the wrong sounds.
pub const MAX_AMPLITUDE_DEG: f64 = 380.0;

/// One side's amplitude from the time from its unlock to its drop at
/// `lift_deg`, or None outside [`MIN_AMPLITUDE_DEG`] to
/// [`MAX_AMPLITUDE_DEG`]: the one rule, at a take's first read and at any
/// later lift ([`at_lift`]), so the gate applies at the lift in use.
pub fn side_deg(unlock_to_drop_s: f64, osc_period_s: f64, lift_deg: f64) -> Option<f64> {
    in_gate(amplitude_deg(unlock_to_drop_s, osc_period_s, lift_deg))
}

/// `a` if it lies in [`MIN_AMPLITUDE_DEG`] to [`MAX_AMPLITUDE_DEG`].
fn in_gate(a: f64) -> Option<f64> {
    (MIN_AMPLITUDE_DEG..=MAX_AMPLITUDE_DEG)
        .contains(&a)
        .then_some(a)
}

/// The beat error from the unlock, ms: the drop's `beat_error_ms` moved
/// by the difference of the even and odd sides' unlocks (ms), or None
/// unless all three are known. The one rule, at a window's first read
/// and at a new lift ([`at_lift`]).
pub fn unlock_beat_error_ms(
    beat_error_ms: Option<f64>,
    even_unlock_ms: Option<f64>,
    odd_unlock_ms: Option<f64>,
) -> Option<f64> {
    Some(beat_error_ms? + (even_unlock_ms? - odd_unlock_ms?))
}

/// A side at a new lift: its amplitude, its kept edges, its culled edges.
type SideAt = (Option<f64>, Option<[f64; 2]>, Option<[f64; 2]>);

/// `w`, read at `from_lift`, as read at `to_lift`: each side whose edges
/// were found is read again with [`side_deg`] and gated at the new lift,
/// both ways, so a side the old lift culled can come back and one the
/// new lift puts out of range goes (#1333). A side left out as an
/// outlier ([`reject_side_outliers`]: edges kept, no amplitude) is read
/// again too: the caller screens the log for outliers afresh, as a read
/// at that lift would ([`crate::take::kept_at_lift`]). The unlock beat
/// error follows the sides that pass the gate. A
/// side with an amplitude but no edges kept (a made-up window) is scaled
/// by the lifts' ratio and gated the same way.
pub fn at_lift(w: &mut AmplitudeWindow, osc_period_s: f64, from_lift: f64, to_lift: f64) {
    // Each side: (kept edges, culled edges, amplitude), then its new state.
    let sides = [
        (
            w.even_unlock_ms.zip(w.even_drop_ms),
            w.culled()[0],
            w.even_deg,
        ),
        (w.odd_unlock_ms.zip(w.odd_drop_ms), w.culled()[1], w.odd_deg),
    ];
    let mut new: [SideAt; 2] = [(None, None, None); 2];
    let mut changed = false;
    for (k, (kept, culled, deg)) in sides.into_iter().enumerate() {
        let Some([u, d]) = kept.map(|(u, d)| [u, d]).or(culled) else {
            new[k].0 = deg.and_then(|a| in_gate(a * to_lift / from_lift));
            continue;
        };
        new[k] = match side_deg((d - u) / 1000.0, osc_period_s, to_lift) {
            Some(a) => (Some(a), Some([u, d]), None),
            None => (None, None, Some([u, d])),
        };
        changed |= new[k].1.is_some() != kept.is_some();
    }
    let [(ed, ek, ec), (od, ok, oc)] = new;
    w.even_deg = ed;
    w.odd_deg = od;
    (w.even_unlock_ms, w.even_drop_ms) = (ek.map(|e| e[0]), ek.map(|e| e[1]));
    (w.odd_unlock_ms, w.odd_drop_ms) = (ok.map(|e| e[0]), ok.map(|e| e[1]));
    w.set_culled([ec, oc]);
    // As at the first read: the unlock beat error needs both sides' edges.
    if changed {
        w.beat_error_unlock_ms =
            unlock_beat_error_ms(w.beat_error_ms, ek.map(|e| e[0]), ok.map(|e| e[0]));
    }
}

/// `wins`, read at `from_lift`, as read at `to_lift`: each window read
/// again at the new lift ([`at_lift`]), then screened for lone
/// wrong-sound sides once over them ([`reject_side_outliers`]), as a read
/// at that lift is. The one lift change, for a kept take
/// ([`crate::take::kept_at_lift`]) and for live (#1333).
pub fn relift(wins: &mut [AmplitudeWindow], osc_period_s: f64, from_lift: f64, to_lift: f64) {
    for w in wins.iter_mut() {
        at_lift(w, osc_period_s, from_lift, to_lift);
    }
    reject_side_outliers(wins, OUTLIER_SPAN_S);
}

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
    /// A side whose edges were found but whose amplitude fell outside
    /// [`MIN_AMPLITUDE_DEG`] to [`MAX_AMPLITUDE_DEG`] at the lift it was
    /// read with: its unlock and drop, ms from the beat time, kept so a
    /// later lift can read it again ([`at_lift`], #1333). None for a side
    /// kept or never found. Even side first. Boxed and None when neither
    /// side was culled, so a long run's windows stay small. Not in the JSON.
    #[serde(skip)]
    pub culled_ms: Option<Box<Culled>>,
}

/// Each side's culled unlock and drop, ms from the beat time: even, odd.
pub type Culled = [Option<[f64; 2]>; 2];

impl AmplitudeWindow {
    /// The same window with Tick and Tock the other way round, for a
    /// stretch whose beats were numbered from the other side (see
    /// [`crate::breaks::renumber`]).
    pub fn swap_sides(&mut self) {
        std::mem::swap(&mut self.even_deg, &mut self.odd_deg);
        std::mem::swap(&mut self.even_unlock_ms, &mut self.odd_unlock_ms);
        std::mem::swap(&mut self.even_drop_ms, &mut self.odd_drop_ms);
        if let Some(c) = &mut self.culled_ms {
            c.swap(0, 1);
        }
        self.beat_error_ms = self.beat_error_ms.map(|v| -v);
        self.beat_error_unlock_ms = self.beat_error_unlock_ms.map(|v| -v);
    }

    /// Each side's culled edges (see [`AmplitudeWindow::culled_ms`]).
    pub fn culled(&self) -> Culled {
        self.culled_ms.as_deref().copied().unwrap_or_default()
    }

    /// Sets each side's culled edges, boxing them only if one is culled.
    pub fn set_culled(&mut self, c: Culled) {
        self.culled_ms = (c != [None, None]).then(|| Box::new(c));
    }

    pub fn mean(&self) -> Option<f64> {
        mean_of_sides(self.even_deg, self.odd_deg)
    }
}

/// A window's amplitude from its two sides': their mean, and none when
/// only one side was read. Tick and Tock can differ, so one side alone
/// would pull the reading towards it, as if the missing side were filled
/// with the other; that side's own Tick or Tock figure still keeps it
/// (#648).
pub fn mean_of_sides(even_deg: Option<f64>, odd_deg: Option<f64>) -> Option<f64> {
    Some((even_deg? + odd_deg?) / 2.0)
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
    // EARLIER_SOUND_MIN comes before it. Each side's guide is first read
    // on a template that reaches the unlock at MIN_AMPLITUDE_DEG
    // ([`template_pre_s`]); where that unlock lies beyond a PRE_S
    // template's search ([`Guide::Long`]), the side is read on long
    // templates throughout, as one of PRE_S finds the impulse there
    // instead (#1623). The balance swings the same arc each way, so a
    // stretch whose unlock lies that far out does so on both sides alike:
    // the sides go long together, when both long unlocks lie beyond the
    // reach and within [`SIDES_UNLOCK_AGREE_SHARE`] of their mean of each
    // other, or not at all. A faint sound before one side's unlock reads as a long unlock on
    // that side alone (#1703).
    let guide = |even: bool| -> Option<(Option<f64>, Option<f64>)> {
        let times = side_times(beats, even);
        if times.len() < 3 {
            return None;
        }
        let unlock = |pre_s: f64| {
            let origin = (pre_s * fs).round() as usize;
            trimmed_template_from(env, fs, &times, pre_s)
                .and_then(|t| edges(&t, fs, origin, cfg.onset_fraction))
                .map(|e| e.unlock)
        };
        Some((unlock(long_pre_s), unlock(PRE_S)))
    };
    let (even_g, odd_g) = (guide(true), guide(false));
    let long_s = |g: Option<(Option<f64>, Option<f64>)>| {
        g.and_then(|g| g.0)
            .map(|l| (l, (l - long_origin as f64) / fs))
            .filter(|(_, at)| *at < -short_reach_s())
    };
    let long = match (long_s(even_g), long_s(odd_g)) {
        (Some(e), Some(o))
            if (e.1 - o.1).abs() <= SIDES_UNLOCK_AGREE_SHARE * (e.1 + o.1).abs() / 2.0 =>
        {
            Some([e.0, o.0])
        }
        _ => None,
    };
    let pick = |g: Option<(Option<f64>, Option<f64>)>, k: usize| -> Option<Guide> {
        match long {
            Some(l) => Some(Guide::Long(l[k])),
            None => g?.1.map(Guide::Short),
        }
    };
    let from = [pick(even_g, 0), pick(odd_g, 1)];
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
        // The unlock's and drop's offsets from the beat times, seconds,
        // and the amplitude when it passes the gate.
        let side = |even: bool| -> Option<(Option<f64>, f64, f64)> {
            let times = side_times(win, even);
            if times.len() < 3 {
                return None;
            }
            let (unlock, drop) =
                side_edges(env, fs, &times, from[usize::from(!even)], long_pre_s, cfg)?;
            let a = side_deg(drop - unlock, osc_period_s, cfg.lift_deg);
            Some((a, unlock, drop))
        };
        // A side outside the gate keeps only its edges, as culled.
        let culled = |s: Option<(Option<f64>, f64, f64)>| {
            s.filter(|s| s.0.is_none())
                .map(|s| [s.1 * 1000.0, s.2 * 1000.0])
        };
        let (even_all, odd_all) = (side(true), side(false));
        let culled_ms = Some([culled(even_all), culled(odd_all)])
            .filter(|c| c != &[None, None])
            .map(Box::new);
        let kept = |s: Option<(Option<f64>, f64, f64)>| s.and_then(|s| Some((s.0?, s.1, s.2)));
        let (even, odd) = (kept(even_all), kept(odd_all));
        let beat_error_ms = timing::beat_error_ms(win);
        let beat_error_unlock_ms = unlock_beat_error_ms(
            beat_error_ms,
            even.map(|s| s.1 * 1000.0),
            odd.map(|s| s.1 * 1000.0),
        );
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
            culled_ms,
        });
        start += window_s;
    }
    out
}

/// Where a side's guide unlock was read ([`windows_between`]), samples
/// into its template: on one of [`PRE_S`], or of [`template_pre_s`] when
/// the unlock lies beyond the shorter one's search.
#[derive(Debug, Clone, Copy)]
enum Guide {
    Short(f64),
    Long(f64),
}

/// How far before the drop a guide's unlock may sit for a window's search
/// on a [`PRE_S`] template to reach it, seconds: past the 3 ms the floor is
/// taken on and the [`UNLOCK_SEARCH_S`] the search starts before the guide.
fn short_reach_s() -> f64 {
    PRE_S - FLOOR_S - UNLOCK_SEARCH_S
}

/// One side's unlock and drop in a window, seconds from its beat times, on
/// its beats `times`, by its guide: sound 1 from the guide's search, or
/// an earlier one as loud as [`EARLIER_SOUND_MIN`] of it on a template
/// [`template_pre_s`] long. On a long guide, a window whose sound 1 sits
/// later than the guide's by more than [`UNLOCK_LATE_S`] took a later
/// sound, the impulse, and gives none: it is withheld, not timed again
/// from a sound it did not find (#1623).
fn side_edges(
    env: &[f32],
    fs: f64,
    times: &[f64],
    guide: Option<Guide>,
    long_pre_s: f64,
    cfg: &AmplitudeConfig,
) -> Option<(f64, f64)> {
    let long_origin = (long_pre_s * fs).round() as usize;
    let long = trimmed_template_from(env, fs, times, long_pre_s);
    let (tmpl, origin, guide_at) = match guide {
        Some(Guide::Long(g)) => (long.clone()?, long_origin, Some(g)),
        g => {
            let origin = (PRE_S * fs).round() as usize;
            let g = g.map(|g| match g {
                Guide::Short(g) | Guide::Long(g) => g,
            });
            (trimmed_template_from(env, fs, times, PRE_S)?, origin, g)
        }
    };
    let guided = edges_guided(&tmpl, fs, origin, cfg.onset_fraction, guide_at)?;
    let height = |e: &Edges| e.unlock_level - e.floor;
    let at = |i: f64, origin: usize| (i - origin as f64) / fs;
    let earlier = long
        .and_then(|t| edges_after(&t, fs, long_origin, cfg.onset_fraction, 0))
        .filter(|e| {
            at(e.unlock, long_origin) < at(guided.unlock, origin) - 0.0003
                && height(e) >= EARLIER_SOUND_MIN * height(&guided)
        });
    let (unlock, drop) = match earlier {
        Some(e) => (at(e.unlock, long_origin), at(e.drop, long_origin)),
        None => (at(guided.unlock, origin), at(guided.drop, origin)),
    };
    if let Some(Guide::Long(g)) = guide {
        if unlock > at(g, long_origin) + UNLOCK_LATE_S {
            return None;
        }
    }
    Some((unlock, drop))
}

#[cfg(test)]
mod tests {
    /// Only a window with both sides read has an amplitude; a one-sided
    /// window is left out of it, not read as its one side (#648).
    #[test]
    fn a_one_sided_window_has_no_amplitude() {
        use super::mean_of_sides;
        assert_eq!(mean_of_sides(Some(270.0), Some(280.0)), Some(275.0));
        assert_eq!(mean_of_sides(Some(270.0), None), None);
        assert_eq!(mean_of_sides(None, Some(280.0)), None);
        assert_eq!(mean_of_sides(None, None), None);
    }

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
                culled_ms: None,
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

    /// A side outside the gate at the lift it was read with keeps its
    /// edges, and a later lift reads it again from them and gates it
    /// there, both ways; read back at the first lift it is as first read,
    /// whatever lifts came between (#1333).
    #[test]
    fn a_side_culled_at_one_lift_comes_back_at_another() {
        // 28,800 bph, sound 1 at -5.3 ms: about 390° at 52°, over the
        // gate; 338° at 45°.
        let fs = 48_000.0;
        let period = 0.125;
        let mut env = vec![0.01f32; (8.5 * fs) as usize];
        let at = |t: f64| (t * fs).round() as usize;
        let mut beats = Vec::new();
        for k in 0..64i64 {
            let t = 0.1 + k as f64 * period;
            env[at(t - 0.0053)..at(t - 0.005)].fill(0.3);
            env[at(t)..at(t + 0.0003)].fill(1.0);
            beats.push(Beat {
                index: k,
                time: t,
                quality: 1.0,
            });
        }
        let cfg = AmplitudeConfig {
            lift_deg: 52.0,
            ..Default::default()
        };
        let osc = 2.0 * period;
        let first = windows_between(&env, fs, &beats, osc, 2.0, 0.0, 8.0, &cfg);
        let w = &first[1];
        assert_eq!((w.even_deg, w.odd_deg), (None, None), "{w:?}");
        assert_eq!((w.even_unlock_ms, w.beat_error_unlock_ms), (None, None));
        let [u, d] = w.culled()[0].expect("the culled side's edges kept");
        let at52 = amplitude_deg((d - u) / 1000.0, osc, 52.0);
        assert!(at52 > MAX_AMPLITUDE_DEG, "{at52}");
        // At 45° it passes: the same figure a read at 45° gives.
        let mut lower = w.clone();
        at_lift(&mut lower, osc, 52.0, 45.0);
        let fresh = windows_between(
            &env,
            fs,
            &beats,
            osc,
            2.0,
            0.0,
            8.0,
            &AmplitudeConfig {
                lift_deg: 45.0,
                ..Default::default()
            },
        );
        let a = lower.even_deg.expect("back at 45°");
        assert!((a - at52 * 45.0 / 52.0).abs() < 1e-9, "{a}");
        assert!(
            (a - fresh[1].even_deg.unwrap()).abs() < 1.0,
            "{a} {:?}",
            fresh[1].even_deg
        );
        assert_eq!(lower.even_unlock_ms, Some(u));
        assert_eq!(lower.culled()[0], None);
        assert!(lower.beat_error_unlock_ms.is_some());
        // Back to 52°, by way of 12° (under the gate): as first read.
        let mut back = lower.clone();
        at_lift(&mut back, osc, 45.0, 12.0);
        assert_eq!(back.even_deg, None, "under the gate at 12°");
        at_lift(&mut back, osc, 12.0, 52.0);
        assert_eq!(format!("{back:?}"), format!("{w:?}"));
        assert_eq!(back.culled(), w.culled());
    }

    /// Swapping Tick and Tock swaps their culled edges too.
    #[test]
    fn swapping_sides_swaps_the_culled_edges() {
        let mut w = AmplitudeWindow {
            start_s: 0.0,
            end_s: 2.0,
            even_deg: None,
            odd_deg: Some(300.0),
            beat_error_ms: None,
            beat_error_unlock_ms: None,
            even_unlock_ms: None,
            odd_unlock_ms: Some(-7.0),
            even_drop_ms: None,
            odd_drop_ms: Some(0.0),
            culled_ms: None,
        };
        w.set_culled([Some([-5.0, 0.0]), None]);
        w.swap_sides();
        assert_eq!(w.culled(), [None, Some([-5.0, 0.0])]);
        assert_eq!(w.even_unlock_ms, Some(-7.0));
    }

    /// A side left out as an outlier keeps its edges and no amplitude: a
    /// new lift reads it again, for the log to be screened afresh. A
    /// made-up window with no edges is scaled by the lifts' ratio and
    /// gated.
    #[test]
    fn an_outlier_side_is_read_again_at_a_new_lift() {
        let mut w = AmplitudeWindow {
            start_s: 0.0,
            end_s: 2.0,
            even_deg: None,
            odd_deg: Some(300.0),
            beat_error_ms: Some(0.1),
            beat_error_unlock_ms: Some(0.3),
            even_unlock_ms: Some(-7.0),
            odd_unlock_ms: None,
            even_drop_ms: Some(0.0),
            odd_drop_ms: None,
            culled_ms: None,
        };
        at_lift(&mut w, 0.25, 52.0, 56.0);
        let again = side_deg(0.007, 0.25, 56.0);
        assert!(again.is_some());
        assert_eq!(w.even_deg, again);
        assert_eq!(w.even_unlock_ms, Some(-7.0));
        assert!((w.odd_deg.unwrap() - 300.0 * 56.0 / 52.0).abs() < 1e-9);
        assert_eq!(w.beat_error_unlock_ms, Some(0.3), "no side changed state");
        at_lift(&mut w, 0.25, 56.0, 70.0);
        assert_eq!(w.odd_deg, None, "403.8° is over the gate");
    }

    /// 2 s windows of a 28,800 bph watch at `amp` and a 59° lift, `secs`
    /// long, whose sound 1 is faint next to the impulse: the unlock at a
    /// twentieth of the drop's height, the impulse at a quarter, at 0.72
    /// of the way from the drop back to the unlock (as on the 69 h run-down
    /// of #1623), over a floor with a little seeded noise. Also the
    /// amplitude the impulse would read.
    fn faint_unlock(amp: f64, secs: f64, seed: u64) -> (Vec<AmplitudeWindow>, f64) {
        let (fs, period, lift) = (48_000.0, 0.125, 59.0);
        let osc = 2.0 * period;
        let dt = osc / std::f64::consts::PI * (lift / (2.0 * amp)).asin();
        let mut state = seed;
        let mut env: Vec<f32> = (0..((secs + 0.5) * fs) as usize)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                0.01 + 0.004 * ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5)
            })
            .collect();
        let at = |t: f64| (t * fs).round() as usize;
        let mut beats = Vec::new();
        for k in 0..(secs / period) as i64 - 2 {
            let t = 0.1 + k as f64 * period;
            env[at(t - dt)..at(t - dt + 0.0003)].fill(0.06);
            env[at(t - 0.72 * dt)..at(t - 0.72 * dt + 0.0003)].fill(0.26);
            env[at(t)..at(t + 0.0003)].fill(1.0);
            beats.push(Beat {
                index: k,
                time: t,
                quality: 1.0,
            });
        }
        let cfg = AmplitudeConfig {
            lift_deg: lift,
            ..Default::default()
        };
        let w = windows_between(&env, fs, &beats, osc, 2.0, 0.0, secs - 0.5, &cfg);
        (w, amplitude_deg(0.72 * dt, osc, lift))
    }

    /// Near stopping, sound 1 is faint next to the impulse. A side reads
    /// the unlock, within 3°, or nothing; never the impulse (142° on a 69 h
    /// run-down whose unlock sat 23.5 ms before the drop, #1623; Norm's
    /// bands on the issue).
    #[test]
    fn a_faint_unlock_before_a_loud_impulse_is_not_read_as_the_impulse() {
        for (amp, seed) in [(100.0, 11), (115.0, 12), (130.0, 13)] {
            let (w, impulse) = faint_unlock(amp, 20.0, seed);
            assert!(impulse > amp + 20.0, "{amp}°: impulse {impulse}°");
            for w in &w {
                for a in [w.even_deg, w.odd_deg].into_iter().flatten() {
                    assert!((a - amp).abs() < 3.0, "{a}° read at {amp}°: {w:?}");
                }
            }
        }
    }

    /// A healthy watch with the same faint unlock reads as before: under
    /// 1% of its sides withheld and its median within 1° (Norm's bands,
    /// #1623).
    #[test]
    fn a_faint_unlock_on_a_healthy_watch_is_read() {
        for (amp, seed) in [(250.0, 21), (275.0, 22), (300.0, 23)] {
            let (w, _) = faint_unlock(amp, 100.0, seed);
            let mut read: Vec<f64> = w
                .iter()
                .flat_map(|w| [w.even_deg, w.odd_deg])
                .flatten()
                .collect();
            assert!(
                read.len() * 100 > 99 * 2 * w.len(),
                "{amp}°: {} of {}",
                read.len(),
                2 * w.len()
            );
            read.sort_by(f64::total_cmp);
            let median = read[read.len() / 2];
            assert!((median - amp).abs() < 1.0, "{amp}°: median {median}°");
        }
    }

    /// A window that doesn't hear the faint unlock the stretch's other
    /// beats show, and so takes the impulse, is withheld, not read at the
    /// impulse nor timed again from the stretch's unlock (Norm's rule 3,
    /// #1623).
    #[test]
    fn a_window_that_misses_the_stretchs_unlock_is_withheld() {
        let (fs, period, lift) = (48_000.0, 0.125, 59.0);
        let osc = 2.0 * period;
        let mut env = vec![0.01f32; (20.5 * fs) as usize];
        let at = |t: f64| (t * fs).round() as usize;
        let mut beats = Vec::new();
        for k in 0..160i64 {
            let t = 0.1 + k as f64 * period;
            if !(6.0..8.0).contains(&t) {
                env[at(t - 0.021)..at(t - 0.0207)].fill(0.06);
            }
            env[at(t - 0.015)..at(t - 0.0147)].fill(0.26);
            env[at(t)..at(t + 0.0003)].fill(1.0);
            beats.push(Beat {
                index: k,
                time: t,
                quality: 1.0,
            });
        }
        let cfg = AmplitudeConfig {
            lift_deg: lift,
            ..Default::default()
        };
        let w = windows_between(&env, fs, &beats, osc, 2.0, 0.0, 20.0, &cfg);
        let missed = w.iter().find(|w| w.start_s == 6.0).unwrap();
        assert_eq!(
            (missed.even_deg, missed.odd_deg),
            (None, None),
            "{missed:?}"
        );
        assert_eq!(missed.culled(), [None, None], "no edges kept to read again");
        let heard = w.iter().find(|w| w.start_s == 10.0).unwrap();
        let want = amplitude_deg(0.021, osc, lift);
        assert!((heard.even_deg.unwrap() - want).abs() < 3.0, "{heard:?}");
    }

    /// 2 s windows of a 28,800 bph watch at `lift` whose unlock sits
    /// 13 ms before the drop on both sides (0.28 of the drop's height), its
    /// impulse at 0.7 of that (0.06), over a floor with seeded noise, with
    /// a faint bump at a twentieth of the drop's height on the Tick from
    /// `tick_bump` and on the Tock from `tock_bump` (seconds before the
    /// drop, start and end), if given. Checks that both sides read the
    /// unlock, within 3°, and that the unlock beat error stays near the
    /// drop's.
    fn reads_past_bumps(
        seed: u64,
        lift: f64,
        tick_bump: Option<[f64; 2]>,
        tock_bump: Option<[f64; 2]>,
    ) {
        let (fs, period, secs) = (48_000.0, 0.125, 30.0);
        let osc = 2.0 * period;
        let dt = 0.013;
        let mut state = seed;
        let mut env: Vec<f32> = (0..((secs + 0.5) * fs) as usize)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                0.016 + 0.004 * ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5)
            })
            .collect();
        let at = |t: f64| (t * fs).round() as usize;
        let mut beats = Vec::new();
        for k in 0..(secs / period) as i64 - 2 {
            let t = 0.1 + k as f64 * period;
            if let Some([a, b]) = if k % 2 == 0 { tick_bump } else { tock_bump } {
                env[at(t - a)..at(t - b)].fill(0.05);
            }
            env[at(t - dt)..at(t - dt + 0.0003)].fill(0.28);
            env[at(t - 0.7 * dt)..at(t - 0.7 * dt + 0.0003)].fill(0.06);
            env[at(t)..at(t + 0.0003)].fill(1.0);
            beats.push(Beat {
                index: k,
                time: t,
                quality: 1.0,
            });
        }
        let cfg = AmplitudeConfig {
            lift_deg: lift,
            ..Default::default()
        };
        let w = windows_between(&env, fs, &beats, osc, 2.0, 0.0, secs - 0.5, &cfg);
        let want = amplitude_deg(dt, osc, lift);
        assert!(!w.is_empty());
        for w in &w {
            for a in [w.even_deg, w.odd_deg] {
                let a = a.unwrap_or_else(|| panic!("a side withheld: {w:?}"));
                assert!((a - want).abs() < 3.0, "{a}° read at {want}°: {w:?}");
            }
            let be = w.beat_error_unlock_ms.unwrap() - w.beat_error_ms.unwrap();
            assert!(be.abs() < 0.1, "unlock beat error {be} ms off: {w:?}");
        }
    }

    /// A faint sound before one side's unlock is not read as its unlock:
    /// the sides go on long templates together or not at all (#1703, as on
    /// Peter's Submariner at Crown Down, whose Tock read 119° and its beat
    /// error 4.7 ms). The bump is on the Tock only, from 22 to 20.5 ms
    /// before the drop: before a 20 ms template starts, so only the long
    /// template sees it.
    #[test]
    fn a_faint_sound_before_one_sides_unlock_is_not_read_as_it() {
        reads_past_bumps(1705, 53.0, None, Some([0.022, 0.0205]));
    }

    /// Bumps on both sides that the long templates read as unlocks beyond
    /// the 20 ms template's reach, but further apart than
    /// [`SIDES_UNLOCK_AGREE_SHARE`] of their mean (the Tick's from 25.5 ms,
    /// the Tock's from 20.9 ms before the drop, about 20%; at a 59° lift
    /// the long template's floor ends at 25.85 ms): one balance can't swing
    /// both, so neither side goes long (#1703, Trevor's review).
    #[test]
    fn long_unlocks_that_disagree_keep_both_sides_short() {
        reads_past_bumps(1706, 59.0, Some([0.0255, 0.025]), Some([0.0209, 0.0204]));
    }
}
