//! The isochronism walk (#741): the take's rate against its amplitude, one
//! point per stretch of time, in time order, so a long run-down shows how
//! the watch walks through rate and amplitude as the mainspring lets down.
//! It sits beside the isochronism curve (rate binned by amplitude), not in
//! its place: a short take's wheel cycle moves rate and amplitude together
//! every few minutes, which the curve keeps and a time bin averages away.
//!
//! Core reads the points on a fine base of [`WALK_BIN_S`]; each surface
//! merges them to its own width with [`merged`], [`default_merge`] giving
//! 100 to 250 points a take. Bins of an hour or more smear steps and pull
//! the extremes in.

use crate::confidence::{too_few_windows, Coverage};
use crate::dsp::median_of;
use crate::longterm::Grid;
use serde::Serialize;

/// The walk's base bin, seconds.
pub const WALK_BIN_S: f64 = 300.0;
/// The most points [`default_merge`] leaves a take.
pub const WALK_MAX_POINTS: usize = 250;

/// One stretch of the take: the median of its rate readings and the
/// median of its amplitude's 2 s windows.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct WalkPoint {
    /// The stretch's middle, seconds from the take's start, and hours
    /// since full wind, `None` when the wind was not given (#1665).
    pub t_s: f64,
    pub wind_h: Option<f64>,
    pub rate_s_per_day: f64,
    pub amplitude_deg: f64,
    /// The rate readings and the amplitude windows each median read
    /// (`readings` kept as the rate's, its name since #1080).
    pub readings: usize,
    pub amp_windows: usize,
    /// The rate readings and amplitude windows the stretch could have
    /// held: the denominators of its coverage.
    pub rate_slots: usize,
    pub amp_slots: usize,
    /// Whether either series was read from under
    /// [`crate::confidence::MIN_WINDOW_COVERAGE`] of its slots
    /// ([`Coverage::is_thin`]); a merged point is thin when any of its
    /// bins is (Norm, 11-12).
    pub thin: bool,
}

impl WalkPoint {
    /// The rate's and the amplitude's coverage.
    pub fn coverage(&self) -> [Coverage; 2] {
        [
            Coverage {
                windows_used: self.readings,
                windows_in_span: self.rate_slots,
            },
            Coverage {
                windows_used: self.amp_windows,
                windows_in_span: self.amp_slots,
            },
        ]
    }
}

/// The walk on its base bins, in time order. Each series follows the
/// floors every window figure does (math-standards.md section 7, Norm
/// 11-12): read from under [`crate::confidence::MIN_WINDOW_SHARE`] of its
/// slots it is withheld, and under
/// [`crate::confidence::MIN_WINDOW_COVERAGE`] it stands, thin. A bin
/// needs both series to place a point: one with either withheld is left
/// out, never filled.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IsochronismWalk {
    pub bin_s: f64,
    pub points: Vec<WalkPoint>,
    /// Where the rate is still read after the last point but the
    /// amplitude is not (a run-down's end): the end of the last such bin,
    /// seconds from the take's start. The walk has no x for those rates,
    /// so it says so rather than draw them (Norm, Hugh, 11-12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_only_to_s: Option<f64>,
}

/// One base bin of the walk: a point, a rate with no amplitude to place
/// it at (ending at the given second), or nothing.
enum Bin {
    Point(WalkPoint),
    RateOnly(f64),
    Empty,
}

/// The walk from the rate's readings (the steadiness report's) and the
/// amplitude's 2 s windows, with their masks and culls (one-sided windows
/// left out), on the same true time, with `wind_h` the hours since full
/// wind at the take's start. Each series is read on its own: pairing a
/// reading's rate with its amplitude needs every window of the reading,
/// and with 82% of windows read that left 37% of readings (#741). `None`
/// with fewer than two bins.
pub fn walk(rate: &Grid, amp: &Grid, wind_h: Option<f64>) -> Option<IsochronismWalk> {
    let n = rate.y.len();
    let per_bin = (WALK_BIN_S / rate.step).round().max(1.0) as usize;
    let bin_s = per_bin as f64 * rate.step;
    let per_bin_windows = (bin_s / amp.step).round().max(1.0) as usize;
    let mut windows: Vec<Vec<f64>> = vec![Vec::new(); n.div_ceil(per_bin)];
    for (i, &v) in amp.y.iter().enumerate() {
        let b = ((amp.time(i) - rate.t0) / bin_s).floor();
        let in_take = amp.time(i) < rate.t0 + n as f64 * rate.step;
        if v.is_finite() && b >= 0.0 && in_take {
            if let Some(w) = windows.get_mut(b as usize) {
                w.push(v);
            }
        }
    }
    let bins: Vec<Bin> = (0..n)
        .step_by(per_bin)
        .zip(&windows)
        .map(|(i0, a)| {
            let end = (i0 + per_bin).min(n);
            let r: Vec<f64> = rate.y[i0..end]
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .collect();
            // A take's last bin can end early: its slots are its own.
            let rate_slots = end - i0;
            let amp_slots = if rate_slots == per_bin {
                per_bin_windows
            } else {
                (rate_slots as f64 * rate.step / amp.step).round().max(1.0) as usize
            };
            if too_few_windows(r.len(), rate_slots) {
                return Bin::Empty;
            }
            if too_few_windows(a.len(), amp_slots) {
                return Bin::RateOnly(rate.t0 + end as f64 * rate.step);
            }
            // The middle of what the bin holds.
            let t_s = rate.t0 + (i0 + end) as f64 / 2.0 * rate.step;
            let (Some(rate_s_per_day), Some(amplitude_deg)) =
                (median_of(r.iter().copied()), median_of(a.iter().copied()))
            else {
                return Bin::Empty;
            };
            let mut p = WalkPoint {
                t_s,
                wind_h: crate::steadiness::hours_since_wind(wind_h, t_s / 3600.0),
                rate_s_per_day,
                amplitude_deg,
                readings: r.len(),
                amp_windows: a.len(),
                rate_slots,
                amp_slots,
                thin: false,
            };
            p.thin = p.coverage().iter().any(|c| c.is_thin());
            Bin::Point(p)
        })
        .collect();
    let last = bins.iter().rposition(|b| matches!(b, Bin::Point(_)))?;
    let rate_only_to_s = bins[last..].iter().rev().find_map(|b| match b {
        Bin::RateOnly(end) => Some(*end),
        _ => None,
    });
    let points: Vec<WalkPoint> = bins
        .into_iter()
        .filter_map(|b| match b {
            Bin::Point(p) => Some(p),
            _ => None,
        })
        .collect();
    (points.len() >= 2).then_some(IsochronismWalk {
        bin_s,
        points,
        rate_only_to_s,
    })
}

/// How many base bins to merge so the walk has at most
/// [`WALK_MAX_POINTS`] points.
pub fn default_merge(w: &IsochronismWalk) -> usize {
    w.points.len().div_ceil(WALK_MAX_POINTS).max(1)
}

/// The walk with every `k` base bins (by time, so a left-out bin leaves
/// its merged bin thinner) merged into one: each series the mean of its
/// bins' medians weighted by its own count (the rate and the time by rate
/// readings, the amplitude by windows).
pub fn merged(w: &IsochronismWalk, k: usize) -> IsochronismWalk {
    let k = k.max(1);
    let width = w.bin_s * k as f64;
    let mut out: Vec<WalkPoint> = Vec::new();
    let mut group: Vec<&WalkPoint> = Vec::new();
    let slot = |p: &WalkPoint| (p.t_s / width).floor() as i64;
    for p in &w.points {
        if group.last().is_some_and(|q| slot(q) != slot(p)) {
            out.push(merge(&group));
            group.clear();
        }
        group.push(p);
    }
    if !group.is_empty() {
        out.push(merge(&group));
    }
    IsochronismWalk {
        bin_s: width,
        points: out,
        rate_only_to_s: w.rate_only_to_s,
    }
}

fn merge(g: &[&WalkPoint]) -> WalkPoint {
    if let [p] = g {
        return (*p).clone();
    }
    let mean = |f: fn(&WalkPoint) -> f64, n: fn(&WalkPoint) -> usize| {
        let total: usize = g.iter().map(|p| n(p)).sum();
        g.iter().map(|p| f(p) * n(p) as f64).sum::<f64>() / total.max(1) as f64
    };
    let rate_n = |p: &WalkPoint| p.readings;
    let amp_n = |p: &WalkPoint| p.amp_windows;
    WalkPoint {
        t_s: mean(|p| p.t_s, rate_n),
        // Hours since full wind run with the take's time, so the mean's
        // is the first's moved on to the mean time.
        wind_h: g[0]
            .wind_h
            .map(|w| w + (mean(|p| p.t_s, rate_n) - g[0].t_s) / 3600.0),
        rate_s_per_day: mean(|p| p.rate_s_per_day, rate_n),
        amplitude_deg: mean(|p| p.amplitude_deg, amp_n),
        readings: g.iter().map(|p| p.readings).sum(),
        amp_windows: g.iter().map(|p| p.amp_windows).sum(),
        rate_slots: g.iter().map(|p| p.rate_slots).sum(),
        amp_slots: g.iter().map(|p| p.amp_slots).sum(),
        thin: g.iter().any(|p| p.thin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A run-down over `hours`: the amplitude falls 300° to 200° and the
    /// rate rises 2 s/d per 10° lost, as rate readings every 10 s and
    /// amplitude windows every 2 s.
    fn run_down(hours: f64) -> (Grid, Grid) {
        let span = hours * 3600.0;
        let amp_at = |t: f64| 300.0 - 100.0 * t / span;
        let grid = |step: f64, f: &dyn Fn(f64) -> f64| Grid {
            t0: 0.0,
            step,
            y: (0..(span / step) as usize)
                .map(|i| f((i as f64 + 0.5) * step))
                .collect(),
        };
        (
            grid(10.0, &|t| 2.0 * (300.0 - amp_at(t)) / 10.0),
            grid(2.0, &amp_at),
        )
    }

    #[test]
    fn a_run_down_walks_down_the_amplitude_in_time_order() {
        let (rate, amp) = run_down(10.0);
        let w = walk(&rate, &amp, Some(1.0)).unwrap();
        assert_eq!(w.bin_s, 300.0);
        assert_eq!(w.points.len(), 120);
        let p = &w.points[0];
        assert_eq!((p.t_s, p.readings, p.amp_windows), (150.0, 30, 150));
        assert!((p.wind_h.unwrap() - (1.0 + 150.0 / 3600.0)).abs() < 1e-12);
        assert!((p.amplitude_deg - (300.0 - 100.0 * 150.0 / 36000.0)).abs() < 1e-9);
        assert!((p.rate_s_per_day - 20.0 * 150.0 / 36000.0).abs() < 1e-9);
        assert!(w.points.windows(2).all(|q| q[1].t_s > q[0].t_s
            && q[1].amplitude_deg < q[0].amplitude_deg
            && q[1].rate_s_per_day > q[0].rate_s_per_day));
    }

    /// Keep the first `keep` of `per` values from bin `bin` of `y`.
    fn thin_bin(y: &mut [f64], per: usize, bin: usize, keep: usize) {
        y[bin * per + keep..(bin + 1) * per].fill(f64::NAN);
    }

    /// Each series on the floors every window figure takes (Norm, 11-12):
    /// at 5% of its slots withheld, so the bin draws nothing; at 30% kept
    /// and thin; at 70% plain. The amplitude in bins 2 to 4, the rate in
    /// bins 5 to 7.
    #[test]
    fn a_bin_is_withheld_under_a_tenth_and_thin_under_three_fifths_in_either_series() {
        let (mut rate, mut amp) = run_down(1.0);
        for (bin, share) in [(2, 0.05), (3, 0.3), (4, 0.7)] {
            thin_bin(&mut amp.y, 150, bin, (150.0 * share) as usize);
        }
        for (bin, share) in [(5, 0.05), (6, 0.3), (7, 0.7)] {
            thin_bin(&mut rate.y, 30, bin, (30.0_f64 * share).round() as usize);
        }
        let w = walk(&rate, &amp, Some(0.0)).unwrap();
        let got: Vec<(f64, usize, usize, bool)> = w
            .points
            .iter()
            .map(|p| (p.t_s, p.readings, p.amp_windows, p.thin))
            .collect();
        assert_eq!(
            got,
            [
                (150.0, 30, 150, false),
                (450.0, 30, 150, false),
                (1050.0, 30, 45, true),
                (1350.0, 30, 105, false),
                (1950.0, 9, 150, true),
                (2250.0, 21, 150, false),
                (2550.0, 30, 150, false),
                (2850.0, 30, 150, false),
                (3150.0, 30, 150, false),
                (3450.0, 30, 150, false),
            ]
        );
        assert!(w
            .points
            .iter()
            .all(|p| (p.rate_slots, p.amp_slots) == (30, 150)));
    }

    /// The floor is the share's own: 3 of a bin's 30 rate readings (10%)
    /// stand, 2 do not; 15 of its 150 amplitude windows stand, 14 do not.
    #[test]
    fn a_tenth_of_the_slots_stands_and_one_fewer_does_not() {
        let (mut rate, mut amp) = run_down(1.0);
        thin_bin(&mut rate.y, 30, 2, 3);
        thin_bin(&mut rate.y, 30, 3, 2);
        thin_bin(&mut amp.y, 150, 4, 15);
        thin_bin(&mut amp.y, 150, 5, 14);
        let ts: Vec<f64> = walk(&rate, &amp, Some(0.0))
            .unwrap()
            .points
            .iter()
            .map(|p| p.t_s)
            .collect();
        assert!(ts.contains(&750.0) && ts.contains(&1350.0));
        assert!(!ts.contains(&1050.0) && !ts.contains(&1650.0));
    }

    /// Where the rate goes on past the last point with no amplitude read,
    /// the walk keeps the end of the last such bin; a bin with neither
    /// read, or one before the last point, does not count.
    #[test]
    fn the_rate_read_past_the_last_point_with_no_amplitude_is_kept_as_its_end() {
        let (mut rate, mut amp) = run_down(1.0);
        // Bin 3's amplitude is withheld mid-take; bins 8 to 10 keep their
        // rate with no amplitude; bin 11 has neither.
        thin_bin(&mut amp.y, 150, 3, 0);
        for bin in 8..12 {
            thin_bin(&mut amp.y, 150, bin, 5);
        }
        thin_bin(&mut rate.y, 30, 11, 1);
        let w = walk(&rate, &amp, Some(0.0)).unwrap();
        assert_eq!(w.points.last().unwrap().t_s, 2250.0);
        assert_eq!(w.rate_only_to_s, Some(3300.0));
        assert_eq!(merged(&w, 4).rate_only_to_s, Some(3300.0));
        // A bin whose amplitude is withheld before the last point only is
        // not past it.
        let (rate, mut amp) = run_down(1.0);
        thin_bin(&mut amp.y, 150, 3, 0);
        assert_eq!(walk(&rate, &amp, Some(0.0)).unwrap().rate_only_to_s, None);
        // Read to the end, there is nothing past it.
        let (rate, amp) = run_down(1.0);
        assert_eq!(walk(&rate, &amp, Some(0.0)).unwrap().rate_only_to_s, None);
    }

    /// A merged point is thin when any of its bins is, and its weights
    /// stay each series' own counts.
    #[test]
    fn a_merged_point_is_thin_when_any_of_its_bins_is() {
        let (rate, mut amp) = run_down(1.0);
        thin_bin(&mut amp.y, 150, 1, 45);
        let m = merged(&walk(&rate, &amp, Some(0.0)).unwrap(), 2);
        assert!(m.points[0].thin);
        assert!(!m.points[1].thin);
        assert_eq!(
            (
                m.points[0].amp_windows,
                m.points[0].amp_slots,
                m.points[0].rate_slots
            ),
            (195, 300, 60)
        );
    }

    /// With 82% of the windows read, as on dd3235_DU_30min, nearly every
    /// reading misses one of its five windows, but every bin keeps well
    /// over half its windows: the walk keeps every bin.
    #[test]
    fn a_take_with_most_windows_read_keeps_every_bin() {
        let (rate, mut amp) = run_down(1.0);
        for (i, v) in amp.y.iter_mut().enumerate() {
            if (i * 37) % 100 < 18 {
                *v = f64::NAN;
            }
        }
        let w = walk(&rate, &amp, Some(0.0)).unwrap();
        assert_eq!(w.points.len(), 12);
        assert!(w
            .points
            .iter()
            .all(|p| p.amp_windows >= 120 && p.amp_windows <= 125));
    }

    #[test]
    fn a_last_bin_cut_short_sits_at_its_own_middle() {
        let (mut rate, mut amp) = run_down(1.0);
        rate.y.extend(vec![0.0; 20]);
        amp.y.extend(vec![200.0; 100]);
        let w = walk(&rate, &amp, Some(0.0)).unwrap();
        let last = w.points.last().unwrap();
        assert_eq!(
            (w.points.len(), last.t_s, last.readings, last.amp_windows),
            (13, 3700.0, 20, 100)
        );
        assert_eq!(w.points[11].t_s, 3450.0);
        assert_eq!(
            (last.rate_slots, last.amp_slots, last.thin),
            (20, 100, false)
        );
    }

    /// A short last bin is judged on its own slots, not a full bin's: 14
    /// of its 20 readings and 70 of its 100 windows (70%) are plain,
    /// where against a full bin's 30 and 150 (47%) they would be thin.
    #[test]
    fn a_last_bin_cut_short_is_judged_on_its_own_slots() {
        let (mut rate, mut amp) = run_down(1.0);
        rate.y
            .extend((0..20).map(|i| if i < 14 { 0.0 } else { f64::NAN }));
        amp.y
            .extend((0..100).map(|i| if i < 70 { 200.0 } else { f64::NAN }));
        let last = walk(&rate, &amp, Some(0.0)).unwrap().points.pop().unwrap();
        assert_eq!(
            (
                last.t_s,
                last.readings,
                last.rate_slots,
                last.amp_windows,
                last.amp_slots
            ),
            (3700.0, 14, 20, 70, 100)
        );
        assert!(!last.thin);
    }

    #[test]
    fn under_two_bins_is_no_walk() {
        let (rate, amp) = run_down(5.0 / 60.0);
        assert!(walk(&rate, &amp, Some(0.0)).is_none());
    }

    #[test]
    fn merging_keeps_a_long_take_to_250_points_each_series_by_its_own_count() {
        let (rate, amp) = run_down(65.0);
        let w = walk(&rate, &amp, Some(0.0)).unwrap();
        assert_eq!(w.points.len(), 780);
        assert_eq!(default_merge(&w), 4);
        let m = merged(&w, default_merge(&w));
        assert_eq!((m.bin_s, m.points.len()), (1200.0, 195));
        assert_eq!((m.points[0].readings, m.points[0].amp_windows), (120, 600));
        assert!((m.points[0].t_s - 600.0).abs() < 1e-9);
        let a: f64 = w.points[..4].iter().map(|p| p.amplitude_deg).sum::<f64>() / 4.0;
        assert!((m.points[0].amplitude_deg - a).abs() < 1e-9);
        // A thin bin weighs less, in its own series only.
        let mut thin = w.clone();
        thin.points[0].readings = 15;
        thin.points[1].amp_windows = 75;
        let (p0, p1) = (&thin.points[0], &thin.points[1]);
        let m = merged(&thin, 2);
        let rate = (15.0 * p0.rate_s_per_day + 30.0 * p1.rate_s_per_day) / 45.0;
        let amp = (150.0 * p0.amplitude_deg + 75.0 * p1.amplitude_deg) / 225.0;
        assert!((m.points[0].rate_s_per_day - rate).abs() < 1e-12);
        assert!((m.points[0].amplitude_deg - amp).abs() < 1e-12);
        assert_eq!(merged(&w, 1).points, w.points);
        let (rate, amp) = run_down(10.0);
        assert_eq!(default_merge(&walk(&rate, &amp, Some(0.0)).unwrap()), 1);
    }
}
