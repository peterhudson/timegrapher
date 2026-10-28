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

/// The beats `seconds` of a watch at `bph` hold.
pub fn expected_beats(seconds: f64, bph: u32) -> f64 {
    seconds * bph as f64 / 3600.0
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
    trimmed_template_from(env, fs, times, PRE_S)
}

/// [`trimmed_template`] from `pre_s` before each time.
pub fn trimmed_template_from(env: &[f32], fs: f64, times: &[f64], pre_s: f64) -> Vec<f32> {
    window_stat(env, fs, times, pre_s, POST_S, |c| {
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
    keep_sides_on_one_sound(&corr, fs, typical, &mut beats);
    beats
}

/// Same-side beats either way of a beat that set where its side's beats
/// lie, for [`keep_sides_on_one_sound`]: about 10 s at 28,800 bph.
const SIDE_LINE_HALF: usize = 20;
/// Fewer same-side beats than this in a run, and the run is left as found.
const SIDE_LINE_MIN: usize = 9;
/// A gap in the beats found, in beats, that ends a run: the beats may have
/// jumped across it.
const SIDE_RUN_GAP: i64 = 4;
/// How far off its side's line a beat must be to have landed on another
/// sound, seconds. A watch's beats sit within a millisecond of their line
/// (see [`NO_WATCH_JITTER_FRACTION`]); a beat's sounds are a few ms apart.
const SIDE_SOUND_GAP_S: f64 = 0.003;

/// Put each beat back on the sound the rest of its side is on.
///
/// One side of a beat can have two sounds a few ms apart that both match
/// the template well; the track then lands on one on some beats and on the
/// other on the rest, a step of several ms in that side's timing. In a
/// fake-mic take of the Daytona at about 160° the Tock had a sound 6 ms
/// before its drop that outmatched the drop on 16% of beats, which put a
/// 1.9 ms line in the beat timing and swung 10 s rates by ±13 s/d. Within
/// each run of beats without a gap, each side's beats should lie along a
/// line to within tenths of a ms; a beat [`SIDE_SOUND_GAP_S`] or more off
/// the running median of its side's is moved to the best match within
/// [`BEAT_WINDOW_S`] of that line, when there is one at least
/// [`BEAT_WINDOW_FLOOR`] as good as the match it had. Most of a side's beats
/// decide which sound it is timed on, whichever came first.
fn keep_sides_on_one_sound(corr: &[f32], fs: f64, typical: f32, beats: &mut [Beat]) {
    let pre = PRE_S * fs;
    let g = BEAT_WINDOW_S * fs;
    let value = |t: f64| -> Option<f32> {
        let i = (t * fs - pre).round();
        (i >= 0.0 && (i as usize) < corr.len()).then(|| corr[i as usize])
    };
    let mut start = 0;
    while start < beats.len() {
        let mut end = start + 1;
        while end < beats.len() && beats[end].index - beats[end - 1].index <= SIDE_RUN_GAP {
            end += 1;
        }
        for side in 0..2 {
            let at: Vec<usize> = (start..end)
                .filter(|&k| beats[k].index.rem_euclid(2) == side)
                .collect();
            if at.len() < SIDE_LINE_MIN {
                continue;
            }
            let mut steps: Vec<f64> = at
                .windows(2)
                .map(|w| {
                    let (a, b) = (&beats[w[0]], &beats[w[1]]);
                    (b.time - a.time) / (b.index - a.index) as f64
                })
                .collect();
            let period = crate::dsp::median(&mut steps);
            let y: Vec<f64> = at
                .iter()
                .map(|&k| beats[k].time - beats[k].index as f64 * period)
                .collect();
            for (n, &k) in at.iter().enumerate() {
                let (lo, hi) = (
                    n.saturating_sub(SIDE_LINE_HALF),
                    (n + SIDE_LINE_HALF + 1).min(at.len()),
                );
                let mut near = y[lo..hi].to_vec();
                let line = crate::dsp::median(&mut near);
                let off = y[n] - line;
                if off.abs() < SIDE_SOUND_GAP_S {
                    continue;
                }
                let Some(v) = value(beats[k].time) else {
                    continue;
                };
                let due = (beats[k].time - off) * fs - pre;
                let (a, b) = ((due - g).max(1.0) as usize, (due + g) as usize);
                if b + 1 >= corr.len() {
                    continue;
                }
                let found = argmax(corr, a, b + 1)
                    .filter(|&(j, w)| j > a && j < b && w >= BEAT_WINDOW_FLOOR * v);
                if let Some((j, w)) = found {
                    beats[k].time = (parabolic(corr, j) + pre) / fs;
                    beats[k].quality = w / typical;
                }
            }
        }
        start = end;
    }
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

/// Beats on each side a stretch needs before its sides are compared: a
/// side check reads the first this many good beats of each side after a
/// jump, and with fewer before it the count stands.
pub const MIN_SIDE_BEATS: usize = 16;
/// Good beats of each side a side check reads before a jump: at most the
/// last this many.
pub const SIDE_BEATS: usize = 2 * MIN_SIDE_BEATS;
/// A beat whose sound a side check reads is better than this
/// ([`Beat::quality`]).
pub const SIDE_QUALITY: f32 = 0.5;
/// A beat whose sound a side check reads comes this near, ms, the typical
/// interval from the beat two before it (of its own side, so the beat
/// error doesn't enter, whichever side is which). Where the watch stumbles
/// the tracker can follow other sounds for a minute or two, beats that
/// pass for good ones but come at the wrong times; the sides read there
/// would be neither Tick nor Tock.
pub const SIDE_STEADY_MS: f64 = 1.0;
/// Intervals the typical one is the median of.
const SIDE_STEPS: usize = 32;

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

impl SideSounds {
    /// The median of each side's beat windows (each cut as
    /// [`crate::profile::snippet`] cuts it), scaled to a peak of 1, or
    /// `None` when either side has fewer than [`MIN_SIDE_BEATS`].
    pub fn of<'a>(
        even: impl IntoIterator<Item = &'a [f32]>,
        odd: impl IntoIterator<Item = &'a [f32]>,
    ) -> Option<SideSounds> {
        fn side<'a>(v: impl IntoIterator<Item = &'a [f32]>) -> Option<Vec<f32>> {
            let v: Vec<&[f32]> = v.into_iter().collect();
            let len = v.iter().map(|s| s.len()).min()?;
            if v.len() < MIN_SIDE_BEATS || len == 0 {
                return None;
            }
            let mut w: Vec<f32> = (0..len)
                .map(|i| median_f32(&mut v.iter().map(|s| s[i]).collect::<Vec<_>>()))
                .collect();
            let top = w.iter().copied().fold(0.0f32, f32::max);
            if top <= 0.0 {
                return None;
            }
            w.iter_mut().for_each(|x| *x /= top);
            Some(w)
        }
        Some(SideSounds {
            even: side(even)?,
            odd: side(odd)?,
        })
    }
}

/// What a side check found at a jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideCall {
    /// The sides after the jump sound as they did before: the count kept
    /// them.
    Kept,
    /// They sound the other way round: every beat from the jump on is
    /// numbered one further, which swaps them back.
    Swapped,
    /// Too alike to tell, by their sound and by the beat error's sign:
    /// the count stands.
    Unsure,
}

/// A good beat's window, by the beat's time.
type SideSnip = (f64, Vec<f32>);

/// Below this size, ms, a side check does not read the sides from the
/// sign of the beat error: about three times the split's scatter (about
/// 0.035 ms) over the [`MIN_SIDE_BEATS`] beats of each side a side check
/// reads after a jump, when each beat scatters by 0.1 ms.
pub const SIDE_SPLIT_MIN_MS: f64 = 0.1;

/// Half the difference between the interval from an even beat to the odd
/// beat after it and that from an odd beat to the even beat after it, ms,
/// from each side's beat times: the beat error's sign and size near the
/// drop, which turns over when the sides swap. Only next beats count (a
/// gap under one and a half times the typical one). `None` with fewer than
/// half [`MIN_SIDE_BEATS`] intervals each way.
pub fn timing_split(even: &[f64], odd: &[f64]) -> Option<f64> {
    let mut all: Vec<(f64, usize)> = even
        .iter()
        .map(|&t| (t, 0))
        .chain(odd.iter().map(|&t| (t, 1)))
        .collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut gaps: Vec<f64> = all
        .windows(2)
        .filter(|p| p[0].1 != p[1].1)
        .map(|p| p[1].0 - p[0].0)
        .collect();
    if gaps.is_empty() {
        return None;
    }
    let typical = crate::dsp::median(&mut gaps);
    let mut after = [Vec::new(), Vec::new()];
    for p in all.windows(2) {
        let d = p[1].0 - p[0].0;
        if p[0].1 != p[1].1 && d < 1.5 * typical {
            after[p[0].1].push(d);
        }
    }
    if after.iter().any(|v| v.len() < MIN_SIDE_BEATS / 2) {
        return None;
    }
    let [mut a, mut b] = after;
    Some(500.0 * (crate::dsp::median(&mut a) - crate::dsp::median(&mut b)))
}

/// The sides after a jump told apart by the sign of the beat error
/// ([`timing_split`]) before and after it: kept when it is the same,
/// swapped when it turned over, unsure when it is under
/// [`SIDE_SPLIT_MIN_MS`] either side, or changed size by more than half.
/// Each is the beat times of the even side, then the odd.
pub fn side_by_timing(before: &[Vec<f64>; 2], after: &[Vec<f64>; 2]) -> SideCall {
    let (Some(b), Some(a)) = (
        timing_split(&before[0], &before[1]),
        timing_split(&after[0], &after[1]),
    ) else {
        return SideCall::Unsure;
    };
    if b.abs() < SIDE_SPLIT_MIN_MS
        || a.abs() < SIDE_SPLIT_MIN_MS
        || (a.abs() - b.abs()).abs() > 0.5 * a.abs().max(b.abs())
    {
        return SideCall::Unsure;
    }
    if a.signum() == b.signum() {
        SideCall::Kept
    } else {
        SideCall::Swapped
    }
}

/// The one check of Tick and Tock across a jump, for a whole file and for
/// live listening alike.
///
/// Counting beats names the sides only while the beats run on without a
/// jump. Where listening stopped and started again (a pause, the computer
/// suspended, a join between two takes, the watch handled for a new
/// position) the count across the hole is a guess, and half the time it
/// swaps the sides. Tick and Tock seldom sound the same, so the first
/// [`MIN_SIDE_BEATS`] good beats of each side after the jump are compared
/// with the last [`SIDE_BEATS`] before it ([`side_margin`]). Where they
/// sound too alike to tell, the sign of the beat error before and after
/// tells them ([`side_by_timing`]), when it is large enough to have one.
/// Those are the same beats however the sound arrives, a file in chunks or
/// listening in passes, so the answer is the same too.
///
/// Feed it each good beat in time order ([`SideCheck::beat`]) and each
/// jump ([`SideCheck::jump`], which may come after beats past it were
/// fed), and ask [`SideCheck::settle`] after feeding beats. On
/// [`SideCall::Swapped`] the caller numbers its beats from the jump on one
/// further.
#[derive(Debug, Clone, Default)]
pub struct SideCheck {
    /// The last good beats of each side (even, odd) whose sides are known,
    /// newest last.
    kept: [std::collections::VecDeque<SideSnip>; 2],
    /// A jump whose sides are still to be checked: its time, and each
    /// side's good beats since.
    pending: Option<(f64, [Vec<SideSnip>; 2])>,
    /// Stretches, start and end in seconds, whose sides were never told
    /// apart: between two jumps too close for a check.
    unsided: Vec<(f64, f64)>,
    /// The jumps whose sides were told apart by their sound, seconds.
    sided: Vec<f64>,
    /// The last two good beats' times and numbers, newest last.
    last: [Option<(f64, i64)>; 2],
    /// The latest intervals, seconds, to good beats from the good beat
    /// two before.
    steps: std::collections::VecDeque<f64>,
}

impl SideCheck {
    /// A beat at `t` seconds, numbered `index`, of quality `quality`, with
    /// its window as [`crate::profile::snippet`] cuts it. Beats no better
    /// than [`SIDE_QUALITY`] are passed over, and so are beats not
    /// [`SIDE_STEADY_MS`] near the typical interval from the beat two
    /// before.
    pub fn beat(&mut self, t: f64, index: i64, quality: f32, window: &[f32]) {
        if quality <= SIDE_QUALITY {
            return;
        }
        let side = index.rem_euclid(2) as usize;
        let two_before = self.last[0];
        self.last = [self.last[1], Some((t, index))];
        let Some(step) = two_before.and_then(|(lt, li)| (index == li + 2).then_some(t - lt)) else {
            return;
        };
        let steps = &mut self.steps;
        steps.push_back(step);
        if steps.len() > SIDE_STEPS {
            steps.pop_front();
        }
        if steps.len() < MIN_SIDE_BEATS {
            return;
        }
        let typical = crate::dsp::median(&mut steps.iter().copied().collect::<Vec<f64>>());
        if (step - typical).abs() * 1000.0 > SIDE_STEADY_MS {
            return;
        }
        let snip = (t, window.to_vec());
        match &mut self.pending {
            Some((_, got)) => got[side].push(snip),
            None => {
                let kept = &mut self.kept[side];
                kept.push_back(snip);
                if kept.len() > SIDE_BEATS {
                    kept.pop_front();
                }
            }
        }
    }

    /// A jump at `t` seconds: the sides from there are checked once enough
    /// good beats of each are heard. A check still owed from an earlier
    /// jump moves here, and the stretch between the two is never told
    /// apart: that stretch is returned. Without [`MIN_SIDE_BEATS`] good
    /// beats a side before, the count stands.
    pub fn jump(&mut self, t: f64) -> Option<(f64, f64)> {
        match self.pending.take() {
            Some((t0, got)) if t0 < t => {
                self.unsided.push((t0, t));
                let got = got.map(|g| g.into_iter().filter(|s| s.0 >= t).collect());
                self.pending = Some((t, got));
                Some((t0, t))
            }
            Some((t0, got)) => {
                // Found after a later jump: the sound before is cut back to
                // this one, and the stretch between can't be told apart.
                self.unsided.push((t, t0));
                self.kept.iter_mut().for_each(|s| s.retain(|x| x.0 < t));
                let known = self.kept.iter().all(|s| s.len() >= MIN_SIDE_BEATS);
                self.pending = known.then_some((t0, got));
                Some((t, t0))
            }
            None => {
                let known = self
                    .kept
                    .iter()
                    .all(|s| s.iter().filter(|x| x.0 < t).count() >= MIN_SIDE_BEATS);
                if known {
                    let got = self.kept.each_mut().map(|s| {
                        let k = s.partition_point(|x| x.0 < t);
                        s.split_off(k).into()
                    });
                    self.pending = Some((t, got));
                }
                None
            }
        }
    }

    /// Once the beats after a pending jump have [`MIN_SIDE_BEATS`] good
    /// beats of each side, compare the first of them with the sides' sound
    /// before: the jump's time and what was found. On [`SideCall::Swapped`]
    /// the beats fed since are taken as numbered one further, as the caller
    /// now numbers them.
    pub fn settle(&mut self) -> Option<(f64, SideCall)> {
        if self
            .pending
            .as_ref()
            .is_none_or(|(_, got)| got.iter().any(|g| g.len() < MIN_SIDE_BEATS))
        {
            return None;
        }
        let (t, mut got) = self.pending.take()?;
        let [even, odd] = &self.kept;
        let was = SideSounds::of(
            even.iter().map(|s| s.1.as_slice()),
            odd.iter().map(|s| s.1.as_slice()),
        );
        fn first(g: &[SideSnip]) -> Vec<&[f32]> {
            g.iter()
                .take(MIN_SIDE_BEATS)
                .map(|s| s.1.as_slice())
                .collect()
        }
        let now = SideSounds::of(first(&got[0]), first(&got[1]));
        let by_sound = match (&was, &now) {
            (Some(b), Some(a)) => match side_margin(b, a) {
                m if m <= -SIDE_MARGIN_MIN => SideCall::Swapped,
                m if m >= SIDE_MARGIN_MIN => SideCall::Kept,
                _ => SideCall::Unsure,
            },
            _ => SideCall::Unsure,
        };
        // Sides that sound alike are told apart by their timing, when the
        // beat error is large enough to have a sign on both sides of the
        // jump.
        let call = match by_sound {
            SideCall::Unsure => {
                let times = |s: &[SideSnip]| s.iter().map(|x| x.0).collect::<Vec<f64>>();
                let after: [Vec<f64>; 2] = [
                    times(&got[0][..MIN_SIDE_BEATS]),
                    times(&got[1][..MIN_SIDE_BEATS]),
                ];
                let before = self
                    .kept
                    .each_ref()
                    .map(|k| k.iter().map(|x| x.0).collect());
                side_by_timing(&before, &after)
            }
            call => call,
        };
        if call != SideCall::Unsure {
            self.sided.push(t);
        }
        if call == SideCall::Swapped {
            got.swap(0, 1);
        }
        self.kept = got.map(|g| {
            let skip = g.len().saturating_sub(SIDE_BEATS);
            g.into_iter().skip(skip).collect()
        });
        Some((t, call))
    }

    /// The jump whose sides are still to be checked, seconds.
    pub fn pending(&self) -> Option<f64> {
        self.pending.as_ref().map(|p| p.0)
    }

    /// Whether Tick and Tock are told apart at `t` seconds: false from a
    /// pending jump on, and in a stretch never told apart.
    pub fn known_at(&self, t: f64) -> bool {
        !self.pending().is_some_and(|p| p <= t)
            && !self.unsided.iter().any(|&(a, b)| a <= t && t < b)
    }

    /// The jumps whose sides were told apart by their sound, seconds.
    pub fn sided(&self) -> &[f64] {
        &self.sided
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

/// A beat whose timing [`jumps_among_good`] reads is better than this
/// ([`Beat::quality`]).
pub const JUMP_QUALITY: f32 = 0.4;

/// [`jumps`] on the good beats alone (better than [`JUMP_QUALITY`]), the
/// one jump search for a whole file and for live listening alike. Where
/// the sound was lost for a moment (a dropout in the audio), the tracker
/// follows noise for a few seconds until it finds the beats again; those
/// noise beats would hide the step from the beats either side of it, and
/// the count across such a stretch is a guess that swaps Tick and Tock
/// half the time. Positions are in `beats`; `before` may hold noise beats
/// too (see [`good_tail`]).
pub fn jumps_among_good(beats: &[Beat], before: &[Beat]) -> Vec<usize> {
    let good = |b: &Beat| b.quality > JUMP_QUALITY;
    let at: Vec<usize> = (0..beats.len()).filter(|&k| good(&beats[k])).collect();
    let found: Vec<Beat> = at.iter().map(|&k| beats[k]).collect();
    let before: Vec<Beat> = before.iter().copied().filter(good).collect();
    jumps(&found, &before).into_iter().map(|p| at[p]).collect()
}

/// Beats kept so far that the jump search looks back through for the good
/// beats before a jump ([`good_tail`]): about four minutes at 28,800 bph,
/// longer than the tracker follows noise after a dropout. The one reach for
/// a whole file and for live listening alike, so both find the same jumps
/// after a long stretch of noise.
pub const JUMP_LOOK_BACK: usize = 2000;

/// The last `n` good beats of `beats` (better than [`JUMP_QUALITY`]) with
/// the noise beats among them, looking back at most [`JUMP_LOOK_BACK`]
/// beats: the stretch [`jumps_among_good`] reads.
pub fn good_tail(beats: &[Beat], n: usize) -> &[Beat] {
    let back = &beats[beats.len().saturating_sub(JUMP_LOOK_BACK)..];
    &back[good_tail_start(back, n)..]
}

/// Where in `beats` the last `n` good beats (better than [`JUMP_QUALITY`])
/// begin, or 0 when there are fewer: the stretch [`jumps_among_good`] reads.
pub fn good_tail_start(beats: &[Beat], n: usize) -> usize {
    beats
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, b)| b.quality > JUMP_QUALITY)
        .nth(n.saturating_sub(1))
        .filter(|_| n > 0)
        .map_or(0, |(k, _)| k)
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
    // position `p` on (forward) or before it (back), and whether the middle
    // three sit within half a jump of one another: beats scattered wider
    // than that (no watch, or a side moving between two of its sounds) say
    // little, while one stray beat does not stop a jump being seen.
    let spread = |p: usize, s: i64, forward: bool| -> Option<(f64, bool)> {
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
        Some((m, v[JUMP_SPAN - 2] - v[1] <= 0.5 * JUMP_S))
    };
    // Both sides' steps at `p`, when both sides have the beats to tell:
    // at most one of the four stretches (each side, before and after) may
    // scatter. Where the watch stumbles, one side can scatter for a while
    // before a step while the other keeps time; its median still shows
    // the step, and the other side must step the same. Noise over both
    // sides (a knock of the stand) scatters two or more, and says nothing.
    let steps = |p: usize| -> Option<(f64, f64)> {
        let mut loose = 0;
        let mut at = |s: i64| -> Option<f64> {
            let (fore, fore_tight) = spread(p, s, true)?;
            let (back, back_tight) = if p == 0 {
                (0.0, true)
            } else {
                spread(p, s, false)?
            };
            loose += usize::from(!fore_tight) + usize::from(!back_tight);
            Some(fore - back)
        };
        let st = (at(0)?, at(1)?);
        (loose <= 1).then_some(st)
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
            let level = |q: usize, forward: bool| spread(best, side(q), forward).map(|x| x.0);
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
    fn noise_beats_around_a_jump_do_not_hide_it() {
        // From beat 200 on both sides land 30 ms later, and the 24 beats
        // before it are noise the tracker followed: badly matched, anywhere.
        let mut b = ticking(400, |k, _| if k >= 200 { 0.030 } else { 0.0 });
        for (n, x) in b[176..200].iter_mut().enumerate() {
            x.quality = 0.05;
            x.time += 0.04 * rng_normal(n as i64);
        }
        assert!(jumps(&b, &[]).is_empty(), "hidden among all the beats");
        assert_eq!(jumps_among_good(&b, &[]), vec![200]);
        // Seen from a chunk starting at the jump, with the noise before it.
        let from = good_tail_start(&b[..200], JUMP_CONTEXT);
        assert_eq!(from, 200 - 24 - JUMP_CONTEXT);
        assert_eq!(jumps_among_good(&b[200..], &b[from..200]), vec![0]);
    }

    #[test]
    fn a_jump_after_long_noise_is_seen_by_file_and_live_alike() {
        // From beat 1700 on both sides land 30 ms later, and the 300 beats
        // before it (about 40 s) are noise: longer than one pass of live
        // listening's good beats.
        let mut b = ticking(1800, |k, _| if k >= 1700 { 0.030 } else { 0.0 });
        for (n, x) in b[1400..1700].iter_mut().enumerate() {
            x.quality = 0.05;
            x.time += 0.04 * rng_normal(n as i64);
        }
        // A whole file's chunk starting at the jump, and live listening's
        // pass over its latest beats, reach the same good beats.
        let before = good_tail(&b[..1700], JUMP_CONTEXT);
        assert_eq!(jumps_among_good(&b[1700..], before), vec![0]);
        // (Live, on a pass 40 beats after the jump; the 152 beats it used
        // to reach back hold no good beats from before it.)
        let tail = good_tail(&b[..1740], 4 * MIN_SIDE_BEATS + JUMP_CONTEXT);
        assert!(b[1740 - 152..1700]
            .iter()
            .all(|x| x.quality <= JUMP_QUALITY));
        let at: Vec<f64> = jumps_among_good(tail, &[])
            .into_iter()
            .map(|p| tail[p].time)
            .collect();
        assert_eq!(at, vec![b[1700].time]);
        // Noise longer than the reach leaves no good beats in sight.
        let mut far = ticking(2600, |k, _| if k >= 2500 { 0.030 } else { 0.0 });
        for (n, x) in far[200..2500].iter_mut().enumerate() {
            x.quality = 0.05;
            x.time += 0.04 * rng_normal(n as i64);
        }
        let before = good_tail(&far[..2500], JUMP_CONTEXT);
        assert_eq!(before.len(), JUMP_LOOK_BACK);
        assert!(before.iter().all(|x| x.quality <= JUMP_QUALITY));
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

    /// The side sounds of `beats`, from their windows in `env`.
    fn side_sounds(env: &[f32], fs: f64, beats: &[Beat]) -> Option<SideSounds> {
        let side = |even: bool| -> Vec<&[f32]> {
            beats
                .iter()
                .filter(|b| (b.index.rem_euclid(2) == 0) == even)
                .filter_map(|b| crate::profile::snippet(env, fs, b.time))
                .collect()
        };
        SideSounds::of(side(true), side(false))
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

    /// A side whose beats match the template at two sounds 6 ms apart, as
    /// the Tock of a fake-mic Daytona take did, and which the track put on
    /// the earlier sound on two beats of every five: every beat goes back
    /// to the sound the rest of its side is on.
    #[test]
    fn a_side_with_two_sounds_stays_on_one() {
        let fs = 8000.0;
        let beat = 0.125;
        let n = 200i64;
        let pre = PRE_S;
        let due = |k: i64| 0.05 + k as f64 * beat + if k % 2 == 1 { 0.0004 } else { 0.0 };
        let mut corr = vec![0.0f32; ((n as f64 + 1.0) * beat * fs) as usize];
        let mut peak = |t: f64, v: f32| {
            let i = ((t - pre) * fs).round() as usize;
            corr[i - 1] += 0.6 * v;
            corr[i] += v;
            corr[i + 1] += 0.6 * v;
        };
        for k in 0..n {
            peak(due(k), 1.0);
            if k % 2 == 1 {
                peak(due(k) - 0.006, 1.05);
            }
        }
        let wrong = |k: i64| k % 2 == 1 && (k / 2) % 5 < 2;
        let mut beats: Vec<Beat> = (0..n)
            .map(|k| Beat {
                index: k,
                time: due(k) - if wrong(k) { 0.006 } else { 0.0 },
                quality: 1.0,
            })
            .collect();
        keep_sides_on_one_sound(&corr, fs, 1.0, &mut beats);
        for b in &beats {
            assert!(
                (b.time - due(b.index)).abs() < 1e-4,
                "beat {} at {} s, due at {} s",
                b.index,
                b.time,
                due(b.index)
            );
        }
        // A watch's own scatter, under a millisecond, is left as it is.
        let mut beats: Vec<Beat> = (0..n)
            .map(|k| Beat {
                index: k,
                time: due(k) + if k % 7 == 0 { 0.0008 } else { 0.0 },
                quality: 1.0,
            })
            .collect();
        let before = beats.clone();
        keep_sides_on_one_sound(&corr, fs, 1.0, &mut beats);
        assert!(beats.iter().zip(&before).all(|(a, b)| a.time == b.time));
    }

    /// A take with a hole whose Tick and Tock trade sounds 6 s after it
    /// (the sides' sounds can change as a watch is handled). The sides are
    /// told apart by the first good beats after the hole, so the whole
    /// file and live listening read the same beats and keep the beat
    /// error's sign the same; before, the file read up to 160 beats a side
    /// to the chunk's end, mostly after the trade, and swapped the sides
    /// where listening kept them.
    #[test]
    fn a_file_and_live_listening_tell_the_sides_apart_alike() {
        use crate::live::{LiveAnalyzer, LiveConfig};
        use crate::stream::{analyze_samples, StreamConfig};
        use crate::synth::{render_beat, Rng, SynthConfig};
        const HOLE_S: f64 = 30.0;
        const TRADE_S: f64 = HOLE_S + 6.0;
        let louder_unlock = [0.9, 0.25, 1.0];
        let cfg = |traded: bool| {
            let mut c = SynthConfig {
                duration_s: 100.0,
                rate_s_per_day: 10.0,
                beat_error_ms: 0.6,
                odd_gains: Some(louder_unlock),
                ..Default::default()
            };
            if traded {
                let even = c.sounds.map(|s| s.gain);
                for (s, g) in c.sounds.iter_mut().zip(louder_unlock) {
                    s.gain = g;
                }
                c.odd_gains = Some(even);
            }
            c
        };
        let (before, after) = (cfg(false), cfg(true));
        let fs = before.sample_rate as f64;
        let nominal = 3600.0 / before.bph as f64;
        // 0.03 s lost keeps the count's sides; 0.155 s swaps them.
        for lost_s in [0.03, 0.155] {
            let mut x = vec![0.0f64; (before.duration_s * fs) as usize];
            let mut rng = Rng::new(7);
            let (mut t, mut k, mut holed) = (0.05, 0u64, false);
            while t < before.duration_s - 0.05 {
                if t >= HOLE_S && !holed {
                    t += lost_s;
                    holed = true;
                }
                let c = if t >= TRADE_S { &after } else { &before };
                render_beat(c, k, t, 270.0, c.beat_error_ms, &mut rng, |i, v| {
                    if let Some(s) = usize::try_from(i).ok().and_then(|i| x.get_mut(i)) {
                        *s += v;
                    }
                });
                t += nominal * (1.0 - before.rate_s_per_day / 86400.0);
                k += 1;
            }
            let noise = 10f64.powf(-before.snr_db / 20.0);
            let samples: Vec<f32> = x
                .iter()
                .map(|&v| ((v + noise * rng.normal()) * 0.5) as f32)
                .collect();

            let file = analyze_samples(&samples, before.sample_rate, &StreamConfig::default());
            let mut live = LiveAnalyzer::new(before.sample_rate, LiveConfig::default());
            for c in samples.chunks(960) {
                live.push(c);
            }
            for (what, beats) in [("file", &file.beats[..]), ("live", live.beats())] {
                let side = |from: f64, to: f64| {
                    let run: Vec<Beat> = beats
                        .iter()
                        .filter(|b| b.time >= from && b.time < to)
                        .copied()
                        .collect();
                    crate::timing::beat_error_ms(&run).expect("beat error")
                };
                let (was, now) = (side(5.0, HOLE_S - 1.0), side(TRADE_S + 5.0, 95.0));
                assert!(
                    was.abs() > 0.4 && (was - now).abs() < 0.1,
                    "{what}, {lost_s} s lost: beat error {was} then {now}"
                );
            }
        }
    }

    /// Sound lost for half a beat and more (a dropout in the audio) puts
    /// the watch's beats half a beat off the count. The tracker then
    /// follows noise for a few seconds before it finds them again, and the
    /// count across that stretch picks the sides by chance. A whole file
    /// finds the jump from the good beats either side, as live listening
    /// does, and its side check keeps the sides.
    #[test]
    fn a_dropout_the_tracker_follows_noise_through_keeps_the_sides() {
        use crate::live::{LiveAnalyzer, LiveConfig};
        use crate::stream::{analyze_samples, StreamConfig};
        use crate::synth::{render_beat, Rng, SynthConfig};
        const HOLE_S: f64 = 30.0;
        // With the Tock's unlock louder, and with both sides at one gain.
        for louder in [true, false] {
            let c = SynthConfig {
                duration_s: 70.0,
                rate_s_per_day: 10.0,
                beat_error_ms: 0.5,
                odd_gains: louder.then_some([0.9, 0.25, 1.0]),
                ..Default::default()
            };
            let fs = c.sample_rate as f64;
            let nominal = 3600.0 / c.bph as f64;
            for beats_lost in [0.55, 1.5, 3.5] {
                let mut x = vec![0.0f64; (c.duration_s * fs) as usize];
                let mut rng = Rng::new(11);
                let (mut t, mut k, mut holed) = (0.05, 0u64, false);
                while t < c.duration_s - 0.05 {
                    if t >= HOLE_S && !holed {
                        t += beats_lost * nominal;
                        holed = true;
                    }
                    render_beat(&c, k, t, 270.0, c.beat_error_ms, &mut rng, |i, v| {
                        if let Some(s) = usize::try_from(i).ok().and_then(|i| x.get_mut(i)) {
                            *s += v;
                        }
                    });
                    t += nominal * (1.0 - c.rate_s_per_day / 86400.0);
                    k += 1;
                }
                let noise = 10f64.powf(-c.snr_db / 20.0);
                let samples: Vec<f32> = x
                    .iter()
                    .map(|&v| ((v + noise * rng.normal()) * 0.5) as f32)
                    .collect();
                let file = analyze_samples(&samples, c.sample_rate, &StreamConfig::default());
                // Live listening finds the jump too, with the same search
                // (where in the noise stretch depends on how the sound
                // arrives).
                let mut live = LiveAnalyzer::new(c.sample_rate, LiveConfig::default());
                for chunk in samples.chunks(960) {
                    live.push(chunk);
                }
                assert!(
                    live.jumps()
                        .iter()
                        .any(|&j| (HOLE_S..HOLE_S + 10.0).contains(&j)),
                    "louder {louder}, {beats_lost} beats lost: live jumps {:?}, file {:?}",
                    live.jumps(),
                    file.jumps
                );
                let side = |from: f64, to: f64| {
                    let run: Vec<Beat> = file
                        .beats
                        .iter()
                        .filter(|b| b.time >= from && b.time < to && b.quality > 0.4)
                        .copied()
                        .collect();
                    crate::timing::beat_error_ms(&run).expect("beat error")
                };
                assert!(
                    file.jumps
                        .iter()
                        .any(|&j| (HOLE_S..HOLE_S + 10.0).contains(&j)),
                    "louder {louder}, {beats_lost} beats lost: jumps {:?}",
                    file.jumps
                );
                let (was, now) = (side(5.0, HOLE_S - 1.0), side(HOLE_S + 15.0, 65.0));
                assert!(
                    was.abs() > 0.3 && (was - now).abs() < 0.1,
                    "louder {louder}, {beats_lost} beats lost: beat error {was} then {now} \
                     (jumps {:?})",
                    file.jumps
                );
            }
        }
    }
}
