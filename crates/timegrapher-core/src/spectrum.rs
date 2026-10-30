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
    /// Periods, seconds, longest first, and the size of the sine fitted
    /// at each (half its peak to peak). Each point is the largest in its
    /// slice of a log-spaced period axis, so narrow lines survive the
    /// thinning.
    pub period_s: Vec<f64>,
    pub amplitude: Vec<f64>,
    /// Every size is read the one way, a sine fitted to the readings
    /// present with Hann weights ([`amplitude_spectrum`]); this names the
    /// grid: `hann_fft` with no reading missing, where that is the Hann
    /// FFT's size, `lomb_scargle` with gaps (#652).
    pub method: &'static str,
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
/// In JSON it also carries its [`SpectrumSupport::description`], the one
/// wording every surface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpectrumSupport {
    pub finite_bins: usize,
    pub total_bins: usize,
}

impl Serialize for SpectrumSupport {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("SpectrumSupport", 3)?;
        st.serialize_field("finite_bins", &self.finite_bins)?;
        st.serialize_field("total_bins", &self.total_bins)?;
        st.serialize_field("description", &self.description())?;
        st.end()
    }
}

impl SpectrumSupport {
    pub fn description(&self) -> String {
        let mut text = format!(
            "FFT input: {} of {} grid samples available.",
            self.finite_bins, self.total_bins
        );
        if self.finite_bins < self.total_bins {
            text.push_str(" With samples missing, each size is a sine fitted to the samples present, weighted by the Hann window (Lomb–Scargle); nothing is filled in.");
        }
        text
    }
}

/// The amplitude spectrum of `g` between `4 × step` and half the span,
/// as the size of the sine at each period (half its peak to peak): the
/// sine fitted by least squares to the readings present, each weighted by
/// a Hann window over the whole grid, less their weighted mean
/// (Lomb–Scargle with weights), nothing filled in. With every reading
/// present this is the Hann-windowed FFT's size, the window keeping a
/// strong line from spreading over the whole axis; with readings missing
/// a line keeps its size, and one reading out leaves the spectrum as it
/// was (#652). Its sums are read off two Fourier transforms (the weighted
/// readings, zero where missing, and the weights of those present), as
/// Press and Rybicki (1989) do. `scale` multiplies the values (1000 for
/// seconds to ms). At most `max_points` points.
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
    let gaps = fin.len() < n;
    // The Hann taper over the whole grid, the weight each present reading
    // carries in the fit (#652).
    let hann =
        |i: usize| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * (i as f64 + 0.5) / n as f64).cos();
    let (mut sw, mut swy) = (0.0, 0.0);
    for (i, &v) in g.y.iter().enumerate() {
        if v.is_finite() {
            sw += hann(i);
            swy += hann(i) * v;
        }
    }
    let mean = swy / sw;
    let l = n.next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(l);
    let transform = |mut buf: Vec<f64>| {
        let mut out = fwd.make_output_vec();
        fwd.process(&mut buf, &mut out).ok().map(|_| out)
    };
    let mut buf = vec![0.0; l];
    let mut present = vec![0.0; l];
    for (i, &v) in g.y.iter().enumerate() {
        if v.is_finite() {
            buf[i] = (v - mean) * scale * hann(i);
            present[i] = hann(i);
        }
    }
    let spec = transform(buf)?;
    let mask = transform(present)?;
    // The size at bin j: the Hann-weighted Lomb–Scargle fit from the sums
    // at ω and 2ω (bin 2j). With every reading present it is the Hann
    // FFT's own size, 2|X|/Σw, but for the 2ω term near 0.
    let size = |j: usize| -> f64 {
        let m = mask.get(2 * j).copied().unwrap_or_default();
        crate::periodicity::LsSums {
            yc: spec[j].re,
            ys: -spec[j].im,
            c2: m.re,
            s2: -m.im,
            n: sw,
        }
        .fit()
        .amplitude()
    };
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
    for j in jmin..=jmax {
        let p = 1.0 / (j as f64 * df);
        let a = size(j);
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
        method: if gaps { "lomb_scargle" } else { "hann_fft" },
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
                s.support.description().contains("Lomb–Scargle"),
                expected < n
            );
            assert_eq!(s.method == "lomb_scargle", expected < n);
            assert_eq!(
                serde_json::to_value(&s).unwrap()["support"]["finite_bins"],
                expected
            );
            assert_eq!(
                serde_json::to_value(&s).unwrap()["support"]["description"],
                s.support.description()
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

    /// 2 at 320 s on 10 s readings: the size at that period, largest
    /// near it.
    fn size_at_320(y: Vec<f64>) -> (f64, &'static str) {
        let s = amplitude_spectrum(
            &Grid {
                t0: 0.0,
                step: 10.0,
                y,
            },
            "amplitude",
            "deg",
            1.0,
            600,
        )
        .unwrap();
        let a = s
            .period_s
            .iter()
            .zip(&s.amplitude)
            .filter(|(p, _)| (*p / 320.0 - 1.0).abs() < 0.03)
            .map(|(_, a)| *a)
            .fold(0.0, f64::max);
        (a, s.method)
    }

    fn line_320(i: usize) -> f64 {
        250.0 + 2.0 * (std::f64::consts::TAU * i as f64 * 10.0 / 320.0).sin()
    }

    /// A line keeps its size as readings go missing: with gaps the size is
    /// a sine fitted to the readings present, where filling them with the
    /// mean shrank it by the share missing (#652). With every reading
    /// present the windowed FFT gives the same size.
    #[test]
    fn a_lines_size_holds_as_readings_go_missing() {
        let whole = size_at_320((0..2048).map(line_320).collect());
        assert_eq!(whole.1, "hann_fft");
        assert!((whole.0 - 2.0).abs() < 0.02, "{}", whole.0);
        for missing in [1, 3, 5] {
            // `missing` of every ten minutes' readings gone, in a block.
            let y = (0..2048)
                .map(|i| {
                    if i % 60 < 6 * missing {
                        f64::NAN
                    } else {
                        line_320(i)
                    }
                })
                .collect();
            let (a, method) = size_at_320(y);
            assert_eq!(method, "lomb_scargle");
            assert!((a - 2.0).abs() < 0.1, "{missing}/10 missing: {a}");
        }
    }

    /// Every bin of the spectrum, longest period first.
    fn bins(y: Vec<f64>) -> Vec<f64> {
        let g = Grid {
            t0: 0.0,
            step: 10.0,
            y,
        };
        amplitude_spectrum(&g, "rate", "ms", 1.0, 1 << 20)
            .unwrap()
            .amplitude
    }

    /// Seeded AR(1) readings at `rho`, scatter 1.
    fn red(n: usize, rho: f64, seed: u64) -> Vec<f64> {
        let mut z = crate::synth::Rng::new(seed);
        let mut v = z.normal();
        (0..n)
            .map(|_| {
                v = rho * v + (1.0 - rho * rho).sqrt() * z.normal();
                v
            })
            .collect()
    }

    /// The median of `a`'s bins over `b`'s, the furthest from 1 of their
    /// 16-bin band means' ratios, and whether the largest bin is the same.
    fn compare(a: &[f64], b: &[f64]) -> (f64, f64, bool) {
        assert_eq!(a.len(), b.len());
        let mut r: Vec<f64> = a.iter().zip(b).map(|(x, y)| x / y).collect();
        r.sort_by(f64::total_cmp);
        let band = a
            .chunks(16)
            .zip(b.chunks(16))
            .map(|(x, y)| x.iter().sum::<f64>() / y.iter().sum::<f64>())
            .fold(1.0f64, |w, q| {
                if (q - 1.0).abs() > (w - 1.0).abs() {
                    q
                } else {
                    w
                }
            });
        let top = |v: &[f64]| (0..v.len()).max_by(|&i, &j| v[i].total_cmp(&v[j]));
        (r[r.len() / 2], band, top(a) == top(b))
    }

    /// On a complete grid the weighted fit is the Hann FFT's size.
    #[test]
    fn a_complete_grid_reads_as_the_hann_fft() {
        let y = red(4096, 0.9, 11);
        let n = y.len();
        let w: Vec<f64> = (0..n)
            .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * (i as f64 + 0.5) / n as f64).cos())
            .collect();
        let m = y.iter().zip(&w).map(|(v, w)| v * w).sum::<f64>() / w.iter().sum::<f64>();
        let mut buf: Vec<f64> = y.iter().zip(&w).map(|(v, w)| (v - m) * w).collect();
        let mut out = RealFftPlanner::<f64>::new()
            .plan_fft_forward(n)
            .make_output_vec();
        RealFftPlanner::<f64>::new()
            .plan_fft_forward(n)
            .process(&mut buf, &mut out)
            .unwrap();
        let wsum: f64 = w.iter().sum();
        let s = bins(y);
        // Bins 2 (half the span) to n/4 (four readings): longest first.
        let hann: Vec<f64> = (2..=n / 4).map(|j| 2.0 * out[j].norm() / wsum).collect();
        assert_eq!(s.len(), hann.len());
        for (k, (a, b)) in s.iter().zip(&hann).enumerate() {
            assert!((a / b - 1.0).abs() < 0.005, "bin {k}: {a} against {b}");
        }
    }

    /// One reading out leaves red noise's spectrum as it was, and 1%
    /// scattered out moves it little: an unweighted fit moved single bins
    /// ×0.12 to ×5 with one reading out (#652).
    #[test]
    fn a_reading_or_a_few_out_leave_the_spectrum_as_it_was() {
        // At ρ 0.99 the shortest periods hold so little that the missing
        // reading's own spike shows over them: one out mid-take lifted a
        // band there by 10-14%, by under 3% at ρ 0.9.
        for (rho, seed, bound) in [(0.9, 3, 0.05), (0.99, 4, 0.15)] {
            let y = red(4096, rho, seed);
            let whole = bins(y.clone());
            let mut one = y.clone();
            one[1733] = f64::NAN;
            let (median, band, top) = compare(&bins(one), &whole);
            assert!(
                (median - 1.0).abs() < 0.02,
                "ρ {rho}, one out: median {median}"
            );
            assert!((band - 1.0).abs() < bound, "ρ {rho}, one out: band {band}");
            assert!(top, "ρ {rho}, one out: largest bin moved");
        }
        // 1% out at random lifts the shortest periods by the gaps' own
        // spikes, a few percent on average and up to a third in a single
        // 16-bin band, so each quarter of the period axis is held instead,
        // over twelve takes.
        for seed in 20..32u64 {
            let y = red(4096, 0.9, seed);
            let whole = bins(y.clone());
            let mut z = crate::synth::Rng::new(seed + 100);
            let few = bins(
                y.iter()
                    .map(|&v| if z.next_f64() < 0.01 { f64::NAN } else { v })
                    .collect(),
            );
            let (median, _, _) = compare(&few, &whole);
            assert!(
                (median - 1.0).abs() < 0.03,
                "1% out, take {seed}: median {median}"
            );
            let q = whole.len().div_ceil(4);
            for (k, (a, b)) in few.chunks(q).zip(whole.chunks(q)).enumerate() {
                let r = a.iter().sum::<f64>() / b.iter().sum::<f64>();
                assert!(
                    (r - 1.0).abs() < 0.10,
                    "1% out, take {seed}: quarter {k} {r}"
                );
            }
        }
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
