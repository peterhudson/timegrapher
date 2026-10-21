//! What the Steadiness pane draws behind each headline, as data: the
//! readings the tests were made on, each series' period search, and each
//! series folded at a cycle. The Autocorrelation, Allan Deviation and CUSUM
//! views come with the statistics themselves ([`crate::steadiness`]); these
//! are the Readings, Periods and Cycle views, so a chart outside the app can
//! draw the same thing.
//!
//! The grids are the ones `steadiness::check` builds, from its own
//! functions (on true time when the take has pauses or a clock fit). On the
//! app's own takes, with no clock fit and only zero-length breaks, the app's
//! pane builds the same grids. The searches and folds are the app's, and
//! only the output is thinned, so a day's take stays small.

use crate::longrun::LongReport;
use crate::longterm::{self, Grid, LongConfig, Search};
use crate::steadiness::Report;
use crate::stream::BeatLog;
use crate::timing;
use serde::Serialize;

/// Most readings drawn per series, as on the app's Readings view.
pub const MAX_READINGS: usize = 3000;
/// Most periods kept from a period search.
pub const MAX_PERIODS: usize = 600;
/// Most folded cycles kept per series; the rest are counted.
pub const MAX_CYCLES: usize = 200;

/// The views behind one series' headline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesViews {
    /// "rate", "amplitude" or "beat_error".
    pub series: &'static str,
    pub readings: Readings,
    pub search: Option<SearchView>,
    pub fold: Option<FoldView>,
}

/// The readings the tests were made on, in time order on the tests' time
/// (the clock's, with each pause's gap): 10 s rate readings, and 2 s
/// windows for the amplitude and the beat error from the unlock.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Readings {
    /// Length of each reading, seconds.
    pub step_s: f64,
    /// Readings in the take, and how many were skipped between those kept
    /// (1 keeps all).
    pub n: usize,
    pub stride: usize,
    /// Middle of each kept reading, seconds, and its value; only readings
    /// with a value.
    pub t_s: Vec<f64>,
    pub values: Vec<f64>,
}

/// The period search: how strongly the series repeats at each period.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchView {
    /// Periods searched, seconds, longest first, and the score at each
    /// (−log10 of the chance from noise).
    pub period_s: Vec<f64>,
    pub score: Vec<f64>,
    /// The score a period needs for the search to list it; whether the
    /// readings hold a cycle there is the tests' call, in the headline.
    pub threshold: f64,
}

/// The readings folded at one period: each cycle's readings by phase, and
/// their median shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FoldView {
    pub period_s: f64,
    /// "own" (the series' own cycle), "rate", "amplitude" or "beat_error"
    /// (another series' cycle), or "wheel" (a wheel's turn, `wheel`).
    pub from: &'static str,
    pub wheel: Option<String>,
    /// Median of the detrended readings in each phase bin; NaN as null.
    pub profile: Vec<f64>,
    /// Cycles folded, and up to [`MAX_CYCLES`] of them, evenly spread, one
    /// row per cycle and one value per phase bin (null where empty).
    pub cycles: usize,
    pub raster: Vec<Vec<f64>>,
}

/// The views for each series, in the report's order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Views {
    pub series: Vec<SeriesViews>,
    /// The periods every series' search covers between them, log10 seconds,
    /// so the Periods plots share one axis.
    pub period_span: Option<[f64; 2]>,
}

const NAMES: [&str; 3] = ["rate", "amplitude", "beat_error"];

/// The readings the tests are made on: `rate_s` rate readings, and the 2 s
/// amplitude windows for the amplitude and the beat error from the unlock.
pub fn grids(log: &BeatLog, rate_s: f64) -> [Grid; 3] {
    grids_with_breaks(log, rate_s, &[])
}

/// The same grids with explicit zero-gap fit boundaries on the log's timeline,
/// using the report's existing cut convention. This does not map a recording
/// clock or insert elapsed pauses. Callers supply the boundaries used for their
/// analysis; the legacy entry point does not infer them from log metadata.
pub fn grids_with_breaks(log: &BeatLog, rate_s: f64, breaks: &[crate::breaks::Break]) -> [Grid; 3] {
    // Match the report's existing explicit-break endpoint convention,
    // including its small offset before a boundary; do not redefine it here.
    let cuts = crate::longrun::cuts(None, breaks);
    let mut wins = timing::windows(&log.beats, log.bph, rate_s, rate_s);
    wins.retain(|w| !cuts.iter().any(|&c| w.start_s < c && c < w.end_s));
    let rate = match (wins.first(), wins.last()) {
        (Some(w0), Some(last)) => {
            let n = ((last.start_s - w0.start_s) / rate_s).round() as usize + 1;
            let mut y = vec![f64::NAN; n];
            for w in &wins {
                let i = ((w.start_s - w0.start_s) / rate_s).round() as usize;
                if i < n {
                    y[i] = w.fit.rate_s_per_day;
                }
            }
            Grid {
                t0: w0.start_s,
                step: rate_s,
                y,
            }
        }
        _ => Grid {
            t0: 0.0,
            step: rate_s,
            y: Vec::new(),
        },
    };
    let aw = &log.amplitude_windows;
    let step = aw.first().map_or(2.0, |w| w.end_s - w.start_s);
    let grid_of = |f: &dyn Fn(&crate::amplitude::AmplitudeWindow) -> Option<f64>| {
        let (Some(w0), Some(last)) = (aw.first(), aw.last()) else {
            return Grid {
                t0: 0.0,
                step,
                y: Vec::new(),
            };
        };
        let n = ((last.start_s - w0.start_s) / step).round() as usize + 1;
        let mut y = vec![f64::NAN; n];
        for w in aw {
            let i = ((w.start_s - w0.start_s) / step).round() as usize;
            if i < n {
                y[i] = f(w).unwrap_or(f64::NAN);
            }
        }
        Grid {
            t0: w0.start_s,
            step,
            y,
        }
    };
    [
        rate,
        grid_of(&|w| w.mean()),
        grid_of(&|w| w.beat_error_unlock_ms),
    ]
}

/// The period search behind each series: the long-run analysis's for the
/// rate and the amplitude, and one over the beat error's readings when it
/// has at least 30.
pub fn searches(grids: &[Grid; 3], long: &LongReport, lc: &LongConfig) -> [Option<Search>; 3] {
    let readings = |g: &Grid| g.y.iter().filter(|v| v.is_finite()).count();
    let be_search = (readings(&grids[2]) >= 30).then(|| {
        let mut g = grids[2].clone();
        g.t0 = 0.0;
        longterm::analyse(&g, lc).search
    });
    [
        Some(long.timing.search.clone()),
        Some(long.amplitude.search.clone()),
        be_search,
    ]
}

/// The three series' readings as `steadiness::check` lays them out: on
/// true time (the clock's mapping, and each pause's gap when the take has
/// pauses), each gap left empty, and no rate reading that touches a gap or
/// lacks beats in any of its 2 s stretches.
/// Built with the tests' own functions, so the views draw, search and fold
/// the readings the tests were made on.
fn check_grids(log: &BeatLog, long: &LongReport, rate_s: f64) -> [Grid; 3] {
    use crate::steadiness::{rate_grid, rate_windows, window_grids};
    let clock = long.clock.as_ref();
    let rate = rate_grid(&rate_windows(log, clock, &long.breaks, rate_s), rate_s);
    let [amplitude, beat_error] = window_grids(log, clock, &long.breaks);
    [rate, amplitude, beat_error]
}

/// Where a series' fold period came from.
#[derive(Debug, Clone, PartialEq)]
pub enum FoldFrom {
    /// Its own cycle.
    Own,
    /// The cycle another series has (0 rate, 1 amplitude, 2 beat error).
    Series(usize),
    /// A wheel's turn, when no series has a cycle.
    Wheel(String),
}

/// Each series folded at the cycle it was found to have, else at another
/// series' cycle or the longest wheel's turn that fits, so its Cycle view
/// still shows whether that repeats in it.
pub fn folds(
    grids: &[Grid; 3],
    report: &Report,
    lc: &LongConfig,
) -> [Option<(longterm::Fold, FoldFrom)>; 3] {
    let own = |i: usize| {
        let s = report.series.get(i)?;
        s.cycle
            .as_ref()
            .map(|c| c.period_s)
            .or(s.period.as_ref().map(|p| p.period_s))
    };
    let fits = |g: &Grid, period: f64| period >= 3.0 * g.step && g.span() >= 2.0 * period;
    std::array::from_fn(|i| {
        let g = &grids[i];
        let (period, from) = own(i)
            .filter(|&p| fits(g, p))
            .map(|p| (p, FoldFrom::Own))
            .or_else(|| {
                (0..3).filter(|&j| j != i).find_map(|j| {
                    own(j)
                        .filter(|&p| fits(g, p))
                        .map(|p| (p, FoldFrom::Series(j)))
                })
            })
            .or_else(|| {
                lc.wheels
                    .iter()
                    .filter(|w| fits(g, w.period_s))
                    .max_by(|a, b| a.period_s.total_cmp(&b.period_s))
                    .map(|w| (w.period_s, FoldFrom::Wheel(w.name.clone())))
            })?;
        Some((longterm::fold(g, period, phase_bins(period, g.step)), from))
    })
}

/// Phase bins for the Cycle view: one per phase the readings fall on, so
/// the drawn cycle keeps its size (at a period of exactly a half whole
/// number of readings, rounding can still split a phase between two
/// neighbouring bins). A 45 s
/// cycle read every 10 s comes back to the same phase every 90 s, after 9
/// readings at 9 phases. A repeat is taken only within two turns: phases
/// that repeat later are too many for each bin to hold enough readings, so
/// the cycle is cut at one bin per reading, 6 to 40.
fn phase_bins(period: f64, step: f64) -> usize {
    let r = period / step;
    (1..=2)
        .map(|m| m as f64 * r)
        .filter(|x| x.round() <= 40.0)
        .find(|x| (x - x.round()).abs() < 0.05)
        .map_or((r.round() as usize).clamp(6, 40), |x| x.round() as usize)
}

/// The periods the searches cover between them, log10 seconds.
pub fn period_span(searches: &[Option<Search>; 3]) -> Option<(f64, f64)> {
    searches
        .iter()
        .flatten()
        .flat_map(|x| x.period_s.iter())
        .filter(|p| **p > 0.0)
        .map(|p| p.log10())
        .fold(None, |r: Option<(f64, f64)>, x| {
            Some(r.map_or((x, x), |(a, b)| (a.min(x), b.max(x))))
        })
        .filter(|(a, b)| a < b)
}

/// Indices of a period search's curve thinned to at most `max`, first
/// and last kept: the axis is cut into equal slices of log period, as the
/// FFT's thinning cuts it, and each slice keeps its lowest and highest
/// score, in order. A narrow peak keeps its top unless a taller one shares
/// its slice (on a 12 h search each slice spans about 3% of period).
fn spread_extremes(period_s: &[f64], score: &[f64], max: usize) -> Vec<usize> {
    let n = period_s.len().min(score.len());
    if n <= max || max < 4 {
        return spread(n, max);
    }
    let slices = (max - 2) / 2;
    let (a, b) = (period_s[0].ln(), period_s[n - 1].ln());
    let mut lows: Vec<Option<usize>> = vec![None; slices];
    let mut highs: Vec<Option<usize>> = vec![None; slices];
    for i in 1..n - 1 {
        let f = if a == b {
            0.0
        } else {
            (period_s[i].ln() - a) / (b - a)
        };
        if !f.is_finite() {
            continue;
        }
        let k = ((f * slices as f64) as usize).min(slices - 1);
        if lows[k].is_none_or(|l| score[i] < score[l]) {
            lows[k] = Some(i);
        }
        if highs[k].is_none_or(|h| score[i] > score[h]) {
            highs[k] = Some(i);
        }
    }
    let mut keep: Vec<usize> = std::iter::once(0)
        .chain(lows.into_iter().flatten())
        .chain(highs.into_iter().flatten())
        .chain(std::iter::once(n - 1))
        .collect();
    keep.sort_unstable();
    keep.dedup();
    keep
}

/// Indices of `n` items thinned evenly to at most `max`, first and last kept.
fn spread(n: usize, max: usize) -> Vec<usize> {
    if n <= max {
        return (0..n).collect();
    }
    let last = (n - 1) as f64;
    (0..max)
        .map(|k| (k as f64 * last / (max - 1) as f64).round() as usize)
        .collect()
}

/// The views behind each headline in `report`, from the take `log` it was
/// made on and its long-run analysis `long` under `lc`.
pub fn views(
    log: &BeatLog,
    report: &Report,
    long: &LongReport,
    lc: &LongConfig,
    rate_s: f64,
) -> Views {
    let grids = check_grids(log, long, rate_s);
    // The beat error's search on the grid the tests searched, pauses and
    // all, so its strongest line is the one the label names.
    let readings = grids[2].y.iter().filter(|v| v.is_finite()).count();
    let searches = [
        Some(long.timing.search.clone()),
        Some(long.amplitude.search.clone()),
        (readings >= 30).then(|| {
            let mut g = grids[2].clone();
            g.t0 = 0.0;
            longterm::analyse(&g, lc).search
        }),
    ];
    let folds = folds(&grids, report, lc);
    let series = (0..3)
        .map(|i| {
            let g = &grids[i];
            let n = g.y.len();
            let stride = n.div_ceil(MAX_READINGS).max(1);
            let (t_s, values) = (0..n)
                .step_by(stride)
                .filter(|&j| g.y[j].is_finite())
                .map(|j| (g.time(j), g.y[j]))
                .unzip();
            let search = searches[i]
                .as_ref()
                .filter(|s| !s.period_s.is_empty())
                .map(|s| {
                    let keep = spread_extremes(&s.period_s, &s.score, MAX_PERIODS);
                    SearchView {
                        period_s: keep.iter().map(|&k| s.period_s[k]).collect(),
                        score: keep.iter().map(|&k| s.score[k]).collect(),
                        threshold: s.threshold,
                    }
                });
            let fold = folds[i].as_ref().map(|(f, from)| {
                let (from, wheel) = match from {
                    FoldFrom::Own => ("own", None),
                    FoldFrom::Series(j) => (NAMES[*j], None),
                    FoldFrom::Wheel(w) => ("wheel", Some(w.clone())),
                };
                FoldView {
                    period_s: f.period_s,
                    from,
                    wheel,
                    profile: f.profile.clone(),
                    cycles: f.raster.len(),
                    raster: spread(f.raster.len(), MAX_CYCLES)
                        .into_iter()
                        .map(|k| f.raster[k].clone())
                        .collect(),
                }
            });
            SeriesViews {
                series: NAMES[i],
                readings: Readings {
                    step_s: g.step,
                    n,
                    stride,
                    t_s,
                    values,
                },
                search,
                fold,
            }
        })
        .collect();
    Views {
        series,
        period_span: period_span(&searches).map(|(a, b)| [a, b]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{LiveAnalyzer, LiveConfig};
    use crate::longrun;
    use crate::periodicity::default_wheels;
    use crate::steadiness::{self, Config};
    use crate::synth::{generate, SynthConfig};

    #[test]
    fn derived_rate_grid_uses_the_reports_known_cut_convention() {
        let mut log = BeatLog {
            sample_rate: 48000,
            duration_s: 40.0,
            bph: 28800,
            lift_deg: 52.0,
            beats: (0..320)
                .map(|index| crate::beats::Beat {
                    time: index as f64 * 0.125,
                    index,
                    quality: 1.0,
                })
                .collect(),
            amplitude_windows: vec![],
            clipped_beats: vec![],
            clipped_samples: 0,
            jumps: vec![15.0, 30.0],
            sided: Vec::new(),
            by_count: Vec::new(),
        };
        let legacy = grids(&log, 10.0)[0].clone();
        let breaks: Vec<_> = log
            .jumps
            .iter()
            .map(|&audio_s| crate::breaks::Break {
                audio_s,
                gap_s: 0.0,
            })
            .collect();
        let baseline = grids_with_breaks(&log, 10.0, &breaks)[0].clone();
        let cuts = crate::longrun::cuts(None, &breaks);
        let expected: Vec<_> = timing::windows(&log.beats, log.bph, 10.0, 10.0)
            .into_iter()
            .filter(|w| !cuts.iter().any(|&c| w.start_s < c && c < w.end_s))
            .collect();
        assert_eq!(expected.len(), 1);
        assert_eq!(
            baseline.y.len(),
            1,
            "both the interior and exact-end cut use the existing report convention"
        );
        assert_eq!(baseline.t0, expected[0].start_s);
        assert_eq!(baseline.y[0], expected[0].fit.rate_s_per_day);
        log.jumps.clear();
        let uncut = grids(&log, 10.0);
        assert_eq!(uncut[0].y.len(), 3);
        assert_eq!(uncut[0].t0, legacy.t0);
        assert_eq!(uncut[0].y, legacy.y, "legacy grids still ignores log jumps");
        assert_eq!(uncut[0].y[0], baseline.y[0], "unaffected reading unchanged");
        log.jumps = vec![15.0];
        let middle = grids_with_breaks(&log, 10.0, &breaks[..1]);
        assert_eq!(middle[0].t0, uncut[0].t0);
        assert_eq!(middle[0].y.len(), 3, "time grid is not compacted");
        assert_eq!(middle[0].y[0], uncut[0].y[0]);
        assert!(middle[0].y[1].is_nan());
        assert_eq!(middle[0].y[2], uncut[0].y[2]);
    }

    #[test]
    fn the_cycle_is_folded_one_bin_per_reading_phase() {
        assert_eq!(phase_bins(45.0, 10.0), 9);
        assert_eq!(phase_bins(60.0, 10.0), 6);
        assert_eq!(phase_bins(30.0, 10.0), 3);
        assert_eq!(phase_bins(600.0, 2.0), 40);
        assert_eq!(phase_bins(42.7, 10.0), 6);
        assert_eq!(phase_bins(41.4, 10.0), 6);
        assert_eq!(phase_bins(58.0, 10.0), 6);
        // A 45 s cycle read every 10 s keeps its size: its readings' peak
        // to peak, at 9 phases 40 degrees apart. 45.003 s keeps each phase
        // off a bin edge, where rounding would split it across two bins.
        let g = Grid {
            t0: 0.0,
            step: 10.0,
            y: (0..360)
                .map(|i| (std::f64::consts::TAU * (i as f64 * 10.0 + 2.5) / 45.003).sin())
                .collect(),
        };
        let f = longterm::fold(&g, 45.003, phase_bins(45.003, 10.0));
        let max = f.profile.iter().copied().fold(f64::MIN, f64::max);
        let min = f.profile.iter().copied().fold(f64::MAX, f64::min);
        let want = 2.0 * (80f64.to_radians()).sin();
        assert!(((max - min) / want - 1.0).abs() < 0.01, "{}", max - min);
    }

    #[test]
    fn thinning_keeps_a_narrow_peak_anywhere_on_the_log_axis() {
        // A 12 h search's periods, longest first, with narrow lines at
        // about 2 h and 1 h, where an even split of the indices puts both
        // in one wide stretch.
        let n = 87_363;
        let period_s: Vec<f64> = (0..n).map(|i| 262_144.0 / (19 + i) as f64).collect();
        let mut score = vec![0.5; n];
        score[17] = 9.0;
        score[54] = 6.0;
        // And one among the short periods, where a slice holds thousands
        // of points.
        score[80_000] = 7.0;
        let keep = spread_extremes(&period_s, &score, MAX_PERIODS);
        assert!(keep.len() <= MAX_PERIODS, "{}", keep.len());
        assert!(keep.windows(2).all(|w| w[0] < w[1]));
        assert_eq!((keep[0], *keep.last().unwrap()), (0, n - 1));
        assert!(
            keep.contains(&17) && keep.contains(&54) && keep.contains(&80_000),
            "{keep:?}"
        );
        // A short curve is kept whole.
        assert_eq!(
            spread_extremes(&period_s[..10], &score[..10], MAX_PERIODS),
            (0..10).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_beat_errors_search_runs_on_the_tests_grid_with_its_pauses() {
        let cfg = SynthConfig {
            duration_s: 120.0,
            beat_error_ms: 0.8,
            beat_error_drift_ms_per_s: 0.01,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 270.0, |_| 0.0);
        let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
        for b in audio.samples.chunks(4800) {
            live.push(b);
        }
        let log = BeatLog {
            sample_rate: live.sample_rate(),
            duration_s: live.duration_s(),
            bph: live.bph().expect("beat rate"),
            lift_deg: live.config().analysis.amplitude.lift_deg,
            beats: live.beats().to_vec(),
            amplitude_windows: live.amplitude_windows().to_vec(),
            clipped_beats: live.clipped_beats().to_vec(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
        };
        let lc = LongConfig {
            wheels: default_wheels(log.bph),
            ..Default::default()
        };
        let aw = &log.amplitude_windows;
        let step = aw[0].end_s - aw[0].start_s;
        // A 6 s pause half way through, inside one window.
        let mid = aw[aw.len() / 2].start_s - 0.5 * step;
        let breaks = [crate::breaks::Break {
            audio_s: mid,
            gap_s: 3.0 * step,
        }];
        let plain = longrun::analyse(&log, None, &lc);
        let paused = longrun::analyse_with_breaks(&log, None, &breaks, &lc);
        let g0 = check_grids(&log, &plain, 10.0)[2].clone();
        let g1 = check_grids(&log, &paused, 10.0)[2].clone();
        assert_eq!(g1.y.len(), g0.y.len() + 3, "the pause is left empty");
        let k = aw.iter().position(|w| w.start_s > mid).unwrap();
        assert!(g1.y[k..k + 3].iter().all(|v| v.is_nan()));
        let same = |a: f64, b: f64| a == b || (a.is_nan() && b.is_nan());
        assert!(same(g1.y[k + 3], g0.y[k]));
        assert!(same(g1.y[k - 1], g0.y[k - 1]));

        // The tests' own beat-error spectrum comes from this grid, and not
        // from the readings' grid without the pause.
        let check = Config::default();
        let report = steadiness::check(&log, None, &paused, &lc, &check);
        let tests = report
            .spectra
            .iter()
            .find(|s| s.series == "beat_error")
            .expect("beat error spectrum");
        let tests = serde_json::to_value(tests).unwrap();
        let spectrum_of = |mut g: Grid| {
            g.t0 = 0.0;
            let d = longterm::analyse(&g, &lc).detrended;
            let s =
                crate::spectrum::amplitude_spectrum(&d, "beat_error", "ms", 1.0, check.max_points);
            serde_json::to_value(s.expect("spectrum")).unwrap()
        };
        assert_eq!(spectrum_of(g1.clone()), tests);
        assert_ne!(spectrum_of(grids(&log, 10.0)[2].clone()), tests);

        // The views draw and fold these readings, on the tests' time: the
        // readings after the pause sit its gap later than the audio puts
        // them, as the tests' grid has them.
        let v = views(&log, &report, &paused, &lc, 10.0);
        let be = &v.series[2].readings;
        let want: Vec<f64> = (0..g1.y.len())
            .filter(|&j| g1.y[j].is_finite())
            .map(|j| g1.time(j))
            .collect();
        assert_eq!(be.t_s, want);
        let at = |t: f64| be.t_s.iter().any(|&x| (x - t).abs() < 1e-9);
        assert!(at(g0.time(k) + 3.0 * step) && !at(g0.time(k)));
    }

    #[test]
    fn the_rate_readings_are_the_tests_readings_across_a_pause() {
        let cfg = SynthConfig {
            duration_s: 300.0,
            ..Default::default()
        };
        let audio = generate(&cfg, |_| 270.0, |_| 0.0);
        let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
        for b in audio.samples.chunks(4800) {
            live.push(b);
        }
        let mut log = BeatLog {
            sample_rate: live.sample_rate(),
            duration_s: live.duration_s(),
            bph: live.bph().expect("beat rate"),
            lift_deg: live.config().analysis.amplitude.lift_deg,
            beats: live.beats().to_vec(),
            amplitude_windows: live.amplitude_windows().to_vec(),
            clipped_beats: live.clipped_beats().to_vec(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
        };
        let lc = LongConfig {
            wheels: default_wheels(log.bph),
            ..Default::default()
        };
        let plain = longrun::analyse(&log, None, &lc);
        // A 30 s pause 3 s into a rate reading, half way through.
        let g = &check_grids(&log, &plain, 10.0)[0];
        let mid = g.t0 + 15.0 * g.step + 3.0;
        // Beats lost for 3 s, 3 s into reading 5, with no break listed: the
        // tests leave that reading out, so the views must too.
        let lost = g.t0 + 5.0 * g.step + 3.0;
        log.beats.retain(|b| !(lost..lost + 3.0).contains(&b.time));
        let breaks = [crate::breaks::Break {
            audio_s: mid,
            gap_s: 30.0,
        }];
        let paused = longrun::analyse_with_breaks(&log, None, &breaks, &lc);
        let report = steadiness::check(&log, None, &paused, &lc, &Config::default());
        let tests = report
            .distributions
            .overall
            .iter()
            .find(|d| d.series == "rate" && d.values == "readings")
            .expect("rate readings");
        let v = views(&log, &report, &paused, &lc, 10.0);
        let r = &v.series[0].readings;
        assert_eq!(r.stride, 1);
        assert_eq!(r.t_s.len(), tests.n);
        let mut sorted = r.values.clone();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        let median = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            0.5 * (sorted[n / 2 - 1] + sorted[n / 2])
        };
        assert!(
            (median - tests.median).abs() < 1e-9,
            "{median} {}",
            tests.median
        );
        // Cutting only at the pause's middle, as the tests did before,
        // keeps readings that reach into the pause from either side, so
        // this test bites.
        let map = longrun::true_time(None, &paused.breaks);
        let cut = longrun::cuts(None, &paused.breaks);
        let beats: Vec<_> = log
            .beats
            .iter()
            .map(|b| crate::beats::Beat {
                time: map(b.time),
                ..*b
            })
            .collect();
        // Without the 2 s check, reading 5 would be kept.
        let gaps = longrun::gaps(None, &paused.breaks);
        let outside = timing::windows(&beats, log.bph, 10.0, 10.0)
            .iter()
            .filter(|w| !longrun::touches_gap(&gaps, w.start_s, w.end_s))
            .count();
        assert_eq!(outside, tests.n + 1);
        let mut wins = timing::windows(&beats, log.bph, 10.0, 10.0);
        wins.retain(|w| !cut.iter().any(|&c| w.start_s < c && c < w.end_s));
        assert!(wins.len() > tests.n, "{} {}", wins.len(), tests.n);
    }

    /// The app's own test take: ten minutes of a watch whose rate swings
    /// 20 s/d once a minute (a fourth wheel fault), heard as the app hears
    /// it. Pins what each view carries, so the app can move its pane onto
    /// this module and show it draws the same thing.
    #[test]
    fn a_once_a_minute_swing_gives_every_view() {
        let cfg = SynthConfig {
            duration_s: 600.0,
            rate_s_per_day: 5.0,
            ..Default::default()
        };
        let audio = generate(
            &cfg,
            |_| 270.0,
            |t| 20.0 * (std::f64::consts::TAU * t / 60.0).sin(),
        );
        let mut live = LiveAnalyzer::new(48000, LiveConfig::default());
        for b in audio.samples.chunks(4800) {
            live.push(b);
        }
        let log = BeatLog {
            sample_rate: live.sample_rate(),
            duration_s: live.duration_s(),
            bph: live.bph().expect("beat rate"),
            lift_deg: live.config().analysis.amplitude.lift_deg,
            beats: live.beats().to_vec(),
            amplitude_windows: live.amplitude_windows().to_vec(),
            clipped_beats: live.clipped_beats().to_vec(),
            clipped_samples: 0,
            jumps: Vec::new(),
            sided: Vec::new(),
            by_count: Vec::new(),
        };
        let lc = LongConfig {
            wheels: default_wheels(log.bph),
            ..Default::default()
        };
        let check = Config::default();
        let long = longrun::analyse(&log, None, &lc);
        let report = steadiness::check(&log, None, &long, &lc, &check);
        assert!(
            report.series[0].period.is_some(),
            "{}",
            report.series[0].headline
        );
        let v = views(&log, &report, &long, &lc, check.rate_reading_s);
        let names: Vec<&str> = v.series.iter().map(|s| s.series).collect();
        assert_eq!(names, ["rate", "amplitude", "beat_error"]);

        // Readings: 10 s rate readings and 2 s windows, none thinned.
        let r = &v.series[0].readings;
        assert_eq!((r.step_s, r.stride), (10.0, 1));
        assert!((55..=60).contains(&r.t_s.len()), "{}", r.t_s.len());
        assert_eq!(r.t_s.len(), r.values.len());
        assert!(r.t_s.windows(2).all(|w| w[0] < w[1]));
        let a = &v.series[1].readings;
        assert_eq!(a.step_s, 2.0);
        assert!((280..=300).contains(&a.n), "{}", a.n);
        // The swing shows: the rate readings span most of its 40 s/d.
        let lo = r.values.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = r.values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(hi - lo > 20.0, "{lo} to {hi}");

        // Periods: the rate's search peaks near a minute, over its threshold.
        let s = v.series[0].search.as_ref().expect("rate search");
        assert_eq!(s.period_s.len(), s.score.len());
        let best = (0..s.score.len())
            .max_by(|&i, &j| s.score[i].total_cmp(&s.score[j]))
            .unwrap();
        assert!(
            (s.period_s[best] - 60.0).abs() < 3.0,
            "{}",
            s.period_s[best]
        );
        assert!(s.score[best] > s.threshold);
        let [a0, b0] = v.period_span.expect("span");
        assert!(a0 < 60f64.log10() && 60f64.log10() < b0);

        // Cycle: the rate folds at its own minute; the steady amplitude at
        // another series' cycle or a wheel's turn, as in the app.
        let f = v.series[0].fold.as_ref().expect("rate fold");
        assert_eq!(f.from, "own");
        assert!((f.period_s - 60.0).abs() < 3.0, "{}", f.period_s);
        assert_eq!(f.profile.len(), 6);
        assert_eq!(f.raster.len(), f.cycles);
        assert!(f.raster.iter().all(|row| row.len() == f.profile.len()));
        let fa = v.series[1].fold.as_ref().expect("amplitude fold");
        assert_ne!(fa.from, "own");
    }

    #[test]
    fn thinning_keeps_both_ends() {
        assert_eq!(spread(5, 10), vec![0, 1, 2, 3, 4]);
        let k = spread(10_000, 200);
        assert_eq!(k.len(), 200);
        assert_eq!(k[0], 0);
        assert_eq!(*k.last().unwrap(), 9_999);
        assert!(k.windows(2).all(|w| w[0] < w[1]));
    }
}
