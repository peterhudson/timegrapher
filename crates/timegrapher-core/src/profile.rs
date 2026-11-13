//! The sound of a tick and of a tock, for drawing and debugging.
//!
//! A profile is one side's median envelope over a stretch of beats, the
//! spread around it, and the marks the engine measured on it: the unlock and
//! drop edges that amplitude and beat error use, and the three sounds the
//! shape module finds. It is built from the same template as
//! [`crate::amplitude`], so the marks are exactly the ones behind the
//! numbers on screen.

use crate::amplitude::{edges_guided, side_deg, AmplitudeConfig};
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

/// The facts on each side of a Tick and Tock profile, in [`facts`]' order,
/// as every surface names them (#510). No amplitude: the Amplitude card
/// reads the same moment with its ±, and the profile's own figure from
/// the median sound, with none, was a second copy beside it (Norm, #1068
/// item 6).
pub const FACT_NAMES: [&str; 4] = ["Beats", "Unlock", "Drop", "Unlock to Drop"];

/// A time from the reference point as the Profile words it: "−0.27 ms".
pub fn ms_text(v: f64) -> String {
    crate::figure_text::true_minus(&format!("{v:+.2} ms"))
}

/// One side's facts with their units, in [`FACT_NAMES`]' order: "–" for
/// an edge not found.
pub fn facts_of(beats: usize, unlock_ms: Option<f64>, drop_ms: Option<f64>) -> [String; 4] {
    let ms = |v: Option<f64>| v.map_or("–".to_string(), ms_text);
    [
        beats.to_string(),
        ms(unlock_ms),
        ms(drop_ms),
        match (unlock_ms, drop_ms) {
            (Some(u), Some(d)) => format!("{:.2} ms", d - u),
            _ => "–".to_string(),
        },
    ]
}

/// [`facts_of`] a side.
pub fn facts(p: &TickProfile) -> [String; 4] {
    facts_of(p.beats, p.unlock_ms, p.drop_ms)
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

/// The fewest beats of a side an averaged profile is made from
/// ([`from_snippets`]); with fewer there is none.
pub const PROFILE_MIN_BEATS: usize = 3;

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
    if snips.len() < PROFILE_MIN_BEATS {
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
    let q = crate::dsp::quantile_f32;
    // The same trimmed-mean template the amplitude measurement builds.
    let tmpl: Vec<f32> = (0..len)
        .map(|k| crate::dsp::trimmed_mean_f32(&mut sorted_at(k), TEMPLATE_TRIM))
        .collect();
    let origin = (PRE_S * fs).round() as usize;
    let to_ms = |s: f64| (s - origin as f64) / fs * 1000.0;
    let guide = guide_unlock_ms.map(|ms| origin as f64 + ms / 1000.0 * fs);

    let stride = ((STEP_MS / 1000.0 * fs).round() as usize).max(1);
    let (mut median, mut p10, mut p90) = (Vec::new(), Vec::new(), Vec::new());
    for k in (0..len).step_by(stride) {
        let c = sorted_at(k);
        median.push(tmpl[k]);
        p10.push(q(&c, 0.1));
        p90.push(q(&c, 0.9));
    }
    let mut p = TickProfile {
        side,
        beats: snips.len(),
        t0_ms: to_ms(0.0),
        step_ms: stride as f64 / fs * 1000.0,
        median,
        p10,
        p90,
        floor: 0.0,
        unlock_ms: None,
        drop_ms: None,
        peak_ms: None,
        amplitude_deg: None,
        sound1_ms: None,
        sound2_ms: None,
        sound3_ms: None,
    };
    mark(&mut p, (&tmpl, fs, origin), osc_period_s, cfg, guide);
    Some(p)
}

/// Measure a profile's marks on `curve` (sampled at `fs`, the beat at
/// sample `origin`): the unlock and drop edges amplitude reads
/// ([`edges_guided`], from `guide` when given), the drop's peak, the
/// amplitude when it is plausible, sounds 1 to 3 ([`shape::measure`]),
/// and the noise floor (the median of the first 3 ms). The one way every
/// profile is marked.
fn mark(
    p: &mut TickProfile,
    (curve, fs, origin): (&[f32], f64, usize),
    osc_period_s: f64,
    cfg: &AmplitudeConfig,
    guide: Option<f64>,
) {
    let to_ms = |s: f64| (s - origin as f64) / fs * 1000.0;
    let quiet = ((0.003 * fs) as usize).min(curve.len());
    p.floor = crate::dsp::median_f32(&mut curve[..quiet].to_vec());
    let e = edges_guided(curve, fs, origin, cfg.onset_fraction, guide);
    let sh = shape::measure(curve, fs, origin, &ShapeConfig::default());
    let from_unlock = |t: Option<f64>| -> Option<f64> { Some(sh.as_ref()?.unlock_at_ms + t?) };
    p.unlock_ms = e.map(|e| to_ms(e.unlock));
    p.drop_ms = e.map(|e| to_ms(e.drop));
    p.peak_ms = e.map(|e| to_ms(e.peak as f64));
    p.amplitude_deg =
        e.and_then(|e| side_deg((e.drop - e.unlock) / fs, osc_period_s, cfg.lift_deg));
    p.sound1_ms = from_unlock(sh.as_ref().and_then(|s| s.t1_ms));
    p.sound2_ms = from_unlock(sh.as_ref().and_then(|s| s.t2_ms));
    p.sound3_ms = from_unlock(sh.as_ref().map(|s| s.t3_ms));
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

/// How long the tick and tock sound is kept every
/// [`crate::analysis_take::PROFILE_EVERY_S`], back from the latest kept;
/// before that, one a minute ([`SOUND_OLD_EVERY_S`]), so a run of days
/// keeps tens of megabytes, not a gigabyte (#483).
pub const SOUND_FULL_S: f64 = 3600.0;
/// How often the sound is kept before the last [`SOUND_FULL_S`].
pub const SOUND_OLD_EVERY_S: f64 = 60.0;

/// Thin `history` (times rising) to one sound a [`SOUND_OLD_EVERY_S`]
/// before the last [`SOUND_FULL_S`], keeping the first. Thinning a whole
/// history once keeps what thinning it after each sound would.
pub fn thin_sound<T>(history: &mut Vec<(f64, T)>) {
    let Some(&(last, _)) = history.last() else {
        return;
    };
    let old = last - SOUND_FULL_S;
    let mut kept: Option<f64> = None;
    history.retain(|(t, _)| {
        let keep = *t >= old || kept.is_none_or(|k| t - k >= SOUND_OLD_EVERY_S - 1e-6);
        if keep {
            kept = Some(*t);
        }
        keep
    });
}

/// The sound `history` kept nearest before `end`, if it is the one shown
/// for that moment: within a step of it, as often as it is kept there.
pub fn kept_sound<T>(history: &[(f64, T)], end: f64) -> Option<&(f64, T)> {
    kept_nearest(history, end).map(|(h, _)| h)
}

/// [`kept_sound`], and whether it comes from before the last
/// [`SOUND_FULL_S`], where the sound is kept once a minute.
fn kept_nearest<T>(history: &[(f64, T)], end: f64) -> Option<(&(f64, T), bool)> {
    let i = history.partition_point(|h| h.0 <= end + 1e-9);
    let h = &history[i.checked_sub(1)?];
    let full_from = history.last().map_or(0.0, |l| l.0 - SOUND_FULL_S);
    let coarse = h.0 < full_from;
    let step = if coarse {
        SOUND_OLD_EVERY_S
    } else {
        crate::analysis_take::PROFILE_EVERY_S
    };
    (end - h.0 <= step + 0.5).then_some((h, coarse))
}

/// What the Tick Tock Profile shows for the moment `end`
/// ([`sound_at`]).
#[derive(Debug, PartialEq)]
pub enum SoundAt<'a, T> {
    /// The sound kept at `at`. Coarse when it comes from before the last
    /// [`SOUND_FULL_S`], kept once a minute, so `at` can be up to a minute
    /// before `end`.
    Kept { at: f64, coarse: bool, sound: &'a T },
    /// The window ending at `end` lies wholly in a masked stretch, or the
    /// window the sound kept for it was made over reaches into one:
    /// culled, never filled from either side.
    Masked,
    /// The beats heard reach to either side of the window, and none fall
    /// in it.
    NoBeats,
    /// No sound was kept for that moment.
    NotKept,
}

/// The sound `history` kept for the window of `window_s` seconds ending at
/// `end`, or why there is none: `end` itself or the whole window masked,
/// or the sound for it
/// made over a window (the `kept_s` seconds before its own time, as it was
/// kept) that reaches into a mask (`masked`, cleaned as
/// [`crate::session::masked_spans`] cleans them); or no beats in the
/// window while `beats` (times rising) reach to either side of it.
pub fn sound_at<'a, T>(
    history: &'a [(f64, T)],
    (end, window_s): (f64, f64),
    masked: &[(f64, f64)],
    beats: &[Beat],
    kept_s: impl Fn(&T) -> f64,
) -> SoundAt<'a, T> {
    let from = end - window_s.max(0.0);
    // The moment itself masked: masked, however little of the window is,
    // so the sound panes agree with the strip's hatching.
    if crate::session::in_mask(masked, end)
        || (window_s > 0.0 && crate::session::masked_within(masked, from, end) >= window_s - 1e-9)
    {
        return SoundAt::Masked;
    }
    if let Some(((at, sound), coarse)) = kept_nearest(history, end) {
        let made_from = at - kept_s(sound).max(0.0);
        if crate::session::in_mask(masked, *at)
            || crate::session::masked_within(masked, made_from, *at) > 0.0
        {
            return SoundAt::Masked;
        }
        return SoundAt::Kept {
            at: *at,
            coarse,
            sound,
        };
    }
    let covered = beats.first().is_some_and(|b| b.time <= from)
        && beats.last().is_some_and(|b| b.time >= end);
    let i = beats.partition_point(|b| b.time <= from);
    let none_in = beats.get(i).is_none_or(|b| b.time > end);
    if covered && none_in {
        SoundAt::NoBeats
    } else {
        SoundAt::NotKept
    }
}

/// One side of [`ProfileAt::Averaged`]: its curve the beat-weighted
/// average of the stretches' curves ("average of N 2 s profiles", never a
/// median), its band the range of their bands (lowest p10 to highest
/// p90, "range of the 2 s bands"), and its marks measured on that curve as
/// on any profile; `stretches` the N averaged, covering `from_s` to
/// `to_s`. A side counts only the stretches that have it.
#[derive(Debug, Clone)]
pub struct SideAverage {
    pub profile: TickProfile,
    pub stretches: usize,
    pub from_s: f64,
    pub to_s: f64,
}

/// The Tick Tock Profile over an Average Over window, built from the 2 s
/// stretch profiles a take kept without its sound ([`profile_at`], #979).
#[derive(Debug, Clone)]
pub enum ProfileAt {
    /// Each side over the stretches kept in the window that have it
    /// ([`SideAverage`]), or `None` for a side none of them has.
    Averaged(Box<[Option<SideAverage>; 2]>),
    /// The window's end, or all of it, is masked, or every stretch in it
    /// reaches into a mask or across a break: culled, never filled.
    Masked,
    /// The beats heard reach to either side of the window, and none fall
    /// in it.
    NoBeats,
    /// No stretch was kept in the window.
    NotKept,
}

/// The Tick Tock Profile for the Average Over window of `window_s`
/// seconds ending at `end` (#979, the math guide's method): the stretches
/// kept in it, never shorter than one stretch, by #636's one rule (a
/// stretch's midpoint in the window, [`crate::session::window_in`]), so
/// a window of one stretch always holds one wherever it ends; less those
/// that reach into a mask (`masked`, cleaned as
/// [`crate::session::masked_spans`] cleans them) or across a break, are
/// averaged side by side, weighted by their beats, on the time axis they
/// were kept on (each aligned on its own drop). Stretches whose curves
/// don't share that axis are left out. With none left, why: as
/// [`sound_at`] says it. `osc_period_s` and `cfg` measure the marks, as
/// [`from_snippets`] does.
pub fn profile_at(
    kept: &[crate::analysis_take::StretchProfiles],
    (end, window_s): (f64, f64),
    masked: &[(f64, f64)],
    breaks: &[crate::breaks::Break],
    beats: &[Beat],
    osc_period_s: f64,
    cfg: &AmplitudeConfig,
) -> ProfileAt {
    // The moment itself masked, or the whole window: masked, as `sound_at`
    // says, so the sound panes agree with the strip's hatching.
    let why = sound_at::<()>(&[], (end, window_s), masked, beats, |_| 0.0);
    if matches!(why, SoundAt::Masked) {
        return ProfileAt::Masked;
    }
    let every = crate::analysis_take::PROFILE_EVERY_S;
    let from = end - window_s.max(every);
    let whole = |p: &&crate::analysis_take::StretchProfiles| {
        crate::session::window_span_in(p.end_s - every, p.end_s, from, end)
    };
    let clean = |p: &&crate::analysis_take::StretchProfiles| {
        let start = p.end_s - every;
        !crate::session::in_mask(masked, p.end_s)
            && crate::session::masked_within(masked, start, p.end_s) <= 0.0
            && !crate::breaks::straddles(breaks, start, p.end_s)
    };
    let inside: Vec<_> = kept.iter().filter(whole).collect();
    let used: Vec<_> = inside.iter().copied().filter(clean).collect();
    if !used.is_empty() {
        let sides = [Side::A, Side::B].map(|side| {
            let each: Vec<(f64, &TickProfile)> = used
                .iter()
                .filter_map(|p| Some((p.end_s, p.sides[side as usize].as_ref()?)))
                .collect();
            averaged(&each, side, osc_period_s, cfg)
        });
        return ProfileAt::Averaged(Box::new(sides));
    }
    // Stretches there, all culled, are masked; else as `sound_at` says.
    match why {
        _ if !inside.is_empty() => ProfileAt::Masked,
        SoundAt::NoBeats => ProfileAt::NoBeats,
        _ => ProfileAt::NotKept,
    }
}

/// One side's profiles averaged as [`profile_at`] says, or `None` with
/// none to average. Those not on the first's time axis are left out.
fn averaged(
    kept: &[(f64, &TickProfile)],
    side: Side,
    osc_period_s: f64,
    cfg: &AmplitudeConfig,
) -> Option<SideAverage> {
    let first = kept.first()?.1;
    let same = |(_, p): &&(f64, &TickProfile)| {
        p.median.len() == first.median.len()
            && (p.t0_ms - first.t0_ms).abs() < 1e-9
            && (p.step_ms - first.step_ms).abs() < 1e-9
    };
    let kept: Vec<&(f64, &TickProfile)> = kept.iter().filter(same).collect();
    let every = crate::analysis_take::PROFILE_EVERY_S;
    let from_s = kept.first()?.0 - every;
    let to_s = kept.last()?.0;
    let each: Vec<&TickProfile> = kept.iter().map(|k| k.1).collect();
    let beats: usize = each.iter().map(|p| p.beats).sum();
    let n = first.median.len();
    let weight = |p: &TickProfile| p.beats.max(1) as f32 / beats.max(1) as f32;
    let median: Vec<f32> = (0..n)
        .map(|k| each.iter().map(|p| p.median[k] * weight(p)).sum())
        .collect();
    let p10 = (0..n)
        .map(|k| each.iter().map(|p| p.p10[k]).fold(f32::INFINITY, f32::min))
        .collect();
    let p90 = (0..n)
        .map(|k| {
            each.iter()
                .map(|p| p.p90[k])
                .fold(f32::NEG_INFINITY, f32::max)
        })
        .collect();
    // The marks, measured on the averaged curve at the stride it was kept
    // at, as `from_snippets` measures them on a template.
    let fs = 1000.0 / first.step_ms;
    let origin = (-first.t0_ms / first.step_ms).round().max(0.0) as usize;
    let mut p = TickProfile {
        side,
        beats,
        t0_ms: first.t0_ms,
        step_ms: first.step_ms,
        floor: 0.0,
        unlock_ms: None,
        drop_ms: None,
        peak_ms: None,
        amplitude_deg: None,
        sound1_ms: None,
        sound2_ms: None,
        sound3_ms: None,
        median: Vec::new(),
        p10,
        p90,
    };
    mark(&mut p, (&median, fs, origin), osc_period_s, cfg, None);
    p.median = median;
    Some(SideAverage {
        profile: p,
        stretches: each.len(),
        from_s,
        to_s,
    })
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

    /// Twenty seconds of a synthetic watch at 280°, kept as an
    /// analysis-only take keeps it: both sides' profiles every 2 s.
    fn kept_stretches() -> (Vec<crate::analysis_take::StretchProfiles>, Vec<Beat>, f64) {
        let sc = SynthConfig {
            duration_s: 20.0,
            ..SynthConfig::default()
        };
        let audio = generate(&sc, |_| 280.0, |_| 0.0);
        let fs = audio.sample_rate as f64;
        let cfg = AnalysisConfig::default();
        let a = analyze(&audio, &cfg);
        let osc = 2.0 * 3600.0 / a.summary.bph as f64;
        let env = envelope(&audio.samples, fs, &cfg.envelope);
        let kept = (1..=10)
            .map(|k| {
                let end_s = 2.0 * k as f64;
                crate::analysis_take::StretchProfiles {
                    end_s,
                    signal: None,
                    sides: profiles(&env, fs, &a.beats, osc, end_s - 2.0, end_s, &cfg.amplitude),
                }
            })
            .collect();
        (kept, a.beats, osc)
    }

    /// The Average Over window from kept 2 s profiles (#979): the
    /// stretches in it averaged by their beats, the band the range of
    /// theirs, the marks measured on the average; stretches in a mask or
    /// across a break culled, never filled.
    #[test]
    fn a_window_of_kept_profiles_averages_the_stretches_in_it() {
        let (kept, beats, osc) = kept_stretches();
        let cfg = AmplitudeConfig::default();
        let at = |window: (f64, f64), masked: &[(f64, f64)], breaks: &[crate::breaks::Break]| {
            profile_at(&kept, window, masked, breaks, &beats, osc, &cfg)
        };
        let ProfileAt::Averaged(sides) = at((20.0, 10.0), &[], &[]) else {
            panic!("no profile");
        };
        let used = &kept[5..];
        for (i, a) in sides.iter().enumerate() {
            let a = a.as_ref().expect("both sides");
            assert_eq!((a.stretches, a.from_s, a.to_s), (5, 10.0, 20.0));
            let p = &a.profile;
            let each: Vec<&TickProfile> =
                used.iter().map(|s| s.sides[i].as_ref().unwrap()).collect();
            assert_eq!(p.beats, each.iter().map(|e| e.beats).sum::<usize>());
            let k = p.median.len() / 2;
            let weighted = |f: &dyn Fn(&TickProfile) -> f64| {
                each.iter().map(|e| f(e) * e.beats as f64).sum::<f64>() / p.beats as f64
            };
            let want = weighted(&|e| e.median[k] as f64) as f32;
            assert!((p.median[k] - want).abs() < 1e-5 * want.abs().max(1.0));
            let lo = each.iter().map(|e| e.p10[k]).fold(f32::INFINITY, f32::min);
            let hi = each
                .iter()
                .map(|e| e.p90[k])
                .fold(f32::NEG_INFINITY, f32::max);
            assert_eq!((p.p10[k], p.p90[k]), (lo, hi));
            // The marks on the average read what the stretches read.
            let amp = p.amplitude_deg.expect("an amplitude");
            let theirs = weighted(&|e| e.amplitude_deg.unwrap());
            assert!(
                (amp - theirs).abs() < 2.0,
                "amplitude {amp}, theirs {theirs}"
            );
            let (u, d) = (p.unlock_ms.unwrap(), p.drop_ms.unwrap());
            let theirs = weighted(&|e| e.drop_ms.unwrap());
            assert!((d - theirs).abs() < 0.2, "drop {d}, theirs {theirs}");
            assert!(u < d && p.sound3_ms.is_some());
        }
        let count = |r: ProfileAt| match r {
            ProfileAt::Averaged(sides) => sides.map(|s| s.map_or(0, |s| s.stretches)),
            other => panic!("{other:?}"),
        };
        // A stretch with no tock counts for the tick alone.
        let mut no_tock = kept.clone();
        no_tock[7].sides[1] = None;
        let one_side = profile_at(&no_tock, (20.0, 10.0), &[], &[], &beats, osc, &cfg);
        assert_eq!(count(one_side), [5, 4]);
        // Shorter than a stretch: the one stretch ending there.
        assert_eq!(count(at((20.0, 0.5), &[], &[])), [1, 1]);
        // Off the 2 s grid, a stretch belongs where its midpoint is (#636,
        // Walter): one in 2 s, five in 10 s, wherever the window ends.
        assert_eq!(count(at((19.3, 2.0), &[], &[])), [1, 1]);
        assert_eq!(count(at((19.3, 10.0), &[], &[])), [5, 5]);
        let ProfileAt::Averaged(sides) = at((19.3, 2.0), &[], &[]) else {
            panic!("no profile");
        };
        let s = sides[0].as_ref().unwrap();
        assert_eq!((s.from_s, s.to_s), (18.0, 20.0));
        // That one stretch masked: masked.
        assert!(matches!(
            at((19.3, 2.0), &[(19.5, 19.8)], &[]),
            ProfileAt::Masked
        ));
        // A mask in 13 to 13.5 s and a break at 15.5 s cull 12-14 and 14-16.
        let brk = crate::breaks::Break {
            audio_s: 15.5,
            gap_s: 1.0,
        };
        assert_eq!(count(at((20.0, 10.0), &[(13.0, 13.5)], &[brk])), [3, 3]);
        // All of it culled: masked; nothing kept there: not kept.
        assert!(matches!(
            at((20.0, 4.0), &[(16.5, 17.0), (18.5, 19.0)], &[]),
            ProfileAt::Masked
        ));
        assert!(matches!(
            at((20.0, 10.0), &[(19.0, 20.5)], &[]),
            ProfileAt::Masked
        ));
        assert!(matches!(at((60.0, 10.0), &[], &[]), ProfileAt::NotKept));
        // No beats heard in 10 to 14 s, with beats either side: no beats.
        let quiet: Vec<Beat> = beats
            .iter()
            .filter(|b| !(10.0..14.0).contains(&b.time))
            .cloned()
            .collect();
        let none = profile_at(&[], (14.0, 4.0), &[], &[], &quiet, osc, &cfg);
        assert!(matches!(none, ProfileAt::NoBeats), "{none:?}");
    }

    #[test]
    fn a_whole_history_thinned_once_keeps_what_thinning_as_it_grew_keeps() {
        // 80 h of sound every 2 s, as an analysis-only take keeps it.
        let all: Vec<(f64, ())> = (0..=144_000).map(|k| (2.0 * k as f64, ())).collect();
        let mut grown = Vec::new();
        for &e in &all {
            grown.push(e);
            thin_sound(&mut grown);
        }
        let mut once = all.clone();
        thin_sound(&mut once);
        assert!(once.len() < 7_000, "{}", once.len());
        let times = |h: &[(f64, ())]| h.iter().map(|e| e.0).collect::<Vec<_>>();
        assert_eq!(times(&once), times(&grown));
    }

    /// The profile for a moment is the sound kept for it, coarse before the
    /// last hour; or why there is none: masked (the window, or the sound
    /// kept for it), no beats where the beats heard reach, or not kept.
    #[test]
    fn the_sound_at_a_moment_is_kept_masked_beatless_or_not_kept() {
        let mut h: Vec<(f64, u32)> = (0..=3000).map(|k| (2.0 * k as f64, k)).collect();
        thin_sound(&mut h);
        let beat = |t: f64| Beat {
            index: (t * 8.0) as i64,
            time: t,
            quality: 1.0,
        };
        // Beats from 0 s to 6000 s, with none from 2000 s to 2100 s.
        let beats: Vec<Beat> = (0..48_000)
            .map(|k| k as f64 / 8.0)
            .filter(|t| !(2000.0..2100.0).contains(t))
            .map(beat)
            .collect();
        let kept = |at: f64, coarse: bool| SoundAt::Kept {
            at,
            coarse,
            sound: &h.iter().find(|e| e.0 == at).unwrap().1,
        };
        // Each sound kept over the 10 s before its time.
        fn at<'a>(
            h: &'a [(f64, u32)],
            end: f64,
            masked: &[(f64, f64)],
            beats: &[Beat],
        ) -> SoundAt<'a, u32> {
            sound_at(h, (end, 10.0), masked, beats, |_| 10.0)
        }
        assert_eq!(at(&h, 5999.0, &[], &beats), kept(5998.0, false));
        // Before the last hour the sound is kept once a minute.
        assert_eq!(at(&h, 1290.0, &[], &beats), kept(1260.0, true));
        // A window wholly masked, the sound kept for it in a mask, or made
        // over a window that reaches into one (6 s of 5094 to 5104 here).
        let masked = [(5000.0, 5100.0)];
        assert_eq!(at(&h, 5050.0, &masked, &beats), SoundAt::Masked);
        assert_eq!(at(&h, 5101.0, &masked, &beats), SoundAt::Masked);
        assert_eq!(at(&h, 5104.0, &masked, &beats), SoundAt::Masked);
        // The moment itself just inside a mask, its window mostly before
        // it and a sound kept from before the mask nearest: masked.
        let early: Vec<(f64, u32)> = h.iter().filter(|e| e.0 < 5000.0).copied().collect();
        assert_eq!(at(&early, 5000.5, &masked, &beats), SoundAt::Masked);
        // A mask that today's window reaches into but the 10 s the sound
        // was kept over doesn't: clear.
        let partly = sound_at(&h, (5112.0, 20.0), &masked, &beats, |_| 10.0);
        assert_eq!(partly, kept(5112.0, false));
        // Kept over a shorter window then (Average Over 2 s): what it was
        // made over decides, not today's window.
        let short = sound_at(&h, (5104.0, 10.0), &masked, &beats, |_| 2.0);
        assert_eq!(short, kept(5104.0, false));
        // Nothing kept: no beats in the window where the beats reach, else not kept.
        let gappy: Vec<(f64, u32)> = h.iter().filter(|e| e.0 < 2000.0).copied().collect();
        assert_eq!(at(&gappy, 2080.0, &[], &beats), SoundAt::NoBeats);
        assert_eq!(at(&gappy, 2200.0, &[], &beats), SoundAt::NotKept);
        assert_eq!(at(&h, 9000.0, &[], &beats), SoundAt::NotKept);
        assert_eq!(at(&h, -5.0, &[], &[]), SoundAt::NotKept);
    }
}
