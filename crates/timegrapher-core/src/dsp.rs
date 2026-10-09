//! Envelope extraction, FFT correlation and small numeric helpers.

use crate::filter::{highpass4, Biquad};
use realfft::RealFftPlanner;

/// How the raw signal is turned into a tick envelope.
#[derive(Debug, Clone)]
pub struct EnvelopeConfig {
    /// High-pass corner in Hz; removes hum, rumble and handling noise.
    pub highpass_hz: f64,
    /// Steady tones to notch out (interference picked up by some mics).
    pub notch_hz: Vec<f64>,
    /// Length of the moving average applied to the rectified signal, seconds.
    pub smooth_s: f64,
    /// Silence knocks and bumps: stretches whose 20 ms energy is more than
    /// this many times a typical tick's are zeroed before anything else
    /// sees them. `None` keeps everything.
    pub burst_gate: Option<f64>,
}

impl Default for EnvelopeConfig {
    fn default() -> Self {
        EnvelopeConfig {
            highpass_hz: 1500.0,
            notch_hz: Vec::new(),
            smooth_s: 0.0002,
            burst_gate: Some(2.0),
        }
    }
}

/// High-pass, optional notches, rectify, then a centred moving average.
pub fn envelope(x: &[f32], fs: f64, cfg: &EnvelopeConfig) -> Vec<f32> {
    let mut y = x.to_vec();
    highpass4(&mut y, fs, cfg.highpass_hz);
    for &f0 in &cfg.notch_hz {
        Biquad::notch(fs, f0, 30.0).filtfilt(&mut y);
    }
    if let Some(factor) = cfg.burst_gate {
        burst_gate(&mut y, fs, factor);
    }
    for v in y.iter_mut() {
        *v = v.abs();
    }
    moving_average(&y, ((cfg.smooth_s * fs).round() as usize).max(1))
}

/// Zero every sample whose 20 ms energy is more than `factor` times a
/// typical tick's: the median, over half-second blocks, of each block's
/// highest 20 ms energy. A knock on the desk or a bump of the stand is
/// much louder than the ticks and would otherwise pull the beat tracking
/// and the templates. Loud stretches longer than 0.1 s are kept. Returns
/// the fraction of samples zeroed.
pub fn burst_gate(y: &mut [f32], fs: f64, factor: f64) -> f64 {
    let w = ((0.02 * fs) as usize).max(1);
    let blk = (0.5 * fs) as usize;
    if y.len() < 2 * blk || w >= y.len() {
        return 0.0;
    }
    let sq: Vec<f32> = y.iter().map(|v| v * v).collect();
    let energy = moving_average(&sq, w);
    let mut peaks: Vec<f32> = energy
        .chunks_exact(blk)
        .map(|c| c.iter().copied().fold(0.0, f32::max))
        .collect();
    let typical = median_f32(&mut peaks);
    if typical.is_nan() || typical <= 0.0 {
        return 0.0;
    }
    let limit = (factor * typical as f64) as f32;
    let loud = energy.iter().filter(|&&e| e > limit).count();
    // Knocks fill a percent or two of a recording. Far more than that means
    // the typical level is not a tick's, as when the watch is only heard in
    // the last part of a window, so the "bursts" are the ticks: keep them.
    if loud as f64 > MAX_GATED * y.len() as f64 {
        return 0.0;
    }
    // A knock is short. A loud stretch longer than MAX_BURST_S (handling,
    // rubbing) is left alone: silencing it would take every beat in it
    // with it, and the beat tracking rides through it better than through
    // a gap.
    let max_run = (MAX_BURST_S * fs) as usize;
    let mut zeroed = 0usize;
    let mut i = 0;
    while i < y.len() {
        if energy[i] <= limit {
            i += 1;
            continue;
        }
        let start = i;
        while i < y.len() && energy[i] > limit {
            i += 1;
        }
        if i - start <= max_run {
            y[start..i].fill(0.0);
            zeroed += i - start;
        }
    }
    zeroed as f64 / y.len() as f64
}

/// Most of a recording the burst gate may silence; see [`burst_gate`].
const MAX_GATED: f64 = 0.04;
/// Longest loud stretch the burst gate silences, seconds.
const MAX_BURST_S: f64 = 0.1;

/// Centred moving average of width `w`, same length as the input.
pub fn moving_average(x: &[f32], w: usize) -> Vec<f32> {
    if w <= 1 || x.is_empty() {
        return x.to_vec();
    }
    let n = x.len();
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0f64);
    let mut acc = 0.0f64;
    for &v in x {
        acc += v as f64;
        prefix.push(acc);
    }
    let half = w / 2;
    (0..n)
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + w - half).min(n);
            ((prefix[b] - prefix[a]) / (b - a) as f64) as f32
        })
        .collect()
}

/// Sliding dot product of `x` with `template`:
/// `out[n] = sum_k x[n + k] * template[k]`, for `n` in `0..x.len()`
/// (samples past the end of `x` count as zero). FFT overlap-save.
#[allow(clippy::needless_range_loop)]
pub fn correlate(x: &[f32], template: &[f32]) -> Vec<f32> {
    let m = template.len();
    let n = x.len();
    if m == 0 || n == 0 {
        return vec![0.0; n];
    }
    let l = (4 * m).next_power_of_two().max(4096);
    let step = l - m + 1;
    let mut planner = RealFftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(l);
    let inv = planner.plan_fft_inverse(l);

    // Spectrum of the reversed template.
    let mut h = vec![0.0f32; l];
    for (k, &v) in template.iter().enumerate() {
        h[m - 1 - k] = v;
    }
    let mut hs = fwd.make_output_vec();
    fwd.process(&mut h, &mut hs).expect("fft");

    let mut out = vec![0.0f32; n];
    let mut buf = vec![0.0f32; l];
    let mut spec = fwd.make_output_vec();
    let scale = 1.0 / l as f32;
    let mut s = 0usize;
    while s < n {
        // Block covers x[s .. s + l]; outputs y[s + j] for j >= m - 1 are
        // valid, and out[n'] = y[n' + m - 1].
        for (j, b) in buf.iter_mut().enumerate() {
            *b = x.get(s + j).copied().unwrap_or(0.0);
        }
        fwd.process(&mut buf, &mut spec).expect("fft");
        for (a, b) in spec.iter_mut().zip(hs.iter()) {
            *a *= *b;
        }
        inv.process(&mut spec, &mut buf).expect("ifft");
        for j in (m - 1)..l {
            let idx = s + j - (m - 1);
            if idx >= n || j - (m - 1) >= step {
                break;
            }
            out[idx] = buf[j] * scale;
        }
        s += step;
    }
    out
}

/// Index and value of the maximum of `x[a..b]` (clamped to the slice).
pub fn argmax(x: &[f32], a: usize, b: usize) -> Option<(usize, f32)> {
    let b = b.min(x.len());
    if a >= b {
        return None;
    }
    let mut best = (a, x[a]);
    for (i, &v) in x[a..b].iter().enumerate() {
        if v > best.1 {
            best = (a + i, v);
        }
    }
    Some(best)
}

/// Sub-sample position of a peak by parabolic interpolation.
pub fn parabolic(x: &[f32], i: usize) -> f64 {
    if i == 0 || i + 1 >= x.len() {
        return i as f64;
    }
    let (y0, y1, y2) = (x[i - 1] as f64, x[i] as f64, x[i + 1] as f64);
    let d = y0 - 2.0 * y1 + y2;
    // Only a local maximum is refined: on a slope (an argmax at the edge
    // of its search window) a nearly flat curve would throw the vertex
    // many samples away.
    if d < 0.0 && y1 >= y0 && y1 >= y2 {
        i as f64 + 0.5 * (y0 - y2) / d
    } else {
        i as f64
    }
}

/// Median of a slice (the slice is reordered). Returns NaN when empty.
pub fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let mid = v.len() / 2;
    v.select_nth_unstable_by(mid, |a, b| a.total_cmp(b));
    let hi = v[mid];
    if v.len() % 2 == 1 {
        hi
    } else {
        let lo = v[..mid].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (lo + hi) / 2.0
    }
}

pub fn median_f32(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    let mid = v.len() / 2;
    v.select_nth_unstable_by(mid, |a, b| a.total_cmp(b));
    v[mid]
}

/// Mean of the middle of `v` after dropping `trim` of the values from each
/// end (0.25 keeps the middle half; 0.5 is the median). Sorts `v`.
pub fn trimmed_mean_f32(v: &mut [f32], trim: f64) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    v.sort_unstable_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let cut = ((n as f64 * trim.clamp(0.0, 0.5)).floor() as usize).min((n - 1) / 2);
    let mid = &v[cut..n - cut];
    (mid.iter().map(|&x| x as f64).sum::<f64>() / mid.len() as f64) as f32
}

/// Median absolute deviation scaled to match a standard deviation.
pub fn robust_sd(v: &[f64]) -> f64 {
    let mut w: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    let m = median(&mut w);
    let mut dev: Vec<f64> = w.iter().map(|x| (x - m).abs()).collect();
    1.4826 * median(&mut dev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_gate_silences_a_knock_not_the_ticks() {
        // A 1 ms tick every 125 ms for 4 s, and a knock ten times as loud.
        let fs = 48_000.0;
        let mut y = vec![0.0f32; (4.0 * fs) as usize];
        for k in 0..32 {
            let i = ((0.05 + k as f64 * 0.125) * fs) as usize;
            for v in &mut y[i..i + 48] {
                *v = 0.5;
            }
        }
        let knock = (2.01 * fs) as usize;
        for v in &mut y[knock..knock + 480] {
            *v = 5.0;
        }
        let ticks: f32 = y.iter().filter(|&&v| v == 0.5).count() as f32;
        let frac = burst_gate(&mut y, fs, 2.0);
        assert!(y[knock..knock + 480].iter().all(|&v| v == 0.0));
        assert_eq!(y.iter().filter(|&&v| v == 0.5).count() as f32, ticks);
        assert!(frac > 0.0 && frac < 0.01, "{frac}");
    }

    #[test]
    fn burst_gate_keeps_a_long_loud_stretch() {
        // Ticks for 8 s, and from 4 s a loud stretch of 0.15 s (handling,
        // not a knock): left alone, so the ticks in it are still there.
        let fs = 48_000.0;
        let mut y = vec![0.0f32; (8.0 * fs) as usize];
        for k in 0..64 {
            let i = ((0.05 + k as f64 * 0.125) * fs) as usize;
            for v in &mut y[i..i + 48] {
                *v = 0.5;
            }
        }
        let a = (4.0 * fs) as usize;
        for (j, v) in y[a..a + (0.15 * fs) as usize].iter_mut().enumerate() {
            *v += if j % 2 == 0 { 3.0 } else { -3.0 };
        }
        let before = y.clone();
        assert_eq!(burst_gate(&mut y, fs, 2.0), 0.0);
        assert_eq!(y, before);
    }

    #[test]
    fn burst_gate_keeps_ticks_when_the_watch_arrives_late() {
        // Faint noise for 3 s, then ticks for 1 s: the typical block is
        // quiet and every tick looks like a burst, so nothing is gated.
        let fs = 48_000.0;
        let mut y: Vec<f32> = (0..(4.0 * fs) as usize)
            .map(|i| 0.001 * ((i * 7919 % 1000) as f32 / 500.0 - 1.0))
            .collect();
        for k in 0..8 {
            let i = ((3.05 + k as f64 * 0.125) * fs) as usize;
            for v in &mut y[i..i + 48] {
                *v = 0.5;
            }
        }
        let before = y.clone();
        assert_eq!(burst_gate(&mut y, fs, 2.0), 0.0);
        assert_eq!(y, before);
    }

    #[test]
    fn parabolic_refines_only_a_peak() {
        // A true peak moves toward its higher neighbour by under a sample.
        let p = parabolic(&[0.0, 1.0, 0.5], 1);
        assert!(p > 1.0 && p < 1.5, "{p}");
        // On a nearly flat falling slope the vertex would be far away.
        assert_eq!(parabolic(&[1.0, 0.999, 0.997], 1), 1.0);
    }

    #[test]
    fn trimmed_mean_ignores_the_ends() {
        let mut v = [1.0, 2.0, 3.0, 4.0, 100.0, -50.0, 2.5, 3.5];
        // Middle half of the sorted values: 2.0, 2.5, 3.0, 3.5.
        assert!((trimmed_mean_f32(&mut v, 0.25) - 2.75).abs() < 1e-6);
        let mut one = [7.0];
        assert_eq!(trimmed_mean_f32(&mut one, 0.25), 7.0);
        let mut w = [1.0, 2.0, 9.0];
        assert_eq!(trimmed_mean_f32(&mut w, 0.5), 2.0);
    }

    #[test]
    fn correlate_matches_direct() {
        let x: Vec<f32> = (0..10_000)
            .map(|i| ((i * 7919) % 101) as f32 / 50.0 - 1.0)
            .collect();
        let t: Vec<f32> = (0..300)
            .map(|i| ((i * 31) % 17) as f32 / 8.0 - 1.0)
            .collect();
        let fast = correlate(&x, &t);
        for &n in &[0usize, 1, 500, 4000, 9_699, 9_999] {
            let direct: f32 = (0..t.len())
                .map(|k| x.get(n + k).copied().unwrap_or(0.0) * t[k])
                .sum();
            assert!(
                (fast[n] - direct).abs() < 1e-2,
                "n={n} fast={} direct={direct}",
                fast[n]
            );
        }
    }

    #[test]
    fn moving_average_flat() {
        let x = vec![2.0f32; 50];
        assert!(moving_average(&x, 7)
            .iter()
            .all(|&v| (v - 2.0).abs() < 1e-6));
    }
}
