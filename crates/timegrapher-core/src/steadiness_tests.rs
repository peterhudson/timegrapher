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
    let a = autocorrelation_less(&y, 10.0, None, &cfg);
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

/// Twelve hours of rate readings 10 s apart drifting from 0 to 6 s/d,
/// as a mainspring running down does, with independent scatter and
/// `cycle` s/d of a 60 s line on top.
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
        beyond_simulated: false,
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
        s.headline.ends_with("; the beat timing repeats every 5.0 s, the escape wheel's period, 0.043 ms peak to peak; 10 s readings are too coarse to show it."),
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
        s.headline.ends_with("; the amplitude repeats every 4.0 s, 0.8° peak to peak; 2 s readings are too coarse to show it."),
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
            "; no level change, trend or correlation between readings detected. The amplitude's period search also finds 20.0 s (0.8° peak to peak)."
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
        s.headline.ends_with(". The beat timing repeats every 4.0 s, the escape wheel's period, 0.043 ms peak to peak; 2 s readings are too coarse to show it."),
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
            " The amplitude's period search also finds 45 s (0.8° peak to peak) and 600 s."
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
#[expect(
    clippy::too_many_lines,
    reason = "over the limit before it was set; split it (architecture guide, section 12)"
)]
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
            "shifts level 1 time: +4.9 s/d, then +9.1 s/d",
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
            "repeats every 46 s, 5.84 s/d peak to peak",
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

/// A plot's title and its text say the same shifts: every level, the
/// middle one too, in the same words: three levels, the middle one
/// highest, that the change finder splits (Walter's walk of #1096, #734).
#[test]
fn a_summary_names_every_level_the_headline_names() {
    let mut z = Noise(734);
    let s = check_series(
        SeriesKind::Rate,
        grid(
            (0..3000)
                .map(|i| match i {
                    0..=999 => 5.0,
                    1000..=1999 => 9.0,
                    _ => 7.0,
                } + 2.0 * z.next())
                .collect(),
            2.0,
        ),
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(s.lead, Lead::ShiftingMean, "{}", s.headline);
    assert!(
        s.summary.starts_with("shifts level 2 times: "),
        "{}",
        s.summary
    );
    assert_eq!(s.summary.matches(", then ").count(), 2, "{}", s.summary);
    assert!(
        s.headline.starts_with(&format!("Rate {}.", s.summary)),
        "{} | {}",
        s.headline,
        s.summary
    );
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
        "repeats every 4 min, 5 turns of 48 s, 9.74 s/d peak to peak"
    );
}

/// A rate that bends with the amplitude, flattest near 245°, still gets
/// no fitted curve and no turn: the headline is the slope and the ends
/// (Peter, 13 Nov).
#[test]
fn the_isochronism_headline_names_no_turn() {
    let mut z = Noise(5);
    let n = 26 * 360;
    let amp: Vec<f64> = (0..n)
        .map(|i| 270.0 - 52.0 * i as f64 / n as f64 + 2.0 * z.next())
        .collect();
    let rate: Vec<f64> = amp
        .iter()
        .map(|a| 0.05 * (a - 245.0).powi(2) + 1.5 * z.next())
        .collect();
    let c = isochronism_curve(
        &grid(rate, 10.0),
        &grid(amp, 10.0),
        &vec![4.0; n],
        &Config::default(),
    )
    .unwrap();
    assert!(!c.headline.contains("changes least"), "{}", c.headline);
    assert!(c.headline.contains("The rate reads"), "{}", c.headline);
    assert!(c.headline.ends_with("°."), "{}", c.headline);
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
    // Each amplitude reading's noise: the 2° scattered on it.
    let noise = vec![4.0; n];
    let c = isochronism_curve(
        &grid(rate.clone(), 10.0),
        &grid(amp.clone(), 10.0),
        &noise,
        &cfg,
    )
    .unwrap();
    assert!(
        (c.s_per_day_per_deg.unwrap() - 0.5).abs() < 0.05,
        "{}",
        c.headline
    );
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

/// Whether the headline names a level change: as its lead, or as the
/// level also moving.
fn headline_names_a_change(s: &SeriesCheck) -> bool {
    s.headline.contains("shifts level") || s.headline.contains("level also moves")
}

/// One rule names a level change (#697): the CUSUM marks its corner only
/// where the headline names one, at one of the change finder's cuts. A
/// steady take whose CUSUM wanders past the band names none, and a real
/// step twice the scatter is still named.
#[test]
fn the_cusum_names_a_change_only_where_the_headline_does() {
    let mut z = Noise(697);
    let mut past_band = 0;
    for (step, runs) in [(0.0, 400), (2.0, 200)] {
        let (mut named, mut moved) = (0, 0);
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
            let cu = s.cusum.as_ref().unwrap();
            assert_eq!(cu.shift, headline_names_a_change(&s), "{}", s.headline);
            if cu.shift {
                let ch = s.changes.as_ref().unwrap();
                assert!(ch.segments[1..].iter().any(|g| g.start_s == cu.at_s));
            } else if step == 0.0 && cu.max > CUSUM_BAND {
                past_band += 1;
            }
            named += usize::from(cu.shift);
            moved += usize::from(cu.shift || s.lead == Lead::Drifting);
        }
        if step == 0.0 {
            assert!(named <= 20, "steady: {named} of {runs} named a change");
        } else {
            // The headline tells a step from a slide by whichever of a
            // line and the stretches explains more (#721): 158 named at
            // this seed, against 111 when a line needed only 0.8 of the
            // stretches' share and two in five of these steps were called
            // slides; either way it says the level moved.
            assert!(named >= 150, "step: {named} of {runs} named a change");
            assert!(moved >= 160, "step: {moved} of {runs} moved");
        }
    }
    // The case #697 found: past the band, and no change named.
    assert!(past_band > 0);
}

/// A cut series is a slide when its line, trend test passed, explains at
/// least as much as the stretches, and a level change when it explains
/// less (#721): either side of the tie, at 1.05 and 0.95 times.
#[test]
fn a_cut_series_is_a_slide_only_when_its_line_explains_at_least_as_much() {
    let cfg = Config::default();
    let mut z = Noise(721);
    let mut s = check_series(
        SeriesKind::Rate,
        correlated(&mut z, 720, 0.0, 3.0, Gaps::None),
        None,
        None,
        None,
        &[],
        &cfg,
    );
    assert!(s.two_state.is_none() && s.cycle.is_none());
    let ch = s.changes.as_mut().unwrap();
    assert!(ch.segments.len() > 1 && ch.p_value < cfg.level_alpha);
    ch.explained = 0.5;
    let t = s.trend.as_mut().unwrap();
    t.p_value = 0.0;
    for (ratio, want) in [
        (1.05, Lead::Drifting),
        (1.0, Lead::Drifting),
        (0.95, Lead::ShiftingMean),
    ] {
        let t = s.trend.as_mut().unwrap();
        (t.explained, t.curve_explained) = (0.5 * ratio, 0.0);
        assert_eq!(lead(&s, &cfg), want, "line at {ratio} of the stretches");
    }
}

/// A step the change finder cuts but that explains under a fifth of the
/// readings' variance is not named, by the headline or the CUSUM (as on
/// an ETA 2824's beat error, #717); nor is a slide the finder cuts into
/// steps, which the headline calls a slide (as on a Reverso's).
#[test]
fn a_small_share_cut_and_a_slide_name_no_change() {
    let mut z = Noise(717);
    let small = (0..20)
        .map(|_| {
            check_series(
                SeriesKind::Rate,
                correlated(&mut z, 720, 0.0, 0.5, Gaps::None),
                None,
                None,
                None,
                &[],
                &Config::default(),
            )
        })
        .find(|s| s.changes.as_ref().unwrap().segments.len() > 1)
        .expect("a small step the finder cuts");
    assert!(small.changes.as_ref().unwrap().explained < Config::default().min_explained);
    assert!(!small.cusum.as_ref().unwrap().shift);
    assert!(!headline_names_a_change(&small), "{}", small.headline);

    let y: Vec<f64> = (0..720)
        .map(|i| i as f64 / 180.0 + 0.3 * z.next())
        .collect();
    let slide = check_series(
        SeriesKind::Rate,
        grid(y, 10.0),
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(slide.lead, Lead::Drifting, "{}", slide.headline);
    assert!(slide.changes.as_ref().unwrap().segments.len() > 1);
    assert!(!slide.cusum.as_ref().unwrap().shift);
    assert!(!headline_names_a_change(&slide), "{}", slide.headline);
}

/// Two hours of 2 s amplitude windows wandering a few degrees over
/// minutes, each window `window_noise`° off, and 10 s rate readings
/// following the true amplitude at −0.5 s/d per degree.
fn noisy_amplitude(window_noise: f64, seed: u64) -> (Grid, Grid, Vec<f64>) {
    let mut z = Noise(seed);
    let true_amp = |t: f64| 250.0 + 3.0 * (t / 900.0).sin() + 2.0 * (t / 377.0).cos();
    let fine = Grid {
        t0: 0.0,
        step: 2.0,
        y: (0..3600)
            .map(|i| true_amp(2.0 * i as f64 + 1.0) + window_noise * z.next())
            .collect(),
    };
    let n = 720;
    let (amp, noise) = pairing_readings(&fine, &rebin(&fine, 0.0, 10.0, n));
    let rate = grid(
        (0..n)
            .map(|i| {
                let t = 10.0 * i as f64;
                let a = (0..5)
                    .map(|k| true_amp(t + 2.0 * k as f64 + 1.0))
                    .sum::<f64>()
                    / 5.0;
                -0.5 * (a - 250.0) + 0.5 * z.next()
            })
            .collect(),
        10.0,
    );
    (rate, amp, noise)
}

/// Noise in the amplitude readings shrinks a least-squares slope towards
/// 0; read off the windows' scatter, it is undone, and the slope carries
/// a ± that covers the truth (#651). Here the readings' noise of about
/// 1.6° (five windows weighted as the rate's fit weights them) shrinks
/// the plain slope by about a quarter.
#[test]
fn the_isochronism_slope_is_read_off_the_amplitudes_noise() {
    let (rate, amp, noise) = noisy_amplitude(3.35, 21);
    // Each reading's noise is that of its five windows weighted as the
    // rate's fit weights them: their variance times the sum of the
    // squared weights, about 0.232 (#951).
    let mean = noise.iter().sum::<f64>() / noise.len() as f64;
    let w2: f64 = (0..5)
        .map(|k| {
            crate::amplitude_slope::rate_fit_weight(k as f64 / 5.0, (k + 1) as f64 / 5.0).powi(2)
        })
        .sum();
    let want = 3.35f64.powi(2) * w2;
    assert!((mean - want).abs() < 0.3, "{mean} against {want}");
    let c = isochronism_curve(&rate, &amp, &noise, &Config::default()).unwrap();
    let (slope, se) = (
        c.s_per_day_per_deg.unwrap(),
        c.s_per_day_per_deg_se.unwrap(),
    );
    assert!(
        (slope + 0.5).abs() < 2.0 * se + 0.01,
        "{slope} ± {se}: {}",
        c.headline
    );
    assert!(se > 0.0 && se < 0.05, "{se}");
    assert!(
        c.reliability.unwrap() > 0.5 && c.reliability.unwrap() < 0.95,
        "{:?}",
        c.reliability
    );
    assert!(c.headline.contains(" ± "), "{}", c.headline);
    // The end bins are their own sentence, not the slope's evidence (#734).
    assert!(
        c.headline.contains("per degree. The rate reads "),
        "{}",
        c.headline
    );
    // Taking the noise as nothing leaves the shrunken slope.
    let plain = isochronism_curve(&rate, &amp, &vec![0.0; noise.len()], &Config::default())
        .unwrap()
        .s_per_day_per_deg
        .unwrap();
    assert!(plain > slope + 0.05, "{plain} against {slope}");
    // The rate against the amplitude within half an hour reads the same
    // noise: with the slow wander taken out, it is most of what is left,
    // so no slope is given there.
    let x = cross_correlation(&rate, &amp, &noise, &Config::default()).unwrap();
    assert!(x.reliability.unwrap() < 0.5, "{}", x.headline);
    assert_eq!(x.s_per_day_per_deg, None);
    assert!(
        x.headline
            .contains("own noise is half or more of their variance")
            && !x.headline.contains("reliability")
            && x.headline.matches("no slope").count() == 1,
        "{}",
        x.headline
    );
}

/// When the amplitude's noise is most of its spread, no slope is read.
#[test]
fn a_slope_is_withheld_when_the_amplitude_is_mostly_noise() {
    let (rate, amp, noise) = noisy_amplitude(30.0, 22);
    let c = isochronism_curve(&rate, &amp, &noise, &Config::default()).unwrap();
    assert!(c.reliability.unwrap() < 0.5);
    assert_eq!(c.s_per_day_per_deg, None);
    assert_eq!(c.rate_change_s_per_day, None);
    // Said in a watchmaker's words, not by the statistic's name (#734).
    assert!(
        c.headline
            .contains("own noise is half or more of their variance, so no slope is read")
            && !c.headline.contains("reliability"),
        "{}",
        c.headline
    );
}

#[test]
fn independent_readings_over_15_minutes_read_no_correlation_after_the_quadratic() {
    // 1000 seeded grids of 90 independent readings 10 s apart (15 min,
    // so the quadratic is taken out first), every reading present and
    // with a 60 s gap. Taking out three terms pulled the lag-one r to
    // about −0.038 on average, and with the Q weighed as a sum over all
    // the readings the Ljung–Box p fell under the detector's level (0.01)
    // in 47 of 1000 (#828). Less its bias the lag-one r reads about 0
    // (−0.004), and p falls under 0.01 in 22 (16 with the gap): the
    // chi-squared tail runs a little light on 90 readings, about 1.6% for
    // white noise with nothing but the mean taken out.
    for gap in [false, true] {
        let mut rng = crate::synth::Rng::new(828);
        let (mut r1, mut low_p) = (0.0, 0);
        let runs = 1000;
        for _ in 0..runs {
            let y: Vec<f64> = (0..90)
                .map(|i| {
                    let v = rng.normal();
                    if gap && (40..46).contains(&i) {
                        f64::NAN
                    } else {
                        v
                    }
                })
                .collect();
            let out = check_series(
                SeriesKind::Amplitude,
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
            let a = out.autocorrelation.as_ref().unwrap();
            r1 += a.r[1];
            low_p += usize::from(a.p_value < Config::default().alpha);
        }
        let mean = r1 / runs as f64;
        assert!(mean.abs() < 0.01, "gap {gap}: mean lag-one r {mean}");
        assert!(low_p <= 30, "gap {gap}: p under 0.01 in {low_p} of {runs}");
    }
}

/// The CUSUM's long-run σ allows for the bias of a ρ read with the
/// series' largest step chosen and taken out (#917): `check_series`
/// reads it with [`Slow::ChosenStep`], which the change finder's
/// [`Slow::Step`] leaves out.
#[test]
fn the_cusum_scales_by_the_rho_of_a_chosen_step() {
    let mut z = Noise(917);
    let mut x = 0.0;
    let y: Vec<f64> = (0..720)
        .map(|_| {
            x = 0.5 * x + 0.75f64.sqrt() * z.next();
            x
        })
        .collect();
    let c = check_series(
        SeriesKind::Rate,
        grid(y.clone(), 10.0),
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    let sst = c.short_term_sd.unwrap();
    let got = c.long_run_sd.unwrap();
    let chosen = confidence::long_run_sd(&y, sst, Slow::ChosenStep);
    let step = confidence::long_run_sd(&y, sst, Slow::Step);
    assert!(
        (got - chosen).abs() < 1e-12 * chosen,
        "{got} against {chosen}"
    );
    assert!(got - step > 1e-3 * step, "{got} against {step}");
}

/// Through the whole check, the rub scenario's slopes against the
/// amplitude read its true coupling of 2.0 s/d per degree (#951): both
/// are taken against the paired amplitude, not the medians the amplitude
/// series shows, which read about 1.9.
#[test]
fn a_rubs_reported_slopes_read_its_true_coupling() {
    let s = crate::scenarios::find("rub").unwrap();
    let log = crate::scenarios::beat_log(&s, 1);
    let lc = crate::longterm::LongConfig {
        wheels: crate::periodicity::standard_wheels(crate::scenarios::BPH, 20),
        ..Default::default()
    };
    let long = crate::longrun::analyse(&log, None, &lc);
    let r = check(&log, None, &long, &lc, &Config::default());
    let x = r.rate_amplitude.unwrap();
    let iso = r.isochronism.unwrap();
    for (what, s) in [
        ("rate against amplitude", x.s_per_day_per_deg),
        ("isochronism", iso.s_per_day_per_deg),
    ] {
        let s = s.unwrap();
        assert!((s - 2.0).abs() < 0.05, "{what}: {s}");
    }
}

#[test]
fn candidates_on_the_same_peak_are_tested_once_with_the_same_answer() {
    // Six hours of 2 s amplitude windows (and their 10 s readings) with
    // a 257.4 s cycle the period search lists, as on a long take: the
    // autocorrelation peaks at it and its turns, and the period search
    // and the autocorrelation's repeat land on the same peak. Testing
    // each peak once must keep the cycle that testing every candidate
    // keeps, bit for bit (Norm's ruling on the cycle search).
    let tau = std::f64::consts::TAU;
    let amp = |n: usize, step: f64, seed: u64| {
        let mut z = Noise(seed);
        let y = (0..n)
            .map(|i| {
                let t = (i as f64 + 0.5) * step;
                270.0 + 2.0 * (tau * t / 257.4).sin() + 0.7 * (tau * t / 61.3).sin() + z.next()
            })
            .collect();
        grid(y, step)
    };
    let (fine, readings) = (amp(10_800, 2.0, 21), amp(2_160, 10.0, 22));
    let mut line = search_line(257.4, 4.0, "deg", None);
    line.other_periods_s = vec![61.3, 128.7];
    line.other_harmonic_of = vec![None, None];
    let cfg = Config {
        acf_alone: true,
        ..Config::default()
    };
    let s = check_series(
        SeriesKind::Amplitude,
        readings.clone(),
        Some(fine.clone()),
        Some(line),
        None,
        &[],
        &cfg,
    );
    let (d, refined) = refined_candidates(&s, &readings, Some(&fine), &cfg);
    let once = same_peaks_once(&refined, d.span());
    assert!(
        refined.len() >= once.len() + 4 && once.len() >= 3,
        "{} candidates, {} peaks",
        refined.len(),
        once.len()
    );
    let every: Vec<(f64, CycleSource)> = refined.iter().map(|&(p, src, _)| (p, src)).collect();
    let all = strongest_cycle(&s, &fine, &d, &every, &[], &cfg);
    let kept = s.cycle.clone();
    assert_eq!(format!("{kept:?}"), format!("{all:?}"));
    let c = kept.unwrap();
    assert!((c.period_s - 257.4).abs() < 0.5, "{c:?}");
}

/// A window goes in the cell its middle lies in, as `Cuts::window_in`
/// takes a window into a stretch (#636): a short window from 3.2 to
/// 4.0 s on a 2 s grid sits in cell 1, where rounding its start would
/// have put it in cell 2.
#[test]
fn a_window_goes_in_the_cell_its_middle_lies_in() {
    let w = |start_s: f64, end_s: f64, deg: f64| crate::amplitude::AmplitudeWindow {
        start_s,
        end_s,
        even_deg: Some(deg),
        odd_deg: Some(deg),
        beat_error_ms: None,
        beat_error_unlock_ms: None,
        even_unlock_ms: None,
        odd_unlock_ms: None,
        even_drop_ms: None,
        odd_drop_ms: None,
        culled_ms: None,
    };
    let log = BeatLog {
        sample_rate: 48000,
        duration_s: 10.0,
        bph: 28800,
        lift_deg: 52.0,
        beats: Vec::new(),
        amplitude_windows: vec![w(0.0, 2.0, 250.0), w(3.2, 4.0, 260.0), w(6.0, 8.0, 270.0)],
        clipped_beats: vec![],
        clipped_samples: 0,
        jumps: vec![],
        sided: vec![],
        by_count: vec![],
        snippets: Vec::new(),
    };
    let g = windows_grid(&log, None, &[], &|w| w.mean());
    assert_eq!((g.t0, g.step), (0.0, 2.0));
    assert_eq!(g.y.len(), 4, "{:?}", g.y);
    assert_eq!(g.y[0], 250.0);
    assert_eq!(g.y[1], 260.0, "{:?}", g.y);
    assert!(g.y[2].is_nan(), "{:?}", g.y);
    assert_eq!(g.y[3], 270.0);
}

/// The readings [`reading_grids`] gives the app's Distributions
/// pane are the ones `check` counts as "readings" (#1144): each of the
/// three counts the same values to the same figures.
#[test]
fn reading_grids_are_the_readings_check_counts() {
    let s = crate::scenarios::find("rub").unwrap();
    let log = crate::scenarios::beat_log(&s, 1);
    let lc = crate::longterm::LongConfig {
        wheels: crate::periodicity::standard_wheels(crate::scenarios::BPH, 20),
        ..Default::default()
    };
    let cfg = Config::default();
    let long = crate::longrun::analyse(&log, None, &lc);
    let r = check(&log, None, &long, &lc, &cfg);
    let (grids, selected) =
        reading_grids(&log, long.clock.as_ref(), &long.breaks, cfg.rate_reading_s);
    use crate::distribution::{AMPLITUDE, BEAT_ERROR, RATE};
    for (kind, g) in [RATE, AMPLITUDE, BEAT_ERROR].into_iter().zip(&grids) {
        let values: Vec<f64> = g.y.iter().copied().filter(|v| v.is_finite()).collect();
        assert!(values.len() > 100, "{}: {}", kind.series, values.len());
        let ours = crate::distribution::distribution(&values, kind, "readings", g.step);
        let theirs = r
            .distributions
            .overall
            .iter()
            .find(|d| d.series == kind.series && d.values == "readings");
        assert_eq!(ours.as_ref(), theirs, "{}", kind.series);
    }
    let be = r
        .series
        .iter()
        .find(|c| c.series == SeriesKind::BeatError)
        .unwrap();
    assert_eq!(selected.beat_error_source, be.beat_error_source);
}

/// A period named where the null table does not reach (here a 1 s step,
/// which the table holds no cells for) carries `period_search_uncalibrated`
/// and its sentence, and the series reports the null as uncalibrated
/// (#1195, Norm's condition 2).
#[test]
fn a_period_the_null_table_does_not_reach_carries_the_caveat() {
    let tau = std::f64::consts::TAU;
    let mut z = Noise(15);
    let y = (0..600)
        .map(|i| (tau * (i as f64 + 0.5) / 60.0).sin() + 0.1 * z.next())
        .collect();
    let r = longterm::analyse(&grid(y, 1.0), &LongConfig::default());
    assert!(!r.components.is_empty());
    assert!(!r.search.null.calibrated);
    let mut s = check_series(
        SeriesKind::Amplitude,
        grid(noise(14, 360, 270.0, 1.5), 10.0),
        None,
        Some(search_line(60.0, 0.8, "deg", None)),
        None,
        &[],
        &Config::default(),
    );
    let before = s.headline.clone();
    hold_to_null(&mut s, &r);
    let c = longterm::period_uncalibrated(&r).expect("caveat");
    assert!(s.caveats.contains(&c), "{:?}", s.caveats);
    assert_eq!(s.headline, format!("{before} {}", c.long(None).unwrap()));
    assert!(!s.null.expect("null").calibrated);
}

/// The same cycle on 2 s windows over 30 min, which the table covers,
/// carries no such caveat, and the series reports its null as held.
#[test]
fn a_period_the_null_table_covers_carries_no_caveat() {
    let tau = std::f64::consts::TAU;
    let mut z = Noise(16);
    let y = (0..900)
        .map(|i| (tau * (i as f64 + 0.5) * 2.0 / 60.0).sin() + 0.5 * z.next())
        .collect();
    let r = longterm::analyse(&grid(y, 2.0), &LongConfig::default());
    assert!(!r.components.is_empty());
    assert!(r.search.null.calibrated);
    let mut s = check_series(
        SeriesKind::Amplitude,
        grid(noise(14, 360, 270.0, 1.5), 10.0),
        None,
        Some(search_line(60.0, 0.8, "deg", None)),
        None,
        &[],
        &Config::default(),
    );
    let before = (s.headline.clone(), s.caveats.clone());
    hold_to_null(&mut s, &r);
    assert_eq!((s.headline.clone(), s.caveats.clone()), before);
    let n = s.null.expect("null");
    assert!(n.calibrated && n.model == longterm::NullModel::Readings);
}

/// Beat timing whose noise is flicker (power falling as 1/f, a sum of
/// relaxations a decade apart), not the walk plus white the null draws,
/// with a 60 s cycle on it: the rate's cycle carries the misfit caveat,
/// and the same cycle on a walk plus white does not (#1218, #1219).
#[test]
fn a_cycle_on_timing_the_null_does_not_model_carries_the_misfit_caveat() {
    let tau = std::f64::consts::TAU;
    let (step, n) = (0.25, 21600);
    let take = |flicker: bool| {
        let mut z = crate::synth::Rng::new(31);
        let taus = [1.0, 10.0, 100.0, 1000.0, 10000.0];
        let mut x = [0.0f64; 5];
        let mut walk = 0.0;
        let y = (0..n)
            .map(|i| {
                let t = i as f64 * step;
                let noise = if flicker {
                    let mut v = 0.0;
                    for (xi, tau) in x.iter_mut().zip(taus) {
                        let phi = (-step / tau).exp();
                        *xi = phi * *xi + (1.0 - phi * phi).sqrt() * z.normal();
                        v += *xi;
                    }
                    1e-4 * v
                } else {
                    walk += 1e-5 * z.normal();
                    walk + 1e-4 * z.normal()
                };
                noise + 3e-4 * (tau * t / 60.0).sin()
            })
            .collect();
        longterm::analyse_timing(&grid(y, step), &LongConfig::default())
    };
    let rate = || {
        check_series(
            SeriesKind::Rate,
            grid(noise(14, 540, 5.0, 1.5), 10.0),
            None,
            Some(search_line(60.0, 0.8, "s/d", None)),
            None,
            &[],
            &Config::default(),
        )
    };
    let r = take(true);
    assert!(!r.components.is_empty() && r.search.null.calibrated);
    let n = r.search.null;
    assert!(n.misfit.unwrap() > n.misfit_limit.unwrap(), "{n:?}");
    let mut s = rate();
    let before = s.headline.clone();
    hold_to_null(&mut s, &r);
    let c = longterm::timing_misfit(&r).expect("caveat");
    assert!(s.caveats.contains(&c), "{:?}", s.caveats);
    assert_eq!(s.headline, format!("{before} {}", c.long(None).unwrap()));

    let r = take(false);
    assert!(!r.components.is_empty() && r.search.null.calibrated);
    let n = r.search.null;
    assert!(n.misfit.unwrap() <= n.misfit_limit.unwrap(), "{n:?}");
    assert!(longterm::timing_misfit(&r).is_none());
    let mut s = rate();
    let before = (s.headline.clone(), s.caveats.clone());
    hold_to_null(&mut s, &r);
    assert_eq!((s.headline.clone(), s.caveats.clone()), before);
}

/// Flicker beat timing with a sharp cycle on it (8 s of every minute
/// late), whose harmonics run far past the ones the search keeps: the
/// misfit takes those harmonics out with the cycle, so it must still see
/// the flicker beneath them and give the caveat (#1245).
#[test]
fn flicker_under_a_sharp_cycle_still_carries_the_misfit_caveat() {
    let (step, n) = (0.25, 21600);
    let mut z = crate::synth::Rng::new(37);
    let taus = [1.0, 10.0, 100.0, 1000.0, 10000.0];
    let mut x = [0.0f64; 5];
    let y = (0..n)
        .map(|i| {
            let t = i as f64 * step;
            let mut v = 0.0;
            for (xi, tau) in x.iter_mut().zip(taus) {
                let phi = (-step / tau).exp();
                *xi = phi * *xi + (1.0 - phi * phi).sqrt() * z.normal();
                v += *xi;
            }
            1e-4 * v + if t % 60.0 < 8.0 { 6e-4 } else { 0.0 }
        })
        .collect();
    let r = longterm::analyse_timing(&grid(y, step), &LongConfig::default());
    assert!(
        r.components.iter().any(|c| (c.period_s - 60.0).abs() < 2.0),
        "{:?}",
        r.components.iter().map(|c| c.period_s).collect::<Vec<_>>()
    );
    let n = r.search.null;
    assert!(n.calibrated, "{n:?}");
    assert!(n.misfit.unwrap() > n.misfit_limit.unwrap(), "{n:?}");
    assert!(longterm::timing_misfit(&r).is_some());
}

/// A white take of `n` readings `step` apart for
/// [`fresh_white_takes_hold_the_ljung_box`], with gaps of shape `kind`
/// placed at random: 0 none, 1 scattered single readings (10%), 2 one gap
/// (20%), 3 three blocks (15% in all), 4 one gap (25%), 5 scattered single
/// readings (15%).
fn white_take(seed: u64, step: f64, n: usize, kind: u8) -> Grid {
    let mut z = crate::synth::Rng::new(seed);
    let mut present = vec![true; n];
    let mut at = |z: &mut crate::synth::Rng, len: usize| {
        let start = (z.next_f64() * (n - len) as f64) as usize;
        present[start..start + len]
            .iter_mut()
            .for_each(|p| *p = false);
    };
    match kind {
        1 => (0..n / 10).for_each(|_| at(&mut z, 1)),
        2 => at(&mut z, n / 5),
        3 => (0..3).for_each(|_| at(&mut z, n * 15 / 300)),
        4 => at(&mut z, n / 4),
        5 => (0..n * 15 / 100).for_each(|_| at(&mut z, 1)),
        _ => {}
    }
    let y = present
        .iter()
        .map(|&p| {
            let v = z.normal();
            if p {
                v
            } else {
                f64::NAN
            }
        })
        .collect();
    grid(y, step)
}

/// Fresh-seed check of the Ljung–Box test against its white null (#1199):
/// white takes of 10 s (and 2 s) readings, and at n 180 and 360 three gap
/// shapes placed at random in each take (scattered single readings, 10%;
/// one gap, 20%; three blocks, 15% in all), each read against the table's
/// null for its present readings, n 150 and 500 between the table's
/// points, takes of 10 and 15 min under the quadratic detrend (with and
/// without gaps), one of 8 h, and gapped takes over 20 min with under
/// 20 min present (25 min with one 25% gap, 22 min with 15% scattered),
/// which read the median the grid got; counts at p 0.05, 0.01 and 0.004 against each
/// one's upper 99% bound. LB_TAG picks new seeds, LB_ROW one row.
/// Power is [`the_white_null_costs_little_power`].
#[test]
#[ignore]
fn fresh_white_takes_hold_the_ljung_box() {
    let cfg = Config::default();
    let env = |k: &str, d: u64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let (tag, draws) = (env("LB_TAG", 1), env("LB_DRAWS", 4000));
    let row: Option<usize> = std::env::var("LB_ROW").ok().and_then(|v| v.parse().ok());
    let bound = |e: f64| e + 2.58 * (e * (1.0 - e / draws as f64)).sqrt();
    let rows = [
        (10.0, 120usize, 0u8),
        (10.0, 180, 0),
        (10.0, 360, 0),
        (10.0, 720, 0),
        (2.0, 900, 0),
        (10.0, 150, 0),
        (10.0, 500, 0),
        (10.0, 180, 1),
        (10.0, 180, 2),
        (10.0, 180, 3),
        (10.0, 360, 1),
        (10.0, 360, 2),
        (10.0, 360, 3),
        (10.0, 90, 0),
        (10.0, 60, 0),
        (2.0, 300, 0),
        (10.0, 90, 3),
        (10.0, 2880, 0),
        (10.0, 150, 4),
        (10.0, 132, 5),
    ];
    for (i, &(step, n, kind)) in rows.iter().enumerate() {
        if row.is_some_and(|r| r != i) {
            continue;
        }
        let mut c = [0usize; 3];
        for d in 0..draws {
            let g = white_take(tag * 1_000_003 + d, step, n, kind);
            let p = detrended_autocorrelation(&g, true, &cfg).p_value;
            for (j, lvl) in [0.05, 0.01, 0.004].iter().enumerate() {
                c[j] += usize::from(p < *lvl);
            }
        }
        let over = [0.05, 0.01, 0.004]
            .iter()
            .zip(c)
            .any(|(l, k)| k as f64 > bound(l * draws as f64));
        let shape = [
            "none",
            "scattered 10%",
            "one 20%",
            "three blocks 15%",
            "one 25%",
            "scattered 15%",
        ][kind as usize];
        eprintln!(
            "LB row {i} step {step} n {n} gaps {shape}: {c:?} of {draws} (bounds {:.0} {:.0} {:.0}){}",
            bound(0.05 * draws as f64),
            bound(0.01 * draws as f64),
            bound(0.004 * draws as f64),
            if over { " OVER" } else { "" }
        );
    }
}

/// What holding the Ljung–Box test to its white null costs in power: AR(1)
/// readings over 30 min named at p 0.01, the plain χ² against the null
/// (#1199). LB_TAG picks new seeds.
#[test]
#[ignore]
fn the_white_null_costs_little_power() {
    let cfg = Config::default();
    let env = |k: &str, d: u64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let (tag, draws) = (env("LB_TAG", 1), env("LB_DRAWS", 4000));
    let take = |seed: u64, rho: f64| {
        let mut z = crate::synth::Rng::new(seed);
        let mut x = 0.0;
        let y = (0..180)
            .map(|_| {
                x = rho * x + (1.0 - rho * rho).sqrt() * z.normal();
                x
            })
            .collect();
        grid(y, 10.0)
    };
    for rho in [0.1, 0.2] {
        let (mut plain, mut held) = (0usize, 0usize);
        for d in 0..draws {
            let g = take(tag * 7_000_001 + d, rho);
            let dt = crate::longterm::detrend(&g, crate::longterm::DETREND_S);
            plain += usize::from(autocorrelation_less(&dt.y, 10.0, None, &cfg).p_value < 0.01);
            held += usize::from(detrended_autocorrelation(&g, true, &cfg).p_value < 0.01);
        }
        eprintln!("LB power rho {rho} n 180: plain {plain} held {held} of {draws} at p 0.01");
    }
}

/// Writes `data/white-null.json`, the white nulls the Ljung–Box test is
/// held to (#1199): gap-free white readings at each point, fixed seeds.
/// `cargo test --release -p timegrapher-core --lib write_white_null_table -- --ignored --nocapture`.
#[test]
#[ignore = "writes the table; run by hand"]
fn write_white_null_table() {
    use super::white::{
        draw_point, point_seed, Table, DRAW_BUDGET, MAX_DRAWS, MIN_DRAWS, TABLE_LAGS, TABLE_LB_LAGS,
    };
    let logs = |from: f64, to: f64, per_decade: f64| -> Vec<usize> {
        let k = ((to / from).log10() * per_decade).ceil() as usize;
        let mut v: Vec<usize> = (0..=k)
            .map(|i| (from * (to / from).powf(i as f64 / k as f64)).round() as usize)
            .collect();
        v.dedup();
        v
    };
    // Quadratic: under 20 min, by n (1 s readings reach 1,199).
    let mut jobs: Vec<(usize, f64)> = logs(30.0, 1199.0, 24.0)
        .into_iter()
        .map(|n| (n, 1.0))
        .collect();
    // Running median: by window, the take 4 to 8 windows long (a quarter of
    // the take to 2 h, then 30 min of up to 4 h).
    for w in logs(5.0, 1800.0, 12.0) {
        for ratio in [4.0, 5.0, 6.0, 7.0, 8.0] {
            jobs.push(((ratio * w as f64).round() as usize, 1800.0 / w as f64));
        }
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|sc| {
        for _ in 0..4 {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&(n, step)) = jobs.get(i) else { break };
                let p = draw_point(n, step, point_seed(n, step));
                eprintln!(
                    "point n {n} window {} draws {} skew {:.3}",
                    p.window, p.draws, p.null.q_skew
                );
                done.lock().unwrap().push(p);
            });
        }
    });
    let mut points = done.into_inner().unwrap();
    points.sort_by_key(|p| (p.window, p.n));
    let (quadratic, median) = points.into_iter().partition(|p| p.window == 0);
    let t = Table {
        note: format!("Generated by write_white_null_table (steadiness_tests.rs), fixed seeds. White gap-free readings, detrended as a take is (longterm::detrend with DETREND_S): a quadratic under 20 min, by readings; else a running median, by its window (5 to 1,800 readings) and the take's length over it (4 to 8). {DRAW_BUDGET} readings drawn a point ({MIN_DRAWS} to {MAX_DRAWS} series). Each point: the mean correlation at lags 1 to {TABLE_LAGS} (or a third of the take), and the mean, variance and skew of Ljung-Box Q over {TABLE_LB_LAGS} lags once that bias is off (#1199)."),
        lb_lags: TABLE_LB_LAGS,
        quadratic,
        median,
    };
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/white-null.json");
    std::fs::write(path, serde_json::to_string(&t).unwrap()).unwrap();
}

/// The white-null table covers the pane's takes and holds a short one, in
/// the default run (the full check is [`fresh_white_takes_hold_the_ljung_box`]):
/// 10 s readings from 20 min to 4 h and past are covered, another number of
/// lags is not, and 1,000 white takes of 30 min stay under the upper 99%
/// bound at p 0.05 and 0.01 (#1199).
#[test]
fn the_white_null_table_holds_a_short_take() {
    for n in [120, 150, 180, 360, 720, 1440, 5000] {
        assert!(white::white_null(n, n, 10.0, 10).is_some(), "{n}");
    }
    assert!(white::white_null(180, 180, 10.0, 5).is_none());
    // A 25 min take with 120 present reads the running median its grid got,
    // not the quadratic of a 20 min take.
    let (gapped, short) = (
        white::white_null(120, 150, 10.0, 10).unwrap(),
        white::white_null(120, 120, 10.0, 10).unwrap(),
    );
    assert!((gapped.bias[1] - short.bias[1]).abs() > 1e-3);
    let null = white::white_null(180, 180, 10.0, 10).unwrap();
    assert!(
        null.bias[1] < -0.01 && null.bias[1] > -0.03,
        "{}",
        null.bias[1]
    );
    let cfg = Config::default();
    let mut c = [0usize; 2];
    for d in 0..1000 {
        let p =
            detrended_autocorrelation(&white_take(77_000 + d, 10.0, 180, 0), true, &cfg).p_value;
        c[0] += usize::from(p < 0.05);
        c[1] += usize::from(p < 0.01);
    }
    assert!(c[0] <= 68 && c[1] <= 18, "{c:?} of 1000");
}

/// Through the whole check, the fourth-wheel scenario's 60 s rate line
/// stands far past p 0.0003 on a calibrated search: the Steadiness period
/// carries `beyond_simulated`, and its finding says the line is beyond the
/// simulated range rather than quoting the tail's extrapolation (#1243).
#[test]
fn a_strong_cycle_reads_beyond_the_simulated_range() {
    let s = crate::scenarios::find("fourth-wheel").unwrap();
    let log = crate::scenarios::beat_log(&s, 1);
    let lc = crate::longterm::LongConfig {
        wheels: crate::periodicity::standard_wheels(crate::scenarios::BPH, 20),
        ..Default::default()
    };
    let long = crate::longrun::analyse(&log, None, &lc);
    let r = check(&log, None, &long, &lc, &Config::default());
    let rate = r
        .series
        .iter()
        .find(|c| c.series == SeriesKind::Rate)
        .unwrap();
    let p = rate.period.as_ref().unwrap();
    assert!((p.period_s - 60.0).abs() < 1.0, "{}", p.period_s);
    assert!(p.beyond_simulated, "significance {}", p.significance);
    let f = r
        .findings
        .iter()
        .find(|f| f.series == SeriesKind::Rate)
        .unwrap();
    assert!(
        f.evidence
            .contains("false alarm under 0.0003, beyond the simulated range"),
        "{}",
        f.evidence
    );
}

/// The white null's p (#1199): with a positive skew, a χ² shifted and
/// scaled to all three moments (skew 2: ν 2, so p = e^(−(q − a)/2b)), 1 at
/// or under its shift; with none, two moments (Satterthwaite: mean 10 and
/// variance 20 are χ² on 10 degrees of freedom).
#[test]
fn the_white_nulls_p_matches_its_moments() {
    let null = |skew: f64| white::WhiteNull {
        bias: vec![0.0],
        q_mean: 10.0,
        q_var: 20.0,
        q_skew: skew,
    };
    let (b, a) = (5f64.sqrt(), 10.0 - 2.0 * 5f64.sqrt());
    let p = null(2.0).p(20.0);
    assert!((p - (-(20.0 - a) / (2.0 * b)).exp()).abs() < 1e-9, "{p}");
    assert!((p - chi2_tail(20.0, 10)).abs() > 5e-3, "{p}");
    assert_eq!(null(2.0).p(a - 0.1), 1.0);
    assert!((null(0.0).p(18.307) - 0.05).abs() < 1e-3);
}

/// Between two of the table's points a take's null is blended in log n:
/// halfway in log n, the mean of the two (#1199).
#[test]
fn the_white_table_is_read_between_its_points() {
    let t = white::table();
    let (lo, hi) = (&t.quadratic[10], &t.quadratic[11]);
    assert!(hi.n > lo.n + 1);
    let n = ((lo.n as f64).ln() * 0.5 + (hi.n as f64).ln() * 0.5)
        .exp()
        .round() as usize;
    let w = ((n as f64).ln() - (lo.n as f64).ln()) / ((hi.n as f64).ln() - (lo.n as f64).ln());
    let got = white::white_null(n, n, 1.0, t.lb_lags).unwrap();
    let want = lo.null.q_var + (hi.null.q_var - lo.null.q_var) * w;
    assert!(
        (got.q_var - want).abs() < 1e-9,
        "{} against {want}",
        got.q_var
    );
    assert!((lo.null.q_var - hi.null.q_var).abs() > 1e-3);
}

/// Candidates on the same peak are tested once, but only when they come
/// from the same source: the source sets how many turns a cycle needs,
/// so a period search's and an autocorrelation's at one period are both
/// kept (Norm's ruling). Of one source, those within the fold's
/// resolution p²/(20·span) of the best fold share's go with it, and one
/// further out is kept (#1398).
#[test]
fn only_one_sources_candidates_within_the_folds_resolution_merge() {
    use CycleSource::{Autocorrelation as Ac, PeriodSearch as Ps};
    // At 600 s in an hour the fold resolves 600²/(20·3600) = 5 s.
    let span = 3600.0;
    assert_eq!(
        same_peaks_once(&[(600.0, Ps, 0.3), (600.0, Ac, 0.2)], span),
        [(600.0, Ps), (600.0, Ac)]
    );
    // The better fold share is kept, in the order found.
    assert_eq!(
        same_peaks_once(&[(604.0, Ps, 0.2), (600.0, Ps, 0.3)], span),
        [(600.0, Ps)]
    );
    assert_eq!(
        same_peaks_once(&[(600.0, Ps, 0.3), (606.0, Ps, 0.2)], span),
        [(600.0, Ps), (606.0, Ps)]
    );
    // No chaining: 604 goes with 600, and 608, 8 s from it, stays.
    assert_eq!(
        same_peaks_once(
            &[(600.0, Ps, 0.3), (604.0, Ps, 0.2), (608.0, Ps, 0.1)],
            span
        ),
        [(600.0, Ps), (608.0, Ps)]
    );
}

/// An autocorrelation peak becomes a candidate only at three times its
/// 95% band: one at twice the band is noise to the cycle search (#1398).
#[test]
fn a_weak_autocorrelation_peak_is_no_candidate() {
    let cfg = Config {
        acf_alone: true,
        ..Config::default()
    };
    let g = grid(noise(31, 720, 0.0, 1.0), 10.0);
    let mut s = check_series(SeriesKind::Rate, g.clone(), None, None, None, &[], &cfg);
    s.two_state = None;
    let n = g.y.len() as f64;
    let acf_peak = |height: f64| {
        let lags = 60;
        let band_by_lag: Vec<f64> = (0..=lags).map(|k| 1.96 / (n - k as f64).sqrt()).collect();
        let mut r = vec![0.0; lags + 1];
        r[0] = 1.0;
        r[30] = height * 1.96 / n.sqrt();
        Autocorrelation {
            lag_s: (0..=lags).map(|k| k as f64 * 10.0).collect(),
            r,
            band: 1.96 / n.sqrt(),
            band_by_lag,
            ljung_box_q: 0.0,
            ljung_box_lags: 10,
            p_value: 1.0,
            max_short_lag_r: 0.0,
            repeat_lag_s: None,
            repeat_r: None,
            memory_s: None,
        }
    };
    let from_acf = |s: &SeriesCheck| -> Vec<f64> {
        refined_candidates(s, &g, None, &cfg)
            .1
            .into_iter()
            .filter(|c| c.1 == CycleSource::Autocorrelation)
            .map(|c| c.0)
            .collect()
    };
    s.autocorrelation = Some(acf_peak(2.0));
    assert!(from_acf(&s).is_empty(), "{:?}", from_acf(&s));
    s.autocorrelation = Some(acf_peak(4.0));
    let found = from_acf(&s);
    assert!(
        found.len() == 1 && (found[0] - 300.0).abs() <= 60.0,
        "{found:?}"
    );
}

/// A cycle's inner cycle fits three or more times and lasts 30 s or
/// more: a strong second harmonic is the cycle's own shape, not a cycle
/// inside it, and a line faster than 30 s is the timing search's
/// (#1398). The cycles here are found and kept, without an inner one.
#[test]
fn a_second_harmonic_or_a_fast_line_is_no_inner_cycle() {
    let tau = std::f64::consts::TAU;
    let cfg = Config::default();
    let strongest = |p: f64, inner: f64, sizes: (f64, f64), step: f64, n: usize| {
        let mut z = Noise(41);
        let y: Vec<f64> = (0..n)
            .map(|i| {
                let t = (i as f64 + 0.5) * step;
                sizes.0 * (tau * t / p).sin() + sizes.1 * (tau * t / inner).sin() + 0.3 * z.next()
            })
            .collect();
        let g = grid(y, step);
        let s = check_series(
            SeriesKind::Rate,
            g.clone(),
            None,
            Some(search_line(p, 2.0 * sizes.0, "s/d", None)),
            None,
            &[],
            &cfg,
        );
        let d = longterm::detrend(&g, longterm::DETREND_S);
        strongest_cycle(&s, &g, &d, &[(p, CycleSource::PeriodSearch)], &[], &cfg)
            .expect("the cycle is kept")
    };
    // 600 s with a second harmonic that explains more than half of it,
    // in 8 h of 10 s readings.
    let c = strongest(600.0, 300.0, (2.0, 3.0), 10.0, 2880);
    assert!((c.period_s - 600.0).abs() < 1.0, "{c:?}");
    assert_eq!((c.inner_period_s, c.turns), (None, None), "{c:?}");
    // 100 s with a 20 s line inside it, five turns, in 2 h of 2 s
    // windows.
    let c = strongest(100.0, 20.0, (2.0, 2.4), 2.0, 3600);
    assert!((c.period_s - 100.0).abs() < 0.5, "{c:?}");
    assert_eq!((c.inner_period_s, c.turns), (None, None), "{c:?}");
}

/// A unit after a number: degrees as "°" with no gap, every other unit
/// after a space, the one rule the report, the app and the MCP server
/// write units by (#1426).
#[test]
fn a_unit_follows_its_number_as_the_sentences_write_it() {
    assert_eq!(unit_after(SeriesKind::Amplitude.unit()), ("", "°"));
    assert_eq!(unit_after(SeriesKind::Rate.unit()), (" ", "s/d"));
    assert_eq!(unit_after(SeriesKind::BeatError.unit()), (" ", "ms"));
}

/// A level shift below zero, as on the YM42's last hours: the screen
/// copies set each figure with a true minus, the words keep their
/// hyphens, and the plain headline and summary keep the ASCII minus for
/// files, pipes and the model (#1437).
#[test]
fn the_screen_copies_carry_a_true_minus_on_figures_only() {
    let mut z = Noise(1437);
    let s = check_series(
        SeriesKind::Rate,
        grid(
            (0..3000)
                .map(|i| if i < 1500 { -5.0 } else { -12.0 } + 2.0 * z.next())
                .collect(),
            2.0,
        ),
        None,
        None,
        None,
        &[],
        &Config::default(),
    );
    assert_eq!(s.lead, Lead::ShiftingMean, "{}", s.headline);
    assert!(s.summary.contains(": -"), "{}", s.summary);
    assert!(!s.headline.contains('\u{2212}'), "{}", s.headline);
    assert_eq!(
        s.screen_headline,
        crate::figure_text::figure_minus(&s.headline)
    );
    assert_eq!(
        s.screen_summary,
        crate::figure_text::figure_minus(&s.summary)
    );
    assert!(
        s.screen_summary.contains(": \u{2212}"),
        "{}",
        s.screen_summary
    );
    assert!(!s.screen_summary.contains('-'), "{}", s.screen_summary);
    // Through `check` too, after its last words are added.
    let sc = crate::scenarios::find("rub").unwrap();
    let log = crate::scenarios::beat_log(&sc, 1);
    let lc = crate::longterm::LongConfig {
        wheels: crate::periodicity::standard_wheels(crate::scenarios::BPH, 20),
        ..Default::default()
    };
    let long = crate::longrun::analyse(&log, None, &lc);
    let r = check(&log, None, &long, &lc, &Config::default());
    for c in &r.series {
        assert_eq!(
            c.screen_headline,
            crate::figure_text::figure_minus(&c.headline)
        );
        assert_eq!(
            c.screen_summary,
            crate::figure_text::figure_minus(&c.summary)
        );
    }
}

/// A spread after a ± is at the series' place, rounded up; a within-take
/// difference (a peak to peak, a change per hour) one place finer (Norm's
/// ruling on #1470, math guide section 11).
#[test]
fn spreads_and_differences_take_their_places() {
    use SeriesKind::{Amplitude, BeatError, Rate};
    assert_eq!(Amplitude.fmt_pm(5.8), "6°");
    assert_eq!(Rate.fmt_pm(0.04), "0.1 s/d");
    assert_eq!(BeatError.fmt_pm(0.011), "0.02 ms");
    assert_eq!(Amplitude.fmt_pm(0.0), "1°");
    assert_eq!(Rate.fmt_pm(0.0), "0.1 s/d");
    assert_eq!(Amplitude.fmt_size(0.44), "0.4°");
    assert_eq!(Rate.fmt_size(5.836), "5.84 s/d");
    assert_eq!(BeatError.fmt_size(0.1234), "0.123 ms");
    assert_eq!(Amplitude.fmt_per_hour(0.49), "+0.5°");
    assert_eq!(Rate.fmt_per_hour(1.27), "+1.27 s/d");
    assert_eq!(BeatError.fmt_per_hour(-0.0004), "0.000 ms");
}

/// The stats line is core's one text for every screen: `screen_stats` is
/// `stats_line` with a true minus on each figure, and its figures sit at
/// the headline's places (#1470; Max, the app and the MCP view alike).
#[test]
fn the_stats_line_is_one_text_with_a_screen_copy() {
    let y: Vec<f64> = (0..60)
        .map(|i| -5.0 + 0.3 * (i as f64 * 1.7).sin())
        .collect();
    let s = check_series(
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
    let line = s.stats_line();
    assert!(
        line.starts_with("Tested on 60 10 s readings · median -5.0 s/d"),
        "{line}"
    );
    let pm = s.series.fmt_pm(s.short_term_sd.expect("a short-term SD"));
    assert!(
        line.contains(&format!("±{pm} reading to reading")),
        "{line}"
    );
    assert!(line.contains("lag-1 r "), "{line}");
    assert_eq!(s.screen_stats, crate::figure_text::figure_minus(&line));
    assert!(
        s.screen_stats.contains("median \u{2212}5.0 s/d"),
        "{}",
        s.screen_stats
    );
    assert!(s.screen_stats.contains("lag-1 r"), "{}", s.screen_stats);
}
