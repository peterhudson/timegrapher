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
    // Found peaks with the number of the beat each one is.
    let mut out: Vec<(f64, f32, i64)> = Vec::new();
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
            if let Some(&(t2, _, _)) = out.iter().rev().take(3).find(|o| o.2 == k - 2) {
                // Update on the tick-to-tick interval so beat error doesn't pull the loop.
                let meas = (t - t2) / 2.0;
                if (meas - per).abs() < 0.05 * per {
                    per = 0.9 * per + 0.1 * meas;
                }
            }
            out.push((t, v, k));
            misses = 0;
            let ty = &mut typical[side(k)];
            *ty = if *ty > 0.0 { 0.95 * *ty + 0.05 * v } else { v };
        } else {
            misses += 1;
        }
        k += 1;
        // Predict beat k from the same side's last beat once there are
        // three beats, so beat error does not enter the prediction.
        let same_side = out.iter().rev().take(3).find(|o| o.2 == k - 2);
        pred = match same_side.filter(|_| out.len() > 2) {
            Some(&(t2, _, _)) => t2 + 2.0 * per,
            None => match out.last() {
                Some(&(t1, _, k1)) => t1 + (k - k1) as f64 * per,
                None => pred + per,
            },
        };
    }
    out.into_iter().map(|(t, v, _)| (t / fs, v)).collect()
}

/// How far from where it is due a beat may be found once the track is
/// running, as a fraction of the beat period. Beat-to-beat changes in
/// timing are far smaller; a knock lands anywhere.
const MAX_OFFSET: f64 = 0.1;

fn side(k: i64) -> usize {
    k.rem_euclid(2) as usize
}
/// Half-width of the window around where a beat is due that pass 2
/// searches when a knock outmatches the beat, seconds. Beat-to-beat changes in timing are tenths of a
/// millisecond; professional timegraphers gate to about 2 ms.
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

#[cfg(test)]
mod tests {
    use super::*;

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
