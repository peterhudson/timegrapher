//! Tests of [`super`]: the series checks, kept beside it to keep the
//! file under the length cap.

use super::*;

#[test]
fn independent_readings_with_most_missing_read_as_no_cycle() {
    // 200 grids of 1200 independent readings 10 s apart with 90% of
    // them missing (a fixed random mask): with the correlation at each
    // lag read from 4 to 14 pairs as if from all 120 readings, the
    // Ljung–Box p fell under 0.01 in 199 and "repeats every" led in
    // 148. Every reading present, it is 1 in 200.
    for sparse in [false, true] {
        let mut rng = crate::synth::Rng::new(20261026);
        let mut mask = crate::synth::Rng::new(991991);
        let (mut low_p, mut repeats, mut correlated) = (0, 0, 0);
        for _ in 0..200 {
            let y: Vec<f64> = (0..1200)
                .map(|_| {
                    let v = rng.normal();
                    if sparse && mask.normal() < 1.28 {
                        f64::NAN
                    } else {
                        v
                    }
                })
                .collect();
            let out = check_series(
                SeriesKind::Rate,
                Grid {
                    t0: 0.0,
                    step: 10.0,
                    y,
                },
                None,
                None,
                None,
                &[],
                &Config::default(),
            );
            low_p += usize::from(out.autocorrelation.as_ref().unwrap().p_value < 0.01);
            repeats += usize::from(out.summary.starts_with("repeats"));
            correlated += usize::from(out.summary.starts_with("successive"));
        }
        assert!(low_p <= 6, "sparse {sparse}: p < 0.01 in {low_p} of 200");
        assert!(
            repeats + correlated <= 2,
            "sparse {sparse}: {repeats} repeat and {correlated} correlated headlines in 200"
        );
    }
}

#[test]
fn a_cycle_of_a_non_whole_number_of_readings_reads_its_full_size() {
    // A 45 s sine, 1.0 peak to peak, in 10 s readings (4.5 a turn),
    // whatever its phase: a fold of five whole bins reads 0.81 to 0.93.
    for k in 0..8 {
        let phase = k as f64 * std::f64::consts::PI / 8.0;
        let d = Grid {
            t0: 0.0,
            step: 10.0,
            y: (0..720)
                .map(|i| {
                    let t = i as f64 * 10.0;
                    0.5 * (std::f64::consts::TAU * t / 45.0 + phase).sin()
                })
                .collect(),
        };
        let c = cycle_at(&d, 45.0, CycleSource::PeriodSearch, &[]);
        assert!(
            (c.peak_to_peak - 1.0).abs() < 0.03,
            "{k}: {}",
            c.peak_to_peak
        );
        assert!(c.explained > 0.95, "{k}: {}", c.explained);
    }
    // A whole number of readings a turn is folded as before.
    let d = Grid {
        t0: 0.0,
        step: 10.0,
        y: (0..720)
            .map(|i| 0.5 * (std::f64::consts::TAU * i as f64 / 6.0).sin())
            .collect(),
    };
    assert_eq!(sized(&d, 60.0), fold(&d, 60.0));
}

#[test]
fn the_band_widens_where_gaps_leave_fewer_pairs() {
    // Readings every 10 s with every other block of 50 missing: at a
    // lag of 50 readings no pair is left; at 60, ten in each of the
    // first 19 blocks.
    let mut rng = crate::synth::Rng::new(1);
    let y: Vec<f64> = (0..2000)
        .map(|i| {
            if (i / 50) % 2 == 1 {
                f64::NAN
            } else {
                rng.normal()
            }
        })
        .collect();
    let cfg = Config::default();
    let a = autocorrelation(&y, 10.0, &cfg);
    assert_eq!(a.band_by_lag.len(), a.lag_s.len());
    let at = |lag: f64| {
        let k = a.lag_s.iter().position(|&l| l == lag).unwrap();
        a.band_by_lag[k]
    };
    // Lag 0: every reading pairs with itself, as for `band`.
    assert!((at(0.0) - a.band).abs() < 1e-12);
    assert!(at(500.0).is_nan());
    assert!(
        (at(600.0) - 1.96 / 190.0f64.sqrt()).abs() < 1e-9,
        "{}",
        at(600.0)
    );
    assert!(at(600.0) > 2.0 * a.band);
}

/// Deterministic noise, roughly Gaussian (sum of uniforms).
struct Noise(u64);
impl Noise {
    fn next(&mut self) -> f64 {
        let mut s = 0.0;
        for _ in 0..12 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            s += (self.0 >> 11) as f64 / (1u64 << 53) as f64;
        }
        s - 6.0
    }
}

fn grid(y: Vec<f64>, step: f64) -> Grid {
    Grid { t0: 0.0, step, y }
}

#[test]
fn tails() {
    assert!((chi2_tail(18.307, 10) - 0.05).abs() < 1e-3);
    assert!((chi2_tail(3.841, 1) - 0.05).abs() < 1e-3);
    assert!((kolmogorov_tail(1.358) - 0.05).abs() < 2e-3);
}

#[test]
fn white_noise_is_steady() {
    let mut z = Noise(1);
    let y: Vec<f64> = (0..2000).map(|_| 250.0 + 3.0 * z.next()).collect();
    let s = check_series(
        SeriesKind::Amplitude,
        grid(y, 2.0),
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(s.lead, Lead::Steady, "{}", s.headline);
    let a = s.allan.unwrap();
    assert!(a.max_excess < 1.5, "{}", a.max_excess);
    assert!(a.slope_short.unwrap() < -0.4);
}

#[test]
fn a_step_is_a_shifting_mean() {
    let mut z = Noise(2);
    let y: Vec<f64> = (0..1800)
        .map(|i| if i < 1000 { 5.0 } else { 9.0 } + 2.0 * z.next())
        .collect();
    let s = check_series(
        SeriesKind::Rate,
        grid(y, 2.0),
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(s.lead, Lead::ShiftingMean, "{}", s.headline);
    let ch = s.changes.unwrap();
    assert_eq!(ch.segments.len(), 2);
    assert!((ch.segments[1].start_s - 2000.0).abs() < 90.0);
    assert!((s.cusum.unwrap().at_s - 2000.0).abs() < 90.0);
}

/// Twelve hours of rate readings 10 s apart drifting from 0 to 6 s/day,
/// as a mainspring running down does, with independent scatter and
/// `cycle` s/day of a 60 s line on top.
fn drifting_rate(cycle: f64) -> SeriesCheck {
    let mut z = Noise(7);
    let y: Vec<f64> = (0..4320)
        .map(|i| {
            let t = i as f64 * 10.0;
            6.0 * t / 43_200.0 + cycle * (std::f64::consts::TAU * t / 60.0).sin() + z.next()
        })
        .collect();
    check_series(
        SeriesKind::Rate,
        grid(y, 10.0),
        None,
        None,
        None,
        &[],
        &Config::default(),
    )
}

/// A slow trend is not memory: the autocorrelation and Ljung–Box read
/// the readings with it taken out (#650).
#[test]
fn a_slow_trend_reads_as_no_memory() {
    let a = drifting_rate(0.0).autocorrelation.unwrap();
    assert!(a.p_value > 0.01, "Ljung–Box p {}", a.p_value);
    assert!(a.memory_s.is_some_and(|m| m <= 20.0), "{:?}", a.memory_s);
    assert!(a.repeat_lag_s.is_none(), "{:?}", a.repeat_lag_s);
}

/// Taking the trend out leaves a minute's cycle on it to be found.
#[test]
fn a_minute_cycle_on_a_slow_trend_is_still_found() {
    let a = drifting_rate(1.5).autocorrelation.unwrap();
    assert!(a.p_value < 1e-6, "Ljung–Box p {}", a.p_value);
    let lag = a.repeat_lag_s.expect("the cycle repeats");
    assert!((lag - 60.0).abs() <= 10.0, "{lag}");
}

fn search_line(period_s: f64, size: f64, size_unit: &'static str, wheel: Option<&str>) -> Period {
    Period {
        period_s,
        significance: 10.0,
        explained: 0.5,
        peak_to_peak: size,
        timing_peak_to_peak_ms: None,
        size,
        size_unit,
        wheel: wheel.map(str::to_string),
        other_periods_s: vec![],
        harmonic_of: None,
        other_harmonic_of: vec![],
    }
}

fn noise(seed: u64, n: usize, level: f64, sd: f64) -> Vec<f64> {
    let mut z = Noise(seed);
    (0..n).map(|_| level + sd * z.next()).collect()
}

#[test]
fn a_fast_rate_line_is_stated_as_the_beat_timing_in_ms() {
    let s = check_series(
        SeriesKind::Rate,
        grid(noise(11, 360, 5.0, 2.0), 10.0),
        None,
        Some(search_line(5.0, 0.0432, "ms", Some("escape wheel"))),
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(s.lead, Lead::Steady, "{}", s.headline);
    assert!(
        s.headline.ends_with("; the beat timing repeats every 5.0 s, the escape wheel's period, 0.04 ms peak to peak; 10 s readings are too coarse to show it."),
        "{}",
        s.headline
    );
}

#[test]
fn a_rate_line_of_30_s_or_more_is_stated_as_the_rate_in_s_per_day() {
    // `series --reading 30`: the fourth wheel turns in two readings.
    let s = check_series(
        SeriesKind::Rate,
        grid(noise(12, 400, 5.0, 2.0), 30.0),
        None,
        Some(search_line(60.0, 1.2, "s/d", Some("fourth wheel"))),
        None,
        &[],
        &Config::default(),
    );
    assert!(
        s.headline.contains("; the rate repeats every 60.0 s, the fourth wheel's period, 1.20 s/d peak to peak; 30 s readings are too coarse to show it."),
        "{}",
        s.headline
    );
    assert!(!s.headline.contains("beat timing"), "{}", s.headline);
}

#[test]
fn an_amplitude_line_is_judged_against_the_2_s_windows() {
    let fine = || grid(noise(13, 1800, 270.0, 3.0), 2.0);
    let readings = || grid(noise(14, 360, 270.0, 1.5), 10.0);
    let s = check_series(
        SeriesKind::Amplitude,
        readings(),
        Some(fine()),
        Some(search_line(4.0, 0.8, "deg", None)),
        None,
        &[],
        &Config::default(),
    );
    assert!(
        s.headline.ends_with("; the amplitude repeats every 4.0 s, 0.80° peak to peak; 2 s readings are too coarse to show it."),
        "{}",
        s.headline
    );
    // 20 s is under three 10 s readings but over three 2 s windows,
    // which can show it: no fast line.
    let s = check_series(
        SeriesKind::Amplitude,
        readings(),
        Some(fine()),
        Some(search_line(20.0, 0.8, "deg", None)),
        None,
        &[],
        &Config::default(),
    );
    assert!(s.faster_than_readings().is_none(), "{}", s.headline);
    assert!(!s.headline.contains("too coarse"), "{}", s.headline);
    // The search's line is named, and the lead says nothing of cycles.
    assert!(
        s.headline.ends_with(
            "; no level change or trend detected. The amplitude's period search also finds 20.0 s (0.80° peak to peak)."
        ),
        "{}",
        s.headline
    );
}

#[test]
fn a_fast_line_beside_a_level_change_is_its_own_sentence() {
    let mut z = Noise(2);
    let y: Vec<f64> = (0..1800)
        .map(|i| if i < 1000 { 5.0 } else { 9.0 } + 2.0 * z.next())
        .collect();
    let s = check_series(
        SeriesKind::Rate,
        grid(y, 2.0),
        None,
        Some(search_line(4.0, 0.0432, "ms", Some("escape wheel"))),
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(s.lead, Lead::ShiftingMean, "{}", s.headline);
    assert!(
        s.headline.ends_with(". The beat timing repeats every 4.0 s, the escape wheel's period, 0.04 ms peak to peak; 2 s readings are too coarse to show it."),
        "{}",
        s.headline
    );
}

#[test]
fn a_fast_lines_alias_is_no_cycle_at_any_number_of_its_turns() {
    // One to seven hours of 10 s rate readings carrying the 30 s alias
    // of a 3.75 s escape-wheel line, a fifth to a half of their
    // variance: the autocorrelation offers every 30 s lag, and 330 s
    // or 390 s (11 and 13 turns of the alias) do not reduce to it.
    let tau = 2.0 * std::f64::consts::PI;
    let runs = [360, 720, 1440, 2520].into_iter().flat_map(|n| {
        [(0.7, 21), (1.0, 22), (1.4, 23)].map(|(amp, seed)| (n, amp, seed + n as u64))
    });
    for (n, amp, seed) in runs {
        let mut z = Noise(seed);
        let y: Vec<f64> = (0..n)
            .map(|i| 5.0 + amp * (tau * i as f64 / 3.0 + 0.4).sin() + z.next())
            .collect();
        let s = check_series(
            SeriesKind::Rate,
            grid(y, 10.0),
            None,
            Some(search_line(3.75, 0.3, "ms", Some("escape wheel"))),
            None,
            &[],
            &Config::default(),
        );
        assert!(s.cycle.is_none(), "seed {seed}: {}", s.headline);
        assert!(
            s.faster_than_readings().is_some(),
            "seed {seed}: {}",
            s.headline
        );
    }
}

/// Fast lines too quick for 10 s readings and the alias each shows at.
const FAST_LINES: [(f64, f64); 3] = [(3.75, 30.0), (1.0 / 0.15, 20.0), (60.0 / 11.0, 60.0)];

/// One to seven hours of 10 s rate readings: noise, the alias of a fast
/// line at `alias_amp`, and slow sines at `slow` (period, amplitude).
fn aliased_readings(
    n: usize,
    seed: u64,
    alias: f64,
    alias_amp: f64,
    slow: &[(f64, f64)],
) -> Vec<f64> {
    let tau = 2.0 * std::f64::consts::PI;
    let mut z = Noise(seed);
    (0..n)
        .map(|i| {
            let t = 10.0 * i as f64;
            // A phase near a quarter turn keeps the 20 s alias, two
            // readings a turn, at nearly its full size.
            5.0 + alias_amp * (tau * t / alias + 1.4).sin()
                + slow
                    .iter()
                    .map(|&(p, a)| a * (tau * t / p + 1.1).sin())
                    .sum::<f64>()
                + z.next()
        })
        .collect()
}

fn rate_check(y: Vec<f64>, fast: f64, listed: &[f64]) -> SeriesCheck {
    let mut line = search_line(fast, 0.3, "ms", Some("escape wheel"));
    line.other_periods_s = listed.to_vec();
    check_series(
        SeriesKind::Rate,
        grid(y, 10.0),
        None,
        Some(line),
        None,
        &[],
        &Config::default(),
    )
}

const HOURS: [usize; 4] = [360, 720, 1440, 2520];

/// Whether a found cycle is `p`, within the take's resolution.
fn near(c: &Cycle, p: f64, n: usize) -> bool {
    (c.period_s - p).abs() <= (p * p / (10.0 * n as f64)).max(0.01 * p)
}

#[test]
fn a_weak_fourth_wheel_beside_a_fast_line_is_sized_on_its_own() {
    // The docs' watch: a 3.75 s escape-wheel line (a 30 s alias in 10 s
    // readings) and a sinusoidal 60 s fourth wheel under a third its
    // size, both lines the period search lists. Folding at 60 s, two
    // turns of the alias, holds the alias too; net of it, the wheel's
    // own swing is 0.6 s/d and too small a share to be the cycle, so
    // the headline names it from the period search.
    for n in HOURS {
        for seed in 0..4 {
            let y = aliased_readings(n, seed + n as u64, 30.0, 1.0, &[(60.0, 0.3)]);
            let s = rate_check(y, 3.75, &[60.0]);
            match &s.cycle {
                Some(c) => assert!(
                    near(c, 60.0, n) && c.peak_to_peak < 1.2,
                    "{n} readings, seed {seed}: {}",
                    s.headline
                ),
                None => assert!(
                    s.headline.contains("period search also finds 60 s"),
                    "{n} readings, seed {seed}: {}",
                    s.headline
                ),
            }
        }
    }
}

#[test]
fn a_listed_line_on_alias_turns_with_nothing_of_its_own_is_no_cycle() {
    // The period search lists 60 s, two turns of the 30 s alias, but
    // the readings hold only the alias: net of it, nothing is left.
    for n in HOURS {
        for seed in 0..4 {
            let y = aliased_readings(n, seed + 5 * n as u64, 30.0, 1.0, &[]);
            let s = rate_check(y, 3.75, &[60.0]);
            assert!(
                s.cycle.is_none(),
                "{n} readings, seed {seed}: {}",
                s.headline
            );
            assert!(
                s.headline
                    .ends_with(" The rate's period search also finds 60 s."),
                "{}",
                s.headline
            );
        }
    }
}

#[test]
fn a_cycle_on_alias_turns_is_sized_net_of_the_alias() {
    // A 60 s fourth wheel of 2 s/d peak to peak beside a 3.75 s line's
    // 30 s alias of 2 s/d: the fold at 60 s holds both, about 3 s/d;
    // net of the alias it is the wheel's own, about 2 s/d.
    for n in HOURS {
        for seed in 0..4 {
            let y = aliased_readings(n, seed + 11 * n as u64, 30.0, 1.0, &[(60.0, 1.0)]);
            let s = rate_check(y, 3.75, &[60.0]);
            let c = s.cycle.as_ref().unwrap();
            assert!(
                near(c, 60.0, n),
                "{n} readings, seed {seed}: {}",
                s.headline
            );
            assert!(
                (1.4..2.6).contains(&c.peak_to_peak),
                "{n} readings, seed {seed}: {} ({:.2})",
                s.headline,
                c.peak_to_peak
            );
        }
    }
}

#[test]
fn a_search_line_the_headline_does_not_name_is_added() {
    // A 30 min amplitude swing leads; the period search also lists a
    // 45 s line the readings' cycle does not name.
    let tau = 2.0 * std::f64::consts::PI;
    let mut z = Noise(41);
    let y: Vec<f64> = (0..3600)
        .map(|i| 270.0 + 6.0 * (tau * 2.0 * i as f64 / 1800.0).sin() + z.next())
        .collect();
    let mut line = search_line(45.0, 0.8, "deg", None);
    line.other_periods_s = vec![1800.0, 600.0];
    let s = check_series(
        SeriesKind::Amplitude,
        grid(y, 2.0),
        None,
        Some(line),
        None,
        &[],
        &Config::default(),
    );
    assert!(
        s.headline.ends_with(
            " The amplitude's period search also finds 45 s (0.80° peak to peak) and 600 s."
        ),
        "{}",
        s.headline
    );
}

#[test]
fn a_real_slow_line_beside_an_alias_is_the_only_cycle_found() {
    // A 450 s line (a third wheel) beside the 30 s, 20 s and 60 s
    // aliases. Listed by the period search, it is the cycle, or, when
    // net of the alias it is too small a share of the readings, the
    // headline names it from the search; unlisted, it may be lost when
    // it does not clearly outfold the alias. No other cycle is ever
    // listed in its place.
    for n in HOURS {
        for seed in 0..4 {
            for (q, a) in FAST_LINES {
                let y = aliased_readings(n, seed + 7 * n as u64, a, 1.0, &[(450.0, 0.7)]);
                let s = rate_check(y.clone(), q, &[450.0]);
                assert!(
                    match &s.cycle {
                        Some(c) => near(c, 450.0, n),
                        None => s.headline.contains("period search also finds 450 s"),
                    },
                    "{a} s alias, listed, {n} readings, seed {seed}: {}",
                    s.headline
                );
                let s = rate_check(y, q, &[]);
                assert!(
                    s.cycle.as_ref().is_none_or(|c| near(c, 450.0, n)),
                    "{a} s alias, unlisted, {n} readings, seed {seed}: {}",
                    s.headline
                );
            }
        }
    }
}

#[test]
fn a_pure_alias_is_no_cycle_at_whole_half_or_third_turns() {
    // The 30 s, 20 s and 60 s aliases alone, at a fifth to two thirds
    // of the readings' variance, over one to seven hours: the
    // autocorrelation offers whole, half and third turns of each.
    for n in HOURS {
        for seed in 0..4 {
            for (q, a) in FAST_LINES {
                for amp in [0.7, 1.0, 1.4] {
                    let y = aliased_readings(n, seed + 3 * n as u64, a, amp, &[]);
                    let s = rate_check(y, q, &[]);
                    assert!(
                        s.cycle.is_none(),
                        "{a} s alias, {n} readings, seed {seed}: {}",
                        s.headline
                    );
                }
            }
        }
    }
}

#[test]
fn a_half_turn_of_an_alias_counts_only_where_it_holds_the_line() {
    let line = |q| Some(search_line(q, 0.3, "ms", None));
    // 75 s is 2.5 turns of the 30 s alias of 3.75 s, and 20 of its own.
    let p = line(3.75);
    let a = alias_of_fast_line(75.0, 10.0, 7200.0, p.as_ref()).unwrap();
    assert!((a - 30.0).abs() < 1e-9);
    // 30 s is 1.5 turns of the 20 s alias of 6.67 s, and 4.5 of its own.
    let p = line(1.0 / 0.15);
    let a = alias_of_fast_line(40.0, 10.0, 7200.0, p.as_ref()).unwrap();
    assert!((a - 20.0).abs() < 1e-9);
    assert_eq!(alias_of_fast_line(30.0, 10.0, 7200.0, p.as_ref()), None);
}

#[test]
fn a_summary_names_each_lead_in_a_few_words() {
    let check = |kind, y: Vec<f64>, step, two: Option<TwoState>| {
        check_series(
            kind,
            grid(y, step),
            None,
            None,
            two,
            &[],
            &Config::default(),
        )
    };
    let mut z = Noise(1);
    let steady = check(
        SeriesKind::Amplitude,
        (0..2000).map(|_| 250.0 + 3.0 * z.next()).collect(),
        2.0,
        None,
    );
    let mut z = Noise(2);
    let step = check(
        SeriesKind::Rate,
        (0..1800)
            .map(|i| if i < 1000 { 5.0 } else { 9.0 } + 2.0 * z.next())
            .collect(),
        2.0,
        None,
    );
    let mut z = Noise(3);
    let slide = check(
        SeriesKind::Amplitude,
        (0..3600)
            .map(|i| 280.0 - 20.0 * i as f64 / 3600.0 + 2.0 * z.next())
            .collect(),
        2.0,
        None,
    );
    let mut z = Noise(4);
    let mut ar = 0.0;
    let wander = check(
        SeriesKind::Rate,
        (0..3000)
            .map(|_| {
                ar = 0.95 * ar + 0.3 * z.next();
                ar + z.next()
            })
            .collect(),
        10.0,
        None,
    );
    let mut z = Noise(5);
    let tau = std::f64::consts::TAU;
    let cycle = check(
        SeriesKind::Rate,
        (0..720)
            .map(|i| 3.0 * (tau * (i as f64 + 0.5) * 10.0 / 46.0).sin() + z.next())
            .collect(),
        10.0,
        None,
    );
    // Irregular switching between 293° and 312° in 2 s windows.
    let dwell = [60.0, 90.0, 40.0, 130.0, 70.0, 50.0, 110.0, 80.0];
    let edges: Vec<f64> = dwell
        .iter()
        .cycle()
        .scan(0.0, |t, d| {
            *t += d;
            Some(*t)
        })
        .take(40)
        .collect();
    let mut z = Noise(6);
    let y: Vec<f64> = (0..900)
        .map(|i| {
            let t = i as f64 * 2.0;
            let k = edges.iter().filter(|&&e| e <= t).count();
            (if k % 2 == 0 { 312.0 } else { 293.0 }) + 3.0 * z.next()
        })
        .collect();
    let samples: Vec<crate::twostate::Sample> = y
        .iter()
        .enumerate()
        .map(|(i, &v)| crate::twostate::Sample {
            start_s: i as f64 * 2.0,
            end_s: i as f64 * 2.0 + 2.0,
            value: v,
            tick: Some(v + 1.0),
            tock: Some(v - 1.0),
            beat_error_unlock_ms: Some(0.2),
            beat_error_drop_ms: Some(0.2),
            tick_unlock_ms: Some(-7.0 + (v - 300.0) * 0.02),
            tock_unlock_ms: Some(-7.0 + (v - 300.0) * 0.02),
        })
        .collect();
    let two = crate::twostate::find(&samples, &crate::twostate::Params::default());
    let levels = check(SeriesKind::Amplitude, y.clone(), 2.0, Some(two));
    // The same switching carried by Tock alone: the measurement.
    let mut one_side = samples.clone();
    for x in &mut one_side {
        x.tick = Some(302.0);
        x.tock = Some(2.0 * x.value - 302.0);
    }
    let hop = crate::twostate::find(&one_side, &crate::twostate::Params::default());
    let measured = check(SeriesKind::Amplitude, y, 2.0, Some(hop));
    let short = check(SeriesKind::BeatError, vec![0.2; 20], 2.0, None);
    for (s, lead, summary) in [
        (&steady, Lead::Steady, "no cycle, level change or trend"),
        (
            &step,
            Lead::ShiftingMean,
            "shifts level once, +4.9 s/d to +9.1 s/d",
        ),
        (&slide, Lead::Drifting, "slides from 280° to 260°"),
        (
            &wander,
            Lead::Wandering,
            "successive readings correlated; no cycle, level change or trend",
        ),
        (
            &cycle,
            Lead::Periodic,
            "repeats every 46 s, 5.8 s/d peak to peak",
        ),
        (
            &levels,
            Lead::TwoStates,
            "switches between 293° and 312°, about every 78 s",
        ),
        (
            &measured,
            Lead::Measurement,
            "switches between 293° and 312°, about every 78 s, on the Tock side only",
        ),
        (&short, Lead::TooShort, "20 readings, too few to search"),
    ] {
        assert_eq!(s.lead, lead, "{}", s.headline);
        assert_eq!(s.summary, summary, "{}", s.headline);
    }
}

#[test]
fn a_slide_is_drifting() {
    let mut z = Noise(3);
    let y: Vec<f64> = (0..3600)
        .map(|i| 280.0 - 20.0 * i as f64 / 3600.0 + 2.0 * z.next())
        .collect();
    let s = check_series(
        SeriesKind::Amplitude,
        grid(y, 2.0),
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(s.lead, Lead::Drifting, "{}", s.headline);
    assert!((s.trend.unwrap().per_hour + 10.0).abs() < 1.0);
}

#[test]
fn a_random_walk_wanders() {
    let mut z = Noise(4);
    let mut ar = 0.0;
    let y: Vec<f64> = (0..3000)
        .map(|_| {
            // AR(1) with a 20-reading memory, no trend or steps.
            ar = 0.95 * ar + 0.3 * z.next();
            ar + z.next()
        })
        .collect();
    let s = check_series(
        SeriesKind::Rate,
        grid(y, 10.0),
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(s.lead, Lead::Wandering, "{}", s.headline);
}

#[test]
fn allan_of_white_noise_falls_as_root_tau() {
    let mut z = Noise(5);
    let y: Vec<f64> = (0..8192).map(|_| z.next()).collect();
    let a = allan(&y, 1.0);
    let (t, d, _) = a[4];
    assert!((d * t.sqrt() - a[0].1).abs() < 0.15 * a[0].1, "{a:?}");
}

#[test]
fn a_long_take_names_the_cycle_inside_a_slower_one() {
    // 16 h of 10 s readings: a 48.004 s cycle whose shape changes from
    // turn to turn and repeats every five turns. Over 16 h a period
    // off by 0.02 s smears the fold, so the period must be found
    // finely.
    let mut z = Noise(7);
    let tau = std::f64::consts::TAU;
    let y: Vec<f64> = (0..5700)
        .map(|i| {
            let t = (i as f64 + 0.5) * 10.0;
            3.0 * (tau * t / 48.004).sin() + 2.0 * (tau * t / 240.02).sin() + 2.0 * z.next()
        })
        .collect();
    let s = check_series(
        SeriesKind::Rate,
        grid(y, 10.0),
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(s.lead, Lead::Periodic, "{}", s.headline);
    let c = s.cycle.unwrap();
    assert!((c.period_s - 240.02).abs() < 0.1, "{c:?}");
    assert!((c.inner_period_s.unwrap() - 48.004).abs() < 0.02, "{c:?}");
    assert_eq!(c.turns, Some(5));
    assert!(
        s.headline.contains("five turns of a 48 s cycle"),
        "{}",
        s.headline
    );
    assert_eq!(
        s.summary,
        "repeats every 4 min, 5 turns of 48 s, 9.7 s/d peak to peak"
    );
}

#[test]
fn isochronism_curve_and_wind_down_follow_the_run_down() {
    // 26 h of 10 s readings: amplitude falls from 270° to 218°, and
    // the rate follows it at 0.5 s/d per degree.
    let mut z = Noise(11);
    let n = 26 * 360;
    let amp: Vec<f64> = (0..n)
        .map(|i| 270.0 - 52.0 * i as f64 / n as f64 + 2.0 * z.next())
        .collect();
    let rate: Vec<f64> = amp
        .iter()
        .map(|a| 0.5 * (a - 250.0) + 1.5 * z.next())
        .collect();
    let be = vec![0.3; n];
    let cfg = Config::default();
    let c = isochronism_curve(&grid(rate.clone(), 10.0), &grid(amp.clone(), 10.0), &cfg).unwrap();
    assert!((c.s_per_day_per_deg - 0.5).abs() < 0.05, "{}", c.headline);
    assert!(c.bins.len() >= 20);
    let w = wind_down(&grid(rate, 10.0), &grid(amp, 10.0), &grid(be, 10.0), &cfg).unwrap();
    assert!(w.at_24h, "{}", w.headline);
    // 24 h of 26 at 2° an hour: about −48° and −24 s/d.
    let da = w.amplitude_change_deg.unwrap();
    assert!((da + 48.0).abs() < 3.0, "{}", w.headline);
    assert!(
        (w.rate_change_s_per_day.unwrap() + 24.0).abs() < 2.0,
        "{}",
        w.headline
    );
    assert!(w.beat_error_change_ms.unwrap().abs() < 1e-9);
}

/// Two hours of 10 s rate readings, detrended: unit noise and sines
/// of (period, amplitude), each with its own phase.
fn sines(seed: u64, parts: &[(f64, f64)]) -> Grid {
    let tau = 2.0 * std::f64::consts::PI;
    let mut z = Noise(seed);
    let y: Vec<f64> = (0..720)
        .map(|i| {
            let t = 10.0 * (i as f64 + 0.5);
            5.0 + z.next()
                + parts
                    .iter()
                    .enumerate()
                    .map(|(j, &(p, a))| a * (tau * t / p + 0.3 + 0.9 * j as f64).sin())
                    .sum::<f64>()
        })
        .collect();
    longterm::detrend(&grid(y, 10.0), 1800.0)
}

/// `half_cycle` of the cycle found at 90 s, nine readings a turn.
fn from_90(d: &Grid, search: Option<&Period>) -> Option<Cycle> {
    from_at(d, 90.0, search)
}

fn from_at(d: &Grid, p: f64, search: Option<&Period>) -> Option<Cycle> {
    let c = cycle_at(d, refine(d, p, 0.01), CycleSource::Autocorrelation, &[]);
    half_cycle(&c, d, search, &[], &Config::default())
}

#[test]
fn a_cycle_found_at_twice_its_period_moves_to_its_own() {
    // 90 s is nine 10 s readings, so folding at 90 s and at 45 s puts
    // the readings in the same groups. A 45 s cycle, a sine or one with
    // a 22.5 s harmonic, moves to 45 s with the size of its two sines;
    // a 90 s sine, and an even mix of 90 s and 45 s, stay.
    for seed in 0..8 {
        for (parts, ptp) in [
            (vec![(45.0, 0.8)], 1.6),
            (vec![(45.0, 0.8), (22.5, 0.4)], 0.0),
        ] {
            let d = sines(100 + seed, &parts);
            let c = from_90(&d, None).unwrap_or_else(|| panic!("seed {seed}: {parts:?}"));
            assert!((c.period_s - 45.0).abs() < 0.5, "seed {seed}: {c:?}");
            let tau = 2.0 * std::f64::consts::PI;
            let curve: Vec<f64> = (0..450)
                .map(|i| {
                    let t = i as f64 / 10.0;
                    parts
                        .iter()
                        .enumerate()
                        .map(|(j, &(p, a))| a * (tau * t / p + 0.3 + 0.9 * j as f64).sin())
                        .sum::<f64>()
                })
                .collect();
            let truth = if ptp > 0.0 {
                ptp
            } else {
                curve.iter().copied().fold(f64::MIN, f64::max)
                    - curve.iter().copied().fold(f64::MAX, f64::min)
            };
            assert!(
                (c.peak_to_peak / truth - 1.0).abs() < 0.1,
                "seed {seed}: {:.2} against {truth:.2}",
                c.peak_to_peak
            );
        }
        for parts in [vec![(90.0, 0.8)], vec![(90.0, 0.8), (45.0, 0.8)]] {
            let d = sines(200 + seed, &parts);
            assert!(from_90(&d, None).is_none(), "seed {seed}: {parts:?}");
        }
    }
}

#[test]
fn a_listed_line_sharing_the_phases_is_left_out_of_them() {
    // A 45 s cycle beside a 30 s line, a third of 90 s: the 30 s line
    // shares the nine phases, so unlisted it keeps the cycle at 90 s;
    // listed by the period search, it is a line of its own.
    for seed in 0..8 {
        let d = sines(300 + seed, &[(45.0, 0.8), (30.0, 0.8)]);
        assert!(from_90(&d, None).is_none(), "seed {seed}");
        let mut line = search_line(5.0, 0.3, "ms", Some("escape wheel"));
        line.other_periods_s = vec![30.0];
        let c = from_90(&d, Some(&line)).unwrap_or_else(|| panic!("seed {seed}"));
        assert!((c.period_s - 45.0).abs() < 0.5, "seed {seed}: {c:?}");
    }
}

#[test]
fn at_seven_readings_a_turn_the_half_is_fitted_alone() {
    // 70 s is seven readings, and its quarter (17.5 s) cannot be told
    // from its third (23.33 s) in them. A 70 s cycle beside a listed
    // 23.33 s line stays at 70 s; a 35 s cycle beside it moves, sized
    // by its own sine.
    let mut line = search_line(5.0, 0.3, "ms", Some("escape wheel"));
    line.other_periods_s = vec![70.0 / 3.0];
    for seed in 0..16 {
        for size in [0.8, 1.2, 1.6] {
            let d = sines(800 + seed, &[(70.0, 0.8), (70.0 / 3.0, size)]);
            assert!(
                from_at(&d, 70.0, Some(&line)).is_none(),
                "seed {seed}, {size}"
            );
        }
        let d = sines(900 + seed, &[(35.0, 0.8), (70.0 / 3.0, 0.8)]);
        let c = from_at(&d, 70.0, Some(&line)).unwrap_or_else(|| panic!("seed {seed}"));
        assert!((c.period_s - 35.0).abs() < 0.5, "seed {seed}: {c:?}");
        assert!(
            (c.peak_to_peak / 1.6 - 1.0).abs() < 0.3,
            "seed {seed}: {c:?}"
        );
    }
}

#[test]
fn a_cycle_does_not_move_onto_a_fast_lines_alias() {
    // An 8.18 s line reads as a 45 s alias in 10 s readings: a 45 s
    // cycle found at 90 s stays there when the search lists that line.
    let fast = 1.0 / (0.1 + 1.0 / 45.0);
    let line = search_line(fast, 0.3, "ms", None);
    for seed in 0..8 {
        let d = sines(400 + seed, &[(45.0, 0.8)]);
        assert!(from_90(&d, None).is_some(), "seed {seed}");
        assert!(from_90(&d, Some(&line)).is_none(), "seed {seed}");
    }
}

#[test]
fn a_fast_lines_alias_sharing_the_phases_is_left_out_of_them() {
    // A 45 s cycle beside a listed 3.75 s escape-wheel line, whose 30 s
    // alias (a third of 90 s) is half the cycle's size and not listed
    // itself: the alias is left out of the nine phases and the cycle
    // moves; a 90 s and 45 s mix beside the same line stays.
    let line = search_line(3.75, 0.3, "ms", Some("escape wheel"));
    for seed in 0..8 {
        let d = sines(600 + seed, &[(45.0, 0.8), (30.0, 0.4)]);
        let c = from_90(&d, Some(&line)).unwrap_or_else(|| panic!("seed {seed}"));
        assert!((c.period_s - 45.0).abs() < 0.5, "seed {seed}: {c:?}");
        let d = sines(700 + seed, &[(90.0, 0.8), (45.0, 0.8), (30.0, 0.4)]);
        assert!(from_90(&d, Some(&line)).is_none(), "seed {seed}");
    }
}

#[test]
fn the_headline_names_a_cycle_found_at_twice_its_period_at_its_own() {
    // Through the whole check: a 45 s cycle beside a listed 3.75 s
    // line's 30 s alias half its size reads as 45 s; a 90 s cycle
    // beside the same alias as 90 s.
    let tau = 2.0 * std::f64::consts::PI;
    for seed in 0..8 {
        for (p, name) in [(45.0, "every 45 s"), (90.0, "every 90 s")] {
            let mut z = Noise(500 + seed);
            let y: Vec<f64> = (0..720)
                .map(|i| {
                    let t = 10.0 * (i as f64 + 0.5);
                    5.0 + 0.8 * (tau * t / p + 0.3).sin()
                        + 0.4 * (tau * t / 30.0 + 1.2).sin()
                        + z.next()
                })
                .collect();
            let s = rate_check(y, 3.75, &[]);
            assert!(
                s.headline.starts_with(&format!("Rate repeats {name}")),
                "{p} s, seed {seed}: {}",
                s.headline
            );
        }
    }
}

#[test]
fn no_reading_is_taken_from_part_of_its_span() {
    // Five 2 s windows a 10 s reading: one missing (the edge of a gap
    // or a mask) leaves the reading out.
    let mut fine = grid(vec![1.0; 15], 2.0);
    fine.y[7] = f64::NAN;
    let r = rebin(&fine, 0.0, 10.0, 3);
    assert_eq!(r.y[0], 1.0);
    assert!(r.y[1].is_nan(), "{:?}", r.y);
    assert_eq!(r.y[2], 1.0);
    // At 15 s, readings hold seven and eight windows in turn: an hour
    // of whole windows keeps every reading, and one missing window
    // loses only its own.
    let mut fine = grid(vec![1.0; 1800], 2.0);
    let r = rebin(&fine, 0.0, 15.0, 240);
    assert!(r.y.iter().all(|v| *v == 1.0), "{:?}", r.y);
    fine.y[100] = f64::NAN;
    let r = rebin(&fine, 0.0, 15.0, 240);
    let lost: Vec<usize> = (0..240).filter(|&b| r.y[b].is_nan()).collect();
    assert_eq!(lost, vec![13]);
    // Rate readings: beats in each 2 s of the 10 s, or none.
    let beats: Vec<crate::beats::Beat> = (0..160)
        .filter(|&i| !(80..96).contains(&i))
        .map(|index| crate::beats::Beat {
            time: index as f64 * 0.125,
            index,
            quality: 1.0,
        })
        .collect();
    assert!(crate::clean::covered(&beats, 0.0, 10.0, 2.0));
    assert!(!crate::clean::covered(&beats, 10.0, 20.0, 2.0));
    // Allan deviation: a block with a reading missing is left out.
    let mut y: Vec<f64> = (0..64).map(|i| (i % 2) as f64).collect();
    y[5] = f64::NAN;
    let whole = allan(&y, 1.0);
    let four = whole.iter().find(|a| a.0 == 4.0).unwrap();
    // 57 pairs of 4-reading blocks, less the 6 with reading 5 in
    // either block: 51, or 12 independent (3 of 4 readings used to
    // be enough, keeping all 57).
    assert_eq!(four.2, 12, "{whole:?}");
    // A break with 1 s of no sound after 300 s of audio: a window
    // reaching 0.3 s into the gap misses the cut halfway through it
    // but is left out; one ending or starting at an edge is whole.
    let breaks = [crate::breaks::Break {
        audio_s: 300.0,
        gap_s: 1.0,
    }];
    let holes = Cuts::new(None, &breaks, &[]);
    let gap = holes.holes()[0];
    assert!(
        (gap.start_s - 300.0).abs() < 1e-5 && gap.end_s == 301.0,
        "{gap:?}"
    );
    // The old cut, halfway through the gap.
    let cut = holes.true_time(300.0) - 0.5 - 1e-6;
    assert!(!(290.3 < cut && cut < 300.3));
    assert!(!holes.clear(290.3, 300.3));
    assert!(!holes.clear(300.8, 310.8));
    assert!(holes.clear(290.0, 300.0));
    assert!(holes.clear(301.0, 311.0));
}

/// `n` readings 10 s apart of AR(1) noise with lag-one correlation
/// `rho`, scatter 1, and `step` added from halfway.
fn correlated(z: &mut Noise, n: usize, rho: f64, step: f64, gaps: Gaps) -> Grid {
    let mut v = z.next();
    let y = (0..n)
        .map(|i| {
            v = rho * v + (1.0 - rho * rho).sqrt() * z.next();
            let missing = match gaps {
                Gaps::None => false,
                // Two blocks of a sixth and a ninth of the take, and a
                // tenth of the rest dropped here and there.
                Gaps::Blocks => {
                    (n / 6..n / 3).contains(&i)
                        || (n / 2..n / 2 + n / 9).contains(&i)
                        || z.next() > 1.28
                }
                Gaps::Half => z.next() > 0.0,
            };
            if missing {
                f64::NAN
            } else {
                v + if i >= n / 2 { step } else { 0.0 }
            }
        })
        .collect();
    grid(y, 10.0)
}

/// Readings missing from [`correlated`] ones.
#[derive(Clone, Copy, Debug)]
enum Gaps {
    None,
    Blocks,
    /// Half of them, at random.
    Half,
}

/// Of `runs` takes of `n` readings, how many the CUSUM and the change
/// finder each flag.
fn flagged(rho: f64, step: f64, n: usize, runs: usize, gaps: Gaps) -> (usize, usize) {
    let mut z = Noise(647);
    let (mut cusum, mut changes) = (0, 0);
    for _ in 0..runs {
        let s = check_series(
            SeriesKind::Rate,
            correlated(&mut z, n, rho, step, gaps),
            None,
            None,
            None,
            &[],
            &Config::default(),
        );
        cusum += usize::from(s.cusum.unwrap().max > CUSUM_BAND);
        changes += usize::from(s.changes.unwrap().segments.len() > 1);
    }
    (cusum, changes)
}

/// Correlated readings that never change level pass the CUSUM's 95% band
/// about 95 times in 100, as independent ones do: scaled by the
/// short-term σ alone it flagged 82% of these at ρ 0.5 and all at 0.9
/// (#647). The change finder, judged against the same long-run σ, finds
/// no change that is not there in 95 of 100 two-hour takes at ρ 0.9
/// (#649).
#[test]
fn correlated_steady_readings_raise_few_false_alarms() {
    // About 5% at ρ 0.5 and 6.5% at ρ 0.9, where 360 readings say least
    // about ρ; 8% leaves room for the draw.
    for (rho, runs) in [(0.5, 1000), (0.9, 2000)] {
        let (cusum, _) = flagged(rho, 0.0, 360, runs, Gaps::None);
        assert!(
            cusum * 100 <= 8 * runs,
            "CUSUM: {cusum} of {runs} at ρ {rho}"
        );
    }
    // A gap must not join readings that were not neighbours: read across
    // it, ρ came out low and the CUSUM flagged 20% with half missing and
    // 8% with the blocks; now about 6% with half missing and 5% with the
    // blocks.
    for gaps in [Gaps::Blocks, Gaps::Half] {
        let (cusum, _) = flagged(0.9, 0.0, 360, 2000, gaps);
        assert!(cusum <= 160, "CUSUM: {cusum} of 2000 with {gaps:?} gaps");
    }
    let (_, changes) = flagged(0.9, 0.0, 720, 400, Gaps::None);
    assert!(changes <= 20, "change finder: {changes} of 400");
}

/// A real step twice the readings' scatter is still found at ρ 0.9.
#[test]
fn a_step_in_correlated_readings_is_still_found() {
    let (cusum, _) = flagged(0.9, 2.0, 360, 200, Gaps::None);
    assert!(cusum >= 160, "CUSUM: {cusum} of 200");
    let (_, changes) = flagged(0.9, 2.0, 720, 200, Gaps::None);
    assert!(changes >= 160, "change finder: {changes} of 200");
}

/// One rule names a level change (#697): the CUSUM marks its corner only
/// where the change finder cut, so a steady take whose CUSUM wanders past
/// the band names none, and a real step twice the scatter is still named,
/// at the finder's cut.
#[test]
fn the_cusum_names_a_change_only_where_the_finder_cuts() {
    let mut z = Noise(697);
    let mut past_band = 0;
    for (step, runs) in [(0.0, 400), (2.0, 200)] {
        let mut named = 0;
        for _ in 0..runs {
            let s = check_series(
                SeriesKind::Rate,
                correlated(&mut z, 720, 0.9, step, Gaps::None),
                None,
                None,
                None,
                &[],
                &Config::default(),
            );
            let (cu, ch) = (s.cusum.unwrap(), s.changes.unwrap());
            assert_eq!(cu.shift, ch.segments.len() > 1);
            if cu.shift {
                assert!(ch.segments[1..].iter().any(|g| g.start_s == cu.at_s));
            } else if step == 0.0 && cu.max > CUSUM_BAND {
                past_band += 1;
            }
            named += usize::from(cu.shift);
        }
        if step == 0.0 {
            assert!(named <= 20, "steady: {named} of {runs} named a change");
        } else {
            assert!(named >= 160, "step: {named} of {runs} named a change");
        }
    }
    // The case #697 found: past the band, and no change named.
    assert!(past_band > 0);
}
