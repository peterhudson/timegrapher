//! The distributions of a take's readings, as data: what the app's
//! Distributions pane draws (a histogram, the summary line beside it, and
//! the points for probability paper or the cumulative share), for the
//! whole take and for each stretch of it, so an agent or a chart outside
//! the app can draw the same thing.
//!
//! The bins come from [`crate::histogram`] with the app's settings: the
//! reading's resolution as the finest bin, and the outermost 0.5% at each
//! end left off the scale as outliers.

use crate::histogram::{self, Histogram, Options};
use crate::longterm::Grid;
use serde::Serialize;

/// The shares at which [`Distribution::quantiles`] gives the value: the
/// marks the app puts on probability paper, from 0.1% to 99.9%.
pub const SHARES: [f64; 11] = [
    0.001, 0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99, 0.999,
];

/// Most points kept in [`Distribution::points`]: enough for probability
/// paper at any width a chart is drawn, small enough for a day's take.
pub const MAX_POINTS: usize = 400;

/// The share of values at each end left off the bins' scale as outliers.
pub const TRIM: f64 = 0.005;

/// Which reading a distribution counts, and how its values print.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kind {
    /// "rate", "amplitude" or "beat_error".
    pub series: &'static str,
    pub unit: &'static str,
    /// The finest bin width worth drawing.
    pub resolution: f64,
    /// Decimals and suffix in the summary line, as the app prints them.
    pub decimals: usize,
    pub suffix: &'static str,
}

pub const RATE: Kind = Kind {
    series: "rate",
    unit: "s/d",
    resolution: 0.1,
    decimals: 1,
    suffix: " s/day",
};
pub const AMPLITUDE: Kind = Kind {
    series: "amplitude",
    unit: "deg",
    resolution: 0.5,
    decimals: 0,
    suffix: "°",
};
pub const BEAT_ERROR: Kind = Kind {
    series: "beat_error",
    unit: "ms",
    resolution: 0.01,
    decimals: 2,
    suffix: " ms",
};

/// The straight line one normal population draws on probability paper:
/// the median, and the interquartile range over 1.349 as its spread.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NormalLine {
    pub centre: f64,
    pub spread: f64,
}

/// How often each value of one reading came up.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Distribution {
    pub series: &'static str,
    pub unit: &'static str,
    /// "readings" (one per reading length, as the tests read them) or
    /// "2 s" (each 2 seconds of beats, amplitude and beat error only).
    pub values: &'static str,
    /// Length of each value, seconds.
    pub step_s: f64,
    /// Values counted, their median, and the 10th and 90th percentiles
    /// (the middle 80%).
    pub n: usize,
    pub median: f64,
    pub p10: f64,
    pub p90: f64,
    /// Bin edges, one more than the counts, lowest first; bin `i` holds
    /// values from `edges[i]` up to, not including, `edges[i + 1]`.
    pub bin_width: f64,
    pub edges: Vec<f64>,
    pub counts: Vec<u32>,
    /// Values off the scale below the first edge and above the last (the
    /// outermost 0.5% each side); counted in `n`, the median and the
    /// percentiles.
    pub below: usize,
    pub above: usize,
    /// Centres of the bins that stand out as separate peaks (each holding
    /// at least 5% of the values, with a dip to half the lower one
    /// between). Two or more suggest the watch moves between states.
    pub peaks: Vec<f64>,
    /// The line for probability paper; `None` when the values have no
    /// spread.
    pub normal_line: Option<NormalLine>,
    /// `[share, value]` at each share in [`SHARES`] that the count can
    /// resolve (a share below 1/n or above 1 − 1/n is left out): enough
    /// to draw probability paper or the cumulative share.
    pub quantiles: Vec<[f64; 2]>,
    /// The probability plot the app draws, `[value, z]` lowest first:
    /// each value against the normal score of its rank
    /// ([`histogram::probability_plot`]), every value up to
    /// [`MAX_POINTS`], evenly by rank beyond that, the lowest and highest
    /// always kept.
    pub points: Vec<[f64; 2]>,
    /// The summary line the app shows beside the histogram.
    pub summary: String,
}

/// The value `q` (0 to 1) of the way through sorted values.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    let x = q * (sorted.len() - 1) as f64;
    let i = x.floor() as usize;
    let j = (i + 1).min(sorted.len() - 1);
    sorted[i] + (sorted[j] - sorted[i]) * (x - i as f64)
}

/// The distribution of `values` (non-finite ones are ignored); `None`
/// with fewer than three.
pub fn distribution(
    values: &[f64],
    kind: Kind,
    label: &'static str,
    step_s: f64,
) -> Option<Distribution> {
    let opt = Options {
        trim: TRIM,
        ..Options::new(kind.resolution)
    };
    let h: Histogram = histogram::histogram_with(values, &opt)?;
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    v.sort_by(f64::total_cmp);
    let n = v.len() as f64;
    let quantiles = SHARES
        .iter()
        .filter(|&&p| p >= 1.0 / n - 1e-12 && p <= 1.0 - 1.0 / n + 1e-12)
        .map(|&p| [p, quantile(&v, p)])
        .collect();
    let plot = histogram::probability_plot(&v);
    let points = if plot.len() <= MAX_POINTS {
        plot
    } else {
        let last = (plot.len() - 1) as f64;
        (0..MAX_POINTS)
            .map(|k| plot[(k as f64 * last / (MAX_POINTS - 1) as f64).round() as usize])
            .collect()
    };
    let peaks: Vec<f64> = h.peaks(0.05).into_iter().map(|i| h.centre(i)).collect();
    let mut d = Distribution {
        series: kind.series,
        unit: kind.unit,
        values: label,
        step_s,
        n: h.n,
        median: h.median,
        p10: h.p10,
        p90: h.p90,
        bin_width: h.bin_width,
        edges: (0..=h.counts.len())
            .map(|i| h.start + i as f64 * h.bin_width)
            .collect(),
        counts: h.counts,
        below: h.below,
        above: h.above,
        peaks,
        normal_line: histogram::normal_line(values)
            .map(|(centre, spread)| NormalLine { centre, spread }),
        quantiles,
        points,
        summary: String::new(),
    };
    d.summary = d.line(kind, "", None);
    Some(d)
}

impl Distribution {
    /// The summary line, as the app shows it beside the histogram: the
    /// count, the median, the middle 80%, then the outliers and the peaks
    /// when there are any. `over` follows the count (" in the session"),
    /// and `only` names the one position counted (" · Dial Up only").
    /// [`Distribution::summary`] is this with neither.
    pub fn line(&self, kind: Kind, over: &str, only: Option<&str>) -> String {
        let d = kind.decimals;
        let f = |x: f64| format!("{x:.d$}{}", kind.suffix);
        let per = if self.values == "2 s" {
            "2-second stretches"
        } else {
            "readings"
        };
        let mut line = format!(
            "{} {per}{over} · median {} · middle 80% {} to {}",
            self.n,
            f(self.median),
            f(self.p10),
            f(self.p90)
        );
        if let Some(p) = only {
            line += &format!(" · {p} only");
        }
        if self.below + self.above > 0 {
            line += &format!(" · {} outliers off the scale", self.below + self.above);
        }
        if self.peaks.len() > 1 {
            let at: Vec<String> = self.peaks.iter().map(|&x| f(x)).collect();
            line += &format!(" · {} peaks, at {}", self.peaks.len(), at.join(" and "));
        }
        line
    }
}

/// Seconds of beats in each "2 s" value, as the amplitude's windows.
pub const STRETCH_S: f64 = 2.0;

/// The rate over each [`STRETCH_S`] of `beats` on a grid from `t0`, as
/// `(start, s/day)`, from stretch `from` on, up to the last stretch the
/// beats reach the end of; returns the stretch to start from next time,
/// so beats arriving one block at a time are worked out once. A stretch
/// across a break (a time in `breaks`) or into a masked stretch
/// (`masked`, start and end) gives no value, as no reading does.
pub fn rates_2s(
    beats: &[crate::beats::Beat],
    bph: u32,
    t0: f64,
    from: usize,
    breaks: &[f64],
    masked: &[(f64, f64)],
    out: &mut Vec<(f64, f64)>,
) -> usize {
    let Some(last) = beats.last() else {
        return from;
    };
    let mut next = from;
    loop {
        let a = t0 + next as f64 * STRETCH_S;
        let b = a + STRETCH_S;
        if b > last.time + 1e-9 {
            return next;
        }
        next += 1;
        if breaks.iter().any(|&x| a < x && x < b) || masked.iter().any(|&(m0, m1)| a < m1 && b > m0)
        {
            continue;
        }
        let lo = beats.partition_point(|x| x.time < a);
        let hi = beats.partition_point(|x| x.time < b);
        if let Some(f) = crate::timing::fit(&beats[lo..hi], bph) {
            out.push((a, f.rate_s_per_day));
        }
    }
}

/// The distributions over one stretch of the take.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stretch {
    pub start_s: f64,
    pub end_s: f64,
    pub distributions: Vec<Distribution>,
}

/// The distributions over the whole take and over each stretch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Distributions {
    pub overall: Vec<Distribution>,
    /// Length of each stretch, seconds; 0 when the take is not split.
    pub stretch_s: f64,
    pub stretches: Vec<Stretch>,
}

/// One series to count: its kind, its label ("readings" or "2 s") and
/// its values on a grid of times.
pub struct Input<'a> {
    pub kind: Kind,
    pub label: &'static str,
    pub grid: &'a Grid,
}

/// The distributions of each input over the take, and over stretches of
/// `stretch_s` from the take's start when the take is at least two
/// stretches long. A last stretch shorter than half the length joins the
/// one before it. `stretch_s` of 0 leaves the take whole.
pub fn distributions(inputs: &[Input], start_s: f64, end_s: f64, stretch_s: f64) -> Distributions {
    let over = |a: f64, b: f64| -> Vec<Distribution> {
        inputs
            .iter()
            .filter_map(|i| {
                let v: Vec<f64> = i
                    .grid
                    .y
                    .iter()
                    .enumerate()
                    .filter(|&(k, _)| {
                        let t = i.grid.time(k);
                        t >= a && t < b
                    })
                    .map(|(_, &y)| y)
                    .collect();
                distribution(&v, i.kind, i.label, i.grid.step)
            })
            .collect()
    };
    let span = end_s - start_s;
    let mut stretches = Vec::new();
    let split = stretch_s > 0.0 && span >= 2.0 * stretch_s;
    if split {
        let mut bounds: Vec<f64> = Vec::new();
        let mut t = start_s;
        while t < end_s - 1e-9 {
            bounds.push(t);
            t += stretch_s;
        }
        if bounds.len() > 1 && end_s - bounds[bounds.len() - 1] < 0.5 * stretch_s {
            bounds.pop();
        }
        bounds.push(end_s);
        for w in bounds.windows(2) {
            // The last stretch takes in the end of the take itself.
            let b = if w[1] == end_s { f64::INFINITY } else { w[1] };
            stretches.push(Stretch {
                start_s: w[0],
                end_s: w[1],
                distributions: over(w[0], b),
            });
        }
    }
    Distributions {
        overall: over(f64::NEG_INFINITY, f64::INFINITY),
        stretch_s: if split { stretch_s } else { 0.0 },
        stretches,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_counts_and_summary_agree() {
        // Two states of amplitude, 230° and 250°, with a stray reading.
        let mut seed = 1u64;
        let mut u = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let mut y: Vec<f64> = (0..2000)
            .map(|i| {
                let mid = if i % 2 == 0 { 230.0 } else { 250.0 };
                let (a, b) = (u(), u());
                mid + 2.0 * (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
            })
            .collect();
        y.push(100.0);
        let d = distribution(&y, AMPLITUDE, "readings", 10.0).unwrap();
        assert_eq!(d.edges.len(), d.counts.len() + 1);
        assert!((d.edges[1] - d.edges[0] - d.bin_width).abs() < 1e-9);
        assert_eq!(
            d.counts.iter().sum::<u32>() as usize + d.below + d.above,
            d.n
        );
        assert_eq!(d.n, 2001);
        assert!(d.below >= 1);
        assert_eq!(d.peaks.len(), 2, "{d:?}");
        assert!(
            d.summary.starts_with("2001 readings · median "),
            "{}",
            d.summary
        );
        assert!((d.peaks[0] - 230.0).abs() <= 2.0 * d.bin_width, "{d:?}");
        assert!((d.peaks[1] - 250.0).abs() <= 2.0 * d.bin_width, "{d:?}");
        assert!(d.summary.contains(" · 2 peaks, at "), "{}", d.summary);
        assert!(d.summary.contains("outliers off the scale"));
        // 2001 values resolve 0.1% to 99.9%; 20 values only 5% to 95%.
        assert_eq!(d.quantiles.len(), SHARES.len());
        let few = distribution(&y[..20], AMPLITUDE, "readings", 10.0).unwrap();
        assert_eq!(few.quantiles.first().unwrap()[0], 0.05);
        assert_eq!(few.quantiles.last().unwrap()[0], 0.95);
        assert!(d.quantiles.windows(2).all(|w| w[0][1] <= w[1][1]));
        // The probability plot, thinned by rank, keeps both ends.
        assert_eq!(d.points.len(), MAX_POINTS);
        assert_eq!(d.points[0][0], 100.0);
        assert!(d
            .points
            .windows(2)
            .all(|w| w[0][0] <= w[1][0] && w[0][1] < w[1][1]));
        assert_eq!(few.points.len(), 20);
        assert!(distribution(&[1.0, f64::NAN, 2.0], RATE, "readings", 10.0).is_none());
    }

    #[test]
    fn two_states_leave_a_gap_in_the_points() {
        // A day of readings in two tight states, 230° and 250°: thinned to
        // MAX_POINTS, the plot still has nothing between them and keeps the
        // lowest and highest values.
        let y: Vec<f64> = (0..8640)
            .map(|i| {
                let mid = if i % 3 == 0 { 250.0 } else { 230.0 };
                mid + ((i * 7919) % 1000) as f64 / 1000.0 - 0.5
            })
            .collect();
        let d = distribution(&y, AMPLITUDE, "readings", 10.0).unwrap();
        assert_eq!(d.points.len(), MAX_POINTS);
        assert!(d.points.iter().all(|p| p[0] < 231.0 || p[0] > 249.0));
        assert!(d.points.iter().any(|p| p[0] < 231.0));
        assert!(d.points.iter().any(|p| p[0] > 249.0));
        let lo = y.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = y.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(d.points.first().unwrap()[0], lo);
        assert_eq!(d.points.last().unwrap()[0], hi);
    }

    #[test]
    fn a_long_take_splits_into_stretches() {
        // 2.6 h of 10 s readings rising by 1 s/d an hour.
        let n = 936;
        let grid = Grid {
            t0: 0.0,
            step: 10.0,
            y: (0..n)
                .map(|i| i as f64 * 10.0 / 3600.0 + (i % 5) as f64 * 0.1)
                .collect(),
        };
        let input = [Input {
            kind: RATE,
            label: "readings",
            grid: &grid,
        }];
        let d = distributions(&input, 0.0, n as f64 * 10.0, 3600.0);
        assert_eq!(d.stretch_s, 3600.0);
        // 2.6 h: two hours and a last 0.6 h of its own.
        assert_eq!(d.stretches.len(), 3);
        let counts: Vec<usize> = d.stretches.iter().map(|s| s.distributions[0].n).collect();
        assert_eq!(counts, vec![360, 360, 216]);
        assert_eq!(d.overall[0].n, n);
        assert!(d.stretches[0].distributions[0].median < d.stretches[2].distributions[0].median);
        // 2.3 h: the last 0.3 h joins the second hour.
        let d = distributions(&input, 0.0, 2.3 * 3600.0, 3600.0);
        assert_eq!(d.stretches.len(), 2);
        assert_eq!(d.stretches[1].end_s, 2.3 * 3600.0);
        // Shorter than two stretches, or 0: whole.
        let d = distributions(&input, 0.0, n as f64 * 10.0, 7200.0);
        assert!(d.stretches.is_empty() && d.stretch_s == 0.0);
        let d = distributions(&input, 0.0, n as f64 * 10.0, 0.0);
        assert!(d.stretches.is_empty());
    }

    #[test]
    fn two_second_rates_skip_breaks_and_masks_and_add_up_block_by_block() {
        // 28,800 bph running 10 s/day fast, for 20 s.
        let period = 0.125 * (1.0 - 10.0 / 86_400.0);
        let beats: Vec<crate::beats::Beat> = (0..160)
            .map(|k| crate::beats::Beat {
                index: k,
                time: 0.01 + k as f64 * period,
                quality: 1.0,
            })
            .collect();
        let mut all = Vec::new();
        let next = rates_2s(&beats, 28_800, 0.0, 0, &[5.0], &[(13.0, 14.5)], &mut all);
        assert_eq!(next, 9, "the last stretch the beats reach the end of");
        // Ten stretches less the one across the break at 5 s, the two the
        // mask reaches into, and the one the beats don't reach the end of.
        let starts: Vec<f64> = all.iter().map(|v| v.0).collect();
        assert_eq!(starts, [0.0, 2.0, 6.0, 8.0, 10.0, 16.0]);
        for (_, r) in &all {
            assert!((r - 10.0).abs() < 0.5, "{r}");
        }
        // The same values worked out as the beats arrive.
        let mut part = Vec::new();
        let mut at = 0;
        for n in [40, 90, 160] {
            at = rates_2s(
                &beats[..n],
                28_800,
                0.0,
                at,
                &[5.0],
                &[(13.0, 14.5)],
                &mut part,
            );
        }
        assert_eq!(part, all);
    }

    #[test]
    fn the_app_line_adds_its_words_to_the_summary() {
        let v: Vec<f64> = (0..50).map(|i| 250.0 + (i % 5) as f64).collect();
        let d = distribution(&v, AMPLITUDE, "2 s", 2.0).unwrap();
        assert!(
            d.summary.starts_with("50 2-second stretches · median "),
            "{}",
            d.summary
        );
        let line = d.line(AMPLITUDE, " in the session", Some("Dial Up"));
        assert!(
            line.starts_with("50 2-second stretches in the session · median ")
                && line.contains(" · Dial Up only"),
            "{line}"
        );
        assert_eq!(d.line(AMPLITUDE, "", None), d.summary);
    }
}
