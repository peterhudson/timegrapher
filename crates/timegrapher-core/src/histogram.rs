//! Histograms of a reading over a session: how often each value came up,
//! so a watch that flips between two states (a high and a low amplitude,
//! say) shows two peaks where an average shows one value between them.
//!
//! The bin width follows the Freedman–Diaconis rule (twice the
//! interquartile range over the cube root of the count), rounded up to 1, 2
//! or 5 times a power of ten so the bin edges are round numbers, and never
//! finer than the reading's own resolution.

use serde::Serialize;

/// Counts of values in bins of equal width.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Histogram {
    /// Lower edge of the first bin.
    pub start: f64,
    pub bin_width: f64,
    /// Values in each bin, from `start` upwards.
    pub counts: Vec<u32>,
    /// Values counted.
    pub n: usize,
    pub median: f64,
    /// The 10th and 90th percentiles: where the middle 80% of values lie.
    pub p10: f64,
    pub p90: f64,
}

impl Histogram {
    /// The centre of bin `i`.
    pub fn centre(&self, i: usize) -> f64 {
        self.start + (i as f64 + 0.5) * self.bin_width
    }

    /// The bins that stand out as separate peaks: local maxima holding at
    /// least `min_share` of the values, with a dip to at most half the lower
    /// peak's height between neighbouring ones. One peak is a single state;
    /// two or more suggest the watch moves between states.
    pub fn peaks(&self, min_share: f64) -> Vec<usize> {
        let c = &self.counts;
        // Smooth over three bins, so one noisy bin isn't a peak.
        let s: Vec<f64> = (0..c.len())
            .map(|i| {
                let lo = i.saturating_sub(1);
                let hi = (i + 2).min(c.len());
                c[lo..hi].iter().map(|&v| v as f64).sum::<f64>() / (hi - lo) as f64
            })
            .collect();
        let floor = min_share * self.n as f64 / 3.0;
        let mut peaks: Vec<usize> = Vec::new();
        for i in 0..s.len() {
            let left = if i == 0 { f64::MIN } else { s[i - 1] };
            let right = if i + 1 == s.len() { f64::MIN } else { s[i + 1] };
            if s[i] < floor || s[i] < left || s[i] <= right {
                continue;
            }
            if let Some(&p) = peaks.last() {
                let dip = s[p..=i].iter().copied().fold(f64::MAX, f64::min);
                if dip > 0.5 * s[p].min(s[i]) {
                    // Not separated: keep the higher of the two.
                    if s[i] > s[p] {
                        *peaks.last_mut().unwrap() = i;
                    }
                    continue;
                }
            }
            peaks.push(i);
        }
        peaks
    }
}

/// The value `q` (0 to 1) of the way through sorted values.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    let x = q * (sorted.len() - 1) as f64;
    let i = x.floor() as usize;
    let j = (i + 1).min(sorted.len() - 1);
    sorted[i] + (sorted[j] - sorted[i]) * (x - i as f64)
}

/// 1, 2 or 5 times a power of ten, at least `x`.
fn round_up(x: f64) -> f64 {
    let p = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * p)
        .find(|&s| s >= x * (1.0 - 1e-9))
        .unwrap_or(10.0 * p)
}

/// A histogram of `values`, ignoring any that aren't finite; `None` with
/// fewer than three. `resolution` is the finest bin width worth drawing
/// (such as 0.5° for amplitude); the bins are capped at 200.
pub fn histogram(values: &[f64], resolution: f64) -> Option<Histogram> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.len() < 3 {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    let (lo, hi) = (v[0], v[n - 1]);
    let iqr = quantile(&v, 0.75) - quantile(&v, 0.25);
    let fd = 2.0 * iqr / (n as f64).cbrt();
    let mut width = round_up(fd.max(resolution).max(1e-12));
    while (hi - lo) / width > 200.0 {
        width = round_up(width * 1.01);
    }
    let start = (lo / width).floor() * width;
    let bins = (((hi - start) / width).floor() as usize + 1).max(1);
    let mut counts = vec![0u32; bins];
    for x in &v {
        let i = (((x - start) / width).floor() as usize).min(bins - 1);
        counts[i] += 1;
    }
    Some(Histogram {
        start,
        bin_width: width,
        counts,
        n,
        median: quantile(&v, 0.5),
        p10: quantile(&v, 0.1),
        p90: quantile(&v, 0.9),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spread of values about `mid`, the same every run.
    fn spread(mid: f64, sd: f64, n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed;
        let mut u = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        (0..n)
            .map(|_| {
                // Box–Muller.
                let (a, b) = (u(), u());
                mid + sd * (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
            })
            .collect()
    }

    #[test]
    fn counts_every_value_in_round_bins() {
        let v = spread(250.0, 3.0, 2000, 1);
        let h = histogram(&v, 0.5).unwrap();
        assert_eq!(h.counts.iter().sum::<u32>() as usize, 2000);
        assert_eq!(h.n, 2000);
        // Round bin edges.
        let k = h.start / h.bin_width;
        assert!((k - k.round()).abs() < 1e-9);
        assert!([0.5, 1.0, 2.0].contains(&h.bin_width), "{}", h.bin_width);
        assert!((h.median - 250.0).abs() < 0.5);
        assert!(h.p10 < h.median && h.median < h.p90);
        assert_eq!(h.peaks(0.05).len(), 1);
    }

    #[test]
    fn two_states_show_two_peaks() {
        let mut v = spread(230.0, 2.0, 1000, 2);
        v.extend(spread(250.0, 2.0, 1000, 3));
        let h = histogram(&v, 0.5).unwrap();
        let p = h.peaks(0.05);
        assert_eq!(p.len(), 2, "{h:?}");
        assert!((h.centre(p[0]) - 230.0).abs() <= h.bin_width, "{h:?}");
        assert!((h.centre(p[1]) - 250.0).abs() <= h.bin_width, "{h:?}");
    }

    #[test]
    fn resolution_and_degenerate_input() {
        assert!(histogram(&[1.0, 2.0], 0.1).is_none());
        assert!(histogram(&[f64::NAN, 1.0, 2.0], 0.1).is_none());
        // All the same value: one bin of the resolution's width.
        let h = histogram(&[5.0; 10], 0.1).unwrap();
        assert_eq!(h.counts, vec![10]);
        assert!((h.bin_width - 0.1).abs() < 1e-12);
    }
}
