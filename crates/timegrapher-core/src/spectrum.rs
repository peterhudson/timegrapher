//! The plain amplitude spectrum of a series: an estimate of sine-wave magnitude at each
//! period the series holds, in the series' own units. This is the view a
//! commercial timegrapher's FFT gives. The period search in [`longterm`]
//! works on the same Fourier transform, but whitens it and sums harmonics
//! to decide which lines stand out; this keeps the raw sizes, so a strong
//! line and a weak one can be compared on one axis (log scale helps).
//!
//! [`longterm`]: crate::longterm

use crate::longterm::Grid;
use realfft::RealFftPlanner;
use serde::Serialize;

/// The spectrum, thinned for drawing.
#[derive(Debug, Clone, Serialize)]
pub struct AmplitudeSpectrum {
    /// `rate` (from the beat timing), `amplitude` or `beat_error`.
    pub series: &'static str,
    /// Unit of `amplitude`: `ms` of timing for the rate, `deg`, `ms`.
    pub unit: &'static str,
    /// Spacing of the series the spectrum was taken of, seconds.
    pub step_s: f64,
    /// Periods, seconds, longest first, and estimated sine-wave size at
    /// each for complete regular inputs (half its peak to peak). Each point is the largest in its
    /// slice of a log-spaced period axis, so narrow lines survive the
    /// thinning.
    pub period_s: Vec<f64>,
    pub amplitude: Vec<f64>,
    /// Finite support of the actual FFT input grid, excluding zero-padding.
    pub support: SpectrumSupport,
    /// The same sizes averaged over segments that hold no missing
    /// reading ([`welch`]); `None` when fewer than two segments fit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub welch: Option<Welch>,
    /// The rate's spectrum (of the beat timing, in ms) as the swing it
    /// makes in the rate readings ([`reading_swing`]); `None` for the
    /// other series.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading_swing: Option<ReadingSwing>,
}

/// The rate's spectrum as the swing, peak to peak in s/d, that each line
/// of the beat timing makes in rate readings of `reading_s`, over the
/// periods those readings can show (twice a reading or longer).
#[derive(Debug, Clone, Serialize)]
pub struct ReadingSwing {
    pub reading_s: f64,
    /// Periods, seconds, longest first, and the swing at each, s/d.
    pub period_s: Vec<f64>,
    pub swing_s_per_day: Vec<f64>,
}

/// The peak-to-peak swing, s/d, that a beat-timing sine `amp_ms` in size
/// with period `period_s` makes in rate readings each averaged over
/// `reading_s`: the timing's slope, `2π·amp/period` ms per second, is
/// 86.4 s/d for each ms per second, and averaging over a reading keeps
/// `sin(x)/x` of it, `x = π·reading/period`.
pub fn reading_swing(amp_ms: f64, period_s: f64, reading_s: f64) -> f64 {
    let x = std::f64::consts::PI * reading_s / period_s;
    let kept = if x == 0.0 { 1.0 } else { (x.sin() / x).abs() };
    2.0 * amp_ms * std::f64::consts::TAU / period_s * 86.4 * kept
}

impl AmplitudeSpectrum {
    /// This spectrum with its [`ReadingSwing`] for rate readings of
    /// `reading_s`, when it is the rate's (the beat timing, in ms).
    pub fn with_reading_swing(mut self, reading_s: f64) -> Self {
        if self.series == "rate" && self.unit == "ms" {
            let (period_s, swing_s_per_day) = self
                .period_s
                .iter()
                .zip(&self.amplitude)
                .filter(|(&p, _)| p >= 2.0 * reading_s)
                .map(|(&p, &a)| (p, reading_swing(a, p, reading_s)))
                .unzip();
            self.reading_swing = Some(ReadingSwing {
                reading_s,
                period_s,
                swing_s_per_day,
            });
        }
        self
    }
}

/// Welch's averaged spectrum: segments of one length, each wholly inside
/// an unbroken stretch, overlapping by half, each with its mean taken off
/// and a Hann window applied; the size at each period is from the mean
/// power over the segments. Nothing is filled in, so gaps change the
/// count of segments but not the sizes. Its longest period is half a
/// segment.
#[derive(Debug, Clone, Serialize)]
pub struct Welch {
    pub segment_s: f64,
    pub segments: usize,
    /// Periods, seconds, longest first, and sizes, thinned as the whole
    /// series' spectrum is.
    pub period_s: Vec<f64>,
    pub amplitude: Vec<f64>,
}

/// Segments in a Welch average at least, and readings in one at least.
const WELCH_MIN_SEGMENTS: usize = 2;
const WELCH_MIN_LEN: usize = 64;

/// [`Welch`]'s spectrum of `g`: segments of the largest power of two of
/// readings that is at most a quarter of the series and fits in its
/// longest unbroken stretch.
pub fn welch(g: &Grid, scale: f64, max_points: usize) -> Option<Welch> {
    let n = g.y.len();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < n {
        if g.y[i].is_finite() {
            let a = i;
            while i < n && g.y[i].is_finite() {
                i += 1;
            }
            runs.push((a, i));
        } else {
            i += 1;
        }
    }
    let longest = runs.iter().map(|(a, b)| b - a).max()?;
    let cap = longest.min(n / 4);
    if cap < WELCH_MIN_LEN {
        return None;
    }
    let l = 1usize << (usize::BITS - 1 - cap.leading_zeros());
    let w: Vec<f64> = (0..l)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * (i as f64 + 0.5) / l as f64).cos())
        .collect();
    let wsum: f64 = w.iter().sum();
    let mut planner = RealFftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(l);
    let mut power = vec![0.0; l / 2 + 1];
    let mut segments = 0;
    let mut buf = vec![0.0; l];
    let mut spec = fwd.make_output_vec();
    for &(a, b) in &runs {
        let mut s = a;
        while s + l <= b {
            let part = &g.y[s..s + l];
            let mean = part.iter().sum::<f64>() / l as f64;
            for ((o, &v), wi) in buf.iter_mut().zip(part).zip(&w) {
                *o = (v - mean) * scale * wi;
            }
            fwd.process(&mut buf, &mut spec).ok()?;
            for (p, c) in power.iter_mut().zip(&spec) {
                *p += c.norm_sqr();
            }
            segments += 1;
            s += l / 2;
        }
    }
    if segments < WELCH_MIN_SEGMENTS {
        return None;
    }
    let df = 1.0 / (l as f64 * g.step);
    let jmin = 2;
    let jmax = (((1.0 / (4.0 * g.step)) / df).floor() as usize).min(power.len() - 1);
    if jmax <= jmin {
        return None;
    }
    let (lo, hi) = (
        (1.0 / (jmax as f64 * df)).ln(),
        (1.0 / (jmin as f64 * df)).ln(),
    );
    let slices = max_points.max(2);
    let mut best: Vec<Option<(f64, f64)>> = vec![None; slices];
    for (j, &pw) in power.iter().enumerate().take(jmax + 1).skip(jmin) {
        let p = 1.0 / (j as f64 * df);
        let a = 2.0 * (pw / segments as f64).sqrt() / wsum;
        let k = (((hi - p.ln()) / (hi - lo)) * (slices - 1) as f64).round() as usize;
        let k = k.min(slices - 1);
        if best[k].is_none_or(|(_, b)| a > b) {
            best[k] = Some((p, a));
        }
    }
    let (period_s, amplitude) = best.into_iter().flatten().unzip();
    Some(Welch {
        segment_s: l as f64 * g.step,
        segments,
        period_s,
        amplitude,
    })
}

/// Input-grid availability, not acoustic coverage or share of watch beats heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SpectrumSupport {
    pub finite_bins: usize,
    pub total_bins: usize,
}

impl SpectrumSupport {
    pub fn description(&self) -> String {
        let mut text = format!(
            "FFT input: {} of {} grid samples available.",
            self.finite_bins, self.total_bins
        );
        if self.finite_bins < self.total_bins {
            text.push_str(" Missing samples affect peak heights and shapes; values are not corrected for gaps.");
        }
        text
    }
}

/// The amplitude spectrum of `g` between `4 × step` and half the span,
/// with the series' mean taken off and empty readings set to it. A Hann
/// window keeps a strong line from spreading over the whole axis; sizes
/// are corrected for it, estimating sine-wave size for complete regular data.
/// Nonfinite bins become zero after subtracting the finite-data mean. The
/// denominator remains the full grid's Hann-weight sum, without a correction
/// for missing data; gap patterns therefore affect heights and shapes.
/// `scale` multiplies the values (1000 for seconds to ms). At most
/// `max_points` points.
pub fn amplitude_spectrum(
    g: &Grid,
    series: &'static str,
    unit: &'static str,
    scale: f64,
    max_points: usize,
) -> Option<AmplitudeSpectrum> {
    let n = g.y.len();
    let fin: Vec<f64> = g.y.iter().copied().filter(|v| v.is_finite()).collect();
    if fin.len() < 16 || g.step <= 0.0 {
        return None;
    }
    let mean = fin.iter().sum::<f64>() / fin.len() as f64;
    let l = n.next_power_of_two();
    let mut buf = vec![0.0; l];
    let mut wsum = 0.0;
    for (i, (b, &v)) in buf.iter_mut().zip(&g.y).enumerate() {
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * (i as f64 + 0.5) / n as f64).cos();
        if v.is_finite() {
            *b = (v - mean) * scale * w;
        }
        wsum += w;
    }
    let mut planner = RealFftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(l);
    let mut spec = fwd.make_output_vec();
    fwd.process(&mut buf, &mut spec).ok()?;
    let df = 1.0 / (l as f64 * g.step);
    let (min_p, max_p) = (4.0 * g.step, g.span() / 2.0);
    let jmin = ((1.0 / max_p) / df).ceil().max(1.0) as usize;
    let jmax = (((1.0 / min_p) / df).floor() as usize).min(spec.len() - 1);
    if jmax <= jmin {
        return None;
    }
    // Thin on a log period axis, keeping the largest value in each slice.
    let (lo, hi) = (
        (1.0 / (jmax as f64 * df)).ln(),
        (1.0 / (jmin as f64 * df)).ln(),
    );
    let slices = max_points.max(2);
    let mut best: Vec<Option<(f64, f64)>> = vec![None; slices];
    for (j, c) in spec.iter().enumerate().take(jmax + 1).skip(jmin) {
        let p = 1.0 / (j as f64 * df);
        let a = 2.0 * c.norm() / wsum;
        let k = (((hi - p.ln()) / (hi - lo)) * (slices - 1) as f64).round() as usize;
        let k = k.min(slices - 1);
        if best[k].is_none_or(|(_, b)| a > b) {
            best[k] = Some((p, a));
        }
    }
    let (period_s, amplitude) = best.into_iter().flatten().unzip();
    Some(AmplitudeSpectrum {
        series,
        unit,
        step_s: g.step,
        period_s,
        amplitude,
        support: SpectrumSupport {
            finite_bins: fin.len(),
            total_bins: n,
        },
        welch: welch(g, scale, max_points),
        reading_swing: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timing_wave_is_drawn_as_the_swing_in_rate_readings() {
        // 0.157 ms of timing at 46 s is 3.7 s/d peak to peak in the rate
        // itself; a 10 s reading keeps sin(x)/x of it, x = π·10/46.
        let x = std::f64::consts::PI * 10.0 / 46.0;
        let swing = reading_swing(0.157, 46.0, 10.0);
        assert!((swing - 3.70 * x.sin() / x).abs() < 0.02, "{swing}");
        // A wave as long as a reading averages away.
        assert!(reading_swing(1.0, 10.0, 10.0) < 1e-9);
    }

    #[test]
    fn only_the_rate_carries_its_swing_from_twice_a_reading() {
        let g = Grid {
            t0: 0.0,
            step: 10.0,
            y: (0..400)
                .map(|i| 1e-4 * (std::f64::consts::TAU * i as f64 / 9.0).sin())
                .collect(),
        };
        let rate = amplitude_spectrum(&g, "rate", "ms", 1000.0, 600)
            .unwrap()
            .with_reading_swing(10.0);
        let s = rate.reading_swing.as_ref().expect("the rate's swing");
        assert_eq!(s.reading_s, 10.0);
        assert!(s.period_s.iter().all(|&p| p >= 20.0));
        for (p, w) in s.period_s.iter().zip(&s.swing_s_per_day) {
            let k = rate.period_s.iter().position(|q| q == p).unwrap();
            assert_eq!(*w, reading_swing(rate.amplitude[k], *p, 10.0));
        }
        let amp = amplitude_spectrum(&g, "amplitude", "deg", 1.0, 600)
            .unwrap()
            .with_reading_swing(10.0);
        assert!(amp.reading_swing.is_none());
    }

    #[test]
    fn a_sine_reads_its_size_at_its_period() {
        // 0.3 ms at 5 s and 0.05 ms at 60 s, on a 0.25 s grid over 2 h.
        let step = 0.25;
        let n = (7200.0 / step) as usize;
        let y: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 * step;
                let tau = 2.0 * std::f64::consts::PI;
                3e-4 * (tau * t / 5.0).sin() + 5e-5 * (tau * t / 60.0).sin() + 0.01
            })
            .collect();
        let g = Grid { t0: 0.0, step, y };
        let s = amplitude_spectrum(&g, "rate", "ms", 1000.0, 600).unwrap();
        assert!(s.period_s.windows(2).all(|w| w[0] > w[1]));
        let at = |p: f64| {
            s.period_s
                .iter()
                .zip(&s.amplitude)
                .filter(|(q, _)| (*q / p - 1.0).abs() < 0.02)
                .map(|(_, a)| *a)
                .fold(0.0, f64::max)
        };
        assert!((at(5.0) - 0.3).abs() < 0.03, "{}", at(5.0));
        assert!((at(60.0) - 0.05).abs() < 0.01, "{}", at(60.0));
        // Away from the lines the window keeps the floor far below them.
        assert!(at(20.0) < 0.005, "{}", at(20.0));
    }

    #[test]
    fn support_counts_actual_input_grid_without_changing_missing_data_eligibility() {
        for pattern in [
            "complete",
            "alternating",
            "central_half",
            "edge_half",
            "nonfinite",
        ] {
            let n = 256;
            let y = (0..n)
                .map(|i| {
                    let v = 10.0 * (std::f64::consts::TAU * i as f64 / 64.0).sin();
                    match pattern {
                        "alternating" if i % 2 != 0 => f64::NAN,
                        "central_half" if (64..192).contains(&i) => f64::NAN,
                        "edge_half" if !(64..192).contains(&i) => f64::NAN,
                        "nonfinite" if i == 0 => f64::INFINITY,
                        "nonfinite" if i == 1 => f64::NEG_INFINITY,
                        _ => v,
                    }
                })
                .collect::<Vec<_>>();
            let expected = y.iter().filter(|v| v.is_finite()).count();
            let s = amplitude_spectrum(
                &Grid {
                    t0: 0.0,
                    step: 1.0,
                    y,
                },
                "amplitude",
                "deg",
                1.0,
                600,
            )
            .unwrap();
            assert_eq!(
                s.support,
                SpectrumSupport {
                    finite_bins: expected,
                    total_bins: n
                }
            );
            assert_eq!(s.period_s.len(), s.amplitude.len());
            assert!(s.amplitude.iter().all(|v| v.is_finite()));
            assert!(s
                .support
                .description()
                .contains(&format!("{expected} of {n}")));
            assert_eq!(
                s.support.description().contains("not corrected for gaps"),
                expected < n
            );
            assert_eq!(
                serde_json::to_value(&s).unwrap()["support"]["finite_bins"],
                expected
            );
        }
        // Counts exclude next-power-of-two FFT padding.
        let s = amplitude_spectrum(
            &Grid {
                t0: 0.0,
                step: 1.0,
                y: (0..65).map(|i| (i as f64).sin()).collect(),
            },
            "rate",
            "ms",
            1.0,
            600,
        )
        .unwrap();
        assert_eq!(s.support.total_bins, 65);
        assert_eq!(s.support.finite_bins, 65);
    }

    #[test]
    fn welch_reads_a_line_through_gaps_without_filling_them() {
        // 0.4 deg at 60 s on 2 s windows over 4 h, with a 10 min gap every
        // hour: the whole-series spectrum has the gaps in it; Welch's
        // segments stay clear of them and read the size.
        let step = 2.0;
        let y: Vec<f64> = (0..7200)
            .map(|i| {
                let t = i as f64 * step;
                if t % 3600.0 >= 3000.0 {
                    f64::NAN
                } else {
                    0.4 * (std::f64::consts::TAU * t / 60.0).sin() + 250.0
                }
            })
            .collect();
        let g = Grid { t0: 0.0, step, y };
        let w = welch(&g, 1.0, 600).unwrap();
        // 1500 readings a stretch: segments of 1024, one per stretch.
        assert_eq!(w.segment_s, 2048.0);
        assert_eq!(w.segments, 4);
        let at = |p: f64| {
            w.period_s
                .iter()
                .zip(&w.amplitude)
                .filter(|(q, _)| (*q / p - 1.0).abs() < 0.02)
                .map(|(_, a)| *a)
                .fold(0.0, f64::max)
        };
        assert!((at(60.0) - 0.4).abs() < 0.04, "{}", at(60.0));
        assert!(at(200.0) < 0.01, "{}", at(200.0));
        // A series with no stretch long enough has none.
        let short = Grid {
            t0: 0.0,
            step,
            y: (0..400)
                .map(|i| if i % 50 == 0 { f64::NAN } else { 1.0 })
                .collect(),
        };
        assert!(welch(&short, 1.0, 600).is_none());
    }

    #[test]
    fn too_short_gives_none() {
        let g = Grid {
            t0: 0.0,
            step: 1.0,
            y: vec![1.0; 8],
        };
        assert!(amplitude_spectrum(&g, "rate", "ms", 1.0, 100).is_none());
    }
}
