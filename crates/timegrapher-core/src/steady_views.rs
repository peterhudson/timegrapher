//! What the app's Steadiness pane draws behind each headline, as data: the
//! readings the tests were made on, each series' period search, and each
//! series folded at a cycle. The Autocorrelation, Allan Deviation and CUSUM
//! views come with the statistics themselves ([`crate::steadiness`]); these
//! are the Readings, Periods and Cycle views, so a chart outside the app can
//! draw the same thing.
//!
//! The grids, searches and folds are worked out as the app's pane works
//! them out; only the output is thinned, so a day's take stays small.

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

/// The readings the tests were made on, in time order: 10 s rate readings,
/// and 2 s windows for the amplitude and the beat error from the unlock.
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

/// The period search: how strongly the readings repeat at each period.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchView {
    /// Periods searched, seconds, longest first, and the score at each
    /// (−log10 of the chance from noise).
    pub period_s: Vec<f64>,
    pub score: Vec<f64>,
    /// The score a period needs to count as a cycle.
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
    let wins = timing::windows(&log.beats, log.bph, rate_s, rate_s);
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
        let bins = ((period / g.step).round() as usize).clamp(6, 40);
        Some((longterm::fold(g, period, bins), from))
    })
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
    let grids = grids(log, rate_s);
    let searches = searches(&grids, long, lc);
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
                    let keep = spread(s.period_s.len().min(s.score.len()), MAX_PERIODS);
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
