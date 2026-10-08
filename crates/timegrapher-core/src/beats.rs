//! Finding every beat.
//!
//! Two passes. A first pass tracks the loudest point of each beat on the
//! envelope and builds a median beat template from it. The second pass
//! correlates the whole envelope with that template and tracks the
//! correlation peaks, which is far more robust than picking maxima: a beat
//! whose unlock happens to be louder than its drop still lines up with the
//! template as a whole.

use crate::dsp::{argmax, correlate, median_f32, parabolic};
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
        // Allow a little slack around the nominal lag for a fast or slow watch.
        let c = lag.round() as usize;
        let w = (lag * 0.004).ceil() as usize + 1;
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
fn track_peaks(x: &[f32], fs: f64, beat: f64) -> Vec<(f64, f32)> {
    let n = x.len();
    let start_span = (beat * fs * 1.5) as usize;
    let Some((first, _)) = argmax(x, 0, start_span) else {
        return Vec::new();
    };
    let mut out: Vec<(f64, f32)> = Vec::new();
    let mut per = beat * fs;
    let mut pred = first as f64;
    let half = 0.3 * beat * fs;
    while pred + half < n as f64 {
        let a = (pred - half).max(0.0) as usize;
        let b = (pred + half) as usize;
        let Some((i, v)) = argmax(x, a, b) else { break };
        let t = parabolic(x, i);
        out.push((t, v));
        let len = out.len();
        if len > 2 {
            // Update on the tick-to-tick interval so beat error doesn't pull the loop.
            let meas = (out[len - 1].0 - out[len - 3].0) / 2.0;
            if (meas - per).abs() < 0.05 * per {
                per = 0.9 * per + 0.1 * meas;
            }
            pred = out[len - 2].0 + 2.0 * per;
        } else {
            pred = t + per;
        }
    }
    out.into_iter().map(|(t, v)| (t / fs, v)).collect()
}

/// Window around each beat used for templates: from `PRE_S` before the
/// drop to `POST_S` after it. 20 ms before covers the unlock down to
/// about 150 degrees of amplitude at common beat rates.
pub const PRE_S: f64 = 0.020;
pub const POST_S: f64 = 0.008;

/// Median of the windows around the given beat times (sample by sample).
pub fn median_template(env: &[f32], fs: f64, times: &[f64]) -> Vec<f32> {
    median_window(env, fs, times, PRE_S, POST_S)
}

/// Median of the windows from `pre_s` before to `post_s` after each time.
/// Windows that run off either end of the envelope are left out.
pub fn median_window(env: &[f32], fs: f64, times: &[f64], pre_s: f64, post_s: f64) -> Vec<f32> {
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
            median_f32(&mut col)
        })
        .collect()
}

/// Find every beat in an envelope.
pub fn detect(env: &[f32], fs: f64, bph: u32) -> (Vec<Beat>, Vec<f32>) {
    let beat = 3600.0 / bph as f64;
    // Pass 1: maxima, used only to build the template.
    let coarse = track_peaks(env, fs, beat);
    let step = (coarse.len() / 2000).max(1);
    let sample: Vec<f64> = coarse.iter().step_by(step).map(|c| c.0).collect();
    let template = median_template(env, fs, &sample);
    let beats = detect_with_template(env, fs, bph, &template);
    (beats, template)
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
    let peaks = track_peaks(&corr, fs, beat);
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
