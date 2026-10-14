//! Finding every beat.
//!
//! Two passes. A first pass tracks the loudest point of each beat on the
//! envelope, keeping each side on one sound, and builds a median beat
//! template from it with each side's beats moved onto their drop. The
//! second pass correlates the whole envelope with that template and tracks
//! the correlation peaks, which is far more robust than picking maxima: a
//! beat whose unlock happens to be louder than its drop still lines up with
//! the template as a whole. The second pass is run again with a template
//! rebuilt from its own beats.

use crate::dsp::{argmax, correlate, median_f32, moving_average, parabolic};
use realfft::RealFftPlanner;
use serde::Serialize;

/// Standard beat rates in vibrations (beats) per hour.
pub const STANDARD_BPH: [u32; 11] = [
    12000, 14400, 17280, 18000, 19800, 21600, 25200, 28800, 36000, 43200, 72000,
];

/// Guess the beat rate from the envelope's autocorrelation.
///
/// Every standard rate is scored by the autocorrelation at its beat
/// period. A full oscillation (tick to tick) correlates better than a beat
/// (tick to toc), so half the true rate can score best. A rate faster than
/// the true one lands its lag in the silence between beats and scores near
/// zero or below, so the fastest rate scoring at least half the best wins.
pub fn guess_bph(env: &[f32], fs: f64) -> u32 {
    let n = env.len().min((fs * 10.0) as usize);
    // Smooth over 2 ms so beat error and jitter don't split the peaks.
    let smooth = crate::dsp::moving_average(&env[..n], (0.002 * fs) as usize);
    let seg = &smooth[..];
    let mean = seg.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let l = (2 * n).next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(l);
    let inv = planner.plan_fft_inverse(l);
    let mut buf = vec![0.0f64; l];
    for (b, &v) in buf.iter_mut().zip(seg) {
        *b = v as f64 - mean;
    }
    let mut spec = fwd.make_output_vec();
    fwd.process(&mut buf, &mut spec).expect("fft");
    for c in spec.iter_mut() {
        *c = realfft::num_complex::Complex::new(c.norm_sqr(), 0.0);
    }
    inv.process(&mut spec, &mut buf).expect("ifft");
    let ac = |lag: f64| -> f64 {
        // Allow a little slack around the nominal lag for a fast or slow
        // watch, and for beat error, which moves the tick-to-toc lag by
        // the beat error either way (Witschi's example fault is 3 ms).
        let c = lag.round() as usize;
        let w = (lag * 0.004 + 0.004 * fs).ceil() as usize + 1;
        (c.saturating_sub(w)..=c + w)
            .filter_map(|i| buf.get(i).copied())
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let scores: Vec<(u32, f64)> = STANDARD_BPH
        .iter()
        .filter(|&&b| 3600.0 / b as f64 * fs * 1.1 < n as f64 / 2.0)
        .map(|&b| (b, ac(3600.0 / b as f64 * fs)))
        .collect();
    let best = scores.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
    scores
        .iter()
        .filter(|s| s.1 >= 0.5 * best)
        .map(|s| s.0)
        .max()
        .unwrap_or(28800)
}

/// One detected beat.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Beat {
    /// Beat number from the start, counting any skipped beats.
    pub index: i64,
    /// Time of the beat's reference point (the drop), seconds.
    pub time: f64,
    /// Correlation with the beat template at that point, relative to the
    /// median over the recording; ~1 for a typical beat, near 0 when lost.
    pub quality: f32,
}

/// Track one peak per beat in `x`, starting near the strongest peak of the
/// first beat-and-a-half. A simple phase-locked loop follows rate changes.
///
/// With a `gate` (seconds), each beat after the first two is the highest
/// peak within that distance of where the same side's last beat predicts
/// it, so each side stays on one sound even when another sound of the beat
/// is about as loud. The whole search span is used instead when it holds a
/// peak more than twice as high, so a lost track finds the beats again.
///
/// Without a gate, once the track is running, a beat whose best match in
/// the span is too far from where it is due (a knock) is looked for within
/// [`BEAT_WINDOW_S`] of where it is due instead, and taken from there if it
/// matches at least [`BEAT_WINDOW_FLOOR`] of that side's typical beat, so
/// the knock does not cost the beat.
fn track_peaks(x: &[f32], fs: f64, beat: f64, gate: Option<f64>) -> Vec<(f64, f32)> {
    let n = x.len();
    let start_span = (beat * fs * 1.5) as usize;
    let Some((first, _)) = argmax(x, 0, start_span) else {
        return Vec::new();
    };
    // Found peaks with the number of the beat each one is, and whether
    // later beats may be predicted from it (not from one found by the beat
    // window, which may be a knock's tail where the track was already off).
    let mut out: Vec<(f64, f32, i64, bool)> = Vec::new();
    let mut per = beat * fs;
    let mut pred = first as f64;
    let mut k = 0i64;
    let mut misses = 0;
    // A slow average of each side's match, for the beat window's floor.
    let mut typical = [0.0f32; 2];
    let half = 0.3 * beat * fs;
    while pred + half < n as f64 {
        let a = (pred - half).max(0.0) as usize;
        let b = (pred + half) as usize;
        let Some((mut i, mut v)) = argmax(x, a, b) else {
            break;
        };
        let mut rescued = false;
        if let Some(g) = gate.filter(|_| out.len() >= 2) {
            let g = g * fs;
            let near = argmax(x, (pred - g).max(0.0) as usize, (pred + g) as usize + 1);
            if let Some((j, w)) = near.filter(|&(_, w)| 2.0 * w >= v) {
                (i, v) = (j, w);
            }
        } else if out.len() >= 4
            && typical[side(k)] > 0.0
            && (i as f64 - pred).abs() >= MAX_OFFSET * per
        {
            // The best match is too far from where the beat is due to be
            // the beat, which would cost a beat. But the beat is where it
            // is due to within a fraction of a millisecond, so a good match
            // there is the beat, hidden by a knock elsewhere in the span.
            // Only then: a window that always took a fair match near the
            // prediction would hold the track on another sound of the beat
            // once a knock had put it there. The best point must be a peak,
            // not the window's edge on the flank of something outside it.
            let g = BEAT_WINDOW_S * fs;
            let (lo, hi) = ((pred - g).max(1.0) as usize, (pred + g) as usize);
            let near = argmax(x, lo, hi + 1).filter(|&(j, w)| {
                j > lo && j < hi && j + 1 < n && w >= BEAT_WINDOW_FLOOR * typical[side(k)]
            });
            if let Some((j, w)) = near {
                (i, v) = (j, w);
                rescued = true;
            }
        }
        let t = parabolic(x, i);
        // Once the correlation track is running, a peak far from where the
        // beat is due is a knock or other noise, not the beat: count the
        // beat as missed rather than let it pull the track off by a beat.
        // After MAX_MISSES in a row the track takes what it finds, to
        // recover. Pass 1 (with a gate) only feeds a median template, so it
        // takes every peak.
        if gate.is_some()
            || out.len() < 4
            || (t - pred).abs() < MAX_OFFSET * per
            || misses >= MAX_MISSES
        {
            let anchors = out.iter().rev().filter(|o| o.3).take(3);
            if let Some(&(t2, ..)) = anchors.clone().find(|o| o.2 == k - 2 && !rescued) {
                // Update on the tick-to-tick interval so beat error doesn't pull the loop.
                let meas = (t - t2) / 2.0;
                if (meas - per).abs() < 0.05 * per {
                    per = 0.9 * per + 0.1 * meas;
                }
            }
            out.push((t, v, k, !rescued));
            misses = 0;
            let ty = &mut typical[side(k)];
            *ty = if *ty > 0.0 { 0.95 * *ty + 0.05 * v } else { v };
        } else {
            misses += 1;
        }
        k += 1;
        // Predict beat k from the same side's last beat once there are
        // three beats, so beat error does not enter the prediction.
        let mut anchors = out.iter().rev().filter(|o| o.3);
        let same_side = anchors.clone().take(3).find(|o| o.2 == k - 2);
        pred = match same_side.filter(|_| out.len() > 2) {
            Some(&(t2, ..)) => t2 + 2.0 * per,
            None => match anchors.next() {
                Some(&(t1, _, k1, _)) => t1 + (k - k1) as f64 * per,
                None => pred + per,
            },
        };
    }
    out.into_iter().map(|(t, v, ..)| (t / fs, v)).collect()
}

/// How far from where it is due a beat may be found once the track is
/// running, as a fraction of the beat period. Beat-to-beat changes in
/// timing are far smaller; a knock lands anywhere.
const MAX_OFFSET: f64 = 0.1;

fn side(k: i64) -> usize {
    k.rem_euclid(2) as usize
}

/// Half-width of the window around where a beat is due that pass 2
/// searches when a knock outmatches the beat, seconds. Beat-to-beat
/// changes in timing are tenths of a millisecond; professional
/// timegraphers gate to about 2 ms.
const BEAT_WINDOW_S: f64 = 0.002;
/// How well the best match in that window must compare with the typical
/// beat's to be taken as the beat; below it the whole span is searched.
const BEAT_WINDOW_FLOOR: f32 = 0.5;
/// Beats in a row that may be missed before the track takes whatever it
/// finds, so it can recover from a real jump.
const MAX_MISSES: usize = 16;

/// Window around each beat used for templates: from `PRE_S` before the
/// drop to `POST_S` after it. 20 ms before covers the unlock down to
/// about 150 degrees of amplitude at common beat rates.
pub const PRE_S: f64 = 0.020;
pub const POST_S: f64 = 0.008;

/// Timing scatter, as a fraction of the beat period, at or above which no
/// regular beat is found. A watch's beats sit within a millisecond of a
/// straight line (0.7 ms at most in every stored take, 0.6% of the beat);
/// on an empty stand the tracker still finds a "beat" in each window, but
/// anywhere in it, and the scatter is 5% of the beat (8.1 and 8.6 ms in the
/// silence take at 21,600). 2% leaves a factor of three on each side.
pub const NO_WATCH_JITTER_FRACTION: f64 = 0.02;

/// False when a regular beat was looked for and not found: the median
/// timing scatter in the rate windows (see [`crate::timing::WindowFit`])
/// is at or above [`NO_WATCH_JITTER_FRACTION`] of a beat, or too few beats
/// were found to fit at all. The readers apply it only with 10 s of sound
/// or more; see [`crate::measure::watch_heard`].
pub fn watch_heard(jitter_us: Option<f64>, bph: u32) -> bool {
    timing_scatter_fraction(jitter_us, bph).is_some_and(|f| f < NO_WATCH_JITTER_FRACTION)
}

/// The timing scatter as a share of the beat period (0 to 1): `jitter_us`
/// over the beat period in microseconds, 3.6e9 / `bph`.
pub fn timing_scatter_fraction(jitter_us: Option<f64>, bph: u32) -> Option<f64> {
    jitter_us.map(|j| j / (3.6e9 / bph.max(1) as f64))
}

/// The words for no regular beat found, from the timing scatter in µs
/// and as a share of the beat (see [`timing_scatter_fraction`]): "No
/// regular beat found (timing scatter 8.6 ms, 5.2% of a beat)". Without a
/// scatter (too few beats to fit), just the fact.
pub fn no_regular_beat_text(jitter_us: Option<f64>, fraction: Option<f64>) -> String {
    match jitter_us.zip(fraction) {
        Some((j, f)) => format!(
            "No regular beat found (timing scatter {:.1} ms, {:.1}% of a beat)",
            j / 1000.0,
            f * 100.0
        ),
        None => "No regular beat found (too few beats to time)".to_string(),
    }
}

/// Median of the windows around the given beat times (sample by sample).
pub fn median_template(env: &[f32], fs: f64, times: &[f64]) -> Vec<f32> {
    median_window(env, fs, times, PRE_S, POST_S)
}

/// Share of beats dropped from each end, point by point, when averaging
/// beats into the template that amplitude, beat error from the unlock and
/// the tick and tock profiles are measured on. Keeping the middle half
/// ignores stray clicks as a median does but averages away more hiss.
pub const TEMPLATE_TRIM: f64 = 0.25;

/// Trimmed mean (see [`TEMPLATE_TRIM`]) of the windows from `PRE_S` before
/// to `POST_S` after each time.
pub fn trimmed_template(env: &[f32], fs: f64, times: &[f64]) -> Vec<f32> {
    window_stat(env, fs, times, PRE_S, POST_S, |c| {
        crate::dsp::trimmed_mean_f32(c, TEMPLATE_TRIM)
    })
}

/// Median of the windows from `pre_s` before to `post_s` after each time.
/// Windows that run off either end of the envelope are left out.
pub fn median_window(env: &[f32], fs: f64, times: &[f64], pre_s: f64, post_s: f64) -> Vec<f32> {
    window_stat(env, fs, times, pre_s, post_s, median_f32)
}

fn window_stat(
    env: &[f32],
    fs: f64,
    times: &[f64],
    pre_s: f64,
    post_s: f64,
    stat: impl Fn(&mut [f32]) -> f32,
) -> Vec<f32> {
    let pre = (pre_s * fs).round() as usize;
    let post = (post_s * fs).round() as usize;
    let len = pre + post;
    let starts: Vec<usize> = times
        .iter()
        .filter_map(|&t| {
            let c = (t * fs).round() as isize - pre as isize;
            (c >= 0 && c as usize + len <= env.len()).then_some(c as usize)
        })
        .collect();
    let mut col = vec![0.0f32; starts.len()];
    (0..len)
        .map(|k| {
            for (c, &s) in col.iter_mut().zip(&starts) {
                *c = env[s + k];
            }
            stat(&mut col)
        })
        .collect()
}

/// Find every beat in an envelope.
///
/// Pass 1 follows the envelope's maxima to build a template. The loudest
/// sound of a beat can change from beat to beat when two of its sounds are
/// about as loud, and a template built on whichever is loudest is a blend
/// of beats aligned on different sounds; on a watch whose two sides differ
/// in shape, its correlation peaks then land on different sounds from beat
/// to beat. So pass 1 keeps each side on one sound, which can be a different
/// one on each side, and each side's beats are moved onto that side's drop
/// before the template is built.
pub fn detect(env: &[f32], fs: f64, bph: u32) -> (Vec<Beat>, Vec<f32>) {
    let beat = 3600.0 / bph as f64;
    // Pass 1: maxima, used only to build the template.
    let coarse = track_peaks(env, fs, beat, Some(PASS1_GATE_S));
    let origin = (PRE_S * fs).round() as usize;
    let mut sample = Vec::new();
    for side in 0..2 {
        let times: Vec<f64> = coarse.iter().skip(side).step_by(2).map(|c| c.0).collect();
        let step = (times.len() / 1000).max(1);
        let times: Vec<f64> = times.into_iter().step_by(step).collect();
        let drop = if times.len() >= 3 {
            let wide = median_window(env, fs, &times, PRE_S, PRE_S);
            drop_offset(&wide, fs, origin).unwrap_or(0.0)
        } else {
            0.0
        };
        sample.extend(times.iter().map(|t| t + drop));
    }
    let template = median_template(env, fs, &sample);
    let beats = detect_with_template(env, fs, bph, &template);
    // Pass 2 lines the beats up more closely than the maxima did, so a
    // template rebuilt from its beats is sharper for the final pass.
    let good: Vec<f64> = beats
        .iter()
        .filter(|b| b.quality > 0.4)
        .map(|b| b.time)
        .collect();
    let step = (good.len() / 2000).max(1);
    let sample: Vec<f64> = good.into_iter().step_by(step).collect();
    if sample.len() < 10 {
        return (beats, template);
    }
    let template = median_template(env, fs, &sample);
    (detect_with_template(env, fs, bph, &template), template)
}

/// How far from its last beat pass 1 looks for a side's next one, seconds.
/// Sounds within a beat are further apart than this, and the beat-to-beat
/// change in timing much less.
const PASS1_GATE_S: f64 = 0.001;

/// Where the drop sits on a template, seconds from `origin`: the last
/// sound that reaches 60% of the template's highest peak above the floor
/// and is separated from the sound before it by a dip below half the lower
/// of the two. A drop's lumpy tail does not count, since it neither stands
/// that high nor dips that far.
fn drop_offset(template: &[f32], fs: f64, origin: usize) -> Option<f64> {
    let t = moving_average(template, ((0.0002 * fs) as usize).max(1));
    let quiet = ((0.003 * fs) as usize).min(t.len());
    let mut floor_v = t[..quiet].to_vec();
    let floor = median_f32(&mut floor_v);
    let (top, top_v) = argmax(&t, quiet, t.len())?;
    let h_top = top_v - floor;
    if h_top <= 0.0 {
        return None;
    }
    let mut drop = top;
    for j in top + 1..t.len().saturating_sub(1) {
        let h = t[j] - floor;
        if h < 0.6 * h_top || t[j] < t[j - 1] || t[j] <= t[j + 1] {
            continue;
        }
        let dip = t[drop..j].iter().copied().fold(f32::INFINITY, f32::min) - floor;
        if dip <= 0.5 * h.min(t[drop] - floor) {
            drop = j;
        }
    }
    Some((parabolic(&t, drop) - origin as f64) / fs)
}

/// Find every beat by correlating with a given template. Long recordings
/// are processed in chunks with one template throughout, so the beat's
/// reference point cannot shift from one chunk to the next.
pub fn detect_with_template(env: &[f32], fs: f64, bph: u32, template: &[f32]) -> Vec<Beat> {
    let beat = 3600.0 / bph as f64;
    // Pass 2: correlate with the zero-mean template and track its peaks.
    let mean = template.iter().sum::<f32>() / template.len().max(1) as f32;
    let zm: Vec<f32> = template.iter().map(|v| v - mean).collect();
    let corr = correlate(env, &zm);
    let pre = PRE_S * fs;
    let peaks = track_peaks(&corr, fs, beat, None);
    let mut q: Vec<f32> = peaks.iter().map(|p| p.1).collect();
    let typical = median_f32(&mut q).max(f32::MIN_POSITIVE);

    let mut beats = Vec::with_capacity(peaks.len());
    let mut index = 0i64;
    for (k, &(t, v)) in peaks.iter().enumerate() {
        if k > 0 {
            index += ((t - peaks[k - 1].0) / beat).round().max(1.0) as i64;
        }
        beats.push(Beat {
            index,
            time: t + pre / fs,
            quality: v / typical,
        });
    }
    beats
}

/// The sound of each side of the beat over a stretch: the median envelope
/// around the even-numbered beats and around the odd-numbered ones, each
/// scaled to a peak of 1. Tick and Tock seldom sound the same, so after a
/// jump in the beats (listening paused, the computer suspended) the sides
/// are told apart by their sound rather than by counting.
#[derive(Debug, Clone, PartialEq)]
pub struct SideSounds {
    pub even: Vec<f32>,
    pub odd: Vec<f32>,
}

/// Beats on each side a stretch needs before its sides are compared.
pub const MIN_SIDE_BEATS: usize = 16;
/// Beats on each side a comparison reads at most, from the stretch's start.
const MAX_SIDE_BEATS: usize = 160;

/// How a jump's stretch compares with the side sounds before it: the
/// correlation of each side with the same side before, less its correlation
/// with the other side, summed over both sides. Positive when the sides kept
/// their names, negative when they swapped. Over 20 s of every stored take
/// it is 0.03 or more (0.03 to 0.42); the stretch after the suspend in the
/// crown-right Yacht-Master take reads -0.15.
pub fn side_margin(before: &SideSounds, after: &SideSounds) -> f64 {
    let r = |a: &[f32], b: &[f32]| pearson(a, b);
    (r(&after.even, &before.even) + r(&after.odd, &before.odd))
        - (r(&after.even, &before.odd) + r(&after.odd, &before.even))
}

/// Below this margin in size the sides of a stretch are not told apart.
pub const SIDE_MARGIN_MIN: f64 = 0.02;

/// The side sounds of `beats` (times in seconds on `env`), or `None` when
/// either side has fewer than [`MIN_SIDE_BEATS`] good beats.
pub fn side_sounds(env: &[f32], fs: f64, beats: &[Beat]) -> Option<SideSounds> {
    let side = |even: bool| -> Option<Vec<f32>> {
        let times: Vec<f64> = beats
            .iter()
            .filter(|b| (b.index.rem_euclid(2) == 0) == even && b.quality > 0.5)
            .take(MAX_SIDE_BEATS)
            .map(|b| b.time)
            .collect();
        if times.len() < MIN_SIDE_BEATS {
            return None;
        }
        let mut w = median_template(env, fs, &times);
        let top = w.iter().copied().fold(0.0f32, f32::max);
        if w.is_empty() || top <= 0.0 {
            return None;
        }
        w.iter_mut().for_each(|v| *v /= top);
        Some(w)
    };
    Some(SideSounds {
        even: side(true)?,
        odd: side(false)?,
    })
}

impl SideSounds {
    /// Half this and half `other`, point by point: the side sounds kept
    /// up to date as a long take goes on.
    pub fn blend(&mut self, other: &SideSounds) {
        for (a, b) in [(&mut self.even, &other.even), (&mut self.odd, &other.odd)] {
            if a.len() == b.len() {
                a.iter_mut().zip(b).for_each(|(x, y)| *x = 0.5 * (*x + y));
            }
        }
    }
}

/// A step in the beats' timing of more than this, on both sides, is a
/// jump, seconds. Where listening stopped and started again the step is
/// anywhere up to half a beat (62.5 ms at 28,800), so 4 ms misses about one
/// hole in sixteen; the largest step both sides make together in a stored
/// take with no hole is 2.3 ms (the ETA 2824 in the holder), and the
/// crown-right Yacht-Master's suspend steps 8.5 ms.
pub const JUMP_S: f64 = 0.004;
/// Beats of each side, either side of a step, whose median timing is
/// compared.
const JUMP_SPAN: usize = 5;
/// Beats from before a chunk that [`jumps`] needs to see a jump at its
/// start.
pub const JUMP_CONTEXT: usize = 2 * JUMP_SPAN + 2;

/// Positions `p` in `beats` where the timing of both sides steps by more
/// than [`JUMP_S`], and by about the same: where listening stopped and
/// started again, so the watch's phase moved on. Each side's step is the
/// median offset of its next `JUMP_SPAN` beats from where the typical
/// intervals put them, against that of its last five; a stray beat moves
/// neither median. A tracker moving from one sound of a beat to another
/// (a few milliseconds, on one side) is not a jump: the other side stays.
/// `before` are the beats just before `beats[0]` (same time base), so a
/// jump at or just before the first of `beats` is found too: as position
/// 0, when `before` runs too near its end to have seen it.
pub fn jumps(beats: &[Beat], before: &[Beat]) -> Vec<usize> {
    let all: Vec<Beat> = before.iter().chain(beats).copied().collect();
    let n = before.len();
    let mut out: Vec<usize> = jumps_in(&all)
        .into_iter()
        .filter(|&p| p + 2 * JUMP_SPAN >= n)
        .map(|p| p.saturating_sub(n))
        .collect();
    out.dedup();
    out
}

fn jumps_in(beats: &[Beat]) -> Vec<usize> {
    // The typical interval after an even beat and after an odd one: they
    // differ by twice the beat error.
    let mut after = [Vec::new(), Vec::new()];
    for p in beats.windows(2) {
        if p[1].index == p[0].index + 1 {
            after[p[0].index.rem_euclid(2) as usize].push(p[1].time - p[0].time);
        }
    }
    if after.iter().any(|v| v.len() < MIN_SIDE_BEATS) {
        return Vec::new();
    }
    let med = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    };
    let step = [med(&mut after[0]), med(&mut after[1])];
    // Beats that scatter like noise (an empty stand) have no timing to
    // jump from: the median interval's spread must be under a quarter of a
    // jump, where a watch's is a tenth of a millisecond or two.
    let mut spread: Vec<f64> = (0..2)
        .flat_map(|s| after[s].iter().map(move |v| (v - step[s]).abs()))
        .collect();
    if med(&mut spread) > 0.25 * JUMP_S {
        return Vec::new();
    }
    // Where beat `b` is due, counted on from beat `a`.
    let due = |a: &Beat, b: &Beat| -> f64 {
        let n = b.index - a.index;
        let first = a.index.rem_euclid(2) as usize;
        let (n_first, n_other) = ((n + 1) / 2, n / 2);
        a.time + n_first as f64 * step[first] + n_other as f64 * step[1 - first]
    };
    let origin = &beats[0];
    let off: Vec<f64> = beats.iter().map(|b| b.time - due(origin, b)).collect();
    let side = |p: usize| beats[p].index.rem_euclid(2);
    // The median offset of the first `JUMP_SPAN` beats of side `s` from
    // position `p` on (forward) or before it (back), when the middle three
    // sit within half a jump of one another: beats scattered wider than
    // that (no watch, or a side moving between two of its sounds) say
    // nothing, while one stray beat does not stop a jump being seen.
    let span = |p: usize, s: i64, forward: bool| -> Option<f64> {
        let mut v: Vec<f64> = if forward {
            (p..off.len())
                .filter(|&q| side(q) == s)
                .take(JUMP_SPAN)
                .map(|q| off[q])
                .collect()
        } else {
            (0..p)
                .rev()
                .filter(|&q| side(q) == s)
                .take(JUMP_SPAN)
                .map(|q| off[q])
                .collect()
        };
        if v.len() < JUMP_SPAN {
            return None;
        }
        let m = med(&mut v);
        (v[JUMP_SPAN - 2] - v[1] <= 0.5 * JUMP_S).then_some(m)
    };
    // Both sides' steps at `p`, when both sides have the beats to tell.
    let steps = |p: usize| -> Option<(f64, f64)> {
        let at = |s: i64| -> Option<f64> {
            let back = if p == 0 { 0.0 } else { span(p, s, false)? };
            Some(span(p, s, true)? - back)
        };
        Some((at(0)?, at(1)?))
    };
    let jumped = |(a, b): (f64, f64)| {
        a.abs() > JUMP_S && b.abs() > JUMP_S && (a - b).abs() <= 0.5 * a.abs().max(b.abs())
    };
    let size = |p: usize| steps(p).map_or(0.0, |(a, b)| 0.5 * (a + b).abs());
    let mut out = Vec::new();
    let mut p = 1;
    while p < off.len() {
        if steps(p).is_some_and(jumped) {
            // The step shows at several positions near it: take the
            // largest, then look on past it.
            let best = (p..(p + 2 * JUMP_SPAN).min(off.len()))
                .filter(|&q| steps(q).is_some_and(jumped))
                .max_by(|&x, &y| size(x).total_cmp(&size(y)))
                .unwrap_or(p);
            // The jump is at the first beat from which the next few all sit
            // nearer the timing after it than the timing before it.
            let lo = best.saturating_sub(2 * JUMP_SPAN).max(1);
            let hi = (best + 2 * JUMP_SPAN).min(off.len() - 1);
            let level = |q: usize, forward: bool| span(best, side(q), forward);
            let after_it = |q: usize| match (level(q, false), level(q, true)) {
                (Some(back), Some(fore)) => (off[q] - fore).abs() < (off[q] - back).abs(),
                _ => false,
            };
            let best = (lo..=hi)
                .find(|&q| (q..(q + JUMP_SPAN).min(off.len())).all(after_it))
                .unwrap_or(best);
            out.push(best);
            p = best + 2 * JUMP_SPAN;
        } else {
            p += 1;
        }
    }
    out
}

fn pearson(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let ma = a[..n].iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let mb = b[..n].iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let (mut s, mut sa, mut sb) = (0.0, 0.0, 0.0);
    for (x, y) in a[..n].iter().zip(&b[..n]) {
        let (x, y) = (*x as f64 - ma, *y as f64 - mb);
        s += x * y;
        sa += x * x;
        sb += y * y;
    }
    if sa <= 0.0 || sb <= 0.0 {
        0.0
    } else {
        s / (sa * sb).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Beats of a watch with 0.5 ms beat error and 0.1 ms of scatter, with
    /// `shift(k, even)` added to beat `k`'s time.
    fn ticking(n: i64, shift: impl Fn(i64, bool) -> f64) -> Vec<Beat> {
        let mut rng = crate::synth::Rng::new(7);
        (0..n)
            .map(|k| {
                let even = k % 2 == 0;
                let be = if even { 0.0 } else { 0.0005 };
                Beat {
                    index: k,
                    time: 0.05 + k as f64 * 0.125 + be + 1e-4 * rng.normal() + shift(k, even),
                    quality: 1.0,
                }
            })
            .collect()
    }

    #[test]
    fn a_hole_in_the_sound_is_a_jump() {
        // From beat 200 on both sides land 30 ms later.
        let b = ticking(400, |k, _| if k >= 200 { 0.030 } else { 0.0 });
        assert_eq!(jumps(&b, &[]), vec![200]);
        // And seen from the beats before a chunk's first beat, when the
        // jump is at that beat or just before it.
        assert_eq!(jumps(&b[200..], &b[188..200]), vec![0]);
        assert_eq!(jumps(&b[203..], &b[191..203]), vec![0]);
    }

    #[test]
    fn a_stray_beat_or_one_side_changing_sound_is_not_a_jump() {
        let stray = ticking(400, |k, _| if k == 200 { 0.030 } else { 0.0 });
        assert!(jumps(&stray, &[]).is_empty());
        // Nor is a stray beat the last before a chunk starts.
        assert!(jumps(&stray[201..], &stray[189..201]).is_empty());
        // One side moves 5 ms onto another of its sounds; the other stays.
        let one_side = ticking(400, |k, even| if k >= 200 && even { -0.005 } else { 0.0 });
        assert!(jumps(&one_side, &[]).is_empty());
        // A step under JUMP_S is not one either.
        let small = ticking(400, |k, _| if k >= 200 { 0.002 } else { 0.0 });
        assert!(jumps(&small, &[]).is_empty());
    }

    #[test]
    fn beats_scattered_like_noise_have_no_jumps() {
        let noise = ticking(400, |k, _| {
            0.008 * rng_normal(k) + if k >= 200 { 0.030 } else { 0.0 }
        });
        assert!(jumps(&noise, &[]).is_empty());
    }

    /// A fixed pseudo-random normal-ish value for beat `k`.
    fn rng_normal(k: i64) -> f64 {
        let mut r = crate::synth::Rng::new(1000 + k as u64);
        r.normal()
    }

    #[test]
    fn side_sounds_tell_the_sides_apart() {
        // Two sides of different shape: the Tick's peak sits earlier.
        let fs = 8000.0;
        let mut env = vec![0.0f32; (20.0 * fs) as usize];
        let mut beats = Vec::new();
        for k in 0..150i64 {
            let t = 0.05 + k as f64 * 0.125;
            let lead = if k % 2 == 0 { 0.006 } else { 0.002 };
            for (dt, v) in [(-lead, 0.6f32), (0.0, 1.0)] {
                let i = ((t + dt) * fs) as usize;
                env[i] = v;
                env[i + 1] = 0.5 * v;
            }
            beats.push(Beat {
                index: k,
                time: t,
                quality: 1.0,
            });
        }
        let before = side_sounds(&env, fs, &beats[..60]).expect("sounds");
        let same = side_sounds(&env, fs, &beats[60..]).expect("sounds");
        assert!(side_margin(&before, &same) > SIDE_MARGIN_MIN);
        // Numbered one further, the sides swap and the margin turns.
        let swapped: Vec<Beat> = beats[60..]
            .iter()
            .map(|b| Beat {
                index: b.index + 1,
                ..*b
            })
            .collect();
        let other = side_sounds(&env, fs, &swapped).expect("sounds");
        assert!(side_margin(&before, &other) < -SIDE_MARGIN_MIN);
    }

    #[test]
    fn a_knock_does_not_move_the_beats() {
        // A beat every 0.125 s for 4 s, and a knock three times as loud
        // 30 ms before beat 20 is due. The beat window keeps beat 20.
        let fs = 8000.0;
        let beat = 0.125;
        let mut x = vec![0.0f32; (4.0 * fs) as usize];
        for k in 0..32 {
            let i = ((0.05 + k as f64 * beat) * fs) as usize;
            x[i - 1] = 0.5;
            x[i] = 1.0;
            x[i + 1] = 0.5;
        }
        let knock = ((0.05 + 20.0 * beat - 0.030) * fs) as usize;
        x[knock] = 3.0;
        let peaks = track_peaks(&x, fs, beat, None);
        assert_eq!(peaks.len(), 32, "beat 20 is found under the knock");
        for (t, _) in &peaks {
            let k = ((t - 0.05) / beat).round();
            assert!(
                (t - 0.05 - k * beat).abs() < 0.001,
                "peak at {t} s is off the beats"
            );
        }
    }
}
