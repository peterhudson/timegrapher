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
    vec![
        Wheel {
            name: "escape wheel".into(),
            period_s: crate::train::escape_period_s(bph, escape_teeth),
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

/// The wheels to name when the movement is not known: none. Which wheel
/// turns once a minute or once an hour, and how many teeth the escape
/// wheel has, depend on the calibre, so without one no cycle is named
/// after a wheel; [`reference_lines`] mark the common periods instead.
pub fn default_wheels(_bph: u32) -> Vec<Wheel> {
    Vec::new()
}

/// The escape wheel's two common turn periods, for a calibre whose table
/// gives no escape wheel. Escape wheels have 15 teeth in many movements
/// and 20 in others (the ETA 2824 family and Rolex's), so both are named
/// "escape wheel" and the line the recording shows picks one: at 28,800
/// vph they are 3.75 s and 5 s, and neither is a whole multiple of the
/// other.
pub fn common_escape_wheels(bph: u32) -> Vec<Wheel> {
    [15, 20]
        .into_iter()
        .map(|t| standard_wheels(bph, t).remove(0))
        .collect()
}

/// A period marked on a chart for reference, not a wheel.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReferenceLine {
    pub label: &'static str,
    pub period_s: f64,
}

/// What to mark when no wheel is named: one minute and one hour, the
/// periods a seconds hand's and a minute hand's wheels turn at in most
/// movements, given as periods alone.
pub fn reference_lines() -> Vec<ReferenceLine> {
    vec![
        ReferenceLine {
            label: "1 min",
            period_s: 60.0,
        },
        ReferenceLine {
            label: "1 h",
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

pub(crate) fn polyfit2(t: &[f64], y: &[f64]) -> [f64; 3] {
    let t0 = t[0];
    let rows = t.iter().zip(y).map(|(&ti, &yi)| {
        let x = ti - t0;
        ([1.0, x, x * x], yi, 1.0)
    });
    let Some([c0, c1, c2]) = crate::dsp::least_squares(rows, 1e-300) else {
        return [0.0; 3];
    };
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
            let mut sums = LsSums {
                n: n as f64,
                ..LsSums::default()
            };
            for (&t, &v) in s.t.iter().zip(&y) {
                let (a, b) = (w * t).sin_cos();
                sums.yc += v * b;
                sums.ys += v * a;
                let (a2, b2) = (2.0 * w * t).sin_cos();
                sums.c2 += b2;
                sums.s2 += a2;
            }
            let f = sums.fit();
            (f.yc * f.yc / f.cc.max(1e-300) + f.ys * f.ys / f.sn.max(1e-300)) / ss
        })
        .collect()
}

/// The sums over the readings (less their mean) at one angular frequency
/// ω that a Lomb–Scargle fit is made from: Σ y·cos ωt, Σ y·sin ωt,
/// Σ cos 2ωt and Σ sin 2ωt, and how many readings. [`lomb_scargle`] adds
/// them up reading by reading; [`crate::spectrum`] reads them off two
/// Fourier transforms of a grid with gaps.
#[derive(Debug, Clone, Copy, Default)]
pub struct LsSums {
    pub yc: f64,
    pub ys: f64,
    pub c2: f64,
    pub s2: f64,
    pub n: f64,
}

/// [`LsSums`] turned about Scargle's τ (tan 2ωτ = Σ sin 2ωt / Σ cos 2ωt),
/// where the cosine and sine terms are orthogonal: Σ y·cos ω(t − τ),
/// Σ y·sin ω(t − τ), Σ cos² ω(t − τ) and Σ sin² ω(t − τ).
#[derive(Debug, Clone, Copy)]
pub struct LsFit {
    pub yc: f64,
    pub ys: f64,
    pub cc: f64,
    pub sn: f64,
}

impl LsSums {
    pub fn fit(&self) -> LsFit {
        let wt2 = self.s2.atan2(self.c2);
        let (sin, cos) = (0.5 * wt2).sin_cos();
        let c2t = self.c2 * wt2.cos() + self.s2 * wt2.sin();
        LsFit {
            yc: self.yc * cos + self.ys * sin,
            ys: self.ys * cos - self.yc * sin,
            cc: 0.5 * (self.n + c2t),
            sn: 0.5 * (self.n - c2t),
        }
    }
}

impl LsFit {
    /// The size (half the peak to peak) of the sine fitted by least
    /// squares.
    pub fn amplitude(&self) -> f64 {
        ((self.yc / self.cc.max(1e-300)).powi(2) + (self.ys / self.sn.max(1e-300)).powi(2)).sqrt()
    }
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
    fn common_escape_wheels_are_both_escape_wheels() {
        let escape: Vec<(String, f64)> = common_escape_wheels(28800)
            .into_iter()
            .map(|w| (w.name, w.period_s))
            .collect();
        assert_eq!(
            escape,
            vec![("escape wheel".into(), 3.75), ("escape wheel".into(), 5.0)]
        );
    }

    #[test]
    fn with_no_calibre_no_wheel_is_named_and_reference_lines_are_plain() {
        assert!(default_wheels(28800).is_empty());
        assert!(default_wheels(21600).is_empty());
        let r: Vec<(&str, f64)> = reference_lines()
            .iter()
            .map(|l| (l.label, l.period_s))
            .collect();
        assert_eq!(r, vec![("1 min", 60.0), ("1 h", 3600.0)]);
    }

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

    /// A bin needs two readings, skips non-finite ones, and sits at its
    /// middle.
    #[test]
    fn a_bin_takes_two_finite_readings_and_sits_at_its_middle() {
        let times = [0.1, 0.5, 1.2, 2.1, 2.4, 2.6, 2.9];
        let values = [1.0, 3.0, 9.0, 5.0, f64::NAN, 7.0, 6.0];
        let s = bin_median(&times, &values, 1.0);
        assert_eq!(s.t, vec![0.6, 2.6]);
        assert_eq!(s.y, vec![2.0, 6.0]);
    }

    /// Over 2 h the drift taken out is a running median 15 min either
    /// side: a 800 s step up from a level of 3, under half the window,
    /// is left at its full height, and the level goes to 0.
    #[test]
    fn over_two_hours_a_running_median_of_half_an_hour_is_taken_out() {
        let t: Vec<f64> = (0..1080).map(|i| f64::from(i) * 10.0).collect();
        let y = t
            .iter()
            .map(|&t| {
                if (5000.0..5800.0).contains(&t) {
                    13.0
                } else {
                    3.0
                }
            })
            .collect();
        let d = detrend(&Series { t: t.clone(), y });
        let at = |s: f64| d.y[t.iter().position(|&x| x == s).unwrap()];
        assert!((at(5400.0) - 10.0).abs() < 1e-12, "{}", at(5400.0));
        assert!(at(2000.0).abs() < 1e-12 && at(9000.0).abs() < 1e-12);
    }

    /// Under 2 h a quadratic is taken out, whole.
    #[test]
    fn under_two_hours_a_quadratic_is_taken_out() {
        let t: Vec<f64> = (0..360).map(|i| f64::from(i) * 10.0).collect();
        let y = t.iter().map(|&t| 2.0 - 1e-3 * t + 4e-7 * t * t).collect();
        let d = detrend(&Series { t, y });
        assert!(d.y.iter().all(|v| v.abs() < 1e-9), "{:?}", &d.y[..4]);
    }

    /// With readings missing, the periodogram is still the share of the
    /// variance a least-squares sine at each period explains: Scargle's τ
    /// and the cos² and sin² sums at it make that so on an uneven grid.
    #[test]
    fn with_gaps_the_power_is_a_least_squares_sines_share() {
        let tau = std::f64::consts::TAU;
        let t: Vec<f64> = (0..900)
            .map(f64::from)
            .filter(|&t| !(130.0..210.0).contains(&t) && (t % 7.0 != 0.0 || t > 600.0))
            .collect();
        let y: Vec<f64> = t
            .iter()
            .enumerate()
            .map(|(i, &t)| {
                1.5 + (tau * t / 47.0 + 0.8).sin() + 0.4 * (((i * 7919) % 13) as f64 / 6.0 - 1.0)
            })
            .collect();
        let s = Series {
            t: t.clone(),
            y: y.clone(),
        };
        let periods = [31.0, 47.0, 64.5, 101.0];
        let got = lomb_scargle(&s, &periods);
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        let y: Vec<f64> = y.iter().map(|v| v - mean).collect();
        let ss: f64 = y.iter().map(|v| v * v).sum();
        for (&p, &g) in periods.iter().zip(&got) {
            let (c, sn): (Vec<f64>, Vec<f64>) = t
                .iter()
                .map(|&t| ((tau * t / p).cos(), (tau * t / p).sin()))
                .unzip();
            let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
            let (a11, a12, a22) = (dot(&c, &c), dot(&c, &sn), dot(&sn, &sn));
            let (b1, b2) = (dot(&c, &y), dot(&sn, &y));
            let det = a11 * a22 - a12 * a12;
            let (u, v) = ((a22 * b1 - a12 * b2) / det, (a11 * b2 - a12 * b1) / det);
            let want = (u * b1 + v * b2) / ss;
            assert!((g - want).abs() < 1e-9, "{p} s: {g} against {want}");
        }
        assert!(got[1] > 0.5, "{got:?}");
    }

    /// The trial periods run from the shortest to exactly the longest.
    #[test]
    fn the_trial_periods_end_at_the_longest() {
        let p = log_periods(4.0, 400.0, 5);
        assert_eq!(p.len(), 5);
        assert!(
            (p[0] - 4.0).abs() < 1e-12 && (p[4] - 400.0).abs() < 1e-9,
            "{p:?}"
        );
        assert!((p[2] - 40.0).abs() < 1e-9, "{p:?}");
    }

    /// A flat-topped peak is found, once, at its last point; its
    /// prominence is its power over the median power.
    #[test]
    fn a_flat_topped_peak_is_found_with_its_prominence() {
        let periods = [10.0, 20.0, 30.0, 40.0, 50.0];
        let found = peaks(&periods, &[0.1, 0.6, 0.6, 0.2, 0.1], &[], 4);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].period_s, 30.0);
        assert!((found[0].prominence - 3.0).abs() < 1e-12, "{found:?}");
    }

    /// A peak within 6% of a wheel's turn is named for it; 8% off is not.
    #[test]
    fn a_peak_within_six_percent_of_a_wheel_is_named() {
        let wheels = [Wheel {
            name: "fourth wheel".into(),
            period_s: 60.0,
        }];
        let named = |p: f64| {
            peaks(&[50.0, p, 70.0], &[0.1, 0.5, 0.1], &wheels, 1)[0]
                .wheel
                .clone()
        };
        assert_eq!(named(62.4).as_deref(), Some("fourth wheel"));
        assert_eq!(named(57.6).as_deref(), Some("fourth wheel"));
        assert_eq!(named(64.8), None);
    }
}
