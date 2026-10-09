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
}

impl Default for EnvelopeConfig {
    fn default() -> Self {
        EnvelopeConfig {
            highpass_hz: 1500.0,
            notch_hz: Vec::new(),
            smooth_s: 0.0002,
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
    for v in y.iter_mut() {
        *v = v.abs();
    }
    moving_average(&y, ((cfg.smooth_s * fs).round() as usize).max(1))
}

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
