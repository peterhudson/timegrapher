//! Searching rate and amplitude for periodic components.
//!
//! A damaged tooth or an eccentric wheel repeats once per turn of its
//! wheel, so a periodic component points at a specific wheel: the escape
//! wheel turns every few seconds, the fourth wheel every minute, the
//! centre wheel every hour.

use crate::dsp::median;
use serde::Serialize;

/// A wheel whose turn period is known for this movement.
#[derive(Debug, Clone, Serialize)]
pub struct Wheel {
    pub name: String,
    pub period_s: f64,
}

/// The wheels whose periods follow from the beat rate alone. The escape
/// wheel advances one tooth per oscillation (two beats).
pub fn standard_wheels(bph: u32, escape_teeth: u32) -> Vec<Wheel> {
    let beat = 3600.0 / bph as f64;
    vec![
        Wheel {
            name: "escape wheel".into(),
            period_s: escape_teeth as f64 * 2.0 * beat,
        },
        Wheel {
            name: "fourth wheel".into(),
            period_s: 60.0,
        },
        Wheel {
            name: "centre wheel".into(),
            period_s: 3600.0,
        },
    ]
}

/// A series sampled into bins of equal length, with gaps left out.
#[derive(Debug, Clone, Serialize)]
pub struct Series {
    pub t: Vec<f64>,
    pub y: Vec<f64>,
}

/// Median of `values` in consecutive bins of `bin_s` seconds.
pub fn bin_median(times: &[f64], values: &[f64], bin_s: f64) -> Series {
    let mut out = Series {
        t: Vec::new(),
        y: Vec::new(),
    };
    if times.is_empty() {
        return out;
    }
    let t0 = times[0];
    let mut cur = 0i64;
    let mut acc: Vec<f64> = Vec::new();
    let flush = |cur: i64, acc: &mut Vec<f64>, out: &mut Series| {
        if acc.len() >= 2 {
            out.t.push(t0 + (cur as f64 + 0.5) * bin_s);
            out.y.push(median(acc));
        }
        acc.clear();
    };
    for (&t, &v) in times.iter().zip(values) {
        if !v.is_finite() {
            continue;
        }
        let b = ((t - t0) / bin_s).floor() as i64;
        if b != cur {
            flush(cur, &mut acc, &mut out);
            cur = b;
        }
        acc.push(v);
    }
    flush(cur, &mut acc, &mut out);
    out
}

/// Remove slow drift: a quadratic for short runs, a running median of
/// 30 minutes for runs over two hours (which keeps hour-scale features
/// shorter than the window).
pub fn detrend(s: &Series) -> Series {
    let n = s.t.len();
    if n < 4 {
        return s.clone();
    }
    let span = s.t[n - 1] - s.t[0];
    let y = if span < 7200.0 {
        let fit = polyfit2(&s.t, &s.y);
        s.t.iter()
            .zip(&s.y)
            .map(|(&t, &y)| y - (fit[0] + fit[1] * t + fit[2] * t * t))
            .collect()
    } else {
        let half = 900.0;
        let mut lo = 0;
        let mut hi = 0;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            while s.t[lo] < s.t[i] - half {
                lo += 1;
            }
            while hi < n && s.t[hi] <= s.t[i] + half {
                hi += 1;
            }
            let mut w = s.y[lo..hi].to_vec();
            out.push(s.y[i] - median(&mut w));
        }
        out
    };
    Series { t: s.t.clone(), y }
}

#[allow(clippy::needless_range_loop)]
pub(crate) fn polyfit2(t: &[f64], y: &[f64]) -> [f64; 3] {
    let t0 = t[0];
    let mut a = [[0.0f64; 3]; 3];
    let mut b = [0.0f64; 3];
    for (&ti, &yi) in t.iter().zip(y) {
        let x = ti - t0;
        let r = [1.0, x, x * x];
        for i in 0..3 {
            b[i] += r[i] * yi;
            for j in 0..3 {
                a[i][j] += r[i] * r[j];
            }
        }
    }
    // Gaussian elimination.
    for c in 0..3 {
        let p = (c..3)
            .max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))
            .unwrap();
        a.swap(c, p);
        b.swap(c, p);
        if a[c][c].abs() < 1e-300 {
            return [0.0; 3];
        }
        for r in 0..3 {
            if r != c {
                let f = a[r][c] / a[c][c];
                for k in 0..3 {
                    a[r][k] -= f * a[c][k];
                }
                b[r] -= f * b[c];
            }
        }
    }
    let (c0, c1, c2) = (b[0] / a[0][0], b[1] / a[1][1], b[2] / a[2][2]);
    // Re-express in absolute t.
    [c0 - c1 * t0 + c2 * t0 * t0, c1 - 2.0 * c2 * t0, c2]
}

/// Normalised Lomb–Scargle periodogram (0..1: the fraction of the
/// series' variance explained by a sinusoid at that period).
pub fn lomb_scargle(s: &Series, periods: &[f64]) -> Vec<f64> {
    let n = s.y.len();
    if n < 4 {
        return vec![0.0; periods.len()];
    }
    let mean = s.y.iter().sum::<f64>() / n as f64;
    let y: Vec<f64> = s.y.iter().map(|v| v - mean).collect();
    let ss: f64 = y.iter().map(|v| v * v).sum();
    if ss <= 0.0 {
        return vec![0.0; periods.len()];
    }
    periods
        .iter()
        .map(|&p| {
            let w = 2.0 * std::f64::consts::PI / p;
            let (mut s2, mut c2) = (0.0, 0.0);
            for &t in &s.t {
                let (a, b) = (2.0 * w * t).sin_cos();
                s2 += a;
                c2 += b;
            }
            let tau = s2.atan2(c2) / (2.0 * w);
            let (mut yc, mut ys, mut cc, mut sn) = (0.0, 0.0, 0.0, 0.0);
            for (&t, &v) in s.t.iter().zip(&y) {
                let (a, b) = (w * (t - tau)).sin_cos();
                yc += v * b;
                ys += v * a;
                cc += b * b;
                sn += a * a;
            }
            (yc * yc / cc.max(1e-300) + ys * ys / sn.max(1e-300)) / ss
        })
        .collect()
}

/// Log-spaced trial periods.
pub fn log_periods(min_s: f64, max_s: f64, count: usize) -> Vec<f64> {
    let (a, b) = (min_s.ln(), max_s.ln());
    (0..count)
        .map(|i| (a + (b - a) * i as f64 / (count - 1).max(1) as f64).exp())
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub period_s: f64,
    /// Fraction of the detrended variance this period explains.
    pub power: f64,
    /// Peak power over the median power of the whole search.
    pub prominence: f64,
    /// A wheel whose turn is within 6% of this period, if any.
    pub wheel: Option<String>,
}

/// The strongest local maxima of a periodogram, strongest first.
pub fn peaks(periods: &[f64], power: &[f64], wheels: &[Wheel], count: usize) -> Vec<Component> {
    let mut sorted = power.to_vec();
    let med = median(&mut sorted).max(1e-12);
    let mut found: Vec<Component> = (1..power.len().saturating_sub(1))
        .filter(|&i| power[i] >= power[i - 1] && power[i] > power[i + 1])
        .map(|i| {
            let p = periods[i];
            let wheel = wheels
                .iter()
                .find(|w| (p / w.period_s - 1.0).abs() < 0.06)
                .map(|w| w.name.clone());
            Component {
                period_s: p,
                power: power[i],
                prominence: power[i] / med,
                wheel,
            }
        })
        .collect();
    found.sort_by(|a, b| b.power.total_cmp(&a.power));
    found.truncate(count);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_sinusoid() {
        let t: Vec<f64> = (0..600).map(|i| i as f64).collect();
        let y: Vec<f64> = t
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                (2.0 * std::f64::consts::PI * t / 60.0).sin()
                    + 0.3 * (((i * 7919) % 13) as f64 / 6.0 - 1.0)
            })
            .collect();
        let s = Series { t, y };
        let periods = log_periods(5.0, 200.0, 400);
        let pw = lomb_scargle(&s, &periods);
        let top = peaks(&periods, &pw, &standard_wheels(28800, 15), 1);
        assert!((top[0].period_s - 60.0).abs() < 2.0, "{:?}", top[0]);
        assert_eq!(top[0].wheel.as_deref(), Some("fourth wheel"));
    }
}
