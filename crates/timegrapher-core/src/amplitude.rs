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
    /// The escapement the take is read for (#2313): on a co-axial a
    /// window's unlock beat error does not depend on the amplitude gate
    /// ([`screen_unlock_beat_error`]).
    pub escapement: crate::escapement::Escapement,
}

impl Default for AmplitudeConfig {
    fn default() -> Self {
        AmplitudeConfig {
            lift_deg: crate::take::DEFAULT_LIFT_DEG,
            onset_fraction: 0.02,
            escapement: crate::escapement::Escapement::SwissLever,
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

/// A side whose sound 1 stands under this share of the median height
/// over the floor of that side's sound 1 in the same chunk is withheld,
/// edges and all: the side was too faint in that window for its unlock to
/// be told from noise, and the edge found there is the loudest bump of
/// noise, which the lift then decides between (#1738). Set from the gap
/// in the training data: the faint windows of Peter's Submariner at Crown
/// Down and Crown Up reach 0.228 at the most, and the lowest share outside
/// them is 0.336, on six sides of dd3235_DU_30min part 1. The line is the
/// gap's log-midpoint, √(0.228 × 0.336), an equal factor of about 1.2 from
/// each side, and was frozen before any held-out run (Norm's ruling).
///
/// Its limit: it catches a side under 0.28 of its chunk median; a misread
/// louder than that passes. On seeded weak watches whose noise sides read
/// an edge at 0.2 to 0.4 of the median, it withheld 48% of them with 20%
/// of the windows noise, 22% with 40% and 5% with 60%, as noise pulls the
/// median down: the rule fades, with no cliff. It withheld none of the
/// healthy sides, the lowest at 0.6 (Peter chose to ship with this limit
/// stated, #1738; the gap is #1791). Pinned by
/// `noise_windows_are_withheld_and_healthy_ones_kept` and
/// `faint_windows_in_the_majority_are_not_withheld`.
pub const FAINT_SOUND_MAX_SHARE: f32 = 0.28;

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
pub(crate) fn in_gate(a: f64) -> Option<f64> {
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
/// were found is read again and gated at the new lift (one with an
/// amplitude scaled by the lifts' ratio, one without read with
/// [`side_deg`] at `osc_period_s`),
/// both ways, so a side the old lift culled can come back and one the
/// new lift puts out of range goes (#1333). A side left out as an
/// outlier ([`reject_side_outliers`]: edges kept, no amplitude) is read
/// again too: the caller screens the log for outliers afresh, as a read
/// at that lift would ([`crate::take::kept_at_lift`]). The unlock beat
/// error counts each side found, kept or culled, unless both are culled,
/// so it moves only where that changes (#1977); [`relift`] screens it
/// after. A
/// side with an amplitude but no edges kept (a made-up window) is scaled
/// by the lifts' ratio and gated the same way.
pub fn at_lift(
    w: &mut AmplitudeWindow,
    osc_period_s: f64,
    from_lift: f64,
    to_lift: f64,
    escapement: crate::escapement::Escapement,
) {
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
        // An amplitude is in proportion to the lift, so a side read at
        // one is scaled, at whatever period it was read; a side culled or
        // left out is read from its edges at `osc_period_s` (#2228).
        let read = match (kept, deg) {
            (Some(_), Some(a)) => in_gate(a * to_lift / from_lift),
            _ => side_deg((d - u) / 1000.0, osc_period_s, to_lift),
        };
        new[k] = match read {
            Some(a) => (Some(a), Some([u, d]), None),
            None => (None, None, Some([u, d])),
        };
        changed |= new[k].1.is_some() != kept.is_some();
    }
    let [(ed, ek, ec), (od, ok, oc)] = new;
    w.even_deg = ed;
    w.odd_deg = od;
    // Its sides as read at the new lift; the caller screens them again.
    w.edit_extra(|x| {
        x.outlier_deg = [None; 2];
        x.sides_moved = [false; 2];
        x.sides_disagree = false;
    });
    (w.even_unlock_ms, w.even_drop_ms) = (ek.map(|e| e[0]), ek.map(|e| e[1]));
    (w.odd_unlock_ms, w.odd_drop_ms) = (ok.map(|e| e[0]), ok.map(|e| e[1]));
    w.set_culled([ec, oc]);
    // As at the first read: each side found counts, kept or culled,
    // unless both are culled on a Swiss lever (#1977, #2313). [`relift`]
    // screens it after, and that screen decides: this clause only saves
    // the work.
    if changed {
        let unheard = ek.is_none() && ok.is_none() && escapement.both_culled_unheard();
        w.beat_error_unlock_ms = (!unheard)
            .then(|| {
                unlock_beat_error_ms(
                    w.beat_error_ms,
                    ek.or(ec).map(|e| e[0]),
                    ok.or(oc).map(|e| e[0]),
                )
            })
            .flatten();
    }
}

/// `wins`, read at `from_lift`, as read at `to_lift`: each window read
/// again at the new lift ([`at_lift`]), then screened for lone
/// wrong-sound sides once over them ([`reject_side_outliers`]), as a read
/// at that lift is. The one lift change, for a kept take
/// ([`crate::take::kept_at_lift`]) and for live (#1333).
pub fn relift(
    wins: &mut [AmplitudeWindow],
    osc_period_s: f64,
    from_lift: f64,
    to_lift: f64,
    escapement: crate::escapement::Escapement,
) {
    for w in wins.iter_mut() {
        at_lift(w, osc_period_s, from_lift, to_lift, escapement);
    }
    reject_side_outliers(wins, OUTLIER_SPAN_S, escapement);
}

/// The lift the unlock search is sized for, degrees: the largest lift in
/// the calibre table ([`crate::calibres::all`], 70°). The search and its
/// floor are the same at every lift, so the unlock, and the beat error
/// from it, are too; the lift enters only in turning the unlock-to-drop
/// time into degrees (#1695). Sizing for the largest lift a take can be
/// read at (90°, [`crate::input_domain::LIFT_RANGE_DEG`]) starts the
/// search 42 ms before the drop at 28,800 bph, where it found an earlier
/// sound on the ETA 2824 holder take and lost its amplitude and beat
/// error. At a lift over 70° the search reaches the unlock down to about
/// 100° × lift / 70.
pub const SEARCH_LIFT_DEG: f64 = 70.0;

/// Seconds before the drop a template must start for sound 1 to be found
/// at [`MIN_AMPLITUDE_DEG`] at this oscillation period and the
/// [`SEARCH_LIFT_DEG`] lift: the unlock, after the 3 ms the floor is taken
/// on and a margin for its edge; never less than [`PRE_S`]. At 18,000 bph
/// and a 52° lift, 180° puts the unlock 18.5 ms before the drop, inside
/// the floor of a 20 ms template.
pub fn template_pre_s(osc_period_s: f64) -> f64 {
    let s = (SEARCH_LIFT_DEG / (2.0 * MIN_AMPLITUDE_DEG)).clamp(0.0, 1.0);
    let unlock_s = osc_period_s / std::f64::consts::PI * s.asin();
    (unlock_s + 0.005).max(PRE_S)
}

/// Where a side's unlock is read: which template, and how far before the
/// beat it starts. The one rule behind amplitude's guides and the Tick
/// and Tock profile (#1924, Norm's ruling): at a low amplitude the unlock
/// lies beyond what a [`PRE_S`] template's search reaches
/// ([`short_reach_s`]), where that search finds the impulse instead, and
/// it is read on a template [`template_pre_s`] long.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnlockWindow {
    /// Whether the unlock is read on the long template.
    pub long: bool,
    /// Seconds before the beat the template starts.
    pub pre_s: f64,
}

impl UnlockWindow {
    /// The [`PRE_S`] template, where the unlock lies within its search.
    pub const SHORT: UnlockWindow = UnlockWindow {
        long: false,
        pre_s: PRE_S,
    };

    /// The template that reaches the unlock down to [`MIN_AMPLITUDE_DEG`]
    /// at this oscillation period ([`template_pre_s`]).
    pub fn long(osc_period_s: f64) -> UnlockWindow {
        UnlockWindow {
            long: true,
            pre_s: template_pre_s(osc_period_s),
        }
    }

    /// The window for the two sides whose unlocks lie `unlocks_s`
    /// seconds from their beat times (even, odd; None where not found):
    /// long when both lie beyond [`short_reach_s`] before it and within
    /// [`SIDES_UNLOCK_AGREE_SHARE`] of their mean distance of each other.
    /// The balance swings the same arc each way, so the sides go long
    /// together or not at all (#1703).
    pub fn of_unlocks(osc_period_s: f64, unlocks_s: [Option<f64>; 2]) -> UnlockWindow {
        if sides_go_long(unlocks_s) {
            UnlockWindow::long(osc_period_s)
        } else {
            UnlockWindow::SHORT
        }
    }

    /// The beat's sample in a window of this template.
    pub fn origin(self, fs: f64) -> usize {
        (self.pre_s * fs).round() as usize
    }
}

/// Whether `new`'s windows read both sides in a lower share of its 2 s
/// windows than `old`'s did over the stretch `new` read, by more than 2
/// standard errors ([`crate::confidence::share_differs`]): a template
/// built from a file's first chunk reading a sound worse than the one a
/// short window gave. Live keeps the template it locked on with then
/// (#1500), and a file's reading the short window's (#1503), so both read
/// such sound alike.
pub(crate) fn reads_fewer_sides(old: &[AmplitudeWindow], new: &[AmplitudeWindow]) -> bool {
    let Some(from) = new.first().map(|w| w.start_s) else {
        return false;
    };
    let count = |ws: &[AmplitudeWindow]| {
        let ws: Vec<_> = ws.iter().filter(|w| w.start_s >= from - 1e-6).collect();
        let both = ws.iter().filter(|w| w.mean().is_some()).count();
        (both as f64, ws.len() as f64)
    };
    let ((k1, n1), (k2, n2)) = (count(old), count(new));
    crate::confidence::share_differs(k1, n1, k2, n2) == Some(false)
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
    /// What a few windows carry besides (see [`Extra`]): boxed, and None
    /// when it is all empty, so a long run's windows stay small. Not in
    /// the JSON.
    #[serde(skip)]
    pub extra: Option<Box<Extra>>,
    /// Each side's sound 1, its height over the floor, as read in this
    /// window, for [`withhold_faint_by_chunk`]; NaN where none was found
    /// ([`AmplitudeWindow::sound_1_of`]). Even side first. Not in the
    /// JSON.
    #[serde(skip)]
    pub sound_1: [f32; 2],
}

/// Each side's culled unlock and drop, ms from the beat time: even, odd.
pub type Culled = [Option<[f64; 2]>; 2];

/// What only a few amplitude windows carry, even side first in each.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Extra {
    /// A side whose edges were found but whose amplitude fell outside
    /// [`MIN_AMPLITUDE_DEG`] to [`MAX_AMPLITUDE_DEG`] at the lift it was
    /// read with: its unlock and drop, ms from the beat time, kept so a
    /// later lift can read it again ([`at_lift`], #1333). None for a side
    /// kept or never found.
    pub culled_ms: Culled,
    /// Each side's amplitude, degrees, as read before
    /// [`reject_side_outliers`] left it out; None for a side it kept.
    /// Kept so a later screen scores every neighbour as first read, and
    /// can judge the same window again (#1738).
    pub outlier_deg: [Option<f64>; 2],
    /// Each side withheld because its sound 1 was too faint against the
    /// same side's in its chunk ([`withhold_faint_by_chunk`], #1738): it
    /// keeps no edges and no amplitude, and is named in the caveats.
    pub faint_withheld: [bool; 2],
    /// The window's two sides' unlock-to-drop times disagree
    /// ([`sides_agree`], Norm's ruling on #1788): one side's edge is on the
    /// wrong sound, so it gives no unlock beat error and no mean
    /// amplitude, and both sides' amplitudes are left out
    /// ([`screen_unlock_beat_error`]).
    pub sides_disagree: bool,
    /// Each side whose amplitude that screen moved to
    /// [`Extra::outlier_deg`], so a screen run again with a new reference
    /// can put it back.
    pub sides_moved: [bool; 2],
}

impl Extra {
    fn swap(&mut self) {
        self.culled_ms.swap(0, 1);
        self.outlier_deg.swap(0, 1);
        self.faint_withheld.swap(0, 1);
        self.sides_moved.swap(0, 1);
    }
}

impl AmplitudeWindow {
    /// The same window with Tick and Tock the other way round, for a
    /// stretch whose beats were numbered from the other side (see
    /// [`crate::breaks::renumber`]).
    pub fn swap_sides(&mut self) {
        std::mem::swap(&mut self.even_deg, &mut self.odd_deg);
        std::mem::swap(&mut self.even_unlock_ms, &mut self.odd_unlock_ms);
        std::mem::swap(&mut self.even_drop_ms, &mut self.odd_drop_ms);
        if let Some(x) = &mut self.extra {
            x.swap();
        }
        self.sound_1.swap(0, 1);
        self.beat_error_ms = self.beat_error_ms.map(|v| -v);
        self.beat_error_unlock_ms = self.beat_error_unlock_ms.map(|v| -v);
    }

    /// Withhold side `k` (0 even, 1 odd) as too faint to read: no
    /// amplitude, no edges, culled or kept, and so no unlock beat error
    /// (#1738).
    fn withhold_side(&mut self, k: usize) {
        let [deg, unlock, drop] = if k == 0 {
            [
                &mut self.even_deg,
                &mut self.even_unlock_ms,
                &mut self.even_drop_ms,
            ]
        } else {
            [
                &mut self.odd_deg,
                &mut self.odd_unlock_ms,
                &mut self.odd_drop_ms,
            ]
        };
        (*deg, *unlock, *drop) = (None, None, None);
        self.beat_error_unlock_ms = None;
        self.edit_extra(|x| {
            x.culled_ms[k] = None;
            x.outlier_deg[k] = None;
            x.sides_moved[k] = false;
            x.faint_withheld[k] = true;
        });
    }

    /// What the window carries besides ([`Extra`]), empty for most.
    pub fn extra(&self) -> Extra {
        self.extra.as_deref().copied().unwrap_or_default()
    }

    /// Changes [`AmplitudeWindow::extra`], boxing it only if it isn't
    /// empty.
    pub fn edit_extra(&mut self, f: impl FnOnce(&mut Extra)) {
        let mut x = self.extra();
        f(&mut x);
        self.extra = (x != Extra::default()).then(|| Box::new(x));
    }

    /// Both sides' unlocks as read, seconds from the beat time, when
    /// both were.
    fn unlocks_s(&self) -> Option<[Option<f64>; 2]> {
        let (e, o) = (self.even_unlock_ms?, self.odd_unlock_ms?);
        Some([Some(e / 1000.0), Some(o / 1000.0)])
    }

    /// Whether both sides' edges were found, kept or culled.
    pub fn found_both(&self) -> bool {
        matches!(edges_found(self), [Some(_), Some(_)])
    }

    /// Whether the window's two sides disagree ([`Extra::sides_disagree`]).
    pub fn sides_disagree(&self) -> bool {
        self.extra().sides_disagree
    }

    /// Each side withheld as too faint ([`Extra::faint_withheld`]).
    pub fn faint_withheld(&self) -> [bool; 2] {
        self.extra().faint_withheld
    }

    /// Side `k`'s sound 1 height, if one was found
    /// ([`AmplitudeWindow::sound_1`]).
    pub fn sound_1_of(&self, k: usize) -> Option<f32> {
        Some(self.sound_1[k]).filter(|h| !h.is_nan())
    }

    /// Each side's amplitude left out as an outlier
    /// ([`Extra::outlier_deg`]).
    pub fn outliers(&self) -> [Option<f64>; 2] {
        self.extra().outlier_deg
    }

    /// Each side's amplitude as read, before any outlier screen.
    fn deg_as_read(&self) -> [Option<f64>; 2] {
        let o = self.outliers();
        [self.even_deg.or(o[0]), self.odd_deg.or(o[1])]
    }

    /// Each side's culled edges ([`Extra::culled_ms`]).
    pub fn culled(&self) -> Culled {
        self.extra().culled_ms
    }

    /// Whether either side has culled edges.
    pub fn has_culled(&self) -> bool {
        self.culled() != [None, None]
    }

    /// Sets each side's culled edges.
    pub fn set_culled(&mut self, c: Culled) {
        self.edit_extra(|x| x.culled_ms = c);
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
pub fn reject_side_outliers(
    wins: &mut [AmplitudeWindow],
    span_s: f64,
    escapement: crate::escapement::Escapement,
) -> usize {
    let n = reject_side_outliers_from(wins, span_s, f64::NEG_INFINITY);
    screen_unlock_beat_error(wins, escapement);
    n
}

/// Each side's edges as found, kept or culled: its unlock and drop, ms
/// from the beat time.
fn edges_found(w: &AmplitudeWindow) -> [Option<[f64; 2]>; 2] {
    let c = w.culled();
    [
        w.even_unlock_ms
            .zip(w.even_drop_ms)
            .map(|(u, d)| [u, d])
            .or(c[0]),
        w.odd_unlock_ms
            .zip(w.odd_drop_ms)
            .map(|(u, d)| [u, d])
            .or(c[1]),
    ]
}

/// The unlock beat error of each window of `wins` with both sides' edges
/// found, kept or culled, and at least one kept (#1977), read again: from
/// every such side whose
/// unlock-to-drop time is within its own side's far fences over `wins`
/// ([`crate::dsp::far_fences`]), and none in a window where a side is
/// outside them, an edge marked on the wrong sound. On a Swiss lever a
/// window with both sides culled gives none: no unlock was heard in it,
/// and the drop's beat error stands. Otherwise the amplitude gate plays
/// no part, so a lift moves it only where a window's last kept side is
/// culled or its first comes back. On a co-axial the gate plays no part
/// at all (#2313, [`crate::escapement::Escapement::both_culled_unheard`]):
/// its unlock-to-drop time is shorter than a lever's at the same
/// amplitude, so a side out of the gate says nothing about its edges, and
/// the far fences alone screen them.
///
/// A window whose two sides' unlock-to-drop times disagree against the
/// escapement's reference over `wins`, their ratio beyond
/// [`SIDES_DISAGREE_RATIO`] of it either way ([`sides_agree`],
/// [`crate::escapement::Escapement::sides_reference`]) has one side's edge
/// on the wrong sound, and the times can't say which: it gives no unlock
/// beat error, and both sides' amplitudes are left out
/// ([`Extra::outlier_deg`], edges kept), so it gives no mean amplitude
/// either. Cull, never fill (Norm's ruling on #1788, #1707). A window
/// judged so by an earlier screen is judged afresh. Windows without both
/// sides' edges are left as they are. Returns how many sides were
/// outside the fences.
pub fn screen_unlock_beat_error(
    wins: &mut [AmplitudeWindow],
    escapement: crate::escapement::Escapement,
) -> usize {
    for w in wins.iter_mut() {
        put_back_sides(w);
    }
    let found: Vec<[Option<[f64; 2]>; 2]> = wins.iter().map(edges_found).collect();
    let fences = [0, 1].map(|k| {
        let mut v: Vec<f64> = found
            .iter()
            .filter_map(|f| f[k].map(|[u, d]| d - u))
            .filter(|x| x.is_finite())
            .collect();
        v.sort_by(f64::total_cmp);
        crate::dsp::far_fences(&v)
    });
    let inside = |k: usize, e: [f64; 2]| {
        fences[k].is_some_and(|(lo, hi)| (lo..=hi).contains(&(e[1] - e[0])))
    };
    let ratios: Vec<f64> = found.iter().filter_map(|f| sides_ratio(*f)).collect();
    let r0 = escapement.sides_reference(&ratios);
    let mut n = 0;
    for (w, f) in wins.iter_mut().zip(found) {
        let [Some(e), Some(o)] = f else { continue };
        // Both sides culled on a lever: no unlock heard at all, so none
        // (#1977).
        if w.even_unlock_ms.is_none()
            && w.odd_unlock_ms.is_none()
            && escapement.both_culled_unheard()
        {
            w.beat_error_unlock_ms = None;
            continue;
        }
        let ok = [inside(0, e), inside(1, o)];
        n += ok.iter().filter(|&&k| !k).count();
        let share = share_of_ratio(SIDES_DISAGREE_RATIO);
        if r0.is_some_and(|r0| !sides_agree(e[1] - e[0], o[1] - o[0], r0, share)) {
            withhold_sides(w);
            continue;
        }
        w.beat_error_unlock_ms = if ok == [true, true] {
            unlock_beat_error_ms(w.beat_error_ms, Some(e[0]), Some(o[0]))
        } else {
            None
        };
    }
    n
}

/// A window's two sides' unlock-to-drop times' ratio, Tick over Tock, from
/// their edges as found (`f`, [`edges_found`]); none where a side's isn't
/// a finite time after its unlock.
fn sides_ratio(f: [Option<[f64; 2]>; 2]) -> Option<f64> {
    let [Some(e), Some(o)] = f else { return None };
    let (e, o) = (e[1] - e[0], o[1] - o[0]);
    let r = e / o;
    (e > 0.0 && o > 0.0 && r.is_finite()).then_some(r)
}

/// Withhold a window whose two sides disagree
/// ([`screen_unlock_beat_error`]): no unlock beat error, and each side's
/// amplitude moved to [`Extra::outlier_deg`], marked so
/// [`put_back_sides`] can move it back.
fn withhold_sides(w: &mut AmplitudeWindow) {
    w.beat_error_unlock_ms = None;
    let deg = [w.even_deg.take(), w.odd_deg.take()];
    w.edit_extra(|x| {
        x.sides_disagree = true;
        for (k, a) in deg.into_iter().enumerate() {
            if let Some(a) = a {
                x.outlier_deg[k] = Some(a);
                x.sides_moved[k] = true;
            }
        }
    });
}

/// Undo [`withhold_sides`]: each side it moved to
/// [`Extra::outlier_deg`] back to its amplitude, so a screen run again
/// judges the window afresh. The unlock beat error is the screen's to give.
fn put_back_sides(w: &mut AmplitudeWindow) {
    let x = w.extra();
    if !x.sides_disagree && x.sides_moved == [false; 2] {
        return;
    }
    let back = [0, 1].map(|k| x.outlier_deg[k].filter(|_| x.sides_moved[k]));
    w.even_deg = w.even_deg.or(back[0]);
    w.odd_deg = w.odd_deg.or(back[1]);
    w.edit_extra(|x| {
        for k in 0..2 {
            if x.sides_moved[k] {
                x.outlier_deg[k] = None;
            }
        }
        x.sides_moved = [false; 2];
        x.sides_disagree = false;
    });
}

/// `wins`, kept for either escapement, as read for `escapement` (#2313):
/// screened again ([`screen_unlock_beat_error`]) for it, so the two reads
/// differ only as the screen does: a window with both sides culled gives
/// no unlock beat error on a lever, and the sides are judged against the
/// escapement's own reference (#1788). A log read for the same
/// escapement is left as it was.
pub fn kept_for(wins: &mut [AmplitudeWindow], escapement: crate::escapement::Escapement) {
    screen_unlock_beat_error(wins, escapement);
}

/// [`reject_side_outliers`] for the windows of `wins` whose middle is at
/// `from_s` or later, each judged afresh against all of `wins` as read
/// (a neighbour left out before still counts as read), so a run judged in
/// parts, as live does, leaves out the same sides as one judged whole
/// (#1738). The windows before `from_s` are left as they are.
pub fn reject_side_outliers_from(wins: &mut [AmplitudeWindow], span_s: f64, from_s: f64) -> usize {
    let mid = |w: &AmplitudeWindow| (w.start_s + w.end_s) / 2.0;
    let first = wins.partition_point(|w| mid(w) < from_s);
    let read: Vec<[Option<f64>; 2]> = wins.iter().map(AmplitudeWindow::deg_as_read).collect();
    // Each side's distance from its neighbours' median, degrees, and in
    // robust SDs.
    let score = |k: usize| -> Vec<Option<(f64, f64)>> {
        (first..wins.len())
            .map(|i| {
                let v = read[i][k]?;
                let lo = wins.partition_point(|w| mid(w) < mid(&wins[i]) - span_s / 2.0);
                let hi = wins.partition_point(|w| mid(w) <= mid(&wins[i]) + span_s / 2.0);
                let near: Vec<f64> = (lo..hi)
                    .filter(|&j| j != i)
                    .filter_map(|j| read[j][k])
                    .collect();
                if near.len() < 5 {
                    return None;
                }
                let med = crate::dsp::median(&mut near.clone());
                let sd = crate::dsp::robust_sd_about(&near, med).max(0.5);
                Some(((v - med).abs(), (v - med).abs() / sd))
            })
            .collect()
    };
    let (even, odd) = (score(0), score(1));
    let lone = |a: Option<(f64, f64)>, b: Option<(f64, f64)>| matches!((a, b), (Some(a), Some(b)) if a.1 > 5.0 && b.0 < 0.25 * a.0);
    let mut n = 0;
    for (i, w) in wins[first..].iter_mut().enumerate() {
        let out = [lone(even[i], odd[i]), lone(odd[i], even[i])];
        let r = read[first + i];
        [w.even_deg, w.odd_deg] = [0, 1].map(|k| r[k].filter(|_| !out[k]));
        w.edit_extra(|x| {
            x.outlier_deg = [0, 1].map(|k| r[k].filter(|_| out[k]));
            x.sides_moved = [false; 2];
        });
        n += usize::from(out[0]) + usize::from(out[1]);
    }
    n
}

/// Amplitude in consecutive windows of `window_s` seconds from `from_s`,
/// up to the last window that ends by `to_s`. Beat times and the window
/// bounds are in the envelope's time base. The templates are cut for the
/// set rate's swing `osc[0]` and the unlock-to-drop times turned into
/// degrees at the balance's own swing `osc[1]`, seconds (#2228): where
/// the sound is looked for is the set rate's, so a live take and a saved
/// one cut it alike, and the amplitude is the balance's. `span` is the
/// window length, from and to, seconds.
pub fn windows_between_at(
    env: &[f32],
    fs: f64,
    beats: &[Beat],
    osc: [f64; 2],
    (window_s, from_s, to_s): (f64, f64, f64),
    cfg: &AmplitudeConfig,
) -> Vec<AmplitudeWindow> {
    let [osc_period_s, own_osc_s] = osc;
    let mut out = Vec::new();
    let long_window = UnlockWindow::long(osc_period_s);
    let long_pre_s = long_window.pre_s;
    let long_origin = long_window.origin(fs);
    let side_times = good_side_times;
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
    // the sides go long together ([`UnlockWindow::of_unlocks`]), when both
    // long unlocks lie beyond the
    // reach and within [`SIDES_UNLOCK_AGREE_SHARE`] of their mean distance
    // from the drop of each other, or not at all. A faint sound before one side's unlock reads as a long unlock on
    // that side alone (#1703).
    let guide = |even: bool| -> Option<(Option<f64>, Option<f64>)> {
        let times = side_times(beats, even);
        if times.len() < 3 {
            return None;
        }
        let unlock = |pre_s: f64| template_unlock(env, fs, &times, pre_s, cfg);
        Some((unlock(long_pre_s), unlock(UnlockWindow::SHORT.pre_s)))
    };
    let (even_g, odd_g) = (guide(true), guide(false));
    let long_l = |g: Option<(Option<f64>, Option<f64>)>| g.and_then(|g| g.0);
    let at = |l: Option<f64>| l.map(|l| (l - long_origin as f64) / fs);
    let long = match (long_l(even_g), long_l(odd_g)) {
        (Some(e), Some(o))
            if UnlockWindow::of_unlocks(osc_period_s, [at(Some(e)), at(Some(o))]).long =>
        {
            Some([e, o])
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
    // Each window's beats, and each side's read in it.
    let mut reads = Vec::new();
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
        let side = |even: bool| -> Option<SideEdges> {
            let times = side_times(win, even);
            if times.len() < 3 {
                return None;
            }
            side_edges(env, fs, &times, from[usize::from(!even)], long_pre_s, cfg)
        };
        reads.push((start, lo..hi, [side(true), side(false)]));
        start += window_s;
    }
    for (start, span, sides) in reads {
        out.push(window_of(
            &beats[span],
            (start, start + window_s),
            &sides,
            own_osc_s,
            cfg,
        ));
    }
    out
}

/// The template a profile of the sound up to `t` seconds is cut on
/// (#1924): [`UnlockWindow::of_unlocks`] of where the latest of `wins`
/// (in time order) to end by `t` that read both sides put their unlocks,
/// or before any has, the first after. The profile goes long where the
/// amplitude reading found the unlock beyond the short window's reach.
/// With no such window (none yet, at the start of a take), the short one.
pub fn profile_window_at(wins: &[AmplitudeWindow], t: f64, osc_period_s: f64) -> UnlockWindow {
    let by = wins.partition_point(|w| w.end_s <= t + 1e-9);
    let read = wins[..by]
        .iter()
        .rev()
        .find_map(AmplitudeWindow::unlocks_s)
        .or_else(|| wins[by..].iter().find_map(AmplitudeWindow::unlocks_s));
    read.map_or(UnlockWindow::SHORT, |u| {
        UnlockWindow::of_unlocks(osc_period_s, u)
    })
}

/// Withhold, edges and all, each side whose sound 1 stands under
/// [`FAINT_SOUND_MAX_SHARE`] of the median of the same side's in its
/// chunk (#1738): `chunk_of` names a window's chunk from its start,
/// seconds, as [`crate::stream::amplitude_chunk_of`] does, the one grid
/// a saved read and live both group by. `wins` in time order. A side
/// withheld keeps its sound 1, so a second call withholds the same.
pub fn withhold_faint_by_chunk(wins: &mut [AmplitudeWindow], chunk_of: impl Fn(f64) -> usize) {
    let mut a = 0;
    while a < wins.len() {
        let k = chunk_of(wins[a].start_s);
        let b = a + wins[a..].partition_point(|w| chunk_of(w.start_s) == k);
        withhold_faint(&mut wins[a..b]);
        a = b;
    }
}

/// [`withhold_faint_by_chunk`] for the windows of one chunk.
fn withhold_faint(wins: &mut [AmplitudeWindow]) {
    for k in 0..2 {
        let heights: Vec<Option<f32>> = wins.iter().map(|w| w.sound_1_of(k)).collect();
        for (w, faint) in wins.iter_mut().zip(faint_sides(&heights)) {
            if faint {
                w.withhold_side(k);
            }
        }
    }
}

/// One window of [`windows_between_at`] from its beats `win`, its start and
/// end, seconds, and each side's read in it, `sides`.
fn window_of(
    win: &[Beat],
    span_s: (f64, f64),
    sides: &[Option<SideEdges>; 2],
    osc_period_s: f64,
    cfg: &AmplitudeConfig,
) -> AmplitudeWindow {
    // The unlock's and drop's offsets from the beat times, seconds,
    // and the amplitude when it passes the gate.
    let side = |k: usize| -> Option<(Option<f64>, f64, f64)> {
        let (unlock, drop) = sides[k].as_ref()?.edges?;
        let a = side_deg(drop - unlock, osc_period_s, cfg.lift_deg);
        Some((a, unlock, drop))
    };
    // A side outside the gate keeps only its edges, as culled.
    let culled = |s: Option<(Option<f64>, f64, f64)>| {
        s.filter(|s| s.0.is_none())
            .map(|s| [s.1 * 1000.0, s.2 * 1000.0])
    };
    let (even_all, odd_all) = (side(0), side(1));
    let culled_ms = [culled(even_all), culled(odd_all)];
    let extra = (culled_ms != [None, None]).then(|| {
        Box::new(Extra {
            culled_ms,
            ..Extra::default()
        })
    });
    let kept = |s: Option<(Option<f64>, f64, f64)>| s.and_then(|s| Some((s.0?, s.1, s.2)));
    let (even, odd) = (kept(even_all), kept(odd_all));
    let beat_error_ms = timing::beat_error_ms(win);
    // Each side found counts, kept or culled, unless both are culled on
    // a Swiss lever ([`screen_unlock_beat_error`], #1977, #2313). The
    // stream screens every window after, and that screen decides: this
    // clause only saves the work.
    let unheard = even.is_none() && odd.is_none() && cfg.escapement.both_culled_unheard();
    let beat_error_unlock_ms = (!unheard)
        .then(|| {
            unlock_beat_error_ms(
                beat_error_ms,
                even_all.map(|s| s.1 * 1000.0),
                odd_all.map(|s| s.1 * 1000.0),
            )
        })
        .flatten();
    AmplitudeWindow {
        start_s: span_s.0,
        end_s: span_s.1,
        even_deg: even.map(|s| s.0),
        odd_deg: odd.map(|s| s.0),
        beat_error_ms,
        beat_error_unlock_ms,
        even_unlock_ms: even.map(|s| s.1 * 1000.0),
        odd_unlock_ms: odd.map(|s| s.1 * 1000.0),
        even_drop_ms: even.map(|s| s.2 * 1000.0),
        odd_drop_ms: odd.map(|s| s.2 * 1000.0),
        extra,
        sound_1: [0, 1].map(|k| sides[k].as_ref().map_or(f32::NAN, |s| s.sound_1)),
    }
}

/// Where a side's guide unlock was read ([`windows_between_at`]), samples
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

/// Whether two sides whose unlocks lie `unlocks_s` seconds from their
/// beat times are read on long templates ([`UnlockWindow::of_unlocks`]).
fn sides_go_long(unlocks_s: [Option<f64>; 2]) -> bool {
    let beyond = |u: Option<f64>| u.filter(|at| *at < -short_reach_s());
    match (beyond(unlocks_s[0]), beyond(unlocks_s[1])) {
        (Some(e), Some(o)) => sides_agree(e, o, 1.0, SIDES_UNLOCK_AGREE_SHARE),
        _ => false,
    }
}

/// Whether a window's two sides, whose unlocks lie `e` and `o` from the
/// drop (Tick, Tock), agree for a watch whose ratio of the two is `r0`
/// ([`crate::escapement::Escapement::sides_reference`]): when `e` and
/// `r0`·`o` lie within `share` of their mean of each other. Times only,
/// so it holds at every lift. The one test for both the long templates
/// ([`sides_go_long`], #1703, at [`SIDES_UNLOCK_AGREE_SHARE`]) and the
/// screen of a window's unlock beat error ([`screen_unlock_beat_error`],
/// at [`SIDES_DISAGREE_RATIO`] as a share, [`share_of_ratio`]).
pub fn sides_agree(e: f64, o: f64, r0: f64, share: f64) -> bool {
    (e - r0 * o).abs() <= share * (e + r0 * o).abs() / 2.0
}

/// How far apart a window's two sides' unlock-to-drop times may lie for
/// its unlock beat error to stand ([`screen_unlock_beat_error`]), as the
/// largest ratio of the longer to the shorter, against the escapement's
/// own ratio (Norm's amended ruling on #1788, #2486): a healthy lever's
/// two sides are not equal (the ratio's median 1.03 to 1.14 on the corpus
/// takes Norm read, and a 90th percentile up to 1.30), while an unlock
/// read on the wrong sound sits far out (about 1.6 on the #2318 clip, 0.55
/// to 0.75 on `dd3235_DU_30min`). The log-midpoint of 1.30 and 1.57,
/// rounded down, and frozen there. Wider than the long templates'
/// [`SIDES_UNLOCK_AGREE_SHARE`], which chooses a template, not a cull.
pub const SIDES_DISAGREE_RATIO: f64 = 1.4;

/// A largest ratio `ratio` of two times as the share of their mean they
/// may lie apart by, as [`sides_agree`] takes it: x and y with
/// max(x/y, y/x) ≤ `ratio` are exactly those within this share.
pub fn share_of_ratio(ratio: f64) -> f64 {
    2.0 * (ratio - 1.0) / (ratio + 1.0)
}

/// The times of the good beats of one side (even or not) in `beats`.
fn good_side_times(beats: &[Beat], even: bool) -> Vec<f64> {
    beats
        .iter()
        .filter(|b| b.is_good() && (b.index.rem_euclid(2) == 0) == even)
        .map(|b| b.time)
        .collect()
}

/// The unlock, samples into the template, on a trimmed template of the
/// beats at `times` starting `pre_s` before them, unguided: how a side's
/// guide is first read ([`windows_between_at`]).
fn template_unlock(
    env: &[f32],
    fs: f64,
    times: &[f64],
    pre_s: f64,
    cfg: &AmplitudeConfig,
) -> Option<f64> {
    let origin = (pre_s * fs).round() as usize;
    trimmed_template_from(env, fs, times, pre_s)
        .and_then(|t| edges(&t, fs, origin, cfg.onset_fraction))
        .map(|e| e.unlock)
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
) -> Option<SideEdges> {
    let long_origin = (long_pre_s * fs).round() as usize;
    let long = trimmed_template_from(env, fs, times, long_pre_s);
    let (tmpl, origin, guide_at) = match guide {
        Some(Guide::Long(g)) => (long.clone()?, long_origin, Some(g)),
        g => {
            let short = UnlockWindow::SHORT;
            let g = g.map(|g| match g {
                Guide::Short(g) | Guide::Long(g) => g,
            });
            let tmpl = trimmed_template_from(env, fs, times, short.pre_s)?;
            (tmpl, short.origin(fs), g)
        }
    };
    let guided = edges_guided(&tmpl, fs, origin, cfg.onset_fraction, guide_at)?;
    let height = |e: &Edges| e.unlock_level - e.floor;
    let sound_1 = height(&guided);
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
    let late = matches!(guide, Some(Guide::Long(g)) if unlock > at(g, long_origin) + UNLOCK_LATE_S);
    Some(SideEdges {
        sound_1,
        edges: (!late).then_some((unlock, drop)),
    })
}

/// A side's read in one window ([`side_edges`]): the height over the floor
/// of the sound 1 its guide found, and its unlock and drop, seconds from
/// its beat times, unless it was withheld as late.
struct SideEdges {
    sound_1: f32,
    edges: Option<(f64, f64)>,
}

/// Which of `heights`, one side's sound 1 in each window of a chunk (None
/// where none was found), stand under [`FAINT_SOUND_MAX_SHARE`] of their
/// median.
fn faint_sides(heights: &[Option<f32>]) -> Vec<bool> {
    let mut found: Vec<f32> = heights.iter().flatten().copied().collect();
    // NaN with none found, which no height is under.
    let median = crate::dsp::median_f32(&mut found);
    heights
        .iter()
        .map(|h| h.is_some_and(|h| h < FAINT_SOUND_MAX_SHARE * median))
        .collect()
}

#[cfg(test)]
#[path = "amplitude_sides_tests.rs"]
mod sides_tests;

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
                extra: None,
                sound_1: [f32::NAN; 2],
            })
            .collect();
        // Window 30's Tock edge lands on the wrong sound.
        wins[30].odd_deg = Some(290.0);
        let n = super::reject_side_outliers(
            &mut wins,
            super::OUTLIER_SPAN_S,
            crate::escapement::Escapement::SwissLever,
        );
        assert_eq!(n, 1);
        assert_eq!(wins[30].odd_deg, None);
        assert!(wins[30].even_deg.is_some());
        assert_eq!(wins[30].beat_error_unlock_ms, Some(0.3));
    }

    /// Judged in parts as the windows come, as live judges them, each part
    /// scored against its neighbours as read, the sides left out are the
    /// same as one pass over the whole run leaves out (#1738, Norm on
    /// #1800), with two wrong sounds close enough to be each other's
    /// neighbours.
    #[test]
    fn side_outliers_judged_in_parts_match_one_pass() {
        let mut read: Vec<super::AmplitudeWindow> = (0..80)
            .map(|i| super::AmplitudeWindow {
                start_s: 2.0 * i as f64,
                end_s: 2.0 * i as f64 + 2.0,
                even_deg: Some(250.0 + 0.4 * (i % 5) as f64),
                odd_deg: Some(240.0 - 0.3 * (i % 4) as f64),
                beat_error_ms: None,
                beat_error_unlock_ms: None,
                even_unlock_ms: None,
                odd_unlock_ms: None,
                even_drop_ms: None,
                odd_drop_ms: None,
                extra: None,
                sound_1: [f32::NAN; 2],
            })
            .collect();
        read[30].odd_deg = Some(290.0);
        read[33].odd_deg = Some(275.0);
        read[50].even_deg = Some(200.0);
        let mut whole = read.clone();
        let n = super::reject_side_outliers(
            &mut whole,
            super::OUTLIER_SPAN_S,
            crate::escapement::Escapement::SwissLever,
        );
        assert_eq!(n, 3);
        let mid = |w: &super::AmplitudeWindow| (w.start_s + w.end_s) / 2.0;
        let span = super::OUTLIER_SPAN_S;
        let mut parts: Vec<super::AmplitudeWindow> = Vec::new();
        for batch in read.chunks(3) {
            let from_s = mid(&batch[0]) - span / 2.0;
            parts.extend_from_slice(batch);
            let context = parts.partition_point(|w| mid(w) < from_s - span / 2.0);
            super::reject_side_outliers_from(&mut parts[context..], span, from_s);
        }
        let sides = |ws: &[super::AmplitudeWindow]| -> Vec<_> {
            ws.iter()
                .map(|w| (w.even_deg, w.odd_deg, w.outliers()))
                .collect()
        };
        assert_eq!(sides(&parts), sides(&whole));
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
        let w = windows_between_at(&env, fs, &beats, [2.0 * period; 2], (2.0, 0.0, 20.0), &cfg);
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

    /// The Tick and Tock profile reads those same watches as amplitude
    /// does (#1924): within 1° of the analysis, its unlock on the long
    /// window where amplitude's was, not the impulse taken for it on a
    /// 20 ms one (18,000 bph at 180° read 326° there). At 270° it stays
    /// on the short window.
    #[test]
    fn a_low_amplitude_profile_reads_as_amplitude_does() {
        use crate::analysis::{analyze, AnalysisConfig};
        use crate::synth::{generate, SynthConfig};
        for (bph, amp) in [
            (14_400, 150.0),
            (18_000, 110.0),
            (18_000, 180.0),
            (21_600, 150.0),
            (28_800, 110.0),
            (28_800, 270.0),
        ] {
            let sc = SynthConfig {
                duration_s: 30.0,
                bph,
                ..Default::default()
            };
            let audio = generate(&sc, |_| amp, |_| 0.0);
            let fs = audio.sample_rate as f64;
            let cfg = AnalysisConfig {
                bph: Some(bph),
                ..Default::default()
            };
            let a = analyze(&audio, &cfg);
            let read = a.summary.amplitude_deg.expect("an amplitude");
            let osc = 7200.0 / bph as f64;
            let env = crate::dsp::envelope(&audio.samples, fs, &cfg.envelope);
            let w = profile_window_at(&a.amplitude_windows, 20.0, osc);
            assert_eq!(w.long, amp < 200.0, "{bph} bph at {amp}°: {w:?}");
            let sides =
                crate::profile::profiles(&env, fs, &a.beats, osc, 10.0, 20.0, &cfg.amplitude, w);
            for p in sides {
                let p = p.expect("a profile");
                let deg = p.amplitude_deg.expect("a profile amplitude");
                assert!(
                    (deg - read).abs() < 1.0,
                    "{bph} bph at {amp}°: {deg}° against {read}°"
                );
                assert!(p.unlock_ms.expect("an unlock") > p.t0_ms + FLOOR_S * 1000.0);
            }
        }
    }

    /// Without a reading of both sides the window is the short one: none
    /// yet, at the start of a take, or one side only (#1924).
    #[test]
    fn without_both_sides_unlocks_the_window_is_short() {
        let osc = 0.25;
        assert_eq!(profile_window_at(&[], 5.0, osc), UnlockWindow::SHORT);
        for at in [[None, None], [Some(-0.025), None], [None, Some(-0.025)]] {
            assert_eq!(UnlockWindow::of_unlocks(osc, at), UnlockWindow::SHORT);
        }
        assert_eq!(
            UnlockWindow::of_unlocks(osc, [Some(-0.008); 2]),
            UnlockWindow::SHORT
        );
        let long = UnlockWindow::of_unlocks(osc, [Some(-0.025), Some(-0.026)]);
        assert_eq!(long, UnlockWindow::long(osc));
        assert!(long.pre_s > PRE_S);
        // Sides far apart are not read long: a faint sound before one
        // side's unlock (#1703).
        let apart = UnlockWindow::of_unlocks(osc, [Some(-0.018), Some(-0.026)]);
        assert_eq!(apart, UnlockWindow::SHORT);
    }

    /// A profile takes the window of the latest amplitude window ending by
    /// its time that read both sides; before any has, the first after
    /// (a take's first stretches, on a saved read).
    #[test]
    fn before_any_window_ends_a_profile_takes_the_first_after() {
        let osc = 0.25;
        let win = |start_s: f64, unlock_ms: Option<f64>| AmplitudeWindow {
            start_s,
            end_s: start_s + 2.0,
            even_deg: None,
            odd_deg: None,
            beat_error_ms: None,
            beat_error_unlock_ms: None,
            even_unlock_ms: unlock_ms,
            odd_unlock_ms: unlock_ms,
            even_drop_ms: None,
            odd_drop_ms: None,
            extra: None,
            sound_1: [f32::NAN; 2],
        };
        let wins = [
            win(10.0, None),
            win(12.0, Some(-25.0)),
            win(14.0, Some(-8.0)),
        ];
        let long = UnlockWindow::long(osc);
        // Every window ends after 5 s; the first to read both sides is long.
        assert_eq!(profile_window_at(&wins, 5.0, osc), long);
        // By 14 s the long one has ended; by 16 s the short one.
        assert_eq!(profile_window_at(&wins, 14.0, osc), long);
        assert_eq!(profile_window_at(&wins, 16.0, osc), UnlockWindow::SHORT);
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
        let first = windows_between_at(&env, fs, &beats, [osc; 2], (2.0, 0.0, 8.0), &cfg);
        let w = &first[1];
        assert_eq!((w.even_deg, w.odd_deg), (None, None), "{w:?}");
        assert_eq!((w.even_unlock_ms, w.beat_error_unlock_ms), (None, None));
        let [u, d] = w.culled()[0].expect("the culled side's edges kept");
        let at52 = amplitude_deg((d - u) / 1000.0, osc, 52.0);
        assert!(at52 > MAX_AMPLITUDE_DEG, "{at52}");
        // At 45° it passes: the same figure a read at 45° gives.
        let mut lower = w.clone();
        at_lift(
            &mut lower,
            osc,
            52.0,
            45.0,
            crate::escapement::Escapement::SwissLever,
        );
        let fresh = windows_between_at(
            &env,
            fs,
            &beats,
            [osc; 2],
            (2.0, 0.0, 8.0),
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
        at_lift(
            &mut back,
            osc,
            45.0,
            12.0,
            crate::escapement::Escapement::SwissLever,
        );
        assert_eq!(back.even_deg, None, "under the gate at 12°");
        at_lift(
            &mut back,
            osc,
            12.0,
            52.0,
            crate::escapement::Escapement::SwissLever,
        );
        assert_eq!(format!("{back:?}"), format!("{w:?}"));
        assert_eq!(back.culled(), w.culled());
    }

    /// The unlock beat error counts each side whose edges were found,
    /// kept or culled, so a new lift doesn't move it, even where the gate
    /// culls a side at one lift and keeps it at another (#1977). A side
    /// whose unlock-to-drop time is beyond its own side's far fences is
    /// left out of its window.
    #[test]
    fn the_unlock_beat_error_is_the_same_at_every_lift() {
        let osc = 0.25;
        let win = |i: usize| {
            let f = ((i * 7) % 40) as f64 / 40.0;
            // The odd side reads higher: near 380° at 50°, over it by 58°.
            let (e, o) = (-(6.6 + 0.3 * f), -(5.45 + 0.5 * f));
            AmplitudeWindow {
                start_s: 2.0 * i as f64,
                end_s: 2.0 * i as f64 + 2.0,
                even_deg: None,
                odd_deg: None,
                beat_error_ms: Some(0.1),
                beat_error_unlock_ms: None,
                // The 31st's even unlock marked on the wrong sound.
                even_unlock_ms: Some(if i == 30 { -27.0 } else { e }),
                odd_unlock_ms: Some(o),
                even_drop_ms: Some(0.0),
                odd_drop_ms: Some(0.0),
                extra: None,
                sound_1: [f32::NAN; 2],
            }
        };
        let mut at50: Vec<AmplitudeWindow> = (0..40).map(win).collect();
        relift(
            &mut at50,
            osc,
            50.0,
            50.0,
            crate::escapement::Escapement::SwissLever,
        );
        let be = |w: &[AmplitudeWindow]| -> Vec<Option<u64>> {
            w.iter()
                .map(|x| x.beat_error_unlock_ms.map(f64::to_bits))
                .collect()
        };
        assert_eq!(at50[30].beat_error_unlock_ms, None, "the wrong sound");
        assert!(
            at50.iter()
                .filter(|w| w.beat_error_unlock_ms.is_some())
                .count()
                == 39
        );
        for lift in [52.0, 55.0, 58.0] {
            let mut w = at50.clone();
            relift(
                &mut w,
                osc,
                50.0,
                lift,
                crate::escapement::Escapement::SwissLever,
            );
            assert_eq!(be(&w), be(&at50), "at {lift}°");
        }
        // The gate did act: at 58° some odd sides are culled.
        let mut at58 = at50.clone();
        relift(
            &mut at58,
            osc,
            50.0,
            58.0,
            crate::escapement::Escapement::SwissLever,
        );
        let culled = at58.iter().filter(|w| w.culled()[1].is_some()).count();
        assert!(culled > 5, "{culled}");
        // At 50° the wrong sound's window keeps its odd side, so its even
        // side is screened: the one side outside.
        let mut again = at50.clone();
        assert_eq!(
            screen_unlock_beat_error(&mut again, crate::escapement::Escapement::SwissLever),
            1
        );
        // With both sides culled, no unlock was heard: none, as the drop's
        // beat error stands.
        let mut both = at50[0].clone();
        both.set_culled(edges_found(&both));
        (both.even_unlock_ms, both.odd_unlock_ms) = (None, None);
        (both.even_drop_ms, both.odd_drop_ms) = (None, None);
        let mut wins = at50.clone();
        wins[0] = both;
        screen_unlock_beat_error(&mut wins, crate::escapement::Escapement::SwissLever);
        assert_eq!(wins[0].beat_error_unlock_ms, None);
        assert_eq!(be(&wins[1..]), be(&at50[1..]));
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
            extra: None,
            sound_1: [f32::NAN; 2],
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
            extra: None,
            sound_1: [f32::NAN; 2],
        };
        at_lift(
            &mut w,
            0.25,
            52.0,
            56.0,
            crate::escapement::Escapement::SwissLever,
        );
        let again = side_deg(0.007, 0.25, 56.0);
        assert!(again.is_some());
        assert_eq!(w.even_deg, again);
        assert_eq!(w.even_unlock_ms, Some(-7.0));
        assert!((w.odd_deg.unwrap() - 300.0 * 56.0 / 52.0).abs() < 1e-9);
        assert_eq!(w.beat_error_unlock_ms, Some(0.3), "no side changed state");
        at_lift(
            &mut w,
            0.25,
            56.0,
            70.0,
            crate::escapement::Escapement::SwissLever,
        );
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
        let w = windows_between_at(&env, fs, &beats, [osc; 2], (2.0, 0.0, secs - 0.5), &cfg);
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
        let w = windows_between_at(&env, fs, &beats, [osc; 2], (2.0, 0.0, 20.0), &cfg);
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
        let w = windows_between_at(&env, fs, &beats, [osc; 2], (2.0, 0.0, secs - 0.5), &cfg);
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

    /// A weak watch read for 60 s whose Tock, in the windows `noise`
    /// names, has no sound 1 the search can hear: in its place a bump of
    /// noise 0.1 to 0.3 times as high, 0 to 3 ms from where sound 1 sits,
    /// in the same place on each of the window's beats so the window's
    /// template keeps it: set so about one noise side in five reads an
    /// edge, as on Peter's Submariner at Crown Down (about 12 of 60). Counts
    /// of the sides that read an edge, and of those withheld as faint
    /// ([`FAINT_SOUND_MAX_SHARE`], #1738). Each window's sound 1 stands 0.7 to 1.3 times its usual
    /// height, as a real one varies.
    fn faint_withheld_counts(seed: u64, noise: &[bool]) -> FaintCounts {
        let (fs, period, secs) = (48_000.0, 0.125, 60.0);
        let dt = 0.013;
        let mut state = seed;
        let mut rand = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 40) as f64 / (1u64 << 24) as f64
        };
        let mut env: Vec<f32> = (0..((secs + 0.5) * fs) as usize)
            .map(|_| 0.016 + 0.004 * (rand() as f32 - 0.5))
            .collect();
        let windows = (secs / 2.0) as usize;
        let gain: Vec<[f64; 2]> = (0..windows)
            .map(|_| [0.7 + 0.6 * rand(), 0.7 + 0.6 * rand()])
            .collect();
        let bump: Vec<(f64, f64)> = (0..windows)
            .map(|_| (0.1 + 0.2 * rand(), 0.003 * rand()))
            .collect();
        let at = |t: f64| (t * fs).round() as usize;
        let mut beats = Vec::new();
        for k in 0..(secs / period) as i64 - 2 {
            let t = 0.1 + k as f64 * period;
            let i = ((t / 2.0) as usize).min(windows - 1);
            let side = usize::from(k % 2 != 0);
            let (height, from) = if side == 1 && noise.get(i).copied().unwrap_or(false) {
                (bump[i].0, t - dt + bump[i].1)
            } else {
                (gain[i][side], t - dt)
            };
            env[at(from)..at(from + 0.0003)].fill(0.016 + 0.12 * height as f32);
            env[at(t - 0.7 * dt)..at(t - 0.7 * dt + 0.0003)].fill(0.03);
            env[at(t)..at(t + 0.0003)].fill(1.0);
            beats.push(Beat {
                index: k,
                time: t,
                quality: 1.0,
            });
        }
        let cfg = AmplitudeConfig::default();
        let mut w = windows_between_at(
            &env,
            fs,
            &beats,
            [2.0 * period; 2],
            (2.0, 0.0, secs - 0.5),
            &cfg,
        );
        // The 60 s as one chunk, as the run pinned below was read.
        withhold_faint_by_chunk(&mut w, |_| 0);
        assert_eq!(w.len(), noise.len());
        // A side read an edge when it kept them, was culled at the gate or
        // withheld as faint.
        let read = |w: &AmplitudeWindow, k: usize| {
            [w.even_unlock_ms, w.odd_unlock_ms][k].is_some()
                || w.culled()[k].is_some()
                || w.faint_withheld()[k]
        };
        let mut c = FaintCounts::default();
        for (w, &n) in w.iter().zip(noise) {
            for k in 0..2 {
                if !read(w, k) {
                    continue;
                }
                let (of, held) = if k == 1 && n {
                    (&mut c.noise, &mut c.noise_held)
                } else {
                    (&mut c.healthy, &mut c.healthy_held)
                };
                *of += 1;
                *held += usize::from(w.faint_withheld()[k]);
            }
        }
        c
    }

    /// [`faint_withheld_counts`]: the noise sides and the healthy sides
    /// that read an edge, and how many of each were withheld as faint.
    #[derive(Default, Debug)]
    struct FaintCounts {
        noise: usize,
        noise_held: usize,
        healthy: usize,
        healthy_held: usize,
    }

    /// [`faint_withheld_counts`] summed over `seeds`.
    fn faint_counts_over(seeds: std::ops::Range<u64>, noise: &[bool]) -> FaintCounts {
        let mut t = FaintCounts::default();
        for seed in seeds {
            let c = faint_withheld_counts(seed, noise);
            t.noise += c.noise;
            t.noise_held += c.noise_held;
            t.healthy += c.healthy;
            t.healthy_held += c.healthy_held;
        }
        t
    }

    /// The Tock's sound 1 left out of every `step`th window from `first`.
    fn every(step: usize, first: usize) -> Vec<bool> {
        (0..29).map(|i| i % step == first % step).collect()
    }

    /// A side whose sound 1 is lost in noise is withheld where it reads
    /// under [`FAINT_SOUND_MAX_SHARE`] of its chunk's median, and a healthy
    /// one is not (#1738). Norm's acceptance, set before the one run on
    /// seeds 2000 to 2029, asked for at least 90% of the noise sides that
    /// read an edge to be withheld at 20% and 40% noise: it withheld 23 of
    /// 48 and 23 of 103, as most read at 0.28 to 0.4, and it failed. Peter
    /// chose to ship with that limit stated (#1738, the gap is #1791). This
    /// pins what was measured: every side read under the line is
    /// withheld, none of the healthy ones (the bar was at most 1%), and the
    /// counts withheld.
    #[test]
    fn noise_windows_are_withheld_and_healthy_ones_kept() {
        let cases: [(Vec<bool>, usize, usize); 2] = [
            (every(5, 2), 48, 23),
            ((0..29).map(|i| i % 5 < 2).collect(), 103, 23),
        ];
        for (noise, read, held) in cases {
            let c = faint_counts_over(2000..2030, &noise);
            assert_eq!((c.noise, c.noise_held), (read, held), "{c:?}");
            assert_eq!(c.healthy_held, 0, "{c:?}");
        }
    }

    /// The limit of [`FAINT_SOUND_MAX_SHARE`], pinned: the share is of the
    /// chunk's median, so where noise windows are the majority of a side's
    /// windows in the chunk the median falls toward their height, and 7 of
    /// the 151 noise sides that read an edge are withheld at 60% noise:
    /// the rule fades, with no cliff (#1738, Norm's ruling). On heights
    /// alone, a faint side among a majority of faint ones is not withheld,
    /// and one among loud ones is.
    #[test]
    fn faint_windows_in_the_majority_are_not_withheld() {
        let noise: Vec<bool> = (0..29).map(|i| i % 5 < 3).collect();
        assert!(noise.iter().filter(|&&n| n).count() * 2 > noise.len());
        let c = faint_counts_over(2000..2030, &noise);
        assert_eq!(
            (c.noise, c.noise_held, c.healthy_held),
            (151, 7, 0),
            "{c:?}"
        );
        let mid = |h: &[f32]| faint_sides(&h.iter().map(|&h| Some(h)).collect::<Vec<_>>());
        assert_eq!(mid(&[0.01, 0.01, 0.01, 0.04, 0.04]), vec![false; 5]);
        assert_eq!(
            mid(&[0.01, 0.04, 0.04, 0.04, 0.04]),
            vec![true, false, false, false, false]
        );
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

    /// Near stopping the sides' faint unlocks lie far out and apart in time
    /// by a share of it, not a fixed time: they still go long together
    /// (#1703; a fixed 2 ms split the sides of Peter's 69 h run-down at
    /// 62 h, Trevor's review). A 28,800 bph watch at a 59° lift, its Tick's
    /// unlock 23 ms and its Tock's 20.5 ms before the drop (2.5 ms, about
    /// 11% of their mean), each at a twentieth of the drop's height, its
    /// impulse at a quarter, 0.72 of the way back, over a floor with seeded
    /// noise. Each side reads its own unlock, within 3°, in every window.
    #[test]
    fn faint_unlocks_more_than_2_ms_apart_but_within_the_share_go_long() {
        let (fs, period, lift, secs) = (48_000.0, 0.125, 59.0, 30.0);
        let osc = 2.0 * period;
        let dts = [0.023, 0.0205];
        let mut state = 1710u64;
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
            let dt = dts[(k % 2) as usize];
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
        let w = windows_between_at(&env, fs, &beats, [osc; 2], (2.0, 0.0, secs - 0.5), &cfg);
        let want = dts.map(|dt| amplitude_deg(dt, osc, lift));
        assert!(!w.is_empty());
        for w in &w {
            for (a, want) in [w.even_deg, w.odd_deg].into_iter().zip(want) {
                let a = a.unwrap_or_else(|| panic!("a side withheld: {w:?}"));
                assert!((a - want).abs() < 3.0, "{a}° read at {want}°: {w:?}");
            }
        }
    }

    /// The unlock is found the same way at every lift: the lift enters
    /// only the conversion to degrees (Norm's rule, #1695). A 28,800 bph
    /// watch whose unlock sits 16 ms before the drop (past a 20 ms
    /// template's search, so on long templates) and whose sides differ by
    /// 0.4 ms, with sounds that rise over 1 ms from a floor with seeded
    /// noise: its unlock beat error is bit-identical at 44°, 52° and 60°,
    /// and each side's amplitude scales with the lift.
    #[test]
    fn the_unlock_beat_error_does_not_depend_on_the_lift() {
        let (fs, period, secs) = (48_000.0, 0.125, 30.0);
        let osc = 2.0 * period;
        let mut state = 7u64;
        let mut env: Vec<f32> = (0..((secs + 0.5) * fs) as usize)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                0.01 + 0.01 * ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5)
            })
            .collect();
        let at = |t: f64| (t * fs).round() as usize;
        let mut rise = |t: f64, peak: f32| {
            let n = at(0.001);
            for (i, v) in env[at(t)..at(t) + n].iter_mut().enumerate() {
                *v += peak * (i + 1) as f32 / n as f32;
            }
        };
        let mut beats = Vec::new();
        for k in 0..(secs / period) as i64 - 2 {
            let t = 0.1 + k as f64 * period;
            let dt = if k % 2 == 0 { 0.0162 } else { 0.0158 };
            rise(t - dt, 0.3);
            rise(t - 0.6 * dt, 0.4);
            rise(t, 1.0);
            beats.push(Beat {
                index: k,
                time: t,
                quality: 1.0,
            });
        }
        let read = |lift: f64| {
            let cfg = AmplitudeConfig {
                lift_deg: lift,
                ..Default::default()
            };
            windows_between_at(&env, fs, &beats, [osc; 2], (2.0, 0.0, secs - 0.5), &cfg)
        };
        let at52 = read(52.0);
        assert!(at52.iter().all(|w| w.beat_error_unlock_ms.is_some()));
        for lift in [44.0, 60.0] {
            let w = read(lift);
            assert_eq!(w.len(), at52.len());
            for (w, b) in w.iter().zip(&at52) {
                assert_eq!(
                    w.beat_error_unlock_ms.map(f64::to_bits),
                    b.beat_error_unlock_ms.map(f64::to_bits),
                    "{lift}°: {w:?} against 52°: {b:?}"
                );
                for (a, b) in [(w.even_deg, b.even_deg), (w.odd_deg, b.odd_deg)] {
                    let (a, b) = (a.unwrap(), b.unwrap());
                    assert!(
                        (a / b - lift / 52.0).abs() < 1e-12,
                        "{lift}°: {a}° and {b}°"
                    );
                }
            }
        }
    }

    /// [`SEARCH_LIFT_DEG`] is the calibre table's largest lift, so the
    /// unlock search reaches [`MIN_AMPLITUDE_DEG`] at every calibre's lift.
    #[test]
    fn the_search_lift_is_the_calibre_tables_largest() {
        let largest = crate::calibres::all()
            .iter()
            .filter_map(|c| c.lift_angle_deg)
            .fold(f64::MIN, f64::max);
        assert_eq!(largest, SEARCH_LIFT_DEG);
    }
}
